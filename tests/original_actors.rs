//! Between pieces, frame by frame: from the first frame after a pair lands until the next
//! pair is in play, the ported actors (player loop, pivot, satellite, post-landing
//! effects) must sit in the same slots, at the same resume addresses, with the same
//! timers and positions as the ROM's, and the field must change on the same frames.

mod support;

use bean_machine::actors::*;
use bean_machine::ai::{off, AiField};
use bean_machine::field::Field;
use bean_machine::Rng;
use bean_machine::pair::{PadInput, RotateKeys};
use support::actors::*;
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P1_CTRL, P1_KEYS, P2};

const RNG_SEED: u32 = 0xFF_1116;
/// Icons the opponent drew this frame appear in the ROM without us making them (add
/// them), and its redraws can retire icons of ours before their turn (drop them).
fn absorb_foreign_icons(engine: &mut Engine, cur: &Ram, tag: u8) {
    let rom_gen = [cur.u8(ICON_GEN), cur.u8(ICON_GEN + 1)];
    for slot in 0..SLOTS {
        let rom_icon = used(cur, slot) && cur.code(slot_addr(slot)) == ICON_CODE;
        match &engine.slots[slot] {
            // Only for sides we didn't redraw ourselves: icons we owed are not excused.
            None | Some(Actor::Opaque) | Some(Actor::OpaqueQuick) | Some(Actor::Vanishing)
                if rom_icon && !engine.icons_drawn[icon_side(cur, slot_addr(slot)) as usize] =>
            {
                engine.slots[slot] = load(cur, slot, tag);
            }
            Some(Actor::Icon(i)) if !rom_icon && rom_gen[i.side as usize] != engine.icon_gen[i.side as usize] => {
                engine.slots[slot] = None;
            }
            _ => {}
        }
    }
    engine.icon_gen = rom_gen;
}

/// Ways to place `n` foreign random draws in the gaps before each of `gaps` actors of
/// ours (the rest come after the last one, where they can't affect us).
fn splits(n: u32, gaps: usize) -> Vec<Vec<u32>> {
    if gaps == 0 {
        return vec![vec![]];
    }
    let mut out = vec![];
    for first in 0..=n {
        for mut rest in splits(n - first, gaps - 1) {
            rest.insert(0, first);
            out.push(rest);
        }
    }
    out
}

/// Actors around the fields (holders, referee, portrait, fades...): mirrored as opaque
/// by this test, which only follows one player's pieces.
fn is_match_level(a: &Actor) -> bool {
    matches!(
        a,
        Actor::PairHolder(_)
            | Actor::PreviewBean(_)
            | Actor::Referee(_)
            | Actor::Sparkle(_)
            | Actor::Animator(_)
            | Actor::Danger(_)
            | Actor::DangerSpark(_)
            | Actor::Portrait(_)
            | Actor::PortraitPalette(_)
            | Actor::CoconutsLight(_)
            | Actor::FfuzzyEyes(_)
            | Actor::FfuzzyPalette(_)
            | Actor::PaletteFade(_)
    )
}

