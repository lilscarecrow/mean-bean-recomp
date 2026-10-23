//! Sprite mappings (`Sprite_Mappings_Table`) and how `DrawActors` turns an actor into
//! hardware sprites.
//!
//! An actor's +$08 picks a mapping list from the table, its +$09 a frame in it; a frame
//! is a piece count and 8-byte pieces: y offset, VDP size byte, layer, tile word, x
//! offset. A piece lands at the actor's position (+$0A x, +$0E y, minus the vertical
//! scroll for y); an x of exactly 0 is nudged to 1 (0 would mask sprites on hardware).

use super::Rom;

/// `Sprite_Mappings_Table`.
pub const MAPPINGS_TABLE: u32 = 0x13940;
/// `DrawActors` stops at this many hardware sprites (the first is a dummy).
pub const MAX_SPRITES: usize = 0x50;
/// `SetSpriteLinks` links layers 0-4.
pub const LAYERS: u8 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Piece {
    pub y: u16,
    /// VDP size: width (bits 2-3) and height (bits 0-1) in tiles, minus one.
    pub size: u8,
    /// Drawing layer (`sprite_layers`): which sprites go in front.
    pub layer: u8,
    /// Pattern word: priority (bit 15), palette line (13-14), V/H flip (12/11), tile (0-10).
    pub tile: u16,
    pub x: u16,
}

impl Piece {
    pub fn width_tiles(&self) -> u8 {
        ((self.size >> 2) & 3) + 1
    }
    pub fn height_tiles(&self) -> u8 {
        (self.size & 3) + 1
    }
}

/// One hardware sprite as `DrawActors` writes it (sprite space: the screen's top-left
/// is (128, 128)).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sprite {
    pub x: u16,
    pub y: u16,
    pub size: u8,
    pub layer: u8,
    pub tile: u16,
}

/// `SetSpriteLinks`: the order the VDP draws `sprites` (table entries 1.., after the
/// dummy entry 0) in, the first in front: layer 0's in table order, then layer 1's, ...
/// A piece on any other layer is in the table but linked from nowhere, so never shown:
/// frame $1A of mapping list 6 (a single landed garbage bean) claims two pieces but has
/// one, and the second is the next frame's header (layer $FE, tile $0503: score font).
pub fn chain(sprites: &[Sprite]) -> Vec<usize> {
    let mut chain: Vec<usize> = (1..=sprites.len()).filter(|&i| sprites[i - 1].layer < LAYERS).collect();
    chain.sort_by_key(|&i| (sprites[i - 1].layer, i));
    chain
}

/// The pieces of frame `frame` of mapping list `mappings`.
pub fn frame(rom: &Rom, mappings: u8, frame: u8) -> Vec<Piece> {
    let list = rom.long(MAPPINGS_TABLE + u32::from(mappings) * 4);
    let at = rom.long(list + u32::from(frame) * 4);
    let count = rom.word(at);
    (0..u32::from(count))
        .map(|i| {
            let p = at + 2 + i * 8;
            Piece { y: rom.word(p), size: rom.byte(p + 2), layer: rom.byte(p + 3), tile: rom.word(p + 4), x: rom.word(p + 6) }
        })
        .collect()
}

/// `DrawActorSprite`: the sprites of an actor at (x, y) showing `frame` of `mappings`.
pub fn place(rom: &Rom, mappings: u8, frame_index: u8, x: u16, y: u16, vscroll: u16) -> Vec<Sprite> {
    place_pieces(frame(rom, mappings, frame_index), x, y, vscroll)
}

/// As `place`, for a frame given as pieces (mapping list 7's frames are built in RAM).
pub fn place_pieces(pieces: Vec<Piece>, x: u16, y: u16, vscroll: u16) -> Vec<Sprite> {
    pieces
        .into_iter()
        .map(|p| {
            let sx = p.x.wrapping_add(x);
            Sprite {
                x: if sx == 0 { 1 } else { sx },
                y: p.y.wrapping_add(y).wrapping_sub(vscroll),
                size: p.size,
                layer: p.layer,
                tile: p.tile,
            }
        })
        .collect()
}
