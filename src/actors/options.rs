//! The options screen (`BC_Options`): a checkered plane B, and the options printed
//! every frame into `plane_a_buffer`, which the vertical blank copies to plane A while
//! `use_plane_a_buffer` is set. Each pad has its own cursor (+$2C / +$2D): player 1's
//! pad reaches every line, the other only its own key assignments; the cursor's line
//! blinks in that pad's color. Left/right change the option; on "input test" A/B/C
//! shows every button of both pads until A+START is held; START leaves (to the title).
//!
//! (The sound test line only shows when `sound_test_enabled` is set, which on a
//! non-Japanese console never lasts past the title: it isn't ported.)

use super::*;
use crate::rom;

/// `ActOptions` (+$26 the blink count).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    pub phase: u8,
    pub blink: u8,
    pub cursors: [u8; 2],
}

impl Options {
    pub fn code(&self) -> u32 {
        [0x23014, 0x23022, 0x23430][usize::from(self.phase)]
    }
}

const OPTIONS_MODE_STRINGS: u32 = 0x22C88;
const CHAR_CONV: u32 = 0x22F94;
const CTRL_STRINGS: u32 = 0x2329E;
const COM_LEVEL_STRINGS: u32 = 0x232EE;
const VS_MATCHES: u32 = 0x23370;
const STR_GAME_MATCH: u32 = 0x23362;
const STR_ON: u32 = 0x233D2;
const STR_OFF: u32 = 0x233D6;
const STR_INPUT_TEST: u32 = 0x233EC;
const STR_SOUND_TEST: u32 = 0x23416;
const LAMP_ON: u32 = 0x234AE;
const LAMP_OFF: u32 = 0x234B2;
const LAMP_PLACES: u32 = 0x234B6;

/// Indexes into `Engine::options` (`$FFFCA4..`).
const OPT_MATCHES: usize = 0;
const OPT_P1_KEYS: usize = 2;
const OPT_P2_KEYS: usize = 5;
pub(super) const OPT_NO_SAMPLES: usize = 8;

/// What a pad picked on the options screen (`.Select`).
enum Picked {
    Nothing,
    InputTest,
    SoundTest,
}

