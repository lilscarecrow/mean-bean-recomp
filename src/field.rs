//! The 6×14 well and what happens to it between pieces: a locked pair settling,
//! pops and chains, and garbage ("refugee") beans dropping in.
//!
//! Rows count from the top. Rows 0-1 are hidden (`pPuyos`, `pPlaceablePuyos`); only the
//! 12 visible rows (2-13, `pVisiblePuyos`) can pop.

use super::bean_order::BeanColor;

pub const COLS: usize = 6;
pub const ROWS: usize = 14;
pub const HIDDEN_ROWS: usize = 2;
/// Groups pop at this size.
pub const POP_SIZE: usize = 4;
/// Most garbage that falls in one drop (`SetGarbageCounts` caps at $1E).
pub const MAX_GARBAGE_DROP: u16 = 30;

pub type Cell = Option<BeanColor>;

/// Offsets for directions 0 up, 1 right, 2 down, 3 left (`word_5374`), as (col, row).
pub const DIR: [(i16, i16); 4] = [(0, -1), (1, 0), (0, 1), (-1, 0)];

#[derive(Clone, Default, PartialEq, Eq)]
pub struct Field {
    cells: [[Cell; COLS]; ROWS],
}

/// A same-colored group found by `find_pops`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub color: BeanColor,
    pub cells: Vec<(usize, usize)>, // (col, row)
}

impl Field {
    /// From the game's field words (14 rows × 6, top row first). A cell is occupied
    /// when bit 7 of its high byte is set; bits 4-6 are the color.
    pub fn from_words(words: &[u16]) -> Field {
        let mut f = Field::default();
        for (i, &w) in words.iter().take(COLS * ROWS).enumerate() {
            let hi = (w >> 8) as u8;
            if hi & 0x80 != 0 {
                f.cells[i / COLS][i % COLS] = BeanColor::from_id((hi >> 4) & 7);
            }
        }
        f
    }

    pub fn get(&self, col: i16, row: i16) -> Cell {
        if (0..COLS as i16).contains(&col) && (0..ROWS as i16).contains(&row) {
            self.cells[row as usize][col as usize]
        } else {
            None
        }
    }

    /// `CheckPuyoLand2`'s test: off the board or occupied.
    pub fn blocked(&self, col: i16, row: i16) -> bool {
        !(0..COLS as i16).contains(&col) || !(0..ROWS as i16).contains(&row) || self.get(col, row).is_some()
    }

    pub fn set(&mut self, col: usize, row: usize, cell: Cell) {
        self.cells[row][col] = cell;
    }

    /// Drops a bean in a column from `row` until it rests (`MarkPuyoSpot` after the fall).
    pub fn drop_bean(&mut self, col: usize, row: usize, color: BeanColor) {
        let mut r = row;
        while r + 1 < ROWS && self.cells[r + 1][col].is_none() {
            r += 1;
        }
        self.cells[r][col] = Some(color);
    }

    /// Locks a pair at its final pivot position: the lower bean settles first, then the
    /// upper one (relevant only when they share a column).
    pub fn lock_pair(&mut self, col: i16, row: i16, rot: u8, pivot: BeanColor, satellite: BeanColor) {
        let (dc, dr) = DIR[(rot & 3) as usize];
        let mut beans = [(col, row, pivot), (col + dc, row + dr, satellite)];
        beans.sort_by_key(|b| std::cmp::Reverse(b.1));
        for (c, r, color) in beans {
            self.drop_bean(c as usize, r.max(0) as usize, color);
        }
    }

    /// Connected same-colored groups of 4+ among the visible rows (`sub_5960`).
    pub fn find_pops(&self) -> Vec<Group> {
        let mut seen = [[false; COLS]; ROWS];
        let mut groups = Vec::new();
        for row in HIDDEN_ROWS..ROWS {
            for col in 0..COLS {
                let Some(color) = self.cells[row][col] else { continue };
                if color == BeanColor::Garbage || seen[row][col] {
                    continue;
                }
                let mut cells = vec![];
                let mut stack = vec![(col, row)];
                seen[row][col] = true;
                while let Some((c, r)) = stack.pop() {
                    cells.push((c, r));
                    for (dc, dr) in DIR {
                        let (nc, nr) = (c as i16 + dc, r as i16 + dr);
                        if nr < HIDDEN_ROWS as i16 || self.get(nc, nr) != Some(color) {
                            continue;
                        }
                        let (nc, nr) = (nc as usize, nr as usize);
                        if !seen[nr][nc] {
                            seen[nr][nc] = true;
                            stack.push((nc, nr));
                        }
                    }
                }
                if cells.len() >= POP_SIZE {
                    groups.push(Group { color, cells });
                }
            }
        }
        groups
    }

