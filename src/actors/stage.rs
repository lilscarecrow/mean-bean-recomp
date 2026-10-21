//! The actors around the two fields during a match: the next-pair holders and their
//! preview beans, the referee bean in the middle (its intro releases both players), the
//! start sparkles, the background animator, the opponent portrait and the "danger"
//! effect when the CPU's field fills up. Their drawing isn't ported; their timing,
//! slots and random draws are, since those shape the match.

use super::*;
use crate::rom;

/// `nullsub_5`: a player's next pairs. +$26..+$2B hold three pairs (the next one
/// first); +$20's high byte is the place in the bean order; +$1E counts pairs dealt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairHolder {
    pub w20: u16,
    pub pairs: [u8; 6],
    pub dealt: u16,
    /// +$07: exercise mode's special-piece color base (`loc_4456`).
    pub b07: u8,
}

pub const PAIR_HOLDER_CODE: u32 = 0x45C8;

/// `loc_45EA`: one bean of the next-pair preview, showing `pairs[index]` of its holder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewBean {
    pub holder_slot: usize,
    /// +$07: 0 (pivot) or 1 (satellite).
    pub index: u8,
    /// +$08 color, +$36 color it last saw, +$06 draw flags.
    pub color: u8,
    pub last: u8,
    pub draw: u8,
    pub anim: Anim,
    /// Where it is drawn (+$0A, +$0E; set when spawned).
    pub x: u16,
    pub y: u16,
}

pub const PREVIEW_BEAN_CODE: u32 = 0x45EA;

/// Preview scripts (`sub_4632`): a normal bean, the special pieces $19 and $1A+.
const PREVIEW_NORMAL: u32 = 0x465E;
const PREVIEW_SPECIAL_19: u32 = 0x89AE;
const PREVIEW_SPECIAL: u32 = 0x4682;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefereeState {
    /// $5BCE: just spawned (in versus mode it waits for both players to be ready).
    Spawned,
    /// $5BF8: the intro animation; on frame $17 (or at its end) the players start.
    Intro,
    /// $5C62: idle animations, chosen at random.
    Idle,
    /// A $19 piece takes its seat (`loc_5D0C`): $5D2C until thrown off, $5D42 away,
    /// $5D58 until back.
    Thrown,
    Away,
    Back,
}

impl RefereeState {
    pub fn code(self) -> u32 {
        match self {
            RefereeState::Spawned => 0x5BCE,
            RefereeState::Intro => 0x5BF8,
            RefereeState::Idle => 0x5C62,
            RefereeState::Thrown => 0x5D2C,
            RefereeState::Away => 0x5D42,
            RefereeState::Back => 0x5D58,
        }
    }
}

/// `loc_5BCE` (spawned by `sub_5B54`): the bean between the fields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Referee {
    pub state: RefereeState,
    /// +$2E: P1's player actor (P2's is linked from it) until the first idle animation,
    /// then the script the current animation repeats from.
    pub link: u32,
    pub anim: Anim,
    /// +$06 draw flags, +$07 flags (bit 0 bobbing, bit 1 alternate idle).
    pub draw: u8,
    pub flags: u8,
    /// +$0A, +$0E (words), +$20 resting y.
    pub x: u16,
    pub y: u16,
    pub base_y: u16,
    /// +$12: set by the walking idles (not applied by this actor).
    pub vx: u32,
    /// +$26 repeats left, +$28 bob counter, +$36 frame last seen.
    pub repeats: u16,
    pub bob: u16,
    pub last_frame: u8,
}

/// Idle scripts by number (`off_3334`); each starts with a repeat count word.
const IDLE_SCRIPTS: u32 = 0x3334;
/// `sub_5EE8`: every other idle is this one.
const IDLE_ALTERNATE: u16 = 0x1E;
/// Sideways steps for frames $41.. (`unk_5D04`).
const REFEREE_STEPS: [u16; 4] = [0xFFFE, 0x0002, 0xFFFC, 0x0004];

/// `sub_602C`: a start sparkle (`sub_5FAA`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Sparkle {
    pub motion: Motion,
    pub draw: u8,
    /// +$26: counts down (nothing reads it).
    pub timer: u16,
}

pub const SPARKLE_CODE: u32 = 0x602C;

/// `sub_3BBA`: a background tile animation (`QueuePlaneCmd $8A`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Animator {
    /// False at $3BBA (just spawned).
    pub started: bool,
    pub anim: Anim,
    /// +$08: the frame last sent to the plane.
    pub shown: u8,
}

pub const ANIMATOR_CODE: u32 = 0x3BCC;

/// `sub_12374` (scenario mode): the CPU's field is filling up. Sparks and a red flash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Danger {
    /// +$26 frames since the last spark, +$28 since the last flash, +$2A flash left.
    pub spark_wait: u16,
    pub flash_wait: u16,
    pub flash: u16,
}

