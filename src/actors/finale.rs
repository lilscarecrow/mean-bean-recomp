//! The ending (`BC_Ending`): Robotnik beaten in his wrecked lair (`sub_C972`), the
//! credits' role call, and the epilogue, then the high score table. START skips a part
//! (`loc_10486`: `bytecode_flag` 2).
//!
//! The defeat: Robotnik stands in the wreck while parts of it smoulder and explode; when
//! his script runs out his ship (`sub_CAFC`) lifts him to the right, a piece of the
//! machine falls after it (`loc_CBBC`) and drags him off screen.

use super::*;
use crate::rom;

/// `loc_CB52`: sound $6A three times, $82, $82 and $C8 frames apart, then $91.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefeatSounds {
    pub stage: u8,
    pub count: u16,
}

/// `sub_CAFC`: Robotnik's ship, waiting for his script to end, then flying him right.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefeatShip {
    pub flying: bool,
    pub parent: usize,
    pub motion: Motion,
}

/// `loc_CBBC`: the piece that falls after him and drags him away.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefeatPiece {
    pub phase: PiecePhase,
    pub parent: usize,
    pub draw: u8,
    pub motion: Motion,
    /// +$2A: frames before it falls; +$28 / +$26: the drag's speed table step.
    pub delay: u16,
    pub tick: u8,
    pub step: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PiecePhase {
    /// $CBBC
    Wait,
    /// $CBD0: steering toward Robotnik.
    Fall,
    /// $CC00: on him, until the ship has him at x $119.
    Hold,
    /// $CC14: dragging him off.
    Drag,
}

impl PiecePhase {
    pub fn code(self) -> u32 {
        match self {
            PiecePhase::Wait => 0xCBBC,
            PiecePhase::Fall => 0xCBD0,
            PiecePhase::Hold => 0xCC00,
            PiecePhase::Drag => 0xCC14,
        }
    }
}

/// `loc_CCA6`: one of the wreck's eight smouldering parts (mappings $31), starting its
/// loop after a random wait; `loc_CCDE` / `loc_CCCA`: an explosion at random places.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DefeatPart {
    pub phase: u8,
    pub draw: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
    /// +$26 (a byte).
    pub wait: u8,
    pub explosion: bool,
}

impl DefeatPart {
    pub fn code(&self) -> u32 {
        match (self.explosion, self.phase) {
            (false, 0) => 0xCCA6,
            (false, 1) => 0xCCB2,
            (false, _) => 0xCCC4,
            (true, 0) => 0xCCCA,
            (true, _) => 0xCCDE,
        }
    }
}

/// `loc_10486`: a button skips the current part.
pub const SKIP_CODE: u32 = 0x10486;

/// `loc_FA04`: a sparkle by the ship every 8 frames (+$26 the count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SparkleSpawner {
    pub count: u16,
}

pub const SPARKLE_SPAWNER_CODE: u32 = 0xFA04;

/// `loc_FA6E`: one sparkle, a frame of `unk_FAA0` every 4 (+$26 the step, +$28 the tick).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShipSparkle {
    pub frame: u8,
    pub x: u16,
    pub y: u16,
    pub step: u16,
    pub tick: u16,
}

pub const SHIP_SPARKLE_CODE: u32 = 0xFA6E;
const SPARKLE_FRAMES: u32 = 0xFAA0;

/// `unk_CD32`: the parts (x, y, script); `unk_CD24`: the explosion; `byte_CC86`: the
/// drag's x steps; `unk_EFC6` / `unk_EFF2`: Robotnik's scripts.
const PARTS: u32 = 0xCD32;
const EXPLOSION: u32 = 0xCD24;
const DRAG_STEPS: u32 = 0xCC86;
const ROBOTNIK_DEFEATED: u32 = 0xEFC6;
const ROBOTNIK_CARRIED: u32 = 0xEFF2;
const SHIP_ART: u32 = 0x3B620;
/// The wrecked lair's maps (machine with the machine's priority bits, wall, floor).
const WRECK_MACHINE: u32 = 0x6622E;
const WRECK_WALL: u32 = 0x66348;
const WRECK_FLOOR: u32 = 0x6646C;
const MACHINE_PRIORITY: u32 = 0x6463A;

