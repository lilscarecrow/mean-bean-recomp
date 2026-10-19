//! The CPU opponent (Compile's Puyo Puyo AI as used by Mean Bean Machine).
//!
//! The original works on raw bytes of the field structure (`pVisiblePuyos` cell words,
//! the group marks `sub_5960` leaves in `pUnk1`, and its own scratch tables `pUnk2`..
//! `pUnk5`), so this port does the same on [`AiField`], a byte image laid out exactly
//! like the game's 874-byte field. That keeps every intermediate table comparable with
//! RAM.

use super::pair::{PadInput, PadSource, Pair, RotateKeys, PAD_A, PAD_C};
use super::bean_order::BeanColor;
use super::field::{Field, COLS, HIDDEN_ROWS, ROWS};
use super::rng::Rng;

/// High byte of a garbage cell: color 6 with link bits $D, as the game writes it.
const GARBAGE_CELL: u8 = 0xED;

/// Offsets in the field structure (`include/constants/ram.asm`).
pub mod off {
    pub const PLACEABLE: usize = 12; // pPlaceablePuyos: hidden row 1
    pub const VISIBLE: usize = 24; // pVisiblePuyos: 12 rows × 6 words
    pub const UNK1: usize = 168; // group marks per visible cell (byte at each word)
    pub const UNK2: usize = 660; // beans per color (8 bytes)
    pub const COUNT: usize = 668; // beans in the visible rows (word)
    pub const UNK3: usize = 670; // per column: free rows, top bean's group mark (6 words)
    pub const UNK4: usize = 682; // placement table for the current pair (12 × 8)
    pub const UNK5: usize = 778; // placement table for the next pair (12 × 8)
    pub const SIZE: usize = 874;
}

/// The field structure as bytes (68000 order).
#[derive(Clone, PartialEq, Eq)]
pub struct AiField {
    pub bytes: [u8; off::SIZE],
}

impl Default for AiField {
    fn default() -> Self {
        AiField { bytes: [0; off::SIZE] }
    }
}

impl std::fmt::Debug for AiField {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "AiField {{ .. }}")
    }
}

impl AiField {
    pub fn from_bytes(bytes: &[u8]) -> AiField {
        let mut f = AiField::default();
        f.bytes.copy_from_slice(&bytes[..off::SIZE]);
        f
    }

    /// High byte of a visible cell (row 0-11, col 0-5): bit 7 bean, bits 4-6 color,
    /// bits 0-3 links (left, right, up, down).
    fn cell(&self, byte_offset: usize) -> u8 {
        self.bytes[off::VISIBLE + byte_offset]
    }

