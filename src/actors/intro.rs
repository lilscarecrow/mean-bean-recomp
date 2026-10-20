//! The stage intro cutscene (`BC_LevelIntro`) and the level transition after it.
//!
//! The intro: the backdrop (`LoadLevelIntroArt`, four Enigma maps, the clouds scrolling
//! line by line), Robotnik's ship on the mountain, the opponent (`ActOpponentIntro`:
//! animations the script picks, and a few opponents' extras), and the script
//! (`ActLevelIntro`, one script per opponent in `off_113FE`): text boxes typed a
//! character every other frame, waits and animation cues; a button ends it.
//!
//! The transition (`SetupLevelTransition`): once the bytecode clears
//! `level_transition_flag`, planes A and B scroll down into place over $38 frames (the
//! match board comes in over the backdrop); plane A's actor then lets the bytecode on.
//!
//! Not ported yet: the last stage's lair intro (`use_lair_background`).

use super::*;
use crate::assets::nemesis;
use crate::rom;

/// `ActLevelIntro`: the script (+$32), the wait (+$26), the box (+$07 shown, +$12 its
/// plane address, +$0E/+$10 size in characters and lines, +$0A/+$0C the cursor).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroScript {
    pub state: IntroScriptState,
    pub ptr: u32,
    pub wait: u16,
    pub shown: bool,
    pub at: u16,
    pub width: u16,
    pub lines: u16,
    pub col: u16,
    pub line: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntroScriptState {
    /// $10FDC: loads the font.
    Start,
    /// $1104C: the script.
    Run,
    /// $110CC: ended (box closed); the bytecode goes on next frame.
    Ending,
}

impl IntroScriptState {
    pub fn code(self) -> u32 {
        match self {
            IntroScriptState::Start => 0x10FDC,
            IntroScriptState::Run => 0x1104C,
            IntroScriptState::Ending => 0x110CC,
        }
    }
}

/// A corner of the text box (`loc_113D8`, mappings $20) or its tail (`sub_113B8`,
/// following the opponent: `word_FF1994` + `byte_113EC`): gone once the box closes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroBoxPart {
    pub parent: usize,
    pub tail: bool,
    pub frame: u8,
    pub x: u16,
    pub y: u16,
}

/// `ActOpponentIntro`: the opponent, animated by the script's cues (`word_FF1990`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpponentIntro {
    /// False until the backdrop has scrolled away (then +$07 clears and it ends).
    pub leaving: bool,
    pub mappings: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
}

pub const OPPONENT_INTRO_CODE: u32 = 0xAA6A;

/// An opponent's extra in its intro (`off_A15E`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntroExtra {
    /// `loc_A758` (Coconuts): his lights flash, colors 10/12 of line 3.
    Flash { parent: usize, count: u8 },
    /// `loc_A560`: the opponent sways sideways (+$36 angle, +$1E centre).
    Sway { parent: usize, angle: u8, centre: u16 },
    /// `loc_A7C4` (Sir Ffuzzy): line 3 color 14 cycles (`word_A810`) every 5 frames.
    Cycle { parent: usize, wait: u8, step: u16 },
    /// `sub_A85C` (Sir Ffuzzy): sparks from the right, every 7-14 frames.
    Sparks { wait: u16 },
    /// `loc_A5F6` (Grounder): moves him round a loop (+$36 angle about +$1E/+$20; his
    /// frame by the angle), stopping at the top until the script has said its piece
    /// (+$28 stopped, +$2A a cue came meanwhile).
    Orbit { parent: usize, angle: u8, stopped: u8, cued: u8, cx: u16, cy: u16 },
    /// `loc_A692`: one of three pieces (mappings $0E) trailing him, blinking (+$26).
    OrbitPiece { parent: usize, orbit: usize, index: u8, angle: u8, count: u8, draw: u8, frame: u8, x: u16, y: u16, cx: u16, cy: u16 },
    /// `sub_A8E2`: one spark (mappings $12) flying left (draw flags $B3 until it first
    /// runs, then $BF).
    Spark { draw: u8, motion: Motion, anim: Anim, life: u16 },
}

/// `loc_B1AC`: the clouds, four bands of plane B scrolling at their own speeds, line by
/// line (positions +$0A.. and speeds +$0E.., 16.16, a long apart).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroClouds {
    pub pos: [u32; 4],
    pub speed: [u32; 4],
}

