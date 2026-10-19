//! The CPU's analysis stages, checked against the ROM's RAM right after each one runs.
//!
//! - `sub_12BAA` (both players, as the player actor reaches $3DF6): bean counts by color,
//!   bean total, per-column free rows and top-bean group marks, reachability.
//! - `sub_12C8A` (CPU only, $3E08 and $3E1A): the placement tables for the current and
//!   the next pair.
//! - `sub_127BA` (CPU only, $3E44): the plan (target, move/pause timers), the CPU's
//!   memory, scratch and the 24 cell scores, with its random draws.
//! - The AI's byte view of a field rendered from the port's colors-only `Field`.

mod support;

use bean_machine::mbm::ai::{self, off, AiField};
use bean_machine::mbm::rng::Rng;
use support::{frames, recordings, Ram, FIELD, P1, P2};

fn field_bytes(ram: &Ram, player: usize) -> AiField {
    AiField::from_bytes(ram.bytes(FIELD[player], off::SIZE))
}

fn diff(want: &[u8], got: &[u8], base: usize) -> String {
    want.iter()
        .zip(got)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .take(8)
        .map(|(i, (a, b))| format!("+{:03X}: ROM {a:02X} ours {b:02X}", base + i))
        .collect::<Vec<_>>()
        .join(", ")
}

#[test]
fn analysis_tables_match_the_original() {
    let (mut analyses, mut tables, mut crowding) = (0, 0, 0);
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let mut prev: Option<Ram> = None;
        for (frame, ram) in frames(&path).enumerate() {
            let Some(p) = prev.replace(ram) else { continue };
            let cur = prev.as_ref().unwrap();
            if poked.contains(&frame) {
                continue;
            }
            for (i, tag) in [P1, P2].into_iter().enumerate() {
                let (Some(a), Some(b)) = (p.player_actor(tag), cur.player_actor(tag)) else { continue };
                if a != b {
                    continue;
                }
                let (was, now) = (p.code(a), cur.code(b));
                if was == now {
                    continue;
                }
                let rom = field_bytes(cur, i);
                if now == 0x3DF6 {
                    let mut ours = rom.clone();
                    ai::analyze(&mut ours);
                    let (lo, hi) = (off::UNK2, off::UNK3 + 12);
                    assert!(
                        rom.bytes[lo..hi] == ours.bytes[lo..hi],
                        "{} frame {frame} P{}: analysis differs: {}",
                        path.display(),
                        i + 1,
                        diff(&rom.bytes[lo..hi], &ours.bytes[lo..hi], lo)
                    );
                    analyses += 1;
                    if tag == P2 && cur.u8(0xFF_1964) & 3 == 0 {
                        let want = cur.u16(0xFF_198C);
                        assert_eq!(ai::crowding(&rom, &field_bytes(cur, 0)), want, "{} frame {frame}: word_FF198C", path.display());
                        crowding += 1;
                    }
                }
                let next = match now {
                    0x3E08 => false,
                    0x3E1A => true,
                    _ => continue,
                };
                if tag != P2 {
                    continue; // only the CPU fills these
                }
                let mut ours = rom.clone();
                ai::placement_table(&mut ours, next);
                let lo = if next { off::UNK5 } else { off::UNK4 };
                assert!(
                    rom.bytes[lo..lo + 96] == ours.bytes[lo..lo + 96],
                    "{} frame {frame}: placement table ({}) differs: {}",
                    path.display(),
                    if next { "next" } else { "current" },
                    diff(&rom.bytes[lo..lo + 96], &ours.bytes[lo..lo + 96], lo)
                );
                tables += 1;
            }
        }
    }
    eprintln!("{analyses} analyses, {crowding} crowding checks, {tables} placement tables identical");
}

const OPPONENT: u32 = 0xFF_0113;
const TIME_MINUTES: u32 = 0xFF_05D0;
const CPU_DIFFICULTY: u32 = 0xFF_0104;
const CONTROL_P1: u32 = 0xFF_1958;
const CPU_MEMORY: [u32; 2] = [0xFF_1D4E, 0xFF_1D58];
const SCRATCH: u32 = 0xFF_1D16;
const SCORES: u32 = 0xFF_1D1E;

fn plan_of(ram: &Ram, actor: u32) -> ai::Plan {
    ai::Plan {
        col: ram.u8(actor + 0x20),
        rot: ram.u8(actor + 0x21),
        timer: ram.u8(actor + 0x26),
        phase: ram.u8(actor + 0x27),
        move_frames: ram.u8(actor + 0x2C),
        pause_frames: ram.u8(actor + 0x2D),
    }
}

fn scratch_of(ram: &Ram) -> ai::PlanScratch {
    let mut s = ai::PlanScratch::default();
    s.bytes.copy_from_slice(ram.bytes(SCRATCH, 8));
    for (i, w) in s.scores.iter_mut().enumerate() {
        *w = ram.u16(SCORES + i as u32 * 2);
    }
    s
}

