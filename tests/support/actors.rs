//! Rebuilding the port's actors from the ROM's RAM, and describing both the same way
//! so they can be compared slot by slot.

use bean_machine::mbm::actors::*;
use bean_machine::mbm::anim::Anim;
use bean_machine::mbm::motion::Motion;
use bean_machine::mbm::pair::Pair;
use bean_machine::mbm::BeanColor;

use super::Ram;

pub const ICON_GEN: u32 = 0xFF_19B0;
pub const CHAINING: u32 = 0xFF_19B2;
pub const ICON_CODE: u32 = 0x9962;

pub fn icon_side(ram: &Ram, a: u32) -> u8 {
    (ram.u32(a + 0x32).wrapping_sub(ICON_GEN) & 1) as u8
}


pub const ACTORS: u32 = 0xFF_E000;

pub fn slot_addr(slot: usize) -> u32 {
    ACTORS + slot as u32 * 0x40
}

/// The slot an actor pointer points at, if it points into the actor table.
pub fn slot_at(addr: u32) -> Option<usize> {
    (ACTORS..ACTORS + 64 * 0x40).contains(&addr).then(|| slot_of(addr))
}

pub fn slot_of(addr: u32) -> usize {
    ((addr - ACTORS) / 0x40) as usize
}

/// A slot is taken when its first word is nonzero (`FindActorSlotQuick`'s test; only
/// `FindActorSlot` also sets bit 7 of the second byte).
pub fn used(ram: &Ram, slot: usize) -> bool {
    ram.u16(slot_addr(slot)) != 0
}

pub fn anim_from(ram: &Ram, a: u32) -> Anim {
    Anim { ptr: ram.u32(a + 0x32), time: ram.u8(a + 0x22), frame: ram.u8(a + 0x09) }
}

pub fn color(ram: &Ram, a: u32) -> BeanColor {
    BeanColor::from_id(ram.u8(a + 8)).unwrap()
}

pub fn fall_from(ram: &Ram, a: u32) -> Fall {
    Fall { y: ram.u32(a + 0x0E), vel: ram.u32(a + 0x16), accel: ram.u16(a + 0x1E), base: ram.u16(a + 0x20) }
}