pub const DANGER_CODE: u32 = 0x12374;
/// Frames between sparks / flashes by the CPU's beans / 8 ($FFFF: none).
const SPARK_RATE: [u16; 9] = [0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 3, 2, 1];
const FLASH_RATE: [u16; 9] = [0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0xFFFF, 0x60, 0x30];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DangerSparkState {
    Start,
    Fly,
    Fade,
}

impl DangerSparkState {
    pub fn code(self) -> u32 {
        match self {
            DangerSparkState::Start => 0x1251C,
            DangerSparkState::Fly => 0x1252E,
            DangerSparkState::Fade => 0x1254E,
        }
    }
}

/// `loc_1251C`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DangerSpark {
    pub state: DangerSparkState,
    pub motion: Motion,
    pub draw: u8,
    pub anim: Anim,
}

const DANGER_SPARK_FLY: u32 = 0x12510;
const DANGER_SPARK_FADE: u32 = 0x12516;

/// `sub_60F4`: the opponent's portrait, a sequence of tile maps per crowding level
/// (`OpponentMappings`), each shown for a number of frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Portrait {
    /// False at $60F4 (just spawned).
    pub started: bool,
    /// +$26 crowding level shown ($FFFF at first), +$28 offset into the opponent's list.
    pub level: u16,
    pub base: u16,
    /// +$22 frames left on this map, +$2C maps shown since the sequence restarted.
    pub wait: u8,
    pub shown: u8,
    /// +$32: next entry; the map last decoded (for drawing).
    pub next: u32,
    pub map: u32,
    /// +$2A | +$0A: the maps' tile base and plane; +$0C: where on it.
    pub tile: u16,
    pub at: u16,
}

pub const PORTRAIT_CODE: u32 = 0x60FE;
const OPPONENT_MAPPINGS: u32 = 0x6256;

/// `ActPalFade` (`FadeToPalette`, `FadeToPal_StepCount`): fades a palette line over
/// a few steps. A newer fade of the same line (its generation counter moved on) ends
/// it. The colors themselves aren't ported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaletteFade {
    /// pfLine (+$08), pfFlag (+$12: the line's generation when it started), pfTimer
    /// (+$26), pfSpeed (+$28: frames per step - 1), pfSteps (+$2A).
    pub line: u16,
    pub flag: u16,
    pub timer: u16,
    pub speed: u16,
    pub steps: u16,
}

pub const PALETTE_FADE_CODE: u32 = 0x1068;

/// `loc_6F38`: cycles one palette color of the portrait by crowding level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortraitPalette {
    /// False at $6F38 (just spawned: it waits a frame).
    pub started: bool,
    pub wait: u8,
    pub step: u16,
    pub level: u8,
}

pub const PORTRAIT_PALETTE_CODE: u32 = 0x6F3E;

/// Address of an actor slot in the game's RAM, and back.
pub fn actor_addr(slot: usize) -> u32 {
    0xFF_E000 + slot as u32 * 0x40
}

pub fn actor_slot(addr: u32) -> usize {
    (addr.wrapping_sub(0xFF_E000) / 0x40) as usize
}

impl Engine {
    /// `RandomBound`.
    pub(super) fn random_below(&mut self, bound: u16) -> u16 {
        ((u32::from(self.random()) * u32::from(bound)) >> 16) as u16
    }

    /// `sub_43B8` (via `sub_4E24`): take the next pair from the player's holder and
    /// deal a new one in behind it.
    pub(super) fn deal_pair(&mut self, holder_slot: usize, p: &Player) -> Result<(u8, u8), Unported> {
        let Some(Actor::PairHolder(mut h)) = self.slots[holder_slot].take() else {
            return Err(Unported("pair holder missing"));
        };
        let next = (h.pairs[0], h.pairs[1]);
        h.pairs.copy_within(2..6, 0);
        let (a, b) = self.new_pair(&mut h, p)?;
        h.pairs[4] = a;
        h.pairs[5] = b;
        h.dealt = h.dealt.wrapping_add(1);
        self.slots[holder_slot] = Some(Actor::PairHolder(h));
        Ok(next)
    }

    /// `sub_43EA`: the pair after next, by mode: the bean order (scenario, versus) or
    /// random colors (exercise).
    fn new_pair(&mut self, h: &mut PairHolder, p: &Player) -> Result<(u8, u8), Unported> {
        match self.level_mode & 3 {
            mode @ (0 | 1) => {
                let which = if mode == 0 { p.id != 0 } else { p.speed_level != 0 };
                let order = &self.bean_orders[usize::from(which)];
                let i = usize::from((h.w20 >> 8) as u8);
                let pair = (order[i], order[(i + 1) & 0xFF]);
                h.w20 = h.w20.wrapping_add(0x200);
                Ok(pair)
            }
            _ => Ok(self.exercise_pair(h, p.speed_level)),
        }
    }

