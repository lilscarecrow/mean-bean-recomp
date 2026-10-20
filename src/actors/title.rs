//! Power-on to the main menu: the boot checks (`GetRegion`, `CheckChecksum`: a US
//! cartridge that passes), the Sega logo (`ActSegaLogo`: it slides in, its outline
//! brightens, its colors cycle; START skips) and the title (`InitTitle`).
//!
//! The title is a state in `word_FF1126`: 0 while Robotnik's machine is shown being
//! built (the machine and its parts on plane B and as sprites, plane B scrolling in,
//! the machine shaking, then eight beans popping out and landing on the logo's
//! letters); a button then fades it out (1) and sets up the finished title (2, 3); it
//! ends as 4 when the build finishes by itself, then $FFFF. On the finished title each
//! bean hops when its own button is pressed, "PRESS START BUTTON" blinks, and START
//! goes to the main menu (`bytecode_flag` 0); waiting it out goes to the game's intro
//! (1).

use super::*;
use crate::rom;

/// `ActSegaLogo` (+$26 the outline color, then the cycle's step; +$28 a 3-frame tick).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SegaLogo {
    pub phase: u8,
    pub delay: u16,
    pub anim: Anim,
    pub x: u16,
    pub color: u16,
    pub tick: u16,
}

const SEGA_LOGO_CODES: [u32; 7] = [0xB354, 0xB39E, 0xB3A4, 0xB3F6, 0xB3FC, 0xB44A, 0xB450];

impl SegaLogo {
    pub fn code(&self) -> u32 {
        SEGA_LOGO_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        SEGA_LOGO_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `ActTimerCtrlSkip`: a button sets `bytecode_flag` $FF, running out 0; either lets
/// the flow on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CtrlSkip {
    pub timer: u16,
}

pub const CTRL_SKIP_CODE: u32 = 0xDD8A;

/// `ActTitleHandler`: the title's buttons.
pub const TITLE_HANDLER_CODE: u32 = 0xE102;

/// `.FadeToBlack` (spawned by the handler): the fade out, the finished title's setup,
/// the fade in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleFade {
    pub phase: u8,
    pub delay: u16,
}

const TITLE_FADE_CODES: [u32; 7] = [0xE156, 0xE172, 0xE1A4, 0xE1AA, 0xE1D0, 0xE1EE, 0xE1F4];

impl TitleFade {
    pub fn code(&self) -> u32 {
        TITLE_FADE_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        TITLE_FADE_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `ActTitleRobotnik`: Robotnik's colors on line 3 changing (`unk_E07E`: set, frames).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleRobotnik {
    pub wait: u8,
    pub step: u16,
}

pub const TITLE_ROBOTNIK_CODE: u32 = 0xE008;

/// `ActTitleMachineText`: the machine (mappings $24 frame $39) that the planes are
/// built around; it drives the build (+$26/+$28: the shake's counts, later the blink
/// and the countdown; +$2A beans out; +$2C 1 while they come out, 2 once done).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MachineText {
    pub phase: u8,
    pub parent: usize,
    pub draw: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
    pub delay: u16,
    pub f26: u8,
    pub f28: u16,
    pub beans: u16,
    pub stage: u8,
}

const MACHINE_TEXT_CODES: [u32; 8] = [0xE354, 0xE386, 0xE3A6, 0xE434, 0xE446, 0xE476, 0xE4C8, 0xE574];

impl MachineText {
    pub fn code(&self) -> u32 {
        MACHINE_TEXT_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        MACHINE_TEXT_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Piston,
    LeftMeter,
    RightMeters,
    Siren,
    Steam,
    Particles,
    Light,
    Lights,
    Electricity,
}

/// `TitleAnimPieces`: the machine's moving parts, placed relative to plane B's scroll
/// (`sub_EBF8`: x = offset + scroll, shown once on screen).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitlePiece {
    pub kind: PieceKind,
    pub phase: u8,
    pub parent: usize,
    pub draw: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
    pub delay: u16,
    pub f26: u16,
    pub f28: u16,
    pub f2a: u8,
}

