//! The centre panel between the fields: the "NEXT" and "SCORE" labels, "STAGE n", the
//! player names, both scores (or a chain step's points while it pops) and the
//! opponent's portrait. The game queues these as plane commands (`QueuePlaneCmd`) and
//! the portrait's map in the Enigma queue (`EniDec`); the vertical blank draws them
//! (`ProcPlaneCommands`, then `ProcEniTilemapQueue`). The engine keeps the same queues
//! and records the tile writes in `plane_writes`.

use super::*;
use crate::mbm::assets::enigma;
use crate::mbm::rom;
use crate::mbm::assets::planes::{FIELD_ORIGIN, ROW_BYTES};

/// A queued plane command that draws part of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PanelCommand {
    /// $80 (`SpecPlane80`): a `StageTextStrings` entry, with its blink byte and the
    /// player its field-relative text and digit are for.
    Text { entry: u8, blink: u8, player: u8 },
    /// $81/$82: a player's score (`sub_9C4E`).
    Score(u8),
    /// $86/$87: a chain step's points, "base x bonus", where the score is (`sub_9A40`).
    Chain(u8),
    /// $97 (`Level_DrawSmallText`): "STAGE n" and the background under it.
    StageText,
    /// $9A: "1P" and "DR R" over the fields.
    Names,
}

/// `StageTextStrings`: per entry, a count of strings - 1, then per string the plane
/// address (0: a field-relative offset follows), the tile base, the tile bytes up to $FF
/// (padded to even).
const STAGE_TEXT_STRINGS: u32 = 0x19CDA;
/// `sub_3CDA`'s entry in scenario mode: NEXT and SCORE.
pub const PANEL_LABELS: u8 = 6;
/// `Str_Stage` ("STAGE n", levels below 12) and `Str_Stage1` ("STAGE 1n").
const STR_STAGE: u32 = 0x1A474;
const STR_STAGE_1: u32 = 0x1A47C;
/// `Str_1P` and `Str_DrR` (4 bytes each).
const STR_1P: u32 = 0x1A1C8;
const STR_DR_R: u32 = 0x1A1CC;
/// `StageTextBGTiles`: plane B's tiles under the stage text (2 rows of 8, base $C000).
const STAGE_TEXT_BG: [[u8; 8]; 2] = [[0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18], [0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29]];
/// Where each player's score is drawn (`word_FF198A` for P1, set by `loc_3C00`; the
/// controls aren't swapped) and its tile base (font palette line 0 / 1).
const SCORE_AT: [u16; 2] = [0xCB1E, 0xCC22];
const SCORE_BASE: [u16; 2] = [0x8500, 0xA500];
/// `byte_1AF64`: the tiles of the score buffer's special bytes ($80 blank, $81 "x").
const SPECIAL_TILES: [u8; 2] = [0, 0x4A];
/// The portrait (`loc_6070`): its maps go to $C61E, tiles from VRAM $8000 on palette
/// line 3, 10 x 7 cells.
const PORTRAIT_AT: u16 = 0xC61E;
const PORTRAIT_BASE: u16 = 0xE400;
const PORTRAIT_SIZE: (usize, usize) = (10, 7);

impl Engine {
    /// `QueuePlaneCmd` for the panel.
    pub(super) fn queue_panel(&mut self, cmd: PanelCommand) {
        self.panel_commands.push(cmd);
    }

    /// The vertical blank's share: the queued panel commands, then the Enigma queue.
    pub(super) fn run_panel_commands(&mut self) {
        for cmd in std::mem::take(&mut self.panel_commands) {
            match cmd {
                PanelCommand::Text { entry, blink, player } => self.draw_text(entry, blink, player),
                PanelCommand::Score(id) => {
                    let score = self.player_by_id(id).map_or(0, |p| p.score);
                    self.draw_number(id, &score_digits(score));
                }
                PanelCommand::Chain(id) => {
                    let (base, bonus) = self.player_by_id(id).map_or((0, 0), |p| (p.base, p.bonus));
                    self.draw_number(id, &chain_digits(base, bonus));
                }
                PanelCommand::StageText => self.draw_stage_text(),
                PanelCommand::Names => {
                    self.small_text(0xC21E, 0xA500, STR_1P, 4);
                    self.small_text(0xC22A, 0xA500, STR_DR_R, 4);
                }
            }
        }
        if let Some(map) = self.portrait_map.take() {
            self.draw_portrait(map);
        }
    }

    fn player_by_id(&self, id: u8) -> Option<&Player> {
        self.slots.iter().flatten().find_map(|a| match a {
            Actor::Player(p) if p.id == id => Some(p),
            _ => None,
        })
    }

    /// A tile byte and the one under it (the next tile), then the next column.
    fn tile_pair(&mut self, at: u16, base: u16, b: u8) {
        self.plane_writes.push((at, (base & 0xFF00) | u16::from(b)));
        self.plane_writes.push((at.wrapping_add(ROW_BYTES), (base & 0xFF00) | u16::from(b.wrapping_add(1))));
    }