/// Rebuilds a ported actor from RAM by its resume address; None if it isn't one.
pub fn load(ram: &Ram, slot: usize, tag: u8) -> Option<Actor> {
    let a = slot_addr(slot);
    if !used(ram, slot) {
        return None;
    }
    let code = ram.code(a);
    // Icons belong to no player (tag $FF) and behave the same whoever drew them.
    if code == ICON_CODE {
        return Some(Actor::Icon(Icon {
            side: icon_side(ram, a),
            generation: ram.u8(a + 0x28),
            frame: ram.u8(a + 9),
            motion: Motion { x: ram.u32(a + 0x0A), y: ram.u32(a + 0x0E), vx: ram.u32(a + 0x12), ..Motion::default() },
            slide: ram.u16(a + 0x26),
        }));
    }
    if ram.u8(a) != tag {
        return None;
    }
    if code == 0x48A2 || code == 0x48BA {
        return Some(Actor::GarbageDrop(GarbageDrop {
            player_slot: slot_of(ram.u32(a + 0x2E)),
            started: code == 0x48BA,
            falling: ram.u16(a + 0x26),
            seen: ram.u16(a + 0x28),
            strength: ram.u16(a + 0x20),
            shake: ram.u16(a + 0x38),
            phase: ram.u8(a + 0x36),
            voiced: ram.u8(a + 7) & 1 != 0,
            frame: ram.u8(a + 9),
        }));
    }
    if code == 0x486A || code == 0x5044 {
        return Some(Actor::GarbageColumn(GarbageColumn {
            player: ram.u8(a + 0x2A),
            drop_slot: slot_of(ram.u32(a + 0x2E)),
            col: ram.u16(a + 0x1A),
            row: ram.u16(a + 0x1C),
            frame: ram.u8(a + 9),
            fall: fall_from(ram, a),
            landed: code == 0x5044,
            linger: ram.u16(a + 0x26),
        }));
    }
    if let Some(actor) = load_stage(ram, a, code) {
        return Some(actor);
    }
    let player_state = [
        PlayerState::WaitStart,
        PlayerState::WaitPair,
        PlayerState::FindPops,
        PlayerState::Redraw,
        PlayerState::Decide,
        PlayerState::SettleDelay,
        PlayerState::AfterDelay,
        PlayerState::WaitGarbage,
        PlayerState::PreSpawn1,
        PlayerState::PreSpawn2,
        PlayerState::PreSpawn3,
        PlayerState::Spawn,
        PlayerState::PopScore,
        PlayerState::PopBackup,
        PlayerState::Blink,
        PlayerState::PopSpawn,
        PlayerState::PopWait,
        PlayerState::Compact,
        PlayerState::FallWait,
        PlayerState::ChainNext,
        PlayerState::TopOut,
        PlayerState::Wipe,
        PlayerState::Cleared,
        PlayerState::ResultsWait,
        PlayerState::ResultsShown,
        PlayerState::BonusWait,
        PlayerState::Bonus,
        PlayerState::TallyWait,
        PlayerState::Tally,
        PlayerState::NoBonusWait,
        PlayerState::NoBonus,
        PlayerState::Continue,
        PlayerState::Leave,
        PlayerState::Done,
        PlayerState::ContinueWait,
        PlayerState::HighScore,
        PlayerState::ContinueCheck,
        PlayerState::ContinueDone,
    ]
    .into_iter()
    .find(|s| s.code() == code);
    if let Some(state) = player_state {
        return Some(Actor::Player(Player {
            id: ram.u8(a + 0x2A),
            state,
            flags: ram.u8(a + 7),
            w26: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
            chain: ram.u8(a + 9),
            fall_speed: ram.u16(a + 0x1A),
            speed_level: ram.u8(a + 0x2B),
            target: (ram.u8(a + 0x20), ram.u8(a + 0x21)),
            move_frames: ram.u8(a + 0x2C),
            pause_frames: ram.u8(a + 0x2D),
            other_slot: slot_of(ram.u32(a + 0x2E)),
            holder_slot: slot_at(ram.u32(a + 0x32)),
            w28: ram.u16(a + 0x28),
            pending_garbage: ram.u16(a + 0x14),
            speed_index: ram.u8(a + 0x08),
            score: ram.u32(a + 0x0A),
            acc: ram.u32(a + 0x0E),
            base: ram.u16(a + 0x12),
            bonus: ram.u16(a + 0x1E),
            beans_total: ram.u16(a + 0x16),
            beans_count: ram.u16(a + 0x18),
            threshold: ram.u16(a + 0x1C),
            garbage_sent: 0,
            pop: None,
        }));
    }
    if code == PivotState::Controlled.code() || code == PivotState::Hidden.code() {
        let sat = ram.u32(a + 0x36);
        return Some(Actor::Pivot(Pivot {
            player_slot: slot_of(ram.u32(a + 0x2E)),
            player_id: ram.u8(a + 0x2A),
            satellite_slot: slot_of(sat),
            color: color(ram, a),
            state: if code == PivotState::Hidden.code() { PivotState::Hidden } else { PivotState::Controlled },
            pair: Pair {
                col: ram.u16(a + 0x1A),
                row: ram.u16(a + 0x1C),
                rot: ram.u8(a + 0x2B),
                fall: ram.u16(a + 0x20),
                slide: ram.u16(a + 0x1E),
                contacts: ram.u16(a + 0x26),
                lock_timer: ram.u16(a + 0x28),
                locked: ram.u8(a + 7) & 8 != 0,
                sat_angle: ram.u8(sat + 0x36),
                sat_turn: ram.u8(sat + 0x38),
                pivot_anim: anim_from(ram, a),
                sat_anim: anim_from(ram, sat),
            },
            flags: 0,
            y: ram.u32(a + 0x0E),
            fall: Fall::default(),
        }));
    }
    let pivot_state = [PivotState::Falling, PivotState::Squash].into_iter().find(|s| s.code() == code);
    if let Some(state) = pivot_state {
        let sat = ram.u32(a + 0x36);
        return Some(Actor::Pivot(Pivot {
            player_slot: slot_of(ram.u32(a + 0x2E)),
            player_id: ram.u8(a + 0x2A),
            satellite_slot: slot_of(sat),
            color: color(ram, a),
            state,
            pair: Pair {
                col: ram.u16(a + 0x1A),
                row: ram.u16(a + 0x1C),
                rot: ram.u8(a + 0x2B),
                pivot_anim: anim_from(ram, a),
                sat_anim: anim_from(ram, sat),
                ..Pair::default()
            },
            flags: ram.u8(a + 7),
            y: ram.u32(a + 0x0E),
            fall: fall_from(ram, a),
        }));
    }
    let sat_state = [SatelliteState::Check, SatelliteState::Falling, SatelliteState::Squash].into_iter().find(|s| s.code() == code);
    if let Some(state) = sat_state {
        return Some(Actor::Satellite(Satellite {
            pivot_slot: usize::MAX,
            player_slot: slot_of(ram.u32(a + 0x2E)),
            player_id: ram.u8(a + 0x2A),
            color: color(ram, a),
            state,
            col: ram.u16(a + 0x1A),
            row: ram.u16(a + 0x1C),
            flags: ram.u8(a + 7),
            y: ram.u32(a + 0x0E),
            anim: anim_from(ram, a),
            fall: fall_from(ram, a),
        }));
    }
    let motion = || Motion {
        x: ram.u32(a + 0x0A),
        y: ram.u32(a + 0x0E),
        vx: ram.u32(a + 0x12),
        vy: ram.u32(a + 0x16),
        ax: ram.u16(a + 0x1A),
        ay: ram.u16(a + 0x1C),
        tx: ram.u16(a + 0x1E),
        ty: ram.u16(a + 0x20),
    };
    if let Some(state) = [BurstState::Wait, BurstState::Burst, BurstState::Animate].into_iter().find(|s| s.code() == code) {
        return Some(Actor::Burst(Burst {
            player: ram.u8(a + 0x2A),
            color: ram.u8(a + 8),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
        }));
    }
    if let Some(state) = [PieceState::Start, PieceState::Wait, PieceState::Fly].into_iter().find(|s| s.code() == code) {
        return Some(Actor::Piece(Piece {
            player: 0,
            color: ram.u8(a + 8),
            motion: motion(),
            draw: ram.u8(a + 6),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
        }));
    }
    if code == 0x4DA4 {
        return Some(Actor::GarbageClear(GarbageClear { player: 0, x: ram.u16(a + 0x0A), y: ram.u16(a + 0x0E), anim: anim_from(ram, a) }));
    }
    let glow_states = [GlowState::Start, GlowState::Hover, GlowState::HoverWait, GlowState::Launch, GlowState::Fly, GlowState::End];
    if let Some(state) = glow_states.into_iter().find(|s| s.code() == code) {
        return Some(Actor::Glow(Glow {
            player: ram.u8(a + 0x2A),
            player_slot: slot_of(ram.u32(a + 0x2E)),
            chain: ram.u8(a + 0x2B),
            motion: motion(),
            draw: ram.u8(a + 6),
            base_x: ram.u16(a + 0x1E),
            counter: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
            anim: anim_from(ram, a),
            state,
        }));
    }
    let effect_state = [EffectState::Wait, EffectState::Start, EffectState::Rise].into_iter().find(|s| s.code() == code);
    effect_state.map(|state| {
        Actor::Effect(Effect {
            player: ram.u8(a + 0x2A),
            state,
            delay: ram.u16(a + 0x24),
            steps_left: ram.u8(a + 0x26),
            ticks: ram.u8(a + 0x28),
            y: ram.u32(a + 0x0E),
        })
    })
}

