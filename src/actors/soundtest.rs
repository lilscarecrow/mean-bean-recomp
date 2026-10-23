//! The sound test (`BC_SoundTest`, from the options' last line, which only shows while
//! `sound_test_enabled` is set: on this cartridge only on a Japanese console, past the
//! region lockout's code, see `boot.rs`). Six rows: three sound effects, the music,
//! a voice and a driver command; UP/DOWN pick a row, LEFT/RIGHT its value (shown in
//! hex, blinking on the row picked; the last three with their names), A/B play it, C
//! stops the sound, START goes back to the options.

use super::*;
use crate::rom;

pub const SOUND_TEST_CODE: u32 = 0x22736;
pub const SOUND_TEST_UPDATE_CODE: u32 = 0x22748;
/// `.MaxSoundSels`: each row's number of values.
const ROW_SIZES: u32 = 0x22836;
/// `off_229EC`: the music, voice and command tables (per value, a pointer to its sound
/// id and then its name).
const NAMED: u32 = 0x229EC;
/// The value rows' places in the plane A buffer (`sub_22956`, $100 apart) and the
/// names' (`SoundTest_SelNonSFX`).
const VALUES_AT: u16 = 0x29C;
const NAMES_AT: u16 = 0x5A4;
const ATTR: u16 = 0xA200;

/// `ActSoundTest`: +$26 the row, +$12.. each row's value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundTest {
    pub started: bool,
    pub row: u16,
    pub values: [u8; 6],
}

impl Engine {
    pub(super) fn run_sound_test_routine(&mut self, at: u32) -> bool {
        match at {
            // SoundTest_SetupPlanes: plane B checkered (palette line 2), plane A clear.
            0x22632 => {
                self.use_plane_a_buffer = true;
                let mut tile = 0x406Cu16;
                for row in 0..0x1C {
                    for col in 0..0x28 {
                        self.plane_writes.push((0xE000 + row * 0x80 + col * 2, tile));
                        tile ^= 1;
                    }
                    tile ^= 2;
                }
                self.plane_a_buffer.fill(0x8500);
            }
            // SpawnSoundTestActor
            0x22676 => {
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::SoundTest(SoundTest { started: false, row: 0, values: [0; 6] }));
                }
            }
            _ => return false,
        }
        true
    }

    /// A row's sound id: the sound effects from $41, the others from their tables.
    fn sound_test_id(&self, t: &SoundTest, row: usize) -> u8 {
        match row.checked_sub(3) {
            None => t.values[row].wrapping_add(0x41),
            Some(table) => rom::byte(self.sound_test_entry(t, table)),
        }
    }

    fn sound_test_entry(&self, t: &SoundTest, table: usize) -> u32 {
        let list = rom::long(NAMED + table as u32 * 4);
        rom::long(list + u32::from(t.values[3 + table]) * 4)
    }

    /// `SoundTest_SelNonSFX`: a named row's name (over blanks).
    fn sound_test_name(&mut self, t: &SoundTest, table: usize) {
        let at = ((table as u16) << 8) + NAMES_AT;
        self.options_print_bytes(at, ATTR, &[b' '; 22]);
        let entry = self.sound_test_entry(t, table) + 1;
        self.options_print(at, ATTR, entry);
    }

    /// `Options_Print` from bytes rather than the ROM.
    fn options_print_bytes(&mut self, mut at: u16, attr: u16, text: &[u8]) {
        for &c in text.iter().take_while(|&&c| c & 0x80 == 0) {
            let w = (attr & 0xFF00) | u16::from(rom::byte(0x22F94 + u32::from(c)));
            if let Some(cell) = self.plane_a_buffer.get_mut(usize::from(at / 2)) {
                *cell = w;
            }
            at = at.wrapping_add(2);
        }
    }

    pub(super) fn run_sound_test(&mut self, mut t: SoundTest, inputs: &Inputs) -> Option<SoundTest> {
        if !t.started {
            self.options_strings(2); // Options_DrawStrings
            for table in (0..3).rev() {
                self.sound_test_name(&t, table); // sub_228EC
            }
            t.started = true;
            return Some(t);
        }
        // sub_22956: the values, the row picked blinking.
        for row in (0..6usize).rev() {
            let id = self.sound_test_id(&t, row);
            let blank = usize::from(t.row) == row && self.frame_count & 1 != 0;
            let text = if blank { [0x20, 0x20] } else { [(id >> 4) + 1, (id & 0x0F) + 1] };
            self.options_print_bytes(VALUES_AT + row as u16 * 0x100, ATTR, &text);
        }
        let press = inputs.pads[0].press | inputs.pads[1].press;
        if press & 0x80 != 0 {
            self.use_plane_a_buffer = false;
            self.bytecode_disabled = false;
            return None;
        }
        if press & 0x10 != 0 {
            self.stop_sound();
            return Some(t);
        }
        if press & 0x60 != 0 {
            // (the driver commands through `PlaySound_ChkPCM`)
            let id = self.sound_test_id(&t, usize::from(t.row));
            if t.row == 5 {
                self.play_sample(id);
            } else {
                self.play_sound(id);
            }
            return Some(t);
        }
        let repeat = inputs.pads[0].repeat | inputs.pads[1].repeat;
        let step: i8 = if repeat & 1 != 0 {
            t.row = if t.row == 0 { 5 } else { t.row - 1 };
            return Some(t);
        } else if repeat & 2 != 0 {
            t.row = if t.row >= 5 { 0 } else { t.row + 1 };
            return Some(t);
        } else if repeat & 8 != 0 {
            1
        } else if repeat & 4 != 0 {
            -1
        } else {
            return Some(t);
        };
        // .MoveSelection (the music row has $1A values on a Japanese console)
        let row = usize::from(t.row);
        let size = if row == 3 && self.console & 0xC0 == 0 { 0x1A } else { rom::byte(ROW_SIZES + row as u32) };
        let mut v = t.values[row].wrapping_add(step as u8);
        if v & 0x80 != 0 {
            v = size.wrapping_sub(1);
        }
        if v >= size {
            v = 0;
        }
        t.values[row] = v;
        if let Some(table) = row.checked_sub(3) {
            self.sound_test_name(&t, table);
        }
        Some(t)
    }
}
