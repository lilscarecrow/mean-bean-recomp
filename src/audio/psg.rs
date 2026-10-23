//! The SN76489 (PSG): three square waves and a noise channel, clocked at the master
//! clock / 15 and counting every 16 of those. Volumes fall 2 dB a step; the noise is
//! the Sega chip's 16-bit shift register (white noise taps bits 0 and 3).

#[derive(Clone)]
pub struct Psg {
    /// Tone periods (10 bits; the noise's control in [3]), attenuations.
    period: [u16; 4],
    volume: [u8; 4],
    counter: [u16; 4],
    output: [bool; 4],
    lfsr: u16,
    /// The register a data byte goes to.
    latch: usize,
    levels: [i32; 16],
}

impl Default for Psg {
    fn default() -> Self {
        Self::new()
    }
}

impl Psg {
    pub fn new() -> Self {
        let mut levels = [0i32; 16];
        for (i, l) in levels.iter_mut().enumerate().take(15) {
            *l = (2048.0 * 10f64.powf(-0.1 * i as f64)) as i32;
        }
        Psg { period: [0; 4], volume: [0x0F; 4], counter: [0; 4], output: [false; 4], lfsr: 0x8000, latch: 0, levels }
    }

    pub fn write(&mut self, data: u8) {
        if data & 0x80 != 0 {
            let ch = usize::from(data >> 5 & 3);
            self.latch = usize::from(data >> 4 & 7);
            if data & 0x10 != 0 {
                self.volume[ch] = data & 0x0F;
            } else if ch == 3 {
                self.period[3] = u16::from(data & 7);
                self.lfsr = 0x8000;
            } else {
                self.period[ch] = (self.period[ch] & 0x3F0) | u16::from(data & 0x0F);
            }
        } else {
            let ch = self.latch >> 1;
            if self.latch & 1 != 0 {
                self.volume[ch] = data & 0x0F;
            } else if ch == 3 {
                self.period[3] = u16::from(data & 7);
                self.lfsr = 0x8000;
            } else {
                self.period[ch] = (self.period[ch] & 0x0F) | u16::from(data & 0x3F) << 4;
            }
        }
    }

    /// Runs `n` of its 16-clock steps; returns their output summed (mono).
    pub fn run(&mut self, n: u32) -> i32 {
        let mut sum = 0;
        for _ in 0..n {
            for ch in 0..3 {
                if self.counter[ch] > 0 {
                    self.counter[ch] -= 1;
                }
                if self.counter[ch] == 0 {
                    self.counter[ch] = self.period[ch].max(1);
                    self.output[ch] = !self.output[ch];
                }
            }
            if self.counter[3] > 0 {
                self.counter[3] -= 1;
            }
            if self.counter[3] == 0 {
                let rate = self.period[3] & 3;
                self.counter[3] = if rate == 3 { self.period[2].max(1) } else { 0x10 << rate };
                self.output[3] = !self.output[3];
                if self.output[3] {
                    let white = self.period[3] & 4 != 0;
                    let fb = if white { (self.lfsr ^ (self.lfsr >> 3)) & 1 } else { self.lfsr & 1 };
                    self.lfsr = (self.lfsr >> 1) | (fb << 15);
                }
            }
            for ch in 0..3 {
                // (a period of 0 or 1 holds the output high)
                let on = self.output[ch] || self.period[ch] <= 1;
                if on {
                    sum += self.levels[usize::from(self.volume[ch])];
                }
            }
            if self.lfsr & 1 != 0 {
                sum += self.levels[usize::from(self.volume[3])];
            }
        }
        sum
    }
}