impl Engine {
    pub(super) fn run_finale_routine(&mut self, at: u32) -> bool {
        match at {
            // sub_23536: the save's checksum (nothing on screen).
            0x23536 => {}
            // sub_F8F4
            0xF8F4 => self.queue_panel(PanelCommand::List(0)),
            0xF840 => self.wreck_machine(0xC000),
            0xF832 => self.wreck_machine(0xD200),
            0xF886 => {
                self.eni_dec_safe(WRECK_WALL, 0x6200, 0x27, 0x12, 0xE000);
            }
            0xF878 => {
                self.eni_dec_safe(WRECK_WALL, 0x6200, 0x27, 0x12, 0xF600);
            }
            0xF8C4 => {
                self.eni_dec_safe(WRECK_FLOOR, 0x200, 0x27, 8, 0xE980);
            }
            0xF8B6 => {
                self.eni_dec_safe(WRECK_FLOOR, 0x200, 0x27, 8, 0xDB80);
            }
            0xC972 => self.start_defeat(),
            0x10E5C => self.load_intro_font(),
            // loc_F958 / loc_F9A4: the ships.
            0xF958 | 0xF9A4 => {
                if let Some(slot) = self.find_actor_slot() {
                    let (code, script, frame, time, x, y) = if at == 0xF958 {
                        (0xF994, 0xF99C, 0x43, 0x20, 0xF2, 0xFFE0)
                    } else {
                        (0xF9E0, 0xF9E8, 0x44, 3, 0xE2, 0xFFC6)
                    };
                    let anim = Anim { ptr: script, time, frame };
                    self.slots[slot] = Some(Actor::IntroShip(IntroShip { code, anim, x, y }));
                }
            }
            0xF9F8 => {
                if let Some(slot) = self.free_slot() {
                    self.slots[slot] = Some(Actor::SparkleSpawner(SparkleSpawner { count: 0 }));
                }
            }
            0xB152 => self.spawn_clouds(),
            // loc_1047A
            0x1047A => {
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::Skip);
                }
            }
            _ => return false,
        }
        true
    }

    /// `loc_F856`: the wrecked machine with the machine's priority bits.
    fn wreck_machine(&mut self, at: u16) {
        self.eni_dec_prio_safe(WRECK_MACHINE, 0x200, 0x27, 0x12, at, MACHINE_PRIORITY);
    }

    /// `sub_C972`.
    fn start_defeat(&mut self) {
        self.hscroll[0] = 0;
        self.hscroll[1] = 0;
        self.vdp_regs[0x0B] &= !3;
        self.intro_cue &= 0xFF00;
        // sub_EDD6
        self.intro_cue = 0;
        let robotnik = self.find_actor_slot();
        if let Some(slot) = robotnik {
            let r = CutsceneRobotnik { started: false, ptr: ROBOTNIK_DEFEATED, time: 0, frame: 0, mappings: 0x2B, x: 0xA0 << 16, y: 0xE0, draw: 0x80 };
            self.slots[slot] = Some(Actor::CutsceneRobotnik(r));
        }
        let parent = robotnik.unwrap_or(0);
        let (rx, ry) = match &self.slots[parent] {
            Some(Actor::CutsceneRobotnik(r)) => (r.x >> 16, r.y),
            _ => (0, 0),
        };
        self.spawn_defeat_parts(parent, rx as u16, ry);
        self.long_step();
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x3000, art: SHIP_ART } });
    }

    fn spawn_defeat_parts(&mut self, parent: usize, rx: u16, ry: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        self.slots[slot] = Some(Actor::DefeatSounds(DefeatSounds { stage: 0, count: 0 }));
        let Some(slot) = self.find_actor_slot() else { return };
        let motion = Motion { x: u32::from(rx) << 16, y: u32::from(ry) << 16, vx: 0x1_D000, vy: 0xFFF9_0000, ..Motion::default() };
        self.slots[slot] = Some(Actor::DefeatShip(DefeatShip { flying: false, parent, motion }));
        let Some(slot) = self.find_actor_slot() else { return };
        let motion = Motion { x: 0x86 << 16, y: 0x37 << 16, vx: 0, vy: 0x1_8000, ax: 0x2000, ay: 0x5000, tx: 0x119, ty: 0x80 };
        let piece = DefeatPiece { phase: PiecePhase::Wait, parent, draw: 0x3F, motion, delay: 0x12A, tick: 1, step: 0 };
        self.slots[slot] = Some(Actor::DefeatPiece(piece));
        for i in 0..8 {
            let Some(slot) = self.find_actor_slot() else { return };
            let at = PARTS + i * 8;
            let anim = Anim::new(rom::long(at + 4));
            let wait = self.random() as u8 & 0x3F;
            let part = DefeatPart { phase: 0, draw: 0x80, anim, x: rom::word(at), y: rom::word(at + 2), wait, explosion: false };
            self.slots[slot] = Some(Actor::DefeatPart(part));
        }
        for waiting in [false, true] {
            let Some(slot) = self.find_actor_slot() else { return };
            let (x, y) = self.explosion_place();
            let anim = Anim { ptr: EXPLOSION, time: 0, frame: 0x16 };
            let part = if waiting {
                DefeatPart { phase: 0, draw: 0, anim, x, y, wait: 0x0F, explosion: true }
            } else {
                DefeatPart { phase: 1, draw: 0x80, anim, x, y, wait: 0, explosion: true }
            };
            self.slots[slot] = Some(Actor::DefeatPart(part));
        }
    }

    /// `sub_CD08`.
    fn explosion_place(&mut self) -> (u16, u16) {
        let r = self.random();
        ((r & 0xFF) + 0x90, (r >> 9) + 0x80)
    }

    pub(super) fn run_defeat_sounds(&mut self, mut s: DefeatSounds) -> Option<DefeatSounds> {
        if s.stage != 0 {
            s.count -= 1;
            if s.count != 0 {
                return Some(s);
            }
        }
        s.stage += 1;
        if s.stage > 3 {
            self.play_sample(0x91);
            return None;
        }
        self.play_sample(0x6A);
        s.count = if s.stage == 3 { 0xC8 } else { 0x82 };
        Some(s)
    }

    fn robotnik_mut(&mut self, slot: usize) -> Option<&mut CutsceneRobotnik> {
        match self.slots[slot].as_mut() {
            Some(Actor::CutsceneRobotnik(r)) => Some(r),
            _ => None,
        }
    }

    pub(super) fn run_defeat_ship(&mut self, mut s: DefeatShip) -> Option<DefeatShip> {
        if !s.flying {
            let r = self.robotnik_mut(s.parent)?;
            if rom::byte(r.ptr) == 0xFE {
                r.ptr = ROBOTNIK_CARRIED;
                s.flying = true;
            }
            return Some(s);
        }
        s.motion.step(0x33);
        s.motion.vy = s.motion.vy.wrapping_add(0x3200);
        let (x, y) = ((s.motion.x >> 16) as u16, (s.motion.y >> 16) as u16);
        if let Some(r) = self.robotnik_mut(s.parent) {
            r.y = y;
            r.x = (r.x & 0xFFFF) | u32::from(x) << 16;
        }
        if x as i16 >= 0x118 {
            return None;
        }
        Some(s)
    }

    pub(super) fn run_defeat_piece(&mut self, mut p: DefeatPiece) -> Option<DefeatPiece> {
        match p.phase {
            PiecePhase::Wait => {
                p.delay -= 1;
                if p.delay == 0 {
                    p.draw = 0xBF;
                    p.phase = PiecePhase::Fall;
                }
            }
            PiecePhase::Fall => {
                p.motion.step(p.draw);
                if (p.motion.x >> 16) as i16 >= 0x119 {
                    p.draw = 0x85;
                    p.motion.vy = 0x3_0000;
                    p.motion.ay = 0x4020;
                    p.motion.ty = 0xBA;
                    p.phase = PiecePhase::Hold;
                }
            }
            PiecePhase::Hold => {
                if self.robotnik_mut(p.parent).is_some_and(|r| (r.x >> 16) as u16 == 0x119) {
                    p.phase = PiecePhase::Drag;
                }
            }
            PiecePhase::Drag => {
                p.motion.step(p.draw);
                let off = (p.motion.x >> 16) as i16 >= 0x1C0;
                if off {
                    p.draw &= 0x7F;
                }
                p.tick -= 1;
                if p.tick == 0 {
                    p.tick = 3;
                    p.step = (p.step & 0xFF00) | (p.step.wrapping_add(1) & 7);
                }
                let d = rom::long(DRAG_STEPS + u32::from(p.step) * 4);
                p.motion.x = p.motion.x.wrapping_add(d);
                let y = (p.motion.y >> 16) as u16;
                if let Some(r) = self.robotnik_mut(p.parent) {
                    if off {
                        r.draw &= 0x7F;
                    }
                    r.x = r.x.wrapping_add(d);
                    r.y = y;
                }
                if (p.motion.x >> 16) as i16 >= 0x1C8 {
                    self.bytecode_disabled = false;
                    self.bytecode_flag = 0;
                    return None;
                }
            }
        }
        Some(p)
    }

    pub(super) fn run_defeat_part(&mut self, mut p: DefeatPart) -> Option<DefeatPart> {
        if p.explosion {
            if p.phase == 0 {
                p.wait = p.wait.wrapping_sub(1);
                if p.wait == 0 {
                    p.draw = 0x80;
                    p.phase = 1;
                }
                return Some(p);
            }
            p.anim.step_with(&mut || self.random());
            if rom::byte(p.anim.ptr) == 0xFE {
                (p.x, p.y) = self.explosion_place();
                p.anim.ptr = EXPLOSION;
            }
            return Some(p);
        }
        match p.phase {
            0 => {
                p.anim.step_with(&mut || self.random());
                p.phase = 1;
            }
            1 => {
                if p.wait != 0 {
                    p.wait -= 1;
                } else {
                    p.phase = 2;
                }
            }
            _ => {
                p.anim.step_with(&mut || self.random());
            }
        }
        Some(p)
    }

    pub(super) fn run_sparkle_spawner(&mut self, mut s: SparkleSpawner) -> Option<SparkleSpawner> {
        if self.word_1126 >> 8 != 0 {
            return Some(s);
        }
        s.count = s.count.wrapping_add(1);
        if s.count & 7 != 0 {
            return Some(s);
        }
        if let Some(slot) = self.find_actor_slot() {
            let y = (self.random() & 0x1F).wrapping_sub(0x40);
            let x = (self.random() & 0x1F) + 0xE2;
            self.slots[slot] = Some(Actor::ShipSparkle(ShipSparkle { frame: 0x4F, x, y, step: 0, tick: 0 }));
        }
        Some(s)
    }

    pub(super) fn run_ship_sparkle(&mut self, mut s: ShipSparkle) -> Option<ShipSparkle> {
        if self.word_1126 >> 8 != 0 {
            return Some(s);
        }
        s.tick = s.tick.wrapping_add(1);
        if s.tick & 3 != 0 {
            return Some(s);
        }
        let f = rom::byte(SPARKLE_FRAMES + u32::from(s.step));
        if f & 0x80 != 0 {
            return None;
        }
        s.frame = f;
        s.step += 1;
        Some(s)
    }

    /// `loc_10486`.
    pub(super) fn run_skip(&mut self, inputs: &Inputs) -> bool {
        if (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0 {
            self.bytecode_disabled = false;
            self.bytecode_flag = 2;
            return false;
        }
        true
    }
}
