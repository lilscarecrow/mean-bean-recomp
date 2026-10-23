//! Exercise mode's special pieces (`sub_4E24`): instead of a pair, a single piece
//! the player moves and drops like one.
//!
//! - $19 (`loc_89D8`): the referee bean itself jumps into the field for it
//!   (`loc_5D0C`: a helper throws it off the top, `word_FF19A8` (`referee_hit`) the
//!   handshake). Landed, it sinks
//!   into the bean below and wanders off sideways (at random) or down through the
//!   beans, painting each in the color of the first (garbage counts as red), then
//!   leaves; at the bottom of an empty column it's worth 10000 points.
//! - $1A (`loc_8D18`): two columns wide; landed, it bores down through both columns,
//!   each bean bursting into debris for (level + 1) * 10 points, then bounces off
//!   with the field shaking (12000 points if it fell to the bottom unimpeded).

use super::*;
use crate::pair::{PadInput, Pair};
use crate::rom;

/// Scripts: `unk_89AE` (falling), `unk_89BC` / `unk_89CA` (wandering left / right),
/// `unk_8B56` (leaving), `unk_8BDC` / `unk_8BD0` (out the bottom, P1 / P2),
/// `unk_8D16`, `unk_8E92` (the $1A piece's bounce), `unk_8F84` (debris), `unk_5E7C` /
/// `unk_5E6E` (the referee thrown and coming back).
const BEAN_FALL: u32 = 0x89AE;
const BEAN_LEAVE: u32 = 0x8B56;
const BEAN_OUT: [u32; 2] = [0x8BDC, 0x8BD0];
const DRILL_STILL: u32 = 0x8D16;
const DRILL_BOUNCE: u32 = 0x8E92;
const DEBRIS_SCRIPT: u32 = 0x8F84;
const THROW_SCRIPT: u32 = 0x5E7C;
const CATCH_SCRIPT: u32 = 0x5E6E;
/// `unk_8CAE`: the wandering steps (column, x step, y step, cell offset, script).
const WANDER: u32 = 0x8CAE;
/// `unk_902A`: the field shake's strength per column, by the piece's column.
const SHAKE: u32 = 0x902A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpecialState {
    /// Just dealt ($89D8 / $8D18).
    Spawn,
    /// $19: waiting for the referee to be thrown ($8A06), $10 frames ($8A1A), one more
    /// ($8A1E).
    WaitReferee,
    Delay,
    Pause,
    /// Moving and falling under control ($8A22 / $8D2A).
    Control,
    /// $4F4E: the player's game ended under it.
    Hidden,
    /// $19: sinking into / wandering to the next bean ($8AA8), leaving ($8B48), out of
    /// the bottom ($8BA6).
    Step,
    Leave,
    Out,
    /// $1A: its landing bounce ($8D6E), boring down ($8DFE), bouncing off ($8E70).
    Landed,
    Bore,
    Bounce,
}

/// A special piece; its fields as the original keeps them (+$06 draw, +$07 flags,
/// +$08 mappings, +$0A/+$0E x/y, +$12/+$16 velocity, +$1A..+$20 column, row, slide,
/// fall (steering once it moves freely), +$26, +$28, +$2B rotation, +$36).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Special {
    pub state: SpecialState,
    pub player: usize,
    pub id: u8,
    pub draw: u8,
    pub flags: u8,
    pub mappings: u8,
    pub anim: Anim,
    pub motion: Motion,
    pub w26: u16,
    pub w28: u16,
    pub rot: u8,
    pub w36: u16,
    /// +$24 (`ActorBookmark_SetDelay`).
    pub delay: u16,
}

impl Special {
    pub fn code(&self) -> u32 {
        use SpecialState::*;
        let bean = self.mappings == 0x19;
        match self.state {
            Spawn => if bean { 0x89D8 } else { 0x8D18 },
            WaitReferee => 0x8A06,
            Delay => 0x8A1A,
            Pause => 0x8A1E,
            Control => if bean { 0x8A22 } else { 0x8D2A },
            Hidden => 0x4F4E,
            Step => 0x8AA8,
            Leave => 0x8B48,
            Out => 0x8BA6,
            Landed => 0x8D6E,
            Bore => 0x8DFE,
            Bounce => 0x8E70,
        }
    }

