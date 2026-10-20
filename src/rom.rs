//! The user's own copy of the ROM, read at runtime for data the port doesn't carry:
//! animation scripts, sprite timings, graphics. Nothing from it is copied into this
//! repository. Install it once at startup ([`install`]); code that needs it calls
//! [`get`]. A later asset pack can stand in for it.

use std::sync::OnceLock;

static ROM: OnceLock<Vec<u8>> = OnceLock::new();

/// Size of the supported cartridge (USA/Europe, `GM MK-1706 -00`).
pub const SIZE: usize = 0x10_0000;

/// Bytes the supported revision has at these addresses (checked by [`install`]).
const SIGNATURE: &[(usize, &[u8])] = &[
    (0x100, b"SEGA"),
    (0x180, b"GM MK-1706 -00"),
    // Anim_PuyoPop, the first animation script the port used.
    (0x4B88, &[0x08, 0x08, 0x01, 0x04, 0x01, 0x05, 0x01, 0x06, 0xFE]),
];

/// Checks that `bytes` is the supported ROM and makes it available to [`get`].
/// Installing again is a no-op if it is the same ROM.
pub fn install(bytes: Vec<u8>) -> Result<(), String> {
    check(&bytes)?;
    if let Some(have) = ROM.get() {
        return if *have == bytes { Ok(()) } else { Err("a different ROM is already installed".into()) };
    }
    let _ = ROM.set(bytes);
    Ok(())
}

/// Reads the ROM from `path` and installs it.
pub fn install_from(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    install(bytes)
}

fn check(bytes: &[u8]) -> Result<(), String> {
    if bytes.len() != SIZE {
        return Err(format!("expected a {SIZE}-byte ROM, got {} bytes", bytes.len()));
    }
    for &(at, want) in SIGNATURE {
        if &bytes[at..at + want.len()] != want {
            return Err(format!("not the supported Mean Bean Machine ROM (bytes at ${at:X} differ)"));
        }
    }
    Ok(())
}

/// The installed ROM, if any.
pub fn get() -> Option<&'static [u8]> {
    ROM.get().map(Vec::as_slice)
}

/// A byte of the installed ROM. Panics if none is installed: callers that can run
/// without one check [`get`] first.
pub fn byte(addr: u32) -> u8 {
    get().expect("this needs the ROM (rom::install)")[addr as usize]
}

pub fn word(addr: u32) -> u16 {
    u16::from(byte(addr)) << 8 | u16::from(byte(addr + 1))
}

pub fn long(addr: u32) -> u32 {
    u32::from(word(addr)) << 16 | u32::from(word(addr + 2))
}
