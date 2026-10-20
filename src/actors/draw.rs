//! What the field looks like in plane A. The game draws cells as tile pairs when they
//! change (`MarkPuyoSpot`, `sub_5782`) and animates beans falling after a pop
//! (`SpecPlane89`); it doesn't redraw what didn't change, so the plane keeps traces of
//! that history (cells a falling bean passed through show its top tile twice). The
//! engine mirrors the cell bytes the drawing reads (color, bean bit, links) and their
//! last drawn copies, and records the tile writes in `plane_writes` for a renderer.

use super::*;
use crate::assets::planes::{cell_words, FIELD_ORIGIN, ROW_BYTES};

/// The cell bytes of one field (14 rows x 6): the high byte of each cell word, and the
/// low byte `sub_5782` keeps as "last drawn" ($FF: not drawn since the field was reset).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldBytes {
    pub cells: [u8; COLS * ROWS],
    pub drawn: [u8; COLS * ROWS],
}

impl Default for FieldBytes {
    /// `ResetPuyoField`: every word $00FF.
    fn default() -> Self {
        FieldBytes { cells: [0; COLS * ROWS], drawn: [0xFF; COLS * ROWS] }
    }
}

/// The byte `MarkPuyoSpot` writes for a bean: bean bit and color; garbage is $ED.
pub fn bean_byte(color: BeanColor) -> u8 {
    if color == BeanColor::Garbage { 0xED } else { 0x80 | (color as u8) << 4 }
}

/// `unk_1A928`..`unk_1A970`: the frames (link nibbles) of a bean landing after a pop,
/// by color group, ending at $FF.
const BOUNCE_FRAMES: [&[u8]; 4] = [
    &[1, 1, 2, 2, 2, 1, 3, 3, 1, 2, 2, 2, 1, 3, 3, 1, 2, 2, 1, 3, 1, 2, 0xFF],
    &[1, 2, 2, 2, 2, 1, 3, 1, 2, 1, 3, 1, 2, 1, 3, 1, 2, 1, 3, 1, 2, 1, 3, 0xFF],
    &[1, 1, 2, 2, 2, 2, 3, 3, 2, 2, 2, 2, 3, 3, 2, 1, 3, 3, 1, 3, 3, 1, 0xFF],
    &[1, 0xFF],
];

impl Engine {
    fn put_words(&mut self, at: u16, words: &[u16]) {
        for (i, &w) in words.iter().enumerate() {
            self.plane_writes.push((at.wrapping_add(i as u16 * 2), w));
        }
    }

    /// `DrawPuyo`: a cell (row counted from the top of the field, 2 and down visible).
    fn draw_cell(&mut self, player: usize, col: usize, row: usize, cell: u8) {
        let at = FIELD_ORIGIN[player].wrapping_add(((row as u16).wrapping_sub(2)) * 0x100).wrapping_add(col as u16 * 4);
        let [tl, tr, bl, br] = cell_words(cell);
        self.put_words(at, &[tl, tr]);
        self.put_words(at + ROW_BYTES, &[bl, br]);
    }

    /// `ResetPuyoField`.
    pub(super) fn reset_field_bytes(&mut self, player: usize) {
        self.field_bytes[player] = FieldBytes::default();
    }

    /// `MarkPuyoSpot`: a landed bean's byte, drawn at once if it is visible.
    pub(super) fn mark_bytes(&mut self, player: usize, col: usize, row: usize, color: BeanColor) {
        let b = bean_byte(color);
        let i = row * COLS + col;
        self.field_bytes[player].cells[i] = b;
        self.field_bytes[player].drawn[i] = b;
        if row >= HIDDEN_ROWS {
            self.draw_cell(player, col, row, b);
        }
    }

    /// `sub_5960`'s links on the visible beans (from the CPU's byte view, which renders
    /// them the same way).
    pub(super) fn relink_bytes(&mut self, player: usize) {
        let ai = &self.ai_fields[player].bytes;
        for i in HIDDEN_ROWS * COLS..ROWS * COLS {
            self.field_bytes[player].cells[i] = ai[(i - HIDDEN_ROWS * COLS) * 2 + super::ai::off::VISIBLE];
        }
    }

    /// `sub_5782`: draw the visible cells whose byte changed since last drawn.
    pub(super) fn redraw_field(&mut self, player: usize) {
        for i in HIDDEN_ROWS * COLS..ROWS * COLS {
            let FieldBytes { cells, drawn } = &mut self.field_bytes[player];
            if cells[i] != drawn[i] {
                drawn[i] = cells[i];
                let cell = cells[i];
                self.draw_cell(player, i % COLS, i / COLS, cell);
            }
        }
    }

    /// `sub_49D2`: the popping beans (and cleared garbage) shown or hidden, then redrawn.
    pub(super) fn blink_bytes(&mut self, player: usize, cells: &[(usize, usize)], backup: &[u8; COLS * ROWS], shown: bool) {
        for &(c, r) in cells {
            let i = r * COLS + c;
            self.field_bytes[player].cells[i] = if shown { backup[i] } else { 0 };
        }
        self.redraw_field(player);
    }

    /// `sub_4DB8`: each column drops its beans' bytes to the bottom (rows 1-13). A moved
    /// byte's drawn copy becomes 0; a vacated cell keeps its old one.
    pub(super) fn compact_bytes(&mut self, player: usize) {
        let FieldBytes { cells, drawn } = &mut self.field_bytes[player];
        for col in (0..COLS).rev() {
            let mut to = ROWS - 1;
            for row in (1..ROWS).rev() {
                let i = row * COLS + col;
                if cells[i] == 0 {
                    continue;
                }
                let b = cells[i];
                cells[i] = 0;
                cells[to * COLS + col] = b;
                drawn[to * COLS + col] = 0;
                to -= 1;
            }
        }
    }

    /// `SpecPlane89`'s drawing of one step of a bean falling after a pop (from its old
    /// cell, `half` tile rows down; `frame` > 0 while it bounces, with its link nibble).
    pub(super) fn draw_fall_step(&mut self, player: usize, b: &FallingBean, color: BeanColor) {
        let drop = b.rows * 2;
        let (half, bounce) = if b.counter >= drop { (drop - 1, b.counter - drop + 1) } else { (b.counter, 0) };
        let group = if color == BeanColor::Garbage { 3 } else { ((color as usize) >> 1) & 3 };
        let frame = BOUNCE_FRAMES[group][usize::from(bounce)];
        let byte = if color == BeanColor::Garbage { 0xED } else { (color as u8) << 4 | frame };
        let tile = crate::assets::planes::cell_pattern(byte);
        let at = FIELD_ORIGIN[player]
            .wrapping_add((b.row as u16).wrapping_sub(2).wrapping_mul(0x100))
            .wrapping_add(b.col as u16 * 4)
            .wrapping_add(u16::from(half) * ROW_BYTES);
        // How much shows: the bean starts above the visible rows (row 1).
        let mut mode = (u16::from(half) + 2 * (b.row as u16 - 1)).min(2);
        if bounce != 0 {
            mode = 1;
        }
        if mode == 2 {
            self.put_words(at, &[0x83FE, 0x83FE]);
        }
        if mode >= 1 {
            self.put_words(at + ROW_BYTES, &[tile, tile + 2]);
        }
        self.put_words(at + 2 * ROW_BYTES, &[tile + 1, tile + 3]);
    }
}
