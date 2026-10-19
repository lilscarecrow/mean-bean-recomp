//! The end of a match: a player's pair can't spawn (`sub_4E24` finds the spawn cell
//! taken) and its player actor runs the top-out sequence (`loc_79FC`). In scenario
//! mode, when the CPU tops out: the winner's actor is deleted, stars rise from its
//! field, the loser's field falls away column by column, and the loser's actor then
//! shows the results (time, a time bonus counted into the score, the password) and
//! hands control back to the game flow (`bytecode_disabled` cleared).

use super::*;
use crate::mbm::rom;

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

/// `loc_72A4` (`sub_7292`): draws the win banner over 10 frames, then holds it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub state: BannerState,
    pub timer: u16,
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
        self.crowding = u16::from(p.id) << 1 | 1;
        self.result_score = 0;
        self.result_beans = 0;
        if p.id == 0 {
            // P2 won: the CPU's actor goes, P1's field falls away, then "continue?".
            self.slots[p.other_slot] = None; // ActorDeleteOther
            if self.opponent == OPP_ROBOTNIK {
                return Err(Unported("Robotnik's victory voice (sub_7C14)"));
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
            return Err(Unported("high score entry (sub_C438)"));
        }
        self.slots[p.other_slot] = None; // ActorDeleteOther
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Banner(Banner { state: BannerState::Start, timer: 0 }));
        }
        self.spawn_stars(FIELD_X[0] + 0x10);
        if self.opponent == OPP_ROBOTNIK {
            return Err(Unported("Robotnik's defeat voice (sub_7BD8)"));
        }
        self.wipe_field(p);
        p.state = PlayerState::Wipe;
        Ok(())
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
    fn wipe_field(&mut self, p: &mut Player) {
        let id = p.id as usize & 1;
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
                    p.state = Cleared;
                }
            }
            Cleared => {
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
                p.delay = 0x20; // sub_7CD2 draws the time
                p.state = BonusWait;
            }
            BonusWait => {
                if wait(&mut p.delay) {
                    p.state = Bonus;
                }
            }
            Bonus => {
                if p.base == 0 {
                    p.delay = 0x40;
                    p.state = NoBonusWait;
                } else {
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
                    self.show_password()?;
                    p.state = Continue; // the bookmark at $7E9E (no delay left)
                } else if p.w26 < 0x780 && p.base != 0 {
                    // loc_7D1A
                    let step = p.base.min(p.w28);
                    p.base -= step;
                    add_points(p, u32::from(step));
                }
            }
            NoBonusWait => {
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = NoBonus;
                }
            }
            NoBonus => {
                self.show_password()?;
                p.state = Continue;
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
            Done | ContinueDone => {
                self.match_over = true; // bytecode_disabled = 0: the game flow resumes
                return Ok(false);
            }
            ContinueWait => {
                if bookmark_ctrl(&mut p.delay, press) {
                    p.state = HighScore;
                }
            }
            HighScore => {
                self.high_score_table = self.level_mode;
                if self.enter_high_score(p) {
                    return Err(Unported("high score name entry (sub_BFA6)"));
                }
                p.state = ContinueCheck;
            }
            ContinueCheck => {
                if p.flags & 2 == 0 {
                    self.fields[p.id as usize & 1] = Field::default(); // ResetPuyoField
                    p.flags = 0;
                    p.state = ContinueDone;
                }
            }
            _ => unreachable!(),
        }
        Ok(true)
    }

    /// `sub_8298`: the sign that rises over the loser's field.
    fn spawn_continue_sign(&mut self, p: &Player) {
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
    fn enter_high_score(&mut self, p: &Player) -> bool {
        let table = usize::from(self.high_score_table);
        if table >= 4 {
            return false;
        }
        let base = table * 0x50;
        let entry = |hs: &[u8], i: usize| u32::from_be_bytes(hs[base + i * 0x10 + 6..base + i * 0x10 + 10].try_into().unwrap());
        let Some(rank) = (0..5).find(|&i| entry(&self.high_scores, i) < p.score) else { return false };
        // Move the lower entries down one (the last falls off).
        for i in (rank..4).rev() {
            let (from, to) = (base + i * 0x10, base + (i + 1) * 0x10);
            self.high_scores.copy_within(from..from + 0x10, to);
        }
        let at = base + rank * 0x10;
        self.high_scores[at] = 0xFF;
        self.high_scores[at + 6..at + 10].copy_from_slice(&p.score.to_be_bytes());
        self.high_scores[at + 0x0A..at + 0x0C].copy_from_slice(&p.beans_total.to_be_bytes());
        true
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

    /// `loc_7E56`: on to the next stage; `sub_7D62` queues the password (plane $9E).
    fn show_password(&mut self) -> Result<(), Unported> {
        if self.level == 0x0F {
            return Err(Unported("the final stage's ending (loc_7E68)"));
        }
        self.password_command = true;
        Ok(())
    }

    /// `SpecPlane9E` (in the vertical blank): the password for the next stage and its
    /// four beans.
    pub(super) fn run_password_command(&mut self) {
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
            StarState::Burst => {
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
                b.timer = b.timer.wrapping_sub(1);
                if b.timer == 0 {
                    b.state = BannerState::Hold;
                }
            }
            BannerState::Hold => {}
        }
        Some(b)
    }

    pub(super) fn run_field_wipe(&mut self, mut w: FieldWipe) -> Option<FieldWipe> {
        if w.anim.step_with(&mut || self.random()) {
            self.release(w.parent);
            return None;
        }
        if w.anim.frame != w.shown {
            w.shown = w.anim.frame; // QueuePlaneCmd $84: drawing isn't ported
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

/// `ActorBookmark_Ctrl`: a wait any button press ends.
fn bookmark_ctrl(delay: &mut u16, press: bool) -> bool {
    if press {
        *delay = 0;
        return true;
    }
    wait(delay)
}
