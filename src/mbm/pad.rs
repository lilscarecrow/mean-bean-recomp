//! Reading a 3-button pad (`ReadController`): the buttons held, the ones newly pressed,
//! and the auto-repeat the pair moves on. Bits: up 0, down 1, left 2, right 3, B 4,
//! C 5, A 6, start 7.
//!
//! Auto-repeat: a direction repeats on the frame it goes down, then once its timer runs
//! out: `pressed_time` frames the first time, `unpressed_time` after that (the game sets
//! 8 and 2 for a match, `BWRITE pressed_time, $0802`). Left/right and up/down have a
//! timer each, whose top 2 bits remember which of the pair was held.

use super::pair::PadInput;

pub const UP: u8 = 0x01;
pub const DOWN: u8 = 0x02;
pub const LEFT: u8 = 0x04;
pub const RIGHT: u8 = 0x08;
pub const B: u8 = 0x10;
pub const C: u8 = 0x20;
pub const A: u8 = 0x40;
pub const START: u8 = 0x80;

/// One pad's bytes (`p1_ctrl_hold`..: hold, press, repeat, horizontal and vertical
/// repeat timers).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pad {
    pub hold: u8,
    pub press: u8,
    pub repeat: u8,
    pub horizontal: u8,
    pub vertical: u8,
}

impl Pad {
    /// `ReadController` with the buttons now down.
    pub fn read(&mut self, buttons: u8, pressed_time: u8, unpressed_time: u8) {
        let before = self.hold;
        self.hold = buttons;
        self.press = buttons & !before;
        // CtrlVertiTimes, then CtrlHorizTimes.
        self.repeat &= 0xFC;
        Self::timer(&mut self.repeat, &mut self.vertical, buttons, 0x03, 6, pressed_time, unpressed_time);
        self.repeat &= 0xF3;
        Self::timer(&mut self.repeat, &mut self.horizontal, buttons, 0x0C, 4, pressed_time, unpressed_time);
    }

    fn timer(repeat: &mut u8, timer: &mut u8, hold: u8, bits: u8, shift: u32, pressed: u8, unpressed: u8) {
        let changed = (timer.rotate_right(shift) ^ hold) & bits != 0;
        if changed {
            *repeat |= hold & bits;
            *timer = ((hold & bits).rotate_left(shift) & 0xC0) | pressed;
        } else if *timer & 0x3F != 0 {
            *timer -= 1;
        } else {
            *repeat |= hold & bits;
            *timer |= unpressed;
        }
    }

    pub fn input(&self) -> PadInput {
        PadInput { hold: self.hold, press: self.press, repeat: self.repeat }
    }
}