/// Does the ROM's RAM show the ported actor in `slot` as we have it?
fn slot_matches(engine: &Engine, cur: &Ram, tag: u8, slot: usize) -> Result<(), String> {
    match &engine.slots[slot] {
        None => {
            if used(cur, slot) && cur.u8(slot_addr(slot)) == tag && load(cur, slot, tag).is_some() {
                return Err(format!("slot {slot:X}: ROM has {:?}, we have nothing", load(cur, slot, tag).map(|a| describe(&a))));
            }
            Ok(())
        }
        Some(Actor::Opaque) | Some(Actor::OpaqueQuick) | Some(Actor::Vanishing) => Ok(()),
        Some(ours) => {
            let want = describe_ram(cur, slot, tag, ours);
            let got = describe(ours);
            if want.as_deref() == Some(got.as_str()) {
                Ok(())
            } else {
                Err(format!("slot {slot:X}
  ROM {want:?}
  our {got:?}"))
            }
        }
    }
}

fn field_matches(engine: &Engine, cur: &Ram, player: usize) -> bool {
    engine.fields[player] == Field::from_words(&cur.field_words(player))
}

const AI_FIELD: [u32; 2] = [0xFF_1D62, 0xFF_20CC];
const CPU_MEMORY: [u32; 2] = [0xFF_1D4E, 0xFF_1D58];
const CPU_READS: u32 = 0xFF_1D0A;

/// The CPU's analysis tables (`pUnk2`..`pUnk5`), memory and pad-read counter.
fn ai_mismatch(engine: &Engine, cur: &Ram, player: usize) -> Option<String> {
    let lo = off::UNK2;
    let rom = cur.bytes(AI_FIELD[player] + lo as u32, off::SIZE - lo);
    let ours = &engine.ai_fields[player].bytes[lo..];
    if rom != ours {
        let at = rom.iter().zip(ours).position(|(a, b)| a != b).unwrap() + lo;
        return Some(format!("AI field +{at:03X}: ROM {:02X} ours {:02X}", rom[at - lo], ours[at - lo]));
    }
    let mem = cur.bytes(CPU_MEMORY[player], 10);
    if mem != engine.cpu_memory[player] {
        return Some(format!("CPU memory: ROM {mem:02X?} ours {:02X?}", engine.cpu_memory[player]));
    }
    let reads = cur.u8(CPU_READS + player as u32);
    (reads != engine.cpu_reads[player]).then(|| format!("CPU pad reads: ROM {reads} ours {}", engine.cpu_reads[player]))
}

fn first_mismatch(engine: &Engine, cur: &Ram, tag: u8, player: usize) -> Option<String> {
    for slot in 0..SLOTS {
        if let Err(m) = slot_matches(engine, cur, tag, slot) {
            return Some(m);
        }
    }
    if let Some(m) = ai_mismatch(engine, cur, player) {
        return Some(m);
    }
    (!field_matches(engine, cur, player)).then(|| {
        format!("field
  ROM
{:?}  our
{:?}", Field::from_words(&cur.field_words(player)), engine.fields[player])
    })
}


#[derive(Default)]
struct Stats {
    pieces: usize,
    /// Piece starts skipped because the pair was already falling.
    in_flight: usize,
    frames: usize,
    unported: std::collections::BTreeMap<&'static str, usize>,
    falls: usize,
    lag_frames: usize,
    split_frames: usize,
    pop_frames: usize,
    chained_pieces: usize,
    longest_chain: u8,
    rng_frames: usize,
    rng_calls: usize,
    rng_foreign: usize,
    rng_resolved: usize,
    icon_redraws: usize,
    visuals: std::collections::BTreeMap<&'static str, usize>,
}

#[test]
fn between_pieces_matches_the_original_frame_by_frame() {
    // (exercise mode's pairs come from the ROM's color tables)
    support::install_rom();
    let mut stats = Stats::default();
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let all: Vec<Ram> = frames(&path).collect();
        for (pi, tag) in [P1, P2].into_iter().enumerate() {
            let mut f = 1;
            while f + 1 < all.len() {
                // A pair that just left player control (pivot code leaves $4F14).
                let landed_now = all[f - 1].pair(tag, false).is_some() && all[f].pair(tag, false).is_none();
                if !landed_now || poked.contains(&f) {
                    f += 1;
                    continue;
                }
                if let Some(end) = check_piece(&all, f, tag, pi, &mut stats, &path, &poked) {
                    f = end;
                } else {
                    f += 1;
                }
            }
        }
    }
    eprintln!(
        "{} pieces, {} frames identical ({} with a physics fall, {} lag and {} split frames);          {} pieces popped ({} frames in pop states, longest chain {}); stopped at unported: {:?}\n\
         random: {} frames fully accounted ({} calls), {} frames with calls by unported actors ({} needed a split)\n\
         pop and garbage visuals (actor-frames): {:?}; icon redraws by us: {}",
        stats.pieces, stats.frames, stats.falls, stats.lag_frames, stats.split_frames,
        stats.chained_pieces, stats.pop_frames, stats.longest_chain, stats.unported,
        stats.rng_frames, stats.rng_calls, stats.rng_foreign, stats.rng_resolved, stats.visuals, stats.icon_redraws
    );
    assert!(stats.pieces > 0 || std::env::var_os("MBM_REQUIRE_RECORDINGS").is_none());
}

/// Runs one piece from frame `start` (first frame after landing). Returns the frame to
/// continue scanning from.
fn check_piece(
    all: &[Ram],
    start: usize,
    tag: u8,
    player: usize,
    stats: &mut Stats,
    path: &std::path::Path,
    poked: &std::collections::HashSet<usize>,
) -> Option<usize> {
    let ram = &all[start];
    let mut engine = Engine::new();
    engine.fields[player] = Field::from_words(&ram.field_words(player));
    engine.level_mode = ram.u8(0xFF_1964);
    engine.difficulty = ram.u8(0xFF_0104);
    engine.glow_xy = (ram.u16(0xFF_19AC), ram.u16(0xFF_19AE));
    engine.icon_gen = [ram.u8(ICON_GEN), ram.u8(ICON_GEN + 1)];
    engine.chaining = [ram.u8(CHAINING) != 0, ram.u8(CHAINING + 1) != 0];
    for i in 0..2 {
        engine.ai_fields[i] = AiField::from_bytes(ram.bytes(AI_FIELD[i], off::SIZE));
        engine.cpu_memory[i] = ram.bytes(CPU_MEMORY[i], 10).try_into().unwrap();
        engine.cpu_reads[i] = ram.u8(CPU_READS + i as u32);
    }
    engine.opponent = ram.u8(0xFF_0113);
    engine.level = ram.u8(0xFF_0112);
    engine.control_player_1 = ram.u8(0xFF_1958);
    engine.crowding = ram.u16(0xFF_198C);
    // The match clock (the CPU plays differently as minutes pass).
    engine.time_frames = ram.u16(0xFF_05CA);
    engine.time_minutes = engine.time_frames / 3600;
    for slot in 0..SLOTS {
        engine.slots[slot] = load(ram, slot, tag).or_else(|| {
            used(ram, slot).then_some(if ram.u8(slot_addr(slot) + 1) & 0x80 == 0 { Actor::OpaqueQuick } else { Actor::Opaque })
        });
    }
    if !engine.slots.iter().any(|s| matches!(s, Some(Actor::Player(_)))) {
        return None; // player actor not in a ported state (e.g. mid-chain)
    }
    // The next pair, as the ROM spawned it.
    let spawn = (start..all.len().min(start + 2000)).find(|&f| all[f].pair(tag, false).is_some())?;
    let in_play = (0..SLOTS).any(|sl| {
        let a = slot_addr(sl);
        used(ram, sl) && ram.u8(a) == tag && (0x4F00..0x5600).contains(&ram.code(a)) && load(ram, sl, tag).is_none()
    });
    if spawn == start || in_play {
        // A pair is still in play (a quick CPU): it can't be rebuilt from RAM.
        stats.in_flight += 1;
        return Some(start + 1);
    }
    if spawn + 1 >= all.len() {
        return None; // the recording ends first
    }
    if (start..=spawn + 1).any(|f| poked.contains(&f)) {
        return Some(spawn + 1); // a test scenario rewrote RAM in the middle
    }
    // (exercise mode: its special pieces and level counters aren't pairs and actors
    // this test rebuilds; whole_matches_run_on_the_engine_alone covers it)
    if ram.u8(0xFF_1964) == 2 {
        *stats.unported.entry("exercise mode (see whole_matches)").or_default() += 1;
        return Some(spawn + 1);
    }
    let (pv, st) = all[spawn].pair(tag, false)?;
    let mut inputs = Inputs::default();
    inputs.next_pair[player] = Some((color(&all[spawn], pv), color(&all[spawn], st)));
    inputs.spawn_slots[player] = Some((slot_of(pv), slot_of(st)));
    let mut fell = false;
    let mut chain_seen = 0u8;
    // True when the last snapshot caught the ROM halfway through a logic frame: the next
    // snapshot finishes that same frame (see `split_frame`).
    let mut finishing: Option<usize> = None;
    for f in start + 1..=spawn + 1 {
        let (prev, cur) = (&all[f - 1], &all[f]);
        if let Some(cut) = finishing.take() {
            // The ROM's vertical blank came mid-RunActors; the rest of the slots ran after.
            engine.rng = Rng::new(prev.u32(RNG_SEED));
            engine.vblank();
            if let Err(Unported(what)) = engine.run_slots(cut..SLOTS, &inputs) {
                *stats.unported.entry(what).or_default() += 1;
                return Some(f);
            }
            if let Some(m) = first_mismatch(&engine, cur, tag, player) {
                panic!("{} frame {f} (piece from {start}), finishing a split frame at slot {cut:X}: {m}", path.display());
            }
            stats.frames += 1;
            continue;
        }
        if prev.bytes(ACTORS, 0x1000) == cur.bytes(ACTORS, 0x1000) {
            stats.lag_frames += 1;
            continue; // lag: the logic didn't run during this video frame
        }
        // Slots of unported actors follow the ROM, and so does garbage the (unported)
        // opponent sends.
        let other = if tag == P1 { P2 } else { P1 };
        for slot in 0..SLOTS {
            match &mut engine.slots[slot] {
                // Also reserve slots that unported code fills this frame (the opponent's
                // actors, garbage icons): our own spawns this frame must go around them.
                None | Some(Actor::Opaque) | Some(Actor::OpaqueQuick) | Some(Actor::Vanishing) => {
                    let foreign_now = used(cur, slot) && cur.u8(slot_addr(slot)) != tag && cur.code(slot_addr(slot)) != ICON_CODE;
                    // A foreign actor that isn't there at the end of the frame deleted itself
                    // on its turn (the slot may already hold one of ours by then).
                    let foreign_gone = used(prev, slot) && prev.u8(slot_addr(slot)) != tag && !foreign_now;
                    let quick = |r: &Ram| r.u8(slot_addr(slot) + 1) & 0x80 == 0;
                    engine.slots[slot] = if foreign_gone {
                        Some(Actor::Vanishing)
                    } else if used(prev, slot) {
                        Some(if quick(prev) { Actor::OpaqueQuick } else { Actor::Opaque })
                    } else if foreign_now {
                        Some(if quick(cur) { Actor::OpaqueQuick } else { Actor::Opaque })
                    } else {
                        None
                    };
                }
                // Unported code (the opponent's spawns) evicted one of our pop visuals.
                Some(a) if a.evictable() && used(cur, slot) && cur.u8(slot_addr(slot)) != tag => {
                    engine.slots[slot] = Some(Actor::Opaque);
                }
                Some(Actor::Player(p)) => {
                    // Garbage as of when this player runs: an opponent in an earlier slot
                    // may already have ended a chain this frame (acc drops to acc % 70).
                    let mut pending = prev.u16(slot_addr(slot) + 0x14);
                    if let Some(sender) = prev.player_actor(other).filter(|&s| slot_of(s) < slot) {
                        let (a, b) = (prev.u32(sender + 0x0E), cur.u32(sender + 0x0E));
                        if a >= 70 && b == a % 70 {
                            pending += (a / 70) as u16;
                        }
                    }
                    p.pending_garbage = pending;
                }
                _ => {}
            }
        }
        // Shared state the unported opponent also writes.
        engine.icon_gen = [prev.u8(ICON_GEN), prev.u8(ICON_GEN + 1)];
        engine.chaining = [prev.u8(CHAINING) != 0, prev.u8(CHAINING + 1) != 0];
        if let Some(o) = prev.player_actor(other) {
            engine.external_pending[(other & 1 ^ 1) as usize] = prev.u16(o + 0x14);
        }
        // The other side's AI tables (its analysis feeds this side's `crowding`).
        engine.ai_fields[player ^ 1] = AiField::from_bytes(prev.bytes(AI_FIELD[player ^ 1], off::SIZE));
        engine.time_frames = prev.u16(0xFF_05CA); // HandleTime advances it in the vertical blank
        if tag == P1 {
            inputs.pads[0] = PadInput { hold: cur.u8(P1_CTRL), press: cur.u8(P1_CTRL + 1), repeat: cur.u8(P1_CTRL + 2) };
            inputs.keys[0] = RotateKeys(cur.bytes(P1_KEYS, 3).try_into().unwrap());
        } else {
            // The pad on the CPU's port (it keeps a few of its bits).
            inputs.pads[1] = PadInput { hold: cur.u8(P1_CTRL + 6), press: cur.u8(P1_CTRL + 7), repeat: cur.u8(P1_CTRL + 8) };
        }
        engine.frame_count = prev.u16(FRAME_COUNT); // the vertical blank counts it
        // The match can end from outside (the opponent topped out): stop there.
        let player_slot = engine.slots.iter().position(|s| matches!(s, Some(Actor::Player(_)))).unwrap();
        let pa = slot_addr(player_slot);
        if cur.u8(pa) != tag || !(0x3D00..0x4100).contains(&cur.code(pa)) {
            *stats.unported.entry("match ended by the other side").or_default() += 1;
            return Some(f);
        }
        let before = engine.clone();
        engine.rng = Rng::new(prev.u32(RNG_SEED));
        if let Err(Unported(what)) = engine.frame(&inputs) {
            *stats.unported.entry(what).or_default() += 1;
            return Some(f);
        }
        for side in 0..2 {
            stats.icon_redraws += usize::from(engine.icons_drawn[side]);
        }
        absorb_foreign_icons(&mut engine, cur, tag);
        // The ROM's own code can overwrite a slot we model through a stale pointer held
        // by an unported actor (seen with a full actor table). Can't be predicted: stop.
        let overwritten = (0..SLOTS).any(|slot| {
            let ours = matches!(&engine.slots[slot], Some(a) if a.code().is_some());
            let a = slot_addr(slot);
            // (Match-level actors, which this test never runs, count as unported.)
            let foreign = match load(cur, slot, tag) {
                None => !known_code(cur.code(a)),
                Some(actor) => is_match_level(&actor),
            };
            ours && used(cur, slot) && cur.u8(a) == tag && foreign
        });
        if overwritten {
            *stats.unported.entry("slot overwritten by an unported actor").or_default() += 1;
            return Some(f);
        }
        for s in engine.slots.iter().flatten() {
            let kind = match s {
                Actor::Burst(_) => Some("burst frames"),
                Actor::Piece(_) => Some("piece frames"),
                Actor::GarbageClear(_) => Some("garbage clear frames"),
                Actor::Glow(g) if g.state == GlowState::Fly => Some("glow flight frames"),
                Actor::Glow(_) => Some("glow frames"),
                Actor::GarbageDrop(_) => Some("garbage drop frames"),
                Actor::GarbageColumn(c) if c.landed => Some("landed single garbage frames"),
                Actor::GarbageColumn(_) => Some("falling garbage column frames"),
                Actor::Icon(_) => Some("icon frames"),
                _ => None,
            };
            if let Some(k) = kind {
                *stats.visuals.entry(k).or_default() += 1;
            }
            if let Actor::Player(p) = s {
                if p.pop.is_some() {
                    stats.pop_frames += 1;
                    chain_seen = chain_seen.max(p.chain + 1);
                }
            }
            fell |= matches!(s, Actor::Pivot(v) if v.state == PivotState::Falling)
                || matches!(s, Actor::Satellite(t) if t.state == SatelliteState::Falling);
        }
        // 1. The whole frame as modelled. 2. Unported actors drew random numbers before
        // some of ours: try every placement. 3. The ROM's logic overran the video frame and
        // the snapshot caught RunActors before slot `cut`: try every cut.
        let rom_seed = cur.u32(RNG_SEED);
        let rom_calls = {
            let mut r = Rng::new(prev.u32(RNG_SEED));
            (0..=600).find(|_| {
                let hit = r.seed == rom_seed;
                r.next();
                hit
            })
        };
        let exact = first_mismatch(&engine, cur, tag, player).is_none();
        let mut verdict = None;
        let mut icon_only = false;
        if exact && engine.rng.seed == rom_seed {
            verdict = Some("exact");
        } else if exact && rom_calls.is_some_and(|n| n > engine.rng_calls) {
            verdict = Some("foreign"); // only unported actors' draws differ
        } else if let Some(n) = rom_calls.filter(|&n| n > engine.rng_calls) {
            let ours = engine.rng_calls;
            let users = engine.rng_users.clone();
            for split in splits(n - ours, users.len()) {
                let mut trial = before.clone();
                trial.rng = Rng::new(prev.u32(RNG_SEED));
                trial.foreign_random = users.iter().copied().zip(split.clone()).collect();
                let ran = trial.frame(&inputs);
                let trip = first_mismatch(&trial, cur, tag, player);
                if std::env::var("MBM_RNG_DEBUG").is_ok() {
                    eprintln!("frame {f} split {split:?} users {users:?}: {:?} calls {} vs {ours}: {:?}", ran, trial.rng_calls, trip);
                }
                // The other (unported) side can take garbage icons away this test can't follow.
                icon_only |= ran.is_ok()
                    && trial.rng_calls == ours
                    && trip.as_deref().is_some_and(|m| m.starts_with("slot ") && m.contains("our \"icon "));
                if ran.is_ok() && trial.rng_calls == ours && trip.is_none() {
                    engine = trial;
                    verdict = Some("resolved");
                    break;
                }
            }
        }
        if verdict.is_none() {
            'cuts: for cut in 0..SLOTS {
                let run_cut = |foreign: Vec<(usize, u32)>| {
                    let mut trial = before.clone();
                    trial.rng = Rng::new(prev.u32(RNG_SEED));
                    trial.foreign_random = foreign;
                    trial.vblank();
                    trial.run_slots(0..cut, &inputs).ok().map(|_| trial)
                };
                let Some(trial) = run_cut(vec![]) else { continue };
                let Some(n) = rom_calls.filter(|&n| n >= trial.rng_calls) else { continue };
                if first_mismatch(&trial, cur, tag, player).is_none() {
                    engine = trial;
                    finishing = Some(cut);
                    verdict = Some("split");
                    break;
                }
                // Unported actors may also have drawn numbers in that first half.
                let (ours, users) = (trial.rng_calls, trial.rng_users.clone());
                for split in splits(n - ours, users.len()) {
                    let Some(t) = run_cut(users.iter().copied().zip(split).collect()) else { continue };
                    if t.rng_calls == ours && first_mismatch(&t, cur, tag, player).is_none() {
                        engine = t;
                        finishing = Some(cut);
                        verdict = Some("split");
                        break 'cuts;
                    }
                }
            }
        }
        match verdict {
            Some("exact") => {
                stats.rng_frames += 1;
                stats.rng_calls += engine.rng_calls as usize;
            }
            Some("foreign") => stats.rng_foreign += 1,
            Some("resolved") => {
                stats.rng_foreign += 1;
                stats.rng_resolved += 1;
            }
            Some(_) => stats.split_frames += 1,
            None if icon_only => {
                *stats.unported.entry("garbage icons the other side took away").or_default() += 1;
                return Some(f);
            }
            None => {
                let m = first_mismatch(&engine, cur, tag, player)
                    .unwrap_or_else(|| format!("we drew {} random numbers, the ROM {:?}", engine.rng_calls, rom_calls));
                // Diagnostic: the cut that gets furthest, and what it trips on.
                let best = (0..SLOTS)
                    .map(|cut| {
                        let mut trial = before.clone();
                        trial.rng = Rng::new(prev.u32(RNG_SEED));
                        trial.vblank();
                        let _ = trial.run_slots(0..cut, &inputs);
                        let first_bad = (0..SLOTS).find(|&sl| slot_matches(&trial, cur, tag, sl).is_err()).unwrap_or(SLOTS);
                        (first_bad, cut, first_mismatch(&trial, cur, tag, player), trial.rng_calls)
                    })
                    .max_by_key(|t| t.0)
                    .unwrap();
                panic!(
                    "{} frame {f} (piece from {start}): {m}
  best cut {:X} (good up to slot {:X}, {} draws vs ROM {:?}): {:?}",
                    path.display(), best.1, best.0, best.3, rom_calls, best.2
                );
            }
        }
        stats.frames += 1;
    }
    stats.pieces += 1;
    stats.falls += usize::from(fell);
    stats.chained_pieces += usize::from(chain_seen > 0);
    stats.longest_chain = stats.longest_chain.max(chain_seen);
    Some(spawn + 1)
}
