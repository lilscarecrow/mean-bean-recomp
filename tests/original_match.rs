//! Whole matches on the engine alone: load the ROM's state once, as both players wait
//! for the match to start, then run every frame with nothing from RAM but the two
//! controller ports. Every actor slot, both fields, the CPU's tables, the clock and the
//! random seed must stay identical to the ROM's until the match ends (or the engine
//! reaches something it doesn't port yet, which the test reports).
//!
//! Lag frames (the ROM's logic not finishing within a video frame) depend on CPU cycle
//! counts, which the port doesn't model: there the engine runs only the vertical blank,
//! as the ROM does. Frames where the logic overran into the next video frame are matched
//! by finding where `RunActors` was cut.

mod support;

use bean_machine::mbm::actors::*;
use bean_machine::mbm::ai::{off, AiField};
use bean_machine::mbm::field::Field;
use bean_machine::mbm::pair::{PadInput, RotateKeys};
use bean_machine::mbm::Rng;
use support::actors::*;
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P1_CTRL, P1_KEYS, P2};

const RNG_SEED: u32 = 0xFF_1116;
const AI_FIELD: [u32; 2] = [0xFF_1D62, 0xFF_20CC];
const CPU_MEMORY: [u32; 2] = [0xFF_1D4E, 0xFF_1D58];
const CPU_READS: u32 = 0xFF_1D0A;
const TIME_FRAMES: u32 = 0xFF_05CA;
const PLAYER_FLAGS: u32 = 0xFF_0144;
const BEAN_ORDERS: [u32; 2] = [0xFF_123E, 0xFF_133E];
const ICON_GEN: u32 = 0xFF_19B0;
const CHAINING: u32 = 0xFF_19B2;
const PLAYER_WAITING: u32 = 0x3DA0;
const PAL_FADE: u32 = 0xFF_0AD6;

/// The engine as the ROM's RAM has it at `ram` (both players waiting to start).
fn engine_from(ram: &Ram) -> Result<Engine, String> {
    let mut e = Engine::new();
    for i in 0..2 {
        e.fields[i] = Field::from_words(&ram.field_words(i));
        e.ai_fields[i] = AiField::from_bytes(ram.bytes(AI_FIELD[i], off::SIZE));
        e.cpu_memory[i] = ram.bytes(CPU_MEMORY[i], 10).try_into().unwrap();
        e.cpu_reads[i] = ram.u8(CPU_READS + i as u32);
        e.bean_orders[i] = ram.bytes(BEAN_ORDERS[i], 256).try_into().unwrap();
        e.player_flags[i] = ram.u8(PLAYER_FLAGS + i as u32);
        e.popping[i] = ram.u8(0xFF_19AA + i as u32) != 0;
    }
    e.level_mode = ram.u8(0xFF_1964);
    e.difficulty = ram.u8(0xFF_0104);
    e.opponent = ram.u8(0xFF_0113);
    e.level = ram.u8(0xFF_0112);
    e.control_player_1 = ram.u8(0xFF_1958);
    e.crowding = ram.u16(0xFF_198C);
    e.time_frames = ram.u16(TIME_FRAMES);
    e.frame_count = ram.u16(FRAME_COUNT);
    e.referee_hit = ram.u16(0xFF_19A8);
    e.danger_flash = ram.u8(0xFF_1121);
    e.portrait_lock = ram.u16(0xFF_1124);
    e.pal_fade_gen = [0, 1, 2, 3].map(|i| ram.u16(PAL_FADE + i * 0x82));
    e.glow_xy = (ram.u16(0xFF_19AC), ram.u16(0xFF_19AE));
    e.icon_gen = [ram.u8(ICON_GEN), ram.u8(ICON_GEN + 1)];
    e.chaining = [ram.u8(CHAINING) != 0, ram.u8(CHAINING + 1) != 0];
    e.rng = Rng::new(ram.u32(RNG_SEED));
    for slot in 0..SLOTS {
        if !used(ram, slot) {
            continue;
        }
        let a = slot_addr(slot);
        let actor = load(ram, slot, ram.u8(a)).ok_or_else(|| format!("slot {slot:X}: actor ${:X} isn't ported", ram.code(a)))?;
        e.slots[slot] = Some(actor);
    }
    Ok(e)
}

