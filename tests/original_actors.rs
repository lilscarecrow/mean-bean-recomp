//! Between pieces, frame by frame: from the first frame after a pair lands until the next
//! pair is in play, the ported actors (player loop, pivot, satellite, post-landing
//! effects) must sit in the same slots, at the same resume addresses, with the same
//! timers and positions as the ROM's, and the field must change on the same frames.

mod support;

use bean_machine::mbm::actors::*;
use bean_machine::mbm::anim::Anim;
use bean_machine::mbm::field::Field;
use bean_machine::mbm::motion::Motion;
use bean_machine::mbm::Rng;
use bean_machine::mbm::pair::{PadInput, Pair, RotateKeys};
use bean_machine::mbm::BeanColor;
use support::{frames, recordings, Ram, FRAME_COUNT, P1, P1_CTRL, P1_KEYS, P2};

const RNG_SEED: u32 = 0xFF_1116;
const ICON_GEN: u32 = 0xFF_19B0;
const CHAINING: u32 = 0xFF_19B2;
const ICON_CODE: u32 = 0x9962;

fn icon_side(ram: &Ram, a: u32) -> u8 {
    (ram.u32(a + 0x32).wrapping_sub(ICON_GEN) & 1) as u8
}


const ACTORS: u32 = 0xFF_E000;

fn slot_addr(slot: usize) -> u32 {
    ACTORS + slot as u32 * 0x40
}

fn slot_of(addr: u32) -> usize {
    ((addr - ACTORS) / 0x40) as usize
}

/// A slot is taken when its first word is nonzero (`FindActorSlotQuick`'s test; only
/// `FindActorSlot` also sets bit 7 of the second byte).
fn used(ram: &Ram, slot: usize) -> bool {
    ram.u16(slot_addr(slot)) != 0
}

fn anim_from(ram: &Ram, a: u32) -> Anim {
    Anim { ptr: ram.u32(a + 0x32), time: ram.u8(a + 0x22), frame: ram.u8(a + 0x09) }
}

fn color(ram: &Ram, a: u32) -> BeanColor {
    BeanColor::from_id(ram.u8(a + 8)).unwrap()
}

fn fall_from(ram: &Ram, a: u32) -> Fall {
    Fall { y: ram.u32(a + 0x0E), vel: ram.u32(a + 0x16), accel: ram.u16(a + 0x1E), base: ram.u16(a + 0x20) }
}

