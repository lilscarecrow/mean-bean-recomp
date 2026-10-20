//! Game over (`BC_GameOver`): the robots' backdrop (an Enigma map with a priority map)
//! and the light over it, "GAME OVER" flying in letter by letter and bobbing, Robotnik
//! (his frames DMAed like in his lair, `sub_EE10`), and the continue countdown
//! (`sub_DAC8`): a digit from 9 down every $50 frames (A/B/C skip ahead), START
//! continues (`bytecode_flag` 0, back to `BC_Scenario`), running out leaves the flag
//! $FF (the high scores). While it counts, plane B's top rows wave (`hblank_buffer_1`),
//! a band lower down scrolls left, and the text's last color cycles.

use super::*;
use crate::motion;
use crate::rom;

/// `sub_DAC8` (+$26 frames to the next digit, +$28 the digit, +$2A the color cycle).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameOverTimer {
    pub done: bool,
    pub wait: u16,
    pub count: u16,
    pub color: u16,
}

pub const GAME_OVER_TIMER_CODE: u32 = 0xDAC8;

/// `loc_DC44`: a letter of "GAME OVER" (frame 0-7).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameOverLetter {
    pub phase: LetterPhase,
    pub frame: u8,
    /// x, y and (+$1E) the letter's place in the word, then its flight there.
    pub motion: Motion,
    /// +$26: frames to wait / to fly.
    pub count: u16,
    /// +$36: the angle; +$38: the spiral's radius.
    pub angle: u8,
    pub radius: u16,
    /// +$32: y without the wobble.
    pub y: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LetterPhase {
    /// $DC44: spiralling in.
    Spiral,
    /// $DCAC: waiting its turn (frame x 8).
    Wait,
    /// $DCCC: flying to its place, wobbling.
    Fly,
    /// $DD06: bobbing.
    Bob,
}

impl LetterPhase {
    pub fn code(self) -> u32 {
        match self {
            LetterPhase::Spiral => 0xDC44,
            LetterPhase::Wait => 0xDCAC,
            LetterPhase::Fly => 0xDCCC,
            LetterPhase::Bob => 0xDD06,
        }
    }
}

/// `sub_EE10`: Robotnik in the game over and the ending (+$32 script, +$22 frame time;
/// his frames DMAed from `DPLC_Robotnik_Opening`). `x` is the long at +$0A (the
/// ending's ship carries him).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CutsceneRobotnik {
    pub started: bool,
    pub ptr: u32,
    pub time: u8,
    pub frame: u8,
    pub mappings: u8,
    pub x: u32,
    pub y: u16,
    /// +$06: the ending's piece hides him once off screen.
    pub draw: u8,
}

pub const CUTSCENE_ROBOTNIK_CODE: u32 = 0xEE16;
pub const GAME_OVER_LETTER_MAPPINGS: u8 = 0x1F;

/// `MapEni_GameOverRobots` (+ `byte_6A144`, its priority bits) and `MapEni_GameOverLight`.
const ROBOTS_MAP: u32 = 0x69FBE;
const ROBOTS_PRIORITY: u32 = 0x6A144;
const LIGHT_MAP: u32 = 0x6A0AA;
/// `unk_EF8E` (his script), `DPLC_Robotnik_Opening`, `unk_A19A` (the laugh, cue 8).
const ROBOTNIK_SCRIPT: u32 = 0xEF8E;
const ROBOTNIK_FRAMES: u32 = 0xEEC2;
const ROBOTNIK_LAUGH: u32 = 0xA19A;
/// `word_DC34`: the letters' places; `unk_BC96`: the cycling color.
const LETTER_X: u32 = 0xDC34;
const CYCLE_COLORS: u32 = 0xBC96;

impl Engine {
    pub(super) fn run_game_over_routine(&mut self, at: u32) -> bool {
        match at {
            // sub_F71E: the robots (EniDecPrioMap).
            0xF71E => {
                self.eni_dec_prio_safe(ROBOTS_MAP, 0x2A0, 0x27, 0x1B, 0xC000, ROBOTS_PRIORITY);
            }
            // sub_F740: the light.
            0xF740 => {
                self.eni_dec_safe(LIGHT_MAP, 0x22A0, 0x27, 0x0E, 0xE000);
            }
            0xDA90 => self.start_game_over(),
            _ => return false,
        }
        true
    }

