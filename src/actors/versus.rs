//! 1P vs 2P (`BC_VersusMode`, level_mode 1). Each round the game flow draws the win
//! counters (`sub_93B4`) and starts a match (`GenPuyoOrder`, `loc_3C00`); each player
//! first picks a level (`sub_3F94`: the selector, a face and a marker) that sets its
//! fall speed and handicap. The loser's actor runs the end of the round (`loc_7F70`):
//! every other actor goes, the winner's "ALL RIGHT!" and the loser's "OH NO" play out,
//! and either the next round starts or, once a player has won the matches set in the
//! options, the loser may press START to play again before the high score table.

use super::*;
use crate::rom;

/// `unk_947C` / `unk_94AE`: per 20 wins, two rows of five tiles; `byte_FF0128`'s
/// stars (`sub_9406`) for the rest.
const WIN_ROWS: [u32; 2] = [0x947C, 0x94AE];
const WIN_ROWS_AT: [u16; 2] = [0xC61E, 0xC628];
const STARS_AT: [u16; 2] = [0xC91E, 0xC930];
const STARS_STEP: [u16; 2] = [0x282, 0x27E];
const STAR_TILE: u16 = 0x832A;
/// `word_3FE0`: the face's x.
const FACE_X: [u16; 2] = [0xD4, 0x14C];
/// (exercise mode: $B8, levels $14 apart rather than $18)
const FACE_Y: u16 = 0xB0;
/// `word_42C8`: five colors per level (palette line 3, colors 4-8 for P1, 9-13 P2).
const FACE_COLORS: u32 = 0x42C8;
/// `off_1AC82`: the selector's box per side (plane address, width, height, fill, map),
/// and `unk_1AA22` (a level's highlighted rows, $30 bytes per level).
const BOXES: u32 = 0x1AC82;
const LEVEL_ROWS: u32 = 0x1AA22;
/// `unk_9E24`: VS fall rates and speed-up thresholds; `unk_9C04`: handicap garbage.
const VS_SPEEDS: u32 = 0x9E24;
const HANDICAP: u32 = 0x9C04;
/// `word_725A`: the winner's stars' heights.
const VS_STAR_Y: [u16; 4] = [0xA0, 0xB8, 0xB0, 0xA8];
/// The results' art and palette (`sub_72E0`).
const RESULTS_SORCERY: u32 = 0x71D12;
const RESULTS_NEMESIS: u32 = 0x59046;
const OH_NO_PALETTE: u32 = 0x3156;
/// Scripts: `off_757E` (the orbiting beans), `byte_767E`, `unk_7692`, `unk_7832`,
/// `unk_78A4` / `unk_78AE`, `unk_7F62`, `unk_8308`.
const ORBIT_SCRIPTS: u32 = 0x757E;
const CHEER_SCRIPT: u32 = 0x767E;
const GROAN_SCRIPT: u32 = 0x7692;
const LOSER_SCRIPT: u32 = 0x7832;
const TEAR_SCRIPTS: [u32; 2] = [0x78A4, 0x78AE];
const RESULT_SCRIPT: u32 = 0x7F62;
const GAME_OVER_SCRIPT: u32 = 0x8308; // (the continue sign's)
/// `byte_7484`: the blinkers' offsets, then the tears' (x, y).
const BLINK_AT: [(u16, u16); 2] = [(0x2C, 4), (0x52, 2)];
const TEAR_AT: [(u16, u16); 2] = [(0x3C, 2), (0xFFFB, 0x37)];
/// `byte_965C` / `byte_9664`: the countdown's characters (tens and ones in the
/// $FE/$FF places) and its blanks.
const COUNTDOWN_TEXT: [u8; 6] = [0x3C, 0x26, 0x2E, 0x1E, 0xFE, 0xFF];
/// Text entries: `SELECT LEVEL` (3 + side), `GAMES WON` ($0D), the continue prompt
/// ($0A) and `PRESS 2P START` (9).
const SELECT_TEXT: u8 = 3;
const WINS_TEXT: u8 = 0x0D;
const PROMPT_TEXT: u8 = 0x0A;
const PRESS_START_TEXT: u8 = 9;
const OPP_ARMS: u8 = 3;
/// `GetPuyoFieldPos` (sprite space).
const FIELD_POS: [(u16, u16); 2] = [(0x90, 0x90), (0x150, 0x90)];

/// `loc_3FE6`: a player's level selector; its resume points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectState {
    /// $3FE6, $4004: the box opens (frames 7 to 0).
    Start,
    Open,
    /// $4062, $4088: up/down, any of A/B/C/START picks.
    Ready,
    Choose,
    /// $4124, $412E: the pick blinks for $18 frames.
    Picked,
    Blink,
    /// $4184, $418C: the box closes; $41B4: done.
    Close,
    Closing,
    Done,
}

impl SelectState {
    pub const ALL: [SelectState; 9] = [
        SelectState::Start,
        SelectState::Open,
        SelectState::Ready,
        SelectState::Choose,
        SelectState::Picked,
        SelectState::Blink,
        SelectState::Close,
        SelectState::Closing,
        SelectState::Done,
    ];