pub const INTRO_CLOUDS_CODE: u32 = 0xB1AC;

/// `loc_F942`: Robotnik's ship on the mountain (mappings $40); the ending's ships
/// (`loc_F994`, `loc_F9E0`) are the same at other places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntroShip {
    pub code: u32,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
}

pub const INTRO_SHIP_CODE: u32 = 0xF942;

/// `ActLevelTransitionFG` / `BG`: wait for `level_transition_flag` to clear, then
/// scroll plane A / B down to 0 over $38 frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub plane_b: bool,
    pub started: bool,
    pub y: u32,
    pub speed: u32,
    pub left: u16,
}

impl Transition {
    pub fn code(&self) -> u32 {
        match (self.plane_b, self.started) {
            (false, false) => 0xAFB2,
            (false, true) => 0xAFD6,
            (true, false) => 0xB01A,
            (true, true) => 0xB03E,
        }
    }
}

/// `off_113FE` (scripts by opponent), `byte_11258` (font: character -> tile),
/// `off_AC48` (animations by opponent), `word_A9E2` (where each opponent stands),
/// `OpponentIntroArt` (by opponent, to $8000), `byte_113EC` (the tail's offset).
const SCRIPTS: u32 = 0x113FE;
const FONT: u32 = 0x11258;
const FONT_ART: u32 = 0x4F9FC;
const ANIMS: u32 = 0xAC48;
const PLACES: u32 = 0xA9E2;
const INTRO_ART: u32 = 0xAA26;
const ARMS_ART: u32 = 0x549F8;
const TAIL_OFFSETS: u32 = 0x113EC;
/// `LoadLevelIntroArt`'s backdrop and its maps (`sub_F5F6` .. `sub_F6C0`): (map, tile
/// word, width - 1, height - 1, plane address).
const BACKDROP_ART: u32 = 0x5FC00;
const BACKDROP_MAPS: [(u32, u16, u16, u16, u16); 4] = [
    (0x6199E, 0x8100, 0x27, 0x1B, 0xD200),
    (0x61A8A, 0x2100, 0x1F, 0x0A, 0xF600),
    (0x61A8A, 0x2100, 0x1F, 0x0A, 0xF640),
    (0x61B22, 0x2100, 0x27, 0x08, 0xFB80),
];
/// `Pal_IntroSky1` (table entry) and `OpponentIntroPals` (by opponent).
const SKY_PALETTE: u8 = 5;
const INTRO_PALETTES: u32 = 0xB140;
/// `LairBackgroundFlags` by level.
const LAIR_FLAGS: [u8; 18] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 0];
/// `unk_B220`.. : lines per cloud band (and on into `unk_B224`).
const CLOUD_BANDS: u32 = 0xB220;
/// Grounder's loop: his frame by angle (`unk_A68A`), the pieces' (`unk_A72C`, `unk_A744`).
const ORBIT_FRAMES: u32 = 0xA68A;
const PIECE_FRAMES: [[u8; 4]; 2] = [[8, 9, 0x0A, 0], [8, 8, 9, 0]];
/// `word_A810`: Sir Ffuzzy's cycling color.
const FFUZZY_CYCLE: u32 = 0xA810;
const OPP_ROBOTNIK: u8 = 12;

