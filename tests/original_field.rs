//! Per piece: the field when a pair spawns, plus where that pair locked, must turn into
//! exactly the field the ROM has when the next pair spawns (pops, chains, garbage drops).

mod support;

use bean_machine::mbm::field::Field;
use bean_machine::mbm::BeanColor;
use support::{frames, recordings, Ram, GARBAGE_DATA, P1, P2};

const GARBAGE_POINTS: u32 = 70;

#[derive(Default)]
struct Tracker {
    /// Field at the last spawn, and the pair's last seen landing position.
    start: Option<Field>,
    locked: Option<(i16, i16, u8, BeanColor, BeanColor)>,
    /// Garbage drops seen since: (column order, pending before the drop).
    drops: Vec<([u8; 6], u16)>,
    actor: u32,
}

fn pending_sent(prev: &Ram, cur: &Ram, sender: u32) -> u16 {
    let (before, after) = (prev.u32(sender + 0x0E), cur.u32(sender + 0x0E));
    if before >= GARBAGE_POINTS && after == before % GARBAGE_POINTS { (before / GARBAGE_POINTS) as u16 } else { 0 }
}

#[test]
fn field_after_each_piece_matches_the_original() {
    let (mut checked, mut with_chains, mut with_drops) = (0, 0, 0);
    for path in recordings() {
        let mut prev: Option<Ram> = None;
        let mut track = [Tracker::default(), Tracker::default()];
        let poked = support::poked_frames(&path);
        for (frame, ram) in frames(&path).enumerate() {
            let Some(p) = prev.take() else {
                prev = Some(ram);
                continue;
            };
            if poked.contains(&frame) {
                // A test scenario rewrote the board: start this piece from what it wrote.
                for (i, t) in track.iter_mut().enumerate() {
                    if t.start.is_some() {
                        t.start = Some(Field::from_words(&ram.field_words(i)));
                    }
                }
            }
            for (i, tag) in [P1, P2].into_iter().enumerate() {
                let (Some(actor), Some(other)) = (ram.player_actor(tag), ram.player_actor(if tag == P1 { P2 } else { P1 })) else {
                    track[i] = Tracker::default();
                    continue;
                };
                let t = &mut track[i];
                if t.actor != actor || p.player_actor(tag) != Some(actor) {
                    *t = Tracker { actor, ..Tracker::default() };
                }
                // Garbage drop starting this frame (+$07 bit 2 rises).
                if ram.u8(actor + 7) & 4 != 0 && p.u8(actor + 7) & 4 == 0 {
                    let pending = p.u16(actor + 0x14) + pending_sent(&p, &ram, other);
                    let order: [u8; 6] = ram.bytes(GARBAGE_DATA, 6).try_into().unwrap();
                    t.drops.push((order, pending));
                }
                if let Some((pivot, sat)) = ram.pair(tag, true) {
                    let new_pair = p.pair(tag, false).is_none() && ram.pair(tag, false).is_some();
                    if new_pair {
                        let now = Field::from_words(&ram.field_words(i));
                        if let (Some(mut want), Some((c, r, rot, pc, sc))) = (t.start.take(), t.locked.take()) {
                            want.lock_pair(c, r, rot, pc, sc);
                            let steps = want.resolve();
                            for &(order, pending) in &t.drops {
                                want.drop_garbage(order, pending);
                                want.settle();
                            }
                            assert_eq!(
                                want, now,
                                "{} frame {frame} P{}: field after pair at col {c} row {r} rot {rot} \
                                 ({} chain steps, {} drops)",
                                path.display(), i + 1, steps.len(), t.drops.len()
                            );
                            checked += 1;
                            with_chains += usize::from(!steps.is_empty());
                            with_drops += usize::from(!t.drops.is_empty());
                        }
                        t.start = Some(now);
                        t.drops.clear();
                    }
                    // Position where it stopped being controlled: during landing the pivot
                    // keeps falling on its own if only the satellite was supported.
                    if ram.code(pivot) == support::PIVOT_FALLING {
                        let color = |a: u32| BeanColor::from_id(ram.u8(a + 8)).unwrap();
                        t.locked = Some((ram.u16(pivot + 0x1A) as i16, ram.u16(pivot + 0x1C) as i16, ram.u8(pivot + 0x2B), color(pivot), color(sat)));
                    }
                }
            }
            prev = Some(ram);
        }
    }
    eprintln!("{checked} pieces checked ({with_chains} with pops, {with_drops} with garbage drops)");
}
