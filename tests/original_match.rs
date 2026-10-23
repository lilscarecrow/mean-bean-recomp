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

use bean_machine::actors::*;
use bean_machine::ai::{off, AiField};
use bean_machine::field::Field;
use bean_machine::pair::{PadInput, RotateKeys};
use bean_machine::Rng;
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
/// `palette_buffer`, `palette_pointers` (flag word + pointer per line: into the buffer
/// itself, or to colors in ROM or RAM) and `pal_fade_data` as the engine keeps them.
fn palette_state(ram: &Ram) -> ([[u16; 16]; 4], [Option<PaletteLoad>; 4], [FadeData; 4]) {
    const BUFFER: u32 = 0xFF_0A56;
    let colors = |at: u32| -> [u16; 16] {
        std::array::from_fn(|c| {
            let a = at + c as u32 * 2;
            if a >= 0xFF_0000 { ram.u16(a) } else { bean_machine::rom::word(a) }
        })
    };
    let palettes = std::array::from_fn(|l| colors(BUFFER + l as u32 * 0x20));
    let loads = std::array::from_fn(|l| {
        let at = 0xFF_0A3E + l as u32 * 6;
        if ram.u16(at) == 0 {
            return None;
        }
        let ptr = ram.u32(at + 2) & 0xFF_FFFF;
        Some(if (BUFFER..BUFFER + 0x80).contains(&ptr) { PaletteLoad::Buffer } else { PaletteLoad::Colors(colors(ptr)) })
    });
    let fades = std::array::from_fn(|l| {
        let at = PAL_FADE + l as u32 * 0x82;
        FadeData { current: ram.bytes(at + 2, 48).try_into().unwrap(), step: ram.bytes(at + 0x32, 48).try_into().unwrap() }
    });
    (palettes, loads, fades)
}

/// The actors of the screens a savestate may start a run on: the menus, the high score
/// table.
fn menu_screen(a: &Actor) -> bool {
    matches!(
        a,
        Actor::MainMenu(_) | Actor::Options(_) | Actor::SoundTest(_) | Actor::PasswordEntry(_) | Actor::HighScoreTable(_) | Actor::TableNameEntry(_) | Actor::TitleHandler
    )
}

/// The game flow's RAM.
fn flow_from(e: &mut Engine, ram: &Ram) {
    e.bytecode_pc = ram.u32(0xFF_0A36);
    e.bytecode_disabled = ram.u8(0xFF_0A3B) != 0;
    e.vdp_regs = ram.bytes(0xFF_0A22, 0x13).try_into().unwrap();
    e.hscroll = std::array::from_fn(|i| ram.u16(0xFF_0622 + i as u32 * 2));
    e.use_lair = ram.u8(0xFF_112A);
    e.intro_cue = ram.u16(0xFF_1990);
    e.intro_198e = ram.u16(0xFF_198E);
    e.opponent_x = ram.u16(0xFF_1994);
    e.word_1126 = ram.u16(0xFF_1126);
    e.last_mode = ram.u8(0xFF_0105);
    e.options = ram.bytes(0xFF_FCA4, 9).try_into().unwrap();
    e.swap_controls = ram.u8(0xFF_1966);
    e.use_plane_a_buffer = ram.u16(0xFF_1956) != 0;
    e.word_112c = ram.u16(0xFF_112C);
    e.score_at = ram.u16(0xFF_198A);
    e.lesson = ram.u16(0xFF_196E);
    e.lesson_tick = ram.u8(0xFF_1129);
    e.vs_wins = [ram.u8(0xFF_0128), ram.u8(0xFF_0129)];
    e.vs_left = [ram.u8(0xFF_012A), ram.u8(0xFF_012B)];
    e.vs_levels = [ram.u16(0xFF_010E), ram.u16(0xFF_0110)];
    e.ex_out = ram.u8(0xFF_1965);
    e.sound_test = ram.u16(0xFF_FC02);
    e.byte_196a = ram.u8(0xFF_196A);
    e.attract_turn = ram.u8(0xFF_1970);
    e.demo_stage = ram.u8(0xFF_1971);
    e.misc_buffer = ram.bytes(0xFF_6800, 0x2000).to_vec();
    e.plane_a_buffer = (0..0x700).map(|i| ram.u16(0xFF_C000 + i * 2)).collect();
    e.text_tick = ram.u8(0xFF_1128);
    e.hblank_count = ram.u16(0xFF_5000);
    e.clouds_slot = (ram.u32(0xFF_112C).wrapping_sub(0xFF_E000) / 0x40) as usize;
    e.repeat_times = (ram.u8(0xFF_1108), ram.u8(0xFF_1109));
    e.input_lock = ram.u16(0xFF_1122);
    e.level_transition = ram.u16(0xFF_1974);
    e.last_winner = ram.u8(0xFF_0115);
    e.opponents_defeated = ram.bytes(0xFF_0116, 18).try_into().unwrap();
    e.eni_buffer = (0..0x800).map(|i| ram.u16(0xFF_3000 + i * 2)).collect();
}

/// What `flow_from` loads, plus level and opponent, for comparing.
#[derive(Debug, PartialEq)]
struct FlowSnap {
    pc: u32,
    disabled: bool,
    level: u8,
    opponent: u8,
    regs: [u8; 0x13],
    hscroll: Vec<u16>,
    intro: (u8, u16, u16, u16, u16, u8),
    repeat: (u8, u8),
    lock: u16,
    transition: u16,
    winner: u8,
    defeated: [u8; 18],
    options: [u8; 9],
    menu: (u8, bool),
    attract: (u8, u8),
    lesson: (u16, u16, u8, u8, u8),
    vs: ([u8; 2], [u8; 2], [u16; 2]),
    ex: (u8, u8),
    paused: u8,
    sound_test: u16,
}

