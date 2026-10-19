//! The original's actor system, as far as one player's pieces need it.
//!
//! The game runs up to 64 actors every frame in slot order (`RunActors`). Each actor is a
//! coroutine: it runs until it calls `ActorBookmark` (yield until next frame, optionally
//! after a delay) and resumes at the instruction after it. Every state here is named by
//! that resume address (`code()`), so it can be compared with the game's RAM.
//!
//! Ported so far: the player's field-controller loop between pieces (wait for the pair,
//! find pops, redraw, delay, spawn the next pair), the pair's pivot and satellite
//! (controlled, landing, physics fall, squash, write to the field) and the little
//! post-landing effect. Pops, garbage and game over stop the engine with `Unported`.

use super::anim::{script, Anim};
use super::bean_order::BeanColor;
use super::field::{Field, DIR};
use super::pair::{PadInput, Pair, RotateKeys, Step};

pub const SLOTS: usize = 64;
/// Screen y of a field's top visible row (`word_5B34`), same for both players.
pub const FIELD_TOP_Y: u16 = 0x90;
/// Frames to wait after a piece settles, by the player's +$2B (`word_3B58`).
const SETTLE_DELAY: [u16; 5] = [12, 8, 4, 2, 0];
/// Physics fall (`sub_4948`): start speed 1 px/frame, + $3000/65536 per frame, max 8.
const FALL_START_VEL: u32 = 0x1_0000;
const FALL_ACCEL: u16 = 0x3000;
const FALL_MAX_VEL: u32 = 0x8_0000;

/// Player flags (+$07 of the player actor).
pub mod flag {
    pub const PIVOT_ACTIVE: u8 = 0x01;
    pub const SATELLITE_ACTIVE: u8 = 0x02;
    pub const GARBAGE_FALLING: u8 = 0x04;
    pub const SOFT_DROP_POINT: u8 = 0x08;
}

/// Something the port can't run yet; the engine stops instead of guessing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unported(pub &'static str);

#[derive(Clone, Debug, PartialEq)]
pub enum Actor {
    /// A slot used by an actor this port doesn't model (keeps slot allocation right).
    Opaque,
    Player(Player),
    Pivot(Pivot),
    Satellite(Satellite),
    Effect(Effect),
}

impl Actor {
    /// The original resume address of this actor's current state (None for `Opaque`).
    pub fn code(&self) -> Option<u32> {
        match self {
            Actor::Opaque => None,
            Actor::Player(p) => Some(p.state.code()),
            Actor::Pivot(p) => Some(p.state.code()),
            Actor::Satellite(s) => Some(s.state.code()),
            Actor::Effect(e) => Some(e.state.code()),
        }
    }
}

// ---------------------------------------------------------------- player

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    /// $3E6E: pair in play; count frames, wait for both beans to be written.
    WaitPair,
    /// $3E9A: look for pops (`sub_5960`).
    FindPops,
    /// $3EA6: redraw links (`sub_5782`).
    Redraw,
    /// $3EB6: pops → chain; otherwise start the settle delay.
    Decide,
    /// $3EC6: settle delay (`ActorBookmark` with `aDelay`).
    SettleDelay,
    /// $3ECA: send garbage if a chain happened, then drop incoming garbage.
    AfterDelay,
    /// $3DD2: wait while garbage falls.
    WaitGarbage,
    /// $3DF6, $3E08, $3E1A: bookkeeping frames before the next pair.
    PreSpawn1,
    PreSpawn2,
    PreSpawn3,
    /// $3E44: spawn the next pair (`sub_4E24`).
    Spawn,
}

impl PlayerState {
    pub fn code(self) -> u32 {
        match self {
            PlayerState::WaitPair => 0x3E6E,
            PlayerState::FindPops => 0x3E9A,
            PlayerState::Redraw => 0x3EA6,
            PlayerState::Decide => 0x3EB6,
            PlayerState::SettleDelay => 0x3EC6,
            PlayerState::AfterDelay => 0x3ECA,
            PlayerState::WaitGarbage => 0x3DD2,
            PlayerState::PreSpawn1 => 0x3DF6,
            PlayerState::PreSpawn2 => 0x3E08,
            PlayerState::PreSpawn3 => 0x3E1A,
            PlayerState::Spawn => 0x3E44,
        }
    }
}