/// Everything the engine models, compared with the ROM's RAM.
fn mismatch(e: &Engine, ram: &Ram) -> Option<String> {
    for slot in 0..SLOTS {
        let a = slot_addr(slot);
        match &e.slots[slot] {
            None if used(ram, slot) => {
                return Some(format!("slot {slot:X}: ROM has actor ${:X} (tag {:02X}), we have nothing", ram.code(a), ram.u8(a)));
            }
            None => {}
            Some(ours) => {
                if !used(ram, slot) {
                    return Some(format!("slot {slot:X}: we have {}, the ROM's slot is free", describe(ours)));
                }
                let want = describe_ram(ram, slot, ram.u8(a), ours);
                let got = describe(ours);
                if want.as_deref() != Some(got.as_str()) {
                    return Some(format!("slot {slot:X}\n  ROM {want:?} (code ${:X})\n  our {got:?}", ram.code(a)));
                }
            }
        }
    }
    for i in 0..2 {
        let rom = Field::from_words(&ram.field_words(i));
        if rom != e.fields[i] {
            return Some(format!("P{} field\n  ROM\n{rom:?}  our\n{:?}", i + 1, e.fields[i]));
        }
        let lo = off::UNK2;
        if ram.bytes(AI_FIELD[i] + lo as u32, off::SIZE - lo) != &e.ai_fields[i].bytes[lo..] {
            return Some(format!("P{} AI tables", i + 1));
        }
        if ram.bytes(CPU_MEMORY[i], 10) != e.cpu_memory[i] || ram.u8(CPU_READS + i as u32) != e.cpu_reads[i] {
            return Some(format!("P{} CPU memory / pad reads", i + 1));
        }
        if ram.u8(PLAYER_FLAGS + i as u32) != e.player_flags[i] {
            return Some(format!("P{} flags: ROM {:02X} ours {:02X}", i + 1, ram.u8(PLAYER_FLAGS + i as u32), e.player_flags[i]));
        }
    }
    if ram.u16(TIME_FRAMES) != e.time_frames {
        return Some(format!("time_frames: ROM {} ours {}", ram.u16(TIME_FRAMES), e.time_frames));
    }
    let gens = [0, 1, 2, 3].map(|i| ram.u16(PAL_FADE + i * 0x82));
    if gens != e.pal_fade_gen {
        return Some(format!("palette fade generations: ROM {gens:?} ours {:?}", e.pal_fade_gen));
    }
    if ram.u8(0xFF_1121) != e.danger_flash {
        return Some(format!("danger flash: ROM {} ours {}", ram.u8(0xFF_1121), e.danger_flash));
    }
    if ram.u16(0xFF_198C) != e.crowding {
        return Some(format!("crowding: ROM {} ours {}", ram.u16(0xFF_198C), e.crowding));
    }
    if ram.u32(RNG_SEED) != e.rng.seed {
        return Some(format!("random seed: ROM {:08X} ours {:08X} ({} draws this frame)", ram.u32(RNG_SEED), e.rng.seed, e.rng_calls));
    }
    None
}

fn inputs_for(ram: &Ram) -> Inputs {
    let mut inputs = Inputs::default();
    inputs.pads[0] = PadInput { hold: ram.u8(P1_CTRL), press: ram.u8(P1_CTRL + 1), repeat: ram.u8(P1_CTRL + 2) };
    inputs.pads[1] = PadInput { hold: ram.u8(P1_CTRL + 6), press: ram.u8(P1_CTRL + 7), repeat: ram.u8(P1_CTRL + 8) };
    inputs.keys[0] = RotateKeys(ram.bytes(P1_KEYS, 3).try_into().unwrap());
    inputs
}

#[derive(Default)]
struct Run {
    frames: usize,
    lag: usize,
    split: usize,
    pieces: usize,
}