impl Engine {
    /// The `BRUN` routines of `BC_LevelIntro` and `BC_LevelTransition`.
    pub(super) fn run_intro_routine(&mut self, at: u32) -> Result<bool, Unported> {
        // Robotnik (by opponent: the lair flag is set later), then the lair's versions.
        if at == 0xA918 && self.opponent == OPP_ROBOTNIK && self.run_lair_routine(at) {
            return Ok(true);
        }
        if at != 0xA918 && self.use_lair != 0 && self.run_lair_routine(at) {
            return Ok(true);
        }
        match at {
            // LoadOpponentIntro
            0xA918 => {
                self.long_step();
                let opp = self.opponent;
                if opp == 3 {
                    self.nemesis(0x6000, ARMS_ART);
                }
                self.nemesis(0x8000, rom::long(INTRO_ART + u32::from(opp) * 4));
                self.intro_cue = 0xFF00;
                if let Some(slot) = self.find_actor_slot() {
                    let place = PLACES + u32::from(opp) * 4;
                    let a = OpponentIntro {
                        leaving: false,
                        mappings: opp + 9,
                        anim: Anim::default(),
                        x: rom::word(place),
                        y: rom::word(place + 2),
                    };
                    self.slots[slot] = Some(Actor::OpponentIntro(a));
                }
            }
            // SetupLevelTransition
            0xAF84 => {
                for (plane_b, _) in [(false, 0), (true, 0)] {
                    if let Some(slot) = self.find_actor_slot() {
                        let t = Transition { plane_b, started: false, y: 0, speed: 0, left: 0 };
                        self.slots[slot] = Some(Actor::Transition(t));
                    }
                }
                self.vscroll[0] = 0xFF20;
                self.vscroll[1] = 0xFF60;
                self.level_transition = 0xFFFF;
            }
            // LoadLevelIntro (the backdrop's planes cleared, then the clouds)
            0xB08C => {
                if let Some(slot) = self.find_actor_slot() {
                    let s = IntroScript {
                        state: IntroScriptState::Start,
                        ptr: 0,
                        wait: 0,
                        shown: false,
                        at: 0,
                        width: 0,
                        lines: 0,
                        col: 0,
                        line: 0,
                    };
                    self.slots[slot] = Some(Actor::IntroScript(s));
                }
                let lair = LAIR_FLAGS[usize::from(self.level).min(17)];
                self.use_lair = lair;
                self.bytecode_flag = lair >> 1;
                self.queue_panel(PanelCommand::List(0));
                self.spawn_clouds();
            }
            // PlayLevelIntroMusic
            0x2692 => {}
            // LoadLevelIntroArt, then the backdrop's maps a frame each
            0xF5A0 => {
                self.long_step();
                self.nemesis(0x2000, BACKDROP_ART);
            }
            0xF5F6 | 0xF640 | 0xF680 | 0xF6C0 => {
                let i = match at {
                    0xF5F6 => 0,
                    0xF640 => 1,
                    0xF680 => 2,
                    _ => 3,
                };
                let (map, base, w, h, to) = BACKDROP_MAPS[i];
                self.eni_dec_safe(map, base, w, h, to);
            }
            // loc_B0D6: the sky's and the opponent's palettes fade in
            0xB0D6 => {
                let sky = table_palette(SKY_PALETTE);
                self.fade_palette(1, 0, 8, &sky);
                let own = table_palette(rom::byte(INTRO_PALETTES + u32::from(self.opponent)));
                self.fade_palette(3, 0, 8, &own);
            }
            // loc_F8FC: Robotnik's ship
            0xF8FC => {
                if let Some(slot) = self.find_actor_slot() {
                    let mut anim = Anim::new(0xF94A);
                    anim.time = 0x20;
                    anim.frame = 0x39;
                    self.slots[slot] = Some(Actor::IntroShip(IntroShip { code: INTRO_SHIP_CODE, anim, x: 0xF1, y: 0xFFE0 }));
                }
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn nemesis(&mut self, vram: u16, art: u32) {
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram, art } });
    }

    /// `Bytecode_FadePal_Intro` (`BFADEI`): outside the lair, the first palette, unless
    /// its offset is 0.
    pub(super) fn fade_intro(&mut self, word: u16, speed: u8, lair: u8) {
        if self.use_lair != 0 {
            // .RobotniksLair: the operand's last byte is the palette.
            let colors = table_palette(lair);
            self.fade_palette(word & 3, speed, 8, &colors);
            return;
        }
        if word >> 3 != 0 {
            let at = 0x27D6 + u32::from(word >> 3);
            let colors: [u16; 16] = std::array::from_fn(|c| rom::word(at + c as u32 * 2));
            self.fade_palette(word & 3, speed, 8, &colors);
        }
    }