    /// `sub_DA90`.
    fn start_game_over(&mut self) {
        self.band_scroll(Some(0x80));
        for i in (0..8u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let motion = Motion { tx: rom::word(LETTER_X + u32::from(i) * 2), ..Motion::default() };
            let letter = GameOverLetter { phase: LetterPhase::Spiral, frame: i, motion, count: 0, angle: i << 5, radius: 0xA0, y: 0 };
            self.slots[slot] = Some(Actor::GameOverLetter(letter));
        }
        let Some(slot) = self.find_actor_slot() else { return };
        self.slots[slot] = Some(Actor::GameOverTimer(GameOverTimer { done: false, wait: 0, count: 0x0A, color: 0 }));
        self.bytecode_flag = 0xFF;
        self.vdp_regs[0x0B] |= 3;
        // sub_ED9C
        self.intro_cue = 0;
        if let Some(slot) = self.find_actor_slot() {
            let r = CutsceneRobotnik { started: false, ptr: ROBOTNIK_SCRIPT, time: 0, frame: 0, mappings: 0x1B, x: 0xE8 << 16, y: 0x90, draw: 0x80 };
            self.slots[slot] = Some(Actor::CutsceneRobotnik(r));
        }
    }

    /// `sub_DBAC` (`set`) / `loc_DBB8`: plane B's lines $C0-$CF.
    fn band_scroll(&mut self, set: Option<u16>) {
        const AT: usize = 0x302 / 2;
        let v = set.unwrap_or_else(|| self.hscroll[AT].wrapping_sub(2));
        for i in 0..0x10 {
            self.hscroll[AT + i * 2] = v;
            self.hscroll[AT + i * 2 + 1] = 0;
        }
    }

    pub(super) fn run_game_over_timer(&mut self, mut t: GameOverTimer, inputs: &Inputs) -> Option<GameOverTimer> {
        if t.done {
            return Some(t);
        }
        let press = inputs.pads[0].press;
        if press & 0x80 != 0 {
            // loc_DB4A: continue.
            self.bytecode_flag = 0;
            self.bytecode_disabled = false;
            t.done = true;
            return Some(t);
        }
        if press & 0x70 != 0 || t.wait == 0 {
            t.count = t.count.wrapping_sub(1);
            if t.count == 0xFFFF {
                self.bytecode_disabled = false;
                t.done = true;
                return Some(t);
            }
            self.queue_panel(PanelCommand::Countdown(t.count as u8));
            t.wait = 0x50;
            if t.count == 1 {
                self.intro_cue = (self.intro_cue & 0xFF00) | 0x88;
            }
        }
        t.wait = t.wait.wrapping_sub(1);
        self.wave_scroll();
        self.band_scroll(None);
        if t.wait & 3 == 0 {
            // loc_DBD2: sub_BC82, then the color into line 3's last entry.
            t.color += 1;
            if t.color >= 0x15 {
                t.color = 0;
            }
            self.palettes[3][15] = rom::word(CYCLE_COLORS + u32::from(t.color) * 2);
            self.load_palette(3, PaletteLoad::Buffer);
        }
        Some(t)
    }

    /// `sub_DB66`: plane B's top $70 lines sheared by a growing sine.
    fn wave_scroll(&mut self) {
        self.hblank_count = self.hblank_count.wrapping_add(1);
        let step = motion::sin(self.hblank_count as u8, 0xB4);
        let mut v = 0u32;
        for i in 0..0x70 {
            self.hscroll[1 + i * 2] = (v >> 16) as u16;
            self.hscroll[2 + i * 2] = 0;
            v = v.wrapping_add(step);
        }
    }