fn motion_at(ram: &Ram, a: u32) -> Motion {
    Motion {
        x: ram.u32(a + 0x0A),
        y: ram.u32(a + 0x0E),
        vx: ram.u32(a + 0x12),
        vy: ram.u32(a + 0x16),
        ax: ram.u16(a + 0x1A),
        ay: ram.u16(a + 0x1C),
        tx: ram.u16(a + 0x1E),
        ty: ram.u16(a + 0x20),
    }
}

/// The match-level actors (any tag; most are $FF).
fn load_stage(ram: &Ram, a: u32, code: u32) -> Option<Actor> {
    Some(match code {
        PAIR_HOLDER_CODE => Actor::PairHolder(PairHolder {
            w20: ram.u16(a + 0x20),
            pairs: ram.bytes(a + 0x26, 6).try_into().unwrap(),
            dealt: ram.u16(a + 0x1E),
            b07: ram.u8(a + 7),
        }),
        PREVIEW_BEAN_CODE => Actor::PreviewBean(PreviewBean {
            holder_slot: slot_of(ram.u32(a + 0x2E)),
            index: ram.u8(a + 7),
            color: ram.u8(a + 8),
            last: ram.u8(a + 0x36),
            draw: ram.u8(a + 6),
            anim: anim_from(ram, a),
        }),
        0x5BF8 | 0x5C62 => {
            let intro = code == 0x5BF8;
            Actor::Referee(Referee {
                state: if intro { RefereeState::Intro } else { RefereeState::Idle },
                link: ram.u32(a + 0x2E),
                anim: anim_from(ram, a),
                draw: ram.u8(a + 6),
                flags: ram.u8(a + 7),
                x: ram.u16(a + 0x0A),
                y: ram.u16(a + 0x0E),
                base_y: ram.u16(a + 0x20),
                vx: ram.u32(a + 0x12),
                repeats: ram.u16(a + 0x26),
                bob: ram.u16(a + 0x28),
                last_frame: ram.u8(a + 0x36),
            })
        }
        SPARKLE_CODE => Actor::Sparkle(Sparkle { motion: motion_at(ram, a), draw: ram.u8(a + 6), timer: ram.u16(a + 0x26) }),
        ANIMATOR_CODE => Actor::Animator(Animator { anim: anim_from(ram, a), shown: ram.u8(a + 8) }),
        DANGER_CODE => Actor::Danger(Danger {
            spark_wait: ram.u16(a + 0x26),
            flash_wait: ram.u16(a + 0x28),
            flash: ram.u16(a + 0x2A),
        }),
        0x1251C | 0x1252E | 0x1254E => Actor::DangerSpark(DangerSpark {
            state: [DangerSparkState::Start, DangerSparkState::Fly, DangerSparkState::Fade].into_iter().find(|s| s.code() == code).unwrap(),
            motion: motion_at(ram, a),
            draw: ram.u8(a + 6),
            anim: anim_from(ram, a),
        }),
        PORTRAIT_CODE => Actor::Portrait(Portrait {
            level: ram.u16(a + 0x26),
            base: ram.u16(a + 0x28),
            wait: ram.u8(a + 0x22),
            shown: ram.u8(a + 0x2C),
            next: ram.u32(a + 0x32),
            map: 0,
        }),
        PALETTE_FADE_CODE => Actor::PaletteFade(PaletteFade {
            line: ram.u16(a + 0x08),
            flag: ram.u16(a + 0x12),
            timer: ram.u16(a + 0x26),
            speed: ram.u16(a + 0x28),
            steps: ram.u16(a + 0x2A),
        }),
        0x7104 | 0x710E | 0x7114 | 0x7132 | 0x714E | 0x7152 => Actor::Star(Star {
            state: [StarState::Init, StarState::Wait, StarState::Delay, StarState::Rise, StarState::Pause, StarState::Burst]
                .into_iter()
                .find(|s| s.code() == code)
                .unwrap(),
            motion: motion_at(ram, a),
            draw: ram.u8(a + 6),
            color: ram.u8(a + 8),
            frame: ram.u8(a + 9),
            timer: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
        }),
        STAR_SPARK_CODE => Actor::StarSpark(StarSpark { motion: motion_at(ram, a), draw: ram.u8(a + 6), anim: anim_from(ram, a) }),
        0x72A4 | 0x72B0 | 0x72D0 => Actor::Banner(Banner {
            state: [BannerState::Start, BannerState::Count, BannerState::Hold].into_iter().find(|s| s.code() == code).unwrap(),
            timer: ram.u16(a + 0x26),
        }),
        FIELD_WIPE_CODE => Actor::FieldWipe(FieldWipe { parent: slot_of(ram.u32(a + 0x2E)), anim: anim_from(ram, a), shown: ram.u8(a + 8) }),
        0x8386 | 0x838A => Actor::FallingColumn(FallingColumn {
            parent: slot_of(ram.u32(a + 0x2E)),
            falling: code == 0x838A,
            motion: motion_at(ram, a),
            draw: ram.u8(a + 6),
            color: ram.u8(a + 8),
            delay: ram.u16(a + 0x24),
            scroll: ram.u16(a + 0x32),
        }),
        CONTINUE_SIGN_CODE => Actor::ContinueSign(ContinueSign {
            parent: slot_of(ram.u32(a + 0x2E)),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            target_y: ram.u16(a + 0x20),
            anim: anim_from(ram, a),
        }),
        0x8490 | 0x84A4 | 0x84D2 => Actor::BouncingBean(BouncingBean {
            state: [BounceState::Wait, BounceState::Fly, BounceState::Gone].into_iter().find(|s| s.code() == code).unwrap(),
            motion: motion_at(ram, a),
            draw: ram.u8(a + 6),
            color: ram.u8(a + 8),
            frame: ram.u8(a + 9),
            angle: ram.u8(a + 0x36),
            timer: ram.u16(a + 0x26),
            followed: ram.u8(a + 7) != 0,
        }),
        BOUNCE_SPARKLE_CODE => Actor::BounceSparkle(BounceSparkle {
            bean: slot_of(ram.u32(a + 0x2E)),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            anim: anim_from(ram, a),
        }),
        PASSWORD_BEAN_CODE => Actor::PasswordBean(PasswordBean {
            color: ram.u8(a + 8),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            anim: anim_from(ram, a),
        }),
        PORTRAIT_PALETTE_CODE => Actor::PortraitPalette(PortraitPalette {
            wait: ram.u8(a + 0x22),
            step: ram.u16(a + 0x26),
            level: ram.u8(a + 0x2A),
        }),
        _ => return None,
    })
}