    pub(super) fn run_intro_script(&mut self, slot: usize, mut s: IntroScript, inputs: &Inputs) -> Option<IntroScript> {
        match s.state {
            IntroScriptState::Start => {
                self.load_intro_font();
                s.ptr = rom::long(SCRIPTS + u32::from(self.opponent) * 4);
                s.state = IntroScriptState::Run;
                return Some(s);
            }
            IntroScriptState::Ending => {
                // (the game's prologue, opponent $10, tells its controller by clearing it)
                if self.opponent == 0x10 {
                    self.opponent = 0;
                } else {
                    self.bytecode_disabled = false;
                }
                return None;
            }
            IntroScriptState::Run => {}
        }
        if self.input_lock == 0 && inputs.pads[0].press & 0xF0 != 0 {
            self.end_intro(&mut s);
            return Some(s);
        }
        if s.wait != 0 {
            s.wait -= 1;
            return Some(s);
        }
        let b = rom::byte(s.ptr);
        s.ptr += 1;
        if b & 0x80 == 0 {
            self.intro_char(&mut s, b);
            return Some(s);
        }
        let arg = rom::byte(s.ptr);
        s.ptr += 1;
        match b & 0x7F {
            0 => self.end_intro(&mut s),
            // loc_1112A: open a box.
            1 => {
                s.width = u16::from(arg & 0x1F);
                s.lines = u16::from((arg >> 5) & 7);
                s.at = rom::word(s.ptr);
                s.ptr += 2;
                s.col = 0;
                s.line = 0;
                let kind = if self.opponent == OPP_ROBOTNIK || self.opponent == 0x10 { 0x9F } else { 0x8C };
                self.queue_panel(PanelCommand::IntroBox { kind, size: arg, at: s.at });
                self.spawn_box_parts(slot, &s);
                s.shown = true;
            }
            // loc_1118E: close it.
            2 => {
                s.ptr -= 1;
                self.close_box(&mut s);
            }
            3 => s.wait = u16::from(arg) * 10,
            4 => self.intro_198e = 0xFF00 | u16::from(arg),
            5 => self.intro_cue = 0xFF00 | u16::from(arg),
            // sub_110F0: a new line.
            6 => {
                s.ptr -= 1;
                new_line(&mut s);
            }
            // sub_1110E: clear the box.
            7 => {
                s.ptr -= 1;
                s.col = 0;
                s.line = 0;
                self.queue_panel(PanelCommand::IntroBox { kind: 0x8E, size: box_size(&s), at: s.at });
            }
            // sub_111C4: a space.
            9 => {
                s.ptr -= 1;
                advance(&mut s);
            }
            // PlaySound_ChkPCM
            10 => {}
            _ => {}
        }
        Some(s)
    }

    /// `loc_110C2`: close the box if open; next frame the bytecode goes on.
    fn end_intro(&mut self, s: &mut IntroScript) {
        self.close_box(s);
        s.state = IntroScriptState::Ending;
    }

    /// `sub_11192`.
    fn close_box(&mut self, s: &mut IntroScript) {
        if s.shown {
            s.shown = false;
            self.queue_panel(PanelCommand::IntroBox { kind: 0x8D, size: box_size(s), at: s.at });
        }
    }

    /// `loc_111EE`: a character straight into plane A, then a frame's wait.
    fn intro_char(&mut self, s: &mut IntroScript, ch: u8) {
        let tile = 0xE000 | u16::from(rom::byte(FONT + u32::from(ch)));
        let to = s.at.wrapping_add((s.col + 1) * 2).wrapping_add((s.line * 2 + 1) << 7);
        self.plane_writes.push((to, tile));
        s.wait = 1;
        self.text_tick = self.text_tick.wrapping_add(1); // (a typing sound every other one)
        advance(s);
    }

    /// `sub_112D8`: the box's corners (sprites) and the tail under it.
    fn spawn_box_parts(&mut self, parent: usize, s: &IntroScript) {
        let x0 = ((s.at >> 1) & 0x3F) * 8 + 0x80;
        let y0 = ((s.at >> 7) & 0x3F) * 8 + 0x80;
        let x1 = x0 + (s.width + 1) * 8;
        let y1 = y0 + s.lines * 16;
        let extra = if self.opponent == OPP_ROBOTNIK || self.opponent == 0x10 { 5 } else { 0 };
        for corner in (0..4u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let x = if corner & 1 != 0 { x1 } else { x0 };
            let y = if corner & 2 != 0 { y1 } else { y0 };
            let part = IntroBoxPart { parent, tail: false, frame: corner + extra, x, y };
            self.slots[slot] = Some(Actor::IntroBoxPart(part));
        }
        if let Some(slot) = self.free_slot() {
            let part = IntroBoxPart { parent, tail: true, frame: 4 + extra, x: 0, y: y1 };
            self.slots[slot] = Some(Actor::IntroBoxPart(part));
        }
    }

