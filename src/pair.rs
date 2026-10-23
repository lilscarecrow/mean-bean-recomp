//! The falling pair while the player controls it, one frame at a time.
//!
//! Mirrors the pivot actor's controlled state (resumes at $4F14 each frame):
//! `sub_5060` (sideways), `sub_5384` (rotate), `sub_50DA` (fall / lock), plus the
//! satellite's rotation animation (`sub_5530`). Fields keep the game's 16-bit words.

use super::anim::{script, Anim};
use super::field::{Field, DIR};

/// Pivot kick when a turn is blocked (`word_542C`), indexed by the new rotation.
const KICK: [(i16, i16); 4] = [(0, 1), (-1, 0), (0, -1), (1, 0)];
/// Rotate-button meaning by OR of the assignments (`byte_577A`): bit 6 left, bit 5 right.
const ROTATE_BITS: [u8; 4] = [0x00, 0x40, 0x20, 0x40];
/// Pad bits in the hold/press bytes.
pub const PAD_UP: u8 = 0x01;
pub const PAD_DOWN: u8 = 0x02;
pub const PAD_LEFT: u8 = 0x04;
pub const PAD_RIGHT: u8 = 0x08;
pub const PAD_B: u8 = 0x10;
pub const PAD_C: u8 = 0x20;
pub const PAD_A: u8 = 0x40;
/// Contacts with the stack (half-cell crossings while supported) before locking.
const MAX_CONTACTS: u16 = 8;

/// One frame of a player's controller, as the game keeps it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PadInput {
    /// Buttons held.
    pub hold: u8,
    /// Buttons newly pressed this frame.
    pub press: u8,
    /// Sideways presses with auto-repeat (what the pair actually moves on).
    pub repeat: u8,
}

/// Where the pivot gets its controller from (`sub_56C0`), asked at each point the ROM
/// reads it: up to four times a frame (move, turn, fall, lock delay). A human's pad is
/// the same all frame; the CPU's (`ai::CpuPad`) depends on where the pair is by then.
pub trait PadSource {
    fn read(&mut self, pair: &Pair) -> PadInput;
}

impl PadSource for PadInput {
    fn read(&mut self, _: &Pair) -> PadInput {
        *self
    }
}

impl<T: PadSource + ?Sized> PadSource for &mut T {
    fn read(&mut self, pair: &Pair) -> PadInput {
        (**self).read(pair)
    }
}

/// Rotate-button assignments for A, B, C: 0 unused, 1 turn left, 2 turn right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RotateKeys(pub [u8; 3]);

impl RotateKeys {
    /// `sub_5712`: which way the pressed buttons turn the pair.
    fn turn(self, press: u8) -> i8 {
        let mut assigned = 0;
        for (mask, key) in [PAD_A, PAD_B, PAD_C].into_iter().zip(self.0) {
            if press & mask != 0 {
                assigned |= key;
            }
        }
        match ROTATE_BITS[(assigned & 3) as usize] {
            0x40 => -1,
            0x20 => 1,
            _ => 0,
        }
    }
}

/// Controlled pair state: the pivot's fields and the satellite's rotation animation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pair {
    /// Pivot column ($1A) and row counting the 2 hidden rows ($1C).
    pub col: u16,
    pub row: u16,
    /// Satellite side: 0 up, 1 right, 2 down, 3 left ($2B).
    pub rot: u8,
    /// Fall progress through the current cell ($20); bit 15 = lower half.
    pub fall: u16,
    /// Sideways slide offset in pixels, back to 0 the next frame ($1E).
    pub slide: u16,
    /// Half-cell crossings while resting on something ($26).
    pub contacts: u16,
    /// Lock delay countdown once fully resting ($28).
    pub lock_timer: u16,
    /// Stopped taking input, waiting for animations to finish ($07 bit 3).
    pub locked: bool,
    /// Satellite angle ($36 of the satellite; $40 per quarter turn) and turn step ($38).
    pub sat_angle: u8,
    pub sat_turn: u8,
    /// Sprite animations; they also time the landing (see `actors`).
    pub pivot_anim: Anim,
    pub sat_anim: Anim,
}

