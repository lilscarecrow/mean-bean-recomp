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
//! post-landing effect, pops and chains, garbage, and the CPU opponent (`ai`). Game
//! over and the CPU's pattern play stop the engine with `Unported`.

use super::ai::{self, AiField};
use super::anim::{script, Anim};
use super::bean_order::BeanColor;
use super::field::{Field, Group, COLS, DIR, HIDDEN_ROWS, ROWS};
use super::motion::{cos, sin, Motion};
use super::pair::{PadInput, Pair, RotateKeys, Step};
use super::rng::Rng;

pub const SLOTS: usize = 64;
/// Screen x of each field's left edge (`word_5B34`).
pub const FIELD_X: [u16; 2] = [0x90, 0x150];
/// Screen y of a field's top visible row (`word_5B34`), same for both players.
pub const FIELD_TOP_Y: u16 = 0x90;
/// Frames to wait after a piece settles, by the player's +$2B (`word_3B58`).
const SETTLE_DELAY: [u16; 5] = [12, 8, 4, 2, 0];
/// Bounce tables after a post-pop fall, by color group (`unk_1A928`..): where their
/// end marker sits. Red/yellow, teal/green, purple/blue, garbage.
const BOUNCE_LEN: [u8; 4] = [22, 23, 22, 1];
/// Chain, color and group bonuses (`sub_9AC2`, `sub_9AEE`, `sub_9B34`).
const CHAIN_BONUS: [u16; 9] = [0, 8, 16, 32, 64, 128, 256, 512, 1024];
const COLOR_BONUS: [u16; 7] = [0, 0, 3, 6, 12, 24, 48];
const GROUP_BONUS: [u16; 12] = [0, 0, 0, 0, 0, 2, 3, 4, 5, 6, 7, 10];
const SCORE_CAP: u32 = 99_999_999;
const GARBAGE_POINTS: u32 = 70;
/// Scenario mode fall rate and beans to the next speed-up, by speed step (`unk_9DC4`).
const SCENARIO_SPEEDS: [(u16, u16); 24] = [
    (0x0400, 0x20), (0x0400, 0x20), (0x0500, 0x20), (0x0500, 0x20), (0x0600, 0x20), (0x0800, 0x20),
    (0x0A00, 0x20), (0x0C00, 0x28), (0x1000, 0x28), (0x2000, 0x30), (0x3000, 0x30), (0x4000, 0x38),
    (0x4800, 0x38), (0x5000, 0x40), (0x6000, 0x40), (0x8000, 0xFFFF), (0x8000, 0xFFFF), (0x8000, 4),
    (0x8000, 4), (0x8000, 4), (0xA000, 4), (0xC000, 4), (0xE000, 4), (0xFE00, 0xFFFF),
];
/// Player flag set while beans fall after a pop.
const FALLING_AFTER_POP: u8 = 0x10;

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
    /// Like `Opaque`, but allocated by `FindActorSlotQuick` (bit 7 of +$01 clear), so a
    /// full table lets `FindActorSlot` take its slot.
    OpaqueQuick,
    /// An unmodelled actor that deletes itself when its turn comes this frame (its slot
    /// is free for anything spawned later in the frame).
    Vanishing,
    Player(Player),
    Pivot(Pivot),
    Satellite(Satellite),
    Effect(Effect),
    Burst(Burst),
    Piece(Piece),
    GarbageClear(GarbageClear),
    Glow(Glow),
    GarbageDrop(GarbageDrop),
    GarbageColumn(GarbageColumn),
    Icon(Icon),
}

impl Actor {
    /// Allocated by `FindActorSlotQuick` (pop visuals): `FindActorSlot` may evict it.
    pub fn evictable(&self) -> bool {
        matches!(self, Actor::Burst(_) | Actor::Piece(_) | Actor::GarbageClear(_) | Actor::Icon(_) | Actor::OpaqueQuick)
    }

    /// The original resume address of this actor's current state (None for `Opaque`).
    pub fn code(&self) -> Option<u32> {
        match self {
            Actor::Opaque | Actor::OpaqueQuick | Actor::Vanishing => None,
            Actor::Player(p) => Some(p.state.code()),
            Actor::Pivot(p) => Some(p.state.code()),
            Actor::Satellite(s) => Some(s.state.code()),
            Actor::Effect(e) => Some(e.state.code()),
            Actor::Burst(b) => Some(b.state.code()),
            Actor::Piece(p) => Some(p.state.code()),
            Actor::GarbageClear(_) => Some(0x4DA4),
            Actor::Glow(g) => Some(g.state.code()),
            Actor::GarbageDrop(d) => Some(if d.started { 0x48BA } else { 0x48A2 }),
            Actor::GarbageColumn(c) => Some(if c.landed { 0x5044 } else { 0x486A }),
            Actor::Icon(_) => Some(0x9962),
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
    /// $3EB6: pops â†’ chain; otherwise start the settle delay.
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
    /// $3EFA: pops found; score this step (`sub_9A56`).
    PopScore,
    /// $3F06: back up the field, start blinking.
    PopBackup,
    /// $3F12: blink the popping beans for 24 frames (`sub_49D2`).
    Blink,
    /// $3F1A: add the points, spawn the pop effects (`sub_9BBA`, `CheckPuyoPop`).
    PopSpawn,
    /// $3F30: let the pop effects play (24 frames).
    PopWait,
    /// $3F34: close the gaps (`sub_4DB8`) and start the fall animation.
    Compact,
    /// $3F42: wait for the fall animation (plane command $89).
    FallWait,
    /// $3F5C: next chain step, look for pops again.
    ChainNext,
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
            PlayerState::PopScore => 0x3EFA,
            PlayerState::PopBackup => 0x3F06,
            PlayerState::Blink => 0x3F12,
            PlayerState::PopSpawn => 0x3F1A,
            PlayerState::PopWait => 0x3F30,
            PlayerState::Compact => 0x3F34,
            PlayerState::FallWait => 0x3F42,
            PlayerState::ChainNext => 0x3F5C,
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
    /// Pending garbage (+$14).
    pub pending_garbage: u16,
    /// Speed step (+$08): bumped each time `beans_count` reaches `threshold`.
    pub speed_index: u8,
    /// Score (+$0A long) and garbage accumulator (+$0E long).
    pub score: u32,
    pub acc: u32,
    /// This step's base points (+$12) and bonus (+$1E).
    pub base: u16,
    pub bonus: u16,
    /// Beans popped (+$16, capped 9999) and since the last speed-up (+$18).
    pub beans_total: u16,
    pub beans_count: u16,
    /// Beans to pop before the next speed-up (+$1C).
    pub threshold: u16,
    /// Garbage this player's chains sent while no opponent actor is modelled.
    pub garbage_sent: u16,
    /// The CPU's plan (`sub_127BA`): target column (+$20) and rotation (+$21), frames
    /// to move (+$2C) and to pause (+$2D). Its timer and phase are `w26`'s two bytes.
    pub target: (u8, u8),
    pub move_frames: u8,
    pub pause_frames: u8,
    /// Work for the chain step in progress.
    pub pop: Option<PopWork>,
}

/// One chain step in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct PopWork {
    pub groups: Vec<Group>,
    /// Popping beans plus the garbage they clear (both blink).
    pub cells: Vec<(usize, usize)>,
    /// Field as it was before blinking (`pPuyosCopy`).
    pub backup: Field,
    /// Beans dropping after the gaps closed (`pUnk6`).
    pub falling: Vec<FallingBean>,
}

/// A bean animating down after a pop: 2 steps per row, then a bounce (`SpecPlane89`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallingBean {
    /// Where it was before the gaps closed.
    pub col: usize,
    pub row: usize,
    pub rows: u8,
    /// Steps taken so far (low byte of its `pUnk6` word).
    pub counter: u8,
    /// Bounce length: index of the end marker in its table (by color group).
    pub bounce: u8,
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
    /// +$1E: added to `vel` every frame ($3000 for the pair, per column for garbage).
    pub accel: u16,
    /// +$20 word: y where the fall started, minus 15.
    pub base: u16,
}