    pub(super) fn run_preview_bean(&mut self, mut v: PreviewBean) -> Option<PreviewBean> {
        let Some(Actor::PairHolder(h)) = &self.slots[v.holder_slot] else {
            return Some(v); // the holder is gone (not seen during a match)
        };
        let color = h.pairs[usize::from(v.index)];
        v.color = color;
        if color != v.last {
            v.last = color;
            v.anim.time = 0;
            v.anim.ptr = match color {
                0x19 => PREVIEW_SPECIAL_19,
                c if c > 0x19 => PREVIEW_SPECIAL,
                _ => PREVIEW_NORMAL,
            };
        }
        v.draw = 0x80;
        if v.index != 0 && v.color >= 0x19 {
            v.draw = 0;
        }
        v.anim.step_with(&mut || self.random());
        Some(v)
    }

    pub(super) fn run_referee(&mut self, mut r: Referee, inputs: &Inputs) -> Result<Option<Referee>, Unported> {
        match r.state {
            RefereeState::Spawned => {
                if self.level_mode == 1 {
                    // loc_5BCE: until both players have picked their level.
                    let p1_slot = actor_slot(r.link);
                    let p2_slot = self.player_link(p1_slot);
                    let flags = |e: &Engine, s: usize| match &e.slots[s] {
                        Some(Actor::Player(p)) => p.flags,
                        _ => 0,
                    };
                    if (flags(self, p1_slot) | flags(self, p2_slot)) & 1 != 0 {
                        return Ok(Some(r));
                    }
                }
                r.state = RefereeState::Intro;
            }
            RefereeState::Intro => {
                let done = r.anim.step_with(&mut || self.random());
                if !done {
                    if (inputs.pads[0].hold | inputs.pads[1].hold) & 0xF0 != 0 {
                        r.anim.time = 0; // a button skips ahead
                    }
                    if r.anim.frame != 0x17 {
                        return Ok(Some(r));
                    }
                    // loc_5C24: sound $63; sparkles unless much garbage is waiting.
                    let p1_slot = actor_slot(r.link);
                    let p2_slot = self.player_link(p1_slot);
                    let pending = self.player_mut(p1_slot).pending_garbage.wrapping_add(self.player_mut(p2_slot).pending_garbage);
                    if pending < 0x25 {
                        self.spawn_sparkles(&r);
                    }
                }
                // loc_5C4A: let both players start.
                let p1_slot = actor_slot(r.link);
                let p2_slot = self.player_link(p1_slot);
                self.player_mut(p1_slot).flags &= !2;
                self.player_mut(p2_slot).flags &= !2;
                r.state = RefereeState::Idle;
            }
            RefereeState::Thrown => {
                if self.referee_hit & 0xFF == 0 {
                    r.draw &= 0x7F;
                    r.state = RefereeState::Away;
                }
            }
            RefereeState::Away => {
                if self.referee_hit & 0xFF != 0 {
                    r.draw |= 0x80;
                    r.state = RefereeState::Back;
                }
            }
            RefereeState::Back => {
                if self.referee_hit & 0xFF == 0 {
                    r.state = RefereeState::Idle; // loc_5C5E
                }
            }
            RefereeState::Idle => {
                if self.referee_hit & 0x8000 != 0 {
                    // loc_5D0C
                    let me = self.current_slot;
                    if self.throw_referee(me) {
                        r.state = RefereeState::Thrown;
                    }
                    return Ok(Some(r));
                }
                if self.player_flags[0] & self.player_flags[1] & 0x80 != 0 {
                    return Err(Unported("match over (loc_5E86)"));
                }
                // sub_5F6E: bob after landing.
                if r.flags & 1 != 0 {
                    let angle = ((r.bob as u8) << 2) | 0x80;
                    let dy = (super::sin(angle, 0x1800) >> 16) as u16;
                    r.y = r.base_y.wrapping_add(dy);
                    r.bob = r.bob.wrapping_sub(1);
                    if r.bob & 0x8000 != 0 {
                        r.flags &= !1;
                    }
                }
                if r.anim.step_with(&mut || self.random()) {
                    if r.repeats != 0 {
                        r.repeats -= 1;
                        r.anim.ptr = r.link;
                    } else {
                        // sub_5EE8, sub_5F26
                        r.flags ^= 2;
                        let n = if r.flags & 2 != 0 {
                            IDLE_ALTERNATE
                        } else if self.level_mode & 3 == 0 {
                            self.random_below(0x16) + 8
                        } else {
                            self.random_below(0x1E)
                        };
                        if n < 4 {
                            r.vx = if n & 1 != 0 { 0xFFFF_8000 } else { 0x8000 };
                            r.draw = 0x82;
                        }
                        let script = rom::long(IDLE_SCRIPTS + u32::from(n) * 4);
                        r.repeats = rom::word(script);
                        r.anim.ptr = script + 2;
                        r.link = script + 2;
                    }
                }
                // loc_5CC2: frames $40.. are cues: land (bob) or step sideways.
                let frame = r.anim.frame;
                if frame != r.last_frame {
                    r.last_frame = frame;
                    if frame == 0x40 {
                        r.flags |= 1;
                        r.bob = 0x20;
                    } else if frame > 0x40 {
                        let step = REFEREE_STEPS.get(usize::from(frame - 0x41)).ok_or(Unported("referee step cue"))?;
                        r.x = r.x.wrapping_add(*step).clamp(0x108, 0x138);
                    }
                }
            }
        }
        Ok(Some(r))
    }

