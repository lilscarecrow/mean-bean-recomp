//! The last stage's intro: Robotnik in his lair (`use_lair_background`).
//!
//! The backdrop is the lair's machine, wall and floor (Enigma maps, the machine with a
//! priority map), its moving parts (`sub_F13A`: fourteen sprites from `word_F456`, one
//! of them blowing bubbles and one launching flying robots, and two tiles of the
//! machine's lights animated by writing their pixels), and Robotnik
//! (`ActRobotnikIntro`): his animation frames are too big for VRAM at once, so each
//! frame's tiles are DMAed to $2000 as it shows (`DPLC_Robotnik_Stage`). His script
//! cues two robots (`sub_A4B4`) and a piece he throws, which then drags him off
//! (`sub_A2C8`).

use super::*;
use crate::mbm::rom;

/// `ActRobotnikIntro` (+$32 its own animation script, +$22 frame time).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobotnikIntro {
    pub started: bool,
    pub leaving: bool,
    pub ptr: u32,
    pub time: u8,
    pub frame: u8,
    pub x: u16,
    pub y: u16,
}

pub const ROBOTNIK_INTRO_CODE: u32 = 0xAAEA;

/// A sprite that only animates (`ActorAnimate`): the lair's machinery (`loc_F296`), or
/// a still one (the bubble and robot launchers' own sprites), or the robots Robotnik
/// calls (`JmpTo_ActorAnimate`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LairSprite {
    pub kind: LairKind,
    pub draw: u8,
    pub mappings: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
    /// +$26: the launchers' timer / counter.
    pub count: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LairKind {
    /// `loc_F296`.
    Machine,
    /// `loc_F29E`: a bubble every 15-30 frames.
    Bubbles,
    /// `loc_F31E`: a flying robot every 128 frames.
    Launcher,
    /// `JmpTo_ActorAnimate` ($A526).
    Robot,
}

impl LairKind {
    pub fn code(self) -> u32 {
        match self {
            LairKind::Machine => 0xF296,
            LairKind::Bubbles => 0xF29E,
            LairKind::Launcher => 0xF31E,
            LairKind::Robot => 0xA526,
        }
    }
}

/// `loc_F300` (a bubble rising, $78 frames) and `loc_F38E` (a flying robot: phases of
/// $96, $10E, then $F0 frames with another script).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LairFlier {
    pub robot: bool,
    pub phase: u8,
    pub draw: u8,
    pub motion: Motion,
    pub anim: Anim,
    pub left: u16,
}

impl LairFlier {
    pub fn code(&self) -> u32 {
        match (self.robot, self.phase) {
            (false, _) => 0xF300,
            (true, 0) => 0xF38E,
            (true, 1) => 0xF3B2,
            (true, _) => 0xF3D8,
        }
    }
}

/// `sub_F1AE`: the machine's lights, two tiles' pixels rewritten every other frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LairLights {
    pub count: u16,
    pub step: u16,
}

pub const LAIR_LIGHTS_CODE: u32 = 0xF1AE;

/// The piece Robotnik throws (`sub_A316`) and what pulls him away (`sub_A404`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThrownPiece {
    pub parent: usize,
    pub phase: ThrowPhase,
    pub delay: u16,
    pub draw: u8,
    pub motion: Motion,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThrowPhase {
    /// $A316: sets a 12-frame delay.
    Start,
    /// $A31E: the delay (`ActorBookmark` counts it down).
    Wait12,
    /// $A322: flying out with Robotnik riding it; at x $120 he's cued and 8 frames pass.
    Fly,
    /// $A368.
    Wait8,
    /// $A36C: spawns his tug, turns back, then $27 frames.
    Turn,
    /// $A3C2.
    Wait27,
    /// $A3C6: a sound.
    Sound,
    /// $A3D4: falls away until x $A8.
    Fall,
    /// `sub_A404`: pulls Robotnik (until x $C0).
    Tug,
}

impl ThrownPiece {
    pub fn code(&self) -> u32 {
        match self.phase {
            ThrowPhase::Start => 0xA316,
            ThrowPhase::Wait12 => 0xA31E,
            ThrowPhase::Fly => 0xA322,
            ThrowPhase::Wait8 => 0xA368,
            ThrowPhase::Turn => 0xA36C,
            ThrowPhase::Wait27 => 0xA3C2,
            ThrowPhase::Sound => 0xA3C6,
            ThrowPhase::Fall => 0xA3D4,
            ThrowPhase::Tug => 0xA404,
        }
    }
}

