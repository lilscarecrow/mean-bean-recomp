//! The main menu (`BC_MainMenu`): the modes on a vine-framed board over a sky whose
//! cloud bands drift at halving speeds (`loc_BDA4`), a bean cursor (`sub_BADA`; the
//! chosen line is redrawn highlighted by plane lists $0C-$0F), and for scenario mode a
//! second board sliding in with Start / Continue (lists $10/$11). A choice blinks the
//! cursor for 16 frames, then sets `level_mode` and `bytecode_flag` for
//! `BC_GameStart` (scenario: 0 Start, 4 Continue to the password screen).
//!
//! Not in the ROM: with `network_line` on, the cursor goes one line further, to
//! [`NETWORK_LINE`] (no plane list highlights it: the front end draws it), and START
//! there sets `network_chosen` and leaves the menu as it is.

use super::*;
use crate::rom;

/// `sub_BADA` (+$26 the line; its low byte starts as the last mode chosen,
/// `byte_FF0105`; +$28 the blink count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MainMenu {
    pub phase: u8,
    pub line: u16,
    pub count: u16,
    pub draw: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
}

const MAIN_MENU_CODES: [u32; 9] = [0xBADA, 0xBAF4, 0xBB9A, 0xBBAC, 0xBE24, 0xBE4A, 0xBEC6, 0xBED2, 0xBF2C];

impl MainMenu {
    pub fn code(&self) -> u32 {
        MAIN_MENU_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        MAIN_MENU_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `loc_BDA4`: plane B's cloud bands (`word_BDF4`: lines per band, each half as fast).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MenuClouds {
    pub count: u16,
}

pub const MENU_CLOUDS_CODE: u32 = 0xBDA4;
pub const MAIN_MENU_MAPPINGS: u8 = 0x40;
/// The extra line `network_line` adds (after OPTIONS, line 3).
pub const NETWORK_LINE: u16 = 4;

const MAIN_MENU_MAP: u32 = 0x6C7C2;
const SCENARIO_MENU_MAP: u32 = 0x6C872;
const MENU_CLOUDS_MAP: u32 = 0x6C910;
const MENU_MOUNTAINS_MAP: u32 = 0x6C99C;
const CURSOR_ANIM: u32 = 0xBC02;
const CLOUD_BANDS: u32 = 0xBDF4;
const LEVEL_MODES: u32 = 0xBBFC;
/// `Pal_MainMenuShadow` (palette table entry).
const PAL_MENU_SHADOW: u8 = 0x3B;
/// `com_level` in `options` (`$FFFCA4..`: matches, COM level, P1 A/B/C, P2 A/B/C,
/// samples off).
pub const OPT_COM_LEVEL: usize = 1;
/// `LoadDefaultOptions`: one match, COM level normal, A/B/C turning right/left/right,
/// voices on.
pub const DEFAULT_OPTIONS: [u8; 9] = [1, 2, 2, 1, 2, 2, 1, 2, 0];

impl Engine {
    pub(super) fn run_menu_routine(&mut self, at: u32) -> bool {
        match at {
            0xF6E6 => self.eni_dec_safe(MAIN_MENU_MAP, 0xE190, 0x21, 0x13, 0xC306),
            0xF702 => self.eni_dec_safe(SCENARIO_MENU_MAP, 0xE190, 0x19, 0x0B, 0xC65E),
            0xF7EC | 0xF7F6 | 0xF800 | 0xF80A => {
                let to = 0xE000 + (at - 0xF7EC) as u16 / 10 * 0x40;
                self.eni_dec_safe(MENU_CLOUDS_MAP, 0x2190, 0x1F, 0x12, to);
            }
            0xF7D0 => self.eni_dec_safe(MENU_MOUNTAINS_MAP, 0x2190, 0x27, 9, 0xF300),
            0xBAAA => self.queue_panel(PanelCommand::List(0x0C)),
            0xBAC0 => self.queue_panel(PanelCommand::List(0x10)),
            0xBA3C => self.start_main_menu(),
            _ => return false,
        }
        true
    }

