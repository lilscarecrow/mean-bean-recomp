//! The password screen (`BC_Password`): ten beans to pick from (the six colors and
//! the garbage bean as digits; then "right", "left" and "end"), four slots filled from
//! the right (`loc_D192`: +$26 the bean under the cursor, +$2A the slot, +$2C the
//! password, a nibble per slot, $F empty). A/C enters, B clears, START (or "end")
//! checks it against `Passwords`: a match sets the level and the difficulty, marks the
//! opponents already beaten and shows the level's name, then on to scenario mode. An
//! empty password goes back to the main menu.

use super::*;
use crate::rom;

/// `loc_D192`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordEntry {
    pub phase: u8,
    pub cursor: u16,
    pub slot: u16,
    pub password: u16,
    pub delay: u16,
    pub x: u16,
}

impl PasswordEntry {
    pub fn code(&self) -> u32 {
        [0xD192, 0xD19E, 0xD33C, 0xD342][usize::from(self.phase)]
    }
}

/// The screen's sprites: `sub_D00C` (a bean to pick), `sub_D0DE` (a slot's marker),
/// `sub_D13A` (a slot's bean), and the level's name (`locret_D348`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordSprite {
    pub kind: PasswordKind,
    pub parent: usize,
    pub index: u16,
    pub draw: u8,
    pub mappings: u8,
    pub anim: Anim,
    pub still: u8,
    pub x: u16,
    pub y: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PasswordKind {
    Bean,
    Marker,
    Digit,
    LevelName,
}

impl PasswordKind {
    pub fn code(self) -> u32 {
        match self {
            PasswordKind::Bean => 0xD00C,
            PasswordKind::Marker => 0xD0DE,
            PasswordKind::Digit => 0xD13A,
            PasswordKind::LevelName => 0xD348,
        }
    }
}

/// `loc_D3B4`: the highlight and cursor colors cycling (+$28/+$29, +$2A/+$2B).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordColors {
    pub started: bool,
    pub tick_a: u8,
    pub step_a: u8,
    pub tick_b: u8,
    pub step_b: u8,
}

pub const PASSWORD_COLORS_CODE: u32 = 0xD3BA;

const PASSWORD_MAP: u32 = 0xCE58;
const BEANS: u32 = 0xD01A;
const DIGIT_ANIM: u32 = 0xD072;
const PASSWORDS: u32 = 0x7018;
const NAME_X: u32 = 0xD34A;
const BEATEN: u32 = 0xD34E;
const HIGHLIGHT_COLORS: u32 = 0xD456;
const CURSOR_COLORS: u32 = 0xD4B0;

impl Engine {
    pub(super) fn run_password_routine(&mut self, at: u32) -> bool {
        match at {
            // Password_LoadBG
            0xCE3A => self.eni_dec_safe(PASSWORD_MAP, 0x6400, 0x27, 0x1B, 0xE000),
            // Password_Checks
            0xCF4A => {
                self.vdp_regs[0x0C] |= 8;
                self.vscroll[0] = 0;
                self.hscroll[0] = 0;
                let Some(entry) = self.find_actor_slot() else { return true };
                let p = PasswordEntry { phase: 0, cursor: 0, slot: 3, password: self.password, delay: 0, x: 0x8C };
                self.slots[entry] = Some(Actor::PasswordEntry(p));
                self.spawn_password_sprites(entry);
                if let Some(slot) = self.find_actor_slot() {
                    let c = PasswordColors { started: false, tick_a: 0, step_a: 0, tick_b: 0, step_b: 0 };
                    self.slots[slot] = Some(Actor::PasswordColors(c));
                }
            }
            _ => return false,
        }
        true
    }