    pub fn code(self) -> u32 {
        match self {
            SelectState::Start => 0x3FE6,
            SelectState::Open => 0x4004,
            SelectState::Ready => 0x4062,
            SelectState::Choose => 0x4088,
            SelectState::Picked => 0x4124,
            SelectState::Blink => 0x412E,
            SelectState::Close => 0x4184,
            SelectState::Closing => 0x418C,
            SelectState::Done => 0x41B4,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelSelect {
    pub state: SelectState,
    /// +$2E the player, +$2A its side, +$07 ($FF while choosing), +$26 the level,
    /// +$28 a counter.
    pub player: usize,
    pub id: u8,
    pub choosing: u8,
    pub level: u16,
    pub timer: u16,
}

/// `loc_42FA` (a face per level) / `loc_436A` (the marker beside it): after $10
/// frames, they follow the selector's level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelFace {
    /// False: `loc_42FA` / `loc_436A`, not run yet.
    pub started: bool,
    pub ready: bool,
    pub marker: bool,
    /// +$2E the selector, +$32 the face (the marker's), +$2A side, +$24 delay.
    pub select: usize,
    pub face: usize,
    pub id: u8,
    pub delay: u16,
    pub draw: u8,
    pub frame: u8,
    pub x: u16,
    pub y: u16,
    /// +$34 the top level's y, +$28 the level shown.
    pub top: u16,
    pub shown: u8,
}

impl LevelFace {
    pub fn code(&self) -> u32 {
        let base = if self.marker { 0x436A } else { 0x42FA };
        match (self.started, self.ready) {
            (false, _) => base,
            (true, false) => base + 8,
            (true, true) => base + 12,
        }
    }
}

/// The end of a round, run by the loser's actor (`loc_7F70`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VsState {
    /// $7F92: the VS mapping; $7FA0, $7FA4, $7FAC: a few frames.
    Mapping,
    Pause1,
    Pause2,
    Delay,
    /// $7FB0: the results' cast; $7FCC: the winner's tally.
    Cast,
    Tally,
    /// $80DA: the next round after $100 frames (a button skips); $80DE.
    NextWait,
    NextGo,
    /// $7FFA: the match is over: $80 frames, then $7FFE the countdown, $8020 the
    /// blinking prompt, $802A until START or the countdown runs out (with
    /// `wait_to_continue`, not in the ROM: no countdown, until both press START; the
    /// same between rounds).
    OverWait,
    Countdown,
    Prompt,
    Continue,
    /// $809A, $80A2: $80 frames, then ($80A6) the high scores.
    Leave,
    LeaveWait,
    LeaveGo,
}

impl VsState {
    pub const ALL: [VsState; 15] = [
        VsState::Mapping,
        VsState::Pause1,
        VsState::Pause2,
        VsState::Delay,
        VsState::Cast,
        VsState::Tally,
        VsState::NextWait,
        VsState::NextGo,
        VsState::OverWait,
        VsState::Countdown,
        VsState::Prompt,
        VsState::Continue,
        VsState::Leave,
        VsState::LeaveWait,
        VsState::LeaveGo,
    ];

    pub fn code(self) -> u32 {
        match self {
            VsState::Mapping => 0x7F92,
            VsState::Pause1 => 0x7FA0,
            VsState::Pause2 => 0x7FA4,
            VsState::Delay => 0x7FAC,
            VsState::Cast => 0x7FB0,
            VsState::Tally => 0x7FCC,
            VsState::NextWait => 0x80DA,
            VsState::NextGo => 0x80DE,
            VsState::OverWait => 0x7FFA,
            VsState::Countdown => 0x7FFE,
            VsState::Prompt => 0x8020,
            VsState::Continue => 0x802A,
            VsState::Leave => 0x809A,
            VsState::LeaveWait => 0x80A2,
            VsState::LeaveGo => 0x80A6,
        }
    }
}

/// The results' cast (`sub_72E0` and what they spawn).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VsCast {
    /// `loc_75CE`: the winner's character, bobbing while the loser's +$07 has bits 1
    /// and 2; then (`loc_7620`) the `unk_8308` sprite. +$20 its middle, +$36 angle.
    Cheer { stage: u8, player: usize, id: u8, alive: u8, draw: u8, mappings: u8, x: u16, y: u16, mid: u16, angle: u8, anim: Anim },
    /// `sub_7522`: 32 beans orbiting it (`FindActorSlotQuick`).
    Orbit { started: bool, parent: usize, mappings: u8, x: u16, y: u16, cx: u16, cy: u16, angle: u8, radius: u16, speed: u8, draw: u8, anim: Anim },
    /// `loc_7654`: a part following its parent at an offset.
    Part { parent: usize, dx: u16, dy: u16, draw: u8, x: u16, y: u16, anim: Anim },
    /// `loc_76B0`: a blinking eye (+$22 its timer, +$26 its phase).
    Blink { parent: usize, dx: u16, dy: u16, draw: u8, frame: u8, x: u16, y: u16, timer: u8, phase: u8 },
    /// `sub_7752`: the loser's character: sways, then bounces.
    Loser { stage: u8, player: usize, id: u8, alive: u8, draw: u8, motion: Motion, timer: u16, count: u16, delay: u16, anim: Anim },
    /// `loc_7840`: its tears, when its frame $18 is $C frames along (+$2C).
    Tear { parent: usize, dx: u16, dy: u16, draw: u8, x: u16, y: u16, falling: bool, anim: Anim, restart: u32 },
    /// `loc_78E8`: the prompt's blinking sprite.
    Prompt { stage: u8, draw: u8, timer: u16, delay: u16 },
    /// `sub_7F5A`: the results' animation.
    Result { anim: Anim },
    /// `loc_95FE`: the countdown's digits (+$17 tens, +$0F ones).
    Countdown { player: usize, tens: u8, ones: u8 },
}