    /// The other player of a linked pair (player actors' +$2E).
    fn player_link(&mut self, slot: usize) -> usize {
        self.player_mut(slot).other_slot
    }

    /// `sub_5FAA`: 32 sparkles fly out from the referee; the match clock restarts.
    fn spawn_sparkles(&mut self, r: &Referee) {
        self.time_frames = 0;
        for i in (0..32u8).rev() {
            let Some(slot) = self.free_slot() else { continue };
            let angle = i << 3;
            let speed = self.random_below(0xC0) + 0x280;
            let motion = Motion {
                x: u32::from(r.x) << 16,
                y: u32::from(r.y) << 16,
                vx: super::sin(angle, speed),
                vy: super::cos(angle.wrapping_add(5), speed),
                ay: 0x2000,
                ty: 0xFFFF,
                ..Motion::default()
            };
            self.slots[slot] = Some(Actor::Sparkle(Sparkle { motion, draw: 0x83, timer: 0x14 }));
        }
    }

    pub(super) fn run_sparkle(&mut self, mut s: Sparkle) -> Option<Sparkle> {
        if s.motion.step(s.draw) {
            return None;
        }
        s.timer = s.timer.wrapping_sub(1);
        Some(s)
    }

    pub(super) fn run_animator(&mut self, mut a: Animator) -> Option<Animator> {
        if !a.started {
            a.started = true;
            a.shown = 0xFF;
            a.anim.ptr = 0x3BF2; // byte_3BF2
            return Some(a);
        }
        a.anim.step_with(&mut || self.random());
        if a.anim.frame != a.shown {
            a.shown = a.anim.frame; // QueuePlaneCmd $8A: drawing isn't ported
        }
        Some(a)
    }

    pub(super) fn run_danger(&mut self, mut d: Danger) -> Option<Danger> {
        if self.popping.iter().any(|&p| p) {
            // loc_1239C: the match is over: cancel fades of line 3, restore its colors.
            self.pal_fade_gen[3] = self.pal_fade_gen[3].wrapping_add(1);
            let colors = self.opponent_palette();
            self.load_palette(3, PaletteLoad::Colors(colors));
            return None;
        }
        d.spark_wait = d.spark_wait.wrapping_add(1) & 0x7FFF;
        d.flash_wait = d.flash_wait.wrapping_add(1) & 0x7FFF;
        let level = usize::from((self.ai_fields[1].count() >> 3).min(8));
        // sub_12460
        if SPARK_RATE[level] < d.spark_wait {
            d.spark_wait = 0;
            if let Some(slot) = self.free_slot() {
                let r = self.random();
                let speed = u16::from(r as u8 & 0x7F) + 0x240;
                let angle = (r as u8 & 0x5F).wrapping_add(0x90);
                let vy = super::sin(angle, speed);
                let vx = super::cos(angle, speed);
                let motion = Motion {
                    x: u32::from(((vx << 4) >> 16) as u16).wrapping_add(0x120) << 16,
                    y: u32::from(((vy << 4) >> 16) as u16).wrapping_add(0x110) << 16,
                    vx,
                    vy,
                    ay: 0x1800,
                    ty: 0xFFFF,
                    ..Motion::default()
                };
                let spark = DangerSpark { state: DangerSparkState::Start, motion, draw: 0, anim: Anim::new(DANGER_SPARK_FLY) };
                self.slots[slot] = Some(Actor::DangerSpark(spark));
            }
        }
        // sub_123AE
        self.danger_flash = 1;
        if d.flash != 0 {
            d.flash -= 1;
            if d.flash == 0 {
                let colors = self.opponent_palette();
                self.fade_palette(3, 0, 8, &colors); // back to the opponent's colors
            }
            return Some(d);
        }
        let rate = FLASH_RATE[level];
        if rate & 0x8000 != 0 {
            self.danger_flash = 0;
        } else if rate < d.flash_wait {
            d.flash_wait = 0;
            self.fade_palette(3, 1, (3 & 7) + 1, &table_palette(PAL_WHITE)); // to white
            d.flash = 0x26;
            self.danger_flash = 0; // falls through into loc_12444
        }
        Some(d)
    }