    /// Write `field` the way the game's structure holds it once the field has settled
    /// (`sub_5960` ran last): each cell's high byte (bean bit, color, links to beans of
    /// the same color left/right/up/down within the visible rows) and each visible
    /// cell's group mark in `pUnk1` ($80 | size for groups of 2-3, $80 otherwise; a
    /// settled field has no groups of 4+), when `marks` (as `sub_5960` runs; garbage
    /// landing later leaves the marks alone). Cell low bytes and the tables stay.
    pub fn render(&mut self, field: &Field, marks: bool) {
        let vis = |c: i16, r: i16| (0..COLS as i16).contains(&c) && (0..12).contains(&r);
        let color = |c: i16, r: i16| field.get(c, r + HIDDEN_ROWS as i16);
        for row in 0..ROWS {
            for col in 0..COLS {
                let at = (row * COLS + col) * 2;
                self.bytes[at] = match field.get(col as i16, row as i16) {
                    None => 0,
                    Some(BeanColor::Garbage) => GARBAGE_CELL,
                    Some(c) => 0x80 | (c as u8) << 4,
                };
            }
        }
        let mut mark_bytes = [0u8; 144];
        let mut seen = [[false; COLS]; 12];
        for row in 0..12i16 {
            for col in 0..COLS as i16 {
                let at = (row as usize * COLS + col as usize) * 2;
                if seen[row as usize][col as usize] {
                    continue;
                }
                seen[row as usize][col as usize] = true;
                mark_bytes[at] = 0x80;
                let Some(c) = color(col, row).filter(|&c| c != BeanColor::Garbage) else { continue };
                // sub_59B0: flood the group, linking every bean to its same-color neighbours.
                let mut group = vec![(col, row)];
                let mut i = 0;
                while i < group.len() {
                    let (gc, gr) = group[i];
                    let mut links = 0u8;
                    for (bit, (dc, dr)) in [(8, (-1, 0)), (4, (1, 0)), (2, (0, -1)), (1, (0, 1))] {
                        let (nc, nr) = (gc + dc, gr + dr);
                        if !vis(nc, nr) || color(nc, nr) != Some(c) {
                            continue;
                        }
                        links |= bit;
                        if !seen[nr as usize][nc as usize] {
                            seen[nr as usize][nc as usize] = true;
                            group.push((nc, nr));
                        }
                    }
                    let cell = off::VISIBLE + (gr as usize * COLS + gc as usize) * 2;
                    self.bytes[cell] = self.bytes[cell] & 0xF0 | links;
                    i += 1;
                }
                let mark = if (2..4).contains(&group.len()) { 0x80 | group.len() as u8 } else { 0x80 };
                for (gc, gr) in group {
                    mark_bytes[(gr as usize * COLS + gc as usize) * 2] = mark;
                }
            }
        }
        if marks {
            self.bytes[off::UNK1..off::UNK1 + 144].copy_from_slice(&mark_bytes);
        }
    }

    /// Beans in the visible rows as of the last analysis (`pCount`).
    pub fn count(&self) -> u16 {
        self.word(off::COUNT)
    }

    fn word(&self, at: usize) -> u16 {
        u16::from_be_bytes([self.bytes[at], self.bytes[at + 1]])
    }

    fn set_word(&mut self, at: usize, w: u16) {
        self.bytes[at..at + 2].copy_from_slice(&w.to_be_bytes());
    }
}

/// `sub_12BAA` + `sub_12C14` + `sub_12B5C`: count the beans by color, then for each
/// column the free rows and the group mark of its top bean; columns the pair can't be
/// steered to (a full column in the way) count as having no room.
pub fn analyze(f: &mut AiField) {
    // pUnk2 (8 colors) and pCount
    for b in &mut f.bytes[off::UNK2..off::UNK2 + 10] {
        *b = 0;
    }
    for cell in 0..72 {
        let hi = f.cell(cell * 2);
        if hi == 0 {
            continue;
        }
        let color = ((hi >> 4) & 7) as usize;
        f.bytes[off::UNK2 + color] = f.bytes[off::UNK2 + color].wrapping_add(1);
        let count = f.word(off::COUNT).wrapping_add(1);
        f.set_word(off::COUNT, count);
    }
    // pUnk3: per column, scanning up from the bottom row
    for col in 0..6usize {
        let mut d1: u16 = 0x0C00;
        let mut at = col * 2 + 0x84;
        loop {
            if f.cell(at) != 0 {
                d1 = d1.wrapping_sub(0x100);
                d1 = (d1 & 0xFF00) | u16::from(f.bytes[off::UNK1 + at]);
            }
            if at < 0x0C {
                break;
            }
            at -= 0x0C;
        }
        f.set_word(off::UNK3 + col * 2, d1);
    }
    // sub_12B5C: columns the pair can't reach (unk_12B8C: columns in between, per column)
    const BETWEEN: [&[i16]; 6] = [&[2, 4], &[2], &[], &[-2], &[-2, -4], &[-2, -4, -6]];
    for col in (0..6usize).rev() {
        for &d in BETWEEN[col] {
            let other = (col as i16 * 2 + d) as usize;
            if f.bytes[off::UNK3 + other] == 0 {
                f.bytes[off::UNK3 + col * 2] = 0;
            }
        }
    }
}

