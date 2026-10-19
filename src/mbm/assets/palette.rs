//! Palettes: 16 colors of 9 bits (`----bbb-ggg-rrr-`), as in CRAM. `Palette_Table`
//! holds the game's palettes, 32 bytes each.

use super::Rom;

/// `Palette_Table`.
pub const PALETTE_TABLE: u32 = 0x27D6;

/// A CRAM color as 8-bit RGB (each 3-bit level spread over 0-255).
pub fn rgb(color: u16) -> [u8; 3] {
    let level = |shift: u16| {
        let v = ((color >> shift) & 0x0E) as u8 >> 1;
        (v << 5) | (v << 2) | (v >> 1)
    };
    [level(0), level(4), level(8)]
}

/// The 16 CRAM words at `addr`.
pub fn read(rom: &Rom, addr: u32) -> [u16; 16] {
    std::array::from_fn(|i| rom.word(addr + i as u32 * 2))
}

/// Palette `index` of `Palette_Table` (what `LoadPalette` gets for `index << 5`).
pub fn from_table(rom: &Rom, index: u8) -> [u16; 16] {
    read(rom, PALETTE_TABLE + u32::from(index) * 32)
}
