//! Actor animation scripts (`ActorAnimate`, `ActorParseAnim`).
//!
//! A script is a list of (time, frame) byte pairs: a frame shows for time+1 calls.
//! `$FE` ends the script (the call reports done); `$FF 00 <long>` jumps and reports
//! done too. A time of $F0 or more picks a random time (`ActorRandomAnimTime`). Scripts
//! are addressed by their original ROM addresses so state can be compared with the
//! game's RAM (`aAnim` long at +$32, `aAnimTime` at +$22, `aFrame` +$09). They are read
//! from the installed ROM (`mbm::rom`); the few written out below work without one.

/// Scripts by their original addresses.
pub mod script {
    /// Pair: squash after a bean drops onto the stack (`unk_4ED4`), ends at `NONE`.
    pub const SQUASH: u32 = 0x4ED4;
    /// Nothing: the satellite's default (`unk_4EE4`, a lone `$FE`).
    pub const NONE: u32 = 0x4EE4;
    /// Pair: bounce when touching down mid-fall, then idle (`unk_4EE6`).
    pub const BOUNCE: u32 = 0x4EE6;
    /// Pair: the pivot's idle blink loop (`unk_4EFC`).
    pub const IDLE: u32 = 0x4EFC;
    /// A popping bean (`Anim_PuyoPop`).
    pub const POP: u32 = 0x4B88;
    /// A piece flying off a popped bean (`Anim_PuyoPoppedPiece`).
    pub const POPPED_PIECE: u32 = 0x4D98;
    /// Garbage cleared by a pop (`Anim_GarbageRemove`).
    pub const GARBAGE_CLEAR: u32 = 0x4DAE;
    /// Chain glow, P1 / P2, while flying (`byte_2374C`, `byte_2377A`) and at the end
    /// (`byte_2375E`, `byte_2378C`).
    pub const GLOW: [u32; 2] = [0x2374C, 0x2377A];
    pub const GLOW_END: [u32; 2] = [0x2375E, 0x2378C];
}

/// Script bytes, by the address of their first byte.
#[rustfmt::skip]
const BLOCKS: &[(u32, &[u8])] = &[
    (0x4B88, &[0x08, 0x08, 0x01, 0x04, 0x01, 0x05, 0x01, 0x06, 0xFE, 0x00]),
    (0x4D98, &[0x01, 0x06, 0x03, 0x05, 0x06, 0x04, 0x03, 0x05, 0x04, 0x06, 0xFE, 0x00]),
    (0x4DAE, &[0x06, 0x00, 0x06, 0x01, 0x06, 0x02, 0x06, 0x03, 0xFE, 0x00]),
    (0x4ED4, &[
        // SQUASH
        0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00,
        // NONE / end of SQUASH
        0xFE, 0x00,
        // BOUNCE
        0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00,
        0xFF, 0x00, 0x00, 0x00, 0x4E, 0xFC, // -> IDLE
        // IDLE
        0x0A, 0x01, 0x08, 0x00,
        0xFF, 0x00, 0x00, 0x00, 0x4E, 0xFC, // -> IDLE
    ]),
    (0x2374C, &[
        // GLOW P1
        0x00, 0x07, 0x00, 0x08, 0x00, 0x09, 0x01, 0x0A, 0x00, 0x09, 0x00, 0x08,
        0xFF, 0x00, 0x00, 0x02, 0x37, 0x4C,
        // GLOW_END P1
        0x02, 0x07, 0x01, 0x08, 0x01, 0x09, 0x06, 0x0A, 0x00, 0x09, 0x00, 0x08, 0x00, 0x07,
        0x03, 0x0B, 0x03, 0x0C, 0x03, 0x0D, 0x03, 0x0E, 0x03, 0x0F, 0x03, 0x10, 0xFE, 0x00,
        // GLOW P2
        0x00, 0x11, 0x00, 0x12, 0x00, 0x13, 0x01, 0x14, 0x00, 0x13, 0x00, 0x12,
        0xFF, 0x00, 0x00, 0x02, 0x37, 0x7A,
        // GLOW_END P2
        0x02, 0x11, 0x01, 0x12, 0x01, 0x13, 0x06, 0x14, 0x00, 0x13, 0x00, 0x12, 0x00, 0x11,
        0x03, 0x15, 0x03, 0x16, 0x03, 0x17, 0x03, 0x18, 0x03, 0x19, 0x03, 0x1A, 0xFE, 0x00,
    ]),
];