    pub(super) fn run_intro_box_part(&mut self, mut p: IntroBoxPart) -> Option<IntroBoxPart> {
        if p.tail {
            p.x = self.opponent_x.wrapping_add(u16::from(rom::byte(TAIL_OFFSETS + u32::from(self.opponent))));
        }
        match &self.slots[p.parent] {
            Some(Actor::IntroScript(s)) if s.shown => Some(p),
            // (the script runs as Opaque while it runs: then it's still open)
            Some(Actor::Opaque) => Some(p),
            _ => None,
        }
    }

    pub(super) fn run_opponent_intro(&mut self, slot: usize, mut o: OpponentIntro) -> Option<OpponentIntro> {
        if o.leaving {
            return None;
        }
        if self.vscroll[0] == 0 {
            o.leaving = true;
            return Some(o);
        }
        self.opponent_x = o.x;
        if self.intro_cue & 0xFF00 != 0 {
            self.intro_cue &= 0x00FF;
            let cue = self.intro_cue as u8;
            if cue & 0x80 != 0 {
                self.intro_extra(slot, &o, cue & 0x7F);
                return Some(o);
            }
            let list = rom::long(ANIMS + u32::from(self.opponent) * 4);
            o.anim.ptr = rom::long(list + u32::from(cue) * 4);
            o.anim.time = 0;
        }
        o.anim.step_with(&mut || self.random());
        Some(o)
    }