/// The parts of a ported actor the ROM's RAM must agree with.
pub fn describe(actor: &Actor) -> String {
    match actor {
        Actor::Opaque | Actor::OpaqueQuick | Actor::Vanishing => "opaque".into(),
        Actor::Player(p) => format!(
            "player {:04X} id {} flags {:02X} delay {} chain {} score {} acc {} base {} bonus {} beans {}/{} speed {:04X} step {} next {} w26 {:04X} w28 {:04X} cpu {:?} {} {} links {:X} {:?}",
            p.state.code(),
            p.id,
            p.flags & 0x17,
            p.delay,
            p.chain,
            p.score,
            p.acc,
            p.base,
            p.bonus,
            p.beans_total,
            p.beans_count,
            p.fall_speed,
            p.speed_index,
            p.threshold,
            p.w26,
            p.w28,
            p.target,
            p.move_frames,
            p.pause_frames,
            p.other_slot,
            p.holder_slot,
        ),
        Actor::Pivot(v) => match v.state {
            PivotState::Init | PivotState::Hidden => format!("pivot {:04X}", v.state.code()),
            PivotState::Controlled => format!("pivot {:04X} {:?}", v.state.code(), v.pair),
            _ => format!(
                "pivot {:04X} col {} row {} y {:08X} vel {:08X} anim {:?} flags {:02X}",
                v.state.code(), v.pair.col, v.pair.row, v.y, v.fall.vel, v.pair.pivot_anim, v.flags
            ),
        },
        Actor::Satellite(s) => match s.state {
            SatelliteState::Init | SatelliteState::Follow => format!("satellite {:04X}", s.state.code()),
            _ => format!(
                "satellite {:04X} col {} row {} y {:08X} vel {:08X} anim {:?} flags {:02X}",
                s.state.code(), s.col, s.row, s.y, s.fall.vel, s.anim, s.flags
            ),
        },
        Actor::Effect(e) => format!("effect {:04X} delay {} steps {} ticks {} y {:08X}", e.state.code(), e.delay, e.steps_left, e.ticks, e.y),
        Actor::Burst(b) => format!("burst {:04X} color {} at {:X},{:X} delay {} anim {:?}", b.state.code(), b.color, b.x, b.y, b.delay, b.anim),
        Actor::Piece(p) => format!(
            "piece {:04X} color {} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} delay {} anim {:?}",
            p.state.code(), p.color, p.motion.x, p.motion.y, p.motion.vx, p.motion.vy, p.draw, p.delay, p.anim
        ),
        Actor::GarbageClear(g) => format!("garbage clear at {:X},{:X} anim {:?}", g.x, g.y, g.anim),
        Actor::GarbageDrop(d) => format!(
            "garbage drop {:04X} falling {} seen {} strength {:04X} shake {:04X} phase {:02X} frame {:02X}",
            if d.started { 0x48BA } else { 0x48A2 }, d.falling, d.seen, d.strength, d.shake, d.phase, d.frame
        ),
        Actor::GarbageColumn(c) => format!(
            "garbage column {:04X} col {} row {} frame {:02X} y {:08X} vel {:08X} linger {}",
            if c.landed { 0x5044 } else { 0x486A }, c.col, c.row, c.frame, c.fall.y, c.fall.vel, c.linger
        ),
        Actor::Icon(i) => format!(
            "icon side {} gen {} frame {} x {:08X} y {:08X} vx {:08X} slide {}",
            i.side, i.generation, i.frame, i.motion.x, i.motion.y, i.motion.vx, i.slide
        ),
        Actor::PairHolder(h) => format!("pair holder {:04X} pairs {:02X?} dealt {} b07 {:02X}", h.w20, h.pairs, h.dealt, h.b07),
        Actor::PreviewBean(v) => format!(
            "preview bean holder {:X} index {} color {:02X} last {:02X} draw {:02X} anim {:?}",
            v.holder_slot, v.index, v.color, v.last, v.draw, v.anim
        ),
        Actor::Referee(r) => format!(
            "referee {:04X} link {:X} anim {:?} draw {:02X} flags {:02X} at {:X},{:X} base {:X} vx {:08X} repeats {} bob {:04X} last {:02X}",
            r.state.code(), r.link, r.anim, r.draw, r.flags, r.x, r.y, r.base_y, r.vx, r.repeats, r.bob, r.last_frame
        ),
        Actor::Sparkle(sp) => format!(
            "sparkle pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} timer {}",
            sp.motion.x, sp.motion.y, sp.motion.vx, sp.motion.vy, sp.draw, sp.timer
        ),
        Actor::Animator(an) => format!("animator anim {:?} shown {:02X}", an.anim, an.shown),
        Actor::Danger(d) => format!("danger spark {} flash {} left {}", d.spark_wait, d.flash_wait, d.flash),
        Actor::DangerSpark(sp) => format!(
            "danger spark {:05X} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} anim {:?}",
            sp.state.code(), sp.motion.x, sp.motion.y, sp.motion.vx, sp.motion.vy, sp.draw, sp.anim
        ),
        Actor::Portrait(pt) => format!("portrait level {:04X} base {} wait {} shown {} next {:X}", pt.level, pt.base, pt.wait, pt.shown, pt.next),
        Actor::PaletteFade(pf) => format!("palette fade line {} gen {} timer {} speed {} steps {}", pf.line, pf.flag, pf.timer, pf.speed, pf.steps),
        Actor::Star(st) => format!(
            "star {:04X} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} color {:02X} frame {} timer {} delay {}",
            st.state.code(), st.motion.x, st.motion.y, st.motion.vx, st.motion.vy, st.draw, st.color, st.frame, st.timer, st.delay
        ),
        Actor::StarSpark(sp) => format!(
            "star spark pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} anim {:?}",
            sp.motion.x, sp.motion.y, sp.motion.vx, sp.motion.vy, sp.draw, sp.anim
        ),
        Actor::Banner(b) => format!("banner {:04X} timer {}", b.state.code(), b.timer),
        Actor::FieldWipe(w) => format!("field wipe parent {:X} anim {:?} shown {:02X}", w.parent, w.anim, w.shown),
        Actor::FallingColumn(c) => format!(
            "falling column {} parent {:X} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} color {} delay {} scroll {:X}",
            if c.falling { "838A" } else { "8386" }, c.parent, c.motion.x, c.motion.y, c.motion.vx, c.motion.vy, c.draw, c.color, c.delay, c.scroll
        ),
        Actor::ContinueSign(c) => format!("continue sign parent {:X} at {:X},{:X} target {:X} anim {:?}", c.parent, c.x, c.y, c.target_y, c.anim),
        Actor::BouncingBean(b) => format!(
            "bouncing bean {:04X} pos {:08X},{:08X} vel {:08X},{:08X} draw {:02X} color {} frame {} angle {:02X} timer {} followed {}",
            b.state.code(), b.motion.x, b.motion.y, b.motion.vx, b.motion.vy, b.draw, b.color, b.frame, b.angle, b.timer, b.followed
        ),
        Actor::BounceSparkle(sp) => format!("bounce sparkle bean {:X} at {:X},{:X} anim {:?}", sp.bean, sp.x, sp.y, sp.anim),
        Actor::PasswordBean(b) => format!("password bean color {:02X} at {:X},{:X} anim {:?}", b.color, b.x, b.y, b.anim),
        Actor::PortraitPalette(pt) => format!("portrait palette wait {} step {:04X} level {}", pt.wait, pt.step, pt.level),
        Actor::Glow(g) => format!(
            "glow {:05X} chain {} pos {:08X},{:08X} vel {:08X},{:08X} base {:X} counter {:04X} draw {:02X} delay {} anim {:?}",
            g.state.code(), g.chain, g.motion.x, g.motion.y, g.motion.vx, g.motion.vy, g.base_x, g.counter, g.draw, g.delay, g.anim
        ),
    }
}

