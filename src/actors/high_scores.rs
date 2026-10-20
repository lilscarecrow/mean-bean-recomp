//! The high score table (`BC_HighScores_1`): both planes cleared, the backdrop
//! (`MapEni_HighScores`), then `loc_C66A` writes the table into plane A (the mode's
//! title, the five names, scores and beans) and slides it in from the right; it stays
//! $100 frames (a button ends it sooner) and hands back to the flow, which fades out to
//! the title screen.

use super::*;
use crate::rom;

/// `loc_C66A` (+$26 the slide's scroll, +$0A.. the entries' first name bytes, +$0F set
/// when one is a new entry still to be named, +$2A set once a button let the flow on).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighScoreTable {
    pub phase: TablePhase,
    /// +$07: a name entry on it is running.
    pub busy: bool,
    pub scroll: u16,
    pub delay: u16,
    pub unnamed: bool,
    pub released: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TablePhase {
    /// $C66A: draws the table.
    Draw,
    /// $C68E: slides in, 4 pixels a frame.
    Slide,
    /// $C70A: `ActorBookmark_Ctrl` for $100 frames.
    Wait,
    /// $C710: then hands back to the flow.
    Done,
}

impl TablePhase {
    pub fn code(self) -> u32 {
        match self {
            TablePhase::Draw => 0xC66A,
            TablePhase::Slide => 0xC68E,
            TablePhase::Wait => 0xC70A,
            TablePhase::Done => 0xC710,
        }
    }
}

/// `sub_C28A`: naming a new entry on the table (+$0A the entry, +$0E the letter being
/// chosen, +$12.. the letters, +$26 the blink count, +$28 a timer it doesn't use).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableNameEntry {
    pub phase: NamePhase,
    pub parent: usize,
    pub busy: bool,
    pub entry: u16,
    pub at: u16,
    pub letters: [u8; 5],
    pub blink: u8,
    pub anim: Anim,
    pub timer: u16,
    pub delay: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NamePhase {
    /// $C28A
    Start,
    /// $C2B0: the letter's flash in.
    Show,
    /// $C2C4: choosing.
    Choose,
    /// $C38C: the flash back to the previous letter.
    Back,
    /// $C3C8: $20 frames once the name is in.
    Wait,
    /// $C3CE
    Done,
}

impl NamePhase {
    pub fn code(self) -> u32 {
        match self {
            NamePhase::Start => 0xC28A,
            NamePhase::Show => 0xC2B0,
            NamePhase::Choose => 0xC2C4,
            NamePhase::Back => 0xC38C,
            NamePhase::Wait => 0xC3C8,
            NamePhase::Done => 0xC3CE,
        }
    }
}

/// `loc_C506`: a cursor bean under the letters still to choose (+$1E which).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TableCursor {
    pub parent: usize,
    pub index: u16,
    pub draw: u8,
    pub anim: Anim,
    pub x: u16,
    pub y: u16,
}

pub const TABLE_CURSOR_CODE: u32 = 0xC506;
/// `unk_C3DC` / `unk_C3E6`: a letter flashing in / out; `unk_C536`: the cursor beans.
const LETTER_IN: u32 = 0xC3DC;
const LETTER_OUT: u32 = 0xC3E6;
const CURSOR_SCRIPT: u32 = 0xC536;
/// The letters wrap at $1C (this revision; the later one stops at $1B going down).
const LETTER_WRAP: u8 = 0x1C;

/// `LoadDefaultHighScores`' table (both modes, 5 x 16 bytes each).
const DEFAULT_HIGH_SCORES: u32 = 0x354;
/// `MapEni_HighScores`; `off_C78E`: the modes' titles.
const HIGH_SCORES_MAP: u32 = 0x6EB68;
const TABLE_TITLES: u32 = 0xC78E;

impl Engine {
    /// `LoadDefaultHighScores` (at boot).
    pub fn load_default_high_scores(&mut self) {
        for (i, b) in self.high_scores[..0xA0].iter_mut().enumerate() {
            *b = rom::byte(DEFAULT_HIGH_SCORES + i as u32);
        }
    }