    /// `off_A15E`: the extras the scenario's scripts cue.
    fn intro_extra(&mut self, parent: usize, o: &OpponentIntro, which: u8) {
        let extra = match which {
            0 => IntroExtra::Flash { parent, count: 0 },
            2 => IntroExtra::Sway { parent, angle: 0, centre: o.x },
            5 => IntroExtra::Cycle { parent, wait: 4, step: 0 },
            // loc_A58A: the loop, then its pieces (2, 1, 0).
            1 => {
                let Some(orbit) = self.find_actor_slot() else { return };
                self.slots[orbit] = Some(Actor::IntroExtra(IntroExtra::Orbit { parent, angle: 0, stopped: 0, cued: 0, cx: o.x, cy: o.y }));
                for index in (0..3u8).rev() {
                    let Some(slot) = self.find_actor_slot() else { continue };
                    let piece = IntroExtra::OrbitPiece {
                        parent,
                        orbit,
                        index,
                        angle: index * 8 + 0x10,
                        count: 0,
                        draw: 0x80,
                        frame: 0x0A,
                        x: 0,
                        y: 0,
                        cx: o.x,
                        cy: o.y,
                    };
                    self.slots[slot] = Some(Actor::IntroExtra(piece));
                }
                return;
            }
            10 => {
                if let Some(slot) = self.free_slot() {
                    self.slots[slot] = Some(Actor::IntroExtra(IntroExtra::Sparks { wait: 4 }));
                }
                return;
            }
            _ => {
                self.unported_extra = Some(which);
                return;
            }
        };
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::IntroExtra(extra));
        }
    }

    fn opponent_alive(&self, parent: usize) -> bool {
        matches!(&self.slots[parent], Some(Actor::OpponentIntro(o)) if !o.leaving)
    }

    pub(super) fn run_intro_extra(&mut self, e: IntroExtra) -> Option<IntroExtra> {
        match e {
            IntroExtra::Flash { parent, count } => {
                if !self.opponent_alive(parent) {
                    return None;
                }
                let count = count.wrapping_add(1);
                if count & 7 == 0 {
                    let (c10, c12) = if count & 8 != 0 { (0xAEE, 0xAEE) } else { (0x06C, 0x28E) };
                    self.palettes[3][10] = c10;
                    self.palettes[3][12] = c12;
                    self.load_palette(3, PaletteLoad::Buffer);
                }
                Some(IntroExtra::Flash { parent, count })
            }
            IntroExtra::Sway { parent, angle, centre } => {
                if !self.opponent_alive(parent) {
                    return None;
                }
                let x = ((sin(angle, 0x1C00) >> 16) as u16).wrapping_add(centre);
                if let Some(Actor::OpponentIntro(o)) = self.slots[parent].as_mut() {
                    o.x = x;
                }
                Some(IntroExtra::Sway { parent, angle: angle.wrapping_sub(1), centre })
            }
            IntroExtra::Cycle { parent, wait, step } => {
                if !self.opponent_alive(parent) {
                    return None;
                }
                if wait != 0 {
                    return Some(IntroExtra::Cycle { parent, wait: wait - 1, step });
                }
                let mut step = step + 1;
                if step as u8 == 0x1B {
                    step = 0;
                }
                self.palettes[3][14] = rom::word(FFUZZY_CYCLE + u32::from(step) * 2);
                self.load_palette(3, PaletteLoad::Buffer);
                Some(IntroExtra::Cycle { parent, wait: 4, step })
            }
            IntroExtra::Sparks { wait } => {
                let (left, borrow) = wait.overflowing_sub(1);
                if !borrow && left & 0x8000 == 0 {
                    return Some(IntroExtra::Sparks { wait: left });
                }
                if let Some(slot) = self.find_actor_slot() {
                    let y = (self.random() & 0x3F) + 0x38;
                    // Random leaves the old seed's high word in d0's top half, which the
                    // swap brings down: (r & 7) << 16 | that, halved, + 2, negated.
                    let old = self.rng.seed;
                    let r = u32::from(self.random() & 7);
                    let d0 = (r << 16) | (old >> 16);
                    let vx = (d0 >> 1).wrapping_add(0x2_0000).wrapping_neg();
                    let motion = Motion { x: 0x1C0 << 16, y: u32::from(y) << 16, vx, ay: 0x400, ty: 0x10, ..Motion::default() };
                    let mut anim = Anim::new(0xA90A);
                    anim.frame = 3;
                    self.slots[slot] = Some(Actor::IntroExtra(IntroExtra::Spark { draw: 0xB3, motion, anim, life: 0xC0 }));
                }
                let wait = (self.random() & 7) + 7;
                Some(IntroExtra::Sparks { wait })
            }
            IntroExtra::Orbit { parent, angle, mut stopped, mut cued, cx, cy } => {
                if !self.opponent_alive(parent) {
                    return None;
                }
                if angle == 0x80 {
                    if stopped == 0 {
                        stopped = 1;
                    } else if self.intro_cue & 0xFF != 0 {
                        cued = 1;
                    } else if cued != 0 {
                        return Some(IntroExtra::Orbit { parent, angle: angle.wrapping_sub(4), stopped: 0, cued: 0, cx, cy });
                    }
                    return Some(IntroExtra::Orbit { parent, angle, stopped, cued, cx, cy });
                }
                let x = ((sin(angle, 0x3800) >> 16) as u16).wrapping_add(cx);
                let y = ((sin(angle >> 1, 0x800) >> 16) as u16).wrapping_add(cy);
                let angle = angle.wrapping_sub(4);
                if let Some(Actor::OpponentIntro(o)) = self.slots[parent].as_mut() {
                    o.x = x;
                    o.y = y;
                    o.anim.frame = rom::byte(ORBIT_FRAMES + u32::from(angle >> 5));
                }
                Some(IntroExtra::Orbit { parent, angle, stopped, cued, cx, cy })
            }
            IntroExtra::OrbitPiece { parent, orbit, index, mut angle, mut count, mut draw, mut frame, mut x, mut y, cx, cy } => {
                if !self.opponent_alive(parent) {
                    return None;
                }
                if matches!(&self.slots[orbit], Some(Actor::IntroExtra(IntroExtra::Orbit { stopped, .. })) if *stopped != 0) {
                    return Some(IntroExtra::OrbitPiece { parent, orbit, index, angle, count, draw, frame, x, y, cx, cy });
                }
                x = ((sin(angle, 0x3800) >> 16) as u16).wrapping_add(cx).wrapping_add(0x10);
                y = ((sin(angle >> 1, 0x800) >> 16) as u16).wrapping_add(cy).wrapping_add(0x20);
                angle = angle.wrapping_sub(4);
                count = count.wrapping_add(1);
                match count & 3 {
                    0 | 2 => draw = 0,
                    1 => {
                        draw = 0x80;
                        frame = PIECE_FRAMES[0][usize::from(index & 3)];
                    }
                    _ => {
                        draw = 0x80;
                        frame = PIECE_FRAMES[1][usize::from(index & 3)];
                    }
                }
                Some(IntroExtra::OrbitPiece { parent, orbit, index, angle, count, draw, frame, x, y, cx, cy })
            }
            IntroExtra::Spark { mut motion, mut anim, life, .. } => {
                let draw = 0xBF;
                motion.step(draw);
                anim.step_with(&mut || self.random());
                let x = (motion.x >> 16) as u16;
                if (x as i16) < 0x60 {
                    return None;
                }
                let (life, borrow) = life.overflowing_sub(1);
                if borrow || life & 0x8000 != 0 {
                    return None;
                }
                Some(IntroExtra::Spark { draw, motion, anim, life })
            }
        }
    }

    /// `sub_10E5C`: the font, decompressed to RAM with every pixel's bit 0 set, DMAed
    /// to $0000.
    pub(super) fn load_intro_font(&mut self) {
        self.long_step();
        self.dma_queue.push(VdpOp::Font { vram: 0, art: FONT_ART });
        // It's decompressed into hblank_buffer_1, whose first word the game over wave
        // later counts on from.
        if let Some(Ok((font, _))) = rom::get().map(|r| nemesis::decompress(&r[FONT_ART as usize..])) {
            self.hblank_count = u16::from_be_bytes([font[0], font[1]]) | 0x1111;
        }
    }

    /// `loc_B152`: the clouds (not in the lair); `dword_FF112C` keeps their slot.
    pub(super) fn spawn_clouds(&mut self) {
        if self.use_lair != 0 {
            return;
        }
        self.word_1126 &= 0xFF;
        self.vdp_regs[0x0B] |= 3;
        if let Some(slot) = self.find_actor_slot() {
            let c = IntroClouds { pos: [0; 4], speed: [0xFFFF_0000, 0xFFFF_4000, 0xFFFF_8000, 0xFFFF_C000] };
            self.slots[slot] = Some(Actor::IntroClouds(c));
            self.clouds_slot = slot;
        }
    }

    pub(super) fn run_intro_clouds(&mut self, mut c: IntroClouds) -> Option<IntroClouds> {
        if self.word_1126 >> 8 != 0 {
            return Some(c);
        }
        for i in 0..4 {
            c.pos[i] = c.pos[i].wrapping_add(c.speed[i]);
        }
        let down = self.vscroll[1].wrapping_sub(0xFF60);
        if down >= 0x58 {
            return None;
        }
        // Lines from 87 up: those the backdrop has scrolled past are cleared, then the
        // bands' plane B scroll (plane A 0).
        let mut line: i32 = 0x15C / 4;
        for _ in 0..down {
            self.hscroll[line as usize * 2] = 0;
            self.hscroll[line as usize * 2 + 1] = 0;
            line -= 1;
        }
        let mut band = 0u32;
        loop {
            let count = u32::from(rom::byte(CLOUD_BANDS + band)) + 1;
            let scroll = (c.pos[3 - (band as usize).min(3)] >> 16) as u16;
            for _ in 0..count {
                if line < 0 {
                    return Some(c);
                }
                self.hscroll[line as usize * 2] = 0;
                self.hscroll[line as usize * 2 + 1] = scroll;
                line -= 1;
            }
            band += 1;
        }
    }

    pub(super) fn run_transition(&mut self, mut t: Transition) -> Option<Transition> {
        if !t.started {
            if self.level_transition != 0 {
                return Some(t);
            }
            t.started = true;
            t.y = if t.plane_b { 0xFF60_0000 } else { 0xFF20_0000 };
            t.speed = if t.plane_b { 0x2_DB6D } else { 0x4_0000 };
            t.left = 0x38;
            return Some(t);
        }
        let stop = |e: &mut Engine, t: &Transition| {
            if t.plane_b {
                e.vscroll[1] = 0;
                e.vdp_regs[0x0B] &= 0xFC;
                e.hscroll[1] = 0;
            } else {
                e.vscroll[0] = 0;
                e.bytecode_disabled = false;
            }
            None
        };
        if self.level < 3 {
            return stop(self, &t);
        }
        t.y = t.y.wrapping_add(t.speed);
        self.vscroll[usize::from(t.plane_b)] = (t.y >> 16) as u16;
        t.left -= 1;
        if t.left == 0 {
            return stop(self, &t);
        }
        Some(t)
    }
}

/// The script's box size byte (lines << 5 | width), as `sub_111B0` builds it.
fn box_size(s: &IntroScript) -> u8 {
    ((s.lines << 5) | s.width) as u8
}

/// `loc_110F4`: back to the line's start, the next line (wrapping to the first).
fn new_line(s: &mut IntroScript) {
    s.col = 0;
    s.line += 1;
    if s.line >= s.lines {
        s.line = 0;
    }
}

/// `loc_11246`.
fn advance(s: &mut IntroScript) {
    s.col += 1;
    if s.col >= s.width {
        new_line(s);
    }
}