/// What the ROM shows in a slot, described the same way (None = not a ported actor).
pub fn describe_ram(ram: &Ram, slot: usize, tag: u8, ours: &Actor) -> Option<String> {
    if let Some(actor) = load(ram, slot, tag) {
        return Some(describe(&actor));
    }
    // Spawned and controlled pairs can't be rebuilt from RAM alone: compare codes only.
    let code = ram.code(slot_addr(slot));
    match ours {
        Actor::Pivot(_) if [0x4F06, 0x4F14].contains(&code) => Some(format!("pivot {code:04X}")),
        Actor::Satellite(_) if [0x5474, 0x5482].contains(&code) => Some(format!("satellite {code:04X}")),
        _ => None,
    }
}


/// Resume addresses of every actor state the port knows.
pub fn known_code(code: u32) -> bool {
    matches!(code, 0x3D00..=0x40FF | 0x4F06 | 0x4F14 | 0x5474 | 0x5482) || load_codes().contains(&code)
}

pub fn load_codes() -> Vec<u32> {
    let mut v = vec![0x4F8A, 0x4FA8, 0x54D4, 0x54FA, 0x5514, 0x4FCE, 0x4FE6, 0x4FF4, 0x4DA4];
    v.extend([BurstState::Wait, BurstState::Burst, BurstState::Animate].map(|s| s.code()));
    v.extend([PieceState::Start, PieceState::Wait, PieceState::Fly].map(|s| s.code()));
    v.extend([GlowState::Start, GlowState::Hover, GlowState::HoverWait, GlowState::Launch, GlowState::Fly, GlowState::End].map(|s| s.code()));
    v
}

