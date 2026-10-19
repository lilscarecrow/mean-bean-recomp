//! The actors around the two fields during a match: the next-pair holders and their
//! preview beans, the referee bean in the middle (its intro releases both players), the
//! start sparkles, the background animator, the opponent portrait and the "danger"
//! effect when the CPU's field fills up. Their drawing isn't ported; their timing,
//! slots and random draws are, since those shape the match.

use super::*;
use crate::mbm::rom;

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
}

pub const PREVIEW_BEAN_CODE: u32 = 0x45EA;

/// Preview scripts (`sub_4632`): a normal bean, the special pieces $19 and $1A+.
const PREVIEW_NORMAL: u32 = 0x465E;
const PREVIEW_SPECIAL_19: u32 = 0x89AE;
const PREVIEW_SPECIAL: u32 = 0x4682;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefereeState {
    /// $5BF8: the intro animation; on frame $17 (or at its end) the players start.
    Intro,
    /// $5C62: idle animations, chosen at random.
    Idle,
}

impl RefereeState {
    pub fn code(self) -> u32 {
        match self {
            RefereeState::Intro => 0x5BF8,
            RefereeState::Idle => 0x5C62,
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
    /// +$26 crowding level shown ($FFFF at first), +$28 offset into the opponent's list.
    pub level: u16,
    pub base: u16,
    /// +$22 frames left on this map, +$2C maps shown since the sequence restarted.
    pub wait: u8,
    pub shown: u8,
    /// +$32: next entry; the map last decoded (for drawing).
    pub next: u32,
    pub map: u32,
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
            _ => Err(Unported("exercise mode pairs (sub_43EA)")),
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
            RefereeState::Idle => {
                if self.referee_hit & 0x8000 != 0 {
                    return Err(Unported("referee knocked out (loc_5D0C)"));
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
        a.anim.step_with(&mut || self.random());
        if a.anim.frame != a.shown {
            a.shown = a.anim.frame; // QueuePlaneCmd $8A: drawing isn't ported
        }
        Some(a)
    }

    pub(super) fn run_danger(&mut self, mut d: Danger) -> Option<Danger> {
        if self.popping.iter().any(|&p| p) {
            // The match is over: cancel fades of line 3, restore its colors (not ported).
            self.pal_fade_gen[3] = self.pal_fade_gen[3].wrapping_add(1);
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
                self.fade_palette(3, 0, 8); // back to the opponent's colors
            }
            return Some(d);
        }
        let rate = FLASH_RATE[level];
        if rate & 0x8000 != 0 {
            self.danger_flash = 0;
        } else if rate < d.flash_wait {
            d.flash_wait = 0;
            self.fade_palette(3, 1, (3 & 7) + 1); // to white
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
        p.next = at + 4;
        Some(p)
    }

    /// `FadeToPalette` (8 steps) / `FadeToPal_StepCount` (`steps & 7` + 1 steps).
    fn fade_palette(&mut self, line: u16, speed: u8, steps: u16) {
        let Some(slot) = self.find_actor_slot() else { return };
        let line = line & 3;
        let generation = &mut self.pal_fade_gen[usize::from(line)];
        *generation = generation.wrapping_add(1);
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
        p.steps = p.steps.wrapping_sub(1);
        if p.steps == 0 {
            *generation = 0;
            return None;
        }
        Some(p)
    }

    pub(super) fn run_portrait_palette(&mut self, mut p: PortraitPalette) -> Option<PortraitPalette> {
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
        p.wait = if level == 3 {
            0
        } else {
            let (steps, wait) = if level == 0 { (0x0E, 6) } else { (4, 4) };
            let mut step = (p.step as u8).wrapping_add(1);
            if step == steps {
                step = 0;
            }
            p.step = (p.step & 0xFF00) | u16::from(step);
            wait
        };
        Some(p) // the palette color itself isn't ported
    }
}
