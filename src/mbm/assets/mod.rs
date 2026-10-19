//! The game's graphics, decoded from the user's ROM: compressed tile art (Nemesis,
//! Sorcery), plane maps (Enigma), palettes and sprite mappings. [`AssetSource`] is what
//! the front end asks; [`RomAssets`] answers from the ROM, and a replacement asset pack
//! can answer instead later.

pub mod enigma;
pub mod nemesis;
pub mod sorcery;

/// MSB-first bit reader over a byte slice (Nemesis and Enigma streams).
pub(crate) struct BitReader<'a> {
    data: &'a [u8],
    byte: usize,
    bit: u8,
}

impl<'a> BitReader<'a> {
    pub fn new(data: &'a [u8], start: usize) -> Self {
        BitReader { data, byte: start, bit: 0 }
    }

    pub fn bit(&mut self) -> Result<u32, String> {
        let b = *self.data.get(self.byte).ok_or("compressed stream runs past the end")?;
        let v = (b >> (7 - self.bit)) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.byte += 1;
        }
        Ok(u32::from(v))
    }

    pub fn bits(&mut self, n: u32) -> Result<u32, String> {
        (0..n).try_fold(0, |acc, _| Ok((acc << 1) | self.bit()?))
    }

    /// Bytes consumed (a partly read byte counts).
    pub fn position(&self) -> usize {
        self.byte + usize::from(self.bit != 0)
    }
}

pub mod palette;
pub mod screen;
pub mod sprites;

/// Big-endian reads from ROM data.
#[derive(Clone, Copy)]
pub struct Rom<'a>(pub &'a [u8]);

impl Rom<'_> {
    pub fn byte(&self, addr: u32) -> u8 {
        self.0[addr as usize]
    }
    pub fn word(&self, addr: u32) -> u16 {
        u16::from_be_bytes([self.0[addr as usize], self.0[addr as usize + 1]])
    }
    pub fn long(&self, addr: u32) -> u32 {
        u32::from(self.word(addr)) << 16 | u32::from(self.word(addr + 2))
    }
    pub fn from(&self, addr: u32) -> &[u8] {
        &self.0[addr as usize..]
    }
}

/// Where the front end gets graphics. Addresses are the original ROM's (the game's own
/// tables point at them); an asset pack replacing the ROM answers for the same ones.
pub trait AssetSource {
    /// Nemesis-compressed tiles at `addr`.
    fn nemesis(&self, addr: u32) -> Result<Vec<u8>, String>;
    /// Sorcery-compressed tiles at `addr` (`PuyoDec`).
    fn sorcery(&self, addr: u32) -> Result<Vec<u8>, String>;
    /// An Enigma plane map at `addr`, tile words offset by `base`.
    fn enigma(&self, addr: u32, base: u16) -> Result<Vec<u16>, String>;
    /// 16 CRAM colors at `addr`.
    fn palette(&self, addr: u32) -> [u16; 16];
    /// The pieces of a sprite frame.
    fn sprite_frame(&self, mappings: u8, frame: u8) -> Vec<sprites::Piece>;
}

/// Assets decoded from the installed ROM (`mbm::rom`).
pub struct RomAssets {
    rom: &'static [u8],
}

impl RomAssets {
    /// Needs the ROM installed (`mbm::rom::install`).
    pub fn new() -> Result<RomAssets, String> {
        super::rom::get().map(|rom| RomAssets { rom }).ok_or_else(|| "no ROM installed".into())
    }

    fn rom(&self) -> Rom<'static> {
        Rom(self.rom)
    }
}

impl AssetSource for RomAssets {
    fn nemesis(&self, addr: u32) -> Result<Vec<u8>, String> {
        nemesis::decompress(self.rom().from(addr)).map(|(t, _)| t)
    }
    fn sorcery(&self, addr: u32) -> Result<Vec<u8>, String> {
        sorcery::decompress(self.rom().from(addr)).map(|(t, _)| t)
    }
    fn enigma(&self, addr: u32, base: u16) -> Result<Vec<u16>, String> {
        enigma::decompress(self.rom().from(addr), base).map(|(m, _)| m)
    }
    fn palette(&self, addr: u32) -> [u16; 16] {
        palette::read(&self.rom(), addr)
    }
    fn sprite_frame(&self, mappings: u8, frame: u8) -> Vec<sprites::Piece> {
        sprites::frame(&self.rom(), mappings, frame)
    }
}

/// Video memory as the game fills it: 64 KiB of 4-bit tiles (32 bytes each) and maps.
/// Art loads at the VRAM addresses the game's loaders use; the renderer reads tiles.
pub struct Vram(pub Vec<u8>);

impl Default for Vram {
    fn default() -> Self {
        Vram(vec![0; 0x1_0000])
    }
}

impl Vram {
    /// Put `bytes` at VRAM address `at` (wrapping at 64 KiB, like the VDP).
    pub fn load(&mut self, at: u16, bytes: &[u8]) {
        for (i, &b) in bytes.iter().enumerate() {
            self.0[(usize::from(at) + i) & 0xFFFF] = b;
        }
    }

    /// The 8x8 pixels (color indices 0-15) of tile `index`, flipped as a pattern word's
    /// bits 11 (H) and 12 (V) say.
    pub fn tile(&self, pattern: u16) -> [[u8; 8]; 8] {
        let base = usize::from(pattern & 0x07FF) * 32;
        let (hflip, vflip) = (pattern & 0x0800 != 0, pattern & 0x1000 != 0);
        std::array::from_fn(|row| {
            let r = if vflip { 7 - row } else { row };
            std::array::from_fn(|col| {
                let c = if hflip { 7 - col } else { col };
                let byte = self.0[(base + r * 4 + c / 2) & 0xFFFF];
                if c % 2 == 0 { byte >> 4 } else { byte & 0x0F }
            })
        })
    }
}
