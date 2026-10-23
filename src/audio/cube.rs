//! The Cube sound driver (`sound/driver/cube.bin`, $F6000 in the ROM), the Z80 program
//! that plays the music, sound effects and voice samples. Ported routine by routine
//! (the Z80 addresses are in the comments); its state lives where the Z80 keeps it, in
//! an 8 KB copy of Z80 RAM that starts as the driver's own image (code, tables and the
//! sound effects' data), so the tests compare it with the Z80 RAM of recordings.
//!
//! Z80 RAM:
//! - $1330-$146F: the music's 10 channels, $20 bytes each: FM1-FM3 (part I), FM4, FM5
//!   (part II), the DAC (FM6), PSG1-3, noise. $1470, $1490, $14B0: the sound effects'
//!   FM4, FM5 and DAC, which mute the music's (`IX+$E0`). A channel: +0 its data
//!   pointer, +2 frames left, +3 stopped, +4 volume, +5 algorithm (PSG: the volume
//!   written), +6 release time, +7 the default length, +8 sustain bit 7 (PSG: rest bit
//!   0, released bit 1), +9 vibrato delay, +$0A its countdown, +$0B the vibrato table,
//!   +$0D its step, +$0E the frequency, +$10 the patch (PSG: the envelope), +$11 the
//!   slide target (PSG: +$12 the envelope step), +$13 the loop start, +$15 the volta
//!   start, +$17 the counted loop, +$19 its count, +$1A/+$1B the volta flags, +$1C the
//!   transposition, +$1D the detune, +$1E the pan (PSG: released), +$1F the slide speed.
//! - $14D0-$14E3: the driver's variables (see the constants below).
//! - $1FF8: a sample to start (from the channels), $1FFC/$1FFD the master volume (and
//!   its fade-in), $1FFE a sound effect's priority and $1FFF the next command (both from
//!   the 68000, `UpdateSound`).
//!
//! The Z80's main loop (`$0023`) is the caller's (see [`super::Audio`]): it polls the
//! YM2612's timer B, whose overflow runs [`Cube::tick`], then the command mailbox
//! ([`Cube::command`]), the sample request ([`Cube::sample_request`]) and writes the next
//! sample byte to the DAC ([`Cube::dac_byte`]), a delay loop of `$0031` turns apart.

use crate::rom;

/// `unk_8660` (in the 68000's code): the priority sent with each ID.
const PRIORITIES: u32 = 0x8660;

/// The driver's image in the ROM and its size (Z80 $0000-$1FF0).
pub const IMAGE: u32 = 0xF6000;
pub const IMAGE_LEN: usize = 0x1FF1;

/// The variables.
const PSG_BITS: u16 = 0x14D1; // the PSG channel's bits for its commands
const CHANNEL: u16 = 0x14D2; // (IY+0): the channel being played
const FADING: u16 = 0x14D3;
const PART: u16 = 0x14D5; // YM2612 part of the FM writes (`$051C`)
const SFX_PASS: u16 = 0x14D6; // the sound effects' channels are being played
const FADE_SPEED: u16 = 0x14D7;
const FADE_WAIT: u16 = 0x14D8;
const FADE_LEVEL: u16 = 0x14D9;
const FADE_TIMER: u16 = 0x14DA; // (word) ticks until the song fades out by itself
const SAMPLE_BANK: u16 = 0x14DC;
const SONG_BANK: u16 = 0x14DD;
const PRIORITY: u16 = 0x14DE;
const TEMP: u16 = 0x14E0; // (word)
const PATCH_REG: u16 = 0x14E2;
const FADE_IN_TICKS: u16 = 0x14E3;
const PAN_COUNT: u16 = 0x14D0;
const SAMPLE: u16 = 0x1FF8;
const FADE_IN_RATE: u16 = 0x1FFC;
const MASTER: u16 = 0x1FFD;
const SFX_PRIORITY: u16 = 0x1FFE;
pub const MAILBOX: u16 = 0x1FFF;

/// The tables.
const FM_FREQS: u16 = 0x10F0;
const PSG_FREQS: u16 = 0x11B0;
const VOLUMES: u16 = 0x1230;
const CARRIERS: u16 = 0x1240;
const VIBRATOS: u16 = 0x1248;
const ENVELOPES: u16 = 0x1294;
const SAMPLES: u16 = 0x14E4;
const SFX_TABLE: u16 = 0x160C;

/// The channels.
const FM1: u16 = 0x1330;
const FM4: u16 = 0x1390;
const DAC: u16 = 0x13D0;
const PSG1: u16 = 0x13F0;
const NOISE: u16 = 0x1450;
const SFX_FM4: u16 = 0x1470;
const CHANNEL_SIZE: u16 = 0x20;
/// A music channel's sound effect channel (part II).
const SFX_OFFSET: u16 = 0xE0;

/// Self-modified operands: the delay loop's count (the sample rate) and the frequency
/// registers of the FM channel being played (`LD B,n`: part I's, part II's).
const SAMPLE_DELAY: u16 = 0x0031;
const FREQ_HI_1: u16 = 0x0793;
const FREQ_LO_1: u16 = 0x079C;
const FREQ_HI_2: u16 = 0x09ED;
const FREQ_LO_2: u16 = 0x09F6;

/// A write to the sound chips.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Write {
    /// YM2612: part 0 or 1, register, value.
    Fm { part: u8, reg: u8, data: u8 },
    /// SN76489 ($7F11).
    Psg(u8),
}

#[derive(Clone)]
pub struct Cube {
    pub ram: Box<[u8; 0x2000]>,
    /// The 68000 address the Z80's $8000-$FFFF window shows (its bank register).
    pub bank: u32,
    /// The sample playing: HL its next byte (in the window), DE the bytes left.
    pub sample_at: u16,
    pub sample_left: u16,
    /// Waiting for a command after `$FF` (`$005D`): no ticks, no samples.
    pub paused: bool,
    /// What it wrote, in order (the caller takes them).
    pub writes: Vec<Write>,
}

impl Default for Cube {
    fn default() -> Self {
        Self::new()
    }
}

impl Cube {
    /// The driver as `LoadCubeDriver` leaves it, before it runs.
    pub fn new() -> Self {
        let mut ram = Box::new([0u8; 0x2000]);
        for (i, b) in ram.iter_mut().take(IMAGE_LEN).enumerate() {
            *b = rom::byte(IMAGE + i as u32);
        }
        Cube { ram, bank: 0, sample_at: 0, sample_left: 0, paused: false, writes: vec![] }
    }

