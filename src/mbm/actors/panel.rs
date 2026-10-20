//! The centre panel between the fields: the "NEXT" and "SCORE" labels, "STAGE n", the
//! player names, both scores (or a chain step's points while it pops) and the
//! opponent's portrait. The game queues these as plane commands (`QueuePlaneCmd`) and
//! the portrait's map in the Enigma queue (`EniDec`); the vertical blank draws them
//! (`ProcPlaneCommands`, then `ProcEniTilemapQueue`). The engine keeps the same queues
//! and records the tile writes in `plane_writes`.

use super::*;
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
    /// A `PlaneCmdLists` entry (`QueuePlaneCmdList`), for the front end's VRAM.
    List(u16),
    /// The intro's text box: $8C / $9F (Robotnik's colors) save what's under it and
    /// draw it, $8D puts the plane back, $8E clears its inside. `size` is lines << 5 |
    /// characters, `at` its plane address.
    IntroBox { kind: u8, size: u8, at: u16 },
    /// $84 (`loc_833E`): a frame (0-4) of the loser's field crumbling away: plane B's
    /// ground under the field, and (frame 0) plane A cleared above and below it.
    Wipe { player: u8, frame: u8 },
    /// $9B (`loc_72A4`): the win banner over P1's field opening from the middle; the
    /// countdown (10 to 1) places its top and bottom edges.
    Banner(u8),
    /// $98 (`sub_7CD2`): the results' time line, with the seconds.
    Time(u16),
    /// $99: the results' bonus line, with the points left to add.
    Bonus(u16),
    /// $92: the game over countdown's digit (`byte_66C9A`, 5 x 7 tiles).
    Countdown(u8),
}

