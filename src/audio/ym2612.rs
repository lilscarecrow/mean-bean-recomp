//! The YM2612 (FM): six channels of four operators, run a sample at a time at its own
//! rate (the master clock / 1008, ~53267 Hz). Log-sine and exponent tables, the
//! envelope generator's rate tables and the detune table are the chip's (as the MAME
//! and Genesis Plus GX cores have them). Channel 6 can play the DAC instead. The LFO,
//! SSG-EG and channel 3's special mode aren't used by this game's driver and aren't
//! modelled; the timers are the caller's (see [`super::Audio`]).

/// The log-sine table: 1024 entries over a cycle, (attenuation << 1) | negative.
const SIN_LEN: usize = 1024;
/// The exponent table: 13 octaves of 256 levels, positive then negative.
const TL_RES_LEN: usize = 256;
const TL_TAB_LEN: usize = 13 * 2 * TL_RES_LEN;
/// Attenuation at or past which an operator is silent.
const ENV_QUIET: u32 = (TL_TAB_LEN >> 3) as u32;
const MAX_ATT: u32 = 0x3FF;

const DT_TAB: [[u8; 32]; 4] = [
    [0; 32],
    [0, 0, 0, 0, 1, 1, 1, 1, 1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 8, 8],
    [1, 1, 1, 1, 2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 13, 14, 16, 16, 16, 16],
    [2, 2, 2, 2, 2, 3, 3, 3, 4, 4, 4, 5, 5, 6, 6, 7, 8, 8, 9, 10, 11, 12, 13, 14, 16, 17, 19, 20, 22, 22, 22, 22],
];

/// The key code's low bits from F-number bits 10-7.
const FN_NOTE: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 3, 3, 3, 3, 3, 3];

/// The envelope's increments: 19 patterns of 8 steps.
const EG_INC: [[u8; 8]; 19] = [
    [0, 1, 0, 1, 0, 1, 0, 1],
    [0, 1, 0, 1, 1, 1, 0, 1],
    [0, 1, 1, 1, 0, 1, 1, 1],
    [0, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 1, 1, 1, 1, 1],
    [1, 1, 1, 2, 1, 1, 1, 2],
    [1, 2, 1, 2, 1, 2, 1, 2],
    [1, 2, 2, 2, 1, 2, 2, 2],
    [2, 2, 2, 2, 2, 2, 2, 2],
    [2, 2, 2, 4, 2, 2, 2, 4],
    [2, 4, 2, 4, 2, 4, 2, 4],
    [2, 4, 4, 4, 2, 4, 4, 4],
    [4, 4, 4, 4, 4, 4, 4, 4],
    [4, 4, 4, 8, 4, 4, 4, 8],
    [4, 8, 4, 8, 4, 8, 4, 8],
    [4, 8, 8, 8, 4, 8, 8, 8],
    [8, 8, 8, 8, 8, 8, 8, 8],
    [16, 16, 16, 16, 16, 16, 16, 16],
    [0, 0, 0, 0, 0, 0, 0, 0],
];

/// A rate (0-127: 32 dummies, 64 real, 32 past the top) → (increment pattern, shift).
fn eg_rate(rate: u32) -> (usize, u32) {
    match rate {
        0..=31 => (18, 0),
        32..=79 => (((rate - 32) & 3) as usize, 11 - (rate - 32) / 4),
        80..=91 => (4 + (rate - 80) as usize, 0),
        _ => (16, 0),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Default, Debug)]
enum Eg {
    Attack,
    Decay,
    Sustain,
    Release,
    #[default]
    Off,
}

#[derive(Clone, Copy, Default)]
struct Slot {
    dt: u8,
    mul: u32,
    tl: u32,
    ks: u8,
    ar: u32,
    d1r: u32,
    d2r: u32,
    rr: u32,
    sl: u32,
    key: bool,
    state: Eg,
    volume: u32,
    phase: u32,
    incr: u32,
    // (rates with the key scale applied)
    eg_ar: (usize, u32),
    eg_d1r: (usize, u32),
    eg_d2r: (usize, u32),
    eg_rr: (usize, u32),
    ar_ksr: u32,
}

