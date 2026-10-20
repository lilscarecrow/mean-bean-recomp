//! The ending's role call (`sub_10800`, once per level 4-17 while `bytecode_flag` is 1):
//! each opponent's stage portrait and actors on the intro's backdrop (`sub_604E`, with
//! `word_FF1124` set), the opponent walking on (`loc_1066C`; Robotnik `loc_10712` with
//! his DPLC frames), their name typed into a box (`sub_10DB2`), then off they fly. After
//! Scratch the backdrop turns into the wrecked lair (`loc_10C8A`), and the last level
//! is Has Bean dropping in (`loc_109AC`).

use super::*;
use crate::rom;

/// `loc_10834`, a step of the script per code (with `ActorBookmark_SetDelay` waits).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleCall {
    pub step: u8,
    pub delay: u16,
    /// +$07: cleared when the opponent may leave.
    pub busy: bool,
}

const ROLE_CALL_CODES: [u32; 13] = [
    0x10834, 0x1084C, 0x10856, 0x10860, 0x10872, 0x10878, 0x10896, 0x1089C, 0x108B2, 0x108B8, 0x108C6, 0x108CC, 0x108DA,
];

impl RoleCall {
    pub fn code(&self) -> u32 {
        ROLE_CALL_CODES[usize::from(self.step)]
    }

    pub fn step_of(code: u32) -> Option<u8> {
        ROLE_CALL_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `loc_1066C` / `loc_10712`: the opponent walking on (4 pixels a frame for $34
/// frames), then waiting for the role call to let them go, then flying off ($20 frames).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Walker {
    pub robotnik: bool,
    pub phase: u8,
    pub parent: usize,
    pub busy: bool,
    pub mappings: u8,
    pub motion: Motion,
    pub anim: Anim,
    pub count: u16,
}

impl Walker {
    pub fn code(&self) -> u32 {
        let codes = if self.robotnik { [0x10712, 0x10724, 0x1073C, 0x1075C, 0x10776] } else { [0x1066C, 0x10678, 0x10690, 0x106E2, 0x106FC] };
        codes[usize::from(self.phase)]
    }
}

/// `sub_104E2`: a still sprite (mappings 8 frame 9) over the backdrop until the level's
/// end.
pub const BACKDROP_PROP_CODE: u32 = 0x104E2;

/// `sub_10DF2`: types the role call's text, a character every 8 frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Typist {
    pub tick: u8,
    pub left: u16,
    pub at: u16,
    pub text: u32,
}

pub const TYPIST_CODE: u32 = 0x10DF2;

/// `loc_10C8A`: the backdrop becomes the wrecked lair (a step a frame).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LairReveal {
    pub step: u8,
    pub delay: u16,
}

const LAIR_REVEAL_CODES: [u32; 8] = [0x10C8A, 0x10CC2, 0x10CD8, 0x10CDE, 0x10CFA, 0x10D0E, 0x10D22, 0x10D36];

impl LairReveal {
    pub fn code(&self) -> u32 {
        LAIR_REVEAL_CODES[usize::from(self.step)]
    }

