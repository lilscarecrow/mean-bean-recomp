//! Plane maps: the game's plane commands (`ProcPlaneCommands`) that the match screen
//! uses, and how a field cell is drawn (`sub_5782` / `DrawPuyo` / `GetPuyoTileID`).
//!
//! A plane command is a long. With bit 31 clear it points at a map descriptor
//! (`QueueLoadMap`): word type, byte width, byte height, word VRAM address, then per
//! type the data; rows are the plane's width apart ($80 bytes on the match screen's
//! 64-cell planes). With bit 31 set the top byte picks a special routine
//! (`SpecPlane80`..`9F`); the match screen's board needs none, `SpecPlane8A` animates
//! the refugee bean's tiles.

use super::{Rom, Vram};

/// `PlaneCmdLists`: the operand of `lea PlaneCmdLists,a2` in `QueuePlaneCmdList`.
const PLANE_CMD_LISTS: u32 = 0x1C95C;
/// Bytes per plane row on the match screen (64 cells).
pub const ROW_BYTES: u16 = 0x80;
/// Where each player's field starts in plane A (`off_5AFA`: visible row 0, column 0).
pub const FIELD_ORIGIN: [u16; 2] = [0xC104, 0xC134];
/// `SpecPlane8A`: the refugee bean's tiles ($32C-$32F) take frame f from $5F00 + f * $80.
const REFUGEE_FRAMES: u16 = 0x5F00;
const REFUGEE_TILES: u16 = 0x6580;
/// `PuyoPalLines`: palette bits of a bean by color (garbage uses entries 6..13).
const PALETTE_BITS: [u16; 14] = [0, 0, 0x4000, 0x4000, 0x2000, 0x2000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000, 0x4000];
/// `byte_58C2`: tile offsets of a cell without the bean bit (bits 0-3 index it).
const PLAIN_OFFSETS: [u8; 6] = [0, 0, 0x10, 0x11, 0x12, 0x14];

/// The commands of `PlaneCmdLists` entry `n`.
pub fn list(rom: &Rom, n: u16) -> Vec<u32> {
    let at = rom.long(PLANE_CMD_LISTS + u32::from(n) * 4);
    (0..u32::from(rom.word(at))).map(|i| rom.long(at + 2 + i * 4)).collect()
}

/// Runs a map command (bit 31 clear) on `vram`.
pub fn run_map(rom: &Rom, vram: &mut Vram, cmd: u32) -> Result<(), String> {
    if cmd & 0x8000_0000 != 0 {
        return Err(format!("plane command {cmd:08X} is a special routine"));
    }
    let a = cmd;
    let (kind, width, height, dest) = (rom.word(a), rom.byte(a + 2), rom.byte(a + 3), rom.word(a + 4));
    let mut put = |row: u8, col: u8, word: u16| {
        let at = dest.wrapping_add(u16::from(row) * ROW_BYTES).wrapping_add(u16::from(col) * 2);
        vram.load(at, &word.to_be_bytes());
    };
    match kind {
        // QueueFillPlane
        0 => {
            let word = rom.word(a + 6);
            for r in 0..height {
                (0..width).for_each(|c| put(r, c, word));
            }
        }
        // QueueCopyMap8: bytes over a base word's high byte
        4 => {
            let (mut src, base) = (rom.long(a + 6), rom.word(a + 10));
            for r in 0..height {
                for c in 0..width {
                    put(r, c, (base & 0xFF00) | u16::from(rom.byte(src)));
                    src += 1;
                }
            }
        }
        // QueueCopyMapDirect / QueueCopyMap16 (words, plus a base for the latter)
        8 | 20 => {
            let mut src = rom.long(a + 6);
            let base = if kind == 20 { rom.word(a + 10) } else { 0 };
            for r in 0..height {
                for c in 0..width {
                    put(r, c, rom.word(src).wrapping_add(base));
                    src += 2;
                }
            }
        }
        _ => return Err(format!("map command type {kind} (at ${a:06X}) isn't ported")),
    }
    Ok(())
}

/// Runs every map command of list `n`.
pub fn run_list(rom: &Rom, vram: &mut Vram, n: u16) -> Result<(), String> {
    list(rom, n).into_iter().try_for_each(|cmd| run_map(rom, vram, cmd))
}

/// The match screen's board and background: list 0 (both planes cleared, queued as the
/// opponent screen ends) and list 3 (`LoadLevelBGArt`).
pub fn match_planes(rom: &Rom, vram: &mut Vram) -> Result<(), String> {
    run_list(rom, vram, 0)?;
    run_list(rom, vram, 3)
}

/// `SpecPlane8A`: show frame `frame` of the refugee bean (garbage) animation.
pub fn refugee_frame(vram: &mut Vram, frame: u8) {
    let from = usize::from(REFUGEE_FRAMES.wrapping_add(u16::from(frame) << 7));
    let tiles = vram.0[from..from + 0x80].to_vec();
    vram.load(REFUGEE_TILES, &tiles);
}

/// `GetPuyoTileID`: the first pattern word of a cell from its high byte (bean bit,
/// color, links): 0 is empty ($83FE), otherwise tile $100 + 4 * (color * 21 + links).
pub fn cell_pattern(cell: u8) -> u16 {
    if cell == 0 {
        return 0x83FE;
    }
    let color = (cell >> 4) & 7;
    let mut low = cell & 0x0F;
    if cell & 0x80 == 0 {
        low = PLAIN_OFFSETS.get(usize::from(low)).copied().unwrap_or(0);
    }
    let tile = (u16::from(color) * 21 + u16::from(low)) * 4 + 0x100;
    let mut entry = usize::from(color);
    if color == 6 {
        entry = usize::from(cell & 7) + 6;
    }
    0x8000 | tile | PALETTE_BITS.get(entry).copied().unwrap_or(0x4000)
}

/// `DrawPuyo`: a cell's 2x2 pattern words (top-left, top-right, bottom-left,
/// bottom-right); an empty cell repeats its two tiles across.
pub fn cell_words(cell: u8) -> [u16; 4] {
    let t = cell_pattern(cell);
    let step = if t == 0x83FE { 0 } else { 2 };
    [t, t + step, t + 1, t + 1 + step]
}

/// Draws a player's 6 x 12 visible cells (high bytes, row by row) into plane A. $FF
/// is `ResetPuyoField`'s "not drawn yet" (no real cell has it): the board shows there.
pub fn draw_field(vram: &mut Vram, player: usize, cells: &[u8; 72]) {
    for (i, &cell) in cells.iter().enumerate() {
        if cell == 0xFF {
            continue;
        }
        let (row, col) = ((i / 6) as u16, (i % 6) as u16);
        let at = FIELD_ORIGIN[player] + row * 0x100 + col * 4;
        let [tl, tr, bl, br] = cell_words(cell);
        vram.load(at, &[tl.to_be_bytes(), tr.to_be_bytes()].concat());
        vram.load(at + ROW_BYTES, &[bl.to_be_bytes(), br.to_be_bytes()].concat());
    }
}