#[derive(Clone, Copy, Default)]
struct Channel {
    slots: [Slot; 4],
    algo: u8,
    fb: u8,
    fnum: u32,
    block: u32,
    kcode: u8,
    /// The latch of `$A4` (block and F-number's top bits) until `$A0`.
    latch: u8,
    pan: (bool, bool),
    op1_out: [i32; 2],
    mem: i32,
}

#[derive(Clone)]
pub struct Ym2612 {
    ch: [Channel; 6],
    /// Part I / II address latches.
    addr: [u8; 2],
    dac_on: bool,
    dac: i32,
    eg_timer: u32,
    eg_cnt: u32,
    sin_tab: Vec<u32>,
    tl_tab: Vec<i32>,
}

impl Default for Ym2612 {
    fn default() -> Self {
        Self::new()
    }
}

/// Slot index (S1, S3, S2, S4) by the register's operator offset (0, 4, 8, C).
const SLOT_OF_REG: [usize; 4] = [0, 2, 1, 3];
const S1: usize = 0;
const S2: usize = 1;
const S3: usize = 2;
const S4: usize = 3;

impl Ym2612 {
    pub fn new() -> Self {
        let mut tl_tab = vec![0i32; TL_TAB_LEN];
        for x in 0..TL_RES_LEN {
            let m = (1u32 << 16) as f64 / 2f64.powf((x as f64 + 1.0) * (128.0 / 1024.0 / 4.0) / 8.0);
            // 16 bits, to 12, rounded to 11, then 13 as on the chip
            let n = ((((m.floor() as i32) >> 4) + 1) >> 1) << 2;
            for i in 0..13 {
                tl_tab[x * 2 + i * 2 * TL_RES_LEN] = n >> i;
                tl_tab[x * 2 + 1 + i * 2 * TL_RES_LEN] = -(n >> i);
            }
        }
        let mut sin_tab = vec![0u32; SIN_LEN];
        for (i, s) in sin_tab.iter_mut().enumerate() {
            let m = (((i * 2) + 1) as f64 * std::f64::consts::PI / SIN_LEN as f64).sin();
            let o = 8.0 * (1.0 / m.abs()).log2() / (128.0 / 1024.0 / 4.0);
            let n = (2.0 * o) as i64;
            let n = (n >> 1) + (n & 1);
            *s = (n * 2) as u32 + u32::from(m < 0.0);
        }
        let mut ym = Ym2612 { ch: [Channel::default(); 6], addr: [0; 2], dac_on: false, dac: 0, eg_timer: 0, eg_cnt: 1, sin_tab, tl_tab };
        for c in &mut ym.ch {
            c.pan = (true, true);
            for s in &mut c.slots {
                s.volume = MAX_ATT;
                s.mul = 1;
            }
        }
        ym
    }

    /// A register write: `part` 0 or 1.
    pub fn write(&mut self, part: u8, reg: u8, data: u8) {
        let part = usize::from(part & 1);
        self.addr[part] = reg;
        if part == 0 && reg < 0x30 {
            self.write_mode(reg, data);
            return;
        }
        if reg < 0x30 || (reg & 3) == 3 {
            return;
        }
        let ch = usize::from(reg & 3) + part * 3;
        let c = &mut self.ch[ch];
        if reg < 0xA0 {
            let s = &mut c.slots[SLOT_OF_REG[usize::from(reg >> 2 & 3)]];
            match reg & 0xF0 {
                0x30 => {
                    s.mul = if data & 0x0F != 0 { u32::from(data & 0x0F) * 2 } else { 1 };
                    s.dt = data >> 4 & 7;
                }
                0x40 => s.tl = u32::from(data & 0x7F) << 3,
                0x50 => {
                    s.ks = 3 - (data >> 6);
                    s.ar = if data & 0x1F != 0 { 32 + (u32::from(data & 0x1F) << 1) } else { 0 };
                }
                0x60 => s.d1r = if data & 0x1F != 0 { 32 + (u32::from(data & 0x1F) << 1) } else { 0 },
                0x70 => s.d2r = if data & 0x1F != 0 { 32 + (u32::from(data & 0x1F) << 1) } else { 0 },
                0x80 => {
                    let sl = u32::from(data >> 4);
                    s.sl = if sl == 15 { 31 * 32 } else { sl * 32 };
                    s.rr = 34 + (u32::from(data & 0x0F) << 2);
                }
                _ => {} // $90: SSG-EG
            }
            Self::refresh(c);
            return;
        }
        match reg & 0xFC {
            0xA0 => {
                c.fnum = u32::from(c.latch & 7) << 8 | u32::from(data);
                c.block = u32::from(c.latch >> 3 & 7);
                c.kcode = (c.block as u8) << 2 | FN_NOTE[(c.fnum >> 7) as usize & 15];
                Self::refresh(c);
            }
            0xA4 => c.latch = data & 0x3F,
            0xB0 => {
                c.fb = data >> 3 & 7;
                c.algo = data & 7;
            }
            0xB4 => c.pan = (data & 0x80 != 0, data & 0x40 != 0),
            _ => {} // channel 3's extra frequencies ($A8-$AE)
        }
    }