impl TitlePiece {
    pub fn code(&self) -> u32 {
        let p = usize::from(self.phase);
        match self.kind {
            PieceKind::Piston => [0xE7BA, 0xE7EA][p],
            PieceKind::LeftMeter => [0xE7FE, 0xE82E][p],
            PieceKind::RightMeters => 0xE840,
            PieceKind::Siren => 0xE85E,
            // (the start differs: $E880 / $E888)
            PieceKind::Steam => [if self.f2a == 0 { 0xE880 } else { 0xE888 }, 0xE89A, 0xE8F0, 0xE90A][p],
            PieceKind::Particles => [0xE938, 0xE944][p],
            PieceKind::Light => [0xE9FC, 0xEA0C, 0xEA1A][p],
            PieceKind::Lights => [0xEB04, 0xEB14, 0xEB22][p],
            PieceKind::Electricity => 0xEBA6,
        }
    }

    pub fn from_code(code: u32) -> Option<(PieceKind, u8)> {
        Some(match code {
            0xE7BA => (PieceKind::Piston, 0),
            0xE7EA => (PieceKind::Piston, 1),
            0xE7FE => (PieceKind::LeftMeter, 0),
            0xE82E => (PieceKind::LeftMeter, 1),
            0xE840 => (PieceKind::RightMeters, 0),
            0xE85E => (PieceKind::Siren, 0),
            0xE880 | 0xE888 => (PieceKind::Steam, 0),
            0xE89A => (PieceKind::Steam, 1),
            0xE8F0 => (PieceKind::Steam, 2),
            0xE90A => (PieceKind::Steam, 3),
            0xE938 => (PieceKind::Particles, 0),
            0xE944 => (PieceKind::Particles, 1),
            0xE9FC => (PieceKind::Light, 0),
            0xEA0C => (PieceKind::Light, 1),
            0xEA1A => (PieceKind::Light, 2),
            0xEB04 => (PieceKind::Lights, 0),
            0xEB14 => (PieceKind::Lights, 1),
            0xEB22 => (PieceKind::Lights, 2),
            0xEBA6 => (PieceKind::Electricity, 0),
            _ => return None,
        })
    }
}

/// `loc_E9C0`: a particle rising from the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleParticle {
    pub draw: u8,
    pub motion: Motion,
    pub anim: Anim,
    pub life: u16,
}

pub const TITLE_PARTICLE_CODE: u32 = 0xE9C0;

/// A bean on the logo (`sub_E27E` on the finished title; `sub_E678` flying out of the
/// machine first): it hops when its button is pressed (+$26 which, +$2A the hop's step).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TitleBean {
    pub from_machine: bool,
    pub phase: u8,
    pub index: u16,
    pub hop: u16,
    pub draw: u8,
    pub motion: Motion,
    pub anim: Anim,
    pub target: u16,
}

impl TitleBean {
    pub fn code(&self) -> u32 {
        match (self.from_machine, self.phase) {
            (false, 0) => 0xE27E,
            (false, _) => 0xE28A,
            (true, 0) => 0xE678,
            (true, 1) => 0xE698,
            (true, 2) => 0xE244,
            (true, _) => 0xE25E,
        }
    }
}

/// The still sprites: `ActTitleRobotnikText` (riding the machine's shake) and the
/// trademark sign (`locret_E5B6`).
pub const ROBOTNIK_TEXT_CODE: u32 = 0xE0F2;
pub const TRADEMARK_CODE: u32 = 0xE5B6;

/// Data: the logo's palette cycle and slide speeds; the title's tables.
const SEGA_LOGO_ANIM: u32 = 0xB48E;
const SEGA_LOGO_SPEEDS: u32 = 0xB374;
const SEGA_LOGO_CYCLE: u32 = 0xB47C;
const TITLE_PIECES: u32 = 0xDEFC;
const ROBOTNIK_STEPS: u32 = 0xE07E;
const ROBOTNIK_COLORS: u32 = 0xE0BA;
const BEAN_PLACES: u32 = 0xE314;
const BEAN_HOPS: u32 = 0xE2B8;
const BEAN_BUTTONS: u32 = 0xE304;
const MACHINE_START: u32 = 0xE5FA;
const MACHINE_BEANS: u32 = 0xE606;
const MACHINE_END: u32 = 0xE612;
const BEAN_FLIGHTS: u32 = 0xE6AE;
const STEAM_2_ANIM: u32 = 0xE930;
const PARTICLE_ANIM: u32 = 0xE9E6;
const LIGHT_TIMES: u32 = 0xEA98;
const LIGHT_COLORS: u32 = 0xEAA4;
const LIGHTS_COLORS: u32 = 0xEB96;
const ELECTRICITY_STEPS: u32 = 0xEBE8;
const STR_PRESS_START: u32 = 0xE5C8;
const STR_COMPILE: u32 = 0xE5DE;
const STR_SEGA: u32 = 0xE5EE;
const TITLE_LOGO_MAP: u32 = 0x94716;
const TITLE_ROBOTNIK_MAP: u32 = 0x94738;
/// `Pal_Title1` (the table entry of line 0's palette; lines 1-3 follow).
const PAL_TITLE: u8 = 0x4F;
const FONT: u32 = 0x11258;

