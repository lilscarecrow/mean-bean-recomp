//! Enigma decompression (`EniDec`): plane maps (tile words). This ROM's variant can
//! only code the two flip bits inline (no priority or palette bits).
//!
//! Header: inline value width in bits, the inline flag mask (bit 1: V flip $1000,
//! bit 0: H flip $800), the incrementing start word and the common word (both plus
//! `base`). Then, per entry, a 4-bit count n (n+1 words):
//! `%00` the incrementing word (+1 each), `%01` the common word, `%100` an inline word,
//! `%101` an inline word +1 each, `%110` -1 each, `%111` n+1 separate inline words
//! (`%111` with n = 15 ends the map).

use super::BitReader;

/// Decompresses an Enigma map with tile words offset by `base` (the art's first tile,
/// with any palette/priority bits); returns the words and the bytes consumed.
pub fn decompress(data: &[u8], base: u16) -> Result<(Vec<u16>, usize), String> {
    if data.len() < 6 {
        return Err("Enigma: stream too short".into());
    }
    let width = u32::from(data[0]);
    let flags = data[1];
    if flags & !3 != 0 {
        return Err(format!("Enigma: inline flags {flags:02X} this variant can't code"));
    }
    let mut incrementing = u16::from_be_bytes([data[2], data[3]]).wrapping_add(base);
    let common = u16::from_be_bytes([data[4], data[5]]).wrapping_add(base);
    let mut bits = BitReader::new(data, 6);
    let mut out = Vec::new();
    let inline = |bits: &mut BitReader| -> Result<u16, String> {
        let mut v = base;
        if flags & 2 != 0 && bits.bit()? != 0 {
            v |= 0x1000;
        }
        if flags & 1 != 0 && bits.bit()? != 0 {
            v |= 0x0800;
        }
        Ok(v.wrapping_add(bits.bits(width)? as u16))
    };
    loop {
        let kind = if bits.bit()? == 0 { bits.bit()? } else { 4 | bits.bits(2)? };
        let count = bits.bits(4)? as usize + 1;
        match kind {
            0 => {
                for _ in 0..count {
                    out.push(incrementing);
                    incrementing = incrementing.wrapping_add(1);
                }
            }
            1 => out.extend(std::iter::repeat(common).take(count)),
            4 | 5 | 6 => {
                let mut v = inline(&mut bits)?;
                for _ in 0..count {
                    out.push(v);
                    v = match kind {
                        5 => v.wrapping_add(1),
                        6 => v.wrapping_sub(1),
                        _ => v,
                    };
                }
            }
            _ => {
                if count == 16 {
                    break;
                }
                for _ in 0..count {
                    out.push(inline(&mut bits)?);
                }
            }
        }
    }
    Ok((out, bits.position()))
}