    fn spawn_password_sprites(&mut self, parent: usize) {
        let blank = PasswordSprite {
            kind: PasswordKind::Bean,
            parent,
            index: 0,
            draw: 0x80,
            mappings: 0,
            anim: Anim { ptr: 0, time: 0, frame: 0 },
            still: 0,
            x: 0,
            y: 0,
        };
        // sub_CFB8
        for (n, i) in (0..10u16).rev().enumerate() {
            let Some(slot) = self.find_actor_slot() else { break };
            let at = BEANS + n as u32 * 8;
            let s = PasswordSprite {
                mappings: rom::byte(at),
                anim: Anim { ptr: rom::long(at + 4), time: rom::byte(at + 1), frame: rom::byte(at + 2) },
                still: rom::byte(at + 3),
                x: 0x198u16.wrapping_sub(i * 0x1C),
                y: 0xD8,
                ..blank.clone()
            };
            self.slots[slot] = Some(Actor::PasswordSprite(s));
        }
        // sub_D096
        for i in (0..4u16).rev() {
            let Some(slot) = self.find_actor_slot() else { break };
            let s = PasswordSprite {
                kind: PasswordKind::Marker,
                index: i,
                mappings: 0x1D,
                anim: Anim { ptr: 0, time: 0, frame: 1 },
                x: 0x148 - i * 0x20,
                y: 0x120,
                ..blank.clone()
            };
            self.slots[slot] = Some(Actor::PasswordSprite(s));
        }
        // sub_D0FC
        for i in (0..4u16).rev() {
            let Some(slot) = self.find_actor_slot() else { break };
            let s = PasswordSprite {
                kind: PasswordKind::Digit,
                index: i,
                draw: 0,
                anim: Anim { ptr: DIGIT_ANIM, time: 0, frame: 0 },
                x: 0x150 - i * 0x20,
                y: 0x118,
                ..blank.clone()
            };
            self.slots[slot] = Some(Actor::PasswordSprite(s));
        }
    }

    pub(super) fn run_password_entry(&mut self, mut p: PasswordEntry, inputs: &Inputs) -> Option<PasswordEntry> {
        let pad = inputs.pads[usize::from(self.swap_controls & 1)];
        match p.phase {
            0 => {
                p.cursor = 0;
                p.phase = 1;
            }
            1 => {
                let press = pad.press;
                if press & 0x80 != 0 {
                    self.check_password(&mut p);
                } else if press & 0x10 != 0 {
                    // loc_D35C: clear the slot.
                    if p.password == 0xFFFF {
                        self.leave_password(&p);
                    } else {
                        let at = p.slot;
                        if at != 3 {
                            p.slot += 1;
                        }
                        let mask = 0xFu16 << (at * 4);
                        if p.password & mask != mask {
                            p.password |= mask;
                            self.play_sample(0x41);
                        }
                    }
                } else if press & 0x60 != 0 {
                    // loc_D21C
                    match p.cursor {
                        7 => {
                            if p.slot != 3 {
                                p.slot += 1;
                                self.play_sample(0x41);
                            }
                        }
                        8 => {
                            if p.slot != 0 {
                                p.slot -= 1;
                                self.play_sample(0x41);
                            }
                        }
                        9 => self.check_password(&mut p),
                        bean => {
                            let at = p.slot;
                            if at != 0 {
                                p.slot -= 1;
                            }
                            let r = p.password.rotate_right(u32::from(at * 4));
                            p.password = ((r & 0xFFF0) | bean).rotate_left(u32::from(at * 4));
                            self.play_sample(0x41);
                        }
                    }
                } else if pad.repeat & 8 != 0 {
                    if p.cursor < 9 {
                        p.cursor += 1;
                        p.x = p.cursor * 0x1C + 0x8C;
                        self.play_sample(0x42);
                    }
                } else if pad.repeat & 4 != 0 {
                    if p.cursor == 0 {
                        p.cursor = 0;
                    } else {
                        p.cursor -= 1;
                        p.x = p.cursor * 0x1C + 0x8C;
                        self.play_sample(0x42);
                    }
                }
            }
            2 => {
                // ActorBookmark_Ctrl
                let press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0;
                if press == 0 && p.delay != 0 {
                    p.delay -= 1;
                    if p.delay != 0 {
                        return Some(p);
                    }
                }
                p.delay = 0;
                p.phase = 3;
            }
            _ => self.bytecode_disabled = false,
        }
        Some(p)
    }