/// `off_AF44`: Robotnik's animations; `DPLC_Robotnik_Stage`: his frames' tiles.
const ROBOTNIK_ANIMS: u32 = 0xAF44;
const ROBOTNIK_FRAMES: u32 = 0xABC8;
const ROBOTNIK_SHIP_ART: u32 = 0x3B620;
/// `sub_F13A`'s tables: codes (`off_F41E`) and places (`word_F456`: x, y, frame, time,
/// script).
const LAIR_PARTS: u32 = 0xF456;
const LAIR_CODES: [LairKind; 14] = [
    LairKind::Bubbles,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Machine,
    LairKind::Launcher,
];
/// `word_F216`..: the lights' pixels (4 words per step).
const LIGHTS: u32 = 0xF216;
/// The lair's maps: machine (with `MapPrio_LairMachine`), wall, floor.
const LAIR_MACHINE: (u32, u16, u32) = (0x6432E, 0x200, 0x6463A);
const LAIR_WALL: (u32, u16) = (0x644B4, 0x6200);
const LAIR_FLOOR: (u32, u16) = (0x645D8, 0x200);
const LAIR_ART: [(u16, u32); 2] = [(0x4000, 0x61BD0), (0x1200, 0x4C432)];

impl Engine {
    /// The lair's versions of the intro's routines.
    pub(super) fn run_lair_routine(&mut self, at: u32) -> bool {
        match at {
            0xA918 => {
                self.long_step();
                self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x3000, art: ROBOTNIK_SHIP_ART } });
                self.intro_cue = 0xFF00;
                if let Some(slot) = self.find_actor_slot() {
                    let r = RobotnikIntro { started: false, leaving: false, ptr: 0, time: 0, frame: 0, x: 0x1C0, y: 0xFF38 };
                    self.slots[slot] = Some(Actor::RobotnikIntro(r));
                }
            }
            0xF5A0 => {
                self.long_step();
                for (vram, art) in LAIR_ART {
                    self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram, art } });
                }
            }
            0xF5F6 => {
                let (map, base, prio) = LAIR_MACHINE;
                self.eni_dec_prio_safe(map, base, 0x27, 0x12, 0xD200, prio);
            }
            0xF640 => {
                self.eni_dec_safe(LAIR_WALL.0, LAIR_WALL.1, 0x27, 0x12, 0xF600);
            }
            0xF680 => {
                self.eni_dec_safe(LAIR_FLOOR.0, LAIR_FLOOR.1, 0x27, 8, 0xDB80);
            }
            0xF6C0 => {}
            0xF8FC => self.spawn_lair_parts(),
            _ => return false,
        }
        true
    }

    /// `sub_F13A`.
    fn spawn_lair_parts(&mut self) {
        for (i, kind) in LAIR_CODES.into_iter().enumerate() {
            let Some(slot) = self.find_actor_slot() else { continue };
            let at = LAIR_PARTS + i as u32 * 10;
            let mut y = rom::word(at + 2);
            if self.use_lair != 0 {
                y = y.wrapping_sub(0xE0);
            }
            let anim = Anim { ptr: rom::long(at + 6), time: rom::byte(at + 5), frame: rom::byte(at + 4) };
            let s = LairSprite { kind, draw: 0x80, mappings: 0x40, anim, x: rom::word(at), y, count: 0 };
            self.slots[slot] = Some(Actor::LairSprite(s));
        }
        if let Some(slot) = self.free_slot() {
            self.slots[slot] = Some(Actor::LairLights(LairLights { count: 0, step: 0 }));
        }
    }

    pub(super) fn run_robotnik_intro(&mut self, slot: usize, mut r: RobotnikIntro) -> Option<RobotnikIntro> {
        if !r.started {
            r.started = true;
            r.ptr = rom::long(ROBOTNIK_ANIMS);
            return Some(r);
        }
        if r.leaving {
            return None;
        }
        if self.vscroll[0] == 0 {
            r.leaving = true;
            return Some(r);
        }
        self.opponent_x = r.x;
        if self.intro_cue & 0xFF00 != 0 {
            self.intro_cue &= 0x00FF;
            let cue = self.intro_cue as u8;
            if cue & 0x80 != 0 {
                self.robotnik_extra(slot, &r, cue & 0x7F);
                return Some(r);
            }
            r.ptr = rom::long(ROBOTNIK_ANIMS + u32::from(cue) * 4);
            r.time = 0;
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

    /// Robotnik's cues: `sub_A4B4` (two robots) and `sub_A2C8` (the thrown piece).
    fn robotnik_extra(&mut self, parent: usize, r: &RobotnikIntro, which: u8) {
        match which {
            9 => {
                for (x, y, frame, script) in [(0x140u16, 0x20u16, 0u8, 0xA52Cu32), (0x170, 0x40, 2, 0xA536)] {
                    let Some(slot) = self.find_actor_slot() else { return };
                    let anim = Anim { ptr: script, time: 0, frame };
                    let s = LairSprite { kind: LairKind::Robot, draw: 0x80, mappings: 0x28, anim, x, y, count: 0 };
                    self.slots[slot] = Some(Actor::LairSprite(s));
                }
            }
            4 => {
                if let Some(slot) = self.find_actor_slot() {
                    let motion = Motion { x: u32::from(r.x) << 16, y: u32::from(r.y) << 16, vx: 0xFFFE_0000, vy: 0x5_0000, ..Motion::default() };
                    let p = ThrownPiece { parent, phase: ThrowPhase::Start, delay: 0, draw: 0xB3, motion };
                    self.slots[slot] = Some(Actor::ThrownPiece(p));
                }
            }
            _ => self.unported_extra = Some(which),
        }
    }

    pub(super) fn run_lair_sprite(&mut self, mut s: LairSprite) -> Option<LairSprite> {
        match s.kind {
            LairKind::Machine | LairKind::Robot => {
                s.anim.step_with(&mut || self.random());
            }
            LairKind::Bubbles => {
                let (left, borrow) = s.count.overflowing_sub(1);
                s.count = left;
                if borrow || left & 0x8000 != 0 {
                    s.count = (self.random() & 0x0F) + 0x0F;
                    if let Some(slot) = self.find_actor_slot() {
                        let motion = Motion { x: u32::from(s.x) << 16, y: u32::from(s.y) << 16, vy: 0xFFFF_8000, ..Motion::default() };
                        let anim = Anim { ptr: 0xF4EA, time: 2, frame: 0x2E };
                        let b = LairFlier { robot: false, phase: 0, draw: 0xB3, motion, anim, left: 0x78 };
                        self.slots[slot] = Some(Actor::LairFlier(b));
                    }
                }
            }
            LairKind::Launcher => {
                s.count = s.count.wrapping_add(1);
                if s.count & 0x7F == 0 {
                    if let Some(slot) = self.find_actor_slot() {
                        let motion = Motion { x: u32::from(s.x) << 16, y: u32::from(s.y) << 16, vx: 0x8000, ..Motion::default() };
                        let mut anim = Anim { ptr: 0xF56E, time: 8, frame: 0x23 };
                        if self.random() & 1 != 0 {
                            anim = Anim { ptr: 0xF57C, time: 8, frame: 0x25 };
                        }
                        let f = LairFlier { robot: true, phase: 0, draw: 0, motion, anim, left: 0x96 };
                        self.slots[slot] = Some(Actor::LairFlier(f));
                    }
                }
            }
        }
        Some(s)
    }

    pub(super) fn run_lair_flier(&mut self, mut f: LairFlier) -> Option<LairFlier> {
        f.draw = if f.robot { 0x83 } else { 0xBF };
        f.motion.step(f.draw);
        f.anim.step_with(&mut || self.random());
        let (left, borrow) = f.left.overflowing_sub(1);
        f.left = left;
        if !borrow && left & 0x8000 == 0 {
            return Some(f);
        }
        if !f.robot {
            return None;
        }
        // loc_F38E's phases: then $10E frames, then the other script for $F0.
        match f.phase {
            0 => {
                f.left = 0x10E;
                f.phase = 1;
            }
            1 => {
                f.anim.ptr = 0xF58A;
                f.left = 0xF0;
                f.phase = 2;
            }
            _ => return None,
        }
        Some(f)
    }

    pub(super) fn run_lair_lights(&mut self, mut l: LairLights) -> Option<LairLights> {
        l.count = l.count.wrapping_add(1);
        if l.count & 1 != 0 {
            return Some(l);
        }
        l.step = l.step.wrapping_add(1);
        let at = LIGHTS + u32::from(l.step & 0x0F) * 8;
        let (a, b, c, d) = (rom::word(at), rom::word(at + 2), rom::word(at + 4), rom::word(at + 6));
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Words { at: 0x5E74, words: vec![a, b, a, b] } });
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Words { at: 0x5E94, words: vec![c, d, c, d] } });
        Some(l)
    }

    pub(super) fn run_thrown_piece(&mut self, mut p: ThrownPiece) -> Option<ThrownPiece> {
        let alive = |e: &Engine, slot: usize| matches!(&e.slots[slot], Some(Actor::RobotnikIntro(r)) if !r.leaving);
        // ActorBookmark after ActorBookmark_SetDelay: the step reruns until the delay
        // runs out, then the next one runs from the next frame.
        let wait = |p: &mut ThrownPiece, next: ThrowPhase| {
            if p.delay != 0 {
                p.delay -= 1;
                if p.delay != 0 {
                    return;
                }
            }
            p.phase = next;
        };
        match p.phase {
            ThrowPhase::Start => {
                p.delay = 0x0C;
                p.phase = ThrowPhase::Wait12;
            }
            ThrowPhase::Wait12 => wait(&mut p, ThrowPhase::Fly),
            ThrowPhase::Fly => {
                if !alive(self, p.parent) {
                    return None;
                }
                p.motion.step(p.draw);
                let vy = p.motion.vy.wrapping_sub(0x1270);
                if vy & 0x8000_0000 == 0 {
                    p.motion.vy = vy;
                }
                let (x, y) = ((p.motion.x >> 16) as u16, (p.motion.y >> 16) as u16);
                if let Some(Actor::RobotnikIntro(r)) = self.slots[p.parent].as_mut() {
                    r.y = y;
                    r.x = x;
                }
                if x as i16 <= 0x120 {
                    self.intro_cue = 0x0101;
                    p.delay = 8;
                    p.phase = ThrowPhase::Wait8;
                }
            }
            ThrowPhase::Wait8 => wait(&mut p, ThrowPhase::Turn),
            ThrowPhase::Turn => {
                if let Some(slot) = self.find_actor_slot() {
                    let motion = Motion {
                        x: p.motion.x & 0xFFFF_0000,
                        y: p.motion.y & 0xFFFF_0000,
                        vx: 0xFFFE_A000,
                        vy: 0xFFFC_0000,
                        ..Motion::default()
                    };
                    let tug = ThrownPiece { parent: p.parent, phase: ThrowPhase::Tug, delay: 0, draw: 0x33, motion };
                    self.slots[slot] = Some(Actor::ThrownPiece(tug));
                }
                p.motion.vx = 0xFFFE_0000;
                p.motion.vy = 0xFFFF_8000;
                p.draw = 0xB3;
                p.delay = 0x27;
                p.phase = ThrowPhase::Wait27;
            }
            ThrowPhase::Wait27 => wait(&mut p, ThrowPhase::Sound),
            ThrowPhase::Sound => p.phase = ThrowPhase::Fall,
            ThrowPhase::Fall => {
                if !alive(self, p.parent) {
                    return None;
                }
                p.motion.step(p.draw);
                p.motion.vy = p.motion.vy.wrapping_sub(0x2000);
                if ((p.motion.x >> 16) as i16) < 0xA8 {
                    return None;
                }
            }
            ThrowPhase::Tug => {
                p.motion.step(p.draw);
                p.motion.vy = p.motion.vy.wrapping_add(0x2000);
                let (x, y) = ((p.motion.x >> 16) as u16, (p.motion.y >> 16) as u16);
                if (x as i16) < 0xC0 {
                    return None;
                }
                if let Some(Actor::RobotnikIntro(r)) = self.slots[p.parent].as_mut() {
                    r.x = x;
                    r.y = y;
                }
            }
        }
        Some(p)
    }
}