    fn x(&self) -> u16 {
        (self.motion.x >> 16) as u16
    }

    fn y(&self) -> u16 {
        (self.motion.y >> 16) as u16
    }

    fn set_x(&mut self, x: u16) {
        self.motion.x = u32::from(x) << 16 | (self.motion.x & 0xFFFF);
    }

    fn set_y(&mut self, y: u16) {
        self.motion.y = u32::from(y) << 16 | (self.motion.y & 0xFFFF);
    }

    /// Column (+$1A), row (+$1C), slide (+$1E), fall (+$20).
    fn pair(&self) -> Pair {
        Pair {
            col: self.motion.ax,
            row: self.motion.ay,
            rot: self.rot,
            fall: self.motion.ty,
            slide: self.motion.tx,
            contacts: self.w26,
            lock_timer: self.w28,
            locked: self.flags & 8 != 0,
            sat_angle: 0,
            sat_turn: 0,
            pivot_anim: self.anim,
            sat_anim: Anim::default(),
        }
    }

    fn set_pair(&mut self, p: &Pair) {
        self.motion.ax = p.col;
        self.motion.ay = p.row;
        self.motion.ty = p.fall;
        self.motion.tx = p.slide;
        self.w26 = p.contacts;
        self.w28 = p.lock_timer;
        self.flags = if p.locked { self.flags | 8 } else { self.flags & !8 };
    }

    /// `sub_543C`.
    fn place(&mut self) {
        let side = usize::from(self.id & 1);
        let x = FIELD_X[side].wrapping_add(self.motion.ax.wrapping_mul(16)).wrapping_add(self.motion.tx).wrapping_add(8);
        let half = if self.motion.ty & 0x8000 != 0 { 8 } else { 0 };
        let y = FIELD_TOP_Y.wrapping_add(self.motion.ay.wrapping_sub(2).wrapping_mul(16)).wrapping_add(half);
        self.set_x(x);
        self.set_y(y);
    }
}

/// `loc_8F92`: a burst bean's debris (+$26 frames on this course, +$28 courses left).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Debris {
    /// (its piece's side: the player's tag)
    pub side: u8,
    pub color: u8,
    pub motion: Motion,
    pub timer: u16,
    pub courses: u16,
    pub anim: Anim,
}

pub const DEBRIS_CODE: u32 = 0x8F92;

/// `loc_9066`: one column of the field shaking (+$36 phase, +$38 strength, +$26 its
/// vertical scroll entry, in bytes).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldShake {
    pub side: u8,
    pub phase: u8,
    pub strength: u16,
    pub at: u16,
}

pub const FIELD_SHAKE_CODE: u32 = 0x9066;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowState {
    /// $5D64, $5D70: the referee's wind-up; $5DC2: flying off; $5DE6: away; $5E1A:
    /// coming back.
    Start,
    WindUp,
    Fly,
    Away,
    Return,
}

/// `loc_5D64`: throws the referee off the top of the screen for a $19 piece, and
/// brings it back (+$36 / +$38 its seat).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefereeThrow {
    pub state: ThrowState,
    pub referee: usize,
    pub draw: u8,
    pub motion: Motion,
    pub anim: Anim,
    pub seat: (u16, u16),
}

impl RefereeThrow {
    pub fn code(&self) -> u32 {
        match self.state {
            ThrowState::Start => 0x5D64,
            ThrowState::WindUp => 0x5D70,
            ThrowState::Fly => 0x5DC2,
            ThrowState::Away => 0x5DE6,
            ThrowState::Return => 0x5E1A,
        }
    }
}