fn flow_snap(e: &Engine) -> FlowSnap {
    FlowSnap {
        pc: e.bytecode_pc,
        disabled: e.bytecode_disabled,
        level: e.level,
        opponent: e.opponent,
        regs: e.vdp_regs,
        hscroll: e.hscroll.to_vec(),
        intro: (e.use_lair, e.intro_cue, e.intro_198e, e.opponent_x, e.word_1126, e.text_tick),
        repeat: e.repeat_times,
        lock: e.input_lock,
        transition: e.level_transition,
        winner: e.last_winner,
        defeated: e.opponents_defeated,
        options: e.options,
        menu: (e.last_mode, e.use_plane_a_buffer),
        attract: (e.attract_turn, e.demo_stage),
        lesson: (e.lesson, e.score_at, e.lesson_tick, e.cpu_pressed, e.cpu_lean),
        vs: (e.vs_wins, e.vs_left, e.vs_levels),
        ex: (e.ex_out, e.byte_196a),
        paused: e.p1_paused,
        sound_test: e.sound_test,
    }
}

fn flow_snap_ram(ram: &Ram) -> FlowSnap {
    FlowSnap {
        pc: ram.u32(0xFF_0A36),
        disabled: ram.u8(0xFF_0A3B) != 0,
        level: ram.u8(0xFF_0112),
        opponent: ram.u8(0xFF_0113),
        regs: ram.bytes(0xFF_0A22, 0x13).try_into().unwrap(),
        hscroll: (0..0x200).map(|i| ram.u16(0xFF_0622 + i * 2)).collect(),
        intro: (ram.u8(0xFF_112A), ram.u16(0xFF_1990), ram.u16(0xFF_198E), ram.u16(0xFF_1994), ram.u16(0xFF_1126), ram.u8(0xFF_1128)),
        repeat: (ram.u8(0xFF_1108), ram.u8(0xFF_1109)),
        lock: ram.u16(0xFF_1122),
        transition: ram.u16(0xFF_1974),
        winner: ram.u8(0xFF_0115),
        defeated: ram.bytes(0xFF_0116, 18).try_into().unwrap(),
        options: ram.bytes(0xFF_FCA4, 9).try_into().unwrap(),
        menu: (ram.u8(0xFF_0105), ram.u16(0xFF_1956) != 0),
        attract: (ram.u8(0xFF_1970), ram.u8(0xFF_1971)),
        lesson: (ram.u16(0xFF_196E), ram.u16(0xFF_198A), ram.u8(0xFF_1129), ram.u8(0xFF_1D0C), ram.u8(0xFF_1D0D)),
        vs: ([ram.u8(0xFF_0128), ram.u8(0xFF_0129)], [ram.u8(0xFF_012A), ram.u8(0xFF_012B)], [ram.u16(0xFF_010E), ram.u16(0xFF_0110)]),
        ex: (ram.u8(0xFF_1965), ram.u8(0xFF_196A)),
        paused: ram.u8(0xFF_0143),
        sound_test: ram.u16(0xFF_FC02),
    }
}

/// `vscroll_buffer`.
const VSCROLL: u32 = 0xFF_05D2;

