//! The attract mode's tutorial (`BC_Tutorial`): a CPU player (`loc_133FA`, a cut-down
//! match player) works through nine lessons (`word_FF196E`: its low byte the lesson,
//! the high byte a count within it), each setting where the pairs go (+$20/+$21) and
//! when they drop (+$27), with the lesson's text typed into a box above the field and
//! a joystick and button showing what the CPU presses.

use super::*;
use crate::assets::planes::FIELD_ORIGIN;
use crate::rom;

/// `unk_13342`: the tutorial's beans, up to and with the $FF that ends them.
const BEAN_ORDER: u32 = 0x13342;
/// `unk_136A2` / `unk_136F8`: lessons 0 and 1 (`loc_13686` / `loc_136DC`): the
/// column, then the rotation, follows the script's frame.
const COLUMN_SCRIPT: u32 = 0x136A2;
const ROTATION_SCRIPT: u32 = 0x136F8;
/// The field lesson 5 starts from: 36 bytes (colors << 4 | $80) ending at $137FE, the
/// last six rows.
const LESSON_FIELD: u32 = 0x137DA;
/// `byte_1386A`: lesson 6's 26 pairs: column, rotation, frames before the drop, and
/// a text (`sub_FF4A`, negative for none).
const LESSON_6: u32 = 0x1386A;
/// `byte_13936`: lesson 8's four pairs: column, rotation.
const LESSON_8: u32 = 0x13936;
/// `unk_FF9E` (per text: a word, then the frames between characters; 8 entries, the
/// last for any past it) and `off_1007A` (the texts' scripts).
const TEXT_TIMES: u32 = 0xFF9E;
const TEXT_SCRIPTS: u32 = 0x1007A;
/// `byte_132BC` (the joystick at rest), `off_132C0` (leaning: down, left, right) and
/// `byte_1332C` / `byte_13328` (the button, at rest and pressed).
const STICK_REST: u32 = 0x132BC;
const STICK_LEANS: u32 = 0x132C0;
const BUTTON_REST: u32 = 0x1332C;
const BUTTON_PRESS: u32 = 0x13328;
/// The joystick's and button's mappings and places (`sub_13274` / `loc_132E4`).
pub const PAD_MAPPINGS: u8 = 0x26;
pub const PAD_AT: [(u16, u16); 2] = [(0x120, 0x150), (0x150, 0x150)];
/// The typing sound (+$08).
const TEXT_SOUND: u8 = 0x69;
/// `word_FF198A` in the tutorial: P1's score a little to the right.
const SCORE_AT: u16 = 0xCB3E;

/// Where `loc_133FA` is: its resume points.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LessonState {
    /// $133FA: just spawned.
    Setup,
    /// $1342A: the fall speed.
    Ready,
    /// $13442: the next pair's lesson (also reached the same frame from elsewhere).
    Next,
    /// $1345E / $13464: the lessons are over: $20 frames, then the game flow.
    EndWait,
    End,
    /// $1348E: while garbage falls.
    Garbage,
    /// $134AA: the pair in play.
    Pair,
    /// $134E0: redraw, then (`$134F8`) two frames.
    Redraw,
    Delay,
    /// $134FE: pops or the next pair.
    Decide,
    /// $1351E, $1352E: blink the pops; $1353A, $1354A: burst them; $13550: close
    /// the gaps; $13562: wait for the beans to fall.
    Backup,
    Blink,
    Pop,
    PopWait,
    Compact,
    Fall,
    /// $135B0: topped out: the field falls away; $135F6: $E0 frames; $135FC: its
    /// ground back.
    Wipe,
    WipeWait,
    Wiped,
}

impl LessonState {
    pub const ALL: [LessonState; 19] = [
        LessonState::Setup,
        LessonState::Ready,
        LessonState::Next,
        LessonState::EndWait,
        LessonState::End,
        LessonState::Garbage,
        LessonState::Pair,
        LessonState::Redraw,
        LessonState::Delay,
        LessonState::Decide,
        LessonState::Backup,
        LessonState::Blink,
        LessonState::Pop,
        LessonState::PopWait,
        LessonState::Compact,
        LessonState::Fall,
        LessonState::Wipe,
        LessonState::WipeWait,
        LessonState::Wiped,
    ];