impl Engine {
    /// `sub_4E24`'s special pieces: $19 in front, or $1A behind.
    pub(super) fn spawn_special(&mut self, player_slot: usize, p: &mut Player, kind: u8) -> Result<(), Unported> {
        let slot = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
        let bean = kind == 0x19;
        let motion = Motion { ax: 2, ay: 1, ..Motion::default() };
        let anim = if bean { Anim { ptr: BEAN_FALL, time: 0, frame: 0x45 } } else { Anim { ptr: DRILL_STILL, time: 0, frame: 3 } };
        let s = Special {
            state: SpecialState::Spawn,
            player: player_slot,
            id: p.id,
            draw: 0,
            flags: 0,
            mappings: kind,
            anim,
            motion,
            w26: 0,
            w28: 0,
            rot: u8::from(!bean),
            w36: 0,
            delay: 0,
        };
        self.slots[slot] = Some(Actor::Special(s));
        p.flags |= flag::PIVOT_ACTIVE;
        Ok(())
    }

    fn special_parent_flags(&self, s: &Special) -> u8 {
        match &self.slots[s.player] {
            Some(Actor::Player(p)) => p.flags,
            _ => 0,
        }
    }

    /// The parent's +$07 bit 0 off: the piece is done.
    fn special_done(&mut self, s: &Special) {
        if let Some(Actor::Player(p)) = &mut self.slots[s.player] {
            p.flags &= !flag::PIVOT_ACTIVE;
        }
    }

    /// `sub_894A`: points for the player, and its score redrawn.
    fn special_points(&mut self, s: &Special, points: u32) {
        let id = s.id;
        if let Some(Actor::Player(p)) = &mut self.slots[s.player] {
            add_points(p, points);
        }
        self.queue_panel(PanelCommand::Score(id));
    }

    /// `sub_5060` + `sub_50DA` under the player's control. True when it locks.
    fn special_fall(&mut self, s: &mut Special, inputs: &Inputs) -> bool {
        let id = usize::from(s.id & 1);
        let fall_speed = match &self.slots[s.player] {
            Some(Actor::Player(p)) => p.fall_speed,
            _ => 0,
        };
        let pad: PadInput = inputs.pads[id];
        let mut pair = s.pair();
        let (step, events) = pair.step_special(&self.fields[id], pad, fall_speed, self.frame_count);
        s.set_pair(&pair);
        self.pair_sounds(id as u8, &events);
        if events.touched_down {
            if s.mappings == 0x1A {
                s.anim.ptr = DRILL_BOUNCE; // sub_5288 / sub_52AE
            }
        }
        if events.soft_drop_point {
            if let Some(Actor::Player(p)) = &mut self.slots[s.player] {
                p.flags |= flag::SOFT_DROP_POINT;
            }
        }
        step == crate::pair::Step::Landed
    }

    /// A cell's byte (+0 of its word) by byte offset into the field.
    fn cell_byte(&self, id: usize, at: u16) -> u8 {
        self.field_bytes[id].cells.get(usize::from(at / 2)).copied().unwrap_or(0)
    }

    /// Write a cell's byte (and the field's color model to match).
    fn set_cell_byte(&mut self, id: usize, at: u16, b: u8, drawn: Option<u8>) {
        let i = usize::from(at / 2);
        if i >= COLS * ROWS {
            return;
        }
        self.field_bytes[id].cells[i] = b;
        if let Some(d) = drawn {
            self.field_bytes[id].drawn[i] = d;
        }
        let color = (b & 0x80 != 0).then(|| BeanColor::from_id(b >> 4 & 7)).flatten();
        self.fields[id].set(i % COLS, i / COLS, color);
    }

