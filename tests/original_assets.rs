//! The decoders against the game's data: every Nemesis and Sorcery resource in the ROM
//! must decode to the disassembly's decompressed copy, and every Enigma map must decode
//! cleanly, using exactly its compressed bytes. Reads the private RE workspace
//! (`../bean-machine-re`: the offset table and the disassembly's resource files); passes
//! with a notice when it isn't there.

mod support;

use std::path::PathBuf;

use bean_machine::assets::{enigma, nemesis, sorcery};

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
    let rom = bean_machine::rom::get().unwrap();
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
    let rom = bean_machine::rom::get().unwrap();
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
                // (in the ending's role call other maps share the buffer that frame)
                if cur.u16(0xFF_1124) != 0 {
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
    use bean_machine::assets::{sprites, Rom};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::rom::get().unwrap());
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
                let (mappings, frame) = (ram.u8(a + 8), ram.u8(a + 9));
                // (mapping list 7's frames are in RAM: the ending's lines of text)
                let placed = if mappings == 7 && frame < 8 {
                    let at = 0xFF_1446 + u32::from(frame) * 0xA2;
                    let pieces = (0..u32::from(ram.u16(at)))
                        .map(|i| {
                            let p = at + 2 + i * 8;
                            sprites::Piece { y: ram.u16(p), size: ram.u8(p + 2), layer: ram.u8(p + 3), tile: ram.u16(p + 4), x: ram.u16(p + 6) }
                        })
                        .collect();
                    sprites::place_pieces(pieces, ram.u16(a + 0x0A), ram.u16(a + 0x0E), vscroll)
                } else {
                    sprites::place(&rom, mappings, frame, ram.u16(a + 0x0A), ram.u16(a + 0x0E), vscroll)
                };
                for s in placed {
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
    use bean_machine::assets::{palette, Rom};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::rom::get().unwrap());
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

/// The match screen against VDP snapshots from the harness (`out/vdp/*.vdp`: RAM, VRAM,
/// CRAM, VSRAM): patterns $0000-$B7FF as `screen::match_vram` builds them (the animated
/// tiles $32C-$32F aside), and the palette lines in RAM as `screen::match_palettes` has
/// them (line 3's color 14 aside: the portrait cycles it).
#[test]
fn match_screen_matches_vdp_snapshots() {
    use bean_machine::assets::{planes, screen, Rom, RomAssets};
    let Some(dir) = workspace() else {
        eprintln!("skipping: no ../bean-machine-re");
        return;
    };
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::rom::get().unwrap());
    let src = RomAssets::new().unwrap();
    let mut snaps: Vec<PathBuf> = std::fs::read_dir(dir.join("out/vdp")).into_iter().flatten().flatten().map(|e| e.path()).collect();
    snaps.retain(|p| p.extension().is_some_and(|x| x == "vdp"));
    snaps.sort();
    let mut checked = 0;
    for path in snaps {
        let data = std::fs::read(&path).unwrap();
        assert_eq!(&data[..8], b"VDPSNAP1", "{}", path.display());
        let (ram, vram) = (&data[8..8 + 0x10000], &data[8 + 0x10000..8 + 0x20000]);
        let (level_mode, opponent) = (ram[0x1964], ram[0x0113]);
        // (match screens only: a player actor, tag $F1/$F2, is there)
        let in_match = (0..64).any(|i| matches!(ram[0xE000 + i * 0x40], 0xF1 | 0xF2));
        if level_mode != 0 || !in_match {
            continue;
        }
        let mut ours = screen::match_vram(&rom, &src, opponent).unwrap();
        // The refugee bean's tiles show the background animator's frame (its +$08).
        let animator = (0..64).map(|i| 0xE000 + i * 0x40).find(|&a| ram[a + 2..a + 6] == [0, 0, 0x3B, 0xCC]);
        if let Some(a) = animator {
            planes::refugee_frame(&mut ours, ram[a + 8]);
        }
        planes::match_planes(&rom, &mut ours).unwrap();
        for tile in 0..0xB800 / 32 {
            if animator.is_none() && (0x32C..0x330).contains(&tile) {
                continue;
            }
            let r = tile * 32..tile * 32 + 32;
            assert_eq!(&ours.0[r.clone()], &vram[r], "{}: tile ${tile:03X} (opponent {opponent})", path.display());
        }
        // Planes A ($C000) and B ($E000) over the visible 40 x 28 cells: the board.
        for (plane, base) in [("A", 0xC000usize), ("B", 0xE000)] {
            for row in 0..28 {
                for col in 0..40 {
                    // The centre panel isn't ported; the fields depend on what was drawn
                    // over the match (original_match.rs replays that).
                    let field = (2..14).contains(&col) || (26..38).contains(&col);
                    if (2..26).contains(&row) && ((15..25).contains(&col) || (plane == "A" && field)) {
                        continue;
                    }
                    let at = base + row * 0x80 + col * 2;
                    assert_eq!(
                        &ours.0[at..at + 2],
                        &vram[at..at + 2],
                        "{}: plane {plane} row {row} col {col} (opponent {opponent})",
                        path.display()
                    );
                }
            }
        }
        let fading = (0..4).any(|i| u16::from_be_bytes([ram[0x0AD6 + i * 0x82], ram[0x0AD7 + i * 0x82]]) != 0);
        if !fading {
            let lines = screen::match_palettes(&rom, &src, opponent);
            for (line, colors) in lines.iter().enumerate() {
                for (c, &want) in colors.iter().enumerate() {
                    if line == 3 && c == 14 {
                        continue;
                    }
                    let at = 0x0A56 + line * 32 + c * 2;
                    let have = u16::from_be_bytes([ram[at], ram[at + 1]]);
                    assert_eq!(want, have, "{}: palette line {line} color {c}", path.display());
                }
            }
        }
        checked += 1;
    }
    eprintln!("{checked} match-screen snapshots: patterns and palettes identical");
}

