//! Between pieces, frame by frame: from the first frame after a pair lands until the next
//! pair is in play, the ported actors (player loop, pivot, satellite, post-landing
//! effects) must sit in the same slots, at the same resume addresses, with the same
//! timers and positions as the ROM's, and the field must change on the same frames.

mod support;

use bean_machine::mbm::actors::*;
use bean_machine::mbm::anim::Anim;
use bean_machine::mbm::field::Field;
use bean_machine::mbm::pair::{PadInput, Pair, RotateKeys};
use bean_machine::mbm::BeanColor;
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P1_CTRL, P1_KEYS, P2};

const ACTORS: u32 = 0xFF_E000;

fn slot_addr(slot: usize) -> u32 {
    ACTORS + slot as u32 * 0x40
}

fn slot_of(addr: u32) -> usize {
    ((addr - ACTORS) / 0x40) as usize
}

fn used(ram: &Ram, slot: usize) -> bool {
    ram.u8(slot_addr(slot) + 1) & 0x80 != 0
}

fn anim_from(ram: &Ram, a: u32) -> Anim {
    Anim { ptr: ram.u32(a + 0x32), time: ram.u8(a + 0x22), frame: ram.u8(a + 0x09) }
}

fn color(ram: &Ram, a: u32) -> BeanColor {
    BeanColor::from_id(ram.u8(a + 8)).unwrap()
}

fn fall_from(ram: &Ram, a: u32) -> Fall {
    Fall { y: ram.u32(a + 0x0E), vel: ram.u32(a + 0x16), base: ram.u16(a + 0x20) }
}

/// Rebuilds a ported actor from RAM by its resume address; None if it isn't one.
fn load(ram: &Ram, slot: usize, tag: u8) -> Option<Actor> {
    let a = slot_addr(slot);
    if ram.u8(a) != tag || !used(ram, slot) {
        return None;
    }
    let code = ram.code(a);
    let player_state = [
        PlayerState::WaitPair,
        PlayerState::FindPops,
        PlayerState::Redraw,
        PlayerState::Decide,
        PlayerState::SettleDelay,
        PlayerState::AfterDelay,
        PlayerState::WaitGarbage,
        PlayerState::PreSpawn1,
        PlayerState::PreSpawn2,
        PlayerState::PreSpawn3,
        PlayerState::Spawn,
        PlayerState::PopScore,
        PlayerState::PopBackup,
        PlayerState::Blink,
        PlayerState::PopSpawn,
        PlayerState::PopWait,
        PlayerState::Compact,
        PlayerState::FallWait,
        PlayerState::ChainNext,
    ]
    .into_iter()
    .find(|s| s.code() == code);
    if let Some(state) = player_state {
        return Some(Actor::Player(Player {
            id: ram.u8(a + 0x2A),
            state,
            flags: ram.u8(a + 7),
            w26: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
            chain: ram.u8(a + 9),
            fall_speed: ram.u16(a + 0x1A),
            speed_level: ram.u8(a + 0x2B),
            pending_garbage: ram.u16(a + 0x14),
            speed_index: ram.u8(a + 0x08),
            score: ram.u32(a + 0x0A),
            acc: ram.u32(a + 0x0E),
            base: ram.u16(a + 0x12),
            bonus: ram.u16(a + 0x1E),
            beans_total: ram.u16(a + 0x16),
            beans_count: ram.u16(a + 0x18),
            threshold: ram.u16(a + 0x1C),
            garbage_sent: 0,
            pop: None,
        }));
    }
    let pivot_state = [PivotState::Falling, PivotState::Squash].into_iter().find(|s| s.code() == code);
    if let Some(state) = pivot_state {
        let sat = ram.u32(a + 0x36);
        return Some(Actor::Pivot(Pivot {
            player_slot: slot_of(ram.u32(a + 0x2E)),
            satellite_slot: slot_of(sat),
            color: color(ram, a),
            state,
            pair: Pair {
                col: ram.u16(a + 0x1A),
                row: ram.u16(a + 0x1C),
                rot: ram.u8(a + 0x2B),
                pivot_anim: anim_from(ram, a),
                sat_anim: anim_from(ram, sat),
                ..Pair::default()
            },
            flags: ram.u8(a + 7),
            y: ram.u32(a + 0x0E),
            fall: fall_from(ram, a),
        }));
    }
    let sat_state = [SatelliteState::Check, SatelliteState::Falling, SatelliteState::Squash].into_iter().find(|s| s.code() == code);
    if let Some(state) = sat_state {
        return Some(Actor::Satellite(Satellite {
            pivot_slot: usize::MAX,
            player_slot: slot_of(ram.u32(a + 0x2E)),
            color: color(ram, a),
            state,
            col: ram.u16(a + 0x1A),
            row: ram.u16(a + 0x1C),
            flags: ram.u8(a + 7),
            y: ram.u32(a + 0x0E),
            anim: anim_from(ram, a),
            fall: fall_from(ram, a),
        }));
    }
    let effect_state = [EffectState::Wait, EffectState::Start, EffectState::Rise].into_iter().find(|s| s.code() == code);
    effect_state.map(|state| {
        Actor::Effect(Effect {
            player: ram.u8(a + 0x2A),
            state,
            delay: ram.u16(a + 0x24),
            steps_left: ram.u8(a + 0x26),
            ticks: ram.u8(a + 0x28),
            y: ram.u32(a + 0x0E),
        })
    })
}