    /// `loc_BA3C`.
    fn start_main_menu(&mut self) {
        self.fade_palette(2, 0, 3, &table_palette(PAL_MENU_SHADOW));
        let Some(slot) = self.find_actor_slot() else { return };
        // (sub_BAB2 here reads its "line" from the routine's own code: list $0C)
        self.queue_panel(PanelCommand::List(0x0C));
        let m = MainMenu {
            phase: 0,
            line: u16::from(self.last_mode),
            count: 0,
            draw: 0x80,
            anim: Anim { ptr: CURSOR_ANIM, time: 0, frame: 0x3E },
            x: 0xC0,
            y: 0xD0,
        };
        self.slots[slot] = Some(Actor::MainMenu(m));
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::MenuClouds(MenuClouds { count: 0 }));
        }
    }

    pub(super) fn run_main_menu(&mut self, mut m: MainMenu, inputs: &Inputs) -> Option<MainMenu> {
        // (GetCtrlData: the pad playing player 1)
        let press = inputs.pads[usize::from(self.swap_controls & 1)].press;
        match m.phase {
            0 => {
                m.count = 0x280;
                self.queue_panel(PanelCommand::List(0x0C + m.line));
                m.phase = 1;
            }
            1 => {
                m.y = m.line * 0x18 + 0xD0;
                m.anim.step_with(&mut || self.random());
                if press & 0xF0 != 0 && m.line == NETWORK_LINE {
                    self.network_chosen = true;
                } else if press & 0xF0 != 0 {
                    // sub_BCC2: 1P vs 2P needs a second pad (there always is one here;
                    // without it, sound $67).
                    self.play_sample(0x41);
                    self.last_mode = m.line as u8;
                    m.count = 0;
                    m.phase = 2;
                } else if press & 1 != 0 {
                    if m.line != 0 {
                        m.line -= 1;
                        self.play_sample(0x42);
                    }
                    self.queue_panel(PanelCommand::List(0x0C + m.line));
                } else if press & 2 != 0 {
                    let last = if self.network_line { NETWORK_LINE } else { 3 };
                    if m.line < last {
                        m.line += 1;
                        self.play_sample(0x42);
                    }
                    if m.line < NETWORK_LINE {
                        self.queue_panel(PanelCommand::List(0x0C + m.line));
                    }
                }
            }
            2 | 6 => {
                m.count = 0x10;
                m.phase += 1;
            }
            3 | 7 => {
                m.draw = (m.count as u8).rotate_right(2) & 0x80;
                m.count -= 1;
                if m.count != 0 {
                    return Some(m);
                }
                if m.phase == 7 {
                    // loc_BEEA: scenario mode from stage 1.
                    self.bonus_level = 1;
                    self.difficulty_option = self.options[OPT_COM_LEVEL];
                    self.level = 3;
                    self.difficulty = [0, 2, 4, 6][usize::from(self.difficulty_option & 3)];
                    for d in &mut self.opponents_defeated[..16] {
                        *d = 0;
                    }
                    self.level_mode = 0;
                    self.bytecode_flag = (m.line as u8) << 2;
                    self.bytecode_disabled = false;
                    m.phase = 8;
                    return Some(m);
                }
                // loc_BBCA
                let mode = rom::byte(LEVEL_MODES + u32::from(m.line));
                self.level_mode = mode;
                self.bytecode_flag = mode;
                if mode == 0 {
                    // loc_BDFE: the scenario board slides in.
                    m.draw = 0x80;
                    m.x = 0x220;
                    m.y = 0xD8;
                    self.opponent = 1;
                    m.line = 0x28;
                    m.phase = 4;
                    return Some(m);
                }
                if mode != 3 {
                    self.swap_controls = 0;
                }
                self.bytecode_disabled = false;
                return None;
            }
            4 => {
                for i in 0..0xB8 {
                    let w = &mut self.hscroll[0x20 + i * 2];
                    *w = w.wrapping_sub(8);
                }
                m.x = m.x.wrapping_sub(8);
                m.line -= 1;
                if m.line == 0 {
                    m.phase = 5;
                }
            }
            5 => {
                m.anim.step_with(&mut || self.random());
                if press & 0xF0 != 0 {
                    self.play_sample(0x41); // loc_BEB4
                    m.count = 0;
                    m.phase = 6;
                } else if press & 1 != 0 {
                    if m.line != 0 {
                        m.line = 0;
                        m.y = 0xD8;
                        self.queue_panel(PanelCommand::List(0x10));
                        self.play_sample(0x42);
                    }
                } else if press & 2 != 0 && m.line == 0 {
                    m.line = 1;
                    m.y = 0xF0;
                    self.queue_panel(PanelCommand::List(0x11));
                    self.play_sample(0x42);
                }
            }
            _ => {}
        }
        Some(m)
    }

    pub(super) fn run_menu_clouds(&mut self, mut c: MenuClouds) -> Option<MenuClouds> {
        c.count = c.count.wrapping_add(1);
        let mut v = c.count;
        let mut line = 0usize;
        let mut band = 0u32;
        loop {
            let n = rom::word(CLOUD_BANDS + band * 2);
            if n & 0x8000 != 0 {
                break;
            }
            for _ in 0..=n {
                self.hscroll[1 + line * 2] = v;
                line += 1;
            }
            v = (v as i16 >> 1) as u16;
            band += 1;
        }
        Some(c)
    }
}
