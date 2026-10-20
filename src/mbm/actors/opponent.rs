//! The scenario's opponent screen between stages (`BC_OpponentScreen`): the backdrop of
//! the opponent's tiles (`DrawOpponentScrBG`), the boxes of the next opponents
//! (`DrawOpponentScrBoxes`), their portraits drawn one a frame, greyed once beaten
//! (`sub_D818`), and the screen's controller (`ActOpponentScr`): it slides the planes
//! in, then after a while flashes "STAGE n" and the portrait frame; a button ends it.
//!
//! Like the match's portrait, the maps go through the Enigma queue (`EniDec` decodes
//! into `eni_tilemap_buffer`, `ProcEniTilemapQueue` copies in the vertical blank).

use super::*;
use crate::mbm::assets::enigma;
use crate::mbm::rom;

/// `ActOpponentScr` (+$26 timer, +$06 draw flags): the sprite is the portrait frame's
/// flash (mappings $22, frame 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpponentScreen {
    pub started: bool,
    pub timer: u16,
    pub draw: u8,
}

pub const OPPONENT_SCREEN_CODE: u32 = 0xD6B4;
pub const OPPONENT_FLASH_AT: (u16, u16) = (0xF8, 0xD0);
pub const OPPONENT_FLASH_MAPPINGS: u8 = 0x22;

/// `sub_D818`: the next opponents' portraits, one box a frame (+$2C: opponents left as
/// nibbles, +$26: the next box's plane address, +$22 high byte: boxes done).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpponentPortraits {
    /// 0 at $D818, 1 at $D81E, 2 at $D824.
    pub step: u8,
    pub done: u8,
    pub left: u16,
    pub at: u16,
}

pub const OPPONENT_PORTRAITS_CODE: u32 = 0xD818;

/// `OpponentScrBGBases` (by level: the backdrop's tile word), `OpponentScrBoxMap` (by
/// level: which of the 4 boxes show), `MapEni_OpponentScrBox`, `word_D7F8` (by level:
/// the next opponents, a nibble a box), `dword_D916` (by opponent: art, map, beaten
/// map, art's VRAM address, tile word), `byte_D906` (palette line by opponent),
/// `word_D860` (by level: a palette fade once all portraits are up).
const BG_BASES: u32 = 0xD5E8;
const BOX_MAP: u32 = 0xD570;
const BOX_ENIGMA: u32 = 0xD580;
const NEXT_OPPONENTS: u32 = 0xD7F8;
const PORTRAIT_TABLE: u32 = 0xD916;
const PORTRAIT_LINES: u32 = 0xD906;
const LAST_FADES: u32 = 0xD860;
const OPPONENT_PALETTES: u32 = 0x6206;
/// `OpponentScrScrlFlags`: levels whose screen doesn't slide in.
const SCROLL_FLAGS: u32 = 0xD698;
/// `.StageTextTiles`: "STAGE" over two rows, then the number.
const STAGE_TEXT: [[u16; 6]; 2] = [[0x8493, 0x8494, 0x8495, 0x8496, 0x8497, 0], [0x8498, 0x8499, 0x849A, 0x849B, 0x849C, 0]];

impl Engine {
    /// Plane row size for the current plane width (`vdp_reg_10`).
    pub(super) fn plane_row(&self) -> u16 {
        match self.vdp_regs[0x10] & 3 {
            0 => 0x40,
            1 => 0x80,
            _ => 0x100,
        }
    }

    /// `EniDec`: decode a map into the buffer and make it the queue's first entry.
    pub(super) fn eni_dec(&mut self, map: u32, base: u16, width: u16, height: u16, at: u16) {
        let Some(rom) = rom::get() else { return };
        if let Ok((words, _)) = enigma::decompress(&rom[map as usize..], base) {
            self.eni_buffer[..words.len().min(0x800)].copy_from_slice(&words[..words.len().min(0x800)]);
        }
        self.eni_queue_set(0, width, height, at);
    }

    fn eni_queue_set(&mut self, i: usize, width: u16, height: u16, at: u16) {
        if i < self.eni_queue.len() {
            self.eni_queue[i] = (width, height, at);
        } else {
            self.eni_queue.push((width, height, at));
        }
    }

    /// `ProcEniTilemapQueue`: each entry copies the buffer from its start.
    pub(super) fn run_eni_queue(&mut self) {
        let row = if self.vdp_regs[0x10] & 2 != 0 { 0x100 } else { 0x80 };
        for (width, height, at) in std::mem::take(&mut self.eni_queue) {
            let base = (at & 0x3FFF) | 0xC000;
            let mut i = 0;
            for r in 0..=height {
                for c in 0..=width {
                    let to = base.wrapping_add(r * row).wrapping_add(c * 2);
                    self.plane_writes.push((to, self.eni_buffer[i & 0x7FF]));
                    i += 1;
                }
            }
        }
    }