    pub(super) fn run_game_over_letter(&mut self, mut l: GameOverLetter) -> Option<GameOverLetter> {
        let hi = |v: u32| (v >> 16) as u16;
        match l.phase {
            LetterPhase::Spiral => {
                let x = (motion::sin(l.angle, l.radius) as i32 >> 8) as u16;
                l.motion.x = (l.motion.x & 0xFFFF) | u32::from(x.wrapping_add(0x120)) << 16;
                let y = (motion::cos(l.angle.wrapping_add(0x10), l.radius) as i32 >> 8) as u16;
                l.motion.y = (l.motion.y & 0xFFFF) | u32::from(y.wrapping_add(0xF0)) << 16;
                l.angle = l.angle.wrapping_add(2);
                l.radius = l.radius.wrapping_sub(1);
                if l.radius == 0xFFFF {
                    // loc_DC7E
                    l.count = u16::from(l.frame) << 3;
                    l.angle = 0;
                    let d = u32::from(l.motion.tx.wrapping_sub(0x120)) << 16;
                    l.motion.vx = (d as i32 >> 7) as u32;
                    l.motion.vy = 0xFFFF_9000;
                    l.phase = LetterPhase::Wait;
                }
            }
            LetterPhase::Wait => {
                if l.count != 0 {
                    l.count -= 1;
                } else {
                    // loc_DCBA
                    l.count = 0x80;
                    l.y = l.motion.y;
                    l.phase = LetterPhase::Fly;
                }
            }
            LetterPhase::Fly => {
                l.motion.y = l.y;
                l.motion.step(0x83);
                l.y = l.motion.y;
                l.count -= 1;
                if l.count == 0 {
                    l.phase = LetterPhase::Bob;
                } else {
                    let wobble = hi(motion::sin(l.count as u8 | 0x80, 0x7800));
                    l.motion.y = (l.motion.y & 0xFFFF) | u32::from(hi(l.motion.y).wrapping_add(wobble)) << 16;
                }
            }
            LetterPhase::Bob => {
                let bob = hi(motion::sin(l.angle | 0x80, 0x1800));
                l.motion.y = (l.motion.y & 0xFFFF) | u32::from(bob.wrapping_add(0xB8)) << 16;
                l.angle = l.angle.wrapping_add(2);
            }
        }
        Some(l)
    }

    pub(super) fn run_cutscene_robotnik(&mut self, mut r: CutsceneRobotnik) -> Option<CutsceneRobotnik> {
        if !r.started {
            r.started = true;
            return Some(r);
        }
        let cue = self.intro_cue as u8;
        if cue & 0x80 != 0 {
            // off_A15E: the prologue's robots (loc_A442) and their dissolving
            // (sub_A1C2 / sub_A1EA), the laugh (sub_A18A).
            match cue & 0x7F {
                3 => {
                    for (x, y, frame, script) in [(0x138u16, 0xF8u16, 0u8, 0xA52Cu32), (0x168, 0x118, 2, 0xA536)] {
                        let Some(slot) = self.find_actor_slot() else { break };
                        let anim = Anim { ptr: script, time: 0, frame };
                        let s = LairSprite { kind: LairKind::Robot, draw: 0x80, mappings: 0x28, anim, x, y, count: 0 };
                        self.slots[slot] = Some(Actor::LairSprite(s));
                    }
                }
                6 => self.spawn_dissolve(false),
                7 => self.spawn_dissolve(true),
                8 => {
                    r.ptr = ROBOTNIK_LAUGH;
                    r.time = 0;
                }
                other => self.unported_extra = Some(other),
            }
            self.intro_cue &= 0xFF00;
        }
        if r.time != 0 {
            r.time -= 1;
            return Some(r);
        }
        let mut at = r.ptr;
        match rom::byte(at) {
            0xFE => return Some(r),
            0xFF => at = rom::long(at + 2),
            _ => {}
        }
        r.time = rom::byte(at);
        r.frame = rom::byte(at + 1);
        r.ptr = at + 2;
        let tiles = rom::long(ROBOTNIK_FRAMES + u32::from(r.frame) * 4);
        self.dma_queue.push(VdpOp::RomCopy { vram: 0x2000, from: tiles, len: 0xB00 });
        Some(r)
    }
}
