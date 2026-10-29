//! Not in the ROM: the main menu's NETWORK line, in the menu's own big letters. The
//! letters are cut from the other lines as the ROM draws them (their plane lists' maps,
//! over the menu's tiles already in VRAM) and put together: N, T and O from OPTIONS, E
//! and R from EXERCISE MODE; there's no W or K, so W is two V's (from 1P VS. 2P MODE)
//! run together and K an I with the right half of an X. Nothing of the art is kept
//! here, only where to cut.
//!
//! The new tiles go in VRAM the menu leaves unused; the line goes under OPTIONS, on the
//! board the engine stretched for it (`stretch_main_menu`).

use super::{planes, Rom, Vram};

/// The plane list that highlights line 0: its commands draw the four lines, in order
/// (24 x 3, 24 x 3, 25 x 3, 14 x 3 words; `QueueCopyMap16`).
const LINES: u16 = 0x0C;
/// The lines' words, not highlighted (the list's bases are this or $E190).
const UNLIT: u16 = 0x8190;
/// The line NETWORK goes under.
const OPTIONS: usize = 3;
/// Where NETWORK's first letter starts: where OPTIONS' does.
const INDENT: usize = 8;
/// Palette line 3 for the highlighted line, as the lists do it.
const LIT_BITS: u16 = 0x6000;
/// Where the new tiles go: free while the menu is up (after its art, before the
/// scroll and sprite tables).
const FIRST_TILE: u16 = 0x552;
const TILES: usize = 0x5C0 - 0x552;
/// The letters' pixels: the board's brown under them, then their shadow, edge and fill
/// (where two letters' pixels meet, the higher one shows).
const BROWN: u8 = 0x0C;
fn rank(px: u8) -> u8 {
    match px {
        0 | BROWN => 0,
        0x0B => 1,
        0x0D => 2,
        _ => 3,
    }
}

/// A piece of a letter: from `line` (0-3), pixels `from..to` of it (from the line's
/// first column), placed `at` pixels into the letter.
struct Cut {
    line: usize,
    from: usize,
    to: usize,
    at: usize,
}

const fn cut(line: usize, from: usize, to: usize, at: usize) -> Cut {
    Cut { line, from, to, at }
}

/// N, E, T, W, O, R, K. Each cut takes in the letter's shadow on its left, so letters
/// set side by side are spaced as on the other lines.
const NETWORK: [&[Cut]; 7] = [
    &[cut(3, 81, 96, 0)],
    &[cut(2, 9, 22, 0)],
    &[cut(3, 41, 55, 0)],
    &[cut(1, 37, 56, 0), cut(1, 37, 56, 10)],
    &[cut(3, 8, 24, 0)],
    &[cut(2, 56, 71, 0)],
    &[cut(3, 55, 64, 0), cut(2, 31, 40, 7)],
];

/// A line's map command: the words (with `base` added), width and plane address.
struct Line {
    words: Vec<u16>,
    width: usize,
    at: u16,
}

fn lines(rom: &Rom, base: Option<u16>) -> Result<Vec<Line>, String> {
    planes::list(rom, LINES)
        .into_iter()
        .map(|a| {
            if a & 0x8000_0000 != 0 || rom.word(a) != 20 || rom.byte(a + 3) != 3 {
                return Err(format!("plane list {LINES:X} isn't the main menu's lines"));
            }
            let (width, at, src) = (usize::from(rom.byte(a + 2)), rom.word(a + 4), rom.long(a + 6));
            let base = base.unwrap_or(rom.word(a + 10));
            let words = (0..width * 3).map(|i| rom.word(src + i as u32 * 2).wrapping_add(base)).collect();
            Ok(Line { words, width, at })
        })
        .collect()
}

/// A pixel of a tile word's pattern.
fn pixel(vram: &Vram, word: u16, x: usize, y: usize) -> u8 {
    let (x, y) = (if word & 0x800 != 0 { 7 - x } else { x }, if word & 0x1000 != 0 { 7 - y } else { y });
    let b = vram.0[usize::from(word & 0x7FF) * 32 + y * 4 + x / 2];
    if x % 2 == 0 { b >> 4 } else { b & 0x0F }
}

