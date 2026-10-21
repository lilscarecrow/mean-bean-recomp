//! The end of a match: a player's pair can't spawn (`sub_4E24` finds the spawn cell
//! taken) and its player actor runs the top-out sequence (`loc_79FC`). In scenario
//! mode, when the CPU tops out: the winner's actor is deleted, stars rise from its
//! field, the loser's field falls away column by column, and the loser's actor then
//! shows the results (time, a time bonus counted into the score, the password) and
//! hands control back to the game flow (`bytecode_disabled` cleared).

use super::*;
use crate::rom;

/// `unk_7A7C`: per column (left to right), the frames before it falls and its
/// sideways acceleration.
const COLUMN_DELAY: [u16; 6] = [0x24, 0x10, 0x04, 0x00, 0x14, 0x20];
const COLUMN_ACCEL: [u16; 6] = [0x0B00, 0x0A00, 0x0800, 0x0700, 0x0900, 0x0B00];
/// `unk_8292`: colors a column with no beans falls as.
const COLUMN_COLORS: [u8; 6] = [0x00, 0x10, 0x30, 0x40, 0x50, 0x20];
/// `unk_837A`: the field wipe.
const WIPE_SCRIPT: u32 = 0x837A;
/// `off_71CC`: the sparks of a star, by its frame.
const STAR_SPARKS: u32 = 0x71DC;
/// `unk_DF8E`: `byte_FF0104` by the difficulty option.
const CPU_DIFFICULTY: [u8; 4] = [0, 2, 4, 6];
/// `Passwords`: 8 bytes per level from level 3 (a word per difficulty).
const PASSWORDS: u32 = 0x7018;
/// `byte_1A320`, `byte_1A328`, `off_1A348`, `unk_1A330`: password beans by digit.
const PASSWORD_COLORS: [u8; 7] = [0, 1, 5, 4, 3, 6, 0x19];
const PASSWORD_FRAMES: [u8; 7] = [0, 0, 0, 0, 0, 0, 0x0A];
const PASSWORD_SCRIPTS: u32 = 0x1A348;
const OPP_ROBOTNIK: u8 = 12;
const PASSWORD_PLACES: [(u8, u16, u16); 4] = [(0x5D, 0xA8, 0x128), (0x3E, 0xB8, 0x128), (0x48, 0xC8, 0x128), (0x23, 0xD8, 0x128)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StarState {
    /// $7104: just spawned.
    Init,
    /// $710E: a short wait, then $7114 counts its own delay down.
    Wait,
    Delay,
    /// $7132: rise for $40 frames.
    Rise,
    /// $714E: 8 frames, then $7152 bursts into sparks.
    Pause,
    Burst,
    /// $7262 (1P vs 2P, `loc_71FC`): +$26 frames, then the burst.
    VsWait,
}

impl StarState {
    pub fn code(self) -> u32 {
        match self {
            StarState::Init => 0x7104,
            StarState::Wait => 0x710E,
            StarState::Delay => 0x7114,
            StarState::Rise => 0x7132,
            StarState::Pause => 0x714E,
            StarState::Burst => 0x7152,
            StarState::VsWait => 0x7262,
        }
    }
}

/// `sub_7104` (four, from `sub_7078`): a star rising from the winner's field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Star {
    pub state: StarState,
    pub motion: Motion,
    pub draw: u8,
    /// +$08 color, +$09 frame (which spark script it bursts into).
    pub color: u8,
    pub frame: u8,
    pub timer: u16,
    pub delay: u16,
}

/// `loc_7270`: a spark from a star's burst.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StarSpark {
    pub motion: Motion,
    pub draw: u8,
    pub anim: Anim,
}

pub const STAR_SPARK_CODE: u32 = 0x7270;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BannerState {
    Start,
    Count,
    Hold,
}

impl BannerState {
    pub fn code(self) -> u32 {
        match self {
            BannerState::Start => 0x72A4,
            BannerState::Count => 0x72B0,
            BannerState::Hold => 0x72D0,
        }
    }
}

/// `loc_72A4` (`sub_7292`): draws the win banner over 10 frames, then "YOU WIN".
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub state: BannerState,
    pub timer: u16,
}

/// `loc_7C2C` (`sub_7C14`, Robotnik won) / `loc_7BF0` (`sub_7BD8`, he lost): waits
/// +$26 frames, then plays his line (sound $6B / $90).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RobotnikVoice {
    /// The sound: `VICTORY` or `DEFEAT`.
    pub line: u8,
    /// False at the spawn address (+$26 not set yet).
    pub started: bool,
    pub wait: u16,
}