fn globals_from(ram: &Ram) -> Engine {
    let mut e = Engine::new();
    e.score_at = ram.u16(0xFF_198A);
    e.p1_paused = ram.u8(0xFF_0143);
    e.pause_press = [ram.u8(P1_CTRL + 1), ram.u8(P1_CTRL + 7)];
    e.cpu_pressed = ram.u8(0xFF_1D0C);
    e.cpu_lean = ram.u8(0xFF_1D0D);
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
    e.vscroll = std::array::from_fn(|i| ram.u16(VSCROLL + i as u32 * 2));
    flow_from(&mut e, ram);
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
    (e.palettes, e.palette_loads, e.pal_fades) = palette_state(ram);
    e.difficulty_option = ram.u8(0xFF_1120);
    e.bonus_level = ram.u8(0xFF_0114);
    e.bytecode_flag = ram.u8(0xFF_0A3A);
    e.result_score = ram.u32(0xFF_195C);
    e.result_beans = ram.u16(0xFF_1960);
    e.password = ram.u16(0xFF_111E);
    e.high_scores[..0xA0].copy_from_slice(ram.bytes(0xFF_FC04, 0xA0));
    e.high_score_table = ram.u8(0xFF_1972);
    e.high_score_both = ram.u8(0xFF_1973) != 0;
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
            // (with an actor left halfway, the ROM may already have what it spawns)
            None if used(ram, slot) && skip.is_none() => {
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
    let (mut want, got) = (flow_snap_ram(ram), flow_snap(e));
    if skip.is_some() {
        // (the half-run actor may already have set the scroll or the lair flag)
        want.hscroll = got.hscroll.clone();
        want.intro = got.intro;
    }
    if want != got {
        return Some(format!("game flow
  ROM {want:X?}
  our {got:X?}"));
    }
    if skip.is_some() {
        // The rest may show the half-run actor's first writes; it's all compared once
        // that actor has finished.
        return None;
    }
    let vscroll: [u16; 40] = std::array::from_fn(|i| ram.u16(VSCROLL + i as u32 * 2));
    if vscroll != e.vscroll {
        return Some(format!("vscroll_buffer
  ROM {vscroll:04X?}
  our {:04X?}", e.vscroll));
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
    let (palettes, loads, fades) = palette_state(ram);
    if palettes != e.palettes {
        return Some(format!("palette_buffer
  ROM {palettes:04X?}
  our {:04X?}", e.palettes));
    }
    if loads != e.palette_loads {
        return Some(format!("palette loads
  ROM {loads:04X?}
  our {:04X?}", e.palette_loads));
    }
    // A fade actor cut in the middle shows its line's fade data already stepped.
    let half_done_fade = skip.and_then(|s| match &e.slots[s] {
        Some(Actor::PaletteFade(p)) => Some(usize::from(p.line)),
        _ => None,
    });
    for line in 0..4 {
        // (only lines that have faded: the rest is whatever an earlier screen left)
        if Some(line) != half_done_fade && e.pal_fades[line] != FadeData::default() && fades[line] != e.pal_fades[line] {
            return Some(format!("line {line} fade data
  ROM {:02X?}
  our {:02X?}", fades[line], e.pal_fades[line]));
        }
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
    /// Game-flow frames run (between matches), and the last frame a run reached.
    flow: usize,
    last: usize,
}

/// `bytecode_done` clear: the snapshot caught `RunBytecode` in the middle of a run.
fn bytecode_mid_run(ram: &Ram) -> bool {
    ram.u8(0xFF_0A3B) == 0 && ram.u8(0xFF_0A3C) == 0
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
    // A frame the ROM's vertical blank cut in the middle of RunActors: the slots left,
    // and the vertical blanks seen while they haven't run yet (a long actor step).
    // (with the original's resume address of the actor at the cut when that actor took
    // a long step: it's still running until that changes)
    let mut finishing: Option<(usize, u16, Option<u32>)> = None;
    // (the vertical blank at the cut was already taken, inside the actor before it)
    let mut taken = 0u16;
    // A logic frame whose bytecode run spans several video frames (art decompression):
    // we run it at once, then only the ROM's vertical blanks until it's done.
    // (with the RNG seed of each snapshot a vertical blank followed, and how many)
    let mut pending: Option<(Inputs, u16, Vec<(u32, u16)>)> = None;
    // Draws owed by a vertical blank taken before its Random ran (it finishes before
    // the next frame's).
    let mut owed_random = 0;
    for f in start + 1..all.len() {
        run.last = f;
        if poked.contains(&f) {
            return Ok(format!("a test scenario wrote RAM at frame {f}"));
        }
        let (prev, cur) = (&all[f - 1], &all[f]);
        // The controllers are the only input.
        let inputs = inputs_for(cur);
        let vblanks = cur.u16(FRAME_COUNT).wrapping_sub(prev.u16(FRAME_COUNT));
        if let Some((started, mut during, mut marks)) = pending.take() {
            // Vertical blanks came in the middle of the long frame: the engine takes them
            // at its first long step. Its logic uses the pads as they were when it began.
            during += vblanks;
            if vblanks != 0 {
                marks.push((prev.u32(RNG_SEED), vblanks));
            }
            if bytecode_mid_run(cur) {
                pending = Some((started, during, marks));
                continue;
            }
            let before = engine.clone();
            // Where the vertical blanks came: at the frame's first long step, or (marks)
            // in the middle of GenPuyoOrder's shuffle: each where the snapshot before it
            // caught the CPU, after that many draws or one more, or inside a Random call
            // that then wrote over the vertical blank's draw; or after the shuffle, once
            // the bytecode is done. (The placement that deals the orders in RAM.)
            let mut setup = (during, 0u16, vec![]);
            {
                // (the shuffle's draws: 504 when P2's order differs)
                let draws = if cur.u8(0xFF_1964) == 1 { 504 } else { 256 };
                let (mut inside, mut tail) = (vec![], 0u16);
                let mut seen = 0usize;
                for &(seed, n) in &marks {
                    let Some(steps) = rng_steps(before.rng.seed, seed) else { break };
                    if steps >= seen + draws {
                        tail += n;
                    } else if steps > seen {
                        inside.extend(std::iter::repeat(steps - seen).take(usize::from(n)));
                    }
                    seen += usize::from(n);
                }
                let orders = |r: &Ram| [r.bytes(BEAN_ORDERS[0], 256).to_vec(), r.bytes(BEAN_ORDERS[1], 256).to_vec()];
                let combos = if inside.is_empty() { 0 } else { 3usize.pow(inside.len() as u32) };
                let found = (0..combos).find_map(|c| {
                    let order_vblanks: Vec<(usize, bool)> = inside
                        .iter()
                        .enumerate()
                        .map(|(i, &d)| match c / 3usize.pow(i as u32) % 3 {
                            0 => (d, false),
                            1 => (d + 1, false),
                            _ => (d, true),
                        })
                        .collect();
                    let mut t = before.clone();
                    t.deferred_vblanks = during - inside.len() as u16 - tail;
                    t.deferred_tail = tail;
                    t.order_vblanks = order_vblanks.clone();
                    t.run_bytecode(&started).ok()?;
                    let dealt = [t.bean_orders[0].to_vec(), t.bean_orders[1].to_vec()];
                    // (GenPuyoOrder ran and took them)
                    (t.order_vblanks.is_empty() && dealt == orders(cur)).then_some((during - inside.len() as u16 - tail, tail, order_vblanks))
                });
                if let Some(found) = found {
                    setup = found;
                }
            }
            let prepared = |e: &Engine| {
                let mut t = e.clone();
                t.deferred_vblanks = setup.0;
                t.deferred_tail = setup.1;
                t.order_vblanks = setup.2.clone();
                t
            };
            engine = prepared(&before);
            if let Err(Unported(what)) = engine.logic(&started) {
                return Ok(format!("frame {f}: {what}"));
            }
            // (or the last vertical blank came after the frame's last long step)
            if mismatch(&engine, cur).is_some() && setup.0 > 1 {
                let mut t = prepared(&before);
                t.deferred_vblanks -= 1;
                t.deferred_tail += 1;
                if t.logic(&started).is_ok() && mismatch(&t, cur).is_none() {
                    engine = t;
                }
            }
            if let Some(m) = mismatch(&engine, cur) {
                // It may also have been cut in the middle of RunActors.
                let try_cut = |cut: usize, exact: bool| {
                    let mut t = prepared(&before);
                    t.run_bytecode(&started).ok()?;
                    t.deferred_vblanks += std::mem::take(&mut t.deferred_tail);
                    t.long_step();
                    t.run_slots(0..cut, &started).ok()?;
                    let ok = if exact { mismatch(&t, cur).is_none() } else { mismatch_except(&t, cur, Some(cut)).is_none() };
                    ok.then_some((cut, t))
                };
                let cut = if std::env::var("MBM_NO_SPLIT").is_ok() {
                    None
                } else {
                    (0..SLOTS).find_map(|cut| try_cut(cut, true)).or_else(|| (0..SLOTS).rev().find_map(|cut| try_cut(cut, false)))
                };
                let Some((cut, t)) = cut else { return Err(format!("frame {f} (end of a long bytecode frame): {m}")) };
                engine = t;
                finishing = Some((cut, 0, None));
                run.split += 1;
                continue;
            }
            run.flow += 1;
            on_frame(f, &mut engine, cur)?;
            continue;
        }
        if vblanks != 0 {
            for _ in 0..std::mem::take(&mut owed_random) {
                engine.random_draw();
            }
        }
        if vblanks == 0 {
            continue; // interrupts off (NemDec): the video frame passed without a vertical blank
        }
        if cur.u32(RNG_SEED) == prev.u32(RNG_SEED) && !bytecode_mid_run(cur) {
            // A vertical blank still busy with big plane commands: its Random (the last
            // thing it does) hasn't run yet. It finishes before the next logic frame.
            (0..vblanks).for_each(|_| engine.vblank());
            continue;
        }
        if bytecode_mid_run(cur) {
            // The logic frame began after this frame's vertical blank.
            (0..vblanks).for_each(|_| engine.vblank());
            pending = Some((inputs, 0, vec![]));
            continue;
        }
        if let Some((cut, seen, long_code)) = finishing.take() {
            let flow_of = |r: &Ram| (r.u32(0xFF_0A36), r.u8(0xFF_0A3B));
            let vblanks = cur.u16(FRAME_COUNT).wrapping_sub(prev.u16(FRAME_COUNT));
            if std::env::var("MBM_CUT_DEBUG").is_ok() {
                eprintln!("frame {f}: finishing at {cut:X}? same actors {}", prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000));
            }
            let unchanged = prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) && flow_of(prev) == flow_of(cur);
            if unchanged || long_code == Some(cur.code(slot_addr(cut))) {
                // Still in the long step of the actor at the cut.
                finishing = Some((cut, seen + vblanks, long_code));
                continue;
            }
            // (the vertical blanks since the cut)
            let n = (seen + vblanks).saturating_sub(std::mem::take(&mut taken));
            let before = engine.clone();
            for _ in 0..n {
                engine.vblank();
            }
            if let Err(Unported(what)) = engine.run_slots(cut..SLOTS, &inputs) {
                return Ok(format!("frame {f}: {what}"));
            }
            // (or the last snapshot caught the actor at the cut halfway, having drawn from
            // the RNG: the last vertical blank then came between its draws)
            if mismatch(&engine, cur).is_some() && n > 0 {
                let mut t = before.clone();
                for _ in 1..n {
                    t.vblank();
                }
                let inside = rng_steps(t.rng.seed, prev.u32(RNG_SEED)).filter(|&k| k > 0).and_then(|k| {
                    t.vblank_after_draws = Some(k as u32);
                    t.run_slots(cut..SLOTS, &inputs).ok()?;
                    (t.vblank_after_draws.is_none() && mismatch(&t, cur).is_none()).then_some(t)
                });
                if let Some(t) = inside {
                    if std::env::var("MBM_CUT_DEBUG").is_ok() {
                        eprintln!("frame {f}: vertical blank inside the actor at {cut:X}");
                    }
                    engine = t;
                }
            }
            if let Some(m) = mismatch(&engine, cur) {
                // Or the long frame ended before the last vertical blank, and the next
                // logic frame is already in the same actor's next long step.
                // (or the vertical blank came after the long frame ended)
                let after = {
                    let mut t = before.clone();
                    for _ in 1..n {
                        t.vblank();
                    }
                    t.run_slots(cut..SLOTS, &inputs).is_ok().then(|| {
                        t.vblank();
                        t
                    })
                };
                if let Some(t) = after.filter(|t| mismatch(t, cur).is_none()) {
                    engine = t;
                    run.frames += 1;
                    on_frame(f, &mut engine, cur)?;
                    continue;
                }
                // (or the rest of the frame was cut again, at a later slot)
                let again = |k: usize, exact: bool| {
                    let mut t = before.clone();
                    for _ in 0..n {
                        t.vblank();
                    }
                    t.run_slots(cut..k, &inputs).ok()?;
                    let ok = if exact { mismatch(&t, cur).is_none() } else { mismatch_except(&t, cur, Some(k)).is_none() };
                    ok.then_some((k, t))
                };
                let later = (cut + 1..SLOTS).find_map(|k| again(k, true)).or_else(|| (cut + 1..SLOTS).rev().find_map(|k| again(k, false)));
                if let Some((k, t)) = later {
                    engine = t;
                    finishing = Some((k, 0, None));
                    run.split += 1;
                    continue;
                }
                // (or it ended, and the next frame was cut too: at the same actor if that's
                // a long step, else wherever the rest of the frame matches)
                let next_frame = |k: usize, exact: bool| {
                    let mut t = before.clone();
                    for _ in 1..n {
                        t.vblank();
                    }
                    t.run_slots(cut..SLOTS, &inputs).ok()?;
                    t.vblank();
                    t.run_bytecode(&inputs).ok()?;
                    t.long_step();
                    t.long_actor = None;
                    t.run_slots(0..k, &inputs).ok()?;
                    let ok = if exact { mismatch(&t, cur).is_none() } else { mismatch_except(&t, cur, Some(k)).is_none() };
                    ok.then_some((k, t))
                };
                let next = match long_code {
                    Some(_) => next_frame(cut, false),
                    None => (0..SLOTS).find_map(|k| next_frame(k, true)).or_else(|| (0..SLOTS).rev().find_map(|k| next_frame(k, false))),
                };
                let Some((k, t)) = next else {
                    // (what the same cut into the next frame disagrees on, for debugging)
                    let probe = {
                        let mut t = before.clone();
                        for _ in 1..n {
                            t.vblank();
                        }
                        let _ = t.run_slots(cut..SLOTS, &inputs);
                        t.vblank();
                        let _ = t.run_bytecode(&inputs);
                        t.long_step();
                        let _ = t.run_slots(0..cut, &inputs);
                        mismatch_except(&t, cur, Some(cut))
                    };
                    return Err(format!("frame {f}, finishing a split frame at slot {cut:X}: {m}
(into the next frame: {probe:?})"));
                };
                engine = t;
                finishing = Some((k, 0, long_code.map(|_| cur.code(slot_addr(k)))));
                run.split += 1;
                continue;
            }
            run.frames += 1;
            on_frame(f, &mut engine, cur)?;
            continue;
        }
        // (the opponent screen's controller only moves the scroll)
        // (and the plane A buffer: the input test only writes there)
        let flow_state = |r: &Ram| (r.u32(0xFF_0A36), r.u8(0xFF_0A3B), r.bytes(0xFF_0622, 0x400).to_vec(), r.bytes(0xFF_C000, 0xE00).to_vec());
        if prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) && flow_state(prev) == flow_state(cur) {
            // Lag: the vertical blank still ran (clock, plane commands, one Random).
            // (Unless an actor began a long step whose first writes show: then it's a
            // split frame, below.)
            let mut t = engine.clone();
            t.vblank();
            if mismatch(&t, cur).is_some() {
                // (the vertical blank still busy, before its Random)
                let mut u = engine.clone();
                u.vblank_before_random();
                if mismatch(&u, cur).is_none() {
                    t = u;
                    owed_random += 1;
                }
            }
            match mismatch(&t, cur) {
                None => {
                    engine = t;
                    run.lag += 1;
                    on_frame(f, &mut engine, cur)?;
                    continue;
                }
                Some(m) => {
                    // (unless it was a frame after all: one whose logic took a long step,
                    // or ended in CheckPause's wait having toggled a pause)
                    let mut probe = engine.clone();
                    let ran = probe.frame(&inputs).is_ok();
                    if !ran || (probe.long_actor.is_none() && mismatch(&probe, cur).is_some()) {
                        return Err(format!("frame {f} (lag frame): {m}"));
                    }
                }
            }
        }
        let before = engine.clone();
        let pieces_before = count_pairs(&engine);
        trace(f, &engine, prev, cur, &inputs);
        match engine.frame(&inputs) {
            Err(Unported(what)) => return Ok(format!("frame {f}: {what}")),
            Ok(()) => {}
        }
        if f == start + 1 && mismatch(&engine, cur).is_some() {
            // The snapshot we started from caught a split frame: its remaining slots run now.
            let finish = (0..SLOTS).find_map(|cut| {
                let mut t = before.clone();
                t.vblank();
                t.run_slots(cut..SLOTS, &inputs).ok()?;
                mismatch(&t, cur).is_none().then_some(t)
            });
            if let Some(t) = finish {
                engine = t;
                run.split += 1;
                on_frame(f, &mut engine, cur)?;
                continue;
            }
        }
        if mismatch(&engine, cur).is_some() {
            // A lag frame whose vertical blank wrote into an actor (a plane command
            // clearing a player's flag), so the actor area did change.
            let mut t = before.clone();
            t.vblank();
            if mismatch(&t, cur).is_none() {
                engine = t;
                run.lag += 1;
                on_frame(f, &mut engine, cur)?;
                continue;
            }
        }
        if let Some(m) = mismatch(&engine, cur) {
            // The logic may have overrun the video frame: find where RunActors was cut
            // (between actors, or in the middle of actor `cut`, which then shows half done).
            // (MBM_NO_SPLIT: report the mismatch instead, for debugging.)
            let no_split = std::env::var("MBM_NO_SPLIT").is_ok();
            // (an exact match at any cut first, then the last cut that matches with the
            // half-run actor left out: earlier cuts can match by accident when the
            // actors before it didn't change this frame)
            let try_cut = |cut: usize, exact: bool| {
                let mut t = before.clone();
                t.vblank();
                t.run_bytecode(&inputs).ok()?;
                t.long_step();
                t.run_slots(0..cut, &inputs).ok()?;
                let ok = if exact { mismatch(&t, cur).is_none() } else { mismatch_except(&t, cur, Some(cut)).is_none() };
                ok.then_some((cut, t))
            };
            // An actor that decompressed art this frame may be the one cut halfway: by a
            // vertical blank at its long step (then the rest of the frame runs from the
            // next slot), or one it's still in.
            let long = engine.long_actor.filter(|_| !no_split);
            let mid = long.and_then(|s| {
                let mut t = before.clone();
                t.vblank();
                t.run_bytecode(&inputs).ok()?;
                t.long_step();
                t.deferred_vblanks = 1;
                t.run_slots(0..s + 1, &inputs).ok()?;
                (t.deferred_vblanks == 0 && mismatch_except(&t, cur, Some(s)).is_none()).then_some((s + 1, t))
            });
            if let Some((next, t)) = mid {
                if std::env::var("MBM_CUT_DEBUG").is_ok() {
                    eprintln!("frame {f}: split inside slot {:X} (for: {m})", next - 1);
                }
                engine = t;
                finishing = Some((next, 0, None));
                taken = 1;
                run.split += 1;
                continue;
            }
            let cut = if no_split {
                None
            } else {
                long.and_then(|s| try_cut(s, false))
                    .or_else(|| (0..SLOTS).find_map(|cut| try_cut(cut, true)))
                    .or_else(|| (0..SLOTS).rev().find_map(|cut| try_cut(cut, false)))
            };
            match cut {
                Some((cut, t)) => {
                    if std::env::var("MBM_CUT_DEBUG").is_ok() {
                        eprintln!("frame {f}: split at slot {cut:X} (for: {m})");
                    }
                    let long_code = (long == Some(cut)).then(|| cur.code(slot_addr(cut)));
                    engine = t;
                    finishing = Some((cut, 0, long_code));
                    run.split += 1;
                }
                None => {
                    if std::env::var("MBM_CUT_DEBUG").is_ok() {
                        for cut in 1..SLOTS {
                            let mut t = before.clone();
                            t.vblank();
                            let _ = t.run_bytecode(&inputs);
                            t.long_step();
                            if t.run_slots(0..cut, &inputs).is_ok() {
                                eprintln!("cut {cut:X}: {:?}", mismatch_except(&t, cur, Some(cut)));
                            }
                        }
                    }
                    return Err(format!("frame {f} (match from {start}): {m}"));
                }
            }
        }
        run.pieces += count_pairs(&engine).saturating_sub(pieces_before);
        run.frames += 1;
        if finishing.is_none() {
            on_frame(f, &mut engine, cur)?;
        }
        if std::mem::take(&mut engine.match_over) {
            run.finished += 1;
            if !ram_says_bytecode_runs(cur) {
                return Err(format!("frame {f}: we ended the match, the ROM's bytecode is still disabled"));
            }
        }
        if !engine.bytecode_disabled {
            run.flow += 1;
        }
    }
    Ok("the recording ended".into())
}

/// Random calls from one seed to another (if within a frame's worth).
fn rng_steps(from: u32, to: u32) -> Option<usize> {
    let mut r = Rng::new(from);
    (0..2000).find(|_| {
        if r.seed == to {
            return true;
        }
        r.next();
        false
    })
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
        let waiting = |r: &Ram| [P1, P2].iter().all(|&t| r.player_actor(t).is_some_and(|a| r.code(a) == PLAYER_WAITING));
        // Also a recording that starts in the middle of a match (from a savestate),
        // when everything in it is ported: its first frame we can load (in its first
        // ten seconds; a pair in flight can't be).
        let match_running = |r: &Ram| {
            !waiting(r)
                && engine_from(r).is_ok_and(|e| {
                    // (a match still being set up is the other test's business)
                    let players = || e.slots.iter().filter_map(|s| if let Some(Actor::Player(p)) = s { Some(p) } else { None });
                    players().count() > 0 && players().all(|p| !matches!(p.state, PlayerState::Setup | PlayerState::WaitReady))
                })
        };
        let mid_start = (0..all.len().min(600)).find(|&f| !poked.contains(&f) && match_running(&all[f]));
        for f in 0..all.len() {
            let mid_match = Some(f) == mid_start;
            // And the game flow right after the opponent screen (bytecode at $1D3E, no
            // actors): the stage intro, the transition, the match after.
            let at_intro = |r: &Ram| r.u32(0xFF_0A36) == 0x1D3E && r.u8(0xFF_0A3B) == 0 && !r.actors().any(|a| used(r, slot_of(a)));
            let flow_start = f > 0 && at_intro(&all[f]) && !at_intro(&all[f - 1]) && !poked.contains(&f);
            // And one that starts on the high score table (its name entry, from a savestate).
            // And power-on: the game flow at its first routine ($18BA).
            let boot = all[f].u32(0xFF_0A36) == 0x18BA && (f == 0 || all[f - 1].u32(0xFF_0A36) != 0x18BA);
            // And one that starts (from a savestate) on another ported screen: the menus,
            // the high score table.
            let on_table = f == 0 && !mid_match && engine_from(&all[0]).is_ok_and(|e| e.slots.iter().flatten().any(menu_screen));
            if !mid_match && !flow_start && !on_table && !boot && (!waiting(&all[f]) || (f > 0 && waiting(&all[f - 1])) || poked.contains(&f)) {
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


enum Start {
    /// Not a clean start (a scenario poked RAM, or actors left over).
    Skipped,
    /// Something not ported yet.
    Stopped(String),
    /// The stage started: the engine as of `frame`.
    Running { engine: Engine, frame: usize, opponent: u8 },
}

/// The game flow's start of the match set up at frame `s`, from the settings and seed
/// before `GenPuyoOrder`: the bean orders, loc_3C00, the portrait's frames, loc_6070.
/// `after` sees the engine after every frame (to take its plane writes).
fn start_from_settings(all: &[Ram], s: usize, poked: &std::collections::HashSet<usize>, after: &mut dyn FnMut(&mut Engine)) -> Result<Start, String> {
    let orders = |r: &Ram| [r.bytes(BEAN_ORDERS[0], 256).to_vec(), r.bytes(BEAN_ORDERS[1], 256).to_vec()];
    // GenPuyoOrder's frame: the last change of the bean orders before the setup.
    let Some(g) = (s.saturating_sub(8)..s).rev().find(|&f| f > 0 && orders(&all[f]) != orders(&all[f - 1])) else {
        return Ok(Start::Stopped("no GenPuyoOrder found".into()));
    };
    if (g - 1..=s).any(|f| poked.contains(&f)) || all[g - 1].actors().any(|a| used(&all[g - 1], slot_of(a))) {
        return Ok(Start::Skipped);
    }
    // (the demo's setup frame is long, and where its vertical blanks fall among the
    // shuffle's and the pairs' draws can't be told: the demo is checked from its first
    // match frame by the whole-match test)
    if all[g - 1].u8(0xFF_1964) & 4 != 0 {
        return Ok(Start::Skipped);
    }
    let opponent = all[g - 1].u8(0xFF_0113);
    // From here the bytecode (BC_Level) deals the bean orders, runs loc_3C00, loc_6070
    // and Level_DrawSmallText, and disables itself; run_from follows it.
    let mut e = globals_from(&all[g - 1]);
    after(&mut e);
    Ok(Start::Running { engine: e, frame: g - 1, opponent })
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
            let (e, f) = match start_from_settings(&all, s, &poked, &mut |e| e.plane_writes.clear()) {
                Err(m) => panic!("{}: {m}", path.display()),
                Ok(Start::Skipped) => continue,
                Ok(Start::Stopped(what)) => {
                    started += 1;
                    *stops.entry(what).or_insert(0) += 1;
                    continue;
                }
                Ok(Start::Running { engine, frame, opponent }) => {
                    started += 1;
                    *opponents.entry(opponent).or_insert(0) += 1;
                    (engine, frame)
                }
            };
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
    use bean_machine::assets::{planes, screen, Rom, RomAssets, Vram};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::rom::get().unwrap());
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
        let mut boxes = vec![];
        let mut check = |f: usize, e: &mut Engine, _: &Ram| -> Result<(), String> {
            bean_machine::game::apply_vdp(e, &mut vram, &mut boxes, &rom, &src).unwrap();
            let Some(snap) = snaps.get(&f) else { return Ok(()) };
            if !e.slots.iter().flatten().any(|a| matches!(a, Actor::Player(_))) {
                return Ok(()); // (between matches)
            }
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

/// The whole visible screen's planes (40 x 28 cells of A and B) against VDP snapshots
/// taken while replaying recorded bot runs (`out/vdp/<run>_NNNNNN.vdp`, and
/// `out/vdp_dense`, one every 7 frames), for the matches started from the settings: the
/// screen is built from the ROM and every plane write the engine makes from the setup
/// on is applied (fields, centre panel, portrait, the end sequence and results).
#[test]
fn screen_planes_match_vdp_snapshots() {
    use bean_machine::assets::{Rom, RomAssets};
    if !support::install_rom() {
        return;
    }
    let rom = Rom(bean_machine::rom::get().unwrap());
    let src = RomAssets::new().unwrap();
    let (mut run, mut compared) = (Run::default(), 0);
    for path in recordings() {
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let out = path.parent().unwrap().parent().unwrap();
        let prefix = format!("{stem}_");
        let snaps: std::collections::BTreeMap<usize, Vec<u8>> = [out.join("vdp"), out.join("vdp_dense")]
            .iter()
            .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
            .filter_map(|e| {
                let name = e.file_name().into_string().ok()?;
                let frame = name.strip_prefix(&prefix)?.strip_suffix(".vdp")?.parse().ok()?;
                Some((frame, std::fs::read(e.path()).ok()?))
            })
            .collect();
        if snaps.is_empty() {
            continue;
        }
        compared += screen_against(&rom, &src, &path, &snaps, &mut run);
    }
    eprintln!("{compared} snapshots: the visible planes identical ({} frames run)", run.frames);
    assert!(compared > 0, "no VDP snapshots to compare");
}

fn screen_against(
    rom: &bean_machine::assets::Rom,
    src: &bean_machine::assets::RomAssets,
    path: &std::path::PathBuf,
    snaps: &std::collections::BTreeMap<usize, Vec<u8>>,
    run: &mut Run,
) -> usize {
    use bean_machine::assets::{planes, screen, Vram};
    let mut compared = 0;
    let all: Vec<Ram> = frames(path).collect();
    let poked = support::poked_frames(path);
    // Besides the match setups: before the first match, the first frame of the game flow
    // with no actors that has a snapshot (the VRAM starts as the snapshot's).
    let setups = match_setups(&all);
    // (right after the opponent screen's fade: bytecode at $1D3E, nothing queued)
    let quiet = |r: &Ram| {
        r.u32(0xFF_0A36) == 0x1D3E && r.u8(0xFF_0A3B) == 0 && r.u8(0xFF_0A3C) != 0 && !r.actors().any(|a| used(r, slot_of(a)))
    };
    let flow_start = (1..setups.first().copied().unwrap_or(all.len()))
        // (and its vertical blank done: the seed moved; or power-on, the flow at $18BA)
        .find(|&f| {
            let boot = all[f].u32(0xFF_0A36) == 0x18BA && all[f - 1].u32(0xFF_0A36) != 0x18BA;
            // (or a savestate's first frames on a ported screen: the menus)
            // (or in a match, its players past their setup)
            let set_up = |p: &Player| !matches!(p.state, PlayerState::Setup | PlayerState::WaitReady);
            let running = |e: &Engine| {
                let players: Vec<&Player> = e.slots.iter().filter_map(|s| if let Some(Actor::Player(p)) = s { Some(p) } else { None }).collect();
                e.slots.iter().flatten().any(menu_screen) || (!players.is_empty() && players.into_iter().all(set_up))
            };
            let on_screen = f < 600 && engine_from(&all[f]).is_ok_and(|e| running(&e));
            (boot || on_screen || quiet(&all[f]) && all[f].u32(RNG_SEED) != all[f - 1].u32(RNG_SEED)) && snaps.contains_key(&f) && !poked.contains(&f)
        });
    let starts: Vec<(usize, bool)> = flow_start.map(|f| (f, true)).into_iter().chain(setups.iter().map(|&s| (s, false))).collect();
    for (s, from_snapshot) in starts {
        let opponent = all[s].u8(0xFF_0113);
        let mut vram: Vram = screen::match_vram(rom, src, opponent).unwrap();
        planes::match_planes(rom, &mut vram).unwrap();
        if from_snapshot {
            vram.0.copy_from_slice(&snaps[&s][8 + 0x10000..8 + 0x20000]);
        }
        let vram = std::cell::RefCell::new(vram);
        let boxes = std::cell::RefCell::new(vec![]);
        let apply = |e: &mut Engine| {
            bean_machine::game::apply_vdp(e, &mut vram.borrow_mut(), &mut boxes.borrow_mut(), rom, src).unwrap();
        };
        let started = if from_snapshot {
            engine_from(&all[s]).map(|engine| Start::Running { engine, frame: s, opponent }).map_err(|e| e.to_string())
        } else {
            start_from_settings(&all, s, &poked, &mut |e| apply(e))
        };
        let (engine, f) = match started {
            Err(m) => panic!("{}: {m}", path.display()),
            Ok(Start::Running { engine, frame, .. }) => (engine, frame),
            Ok(Start::Stopped(why)) => {
                eprintln!("match set up at {s}: not started ({why})");
                continue;
            }
            Ok(Start::Skipped) => {
                eprintln!("match set up at {s}: skipped (not a clean start)");
                continue;
            }
        };
        let mut check = |f: usize, e: &mut Engine, _: &Ram| -> Result<(), String> {
            if let Ok(r) = std::env::var("MBM_VRAM_DEBUG") {
                let (a, b) = r.split_once('-').unwrap();
                if (a.parse::<usize>().unwrap()..=b.parse::<usize>().unwrap()).contains(&f) {
                    let ev: Vec<String> = e.vdp_events.iter().map(|v| format!("{}@{}", match &v.what { VdpOp::Words { at, words } => format!("Words({at:04X}x{} [{:04X?}])", words.len(), words.get(0x192)), o => format!("{o:X?}") }, v.at)).collect();
                    eprintln!("[vram] frame {f}: pc {:X} writes {} eni_queue {:X?} events {ev:?}", e.bytecode_pc, e.plane_writes.len(), e.eni_queue);
                }
            }
            apply(e);
            let Some(snap) = snaps.get(&f) else { return Ok(()) };
            let want = &snap[8 + 0x10000..8 + 0x20000];
            let vram = vram.borrow();
            let mut wrong = vec![];
            // The visible cells of each plane, as the registers lay them out (the opponent
            // screen's planes are 128 cells wide and scroll sideways).
            let mut regs = [0u8; 24];
            regs[..0x13].copy_from_slice(&e.vdp_regs);
            let layout = bean_machine::render::Layout::from_registers(&regs);
            let width = usize::from(layout.plane_width);
            for (plane, base, scroll) in [("A", usize::from(layout.plane_a), e.hscroll[0]), ("B", usize::from(layout.plane_b), e.hscroll[1])] {
                let mut cols: Vec<usize> = (0..320).map(|x| ((x + 0x400 - usize::from(scroll & 0x3FF)) % (width * 8)) / 8).collect();
                cols.dedup();
                for row in 0..28 {
                    for &col in &cols {
                        let at = base + row * width * 2 + col * 2;
                        if vram.0[at..at + 2] != want[at..at + 2] {
                            wrong.push(format!("{plane} r{row} c{col}: ROM {:02X}{:02X} ours {:02X}{:02X}", want[at], want[at + 1], vram.0[at], vram.0[at + 1]));
                        }
                    }
                }
            }
            // On the opponent screen and in the intro, the art the flow loads (patterns
            // $0000-$B7FF) too.
            let intro = e.slots.iter().flatten().any(|a| matches!(a, Actor::IntroScript(_)));
            if intro || (0x1CDA..0x1D76).contains(&e.bytecode_pc) || e.slots.iter().flatten().any(|a| matches!(a, Actor::OpponentScreen(_))) {
                // (tiles $32C-$32F: the refugee bean's animation, which the front end runs)
                let differs = |t: usize| !(0x32C..0x330).contains(&t) && vram.0[t * 32..t * 32 + 32] != want[t * 32..t * 32 + 32];
                if let Some(t) = (0..0xB800 / 32).find(|&t| differs(t)) {
                    wrong.push(format!("pattern ${t:03X} (VRAM ${:04X})", t * 32));
                }
            }
            if !wrong.is_empty() {
                return Err(format!("frame {f}: {} cells differ: {}", wrong.len(), wrong.iter().take(24).cloned().collect::<Vec<_>>().join(", ")));
            }
            compared += 1;
            Ok(())
        };
        match run_from(&all, f, engine, &poked, run, &mut check) {
            Ok(why) => eprintln!("match from {f}: {why}"),
            Err(m) => panic!("{}: {m}", path.display()),
        }
    }
    compared
}
