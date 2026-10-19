//! Frame by frame: P1's controlled pair, rebuilt from RAM and stepped once, must match the
//! ROM's next frame (column, row, rotation, fall progress, slide, contacts, lock timer,
//! lock flag, satellite animation), or land on the same frame the ROM does.
//!
//! Only P1: its inputs are in RAM. The CPU's inputs never touch RAM.

mod support;

use bean_machine::mbm::field::Field;
use bean_machine::mbm::pair::{Pair, PadInput, RotateKeys, Step};
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P1_CTRL, P1_KEYS, PIVOT_FALLING, PIVOT_LANDING};

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
    }
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
            let step = pair.step(
                &field,
                pad(inputs),
                keys,
                speed,
                inputs.u16(FRAME_COUNT),
                sat < pivot,
            );
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
