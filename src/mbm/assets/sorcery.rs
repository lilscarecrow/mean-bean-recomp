//! "Sorcery" compression (`PuyoDec`, the format Compile used for Puyo Puyo): an LZ77 over
//! a 256-byte ring. A byte $01-$7F copies that many literal bytes; a byte $80+n copies
//! n+3 bytes from (bytes written - 1 - next byte) in the ring; $00 ends the stream. The
//! game writes the output to VRAM four bytes at a time, so a final partial group of
//! four is lost; `decompress` keeps whole groups only, like the game.

/// Decompresses a Sorcery stream; returns the data and the number of bytes consumed.
pub fn decompress(data: &[u8]) -> Result<(Vec<u8>, usize), String> {
    let mut ring = [0u8; 256];
    let mut pos: u8 = 0;
    let mut out = Vec::new();
    let mut at = 0;
    let next = |at: &mut usize| -> Result<u8, String> {
        let b = data.get(*at).copied().ok_or_else(|| "Sorcery: stream runs past the end".to_string())?;
        *at += 1;
        Ok(b)
    };
    loop {
        let op = next(&mut at)?;
        if op == 0 {
            break;
        }
        if op & 0x80 == 0 {
            for _ in 0..op {
                let b = next(&mut at)?;
                ring[usize::from(pos)] = b;
                pos = pos.wrapping_add(1);
                out.push(b);
            }
        } else {
            let count = usize::from(op & 0x7F) + 3;
            let mut from = pos.wrapping_sub(next(&mut at)?).wrapping_sub(1);
            for _ in 0..count {
                let b = ring[usize::from(from)];
                ring[usize::from(pos)] = b;
                pos = pos.wrapping_add(1);
                from = from.wrapping_add(1);
                out.push(b);
            }
        }
    }
    out.truncate(out.len() / 4 * 4);
    Ok((out, at))
}