impl Engine {
    pub(super) fn run_title_routine(&mut self, at: u32) -> bool {
        match at {
            // GetRegion: a US console (the flag's clear).
            0x224B8 => self.bytecode_flag = 0,
            // InitDebugFlags
            0x279E => self.control_player_1 = 0xFF,
            // CheckChecksum: it takes a while; it passes.
            0x234D6 => {
                self.long_step();
                self.bytecode_flag = 0;
            }
            // LoadSegaLogo
            0xB324 => {
                if let Some(slot) = self.find_actor_slot() {
                    let l = SegaLogo { phase: 0, delay: 0, anim: Anim::new(SEGA_LOGO_ANIM), x: 0x20, color: 0, tick: 0 };
                    self.slots[slot] = Some(Actor::SegaLogo(l));
                }
            }
            // Title_LoadBG / Title_LoadFG
            0xEC34 => {
                self.eni_dec(TITLE_ROBOTNIK_MAP, 0x6000, 0x27, 0x16, 0xE100);
                for w in &mut self.eni_buffer[0x394..0x394 + 12] {
                    *w = 0;
                }
                self.long_step();
            }
            0xEC0C => self.eni_dec_safe(TITLE_LOGO_MAP, 0, 0x17, 7, 0xCDB0),
            0xDE3C => self.init_title(),
            _ => return false,
        }
        true
    }