    pub(super) fn run_danger_spark(&mut self, mut s: DangerSpark) -> Option<DangerSpark> {
        match s.state {
            DangerSparkState::Start => {
                s.anim.step();
                s.draw = 0x87;
                s.state = DangerSparkState::Fly;
            }
            DangerSparkState::Fly => {
                s.motion.step(s.draw);
                if s.anim.step() {
                    s.anim.ptr = DANGER_SPARK_FADE;
                    s.state = DangerSparkState::Fade;
                }
            }
            DangerSparkState::Fade => {
                if s.anim.step() {
                    return None;
                }
            }
        }
        Some(s)
    }

    pub(super) fn run_portrait(&mut self, mut p: Portrait) -> Option<Portrait> {
        if !p.started {
            p.started = true;
            p.level = 0xFFFF;
            return Some(p);
        }
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        let maps = rom::long(OPPONENT_MAPPINGS + u32::from(self.opponent) * 4);
        if self.crowding != p.level {
            p.level = self.crowding;
            p.next = rom::long(maps + u32::from(self.crowding.wrapping_add(p.base)) * 4);
            p.wait = 0;
        }
        if p.wait != 0 {
            p.wait -= 1;
            return Some(p);
        }
        let mut at = p.next;
        let mut time = rom::word(at);
        at += 2;
        if time & 0x8000 != 0 {
            at = rom::long(maps + u32::from(p.level) * 4);
            time = rom::word(at);
            at += 2;
            p.shown = 0xFF;
        }
        p.shown = p.shown.wrapping_add(1);
        p.wait = time as u8;
        p.map = rom::long(at);
        self.eni_dec(p.map, p.tile, 9, 6, p.at); // queued for the vertical blank
        p.next = at + 4;
        Some(p)
    }

    /// `FadeToPalette` (8 steps) / `FadeToPal_StepCount` (`steps & 7` + 1 steps).
    pub(super) fn fade_palette(&mut self, line: u16, speed: u8, steps: u16, target: &[u16; 16]) {
        let Some(slot) = self.find_actor_slot() else { return };
        let line = line & 3;
        let generation = &mut self.pal_fade_gen[usize::from(line)];
        *generation = generation.wrapping_add(1);
        self.start_fade(usize::from(line), target);
        let generation = &self.pal_fade_gen[usize::from(line)];
        let fade = PaletteFade { line, flag: *generation, timer: 0, speed: u16::from(speed), steps };
        self.slots[slot] = Some(Actor::PaletteFade(fade));
    }

    pub(super) fn run_palette_fade(&mut self, mut p: PaletteFade) -> Option<PaletteFade> {
        let generation = &mut self.pal_fade_gen[usize::from(p.line)];
        if *generation != p.flag {
            return None;
        }
        p.timer = p.timer.wrapping_add(1);
        if p.speed >= p.timer {
            return Some(p);
        }
        p.timer = 0;
        self.fade_step(usize::from(p.line));
        let generation = &mut self.pal_fade_gen[usize::from(p.line)];
        p.steps = p.steps.wrapping_sub(1);
        if p.steps == 0 {
            *generation = 0;
            return None;
        }
        Some(p)
    }

    pub(super) fn run_portrait_palette(&mut self, mut p: PortraitPalette) -> Option<PortraitPalette> {
        if !p.started {
            p.started = true;
            return Some(p);
        }
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        if self.danger_flash != 0 {
            return Some(p);
        }
        p.wait = p.wait.wrapping_sub(1);
        if p.wait != 0 {
            return Some(p);
        }
        let level = self.crowding as u8;
        if level != p.level {
            p.step = 0;
            p.level = level;
        }
        let color_at;
        p.wait = if level == 3 {
            color_at = ARMS_COLORS[2];
            0
        } else {
            let (steps, wait) = if level == 0 { (0x0E, 6) } else { (4, 4) };
            let mut step = (p.step as u8).wrapping_add(1);
            if step == steps {
                step = 0;
            }
            p.step = (p.step & 0xFF00) | u16::from(step);
            color_at = ARMS_COLORS[usize::from(level != 0)] + u32::from(step) * 2;
            wait
        };
        // loc_6FBA: the portrait's color 14 (line 2's while word_FF1124 is set).
        let color = rom::word(color_at);
        if self.portrait_lock != 0 {
            self.set_palette_color(2, 14, color);
        } else {
            self.set_palette_color(3, 14, color);
        }
        Some(p)
    }
}

/// `loc_685E` (Frankly's stage, six of them): while the player is crowded (crowding 1),
/// a sprite that hops between places of a table, pausing between hops.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BgSlide {
    pub draw: u8,
    /// +$09: its sprite frame (from the spawn table).
    pub frame: u8,
    /// +$22 steps left of this hop, +$26 pause before it, +$28 place (byte offset into
    /// `unk_690A`), +$2A/+$2C step per frame, +$32 pause index (`unk_6902`, 8 long).
    pub steps: u8,
    pub pause: u8,
    pub place: u8,
    pub dx: u16,
    pub dy: u16,
    pub phase: u8,
    pub x: u16,
    pub y: u16,
}