/// `sub_127BA` as the CPU's player actor leaves $3E1A: target, timers, mode memory,
/// scratch and the 24 cell scores. The planner's RandomBound draws come after an unknown
/// number of other draws in the same frame, so the RNG offset is searched.
#[test]
fn planner_matches_the_original() {
    let (mut plans, mut patterns, mut openings) = (0, 0, 0);
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let mut prev: Option<Ram> = None;
        for (frame, ram) in frames(&path).enumerate() {
            let Some(p) = prev.replace(ram) else { continue };
            let cur = prev.as_ref().unwrap();
            if poked.contains(&frame) || poked.contains(&(frame - 1)) {
                continue;
            }
            for (i, tag) in [P1, P2].into_iter().enumerate() {
                let (Some(a), Some(b)) = (p.player_actor(tag), cur.player_actor(tag)) else { continue };
                if a != b || p.code(a) != 0x3E1A || cur.code(b) != 0x3E44 {
                    continue;
                }
                let id = cur.u8(b + 0x2A);
                if !ai::is_cpu(p.u8(support::addr::LEVEL_MODE), id, p.u8(CONTROL_P1)) {
                    continue;
                }
                let preview = p.u32(a + 0x32);
                let ctx = ai::PlanContext {
                    player_id: id,
                    opponent: p.u8(OPPONENT),
                    difficulty: p.u8(CPU_DIFFICULTY),
                    time_minutes: p.u16(TIME_MINUTES),
                    pair: (p.u8(preview + 0x26), p.u8(preview + 0x27)),
                };
                let field = field_bytes(&p, i);
                let want = (plan_of(cur, b), cur.bytes(CPU_MEMORY[i], 10).to_vec(), scratch_of(cur));
                let seed = p.u32(support::addr::RNG_SEED);
                let mut outcome = None;
                let mut rng = Rng::new(seed);
                for _ in 0..64 {
                    let mut r = rng;
                    let mut plan = plan_of(&p, a);
                    let mut mem: ai::CpuMemory = p.bytes(CPU_MEMORY[i], 10).try_into().unwrap();
                    let mut scratch = scratch_of(&p);
                    let res = ai::plan(&field, &ctx, &mut mem, &mut plan, &mut scratch, &mut r);
                    if res.is_err() {
                        outcome = Some(Err(()));
                        break;
                    }
                    let got = (plan, mem.to_vec(), scratch);
                    let end = cur.u32(support::addr::RNG_SEED);
                    let reaches_end = (0..64).any(|_| {
                        let hit = r.seed == end;
                        r.next();
                        hit
                    });
                    if got == want && reaches_end {
                        outcome = Some(Ok(()));
                        break;
                    }
                    rng.next();
                }
                match outcome {
                    Some(Ok(())) => {
                        plans += 1;
                        if ctx.opponent == 1 && field_bytes(cur, i).bytes[off::COUNT + 1] < 0x18 {
                            openings += 1;
                        }
                    }
                    Some(Err(())) => patterns += 1,
                    None => {
                        let mut r = Rng::new(seed);
                        let mut plan = plan_of(&p, a);
                        let mut mem: ai::CpuMemory = p.bytes(CPU_MEMORY[i], 10).try_into().unwrap();
                        let mut scratch = scratch_of(&p);
                        let _ = ai::plan(&field, &ctx, &mut mem, &mut plan, &mut scratch, &mut r);
                        panic!(
                            "{} frame {frame} P{} opp {}: planner differs\n ROM  {:?} mem {:02X?}\n ours {:?} mem {:02X?}\n ROM  scratch {:02X?} scores {:04X?}\n ours scratch {:02X?} scores {:04X?}",
                            path.display(),
                            i + 1,
                            ctx.opponent,
                            want.0,
                            want.1,
                            plan,
                            mem,
                            want.2.bytes,
                            want.2.scores,
                            scratch.bytes,
                            scratch.scores
                        );
                    }
                }
            }
        }
    }
    eprintln!("{plans} plans identical ({openings} Frankly openings), {patterns} pattern plans unported");
}

/// The AI's view of a field rendered from the port's `Field` (colors only) must give
/// the ROM's cell high bytes and group marks wherever the AI reads them.
#[test]
fn rendered_ai_field_matches_the_original() {
    use bean_machine::mbm::field::Field;
    let mut checked = 0;
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let mut prev: Option<Ram> = None;
        // sub_5960 has marked this match's field (else the marks are stale or blank).
        let mut marked = [false; 2];
        for (frame, ram) in frames(&path).enumerate() {
            let Some(p) = prev.replace(ram) else { continue };
            let cur = prev.as_ref().unwrap();
            if poked.contains(&frame) {
                continue;
            }
            for (i, tag) in [P1, P2].into_iter().enumerate() {
                let (Some(a), Some(b)) = (p.player_actor(tag), cur.player_actor(tag)) else {
                    marked[i] = false;
                    continue;
                };
                if a != b {
                    marked[i] = false;
                    continue;
                }
                if p.code(a) == 0x3E9A && cur.code(b) != 0x3E9A {
                    marked[i] = true;
                }
                if p.code(a) == 0x3DF6 || cur.code(b) != 0x3DF6 || !marked[i] {
                    continue;
                }
                let rom = field_bytes(cur, i);
                let mut ours = rom.clone();
                ours.render(&Field::from_words(&cur.field_words(i)), true);
                for cell in 0..72 {
                    for (what, at) in [("cell", off::VISIBLE + cell * 2), ("mark", off::UNK1 + cell * 2)] {
                        assert_eq!(
                            rom.bytes[at],
                            ours.bytes[at],
                            "{} frame {frame} P{}: {what} {} (col {}, visible row {})",
                            path.display(),
                            i + 1,
                            cell,
                            cell % 6,
                            cell / 6
                        );
                    }
                }
                checked += 1;
            }
        }
    }
    eprintln!("{checked} rendered AI fields identical");
}