/// `loc_12C38` (CPU in scenario mode only): how far along the match is, for the
/// planner: 2 if the CPU has 36+ beans, 1 if the player has, else 0 (`word_FF198C`).
pub fn crowding(cpu: &AiField, player: &AiField) -> u16 {
    if cpu.word(off::UNK2 + 8) >= 0x24 {
        2
    } else if player.word(off::UNK2 + 8) >= 0x24 {
        1
    } else {
        0
    }
}

/// Value of landing on a visible row, per column (`off_12D2C` → `unk_12D44`, `unk_12D50`,
/// `unk_12D5C`), laid out as in ROM: row 12 (reachable in the `next` pass) reads the
/// next table's first byte, or for the last table the first byte of `sub_12D68` ($4A).
#[rustfmt::skip]
const LANDING: [u8; 37] = [
    4, 5, 6, 7, 8, 9, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    2, 5, 6, 7, 8, 9, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    1, 2, 3, 7, 8, 9, 0x0A, 0x0B, 0x0C, 0x0D, 0x0E, 0x0F,
    0x4A,
];
const LANDING_BY_COL: [usize; 6] = [0, 12, 24, 12, 12, 0];

/// `sub_12C8A`: for each column and each of the two cells a vertical pair would fill,
/// a base value from how high it lands, then per color how much matching beans around
/// that cell add (written to `pUnk4`, or `pUnk5` for the `next` pass).
pub fn placement_table(f: &mut AiField, next: bool) {
    let table = if next { off::UNK5 } else { off::UNK4 };
    for d0 in 0..12usize {
        let a5 = table + d0 * 8;
        let col = d0 >> 1;
        let free = i16::from(f.bytes[off::UNK3 + col * 2]);
        let mut row = (d0 & 1) as i16 - 2 + free;
        let mut value = 0u8;
        if row >= 0 {
            // sub_12D68: on the `next` pass, look below the landing spot past beans of
            // the same color (they would pop out of the way).
            let mut blocked = false;
            if next {
                row += 1;
                if row >= 0x0C {
                    value = 0x1F;
                    blocked = true;
                } else {
                    let mut at = (row as usize * 6 + col) * 2;
                    let color = f.cell(at) & 0xF0;
                    at += 0x0C;
                    while f.cell(at) & 0xF0 == color {
                        at += 0x0C;
                        row += 1;
                        if row >= 0x0B {
                            break;
                        }
                    }
                }
            }
            if !blocked {
                value = LANDING[LANDING_BY_COL[col] + row as usize] | 0x10;
            }
        }
        for c in 0..8 {
            f.bytes[a5 + c] = value;
        }
        // sub_12DC2: neighbours of the landing cell
        if f.bytes[a5] == 0 || row < 0 {
            continue;
        }
        let at = (row as usize * 6 + col) * 2;
        neighbour(f, a5, at, col, -1);
        neighbour(f, a5, at, col, 1);
        // `bsr.w *+4` calls sub_12DEA and then falls into it: the bean below counts twice.
        below(f, a5, row as usize, at);
        below(f, a5, row as usize, at);
    }
}

/// `sub_12E36`: a sideways neighbour of the landing cell adds to its color's entry
/// (by its group mark: groups of 2-3 count more, 1 for a single bean).
fn neighbour(f: &mut AiField, a5: usize, at: usize, col: usize, side: i16) {
    let ncol = col as i16 + side;
    if !(0..6).contains(&ncol) {
        return;
    }
    add_neighbour(f, a5, (at as i16 + side * 2) as usize);
}

fn add_neighbour(f: &mut AiField, a5: usize, at: usize) {
    let hi = f.cell(at);
    if hi == 0 {
        return;
    }
    let color = ((hi >> 4) & 7) as usize;
    let mut weight = f.bytes[off::UNK1 + at] & 3;
    if weight == 0 {
        weight = 1;
    }
    f.bytes[a5 + color] = f.bytes[a5 + color].wrapping_add(weight << 4);
}