    /// `DrawSmallText`: `count` bytes from the ROM as tile pairs.
    fn small_text(&mut self, at: u16, base: u16, text: u32, count: u32) {
        for i in 0..count {
            self.tile_pair(at.wrapping_add(i as u16 * 2), base, rom::byte(text + i));
        }
    }

    /// `SpecPlane80`.
    fn draw_text(&mut self, entry: u8, blink: u8, player: u8) {
        let mut at = rom::long(STAGE_TEXT_STRINGS + u32::from(entry) * 4);
        // The blink byte: 0 shows the text, negative masks it, positive hides it while
        // frame_count bit 4 is set.
        let shown = match blink {
            0 => 0xFF,
            b if b & 0x80 != 0 => !b,
            _ if self.frame_count & 0x10 != 0 => 0,
            _ => 0xFF,
        };
        let strings = rom::word(at).wrapping_add(1);
        at += 2;
        let mut read = 0u32;
        for _ in 0..strings {
            at += read & 1;
            read = 0;
            let mut to = rom::word(at);
            at += 2;
            if to == 0 {
                to = FIELD_ORIGIN[usize::from(player & 1)].wrapping_add(rom::word(at));
                at += 2;
            }
            let base = rom::word(at);
            at += 2;
            loop {
                let b = rom::byte(at);
                at += 1;
                read += 1;
                if b == 0xFF {
                    break;
                }
                let mut b = b & shown;
                if b == 0xFE {
                    b = player.wrapping_add(0x37) << 1;
                }
                self.tile_pair(to, base, b);
                to = to.wrapping_add(2);
            }
        }
    }

    /// `loc_1AF0C`: 8 bytes of the score buffer as tile pairs where the player's score is.
    fn draw_number(&mut self, id: u8, buffer: &[u8; 8]) {
        let side = usize::from(id & 1);
        for (i, &b) in buffer.iter().enumerate() {
            let tile = if b & 0x80 != 0 { SPECIAL_TILES[usize::from(b & 1)] } else { (b + 1) << 1 };
            self.tile_pair(SCORE_AT[side].wrapping_add(i as u16 * 2), SCORE_BASE[side], tile);
        }
    }

    /// `SpecPlane97`: `GetStageText`, `DrawStageText`, `DrawBGUnderStageText`.
    fn draw_stage_text(&mut self) {
        let (text, digit, n) = if self.level < 0x0C { (STR_STAGE, 6, self.level.wrapping_sub(3)) } else { (STR_STAGE_1, 7, self.level.wrapping_sub(0x0D)) };
        let mut buffer: [u8; 8] = std::array::from_fn(|i| rom::byte(text + i as u32));
        buffer[digit] = n.wrapping_add(0x37) << 1;
        for (i, &b) in buffer.iter().enumerate() {
            self.tile_pair(0xC520 + i as u16 * 2, 0xC500, b);
        }
        for (row, tiles) in STAGE_TEXT_BG.iter().enumerate() {
            for (col, &t) in tiles.iter().enumerate() {
                self.plane_writes.push((0xE520 + row as u16 * ROW_BYTES + col as u16 * 2, 0xC000 | u16::from(t)));
            }
        }
    }

    /// `ProcEniTilemapQueue` for the portrait's map (decoded by `sub_60F4`).
    fn draw_portrait(&mut self, map: u32) {
        let Some(rom) = rom::get() else { return };
        let Ok((words, _)) = enigma::decompress(&rom[map as usize..], PORTRAIT_BASE) else { return };
        let (w, h) = PORTRAIT_SIZE;
        for (i, &word) in words.iter().take(w * h).enumerate() {
            let at = PORTRAIT_AT + (i / w) as u16 * ROW_BYTES + (i % w) as u16 * 2;
            self.plane_writes.push((at, word));
        }
    }
}

/// `sub_1B350`: a score as 8 digits (leading zeros shown).
fn score_digits(score: u32) -> [u8; 8] {
    let (hi, lo) = (score / 10000, score % 10000);
    std::array::from_fn(|i| {
        let (v, k) = if i < 4 { (hi, 3 - i) } else { (lo, 7 - i) };
        (v / 10u32.pow(k as u32) % 10) as u8
    })
}

/// `SpecPlane86`'s buffer: right-aligned "base x bonus" (just the base with no bonus),
/// blanks before it.
fn chain_digits(base: u16, bonus: u16) -> [u8; 8] {
    // sub_1B396: the digits of v, last first (none for 0).
    fn put(buffer: &mut [u8; 8], end: &mut usize, mut v: u32) {
        while v != 0 {
            *end -= 1;
            buffer[*end] = (v % 10) as u8;
            v /= 10;
        }
    }
    let mut buffer = [0x80u8; 8];
    let mut end = 8;
    if bonus != 0 {
        put(&mut buffer, &mut end, u32::from(bonus));
        end -= 1;
        buffer[end] = 0x81;
    }
    put(&mut buffer, &mut end, u32::from(base) % 10000);
    buffer
}