    pub(super) fn run_special(&mut self, mut s: Special, inputs: &Inputs) -> Option<Special> {
        use SpecialState::*;
        let id = usize::from(s.id & 1);
        let mut random = || 0;
        match s.state {
            Spawn => {
                s.place();
                s.draw = 0x80;
                if s.mappings == 0x1A {
                    s.set_x(s.x().wrapping_add(8));
                    s.state = Control;
                } else if self.referee_hit >> 8 == 0 {
                    self.referee_hit = u16::from(s.id | 0x80) << 8 | 0xFF;
                    s.state = WaitReferee;
                } else {
                    s.state = Control;
                }
            }
            WaitReferee => {
                if self.referee_hit & 0xFF == 0 {
                    s.delay = 0x10;
                    s.state = Delay;
                }
            }
            Delay => {
                if wait(&mut s.delay) {
                    s.state = Pause;
                }
            }
            Pause => s.state = Control,
            Hidden => {}
            Control => {
                if self.special_parent_flags(&s) & 1 == 0 {
                    // loc_4F3A
                    s.draw = 0;
                    s.state = Hidden;
                    return Some(s);
                }
                s.anim.step_with(&mut random);
                let bean = s.mappings == 0x19;
                if s.motion.ay < 3 {
                    if bean {
                        s.anim.frame = 0x45;
                    } else if s.anim.frame < 3 {
                        s.anim.frame += 3;
                    }
                }
                if !self.special_fall(&mut s, inputs) {
                    s.place();
                    if !bean {
                        s.set_x(s.x().wrapping_add(8));
                    }
                } else if bean {
                    self.bean_landed(&mut s);
                } else {
                    s.state = Landed;
                }
            }
            Step => {
                if s.w28 & 3 == 0 {
                    self.play_sample(0x4B);
                }
                s.anim.step_with(&mut random);
                s.set_x(s.x().wrapping_add((s.motion.vx >> 16) as u16));
                s.set_y(s.y().wrapping_add((s.motion.vy >> 16) as u16));
                s.w28 = s.w28.wrapping_sub(1);
                if s.w28 == 0 {
                    // loc_8ADA: the bean there takes the color.
                    let b = (s.w36 >> 8) as u8;
                    self.set_cell_byte(id, s.w26, b, None);
                    self.redraw_field(id);
                    if !self.bean_wander(&mut s) {
                        self.special_done(&s);
                        s.draw = 0xAF;
                        s.anim.ptr = BEAN_LEAVE;
                        s.motion.vx = if s.id == 0 { 3 << 16 } else { 0xFFFE << 16 } | (s.motion.vx & 0xFFFF);
                        s.motion.vy &= 0xFFFF;
                        s.motion.ax = 0x1A00;
                        s.motion.ay = 0x0800;
                        s.motion.tx = s.x();
                        s.motion.ty = 0;
                        s.state = Leave;
                    }
                }
            }
            Leave => {
                s.anim.step_with(&mut random);
                if s.motion.step(s.draw) {
                    return self.bean_gone(&s);
                }
            }
            Out => {
                s.anim.step_with(&mut random);
                if s.anim.frame >= 0x43 {
                    s.set_x(s.x().wrapping_add((s.motion.vx >> 16) as u16));
                    if s.x().wrapping_sub(0x78) >= 0x150 {
                        return self.bean_gone(&s);
                    }
                }
            }
            Landed => {
                if s.anim.step_with(&mut random) {
                    self.drill_landed(&mut s);
                } else if s.motion.ay < 3 && s.anim.frame < 3 {
                    s.anim.frame += 3;
                }
            }
            Bore => {
                s.motion.step(s.draw);
                if (s.motion.vy >> 16) as u16 >= 6 {
                    s.motion.vy = 6 << 16;
                }
                self.drill_bore(&mut s);
                if s.y() >= 0x148 {
                    s.set_y(0x148);
                    s.anim.ptr = DRILL_BOUNCE;
                    s.w26 = 0x80;
                    self.play_sample(0x49);
                    self.drill_bounce(&mut s);
                }
            }
            Bounce => {
                s.anim.step_with(&mut random);
                s.motion.step(s.draw);
                if s.y() >= 0x170 {
                    self.special_done(&s);
                    return None;
                }
            }
        }
        Some(s)
    }

