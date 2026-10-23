//! The boot checks before the Sega logo. `GetRegion`: this is a US cartridge, so any
//! other console (Japanese, or a PAL one) gets the region lockout (`RegionLockout`: two
//! lines of text, then `ActRegionLockout`), which on a Japanese console a code on pad 2
//! gets past (A A ↑ → ↓ ← B B → ↓ ← ↑ C C ↓ ← ↑ → START START, the first press giving
//! $258 frames for the rest; one wrong press and it's stuck). `CheckChecksum`: the ROM's
//! words from $200 summed against the header's; a mismatch shows "WARNING. CHECKSUM IS
//! WRONG." for $100 frames or until a button (`ChecksumError`, `ActChecksumError`).

use super::*;
use crate::rom;

/// `ActRegionLockout` (its first frame), `ActRegionLockout`'s code check, and where a
/// wrong press leaves it.
pub const LOCKOUT_CODE: u32 = 0x2250A;
pub const LOCKOUT_CHECK_CODE: u32 = 0x2251C;
pub const LOCKOUT_STUCK_CODE: u32 = 0x22578;
/// `ActChecksumError`, then waiting (`ActorBookmark_Ctrl`).
pub const CHECKSUM_ERROR_CODE: u32 = 0x22714;
pub const CHECKSUM_ERROR_WAIT_CODE: u32 = 0x2271E;

/// `Str_RegionLockNTSC` (2 lines of 40), its font table, where it goes.
const LOCKOUT_TEXT: u32 = 0x22418;
const LOCKOUT_FONT: u32 = 0x225B2;
const LOCKOUT_AT: u16 = 0xC500;
const LOCKOUT_ATTR: u16 = 0x0500;
/// `LockoutBypassCode` (presses, then $FF).
const BYPASS_CODE: u32 = 0x23520;
/// `Str_ChecksumWarning`, `Str_ChecksumIncorrect`.
const CHECKSUM_WARNING: u32 = 0x226F6;
const CHECKSUM_INCORRECT: u32 = 0x22700;

/// `ActRegionLockout`: +$26 the code's next press, +$28 the frames left for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegionLockout {
    pub state: LockoutState,
    pub index: u16,
    pub timer: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LockoutState {
    Start,
    Check,
    Stuck,
}

impl RegionLockout {
    pub fn code(&self) -> u32 {
        match self.state {
            LockoutState::Start => LOCKOUT_CODE,
            LockoutState::Check => LOCKOUT_CHECK_CODE,
            LockoutState::Stuck => LOCKOUT_STUCK_CODE,
        }
    }
}

/// `ActChecksumError`: +$24 the delay.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChecksumError {
    pub started: bool,
    pub delay: u16,
}

impl Engine {
    pub(super) fn run_boot_routine(&mut self, at: u32) -> bool {
        match at {
            // GetRegion: clear for a US (NTSC) console.
            0x224B8 => self.bytecode_flag = (self.console & 0xC0).wrapping_sub(0x80),
            // RegionLockout (`RegionLock_Print` straight into plane A)
            0x224D6 => {
                let mut at = LOCKOUT_AT;
                for line in 0..2u32 {
                    let mut to = at;
                    for i in 0..40u32 {
                        let c = rom::byte(LOCKOUT_TEXT + line * 40 + i);
                        let tile = u16::from(rom::byte(LOCKOUT_FONT + u32::from(c))).wrapping_add(LOCKOUT_ATTR);
                        self.plane_writes.push((to, tile));
                        to = to.wrapping_add(2);
                    }
                    at = at.wrapping_add(0x80);
                }
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::RegionLockout(RegionLockout { state: LockoutState::Start, index: 0, timer: 0 }));
                }
            }
            // CheckChecksum: it takes a while.
            0x234D6 => {
                self.long_step();
                let end = rom::long(0x1A4).wrapping_add(1);
                let mut sum = 0u16;
                let mut a = 0x200u32;
                while a < end {
                    sum = sum.wrapping_add(rom::word(a));
                    a += 2;
                }
                self.rom_checksum = sum;
                self.bytecode_flag = if sum == rom::word(0x18E) { 0 } else { 0xFF };
            }
            // ChecksumError: plane B checkered, plane A's buffer cleared, the warning.
            0x22682 => {
                self.use_plane_a_buffer = true;
                let mut tile = 0x406Cu16;
                for row in 0..0x1C {
                    for col in 0..0x28 {
                        self.plane_writes.push((0xE000 + row * 0x80 + col * 2, tile));
                        tile ^= 1;
                    }
                    tile ^= 2;
                }
                self.plane_a_buffer.fill(0x8500);
                self.print_raw(0x5A0, 0xA500, CHECKSUM_WARNING);
                self.print_raw(0x796, 0x8500, CHECKSUM_INCORRECT);
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::ChecksumError(ChecksumError { started: false, delay: 0 }));
                }
            }
            _ => return false,
        }
        true
    }

    /// `Options_PrintRaw`: each character two tiles high (2c over 2c + 1) into the plane
    /// A buffer at a byte offset.
    fn print_raw(&mut self, mut at: u16, attr: u16, mut text: u32) {
        loop {
            let c = rom::byte(text);
            if c & 0x80 != 0 {
                return;
            }
            let top = (attr & 0xFF00) | u16::from(c << 1);
            for (off, w) in [(0, top), (0x80, top + 1)] {
                if let Some(cell) = self.plane_a_buffer.get_mut(usize::from(at.wrapping_add(off) / 2)) {
                    *cell = w;
                }
            }
            at = at.wrapping_add(2);
            text += 1;
        }
    }

    pub(super) fn run_region_lockout(&mut self, mut l: RegionLockout, inputs: &Inputs) -> Option<RegionLockout> {
        match l.state {
            LockoutState::Start => {
                l.index = 0;
                l.timer = 0x258;
                l.state = LockoutState::Check;
            }
            LockoutState::Check => {
                // (on a PAL console there's no way past)
                if self.console & 0x40 != 0 {
                    l.state = LockoutState::Stuck;
                    return Some(l);
                }
                if l.index != 0 {
                    l.timer = l.timer.wrapping_sub(1);
                    if l.timer & 0x8000 != 0 {
                        l.state = LockoutState::Stuck;
                        return Some(l);
                    }
                }
                let want = rom::byte(BYPASS_CODE + u32::from(l.index));
                if want == 0xFF {
                    self.bytecode_disabled = false;
                    return Some(l);
                }
                let press = inputs.pads[1].press;
                if press == 0 {
                    return Some(l);
                }
                if press != want {
                    l.state = LockoutState::Stuck;
                    return Some(l);
                }
                l.index += 1;
            }
            LockoutState::Stuck => {}
        }
        Some(l)
    }

    pub(super) fn run_checksum_error(&mut self, mut c: ChecksumError, inputs: &Inputs) -> Option<ChecksumError> {
        if !c.started {
            c.started = true;
            c.delay = 0x100;
            return Some(c);
        }
        let press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0;
        if !bookmark_ctrl(&mut c.delay, press) {
            return Some(c);
        }
        self.use_plane_a_buffer = false;
        self.bytecode_disabled = false;
        None
    }
}