    /// `SpawnOpponentScrActors`.
    pub(super) fn spawn_opponent_screen(&mut self) {
        self.vdp_regs[0x0C] |= 8; // EnableSHMode
        self.vdp_regs[0x0B] &= 0xFC; // DisableLineHScroll, then ClearScroll
        self.vscroll = [0; 40];
        self.hscroll = [0; 2];
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::OpponentScreen(OpponentScreen { started: false, timer: 0, draw: 0 }));
        }
        self.hscroll = [0xFF88; 2];
        if rom::byte(SCROLL_FLAGS + u32::from(self.level)) == 0 && self.last_winner == 0 {
            self.hscroll = [0xFFF0; 2];
        }
        // DrawOpponentScrBG: the backdrop, 5 x 5 blocks of 13 x 6 tiles.
        let mut tile = rom::word(BG_BASES + u32::from(self.level) * 2);
        for i in 0..0x4F {
            self.eni_buffer[i] = tile;
            tile = (tile & 0xFF00) | u16::from((tile as u8).wrapping_add(1));
        }
        let mut at = 0xE004u16;
        for r in 0..5 {
            for c in 0..5 {
                self.eni_queue_set(r * 5 + c, 0x0C, 5, at);
                at = at.wrapping_add(0x1A);
            }
            at = at.wrapping_add(0x57E);
        }
        // loc_D7A0: the portraits' actor and their art.
        let next = rom::word(NEXT_OPPONENTS + u32::from(self.level) * 2);
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::OpponentPortraits(OpponentPortraits { step: 0, done: 0, left: next, at: 0xCA08 }));
        }
        let mut n = next;
        for _ in 0..4 {
            let id = u32::from(n & 0x0F);
            if id != 0 {
                let entry = PORTRAIT_TABLE + id * 16;
                let (art, vram) = (rom::long(entry), rom::word(entry + 0x0C));
                self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram, art } });
            }
            n >>= 4;
        }
    }

    /// `DrawOpponentScrBoxes`: the box frame (its border in front) at each shown box.
    pub(super) fn draw_opponent_boxes(&mut self) {
        let base = rom::word(BG_BASES + u32::from(self.level) * 2) & 0x7FFF;
        self.eni_dec(BOX_ENIGMA, base, 0x0B, 8, 0xE000);
        // The frame's tiles in front: the top row, both sides, the bottom row.
        let mut i = 0;
        for _ in 0..12 {
            self.eni_buffer[i] |= 0x8000;
            i += 1;
        }
        for _ in 0..7 {
            self.eni_buffer[i] |= 0x8000;
            i += 11;
            self.eni_buffer[i] |= 0x8000;
            i += 1;
        }
        for _ in 0..12 {
            self.eni_buffer[i] |= 0x8000;
            i += 1;
        }
        let mut shown = rom::byte(BOX_MAP + u32::from(self.level));
        let (mut entry, mut at) = (0, 0xE906u16);
        for _ in 0..4 {
            let bit = shown & 1;
            shown >>= 1;
            if bit != 0 {
                self.eni_queue_set(entry, 0x0B, 8, at);
                entry += 1;
            }
            at = at.wrapping_add(0x1A);
        }
    }

    pub(super) fn run_opponent_screen(&mut self, mut s: OpponentScreen, inputs: &Inputs) -> Option<OpponentScreen> {
        if !s.started {
            s.started = true;
            s.timer = 0xA0;
            return Some(s);
        }
        let done = |e: &mut Engine| {
            e.bytecode_disabled = false; // ActOpponentScr_Done
            None
        };
        if self.input_lock == 0 && inputs.pads[0].press & 0xF0 != 0 {
            return done(self);
        }
        if self.hscroll[1] != 0xFF88 {
            self.hscroll[0] = self.hscroll[0].wrapping_sub(2);
            self.hscroll[1] = self.hscroll[1].wrapping_sub(2);
            return Some(s);
        }
        s.timer = s.timer.wrapping_sub(1);
        if s.timer == 0 {
            return done(self);
        }
        if s.timer == 0x80 {
            self.stage_banner();
        } else if s.timer < 0x80 {
            s.draw = ((s.timer as u8).rotate_left(5)) & 0x80;
        }
        Some(s)
    }

    /// `.StartFlash`: "STAGE n" under the portrait frame.
    fn stage_banner(&mut self) {
        let mut n = self.level.wrapping_sub(2) as i8;
        let mut tens = 0u16;
        if n >= 0x0A {
            tens = 0x8476;
            n -= 0x0A;
        }
        let mut units = 0x8475u16.wrapping_add(n as u8 as u16);
        let mut i = 0;
        for row in STAGE_TEXT {
            for t in row {
                self.eni_buffer[i] = t;
                i += 1;
            }
            self.eni_buffer[i] = tens;
            i += 1;
            if tens & 0xFF != 0 {
                tens += 0x0A;
            }
            self.eni_buffer[i] = units;
            i += 1;
            units = units.wrapping_add(0x0A);
        }
        self.eni_queue_set(0, 7, 1, 0xC73E);
    }

    pub(super) fn run_opponent_portraits(&mut self, mut p: OpponentPortraits) -> Option<OpponentPortraits> {
        if p.step < 2 {
            p.step += 1;
            return Some(p);
        }
        if p.done == 4 {
            let fade = rom::word(LAST_FADES + u32::from(self.level) * 2);
            if fade & 0x8000 == 0 {
                let colors = table_palette(fade as u8);
                self.fade_palette(fade >> 8, 0, 8, &colors);
            }
            return None;
        }
        p.done += 1;
        let id = p.left & 0x0F;
        p.left >>= 4;
        if id != 0 {
            let entry = PORTRAIT_TABLE + u32::from(id) * 16;
            let mut base = rom::word(entry + 0x0E);
            let map = if self.opponents_defeated[usize::from(id)] != 0 {
                base &= 0x7FFF;
                rom::long(entry + 8)
            } else {
                rom::long(entry + 4)
            };
            self.eni_dec(map, base, 9, 6, p.at);
            let colors = table_palette(rom::byte(OPPONENT_PALETTES + u32::from(id)));
            self.fade_palette(u16::from(rom::byte(PORTRAIT_LINES + u32::from(id))), 0, 8, &colors);
        }
        p.at = p.at.wrapping_add(0x1A);
        Some(p)
    }
}

impl OpponentPortraits {
    pub fn code(&self) -> u32 {
        OPPONENT_PORTRAITS_CODE + u32::from(self.step) * 6
    }
}