    /// `loc_8A54`: the $19 piece settles one row lower, into the bean there (or out of
    /// the bottom).
    fn bean_landed(&mut self, s: &mut Special) {
        self.play_land_sound();
        let id = usize::from(s.id & 1);
        s.motion.ay = s.motion.ay.wrapping_add(1);
        if s.motion.ay >= ROWS as u16 {
            // loc_8B64
            s.draw = 0x82;
            s.motion.vx = (if s.id == 0 { 0xFFFF } else { 1 } << 16) | (s.motion.vx & 0xFFFF);
            s.anim.ptr = BEAN_OUT[usize::from(s.id != 0)];
            self.special_done(s);
            self.special_points(s, 10000);
            s.state = SpecialState::Out;
            return;
        }
        s.w26 = (s.motion.ay * COLS as u16 + s.motion.ax) * 2;
        let mut b = self.cell_byte(id, s.w26) & 0xF0;
        if b >= 0xE0 {
            b = 0x80;
        }
        s.w36 = u16::from(b) << 8 | (s.w36 & 0xFF);
        s.motion.vy = 1 << 16 | (s.motion.vy & 0xFFFF);
        s.w28 = 0x10;
        s.state = SpecialState::Step;
    }

    /// `sub_8C06`: wander left or right (at random) onto a bean of another color, or
    /// else down a row; false at the bottom.
    fn bean_wander(&mut self, s: &mut Special) -> bool {
        let id = usize::from(s.id & 1);
        s.flags ^= 0x80;
        let way = u32::from(self.random() & 1);
        // sub_8C5C
        let at = WANDER + way * 10;
        let col = s.motion.ax.wrapping_add(rom::word(at));
        let cell = s.w26.wrapping_add(rom::word(at + 4));
        let b = self.cell_byte(id, cell);
        let moved = col < 6 && b & 0x80 != 0 && u16::from(b & 0xF0) != s.w36 >> 8;
        if moved {
            s.motion.ax = col;
            s.w26 = cell;
            s.motion.vx = u32::from(rom::word(at)) << 16 | (s.motion.vx & 0xFFFF);
            s.motion.vy = u32::from(rom::word(at + 2)) << 16 | (s.motion.vy & 0xFFFF);
            s.anim.ptr = rom::long(at + 6);
        } else {
            // loc_8C28
            if s.motion.ay >= 0x0D {
                return false;
            }
            s.motion.ay += 1;
            s.w26 = s.w26.wrapping_add(0x0C);
            s.motion.vx &= 0xFFFF;
            s.motion.vy = 1 << 16 | (s.motion.vy & 0xFFFF);
            s.anim.ptr = BEAN_FALL;
        }
        s.w28 = 0x10;
        s.state = SpecialState::Step;
        true
    }

    /// `loc_8BE8`: gone; the referee may come back.
    fn bean_gone(&mut self, s: &Special) -> Option<Special> {
        if (self.referee_hit >> 8) as u8 == s.id | 0x80 {
            self.referee_hit |= 0xFF;
        }
        None
    }

    /// `loc_8D90`: the $1A piece starts boring (or at the bottom already, bounces off).
    fn drill_landed(&mut self, s: &mut Special) {
        let id = usize::from(s.id & 1);
        let col = s.motion.ax * 2;
        for at in [col, col + 2, col + 0x0C, col + 0x0E] {
            self.set_cell_byte(id, at, 0, Some(0));
        }
        s.anim.frame = 0;
        if s.motion.ay >= ROWS as u16 - 1 {
            self.special_points(s, 12000);
            self.drill_bounce(s);
            return;
        }
        s.w26 = ((s.motion.ay + 1) * COLS as u16 + s.motion.ax) * 2;
        s.w36 = s.y();
        s.motion.tx = 0;
        s.draw = 0x85;
        s.motion.ay = 0x1A00;
        s.motion.ty = 0xFFFF;
        s.state = SpecialState::Bore;
    }