/// `ActorRandomAnimTime`: times by the entry's low 3 bits (row) and `Random & 7`.
#[rustfmt::skip]
const RANDOM_TIMES: [[u8; 8]; 7] = [
    [0x30, 0x60, 0x02, 0x6C, 0x40, 0x46, 0x50, 0x5C],
    [0x30, 0x60, 0x20, 0x6C, 0x40, 0x46, 0x50, 0x5C],
    [0x00, 0x00, 0x00, 0x00, 0x00, 0x46, 0x50, 0x5C],
    [0x80, 0xA0, 0x60, 0x78, 0xBD, 0xAA, 0xB4, 0xC0],
    [0x00, 0x00, 0x00, 0x00, 0x20, 0x10, 0x0C, 0x30],
    [0x30, 0x60, 0x00, 0x6C, 0x40, 0x46, 0x50, 0x5C],
    [0x30, 0x60, 0x00, 0x6C, 0x40, 0x46, 0x50, 0x5C],
];

fn byte(addr: u32) -> u8 {
    if super::rom::get().is_some() {
        return super::rom::byte(addr);
    }
    for &(base, bytes) in BLOCKS {
        if let Some(&b) = addr.checked_sub(base).and_then(|i| bytes.get(i as usize)) {
            return b;
        }
    }
    panic!("animation script at ${addr:X} is not ported");
}

fn long(addr: u32) -> u32 {
    (0..4).fold(0, |acc, i| (acc << 8) | u32::from(byte(addr + i)))
}

/// An actor's animation state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anim {
    /// Next script entry (ROM address).
    pub ptr: u32,
    /// Calls left on the current frame.
    pub time: u8,
    /// Current sprite frame.
    pub frame: u8,
}

impl Default for Anim {
    /// The empty script: finished from the first call.
    fn default() -> Anim {
        Anim::new(script::NONE)
    }
}

impl Anim {
    pub fn new(ptr: u32) -> Anim {
        Anim { ptr, time: 0, frame: 0 }
    }

    /// `ActorAnimate`: returns true ("carry") when the script ended or jumped. For
    /// scripts without random times.
    pub fn step(&mut self) -> bool {
        let at = self.ptr;
        self.step_with(&mut || panic!("animation script near ${at:X} has random times: use step_with"))
    }

    /// `ActorAnimate`, drawing random frame times from `random` (`Random`).
    pub fn step_with(&mut self, random: &mut dyn FnMut() -> u16) -> bool {
        if self.time != 0 {
            self.time -= 1;
            return false;
        }
        match byte(self.ptr) {
            0xFE => true,
            0xFF => {
                self.ptr = long(self.ptr + 2);
                self.parse(random);
                true
            }
            _ => {
                self.parse(random);
                false
            }
        }
    }

    /// `ActorParseAnim`.
    fn parse(&mut self, random: &mut dyn FnMut() -> u16) {
        let mut t = byte(self.ptr);
        if t >= 0xF0 {
            t = RANDOM_TIMES[usize::from(t & 7)][usize::from(random() as u8 & 7)];
        }
        self.time = t;
        self.frame = byte(self.ptr + 1);
        self.ptr += 2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squash_takes_fifteen_calls() {
        let mut a = Anim::new(script::SQUASH);
        let calls = (1..).find(|_| a.step()).unwrap();
        assert_eq!(calls, 15);
    }

    #[test]
    fn idle_loops_every_nineteen_calls() {
        let mut a = Anim::new(script::IDLE);
        let first = (1..).find(|_| a.step()).unwrap();
        assert_eq!(first, 11 + 9 + 1);
        assert_eq!(a.ptr, script::IDLE + 2);
    }

    #[test]
    fn pop_scripts_lengths() {
        let calls = |ptr| {
            let mut a = Anim::new(ptr);
            (1..).find(|_| a.step()).unwrap()
        };
        assert_eq!(calls(script::POP), 9 + 2 + 2 + 2 + 1);
        assert_eq!(calls(script::POPPED_PIECE), 2 + 4 + 7 + 4 + 5 + 1);
        assert_eq!(calls(script::GARBAGE_CLEAR), 4 * 7 + 1);
        assert_eq!(calls(script::GLOW[0]), 1 + 1 + 1 + 2 + 1 + 1 + 1);
    }
}
