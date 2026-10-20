//! Frame by frame: P1's controlled pair, rebuilt from RAM and stepped once, must match the
//! ROM's next frame (column, row, rotation, fall progress, slide, contacts, lock timer,
//! lock flag, satellite animation), or land on the same frame the ROM does.
//!
//! P1 with its pad from RAM; P2 with the CPU's pad (`ai::CpuPad`), which never touches
//! RAM, worked out at each point the pivot reads it.

mod support;

use bean_machine::ai;
use bean_machine::anim::Anim;
use bean_machine::field::Field;
use bean_machine::pair::{Pair, PadInput, RotateKeys, Step};
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P2, P1_CTRL, P1_KEYS, PIVOT_FALLING, PIVOT_LANDING};

static LAG_FRAMES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static COVERAGE: std::sync::Mutex<std::collections::BTreeMap<&'static str, usize>> =
    std::sync::Mutex::new(std::collections::BTreeMap::new());

fn pair_from(ram: &Ram, pivot: u32, sat: u32) -> Pair {
    Pair {
        col: ram.u16(pivot + 0x1A),
        row: ram.u16(pivot + 0x1C),
        rot: ram.u8(pivot + 0x2B),
        fall: ram.u16(pivot + 0x20),
        slide: ram.u16(pivot + 0x1E),
        contacts: ram.u16(pivot + 0x26),
        lock_timer: ram.u16(pivot + 0x28),
        locked: ram.u8(pivot + 7) & 8 != 0,
        sat_angle: ram.u8(sat + 0x36),
        sat_turn: ram.u8(sat + 0x38),
        pivot_anim: anim_from(ram, pivot),
        sat_anim: anim_from(ram, sat),
    }
}

/// aAnim (+$32), aAnimTime (+$22), aFrame (+$09).
fn anim_from(ram: &Ram, actor: u32) -> Anim {
    Anim { ptr: ram.u32(actor + 0x32), time: ram.u8(actor + 0x22), frame: ram.u8(actor + 0x09) }
}

fn pad(ram: &Ram) -> PadInput {
    PadInput { hold: ram.u8(P1_CTRL), press: ram.u8(P1_CTRL + 1), repeat: ram.u8(P1_CTRL + 2) }
}

/// Steps every controlled frame with inputs taken from `input_lag` frames before the
/// result (0 = same frame, 1 = previous). Returns (frames checked, first mismatch).
fn run(input_lag: usize) -> (usize, Option<String>) {
    let mut checked = 0;
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let mut history: Vec<Ram> = Vec::new();
        for (frame, ram) in frames(&path).enumerate() {
            history.push(ram);
            if history.len() > 3 {
                history.remove(0);
            }
            if history.len() < 3 || poked.contains(&frame) || poked.contains(&(frame - 1)) {
                continue;
            }
            let (prev, cur) = (&history[1], &history[2]);
            let Some((pivot, sat)) = prev.pair(P1, false) else { continue };
            // Lag frame: the 68000 ran out of time and no actor ran at all. Depends on CPU
            // cycle counts, which the port deliberately doesn't model.
            if prev.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) {
                LAG_FRAMES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                continue;
            }
            let Some(player) = prev.player_actor(P1) else { continue };
            if ai::is_cpu(prev.u8(support::addr::LEVEL_MODE), 0, prev.u8(CONTROL_P1)) {
                continue; // the CPU plays P1 (the demo): see cpu_pair_matches_the_original_every_frame
            }
            let code_now = cur.code(pivot);
            if code_now != PIVOT_FALLING && !PIVOT_LANDING.contains(&code_now) {
                continue; // the pair went away for another reason (match end)
            }
            let inputs = &history[2 - input_lag];
            let field = Field::from_words(&prev.field_words(0));
            let keys = RotateKeys(prev.bytes(P1_KEYS, 3).try_into().unwrap());
            let mut pair = pair_from(prev, pivot, sat);
            // The fall rate can change during the frame; whichever actor runs first wins.
            let speed = if player < pivot { cur.u16(player + 0x1A) } else { prev.u16(player + 0x1A) };
            if sat < pivot {
                pair.step_satellite();
            }
            let (step, events) = pair.step_pivot(&field, pad(inputs), keys, speed, inputs.u16(FRAME_COUNT));
            if sat > pivot {
                pair.step_satellite();
            }
            // The player actor (an earlier slot) has already run: the flag is still set.
            if player < pivot && (cur.u8(player + 7) & 8 != 0) != events.soft_drop_point {
                let rom = cur.u8(player + 7) & 8 != 0;
                return (checked, Some(format!("{} frame {frame}: soft-drop point: ROM {rom}, ours {}", path.display(), events.soft_drop_point)));
            }
            checked += 1;
            if input_lag == 0 {
                let before = pair_from(prev, pivot, sat);
                let mut cov = COVERAGE.lock().unwrap();
                let mut hit = |k: &'static str, on: bool| *cov.entry(k).or_insert(0) += usize::from(on);
                hit("lock delay running", pair.lock_timer != 0);
                hit("contacts counted", pair.contacts > before.contacts);
                hit("rotations", pair.rot != before.rot);
                hit("sideways kicks", pair.rot != before.rot && pair.col != before.col && before.slide == 0);
                hit("upward kicks", pair.rot != before.rot && pair.row < before.row);
                hit("turns refused", pad(inputs).press & 0x70 != 0 && pair.rot == before.rot && !before.locked);
                hit("blocked moves", pad(inputs).repeat & 0x0C != 0 && pair.col == before.col && before.slide == 0 && !before.locked);
                hit("fast fall (rate > $8000)", speed >= 0x8001);
                hit("satellite slot first", sat < pivot);
                hit("soft drop", pad(inputs).hold & 0x0E == 2);
            }
            let landed = PIVOT_LANDING.contains(&code_now);
            let mismatch = if landed {
                (step != Step::Landed).then(|| format!("ROM landed, we did not: {pair:?}"))
            } else if step == Step::Landed {
                Some("we landed, ROM did not".to_string())
            } else {
                let want = pair_from(cur, pivot, sat);
                (want != pair).then(|| format!("\n want {want:?}\n  got {pair:?}"))
            };
            if let Some(m) = mismatch {
                let input = pad(inputs);
                return (checked, Some(format!("{} frame {frame} (input {input:?}): {m}", path.display())));
            }
        }
    }
    (checked, None)
}