/// What else a pivot's frame did, for its player and the stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PivotEvents {
    /// The player earns a soft-drop point (`sub_50DA` sets the player's flag as soon as
    /// it sees down held on an odd frame, whatever the pair does next).
    pub soft_drop_point: bool,
    /// The pair touched down while falling (`PuyoLandEffects`).
    pub touched_down: bool,
    /// It moved a column (`PlayPuyoMoveSound`) / turned (`PlayPuyoRotateSound`).
    pub moved: bool,
    pub turned: bool,
}

/// What a frame did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Still falling (or finishing a slide or turn after locking).
    Falling,
    /// Locked in place: the beans now drop into the field.
    Landed,
}

impl Pair {
    /// `CheckPuyoLand2`: bit 0 = pivot blocked one step in `dir`, bit 1 = satellite.
    fn blocked(&self, field: &Field, dir: u8) -> u8 {
        let (dc, dr) = DIR[(dir & 3) as usize];
        let (rc, rr) = DIR[(self.rot & 3) as usize];
        let (c, r) = (self.col as i16 + dc, self.row as i16 + dr);
        let mut bits = 0;
        if field.blocked(c, r) {
            bits |= 1;
        }
        if field.blocked(c + rc, r + rr) {
            bits |= 2;
        }
        bits
    }

    /// `CheckPuyoLand`: something under either bean; a vertical pair counts as both.
    fn resting(&self, field: &Field) -> u8 {
        let bits = self.blocked(field, 2);
        if bits != 0 && self.rot & 1 == 0 { bits | 3 } else { bits }
    }

    /// One frame. `fall_speed` is the player's per-frame fall rate ($1A of the player
    /// actor); `frame_count` decides soft-drop points; `satellite_first` is true when the
    /// satellite's actor slot comes before the pivot's (it then animates first). The
    /// soft-drop point is dropped here: see [`Pair::step_pivot`].
    pub fn step(
        &mut self,
        field: &Field,
        pad: impl PadSource,
        keys: RotateKeys,
        fall_speed: u16,
        frame_count: u16,
        satellite_first: bool,
    ) -> Step {
        if satellite_first {
            self.step_satellite();
        }
        let (step, _) = self.step_pivot(field, pad, keys, fall_speed, frame_count);
        if !satellite_first {
            self.step_satellite();
        }
        step
    }

    /// The pivot actor's controlled frame ($4F14): animate, move, rotate, fall.
    pub fn step_pivot(
        &mut self,
        field: &Field,
        mut pad: impl PadSource,
        keys: RotateKeys,
        fall_speed: u16,
        frame_count: u16,
    ) -> (Step, PivotEvents) {
        self.pivot_anim.step();
        let mut events = PivotEvents::default();
        self.slide_or_move(field, &mut pad, &mut events);
        self.rotate(field, &mut pad, keys, &mut events);
        let step = self.fall(field, &mut pad, fall_speed, frame_count, &mut events);
        (step, events)
    }

    /// A special piece's controlled frame ($8A22 / $8D2A): move and fall, no turning
    /// (its own animation is the actor's).
    pub fn step_special(&mut self, field: &Field, mut pad: impl PadSource, fall_speed: u16, frame_count: u16) -> (Step, PivotEvents) {
        let anim = self.pivot_anim;
        let mut events = PivotEvents::default();
        self.slide_or_move(field, &mut pad, &mut events);
        let step = self.fall(field, &mut pad, fall_speed, frame_count, &mut events);
        self.pivot_anim = anim;
        (step, events)
    }

    /// The satellite actor's frame ($5482): animate, turn.
    pub fn step_satellite(&mut self) {
        self.sat_anim.step();
        self.animate_satellite();
    }

