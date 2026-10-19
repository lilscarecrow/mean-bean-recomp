#![allow(dead_code)]
//! Reads RAM recordings made by the private ground-truth harness (the real ROM running
//! in Genesis Plus GX). Recordings never live in this repo.
//!
//! Location: `$MBM_RECORDINGS`, else `../bean-machine-re/out` next to this repo.
//! Tests that need them print a notice and pass when none are found, unless
//! `MBM_REQUIRE_RECORDINGS=1` is set (then missing recordings fail the test).

use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;

use flate2::read::ZlibDecoder;

pub const RAM_BASE: u32 = 0xFF_0000;
pub const RAM_SIZE: usize = 0x1_0000;

/// Addresses of the original's variables (from the disassembly's ram.asm).
pub mod addr {
    pub const LEVEL: u32 = 0xFF_0112;
    pub const RNG_SEED: u32 = 0xFF_1116;
    pub const P1_BEAN_ORDER: u32 = 0xFF_123E;
    pub const P2_BEAN_ORDER: u32 = 0xFF_133E;
    pub const LEVEL_MODE: u32 = 0xFF_1964;
}

/// One frame of 68000 work RAM ($FF0000-$FFFFFF), big-endian as on hardware.
pub struct Ram(pub Vec<u8>);

/// Player tags in actor byte 0.
pub const P1: u8 = 0xF1;
pub const P2: u8 = 0xF2;
pub const FIELD: [u32; 2] = [0xFF_1D62, 0xFF_20CC];
pub const GARBAGE_DATA: u32 = 0xFF_1996;
pub const FRAME_COUNT: u32 = 0xFF_05C6;
/// P1 controller: hold, press, auto-repeat (horizontal) bytes.
pub const P1_CTRL: u32 = 0xFF_110A;
/// P1 rotate-button assignments for A, B, C (0 unused, 1 left, 2 right).
pub const P1_KEYS: u32 = 0xFF_FCA6;
const ACTORS: u32 = 0xFF_E000;
const ACTOR_SIZE: u32 = 0x40;
const ACTOR_COUNT: u32 = 0x40;
/// Pivot actor code addresses: player-controlled, and landing (before MarkPuyoSpot).
pub const PIVOT_FALLING: u32 = 0x4F14;
pub const PIVOT_LANDING: std::ops::Range<u32> = 0x4F50..0x4FB2;

impl Ram {
    pub fn u16(&self, addr: u32) -> u16 {
        u16::from_be_bytes(self.bytes(addr, 2).try_into().unwrap())
    }

    pub fn actors(&self) -> impl Iterator<Item = u32> + '_ {
        (0..ACTOR_COUNT).map(|i| ACTORS + i * ACTOR_SIZE)
    }

    pub fn code(&self, actor: u32) -> u32 {
        self.u32(actor + 2)
    }

    /// The player's field-controller actor (tag, code in the $3D00-$40FF coroutine).
    pub fn player_actor(&self, tag: u8) -> Option<u32> {
        self.actors().find(|&a| self.u8(a) == tag && (0x3D00..0x4100).contains(&self.code(a)))
    }

    /// The player's pair: (pivot, satellite), with the pivot in one of `states`.
    pub fn pair(&self, tag: u8, landing: bool) -> Option<(u32, u32)> {
        self.actors().find_map(|a| {
            let code = self.code(a);
            let ok = code == PIVOT_FALLING || (landing && PIVOT_LANDING.contains(&code));
            if self.u8(a) != tag || !ok {
                return None;
            }
            let sat = self.u32(a + 0x36);
            (sat >= ACTORS && sat < ACTORS + ACTOR_COUNT * ACTOR_SIZE && self.u8(sat) == tag).then_some((a, sat))
        })
    }

    /// The whole 14Ã—6 field of a player (0 = P1) as words.
    pub fn field_words(&self, player: usize) -> Vec<u16> {
        (0..84).map(|i| self.u16(FIELD[player] + i * 2)).collect()
    }
    pub fn bytes(&self, addr: u32, len: usize) -> &[u8] {
        let off = (addr - RAM_BASE) as usize;
        &self.0[off..off + len]
    }

    pub fn u8(&self, addr: u32) -> u8 {
        self.bytes(addr, 1)[0]
    }

    pub fn u32(&self, addr: u32) -> u32 {
        u32::from_be_bytes(self.bytes(addr, 4).try_into().unwrap())
    }
}

/// All `.ramrec` files found, or an empty list (after a notice) when there are none.
pub fn recordings() -> Vec<PathBuf> {
    let dir = std::env::var_os("MBM_RECORDINGS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bean-machine-re/out"));
    let mut files = vec![];
    let mut dirs = vec![dir.clone()];
    while let Some(d) = dirs.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                dirs.push(p);
            } else if p.extension().is_some_and(|x| x == "ramrec") {
                files.push(p);
            }
        }
    }
    files.sort();
    if files.is_empty() {
        let msg = format!("no harness recordings in {} (set MBM_RECORDINGS)", dir.display());
        if std::env::var_os("MBM_REQUIRE_RECORDINGS").is_some() {
            panic!("{msg}");
        }
        eprintln!("skipping: {msg}");
    }
    files
}

/// Frames where a test scenario wrote RAM (`<recording>.pokes.json`, a JSON list).
pub fn poked_frames(path: &PathBuf) -> std::collections::HashSet<usize> {
    let mut p = path.clone().into_os_string();
    p.push(".pokes.json");
    std::fs::read_to_string(p)
        .map(|s| s.trim().trim_matches(['[', ']']).split(',').filter_map(|n| n.trim().parse().ok()).collect())
        .unwrap_or_default()
}

/// Iterates the frames of a recording: `b"RAMREC1\0"`, u32 count, then per frame
/// u32 length + zlib(RAM XOR previous RAM).
pub fn frames(path: &PathBuf) -> impl Iterator<Item = Ram> {
    let mut file = BufReader::new(File::open(path).expect("open recording"));
    let mut header = [0u8; 12];
    file.read_exact(&mut header).expect("read header");
    assert_eq!(&header[..8], b"RAMREC1\0", "{} is not a RAM recording", path.display());
    let count = u32::from_le_bytes(header[8..].try_into().unwrap());
    let mut ram = vec![0u8; RAM_SIZE];
    (0..count).map(move |_| {
        let mut len = [0u8; 4];
        file.read_exact(&mut len).expect("frame length");
        let mut blob = vec![0u8; u32::from_le_bytes(len) as usize];
        file.read_exact(&mut blob).expect("frame data");
        let mut delta = Vec::with_capacity(RAM_SIZE);
        ZlibDecoder::new(&blob[..]).read_to_end(&mut delta).expect("inflate frame");
        for (r, d) in ram.iter_mut().zip(&delta) {
            *r ^= d;
        }
        Ram(ram.clone())
    })
}