    /// From a Z80 RAM snapshot (taken between ticks, with the sample bank in place).
    pub fn from_ram(ram: &[u8]) -> Self {
        let mut c = Self::new();
        c.ram.copy_from_slice(&ram[..0x2000]);
        c.set_sample_bank();
        c
    }

    /// `$0000`: the variables, then song $20 (silence; it also starts timer B).
    pub fn reset(&mut self) {
        self.wr(SFX_PRIORITY, 0);
        self.wr(PRIORITY, 0xFF);
        self.wr(MASTER, 0x0F);
        self.wr(0x1FF9, 0xFF);
        self.wr(FADE_IN_RATE, 0x0F);
        self.dispatch(0x20);
        self.set_sample_bank();
    }

    // ---- memory -------------------------------------------------------------------

    fn rd(&self, at: u16) -> u8 {
        match at {
            0..=0x1FFF => self.ram[usize::from(at)],
            0x8000..=0xFFFF => rom::byte(self.bank | u32::from(at & 0x7FFF)),
            _ => 0xFF,
        }
    }

    fn wr(&mut self, at: u16, v: u8) {
        if at < 0x2000 {
            self.ram[usize::from(at)] = v;
        }
    }

    fn word(&self, at: u16) -> u16 {
        u16::from_le_bytes([self.rd(at), self.rd(at.wrapping_add(1))])
    }

    fn set_word(&mut self, at: u16, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        self.wr(at, lo);
        self.wr(at.wrapping_add(1), hi);
    }

    /// A channel's byte.
    fn c(&self, ix: u16, off: u16) -> u8 {
        self.ram[usize::from(ix + off)]
    }

    fn set(&mut self, ix: u16, off: u16, v: u8) {
        self.ram[usize::from(ix + off)] = v;
    }

    fn channel(&self) -> u8 {
        self.ram[usize::from(CHANNEL)]
    }

    // ---- banks (`$6000`: nine writes shift in address bits 15-23) ------------------

    /// `$0143`: the sample's bank.
    fn set_sample_bank(&mut self) {
        self.bank = 0x8_0000 | u32::from(self.rd(SAMPLE_BANK) & 0x0F) << 15;
    }

    /// `$016B`: the song's ($F0000 or $F8000).
    fn set_song_bank(&mut self) {
        self.bank = 0xF_0000 | u32::from(self.rd(SONG_BANK) & 1) << 15;
    }

    /// `$0182`: the patches ($F8000).
    fn set_patch_bank(&mut self) {
        self.bank = 0xF_8000;
    }

    /// `$0196`: the voices ($F0000).
    fn set_voice_bank(&mut self) {
        self.bank = 0xF_0000;
    }

    // ---- chip writes ---------------------------------------------------------------

    /// `$0522`: part I.
    fn ym1(&mut self, reg: u8, data: u8) {
        self.writes.push(Write::Fm { part: 0, reg, data });
    }

    /// `$0544`: part II.
    fn ym2(&mut self, reg: u8, data: u8) {
        self.writes.push(Write::Fm { part: 1, reg, data });
    }

    /// `$051C`: the part in `$14D5` (part II through `$0532`, with whatever channel IX
    /// last pointed at).
    fn ym_part(&mut self, ix: u16, reg: u8, data: u8) {
        if self.rd(PART) != 0 {
            self.ym2_unless_sfx(ix, reg, data);
        } else {
            self.ym1(reg, data);
        }
    }

    /// Whether the music channel `ix` is muted: its sound effect channel plays (and this
    /// isn't the sound effects' pass).
    fn muted(&self, ix: u16) -> bool {
        self.rd(SFX_PASS) == 0 && self.c(ix, SFX_OFFSET + 3) == 0
    }

    /// `$0532`: part II, unless the channel's sound effect plays.
    fn ym2_unless_sfx(&mut self, ix: u16, reg: u8, data: u8) {
        if !self.muted(ix) {
            self.ym2(reg, data);
        }
    }

    /// `$0508`: part I (the key register for part II's channels), the same way.
    fn ym1_unless_sfx(&mut self, ix: u16, reg: u8, data: u8) {
        if !self.muted(ix) {
            self.ym1(reg, data);
        }
    }

    fn psg(&mut self, data: u8) {
        self.writes.push(Write::Psg(data));
    }

    /// `$0503`: timer B's flag reset (and the timer kept running).
    fn timer_reset(&mut self) {
        self.ym1(0x27, 0x3A);
    }

    /// `$0554`: a PSG channel's volume.
    fn psg_volume(&mut self, a: u8) {
        let v = (a & 0x0F) | self.rd(PSG_BITS) | 0x90;
        self.psg(v);
    }

    /// `$0561`: a sample for the DAC (from the music only while its effects' channel
    /// is silent and it isn't fading).
    fn request_sample(&mut self, ix: u16, sample: u8) {
        if self.rd(SFX_PASS) == 0 {
            if self.c(ix, SFX_OFFSET + 3) == 0 || self.rd(FADE_LEVEL) >= 3 || self.rd(MASTER) != 0x0F {
                return;
            }
        }
        self.wr(SAMPLE, sample);
    }

    // ---- the main loop's calls -------------------------------------------------------

    /// `UpdateSound`'s side (the 68000, at the vertical blank): a command into the empty
    /// mailbox, with its priority from `unk_8660` (17 zeros; the IDs past them read on
    /// into the code after it). False when the mailbox is full.
    pub fn post(&mut self, id: u8) -> bool {
        if self.rd(MAILBOX) != 0 {
            return false;
        }
        self.wr(MAILBOX, id);
        let p = rom::byte(PRIORITIES + u32::from(id));
        if p != 0 {
            self.wr(SFX_PRIORITY, p);
        }
        true
    }

    /// `$01DC` with the mailbox's command (`$0034`: when it isn't 0).
    pub fn command(&mut self) {
        let a = self.rd(MAILBOX);
        if self.paused {
            // $00A4: only a command ends the pause ($FF taken, any other left for the
            // main loop), then the volumes come back ($00B2).
            if a == 0 {
                return;
            }
            if a == 0xFF {
                self.wr(MAILBOX, 0);
            }
            self.paused = false;
            self.restore_volumes();
            return;
        }
        self.dispatch(a);
    }

    fn dispatch(&mut self, a: u8) {
        self.wr(MAILBOX, 0);
        match a {
            0xFF => self.pause(),
            0xFE => {
                self.stop_all();
                self.sample_left = 0;
            }
            0xFD => self.start_fade(),
            0xF0 => {
                // $0137
                let m = self.rd(MASTER) & 0x0F;
                self.wr(MASTER, m);
                self.restore_volumes();
            }
            0x41.. => self.start_effect(a),
            _ => self.start_song(a),
        }
    }

