//! Exercise mode (`BC_ExerciseMode`, level_mode 2): one player against the clock of
//! rising speeds, from level 1, 3 or 5 (`sub_3F94`'s selector in steps of two). The
//! pairs are random (`sub_43EA`), now and then a special piece; each level brings a
//! bonus and a faster fall, popping beans scores by the level. The side that didn't
//! press START shows PRESS 2P START (`loc_813C`) and may join; when both sides have
//! sat out their time (`byte_FF1965`), the high score table follows.

use super::*;
use crate::rom;

/// `unk_444A` (colors by level, from 1) and `unk_4450` (the colors).
const COLOR_COUNTS: u32 = 0x444A;
const COLORS: u32 = 0x4450;
/// `unk_9DB6` (per level, the speed steps' row) and `unk_9E7C` (rates, thresholds).
const SPEED_ROWS: u32 = 0x9DB6;
const SPEEDS: u32 = 0x9E7C;
/// `dword_9C32`: per level picked (1, 3, 5): a score bonus, the holder's special-piece
/// threshold (+$20) and color (+$07).
const LEVEL_START: u32 = 0x9C32;
/// `sub_9502`'s counter: where each side's level is shown ($A, $E).
const LEVEL_AT: [u16; 2] = [0xC620, 0xC62C];
const SIDE_BASE: [u16; 2] = [0x8500, 0xA500];
/// `SpecPlane93`: where each side's bean count is.
const BEANS_AT: [u16; 2] = [0xC71E, 0xC72A];
/// Text entries: the exercise panel ($0F), the join prompt ($17).
const PANEL_TEXT: u8 = 0x0F;
const JOIN_TEXT: u8 = 0x17;
const PROMPT_TEXT: u8 = 0x0A;
/// `loc_813C`: frames a side waits before it's out.
const JOIN_TIME: u16 = 0x1C0;

/// The exercise mode's player after its game (`loc_80EE`..).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExState {
    /// $8102: GAME OVER for $C0 frames; $8108: the high score; $8120: its name.
    Over,
    Score,
    Name,
    /// $8146: PRESS 2P START (or out); $81D0: joining; $820C: the end.
    Waiting,
    Join,
    Leave,
}

impl ExState {
    pub const ALL: [ExState; 6] = [ExState::Over, ExState::Score, ExState::Name, ExState::Waiting, ExState::Join, ExState::Leave];

    pub fn code(self) -> u32 {
        match self {
            ExState::Over => 0x8102,
            ExState::Score => 0x8108,
            ExState::Name => 0x8120,
            ExState::Waiting => 0x8146,
            ExState::Join => 0x81D0,
            ExState::Leave => 0x820C,
        }
    }
}

/// `sub_9572`: a side's level, shown (blinking while +$26 counts down) by `sub_9502`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LevelCounter {
    pub side: u8,
    /// +$26 frames left, +$17 / +$0F the tens and ones tiles.
    pub count: u16,
    pub tens: u8,
    pub ones: u8,
}

pub const LEVEL_COUNTER_CODE: u32 = 0x9572;

impl Engine {
    /// `loc_3C00`'s extras: the panel's text and the level's frame (`sub_9376`).
    pub(super) fn start_exercise(&mut self) {
        self.ex_out = 0;
        self.queue_panel(PanelCommand::Text { entry: PANEL_TEXT, blink: 0, player: 0 });
        for (at, words) in [(0xC726u16, [0xC1FC, 0xC1FE]), (0xC7A6, [0xC1FD, 0xC1FF])] {
            self.plane_writes.push((at, words[0]));
            self.plane_writes.push((at + 2, words[1]));
        }
    }

    /// `sub_43EA` (exercise): two random colors of the level's (the second first), or
    /// a special piece once the holder has dealt past its threshold (`loc_4456`).
    pub(super) fn exercise_pair(&mut self, h: &mut PairHolder, level: u8) -> (u8, u8) {
        let n = u16::from(rom::byte(COLOR_COUNTS + u32::from(level.min(4))));
        let first = rom::byte(COLORS + u32::from(self.random_below(n)));
        let second = rom::byte(COLORS + u32::from(self.random_below(n)));
        if h.w20 < h.dealt {
            h.dealt = 0;
            h.w20 = h.w20.checked_add(0x0C).unwrap_or(0xFFFF);
            let special = h.b07.wrapping_add(0x19);
            return (special, special);
        }
        (second, first)
    }