/// `sub_12DEA`: the bean below the landing cell counts too, unless it is garbage or
/// already linked sideways to a bean that reaches up.
fn below(f: &mut AiField, a5: usize, row: usize, at: usize) {
    if row >= 0x0B {
        return;
    }
    let b = at + 0x0C;
    let hi = f.cell(b);
    if hi == 0 || hi & 0x70 == 0x60 {
        return;
    }
    let sideways = hi & 0x0C;
    if (f.cell(b + 2) << 1) & 4 & sideways != 0 {
        return;
    }
    if (f.cell(b.wrapping_sub(2)) << 2) & 8 & sideways != 0 {
        return;
    }
    add_neighbour(f, a5, b);
}

// ---------------------------------------------------------------- planner (stage C)

/// Per-opponent AI records (`off_12584`, 8 bytes each), by `opponent`; a CPU playing
/// P1 uses record 5. Bytes: move-timer base (mode 0, mode 1), pause-timer base, bean
/// count that switches to mode 1, count that switches back, column preference bits,
/// pattern count ($FF none), bit 7 score weighting + allowed rotations (bits 0-1).
#[rustfmt::skip]
pub const RECORDS: [[u8; 8]; 16] = [
    [0x00, 0xC0, 0x03, 0x00, 0x00, 0x00, 0xFF, 0x00],
    [0x00, 0x60, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x20, 0x00, 0xFF, 0x3C, 0x30, 0x6E, 0xFF, 0x83],
    [0x00, 0x80, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x82],
    [0x00, 0x40, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x10, 0x00, 0xFF, 0x30, 0x30, 0x2C, 0xFF, 0x83],
    [0x40, 0x08, 0xFF, 0x30, 0x24, 0x00, 0x0C, 0x83],
    [0x00, 0x08, 0x08, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x00, 0x00, 0xFF, 0x3C, 0x30, 0x2A, 0xFF, 0x83],
    [0x00, 0x00, 0xFF, 0x30, 0x24, 0x66, 0xFF, 0x83],
    [0x00, 0x00, 0xFF, 0x30, 0x24, 0x00, 0x08, 0x83],
    [0x00, 0x00, 0xFF, 0x24, 0x24, 0x00, 0x08, 0x83],
    [0x00, 0x10, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x00, 0x20, 0x08, 0x00, 0x00, 0x00, 0xFF, 0x83],
    [0x00, 0x00, 0xFF, 0x00, 0x00, 0x00, 0xFF, 0x83],
];

/// Opponents with a special opening (`off_126F8`).
const OPP_FRANKLY: u8 = 1;
const OPP_COCONUTS: u8 = 7;

/// The 22 placements the planner compares: byte offsets into the 24 score words for
/// the pivot's and the satellite's cell (`unk_12860`), in the order it reads them; the
/// k-th one is placement 21 − k.
#[rustfmt::skip]
const PLACEMENT_CELLS: [(usize, usize); 22] = [
    (0x2C, 0x26), (0x24, 0x1E), (0x1C, 0x16), (0x14, 0x0E), (0x0C, 0x06),
    (0x24, 0x2E), (0x1C, 0x26), (0x14, 0x1E), (0x0C, 0x16), (0x04, 0x0E),
    (0x28, 0x2E), (0x20, 0x26), (0x18, 0x1E), (0x10, 0x16), (0x08, 0x0E), (0x00, 0x06),
    (0x2C, 0x2A), (0x24, 0x22), (0x1C, 0x1A), (0x14, 0x12), (0x0C, 0x0A), (0x04, 0x02),
];
/// Placement → (pivot column, rotation) (`byte_12B30`).
#[rustfmt::skip]
const PLACEMENTS: [(u8, u8); 22] = [
    (0, 0), (1, 0), (2, 0), (3, 0), (4, 0), (5, 0),
    (0, 2), (1, 2), (2, 2), (3, 2), (4, 2), (5, 2),
    (0, 1), (1, 1), (2, 1), (3, 1), (4, 1),
    (1, 3), (2, 3), (3, 3), (4, 3), (5, 3),
];
/// Score bonus lookups (`byte_12A2C`).
#[rustfmt::skip]
const NEIGHBOUR_SCORE: [u8; 40] = [
    0, 0, 0, 0, 3, 5, 7, 0xFF, 2, 4, 6, 0xFF, 1, 3, 5, 0xFF, 0, 0, 4, 0xFF,
    0, 0, 0, 0, 3, 5, 7, 0, 2, 4, 6, 0, 1, 3, 5, 0, 4, 6, 8, 0,
];