/// `SpecPlane92`'s digits (5 x 7 words each, less $1E00) and where they go.
const COUNTDOWN_DIGITS: u32 = 0x66C9A;
const COUNTDOWN_AT: u16 = 0xE70C;

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
/// `sub_72CA`'s entry once the banner is open: "YOU WIN" (blinking).
pub const WIN_TEXT: u8 = 0x0C;
/// `byte_1B0DA`: the crumbling ground by level (3 = the lessons' board), and
/// `off_1B0EA`: per board and side, a descriptor: map, bytes per frame, fill word, map
/// base, width - 1, plane B address, plane A below and above the field.
const WIPE_BOARD: u32 = 0x1B0DA;
const WIPE_DESCRIPTORS: u32 = 0x1B0EA;
/// `word_1A12E`: the banner's top edge (12 words) over its fill; `word_1A146`: fill
/// over the bottom edge.
const BANNER_TOP: u32 = 0x1A12E;
const BANNER_BOTTOM: u32 = 0x1A146;
const BANNER_AT: u16 = 0xC204;
/// `sub_7D78`'s entry when there's no time bonus.
pub const NO_BONUS_TEXT: u8 = 5;
/// `loc_7E68`'s entry after the last stage.
pub const FINAL_TEXT: u8 = 0x16;
/// The results' lines over P1's field (`DrawPlayerText`, 10 characters, base $A500):
/// `byte_1A20C`/`byte_1A216` (time), `unk_1A25C`/`unk_1A266` (bonus), `unk_1A38E`
/// (over the password); and where their numbers go (5 digits, base $8500).
const RESULT_LINES: [(u16, u32); 5] = [(0xC506, 0x1A20C), (0xC606, 0x1A216), (0xC706, 0x1A25C), (0xC806, 0x1A266), (0xC906, 0x1A38E)];
const TIME_AT: u16 = 0xC608;
/// `unk_C156` ("RANK") and `unk_C15C` ("YOUR NAME"): name entry characters to a
/// negative byte.
const NAME_RANK_TEXT: u32 = 0xC156;
const NAME_PROMPT_TEXT: u32 = 0xC15C;
const BONUS_AT: u16 = 0xC808;

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
                PanelCommand::Wipe { player, frame } => self.draw_wipe(player, frame),
                PanelCommand::IntroBox { kind, size, at } => self.draw_intro_box(kind, size, at),
                PanelCommand::List(list) => {
                    let row = self.plane_row();
                    self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::PlaneList { list, row } });
                }
                PanelCommand::Time(secs) => {
                    self.result_line(0);
                    self.result_line(1);
                    self.result_number(TIME_AT, secs);
                }
                PanelCommand::Bonus(points) => {
                    self.result_line(2);
                    self.result_line(3);
                    self.result_number(BONUS_AT, points);
                }
                PanelCommand::Countdown(digit) => {
                    let from = COUNTDOWN_DIGITS + u32::from(digit) * 0x46;
                    for i in 0..35u16 {
                        let to = COUNTDOWN_AT.wrapping_add((i / 5) * 0x80).wrapping_add((i % 5) * 2);
                        self.plane_writes.push((to, rom::word(from + u32::from(i) * 2).wrapping_sub(0x1E00)));
                    }
                }
                PanelCommand::Banner(t) => {
                    // SpecPlane9B: 2 rows from the top edge down, then 2 rows from the
                    // fill down to the bottom edge.
                    let top = BANNER_AT.wrapping_add(u16::from(t.wrapping_sub(1)) << 7);
                    self.copy_words(top, BANNER_TOP, 12, 2);
                    let bottom = BANNER_AT.wrapping_add(u16::from(0x12u8.wrapping_sub(t)) << 7);
                    self.copy_words(bottom, BANNER_BOTTOM, 12, 2);
                }
            }
        }
        self.run_eni_queue();
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

    /// A line of the results (`DrawPlayerText`; the controls aren't swapped).
    pub(super) fn result_line(&mut self, line: usize) {
        let (at, text) = RESULT_LINES[line];
        self.small_text(at, 0xA500, text, 10);
    }

    /// `loc_1A3BA`: a number right-aligned in 5 characters ("0" for none).
    fn result_number(&mut self, at: u16, v: u16) {
        let mut buffer = [0u8, 0, 0, 0, 2];
        let (mut v, mut i) = (v, 5);
        while v != 0 {
            i -= 1;
            buffer[i] = ((v % 10) as u8 + 1) << 1;
            v /= 10;
        }
        for (k, &b) in buffer.iter().enumerate() {
            self.tile_pair(at + k as u16 * 2, 0x8500, b);
        }
    }

    /// `sub_C1E8`: a name entry character (0 blank, 1-26 letters, more for the rest)
    /// as a tile pair; the end mark ($B6) blinks with the frame counter.
    fn name_char(&mut self, at: u16, ch: u8) {
        let mut c = ch;
        if c != 0 {
            c = c.wrapping_add(0x3F);
            if c >= 0x5F {
                c = c.wrapping_sub(0x29);
            }
        }
        let b = c << 1;
        let (top, bottom) = if b == 0xB6 {
            (0xC500, 0xC5B6 | ((self.frame_count >> 3) & 1))
        } else {
            (0xC500 | u16::from(b), 0xC500 | u16::from(b.wrapping_add(1)))
        };
        self.plane_writes.push((at, top));
        self.plane_writes.push((at.wrapping_add(ROW_BYTES), bottom));
    }

    /// `loc_C00A`: "RANK n" and "YOUR NAME" over the player's field.
    pub(super) fn name_prompt(&mut self, player: u8, rank: u8) {
        let origin = FIELD_ORIGIN[usize::from(player & 1)];
        let mut at = origin + 0x606;
        for i in 0.. {
            let ch = rom::byte(NAME_RANK_TEXT + i);
            if ch & 0x80 != 0 {
                break;
            }
            self.name_char(at, ch);
            at += 2;
        }
        self.name_char(at, rank.wrapping_add(0x21));
        let mut at = origin + 0x702;
        for i in 0.. {
            let ch = rom::byte(NAME_PROMPT_TEXT + i);
            if ch & 0x80 != 0 {
                break;
            }
            self.name_char(at, ch);
            at += 2;
        }
    }

    /// `sub_C168`: the 3 letters: those before the cursor, the cursor's (hidden while
    /// `blink` bit 3 is set), then the "unset" mark; all blank with `hide`.
    pub(super) fn name_letters(&mut self, player: u8, letters: &[u8], cursor: u16, blink: u8, hide: bool) {
        let at = FIELD_ORIGIN[usize::from(player & 1)] + 0x888;
        for (i, &l) in letters.iter().take(3).enumerate() {
            let i = i as u16;
            let mut ch = match i.cmp(&cursor) {
                std::cmp::Ordering::Less => l,
                std::cmp::Ordering::Equal => if blink & 8 != 0 { 0 } else { l },
                std::cmp::Ordering::Greater => 0x1C,
            };
            if hide {
                ch = 0;
            }
            self.name_char(at + i * 2, ch);
        }
    }

    /// `SpecPlane8C` / `9F` / `8D` / `8E` (`loc_1A7C4`: d3 = characters + 1, d4 = lines
    /// * 2).
    fn draw_intro_box(&mut self, kind: u8, size: u8, at: u16) {
        let row = self.plane_row();
        let d3 = u16::from(size & 0x1F) + 1;
        let d4 = u16::from(size >> 5) * 2;
        let event = |e: &mut Engine, what: VdpOp| e.vdp_events.push(VdpEvent { at: e.plane_writes.len(), what });
        match kind {
            0x8C | 0x9F => {
                event(self, VdpOp::SaveBox { at, cols: d3 + 1, rows: d4 + 1, row });
                let base: u16 = if kind == 0x8C { 0xE300 } else { 0xE100 };
                let (w, h) = (d3 - 2, d4 - 2);
                for c in 0..=w {
                    self.plane_writes.push((at.wrapping_add(2 + c * 2), base | 0xF8));
                }
                let mut line = at.wrapping_add(row);
                for _ in 0..=h {
                    self.plane_writes.push((line, base | 0xFA));
                    for c in 0..=w {
                        self.plane_writes.push((line.wrapping_add(2 + c * 2), base | 0xFB));
                    }
                    self.plane_writes.push((line.wrapping_add(2 + (w + 1) * 2), base | 0xFC));
                    line = line.wrapping_add(row);
                }
                for c in 0..=w {
                    self.plane_writes.push((line.wrapping_add(2 + c * 2), base | 0xFE));
                }
            }
            0x8D => event(self, VdpOp::RestoreBox { at, cols: d3 + 1, rows: d4 + 1, row }),
            // 0x8E: FillPlane from one row and column in.
            _ => self.fill(at.wrapping_add(0x82), 0x83FB, d3 - 1, d4 - 1),
        }
    }

    /// `CopyTilemap`: `rows` rows of `width` words from the ROM.
    fn copy_words(&mut self, at: u16, from: u32, width: u16, rows: u16) {
        for i in 0..width * rows {
            let to = at.wrapping_add((i / width) * ROW_BYTES).wrapping_add((i % width) * 2);
            self.plane_writes.push((to, rom::word(from + u32::from(i) * 2)));
        }
    }

    /// `CopyTilemap8`: `rows` rows of `width` bytes from the ROM, each the low byte of a
    /// word on `base`.
    fn copy_bytes(&mut self, at: u16, from: u32, base: u16, width: u16, rows: u16) {
        for i in 0..width * rows {
            let to = at.wrapping_add((i / width) * ROW_BYTES).wrapping_add((i % width) * 2);
            self.plane_writes.push((to, (base & 0xFF00) | u16::from(rom::byte(from + u32::from(i)))));
        }
    }

    /// `FillPlane`.
    fn fill(&mut self, at: u16, word: u16, width: u16, rows: u16) {
        for i in 0..width * rows {
            self.plane_writes.push((at.wrapping_add((i / width) * ROW_BYTES).wrapping_add((i % width) * 2), word));
        }
    }

    /// `SpecPlane84`.
    fn draw_wipe(&mut self, player: u8, frame: u8) {
        let board = match self.level_mode & 3 {
            0 | 1 => rom::byte(WIPE_BOARD + u32::from(self.level)),
            _ => 3,
        };
        let d = rom::long(WIPE_DESCRIPTORS + (u32::from(board) << 1 | u32::from(player & 1)) * 4);
        let (map, stride, fill) = (rom::long(d), rom::word(d + 4), rom::word(d + 6));
        let (base, width) = (rom::word(d + 8), rom::word(d + 0x0A) + 1);
        let (ground, below, above) = (rom::word(d + 0x0C), rom::word(d + 0x0E), rom::word(d + 0x10));
        // loc_1B0A0: maps of $40 bytes a frame or more are words.
        let copy = |e: &mut Engine, at: u16, from: u32, base: u16, width: u16| {
            if stride < 0x40 {
                e.copy_bytes(at, from, base, width, 2);
            } else {
                e.copy_words(at, from, width, 2);
            }
        };
        copy(self, ground, map + u32::from(frame) * u32::from(stride), base, width);
        match frame {
            0 => {
                self.fill(above, fill, 12, 2);
                self.fill(below, fill, 12, 0x26);
            }
            5 => {
                copy(self, above, map + u32::from(stride) * 6, fill, 12);
                copy(self, ground, map, fill, width);
            }
            _ => {}
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