    /// `loc_3D44` (exercise): the field, the score, the level and bean counts, then the
    /// level select.
    pub(super) fn set_up_exercise(&mut self, p: &mut Player) {
        let id = usize::from(p.id & 1);
        self.popping[id] = false;
        self.fields[id] = Field::default(); // ResetPuyoField
        self.reset_field_bytes(id);
        add_points(p, 0); // sub_9C4A
        self.queue_panel(PanelCommand::Score(p.id));
        self.show_level(p, 0); // sub_9502
        self.queue_panel(PanelCommand::Beans { player: p.id, count: p.beans_total }); // sub_94E0
        p.flags |= 1;
        self.spawn_level_select(self.current_slot, p.id);
        p.state = PlayerState::WaitReady;
    }

    /// `sub_9502`: show the level (+1) for `frames` frames, blinking.
    pub(super) fn show_level(&mut self, p: &Player, frames: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        let n = u16::from(p.speed_level) + 1;
        let tens = if n / 10 == 0 { 0 } else { ((n / 10 + 1) * 2) as u8 };
        let ones = ((n % 10 + 1) * 2) as u8;
        self.slots[slot] = Some(Actor::LevelCounter(LevelCounter { side: p.id & 1, count: frames, tens, ones }));
    }

    pub(super) fn run_level_counter(&mut self, mut c: LevelCounter) -> Option<LevelCounter> {
        let side = usize::from(c.side);
        // (blank while +$27 has bit 2, or at 0)
        let chars = if c.count == 0 || c.count & 4 != 0 { [0, 0] } else { [c.tens, c.ones] };
        let mut at = LEVEL_AT[side];
        for ch in chars {
            self.plane_writes.push((at, SIDE_BASE[side] | u16::from(ch)));
            self.plane_writes.push((at.wrapping_add(0x80), SIDE_BASE[side] | u16::from(ch.wrapping_add(1))));
            at = at.wrapping_add(2);
        }
        c.count = c.count.wrapping_sub(1);
        if c.count == 0 || c.count == 0xFFFF {
            return None;
        }
        Some(c)
    }

    /// `loc_9D78`: the fall rate and threshold for the speed step and the level.
    pub(super) fn exercise_fall_rate(&mut self, p: &mut Player) {
        let row = rom::byte(SPEED_ROWS + u32::from(p.speed_level.min(0x0C)));
        let at = SPEEDS + u32::from((p.speed_index & 7) | row) * 4;
        p.fall_speed = rom::word(at);
        p.threshold = rom::word(at + 2);
    }

    /// `loc_9C0E`: the level's start: a score bonus and the holder's special pieces.
    pub(super) fn exercise_start_level(&mut self, p: &mut Player) {
        let at = LEVEL_START + u32::from((p.speed_level << 2) & 0x38);
        if let Some(Actor::PairHolder(h)) = p.holder_slot.and_then(|s| self.slots[s].as_mut()) {
            h.w20 = rom::word(at + 4);
            h.b07 = rom::byte(at + 6);
        }
        add_points(p, rom::long(at)); // sub_9C4A
        self.queue_panel(PanelCommand::Score(p.id));
    }

    /// `sub_9332`: clearing the field (with beans popped) is worth a bonus.
    pub(super) fn all_clear_bonus(&mut self, p: &mut Player) {
        let count = super::super::ai::off::COUNT;
        let bytes = &self.ai_fields[usize::from(p.id & 1)].bytes;
        if p.beans_total == 0 || bytes[count] != 0 || bytes[count + 1] != 0 {
            return;
        }
        self.play_sample(0x4C);
        add_points(p, (u32::from(p.speed_level) + 1) * 0x1F4 + 0x1F40);
        self.queue_panel(PanelCommand::Score(p.id));
    }

    /// `sub_88A8` (level 1): when the field gets crowded, the pair after next becomes a
    /// special piece ($19 or $1A, first at random, then the other; twice at most).
    pub(super) fn exercise_specials_due(&mut self, p: &Player) {
        if self.level_mode != 2 || p.speed_level != 0 {
            return;
        }
        let Some(slot) = p.holder_slot else { return };
        let Some(Actor::PairHolder(h)) = &self.slots[slot] else { return };
        if h.b07 & 0x80 != 0 || h.dealt < 2 {
            return;
        }
        let ai = &self.ai_fields[usize::from(p.id & 1)].bytes;
        let (count, free) = (super::super::ai::off::COUNT, super::super::ai::off::UNK3);
        let beans = u16::from_be_bytes([ai[count], ai[count + 1]]);
        let low = (0..6).filter(|c| ai[free + c * 2] < 3).count();
        if beans < 0x36 && low < 2 {
            return;
        }
        let first = (h.b07 == 0).then(|| (self.random() & 1) as u8);
        let Some(Actor::PairHolder(h)) = &mut self.slots[slot] else { return };
        if let Some(r) = first {
            h.b07 = r;
        }
        h.b07 = h.b07.wrapping_add(0x41);
        let special = if h.b07 & 1 != 0 { 0x1A } else { 0x19 };
        h.pairs[2] = special;
        h.pairs[3] = special;
        h.dealt = 0;
    }

