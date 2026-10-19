//! Pure game rules: the 6x12 well, falling pairs, gravity, pops and scoring.
//! No Bevy in here, so it can be unit-tested and reused for a CPU opponent / versus mode.

pub const W: usize = 6;
/// Rows 0..12 are visible; row 12 is the hidden spawn row (beans there never pop).
pub const H: usize = 13;
pub const VISIBLE_H: usize = 12;
pub const SPAWN_X: i32 = 2;
pub const SPAWN_Y: i32 = 11;
pub const POP_SIZE: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bean {
    Red,
    Green,
    Blue,
    Yellow,
    Purple,
    /// "Refugee" / nuisance bean: never pops by itself, cleared when a neighbor pops.
    Garbage,
}

impl Bean {
    pub const COLORS: [Bean; 5] = [Bean::Red, Bean::Green, Bean::Blue, Bean::Yellow, Bean::Purple];
    pub const ALL: [Bean; 6] = [Bean::Red, Bean::Green, Bean::Blue, Bean::Yellow, Bean::Purple, Bean::Garbage];

    pub fn index(self) -> usize {
        self as usize
    }
}

#[derive(Clone, Default)]
pub struct Board {
    cells: [[Option<Bean>; W]; H],
}

/// What a single pop step cleared, for scoring.
#[derive(Default, Debug, PartialEq)]
pub struct ClearInfo {
    pub beans: usize,
    pub group_sizes: Vec<usize>,
    pub colors: usize,
    pub garbage: usize,
}

fn in_bounds(x: i32, y: i32) -> bool {
    x >= 0 && y >= 0 && (x as usize) < W && (y as usize) < H
}

const DIRS: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];

impl Board {
    pub fn get(&self, x: i32, y: i32) -> Option<Bean> {
        if in_bounds(x, y) { self.cells[y as usize][x as usize] } else { None }
    }

    pub fn set(&mut self, x: i32, y: i32, bean: Option<Bean>) {
        if in_bounds(x, y) {
            self.cells[y as usize][x as usize] = bean;
        }
    }

    pub fn is_free(&self, x: i32, y: i32) -> bool {
        in_bounds(x, y) && self.cells[y as usize][x as usize].is_none()
    }

    /// Drops every unsupported bean by one row. Returns whether anything moved,
    /// so the caller can animate settling one row per tick.
    pub fn gravity_step(&mut self) -> bool {
        let mut moved = false;
        for y in 1..H {
            for x in 0..W {
                if self.cells[y][x].is_some() && self.cells[y - 1][x].is_none() {
                    self.cells[y - 1][x] = self.cells[y][x].take();
                    moved = true;
                }
            }
        }
        moved
    }

    /// Connected same-color groups of at least POP_SIZE in the visible rows.
    pub fn find_pops(&self) -> Vec<Vec<(i32, i32)>> {
        let mut seen = [[false; W]; H];
        let mut groups = Vec::new();
        for y in 0..VISIBLE_H {
            for x in 0..W {
                let Some(color) = self.cells[y][x] else { continue };
                if color == Bean::Garbage || seen[y][x] {
                    continue;
                }
                let mut group = Vec::new();
                let mut stack = vec![(x as i32, y as i32)];
                seen[y][x] = true;
                while let Some((cx, cy)) = stack.pop() {
                    group.push((cx, cy));
                    for (dx, dy) in DIRS {
                        let (nx, ny) = (cx + dx, cy + dy);
                        if in_bounds(nx, ny)
                            && (ny as usize) < VISIBLE_H
                            && !seen[ny as usize][nx as usize]
                            && self.get(nx, ny) == Some(color)
                        {
                            seen[ny as usize][nx as usize] = true;
                            stack.push((nx, ny));
                        }
                    }
                }
                if group.len() >= POP_SIZE {
                    groups.push(group);
                }
            }
        }
        groups
    }

    /// Removes the popped groups plus any garbage touching them.
    pub fn clear(&mut self, groups: &[Vec<(i32, i32)>]) -> ClearInfo {
        let mut info = ClearInfo::default();
        let mut colors: Vec<Bean> = Vec::new();
        for group in groups {
            info.group_sizes.push(group.len());
            if let Some(c) = self.get(group[0].0, group[0].1) {
                if !colors.contains(&c) {
                    colors.push(c);
                }
            }
        }
        for &(x, y) in groups.iter().flatten() {
            for (dx, dy) in DIRS {
                let (nx, ny) = (x + dx, y + dy);
                if (ny as usize) < VISIBLE_H && self.get(nx, ny) == Some(Bean::Garbage) {
                    self.set(nx, ny, None);
                    info.garbage += 1;
                }
            }
        }
        for &(x, y) in groups.iter().flatten() {
            if self.get(x, y).is_some() {
                self.set(x, y, None);
                info.beans += 1;
            }
        }
        info.colors = colors.len();
        info
    }