/// NETWORK's pixels, 24 high, and how wide.
fn letters(vram: &Vram, lines: &[Line]) -> (Vec<[u8; 24]>, usize) {
    let mut columns: Vec<[u8; 24]> = vec![[BROWN; 24]; INDENT];
    let mut x0 = INDENT;
    for letter in NETWORK {
        let mut end = x0;
        for c in letter {
            let line = &lines[c.line];
            for x in c.from..c.to.min(line.width * 8) {
                let to = x0 + c.at + x - c.from;
                if columns.len() <= to {
                    columns.resize(to + 1, [BROWN; 24]);
                }
                for (y, out) in columns[to].iter_mut().enumerate() {
                    let px = pixel(vram, line.words[(y / 8) * line.width + x / 8], x % 8, y % 8);
                    if rank(px) > rank(*out) {
                        *out = px;
                    }
                }
                end = end.max(to + 1);
            }
        }
        x0 = end;
    }
    (columns, x0)
}

/// Tiles made here, from [`FIRST_TILE`] on.
struct Tiles(u16);

impl Tiles {
    /// Loads an 8 x 8 picture (`px(x, y)`) as a new tile and puts it in plane A at `at`
    /// (palette bits `bits`), unless it's all brown.
    fn put(&mut self, vram: &mut Vram, at: u16, bits: u16, px: impl Fn(usize, usize) -> u8) -> Result<(), String> {
        let mut pattern = [0u8; 32];
        for y in 0..8 {
            for x in 0..8 {
                pattern[y * 4 + x / 2] |= if x % 2 == 0 { px(x, y) << 4 } else { px(x, y) };
            }
        }
        if pattern.iter().all(|&b| b == BROWN << 4 | BROWN) {
            return Ok(());
        }
        if usize::from(self.0 - FIRST_TILE) >= TILES {
            return Err("the NETWORK line needs more tiles than are free".into());
        }
        vram.load(self.0 * 32, &pattern);
        vram.load(at, &(0x8000 | bits | self.0).to_be_bytes());
        self.0 += 1;
        Ok(())
    }
}

/// Draws the NETWORK line into plane A (rows `row` bytes apart) with the cursor on
/// `cursor` (after that line's plane list): NETWORK highlighted there, and then none
/// of the other lines. OPTIONS' bottom row is drawn again without the tips of the
/// leaves the ROM gives it (the board's edge was under it; NETWORK is now).
pub fn draw(rom: &Rom, vram: &mut Vram, cursor: u16, row: u16) -> Result<(), String> {
    let lit = cursor == crate::actors::NETWORK_LINE;
    let all = lines(rom, None)?;
    if lit {
        for line in lines(rom, Some(UNLIT))? {
            for (i, w) in line.words.iter().enumerate() {
                let at = line.at.wrapping_add((i / line.width) as u16 * row).wrapping_add((i % line.width) as u16 * 2);
                vram.load(at, &w.to_be_bytes());
            }
        }
    }
    let mut tiles = Tiles(FIRST_TILE);
    let options = &all[OPTIONS];
    let bits = if usize::from(cursor) == OPTIONS { LIT_BITS } else { 0 };
    for c in 0..options.width {
        let w = options.words[2 * options.width + c];
        // (the letters' pixels are $B-$E; the leaves' greens, below)
        let pattern: Vec<u8> = (0..64).map(|i| pixel(vram, w, i % 8, i / 8)).collect();
        if pattern.iter().any(|px| !(0x0B..=0x0E).contains(px)) {
            let at = options.at.wrapping_add(2 * row).wrapping_add(c as u16 * 2);
            tiles.put(vram, at, bits, |x, y| match pattern[y * 8 + x] {
                px @ 0x0B..=0x0E => px,
                _ => BROWN,
            })?;
        }
    }
    let (columns, width) = letters(vram, &all);
    // Under OPTIONS, from the same column.
    let at = options.at.wrapping_add(3 * row);
    let bits = if lit { LIT_BITS } else { 0 };
    for r in 0..3 {
        for c in 0..width.div_ceil(8) {
            let to = at.wrapping_add(r as u16 * row).wrapping_add(c as u16 * 2);
            tiles.put(vram, to, bits, |x, y| columns.get(c * 8 + x).map_or(BROWN, |col| col[r * 8 + y]))?;
        }
    }
    Ok(())
}