    /// Removes popped groups and any visible garbage touching them (`sub_58C8`).
    pub fn pop(&mut self, groups: &[Group]) {
        let mut clear = vec![];
        for &(c, r) in groups.iter().flat_map(|g| &g.cells) {
            clear.push((c, r));
            for (dc, dr) in DIR {
                let (nc, nr) = (c as i16 + dc, r as i16 + dr);
                if nr >= HIDDEN_ROWS as i16 && self.get(nc, nr) == Some(BeanColor::Garbage) {
                    clear.push((nc as usize, nr as usize));
                }
            }
        }
        for (c, r) in clear {
            self.cells[r][c] = None;
        }
    }

    /// Lets everything fall to rest.
    pub fn settle(&mut self) {
        for col in 0..COLS {
            let stack: Vec<BeanColor> = (0..ROWS).rev().filter_map(|r| self.cells[r][col]).collect();
            for r in 0..ROWS {
                self.cells[r][col] = None;
            }
            for (i, color) in stack.into_iter().enumerate() {
                self.cells[ROWS - 1 - i][col] = Some(color);
            }
        }
    }

    /// Pops and settles until nothing more pops. Returns each chain step's groups.
    pub fn resolve(&mut self) -> Vec<Vec<Group>> {
        let mut steps = vec![];
        loop {
            self.settle();
            let groups = self.find_pops();
            if groups.is_empty() {
                return steps;
            }
            self.pop(&groups);
            steps.push(groups);
        }
    }

    /// Empty cells per column over the 13 lowest rows (`GetGarbageFreeSpace`).
    pub fn garbage_room(&self) -> [u8; COLS] {
        std::array::from_fn(|c| (1..ROWS).filter(|&r| self.cells[r][c].is_none()).count() as u8)
    }

    /// `SetGarbageCounts`: deals `min(pending, 30)` garbage beans round-robin over the
    /// shuffled column `order`, skipping columns with no room, and drops them in.
    /// Returns how many were placed (the game subtracts that from pending).
    pub fn drop_garbage(&mut self, order: [u8; COLS], pending: u16) -> u16 {
        let room = self.garbage_room();
        let mut counts = [0u8; COLS];
        let mut placed = 0;
        for i in 0..pending.min(MAX_GARBAGE_DROP) as usize {
            let c = order[i % COLS] as usize;
            if counts[c] < room[c] {
                counts[c] += 1;
                placed += 1;
            }
        }
        for (c, &n) in counts.iter().enumerate() {
            for _ in 0..n {
                self.drop_bean(c, 0, BeanColor::Garbage);
            }
        }
        placed
    }
}

impl std::fmt::Debug for Field {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for (r, row) in self.cells.iter().enumerate() {
            let line: String = row
                .iter()
                .map(|c| match c {
                    None => '.',
                    Some(BeanColor::Red) => 'R',
                    Some(BeanColor::Yellow) => 'Y',
                    Some(BeanColor::Teal) => 'T',
                    Some(BeanColor::Green) => 'G',
                    Some(BeanColor::Purple) => 'P',
                    Some(BeanColor::Blue) => 'B',
                    Some(BeanColor::Garbage) => '#',
                })
                .collect();
            writeln!(f, "{}{line}", if r < HIDDEN_ROWS { "~" } else { " " })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use BeanColor::*;

    fn field(rows: &[&str]) -> Field {
        // rows are the bottom rows, listed top to bottom
        let mut f = Field::default();
        let top = ROWS - rows.len();
        for (i, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                let cell = match ch {
                    'R' => Some(Red),
                    'Y' => Some(Yellow),
                    'G' => Some(Green),
                    'P' => Some(Purple),
                    'B' => Some(Blue),
                    '#' => Some(Garbage),
                    _ => None,
                };
                f.set(c, top + i, cell);
            }
        }
        f
    }

    #[test]
    fn pair_locks_and_pops_with_garbage() {
        let mut f = field(&["#.....", "RRR..."]);
        f.lock_pair(3, 2, 0, Red, Yellow); // vertical: red pivot below, yellow above
        let steps = f.resolve();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0][0].cells.len(), 4);
        assert_eq!(f.get(0, 13), None, "garbage next to the pop is cleared");
        assert_eq!(f.get(3, 13), Some(Yellow));
    }

    #[test]
    fn hidden_rows_do_not_pop() {
        let mut f = Field::default();
        for c in 0..4 {
            f.set(c, 1, Some(Red));
        }
        assert!(f.find_pops().is_empty());
    }

    #[test]
    fn garbage_fills_round_robin_and_respects_room() {
        let mut f = Field::default();
        for r in 1..ROWS {
            f.set(5, r, Some(Blue)); // column 5 has no room
        }
        let placed = f.drop_garbage([5, 0, 1, 2, 3, 4], 8);
        assert_eq!(placed, 6);
        assert_eq!(f.garbage_room(), [11, 12, 12, 12, 12, 0]);
    }
}