impl VsCast {
    pub fn code(&self) -> u32 {
        match self {
            VsCast::Cheer { stage: 0, .. } => 0x75CE,
            VsCast::Cheer { stage: 1, .. } => 0x75E6,
            VsCast::Cheer { .. } => 0x764E,
            VsCast::Orbit { started, .. } => if *started { 0x7534 } else { 0x7522 },
            VsCast::Part { .. } => 0x7654,
            VsCast::Blink { .. } => 0x76B0,
            VsCast::Loser { stage, .. } => [0x7752, 0x7786, 0x77CC, 0x77DC, 0x77E0, 0x780E][usize::from(*stage).min(5)],
            VsCast::Tear { .. } => 0x7840,
            VsCast::Prompt { stage, .. } => [0x78E8, 0x78FE, 0x791A, 0x791E][usize::from(*stage).min(3)],
            VsCast::Result { .. } => 0x7F5A,
            VsCast::Countdown { .. } => 0x95FE,
        }
    }
}

impl Engine {
    pub(super) fn run_versus_routine(&mut self, at: u32) -> bool {
        match at {
            0x9308 => self.reset_vs_rounds(),
            0x93B4 => self.draw_win_counters(),
            _ => return false,
        }
        true
    }

    /// `sub_9308`: the matches to win (`game_matches`, at least 1) for each side.
    fn reset_vs_rounds(&mut self) {
        self.level = 4;
        self.opponent = OPP_ARMS;
        let matches = self.options[0].max(1);
        self.vs_left = [matches, matches];
    }

    /// `sub_93B4`: per side, a 2x5 tile block per 20 wins and a star per win left over
    /// (in columns of five), straight into VRAM.
    fn draw_win_counters(&mut self) {
        for side in 0..2 {
            let wins = u16::from(self.vs_wins[side]);
            let (rows, stars) = (wins / 20, wins % 20);
            let table = WIN_ROWS[side] + u32::from(rows) * 10;
            for row in 0..2u16 {
                let at = WIN_ROWS_AT[side].wrapping_add(row * 0x80);
                for i in 0..5u16 {
                    let b = rom::byte(table + u32::from(row * 5 + i));
                    self.plane_writes.push((at.wrapping_add(i * 2), 0x8000 | u16::from(b)));
                }
            }
            let mut at = STARS_AT[side];
            for n in 0..stars {
                self.plane_writes.push((at, STAR_TILE));
                at = at.wrapping_sub(0x80);
                if n % 5 == 4 {
                    at = at.wrapping_add(STARS_STEP[side]);
                }
            }
        }
    }

    /// Not in the ROM (`wait_to_continue`): true once both players have pressed START
    /// since the wait began.
    fn both_started(&mut self, inputs: &Inputs) -> bool {
        let ready = self.continue_ready.get_or_insert([false; 2]);
        for (r, pad) in ready.iter_mut().zip(&inputs.pads) {
            *r |= pad.press & 0x80 != 0;
        }
        if *ready == [true, true] {
            self.continue_ready = None;
            return true;
        }
        false
    }

    /// `sub_3F94` (VS): the selector, its face and marker.
    pub(super) fn spawn_level_select(&mut self, player: usize, id: u8) {
        let Some(select) = self.find_actor_slot() else { return };
        self.slots[select] = Some(Actor::LevelSelect(LevelSelect { state: SelectState::Start, player, id, choosing: 0xFF, level: 5, timer: 0 }));
        let side = usize::from(id & 1);
        // loc_41EC
        let Some(face) = self.free_slot() else { return };
        let frame = if id == 0 { 0 } else { 5 };
        let top = if self.level_mode & 2 != 0 { 0xB8 } else { FACE_Y };
        let f = LevelFace { started: false, ready: false, marker: false, select, face: 0, id, delay: 0, draw: 0, frame, x: FACE_X[side], y: top, top, shown: 0 };
        self.slots[face] = Some(Actor::LevelFace(f));
        self.face_colors(id, 0);
        let Some(marker) = self.free_slot() else { return };
        // (sub_4294 leaves its loop counter, $FFFF, where the y was)
        let m = LevelFace { started: false, ready: false, marker: true, select, face, id: 0, delay: 0, draw: 0, frame: 0, x: FACE_X[side] + 0x10, y: 0x1F, top: 0, shown: 0 };
        self.slots[marker] = Some(Actor::LevelFace(m));
    }

    /// `sub_4294`: a level's face colors into the palette buffer.
    fn face_colors(&mut self, id: u8, level: u8) {
        let first = if id == 0 { 4 } else { 9 };
        for c in 0..5 {
            self.palettes[3][first + c] = rom::word(FACE_COLORS + u32::from(level) * 10 + c as u32 * 2);
        }
    }