/// The parts of a ported actor the ROM's RAM must agree with.
fn describe(actor: &Actor) -> String {
    match actor {
        Actor::Opaque => "opaque".into(),
        Actor::Player(p) => format!(
            "player {:04X} flags {:02X} delay {} chain {} score {} acc {} base {} bonus {} beans {}/{} speed {:04X} step {} next {} w26 {:04X}",
            p.state.code(),
            p.flags & 0x17,
            p.delay,
            p.chain,
            p.score,
            p.acc,
            p.base,
            p.bonus,
            p.beans_total,
            p.beans_count,
            p.fall_speed,
            p.speed_index,
            p.threshold,
            p.w26,
        ),
        Actor::Pivot(v) => match v.state {
            PivotState::Init | PivotState::Controlled => format!("pivot {:04X}", v.state.code()),
            _ => format!(
                "pivot {:04X} col {} row {} y {:08X} vel {:08X} anim {:?} flags {:02X}",
                v.state.code(), v.pair.col, v.pair.row, v.y, v.fall.vel, v.pair.pivot_anim, v.flags
            ),
        },
        Actor::Satellite(s) => match s.state {
            SatelliteState::Init | SatelliteState::Follow => format!("satellite {:04X}", s.state.code()),
            _ => format!(
                "satellite {:04X} col {} row {} y {:08X} vel {:08X} anim {:?} flags {:02X}",
                s.state.code(), s.col, s.row, s.y, s.fall.vel, s.anim, s.flags
            ),
        },
        Actor::Effect(e) => format!("effect {:04X} delay {} steps {} ticks {} y {:08X}", e.state.code(), e.delay, e.steps_left, e.ticks, e.y),
    }
}