    pub(super) fn run_high_score_routine(&mut self, at: u32) -> bool {
        match at {
            // ClearPlaneA / ClearPlaneB
            0x23B70 => self.clear_vram(0xC000, 0x1000),
            0x23B9A => self.clear_vram(0xE000, 0x1000),
            // sub_F75C
            0xF75C => {
                self.eni_dec_safe(HIGH_SCORES_MAP, 0x6280, 0x27, 0x1B, 0xE000);
            }
            // loc_C54C
            0xC54C => {
                self.vscroll = [0; 40];
                self.hscroll = [0; 0x200];
                if let Some(slot) = self.find_actor_slot() {
                    let t = HighScoreTable { phase: TablePhase::Draw, busy: false, scroll: 0, delay: 0, unnamed: false, released: false };
                    self.slots[slot] = Some(Actor::HighScoreTable(t));
                }
            }
            _ => return false,
        }
        true
    }

    /// Words of zero straight into VRAM.
    pub(super) fn clear_vram(&mut self, at: u16, words: usize) {
        self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Words { at, words: vec![0; words] } });
    }

    pub(super) fn run_high_score_table(&mut self, slot: usize, mut t: HighScoreTable, inputs: &Inputs) -> Option<HighScoreTable> {
        let press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0;
        match t.phase {
            TablePhase::Draw => {
                t.scroll = 0x13C;
                self.table_scroll(0x140);
                self.draw_table_title();
                for i in (0..5).rev() {
                    let (entry, at) = self.high_score_entry(i);
                    if self.high_scores[entry] & 0x80 != 0 {
                        t.unnamed = true;
                    }
                    // sub_C7B2: the name up to its $FF.
                    let mut c = entry;
                    let mut to = at;
                    while self.high_scores[c] & 0x80 == 0 {
                        self.table_char(to, 0xC400, self.high_scores[c]);
                        to = to.wrapping_add(4);
                        c += 1;
                    }
                }
                for i in (0..5).rev() {
                    let (entry, at) = self.high_score_entry(i);
                    let score = u32::from_be_bytes(self.high_scores[entry + 6..entry + 10].try_into().unwrap());
                    self.table_digits(at.wrapping_add(0x14), &digits8(score));
                }
                for i in (0..5).rev() {
                    let (entry, at) = self.high_score_entry(i);
                    let beans = u16::from_be_bytes([self.high_scores[entry + 10], self.high_scores[entry + 11]]);
                    self.table_digits(at.wrapping_add(0x30), &digits8(u32::from(beans))[3..]);
                }
                t.phase = TablePhase::Slide;
            }
            TablePhase::Slide => {
                self.table_scroll(t.scroll);
                let (left, borrow) = t.scroll.overflowing_sub(4);
                t.scroll = left;
                if borrow {
                    // loc_C6D0: an entry still to be named gets its name entry here.
                    if self.bytecode_disabled {
                        for i in (0..5).rev() {
                            let (entry, _) = self.high_score_entry(i);
                            if self.high_scores[entry] & 0x80 != 0 {
                                t.busy = true;
                                self.spawn_table_name_entry(slot, i);
                            }
                        }
                    }
                    t.delay = 0x100;
                    t.phase = TablePhase::Wait;
                } else if !t.unnamed && press != 0 && !t.released {
                    self.bytecode_disabled = false;
                    t.released = true;
                }
            }
            TablePhase::Wait => {
                // ActorBookmark_Ctrl
                if press == 0 && t.delay != 0 {
                    t.delay -= 1;
                    if t.delay != 0 {
                        return Some(t);
                    }
                }
                t.delay = 0;
                t.phase = TablePhase::Done;
            }
            TablePhase::Done => {
                if t.busy {
                    return Some(t);
                }
                // loc_C710 / loc_C71A: the second table (byte_FF1973) isn't shown here.
                if self.high_score_both {
                    self.unported_note = Some("the second high score table (byte_FF1973)");
                }
                if !t.released {
                    self.bytecode_disabled = false;
                }
                return None;
            }
        }
        Some(t)
    }

    /// `sub_C24C`.
    fn spawn_table_name_entry(&mut self, parent: usize, entry: u16) {
        if let Some(slot) = self.find_actor_slot() {
            let n = TableNameEntry {
                phase: NamePhase::Start,
                parent,
                busy: false,
                entry,
                at: 0,
                letters: [0; 5],
                blink: 0,
                anim: Anim { ptr: 0, time: 0, frame: 0 },
                timer: 0x780,
                delay: 0,
            };
            self.slots[slot] = Some(Actor::TableNameEntry(n));
        }
    }

    pub(super) fn run_table_name_entry(&mut self, slot: usize, mut n: TableNameEntry, inputs: &Inputs) -> Option<TableNameEntry> {
        match n.phase {
            NamePhase::Start => {
                n.busy = true;
                // sub_C4BA: two cursor beans.
                let y = n.entry * 0x18 + 0xD8;
                for (index, x) in [(1u16, 0xC8u16), (2, 0xD8)] {
                    if let Some(s) = self.find_actor_slot() {
                        let c = TableCursor { parent: slot, index, draw: 0, anim: Anim { ptr: CURSOR_SCRIPT, time: 0, frame: 0 }, x, y };
                        self.slots[s] = Some(Actor::TableCursor(c));
                    }
                }
                n.letters = [1; 5];
                self.name_letter_in(&mut n);
            }
            NamePhase::Show => {
                if n.anim.step_with(&mut || self.random()) {
                    n.phase = NamePhase::Choose;
                } else {
                    
                    self.draw_name_letter(&n);
                }
            }
            NamePhase::Choose => {
                n.blink = n.blink.wrapping_add(1);
                let (p, q) = (inputs.pads[0], inputs.pads[1]);
                let press = p.press | q.press;
                let i = usize::from(n.at);
                if press & 0x04 != 0 {
                    if n.at != 0 {
                        n.anim = Anim { ptr: LETTER_OUT, time: 0, frame: n.anim.frame };
                        n.phase = NamePhase::Back;
                    }
                } else if press & 0x70 != 0 {
                    n.anim.frame = 0x80;
                    self.draw_name_letter(&n);
                    n.at += 1;
                    if n.at >= 3 {
                        // loc_C3AA: the name into the table.
                        let (entry, _) = self.high_score_entry(n.entry);
                        self.high_scores[entry..entry + 3].copy_from_slice(&n.letters[..3]);
                        self.high_scores[entry + 3] = 0xFF;
                        n.busy = false;
                        n.timer = 0;
                        n.delay = 0x20;
                        n.phase = NamePhase::Wait;
                    } else {
                        self.name_letter_in(&mut n);
                    }
                } else {
                    let held = p.repeat | q.repeat;
                    if held & 0x02 != 0 {
                        n.letters[i] += 1;
                        if n.letters[i] >= LETTER_WRAP {
                            n.letters[i] = 0;
                        }
                        n.blink = 0;
                    } else if held & 0x01 != 0 {
                        n.letters[i] = n.letters[i].wrapping_sub(1);
                        if n.letters[i] & 0x80 != 0 {
                            n.letters[i] = LETTER_WRAP;
                        }
                        n.blink = 0;
                    } else {
                        n.anim.frame = ((n.blink << 1) & 0x20) | 0x80;
                        self.draw_name_letter(&n);
                    }
                }
            }
            NamePhase::Back => {
                if n.anim.step_with(&mut || self.random()) {
                    n.at -= 1;
                    self.name_letter_in(&mut n);
                } else {
                    
                    self.draw_name_letter(&n);
                }
            }
            NamePhase::Wait => {
                n.delay -= 1;
                if n.delay == 0 {
                    n.phase = NamePhase::Done;
                }
            }
            NamePhase::Done => {
                if let Some(Actor::HighScoreTable(t)) = self.slots[n.parent].as_mut() {
                    t.busy = false;
                }
                return None;
            }
        }
        Some(n)
    }

    /// $C2A2.
    fn name_letter_in(&mut self, n: &mut TableNameEntry) {
        n.anim.ptr = LETTER_IN;
        n.phase = NamePhase::Show;
    }

    /// `sub_C3F0`: the letter being chosen (frame bit 7 clear: that character instead;
    /// set: the letter plus the frame's low bits).
    fn draw_name_letter(&mut self, n: &TableNameEntry) {
        let (_, at) = self.high_score_entry(n.entry);
        let at = at.wrapping_add(n.at * 4);
        let letter = n.letters[usize::from(n.at)];
        let f = n.anim.frame;
        let c = if f & 0x80 == 0 { f } else { letter.wrapping_add(f & 0x7F) };
        self.table_char(at, 0xC400, c);
    }

    pub(super) fn run_table_cursor(&mut self, mut c: TableCursor) -> Option<TableCursor> {
        let at = match &self.slots[c.parent] {
            Some(Actor::TableNameEntry(n)) if n.busy => n.at,
            _ => return None,
        };
        c.draw = if at >= c.index { 0 } else { 0x80 };
        c.anim.step_with(&mut || self.random());
        Some(c)
    }

    /// `sub_C778`: plane A's lines $28-$97 scrolled to `x`.
    fn table_scroll(&mut self, x: u16) {
        for i in 0..0x70 {
            self.hscroll[0xA0 + i * 2] = x;
            self.hscroll[0xA1 + i * 2] = 0;
        }
    }

    /// `GetHighScoreEntry`: the entry's offset in `high_scores` and where its line is drawn.
    fn high_score_entry(&self, i: u16) -> (usize, u16) {
        let entry = usize::from(self.high_score_table) * 0x50 + usize::from(i) * 0x10;
        (entry, 0xCA0C + i * 0x300)
    }

    /// `sub_C7CA`: the mode's title.
    fn draw_table_title(&mut self) {
        let mut at = rom::long(TABLE_TITLES + u32::from(self.high_score_table) * 4);
        let mut to = 0xD90E_u16;
        loop {
            let c = rom::byte(at);
            if c & 0x80 != 0 {
                break;
            }
            self.table_char(to, 0xA400, c);
            to = to.wrapping_add(4);
            at += 1;
        }
    }

    /// `sub_C800`: a 2x2-tile character.
    fn table_char(&mut self, at: u16, base: u16, c: u8) {
        let tile = (base & 0xFF00) | u16::from(((c & 0x0F) << 1) | ((c & 0x30) << 2));
        self.plane_writes.push((at, tile));
        self.plane_writes.push((at.wrapping_add(2), tile + 1));
        self.plane_writes.push((at.wrapping_add(0x100), tile + 0x20));
        self.plane_writes.push((at.wrapping_add(0x102), tile + 0x21));
    }

    /// `loc_C5EE`: digits a column of two tiles each, leading zeros blank.
    fn table_digits(&mut self, at: u16, digits: &[u8]) {
        let mut shown = 0u8;
        for (i, &d) in digits.iter().enumerate() {
            if d != 0 {
                shown = 1;
            }
            let t = d.wrapping_add(shown) << 1;
            let to = at.wrapping_add(i as u16 * 2);
            self.plane_writes.push((to, 0xA500 | u16::from(t)));
            self.plane_writes.push((to.wrapping_add(0x100), 0xA500 | u16::from(t.wrapping_add(1))));
        }
    }
}

/// `sub_1B350`: eight decimal digits (the top four from the quotient by 10000).
fn digits8(n: u32) -> [u8; 8] {
    let (hi, lo) = ((n / 10000) as u16, (n % 10000) as u16);
    let mut out = [0u8; 8];
    for (k, part) in [hi, lo].into_iter().enumerate() {
        let mut v = part;
        for i in (0..4).rev() {
            out[k * 4 + i] = (v % 10) as u8;
            v /= 10;
        }
    }
    out
}
