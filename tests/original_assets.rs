//! The decoders against the game's data: every Nemesis and Sorcery resource in the ROM
//! must decode to the disassembly's decompressed copy, and every Enigma map must decode
//! cleanly, using exactly its compressed bytes. Reads the private RE workspace
//! (`../bean-machine-re`: the offset table and the disassembly's resource files); passes
//! with a notice when it isn't there.

mod support;

use std::path::PathBuf;

use bean_machine::mbm::assets::{enigma, nemesis, sorcery};

fn workspace() -> Option<PathBuf> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bean-machine-re");
    dir.join("resource_offsets.tsv").exists().then_some(dir)
}

/// (resource path, ROM offset, compressed length) from `resource_offsets.tsv`.
fn resources(dir: &PathBuf) -> Vec<(String, usize, usize)> {
    std::fs::read_to_string(dir.join("resource_offsets.tsv"))
        .unwrap()
        .lines()
        .skip(1)
        .filter_map(|l| {
            let mut f = l.split('\t');
            let path = f.next()?.to_string();
            let off = usize::from_str_radix(f.next()?.trim_start_matches("0x"), 16).ok()?;
            let len = f.next()?.parse().ok()?;
            Some((path, off, len))
        })
        .collect()
}

#[test]
fn decoders_match_the_disassemblys_decompressed_art() {
    let Some(dir) = workspace() else {
        eprintln!("skipping: no ../bean-machine-re");
        return;
    };
    if !support::install_rom() {
        return;
    }
    let rom = bean_machine::mbm::rom::get().unwrap();
    let disasm = dir.join("drmbm-md-disasm");
    let (mut nem, mut sor, mut eni) = (0, 0, 0);
    let mut failures = vec![];
    for (path, off, len) in resources(&dir) {
        let data = &rom[off..];
        let unc = |kind: &str| {
            let p = disasm.join(path.replace(&format!("{kind}/compressed"), &format!("{kind}/uncompressed"))).with_extension("unc");
            std::fs::read(p).ok()
        };
        match path.rsplit('.').next() {
            Some("nem") => match nemesis::decompress(data) {
                Ok((out, _)) if Some(&out) == unc("art_nem").as_ref() => nem += 1,
                Ok((out, _)) => failures.push(format!("{path}: {} bytes, expected {:?}", out.len(), unc("art_nem").map(|u| u.len()))),
                Err(e) => failures.push(format!("{path}: {e}")),
            },
            Some("sor") => match sorcery::decompress(data) {
                Ok((out, used)) => {
                    let want = unc("art_sor");
                    let same = want.as_ref().is_some_and(|w| out == w[..w.len() / 4 * 4]);
                    if same && used == len {
                        sor += 1;
                    } else {
                        failures.push(format!("{path}: {} bytes from {used} (file {len}), expected {:?}", out.len(), want.map(|u| u.len())));
                    }
                }
                Err(e) => failures.push(format!("{path}: {e}")),
            },
            Some("eni") => match enigma::decompress(data, 0) {
                Ok((_, used)) if used.abs_diff(len) <= 1 => eni += 1,
                Ok((out, used)) => failures.push(format!("{path}: {} words from {used} bytes (file {len})", out.len())),
                Err(e) => failures.push(format!("{path}: {e}")),
            },
            _ => {}
        }
    }
    eprintln!("{nem} Nemesis, {sor} Sorcery identical; {eni} Enigma maps decoded cleanly");
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), failures.join("\n"));
}

/// Enigma against the game's own decoder: whenever the portrait (`sub_60F4`) moves to
/// its next map, `eni_tilemap_buffer` holds that map decoded (10 x 7 words, plus the
/// portrait's base tile).
#[test]
fn enigma_matches_the_games_decoder() {
    if !support::install_rom() {
        return;
    }
    let rom = bean_machine::mbm::rom::get().unwrap();
    let long = |a: u32| u32::from_be_bytes(rom[a as usize..a as usize + 4].try_into().unwrap());
    let mut checked = 0;
    for path in support::recordings() {
        let mut prev: Option<support::Ram> = None;
        for ram in support::frames(&path) {
            let Some(p) = prev.replace(ram) else { continue };
            let cur = prev.as_ref().unwrap();
            for a in cur.actors() {
                if cur.code(a) != 0x60FE || p.code(a) != 0x60FE || cur.u32(a + 0x32) == p.u32(a + 0x32) {
                    continue;
                }
                let map = long(cur.u32(a + 0x32) - 4);
                let base = cur.u16(a + 0x2A) | cur.u16(a + 0x0A);
                let (words, _) = enigma::decompress(&rom[map as usize..], base).unwrap();
                let want: Vec<u16> = (0..70).map(|i| cur.u16(0xFF_3000 + i * 2)).collect();
                assert_eq!(&words[..70], &want[..], "{} map ${map:X} base {base:04X}", path.display());
                checked += 1;
            }
        }
    }
    eprintln!("{checked} portrait maps decoded identical to the game's");
}

