//! The sound: the Z80's sound driver ported ([`cube`]), run against the chips it
//! drives ([`ym2612`], [`psg`]). [`Audio`] takes a frame's sound requests and gives back
//! that frame's samples.
//!
//! Each frame, as on the console:
//! - at the vertical blank the 68000's `UpdateSound` posts one command: `sound_playing`
//!   if something set it (a fade, a pause), else the queue's next ID; one that finds the
//!   mailbox full waits for the next frame;
//! - the Z80 loops: when timer B has overflowed it ticks, then it takes a command from
//!   the mailbox, starts a sample when one is asked for, and writes the sample's next
//!   byte to the DAC, `$0031` turns of a delay loop apart.
//!
//! Timer B runs on the YM2612's sample clock ((256 - TB) * 16 samples a tick). The
//! driver's logic is exact (see `tests/original_sound.rs`); how long the Z80 spends on
//! a tick or a command (it writes nothing to the DAC meanwhile) is an estimate from its
//! number of chip writes, fitted to recordings, and the commands' moment in the frame is
//! a typical one, so writes can land a little off their real times within a frame.

pub mod cube;
pub mod psg;
pub mod ym2612;

use std::collections::VecDeque;

use crate::sound::Sound;
use cube::{Cube, Write};

/// NTSC master clock, a frame of it, a YM2612 sample (144 clocks of its /7 clock), a
/// PSG step (16 clocks of its /15 clock).
pub const MASTER_HZ: f64 = 53_693_175.0;
pub const FRAME_CLOCKS: u64 = 896_040;
const FM_SAMPLE: u64 = 1008;
const PSG_STEP: u64 = 240;
/// The YM2612's sample rate.
pub const FM_HZ: f64 = MASTER_HZ / FM_SAMPLE as f64;

/// When in the frame `UpdateSound` posts (after the vertical blank's DMA).
const VBLANK_AT: u64 = 100_000;
/// When the boot code loads and starts the driver (`InitSound`): power-on frame 8.
const BOOT_AT: u64 = 8 * FRAME_CLOCKS + 830_000;
/// The Z80's time for a tick or command: a base and a time per chip write (with the
/// YM2612 busy waits), fitted to recordings.
const EVENT_BASE: u64 = 30_000;
const PER_WRITE: u64 = 7_500;
/// A turn of the main loop: with a sample byte, 203 + 13n Z80 cycles; without, about
/// 121 + 13n (`n` the delay count).
const LOOP_DAC: u64 = 3045;
const LOOP_IDLE: u64 = 1815;
const LOOP_STEP: u64 = 195;

/// Output levels: the PSG's loudest channel against an FM channel's ±8192.
const PSG_GAIN: i32 = 2;
/// The output's coupling capacitor: a high-pass at a few Hz (a DAC byte left set after
/// a sample is cut short would otherwise sit there as DC).
const DC_POLE: f32 = 0.9997;

pub struct Audio {
    pub cube: Cube,
    ym: ym2612::Ym2612,
    psg: psg::Psg,
    /// The 68000's side: `sound_queue` and `sound_playing`.
    queue: VecDeque<u8>,
    playing: u8,
    /// Master clocks: this frame's start, where the Z80 is.
    now: u64,
    z80: u64,
    /// Timer B: its register, the next overflow, the flag.
    tb: u8,
    timer_next: u64,
    timer_flag: bool,
    /// Writes not yet heard (time order), the next FM sample's time and the PSG's.
    writes: VecDeque<(u64, Write)>,
    fm_at: u64,
    psg_at: u64,
    /// Resampling to `rate`: the last FM sample and the position between it and the next.
    pub rate: u32,
    last: (f32, f32),
    pos: f64,
    out: Vec<f32>,
    /// The output's AC coupling (a DC blocker): last input and output.
    dc: [(f32, f32); 2],
    /// Whether the boot code has started the driver yet.
    started: bool,
}

impl Audio {
    /// The sound from power-on (the driver starts at `InitSound`), with output at `rate`
    /// Hz.
    pub fn new(rate: u32) -> Audio {
        let mut a = Audio {
            cube: Cube::new(),
            ym: ym2612::Ym2612::new(),
            psg: psg::Psg::new(),
            queue: VecDeque::new(),
            playing: 0,
            now: 0,
            z80: 0,
            tb: 0,
            timer_next: u64::MAX,
            timer_flag: false,
            writes: VecDeque::new(),
            fm_at: 0,
            psg_at: 0,
            rate,
            last: (0.0, 0.0),
            pos: 0.0,
            out: vec![],
            dc: [(0.0, 0.0); 2],
            started: false,
        };
        a.z80 = BOOT_AT;
        a
    }