    /// `$01AA`: `$1FF8`, the sample to start ($FE stops it).
    pub fn sample_request(&mut self) {
        let a = self.rd(SAMPLE);
        if a == 0 {
            return;
        }
        if a == 0xFE {
            self.sample_at = 0xC000;
            self.sample_left = 0;
            return;
        }
        // $01B5: rate, bank, length and start.
        let at = SAMPLES + u16::from(a.wrapping_sub(1)) * 8;
        let rate = self.rd(at);
        self.wr(SAMPLE_DELAY, rate);
        let bank = self.rd(at + 2);
        self.wr(SAMPLE_BANK, bank);
        self.sample_left = self.word(at + 4);
        self.sample_at = self.word(at + 6);
        self.wr(SAMPLE, 0);
        self.set_sample_bank();
    }

    /// `$0046`: the sample's next byte for the DAC (register $2A), if one is playing.
    pub fn dac_byte(&mut self) -> Option<u8> {
        if self.sample_left == 0 {
            return None;
        }
        let b = self.rd(self.sample_at);
        self.sample_at = self.sample_at.wrapping_add(1);
        self.sample_left -= 1;
        Some(b)
    }

    /// The delay loop's count after each sample byte (`$0031`).
    pub fn sample_delay(&self) -> u8 {
        self.ram[usize::from(SAMPLE_DELAY)]
    }

    // ---- commands ------------------------------------------------------------------

    /// `$005D`: everything silent until the next command.
    fn pause(&mut self) {
        self.wr(SFX_PASS, 0);
        self.wr(PART, 0);
        for part in 0..2 {
            self.wr(PART, part);
            let mut reg = 0x40u8;
            for _ in 0..4 {
                for _ in 0..3 {
                    if part == 0 {
                        self.ym1(reg, 0x7F);
                    } else {
                        self.ym2(reg, 0x7F);
                    }
                    reg += 1;
                }
                reg += 1;
            }
        }
        for v in [0x9F, 0xBF, 0xDF, 0xFF] {
            self.psg(v);
        }
        self.paused = true;
    }

    /// `$00BB`: every FM channel's patch and volume again (the fade-in, a pause's end).
    fn restore_volumes(&mut self) {
        self.wr(PART, 0);
        for ch in 0..3u8 {
            self.wr(CHANNEL, ch);
            let ix = FM1 + u16::from(ch) * CHANNEL_SIZE;
            let v = self.c(ix, 4);
            self.fm1_patch_volume(ix, v);
        }
        self.wr(PART, 1);
        for ch in 0..2u8 {
            self.wr(CHANNEL, ch);
            let ix = FM4 + u16::from(ch) * CHANNEL_SIZE;
            let v = self.c(ix, 4);
            self.fm2_patch_volume(ix, v);
        }
        self.wr(CHANNEL, 0);
        self.wr(SFX_PASS, 1);
        for ch in 0..2u8 {
            self.wr(CHANNEL, ch);
            let ix = SFX_FM4 + u16::from(ch) * CHANNEL_SIZE;
            if self.c(ix, 3) == 0 {
                let v = self.c(ix, 4);
                self.fm2_patch_volume(ix, v);
            }
        }
    }

    /// `$0391`: every channel silent and stopped.
    fn stop_all(&mut self) {
        self.wr(PART, 0);
        self.wr(SFX_PASS, 0);
        for ch in 0..3u8 {
            self.wr(CHANNEL, ch);
            self.fm1_patch_volume(FM1 + u16::from(ch) * CHANNEL_SIZE, 0);
            self.ym1(0x28, ch);
        }
        self.wr(PART, 1);
        for ch in 0..2u8 {
            self.wr(CHANNEL, ch);
            // (only where the effects' channel is stopped)
            if self.c(SFX_FM4 + u16::from(ch) * CHANNEL_SIZE, 3) != 0 {
                self.fm2_patch_volume(FM4 + u16::from(ch) * CHANNEL_SIZE, 0);
                self.ym1(0x28, 4 + ch);
            }
        }
        for v in [0x9F, 0xBF, 0xDF, 0xFF] {
            self.psg(v);
        }
        for i in 0..10 {
            self.set(FM1 + i * CHANNEL_SIZE, 3, 1);
        }
        self.wr(FADING, 0);
        self.wr(FADE_LEVEL, 0);
        self.wr(FADE_WAIT, 0x63);
        self.wr(PRIORITY, 0xFF);
    }

    /// `$10DF` ($FD, or the song's own timer): fade out.
    fn start_fade(&mut self) {
        self.wr(FADE_SPEED, 0x12);
        self.wr(FADING, 1);
        self.wr(FADE_WAIT, 0x12);
    }

    /// `$01FB`: songs $01-$20 from the table at $F937C, $21-$40 from $F2280.
    fn start_song(&mut self, a: u8) {
        let (table, n) = if a < 0x21 {
            self.wr(SONG_BANK, 1);
            (0x937Cu16, a)
        } else {
            self.wr(SONG_BANK, 0);
            (0xA280, a - 0x20)
        };
        self.set_song_bank();
        let mut hl = self.word(table + u16::from(n.wrapping_sub(1).wrapping_mul(2)));
        if self.rd(hl) != 0 {
            // (`JP $02DC` with the type as an effect's id, stack unbalanced: no song in
            // this ROM does it)
            return;
        }
        let m = self.rd(FADE_IN_RATE) & 0x0F;
        self.wr(MASTER, m);
        self.wr(FADE_IN_TICKS, 0);
        self.wr(PAN_COUNT, 0);
        self.stop_all();
        hl = hl.wrapping_add(1);
        let timer = self.word(hl);
        hl = hl.wrapping_add(2);
        self.set_word(FADE_TIMER, timer);
        self.ym1(0x2B, 0x80);
        let tempo = self.rd(hl).wrapping_add(3);
        self.ym1(0x26, tempo);
        self.wr(FADING, 0);
        self.wr(FADE_LEVEL, 0);
        self.wr(FADE_WAIT, 0x63);
        hl = hl.wrapping_add(1);
        for i in 0..10 {
            let ix = FM1 + i * CHANNEL_SIZE;
            let de = self.word(hl);
            hl = hl.wrapping_add(2);
            self.init_channel(ix, de);
            if self.rd(de) == 0xFF {
                self.set(ix, 3, 1);
            }
        }
        // (IX is past the last channel, at $1470)
        for part in [1u8, 0] {
            self.wr(PART, part);
            for reg in 0xB4..=0xB6 {
                self.ym_part(FM1 + 10 * CHANNEL_SIZE, reg, 0xC0);
            }
        }
        self.set(FM4, 0x1E, 0xC0);
        self.set(FM4 + CHANNEL_SIZE, 0x1E, 0xC0);
        self.wr(SAMPLE, 0xFE);
        self.timer_reset();
        self.set_sample_bank();
    }