#[test]
fn controlled_pair_matches_the_original_every_frame() {
    let mut report = vec![];
    for lag in [0, 1] {
        let (checked, mismatch) = run(lag);
        match mismatch {
            None => {
                let skipped = LAG_FRAMES.load(std::sync::atomic::Ordering::Relaxed);
                eprintln!("input lag {lag}: {checked} frames, all identical ({skipped} lag frames skipped)");
                eprintln!("coverage: {:?}", COVERAGE.lock().unwrap());
                return;
            }
            Some(m) => report.push(format!("input lag {lag}: failed after {checked} frames: {m}")),
        }
    }
    panic!("{}", report.join("\n"));
}

const LEVEL: u32 = 0xFF_0112;
const CONTROL_P1: u32 = 0xFF_1958;
const CPU_READS: u32 = 0xFF_1D0A;

fn cpu_plan(ram: &Ram, player: u32) -> ai::Plan {
    ai::Plan {
        col: ram.u8(player + 0x20),
        rot: ram.u8(player + 0x21),
        timer: ram.u8(player + 0x26),
        phase: ram.u8(player + 0x27),
        move_frames: ram.u8(player + 0x2C),
        pause_frames: ram.u8(player + 0x2D),
    }
}

/// P2 played by the CPU: the pair stepped with `sub_12E6C` as its pad (read at every
/// point the pivot reads it), the CPU's move/pause timer and its read counter. Random
/// draws by the pad (low levels) come after an unknown number of other draws, so the
/// RNG offset is searched; the frame's final seed must still be reachable.
#[test]
fn cpu_pair_matches_the_original_every_frame() {
    let (mut checked, mut draws, mut lag) = (0usize, 0usize, 0usize);
    let mut cover = std::collections::BTreeMap::<&str, usize>::new();
    for path in recordings() {
        let poked = support::poked_frames(&path);
        let mut prev: Option<Ram> = None;
        for (frame, ram) in frames(&path).enumerate() {
            let Some(p) = prev.replace(ram) else { continue };
            let cur = prev.as_ref().unwrap();
            if poked.contains(&frame) || poked.contains(&(frame - 1)) {
                continue;
            }
            let Some((pivot, sat)) = p.pair(P2, false) else { continue };
            if p.bytes(0xFF_E000, 0x1000) == cur.bytes(0xFF_E000, 0x1000) {
                lag += 1;
                continue;
            }
            let Some(player) = p.player_actor(P2) else { continue };
            let id = p.u8(player + 0x2A);
            if !ai::is_cpu(p.u8(support::addr::LEVEL_MODE), id, p.u8(CONTROL_P1)) {
                continue;
            }
            let code_now = cur.code(pivot);
            if code_now != PIVOT_FALLING && !PIVOT_LANDING.contains(&code_now) {
                continue;
            }
            let field = Field::from_words(&p.field_words(1));
            let speed = if player < pivot { cur.u16(player + 0x1A) } else { p.u16(player + 0x1A) };
            let human = PadInput { hold: cur.u8(P1_CTRL + 6), press: cur.u8(P1_CTRL + 7), repeat: cur.u8(P1_CTRL + 8) };
            let seed = p.u32(support::addr::RNG_SEED);
            let end = cur.u32(support::addr::RNG_SEED);
            let want = (pair_from(cur, pivot, sat), cpu_plan(cur, player), cur.u8(CPU_READS + u32::from(id)));
            let landed = PIVOT_LANDING.contains(&code_now);
            let attempt = |skip: usize| {
                let mut rng = bean_machine::rng::Rng::new(seed);
                for _ in 0..skip {
                    rng.next();
                }
                let mut plan = cpu_plan(&p, player);
                if player < pivot {
                    plan.timer = plan.timer.wrapping_add(1); // the player actor runs first
                }
                let mut reads = p.u8(CPU_READS + u32::from(id));
                let (mut pressed, mut lean) = (0, 0);
                let mut pair = pair_from(&p, pivot, sat);
                let before_rng = rng.seed;
                let step = {
                    let pad = ai::CpuPad {
                        human,
                        plan: &mut plan,
                        player_id: id,
                        level: p.u8(LEVEL),
                        reads: &mut reads,
                        pressed: &mut pressed,
                        lean: &mut lean,
                        rng: &mut rng,
                    };
                    pair.step(&field, pad, ai::CPU_KEYS, speed, cur.u16(FRAME_COUNT), sat < pivot)
                };
                if player > pivot {
                    plan.timer = plan.timer.wrapping_add(1);
                }
                let drew = rng.seed != before_rng;
                let reaches_end = (0..64).any(|_| {
                    let hit = rng.seed == end;
                    rng.next();
                    hit
                });
                (step, pair, plan, reads, drew, reaches_end)
            };
            let mut found = None;
            for skip in 0..64 {
                let (step, pair, plan, reads, drew, reaches_end) = attempt(skip);
                let ok = if landed {
                    step == Step::Landed && (plan, reads) == (want.1, want.2)
                } else {
                    step != Step::Landed && (pair, plan, reads) == want
                };
                if ok && reaches_end {
                    found = Some(drew);
                    break;
                }
                if !drew && skip == 0 && !ok {
                    break; // the pad drew nothing: the RNG offset can't matter
                }
            }
            let Some(drew) = found else {
                let (step, pair, plan, reads, _, _) = attempt(0);
                panic!(
                    "{} frame {frame} (level {}, speed {speed:04X}): CPU pair differs\n want {:?} landed={landed}\n      {:?} reads {}\n  got {:?} {step:?}\n      {:?} reads {}",
                    path.display(),
                    p.u8(LEVEL),
                    want.0,
                    want.1,
                    want.2,
                    pair,
                    plan,
                    reads
                );
            };
            checked += 1;
            draws += usize::from(drew);
            let before = cpu_plan(&p, player);
            let after = cpu_plan(cur, player);
            let mut hit = |k: &'static str, on: bool| *cover.entry(k).or_insert(0) += usize::from(on);
            hit("phase flips", before.phase != after.phase);
            hit("moves", cur.u16(pivot + 0x1A) != p.u16(pivot + 0x1A));
            hit("turns", cur.u8(pivot + 0x2B) != p.u8(pivot + 0x2B));
            hit("landed", landed);
        }
    }
    eprintln!("{checked} CPU frames identical ({draws} with pad random draws, {lag} lag frames skipped); {cover:?}");
}