pub const BG_SLIDE_CODE: u32 = 0x685E;
const BG_SLIDE_PAUSES: u32 = 0x6902;
const BG_SLIDE_PLACES: u32 = 0x690A;

/// `loc_6AFE` (opponent 14's stage): a sprite that runs back and forth while nobody is
/// crowded, animates when the CPU is, and throws sparks on cue from the portrait while
/// the player is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BgRunner {
    /// +$2E: the portrait it watches.
    pub portrait: usize,
    pub draw: u8,
    /// +$26 spark latch, +$28 direction, +$2A pause, +$2C crowding last seen.
    pub latch: u8,
    pub left: u8,
    pub pause: u8,
    pub level: u8,
    pub x: u16,
    pub y: u16,
    /// Its animation; while running, +$22 (the animation's time) counts frames and
    /// +$09 (its frame) flips between two.
    pub anim: Anim,
}

pub const BG_RUNNER_CODE: u32 = 0x6AFE;
const BG_RUNNER_SCRIPT: u32 = 0x6CAC;
/// `byte_6D6E`: the three sparks' steps and start x (dx, dy, x); they start at y $E2.
const BG_SPARK_STEPS: [(u16, u16, u16); 3] = [(0x0002, 0xFFFE, 0x0125), (0x0000, 0xFFFC, 0x011B), (0xFFFE, 0xFFFE, 0x0111)];

/// `loc_6D50`: a spark from `BgRunner`, moving a fixed step for 9 frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BgSpark {
    pub frame: u8,
    pub left: u8,
    pub dx: u16,
    pub dy: u16,
    pub x: u16,
    pub y: u16,
}

pub const BG_SPARK_CODE: u32 = 0x6D50;

/// `ActShakeField` (`PuyoLandEffects`, Frankly's and Dragon Breath's stages): P2's
/// field shakes when its pair touches down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShakeField {
    /// +$36 angle, +$38 strength, +$32 its vertical scroll entries.
    pub angle: u8,
    pub strength: u16,
    pub scroll: u32,
}

pub const SHAKE_FIELD_CODE: u32 = 0x397A;
/// `vscroll_buffer`'s RAM address (`ShakeField::scroll` is one).
pub const VSCROLL_BUFFER: u32 = 0xFF_05D2;
const OPP_FRANKLY: u8 = 1;
const OPP_DRAGON: u8 = 10;

impl Engine {
    pub(super) fn run_bg_slide(&mut self, mut b: BgSlide) -> Option<BgSlide> {
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        if self.crowding != 1 {
            b.draw = 0;
            b.steps = 0;
            return Some(b);
        }
        if b.steps != 0 {
            if b.pause != 0 {
                b.pause -= 1;
                return Some(b);
            }
            b.x = b.x.wrapping_add(b.dx);
            b.y = b.y.wrapping_add(b.dy);
            b.steps -= 1;
            b.draw = 0x80;
            return Some(b);
        }
        let at = BG_SLIDE_PLACES + u32::from(b.place);
        b.x = rom::word(at);
        b.y = rom::word(at + 2);
        b.steps = 0x0A;
        b.pause = rom::byte(BG_SLIDE_PAUSES + u32::from(b.phase));
        b.phase = (b.phase + 1) & 7;
        b.draw = 0;
        if self.portrait_lock != 0 {
            b.x = b.x.wrapping_sub(0x58);
            b.y = b.y.wrapping_sub(0xF8);
        }
        Some(b)
    }