    /// A frame: what the game asked for in it (heard from the next vertical blank on),
    /// and its samples (stereo, interleaved, at `rate`).
    pub fn frame(&mut self, sounds: &[Sound]) -> &[f32] {
        let end = self.now + FRAME_CLOCKS;
        let vblank = self.now + VBLANK_AT;
        let mut posted = false;
        while self.z80 < end {
            if !posted && self.z80 >= vblank {
                self.update_sound();
                posted = true;
            }
            let until = if posted { end } else { vblank };
            self.z80_turn(until);
        }
        if !posted {
            self.update_sound();
        }
        for &s in sounds {
            match s {
                Sound::Play(id) => self.queue.push_back(id),
                Sound::Command(c) => self.playing = c,
            }
        }
        self.render(end);
        self.now = end;
        &self.out
    }

    /// `UpdateSound` (and `ProcessSoundQueue`).
    fn update_sound(&mut self) {
        if self.playing == 0 {
            self.playing = self.queue.pop_front().unwrap_or(0);
        }
        if self.playing != 0 && self.cube.post(self.playing) {
            self.playing = 0;
        }
    }

    /// Timer B's overflows up to `t`.
    fn run_timer(&mut self, t: u64) {
        while self.timer_next <= t {
            self.timer_flag = true;
            self.timer_next += (256 - u64::from(self.tb)) * 16 * FM_SAMPLE;
        }
    }

    /// The driver's writes since the last call, timed from `t` on (one per `PER_WRITE`);
    /// the time they took.
    fn take_writes(&mut self, t: u64) -> u64 {
        let w = std::mem::take(&mut self.cube.writes);
        for (i, &w) in w.iter().enumerate() {
            let at = t + i as u64 * PER_WRITE;
            if let Write::Fm { part: 0, reg, data } = w {
                if reg == 0x26 {
                    self.tb = data;
                }
                if reg == 0x27 && self.timer_next == u64::MAX {
                    // (the timer starts with the first load)
                    self.timer_next = at + (256 - u64::from(self.tb)) * 16 * FM_SAMPLE;
                }
            }
            self.writes.push_back((at, w));
        }
        if w.is_empty() { 0 } else { EVENT_BASE + PER_WRITE * w.len() as u64 }
    }

    /// One turn of the Z80's main loop (`$0023`), or its wait in a pause (until `until`).
    fn z80_turn(&mut self, until: u64) {
        if !self.started {
            // ($0000: the variables, song $20)
            self.started = true;
            self.cube.reset();
            let t = self.take_writes(self.z80);
            self.z80 += t;
            return;
        }
        if self.cube.paused {
            if self.cube.ram[usize::from(cube::MAILBOX)] != 0 {
                self.cube.command();
                let t = self.take_writes(self.z80);
                self.z80 += t.max(LOOP_IDLE);
            } else {
                self.z80 = until;
            }
            return;
        }
        self.run_timer(self.z80);
        let n = u64::from(self.cube.sample_delay());
        if self.timer_flag {
            self.timer_flag = false;
            self.cube.tick();
            let t = self.take_writes(self.z80);
            self.z80 += t;
        } else {
            // (the delay loop)
            self.z80 += LOOP_STEP * n;
        }
        // $0034: the mailbox, the sample request, a byte to the DAC.
        if self.cube.ram[usize::from(cube::MAILBOX)] != 0 {
            self.cube.command();
            let t = self.take_writes(self.z80);
            self.z80 += t;
            if self.cube.paused {
                return;
            }
        }
        self.cube.sample_request();
        self.take_writes(self.z80);
        match self.cube.dac_byte() {
            Some(b) => {
                self.writes.push_back((self.z80, Write::Fm { part: 0, reg: 0x2A, data: b }));
                self.z80 += LOOP_DAC;
            }
            None => self.z80 += LOOP_IDLE,
        }
    }

    /// The chips up to `end`, resampled into `out`.
    fn render(&mut self, end: u64) {
        self.out.clear();
        let step = FM_HZ / f64::from(self.rate);
        while self.fm_at < end {
            while self.writes.front().is_some_and(|w| w.0 <= self.fm_at) {
                let (_, w) = self.writes.pop_front().unwrap();
                match w {
                    Write::Fm { part, reg, data } => self.ym.write(part, reg, data),
                    Write::Psg(d) => self.psg.write(d),
                }
            }
            let (l, r) = self.ym.sample();
            // The PSG's steps in this FM sample, averaged.
            let mut steps = 0;
            while self.psg_at < self.fm_at + FM_SAMPLE {
                self.psg_at += PSG_STEP;
                steps += 1;
            }
            let p = if steps > 0 { self.psg.run(steps) * PSG_GAIN / steps as i32 } else { 0 };
            let mut block = |i: usize, x: f32| {
                let (px, py) = self.dc[i];
                let y = x - px + DC_POLE * py;
                self.dc[i] = (x, y);
                y
            };
            let cur = (block(0, (l + p) as f32 / 32768.0), block(1, (r + p) as f32 / 32768.0));
            // Linear resampling between the last FM sample and this one.
            while self.pos < 1.0 {
                let t = self.pos as f32;
                self.out.push(self.last.0 + (cur.0 - self.last.0) * t);
                self.out.push(self.last.1 + (cur.1 - self.last.1) * t);
                self.pos += step;
            }
            self.pos -= 1.0;
            self.last = cur;
            self.fm_at += FM_SAMPLE;
        }
    }
}