    pub fn code(self) -> u32 {
        match self {
            LessonState::Setup => 0x133FA,
            LessonState::Ready => 0x1342A,
            LessonState::Next => 0x13442,
            LessonState::EndWait => 0x1345E,
            LessonState::End => 0x13464,
            LessonState::Garbage => 0x1348E,
            LessonState::Pair => 0x134AA,
            LessonState::Redraw => 0x134E0,
            LessonState::Delay => 0x134F8,
            LessonState::Decide => 0x134FE,
            LessonState::Backup => 0x1351E,
            LessonState::Blink => 0x1352E,
            LessonState::Pop => 0x1353A,
            LessonState::PopWait => 0x1354A,
            LessonState::Compact => 0x13550,
            LessonState::Fall => 0x13562,
            LessonState::Wipe => 0x135B0,
            LessonState::WipeWait => 0x135F6,
            LessonState::Wiped => 0x135FC,
        }
    }
}

/// The lessons' helpers: `loc_13686` / `loc_136DC` (the target column / rotation
/// follows an animation script), `loc_1373C` (after `time` frames, drop) and
/// `loc_138D2` (the same, counted the other way; `FindActorSlotQuick`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LessonHelper {
    Column { parent: usize, anim: Anim },
    Rotation { parent: usize, anim: Anim },
    Timer { parent: usize, time: u16 },
    Cue { parent: usize, time: u16 },
}

impl LessonHelper {
    pub fn code(&self) -> u32 {
        match self {
            LessonHelper::Column { .. } => 0x13686,
            LessonHelper::Rotation { .. } => 0x136DC,
            LessonHelper::Timer { .. } => 0x1373C,
            LessonHelper::Cue { .. } => 0x138D2,
        }
    }
}

/// `loc_FFBE`: a lesson's text, typed a character at a time straight into VRAM.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonText {
    pub parent: usize,
    /// +$32 the script, +$26 frames to wait, +$2A frames per character, +$12 the
    /// VRAM address, +$08 the typing sound, +$28 (unused).
    pub ptr: u32,
    pub wait: u16,
    pub pace: u16,
    pub at: u16,
    pub sound: u8,
    pub w28: u16,
}

/// `sub_13274` / `loc_132E4`: the CPU's joystick and button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LessonPad {
    pub button: bool,
    pub started: bool,
    pub anim: Anim,
}

impl LessonPad {
    pub fn code(&self) -> u32 {
        match (self.button, self.started) {
            (false, false) => 0x13274,
            (false, true) => 0x1329A,
            (true, false) => 0x132E4,
            (true, true) => 0x1330A,
        }
    }
}

impl Engine {
    pub(super) fn run_tutorial_routine(&mut self, at: u32) -> bool {
        match at {
            // loc_13330
            0x13330 => {
                for i in 0..0x100u32 {
                    let b = rom::byte(BEAN_ORDER + i);
                    self.bean_orders[0][i as usize] = b;
                    if b & 0x80 != 0 {
                        break;
                    }
                }
            }
            0x1339C => self.start_tutorial(),
            _ => return false,
        }
        true
    }