    pub(super) fn run_bg_runner(&mut self, mut r: BgRunner) -> Option<BgRunner> {
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        let level = self.crowding as u8;
        let alt = self.portrait_lock != 0;
        if level != r.level {
            r.level = level;
            match level & 3 {
                0 => {
                    r.anim.frame = 0;
                    (r.latch, r.left, r.pause) = (0, 0, 0);
                    r.x = 0x102;
                    r.y = 0xE6;
                    // (with word_FF1124 set the ROM adjusts the wrong pointer: no effect)
                }
                2 => {
                    r.anim.time = 0;
                    r.anim.ptr = BG_RUNNER_SCRIPT; // (the frame stays)
                    r.x = 0x12B;
                    r.y = 0xFA;
                }
                _ => {
                    if let Some(Actor::Portrait(p)) = self.slots[r.portrait].as_mut() {
                        p.shown = 0;
                    }
                    r.draw = 0;
                }
            }
            return Some(r);
        }
        match level & 3 {
            0 => {
                if r.pause != 0 {
                    r.pause -= 1;
                    r.draw = 0;
                    return Some(r);
                }
                r.draw = 0x80;
                r.anim.time += 1;
                if r.anim.time == 3 {
                    r.anim.frame ^= 1;
                    r.anim.time = 0;
                }
                let (lo, hi): (i16, i16) = if alt { (0xAC, 0xC4) } else { (0x104, 0x11C) };
                if r.left != 0 {
                    if (r.x as i16) > lo {
                        r.x = r.x.wrapping_sub(6);
                        return Some(r);
                    }
                } else if (r.x as i16) <= hi {
                    r.x = r.x.wrapping_add(6);
                    return Some(r);
                }
                // loc_6C3C: pause, then start again from one end.
                r.draw = 0;
                let n = self.random() as u8;
                r.pause = n & 0x3F;
                if n & 1 != 0 {
                    r.x = hi as u16;
                    r.left = 1;
                } else {
                    r.x = lo as u16;
                    r.left = u8::from(alt);
                }
            }
            1 => {
                let shown = match &self.slots[r.portrait] {
                    Some(Actor::Portrait(p)) => p.shown,
                    _ => 0,
                };
                if shown != 2 && shown != 8 {
                    r.latch = 0;
                } else if r.latch == 0 {
                    r.latch = 1;
                    let mut next = BG_SPARK_STEPS.iter();
                    for n in (0..3u8).rev() {
                        let Some(slot) = self.find_actor_slot() else { continue };
                        let &(dx, dy, x) = next.next().unwrap();
                        let (mut x, mut y, mut frame) = (x, 0xE2u16, 2 + n);
                        if alt {
                            (x, y, frame) = (x.wrapping_sub(0x58), y.wrapping_sub(0xF8), frame + 8);
                        }
                        self.slots[slot] = Some(Actor::BgSpark(BgSpark { frame, left: 0x0A, dx, dy, x, y }));
                    }
                }
            }
            2 => {
                r.draw = 0x80;
                r.anim.step_with(&mut || self.random());
            }
            _ => {}
        }
        Some(r)
    }

    pub(super) fn run_bg_spark(&mut self, mut s: BgSpark) -> Option<BgSpark> {
        s.left = s.left.wrapping_sub(1);
        if s.left == 0 {
            return None;
        }
        s.x = s.x.wrapping_add(s.dx);
        s.y = s.y.wrapping_add(s.dy);
        Some(s)
    }

    /// `PuyoLandEffects`: on two stages, P2's pair touching down shakes its field.
    pub(super) fn land_effects(&mut self, player_id: u8) {
        if !matches!(self.opponent, OPP_FRANKLY | OPP_DRAGON) || player_id == 0 {
            return;
        }
        let Some(slot) = self.free_slot() else { return };
        self.slots[slot] = Some(Actor::ShakeField(ShakeField { angle: 0, strength: 0x400, scroll: VSCROLL_BUFFER + 0x34 }));
    }

    pub(super) fn run_shake_field(&mut self, mut s: ShakeField) -> Option<ShakeField> {
        // Plane A of 6 columns from `scroll` (plane B's words cleared).
        let first = (s.scroll.wrapping_sub(VSCROLL_BUFFER) / 2) as usize;
        let mut angle = s.angle;
        for col in 0..6 {
            let at = (first + col * 2) % 40;
            self.vscroll[at] = (sin(angle & 0x7F, s.strength) >> 16) as u16;
            self.vscroll[(at + 1) % 40] = 0;
            angle = angle.wrapping_add(0x48);
        }
        s.angle = s.angle.wrapping_add(0x28);
        let (left, borrow) = s.strength.overflowing_sub(0x20);
        s.strength = left;
        (!borrow).then_some(s)
    }
}

/// `loc_6954` (Coconuts' stage, `loc_6946`): the portrait's colors 10 and 12 by
/// crowding level: steady at 0, a slow pulse at 1 (`word_6A66`, 18 steps every other
/// frame), a quick one at 2 (`word_6A4E`, 6 steps every 4 frames), the plain palette
/// at 3.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CoconutsLight {
    /// False at $6954 (just spawned).
    pub started: bool,
    /// +$2A crowding level seen, +$26 step (level 2 counts in its high byte), +$22 wait.
    pub level: u8,
    pub counter: u16,
    pub wait: u8,
}

pub const COCONUTS_LIGHT_CODE: u32 = 0x695A;
const COCONUTS_STEADY: u32 = 0x6A4A;
const COCONUTS_QUICK: u32 = 0x6A4E;
const COCONUTS_SLOW: u32 = 0x6A66;

/// `loc_6DCE` (Sir Ffuzzy's stage, `loc_6D80`): his eyes, a sprite shown while the
/// player is crowded (level 2): open for a random while, then a quick blink.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfuzzyEyes {
    /// +$06 draw flags, +$09 frame, +$22 wait, +$26 blink counter.
    pub draw: u8,
    pub frame: u8,
    pub wait: u8,
    pub counter: u16,
}

