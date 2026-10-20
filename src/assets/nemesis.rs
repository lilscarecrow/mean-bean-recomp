//! Nemesis decompression (`NemDec`): 8x8 tiles, 4 bits per pixel, as run lengths of
//! nibbles coded with a per-file prefix code. Output is whole tiles (32 bytes each).
//!
//! Header: a word whose top bit selects XOR mode (each 4-byte row is XORed with the
//! previous output row) and whose low 15 bits are the tile count; then the code table:
//! `$FF` ends it, a byte < $80 is (length << 4 ... see `read_table`), bytes >= $80 set
//! the nibble. The code `%111111` (6 bits) escapes to an inline 3-bit count and nibble.

use super::BitReader;

/// Decompresses a Nemesis stream; returns the tiles and the number of bytes consumed.
pub fn decompress(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    if data.len() < 2 {
        return Err("Nemesis: stream too short".into());
    }
    let header = u16::from_be_bytes([data[0], data[1]]);
    let xor = header & 0x8000 != 0;
    let tiles = usize::from(header & 0x7FFF);
    let (table, mut at) = read_table(data, 2)?;
    let mut bits = BitReader::new(data, at);
    let total_rows = tiles * 8;
    let mut out = Vec::with_capacity(tiles * 32);
    let mut row: u32 = 0;
    let mut nibbles = 0;
    let mut prev_row: u32 = 0;
    let mut push = |nibble: u8, out: &mut Vec<u8>, row: &mut u32, nibbles: &mut usize| {
        *row = (*row << 4) | u32::from(nibble);
        *nibbles += 1;
        if *nibbles == 8 {
            let value = if xor {
                prev_row ^= *row;
                prev_row
            } else {
                *row
            };
            out.extend_from_slice(&value.to_be_bytes());
            *row = 0;
            *nibbles = 0;
        }
    };
    while out.len() < total_rows * 4 {
        // Codes are at most 8 bits; find the one that matches.
        let mut code: u32 = 0;
        let mut found = None;
        for len in 1..=8u8 {
            code = (code << 1) | bits.bit()?;
            if len == 6 && code == 0x3F {
                found = Some(None); // escape: inline count and nibble
                break;
            }
            if let Some(&(count, nibble)) = table.get(&(len, code as u8)) {
                found = Some(Some((count, nibble)));
                break;
            }
        }
        let (count, nibble) = match found {
            Some(Some(entry)) => entry,
            Some(None) => {
                let count = bits.bits(3)? as u8 + 1;
                let nibble = bits.bits(4)? as u8;
                (count, nibble)
            }
            None => return Err(format!("Nemesis: no code matches at byte {}", bits.position())),
        };
        for _ in 0..count {
            if out.len() >= total_rows * 4 {
                break;
            }
            push(nibble, &mut out, &mut row, &mut nibbles);
        }
    }
    at = bits.position();
    Ok((out, at))
}

/// The code table: (code length, code) -> (run length, nibble).
fn read_table(data: &[u8], mut at: usize) -> Result<(std::collections::HashMap<(u8, u8), (u8, u8)>, usize), String> {
    let mut table = std::collections::HashMap::new();
    let byte = |at: usize| data.get(at).copied().ok_or_else(|| "Nemesis: table runs past the end".to_string());
    let mut b = byte(at)?;
    at += 1;
    let mut nibble = 0u8;
    loop {
        if b == 0xFF {
            break;
        }
        if b & 0x80 != 0 {
            nibble = b & 0x0F;
            b = byte(at)?;
            at += 1;
            continue;
        }
        // %0ccclllll: run length ccc+1, code length lllll (low nibble); then the code.
        let count = ((b >> 4) & 7) + 1;
        let len = b & 0x0F;
        let code = byte(at)?;
        at += 1;
        table.insert((len, code), (count, nibble));
        b = byte(at)?;
        at += 1;
    }
    Ok((table, at))
}