    /// `PuyoLandEffects` → `sub_5288` / `sub_52AE` when the pair touches down while
    /// still falling: the pivot bounces, a supported satellite squashes. (Assumes normal
    /// bean colors; the special $19/$1A pieces use another script.)
    fn touch_down(&mut self, resting: u8, events: &mut PivotEvents) {
        events.touched_down = true;
        if resting & 1 != 0 {
            self.pivot_anim.ptr = script::BOUNCE;
        }
        if resting & 2 != 0 {
            self.sat_anim.ptr = script::SQUASH;
        }
    }

    /// `sub_5060`.
    fn slide_or_move(&mut self, field: &Field, pad: &mut impl PadSource, events: &mut PivotEvents) {
        if self.slide != 0 {
            let toward_zero = if self.slide & 0x8000 != 0 { 8 } else { 8u16.wrapping_neg() };
            self.slide = self.slide.wrapping_add(toward_zero);
            return;
        }
        if self.locked {
            return;
        }
        let pad = pad.read(self);
        let (dir, dcol, slide): (u8, u16, u16) = if pad.repeat & PAD_LEFT != 0 {
            (3, 0xFFFF, 8)
        } else if pad.repeat & PAD_RIGHT != 0 {
            (1, 1, 8u16.wrapping_neg())
        } else {
            return;
        };
        if self.blocked(field, dir) != 0 {
            return;
        }
        self.col = self.col.wrapping_add(dcol);
        self.slide = slide;
        events.moved = true;
    }

    /// `sub_5384`.
    fn rotate(&mut self, field: &Field, pad: &mut impl PadSource, keys: RotateKeys, events: &mut PivotEvents) {
        if self.locked {
            return;
        }
        let (turn, anim) = match keys.turn(pad.read(self).press) {
            -1 => (3u8, 0xF8u8),
            1 => (1, 0x08),
            _ => return,
        };
        let new = (self.rot + turn) & 3;
        if self.blocked(field, new) & 1 != 0 {
            // The satellite's new spot is taken: push the pivot the other way if it can go.
            if self.blocked(field, new ^ 2) & 1 != 0 {
                return;
            }
            let (dc, dr) = KICK[new as usize];
            self.col = self.col.wrapping_add(dc as u16);
            self.row = self.row.wrapping_add(dr as u16);
            if dr != 0 {
                self.fall = 0x7FFE;
            }
        }
        self.sat_angle = self.rot.rotate_right(2);
        self.rot = new;
        self.sat_turn = anim;
        events.turned = true;
    }

    fn down_only(pad: PadInput) -> bool {
        pad.hold & 0x0E == PAD_DOWN
    }

    /// `sub_50DA`.
    fn fall(&mut self, field: &Field, pad: &mut impl PadSource, fall_speed: u16, frame_count: u16, events: &mut PivotEvents) -> Step {
        if self.locked {
            return self.finish_lock();
        }
        if fall_speed >= 0x8001 {
            return self.fall_fast(field, pad, fall_speed, events);
        }
        let read = pad.read(self);
        // Quirk: reading the pad (sub_56C0) overwrites the low byte of the register
        // holding the fall rate with the auto-repeat byte, so a sideways press nudges it.
        let mut speed = (fall_speed & 0xFF00) | u16::from(read.repeat);
        if Self::down_only(read) {
            speed = 0x8000;
            events.soft_drop_point = frame_count & 1 != 0;
        }
        let falling = Step::Falling;
        let (sum, carry) = self.fall.overflowing_add(speed);
        if carry {
            // Reached the next cell.
            if self.resting(field) == 0 {
                self.fall = 0;
                self.row = self.row.wrapping_add(1);
                return falling;
            }
            return self.lock_delay(pad, falling);
        }
        let crossed_half = (self.fall ^ sum) & 0x8000 != 0;
        self.fall = sum;
        let resting = if crossed_half { self.resting(field) } else { 0 };
        if resting != 0 {
            self.touch_down(resting, events);
            self.contacts += 1;
            if self.contacts >= MAX_CONTACTS {
                return self.lock();
            }
        }
        falling
    }