pub const FFUZZY_EYES_CODE: u32 = 0x6DCE;
/// Where the eyes are drawn (+$0A, +$0E) and their mappings (+$08).
pub const FFUZZY_EYES_AT: (u16, u16) = (0x110, 0xE8);
pub const FFUZZY_EYES_MAPPINGS: u8 = 0x2F;

/// `loc_6E46` (Sir Ffuzzy's stage): the portrait's colors 13 and 2, 8 steps with waits
/// by crowding level (`unk_6EE2`, `unk_6F02`; level 3: `unk_6F06`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FfuzzyPalette {
    /// False at $6E46 (just spawned).
    pub started: bool,
    /// +$22 wait, +$26 step.
    pub wait: u8,
    pub counter: u16,
}

pub const FFUZZY_PALETTE_CODE: u32 = 0x6E4C;
const FFUZZY_WAITS: u32 = 0x6EE2;
const FFUZZY_COLORS: u32 = 0x6F02;
const FFUZZY_CROWDED: u32 = 0x6F06;

impl Engine {
    /// Two colors of the portrait (line 2's while `word_FF1124` is set).
    fn portrait_colors(&mut self, colors: [(usize, u16); 2]) {
        let line = if self.portrait_lock != 0 { 2 } else { 3 };
        for (color, value) in colors {
            self.palettes[line][color] = value;
        }
        self.load_palette(line, PaletteLoad::Buffer);
    }

    pub(super) fn run_coconuts_light(&mut self, mut c: CoconutsLight) -> Option<CoconutsLight> {
        if !c.started {
            c.started = true;
            return Some(c);
        }
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        let level = self.crowding;
        if level as u8 != c.level {
            c.counter = 0;
            c.level = level as u8;
        }
        let at = match level {
            0 => COCONUTS_STEADY,
            1 => {
                if c.wait != 0 {
                    c.wait -= 1;
                    return Some(c);
                }
                c.wait = 1;
                c.counter = c.counter.wrapping_add(1);
                if c.counter == 0x12 {
                    c.counter = 0;
                }
                COCONUTS_SLOW + u32::from(c.counter) * 4
            }
            2 => {
                if self.danger_flash != 0 {
                    return Some(c);
                }
                let mut step = ((c.counter >> 8) as u8).wrapping_add(1);
                c.counter = (c.counter & 0xFF) | u16::from(step) << 8;
                if step & 3 != 0 {
                    return Some(c);
                }
                step &= 0x1C;
                if step == 0x18 {
                    step = 0;
                    c.counter &= 0xFF;
                }
                COCONUTS_QUICK + u32::from(step)
            }
            _ => {
                // sub_61C0
                let colors = self.opponent_palette();
                self.load_palette(3, PaletteLoad::Colors(colors));
                return Some(c);
            }
        };
        self.portrait_colors([(10, rom::word(at)), (12, rom::word(at + 2))]);
        Some(c)
    }

    pub(super) fn run_ffuzzy_eyes(&mut self, mut e: FfuzzyEyes) -> Option<FfuzzyEyes> {
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        if self.crowding != 2 {
            e.draw = 0;
            e.wait = 0x32;
            e.frame = 0;
            return Some(e);
        }
        e.draw = 0x80;
        e.wait = e.wait.wrapping_sub(1);
        if e.wait != 0 {
            return Some(e);
        }
        e.counter = e.counter.wrapping_add(1);
        let step = e.counter & 3;
        e.wait = if step == 0 {
            e.frame = 0;
            self.random() as u8 & 0x3F
        } else if step & 1 != 0 {
            e.frame = 1;
            6
        } else {
            e.frame = 2;
            5
        };
        Some(e)
    }

    pub(super) fn run_ffuzzy_palette(&mut self, mut p: FfuzzyPalette) -> Option<FfuzzyPalette> {
        if !p.started {
            p.started = true;
            return Some(p);
        }
        if self.portrait_lock & 0xFF00 != 0 {
            return None;
        }
        if self.danger_flash != 0 {
            return Some(p);
        }
        p.wait = p.wait.wrapping_sub(1);
        if p.wait != 0 {
            return Some(p);
        }
        let step = (p.counter as u8).wrapping_add(1) & 7;
        p.counter = (p.counter & 0xFF00) | u16::from(step);
        let level = self.crowding;
        let at = if level as u8 == 3 {
            FFUZZY_CROWDED
        } else {
            let i = u32::from(level << 3) + u32::from(step);
            p.wait = rom::byte(FFUZZY_WAITS + (i & 0xFFFF));
            FFUZZY_COLORS + u32::from(step) * 4
        };
        self.portrait_colors([(13, rom::word(at)), (2, rom::word(at + 2))]);
        Some(p)
    }
}