impl Engine {
    pub(super) fn run_options_routine(&mut self, at: u32) -> bool {
        match at {
            // Options_SetupPlanes: plane B checkered, plane A from the buffer.
            0x22BF0 => {
                self.use_plane_a_buffer = true;
                let mut tile = 0x6Cu16;
                for row in 0..0x1C {
                    for col in 0..0x28 {
                        self.plane_writes.push((0xE000 + row * 0x80 + col * 2, tile));
                        tile ^= 1;
                    }
                    tile ^= 2;
                }
                self.plane_a_buffer.fill(0x8500);
            }
            // SpawnOptionsActor
            0x22C34 => {
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::Options(Options { phase: 0, blink: 0, cursors: [0; 2] }));
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn run_options(&mut self, mut o: Options, inputs: &Inputs) -> Option<Options> {
        match o.phase {
            0 => {
                self.options_strings(0);
                o.phase = 1;
            }
            1 => {
                o.blink = o.blink.wrapping_add(1);
                self.print_options(&o);
                if (inputs.pads[0].press | inputs.pads[1].press) & 0x80 != 0 {
                    self.use_plane_a_buffer = false;
                    self.bytecode_flag = 0;
                    self.bytecode_disabled = false;
                    return None;
                }
                for pad in 0..2 {
                    match self.options_ctrl(&mut o, pad, inputs.pads[pad].press) {
                        Picked::InputTest => {
                            self.options_strings(1);
                            o.phase = 2;
                            break;
                        }
                        Picked::SoundTest => {
                            self.use_plane_a_buffer = false;
                            self.bytecode_flag = 1;
                            self.bytecode_disabled = false;
                            return None;
                        }
                        Picked::Nothing => {}
                    }
                }
            }
            _ => {
                self.input_test_lamps(inputs);
                let a_start = |h: u8| h & 0xC0 == 0xC0;
                if a_start(inputs.pads[0].hold) || a_start(inputs.pads[1].hold) {
                    self.play_sample(0); // loc_2345A
                    self.options_strings(0);
                    o.phase = 1;
                }
            }
        }
        Some(o)
    }

    /// `OptionsCtrl`: what it picked, if anything.
    fn options_ctrl(&mut self, o: &mut Options, pad: usize, press: u8) -> Picked {
        let last = if usize::from(self.swap_controls) == pad { if self.sound_test != 0 { 7 } else { 6 } } else { 2 };
        let cursor = &mut o.cursors[pad];
        if press & 1 != 0 {
            *cursor = if *cursor == 0 { last } else { *cursor - 1 };
            self.play_sample(0x42);
        } else if press & 2 != 0 {
            *cursor += 1;
            if *cursor > last {
                *cursor = 0;
            }
            self.play_sample(0x42);
        } else if press & 0x0C != 0 {
            let step: i8 = if press & 4 != 0 { -1 } else { 1 };
            match *cursor {
                0..=2 => {
                    let at = if pad == 0 { OPT_P1_KEYS } else { OPT_P2_KEYS } + usize::from(*cursor);
                    let mut v = (self.options[at] as i8).wrapping_add(step);
                    if v < 0 {
                        v = 2;
                    }
                    if v >= 3 {
                        v = 0;
                    }
                    self.options[at] = v as u8;
                }
                3 => {
                    let c = &mut self.options[OPT_COM_LEVEL];
                    *c = c.wrapping_add(1);
                    if step > 0 {
                        *c = c.wrapping_sub(2);
                    }
                    *c &= 3;
                }
                4 => {
                    let m = &mut self.options[OPT_MATCHES];
                    *m = ((m.wrapping_sub(1) as i8).wrapping_add(step) as u8 & 7) + 1;
                }
                5 => self.options[OPT_NO_SAMPLES] ^= 0xFF,
                _ => return Picked::Nothing,
            }
            self.play_sample(0);
        } else if press & 0x70 != 0 {
            let picked = match *cursor {
                6 => Picked::InputTest,
                7 => Picked::SoundTest,
                _ => return Picked::Nothing,
            };
            self.play_sample(0);
            return picked;
        }
        Picked::Nothing
    }

    /// `Options_DrawStrings`: the buffer cleared, then the screen's fixed strings.
    pub(super) fn options_strings(&mut self, which: u32) {
        self.plane_a_buffer.fill(0x8500);
        let list = rom::long(OPTIONS_MODE_STRINGS + which * 4);
        let count = rom::word(list);
        for i in 0..u32::from(count) {
            let s = rom::long(list + 2 + i * 4);
            self.options_print(rom::word(s), rom::word(s + 2), s + 4);
        }
    }

    /// `Options_Print`: characters through `Options_CharConv` onto the attribute's high
    /// byte, at a byte offset into the buffer.
    pub(super) fn options_print(&mut self, mut at: u16, attr: u16, mut text: u32) {
        loop {
            let c = rom::byte(text);
            if c & 0x80 != 0 {
                return;
            }
            let w = (attr & 0xFF00) | u16::from(rom::byte(CHAR_CONV + u32::from(c)));
            if let Some(cell) = self.plane_a_buffer.get_mut(usize::from(at / 2)) {
                *cell = w;
            }
            at = at.wrapping_add(2);
            text += 1;
        }
    }

    /// `Options_PrintSelect`: blinking in the color of the pad whose cursor is on it.
    fn options_print_select(&mut self, o: &Options, line: u8, at: u16, text: u32) {
        let mut attr = 0x8200;
        if o.blink & 1 != 0 {
            if o.cursors[0] == line {
                attr = 0xC200;
            } else if o.cursors[1] == line {
                attr = 0xA200;
            }
        }
        self.options_print(at, attr, text);
    }

    /// `PrintMainOptions`.
    fn print_options(&mut self, o: &Options) {
        for (pad, keys, at, color) in [(0usize, OPT_P1_KEYS, 0x410u16, 0xC200u16), (1, OPT_P2_KEYS, 0x430, 0xA200)] {
            let highlight = if o.blink & 1 != 0 { color } else { 0x8200 };
            for b in 0..3u16 {
                let text = rom::long(CTRL_STRINGS + u32::from(self.options[keys + usize::from(b)]) * 4);
                let attr = if u16::from(o.cursors[pad]) == b { highlight } else { 0x8200 };
                self.options_print(at + b * 0x100, attr, text);
            }
        }
        let com = rom::long(COM_LEVEL_STRINGS + u32::from(self.options[OPT_COM_LEVEL]) * 4);
        self.options_print_select(o, 3, 0x7AE, com);
        let m = self.options[OPT_MATCHES];
        let index = u32::from(if m == 0 { 0 } else { m - 1 });
        self.options_print(0x8AE, 0xC200, rom::long(VS_MATCHES + index * 4));
        let at = if index * 4 >= 0x14 { 0x8B2 } else { 0x8B0 };
        self.options_print_select(o, 4, at, STR_GAME_MATCH);
        let sampling = if self.options[OPT_NO_SAMPLES] == 0 { STR_ON } else { STR_OFF };
        self.options_print_select(o, 5, 0x9AE, sampling);
        self.options_print_select(o, 6, 0xA9E, STR_INPUT_TEST);
        if self.sound_test != 0 {
            self.options_print_select(o, 7, 0xB9E, STR_SOUND_TEST);
        }
    }

    /// `sub_23468`: a lamp per button of both pads.
    fn input_test_lamps(&mut self, inputs: &Inputs) {
        let mut bits = u32::from(inputs.pads[0].hold) << 8 | u32::from(inputs.pads[1].hold);
        for i in 0..16u32 {
            let mut at = rom::word(LAMP_PLACES + i * 2);
            let on = bits & 1 != 0;
            bits = bits.rotate_right(1);
            let (mut text, attr) = if on { (LAMP_ON, 0xE500u16) } else { (LAMP_OFF, 0xC500u16) };
            loop {
                let c = rom::byte(text);
                if c & 0x80 != 0 {
                    break;
                }
                let t = c.wrapping_shl(1);
                let top = (attr & 0xFF00) | u16::from(t);
                let bottom = (attr & 0xFF00) | u16::from(t.wrapping_add(1));
                let i = usize::from(at / 2);
                if i + 0x40 < self.plane_a_buffer.len() {
                    self.plane_a_buffer[i] = top;
                    self.plane_a_buffer[i + 0x40] = bottom;
                }
                at = at.wrapping_add(2);
                text += 1;
            }
        }
    }
}