    /// `loc_5222`: fall rates above $8000 (a cell or more every other frame).
    fn fall_fast(&mut self, field: &Field, pad: &mut impl PadSource, fall_speed: u16, events: &mut PivotEvents) -> Step {
        let falling = Step::Falling;
        let before = self.fall;
        let (sum, carry) = self.fall.overflowing_add(fall_speed);
        self.fall = sum;
        if !carry {
            return falling;
        }
        let resting = self.resting(field);
        if resting == 0 {
            self.row = self.row.wrapping_add(1);
            self.fall &= !1; // andi.b #$FE,$21(a0)
            return falling;
        }
        if before != 0xFFFF {
            self.touch_down(resting, events);
            self.contacts += 1;
            if self.contacts >= MAX_CONTACTS {
                return self.lock();
            }
        }
        self.fall = 0xFFFF;
        self.lock_delay(pad, falling)
    }

    /// `loc_5182`: resting on the stack. Start a 32-frame countdown, lock when it runs
    /// out or the player holds down.
    fn lock_delay(&mut self, pad: &mut impl PadSource, falling: Step) -> Step {
        if self.lock_timer == 0 {
            // sub_51DA reads the high byte of the column word (always 0 here): $20 frames.
            let hi = (self.col >> 8) as u8;
            let hi = if hi & 0x80 != 0 { 0x7F } else { hi };
            self.lock_timer = 0x20 - u16::from(hi >> 3);
            return falling;
        }
        self.lock_timer -= 1;
        if self.lock_timer == 0 || Self::down_only(pad.read(self)) {
            return self.lock();
        }
        falling
    }

    /// `loc_51B4`: stop taking input; land once the slide and turn animations are done.
    fn lock(&mut self) -> Step {
        self.locked = true;
        self.finish_lock()
    }

    fn finish_lock(&mut self) -> Step {
        if self.slide != 0 || self.sat_turn != 0 {
            Step::Falling
        } else {
            Step::Landed
        }
    }

    /// `sub_5530`.
    fn animate_satellite(&mut self) {
        if self.sat_turn == 0 {
            return;
        }
        self.sat_angle = self.sat_angle.wrapping_add(self.sat_turn);
        if self.sat_angle & 0x3F == 0 {
            self.sat_turn = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEYS: RotateKeys = RotateKeys([2, 1, 2]);

    fn spawn() -> Pair {
        Pair { col: 2, row: 1, ..Pair::default() }
    }

    #[test]
    fn falls_one_row_per_overflow() {
        let field = Field::default();
        let mut p = spawn();
        for _ in 0..3 {
            p.step(&field, PadInput::default(), KEYS, 0x4000, 0, false);
        }
        assert_eq!((p.row, p.fall), (1, 0xC000));
        p.step(&field, PadInput::default(), KEYS, 0x4000, 0, false);
        assert_eq!((p.row, p.fall), (2, 0));
    }

    #[test]
    fn rotation_kicks_off_the_wall() {
        let field = Field::default();
        let mut p = Pair { col: 5, row: 5, ..Pair::default() };
        let press_a = PadInput { press: PAD_A, ..PadInput::default() };
        p.step(&field, press_a, KEYS, 0, 0, false); // A = turn right: satellite to col 6 → kick left
        assert_eq!((p.col, p.rot, p.sat_turn), (4, 1, 8));
    }

    #[test]
    fn locks_after_delay_on_the_floor() {
        let field = Field::default();
        let mut p = Pair { row: 13, fall: 0xF000, col: 0, ..Pair::default() };
        let mut frames = 0;
        while p.step(&field, PadInput::default(), KEYS, 0x2000, 0, false) != Step::Landed {
            frames += 1;
            assert!(frames < 100);
        }
        assert!(p.locked);
        assert_eq!(frames, 32);
    }
}