    /// Classic rule: you lose when the spawn cell (3rd column, top visible row) is filled.
    pub fn is_topped_out(&self) -> bool {
        self.get(SPAWN_X, SPAWN_Y).is_some()
    }

    pub fn column_height(&self, x: usize) -> usize {
        (0..H).take_while(|&y| self.cells[y][x].is_some()).count()
    }

    /// Hangs `count` garbage beans from the top of the well: full rows of 6, plus the
    /// remainder in distinct random columns. They are left floating so `gravity_step`
    /// can animate the fall. Beans that don't fit are lost. Returns how many were placed.
    pub fn drop_garbage(&mut self, count: usize, rng: &mut Rng) -> usize {
        let (rows, extra) = (count / W, count % W);
        let mut cols: Vec<usize> = (0..W).collect();
        for i in 0..extra {
            let j = i + rng.below(W - i);
            cols.swap(i, j);
        }
        let mut placed = 0;
        for x in 0..W {
            let mut n = rows + usize::from(cols[..extra].contains(&x));
            let mut y = H as i32 - 1;
            while n > 0 && self.is_free(x as i32, y) {
                self.set(x as i32, y, Some(Bean::Garbage));
                y -= 1;
                n -= 1;
                placed += 1;
            }
        }
        placed
    }
}

/// Puyo Puyo Tsu-style score for one step of a chain (`chain` starts at 1).
pub fn score(chain: u32, info: &ClearInfo) -> u64 {
    const CHAIN_POWER: [u32; 19] = [0, 8, 16, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448, 480, 512];
    const COLOR_BONUS: [u32; 6] = [0, 0, 3, 6, 12, 24];
    let chain_power = CHAIN_POWER[(chain.saturating_sub(1) as usize).min(CHAIN_POWER.len() - 1)];
    let color_bonus = COLOR_BONUS[info.colors.min(5)];
    let group_bonus: u32 = info
        .group_sizes
        .iter()
        .map(|&n| match n {
            0..=4 => 0,
            5..=10 => n as u32 - 3,
            _ => 10,
        })
        .sum();
    let bonus = (chain_power + color_bonus + group_bonus).clamp(1, 999);
    10 * info.beans as u64 * bonus as u64
}

/// A falling pair. `rot`: 0 = satellite above the pivot, 1 = right, 2 = below, 3 = left.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub x: i32,
    pub y: i32,
    pub rot: u8,
    pub pivot: Bean,
    pub satellite: Bean,
}

fn rot_offset(rot: u8) -> (i32, i32) {
    match rot % 4 {
        0 => (0, 1),
        1 => (1, 0),
        2 => (0, -1),
        _ => (-1, 0),
    }
}

impl Piece {
    pub fn new(pivot: Bean, satellite: Bean) -> Self {
        Piece { x: SPAWN_X, y: SPAWN_Y, rot: 0, pivot, satellite }
    }

    pub fn satellite_pos(&self) -> (i32, i32) {
        let (dx, dy) = rot_offset(self.rot);
        (self.x + dx, self.y + dy)
    }

    pub fn cells(&self) -> [(i32, i32, Bean); 2] {
        let (sx, sy) = self.satellite_pos();
        [(self.x, self.y, self.pivot), (sx, sy, self.satellite)]
    }

    pub fn fits(&self, board: &Board) -> bool {
        self.cells().iter().all(|&(x, y, _)| board.is_free(x, y))
    }

    pub fn try_move(&mut self, board: &Board, dx: i32, dy: i32) -> bool {
        let moved = Piece { x: self.x + dx, y: self.y + dy, ..*self };
        if moved.fits(board) {
            *self = moved;
            true
        } else {
            false
        }
    }

    /// Rotates by `dir` (+1 clockwise, -1 counter-clockwise) with wall/floor kicks.
    /// If a 90° turn is impossible (e.g. in a 1-wide well), flips 180° instead.
    pub fn try_rotate(&mut self, board: &Board, dir: i8) -> bool {
        let rot = (self.rot as i8 + dir).rem_euclid(4) as u8;
        let flip = (self.rot + 2) % 4;
        for r in [rot, flip] {
            let turned = Piece { rot: r, ..*self };
            let (ox, oy) = rot_offset(r);
            // Kick the pivot away from whatever the satellite would hit.
            for candidate in [turned, Piece { x: turned.x - ox, y: turned.y - oy, ..turned }] {
                if candidate.fits(board) {
                    *self = candidate;
                    return true;
                }
            }
        }
        false
    }