/// `ReadController` against RAM: P1's recorded hold byte, read through `Pad`, gives the
/// same press and auto-repeat bytes and repeat timers every frame.
#[test]
fn pad_reading_matches_the_original() {
    use bean_machine::pad::Pad;
    let mut checked = 0;
    for path in recordings() {
        let mut pad: Option<Pad> = None;
        let mut times = (0, 0);
        for ram in frames(&path) {
            let rom_pad = Pad {
                hold: ram.u8(P1_CTRL),
                press: ram.u8(P1_CTRL + 1),
                repeat: ram.u8(P1_CTRL + 2),
                horizontal: ram.u8(P1_CTRL + 3),
                vertical: ram.u8(P1_CTRL + 4),
            };
            if let Some(mut ours) = pad {
                // The pads are read before the game flow runs, which may change the
                // repeat times: the previous frame's apply.
                ours.read(rom_pad.hold, times.0, times.1);
                if ours == rom_pad {
                    checked += 1;
                } else if pad != Some(rom_pad) {
                    // (frames where the game didn't read the pads leave them as they were)
                    panic!("{}: ours {ours:?}, ROM {rom_pad:?}", path.display());
                }
            }
            pad = Some(rom_pad);
            times = (ram.u8(0xFF_1108), ram.u8(0xFF_1109));
        }
    }
    eprintln!("{checked} pad reads identical");
}