    /// `loc_D282`.
    fn check_password(&mut self, p: &mut PasswordEntry) {
        if p.password == 0xFFFF {
            self.leave_password(p);
            return;
        }
        let Some(n) = (0..0x30u32).rev().find(|&i| rom::word(PASSWORDS + i * 2) == p.password) else {
            self.play_sample(0x46); // (no such password)
            return;
        };
        self.password = p.password;
        self.difficulty_option = (n as u8 & 3) ^ 3;
        let stage = n >> 2;
        self.level = stage as u8 + 4;
        for i in 0..=stage {
            self.opponents_defeated[usize::from(rom::byte(BEATEN + i))] = 0xFF;
        }
        self.play_sample(0x5D);
        self.difficulty = [0, 2, 4, 6][usize::from(self.difficulty_option & 3)];
        if let Some(slot) = self.find_actor_slot() {
            let d = self.difficulty_option;
            let s = PasswordSprite {
                kind: PasswordKind::LevelName,
                parent: 0,
                index: 0,
                draw: 0x80,
                mappings: 0x1D,
                anim: Anim { ptr: 0, time: 0, frame: d + 6 },
                still: 0,
                x: 0x80 + u16::from(rom::byte(NAME_X + u32::from(d))),
                y: 0x140,
            };
            self.slots[slot] = Some(Actor::PasswordSprite(s));
        }
        p.delay = 0x5A;
        p.phase = 2;
    }

    /// `loc_D394`: back to the main menu.
    fn leave_password(&mut self, p: &PasswordEntry) {
        self.play_sample(0x49);
        self.level = p.cursor as u8;
        self.bytecode_flag = 6;
        self.bytecode_disabled = false;
    }

    pub(super) fn run_password_sprite(&mut self, mut s: PasswordSprite) -> Option<PasswordSprite> {
        let (slot, password) = match &self.slots[s.parent] {
            Some(Actor::PasswordEntry(p)) => (p.slot, p.password),
            _ => (0, 0xFFFF),
        };
        match s.kind {
            PasswordKind::Bean => {
                if s.still == 0 {
                    s.anim.step_with(&mut || self.random());
                }
            }
            PasswordKind::Marker => s.anim.frame = u8::from(slot != s.index),
            PasswordKind::Digit => {
                let nibble = (password >> (s.index * 4)) & 0xF;
                if nibble == 0xF {
                    s.draw = 0;
                    return Some(s);
                }
                let at = BEANS + u32::from(nibble) * 8;
                let m = rom::byte(at);
                if m != s.mappings {
                    s.mappings = m;
                    s.anim = Anim { ptr: rom::long(at + 4), time: 0, frame: 0 };
                }
                s.draw = 0x80;
                s.anim.step_with(&mut || self.random());
            }
            PasswordKind::LevelName => {}
        }
        Some(s)
    }

    pub(super) fn run_password_colors(&mut self, mut c: PasswordColors) -> Option<PasswordColors> {
        if !c.started {
            c.started = true;
            return Some(c);
        }
        if c.tick_a == 0 {
            c.tick_a = 5;
            c.step_a += 1;
            if c.step_a == 0x0F {
                c.step_a = 0;
            }
            let from = HIGHLIGHT_COLORS + u32::from(c.step_a) * 6;
            for (i, at) in [7usize, 0x0D, 0x0E].into_iter().enumerate() {
                self.palettes[0][at] = rom::word(from + i as u32 * 2);
            }
            self.load_palette(0, PaletteLoad::Buffer);
        }
        c.tick_a = c.tick_a.wrapping_sub(1);
        if c.tick_b == 0 {
            c.tick_b = 2;
            c.step_b += 1;
            if c.step_b == 0x11 {
                c.step_b = 0;
            }
            self.palettes[1][7] = rom::word(CURSOR_COLORS + u32::from(c.step_b) * 2);
            self.load_palette(1, PaletteLoad::Buffer);
        }
        c.tick_b = c.tick_b.wrapping_sub(1);
        Some(c)
    }
}