    /// Writes the pair into the board. Beans then settle via `gravity_step`.
    pub fn lock(&self, board: &mut Board) {
        for (x, y, bean) in self.cells() {
            board.set(x, y, Some(bean));
        }
    }
}

/// Small xorshift RNG so the rules don't depend on a crate's API.
#[derive(Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Rng(seed.max(1))
    }

    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    pub fn pair(&mut self, colors: usize) -> (Bean, Bean) {
        (Bean::COLORS[self.below(colors)], Bean::COLORS[self.below(colors)])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Bean::*;

    fn board_from(rows: &[&str]) -> Board {
        // rows[0] is the bottom row; chars: R G B Y P, # garbage, . empty
        let mut b = Board::default();
        for (y, row) in rows.iter().enumerate() {
            for (x, ch) in row.chars().enumerate() {
                let bean = match ch {
                    'R' => Some(Red),
                    'G' => Some(Green),
                    'B' => Some(Blue),
                    'Y' => Some(Yellow),
                    'P' => Some(Purple),
                    '#' => Some(Garbage),
                    _ => None,
                };
                b.set(x as i32, y as i32, bean);
            }
        }
        b
    }

    #[test]
    fn four_in_a_row_pops() {
        let b = board_from(&["RRRR.."]);
        let pops = b.find_pops();
        assert_eq!(pops.len(), 1);
        assert_eq!(pops[0].len(), 4);
    }

    #[test]
    fn three_does_not_pop() {
        assert!(board_from(&["RRR...", "GGG..."]).find_pops().is_empty());
    }

    #[test]
    fn l_shape_pops_and_clears_adjacent_garbage() {
        let mut b = board_from(&["RRR#..", "R....."]);
        let pops = b.find_pops();
        assert_eq!(pops.len(), 1);
        let info = b.clear(&pops);
        assert_eq!(info.beans, 4);
        assert_eq!(info.garbage, 1);
        assert!(b.get(3, 0).is_none());
    }

    #[test]
    fn gravity_settles_floating_beans() {
        // Rows are listed bottom-up: green floats at y=2 with red on top of it.
        let mut b = board_from(&["......", "......", "G.....", "R....."]);
        while b.gravity_step() {}
        assert_eq!(b.get(0, 0), Some(Green));
        assert_eq!(b.get(0, 1), Some(Red));
        assert_eq!(b.get(0, 2), None);
    }

    #[test]
    fn two_chain() {
        // Four reds in column 3 (+ one at x=4) pop; the blue above them drops next to three blues.
        let mut b = board_from(&["BBBR..", "...RR.", "...R..", "...B.."]);
        let mut chain = 0;
        loop {
            while b.gravity_step() {}
            let pops = b.find_pops();
            if pops.is_empty() {
                break;
            }
            chain += 1;
            b.clear(&pops);
        }
        assert_eq!(chain, 2);
        assert!(b.get(0, 0).is_none());
    }

    #[test]
    fn hidden_row_never_pops() {
        let mut b = Board::default();
        for x in 0..4 {
            b.set(x, 12, Some(Red));
        }
        assert!(b.find_pops().is_empty());
    }

    #[test]
    fn rotation_kicks_off_wall() {
        let b = Board::default();
        let mut p = Piece { x: 5, y: 5, rot: 0, pivot: Red, satellite: Blue };
        assert!(p.try_rotate(&b, 1)); // satellite would go to x=6 -> kick left
        assert_eq!((p.x, p.rot), (4, 1));
    }

    #[test]
    fn rotation_flips_in_narrow_well() {
        let mut b = Board::default();
        for y in 0..8 {
            b.set(1, y, Some(Green));
            b.set(3, y, Some(Green));
        }
        let mut p = Piece { x: 2, y: 3, rot: 0, pivot: Red, satellite: Blue };
        assert!(p.try_rotate(&b, 1));
        assert_eq!(p.rot, 2);
        assert!(p.fits(&b));
    }

    #[test]
    fn garbage_drops_rows_plus_remainder() {
        let mut b = board_from(&["RRGGBB"]);
        let mut rng = Rng::new(7);
        assert_eq!(b.drop_garbage(8, &mut rng), 8);
        while b.gravity_step() {}
        let heights: Vec<usize> = (0..W).map(|x| b.column_height(x)).collect();
        assert_eq!(heights.iter().sum::<usize>(), 6 + 8);
        assert!(heights.iter().all(|&h| h == 2 || h == 3));
        assert_eq!(heights.iter().filter(|&&h| h == 3).count(), 2);
    }

    #[test]
    fn chain_score_grows() {
        let info = ClearInfo { beans: 4, group_sizes: vec![4], colors: 1, garbage: 0 };
        assert_eq!(score(1, &info), 40);
        assert_eq!(score(2, &info), 320);
    }
}