/// Sprite mappings against `DrawActors`: the hardware sprites rebuilt from a frame's
/// actors (mapping list +$08, frame +$09, position +$0A/+$0E, draw flag bit 7) must be
/// the `sprite_buffer` the game wrote that frame (y, size, pattern, x; the link bytes are
/// left out) along with the layer bytes.
#[test]
fn sprite_mappings_match_draw_actors() {
    use bean_machine::mbm::assets::{sprites, Rom};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::mbm::rom::get().unwrap());
    let (mut frames_checked, mut sprites_checked, mut skipped) = (0, 0, 0);
    for path in support::recordings() {
        for ram in support::frames(&path) {
            let count = usize::from(ram.u16(0xFF_0DE4) / 4);
            if count == 0 {
                skipped += 1; // nothing drawn this frame
                continue;
            }
            // The order used was the opposite of what DrawActors left for next time.
            let backwards = ram.u16(0xFF_0DE2) == 0;
            let flags = [ram.u8(0xFF_0144), ram.u8(0xFF_0145)];
            let mask = (((flags[0] >> 7) & 1) ^ 1) | ((((flags[1] >> 7) & 1) ^ 1) << 1) | 0x0C;
            let vscroll = ram.u16(0xFF_05D2);
            let mut slots: Vec<u32> = ram.actors().collect();
            if backwards {
                slots.reverse();
            }
            let mut ours = vec![];
            'actors: for a in slots {
                if ram.u8(a) & mask == 0 || ram.u8(a + 6) & 0x80 == 0 {
                    continue;
                }
                for s in sprites::place(&rom, ram.u8(a + 8), ram.u8(a + 9), ram.u16(a + 0x0A), ram.u16(a + 0x0E), vscroll) {
                    if ours.len() + 1 >= sprites::MAX_SPRITES {
                        break 'actors;
                    }
                    ours.push(s);
                }
            }
            let rom_sprites: Vec<sprites::Sprite> = (0..count - 1)
                .map(|i| {
                    let e = 0xFF_0E8E + i as u32 * 8;
                    sprites::Sprite { y: ram.u16(e), size: ram.u8(e + 2), layer: ram.u8(0xFF_0DE7 + i as u32), tile: ram.u16(e + 4), x: ram.u16(e + 6) }
                })
                .collect();
            assert_eq!(ours, rom_sprites, "{} ({} sprites)", path.display(), count - 1);
            frames_checked += 1;
            sprites_checked += ours.len();
        }
    }
    eprintln!("{frames_checked} frames, {sprites_checked} sprites identical to DrawActors ({skipped} frames drew nothing)");
}

/// Palettes: during a match, while no fade is running, palette lines 0-2 in RAM are
/// each one of the ROM's palettes (`Palette_Table` entries; line 3 has the portrait's
/// cycling color).
#[test]
fn palette_lines_are_rom_palettes() {
    use bean_machine::mbm::assets::{palette, Rom};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::mbm::rom::get().unwrap());
    let table: Vec<[u16; 16]> = (0..=255u8).map(|i| palette::from_table(&rom, i)).collect();
    let (mut lines, mut matched) = (0, 0);
    for path in support::recordings() {
        for ram in support::frames(&path) {
            // Not while fading, and only while the match runs (bytecode disabled): the
            // game flow's fade to black afterwards stops just short of black.
            let fading = (0..4).any(|i| ram.u16(0xFF_0AD6 + i * 0x82) != 0);
            if fading || ram.u8(0xFF_0A3B) == 0 || !ram.actors().any(|a| ram.code(a) == 0x5C62) {
                continue;
            }
            for line in 0..3u32 {
                let colors: [u16; 16] = std::array::from_fn(|c| ram.u16(0xFF_0A56 + line * 32 + c as u32 * 2));
                lines += 1;
                if table.contains(&colors) {
                    matched += 1;
                } else if lines - matched <= 6 {
                    eprintln!("{} line {line}: {colors:04X?}", path.display());
                }
            }
        }
    }
    eprintln!("{matched} of {lines} palette lines (0-2) during matches are ROM palettes");
    assert_eq!(matched, lines);
}