    /// `$0351`: a channel starts at `de`.
    fn init_channel(&mut self, ix: u16, de: u16) {
        self.set_word(ix, de);
        self.set(ix, 0x1E, 0xC0);
        for off in [2, 3, 8, 0x13, 0x14, 0x1C, 0x1D, 0x1F] {
            self.set(ix, off, 0);
        }
        self.set(ix, 0x1E, 1);
    }

    /// `$02E3` (effects $41-$80, data in the driver) / `$037B` (voices $81 up, from the
    /// table at $F0000).
    fn start_effect(&mut self, a: u8) {
        let mut hl = if a >= 0x81 {
            self.set_voice_bank();
            self.word(0x8000 + u16::from(a.wrapping_sub(0x81)) * 2)
        } else {
            self.word(SFX_TABLE + u16::from(a.wrapping_sub(0x41)) * 2)
        };
        // $02F3
        let kind = self.rd(hl);
        hl = hl.wrapping_add(1);
        if kind == 1 {
            // Over the music's channels.
            for i in 0..10 {
                let de = self.word(hl);
                hl = hl.wrapping_add(2);
                if self.rd(de) != 0xFF {
                    self.init_channel(FM1 + i * CHANNEL_SIZE, de);
                }
            }
        } else {
            // $0314: on the effects' channels, unless one of higher priority plays.
            let d = self.rd(SFX_PRIORITY);
            if self.rd(PRIORITY) >= d {
                self.wr(PRIORITY, d);
                for i in 0..3u8 {
                    let ix = SFX_FM4 + u16::from(i) * CHANNEL_SIZE;
                    let de = self.word(hl);
                    hl = hl.wrapping_add(2);
                    if self.rd(de) != 0xFF {
                        self.init_channel(ix, de);
                        self.ym1(0x28, 4 + i);
                        self.ym2(0xB4 + i, 0xC0);
                    }
                }
            }
        }
        self.set_sample_bank();
    }

    // ---- the tick --------------------------------------------------------------------

    /// `$0436`: timer B overflowed.
    pub fn tick(&mut self) {
        let n = self.rd(FADE_IN_TICKS).wrapping_add(1);
        self.wr(FADE_IN_TICKS, n);
        let rate = (self.rd(FADE_IN_RATE) >> 4) << 2;
        if rate != 0 && rate == n {
            self.wr(FADE_IN_TICKS, 0);
            let m = self.rd(MASTER);
            if m != 0x0F {
                self.wr(MASTER, m + 1);
                self.restore_volumes();
            }
        }
        self.set_song_bank();
        let t = self.word(FADE_TIMER);
        if t != 0 {
            self.set_word(FADE_TIMER, t - 1);
            if t == 1 {
                self.start_fade();
            }
        }
        if self.rd(FADING) != 0 {
            let w = self.rd(FADE_WAIT);
            if w != 0 {
                self.wr(FADE_WAIT, w - 1);
            } else {
                let s = self.rd(FADE_SPEED);
                self.wr(FADE_WAIT, s);
                let l = self.rd(FADE_LEVEL).wrapping_add(1);
                self.wr(FADE_LEVEL, l);
                if l == 0x0C {
                    // (the sample plays on: the tick keeps DE)
                    self.stop_all();
                    self.set_sample_bank();
                    return;
                }
            }
        }
        self.timer_reset();
        self.wr(SFX_PASS, 0);
        self.wr(PART, 0);
        for ch in 0..3 {
            self.wr(CHANNEL, ch);
            self.fm1_channel();
        }
        self.wr(PART, 1);
        for ch in 0..2 {
            self.wr(CHANNEL, ch);
            self.fm2_channel();
        }
        self.wr(CHANNEL, 2);
        self.dac_channel();
        for ch in 0..3 {
            self.wr(CHANNEL, ch);
            self.psg_channel();
        }
        self.wr(CHANNEL, 3);
        self.noise_channel();
        self.set_voice_bank();
        self.wr(SFX_PASS, 1);
        for ch in 0..2 {
            self.wr(CHANNEL, ch);
            self.fm2_channel();
        }
        self.wr(CHANNEL, 2);
        self.dac_channel();
        self.set_sample_bank();
    }

    // ---- FM channels -------------------------------------------------------------------

    /// `$0585`: FM1-FM3.
    fn fm1_channel(&mut self) {
        let ch = self.channel();
        let ix = FM1 + u16::from(ch) * CHANNEL_SIZE;
        self.wr(FREQ_LO_1, ch.wrapping_add(0xA0));
        self.wr(FREQ_HI_1, ch.wrapping_add(0xA4));
        self.fm_events(ix, false);
    }

    /// `$07AF`: FM4 and FM5 (the effects' with `$14D6` set).
    fn fm2_channel(&mut self) {
        let ch = self.channel();
        let mut ix = FM4 + u16::from(ch) * CHANNEL_SIZE;
        if self.rd(SFX_PASS) != 0 {
            ix += SFX_OFFSET;
        }
        self.wr(FREQ_LO_2, ch.wrapping_add(0xA0));
        self.wr(FREQ_HI_2, ch.wrapping_add(0xA4));
        self.fm_events(ix, true);
    }

    /// The key register's channel code.
    fn key_channel(&self, part2: bool) -> u8 {
        self.channel() + if part2 { 4 } else { 0 }
    }

    fn key(&mut self, ix: u16, part2: bool, data: u8) {
        if part2 {
            self.ym1_unless_sfx(ix, 0x28, data);
        } else {
            self.ym1(0x28, data);
        }
    }

    /// A channel's own FM write: part I directly, part II unless muted.
    fn fm_write(&mut self, ix: u16, part2: bool, reg: u8, data: u8) {
        if part2 {
            self.ym2_unless_sfx(ix, reg, data);
        } else {
            self.ym1(reg, data);
        }
    }