    pub(super) fn run_level_select(&mut self, mut s: LevelSelect, inputs: &Inputs) -> Option<LevelSelect> {
        let id = s.id;
        let level_cmd = |level: u16| PanelCommand::VsLevel { player: id, word: level };
        match s.state {
            SelectState::Start => {
                self.queue_panel(PanelCommand::Text { entry: SELECT_TEXT + id, blink: 0, player: 0 });
                s.timer = 7;
                s.state = SelectState::Open;
            }
            SelectState::Open => {
                self.queue_panel(PanelCommand::VsBox { player: id, frame: s.timer as u8 });
                s.timer = s.timer.wrapping_sub(1);
                if s.timer == 0xFFFF {
                    // loc_4022
                    s.level = self.vs_levels[usize::from(id & 1)];
                    self.queue_panel(level_cmd(0x8000 | (s.level & 0xFF)));
                    self.play_sample(0x5C);
                    s.state = SelectState::Ready;
                }
            }
            SelectState::Ready => {
                s.timer = 0x180;
                s.state = SelectState::Choose;
            }
            SelectState::Choose => {
                let press = inputs.pads[usize::from((id ^ self.swap_controls) & 1)].press;
                if press & 0xF0 != 0 {
                    // loc_4112
                    self.play_sample(0x41);
                    s.timer = 0;
                    s.state = SelectState::Picked;
                } else {
                    let step = if press & 1 != 0 && s.level != 0 {
                        Some(if self.level_mode & 2 != 0 { 0xFFFEu16 } else { 0xFFFF })
                    } else if press & 1 == 0 && press & 2 != 0 && s.level < 4 {
                        Some(if self.level_mode & 2 != 0 { 2 } else { 1 })
                    } else {
                        None
                    };
                    if let Some(step) = step {
                        self.queue_panel(level_cmd(s.level));
                        s.level = s.level.wrapping_add(step);
                        self.queue_panel(level_cmd(0x8000 | (s.level & 0xFF)));
                        self.play_sample(0x42);
                    }
                }
            }
            SelectState::Picked => {
                s.timer = 0x18;
                s.state = SelectState::Blink;
            }
            SelectState::Blink => {
                let blink = if s.timer & 2 != 0 { 0x8000 } else { 0 };
                self.queue_panel(level_cmd(s.level | blink));
                s.timer = s.timer.wrapping_sub(1);
                if s.timer == 0 {
                    // loc_4158
                    if let Some(Actor::Player(p)) = &mut self.slots[s.player] {
                        p.speed_level = s.level as u8;
                        p.speed_index = 0;
                    }
                    self.vs_levels[usize::from(id & 1)] = s.level;
                    s.choosing = 0;
                    s.state = SelectState::Close;
                }
            }
            SelectState::Close => {
                s.level = 0;
                s.state = SelectState::Closing;
            }
            SelectState::Closing => {
                self.queue_panel(PanelCommand::VsBox { player: id, frame: s.level as u8 });
                s.level += 1;
                if s.level >= 8 {
                    s.state = SelectState::Done;
                }
            }
            SelectState::Done => {
                self.queue_panel(PanelCommand::VsBox { player: id, frame: 0xFF });
                self.queue_panel(PanelCommand::Text { entry: SELECT_TEXT + id, blink: 0xFF, player: 0 });
                if let Some(Actor::Player(p)) = &mut self.slots[s.player] {
                    p.flags &= !1;
                }
                return None;
            }
        }
        Some(s)
    }

    pub(super) fn run_level_face(&mut self, mut f: LevelFace) -> Option<LevelFace> {
        if !f.started {
            f.started = true;
            f.delay = 0x10;
            return Some(f);
        }
        if !f.ready {
            if wait(&mut f.delay) {
                f.ready = true;
            }
            return Some(f);
        }
        f.draw = 0x80;
        let (choosing, level) = match &self.slots[f.select] {
            Some(Actor::LevelSelect(s)) => (s.choosing, s.level as u8),
            _ => (0, 0),
        };
        if choosing == 0 {
            return None;
        }
        if f.marker {
            let (shown, top) = match &self.slots[f.face] {
                Some(Actor::LevelFace(face)) => (face.shown, face.top),
                _ => (0, 0),
            };
            f.frame = shown;
            f.y = u16::from(shown) * self.level_spacing() + top + 0x20;
            return Some(f);
        }
        if level == f.shown {
            return Some(f);
        }
        f.shown = level;
        f.frame = if f.id == 0 { level } else { level + 5 };
        f.y = u16::from(level) * self.level_spacing() + f.top;
        self.face_colors(f.id, level);
        self.load_palette(3, PaletteLoad::Buffer);
        Some(f)
    }

    /// The faces' spacing per level (`loc_4338`).
    fn level_spacing(&self) -> u16 {
        if self.level_mode & 2 != 0 { 0x14 } else { 0x18 }
    }

    /// `loc_9CF8` (VS: `loc_9D42`): the fall rate and speed-up threshold for the speed
    /// step and the level picked.
    pub(super) fn vs_fall_rate(&mut self, p: &mut Player) {
        let index = (p.speed_index & 7) | ((p.speed_level << 2) & 0x18);
        let at = VS_SPEEDS + u32::from(index) * 4;
        p.fall_speed = rom::word(at);
        p.threshold = rom::word(at + 2);
    }

    /// `sub_9BCE` (VS): the level's handicap garbage.
    pub(super) fn vs_handicap(&mut self, p: &mut Player) {
        p.pending_garbage = rom::word(HANDICAP + u32::from(p.speed_level) * 2);
    }

    /// `loc_7C50`: the loser's field falls away; the winner's actor goes, its wins
    /// count, and stars burst over its field (`loc_71FC`).
    pub(super) fn vs_round_lost(&mut self, p: &mut Player) {
        self.popping[usize::from(p.id & 1)] = true;
        self.player_flags = [0, 0];
        self.delete_player(p.other_slot);
        self.play_sample(0x59);
        self.opponent = OPP_ARMS;
        let winner = usize::from((p.id ^ 1) & 1);
        self.vs_wins[winner] += 1;
        if self.vs_wins[winner] >= 100 {
            self.vs_wins = [0, 0];
            self.vs_wins[winner] = 1;
        }
        let x0 = if p.id == 0 { 0x160 } else { 0xA0 };
        for i in (0..4u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let x = (self.random() & 0x3F) + x0;
            let motion = Motion { x: u32::from(x) << 16, y: u32::from(VS_STAR_Y[usize::from(3 - i)]) << 16, ..Motion::default() };
            let star = Star { state: StarState::VsWait, motion, draw: 0, color: 0x25, frame: i, timer: u16::from(i) << 4, delay: 0 };
            self.slots[slot] = Some(Actor::Star(star));
        }
        self.wipe_field(p);
        p.state = PlayerState::Wipe;
    }