impl RobotnikVoice {
    pub const VICTORY: u8 = 0x6B;
    pub const DEFEAT: u8 = 0x90;

    pub fn code(&self) -> u32 {
        let spawn = if self.line == Self::VICTORY { 0x7C2C } else { 0x7BF0 };
        if self.started { spawn + 12 } else { spawn }
    }
}

/// `loc_833E`: wipes the loser's field tiles; tells the loser when done.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldWipe {
    pub parent: usize,
    pub anim: Anim,
    pub shown: u8,
}

pub const FIELD_WIPE_CODE: u32 = 0x833E;

/// `loc_8386`: one column of the loser's field falling off the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FallingColumn {
    pub parent: usize,
    pub falling: bool,
    pub motion: Motion,
    pub draw: u8,
    pub color: u8,
    pub delay: u16,
    /// +$32: its entry in the vertical scroll table.
    pub scroll: u16,
}

/// `loc_1A318` (plane command $9E): a bean of the password.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PasswordBean {
    pub color: u8,
    pub x: u16,
    pub y: u16,
    pub anim: Anim,
}

pub const PASSWORD_BEAN_CODE: u32 = 0x1A318;

impl Engine {
    /// `loc_79FC`: the player's pair can't spawn.
    pub(super) fn top_out(&mut self, p: &mut Player) -> Result<(), Unported> {
        if self.level_mode == 1 {
            // (no StopSound pause)
            self.vs_round_lost(p);
            return Ok(());
        }
        if self.level_mode >= 1 {
            return Err(Unported("top-out outside scenario mode (loc_7A12)"));
        }
        p.state = PlayerState::TopOut; // StopSound, then the next frame
        Ok(())
    }

