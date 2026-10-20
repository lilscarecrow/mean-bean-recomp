//! The palettes during a match: `palette_buffer` (what the vertical blank last copied
//! to CRAM, which some actors also write directly), the lines waiting to be copied
//! (`LoadPalette` sets a line's pointer; `CopyPalToCRAM` copies it in the vertical
//! blank), and the fades (`ActPalFade`: each line's colors split into 3-bit channels
//! with 4 fraction bits, stepped 8 times by an eighth of the distance).

use super::*;
use crate::rom;

/// What a line's palette pointer points at when the vertical blank copies it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteLoad {
    /// A palette in the ROM, or a fade's output (as it is now).
    Colors([u16; 16]),
    /// The palette buffer itself (after an actor wrote colors into it).
    Buffer,
}

/// A line's `pal_fade_data` (+$02 the channels as they fade, +$32 the step per channel;
/// the generation word is `Engine::pal_fade_gen`). Channels are B, R, G per color,
/// each 3 bits << 4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FadeData {
    pub current: [u8; 48],
    pub step: [u8; 48],
}

impl Default for FadeData {
    fn default() -> Self {
        FadeData { current: [0; 48], step: [0; 48] }
    }
}

/// `Palette_Table` ($20 bytes each) and its white palette (`Pal_White`).
const PALETTE_TABLE: u32 = 0x27D6;
pub const PAL_WHITE: u8 = 0x0C;
/// `OpponentPalettes`: each opponent's palette index.
const OPPONENT_PALETTES: u32 = 0x6206;
/// The Arms portrait's cycling color by crowding (`unk_6FF2` level 0, `unk_700E` 1-2,
/// `unk_7016` 3).
pub const ARMS_COLORS: [u32; 3] = [0x6FF2, 0x700E, 0x7016];

/// `ActPalFade_Split`.
fn split(colors: &[u16; 16]) -> [u8; 48] {
    let mut out = [0u8; 48];
    for (i, &c) in colors.iter().enumerate() {
        let [hi, lo] = c.to_be_bytes();
        out[i * 3] = (hi << 3) & 0x70;
        out[i * 3 + 1] = (lo << 3) & 0x70;
        out[i * 3 + 2] = (lo >> 1) & 0x70;
    }
    out
}

/// `ActPalFade_GetMDPal` (a channel with bit 1 set rounds up).
fn combine(channels: &[u8; 48]) -> [u16; 16] {
    let round = |v: u8| if v & 2 != 0 { v.wrapping_add(4) } else { v };
    std::array::from_fn(|i| {
        let (b, r, g) = (round(channels[i * 3]), round(channels[i * 3 + 1]), round(channels[i * 3 + 2]));
        let hi = (b >> 3) & 0x0E;
        let lo = ((g << 1) & 0xE0) | ((r >> 3) & 0x0E);
        u16::from_be_bytes([hi, lo])
    })
}

/// A palette of the table.
pub fn table_palette(index: u8) -> [u16; 16] {
    std::array::from_fn(|c| rom::word(PALETTE_TABLE + u32::from(index) * 0x20 + c as u32 * 2))
}

impl Engine {
    /// `LoadPalette`.
    pub(super) fn load_palette(&mut self, line: usize, load: PaletteLoad) {
        self.palette_loads[line & 3] = Some(load);
    }

    /// The opponent's portrait palette (`sub_61C0`).
    pub(super) fn opponent_palette(&self) -> [u16; 16] {
        table_palette(rom::byte(OPPONENT_PALETTES + u32::from(self.opponent)))
    }

    /// The game flow's `BPAL`s before a match (`assets::screen::level_palettes`).
    pub fn load_palettes(&mut self, lines: &[[u16; 16]; 4]) {
        for (line, colors) in lines.iter().enumerate() {
            self.load_palette(line, PaletteLoad::Colors(*colors));
        }
    }

    /// `CopyPalToCRAM` (in the vertical blank).
    pub(super) fn copy_palettes(&mut self) {
        for line in 0..4 {
            if let Some(PaletteLoad::Colors(c)) = self.palette_loads[line].take() {
                self.palettes[line] = c;
            }
        }
    }

    /// `FadeToPal_Setup`: the line as it is now towards `target`.
    pub(super) fn start_fade(&mut self, line: usize, target: &[u16; 16]) {
        let current = split(&self.palettes[line]);
        let to = split(target);
        let step = std::array::from_fn(|i| ((to[i].wrapping_sub(current[i]) as i8) >> 3) as u8);
        self.pal_fades[line] = FadeData { current, step };
    }

    /// `ActPalFade_DoFade`: one step, and the line is loaded from the fade's output.
    pub(super) fn fade_step(&mut self, line: usize) {
        let f = &mut self.pal_fades[line];
        for i in 0..48 {
            f.current[i] = f.current[i].wrapping_add(f.step[i]);
        }
        let colors = combine(&f.current);
        self.load_palette(line, PaletteLoad::Colors(colors));
    }

    /// A color written into the palette buffer, then the line loaded from it.
    pub(super) fn set_palette_color(&mut self, line: usize, color: usize, value: u16) {
        self.palettes[line][color] = value;
        self.load_palette(line, PaletteLoad::Buffer);
    }
}