/// The planner's part of a CPU player actor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Target column (+$20) and rotation (+$21).
    pub col: u8,
    pub rot: u8,
    /// Move/pause timer (+$26) and phase (+$27, bit 7 = pausing).
    pub timer: u8,
    pub phase: u8,
    /// Frames to move (+$2C) and to pause (+$2D) per phase; $FF = never switch.
    pub move_frames: u8,
    pub pause_frames: u8,
}

/// Per-CPU memory between pieces (`byte_FF1D4E` / `byte_FF1D58`, 10 bytes): +0 mode,
/// +1 pattern active, +2 pattern column, +3 pieces before patterns, +4 pattern
/// pointer, +8/+9 pattern span.
pub type CpuMemory = [u8; 10];

/// Everything the planner reads besides the field.
pub struct PlanContext {
    pub player_id: u8,
    pub opponent: u8,
    /// `byte_FF0104`.
    pub difficulty: u8,
    pub time_minutes: u16,
    /// Pivot and satellite colors of the pair about to fall (the preview's +$26/+$27).
    pub pair: (u8, u8),
}

/// Scratch the planner leaves in RAM: `byte_FF1D16`..`FF1D1D` and the 24 score words
/// at `byte_FF1D1E`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PlanScratch {
    pub bytes: [u8; 8],
    pub scores: [u16; 24],
}

pub fn record_for(player_id: u8, opponent: u8) -> [u8; 8] {
    // sub_12562
    RECORDS[if player_id == 0 { 5 } else { opponent as usize & 15 }]
}

/// `sub_12906`: a timer of base/2 + RandomBound(base/2), or $FF for "never".
fn timer(base: u8, rng: &mut Rng) -> u8 {
    if base == 0xFF {
        return 0xFF;
    }
    let half = base >> 1;
    let r = rng.below(u16::from(half)) as u8;
    r.wrapping_add(half)
}

/// `sub_127BA`: pick where the next pair goes. Returns Err for pattern play.
pub fn plan(
    f: &AiField,
    ctx: &PlanContext,
    mem: &mut CpuMemory,
    p: &mut Plan,
    scratch: &mut PlanScratch,
    rng: &mut Rng,
) -> Result<(), &'static str> {
    let rec = record_for(ctx.player_id, ctx.opponent);
    let count = f.word(off::COUNT) as u8; // 9(a4): low byte of pCount
    // sub_12674: mode 0 builds, mode 1 (more beans on the field, or 2+ minutes) fights.
    if ctx.time_minutes >= 2 {
        mem[0] = 1;
    } else if mem[0] == 0 {
        if count >= rec[3] {
            mem[0] ^= 1;
            mem[1] = 0;
        }
    } else if count < rec[4] {
        mem[0] ^= 1;
    }
    let mode = mem[0];
    // sub_128B8
    let mut base = rec[mode as usize];
    let bump = ctx.difficulty.saturating_sub(2) << 4;
    let (sum, over) = base.overflowing_add(bump);
    base = if over { 0xFF } else { sum };
    p.move_frames = timer(base, rng);
    p.pause_frames = timer(rec[2], rng);
    p.phase = 0;
    p.timer = 0;
    // sub_12FAE: pattern play (a few opponents)
    if rec[6] & 0x80 == 0 && mode == 0 {
        if mem[3] < 8 {
            mem[3] += 1;
        } else {
            return Err("CPU pattern play (sub_12FAE)");
        }
    }
    // sub_126DE: openings
    if ctx.player_id != 0 {
        let free = |col: usize| f.bytes[off::UNK3 + col * 2];
        match ctx.opponent {
            OPP_FRANKLY if f.word(off::COUNT) < 0x18 => {
                // The emptiest column (the last one wins ties).
                let mut best = 0;
                for col in (0..6).rev() {
                    if free(col) >= best {
                        p.col = col as u8;
                        best = free(col);
                    }
                }
                p.rot = 0;
                p.move_frames = 0;
                p.pause_frames = 0xFF;
                return Ok(());
            }
            OPP_COCONUTS if free(0) | free(5) != 0 => {
                p.col = if free(0) < free(5) { 5 } else { 0 };
                p.rot = 0;
                p.move_frames = 0;
                p.pause_frames = 0xFF;
                return Ok(());
            }
            _ => {}
        }
    }
    // Scores per cell (column × upper/lower) for the pivot's and the satellite's color.
    for cell in 0..12usize {
        for (i, color) in [ctx.pair.0, ctx.pair.1].into_iter().enumerate() {
            scratch.scores[cell * 2 + i] = cell_score(f, &rec, mode, ctx.player_id, cell, color, &mut scratch.bytes);
        }
    }
    // Best placement; ties go to the lowest placement number.
    let (mut best, mut pick) = (0u16, 0usize);
    for (k, &(a, b)) in PLACEMENT_CELLS.iter().enumerate() {
        let placement = 21 - k;
        let (sa, sb) = (scratch.scores[a / 2], scratch.scores[b / 2]);
        let total = if sa == 0 || sb == 0 { 0 } else { sa.wrapping_add(sb) };
        if total >= best {
            best = total;
            pick = placement;
        }
    }
    let (col, rot) = PLACEMENTS[pick];
    p.col = col;
    p.rot = rot & (rec[7] & 3);
    Ok(())
}