    /// `loc_1339C`.
    fn start_tutorial(&mut self) {
        self.score_at = SCORE_AT;
        self.lesson = 0;
        self.time_frames = 0;
        self.vscroll = [0; 40];
        self.hscroll = [0; 0x200];
        self.vdp_regs[0x0B] |= 4;
        if let Some(slot) = self.find_actor_slot() {
            let mut p = Player::new(0);
            p.flags = 0;
            p.state = PlayerState::Lesson(LessonState::Setup);
            self.slots[slot] = Some(Actor::Player(p));
        }
        self.queue_panel(PanelCommand::Text { entry: 1, blink: 0, player: 0 });
        // sub_3BB0
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Animator(Animator { started: false, anim: Anim::new(0), shown: 0 }));
        }
        // sub_1325C
        for button in [false, true] {
            if let Some(slot) = self.find_actor_slot() {
                self.slots[slot] = Some(Actor::LessonPad(LessonPad { button, started: false, anim: Anim::new(0) }));
            }
        }
    }

    /// `loc_133FA`.
    pub(super) fn run_lesson_player(&mut self, slot: usize, mut p: Player, state: LessonState, inputs: &Inputs) -> Result<Option<Player>, Unported> {
        use LessonState::*;
        let id = p.id as usize & 1;
        let next = |p: &mut Player, s: LessonState| p.state = PlayerState::Lesson(s);
        match state {
            Setup => {
                add_points(&mut p, 0); // sub_9C4A
                self.queue_panel(PanelCommand::Score(p.id));
                self.fields[id] = Field::default(); // ResetPuyoField
                self.reset_field_bytes(id);
                self.spawn_pair_holder(&mut p, 0xFF38); // loc_44EE: its preview off screen
                self.redraw_field(id);
                // (after the loads before it, the vertical blank that has been waiting
                // comes when interrupts are back on)
                self.long_step();
                next(&mut p, Ready);
            }
            Ready => {
                self.difficulty = 0;
                p.speed_index = 7;
                // loc_9CF8
                let &(speed, threshold) = SCENARIO_SPEEDS.get(usize::from(p.speed_index)).ok_or(Unported("speed step past unk_9DC4"))?;
                p.fall_speed = speed;
                p.threshold = threshold;
                next(&mut p, Next);
            }
            Next => return Ok(self.next_lesson(slot, p)),
            EndWait => {
                if wait(&mut p.delay) {
                    next(&mut p, End);
                }
            }
            End => {
                self.bytecode_flag = 0;
                self.bytecode_disabled = false;
                return Ok(None);
            }
            Garbage => {
                if p.flags & flag::GARBAGE_FALLING == 0 {
                    // loc_1349A: sub_4E24
                    if self.spawn_pair(slot, &mut p, inputs)? {
                        next(&mut p, Pair);
                    } else {
                        // loc_1358A: the lesson on running out of space is over.
                        self.popping[0] = true;
                        self.lesson = 5;
                        self.wipe_field(&mut p);
                        next(&mut p, Wipe);
                    }
                }
            }
            Pair => {
                if p.flags & (flag::PIVOT_ACTIVE | flag::SATELLITE_ACTIVE) == 0 {
                    self.count_pops(&mut p); // loc_134D0
                    next(&mut p, Redraw);
                } else if p.flags & flag::SOFT_DROP_POINT != 0 {
                    p.flags &= !flag::SOFT_DROP_POINT;
                    add_points(&mut p, 1); // sub_9C4A
                    self.queue_panel(PanelCommand::Score(p.id));
                }
            }
            Redraw => {
                self.redraw_field(id);
                p.delay = 2;
                next(&mut p, Delay);
            }
            Delay => {
                if wait(&mut p.delay) {
                    next(&mut p, Decide);
                }
            }
            Decide => {
                if p.w26 == 0 {
                    return Ok(self.next_lesson(slot, p));
                }
                self.start_pops(&mut p);
                self.score_step(&mut p);
                self.queue_panel(PanelCommand::Chain(p.id));
                next(&mut p, Backup);
            }
            Backup => {
                self.back_up_pops(&mut p);
                next(&mut p, Blink);
            }
            Blink => {
                if self.blink_pops(&mut p) {
                    next(&mut p, Pop);
                }
            }
            Pop => {
                self.spawn_pops(&p); // CheckPuyoPop
                p.delay = 0x18;
                next(&mut p, PopWait);
            }
            PopWait => {
                if wait(&mut p.delay) {
                    next(&mut p, Compact);
                }
            }
            Compact => {
                self.compact_pops(&mut p);
                next(&mut p, Fall);
            }
            Fall => {
                if p.flags & FALLING_AFTER_POP != 0 {
                    self.fall_commands.push(slot); // loc_4E14
                } else {
                    // loc_13572: sub_9BBA, the next chain step, then loc_134D0.
                    let points = if p.bonus == 0 { u32::from(p.base) } else { u32::from(p.base) * u32::from(p.bonus) };
                    add_points(&mut p, points);
                    p.chain = p.chain.saturating_add(1);
                    p.pop = None;
                    self.count_pops(&mut p);
                    next(&mut p, Redraw);
                }
            }
            Wipe => {
                if p.w26 == 0 {
                    // loc_135BA
                    self.fields[id] = Field::default();
                    self.reset_field_bytes(id);
                    self.redraw_field(id);
                    let first = usize::from(FIELD_ORIGIN[id] & 0x7F) / 2;
                    for i in 0..12 {
                        self.vscroll[(first + i) % 40] = 0;
                    }
                    p.delay = 0xE0;
                    next(&mut p, WipeWait);
                }
            }
            WipeWait => {
                if wait(&mut p.delay) {
                    next(&mut p, Wiped);
                }
            }
            Wiped => {
                self.queue_panel(PanelCommand::Wipe { player: 0, frame: 5 });
                return Ok(self.next_lesson(slot, p));
            }
        }
        Ok(Some(p))
    }

    /// $13442: the lessons are over, or the next pair's lesson and any garbage.
    fn next_lesson(&mut self, slot: usize, mut p: Player) -> Option<Player> {
        self.popping = [false, false];
        if self.lesson as u8 >= 9 {
            p.delay = 0x20;
            p.state = PlayerState::Lesson(LessonState::EndWait);
            return Some(p);
        }
        p.chain = 0;
        // sub_13610: each lesson's text, the first time.
        if self.lesson >> 8 == 0 {
            self.spawn_lesson_text(slot, u16::from(self.lesson as u8) + 7);
        }
        self.lesson_step(slot, &mut p);
        self.spawn_garbage(slot, &mut p);
        p.state = PlayerState::Lesson(LessonState::Garbage);
        Some(p)
    }

    /// `sub_1362C`: the lesson's moves for the next pair.
    fn lesson_step(&mut self, slot: usize, p: &mut Player) {
        let [count, lesson] = self.lesson.to_be_bytes();
        self.lesson = self.lesson & 0xFF00 | u16::from(lesson.wrapping_add(1));
        let drop_after = |e: &mut Engine, time: u16| {
            // loc_1371E
            if let Some(s) = e.find_actor_slot() {
                e.slots[s] = Some(Actor::LessonHelper(LessonHelper::Timer { parent: slot, time }));
            }
        };
        let phase = |p: &mut Player, v: u8| p.w26 = p.w26 & 0xFF00 | u16::from(v);
        match lesson {
            // Move: the column follows a script.
            0 => {
                if let Some(s) = self.find_actor_slot() {
                    self.slots[s] = Some(Actor::LessonHelper(LessonHelper::Column { parent: slot, anim: Anim::new(COLUMN_SCRIPT) }));
                }
            }
            // Turn.
            1 => {
                p.target.0 = 2;
                if let Some(s) = self.find_actor_slot() {
                    self.slots[s] = Some(Actor::LessonHelper(LessonHelper::Rotation { parent: slot, anim: Anim::new(ROTATION_SCRIPT) }));
                }
            }
            // Drop.
            2 => {
                p.target = (4, 3);
                drop_after(self, 0x80);
            }
            3 => {
                phase(p, 0);
                p.target = (3, 2);
                drop_after(self, 0x80);
            }
            // Fill the middle column until the field tops out (loc_1358A).
            4 => {
                phase(p, 0x80);
                self.lesson = u16::from_be_bytes([count.wrapping_add(1), 4]);
                p.target = (2, 0);
            }
            // A field ready for chains.
            5 => {
                phase(p, 0);
                self.fields[0] = Field::default(); // ResetPuyoField
                self.reset_field_bytes(0);
                let base = COLS * ROWS - 36;
                for i in 0..36 {
                    let b = rom::byte(LESSON_FIELD + i as u32) << 4;
                    self.field_bytes[0].cells[base + i] = b;
                    if b & 0x80 != 0 {
                        self.fields[0].set(i % COLS, (base + i) / COLS, BeanColor::from_id(b >> 4 & 7));
                    }
                }
                let mut q = p.clone();
                self.count_pops(&mut q); // sub_5960 (just for the links)
                self.redraw_field(0);
                p.target = (4, 0);
                drop_after(self, 0x80);
            }
            // loc_137FE: the chains, pair by pair.
            6 => {
                self.lesson = u16::from_be_bytes([count.wrapping_add(1), 6]);
                if count.wrapping_add(1) >= 0x1A {
                    self.lesson = 7;
                }
                let at = LESSON_6 + u32::from(count) * 4;
                p.target = (rom::byte(at), rom::byte(at + 1));
                phase(p, 0);
                if let Some(s) = self.free_slot() {
                    self.slots[s] = Some(Actor::LessonHelper(LessonHelper::Cue { parent: slot, time: u16::from(rom::byte(at + 2)) }));
                }
                let text = rom::byte(at + 3);
                if text & 0x80 == 0 {
                    self.spawn_lesson_text(slot, u16::from(text));
                }
            }
            // Garbage, then the last pairs.
            7 | 8 => {
                if lesson == 7 {
                    p.pending_garbage = 0x12;
                }
                // loc_138F6
                let n = count;
                let mut word = u16::from_be_bytes([n.wrapping_add(1), 8]);
                if n.wrapping_add(1) >= 4 {
                    word = word & 0xFF00 | 9;
                }
                self.lesson = word;
                let at = LESSON_8 + u32::from(n) * 2;
                p.target = (rom::byte(at), rom::byte(at + 1));
                phase(p, 0x80);
            }
            _ => {}
        }
    }

    /// `sub_FF4A`.
    fn spawn_lesson_text(&mut self, parent: usize, text: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        let entry = u32::from((text * 4).min(0x1C));
        self.slots[slot] = Some(Actor::LessonText(LessonText {
            parent,
            ptr: rom::long(TEXT_SCRIPTS + u32::from(text) * 4),
            wait: 0,
            pace: rom::word(TEXT_TIMES + entry + 2),
            at: 0,
            sound: TEXT_SOUND,
            w28: rom::word(TEXT_TIMES + entry),
        }));
    }

    /// `loc_FFBE`.
    pub(super) fn run_lesson_text(&mut self, mut t: LessonText) -> Option<LessonText> {
        if t.wait != 0 {
            t.wait -= 1;
            return Some(t);
        }
        let w = rom::word(t.ptr);
        t.ptr += 2;
        if w >> 8 != 0xFF {
            // sub_10058 (the typing sound every other character isn't ported)
            self.plane_writes.push((t.at, w ^ 0x8000));
            t.at = t.at.wrapping_add(2);
            self.lesson_tick = self.lesson_tick.wrapping_add(1);
            t.wait = t.pace;
            return Some(t);
        }
        let arg = rom::word(t.ptr);
        t.ptr += 2;
        match w & 0xFF {
            0x00 => return None,
            0x04 => t.wait = arg,
            0x08 => t.at = arg,
            0x0C => self.queue_panel(PanelCommand::List(arg)),
            0x10 => {
                if let Some(Actor::Player(p)) = &mut self.slots[t.parent] {
                    p.flags = arg as u8;
                }
            }
            _ => t.sound = arg as u8,
        }
        Some(t)
    }

    pub(super) fn run_lesson_helper(&mut self, h: LessonHelper) -> Option<LessonHelper> {
        match h {
            LessonHelper::Column { parent, mut anim } | LessonHelper::Rotation { parent, mut anim } => {
                let rotation = matches!(h, LessonHelper::Rotation { .. });
                if anim.step() {
                    return None;
                }
                if let Some(Actor::Player(p)) = &mut self.slots[parent] {
                    if rotation {
                        p.target.1 = anim.frame;
                    } else {
                        p.target.0 = anim.frame;
                    }
                }
                Some(if rotation { LessonHelper::Rotation { parent, anim } } else { LessonHelper::Column { parent, anim } })
            }
            LessonHelper::Timer { parent, time } => {
                let time = time.wrapping_sub(1);
                if time != 0 {
                    return Some(LessonHelper::Timer { parent, time });
                }
                self.lesson_drop(parent);
                None
            }
            LessonHelper::Cue { parent, time } => {
                if time != 0 {
                    return Some(LessonHelper::Cue { parent, time: time - 1 });
                }
                self.lesson_drop(parent);
                None
            }
        }
    }

    /// The parent's +$27: hold down.
    fn lesson_drop(&mut self, parent: usize) {
        if let Some(Actor::Player(p)) = &mut self.slots[parent] {
            p.w26 = p.w26 & 0xFF00 | 0x80;
        }
    }

    pub(super) fn run_lesson_pad(&mut self, mut pad: LessonPad) -> Option<LessonPad> {
        if !pad.started {
            pad.started = true;
            pad.anim = Anim::new(if pad.button { BUTTON_REST } else { STICK_REST });
            return Some(pad);
        }
        if pad.button {
            if self.cpu_pressed != 0 {
                pad.anim.ptr = BUTTON_PRESS;
                self.cpu_pressed = 0;
            }
        } else if self.cpu_lean != 0 {
            pad.anim.ptr = rom::long(STICK_LEANS + u32::from(self.cpu_lean & 0x7F));
            self.cpu_lean = 0;
        }
        pad.anim.step();
        Some(pad)
    }
}