    /// $7A12: `sub_7A94` (scenario: `loc_7ACE`) and `sub_7926`.
    pub(super) fn announce_result(&mut self, p: &mut Player) -> Result<(), Unported> {
        self.popping = [true, true];
        self.player_flags = [0, 0];
        self.bytecode_flag = p.id ^ 1;
        self.last_winner = p.id ^ 1; // byte_FF0115
        self.crowding = u16::from(p.id) << 1 | 1;
        self.result_score = 0;
        self.result_beans = 0;
        if p.id == 0 {
            // P2 won: the CPU's actor goes, P1's field falls away, then "continue?".
            self.delete_player(p.other_slot); // ActorDeleteOther
            if self.opponent == OPP_ROBOTNIK {
                self.spawn_voice(RobotnikVoice::VICTORY); // sub_7C14
            }
            self.wipe_field(p);
            p.state = PlayerState::Wipe;
            return Ok(());
        }
        // loc_7B2C: P1 won.
        self.difficulty = CPU_DIFFICULTY[usize::from(self.difficulty_option & 3)];
        let secs = (self.time_frames / 60).min(999);
        p.beans_total = secs;
        let left = (u16::from(self.bonus_level) + 1) * 10 + 0x6E;
        let x = u32::from(left.saturating_sub(secs));
        p.base = ((x * x) as u16 as u32 * 3) as u16; // the time bonus
        let winner = self.player_mut(p.other_slot).clone();
        p.score = winner.score;
        self.result_score = winner.score.wrapping_add(u32::from(p.base)).min(99_999_999);
        self.result_beans = winner.beans_total;
        if self.level == 0x0F || self.level == 2 {
            // loc_7BB6: the last stage (or lesson) files the winner's score with the
            // time bonus in the high score table now; the name is entered later.
            let mut entry = winner.clone();
            entry.score = self.result_score;
            self.high_score_table = self.level_mode;
            self.enter_high_score(&entry);
        }
        self.delete_player(p.other_slot); // ActorDeleteOther
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Banner(Banner { state: BannerState::Start, timer: 0 }));
        }
        self.spawn_stars(FIELD_X[0] + 0x10);
        if self.opponent == OPP_ROBOTNIK {
            self.spawn_voice(RobotnikVoice::DEFEAT); // sub_7BD8
        }
        self.wipe_field(p);
        p.state = PlayerState::Wipe;
        Ok(())
    }

    /// `ActorDeleteOther` on a player, keeping falling beans whose step is queued.
    pub(super) fn delete_player(&mut self, slot: usize) {
        if let Some(Actor::Player(p)) = self.slots[slot].take() {
            if let (true, Some(work)) = (self.fall_commands.contains(&slot), p.pop) {
                self.orphan_fall = Some((slot, p.id, work));
            }
        }
    }

    /// `FindActorSlotQuick` for one of Robotnik's lines.
    fn spawn_voice(&mut self, line: u8) {
        if let Some(slot) = self.free_slot() {
            self.slots[slot] = Some(Actor::RobotnikVoice(RobotnikVoice { line, started: false, wait: 0 }));
        }
    }

    /// `loc_7C2C` / `loc_7BF0`: a pause, then Robotnik's line (the sound isn't ported).
    pub(super) fn run_voice(&mut self, mut v: RobotnikVoice) -> Option<RobotnikVoice> {
        if !v.started {
            v.started = true;
            v.wait = if v.line == RobotnikVoice::VICTORY { 0x20 } else { 0x10 };
            return Some(v);
        }
        v.wait = v.wait.wrapping_sub(1);
        // bpl: past 0 the sample plays and the actor ends.
        (v.wait & 0x8000 == 0).then_some(v)
    }

    /// `sub_7078`: four stars from the winner's field (left edge `x0`).
    fn spawn_stars(&mut self, x0: u16) {
        for i in (0..4u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let timer = u16::from(i) << 4 | (self.random() & 0x0F);
            let x = self.random_below(0x40) + x0;
            let motion = Motion {
                x: u32::from(x) << 16,
                y: 0x160 << 16,
                vx: 0x1_0000,
                vy: 0xFFFC_0000,
                ax: 0x2000,
                ay: 0x0C00,
                tx: x,
                ty: 0xFFFF,
            };
            let star = Star { state: StarState::Init, motion, draw: 0x0F, color: 0x25, frame: i, timer, delay: 0 };
            self.slots[slot] = Some(Actor::Star(star));
        }
    }

    /// `sub_7926`: the loser's field falls away; `w26` counts the actors still busy.
    pub(super) fn wipe_field(&mut self, p: &mut Player) {
        let id = p.id as usize & 1;
        // The beans lose their sideways links, and the field is redrawn.
        for i in HIDDEN_ROWS * COLS..ROWS * COLS {
            let b = &mut self.field_bytes[id].cells[i];
            if *b != 0 && *b & 0x70 != 0x60 {
                *b &= 0xF3;
            }
        }
        self.redraw_field(id);
        let me = self.current_slot;
        let mut busy = 0;
        if let Some(slot) = self.find_actor_slot() {
            busy += 1;
            self.slots[slot] = Some(Actor::FieldWipe(FieldWipe { parent: me, anim: Anim::new(WIPE_SCRIPT), shown: 0xFF }));
        }
        let mut x = FIELD_X[id] + 0x17;
        for col in 0..COLS {
            if let Some(slot) = self.find_actor_slot() {
                busy += 1;
                let color = self.column_color(id, col);
                let motion = Motion { x: u32::from(x) << 16, ax: COLUMN_ACCEL[col], ay: 0x2000, ty: 0xFFFF, ..Motion::default() };
                let column = FallingColumn {
                    parent: me,
                    falling: false,
                    motion,
                    draw: 4,
                    color,
                    delay: COLUMN_DELAY[col],
                    // 4 bytes of vertical scroll per 16-pixel column of the screen.
                    scroll: ((FIELD_X[id] - 0x80) / 4 & 0x7F) + col as u16 * 4,
                };
                self.slots[slot] = Some(Actor::FallingColumn(column));
            }
            x += 0x0A;
        }
        p.w26 = busy;
    }

    /// `sub_8250`: a column falls as the color of its highest bean (garbage doesn't
    /// count), or a random one when it has none.
    fn column_color(&mut self, id: usize, col: usize) -> u8 {
        for row in HIDDEN_ROWS..ROWS {
            match self.fields[id].get(col as i16, row as i16) {
                Some(BeanColor::Garbage) | None => {}
                Some(c) => return c as u8,
            }
        }
        COLUMN_COLORS[usize::from(self.random_below(5))] >> 4
    }

    /// The results, from $7A1E on. Returns false when the actor is done.
    pub(super) fn run_results(&mut self, p: &mut Player, inputs: &Inputs) -> Result<bool, Unported> {
        use PlayerState::*;
        let press = (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0;
        match p.state {
            Wipe => {
                if p.w26 == 0 {
                    self.fields[p.id as usize & 1] = Field::default(); // ResetPuyoField
                    self.reset_field_bytes(p.id as usize & 1);
                    self.redraw_field(p.id as usize & 1);
                    p.state = Cleared;
                }
            }
            Cleared => {
                // The field's columns stop scrolling (plane A and B words).
                let first = usize::from(FIELD_ORIGIN[p.id as usize & 1] & 0x7F) / 2;
                for i in 0..12 {
                    self.vscroll[(first + i) % 40] = 0;
                }
                if self.level_mode & 3 == 1 {
                    self.start_vs_results(p);
                    return Ok(true);
                }
                // loc_7D78
                if p.id == 0 {
                    // loc_7EC2
                    p.flags = 1;
                    self.spawn_continue_sign(p);
                    p.delay = 0x80;
                    p.state = ContinueWait;
                    return Ok(true);
                }
                p.id = 0;
                p.delay = 0x60;
                p.state = ResultsWait;
            }
            ResultsWait => {
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = ResultsShown;
                }
            }
            ResultsShown => {
                self.queue_panel(PanelCommand::Time(p.beans_total)); // sub_7CD2
                p.delay = 0x20;
                p.state = BonusWait;
            }
            BonusWait => {
                if wait(&mut p.delay) {
                    p.state = Bonus;
                }
            }
            Bonus => {
                if p.base == 0 {
                    self.queue_panel(PanelCommand::Text { entry: NO_BONUS_TEXT, blink: 0, player: 0 });
                    p.delay = 0x40;
                    p.state = NoBonusWait;
                } else {
                    self.queue_panel(PanelCommand::Bonus(p.base));
                    p.w28 = (p.base >> 7).max(1);
                    p.w26 = 0x80;
                    p.delay = 0x3C;
                    p.state = TallyWait;
                }
            }
            TallyWait => {
                if wait(&mut p.delay) {
                    p.state = Tally;
                }
            }
            Tally => {
                // sub_56C0: the actor is player 0 now, so this is P1's pad.
                let skip = inputs.pads[0].press & 0xF0 != 0;
                p.w26 = if skip { p.w26 } else { p.w26.wrapping_sub(1) };
                if skip || p.w26 == 0 {
                    add_points(p, u32::from(p.base));
                    self.queue_panel(PanelCommand::Score(p.id));
                    self.queue_panel(PanelCommand::Bonus(0)); // $99000000
                    let next = self.after_results(p);
                    p.state = next; // the bookmark at $7E9E (no delay left)
                } else if p.w26 < 0x780 && p.base != 0 {
                    // loc_7D1A
                    let step = p.base.min(p.w28);
                    p.base -= step;
                    add_points(p, u32::from(step));
                    self.queue_panel(PanelCommand::Score(p.id));
                    self.queue_panel(PanelCommand::Bonus(p.base));
                }
            }
            NoBonusWait => {
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = NoBonus;
                }
            }
            NoBonus => {
                let next = self.after_results(p);
                p.state = next;
            }
            FinalWait => {
                if wait(&mut p.delay) {
                    p.state = FinalLeave;
                }
            }
            FinalLeave => {
                p.flags = 0;
                p.state = FinalDone;
            }
            Continue => {
                p.delay = 0x0A;
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = Leave;
                }
            }
            Leave => {
                p.flags = 0;
                p.state = Done;
            }
            Done | ContinueDone | FinalDone => {
                self.match_over = true; // bytecode_disabled = 0: the game flow resumes
                self.bytecode_disabled = false;
                return Ok(false);
            }
            ContinueWait => {
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = HighScore;
                }
            }
            HighScore => {
                self.high_score_table = self.level_mode;
                if let Some(rank) = self.enter_high_score(p) {
                    self.spawn_name_entry(p, rank);
                }
                p.state = ContinueCheck;
            }
            ContinueCheck => {
                if p.flags & 2 == 0 {
                    self.fields[p.id as usize & 1] = Field::default(); // ResetPuyoField
                    self.reset_field_bytes(p.id as usize & 1);
                    self.redraw_field(p.id as usize & 1);
                    p.flags = 0;
                    p.state = ContinueDone;
                }
            }
            _ => unreachable!(),
        }
        Ok(true)
    }

    /// `sub_8298`: the sign that rises over the loser's field.
    pub(super) fn spawn_continue_sign(&mut self, p: &Player) {
        let Some(slot) = self.find_actor_slot() else { return };
        let id = usize::from(p.id & 1);
        let sign = ContinueSign {
            parent: self.current_slot,
            x: FIELD_X[id] + 0x30,
            y: 0x168,
            target_y: FIELD_TOP_Y + 0x40,
            anim: Anim::new(CONTINUE_SIGN_SCRIPT),
        };
        self.slots[slot] = Some(Actor::ContinueSign(sign));
    }

    /// `sub_C438`: put the score in the high score table of `high_score_table` if it
    /// beats an entry (scores at +6, beans at +$0A, a new entry marked $FF at +0).
    fn enter_high_score(&mut self, p: &Player) -> Option<u16> {
        let table = usize::from(self.high_score_table);
        if table >= 4 {
            return None;
        }
        let base = table * 0x50;
        let entry = |hs: &[u8], i: usize| u32::from_be_bytes(hs[base + i * 0x10 + 6..base + i * 0x10 + 10].try_into().unwrap());
        let rank = (0..5).find(|&i| entry(&self.high_scores, i) < p.score)?;
        // Move the lower entries down one (the last falls off).
        for i in (rank..4).rev() {
            let (from, to) = (base + i * 0x10, base + (i + 1) * 0x10);
            self.high_scores.copy_within(from..from + 0x10, to);
        }
        let at = base + rank * 0x10;
        self.high_scores[at] = 0xFF;
        self.high_scores[at + 6..at + 10].copy_from_slice(&p.score.to_be_bytes());
        self.high_scores[at + 0x0A..at + 0x0C].copy_from_slice(&p.beans_total.to_be_bytes());
        Some(rank as u16)
    }

    /// `sub_BFA6`: enter a name for the new high score; the player waits on flag bit 1.
    fn spawn_name_entry(&mut self, p: &mut Player, rank: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        p.flags = 3;
        let entry = NameEntry {
            state: NameEntryState::Start,
            parent: self.current_slot,
            player_id: p.id,
            rank,
            timeout: 0x780,
            cursor: 0,
            letters: [0; 5],
            blink: 0,
            delay: 0,
        };
        self.slots[slot] = Some(Actor::NameEntry(entry));
    }

    pub(super) fn run_name_entry(&mut self, mut n: NameEntry, inputs: &Inputs) -> Option<NameEntry> {
        match n.state {
            NameEntryState::Start => {
                self.name_prompt(n.player_id, n.rank as u8);
                n.state = NameEntryState::Clear;
            }
            NameEntryState::Clear => {
                n.letters = [1; 5]; // "A"
                n.state = NameEntryState::Edit;
            }
            NameEntryState::Edit => {
                self.name_letters(n.player_id, &n.letters, n.cursor, n.blink, false);
                n.blink = n.blink.wrapping_add(1);
                let pad = inputs.pads[usize::from(n.player_id & 1)];
                let at = usize::from(n.cursor).min(4);
                if pad.press & 0x04 != 0 {
                    n.cursor = n.cursor.saturating_sub(1); // left: back a letter
                } else if pad.press & 0x70 != 0 {
                    n.cursor += 1;
                    if n.cursor >= 3 {
                        self.save_name(&n);
                        n.timeout = 0;
                        n.delay = 0x20;
                        n.state = NameEntryState::Shown;
                    }
                } else if pad.repeat & 0x02 != 0 {
                    n.letters[at] = if n.letters[at] + 1 >= 0x1B { 0 } else { n.letters[at] + 1 };
                    n.blink = 0;
                } else if pad.repeat & 0x01 != 0 {
                    n.letters[at] = if n.letters[at] == 0 || n.letters[at] & 0x80 != 0 { 0x1A } else { n.letters[at] - 1 };
                    n.blink = 0;
                }
            }
            NameEntryState::Shown => {
                if wait(&mut n.delay) {
                    // sub_C168 with +$25 (the delay's low byte, 0 now): the letters shown.
                    self.name_letters(n.player_id, &n.letters, n.cursor, n.blink, false);
                    n.state = NameEntryState::Done;
                }
            }
            NameEntryState::Done => {
                if let Some(Actor::Player(p)) = self.slots[n.parent].as_mut() {
                    p.flags &= !2;
                }
                return None;
            }
        }
        Some(n)
    }

    /// `loc_C0F8` + `sub_C420`: the name goes into the entry holding the score.
    fn save_name(&mut self, n: &NameEntry) {
        let Some(Actor::Player(p)) = &self.slots[n.parent] else { return };
        let score = p.score;
        let base = usize::from(self.high_score_table) * 0x50;
        for rank in usize::from(n.rank)..5 {
            let at = base + rank * 0x10;
            if self.high_scores[at + 6..at + 10] == score.to_be_bytes() {
                self.high_scores[at..at + 3].copy_from_slice(&n.letters[..3]);
                self.high_scores[at + 3] = 0xFF;
                return; // (sub_23536 then backs up the save block: not modelled)
            }
        }
    }

    pub(super) fn run_continue_sign(&mut self, mut c: ContinueSign) -> Option<ContinueSign> {
        let shown = matches!(&self.slots[c.parent], Some(Actor::Player(p)) if p.flags & 1 != 0);
        if !shown {
            return None;
        }
        c.anim.step_with(&mut || self.random());
        if c.target_y < c.y {
            c.y -= 1;
        }
        Some(c)
    }

    /// `sub_83F6`: four beans bounce out of a column as it falls off the bottom, the
    /// last one with a sparkle following it.
    fn spawn_bounce(&mut self, c: &FallingColumn) {
        let mut last = None;
        for i in (0..4u16).rev() {
            let Some(slot) = self.free_slot() else { return };
            let motion = Motion {
                y: 0x15F << 16,
                vy: 0xFFFD_0000,
                ay: c.motion.ax,
                tx: (c.motion.x >> 16) as u16,
                ty: 0xFFFF,
                ..Motion::default()
            };
            let bean = BouncingBean {
                state: BounceState::Wait,
                motion,
                draw: 0,
                color: c.color,
                frame: BOUNCE_FRAMES[usize::from(i)],
                angle: (c.motion.x >> 16) as u8,
                timer: i * 4 + 1,
                followed: false,
            };
            self.slots[slot] = Some(Actor::BouncingBean(bean));
            last = Some(slot);
        }
        let Some(bean_slot) = last else { return };
        let Some(slot) = self.free_slot() else { return };
        if let Some(Actor::BouncingBean(b)) = self.slots[bean_slot].as_mut() {
            b.followed = true;
        }
        let mut anim = Anim::new(BOUNCE_SPARKLE_SCRIPT);
        anim.frame = 0x11;
        let follower = BounceSparkle { bean: bean_slot, x: 0, y: 0, anim };
        self.slots[slot] = Some(Actor::BounceSparkle(follower));
    }

    pub(super) fn run_bouncing_bean(&mut self, mut b: BouncingBean) -> Option<BouncingBean> {
        match b.state {
            BounceState::Wait => {
                b.timer = b.timer.wrapping_sub(1);
                if b.timer == 0 {
                    b.draw = 0x85;
                    b.state = BounceState::Fly;
                }
            }
            BounceState::Fly => {
                if b.motion.step(b.draw) {
                    b.followed = false;
                    b.state = BounceState::Gone;
                } else {
                    let dx = (super::sin(b.angle, 0x1000) >> 16) as u16;
                    b.motion.x = (b.motion.x & 0xFFFF) | u32::from(dx.wrapping_add(b.motion.tx)) << 16;
                    b.angle = b.angle.wrapping_add(5);
                }
            }
            BounceState::Gone => return None,
        }
        Some(b)
    }

    pub(super) fn run_bounce_sparkle(&mut self, mut s: BounceSparkle) -> Option<BounceSparkle> {
        let Some(Actor::BouncingBean(b)) = &self.slots[s.bean] else { return None };
        if !b.followed {
            return None;
        }
        let (x, y) = ((b.motion.x >> 16) as u16, (b.motion.y >> 16) as u16);
        s.anim.step_with(&mut || self.random());
        s.x = x;
        s.y = y;
        Some(s)
    }

    /// `loc_7E56`: on to the next stage: `sub_7D62` queues the password (plane $9E); after
    /// the last stage, `loc_7E68` shows the ending text for $78 frames instead.
    fn after_results(&mut self, p: &mut Player) -> PlayerState {
        if self.level == 0x0F {
            self.queue_panel(PanelCommand::Text { entry: FINAL_TEXT, blink: 0, player: 0 });
            p.delay = 0x78;
            return PlayerState::FinalWait;
        }
        self.password_command = true;
        PlayerState::Continue
    }

    /// `SpecPlane9E` (in the vertical blank): the password for the next stage and its
    /// four beans.
    pub(super) fn run_password_command(&mut self) {
        self.result_line(4);
        let level = u32::from(self.level.saturating_sub(3));
        let difficulty = u32::from((self.difficulty_option as i8).wrapping_neg().wrapping_sub(1) as u8 & 3);
        let mut code = rom::word(PASSWORDS + level * 8 + difficulty * 2);
        self.password = code;
        for &(time, x, y) in &PASSWORD_PLACES {
            code = code.rotate_left(4);
            let Some(slot) = self.find_actor_slot() else { continue };
            let digit = usize::from(code & 0x0F);
            let mut anim = Anim::new(rom::long(PASSWORD_SCRIPTS + digit as u32 * 4));
            anim.time = time;
            anim.frame = PASSWORD_FRAMES.get(digit).copied().unwrap_or(0);
            let color = PASSWORD_COLORS.get(digit).copied().unwrap_or(0);
            self.slots[slot] = Some(Actor::PasswordBean(PasswordBean { color, x, y, anim }));
        }
    }

    pub(super) fn run_star(&mut self, mut s: Star) -> Option<Star> {
        match s.state {
            StarState::Init => {
                s.delay = 0x18;
                s.state = StarState::Wait;
            }
            StarState::Wait => {
                if wait(&mut s.delay) {
                    s.state = StarState::Delay;
                }
            }
            StarState::Delay => {
                if s.timer != 0 {
                    s.timer -= 1;
                } else {
                    s.draw |= 0x80;
                    s.timer = 0x40;
                    s.state = StarState::Rise;
                }
            }
            StarState::Rise => {
                s.motion.step(s.draw);
                s.timer = s.timer.wrapping_sub(1);
                if s.timer == 0 {
                    s.draw = 0;
                    s.delay = 8;
                    s.state = StarState::Pause;
                }
            }
            StarState::Pause => {
                if wait(&mut s.delay) {
                    s.state = StarState::Burst;
                }
            }
            StarState::VsWait if s.timer != 0 => s.timer -= 1,
            StarState::Burst | StarState::VsWait => {
                let script = STAR_SPARKS + u32::from(s.frame) * 8;
                for i in (0..8u8).rev() {
                    let Some(slot) = self.free_slot() else { continue };
                    let mut anim = Anim::new(script);
                    anim.frame = s.frame;
                    let angle = i << 5;
                    let motion = Motion {
                        x: s.motion.x & 0xFFFF_0000,
                        y: s.motion.y & 0xFFFF_0000,
                        vx: super::sin(angle, 0x100),
                        vy: super::cos(angle, 0x100),
                        ay: 0x0800,
                        ty: 0xFFFF,
                        ..Motion::default()
                    };
                    self.slots[slot] = Some(Actor::StarSpark(StarSpark { motion, draw: 0x87, anim }));
                }
                return None;
            }
        }
        Some(s)
    }

    pub(super) fn run_star_spark(&mut self, mut s: StarSpark) -> Option<StarSpark> {
        if s.motion.step(s.draw) || s.anim.step_with(&mut || self.random()) {
            return None;
        }
        Some(s)
    }

    pub(super) fn run_banner(&mut self, mut b: Banner) -> Option<Banner> {
        match b.state {
            BannerState::Start => {
                b.timer = 0x0A;
                b.state = BannerState::Count;
            }
            BannerState::Count => {
                self.queue_panel(PanelCommand::Banner(b.timer as u8));
                b.timer = b.timer.wrapping_sub(1);
                if b.timer == 0 {
                    b.state = BannerState::Hold;
                }
            }
            // loc_72CA: queued every frame (the text blinks with the frame counter).
            BannerState::Hold => self.queue_panel(PanelCommand::Text { entry: WIN_TEXT, blink: 0x0F, player: 0 }),
        }
        Some(b)
    }

    pub(super) fn run_field_wipe(&mut self, mut w: FieldWipe) -> Option<FieldWipe> {
        if w.anim.step_with(&mut || self.random()) {
            self.release(w.parent);
            return None;
        }
        if w.anim.frame != w.shown {
            w.shown = w.anim.frame;
            let player = match &self.slots[w.parent] {
                Some(Actor::Player(p)) => p.id,
                _ => 0,
            };
            self.queue_panel(PanelCommand::Wipe { player, frame: w.shown });
        }
        Some(w)
    }

    pub(super) fn run_falling_column(&mut self, mut c: FallingColumn) -> Result<Option<FallingColumn>, Unported> {
        if !c.falling {
            if wait(&mut c.delay) {
                c.falling = true;
            }
            return Ok(Some(c));
        }
        c.motion.y = c.motion.y.wrapping_add(c.motion.vy);
        if (c.motion.y >> 16) as u16 >= 0xD0 {
            // sub_83D2: in scenario mode only P1's field throws beans up as it goes.
            let parent_id = match &self.slots[c.parent] {
                Some(Actor::Player(p)) => p.id,
                _ => 1,
            };
            // sub_83D2 (unk_83EC): by mode and player.
            const BOUNCE: [bool; 8] = [true, false, false, false, true, true, true, true];
            if BOUNCE.get(usize::from(self.level_mode << 1 | parent_id)).copied().unwrap_or(true) {
                self.spawn_bounce(&c);
            }
            self.release(c.parent);
            return Ok(None);
        }
        c.motion.step(c.draw);
        // The column of plane A scrolls down with it.
        self.vscroll[usize::from(c.scroll / 2) % 40] = ((c.motion.y >> 16) as u16).wrapping_neg();
        Ok(Some(c))
    }

    /// A helper of the loser's actor is done (its +$26 counts them).
    fn release(&mut self, parent: usize) {
        if let Some(Actor::Player(p)) = self.slots[parent].as_mut() {
            p.w26 = p.w26.wrapping_sub(1);
        }
    }

    pub(super) fn run_password_bean(&mut self, mut b: PasswordBean) -> Option<PasswordBean> {
        b.anim.step_with(&mut || self.random());
        Some(b)
    }
}