/// Runs one match from `start`. Ok(reason it stopped) or Err(first mismatch).
fn run_match(all: &[Ram], start: usize, poked: &std::collections::HashSet<usize>, run: &mut Run) -> Result<String, String> {
    let mut engine = match engine_from(&all[start]) {
        Ok(e) => e,
        Err(e) => return Ok(format!("at the start: {e}")),
    };
    let mut finishing: Option<usize> = None;
    for f in start + 1..all.len() {
        if poked.contains(&f) {
            return Ok(format!("a test scenario wrote RAM at frame {f}"));
        }
        let (prev, cur) = (&all[f - 1], &all[f]);
        // The controllers are the only input.
        let inputs = inputs_for(cur);
        if let Some(cut) = finishing.take() {
            engine.vblank();
            if let Err(Unported(what)) = engine.run_slots(cut..SLOTS, &inputs) {
                return Ok(format!("frame {f}: {what}"));
            }
            if let Some(m) = mismatch(&engine, cur) {
                return Err(format!("frame {f}, finishing a split frame at slot {cut:X}: {m}"));
            }
            run.frames += 1;
            continue;
        }
        if prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) {
            // Lag: the vertical blank still ran (clock, plane commands, one Random).
            engine.vblank();
            if let Some(m) = mismatch(&engine, cur) {
                return Err(format!("frame {f} (lag frame): {m}"));
            }
            run.lag += 1;
            continue;
        }
        let before = engine.clone();
        let pieces_before = count_pairs(&engine);
        trace(f, &engine, prev, cur, &inputs);
        match engine.frame(&inputs) {
            Err(Unported(what)) => return Ok(format!("frame {f}: {what}")),
            Ok(()) => {}
        }
        if let Some(m) = mismatch(&engine, cur) {
            // The logic may have overrun the video frame: find where RunActors was cut.
            let cut = (1..SLOTS).find_map(|cut| {
                let mut t = before.clone();
                t.vblank();
                t.run_slots(0..cut, &inputs).ok()?;
                mismatch(&t, cur).is_none().then_some((cut, t))
            });
            match cut {
                Some((cut, t)) => {
                    engine = t;
                    finishing = Some(cut);
                    run.split += 1;
                }
                None => return Err(format!("frame {f} (match from {start}): {m}")),
            }
        }
        run.pieces += count_pairs(&engine).saturating_sub(pieces_before);
        run.frames += 1;
    }
    Ok("the recording ended".into())
}

/// `MBM_TRACE=from-to`: print a line per frame in that range (before the frame runs).
fn trace(f: usize, e: &Engine, prev: &Ram, cur: &Ram, inputs: &Inputs) {
    let Ok(range) = std::env::var("MBM_TRACE") else { return };
    let (a, b) = range.split_once('-').unwrap_or((&range, &range));
    let (a, b): (usize, usize) = (a.parse().unwrap_or(0), b.parse().unwrap_or(usize::MAX));
    if !(a..=b).contains(&f) {
        return;
    }
    let score = |r: &Ram, t: u8| r.player_actor(t).map_or(0, |p| r.u32(p + 0x0A));
    eprintln!(
        "frame {f}: frame_count ROM {}->{} ours {} | pad1 {:?} | P1 score ROM {}->{} | seed ROM {:08X} ours {:08X}",
        prev.u16(FRAME_COUNT), cur.u16(FRAME_COUNT), e.frame_count, inputs.pads[0], score(prev, P1), score(cur, P1),
        prev.u32(RNG_SEED), e.rng.seed
    );
}

/// Pairs dealt so far (the holders' counters).
fn count_pairs(e: &Engine) -> usize {
    e.slots.iter().map(|s| if let Some(Actor::PairHolder(h)) = s { usize::from(h.dealt) } else { 0 }).sum()
}

#[test]
fn whole_matches_run_on_the_engine_alone() {
    if !support::install_rom() {
        return;
    }
    let mut run = Run::default();
    let mut matches = 0;
    let mut stops = std::collections::BTreeMap::<String, usize>::new();
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let all: Vec<Ram> = frames(&path).collect();
        for f in 0..all.len() {
            let waiting = |r: &Ram| [P1, P2].iter().all(|&t| r.player_actor(t).is_some_and(|a| r.code(a) == PLAYER_WAITING));
            if !waiting(&all[f]) || (f > 0 && waiting(&all[f - 1])) || poked.contains(&f) {
                continue;
            }
            matches += 1;
            match run_match(&all, f, &poked, &mut run) {
                Ok(why) => {
                    let key = why.split_once(": ").map_or(why.as_str(), |(_, k)| k).to_string();
                    eprintln!("{} match from {f}: stopped, {why}", path.display());
                    *stops.entry(key).or_default() += 1;
                }
                Err(m) => panic!("{}: {m}", path.display()),
            }
        }
    }
    eprintln!(
        "{matches} matches, {} frames identical ({} lag, {} split), {} pairs dealt; stopped at: {stops:?}",
        run.frames, run.lag, run.split, run.pieces
    );
}