    pub fn step_of(code: u32) -> Option<u8> {
        LAIR_REVEAL_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `loc_10A14`: Has Bean dropping in, then bouncing; `loc_10AC8`: his shadow, which
/// shows after $B4 frames and tints palette line 3's color 9 by its frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HasBean {
    pub phase: u8,
    pub delay: u16,
    pub draw: u8,
    pub mappings: u8,
    pub motion: Motion,
    pub anim: Anim,
}

impl HasBean {
    pub fn code(&self) -> u32 {
        [0x10A14, 0x10A26, 0x10A2C, 0x10A5C, 0x10A9E][usize::from(self.phase)]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HasBeanShadow {
    pub phase: u8,
    pub draw: u8,
    pub anim: Anim,
    pub count: u16,
}

impl HasBeanShadow {
    pub fn code(&self) -> u32 {
        [0x10AC8, 0x10AF4, 0x10B12, 0x10B26][usize::from(self.phase)]
    }
}

/// `unk_10822` (opponent by level), `byte_10C78` (levels that clear everything after),
/// `unk_10628` (walk-on places), `byte_10702` (walk script by opponent), the role call
/// texts, `OpponentIntroPals2`.
const LEVEL_OPPONENTS: u32 = 0x10822;
const LEVEL_RESETS: u32 = 0x10C78;
const WALK_PLACES: u32 = 0x10628;
const WALK_SCRIPTS: u32 = 0x10702;
const ROLE_CALL_TEXT: u32 = 0x10EAE;
const WALK_PALETTES: u32 = 0x10616;
const INTRO_ANIMS: u32 = 0xAC48;
const INTRO_ART: u32 = 0xAA26;
const ARMS_ART_2: u32 = 0x549F8;
const ROBOTNIK_STAGE_FRAMES: u32 = 0xABC8;
const FONT: u32 = 0x11258;
const ENDING_BG_ART: u32 = 0x6495A;
const ROBOTNIK_MAP: u32 = 0x48712;
/// The lair's palettes (`Pal_RobotnikLair`, `Pal_GameIntroGrounder`, `Pal_Robotnik`,
/// `Pal_GameIntroRobotnik`).
const LAIR_PALETTES: [u32; 4] = [0x2E56, 0x2E76, 0x2FB6, 0x2E96];
/// Has Bean: `ArtPuyo_LevelSprites`, `ArtNem_HasBeanShadow`, `Pal_HasBeanShadow`, his
/// scripts (falling, landed) and the shadow's, `word_10B5A` (its tints).
const LEVEL_SPRITES_ART: u32 = 0x77A76;
const HAS_BEAN_SHADOW_ART: u32 = 0x99982;
const HAS_BEAN_SHADOW_PALETTE: u32 = 0x32F6;
const HAS_BEAN_FALL: u32 = 0x10ABA;
const HAS_BEAN_LANDED: u32 = 0x10AA4;
const SHADOW_SCRIPT: u32 = 0x10B68;
const SHADOW_TINTS: u32 = 0x10B5A;

const OPP_ARMS: u8 = 3;
const OPP_NASU_GRAVE: u8 = 4;
const OPP_ROBOTNIK: u8 = 12;

fn rom_palette(at: u32) -> [u16; 16] {
    std::array::from_fn(|c| rom::word(at + c as u32 * 2))
}

impl Engine {
    pub(super) fn run_credits_routine(&mut self, at: u32) -> bool {
        match at {
            // sub_10800
            0x10800 => {
                self.opponent = rom::byte(LEVEL_OPPONENTS + u32::from(self.level));
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::RoleCall(RoleCall { step: 0, delay: 0, busy: false }));
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn run_role_call(&mut self, slot: usize, mut r: RoleCall) -> Option<RoleCall> {
        // ActorBookmark_SetDelay's waits: the steps at 4, 6, 8 and 10.
        if matches!(r.step, 4 | 6 | 8 | 10) {
            if r.delay != 0 {
                r.delay -= 1;
                if r.delay != 0 {
                    return Some(r);
                }
            }
            r.step += 1;
            return Some(r);
        }
        match r.step {
            0 => {
                self.word_1126 = (self.word_1126 & 0xFF) | 0x100;
                r.busy = true;
                // sub_1090A: only the last level has something (Has Bean).
                if rom::byte(0x1098C + u32::from(self.level)) == 2 {
                    self.has_bean_drop();
                }
            }
            1 => {
                // sub_1095A
                if self.opponent < 0x10 {
                    let art_at = if self.opponent == OPP_ROBOTNIK { 0x8000 } else { 0x6000 };
                    self.portrait_lock = 1;
                    self.long_step();
                    self.start_portrait(art_at, 0xC000, 0xD688);
                    if self.opponent == OPP_NASU_GRAVE {
                        self.spawn_backdrop_prop();
                    }
                }
                if self.opponent == OPP_ROBOTNIK {
                    self.palettes = [[0; 16]; 4];
                    self.palette_loads = [Some(PaletteLoad::Buffer); 4];
                }
            }
            2 => {
                // sub_1093A
                if self.opponent < 0x10 {
                    self.spawn_walker(slot);
                    if self.opponent != OPP_NASU_GRAVE {
                        self.spawn_backdrop_prop();
                    }
                }
            }
            3 => {
                self.word_1126 &= 0xFF;
                r.delay = 0x80;
            }
            5 => {
                self.role_call_text(self.level);
                self.crowding = 1;
                r.delay = 0xA0;
            }
            7 => {
                if self.level >= 0x11 {
                    r.delay = 0x100;
                    r.step = 8;
                    return Some(r);
                }
                r.step = 9;
                return self.role_call_release(r);
            }
            9 => return self.role_call_release(r),
            11 => self.portrait_lock = 0x101,
            12 => {
                self.role_call_level_end();
                self.bytecode_disabled = false;
                self.bytecode_flag = 0;
                self.level = self.level.wrapping_add(1);
                if self.level < 0x12 {
                    self.bytecode_flag = 1;
                }
                return None;
            }
            _ => {}
        }
        r.step += 1;
        Some(r)
    }

    /// $108B8: the opponent may go; then $24 frames.
    fn role_call_release(&mut self, mut r: RoleCall) -> Option<RoleCall> {
        r.busy = false;
        r.delay = 0x24;
        r.step = 10;
        Some(r)
    }

    /// `sub_104B0`.
    fn spawn_backdrop_prop(&mut self) {
        if let Some(slot) = self.free_slot() {
            self.slots[slot] = Some(Actor::BackdropProp);
        }
    }

    pub(super) fn run_backdrop_prop(&mut self) -> bool {
        self.portrait_lock & 0xFF00 == 0
    }

    /// `sub_104F2`.
    fn spawn_walker(&mut self, parent: usize) {
        let opp = self.opponent;
        if opp == OPP_ARMS {
            self.long_step();
            self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x600, art: ARMS_ART_2 } });
            self.role_call_text(0x12);
        }
        if opp != OPP_ROBOTNIK {
            self.long_step();
            let art = rom::long(INTRO_ART + u32::from(opp) * 4);
            self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x8000, art } });
        }
        let robotnik = opp == OPP_ROBOTNIK;
        if let Some(slot) = self.free_slot() {
            let place = WALK_PLACES + u32::from(opp) * 4;
            let which = rom::byte(WALK_SCRIPTS + u32::from(opp));
            let script = rom::long(rom::long(INTRO_ANIMS + u32::from(opp) * 4) + u32::from(which) * 4);
            let motion = Motion {
                x: u32::from(rom::word(place)) << 16,
                y: u32::from(rom::word(place + 2)) << 16,
                vx: 6 << 16,
                ay: 0x4000,
                tx: 4,
                ..Motion::default()
            };
            let w = Walker {
                robotnik,
                phase: 0,
                parent,
                busy: true,
                mappings: if robotnik { 0x2B } else { opp + 9 },
                motion,
                anim: Anim::new(script),
                count: 0x34,
            };
            self.slots[slot] = Some(Actor::Walker(w));
        }
        // loc_105F6
        let colors = table_palette(rom::byte(WALK_PALETTES + u32::from(opp)));
        self.load_palette(3, PaletteLoad::Colors(colors));
    }

    pub(super) fn run_walker(&mut self, mut w: Walker) -> Option<Walker> {
        let parent_busy = match &self.slots[w.parent] {
            Some(Actor::RoleCall(r)) => r.busy,
            _ => false,
        };
        match w.phase {
            0 => {
                if w.robotnik {
                    w.motion.y = (w.motion.y & 0xFFFF) | 8 << 16;
                    self.robotnik_stage_frame(&mut w);
                } else {
                    w.anim.step_with(&mut || self.random());
                }
                w.phase = 1;
            }
            1 => {
                w.motion.x = w.motion.x.wrapping_add(u32::from(w.motion.tx) << 16);
                w.count -= 1;
                if w.count == 0 {
                    w.phase = 2;
                }
            }
            2 => {
                if w.robotnik {
                    self.robotnik_stage_frame(&mut w);
                } else {
                    w.anim.step_with(&mut || self.random());
                    if rom::byte(w.anim.ptr) == 0xFE {
                        let which = rom::byte(WALK_SCRIPTS + u32::from(self.opponent));
                        w.anim.ptr = rom::long(rom::long(INTRO_ANIMS + u32::from(self.opponent) * 4) + u32::from(which) * 4);
                    }
                }
                if !parent_busy {
                    w.count = 0x20;
                    w.phase = 3;
                }
            }
            3 => {
                w.motion.step(0xB7);
                w.count -= 1;
                if w.count == 0 {
                    w.busy = false;
                    w.phase = 4;
                }
            }
            _ => return None,
        }
        Some(w)
    }

    /// `sub_1077C`: Robotnik's frame (DMAed from `DPLC_Robotnik_Stage`).
    fn robotnik_stage_frame(&mut self, w: &mut Walker) {
        if w.anim.time != 0 {
            w.anim.time -= 1;
            return;
        }
        let mut at = w.anim.ptr;
        if rom::byte(at) == 0xFF {
            at = rom::long(at + 2);
        }
        w.anim.time = rom::byte(at);
        w.anim.frame = rom::byte(at + 1);
        w.anim.ptr = at + 2;
        let tiles = rom::long(ROBOTNIK_STAGE_FRAMES + u32::from(w.anim.frame) * 4);
        self.dma_queue.push(VdpOp::RomCopy { vram: 0x2000, from: tiles, len: 0xB00 });
    }

    /// `sub_10DB2`: a role call text's box (plane command $91), then its typist.
    fn role_call_text(&mut self, which: u8) {
        let at = rom::long(ROLE_CALL_TEXT + u32::from(which) * 4);
        let (len, to) = (rom::byte(at + 1), rom::word(at + 2));
        self.queue_panel(PanelCommand::TextBox { len, at: to });
        if let Some(slot) = self.free_slot() {
            let t = Typist { tick: 0, left: rom::word(at), at: rom::word(at + 2).wrapping_add(0x82), text: at + 4 };
            self.slots[slot] = Some(Actor::Typist(t));
        }
    }

    pub(super) fn run_typist(&mut self, mut t: Typist) -> Option<Typist> {
        t.tick = t.tick.wrapping_add(1);
        if t.tick & 7 != 0 {
            return Some(t);
        }
        // sub_10E18
        let c = rom::byte(t.text);
        t.text += 1;
        let tile = 0xE500 | u16::from(rom::byte(FONT + u32::from(c)));
        self.plane_writes.push((t.at, tile));
        t.at = t.at.wrapping_add(2);
        t.left -= 1;
        if t.left == 0 {
            return None;
        }
        Some(t)
    }

    /// `sub_10BF8`.
    fn role_call_level_end(&mut self) {
        if rom::byte(LEVEL_RESETS + u32::from(self.level)) != 0 {
            self.palettes = [[0; 16]; 4];
            self.palette_loads = [Some(PaletteLoad::Buffer); 4];
            self.slots.iter_mut().for_each(|s| *s = None);
            self.pal_fade_gen = [0; 4];
            self.vscroll = [0; 40];
            self.hscroll = [0; 0x200];
            self.vscroll[0] = 0xFF20;
            self.vscroll[1] = 0xFF60;
        }
        if self.opponent == OPP_ROBOTNIK {
            self.queue_panel(PanelCommand::List(0));
        } else if self.level >= 0x11 {
            return;
        } else {
            self.queue_panel(PanelCommand::List(0x24));
        }
        if self.level == 0x0F {
            if let Some(slot) = self.find_actor_slot() {
                self.slots[slot] = Some(Actor::LairReveal(LairReveal { step: 0, delay: 0 }));
            }
        }
    }

    pub(super) fn run_lair_reveal(&mut self, mut l: LairReveal) -> Option<LairReveal> {
        match l.step {
            0 => {
                if let Some(c) = self.slots.get_mut(self.clouds_slot) {
                    *c = None;
                }
                for w in &mut self.hscroll[..0x142] {
                    *w = 0;
                }
                self.queue_panel(PanelCommand::List(0));
                self.palettes = [[0; 16]; 4];
                self.palette_loads = [Some(PaletteLoad::Buffer); 4];
            }
            1 => {
                self.eni_dec_safe(0x6646C, 0x200, 0x27, 8, 0xDB80);
                l.delay = 4;
            }
            2 => {
                if l.delay != 0 {
                    l.delay -= 1;
                    if l.delay != 0 {
                        return Some(l);
                    }
                }
            }
            3 => {
                self.use_lair = 1;
                self.long_step();
                self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x4000, art: ENDING_BG_ART } });
            }
            4 => {
                self.eni_dec_safe(0x66348, 0x6200, 0x27, 0x12, 0xF600);
            }
            5 => {
                self.eni_dec_prio_safe(0x6622E, 0x200, 0x27, 0x12, 0xD200, 0x6463A);
            }
            6 => {
                self.eni_dec_safe(ROBOTNIK_MAP, 0xC400, 9, 6, 0xD688);
            }
            _ => {
                for (line, at) in LAIR_PALETTES.into_iter().enumerate() {
                    self.fade_palette(line as u16, 0, 8, &rom_palette(at));
                }
                return None;
            }
        }
        l.step += 1;
        Some(l)
    }

    /// `loc_109AC`: Has Bean (level 17).
    fn has_bean_drop(&mut self) {
        self.role_call_text(0x13);
        if let Some(slot) = self.find_actor_slot() {
            let shadow = HasBeanShadow { phase: 0, draw: 0, anim: Anim { ptr: 0, time: 0, frame: 0 }, count: 0 };
            self.slots[slot] = Some(Actor::HasBeanShadow(shadow));
        }
        self.long_step();
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Sorcery { vram: 0x2000, art: LEVEL_SPRITES_ART } });
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0x400, art: HAS_BEAN_SHADOW_ART } });
        let colors = table_palette(rom::byte(WALK_PALETTES + u32::from(self.opponent)));
        self.load_palette(3, PaletteLoad::Colors(colors));
        self.load_palette(2, PaletteLoad::Colors(rom_palette(HAS_BEAN_SHADOW_PALETTE)));
        if let Some(slot) = self.free_slot() {
            let b = HasBean { phase: 0, delay: 0, draw: 0, mappings: 0, motion: Motion::default(), anim: Anim { ptr: 0, time: 0, frame: 0 } };
            self.slots[slot] = Some(Actor::HasBean(b));
        }
    }

    pub(super) fn run_has_bean(&mut self, mut b: HasBean) -> Option<HasBean> {
        match b.phase {
            0 => {
                b.anim.ptr = HAS_BEAN_FALL;
                b.delay = 0x80;
                b.phase = 1;
            }
            1 => {
                b.delay -= 1;
                if b.delay == 0 {
                    b.phase = 2;
                }
            }
            2 => {
                b.draw = 0x95;
                b.mappings = 8;
                b.anim.frame = 0;
                b.motion.x = (b.motion.x & 0xFFFF) | 0x120 << 16;
                b.motion.y = (b.motion.y & 0xFFFF) | 0xFF90 << 16;
                b.motion.ty = 0xFFFF;
                b.motion.ay = 0x1000;
                b.phase = 3;
            }
            3 => {
                b.motion.y = (b.motion.y & 0xFFFF) | u32::from(b.motion.tx) << 16;
                b.motion.step(b.draw);
                b.anim.step_with(&mut || self.random());
                let y = (b.motion.y >> 16) as u16;
                b.motion.tx = y;
                b.motion.y = (b.motion.y & 0xFFFF) | u32::from(y.wrapping_sub(0x70)) << 16;
                if b.motion.tx >= 0xC0 {
                    b.anim.ptr = HAS_BEAN_LANDED;
                    b.phase = 4;
                }
            }
            _ => {
                b.anim.step_with(&mut || self.random());
            }
        }
        Some(b)
    }

    pub(super) fn run_has_bean_shadow(&mut self, mut s: HasBeanShadow) -> Option<HasBeanShadow> {
        match s.phase {
            0 => {
                s.anim = Anim { ptr: SHADOW_SCRIPT, time: 0, frame: 0x0A };
                s.count = 0xB4;
                s.phase = 1;
            }
            1 => {
                self.palettes[3][9] = 0x24;
                self.load_palette(3, PaletteLoad::Buffer);
                s.phase = 2;
            }
            2 => {
                s.count -= 1;
                if s.count == 0 {
                    s.draw = 0x80;
                    s.phase = 3;
                }
            }
            _ => {
                let before = s.anim.frame;
                s.anim.step_with(&mut || self.random());
                if s.anim.frame != before {
                    let i = u32::from(before.wrapping_sub(0x0A));
                    self.palettes[3][9] = rom::word(SHADOW_TINTS + i * 2);
                    self.load_palette(3, PaletteLoad::Buffer);
                }
            }
        }
        Some(s)
    }
}