    /// `loc_7F70`: every other actor goes (InitActors: the loser's own actor too, but
    /// for its tag and side).
    pub(super) fn start_vs_results(&mut self, p: &mut Player) {
        let me = self.current_slot;
        for (slot, s) in self.slots.iter_mut().enumerate() {
            if slot != me {
                *s = None;
            }
        }
        self.pal_fade_gen = [0; 4];
        let mut fresh = Player::new(p.id);
        fresh.flags = 0;
        *p = fresh;
        p.state = PlayerState::Vs(VsState::Mapping);
    }

    pub(super) fn run_vs_results(&mut self, p: &mut Player, state: VsState, inputs: &Inputs) -> bool {
        use VsState::*;
        let next = |p: &mut Player, s: VsState| p.state = PlayerState::Vs(s);
        let any_press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0;
        match state {
            Mapping => {
                self.queue_panel(PanelCommand::List(0x17));
                next(p, Pause1);
            }
            Pause1 => next(p, Pause2),
            Pause2 => {
                p.delay = 4;
                next(p, Delay);
            }
            Delay => {
                if wait(&mut p.delay) {
                    next(p, Cast);
                }
            }
            Cast => {
                self.crowding = 2;
                p.flags = 0x0E;
                self.spawn_vs_cast(p);
                self.draw_wins();
                next(p, Tally);
            }
            Tally => {
                // sub_7F24
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::VsCast(VsCast::Result { anim: Anim::new(RESULT_SCRIPT) }));
                }
                let side = usize::from(p.id & 1);
                self.vs_left[side] = self.vs_left[side].wrapping_sub(1);
                if self.vs_left[side] != 0 {
                    p.delay = 0x100;
                    next(p, NextWait);
                } else {
                    self.reset_vs_rounds();
                    p.flags |= 1;
                    self.spawn_continue_sign(p); // sub_8298: GAME OVER
                    p.delay = 0x80;
                    next(p, OverWait);
                }
            }
            NextWait => {
                let go = if self.wait_to_continue { self.both_started(inputs) } else { bookmark_ctrl(&mut p.delay, any_press) };
                if go {
                    next(p, NextGo);
                }
            }
            NextGo => {
                self.bytecode_disabled = false;
                self.bytecode_flag = 0;
                return false;
            }
            OverWait => {
                if bookmark_ctrl(&mut p.delay, any_press) {
                    next(p, Countdown);
                }
            }
            Countdown => {
                p.w28 = 0x280;
                p.flags &= !8;
                // sub_95B0 (none when waiting for START: there's nothing to count)
                if !self.wait_to_continue
                    && let Some(slot) = self.find_actor_slot()
                {
                    let player = self.current_slot;
                    self.slots[slot] = Some(Actor::VsCast(VsCast::Countdown { player, tens: 0, ones: 0 }));
                }
                next(p, Prompt);
            }
            Prompt => {
                // sub_78B8
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::VsCast(VsCast::Prompt { stage: 0, draw: 0, timer: 0, delay: 0 }));
                }
                next(p, Continue);
            }
            Continue => {
                let press = inputs.pads[usize::from((p.id ^ self.swap_controls) & 1)].press;
                let again = if self.wait_to_continue { self.both_started(inputs) } else { press & 0x80 != 0 };
                if again {
                    // loc_80B8: play again.
                    self.play_sample(0x41);
                    self.bytecode_disabled = false;
                    self.bytecode_flag = 0;
                    return false;
                }
                let prompt = PanelCommand::Text { entry: PROMPT_TEXT, blink: 0, player: p.id };
                if self.wait_to_continue {
                    self.queue_panel(prompt);
                    self.queue_panel(PanelCommand::Text { entry: PRESS_START_TEXT, blink: 0x0F, player: p.id });
                } else if p.w28 == 0 {
                    // loc_8084
                    self.queue_panel(prompt);
                    p.flags &= !2;
                    self.crowding = 3;
                    next(p, Leave);
                } else {
                    p.w28 -= 1;
                    if press & 0x70 != 0 {
                        p.w28 &= 0xFFC0;
                    }
                    self.queue_panel(prompt);
                    self.queue_panel(PanelCommand::Text { entry: PRESS_START_TEXT, blink: 0x0F, player: p.id });
                }
            }
            Leave => {
                p.delay = 0x80;
                next(p, LeaveWait);
            }
            LeaveWait => {
                if bookmark_ctrl(&mut p.delay, any_press) {
                    next(p, LeaveGo);
                }
            }
            LeaveGo => {
                self.bytecode_disabled = false;
                self.bytecode_flag = 0xFF;
                return false;
            }
        }
        true
    }

    /// `sub_9794`: each side's wins as two digits, straight into VRAM, then `GAMES WON`.
    fn draw_wins(&mut self) {
        for (side, at) in [0xC320u16, 0xC32C].into_iter().enumerate() {
            let wins = u16::from(self.vs_wins[side]);
            let tens = wins / 10;
            let tens = if tens == 0 { 0 } else { (tens + 1) * 2 };
            let ones = (wins % 10 + 1) * 2;
            for (i, digit) in [tens, ones].into_iter().enumerate() {
                let to = at.wrapping_add(i as u16 * 2);
                let tile = 0xA500 | (digit & 0xFF);
                self.plane_writes.push((to, tile));
                self.plane_writes.push((to.wrapping_add(0x80), 0xA500 | ((digit + 1) & 0xFF)));
            }
        }
        self.queue_panel(PanelCommand::Text { entry: WINS_TEXT, blink: 0, player: 0 });
    }

    /// `sub_72E0`: the results' art and palette, then the winner's and the loser's
    /// characters with their parts.
    fn spawn_vs_cast(&mut self, p: &Player) {
        self.long_step();
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Sorcery { vram: 0x4000, art: RESULTS_SORCERY } });
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x2000, art: RESULTS_NEMESIS } });
        let colors = std::array::from_fn(|c| rom::word(OH_NO_PALETTE + c as u32 * 2));
        self.load_palette(3, PaletteLoad::Colors(colors));
        let me = self.current_slot;
        let put = |e: &mut Engine, cast: VsCast| -> Option<usize> {
            let slot = e.find_actor_slot()?;
            e.slots[slot] = Some(Actor::VsCast(cast));
            Some(slot)
        };
        let anim = |script: u32, frame: u8| Anim { ptr: script, time: 0, frame };
        let cheer = VsCast::Cheer { stage: 0, player: me, id: p.id ^ 1, alive: 0xFF, draw: 0x80, mappings: 0x27, x: 0, y: 0, mid: 0, angle: 0, anim: Anim::new(0) };
        let Some(cheer) = put(self, cheer) else { return };
        let part = VsCast::Part { parent: cheer, dx: 0x4B, dy: 0x1F, draw: 0x80, x: 0, y: 0, anim: anim(CHEER_SCRIPT, 0x12) };
        if put(self, part).is_none() {
            return;
        }
        for (dx, dy) in BLINK_AT {
            let timer = (self.random() & 0x3F) as u8;
            let blink = VsCast::Blink { parent: cheer, dx, dy, draw: 0, frame: 0x15, x: 0, y: 0, timer, phase: 0 };
            if put(self, blink).is_none() {
                return;
            }
        }
        let loser = VsCast::Loser { stage: 0, player: me, id: p.id, alive: 0xFF, draw: 0x93, motion: Motion::default(), timer: 0, count: 0, delay: 0, anim: anim(LOSER_SCRIPT, 1) };
        let Some(loser) = put(self, loser) else { return };
        let part = VsCast::Part { parent: loser, dx: 0x4C, dy: 0x2F, draw: 0x80, x: 0, y: 0, anim: anim(GROAN_SCRIPT, 0x19) };
        if put(self, part).is_none() {
            return;
        }
        for ((dx, dy), script) in TEAR_AT.into_iter().zip(TEAR_SCRIPTS) {
            let tear = VsCast::Tear { parent: loser, dx, dy, draw: 0, x: 0, y: 0, falling: false, anim: anim(script, 0x1E), restart: script };
            if put(self, tear).is_none() {
                return;
            }
        }
    }

    /// The loser's actor's +$07 (0 once it's gone).
    fn vs_flags(&self, slot: usize) -> u8 {
        match &self.slots[slot] {
            Some(Actor::Player(p)) => p.flags,
            _ => 0,
        }
    }

    /// A cast member's +$07, position and animation (for the parts following it).
    fn cast_of(&self, slot: usize) -> Option<(u8, u16, u16, u8, u8)> {
        match &self.slots[slot] {
            Some(Actor::VsCast(VsCast::Cheer { alive, x, y, anim, .. })) => Some((*alive, *x, *y, anim.frame, anim.time)),
            Some(Actor::VsCast(VsCast::Loser { alive, motion, anim, .. })) => Some((*alive, (motion.x >> 16) as u16, (motion.y >> 16) as u16, anim.frame, anim.time)),
            _ => None,
        }
    }

    pub(super) fn run_vs_cast(&mut self, cast: VsCast) -> Option<VsCast> {
        let mut random = || 0;
        match cast {
            VsCast::Cheer { stage, player, id, alive, mut draw, mut mappings, mut x, mut y, mut mid, mut angle, mut anim } => {
                let flags = self.vs_flags(player);
                let mut stage = stage;
                let mut alive = alive;
                match stage {
                    0 => {
                        let (fx, fy) = FIELD_POS[usize::from((id ^ self.swap_controls) & 1)];
                        x = fx;
                        mid = fy + 0x48;
                        self.spawn_orbits(x, mid, mappings);
                        stage = 1;
                    }
                    1 => {
                        if flags & 4 == 0 {
                            return None;
                        }
                        if flags & 2 != 0 {
                            y = ((super::sin(angle | 0x80, 0x1000) >> 16) as u16).wrapping_add(mid);
                            angle = angle.wrapping_add(4);
                        } else {
                            // loc_7620
                            alive = 0;
                            draw = 0x80;
                            mappings = 0x29;
                            anim = Anim::new(GAME_OVER_SCRIPT);
                            y = mid.wrapping_sub(8);
                            x = x.wrapping_add(0x30);
                            stage = 2;
                        }
                    }
                    _ => {
                        anim.step_with(&mut random);
                    }
                }
                Some(VsCast::Cheer { stage, player, id, alive, draw, mappings, x, y, mid, angle, anim })
            }
            VsCast::Orbit { started, parent, mappings, mut x, mut y, cx, cy, mut angle, radius, speed, mut draw, mut anim } => {
                if started {
                    let alive = self.cast_of(parent).map_or(0, |c| c.0);
                    if alive == 0 {
                        return None;
                    }
                }
                anim.step_with(&mut random);
                if !started {
                    draw = 0x80;
                    return Some(VsCast::Orbit { started: true, parent, mappings, x, y, cx, cy, angle, radius, speed, draw, anim });
                }
                x = ((super::sin(angle, radius) >> 16) as u16).wrapping_add(cx);
                y = ((super::cos(angle.wrapping_add(0x28), radius >> 1) >> 16) as u16).wrapping_add(cy);
                angle = angle.wrapping_add(speed);
                Some(VsCast::Orbit { started, parent, mappings, x, y, cx, cy, angle, radius, speed, draw, anim })
            }
            VsCast::Part { parent, dx, dy, draw, x: _, y: _, mut anim } => {
                let (alive, px, py, _, _) = self.cast_of(parent).unwrap_or_default();
                if alive == 0 {
                    return None;
                }
                let x = px.wrapping_add(dx);
                let y = py.wrapping_add(dy);
                anim.step_with(&mut random);
                Some(VsCast::Part { parent, dx, dy, draw, x, y, anim })
            }
            VsCast::Blink { parent, dx, dy, mut draw, mut frame, x: _, y: _, mut timer, mut phase } => {
                let (alive, px, py, _, _) = self.cast_of(parent).unwrap_or_default();
                if alive == 0 {
                    return None;
                }
                let x = px.wrapping_add(dx);
                let y = py.wrapping_add(dy);
                if timer != 0 {
                    timer -= 1;
                } else {
                    match phase {
                        0 => {
                            draw = 0x80;
                            frame = 0x15;
                            phase = 1;
                            timer = 5;
                        }
                        1 => {
                            frame = 0x16;
                            phase = 2;
                            timer = 5;
                        }
                        _ => {
                            phase = 0;
                            draw = 0;
                            timer = ((self.random() & 0x3F) as u8).wrapping_add(0x44);
                        }
                    }
                }
                Some(VsCast::Blink { parent, dx, dy, draw, frame, x, y, timer, phase })
            }
            VsCast::Loser { stage, player, id, alive, mut draw, mut motion, mut timer, mut count, mut delay, mut anim } => {
                let mut stage = stage;
                // sub_772C: it goes once the loser's +$07 loses bit 1 or 2.
                let check = |e: &Engine| e.vs_flags(player) & 6 == 6;
                match stage {
                    0 => {
                        let (fx, fy) = FIELD_POS[usize::from((id ^ self.swap_controls) & 1)];
                        motion.x = u32::from(fx.wrapping_sub(0x10)) << 16;
                        motion.y = u32::from(fy.wrapping_sub(0x30)) << 16;
                        motion.vx = 0x1_0000;
                        motion.vy = 0x8000;
                        count = 6;
                        timer = 0x20;
                        stage = 1;
                    }
                    1 => {
                        if !check(self) {
                            return None;
                        }
                        if timer != 0 {
                            motion.step(draw);
                            anim.step_with(&mut random);
                            timer -= 1;
                        } else if count != 0 {
                            count -= 1;
                            motion.vx = motion.vx.wrapping_neg();
                            timer = 0x20;
                        } else {
                            motion.vx = motion.vx.wrapping_neg();
                            delay = 0x10;
                            stage = 2;
                        }
                    }
                    2 => {
                        motion.step(draw);
                        anim.step_with(&mut random);
                        if wait(&mut delay) {
                            stage = 3;
                        }
                    }
                    3 => stage = 4,
                    4 => {
                        if !check(self) {
                            return None;
                        }
                        let py = (motion.y >> 16) as u16;
                        if py >= 0x90 {
                            motion.y = u32::from(py - 1) << 16 | (motion.y & 0xFFFF);
                        } else {
                            // loc_77F4
                            draw = 0x85;
                            motion.vy = 0;
                            motion.ay = 0x2000;
                            motion.ty = 0xFFFF;
                            stage = 5;
                        }
                    }
                    _ => {
                        if !check(self) {
                            return None;
                        }
                        motion.step(draw);
                        anim.step_with(&mut random);
                        if (motion.y >> 16) as u16 >= 0xD0 {
                            motion.y = 0xD0 << 16 | (motion.y & 0xFFFF);
                            stage = 4;
                        }
                    }
                }
                Some(VsCast::Loser { stage, player, id, alive, draw, motion, timer, count, delay, anim })
            }
            VsCast::Tear { parent, dx, dy, mut draw, x: _, y: _, mut falling, mut anim, restart } => {
                let (alive, px, py, frame, time) = self.cast_of(parent).unwrap_or_default();
                if alive == 0 {
                    return None;
                }
                let x = px.wrapping_add(dx);
                let y = py.wrapping_add(dy);
                if !falling {
                    if frame == 0x18 && time == 0x0C {
                        falling = true;
                    }
                } else {
                    draw = 0x80;
                    if anim.step_with(&mut random) {
                        draw = 0;
                        falling = false;
                        anim.ptr = restart;
                    }
                }
                Some(VsCast::Tear { parent, dx, dy, draw, x, y, falling, anim, restart })
            }
            VsCast::Prompt { stage, mut draw, mut timer, mut delay } => {
                let mut stage = stage;
                if stage == 1 {
                    if timer != 0 {
                        timer -= 1;
                        return Some(VsCast::Prompt { stage, draw, timer, delay });
                    }
                    // loc_790C
                    draw = 0x80;
                    delay = 0x30;
                    stage = 2;
                } else if stage == 2 {
                    if wait(&mut delay) {
                        stage = 3;
                    }
                } else {
                    if stage == 3 {
                        draw = 0;
                    }
                    // loc_78E8
                    timer = self.random_below(0x40) + 0x20;
                    stage = 1;
                }
                Some(VsCast::Prompt { stage, draw, timer, delay })
            }
            VsCast::Result { mut anim } => {
                anim.step_with(&mut random);
                Some(VsCast::Result { anim })
            }
            VsCast::Countdown { player, mut tens, mut ones } => {
                let t = match &self.slots[player] {
                    Some(Actor::Player(p)) => p.w28,
                    _ => 0,
                };
                if t == 0 {
                    self.countdown_text(&[0; 6], 0, 0);
                    return None;
                }
                if t & 0x3F != 0 {
                    return Some(VsCast::Countdown { player, tens, ones });
                }
                let n = t >> 6;
                tens = if n / 10 == 0 { 0 } else { ((n / 10 + 1) * 2) as u8 };
                ones = ((n % 10 + 1) * 2) as u8;
                self.countdown_text(&COUNTDOWN_TEXT, tens, ones);
                self.play_sample(0x69);
                Some(VsCast::Countdown { player, tens, ones })
            }
        }
    }

    /// `sub_7498`: 32 beans around the winner's character.
    fn spawn_orbits(&mut self, x: u16, mid: u16, mappings: u8) {
        let parent = self.current_slot;
        for i in (0..0x20u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let angle = (i << 4) | ((i >> 1) & 8);
            let radius = (u16::from(i & 0x10) << 8) + 0x2000;
            let script = rom::long(ORBIT_SCRIPTS + u32::from((i << 2) & 0x0C));
            let speed = if i < 0x10 { 0xFF } else { 1 };
            let orbit = VsCast::Orbit { started: false, parent, mappings, x: 0, y: 0, cx: x + 0x30, cy: mid + 0x18, angle, radius, speed, draw: 0, anim: Anim::new(script) };
            self.slots[slot] = Some(Actor::VsCast(orbit));
        }
    }

    /// `sub_9728`: the countdown's characters as tile pairs ($A5xx, top and bottom).
    fn countdown_text(&mut self, text: &[u8; 6], tens: u8, ones: u8) {
        let mut at = 0xCCA2u16;
        for &b in text {
            let c = match b {
                0xFF => ones,
                0xFE => tens,
                c => c,
            };
            let tile = 0xA500 | u16::from(c);
            self.plane_writes.push((at, tile));
            self.plane_writes.push((at.wrapping_add(0x80), 0xA500 | u16::from(c.wrapping_add(1))));
            at = at.wrapping_add(2);
        }
    }

    /// `SpecPlane83`: a frame (0 open .. 7 nearly closed, $FF gone) of a selector's box.
    pub(super) fn draw_vs_box(&mut self, player: u8, frame: u8) {
        let d = rom::long(BOXES + u32::from((self.level_mode & 2) | (player & 1)) * 4);
        let (base, width, height, fill, map) = (rom::word(d), rom::word(d + 2), rom::word(d + 4), rom::word(d + 6), rom::long(d + 8));
        let row = self.plane_row();
        let line = |e: &mut Engine, at: u16, words: &[u16]| {
            for (i, &w) in words.iter().enumerate() {
                e.plane_writes.push((at.wrapping_add(i as u16 * 2), w));
            }
        };
        let blank = vec![fill; usize::from(width) + 2];
        if frame & 0x80 != 0 {
            let at = base.wrapping_add((height / 2 + 1) * row);
            line(self, at, &blank);
            line(self, at.wrapping_add(row), &blank);
            return;
        }
        let k = u16::from(frame);
        let mut at = base.wrapping_add(k * row);
        line(self, at, &blank);
        at = at.wrapping_add(row);
        let edge = |l: u16, m: u16, r: u16| {
            let mut v = vec![l];
            v.extend(std::iter::repeat(m).take(usize::from(width)));
            v.push(r);
            v
        };
        line(self, at, &edge(0xC4D8, 0xC4D9, 0xC4DA));
        at = at.wrapping_add(row);
        let mut from = map + u32::from(k * width * 2);
        for _ in 0..height.saturating_sub(k * 2) {
            let mut v = vec![0xC4DB];
            for _ in 0..width {
                v.push(rom::word(from));
                from += 2;
            }
            v.push(0xC4DC);
            line(self, at, &v);
            at = at.wrapping_add(row);
        }
        line(self, at, &edge(0xC4DD, 0xC4DE, 0xC4DF));
        at = at.wrapping_add(row);
        line(self, at, &blank);
    }

    /// `SpecPlane88`: a level's rows in a selector's box, highlighted ($8000) or not.
    pub(super) fn draw_vs_level(&mut self, player: u8, word: u16) {
        let d = rom::long(BOXES + u32::from(player) * 4);
        let base = rom::word(d);
        let level = word & 0xFF;
        let row = self.plane_row();
        let (mut at, rows) = if self.level_mode & 2 != 0 {
            ((((level >> 1) << 2) + 3) * row + 2 + base, 4)
        } else {
            ((level * 3 + 2) * row + 2 + base, 2)
        };
        let mut from = LEVEL_ROWS + u32::from(level) * 0x30;
        for _ in 0..rows {
            for i in 0..6u16 {
                self.plane_writes.push((at.wrapping_add(i * 2), rom::word(from) | (word & 0x8000)));
                from += 2;
            }
            at = at.wrapping_add(row);
        }
    }
}