    /// `$05A4` / `$07D9`: the channel's events until one takes time, then `$06C8` /
    /// `$0922` (the note going on).
    fn fm_events(&mut self, ix: u16, part2: bool) {
        loop {
            if self.c(ix, 3) != 0 {
                return;
            }
            if self.c(ix, 6) == self.c(ix, 2) && self.c(ix, 8) == 0 {
                let k = self.key_channel(part2);
                self.key(ix, part2, k);
            }
            if self.c(ix, 2) != 0 {
                self.fm_sustain(ix, part2);
                return;
            }
            let mut de = self.word(ix);
            loop {
                // $05CD
                self.set(ix, 0x0D, 0);
                let d = self.c(ix, 9);
                self.set(ix, 0x0A, d);
                let a = self.rd(de);
                if a & 0xF8 != 0xF8 {
                    break;
                }
                match a {
                    0xFF => {
                        let l = self.rd(de.wrapping_add(1));
                        let h = self.rd(de.wrapping_add(2));
                        let to = u16::from_le_bytes([l, h]);
                        let jump = if part2 { to != 0 } else { h != 0 };
                        if jump {
                            de = to;
                            continue;
                        }
                        self.fm_end(ix, part2, l);
                        return;
                    }
                    0xFE => {
                        de = de.wrapping_add(1);
                        let p = self.rd(de);
                        self.set(ix, 0x10, p);
                        let v = self.c(ix, 4);
                        self.fm_patch_volume(ix, part2, v);
                        de = de.wrapping_add(1);
                    }
                    0xFD => {
                        de = de.wrapping_add(1);
                        let v = self.rd(de) & 0x0F;
                        self.fm_patch_volume(ix, part2, v);
                        de = de.wrapping_add(1);
                    }
                    0xFC => self.slide_command(ix, &mut de),
                    0xFB => self.vibrato_command(ix, &mut de),
                    0xFA => {
                        if part2 {
                            self.pan2_command(ix, &mut de);
                        } else {
                            self.pan1_command(&mut de);
                        }
                    }
                    0xF9 => self.shift_command(ix, &mut de),
                    _ => self.loop_command(ix, &mut de),
                }
            }
            // $0654 / $08AC: a note (or a rest).
            let note = self.rd(de) & 0x7F;
            if note != 0x70 {
                let n = note.wrapping_add(self.c(ix, 0x1C));
                let at = FM_FREQS.wrapping_add(u16::from(n) * 2);
                let f = self.word(at).wrapping_add(u16::from(self.c(ix, 0x1D)));
                let [lo, hi] = f.to_le_bytes();
                // (part II's notes go to the registers part I's last channel left: FM6's)
                let (hi_reg, lo_reg) = (self.rd(FREQ_HI_1), self.rd(FREQ_LO_1));
                let sliding = self.c(ix, 0x1F) != 0;
                self.set(ix, 0x12, hi);
                if !sliding {
                    self.set(ix, 0x0F, hi);
                    self.set(ix, 0x12, 0);
                }
                self.fm_write(ix, part2, hi_reg, hi);
                self.set(ix, 0x11, lo);
                if !sliding {
                    self.set(ix, 0x0E, lo);
                    self.set(ix, 0x11, 0);
                }
                self.fm_write(ix, part2, lo_reg, lo);
                let k = self.key_channel(part2) | 0xF0;
                self.key(ix, part2, k);
            }
            // $06AC: the length.
            let len = if self.rd(de) & 0x80 != 0 {
                de = de.wrapping_add(1);
                let l = self.rd(de);
                self.set(ix, 7, l);
                l
            } else {
                self.c(ix, 7)
            };
            self.set(ix, 2, len);
            de = de.wrapping_add(1);
            self.set_word(ix, de);
        }
    }

    /// The channel's data ended (`$05F5` / `$0825`).
    fn fm_end(&mut self, ix: u16, part2: bool, next: u8) {
        if !part2 {
            // (a song can start another sound as it ends)
            if next != 0 {
                self.wr(MAILBOX, next);
            }
            self.set(ix, 3, 1);
            self.fm1_patch_volume(ix, 0);
            return;
        }
        self.set(ix, 3, 1);
        self.wr(PRIORITY, 0xFF);
        if self.rd(SFX_PASS) != 0 {
            // An effect's end: the music channel's pan and patch back.
            let music = ix - SFX_OFFSET;
            let pan = self.c(music, 0x1E);
            self.ym2(0xB4 + self.channel(), pan);
            if self.c(music, 3) == 0 {
                let v = self.c(music, 4);
                self.fm2_patch_volume(music, v);
                return;
            }
            self.fm2_patch_volume(music, 0);
            return;
        }
        self.fm2_patch_volume(ix, 0);
    }

    fn fm_patch_volume(&mut self, ix: u16, part2: bool, v: u8) {
        if part2 {
            self.fm2_patch_volume(ix, v);
        } else {
            self.fm1_patch_volume(ix, v);
        }
    }

    /// `$06C8` / `$0922`: a frame of the note: the slide, the vibrato, the frequency.
    fn fm_sustain(&mut self, ix: u16, part2: bool) {
        let left = self.c(ix, 2).wrapping_sub(1);
        self.set(ix, 2, left);
        let target = u16::from_le_bytes([self.c(ix, 0x11), self.c(ix, 0x12)]);
        if target != 0 {
            let cur = u16::from_le_bytes([self.c(ix, 0x0E), self.c(ix, 0x0F)]);
            let (d, below) = cur.overflowing_sub(target);
            self.wr(TEMP, (d >> 8) as u8);
            let speed = self.c(ix, 0x1F);
            let step = if below { u16::from(speed) } else { 0xFF00 | u16::from(speed.wrapping_neg()) };
            let new = cur.wrapping_add(step);
            let d2 = new.wrapping_sub(target);
            if (self.rd(TEMP) ^ (d2 >> 8) as u8) & 0x80 != 0 {
                // $073F: there.
                let [lo, hi] = target.to_le_bytes();
                self.set(ix, 0x0F, hi);
                self.set(ix, 0x0E, lo);
                self.set(ix, 0x11, 0);
                self.set(ix, 0x12, 0);
            } else {
                // Across an octave's end, into the next block.
                let f = new & 0x07FF;
                self.set_word(TEMP, f);
                let adj = if f >= 0x04D4 {
                    0x0596
                } else if f < 0x026A {
                    0xFA6A
                } else {
                    0
                };
                let [lo, hi] = new.wrapping_add(adj).to_le_bytes();
                self.set(ix, 0x0F, hi);
                self.set(ix, 0x0E, lo);
            }
        }
        // $074C: the vibrato.
        let delta = if self.c(ix, 0x0A) != 0 {
            let d = self.c(ix, 0x0A) - 1;
            self.set(ix, 0x0A, d);
            0
        } else {
            self.vibrato_step(ix).unwrap_or(0)
        };
        let f = u16::from_le_bytes([self.c(ix, 0x0E), self.c(ix, 0x0F)]).wrapping_add(delta as i8 as u16);
        let [lo, hi] = f.to_le_bytes();
        let (hi_reg, lo_reg) = if part2 { (self.rd(FREQ_HI_2), self.rd(FREQ_LO_2)) } else { (self.rd(FREQ_HI_1), self.rd(FREQ_LO_1)) };
        self.set(ix, 0x0F, hi);
        self.fm_write(ix, part2, hi_reg, hi);
        self.set(ix, 0x0E, lo);
        self.fm_write(ix, part2, lo_reg, lo);
        if part2 && self.rd(SFX_PASS) != 0 {
            return;
        }
        if self.rd(FADE_WAIT) != 0 {
            return;
        }
        let v = self.c(ix, 4);
        self.fm_patch_volume(ix, part2, v);
    }

