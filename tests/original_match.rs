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
    let mut e = globals_from(ram);
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

/// Everything but the actors: what the game flow leaves for a match (settings, the
/// score so far, tables kept between matches, the random seed, the clocks).
fn globals_from(ram: &Ram) -> Engine {
    let mut e = Engine::new();
    for i in 0..2 {
        e.fields[i] = Field::from_words(&ram.field_words(i));
        let words = ram.field_words(i);
        for (j, w) in words.iter().enumerate() {
            e.field_bytes[i].cells[j] = (w >> 8) as u8;
            e.field_bytes[i].drawn[j] = *w as u8;
        }
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
    e.difficulty_option = ram.u8(0xFF_1120);
    e.bonus_level = ram.u8(0xFF_0114);
    e.bytecode_flag = ram.u8(0xFF_0A3A);
    e.result_score = ram.u32(0xFF_195C);
    e.result_beans = ram.u16(0xFF_1960);
    e.password = ram.u16(0xFF_111E);
    e.high_scores[..0xA0].copy_from_slice(ram.bytes(0xFF_FC04, 0xA0));
    e.high_score_table = ram.u8(0xFF_1972);
    e.glow_xy = (ram.u16(0xFF_19AC), ram.u16(0xFF_19AE));
    e.icon_gen = [ram.u8(ICON_GEN), ram.u8(ICON_GEN + 1)];
    e.chaining = [ram.u8(CHAINING) != 0, ram.u8(CHAINING + 1) != 0];
    e.rng = Rng::new(ram.u32(RNG_SEED));
    e
}

/// What `DrawActors` reads of each actor (draw flags, mapping list, frame, x, y) must
/// be the ROM's, for every actor that is drawn (bit 7 of +$06) in either.
fn draws_match(e: &Engine, ram: &Ram) -> Result<(), String> {
    for slot in 0..SLOTS {
        let Some(ours) = e.draw_of(slot) else { continue };
        let a = slot_addr(slot);
        let rom = Draw { flags: ram.u8(a + 6), mappings: ram.u8(a + 8), frame: ram.u8(a + 9), x: ram.u16(a + 0x0A), y: ram.u16(a + 0x0E) };
        if !ours.shown() && !rom.shown() {
            continue;
        }
        if ours != rom {
            let what = e.slots[slot].as_ref().map(|a| format!("{:X}", a.code().unwrap_or(0))).unwrap_or_default();
            return Err(format!("slot {slot:X} (${what}) draws
  ROM {rom:X?}
  our {ours:X?}"));
        }
    }
    Ok(())
}

/// Everything the engine models, compared with the ROM's RAM.
fn mismatch(e: &Engine, ram: &Ram) -> Option<String> {
    mismatch_except(e, ram, None)
}

/// As `mismatch`, leaving out one slot (an actor the snapshot caught halfway through).
fn mismatch_except(e: &Engine, ram: &Ram, skip: Option<usize>) -> Option<String> {
    for slot in (0..SLOTS).filter(|&s| Some(s) != skip) {
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
        let words = ram.field_words(i);
        for (j, w) in words.iter().enumerate() {
            let (cell, drawn) = ((w >> 8) as u8, *w as u8);
            if (cell, drawn) != (e.field_bytes[i].cells[j], e.field_bytes[i].drawn[j]) {
                return Some(format!(
                    "P{} cell bytes at row {} col {}: ROM {cell:02X}/{drawn:02X} ours {:02X}/{:02X}",
                    i + 1,
                    j / 6,
                    j % 6,
                    e.field_bytes[i].cells[j],
                    e.field_bytes[i].drawn[j]
                ));
            }
        }
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
    let results = (ram.u8(0xFF_0A3A), ram.u32(0xFF_195C), ram.u16(0xFF_1960), ram.u16(0xFF_111E), ram.u8(0xFF_0104));
    if results != (e.bytecode_flag, e.result_score, e.result_beans, e.password, e.difficulty) {
        return Some(format!(
            "results (winner, score, beans, password, CPU difficulty): ROM {results:X?} ours {:X?}",
            (e.bytecode_flag, e.result_score, e.result_beans, e.password, e.difficulty)
        ));
    }
    if ram.bytes(0xFF_FC04, 0xA0) != &e.high_scores[..0xA0] || ram.u8(0xFF_1972) != e.high_score_table {
        return Some("high score tables".into());
    }
    if (0..2).any(|i| (ram.u8(0xFF_19AA + i) != 0) != e.popping[i as usize]) {
        return Some("puyos_popping".into());
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
    finished: usize,
}

fn ram_says_bytecode_runs(ram: &Ram) -> bool {
    ram.u8(0xFF_0A3B) == 0
}

/// Runs one match from `start`. Ok(reason it stopped) or Err(first mismatch).
fn run_match(all: &[Ram], start: usize, poked: &std::collections::HashSet<usize>, run: &mut Run) -> Result<String, String> {
    match engine_from(&all[start]) {
        Ok(engine) => run_from(all, start, engine, poked, run, &mut |f, e, ram| {
            e.plane_writes.clear();
            draws_match(e, ram).map_err(|m| format!("frame {f}: {m}"))
        }),
        Err(e) => Ok(format!("at the start: {e}")),
    }
}

/// Runs `engine` (as of frame `start`) on to the end of the match.
fn run_from(
    all: &[Ram],
    start: usize,
    mut engine: Engine,
    poked: &std::collections::HashSet<usize>,
    run: &mut Run,
    on_frame: &mut dyn FnMut(usize, &mut Engine, &Ram) -> Result<(), String>,
) -> Result<String, String> {
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
            on_frame(f, &mut engine, cur)?;
            continue;
        }
        if prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) {
            // Lag: the vertical blank still ran (clock, plane commands, one Random).
            engine.vblank();
            if let Some(m) = mismatch(&engine, cur) {
                return Err(format!("frame {f} (lag frame): {m}"));
            }
            run.lag += 1;
            on_frame(f, &mut engine, cur)?;
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
            // The logic may have overrun the video frame: find where RunActors was cut
            // (between actors, or in the middle of actor `cut`, which then shows half done).
            let cut = (1..SLOTS).find_map(|cut| {
                let mut t = before.clone();
                t.vblank();
                t.run_slots(0..cut, &inputs).ok()?;
                let ok = mismatch(&t, cur).is_none() || mismatch_except(&t, cur, Some(cut)).is_none();
                ok.then_some((cut, t))
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
        if finishing.is_none() {
            on_frame(f, &mut engine, cur)?;
        }
        if engine.match_over {
            run.finished += 1;
            if ram_says_bytecode_runs(cur) {
                return Ok(format!("frame {f}: match over, the game flow takes over"));
            }
            return Err(format!("frame {f}: we ended the match, the ROM's bytecode is still disabled"));
        }
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
            // Also a recording that starts in the middle of a match (from a savestate),
            // when everything in it is ported.
            let mid_match = f == 0
                && !waiting(&all[0])
                && engine_from(&all[0]).is_ok_and(|e| {
                    // (a match still being set up is the other test's business)
                    let players = || e.slots.iter().filter_map(|s| if let Some(Actor::Player(p)) = s { Some(p) } else { None });
                    players().count() > 0 && players().all(|p| !matches!(p.state, PlayerState::Setup | PlayerState::WaitReady))
                });
            if !mid_match && (!waiting(&all[f]) || (f > 0 && waiting(&all[f - 1])) || poked.contains(&f)) {
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
        "{matches} matches ({} played to the end), {} frames identical ({} lag, {} split), {} pairs dealt; stopped at: {stops:?}",
        run.finished, run.frames, run.lag, run.split, run.pieces
    );
}

/// The frames where the game flow starts a match: P1's player actor appears ($3D80 at
/// the end of its first frame), none in the frame before.
fn match_setups(all: &[Ram]) -> Vec<usize> {
    let p1_at = |r: &Ram, code: u32| r.actors().any(|a| r.u8(a) == P1 && r.code(a) == code);
    let any_p1 = |r: &Ram| r.actors().any(|a| r.u8(a) == P1);
    (1..all.len()).filter(|&f| p1_at(&all[f], 0x3D80) && !any_p1(&all[f - 1])).collect()
}

/// Matches built from nothing: the game flow's settings and seed from before
/// `GenPuyoOrder`, no actors at all. The engine deals the bean orders, starts the match
/// and the stage on the frames the bytecode does, then runs it to the end.
#[test]
fn matches_start_from_the_settings_alone() {
    if !support::install_rom() {
        return;
    }
    let mut run = Run::default();
    let (mut started, mut stops) = (0, std::collections::BTreeMap::<String, usize>::new());
    let mut opponents = std::collections::BTreeMap::<u8, usize>::new();
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let all: Vec<Ram> = frames(&path).collect();
        for s in match_setups(&all) {
            let orders = |r: &Ram| [r.bytes(BEAN_ORDERS[0], 256).to_vec(), r.bytes(BEAN_ORDERS[1], 256).to_vec()];
            // GenPuyoOrder's frame: the last change of the bean orders before the setup.
            let Some(g) = (s.saturating_sub(8)..s).rev().find(|&f| f > 0 && orders(&all[f]) != orders(&all[f - 1])) else {
                *stops.entry("no GenPuyoOrder found".into()).or_insert(0) += 1;
                continue;
            };
            if (g - 1..=s).any(|f| poked.contains(&f)) || all[g - 1].actors().any(|a| used(&all[g - 1], slot_of(a))) {
                continue;
            }
            started += 1;
            *opponents.entry(all[g - 1].u8(0xFF_0113)).or_insert(0) += 1;
            let mut e = globals_from(&all[g - 1]);
            let fail = |f: usize, what: &str, m: String| -> String { format!("{} frame {f} ({what}): {m}", path.display()) };
            // GenPuyoOrder, then the frames up to the setup.
            e.vblank();
            e.deal_bean_orders();
            if let Some(m) = mismatch(&e, &all[g]) {
                panic!("{}", fail(g, "bean orders", m));
            }
            for f in g + 1..s {
                e.vblank();
                if let Some(m) = mismatch(&e, &all[f]) {
                    panic!("{}", fail(f, "before the setup", m));
                }
            }
            // loc_3C00 and the new actors' first frame.
            e.vblank();
            if let Err(Unported(what)) = e.start_match().and_then(|_| e.run_slots(0..SLOTS, &inputs_for(&all[s]))) {
                *stops.entry(what.to_string()).or_insert(0) += 1;
                continue;
            }
            if let Some(m) = mismatch(&e, &all[s]) {
                panic!("{}", fail(s, "loc_3C00", m));
            }
            // A vertical blank, then the portrait art decompresses with interrupts off.
            e.vblank();
            if let Some(m) = mismatch(&e, &all[s + 1]) {
                panic!("{}", fail(s + 1, "before loc_6070", m));
            }
            let mut f = s + 2;
            // (no vertical blank, no actors: the frame counter and the actor table stand still)
            let still = |a: &Ram, b: &Ram| a.u16(FRAME_COUNT) == b.u16(FRAME_COUNT) && a.bytes(0xFF_E000, 0x1000) == b.bytes(0xFF_E000, 0x1000);
            while f < all.len() && still(&all[f], &all[f - 1]) {
                f += 1;
            }
            // loc_6070: the portrait and the stage's actors, then a frame of actors.
            e.vblank();
            if let Err(Unported(what)) = e.start_stage().and_then(|_| e.run_slots(0..SLOTS, &inputs_for(&all[f]))) {
                *stops.entry(what.to_string()).or_insert(0) += 1;
                continue;
            }
            if let Some(m) = mismatch(&e, &all[f]) {
                panic!("{}", fail(f, "loc_6070", m));
            }
            let mut drain = |f: usize, e: &mut Engine, ram: &Ram| {
                e.plane_writes.clear();
                draws_match(e, ram).map_err(|m| format!("frame {f}: {m}"))
            };
            match run_from(&all, f, e, &poked, &mut run, &mut drain) {
                Ok(why) => {
                    let key = why.split_once(": ").map_or(why.as_str(), |(_, k)| k).to_string();
                    *stops.entry(key).or_insert(0) += 1;
                }
                Err(m) => panic!("{}: {m}", path.display()),
            }
        }
    }
    eprintln!(
        "{started} matches started from the settings alone (by opponent: {opponents:?}), {} frames identical ({} played to the end); stopped at: {stops:?}",
        run.frames, run.finished
    );
}

/// The field in plane A against VDP snapshots taken during the same bot run as
/// `batch/k0_f3` (`out/vdp/k0_f3_NNNNNN.vdp`): from each match's start the screen is
/// built from the ROM, the engine's plane writes are applied every frame, and at the
/// snapshot frames both fields' plane cells must be the ROM's.
#[test]
fn field_planes_match_vdp_snapshots() {
    use bean_machine::mbm::assets::{planes, screen, Rom, RomAssets, Vram};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::mbm::rom::get().unwrap());
    let src = RomAssets::new().unwrap();
    let Some(path) = recordings().into_iter().find(|p| p.ends_with("batch/k0_f3.ramrec") || p.ends_with("batch\\k0_f3.ramrec")) else {
        return;
    };
    let vdp_dir = path.parent().unwrap().parent().unwrap().join("vdp");
    let snaps: std::collections::BTreeMap<usize, Vec<u8>> = std::fs::read_dir(&vdp_dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().into_string().ok()?;
            let frame = name.strip_prefix("k0_f3_")?.strip_suffix(".vdp")?.parse().ok()?;
            Some((frame, std::fs::read(e.path()).ok()?))
        })
        .collect();
    if snaps.is_empty() {
        eprintln!("skipping: no VDP snapshots in {}", vdp_dir.display());
        return;
    }
    let all: Vec<Ram> = frames(&path).collect();
    let poked = support::poked_frames(&path);
    let (mut run, mut compared) = (Run::default(), 0);
    let waiting = |r: &Ram| [P1, P2].iter().all(|&t| r.player_actor(t).is_some_and(|a| r.code(a) == PLAYER_WAITING));
    for start in (0..all.len()).filter(|&f| waiting(&all[f]) && (f == 0 || !waiting(&all[f - 1]))) {
        let Ok(engine) = engine_from(&all[start]) else { continue };
        let mut vram: Vram = screen::match_vram(&rom, &src, engine.opponent).unwrap();
        planes::match_planes(&rom, &mut vram).unwrap();
        let mut check = |f: usize, e: &mut Engine, _: &Ram| -> Result<(), String> {
            for (at, w) in e.plane_writes.drain(..) {
                vram.load(at, &w.to_be_bytes());
            }
            let Some(snap) = snaps.get(&f) else { return Ok(()) };
            let want = &snap[8 + 0x10000..8 + 0x20000];
            for (player, first_col) in [(0, 2usize), (1, 26)] {
                for row in 2..26 {
                    for col in first_col..first_col + 12 {
                        let at = 0xC000 + row * 0x80 + col * 2;
                        if vram.0[at..at + 2] != want[at..at + 2] {
                            return Err(format!(
                                "frame {f}: P{} plane A row {row} col {col}: ROM {:02X}{:02X} ours {:02X}{:02X}",
                                player + 1,
                                want[at],
                                want[at + 1],
                                vram.0[at],
                                vram.0[at + 1]
                            ));
                        }
                    }
                }
            }
            compared += 1;
            Ok(())
        };
        if let Err(m) = run_from(&all, start, engine, &poked, &mut run, &mut check) {
            panic!("{}: {m}", path.display());
        }
    }
    eprintln!("{compared} snapshots: both fields in plane A identical ({} frames run)", run.frames);
    assert!(compared > 0);
}