/// The software VDP against the emulator's own screenshots: each snapshot's VRAM,
/// CRAM, VSRAM and registers rendered by `mbm::render` must give the screenshot's
/// pixels. The emulator's 8-bit values per intensity level are learned from the frame
/// itself; every pixel must then agree with them.
#[test]
fn renderer_matches_emulator_screenshots() {
    use bean_machine::render::{self, Layout};
    let Some(dir) = workspace() else {
        eprintln!("skipping: no ../bean-machine-re");
        return;
    };
    let mut checked = 0;
    // Every screenshot in out/vdp with its snapshot (k0_f3: matches and the opponent
    // screen between them).
    let mut frames: Vec<String> = std::fs::read_dir(dir.join("out/vdp"))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok()?.strip_prefix("k0_f3_")?.strip_suffix(".rgb").map(String::from))
        .collect();
    frames.sort();
    for frame in &frames {
        let (Ok(snap), Ok(shot)) = (
            std::fs::read(dir.join(format!("out/vdp/k0_f3_{frame}.vdp"))),
            std::fs::read(dir.join(format!("out/vdp/k0_f3_{frame}.rgb"))),
        ) else {
            continue;
        };
        let ram = &snap[8..8 + 0x10000];
        let vram = &snap[8 + 0x10000..8 + 0x20000];
        let cram_raw = &snap[8 + 0x20000..8 + 0x20080];
        let vsram_raw = &snap[8 + 0x20080..8 + 0x20100];
        // This core keeps CRAM packed as bbbgggrrr; the VDP format is ----bbb-ggg-rrr-.
        let cram: [u16; 64] = std::array::from_fn(|i| {
            let v = u16::from_be_bytes([cram_raw[i * 2], cram_raw[i * 2 + 1]]);
            ((v >> 6) & 7) << 9 | ((v >> 3) & 7) << 5 | (v & 7) << 1
        });
        let vsram: Vec<u16> = (0..40).map(|i| u16::from_be_bytes([vsram_raw[i * 2], vsram_raw[i * 2 + 1]])).collect();
        let layout = Layout::from_registers(&ram[0x0A22..0x0A22 + 24]);
        let pixels = render::render(&layout, vram, &cram, &vsram);
        // Learn level -> value from the majority, then count disagreements.
        let mut votes = vec![std::collections::HashMap::<u8, usize>::new(); 15 * 3];
        for (i, p) in pixels.iter().enumerate() {
            for (ch, level) in render::levels(*p).into_iter().enumerate() {
                *votes[usize::from(level) * 3 + ch].entry(shot[i * 3 + ch]).or_insert(0) += 1;
            }
        }
        let table: Vec<Option<u8>> = votes.iter().map(|v| v.iter().max_by_key(|(_, n)| **n).map(|(k, _)| *k)).collect();
        if std::env::var_os("MBM_LEVELS").is_some() {
            eprintln!("{frame} levels (r, g, b): {:?}", table.chunks(3).collect::<Vec<_>>());
        }
        let mut wrong = vec![];
        for (i, p) in pixels.iter().enumerate() {
            let ours: Vec<u8> = render::levels(*p).into_iter().enumerate().map(|(ch, l)| table[usize::from(l) * 3 + ch].unwrap()).collect();
            if ours[..] != shot[i * 3..i * 3 + 3] {
                wrong.push((i % 320, i / 320, *p, shot[i * 3..i * 3 + 3].to_vec()));
            }
        }
        assert!(wrong.is_empty(), "frame {frame}: {} pixels differ, first {:?}", wrong.len(), &wrong[..wrong.len().min(5)]);
        checked += 1;
    }
    eprintln!("{checked} frames rendered pixel-identical to the emulator");
}