    /// `sub_8EA4`: every 16 pixels down, the two beans there burst.
    fn drill_bore(&mut self, s: &mut Special) {
        let id = usize::from(s.id & 1);
        let rows = s.y().wrapping_sub(s.w36) >> 4;
        if rows == s.motion.tx {
            return;
        }
        s.motion.tx = rows;
        // sub_8EF6
        let level = match &self.slots[s.player] {
            Some(Actor::Player(p)) => p.speed_level,
            _ => 0,
        };
        let worth = (u32::from(level) + 1) * 10;
        let mut points = 0;
        for (i, at) in [s.w26, s.w26.wrapping_add(2)].into_iter().enumerate() {
            let b = self.cell_byte(id, at);
            let color = b >> 4 & 7;
            if b & 0x80 == 0 || color == 6 {
                continue;
            }
            for _ in 0..2 {
                let Some(slot) = self.free_slot() else { continue };
                let motion = Motion { x: u32::from(s.x() + (i as u16) * 16) << 16, y: u32::from(s.y()) << 16, ..Motion::default() };
                self.slots[slot] = Some(Actor::Debris(Debris { side: s.id & 1, color, motion, timer: 0, courses: 8, anim: Anim { ptr: DEBRIS_SCRIPT, time: 0, frame: 4 } }));
            }
            points += worth;
        }
        self.special_points(s, points);
        self.set_cell_byte(id, s.w26, 0, Some(0xFF));
        self.set_cell_byte(id, s.w26.wrapping_add(2), 0, Some(0xFF));
        s.w26 = s.w26.wrapping_add(0x0C);
        self.redraw_field(id);
        self.play_sample(0x4A);
    }

    /// `loc_8E40`: the field shakes; the piece bounces off.
    fn drill_bounce(&mut self, s: &mut Special) {
        let id = usize::from(s.id & 1);
        // sub_8FDA
        let mut at = FIELD_ORIGIN_SCROLL[id];
        let strength = ((s.motion.vy >> 16) as u16).wrapping_mul(2).wrapping_add(4);
        for col in 0..6u32 {
            if let Some(slot) = self.find_actor_slot() {
                let w = rom::word(SHAKE + u32::from(s.motion.ax) * 12 + col * 2);
                self.slots[slot] = Some(Actor::FieldShake(FieldShake { side: s.id & 1, phase: 0x80, strength: w.wrapping_mul(strength), at }));
            }
            at += 4;
        }
        s.draw = 0xB7;
        s.motion.vy = s.motion.vy.wrapping_neg();
        s.motion.ay = 0x3400;
        s.motion.ty = 0xFFFF;
        s.motion.vx = (s.motion.vx & 0xFFFF_0000) | 0x8000;
        if s.id == 0 {
            s.motion.vx = s.motion.vx.wrapping_neg();
        }
        s.state = SpecialState::Bounce;
    }

    pub(super) fn run_debris(&mut self, mut d: Debris) -> Option<Debris> {
        if d.timer == 0 {
            if d.courses == 0 {
                return None;
            }
            d.courses -= 1;
            d.timer = (d.courses + 1) * 2;
            let angle = self.random() as u8;
            d.motion.vx = super::sin(angle, 0x200);
            d.motion.vy = super::cos(angle, 0x200);
        }
        d.timer -= 1;
        if d.motion.step(0x83) {
            return None;
        }
        d.anim.step_with(&mut || 0);
        Some(d)
    }

    pub(super) fn run_field_shake(&mut self, mut f: FieldShake) -> Option<FieldShake> {
        let v = (super::sin(f.phase | 0x80, f.strength) >> 16) as u16;
        if let Some(w) = self.vscroll.get_mut(usize::from(f.at / 2)) {
            *w = v;
        }
        let (phase, borrow) = f.phase.overflowing_sub(4);
        f.phase = phase;
        (!borrow).then_some(f)
    }