    /// `loc_7CAC`: the player's field falls away; its game is over.
    pub(super) fn exercise_topped_out(&mut self, p: &mut Player) {
        let id = usize::from(p.id & 1);
        self.popping[id] = true;
        self.player_flags[id] = 0;
        self.play_sample(0x59);
        self.wipe_field(p);
        p.state = PlayerState::Wipe;
    }

    /// `loc_80EE` (from the cleared field): GAME OVER.
    pub(super) fn exercise_over(&mut self, p: &mut Player) {
        p.flags = 1;
        self.spawn_continue_sign(p); // sub_8298
        p.delay = 0xC0;
        p.state = PlayerState::Ex(ExState::Over);
    }

    /// `loc_813C`: this side waits for START.
    pub(super) fn exercise_wait(&mut self, p: &mut Player) {
        p.w26 = JOIN_TIME;
        p.state = PlayerState::Ex(ExState::Waiting);
    }

    pub(super) fn run_exercise_player(&mut self, p: &mut Player, state: ExState, inputs: &Inputs) -> bool {
        let id = usize::from(p.id & 1);
        match state {
            ExState::Over => {
                if wait(&mut p.delay) {
                    p.state = PlayerState::Ex(ExState::Score);
                }
            }
            ExState::Score => {
                self.high_score_table = 1;
                if let Some(rank) = self.enter_high_score(p) {
                    self.spawn_name_entry(p, rank);
                }
                p.state = PlayerState::Ex(ExState::Name);
            }
            ExState::Name => {
                if p.flags & 2 == 0 {
                    self.fields[id] = Field::default();
                    self.reset_field_bytes(id);
                    self.redraw_field(id);
                    self.exercise_wait(p);
                }
            }
            ExState::Waiting => {
                if self.ex_out ^ 3 == 0 {
                    // loc_81FC
                    self.bytecode_disabled = false;
                    p.flags &= !1;
                    p.state = PlayerState::Ex(ExState::Leave);
                    return true;
                }
                let press = inputs.pads[usize::from((p.id ^ self.swap_controls) & 1)].press;
                if press & 0x80 != 0 {
                    // loc_81AE
                    self.ex_out &= !(p.id + 1);
                    self.play_sample(0x41);
                    p.flags &= !1;
                    p.state = PlayerState::Ex(ExState::Join);
                    return true;
                }
                p.w26 = p.w26.wrapping_sub(1);
                if p.w26 == 0xFFFF {
                    p.w26 = 0;
                    self.ex_out |= p.id + 1;
                }
                self.queue_panel(PanelCommand::Text { entry: PROMPT_TEXT, blink: 0, player: p.id });
                self.queue_panel(PanelCommand::Text { entry: JOIN_TEXT, blink: 0x0F, player: p.id });
            }
            ExState::Join => {
                self.queue_panel(PanelCommand::Wipe { player: p.id, frame: 5 });
                // sub_8210
                p.speed_index = 0;
                p.speed_level = 0;
                p.score = 0;
                p.acc = 0;
                p.beans_total = 0;
                p.beans_count = 0;
                if let Some(slot) = p.holder_slot {
                    if let Some(Actor::PairHolder(mut h)) = self.slots[slot].take() {
                        h.w20 = 0xFFFF;
                        h.dealt = 0;
                        h.b07 = 0;
                        for at in [4, 2, 0] {
                            let (a, b) = self.exercise_pair(&mut h, 0);
                            h.pairs[at] = a;
                            h.pairs[at + 1] = b;
                        }
                        self.slots[slot] = Some(Actor::PairHolder(h));
                    }
                }
                self.fields[id] = Field::default();
                self.reset_field_bytes(id);
                self.redraw_field(id);
                self.set_up_exercise(p);
            }
            ExState::Leave => return false,
        }
        true
    }

    /// `SpecPlane93`: a side's bean count as 4 digits (tile pairs, from 0).
    pub(super) fn draw_beans(&mut self, player: u8, count: u16) {
        let side = usize::from(player & 1);
        let row = self.plane_row();
        let mut at = BEANS_AT[side];
        let digits = [count / 1000 % 10, count / 100 % 10, count / 10 % 10, count % 10];
        for d in digits {
            let tile = ((d + 1) << 1) as u8;
            self.plane_writes.push((at, SIDE_BASE[side] | u16::from(tile)));
            self.plane_writes.push((at.wrapping_add(row), SIDE_BASE[side] | u16::from(tile.wrapping_add(1))));
            at = at.wrapping_add(2);
        }
    }
}