    /// `$0758`: the vibrato table's next step (`None`: held at `$81`; the FM channels
    /// take it as 0, the PSG's skip the frequency).
    fn vibrato_step(&mut self, ix: u16) -> Option<u8> {
        loop {
            let table = u16::from_le_bytes([self.c(ix, 0x0B), self.c(ix, 0x0C)]);
            let i = self.c(ix, 0x0D);
            self.set(ix, 0x0D, i.wrapping_add(1));
            let v = self.rd(table.wrapping_add(u16::from(i)));
            match v {
                0x81 => {
                    self.set(ix, 0x0D, i);
                    return None;
                }
                0x80 => self.set(ix, 0x0D, 0),
                _ => return Some(v),
            }
        }
    }

    /// `$0A14` (part I) / `$0AC1` (part II): the patch, at volume `v` (with the fade and
    /// the master volume, except an effect's).
    fn fm1_patch_volume(&mut self, ix: u16, v: u8) {
        self.fm_patch(ix, v, false);
    }

    fn fm2_patch_volume(&mut self, ix: u16, v: u8) {
        self.fm_patch(ix, v, true);
    }

    fn fm_patch(&mut self, ix: u16, v: u8, part2: bool) {
        self.set_patch_bank();
        self.set(ix, 4, v);
        let level = if part2 && self.rd(SFX_PASS) != 0 {
            v
        } else {
            let h = self.rd(FADE_LEVEL).wrapping_add(0x0F);
            let a = self.rd(MASTER).wrapping_add(v);
            a.checked_sub(h).unwrap_or(0)
        };
        let mut hl = 0x8000u16.wrapping_add(u16::from(self.c(ix, 0x10)).wrapping_mul(29));
        let alg = self.rd(hl.wrapping_add(0x1C)) & 7;
        self.set(ix, 5, alg);
        let mut reg = self.channel().wrapping_add(0x30);
        for _ in 0..4 {
            let b = self.rd(hl);
            self.fm_write(ix, part2, reg, b);
            hl = hl.wrapping_add(1);
            reg = reg.wrapping_add(4);
        }
        self.wr(PATCH_REG, reg);
        let curve = self.rd(VOLUMES.wrapping_add(u16::from(level)));
        let mut carriers = self.rd(CARRIERS.wrapping_add(u16::from(alg)));
        for _ in 0..4 {
            let tl = self.rd(hl);
            let out = if carriers & 1 != 0 {
                let a = 0x7Fu8.wrapping_sub(tl).wrapping_add(curve);
                if a < 0x7F { a } else { 0x7F }
            } else {
                tl
            };
            carriers >>= 1;
            self.fm_write(ix, part2, reg, out);
            hl = hl.wrapping_add(1);
            reg = reg.wrapping_add(4);
        }
        for _ in 0..0x14 {
            let b = self.rd(hl);
            self.fm_write(ix, part2, reg, b);
            hl = hl.wrapping_add(1);
            reg = reg.wrapping_add(4);
        }
        let b = self.rd(hl);
        self.fm_write(ix, part2, reg.wrapping_add(0x10), b);
        if part2 && self.rd(SFX_PASS) != 0 {
            self.set_voice_bank();
        } else {
            self.set_song_bank();
        }
    }

    // ---- shared commands ----------------------------------------------------------------