/// `sub_82E0` (`sub_8298`): the "continue?" sign rising over the loser's field while
/// the loser's flag bit 0 is set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContinueSign {
    pub parent: usize,
    pub x: u16,
    pub y: u16,
    /// +$20: where it stops rising.
    pub target_y: u16,
    pub anim: Anim,
}

pub const CONTINUE_SIGN_CODE: u32 = 0x82E0;
const CONTINUE_SIGN_SCRIPT: u32 = 0x8308;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BounceState {
    /// $8490: a short stagger.
    Wait,
    /// $84A4: fly up and fall, swaying.
    Fly,
    /// $84D2: gone off the screen.
    Gone,
}

impl BounceState {
    pub fn code(self) -> u32 {
        match self {
            BounceState::Wait => 0x8490,
            BounceState::Fly => 0x84A4,
            BounceState::Gone => 0x84D2,
        }
    }
}

/// `loc_8490`: a bean thrown up by a falling column of P1's field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BouncingBean {
    pub state: BounceState,
    /// x's high word is set from `tx` (+$1E) and the sway angle (+$36) each frame.
    pub motion: Motion,
    pub draw: u8,
    pub color: u8,
    pub frame: u8,
    pub angle: u8,
    pub timer: u16,
    /// +$07: a sparkle follows it.
    pub followed: bool,
}

