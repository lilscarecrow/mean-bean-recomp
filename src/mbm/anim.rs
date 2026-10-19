//! Actor animation scripts (`ActorAnimate`, `ActorParseAnim`).
//!
//! A script is a list of (time, frame) byte pairs: a frame shows for time+1 calls.
//! `$FE` ends the script (the call reports done); `$FF 00 <long>` jumps and reports
//! done too. Scripts are addressed by their original ROM addresses so state can be
//! compared with the game's RAM (`aAnim` long at +$32, `aAnimTime` at +$22, `aFrame` +$09).

/// The pair's landing scripts, at their original addresses.
pub mod script {
    /// Squash after a bean drops onto the stack (`unk_4ED4`), ends at `UNK_4EE4`.
    pub const SQUASH: u32 = 0x4ED4;
    /// Nothing: the satellite's default (`unk_4EE4`, a lone `$FE`).
    pub const NONE: u32 = 0x4EE4;
    /// Bounce when the pair touches down mid-fall, then idle (`unk_4EE6`).
    pub const BOUNCE: u32 = 0x4EE6;
    /// The pivot's idle blink loop (`unk_4EFC`).
    pub const IDLE: u32 = 0x4EFC;
}

const BASE: u32 = 0x4ED4;
#[rustfmt::skip]
const BYTES: [u8; 0x32] = [
    // $4ED4 SQUASH
    0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00,
    // $4EE4 NONE / end of SQUASH
    0xFE, 0x00,
    // $4EE6 BOUNCE
    0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00, 0x01, 0x02, 0x00, 0x00, 0x02, 0x03, 0x00, 0x00,
    0xFF, 0x00, 0x00, 0x00, 0x4E, 0xFC, // -> IDLE
    // $4EFC IDLE
    0x0A, 0x01, 0x08, 0x00,
    0xFF, 0x00, 0x00, 0x00, 0x4E, 0xFC, // -> IDLE
];

fn byte(addr: u32) -> u8 {
    let i = addr.checked_sub(BASE).expect("animation script outside the ported range") as usize;
    *BYTES.get(i).expect("animation script outside the ported range")
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

    /// `ActorAnimate`: returns true ("carry") when the script ended or jumped.
    pub fn step(&mut self) -> bool {
        if self.time != 0 {
            self.time -= 1;
            return false;
        }
        match byte(self.ptr) {
            0xFE => true,
            0xFF => {
                self.ptr = long(self.ptr + 2);
                self.parse();
                true
            }
            _ => {
                self.parse();
                false
            }
        }
    }

    /// `ActorParseAnim` (random times, entry bytes >= $F0, aren't used by these scripts).
    fn parse(&mut self) {
        let t = byte(self.ptr);
        assert!(t < 0xF0, "random animation times are not ported yet");
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
}