impl Fall {
    fn start(y: u32) -> Fall {
        Fall::with(y, FALL_START_VEL, FALL_ACCEL)
    }

    fn with(y: u32, vel: u32, accel: u16) -> Fall {
        Fall { y, vel, accel, base: ((y >> 16) as u16).wrapping_sub(15) }
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
        self.vel = self.vel.wrapping_add(u32::from(self.accel)).min(FALL_MAX_VEL);
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

// ---------------------------------------------------------------- pop visuals

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstState {
    /// $4B72: staggered start (`aDelay` 0, 4, 8, 12 by pop order).
    Wait,
    /// $4B76: burst into pieces.
    Burst,
    /// $4B7E: pop animation, then gone.
    Animate,
}

impl BurstState {
    pub fn code(self) -> u32 {
        match self {
            BurstState::Wait => 0x4B72,
            BurstState::Burst => 0x4B76,
            BurstState::Animate => 0x4B7E,
        }
    }
}

/// A popping bean (`ActPuyoPop`).
#[derive(Clone, Debug, PartialEq)]
pub struct Burst {
    pub player: u8,
    pub color: u8,
    pub x: u16,
    pub y: u16,
    pub delay: u16,
    pub anim: Anim,
    pub state: BurstState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceState {
    /// $4D74: just thrown.
    Start,
    /// $4D7C: wait 4 frames.
    Wait,
    /// $4D80: fly under gravity until the animation ends or it leaves the screen.
    Fly,
}

impl PieceState {
    pub fn code(self) -> u32 {
        match self {
            PieceState::Start => 0x4D74,
            PieceState::Wait => 0x4D7C,
            PieceState::Fly => 0x4D80,
        }
    }
}

/// A piece flying off a popped bean (`ActPuyoPoppedPiece`).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub player: u8,
    pub color: u8,
    pub motion: Motion,
    pub draw: u8,
    pub delay: u16,
    pub anim: Anim,
    pub state: PieceState,
}

/// Garbage cleared by a pop (`ActGarbageRemove`, $4DA4): plays its animation, then gone.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageClear {
    pub player: u8,
    pub x: u16,
    pub y: u16,
    pub anim: Anim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlowState {
    /// $23628: just spawned (first chain step: straight to `End`).
    Start,
    /// $23642, $2364C: hover for 8 frames.
    Hover,
    HoverWait,
    /// $23658: aim at the other side.
    Launch,
    /// $2369A: fly there in 33 frames, wobbling.
    Fly,
    /// $23736: final animation, then gone.
    End,
}

impl GlowState {
    pub fn code(self) -> u32 {
        match self {
            GlowState::Start => 0x23628,
            GlowState::Hover => 0x23642,
            GlowState::HoverWait => 0x2364C,
            GlowState::Launch => 0x23658,
            GlowState::Fly => 0x2369A,
            GlowState::End => 0x23736,
        }
    }
}

/// The chain glow (`ActGarbageGlow`).
#[derive(Clone, Debug, PartialEq)]
pub struct Glow {
    pub player: u8,
    /// The chaining player's actor (+$2E).
    pub player_slot: usize,
    /// Chain step it was spawned on (+$2B; 0 = first step).
    pub chain: u8,
    pub motion: Motion,
    pub draw: u8,
    /// Un-wobbled x (+$1E word) and flight counter (+$26 word).
    pub base_x: u16,
    pub counter: u16,
    pub delay: u16,
    pub anim: Anim,
    pub state: GlowState,
}

// ---------------------------------------------------------------- garbage

/// Per-column acceleration of falling garbage (`word_47BA`).
const GARBAGE_ACCEL: [u16; 6] = [0x2400, 0x2600, 0x2000, 0x2A00, 0x2200, 0x2800];
/// Start speed of falling garbage, px/frame, by difficulty (`word_4730`).
const GARBAGE_START_SPEED: [u16; 3] = [3, 2, 0];
/// Small-icon nudge on P1's side, by size (`unk_9924`).
const ICON_NUDGE: [i16; 5] = [4, -8, -20, -32, -44];

/// A garbage drop in progress (`ActGarbagePuyo`): counts the falling columns, shakes
/// the field each time one lands, and clears the player's "garbage falling" flag when
/// everything is down.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageDrop {
    pub player_slot: usize,
    /// False before its first run ($48A2), true in the update loop ($48BA).
    pub started: bool,
    /// Columns still falling (+$26) and the count last seen (+$28).
    pub falling: u16,
    pub seen: u16,
    /// +$20: (1 + columns) × $100, the shake strength when a column lands.
    pub strength: u16,
    /// +$38: current shake strength, -$40 per frame; +$36 shake phase.
    pub shake: u16,
    pub phase: u8,
    /// +$07 bit 0: the "incoming!" voice played.
    pub voiced: bool,
    /// +$09: copied from the last column to land.
    pub frame: u8,
}

/// One column of falling garbage (`sub_486A`): the whole stack drops as one sprite and
/// is written into the field when it lands.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageColumn {
    pub player: u8,
    pub drop_slot: usize,
    pub col: u16,
    /// Row of the stack's bottom bean (+$1C).
    pub row: u16,
    /// +$09: beans in the stack + $13 (and +6 while a single bean lingers).
    pub frame: u8,
    pub fall: Fall,
    /// Landed: a single bean lingers 48 frames ($5044); taller stacks vanish at once.
    pub landed: bool,
    pub linger: u16,
}