/// `sub_12920` for one cell and color. The result's high byte is $FF: the register
/// still holds -1 from an earlier `dbf` loop (`sub_1295E`).
fn cell_score(f: &AiField, rec: &[u8; 8], mode: u8, player_id: u8, cell: usize, color: u8, s: &mut [u8; 8]) -> u16 {
    // s[0..8] = FF1D16..FF1D1D
    // sub_1295E: column preference bit (mode 0 only)
    let prefs = if mode == 0 { rec[5] } else { 0 };
    let col = cell >> 1;
    s[7] = (prefs << (col + 1)) & 0x80;
    // sub_12AA4
    let base = off::UNK4 + cell * 8;
    let here = f.bytes[base + color as usize];
    s[0] = here & 0x0F;
    s[3] = here >> 4;
    s[4] = best_other(f, base, color);
    s[5] = f.bytes[base + 0x60 + color as usize] >> 4;
    s[6] = best_other(f, base + 0x60, color);
    // sub_12982: low landing spots
    if s[0].wrapping_sub(1) < 3 {
        s[0] = s[0].wrapping_add(3);
        if s[3] < 4 {
            s[3] = 0;
            s[5] = 0;
            s[0] = s[0].wrapping_sub(4);
        }
    }
    // sub_129B8
    s[1] = 0;
    s[2] = 0;
    if s[3] != 0 {
        let mut i = (s[3] - 1).min(3) | (s[4].min(4) << 2);
        if s[7] != 0 {
            i += 0x14;
        }
        let v = NEIGHBOUR_SCORE[i as usize];
        s[1] = if v & 0x80 != 0 { s[3].wrapping_mul(2).wrapping_add(1) } else { v };
    }
    if s[5] != 0 {
        let i = (s[5] - 1).min(3) | (s[6].min(4) << 2);
        let v = NEIGHBOUR_SCORE[i as usize];
        s[2] = if v & 0x80 != 0 { s[5].wrapping_mul(2).wrapping_add(1) } else { v };
    }
    let (mut same, mut next) = (s[1], s[2]);
    let weighted = ((player_id << 7) ^ 0x80) | rec[7];
    if weighted & 0x80 != 0 {
        same <<= 3;
        next <<= 1;
    }
    0xFF00 | u16::from(s[0].wrapping_add(same).wrapping_add(next))
}

/// `sub_12AEA`: the strongest entry among the other colors (0-5) of a cell, / 16.
fn best_other(f: &AiField, base: usize, color: u8) -> u8 {
    let mut best = 0u8;
    for c in 0..6usize {
        if c == color as usize {
            continue;
        }
        best = best.max(f.bytes[base + c]);
    }
    best >> 4
}