/// What the ROM shows in a slot, described the same way (None = not a ported actor).
fn describe_ram(ram: &Ram, slot: usize, tag: u8, ours: &Actor) -> Option<String> {
    if let Some(actor) = load(ram, slot, tag) {
        return Some(describe(&actor));
    }
    // Spawned and controlled pairs can't be rebuilt from RAM alone: compare codes only.
    let code = ram.code(slot_addr(slot));
    match ours {
        Actor::Pivot(_) if [0x4F06, 0x4F14].contains(&code) => Some(format!("pivot {code:04X}")),
        Actor::Satellite(_) if [0x5474, 0x5482].contains(&code) => Some(format!("satellite {code:04X}")),
        _ => None,
    }
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
        Some(Actor::Opaque) => Ok(()),
        Some(ours) => {
            let mut want = describe_ram(cur, slot, tag, ours);
            let mut got = describe(ours);
            if tag == P2 && matches!(ours, Actor::Player(_)) {
                // The CPU's (unported) planning uses the player actor's +$26 as scratch.
                let mask = |s: &str| s.split(" w26 ").next().unwrap().to_string();
                want = want.map(|w| mask(&w));
                got = mask(&got);
            }
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

fn first_mismatch(engine: &Engine, cur: &Ram, tag: u8, player: usize) -> Option<String> {
    for slot in 0..SLOTS {
        if let Err(m) = slot_matches(engine, cur, tag, slot) {
            return Some(m);
        }
    }
    (!field_matches(engine, cur, player)).then(|| {
        format!("field
  ROM
{:?}  our
{:?}", Field::from_words(&cur.field_words(player)), engine.fields[player])
    })
}

/// The ROM's logic overran the video frame and the snapshot caught `RunActors` halfway:
/// slots below some cut show the new frame, slots from the cut on still show the old one.
fn split_frame(before: &Engine, after: &Engine, cur: &Ram, tag: u8, player: usize) -> bool {
    let field_ok = field_matches(before, cur, player) || field_matches(after, cur, player);
    field_ok
        && (0..=SLOTS).any(|cut| {
            (0..cut).all(|s| slot_matches(after, cur, tag, s).is_ok())
                && (cut..SLOTS).all(|s| slot_matches(before, cur, tag, s).is_ok())
        })
}

#[derive(Default)]
struct Stats {
    pieces: usize,
    frames: usize,
    unported: std::collections::BTreeMap<&'static str, usize>,
    falls: usize,
    lag_frames: usize,
    split_frames: usize,
    pop_frames: usize,
    chained_pieces: usize,
    longest_chain: u8,
}

#[test]
fn between_pieces_matches_the_original_frame_by_frame() {
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
        "{} pieces, {} frames identical ({} with a physics fall, {} lag and {} split frames);          {} pieces popped ({} frames in pop states, longest chain {}); stopped at unported: {:?}",
        stats.pieces, stats.frames, stats.falls, stats.lag_frames, stats.split_frames,
        stats.chained_pieces, stats.pop_frames, stats.longest_chain, stats.unported
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
    for slot in 0..SLOTS {
        engine.slots[slot] = load(ram, slot, tag).or_else(|| used(ram, slot).then_some(Actor::Opaque));
    }
    if !engine.slots.iter().any(|s| matches!(s, Some(Actor::Player(_)))) {
        return None; // player actor not in a ported state (e.g. mid-chain)
    }
    // The next pair, as the ROM spawned it.
    let spawn = (start..all.len().min(start + 2000)).find(|&f| all[f].pair(tag, false).is_some())?;
    if spawn + 1 >= all.len() {
        return None; // the recording ends first
    }
    if (start..=spawn + 1).any(|f| poked.contains(&f)) {
        return Some(spawn + 1); // a test scenario rewrote RAM in the middle
    }
    let (pv, st) = all[spawn].pair(tag, false)?;
    let mut inputs = Inputs::default();
    inputs.next_pair[player] = Some((color(&all[spawn], pv), color(&all[spawn], st)));
    inputs.spawn_slots[player] = Some((slot_of(pv), slot_of(st)));
    let mut fell = false;
    let mut chain_seen = 0u8;
    // True when the last snapshot caught the ROM halfway through a logic frame: the next
    // snapshot finishes that same frame (see `split_frame`).
    let mut finishing = false;
    for f in start + 1..=spawn + 1 {
        let (prev, cur) = (&all[f - 1], &all[f]);
        if finishing {
            finishing = false;
            if let Some(m) = first_mismatch(&engine, cur, tag, player) {
                panic!("{} frame {f} (piece from {start}), finishing a split frame: {m}", path.display());
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
                None | Some(Actor::Opaque) => engine.slots[slot] = used(prev, slot).then_some(Actor::Opaque),
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
        if tag == P1 {
            inputs.pads[0] = PadInput { hold: cur.u8(P1_CTRL), press: cur.u8(P1_CTRL + 1), repeat: cur.u8(P1_CTRL + 2) };
            inputs.keys[0] = RotateKeys(cur.bytes(P1_KEYS, 3).try_into().unwrap());
        }
        inputs.frame_count = cur.u16(FRAME_COUNT);
        // The match can end from outside (the opponent topped out): stop there.
        let player_slot = engine.slots.iter().position(|s| matches!(s, Some(Actor::Player(_)))).unwrap();
        let pa = slot_addr(player_slot);
        if cur.u8(pa) != tag || !(0x3D00..0x4100).contains(&cur.code(pa)) {
            *stats.unported.entry("match ended by the other side").or_default() += 1;
            return Some(f);
        }
        let before = engine.clone();
        if let Err(Unported(what)) = engine.frame(&inputs) {
            *stats.unported.entry(what).or_default() += 1;
            return Some(f);
        }
        for s in engine.slots.iter().flatten() {
            if let Actor::Player(p) = s {
                if p.pop.is_some() {
                    stats.pop_frames += 1;
                    chain_seen = chain_seen.max(p.chain + 1);
                }
            }
            fell |= matches!(s, Actor::Pivot(v) if v.state == PivotState::Falling)
                || matches!(s, Actor::Satellite(t) if t.state == SatelliteState::Falling);
        }
        match first_mismatch(&engine, cur, tag, player) {
            None => {}
            Some(m) => {
                if split_frame(&before, &engine, cur, tag, player) {
                    stats.split_frames += 1;
                    finishing = true;
                } else {
                    panic!("{} frame {f} (piece from {start}): {m}", path.display());
                }
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