/// `unk_848C`: sprite frames of the four beans.
const BOUNCE_FRAMES: [u8; 4] = [8, 4, 5, 6];

/// `loc_84D6`: a sparkle following a bouncing bean.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BounceSparkle {
    pub bean: usize,
    pub x: u16,
    pub y: u16,
    pub anim: Anim,
}

pub const BOUNCE_SPARKLE_CODE: u32 = 0x84D6;
const BOUNCE_SPARKLE_SCRIPT: u32 = 0x84F4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NameEntryState {
    /// $C00A: draw the prompt; $C04C: every letter "A".
    Start,
    Clear,
    /// $C060: up/down change the letter, A/B/C next, left back.
    Edit,
    /// $C138: show the name for $20 frames; $C146: let the player go.
    Shown,
    Done,
}

impl NameEntryState {
    pub fn code(self) -> u32 {
        match self {
            NameEntryState::Start => 0xC00A,
            NameEntryState::Clear => 0xC04C,
            NameEntryState::Edit => 0xC060,
            NameEntryState::Shown => 0xC138,
            NameEntryState::Done => 0xC146,
        }
    }
}

/// `loc_C00A` (`sub_BFA6`): three letters for a new high score.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NameEntry {
    pub state: NameEntryState,
    /// +$2E the player (it waits on flag bit 1), +$2A its id.
    pub parent: usize,
    pub player_id: u8,
    /// +$0A the rank, +$28 a timeout nobody counts here, +$0E the letter being edited,
    /// +$12.. the letters (0 blank, 1-26 A-Z), +$26 the cursor blink, +$24 delay.
    pub rank: u16,
    pub timeout: u16,
    pub cursor: u16,
    pub letters: [u8; 5],
    pub blink: u8,
    pub delay: u16,
}

/// `ActorBookmark_Ctrl`: a wait any button press ends.
pub(super) fn bookmark_ctrl(delay: &mut u16, press: bool) -> bool {
    if press {
        *delay = 0;
        return true;
    }
    wait(delay)
}
