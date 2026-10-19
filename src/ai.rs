//! CPU opponent: tries every reachable placement of the current pair, then every placement
//! of the next pair on the resulting board, and keeps the move with the best two-step outlook.

use crate::board::{score, Bean, Board, Piece, Rng, SPAWN_X, VISIBLE_H, W};

/// Result of letting a board settle and pop until nothing more happens.
pub struct Outcome {
    #[allow(dead_code)] // read by tests
    pub chain: u32,
    pub score: u64,
    pub dead: bool,
}

pub fn resolve(board: &mut Board) -> Outcome {
    let (mut chain, mut total) = (0, 0);
    loop {
        while board.gravity_step() {}
        let groups = board.find_pops();
        if groups.is_empty() {
            break;
        }
        chain += 1;
        let info = board.clear(&groups);
        total += score(chain, &info);
    }
    Outcome { chain, score: total, dead: board.is_topped_out() }
}

/// Every final position a pair can be steered to: rotate at the spawn column, then slide
/// sideways along the spawn row while nothing blocks it.
pub fn placements(board: &Board, pair: (Bean, Bean)) -> Vec<Piece> {
    let mut out = Vec::new();
    for rot in 0..4u8 {
        // A same-colored pair looks identical upside down.
        if pair.0 == pair.1 && rot >= 2 {
            continue;
        }
        let spawn = Piece { rot, ..Piece::new(pair.0, pair.1) };
        if !spawn.fits(board) {
            continue;
        }
        for x in 0..W as i32 {
            let step = if x >= SPAWN_X { 1 } else { -1 };
            let reachable = (0..=(x - SPAWN_X).abs()).all(|i| Piece { x: SPAWN_X + i * step, ..spawn }.fits(board));
            if reachable {
                out.push(Piece { x, ..spawn });
            }
        }
    }
    out
}

/// Hard-drops `piece` onto a copy of `board`.
pub fn place(board: &Board, mut piece: Piece) -> Board {
    let mut b = board.clone();
    while piece.try_move(&b, 0, -1) {}
    piece.lock(&mut b);
    b
}

/// How promising a settled board looks: matching neighbors are good (future pops),
/// tall stacks are bad, and a filling spawn column is very bad.
fn shape(board: &Board) -> f32 {
    let mut v = 0.0;
    for x in 0..W {
        let h = board.column_height(x) as f32;
        v -= h * h * 0.6;
        if (x as i32 == SPAWN_X || x as i32 == SPAWN_X + 1) && h >= 9.0 {
            v -= 400.0;
        }
    }
    for y in 0..VISIBLE_H as i32 {
        for x in 0..W as i32 {
            let Some(bean) = board.get(x, y).filter(|&b| b != Bean::Garbage) else { continue };
            if board.get(x + 1, y) == Some(bean) {
                v += 8.0;
            }
            if board.get(x, y + 1) == Some(bean) {
                v += 8.0;
            }
        }
    }
    v
}

/// Picks where to put `current`, looking one piece ahead at `next`. A little noise from
/// `rng` breaks ties so the CPU doesn't play the exact same game every time.
pub fn best_move(board: &Board, current: (Bean, Bean), next: (Bean, Bean), rng: &mut Rng) -> Option<Piece> {
    let mut best: Option<(f32, Piece)> = None;
    for p in placements(board, current) {
        let mut b1 = place(board, p);
        let o1 = resolve(&mut b1);
        if o1.dead {
            continue;
        }
        let mut follow_up = -2000.0f32;
        for q in placements(&b1, next) {
            let mut b2 = place(&b1, q);
            let o2 = resolve(&mut b2);
            if !o2.dead {
                follow_up = follow_up.max(o2.score as f32 + shape(&b2));
            }
        }
        let value = o1.score as f32 + follow_up + rng.below(12) as f32;
        if best.as_ref().is_none_or(|(v, _)| value > *v) {
            best = Some((value, p));
        }
    }
    best.map(|(_, p)| p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Rng;

    #[test]
    fn takes_an_obvious_pop() {
        let mut b = Board::default();
        for x in 0..3 {
            b.set(x, 0, Some(Bean::Red));
        }
        // A red pair next to three reds: some placement must pop.
        let p = best_move(&b, (Bean::Red, Bean::Red), (Bean::Blue, Bean::Green), &mut Rng::new(1)).unwrap();
        let mut after = place(&b, p);
        assert!(resolve(&mut after).chain >= 1);
    }

    #[test]
    fn cpu_survives_a_long_solo_game() {
        let mut rng = Rng::new(12345);
        let mut board = Board::default();
        let mut queue: Vec<(Bean, Bean)> = (0..2).map(|_| rng.pair(4)).collect();
        let mut best_chain = 0;
        for turn in 0..150 {
            let current = queue.remove(0);
            queue.push(rng.pair(4));
            let p = best_move(&board, current, queue[0], &mut rng).unwrap_or_else(|| panic!("no move on turn {turn}"));
            board = place(&board, p);
            let o = resolve(&mut board);
            best_chain = best_chain.max(o.chain);
            assert!(!o.dead, "topped out on turn {turn}");
        }
        assert!(best_chain >= 2, "never made a chain");
    }
}