    /// `loc_5D0C` (from the referee's idle loop, a $19 piece wanting its seat): the
    /// helper that throws it. False when there's no slot for one.
    pub(super) fn throw_referee(&mut self, referee: usize) -> bool {
        let Some(slot) = self.find_actor_slot() else {
            self.referee_hit = 0;
            return false;
        };
        let t = RefereeThrow { state: ThrowState::Start, referee, draw: 0, motion: Motion::default(), anim: Anim::new(0), seat: (0, 0) };
        self.slots[slot] = Some(Actor::RefereeThrow(t));
        true
    }

    pub(super) fn run_referee_throw(&mut self, mut t: RefereeThrow) -> Option<RefereeThrow> {
        let mut random = || 0;
        let referee = match &self.slots[t.referee] {
            Some(Actor::Referee(r)) => Some((r.x, r.y)),
            _ => None,
        };
        let set = |e: &mut Engine, x: Option<u16>, y: Option<u16>, frame: Option<u8>| {
            if let Some(Actor::Referee(r)) = &mut e.slots[t.referee] {
                if let Some(x) = x {
                    r.x = x;
                }
                if let Some(y) = y {
                    r.y = y;
                }
                if let Some(f) = frame {
                    r.anim.frame = f;
                }
            }
        };
        let hi = |v: u32| (v >> 16) as u16;
        match t.state {
            ThrowState::Start => {
                t.anim = Anim::new(THROW_SCRIPT);
                t.state = ThrowState::WindUp;
            }
            ThrowState::WindUp => {
                if !t.anim.step_with(&mut random) {
                    set(self, None, None, Some(t.anim.frame));
                    return Some(t);
                }
                // loc_5D84
                let (x, y) = referee.unwrap_or_default();
                t.draw = 5;
                t.seat = (x, y);
                t.motion.x = u32::from(x) << 16 | (t.motion.x & 0xFFFF);
                t.motion.y = u32::from(y) << 16 | (t.motion.y & 0xFFFF);
                t.motion.vy = 0xFFFF_0000 | (t.motion.vy & 0xFFFF);
                t.motion.ax = 0x0A00;
                t.motion.ay = 0x1800;
                self.play_sample(0);
                t.state = ThrowState::Fly;
            }
            ThrowState::Fly => {
                if t.motion.step(t.draw) {
                    // loc_5DDC
                    self.referee_hit &= 0xFF00;
                    t.state = ThrowState::Away;
                } else {
                    set(self, Some(hi(t.motion.x)), Some(hi(t.motion.y)), None);
                }
            }
            ThrowState::Away => {
                if self.referee_hit & 0xFF != 0 {
                    // loc_5DF2
                    t.draw = 5;
                    t.motion.x = u32::from(t.seat.0) << 16 | (t.motion.x & 0xFFFF);
                    t.motion.vy = 0;
                    t.motion.ty = 0xFFFF;
                    t.motion.ay = 0x1800;
                    t.anim.ptr = CATCH_SCRIPT;
                    t.state = ThrowState::Return;
                }
            }
            ThrowState::Return => {
                t.motion.step(t.draw);
                t.anim.step_with(&mut random);
                let y = hi(t.motion.y);
                set(self, Some(hi(t.motion.x)), Some(y), Some(t.anim.frame));
                if y >= t.seat.1 {
                    // loc_5E44
                    set(self, Some(t.seat.0), Some(t.seat.1), Some(0x0D));
                    if let Some(Actor::Referee(r)) = &mut self.slots[t.referee] {
                        r.last_frame = 0xFF;
                    }
                    self.referee_hit = 0;
                    self.play_sample(0x45);
                    return None;
                }
            }
        }
        Some(t)
    }
}

/// `GetPuyoField`'s offset & $7F: each field's first byte in `vscroll_buffer`.
const FIELD_ORIGIN_SCROLL: [u16; 2] = [crate::assets::planes::FIELD_ORIGIN[0] & 0x7F, crate::assets::planes::FIELD_ORIGIN[1] & 0x7F];