/// An incoming-garbage icon above a field (`sub_9962`): slides into place over 16
/// frames, vanishes when its side's icons are redrawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    /// Which generation counter it watches (0 = over P2's field, 1 = over P1's).
    pub side: u8,
    pub generation: u8,
    /// +$09: 5 = 6 beans, 6 = 30 beans, 0-4 = 1-5 beans.
    pub frame: u8,
    pub motion: Motion,
    /// Frames of sliding left (+$26).
    pub slide: u16,
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

/// `ActPuyoPoppedPiece`.
fn run_piece(mut p: Piece) -> Option<Piece> {
    match p.state {
        PieceState::Start => {
            p.delay = 4;
            p.state = PieceState::Wait;
        }
        PieceState::Wait => {
            if wait(&mut p.delay) {
                p.state = PieceState::Fly;
            }
        }
        PieceState::Fly => {
            p.draw = 0x87;
            if p.anim.step() || p.motion.step(p.draw) {
                return None;
            }
        }
    }
    Some(p)
}

/// `sub_9C64`: points go to the score (capped) and the garbage accumulator (capped
/// at 65,535).
fn add_points(p: &mut Player, points: u32) {
    p.acc = p.acc.wrapping_add(points);
    if p.acc >> 16 != 0 {
        p.acc = 0xFFFF;
    }
    p.score = p.score.checked_add(points).unwrap_or(SCORE_CAP).min(SCORE_CAP);
}

/// `sub_4DB8`: close the gaps in rows 1-13 at once (row 0 stays put), noting how far
/// each bean dropped and which bounce its color plays.
fn close_gaps(field: &mut Field, backup: &Field) -> Vec<FallingBean> {
    let mut falling = vec![];
    for col in (0..COLS).rev() {
        let mut target = ROWS - 1;
        let mut gap = 0u8;
        for row in (1..ROWS).rev() {
            gap += 1;
            let Some(bean) = field.get(col as i16, row as i16) else { continue };
            field.set(col, row, None);
            field.set(col, target, Some(bean));
            target -= 1;
            gap -= 1;
            if gap != 0 {
                let color = backup.get(col as i16, row as i16).unwrap_or(bean);
                let group = if color == BeanColor::Garbage { 3 } else { ((color as usize) >> 1) & 3 };
                falling.push(FallingBean { col, row, rows: gap, counter: 0, bounce: BOUNCE_LEN[group] });
            }
        }
    }
    falling
}

/// The CPU's plan as the player actor holds it (timer and phase in `w26`).
fn player_plan(p: &Player) -> ai::Plan {
    ai::Plan {
        col: p.target.0,
        rot: p.target.1,
        timer: (p.w26 >> 8) as u8,
        phase: p.w26 as u8,
        move_frames: p.move_frames,
        pause_frames: p.pause_frames,
    }
}