    /// `InitTitle`.
    fn init_title(&mut self) {
        // InitTitleFlags
        self.difficulty = [0, 2, 4, 6][usize::from(self.difficulty_option & 3)];
        self.result_score = 0;
        self.result_beans = 0;
        self.player_flags = [0; 2];
        self.level = 0;
        self.opponent = 0;
        self.last_winner = 0;
        self.last_mode = 0;
        self.portrait_lock &= 0x00FF;
        self.swap_controls = 0;
        self.word_1126 = 0;
        self.vdp_regs[0x10] = 3;
        self.vdp_regs[0x0B] = 0;
        self.opponents_defeated = [0; 18];
        self.vscroll = [0; 40];
        self.hscroll = [0; 0x200];
        let Some(slot) = self.find_actor_slot() else { return };
        self.slots[slot] = Some(Actor::TitleHandler);
        let Some(robotnik) = self.find_actor_slot() else { return };
        self.slots[robotnik] = Some(Actor::TitleRobotnik(TitleRobotnik { wait: 1, step: 0 }));
        let Some(machine) = self.find_actor_slot() else { return };
        let m = MachineText {
            phase: 0,
            parent: robotnik,
            draw: 0,
            anim: Anim { ptr: 0, time: 0, frame: 0x39 },
            x: 0,
            y: 0xCF,
            delay: 0,
            f26: 0,
            f28: 0,
            beans: 0,
            stage: 0,
        };
        self.slots[machine] = Some(Actor::MachineText(m));
        for i in 0..10u32 {
            let at = TITLE_PIECES + i * 12;
            let Some(slot) = self.find_actor_slot() else { break };
            let code = rom::long(at);
            let Some((kind, _)) = TitlePiece::from_code(code) else { continue };
            let p = TitlePiece {
                kind,
                phase: 0,
                parent: machine,
                draw: 0,
                anim: Anim { ptr: rom::long(at + 4), time: 0, frame: 0 },
                x: rom::word(at + 8),
                y: rom::word(at + 10),
                delay: 0,
                f26: 0,
                f28: 0,
                f2a: u8::from(code == 0xE888),
            };
            self.slots[slot] = Some(Actor::TitlePiece(p));
        }
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::RobotnikText { y: 0x90 });
        }
    }

    pub(super) fn run_sega_logo(&mut self, mut l: SegaLogo) -> Option<SegaLogo> {
        match l.phase {
            0 => {
                l.anim.step_with(&mut || self.random());
                if rom::byte(l.anim.ptr) == 0xFE {
                    // ActSegaLogo_Delay
                    l.anim.frame = 0x13;
                    l.tick = 3;
                    l.delay = 0x3C;
                    l.phase = 1;
                } else {
                    l.x = l.x.wrapping_add(u16::from(rom::byte(SEGA_LOGO_SPEEDS + u32::from(l.anim.frame))));
                }
            }
            1 | 3 | 5 => {
                l.delay -= 1;
                if l.delay == 0 {
                    l.phase += 1;
                }
            }
            2 => {
                l.tick -= 1;
                if l.tick == 0 {
                    // ActSegaLogo_Outline
                    if let Some(slot) = self.find_actor_slot() {
                        self.slots[slot] = Some(Actor::CtrlSkip(CtrlSkip { timer: 0xA00 }));
                    }
                    l.tick = 3;
                    if l.color == 0xEEE {
                        l.color = 0;
                        l.tick = 3;
                        l.delay = 0x3C;
                        l.phase = 3;
                    } else {
                        l.color += 0x222;
                        self.palettes[0][1] = l.color;
                        self.load_palette(0, PaletteLoad::Buffer);
                    }
                }
            }
            4 => {
                l.tick -= 1;
                if l.tick == 0 {
                    l.tick = 3;
                    l.color += 1;
                    if l.color == 0x11 {
                        l.delay = 0x3C;
                        l.phase = 5;
                    } else {
                        let from = SEGA_LOGO_CYCLE - u32::from(l.color) * 2;
                        for i in 0..9 {
                            self.palettes[0][2 + i] = rom::word(from + i as u32 * 2);
                        }
                        self.load_palette(0, PaletteLoad::Buffer);
                    }
                }
            }
            _ => {
                self.bytecode_disabled = false;
                return None;
            }
        }
        Some(l)
    }

    pub(super) fn run_ctrl_skip(&mut self, mut c: CtrlSkip, inputs: &Inputs) -> Option<CtrlSkip> {
        if (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0 {
            self.bytecode_flag = 0xFF;
            self.bytecode_disabled = false;
            return None;
        }
        if c.timer != 0 {
            c.timer -= 1;
            if c.timer == 0 {
                self.bytecode_flag = 0;
                self.bytecode_disabled = false;
                return None;
            }
        }
        Some(c)
    }

    /// `ActTitleHandler`.
    pub(super) fn run_title_handler(&mut self, inputs: &Inputs) -> bool {
        // CheckIfJapan: on other consoles sound_test_enabled flips straight back.
        self.sound_test = 0;
        let press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0;
        if press == 0 {
            return true;
        }
        let low = self.word_1126 as u8;
        if low & 0x80 != 0 {
            if press & 0x80 == 0 {
                return true;
            }
            // loc_ECE8: the pad that pressed START plays player 1.
            let second = inputs.pads[1].press & 0x80 != 0;
            self.swap_controls = u8::from(second);
            self.byte_196a = u8::from(second);
            self.bytecode_disabled = false;
            self.bytecode_flag = 0;
            return false;
        }
        if low != 0 {
            return true;
        }
        self.word_1126 = 1;
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::TitleFade(TitleFade { phase: 0, delay: 0 }));
        }
        true
    }

    fn fades_running(&self) -> bool {
        self.pal_fade_gen.iter().any(|&g| g != 0)
    }

    pub(super) fn run_title_fade(&mut self, mut t: TitleFade) -> Option<TitleFade> {
        match t.phase {
            0 => {
                let black = table_palette(0);
                for line in (0..4).rev() {
                    self.fade_palette(line, 0, 8, &black);
                }
                t.phase = 1;
            }
            1 => {
                if !self.fades_running() {
                    // .SetupScr
                    self.word_1126 = 2;
                    self.hscroll[0] = 0x160;
                    self.vscroll[0] = 0;
                    self.spawn_title_beans();
                    t.delay = 0x1E;
                    t.phase = 2;
                }
            }
            2 | 5 => {
                t.delay -= 1;
                if t.delay == 0 {
                    t.phase += 1;
                }
            }
            3 => {
                for line in (0..4u16).rev() {
                    let colors = table_palette(PAL_TITLE + line as u8);
                    self.fade_palette(line, 0, 8, &colors);
                }
                t.phase = 4;
            }
            4 => {
                if !self.fades_running() {
                    self.word_1126 = 3;
                    t.delay = 0x3C;
                    t.phase = 5;
                }
            }
            _ => {
                self.word_1126 = 0xFFFF;
                return None;
            }
        }
        Some(t)
    }

    /// `sub_E202`: the eight beans on the logo (+$22 their animations offset).
    fn spawn_title_beans(&mut self) {
        let mut time = 1u8;
        for i in (0..8u16).rev() {
            let Some(slot) = self.find_actor_slot() else { return };
            let at = BEAN_PLACES + u32::from(7 - i) * 8;
            let motion = Motion { x: u32::from(rom::word(at + 4)) << 16, y: u32::from(rom::word(at + 6)) << 16, ..Motion::default() };
            let anim = Anim { ptr: rom::long(at), time, frame: 0 };
            let b = TitleBean { from_machine: false, phase: 0, index: i, hop: 0, draw: 0x80, motion, anim, target: 0 };
            self.slots[slot] = Some(Actor::TitleBean(b));
            time = time.wrapping_add(4);
        }
    }

    pub(super) fn run_title_robotnik(&mut self, mut r: TitleRobotnik) -> Option<TitleRobotnik> {
        if self.word_1126 == 1 {
            return None;
        }
        r.wait = r.wait.wrapping_sub(1);
        if r.wait != 0 {
            return Some(r);
        }
        let at = ROBOTNIK_STEPS + u32::from(r.step) * 2;
        r.step += 1;
        let set = rom::byte(at);
        if set & 0x80 != 0 {
            return None;
        }
        r.wait = rom::byte(at + 1);
        let from = ROBOTNIK_COLORS + u32::from(set) * 8;
        for i in 0..4 {
            self.palettes[3][6 + i] = rom::word(from + i as u32 * 2);
        }
        self.load_palette(3, PaletteLoad::Buffer);
        Some(r)
    }

    /// `sub_EBF8`: x on plane B's scroll; shown once it's on the screen's right side of 0.
    fn on_scroll(&self, offset: u16, x: &mut u16, draw: &mut u8) {
        *x = offset.wrapping_add(self.hscroll[0]);
        if *x & 0x8000 == 0 {
            *draw = 0x80;
        }
    }

    pub(super) fn run_machine_text(&mut self, slot: usize, mut m: MachineText) -> Option<MachineText> {
        let state = self.word_1126;
        match m.phase {
            0 => {
                if state == 2 {
                    return Some(self.machine_finish(slot, m));
                }
                let step = match &self.slots[m.parent] {
                    Some(Actor::TitleRobotnik(r)) => r.step,
                    _ => 0,
                };
                if step == 0x1A {
                    self.portrait_lock = (self.portrait_lock & 0xFF) | 0x100;
                    m.phase = 1;
                }
            }
            1 => {
                m.f26 = self.random() as u8 & 0x1F;
                m.f28 = 0x0606;
                m.phase = 2;
            }
            2 => {
                if state == 2 {
                    return Some(self.machine_finish(slot, m));
                }
                if self.hscroll[0] == 0x160 {
                    if self.vscroll[0] != 0 {
                        self.machine_shake(&mut m);
                    } else {
                        self.on_scroll(0, &mut m.x, &mut m.draw);
                        m.anim.ptr = MACHINE_START;
                        m.delay = 0x3C;
                        m.phase = 3;
                    }
                } else {
                    self.hscroll[0] += 1;
                    self.machine_shake(&mut m);
                }
            }
            3 => {
                if state == 2 {
                    m.delay = 0;
                    return Some(self.machine_finish(slot, m));
                }
                m.delay -= 1;
                if m.delay == 0 {
                    m.phase = 4;
                }
            }
            4 => {
                if state == 2 {
                    return Some(self.machine_finish(slot, m));
                }
                m.stage = 1;
                if m.anim.step_with(&mut || self.random()) {
                    m.anim.ptr = MACHINE_BEANS;
                    m.anim.time = 0;
                    m.phase = 5;
                }
            }
            5 => {
                if state == 2 {
                    return Some(self.machine_finish(slot, m));
                }
                m.anim.step_with(&mut || self.random());
                if m.anim.frame == 0x3F && m.anim.time == 3 {
                    self.spawn_machine_bean(m.beans);
                    m.beans += 1;
                    if m.beans == 8 {
                        m.anim.ptr = MACHINE_END;
                        m.anim.time = 0;
                        m.phase = 6;
                    }
                }
            }
            6 => {
                if state == 2 || m.anim.step_with(&mut || self.random()) {
                    return Some(self.machine_finish(slot, m));
                }
            }
            _ => {
                if m.f28 == 0x258 {
                    self.word_1126 = 0xFFFF;
                }
                m.f28 = m.f28.wrapping_sub(1);
                if m.f28 == 0 {
                    // loc_E5B8: on to the game's intro.
                    self.bytecode_flag = 1;
                    self.bytecode_disabled = false;
                    return Some(m);
                }
                m.f26 = m.f26.wrapping_add(1);
                if m.f26 & 0x0F == 0 {
                    if m.f26 & 0x10 != 0 {
                        self.title_text(0xD6BA, STR_PRESS_START, true);
                    } else {
                        self.title_text(0xD6BA, STR_PRESS_START, false);
                    }
                }
            }
        }
        Some(m)
    }

    /// $E3C4: the machine shakes: a random pause, up 2 pixels a frame, down again.
    fn machine_shake(&mut self, m: &mut MachineText) {
        let (up, down) = ((m.f28 >> 8) as u8, m.f28 as u8);
        if m.f26 != 0 {
            m.f26 -= 1;
        } else if up != 0 {
            m.f28 = (u16::from(up - 1) << 8) | u16::from(down);
            self.vscroll[0] = self.vscroll[0].wrapping_add(2);
        } else if down != 0 {
            m.f28 = u16::from(down - 1);
            self.vscroll[0] = self.vscroll[0].wrapping_sub(2);
        } else {
            let r = self.random() & 0x707;
            m.f26 = r as u8;
            let n = (r >> 8) as u8;
            m.f28 = (u16::from(n) << 8) | u16::from(n);
        }
        self.on_scroll(0, &mut m.x, &mut m.draw);
    }

    /// $E4E2: the finished title.
    fn machine_finish(&mut self, slot: usize, mut m: MachineText) -> MachineText {
        let _ = slot;
        if self.word_1126 == 0 {
            self.word_1126 = 4;
        }
        self.on_scroll(0, &mut m.x, &mut m.draw);
        m.anim.frame = 0x39;
        m.draw = 0x80;
        m.stage = 2;
        if let Some(s) = self.find_actor_slot() {
            self.slots[s] = Some(Actor::Trademark);
        }
        self.title_text(0xF918, STR_SEGA, false);
        self.title_text(0xFA18, STR_COMPILE, false);
        m.f28 = 0x2B2;
        m.phase = 7;
        m
    }

    /// `sub_EC6A` / `sub_EC98`: a string in the font (or blanked) at a plane address.
    fn title_text(&mut self, at: u16, text: u32, blank: bool) {
        let mut to = at;
        let mut i = text;
        loop {
            let c = rom::byte(i);
            if c & 0x80 != 0 {
                break;
            }
            let tile = if blank { 0xE500 } else { 0xE500 | u16::from(rom::byte(FONT + u32::from(c))) };
            self.plane_writes.push((to, tile));
            to = to.wrapping_add(2);
            i += 1;
        }
    }

    /// `sub_E61E`: a bean flies out of the machine to its letter.
    fn spawn_machine_bean(&mut self, index: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        let at = BEAN_FLIGHTS + u32::from(index) * 16;
        let motion = Motion {
            x: 0x180 << 16,
            y: 0xD0 << 16,
            vx: rom::long(at + 4),
            vy: rom::long(at + 8),
            ay: u16::from(rom::byte(at + 13)) << 8,
            ty: 0x100,
            ..Motion::default()
        };
        let anim = Anim { ptr: rom::long(at), time: 0, frame: rom::byte(at + 12) };
        let b = TitleBean { from_machine: true, phase: 0, index, hop: 0, draw: 0xBF, motion, anim, target: rom::word(at + 14) };
        self.slots[slot] = Some(Actor::TitleBean(b));
    }

    pub(super) fn run_title_bean(&mut self, mut b: TitleBean, inputs: &Inputs) -> Option<TitleBean> {
        if b.from_machine {
            if self.word_1126 == 2 {
                return None;
            }
            if b.phase == 0 {
                b.motion.step(b.draw);
                if b.target as i16 >= (b.motion.x >> 16) as i16 {
                    b.phase = 1;
                }
                return Some(b);
            }
        }
        // Idle: $E27E (on the logo), $E698 then $E244 (landed from the machine);
        // hopping: $E28A / $E25E.
        let idle = if b.from_machine { b.phase <= 2 } else { b.phase == 0 };
        if idle {
            if self.bean_pressed(&b, inputs) {
                b.phase = if b.from_machine { 3 } else { 1 };
                return Some(b);
            }
        } else if b.hop == 0x0B {
            b.hop = 0;
            b.phase = if b.from_machine { 2 } else { 0 };
        } else {
            let dy = rom::word(BEAN_HOPS + u32::from(b.hop) * 2);
            b.motion.y = b.motion.y.wrapping_add(u32::from(dy) << 16);
            b.hop += 1;
        }
        b.anim.step_with(&mut || self.random());
        Some(b)
    }

    /// `sub_E2CE`: this bean's button (P1's in the high byte, P2's in the low).
    fn bean_pressed(&self, b: &TitleBean, inputs: &Inputs) -> bool {
        let pressed = u16::from(inputs.pads[0].press) << 8 | u16::from(inputs.pads[1].press);
        pressed & rom::word(BEAN_BUTTONS + u32::from(b.index) * 2) != 0
    }

    pub(super) fn run_title_piece(&mut self, mut p: TitlePiece) -> Option<TitlePiece> {
        let stage = match &self.slots[p.parent] {
            Some(Actor::MachineText(m)) => m.stage,
            _ => 0,
        };
        let state = self.word_1126;
        match p.kind {
            PieceKind::Piston | PieceKind::LeftMeter => {
                if p.phase == 1 {
                    return Some(p);
                }
                p.anim.step_with(&mut || self.random());
                let (offset, last) = if p.kind == PieceKind::Piston { (0xFFA0, 0x1F) } else { (0xFFB0, 0x12) };
                self.on_scroll(offset, &mut p.x, &mut p.draw);
                if stage != 0 && p.anim.frame == last {
                    p.phase = 1;
                }
            }
            PieceKind::RightMeters | PieceKind::Siren => {
                p.anim.step_with(&mut || self.random());
                let offset = if p.kind == PieceKind::Siren { 0xFFC3 } else { 0xFFC0 };
                self.on_scroll(offset, &mut p.x, &mut p.draw);
            }
            PieceKind::Steam => match p.phase {
                0 => {
                    // ActTitleESteam1 / 2 (+$28 the offset, +$2A the second's delay)
                    if p.f2a != 0 {
                        p.f28 = 0x35;
                        p.f2a = 0x14;
                    } else {
                        p.f28 = 0x2F;
                    }
                    p.phase = 1;
                }
                1 => {
                    self.on_scroll(p.f28, &mut p.x, &mut p.draw);
                    let mut ending = stage != 0;
                    if state == 1 {
                        p.draw = 0;
                        p.anim.frame = 0;
                        p.anim.ptr = STEAM_2_ANIM;
                        ending = true;
                    } else {
                        p.anim.step_with(&mut || self.random());
                        if !ending && p.anim.frame == 0 {
                            p.draw = 0;
                        }
                    }
                    if ending && p.anim.frame == 0 {
                        p.draw = 0;
                        p.phase = 2;
                    }
                }
                2 => {
                    if stage == 2 {
                        p.anim.time = p.f2a;
                        p.phase = 3;
                    }
                }
                _ => {
                    p.anim.step_with(&mut || self.random());
                    self.on_scroll(p.f28, &mut p.x, &mut p.draw);
                    if p.anim.frame == 0 {
                        p.draw = 0;
                    }
                }
            },
            PieceKind::Particles => {
                if p.phase == 0 {
                    p.f26 = 1;
                    p.phase = 1;
                    return Some(p);
                }
                if stage != 0 {
                    return None;
                }
                self.on_scroll(0xFF9F, &mut p.x, &mut p.draw);
                p.draw = 0;
                p.f26 = p.f26.wrapping_sub(1);
                if p.f26 & 0x8000 != 0 {
                    p.f26 = (self.random() & 0x0F) + 0x0F;
                    if let Some(slot) = self.find_actor_slot() {
                        let motion = Motion { x: u32::from(p.x) << 16, y: u32::from(p.y) << 16, vy: 0xFFFF_8000, ..Motion::default() };
                        let anim = Anim { ptr: PARTICLE_ANIM, time: 2, frame: 0x40 };
                        self.slots[slot] = Some(Actor::TitleParticle(TitleParticle { draw: 0x33, motion, anim, life: 0x3C }));
                    }
                }
            }
            PieceKind::Light | PieceKind::Lights => {
                let light = p.kind == PieceKind::Light;
                let offset = if light { 0xFF78 } else { 0xFFCF };
                match p.phase {
                    0 => {
                        p.anim.frame = if light { 0x10 } else { 0x0F };
                        p.delay = 0x0A;
                        p.phase = 1;
                    }
                    1 => {
                        self.on_scroll(offset, &mut p.x, &mut p.draw);
                        p.delay -= 1;
                        if p.delay == 0 {
                            p.phase = 2;
                        }
                    }
                    _ => {
                        self.on_scroll(offset, &mut p.x, &mut p.draw);
                        if !(state as i16 <= 0 || state == 3) {
                            return Some(p);
                        }
                        if p.anim.time != 0 {
                            p.anim.time -= 1;
                            return Some(p);
                        }
                        if light {
                            let mut step = (p.f26 as u8).wrapping_add(1);
                            if step as i8 >= 0x0C {
                                step = 0;
                            }
                            p.f26 = (p.f26 & 0xFF00) | u16::from(step);
                            p.anim.time = rom::byte(LIGHT_TIMES + u32::from(p.f26));
                            let from = LIGHT_COLORS + u32::from(p.f26) * 8;
                            for i in 0..4 {
                                self.palettes[1][11 + i] = rom::word(from + i as u32 * 2);
                            }
                        } else {
                            p.f26 = p.f26.wrapping_add(1);
                            p.anim.time = 0x0A;
                            let from = LIGHTS_COLORS + u32::from(p.f26 & 1) * 8;
                            for (i, c) in [1, 2, 4, 5].into_iter().enumerate() {
                                self.palettes[1][c] = rom::word(from + i as u32 * 2);
                            }
                        }
                        self.load_palette(1, PaletteLoad::Buffer);
                    }
                }
            }
            PieceKind::Electricity => {
                self.on_scroll(0xFFE8, &mut p.x, &mut p.draw);
                if (p.f28 as i16) < 2 {
                    p.draw = 0;
                }
                if p.anim.time != 0 {
                    p.anim.time -= 1;
                    return Some(p);
                }
                p.f28 = (p.f28 & 0xFF00) | u16::from((p.f28 as u8).wrapping_add(1) & 7);
                let w = rom::word(ELECTRICITY_STEPS + u32::from(p.f28) * 2);
                p.anim.frame = w as u8;
                p.anim.time = (w >> 8) as u8;
            }
        }
        Some(p)
    }

    pub(super) fn run_title_particle(&mut self, mut p: TitleParticle) -> Option<TitleParticle> {
        let mut x = 0;
        self.on_scroll(0xFF9F, &mut x, &mut p.draw);
        p.motion.x = (p.motion.x & 0xFFFF) | u32::from(x) << 16;
        p.draw |= 0x3F;
        p.motion.step(p.draw);
        p.anim.step_with(&mut || self.random());
        p.life = p.life.wrapping_sub(1);
        if p.life & 0x8000 != 0 {
            return None;
        }
        Some(p)
    }
}

/// (sprites: the Sega logo's mappings and place)
pub const SEGA_LOGO_MAPPINGS: u8 = 0x32;
pub const SEGA_LOGO_Y: u16 = 0xE0;