    fn write_mode(&mut self, reg: u8, data: u8) {
        match reg {
            0x28 => {
                let ch = match data & 7 {
                    c @ 0..=2 => usize::from(c),
                    c @ 4..=6 => usize::from(c) - 1,
                    _ => return,
                };
                let c = &mut self.ch[ch];
                // bits 4-7: S1, S2, S3, S4
                for (bit, slot) in [(0x10, S1), (0x20, S2), (0x40, S3), (0x80, S4)] {
                    let s = &mut c.slots[slot];
                    if data & bit != 0 {
                        if !s.key {
                            s.key = true;
                            s.phase = 0;
                            s.state = Eg::Attack;
                            if s.ar_ksr >= 94 {
                                s.volume = 0;
                                s.state = Eg::Decay;
                            }
                        }
                    } else if s.key {
                        s.key = false;
                        if s.state != Eg::Off {
                            s.state = Eg::Release;
                        }
                    }
                }
            }
            0x2A => self.dac = (i32::from(data) - 0x80) << 6,
            0x2B => self.dac_on = data & 0x80 != 0,
            _ => {} // LFO, timers (the caller's), channel 3 mode
        }
    }

    /// A channel's phase increments and key-scaled rates from its frequency.
    fn refresh(c: &mut Channel) {
        let fc = (c.fnum << c.block) >> 1;
        let kc = c.kcode;
        for s in &mut c.slots {
            let d = u32::from(DT_TAB[usize::from(s.dt & 3)][usize::from(kc & 31)]);
            let f = if s.dt & 4 != 0 { fc.wrapping_sub(d) } else { fc + d } & 0x1FFFF;
            s.incr = (f * s.mul) >> 1;
            let ksr = u32::from(kc >> s.ks);
            s.ar_ksr = s.ar + ksr;
            s.eg_ar = if s.ar_ksr < 94 { eg_rate(s.ar_ksr) } else { (17, 0) };
            s.eg_d1r = eg_rate(s.d1r + ksr);
            s.eg_d2r = eg_rate(s.d2r + ksr);
            s.eg_rr = eg_rate(s.rr + ksr);
        }
    }

    fn op(&self, phase: u32, env: u32, pm: i32) -> i32 {
        let i = ((phase >> 10) as i32).wrapping_add(pm) as usize & (SIN_LEN - 1);
        let p = (env << 3) as usize + self.sin_tab[i] as usize;
        if p >= TL_TAB_LEN { 0 } else { self.tl_tab[p] }
    }