/// A player's field controller (tag $F1/$F2).
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub id: u8,
    pub state: PlayerState,
    pub flags: u8,
    /// +$26 word: frame counter (high byte) while a pair is in play, pop count after.
    pub w26: u16,
    /// aDelay (+$24).
    pub delay: u16,
    /// Chain step (+$09).
    pub chain: u8,
    /// Fall rate (+$1A).
    pub fall_speed: u16,
    /// +$2B: indexes the settle delay.
    pub speed_level: u8,
    /// Points earned by soft drop (added to the score by `sub_9C4A`).
    pub soft_drop_points: u32,
    /// Pending garbage (+$14), only checked here: dropping it isn't ported yet.
    pub pending_garbage: u16,
}

// ---------------------------------------------------------------- pair

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PivotState {
    /// $4F06: just spawned.
    Init,
    /// $4F14: under the player's control.
    Controlled,
    /// $4F8A: dropping further on its own (`sub_4948`).
    Falling,
    /// $4FA8: landing animation, then written to the field.
    Squash,
}

impl PivotState {
    pub fn code(self) -> u32 {
        match self {
            PivotState::Init => 0x4F06,
            PivotState::Controlled => 0x4F14,
            PivotState::Falling => 0x4F8A,
            PivotState::Squash => 0x4FA8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatelliteState {
    /// $5474: just spawned.
    Init,
    /// $5482: following the pivot.
    Follow,
    /// $54D4: the pivot landed; supported or not?
    Check,
    /// $54FA: dropping further on its own.
    Falling,
    /// $5514: landing animation, then written to the field.
    Squash,
}

impl SatelliteState {
    pub fn code(self) -> u32 {
        match self {
            SatelliteState::Init => 0x5474,
            SatelliteState::Follow => 0x5482,
            SatelliteState::Check => 0x54D4,
            SatelliteState::Falling => 0x54FA,
            SatelliteState::Squash => 0x5514,
        }
    }
}

/// A bean dropping under gravity (`sub_4948`): y and speed are 16.16 pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fall {
    pub y: u32,
    pub vel: u32,
    /// +$20 word: y where the fall started, minus 15.
    pub base: u16,
}

impl Fall {
    fn start(y: u32) -> Fall {
        Fall { y, vel: FALL_START_VEL, base: ((y >> 16) as u16).wrapping_sub(15) }
    }

    /// One frame. Returns true once the bean rests (y snapped to 8 px). `row` advances
    /// each time the bean crosses into the next cell.
    fn step(&mut self, field: &Field, col: u16, row: &mut u16) -> bool {
        let next = self.y.wrapping_add(self.vel);
        let before = ((self.y >> 16) as u16).wrapping_sub(self.base);
        let after = ((next >> 16) as u16).wrapping_sub(self.base);
        if (before ^ after) & 0x10 != 0 {
            let (dc, dr) = DIR[2];
            if field.blocked(col as i16 + dc, *row as i16 + dr) {
                // Only the pixel word is rewritten: the fraction stays.
                let pixel = ((next >> 16) as u16) & !7;
                self.y = (u32::from(pixel) << 16) | (self.y & 0xFFFF);
                return true;
            }
            *row = row.wrapping_add(1);
        }
        self.y = next;
        self.vel = self.vel.wrapping_add(u32::from(FALL_ACCEL)).min(FALL_MAX_VEL);
        false
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pivot {
    pub player_slot: usize,
    pub satellite_slot: usize,
    pub color: BeanColor,
    pub state: PivotState,
    /// Controlled-pair state (also carries the satellite's turn and animation).
    pub pair: Pair,
    /// Own +$07 after landing: bit 0 pivot supported, bit 1 satellite, bit 2 landed.
    pub flags: u8,
    /// Screen y (16.16), recomputed from row/fall while controlled (`sub_543C`).
    pub y: u32,
    pub fall: Fall,
}

impl Pivot {
    /// `sub_543C` (y only): top of field + (row-2)*16, +8 in the lower half of a cell.
    fn place(&mut self) {
        let half = if self.pair.fall & 0x8000 != 0 { 8 } else { 0 };
        let y = FIELD_TOP_Y.wrapping_add(self.pair.row.wrapping_sub(2).wrapping_mul(16)).wrapping_add(half);
        self.y = (u32::from(y) << 16) | (self.y & 0xFFFF);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Satellite {
    pub pivot_slot: usize,
    pub player_slot: usize,
    pub color: BeanColor,
    pub state: SatelliteState,
    /// After the pivot lands: own column/row, copied flags, animation, fall.
    pub col: u16,
    pub row: u16,
    pub flags: u8,
    pub y: u32,
    pub anim: Anim,
    pub fall: Fall,
}

// ---------------------------------------------------------------- effect

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectState {
    /// $4FCE: wait 24 frames.
    Wait,
    /// $4FE6: start rising.
    Start,
    /// $4FF4: rise 1 px every 4 frames, 6 times, then vanish.
    Rise,
}

impl EffectState {
    pub fn code(self) -> u32 {
        match self {
            EffectState::Wait => 0x4FCE,
            EffectState::Start => 0x4FE6,
            EffectState::Rise => 0x4FF4,
        }
    }
}

/// What's left of a pair bean after it is written into the field (`loc_4FC0`).
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub player: u8,
    pub state: EffectState,
    pub delay: u16,
    pub steps_left: u8,
    pub ticks: u8,
    pub y: u32,
}

impl Effect {
    fn new(player: u8, y: u32) -> Effect {
        Effect { player, state: EffectState::Wait, delay: 0x18, steps_left: 0, ticks: 0, y }
    }
}

// ---------------------------------------------------------------- engine

/// What the engine needs from outside each frame.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub pads: [PadInput; 2],
    pub keys: [RotateKeys; 2],
    pub frame_count: u16,
    /// The next pair each player will get (pivot, satellite): bean order isn't wired in yet.
    pub next_pair: [Option<(BeanColor, BeanColor)>; 2],
    /// Slots the next spawned pivot/satellite get, when the caller wants to force them
    /// (slots of actors this port doesn't model can't be predicted yet).
    pub spawn_slots: [Option<(usize, usize)>; 2],
}

impl Default for RotateKeys {
    fn default() -> Self {
        RotateKeys([2, 1, 2])
    }
}

/// `ActorBookmark` with a delay: true when the wait is over.
fn wait(delay: &mut u16) -> bool {
    if *delay == 0 {
        return true;
    }
    *delay -= 1;
    *delay == 0
}

#[derive(Clone)]
pub struct Engine {
    pub slots: Vec<Option<Actor>>,
    pub fields: [Field; 2],
    /// `puyos_popping` per player (MarkPuyoSpot does nothing while set).
    pub popping: [bool; 2],
}

impl Engine {
    pub fn new() -> Engine {
        Engine { slots: vec![None; SLOTS], fields: [Field::default(), Field::default()], popping: [false; 2] }
    }

    /// `FindActorSlot`: the first free slot.
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    fn player_mut(&mut self, slot: usize) -> &mut Player {
        match self.slots[slot].as_mut() {
            Some(Actor::Player(p)) => p,
            other => panic!("slot {slot} is not a player: {other:?}"),
        }
    }

    fn pivot_mut(&mut self, slot: usize) -> &mut Pivot {
        match self.slots[slot].as_mut() {
            Some(Actor::Pivot(p)) => p,
            other => panic!("slot {slot} is not a pivot: {other:?}"),
        }
    }

    /// One frame: every actor in slot order (`RunActors`).
    pub fn frame(&mut self, inputs: &Inputs) -> Result<(), Unported> {
        for slot in 0..SLOTS {
            // Leave a placeholder so the running actor's slot still counts as used.
            let Some(actor) = self.slots[slot].replace(Actor::Opaque) else {
                self.slots[slot] = None;
                continue;
            };
            let next = match actor {
                Actor::Opaque => Some(Actor::Opaque),
                Actor::Player(p) => self.run_player(slot, p, inputs)?.map(Actor::Player),
                Actor::Pivot(p) => self.run_pivot(slot, p, inputs)?,
                Actor::Satellite(s) => self.run_satellite(slot, s)?,
                Actor::Effect(e) => self.run_effect(e).map(Actor::Effect),
            };
            self.slots[slot] = next;
        }
        Ok(())
    }

    fn run_player(&mut self, slot: usize, mut p: Player, inputs: &Inputs) -> Result<Option<Player>, Unported> {
        use PlayerState::*;
        match p.state {
            WaitPair => {
                p.w26 = (p.w26 & 0x00FF) | (u16::from(((p.w26 >> 8) as u8).wrapping_add(1)) << 8);
                if p.flags & (flag::PIVOT_ACTIVE | flag::SATELLITE_ACTIVE) == 0 {
                    p.state = FindPops; // loc_3E96 bookmarks straight away
                } else if p.flags & flag::SOFT_DROP_POINT != 0 {
                    p.flags &= !flag::SOFT_DROP_POINT;
                    p.soft_drop_points += 1;
                }
            }
            FindPops => {
                p.w26 = self.fields[p.id as usize].find_pops().len() as u16;
                p.state = Redraw;
            }
            Redraw => p.state = Decide,
            Decide => {
                if p.w26 != 0 {
                    return Err(Unported("pops and chains (loc_3EDA)"));
                }
                p.delay = SETTLE_DELAY[p.speed_level.min(4) as usize];
                p.state = SettleDelay;
            }
            SettleDelay => {
                if wait(&mut p.delay) {
                    p.state = AfterDelay;
                }
            }
            AfterDelay => {
                if p.chain != 0 {
                    return Err(Unported("sending garbage after a chain (sub_998A)"));
                }
                p.chain = 0;
                if p.pending_garbage != 0 {
                    return Err(Unported("garbage drop (SpawnGarbage)"));
                }
                p.state = WaitGarbage;
            }
            WaitGarbage => {
                if p.flags & flag::GARBAGE_FALLING == 0 {
                    p.state = PreSpawn1;
                }
            }
            PreSpawn1 => p.state = PreSpawn2,
            PreSpawn2 => p.state = PreSpawn3,
            PreSpawn3 => p.state = Spawn,
            Spawn => {
                self.spawn_pair(slot, &mut p, inputs)?;
                p.state = WaitPair;
            }
        }
        Ok(Some(p))
    }

    /// `sub_4E24`: the next pair at column 2, row 2, satellite above.
    fn spawn_pair(&mut self, player_slot: usize, p: &mut Player, inputs: &Inputs) -> Result<(), Unported> {
        let id = p.id as usize;
        if self.fields[id].get(2, 2).is_some() {
            return Err(Unported("game over (spawn cell filled)"));
        }
        let (pivot_color, sat_color) = inputs.next_pair[id].ok_or(Unported("no next pair given"))?;
        let (pivot_slot, sat_slot) = match inputs.spawn_slots[id] {
            Some(slots) => slots,
            None => {
                let a = self.free_slot().ok_or(Unported("no free actor slot"))?;
                self.slots[a] = Some(Actor::Opaque);
                let b = self.free_slot().ok_or(Unported("no free actor slot"))?;
                (a, b)
            }
        };
        let pair = Pair {
            col: 2,
            row: 2,
            pivot_anim: Anim::new(script::IDLE),
            sat_anim: Anim::new(script::NONE),
            ..Pair::default()
        };
        self.slots[pivot_slot] = Some(Actor::Pivot(Pivot {
            player_slot,
            satellite_slot: sat_slot,
            color: pivot_color,
            state: PivotState::Init,
            pair,
            flags: 0,
            y: 0,
            fall: Fall::default(),
        }));
        self.slots[sat_slot] = Some(Actor::Satellite(Satellite {
            pivot_slot,
            player_slot,
            color: sat_color,
            state: SatelliteState::Init,
            col: 0,
            row: 0,
            flags: 0,
            y: 0,
            anim: Anim::new(script::NONE),
            fall: Fall::default(),
        }));
        p.flags |= flag::PIVOT_ACTIVE | flag::SATELLITE_ACTIVE;
        Ok(())
    }

    fn run_pivot(&mut self, _slot: usize, mut v: Pivot, inputs: &Inputs) -> Result<Option<Actor>, Unported> {
        let (id, player_flags, fall_speed) = {
            let p = self.player_mut(v.player_slot);
            (p.id as usize, p.flags, p.fall_speed)
        };
        match v.state {
            PivotState::Init => {
                v.place();
                v.state = PivotState::Controlled;
            }
            PivotState::Controlled => {
                if player_flags & flag::PIVOT_ACTIVE == 0 {
                    return Err(Unported("pair hidden while in play (loc_4F3A)"));
                }
                let step = v.pair.step_pivot(&self.fields[id], inputs.pads[id], inputs.keys[id], fall_speed, inputs.frame_count);
                if let Step::Falling { soft_drop_point: true } = step {
                    self.player_mut(v.player_slot).flags |= flag::SOFT_DROP_POINT;
                }
                v.place();
                if step == Step::Landed {
                    // loc_4F50: which beans are supported, and tell the satellite.
                    let resting = {
                        let field = &self.fields[id];
                        let below = |c: i16, r: i16| field.blocked(c, r + 1);
                        let (dc, dr) = DIR[(v.pair.rot & 3) as usize];
                        let (c, r) = (v.pair.col as i16, v.pair.row as i16);
                        let mut bits = u8::from(below(c, r)) | (u8::from(below(c + dc, r + dr)) << 1);
                        if bits != 0 && v.pair.rot & 1 == 0 {
                            bits |= 3;
                        }
                        bits
                    };
                    v.flags = resting | 4;
                    if resting & 1 != 0 {
                        v.pair.pivot_anim.time = 0;
                        v.state = PivotState::Squash;
                    } else {
                        v.fall = Fall::start(v.y);
                        v.pair.pivot_anim.frame = 0;
                        v.state = PivotState::Falling;
                    }
                }
            }
            PivotState::Falling => {
                let mut row = v.pair.row;
                if v.fall.step(&self.fields[id], v.pair.col, &mut row) {
                    v.pair.pivot_anim.ptr = script::SQUASH;
                    v.pair.pivot_anim.time = 0;
                    v.state = PivotState::Squash;
                }
                v.pair.row = row;
                v.y = v.fall.y;
            }
            PivotState::Squash => {
                if v.pair.pivot_anim.step() {
                    self.player_mut(v.player_slot).flags &= !flag::PIVOT_ACTIVE;
                    self.mark(id, v.pair.col, v.pair.row, v.color);
                    return Ok(Some(Actor::Effect(Effect::new(id as u8, v.y))));
                }
            }
        }
        Ok(Some(Actor::Pivot(v)))
    }

    fn run_satellite(&mut self, _slot: usize, mut s: Satellite) -> Result<Option<Actor>, Unported> {
        match s.state {
            SatelliteState::Init => {
                self.follow(&mut s);
                s.state = SatelliteState::Follow;
            }
            SatelliteState::Follow => {
                let landed = {
                    let pivot = self.pivot_mut(s.pivot_slot);
                    pivot.pair.step_satellite();
                    pivot.flags & 4 != 0
                };
                self.follow(&mut s);
                if landed {
                    // loc_549E: take over position, flags and animation from the pivot.
                    let pivot = self.pivot_mut(s.pivot_slot).clone();
                    let (dc, dr) = DIR[(pivot.pair.rot & 3) as usize];
                    s.col = pivot.pair.col.wrapping_add(dc as u16);
                    s.row = pivot.pair.row.wrapping_add(dr as u16);
                    s.player_slot = pivot.player_slot;
                    s.flags = pivot.flags;
                    s.anim = pivot.pair.sat_anim;
                    s.state = SatelliteState::Check;
                }
            }
            SatelliteState::Check => {
                if s.flags & 2 != 0 {
                    s.state = SatelliteState::Squash;
                } else {
                    s.fall = Fall::start(s.y);
                    s.state = SatelliteState::Falling;
                }
            }
            SatelliteState::Falling => {
                let id = self.player_mut(s.player_slot).id as usize;
                let mut row = s.row;
                if s.fall.step(&self.fields[id], s.col, &mut row) {
                    s.anim.ptr = script::SQUASH;
                    s.state = SatelliteState::Squash;
                }
                s.row = row;
                s.y = s.fall.y;
            }
            SatelliteState::Squash => {
                if s.anim.step() {
                    let id = {
                        let p = self.player_mut(s.player_slot);
                        p.flags &= !flag::SATELLITE_ACTIVE;
                        p.id as usize
                    };
                    self.mark(id, s.col, s.row, s.color);
                    return Ok(Some(Actor::Effect(Effect::new(id as u8, s.y))));
                }
            }
        }
        Ok(Some(Actor::Satellite(s)))
    }

    /// `sub_5552` (y only): 16 px from the pivot on the satellite's side.
    fn follow(&mut self, s: &mut Satellite) {
        let pivot = self.pivot_mut(s.pivot_slot);
        let (_, dr) = DIR[(pivot.pair.sat_angle >> 6) as usize & 3];
        let y = ((pivot.y >> 16) as u16).wrapping_add((dr * 16) as u16);
        s.y = (u32::from(y) << 16) | (s.y & 0xFFFF);
    }

    /// `MarkPuyoSpot`.
    fn mark(&mut self, player: usize, col: u16, row: u16, color: BeanColor) {
        if !self.popping[player] {
            self.fields[player].set(col as usize, row as usize, Some(color));
        }
    }

    fn run_effect(&mut self, mut e: Effect) -> Option<Effect> {
        let popping = self.popping[e.player as usize];
        match e.state {
            EffectState::Wait => {
                if popping {
                    return None;
                }
                if wait(&mut e.delay) {
                    e.state = EffectState::Start;
                }
            }
            EffectState::Start => {
                e.steps_left = 6;
                e.ticks = 0;
                e.state = EffectState::Rise;
            }
            EffectState::Rise => {
                if popping {
                    return None;
                }
                e.ticks += 1;
                if e.ticks >= 4 {
                    e.ticks = 0;
                    e.y = e.y.wrapping_sub(0x1_0000);
                    e.steps_left -= 1;
                    if e.steps_left == 0 {
                        return None;
                    }
                }
            }
        }
        Some(e)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}