fn set_player_plan(p: &mut Player, plan: ai::Plan) {
    p.target = (plan.col, plan.rot);
    p.w26 = u16::from(plan.timer) << 8 | u16::from(plan.phase);
    p.move_frames = plan.move_frames;
    p.pause_frames = plan.pause_frames;
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
    /// `level_mode` (0 scenario, 1 vs, 2 exercise, ...).
    pub level_mode: u8,
    /// `byte_FF0104`: offsets the scenario speed table (from the difficulty).
    pub difficulty: u8,
    /// Plane commands $89 (post-pop fall animation) queued this frame, by player slot;
    /// the vertical-blank handler runs them before the next frame's actors.
    pub fall_commands: Vec<usize>,
    /// The game's random number generator (`rng_seed`).
    pub rng: Rng,
    /// `Random` calls made during the last frame (vertical blank included).
    pub rng_calls: u32,
    /// `garbage_glow_x`/`_y`: where the last popped bean was; the glow starts there.
    pub glow_xy: (u16, u16),
    /// Spawns of actors this port doesn't have yet.
    pub unported_spawns: u32,
    /// `byte_FF19B2`/`_B3`: a player is in the middle of a chain.
    pub chaining: [bool; 2],
    /// `byte_FF19B0`/`_B1`: icon generations; icons of an older generation vanish.
    /// Index 0 = icons over P2's field, 1 = over P1's.
    pub icon_gen: [u8; 2],
    /// Pending garbage of a player with no actor in this engine (set by the caller).
    pub external_pending: [u16; 2],
    /// Sides whose icons this engine redrew during the last frame.
    pub icons_drawn: [bool; 2],
    /// `Random` calls made by unported actors, as (before slot, count): drawn just
    /// before that slot runs (used when checking against recordings).
    pub foreign_random: Vec<(usize, u32)>,
    /// Slots whose actors drew random numbers last frame, in order.
    pub rng_users: Vec<usize>,
    current_slot: usize,
    /// The CPU's byte image of each field: cells with links, group marks, its tables.
    pub ai_fields: [AiField; 2],
    /// Per-CPU memory between pieces (`byte_FF1D4E` / `byte_FF1D58`).
    pub cpu_memory: [ai::CpuMemory; 2],
    /// Pad reads since the last plan (`byte_FF1D0A` / `_0B`).
    pub cpu_reads: [u8; 2],
    /// `byte_FF1D0C` / `byte_FF1D0D`: for the CPU's controller sprites (not ported).
    pub cpu_pressed: u8,
    pub cpu_lean: u8,
    /// `word_FF198C` (scenario mode): 2 if the CPU has 36+ beans, 1 if the player has.
    pub crowding: u16,
    /// `opponent`, `level`, `time_minutes`, `control_player_1`.
    pub opponent: u8,
    pub level: u8,
    pub time_minutes: u16,
    pub control_player_1: u8,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            slots: vec![None; SLOTS],
            fields: [Field::default(), Field::default()],
            popping: [false; 2],
            level_mode: 0,
            difficulty: 0,
            fall_commands: vec![],
            rng: Rng::new(0),
            rng_calls: 0,
            glow_xy: (0, 0),
            unported_spawns: 0,
            chaining: [false; 2],
            icon_gen: [0; 2],
            external_pending: [0; 2],
            icons_drawn: [false; 2],
            foreign_random: vec![],
            rng_users: vec![],
            current_slot: usize::MAX,
            ai_fields: [AiField::default(), AiField::default()],
            cpu_memory: [[0; 10]; 2],
            cpu_reads: [0; 2],
            cpu_pressed: 0,
            cpu_lean: 0,
            crowding: 0,
            opponent: 0,
            level: 0,
            time_minutes: 0,
            control_player_1: 0xFF,
        }
    }

    /// `sub_12646`: the CPU plays this player.
    pub fn is_cpu(&self, id: u8) -> bool {
        ai::is_cpu(self.level_mode, id, self.control_player_1)
    }

    /// Count the draws code outside `random()` made from `before` on (at most a few).
    fn note_draws(&mut self, before: Rng) {
        let mut r = before;
        for _ in 0..64 {
            if r == self.rng {
                return;
            }
            r.next();
            self.rng_calls += 1;
            if self.current_slot != usize::MAX && self.rng_users.last() != Some(&self.current_slot) {
                self.rng_users.push(self.current_slot);
            }
        }
        panic!("lost track of random draws");
    }

    fn random(&mut self) -> u16 {
        self.rng_calls += 1;
        if self.current_slot != usize::MAX && self.rng_users.last() != Some(&self.current_slot) {
            self.rng_users.push(self.current_slot);
        }
        self.rng.next()
    }

    /// `SpecPlane89`: one step of every bean falling after a pop. The flag stays set
    /// while any bean was still moving at the start of the step.
    fn run_fall_command(&mut self, player_slot: usize) {
        let p = self.player_mut(player_slot);
        p.flags &= !FALLING_AFTER_POP;
        let Some(work) = p.pop.as_mut() else { return };
        let mut any = false;
        work.falling.retain_mut(|b| {
            any = true;
            let drop_steps = b.rows * 2;
            if b.counter >= drop_steps && b.counter - drop_steps + 1 >= b.bounce {
                return false; // reached the end marker
            }
            b.counter += 1;
            true
        });
        if any {
            p.flags |= FALLING_AFTER_POP;
        }
    }

    /// `FindActorSlot`: the first free slot.
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// `FindActorSlot`: a free slot, or, with the table full, the first one held by a
    /// `FindActorSlotQuick` actor (which is wiped and replaced).
    fn find_actor_slot(&self) -> Option<usize> {
        self.free_slot().or_else(|| self.slots.iter().position(|s| s.as_ref().is_some_and(Actor::evictable)))
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
        self.vblank();
        self.run_slots(0..SLOTS, inputs)
    }

    /// The vertical blank before a frame's logic: queued plane commands, then one
    /// `Random`. (When the logic overruns a video frame, the ROM's vertical blank lands
    /// in the middle of `RunActors`; see `run_slots`.)
    pub fn vblank(&mut self) {
        self.rng_calls = 0;
        self.icons_drawn = [false; 2];
        self.rng_users.clear();
        self.current_slot = usize::MAX;
        for slot in std::mem::take(&mut self.fall_commands) {
            self.run_fall_command(slot);
        }
        self.random();
    }

    /// `RunActors` over some of the slots (all of them in a normal frame).
    pub fn run_slots(&mut self, slots: std::ops::Range<usize>, inputs: &Inputs) -> Result<(), Unported> {
        let foreign = std::mem::take(&mut self.foreign_random);
        for slot in slots {
            for &(_, n) in foreign.iter().filter(|(s, _)| *s == slot) {
                (0..n).for_each(|_| {
                    self.rng.next();
                });
            }
            self.current_slot = slot;
            // Leave a placeholder so the running actor's slot still counts as used.
            let Some(actor) = self.slots[slot].replace(Actor::Opaque) else {
                self.slots[slot] = None;
                continue;
            };
            let next = match actor {
                Actor::Opaque => Some(Actor::Opaque),
                Actor::OpaqueQuick => Some(Actor::OpaqueQuick),
                Actor::Vanishing => None,
                Actor::Player(p) => self.run_player(slot, p, inputs)?.map(Actor::Player),
                Actor::Pivot(p) => self.run_pivot(slot, p, inputs)?,
                Actor::Satellite(s) => self.run_satellite(slot, s)?,
                Actor::Effect(e) => self.run_effect(e).map(Actor::Effect),
                Actor::Burst(b) => self.run_burst(b).map(Actor::Burst),
                Actor::Piece(p) => run_piece(p).map(Actor::Piece),
                Actor::GarbageClear(mut g) => (!g.anim.step()).then_some(Actor::GarbageClear(g)),
                Actor::Glow(g) => self.run_glow(g).map(Actor::Glow),
                Actor::GarbageDrop(d) => self.run_garbage_drop(d).map(Actor::GarbageDrop),
                Actor::GarbageColumn(c) => self.run_garbage_column(c).map(Actor::GarbageColumn),
                Actor::Icon(i) => self.run_icon(i).map(Actor::Icon),
            };
            self.slots[slot] = next;
        }
        self.current_slot = usize::MAX;
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
                    add_points(&mut p, 1);
                }
            }
            FindPops => {
                // sub_5960 also leaves links and group marks, which the CPU reads later.
                self.ai_fields[p.id as usize].render(&self.fields[p.id as usize], true);
                p.w26 = self.fields[p.id as usize].find_pops().len() as u16;
                p.state = Redraw;
            }
            Redraw => p.state = Decide,
            Decide => {
                if p.w26 != 0 {
                    // loc_3EDA: the popping beans and the garbage they clear (sub_58C8).
                    let field = &self.fields[p.id as usize];
                    let groups = field.find_pops();
                    let mut cells: Vec<(usize, usize)> = groups.iter().flat_map(|g| g.cells.iter().copied()).collect();
                    for (c, r) in cells.clone() {
                        for (dc, dr) in DIR {
                            let (nc, nr) = (c as i16 + dc, r as i16 + dr);
                            let cell = (nc as usize, nr as usize);
                            if nr >= HIDDEN_ROWS as i16 && field.get(nc, nr) == Some(BeanColor::Garbage) && !cells.contains(&cell) {
                                cells.push(cell);
                            }
                        }
                    }
                    p.pop = Some(PopWork { groups, cells, backup: field.clone(), falling: vec![] });
                    self.chaining[p.id as usize & 1] = true;
                    p.state = PopScore;
                } else {
                    p.delay = SETTLE_DELAY[p.speed_level.min(4) as usize];
                    p.state = SettleDelay;
                }
            }
            PopScore => {
                self.score_step(&mut p);
                p.state = PopBackup;
            }
            PopBackup => {
                let backup = self.fields[p.id as usize].clone();
                p.pop.as_mut().unwrap().backup = backup;
                p.delay = 0x18;
                p.state = Blink;
            }
            Blink => {
                // sub_49D2: shown on even delay values, hidden on odd ones.
                let work = p.pop.as_ref().unwrap();
                let field = &mut self.fields[p.id as usize];
                for &(c, r) in &work.cells {
                    let shown = if p.delay & 1 == 0 { work.backup.get(c as i16, r as i16) } else { None };
                    field.set(c, r, shown);
                }
                if wait(&mut p.delay) {
                    p.state = PopSpawn;
                }
            }
            PopSpawn => {
                let points = if p.bonus == 0 { u32::from(p.base) } else { u32::from(p.base) * u32::from(p.bonus) };
                add_points(&mut p, points);
                self.spawn_pops(&p);
                if self.level_mode & 2 == 0 {
                    self.spawn_glow(slot, &p);
                }
                p.delay = 0x18;
                p.state = PopWait;
            }
            PopWait => {
                if wait(&mut p.delay) {
                    p.state = Compact;
                }
            }
            Compact => {
                let id = p.id as usize;
                let work = p.pop.as_mut().unwrap();
                work.falling = close_gaps(&mut self.fields[id], &work.backup);
                p.flags |= FALLING_AFTER_POP;
                p.state = FallWait;
            }
            FallWait => {
                if p.flags & FALLING_AFTER_POP != 0 {
                    self.fall_commands.push(slot); // loc_4E14
                } else {
                    self.speed_up(&mut p)?;
                    p.state = ChainNext;
                }
            }
            ChainNext => {
                p.chain = p.chain.saturating_add(1);
                p.pop = None;
                p.state = FindPops; // loc_3E96 bookmarks straight away
            }
            SettleDelay => {
                if wait(&mut p.delay) {
                    p.state = AfterDelay;
                }
            }
            AfterDelay => {
                if p.chain != 0 && self.level_mode & 2 == 0 {
                    self.send_garbage(slot, &mut p);
                }
                // loc_3DB0
                p.chain = 0;
                self.chaining[p.id as usize & 1] = false;
                self.spawn_garbage(slot, &mut p);
                if self.level_mode & 2 == 0 && !self.chaining[(p.id as usize & 1) ^ 1] {
                    // sub_9814: this player's own incoming garbage.
                    self.show_icons(p.id ^ 1, u32::from(p.pending_garbage));
                }
                p.state = WaitGarbage;
            }
            WaitGarbage => {
                if p.flags & flag::GARBAGE_FALLING == 0 {
                    // loc_3DDE: sub_12BAA
                    let id = p.id as usize;
                    self.ai_fields[id].render(&self.fields[id], false);
                    ai::analyze(&mut self.ai_fields[id]);
                    if self.level_mode & 3 == 0 && id == 1 {
                        self.crowding = ai::crowding(&self.ai_fields[1], &self.ai_fields[0]);
                    }
                    p.state = PreSpawn1;
                }
            }
            PreSpawn1 => {
                if self.is_cpu(p.id) {
                    ai::placement_table(&mut self.ai_fields[p.id as usize], false);
                }
                p.state = PreSpawn2;
            }
            PreSpawn2 => {
                if self.is_cpu(p.id) {
                    ai::placement_table(&mut self.ai_fields[p.id as usize], true);
                }
                p.state = PreSpawn3;
            }
            PreSpawn3 => {
                self.cpu_reads[p.id as usize & 1] = 0;
                if self.is_cpu(p.id) {
                    self.plan(&mut p, inputs)?;
                }
                p.state = Spawn;
            }
            Spawn => {
                self.spawn_pair(slot, &mut p, inputs)?;
                p.state = WaitPair;
            }
        }
        Ok(Some(p))
    }

    /// `sub_127BA`: where the CPU puts the pair about to spawn.
    fn plan(&mut self, p: &mut Player, inputs: &Inputs) -> Result<(), Unported> {
        let id = p.id as usize;
        let (pivot, sat) = inputs.next_pair[id].ok_or(Unported("no next pair given"))?;
        let ctx = ai::PlanContext {
            player_id: p.id,
            opponent: self.opponent,
            difficulty: self.difficulty,
            time_minutes: self.time_minutes,
            pair: (pivot as u8, sat as u8),
        };
        let mut plan = player_plan(p);
        let mut scratch = ai::PlanScratch::default();
        let before = self.rng;
        let res = ai::plan(&self.ai_fields[id], &ctx, &mut self.cpu_memory[id], &mut plan, &mut scratch, &mut self.rng);
        self.note_draws(before);
        res.map_err(Unported)?;
        set_player_plan(p, plan);
        Ok(())
    }

    /// `CheckPuyoPop`: a burst for every popping bean and a clear effect for every
    /// cleared garbage bean, in reading order; bursts start 0, 4, 8, 12 frames apart.
    fn spawn_pops(&mut self, p: &Player) {
        let id = p.id as usize;
        let work = p.pop.as_ref().unwrap();
        let mut stagger = 0u16;
        for row in HIDDEN_ROWS..ROWS {
            for col in 0..COLS {
                if !work.cells.contains(&(col, row)) {
                    continue;
                }
                let x = FIELD_X[id] + 8 + col as u16 * 16;
                let y = FIELD_TOP_Y + 8 + (row - HIDDEN_ROWS) as u16 * 16;
                let Some(slot) = self.free_slot() else { continue };
                let color = work.backup.get(col as i16, row as i16).map_or(0, |c| c as u8);
                if color == BeanColor::Garbage as u8 {
                    self.slots[slot] = Some(Actor::GarbageClear(GarbageClear { player: p.id, x, y, anim: Anim::new(script::GARBAGE_CLEAR) }));
                } else {
                    self.glow_xy = (x, y);
                    self.slots[slot] = Some(Actor::Burst(Burst {
                        player: p.id,
                        color,
                        x,
                        y,
                        delay: stagger,
                        anim: Anim { ptr: script::POP, time: 0, frame: 8 },
                        state: BurstState::Wait,
                    }));
                    stagger = (stagger + 4) & 0xF;
                }
            }
        }
    }

    /// `SpawnGarbageGlow`.
    fn spawn_glow(&mut self, player_slot: usize, p: &Player) {
        let Some(slot) = self.find_actor_slot() else { return };
        let (x, y) = self.glow_xy;
        self.slots[slot] = Some(Actor::Glow(Glow {
            player: p.id,
            player_slot,
            chain: p.chain,
            motion: Motion { x: u32::from(x) << 16, y: u32::from(y) << 16, ..Motion::default() },
            draw: 0,
            base_x: 0,
            counter: 0,
            delay: 0,
            anim: Anim::new(script::GLOW[p.id as usize & 1]),
            state: GlowState::Start,
        }));
    }

    fn run_burst(&mut self, mut b: Burst) -> Option<Burst> {
        match b.state {
            BurstState::Wait => {
                if wait(&mut b.delay) {
                    b.state = BurstState::Burst;
                }
            }
            BurstState::Burst => {
                // ActPuyoPop_PopNormal: four pieces, roughly up-left, up-right, ...
                for i in (0..4u8).rev() {
                    let Some(slot) = self.free_slot() else { continue };
                    let angle = (i << 4).wrapping_add(0x64).wrapping_add((self.random() & 7) as u8);
                    self.slots[slot] = Some(Actor::Piece(Piece {
                        player: b.player,
                        color: b.color,
                        motion: Motion {
                            x: u32::from(b.x) << 16,
                            y: u32::from(b.y) << 16,
                            vx: sin(angle, 0x400),
                            vy: cos(angle, 0x400),
                            ay: 0x4000,
                            ty: 0xFFFF,
                            ..Motion::default()
                        },
                        draw: 0,
                        delay: 0,
                        anim: Anim { ptr: script::POPPED_PIECE, time: 0, frame: 6 },
                        state: PieceState::Start,
                    }));
                }
                b.state = BurstState::Animate;
            }
            BurstState::Animate => {
                if b.anim.step() {
                    return None;
                }
            }
        }
        Some(b)
    }

    fn run_glow(&mut self, mut g: Glow) -> Option<Glow> {
        match g.state {
            GlowState::Start => {
                if g.chain == 0 {
                    self.glow_finish(&mut g);
                } else {
                    g.anim.step();
                    g.draw = 0x83;
                    g.state = GlowState::Hover;
                }
            }
            GlowState::Hover => {
                g.delay = 8;
                g.state = GlowState::HoverWait;
            }
            GlowState::HoverWait => {
                g.anim.step();
                if wait(&mut g.delay) {
                    g.state = GlowState::Launch;
                }
            }
            GlowState::Launch => {
                // Aim at the other player's side in 32 frames.
                g.counter = 0x1F;
                let target_x: u32 = if g.player == 0 { 0x180 << 16 } else { 0xC0 << 16 };
                g.motion.vx = ((target_x.wrapping_sub(g.motion.x) as i32) >> 5) as u32;
                g.motion.vy = ((0x88_0000u32.wrapping_sub(g.motion.y) as i32) >> 5) as u32;
                g.base_x = (g.motion.x >> 16) as u16;
                g.state = GlowState::Fly;
            }
            GlowState::Fly => {
                g.anim.step();
                g.motion.x = (u32::from(g.base_x) << 16) | (g.motion.x & 0xFFFF);
                g.motion.step(g.draw);
                g.base_x = (g.motion.x >> 16) as u16;
                let side: u8 = if g.player == 0 { 0x80 } else { 0 };
                let angle = side | ((g.counter as u8) << 2);
                let wobble = (sin(angle, 0x6000) >> 16) as u16;
                g.motion.x = (u32::from(((g.motion.x >> 16) as u16).wrapping_add(wobble)) << 16) | (g.motion.x & 0xFFFF);
                let done = g.counter == 0;
                g.counter = g.counter.wrapping_sub(1);
                if done {
                    g.anim.ptr = script::GLOW_END[g.player as usize & 1];
                    self.glow_finish(&mut g);
                }
            }
            GlowState::End => {
                if g.anim.step() {
                    return None;
                }
            }
        }
        Some(g)
    }

    /// `loc_2371C`: refresh the opponent's incoming-garbage icons (not ported: they
    /// are separate actors), then play out the animation.
    fn glow_finish(&mut self, g: &mut Glow) {
        // loc_984A: the opponent's icons, showing what this chain will send.
        let (acc, id) = match &self.slots[g.player_slot] {
            Some(Actor::Player(p)) => (p.acc, p.id),
            _ => (0, g.player),
        };
        let total = (acc & 0xFFFF) / GARBAGE_POINTS + u32::from(self.pending_of(id ^ 1));
        self.show_icons(id, total & 0xFFFF);
        g.state = GlowState::End;
    }

    fn pending_of(&self, id: u8) -> u16 {
        self.slots
            .iter()
            .find_map(|s| match s {
                Some(Actor::Player(p)) if p.id == id => Some(p.pending_garbage),
                _ => None,
            })
            .unwrap_or(self.external_pending[id as usize & 1])
    }

    /// `SpawnGarbage`: shuffle the columns, deal up to 30 pending beans into them, and
    /// start one falling stack per column plus the drop's controller.
    fn spawn_garbage(&mut self, player_slot: usize, p: &mut Player) {
        if p.pending_garbage == 0 {
            return;
        }
        let id = p.id as usize & 1;
        // SetGarbageOrder
        let mut order = [0u8, 1, 2, 3, 4, 5];
        for i in (0..COLS).rev() {
            let r = self.random();
            let j = ((u32::from(r) * 6) >> 16) as usize;
            order.swap(i, j);
        }
        // GetGarbageFreeSpace / SetGarbageCounts
        let room = self.fields[id].garbage_room();
        let mut counts = [0u8; COLS];
        let mut placed = 0u16;
        for i in 0..p.pending_garbage.min(super::field::MAX_GARBAGE_DROP) as usize {
            let c = order[i % COLS] as usize;
            if counts[c] < room[c] {
                counts[c] += 1;
                placed += 1;
            }
        }
        p.pending_garbage -= placed;
        let Some(drop_slot) = self.find_actor_slot() else { return };
        self.slots[drop_slot] = Some(Actor::GarbageDrop(GarbageDrop {
            player_slot,
            started: false,
            falling: 0,
            seen: 0,
            strength: 0x0100,
            shake: 0,
            phase: 0,
            voiced: false,
            frame: 0,
        }));
        p.flags |= flag::GARBAGE_FALLING;
        let speed_index = if self.level_mode == 0 && self.difficulty < 3 { self.difficulty as usize } else { 2 };
        let speed = u32::from(GARBAGE_START_SPEED[speed_index]) << 16;
        let y = FIELD_TOP_Y - 8;
        for col in (0..COLS).rev() {
            if counts[col] == 0 {
                continue;
            }
            let Some(slot) = self.find_actor_slot() else { continue };
            if let Some(Actor::GarbageDrop(d)) = self.slots[drop_slot].as_mut() {
                d.falling += 1;
                d.strength = d.strength.wrapping_add(0x100);
            }
            self.slots[slot] = Some(Actor::GarbageColumn(GarbageColumn {
                player: p.id,
                drop_slot,
                col: col as u16,
                row: 1,
                frame: counts[col] + 0x13,
                fall: Fall::with(u32::from(y) << 16, speed, GARBAGE_ACCEL[col]),
                landed: false,
                linger: 0,
            }));
        }
    }

    fn run_garbage_drop(&mut self, mut d: GarbageDrop) -> Option<GarbageDrop> {
        if !d.started {
            d.seen = d.falling;
            d.started = true;
            return Some(d);
        }
        if d.falling != d.seen {
            d.seen = d.falling;
            d.shake = d.strength;
            d.voiced = true;
        }
        d.phase = d.phase.wrapping_add(0x18);
        if d.shake != 0 {
            d.shake = d.shake.wrapping_sub(0x40);
            return Some(d);
        }
        if d.falling != 0 {
            return Some(d);
        }
        if let Some(Actor::Player(p)) = self.slots[d.player_slot].as_mut() {
            p.flags &= !flag::GARBAGE_FALLING;
        }
        None
    }

    fn run_garbage_column(&mut self, mut c: GarbageColumn) -> Option<GarbageColumn> {
        let id = c.player as usize & 1;
        if c.landed {
            if self.popping[id] {
                return None;
            }
            c.linger -= 1;
            return (c.linger != 0).then_some(c);
        }
        let mut row = c.row;
        let down = c.fall.step(&self.fields[id], c.col, &mut row);
        c.row = row;
        if !down {
            return Some(c);
        }
        // Write the stack bottom-up.
        for _ in 0..c.frame.saturating_sub(0x13) {
            self.mark(id, c.col, c.row, BeanColor::Garbage);
            c.row = c.row.wrapping_sub(1);
        }
        if let Some(Actor::GarbageDrop(d)) = self.slots[c.drop_slot].as_mut() {
            d.falling = d.falling.wrapping_sub(1);
            d.frame = c.frame;
        }
        // loc_502C
        if c.frame >= 0x15 {
            return None;
        }
        c.frame += 6;
        c.linger = 0x30;
        c.landed = true;
        Some(c)
    }

    /// `loc_9858`: redraw one side's incoming-garbage icons for `total` beans:
    /// up to six big icons (30 or 6 beans), then a small one for the rest.
    fn show_icons(&mut self, side: u8, total: u32) {
        if self.popping.iter().any(|&p| p) {
            return;
        }
        let side = side & 1;
        self.icons_drawn[side as usize] = true;
        self.icon_gen[side as usize] = self.icon_gen[side as usize].wrapping_add(1);
        let generation = self.icon_gen[side as usize];
        let (start_x, mut target_x, step): (u16, u16, u16) =
            if side == 0 { (0x180, 0x150, 0x10) } else { (0xC0, 0xE0, 0xFFF0) };
        let (mut big, small) = (total / 6, total % 6);
        let mut index = 0;
        let icon = |engine: &mut Engine, target: u16, frame: u8, nudge: i16| -> bool {
            let Some(slot) = engine.free_slot() else { return false };
            let vx = ((u32::from(target.wrapping_sub(start_x)) << 16) as i32 >> 4) as u32;
            let x = start_x.wrapping_add(nudge as u16);
            engine.slots[slot] = Some(Actor::Icon(Icon {
                side,
                generation,
                frame,
                motion: Motion { x: u32::from(x) << 16, y: 0x88 << 16, vx, ..Motion::default() },
                slide: 0x10,
            }));
            true
        };
        while big != 0 && index < 6 {
            // sub_98B4
            let frame = if big - 1 >= 4 { 6 } else { 5 };
            if icon(self, target_x, frame, 0) {
                big -= 1;
                if frame == 6 {
                    big -= 4;
                }
            }
            target_x = target_x.wrapping_add(step);
            index += 1;
        }
        if index < 6 && small != 0 {
            // sub_98F2
            let frame = (small - 1) as u8;
            let nudge = if start_x == 0xC0 { ICON_NUDGE[frame as usize] } else { 0 };
            icon(self, target_x, frame, nudge);
        }
    }

    fn run_icon(&mut self, mut i: Icon) -> Option<Icon> {
        if i.slide != 0 {
            i.motion.step(0xA2);
            i.slide -= 1;
        }
        (self.icon_gen[i.side as usize] == i.generation).then_some(i)
    }

    /// `sub_9A56`: base points and bonus for this chain step, and the bean counters.
    fn score_step(&self, p: &mut Player) {
        let groups = &p.pop.as_ref().unwrap().groups;
        let beans: usize = groups.iter().map(|g| g.cells.len()).sum();
        let mut colors: Vec<u8> = groups.iter().map(|g| g.color as u8).collect();
        colors.sort_unstable();
        colors.dedup();
        let bonus = CHAIN_BONUS[p.chain.min(8) as usize]
            + COLOR_BONUS[colors.len().min(6)]
            + groups.iter().map(|g| GROUP_BONUS[g.cells.len().min(11)]).sum::<u16>();
        let mut multiplier = 1u32;
        if self.level_mode & 2 != 0 {
            let level = if p.speed_level >= 0x62 { 0x63 } else { p.speed_level };
            multiplier = u32::from(level) + 1;
        }
        p.beans_total = (p.beans_total as usize + beans).min(9999) as u16;
        p.beans_count = (p.beans_count as usize + beans).min(0xFFFF) as u16;
        p.base = (10 * beans as u32 * multiplier).min(0xFFFF) as u16;
        p.bonus = bonus.min(999);
    }

    /// `sub_9CA2`: enough beans popped â†’ next speed step.
    fn speed_up(&mut self, p: &mut Player) -> Result<(), Unported> {
        if p.beans_count < p.threshold {
            return Ok(());
        }
        p.beans_count = 0;
        p.speed_index = p.speed_index.wrapping_add(1);
        if self.level_mode & 3 != 0 {
            return Err(Unported("speed-ups outside scenario mode (unk_9E24, unk_9E7C)"));
        }
        let step = p.speed_index.saturating_sub(self.difficulty) as usize;
        let &(speed, threshold) = SCENARIO_SPEEDS.get(step).ok_or(Unported("speed step past unk_9DC4"))?;
        p.fall_speed = speed;
        p.threshold = threshold;
        Ok(())
    }

    /// `sub_998A`: a finished chain sends acc / 70 garbage beans; the rest stays.
    fn send_garbage(&mut self, slot: usize, p: &mut Player) {
        let acc = p.acc & 0xFFFF;
        let sent = (acc / GARBAGE_POINTS) as u16;
        p.acc = acc % GARBAGE_POINTS;
        let id = p.id;
        let opponent = (0..SLOTS).find(|&s| s != slot && matches!(&self.slots[s], Some(Actor::Player(o)) if o.id != id));
        match opponent {
            Some(o) => {
                let o = self.player_mut(o);
                o.pending_garbage = o.pending_garbage.wrapping_add(sent);
            }
            None => p.garbage_sent = p.garbage_sent.wrapping_add(sent),
        }
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
                let a = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
                self.slots[a] = Some(Actor::Opaque);
                let b = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
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
        let (id, player_flags, fall_speed, mut plan) = {
            let p = self.player_mut(v.player_slot);
            (p.id as usize, p.flags, p.fall_speed, player_plan(p))
        };
        let cpu = self.is_cpu(id as u8);
        match v.state {
            PivotState::Init => {
                v.place();
                v.state = PivotState::Controlled;
            }
            PivotState::Controlled => {
                if player_flags & flag::PIVOT_ACTIVE == 0 {
                    return Err(Unported("pair hidden while in play (loc_4F3A)"));
                }
                let step = if cpu {
                    let before = self.rng;
                    let pad = ai::CpuPad {
                        human: inputs.pads[id],
                        plan: &mut plan,
                        player_id: id as u8,
                        level: self.level,
                        reads: &mut self.cpu_reads[id & 1],
                        pressed: &mut self.cpu_pressed,
                        lean: &mut self.cpu_lean,
                        rng: &mut self.rng,
                    };
                    let step = v.pair.step_pivot(&self.fields[id], pad, ai::CPU_KEYS, fall_speed, inputs.frame_count);
                    self.note_draws(before);
                    set_player_plan(self.player_mut(v.player_slot), plan);
                    step
                } else {
                    v.pair.step_pivot(&self.fields[id], inputs.pads[id], inputs.keys[id], fall_speed, inputs.frame_count)
                };
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