    fn channel_out(&mut self, ch: usize) -> i32 {
        let c = self.ch[ch];
        let env = |s: &Slot| s.volume + s.tl;
        let (mut m2, mut c1, mut c2, mut mem) = (0i32, 0i32, 0i32, 0i32);
        let mut out = 0i32;
        // The delayed MEM value into M2 or C2.
        match c.algo {
            0..=2 | 5 => m2 = c.mem,
            3 => c2 = c.mem,
            _ => {}
        }
        // S1 with feedback.
        let fb_in = c.op1_out[0] + c.op1_out[1];
        let o1 = c.op1_out[1];
        match c.algo {
            0 | 3 | 4 | 6 => c1 = o1,
            1 => mem = o1,
            2 => c2 = o1,
            5 => {
                mem = o1;
                c1 = o1;
                c2 = o1;
            }
            _ => out += o1,
        }
        let e = env(&c.slots[S1]);
        let new1 = if e < ENV_QUIET {
            let pm = if c.fb != 0 { fb_in >> (10 - c.fb) } else { 0 };
            self.op(c.slots[S1].phase, e, pm)
        } else {
            0
        };
        // S3 (M2)
        let e = env(&c.slots[S3]);
        if e < ENV_QUIET {
            let v = self.op(c.slots[S3].phase, e, m2 >> 1);
            match c.algo {
                0..=3 => c2 += v,
                4 => c2 += v,
                _ => out += v,
            }
        }
        // S2 (C1)
        let e = env(&c.slots[S2]);
        if e < ENV_QUIET {
            let v = self.op(c.slots[S2].phase, e, c1 >> 1);
            match c.algo {
                0..=3 => mem += v,
                _ => out += v,
            }
        }
        // S4 (C2)
        let e = env(&c.slots[S4]);
        if e < ENV_QUIET {
            out += self.op(c.slots[S4].phase, e, c2 >> 1);
        }
        let ch_mut = &mut self.ch[ch];
        ch_mut.op1_out = [o1, new1];
        ch_mut.mem = mem;
        out.clamp(-8192, 8191)
    }

    fn advance_eg(&mut self) {
        let cnt = self.eg_cnt;
        for c in &mut self.ch {
            for s in &mut c.slots {
                let step = |r: (usize, u32)| -> Option<u32> {
                    if cnt & ((1 << r.1) - 1) == 0 { Some(u32::from(EG_INC[r.0][((cnt >> r.1) & 7) as usize])) } else { None }
                };
                match s.state {
                    Eg::Attack => {
                        if let Some(inc) = step(s.eg_ar) {
                            let v = s.volume as i32 + ((!(s.volume as i32)) * inc as i32 >> 4);
                            if v <= 0 {
                                s.volume = 0;
                                s.state = Eg::Decay;
                            } else {
                                s.volume = v as u32;
                            }
                        }
                    }
                    Eg::Decay => {
                        if let Some(inc) = step(s.eg_d1r) {
                            s.volume += inc;
                            if s.volume >= s.sl {
                                s.state = Eg::Sustain;
                            }
                        }
                    }
                    Eg::Sustain => {
                        if let Some(inc) = step(s.eg_d2r) {
                            s.volume = (s.volume + inc).min(MAX_ATT);
                        }
                    }
                    Eg::Release => {
                        if let Some(inc) = step(s.eg_rr) {
                            s.volume += inc;
                            if s.volume >= MAX_ATT {
                                s.volume = MAX_ATT;
                                s.state = Eg::Off;
                            }
                        }
                    }
                    Eg::Off => {}
                }
            }
        }
    }

    /// One sample: (left, right), each channel ±8192 summed.
    pub fn sample(&mut self) -> (i32, i32) {
        let (mut l, mut r) = (0, 0);
        for ch in 0..6 {
            let v = if ch == 5 && self.dac_on { self.dac } else { self.channel_out(ch) };
            let (pl, pr) = self.ch[ch].pan;
            if pl {
                l += v;
            }
            if pr {
                r += v;
            }
        }
        // The envelope every 3 samples, then the phases.
        self.eg_timer += 1;
        if self.eg_timer >= 3 {
            self.eg_timer = 0;
            self.advance_eg();
            self.eg_cnt += 1;
            if self.eg_cnt == 4096 {
                self.eg_cnt = 1;
            }
        }
        for c in &mut self.ch {
            for s in &mut c.slots {
                s.phase = s.phase.wrapping_add(s.incr) & 0xF_FFFF;
            }
        }
        (l, r)
    }
}
