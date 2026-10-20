//! The match screen's video memory and palettes, as the game flow sets them up
//! (`BC_OpponentScreen` .. `BC_Level`, `LoadLevelBGArt`, `loc_6070`). Every address is
//! read from the ROM's own code and tables, so another asset source only has to answer
//! for the same addresses.
//!
//! Patterns: the board at $0000 (`ArtPuyo_LevelBG`, Sorcery), the stage's sprites at
//! $2000 (Sorcery), the opponent's portrait at $8000 (Nemesis, `OpponentArt`) and the
//! battle font at $A000 (Sorcery); on Frankly's stage, his lightning at $1E00. Tiles
//! $32C-$32F are animated at run time; $B800 up holds the VDP's tables and maps.

use super::{palette, AssetSource, Rom, Vram};

/// `lea (ArtPuyo_LevelBG).l,a0` in `LoadLevelBGArt`: the operand.
const LEVEL_BG_ART: u32 = 0x2724;
/// `BC_Level`'s `BPUYO $2000, ArtPuyo_LevelSprites` / `BPUYO $A000, ArtPuyo_LevelFonts`:
/// (VRAM address word, art long) operands.
const LEVEL_LOADS: [u32; 2] = [0x1E7E, 0x1E86];
/// `BC_Level`'s `BPAL` operands (palette offset << 3 | line), then `LoadLevelBGPal`'s
/// palette offset (`adda.l #...`) for line 2.
const LEVEL_PALETTES: [u32; 3] = [0x1E92, 0x1E96, 0x1E9A];
const LEVEL_BG_PALETTE: u32 = 0x2748;
/// `OpponentArt` (longs, by opponent; `loc_6070` decodes it to $8000) and
/// `OpponentPalettes` (bytes: `Palette_Table` index; `sub_61E0` loads it to line 3).
const OPPONENT_ART: u32 = 0x6216;
const OPPONENT_PALETTES: u32 = 0x6206;
/// `loc_67F0`: Frankly's lightning (Nemesis, to $1E00).
const FRANKLY_LIGHTNING: u32 = 0x67F8;
const OPP_FRANKLY: u8 = 1;

/// The patterns set up for a match against `opponent` in scenario mode.
pub fn match_vram(rom: &Rom, src: &dyn AssetSource, opponent: u8) -> Result<Vram, String> {
    let mut vram = Vram::default();
    vram.load(0x0000, &src.sorcery(rom.long(LEVEL_BG_ART))?);
    for at in LEVEL_LOADS {
        vram.load(rom.word(at), &src.sorcery(rom.long(at + 2))?);
    }
    vram.load(0x8000, &src.nemesis(rom.long(OPPONENT_ART + u32::from(opponent) * 4))?);
    if opponent == OPP_FRANKLY {
        vram.load(0x1E00, &src.nemesis(rom.long(FRANKLY_LIGHTNING))?);
    }
    Ok(vram)
}

/// The four palette lines `BC_Level` loads (its `BPAL`s and `LoadLevelBGPal`), in the
/// frame of `GenPuyoOrder`.
pub fn level_palettes(rom: &Rom, src: &dyn AssetSource) -> [[u16; 16]; 4] {
    let mut lines = [[0u16; 16]; 4];
    for at in LEVEL_PALETTES {
        let v = rom.word(at);
        lines[usize::from(v & 3)] = src.palette(palette::PALETTE_TABLE + u32::from(v >> 3));
    }
    lines[2] = src.palette(palette::PALETTE_TABLE + rom.long(LEVEL_BG_PALETTE));
    lines
}

/// The four palette lines of a match against `opponent` once the screen is set up
/// (`loc_6070` puts the portrait's on line 3).
pub fn match_palettes(rom: &Rom, src: &dyn AssetSource, opponent: u8) -> [[u16; 16]; 4] {
    let mut lines = level_palettes(rom, src);
    let index = rom.byte(OPPONENT_PALETTES + u32::from(opponent));
    lines[3] = src.palette(palette::PALETTE_TABLE + u32::from(index) * 32);
    lines
}