/// Rebuilds a ported actor from RAM by its resume address; None if it isn't one.
fn load(ram: &Ram, slot: usize, tag: u8) -> Option<Actor> {
    let a = slot_addr(slot);
    if !used(ram, slot) {
        return None;
    }
    let code = ram.code(a);
    // Icons belong to no player (tag $FF) and behave the same whoever drew them.
    if code == ICON_CODE {
        return Some(Actor::Icon(Icon {
            side: icon_side(ram, a),
            generation: ram.u8(a + 0x28),
            frame: ram.u8(a + 9),
            motion: Motion { x: ram.u32(a + 0x0A), y: ram.u32(a + 0x0E), vx: ram.u32(a + 0x12), ..Motion::default() },
            slide: ram.u16(a + 0x26),
        }));
    }
    if ram.u8(a) != tag {
        return None;
    }
    if code == 0x48A2 || code == 0x48BA {
        return Some(Actor::GarbageDrop(GarbageDrop {
            player_slot: slot_of(ram.u32(a + 0x2E)),
            started: code == 0x48BA,
            falling: ram.u16(a + 0x26),
            seen: ram.u16(a + 0x28),
            strength: ram.u16(a + 0x20),
            shake: ram.u16(a + 0x38),
            phase: ram.u8(a + 0x36),
            voiced: ram.u8(a + 7) & 1 != 0,
            frame: ram.u8(a + 9),
        }));
    }
    if code == 0x486A || code == 0x5044 {
        return Some(Actor::GarbageColumn(GarbageColumn {
            player: ram.u8(a + 0x2A),
            drop_slot: slot_of(ram.u32(a + 0x2E)),
            col: ram.u16(a + 0x1A),
            row: ram.u16(a + 0x1C),
            frame: ram.u8(a + 9),
            fall: fall_from(ram, a),
            landed: code == 0x5044,
            linger: ram.u16(a + 0x26),
        }));
    }
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
    let motion = || Motion {
        x: ram.u32(a + 0x0A),
        y: ram.u32(a + 0x0E),
        vx: ram.u32(a + 0x12),
        vy: ram.u32(a + 0x16),
        ax: ram.u16(a + 0x1A),
        ay: ram.u16(a + 0x1C),
        tx: ram.u16(a + 0x1E),
        ty: ram.u16(a + 0x20),
    };
    if let Some(state) = [BurstState::Wait, BurstState::Burst, BurstState::Animate].into_iter().find(|s| s.code() == code) {
        return Some(Actor::Burst(Burst {
            player: ram.u8(a + 0x2A),
            color: ram.u8(a + 8),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
        }));
    }
    if let Some(state) = [PieceState::Start, PieceState::Wait, PieceState::Fly].into_iter().find(|s| s.code() == code) {
        return Some(Actor::Piece(Piece {
            player: 0,
            color: ram.u8(a + 8),
            motion: motion(),
            draw: ram.u8(a + 6),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
        }));
    }
    if code == 0x4DA4 {
        return Some(Actor::GarbageClear(GarbageClear { player: 0, x: ram.u16(a + 0x0A), y: ram.u16(a + 0x0E), anim: anim_from(ram, a) }));
    }
    let glow_states = [GlowState::Start, GlowState::Hover, GlowState::HoverWait, GlowState::Launch, GlowState::Fly, GlowState::End];
    if let Some(state) = glow_states.into_iter().find(|s| s.code() == code) {
        return Some(Actor::Glow(Glow {
            player: ram.u8(a + 0x2A),
            player_slot: slot_of(ram.u32(a + 0x2E)),
            chain: ram.u8(a + 0x2B),
            motion: motion(),
            draw: ram.u8(a + 6),
            base_x: ram.u16(a + 0x1E),
            counter: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
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
        Actor::Opaque | Actor::OpaqueQuick | Actor::Vanishing => "opaque".into(),
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
        Actor::Burst(b) => format!("burst {:04X} color {} at {:X},{:X} delay {} anim {:?}", b.state.code(), b.color, b.x, b.y, b.delay, b.anim),
        Actor::Piece(p) => format!(
            "piece {:04X} color {} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} delay {} anim {:?}",
            p.state.code(), p.color, p.motion.x, p.motion.y, p.motion.vx, p.motion.vy, p.draw, p.delay, p.anim
        ),
        Actor::GarbageClear(g) => format!("garbage clear at {:X},{:X} anim {:?}", g.x, g.y, g.anim),
        Actor::GarbageDrop(d) => format!(
            "garbage drop {:04X} falling {} seen {} strength {:04X} shake {:04X} phase {:02X} frame {:02X}",
            if d.started { 0x48BA } else { 0x48A2 }, d.falling, d.seen, d.strength, d.shake, d.phase, d.frame
        ),
        Actor::GarbageColumn(c) => format!(
            "garbage column {:04X} col {} row {} frame {:02X} y {:08X} vel {:08X} linger {}",
            if c.landed { 0x5044 } else { 0x486A }, c.col, c.row, c.frame, c.fall.y, c.fall.vel, c.linger
        ),
        Actor::Icon(i) => format!(
            "icon side {} gen {} frame {} x {:08X} y {:08X} vx {:08X} slide {}",
            i.side, i.generation, i.frame, i.motion.x, i.motion.y, i.motion.vx, i.slide
        ),
        Actor::Glow(g) => format!(
            "glow {:05X} chain {} pos {:08X},{:08X} vel {:08X},{:08X} base {:X} counter {:04X} draw {:02X} delay {} anim {:?}",
            g.state.code(), g.chain, g.motion.x, g.motion.y, g.motion.vx, g.motion.vy, g.base_x, g.counter, g.draw, g.delay, g.anim
        ),
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

/// Resume addresses of every actor state the port knows.
fn known_code(code: u32) -> bool {
    matches!(code, 0x3D00..=0x40FF | 0x4F06 | 0x4F14 | 0x5474 | 0x5482) || load_codes().contains(&code)
}

fn load_codes() -> Vec<u32> {
    let mut v = vec![0x4F8A, 0x4FA8, 0x54D4, 0x54FA, 0x5514, 0x4FCE, 0x4FE6, 0x4FF4, 0x4DA4];
    v.extend([BurstState::Wait, BurstState::Burst, BurstState::Animate].map(|s| s.code()));
    v.extend([PieceState::Start, PieceState::Wait, PieceState::Fly].map(|s| s.code()));
    v.extend([GlowState::Start, GlowState::Hover, GlowState::HoverWait, GlowState::Launch, GlowState::Fly, GlowState::End].map(|s| s.code()));
    v
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
    rng_frames: usize,
    rng_calls: usize,
    rng_foreign: usize,
    rng_resolved: usize,
    icon_redraws: usize,
    visuals: std::collections::BTreeMap<&'static str, usize>,
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
            ours && used(cur, slot) && cur.u8(a) == tag && load(cur, slot, tag).is_none() && !known_code(cur.code(a))
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
                trial.foreign_random = users.iter().copied().zip(split).collect();
                if trial.frame(&inputs).is_ok() && trial.rng_calls == ours && first_mismatch(&trial, cur, tag, player).is_none() {
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