/// `sub_12646`: whether the CPU plays this player (`level_mode`, player id 0/1,
/// `control_player_1`). Bit 2 of the mode is the attract demo, where both are CPUs.
pub fn is_cpu(level_mode: u8, player_id: u8, control: u8) -> bool {
    level_mode & 4 != 0 || (((level_mode << 1) | player_id) ^ 1) & control == 0
}

// ---------------------------------------------------------------- pad (stage D)

/// Sideways auto-repeat bit toward the target: right, left (`byte_12F04`).
const TOWARD: [u8; 2] = [0x08, 0x04];
/// Which way the CPU's on-screen controller leans (`byte_12F46`), by left/right bits.
const LEAN: [u8; 4] = [0, 0x84, 0x88, 0];

/// `sub_12E6C`: the CPU's controller, worked out each time the pivot reads its pad.
pub struct CpuPad<'a> {
    /// The human pad on the CPU's port. Revision A keeps part of it: everything but
    /// down, the rotate presses and the sideways repeats.
    pub human: PadInput,
    pub plan: &'a mut Plan,
    pub player_id: u8,
    /// `level`: below 4, the CPU playing P2 only steps sideways every 16th read and
    /// sometimes the wrong way.
    pub level: u8,
    /// `byte_FF1D0A` / `byte_FF1D0B`: pad reads since the plan, saturating at $FF.
    pub reads: &'a mut u8,
    /// `byte_FF1D0C` (a rotate was pressed) and `byte_FF1D0D` (lean: $80 down, $84
    /// left, $88 right), picked up by the CPU's controller sprites.
    pub pressed: &'a mut u8,
    pub lean: &'a mut u8,
    pub rng: &'a mut Rng,
}

impl PadSource for CpuPad<'_> {
    fn read(&mut self, pair: &Pair) -> PadInput {
        let mut hold = self.human.hold & 0xFD;
        let mut press = self.human.press & 0x8F;
        let mut repeat = self.human.repeat & 0xF3;
        *self.reads = self.reads.checked_add(1).unwrap_or(0xFF);
        // sub_12F82: alternate between moving and pausing.
        let p = &mut *self.plan;
        let limit = if p.phase != 0 { p.pause_frames } else { p.move_frames };
        if p.timer > limit {
            p.timer = 0;
            p.phase ^= 0x80;
        }
        // Hold down while pausing, unless there is still somewhere to go.
        let mut down = u16::from(p.phase) << 2;
        let dx = p.col.wrapping_sub(pair.col as u8);
        if dx != 0 {
            down = 0;
            repeat |= TOWARD[usize::from(dx >> 7)];
            if self.player_id != 0 && self.level < 4 && p.phase & 0x80 == 0 && *self.reads != 0xFF {
                // sub_12F4A
                if *self.reads & 0x0F != 0 {
                    repeat &= 0xF3;
                } else if self.rng.next() & 1 != 0 {
                    repeat ^= 0x0C;
                }
            }
        }
        let dr = p.rot.wrapping_sub(pair.rot);
        if dr != 0 {
            down = 0;
            press |= PAD_C;
            if dr & 3 == 3 {
                press ^= PAD_C | PAD_A;
            }
        }
        hold |= (down >> 8) as u8;
        press |= down as u8;
        // sub_12F06
        press &= 0x70;
        if press != 0 {
            *self.pressed = 0xFF;
        }
        if hold & 0x02 != 0 {
            *self.lean = 0x80;
        } else if LEAN[usize::from(repeat >> 2 & 3)] != 0 {
            *self.lean = LEAN[usize::from(repeat >> 2 & 3)];
        }
        PadInput { hold, press, repeat }
    }
}

/// The CPU's turn buttons: its pad's $40 turns left and $20 right, as the human
/// assignments would map them (`sub_5712` is skipped for the CPU).
pub const CPU_KEYS: RotateKeys = RotateKeys([1, 0, 2]);