    /// `$0F94` (FM `$FC`): $FF slide off, $81 up the slide's speed, else `$0FAD`.
    fn slide_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de);
        if a == 0xFF {
            self.set(ix, 0x1F, 0);
        } else if a >= 0x81 {
            self.set(ix, 0x1F, a & 0x7F);
        } else {
            self.set_release(ix, a);
        }
        *de = de.wrapping_add(1);
    }

    /// `$0FAB` (PSG and DAC `$FC`).
    fn release_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de);
        self.set_release(ix, a);
        *de = de.wrapping_add(1);
    }

    /// `$0FAD`: bit 7 sustain, the rest the release time.
    fn set_release(&mut self, ix: u16, a: u8) {
        self.set(ix, 8, a & 0x80);
        self.set(ix, 6, a & 0x7F);
    }

    /// `$0FBB` (`$FB`): the vibrato table (high nibble) and delay (low).
    fn vibrato_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de);
        let at = VIBRATOS + u16::from((a >> 4) << 1);
        let [lo, hi] = self.word(at).to_le_bytes();
        self.set(ix, 0x0B, lo);
        self.set(ix, 0x0C, hi);
        // (RLA after `CP $FB`: carry clear)
        let mut d = (a << 1) & 0x1E;
        if d != 0 {
            d -= 1;
        }
        self.set(ix, 9, d);
        *de = de.wrapping_add(1);
    }

    /// `$0FE0` (part I `$FA`): the pan, or with bit 0 `$14D0` counts.
    fn pan1_command(&mut self, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de);
        if a & 1 != 0 {
            let n = self.rd(PAN_COUNT).wrapping_add(1);
            self.wr(PAN_COUNT, n);
            *de = de.wrapping_add(1);
            return;
        }
        let reg = 0xB4u8.wrapping_add(self.channel());
        *de = de.wrapping_add(1);
        self.ym1(reg, a & 0xC0);
    }

    /// `$0FF4` (part II and DAC `$FA`): the pan, kept in +$1E.
    fn pan2_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de) & 0xC0;
        self.set(ix, 0x1E, a);
        let reg = 0xB4u8.wrapping_add(self.channel());
        *de = de.wrapping_add(1);
        self.ym2_unless_sfx(ix, reg, a);
    }

    /// `$1010` (`$F9`): transposition (low nibble, bit 7 down) and detune.
    fn shift_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let a = self.rd(*de);
        let mut t = a & 0x8F;
        if t & 0x80 != 0 {
            t |= 0xF0;
        }
        self.set(ix, 0x1C, t);
        self.set(ix, 0x1D, a.rotate_right(3) & 0x0E);
        *de = de.wrapping_add(1);
    }

    /// `$1028` (`$F8`): loops and voltas.
    fn loop_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let b = self.rd(*de);
        *de = de.wrapping_add(1);
        match b >> 5 {
            0 => self.set_word_c(ix, 0x13, *de),
            1 => {
                self.set_word_c(ix, 0x15, *de);
                self.set(ix, 0x1A, 0);
                self.set(ix, 0x1B, 0);
            }
            2 => {
                if self.c(ix, 0x1A) == 0 {
                    self.set(ix, 0x1A, 1);
                } else {
                    self.skip_to(de, 0x60);
                }
            }
            3 => {
                if self.c(ix, 0x1B) == 0 {
                    self.set(ix, 0x1B, 1);
                } else {
                    self.skip_to(de, 0x80);
                }
            }
            4 => {}
            5 => {
                let off = if b & 1 != 0 { 0x13 } else { 0x15 };
                *de = u16::from_le_bytes([self.c(ix, off), self.c(ix, off + 1)]);
            }
            6 => {
                self.set_word_c(ix, 0x17, *de);
                self.set(ix, 0x19, (b & 0x1F) + 1);
            }
            _ => {
                let n = self.c(ix, 0x19).wrapping_sub(1);
                self.set(ix, 0x19, n);
                if n != 0 {
                    *de = u16::from_le_bytes([self.c(ix, 0x17), self.c(ix, 0x18)]);
                }
            }
        }
    }

    fn set_word_c(&mut self, ix: u16, off: u16, v: u16) {
        let [lo, hi] = v.to_le_bytes();
        self.set(ix, off, lo);
        self.set(ix, off + 1, hi);
    }

    /// `$105C` / `$108A`: past the volta's other section, to `$F8 sect` (or `$FF`), left
    /// on it.
    fn skip_to(&mut self, de: &mut u16, sect: u8) {
        loop {
            let b = self.rd(*de);
            *de = de.wrapping_add(1);
            let c = self.rd(*de);
            *de = de.wrapping_add(1);
            if b == 0xFF || (b == 0xF8 && c == sect) {
                *de = de.wrapping_sub(2);
                return;
            }
            if b != 0xF8 && b & 0x80 == 0 {
                *de = de.wrapping_sub(1);
            }
        }
    }

    // ---- the DAC channel ----------------------------------------------------------------

    /// `$0B7A`: the music's (or with `$14D6` the effects') samples.
    fn dac_channel(&mut self) {
        let mut ix = DAC;
        if self.rd(SFX_PASS) != 0 {
            ix += SFX_OFFSET;
        }
        loop {
            if self.c(ix, 3) != 0 {
                return;
            }
            if self.c(ix, 6) == self.c(ix, 2) && self.c(ix, 8) == 0 {
                self.request_sample(ix, 0xFE);
            }
            if self.c(ix, 2) != 0 {
                let left = self.c(ix, 2) - 1;
                self.set(ix, 2, left);
                return;
            }
            let mut de = self.word(ix);
            loop {
                let a = self.rd(de);
                if a & 0xF8 != 0xF8 {
                    break;
                }
                match a {
                    0xFF => {
                        let to = self.word(de.wrapping_add(1));
                        if to == 0 {
                            self.set(ix, 3, 1);
                            self.wr(PRIORITY, 0xFF);
                            return;
                        }
                        de = to;
                    }
                    0xFC => self.release_command(ix, &mut de),
                    0xFA => self.pan2_command(ix, &mut de),
                    0xF8 => self.loop_command(ix, &mut de),
                    _ => de = de.wrapping_add(2),
                }
            }
            let note = self.rd(de) & 0x7F;
            if note != 0x70 {
                self.request_sample(ix, note + 1);
            }
            let len = self.length(ix, &mut de);
            self.set(ix, 2, len);
            de = de.wrapping_add(1);
            self.set_word(ix, de);
        }
    }

    /// A note's length: its own (and the default from now on) or the default.
    fn length(&mut self, ix: u16, de: &mut u16) -> u8 {
        if self.rd(*de) & 0x80 != 0 {
            *de = de.wrapping_add(1);
            let l = self.rd(*de);
            self.set(ix, 7, l);
            l
        } else {
            self.c(ix, 7)
        }
    }

    // ---- PSG ----------------------------------------------------------------------------

    /// `$0C27`: PSG1-3.
    fn psg_channel(&mut self) {
        let ch = self.channel();
        let ix = PSG1 + u16::from(ch) * CHANNEL_SIZE;
        self.wr(PSG_BITS, (ch << 5) & 0x60);
        loop {
            if self.c(ix, 3) != 0 {
                return;
            }
            if self.c(ix, 2) != 0 {
                self.psg_sustain(ix);
                return;
            }
            let mut de = self.word(ix);
            loop {
                let a = self.rd(de);
                if a & 0xF8 != 0xF8 {
                    break;
                }
                match a {
                    0xFF => {
                        let to = self.word(de.wrapping_add(1));
                        if to == 0 {
                            self.set(ix, 3, 1);
                            self.wr(PRIORITY, 0xFF);
                            self.psg_volume(0x0F);
                            return;
                        }
                        de = to;
                    }
                    0xFD => self.envelope_command(ix, &mut de),
                    0xFC => self.release_command(ix, &mut de),
                    0xFB => self.vibrato_command(ix, &mut de),
                    0xFA => {
                        // a new tempo
                        de = de.wrapping_add(1);
                        let t = self.rd(de);
                        de = de.wrapping_add(1);
                        self.ym1(0x26, t);
                    }
                    0xF9 => self.shift_command(ix, &mut de),
                    0xF8 => self.loop_command(ix, &mut de),
                    _ => de = de.wrapping_add(2),
                }
            }
            let note = self.rd(de) & 0x7F;
            let flags = if note == 0x70 {
                self.c(ix, 8) | 1
            } else {
                if self.c(ix, 0x1E) != 0 {
                    self.set(ix, 0x12, 0);
                    self.set(ix, 0x1E, 0);
                    let d = self.c(ix, 9);
                    self.set(ix, 0x0A, d);
                }
                self.set(ix, 0x0D, 0);
                let n = note.wrapping_add(self.c(ix, 0x1C)).wrapping_sub(0x15);
                let f = self.word(PSG_FREQS.wrapping_add(u16::from(n) * 2)).wrapping_add(u16::from(self.c(ix, 0x1D) >> 1));
                let [lo, hi] = f.to_le_bytes();
                self.set(ix, 0x0E, lo);
                self.set(ix, 0x0F, hi);
                self.c(ix, 8) & 0x80
            };
            self.set(ix, 8, flags);
            let len = self.length(ix, &mut de);
            self.set(ix, 2, len);
            de = de.wrapping_add(1);
            self.set_word(ix, de);
        }
    }

    /// `$0D30`: a frame of a PSG note.
    fn psg_sustain(&mut self, ix: u16) {
        let left = self.c(ix, 2).wrapping_sub(1);
        self.set(ix, 2, left);
        let delta = if self.c(ix, 0x0A) != 0 {
            let d = self.c(ix, 0x0A) - 1;
            self.set(ix, 0x0A, d);
            Some(0)
        } else {
            self.vibrato_step(ix)
        };
        if let Some(d) = delta {
            // $0D67 (the PSG's period goes the other way)
            let f = u16::from_le_bytes([self.c(ix, 0x0E), self.c(ix, 0x0F)]).wrapping_add(d.wrapping_neg() as i8 as u16);
            let [lo, hi] = f.to_le_bytes();
            self.set(ix, 0x0F, hi);
            self.set(ix, 0x0E, lo);
        }
        let master = if self.channel() == 2 { 0x0F } else { self.rd(MASTER) };
        self.envelope_frame(ix, master, self.channel() != 2);
        // $0DFA: the period and the volume.
        let (hi, lo) = (self.c(ix, 0x0F), self.c(ix, 0x0E));
        let bits = self.rd(PSG_BITS);
        self.psg((lo & 0x0F) | bits | 0x80);
        let period = u16::from_le_bytes([lo, hi]);
        self.psg(((period >> 4) & 0x3F) as u8);
        let v = 0x0Fu8.wrapping_sub(self.c(ix, 5));
        self.psg(v | bits | 0x90);
    }

    /// `$0D82` / `$0F23`: the envelope's step, the volume (+5) from it, the release, and
    /// the fade (`fades`: not PSG3).
    fn envelope_frame(&mut self, ix: u16, master: u8, fades: bool) {
        let at = self.envelope_at(ix);
        let b = self.rd(at);
        if b & 0x80 == 0 {
            let s = self.c(ix, 0x12).wrapping_add(1);
            self.set(ix, 0x12, s);
        }
        let b = 0x0Fu8.wrapping_sub(b & 0x7F);
        let v = self.c(ix, 4).checked_sub(b).unwrap_or(0);
        let out = master.wrapping_add(v).checked_sub(0x0F).unwrap_or(0);
        self.set(ix, 5, out);
        let flags = self.c(ix, 8);
        let left = self.c(ix, 2);
        let release = left == 0 || (flags & 0x80 == 0 && left == self.c(ix, 6));
        if release && flags == 0 {
            // $0DC9: on past the envelope's next end mark.
            self.set(ix, 0x1E, 1);
            self.set(ix, 8, 2);
            loop {
                let at = self.envelope_at(ix);
                let s = self.c(ix, 0x12).wrapping_add(1);
                self.set(ix, 0x12, s);
                if self.rd(at) & 0x80 != 0 {
                    break;
                }
            }
        }
        if self.rd(FADE_WAIT) == 0 && fades {
            let v = self.c(ix, 4);
            if v != 0 {
                self.set(ix, 4, v - 1);
            }
        }
    }

    /// `$0E2C`: the envelope's current byte.
    fn envelope_at(&self, ix: u16) -> u16 {
        u16::from_le_bytes([self.c(ix, 0x10), self.c(ix, 0x11)]).wrapping_add(u16::from(self.c(ix, 0x12)))
    }

    /// `$0E3C` (`$FD`): volume (low nibble; not while fading, but always on PSG3) and
    /// envelope (high).
    fn envelope_command(&mut self, ix: u16, de: &mut u16) {
        *de = de.wrapping_add(1);
        let c = self.rd(*de);
        if self.channel() == 2 || self.rd(FADING) == 0 {
            self.set(ix, 4, c & 0x0F);
        }
        *de = de.wrapping_add(1);
        let [lo, hi] = self.word(ENVELOPES + u16::from(c >> 4) * 2).to_le_bytes();
        self.set(ix, 0x10, lo);
        self.set(ix, 0x11, hi);
    }

    /// `$0E6C`: the noise channel.
    fn noise_channel(&mut self) {
        let ix = NOISE;
        self.wr(PSG_BITS, 0x60);
        loop {
            if self.c(ix, 3) != 0 {
                return;
            }
            if self.c(ix, 2) != 0 {
                // $0F20
                let left = self.c(ix, 2) - 1;
                self.set(ix, 2, left);
                let master = self.rd(MASTER);
                self.envelope_frame(ix, master, true);
                let v = 0x0Fu8.wrapping_sub(self.c(ix, 5));
                self.psg(v | 0xF0);
                return;
            }
            let mut de = self.word(ix);
            loop {
                let a = self.rd(de);
                if a & 0xF8 != 0xF8 {
                    break;
                }
                match a {
                    0xFF => {
                        let to = self.word(de.wrapping_add(1));
                        if to == 0 {
                            self.set(ix, 3, 1);
                            self.wr(PRIORITY, 0xFF);
                            self.psg_volume(0x0F);
                            return;
                        }
                        de = to;
                    }
                    0xFD => self.envelope_command(ix, &mut de),
                    0xFC => self.release_command(ix, &mut de),
                    0xF8 => self.loop_command(ix, &mut de),
                    _ => de = de.wrapping_add(2),
                }
            }
            let note = self.rd(de) & 0x7F;
            let flags = if note == 0x70 {
                self.c(ix, 8) | 1
            } else {
                if self.c(ix, 0x1E) != 0 {
                    self.set(ix, 0x12, 0);
                    self.set(ix, 0x1E, 0);
                }
                self.psg((note & 7) | 0xE0);
                self.c(ix, 8) & 0x80
            };
            self.set(ix, 8, flags);
            let len = self.length(ix, &mut de);
            self.set(ix, 2, len);
            de = de.wrapping_add(1);
            self.set_word(ix, de);
        }
    }
}
