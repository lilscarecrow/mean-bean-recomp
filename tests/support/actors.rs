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
            player: ram.u8(a + 0x2A),
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
        PlayerState::Setup,
        PlayerState::WaitReady,
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
        PlayerState::FinalWait,
        PlayerState::FinalLeave,
        PlayerState::FinalDone,
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
            x: ram.u16(a + 0x0A),
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
            color: ram.u8(a + 8),
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
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
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0x5BCE | 0x5BF8 | 0x5C62 => {
            Actor::Referee(Referee {
                state: [RefereeState::Spawned, RefereeState::Intro, RefereeState::Idle].into_iter().find(|s| s.code() == code).unwrap(),
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
        ANIMATOR_CODE | 0x3BBA => Actor::Animator(Animator { started: code == ANIMATOR_CODE, anim: anim_from(ram, a), shown: ram.u8(a + 8) }),
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
        PORTRAIT_CODE | 0x60F4 => Actor::Portrait(Portrait {
            started: code == PORTRAIT_CODE,
            level: ram.u16(a + 0x26),
            base: ram.u16(a + 0x28),
            wait: ram.u8(a + 0x22),
            shown: ram.u8(a + 0x2C),
            next: ram.u32(a + 0x32),
            map: 0,
            tile: ram.u16(a + 0x2A) | ram.u16(a + 0x0A),
            at: ram.u16(a + 0x0C),
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
        BG_SLIDE_CODE => Actor::BgSlide(BgSlide {
            draw: ram.u8(a + 6),
            frame: ram.u8(a + 9),
            steps: ram.u8(a + 0x22),
            pause: ram.u8(a + 0x26),
            place: ram.u8(a + 0x28),
            dx: ram.u16(a + 0x2A),
            dy: ram.u16(a + 0x2C),
            phase: ram.u8(a + 0x32),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        BG_RUNNER_CODE => Actor::BgRunner(BgRunner {
            portrait: slot_of(ram.u32(a + 0x2E)),
            draw: ram.u8(a + 6),
            latch: ram.u8(a + 0x26),
            left: ram.u8(a + 0x28),
            pause: ram.u8(a + 0x2A),
            level: ram.u8(a + 0x2C),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            anim: anim_from(ram, a),
        }),
        BG_SPARK_CODE => Actor::BgSpark(BgSpark {
            frame: ram.u8(a + 9),
            left: ram.u8(a + 0x28),
            dx: ram.u16(a + 0x2A),
            dy: ram.u16(a + 0x2C),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0xC00A | 0xC04C | 0xC060 | 0xC138 | 0xC146 => Actor::NameEntry(NameEntry {
            state: [NameEntryState::Start, NameEntryState::Clear, NameEntryState::Edit, NameEntryState::Shown, NameEntryState::Done]
                .into_iter()
                .find(|s| s.code() == code)
                .unwrap(),
            parent: slot_of(ram.u32(a + 0x2E)),
            player_id: ram.u8(a + 0x2A),
            rank: ram.u16(a + 0x0A),
            timeout: ram.u16(a + 0x28),
            cursor: ram.u16(a + 0x0E),
            letters: ram.bytes(a + 0x12, 5).try_into().unwrap(),
            blink: ram.u8(a + 0x26),
            delay: ram.u16(a + 0x24),
        }),
        SHAKE_FIELD_CODE => Actor::ShakeField(ShakeField {
            angle: ram.u8(a + 0x36),
            strength: ram.u16(a + 0x38),
            scroll: ram.u32(a + 0x32),
        }),
        PASSWORD_BEAN_CODE => Actor::PasswordBean(PasswordBean {
            color: ram.u8(a + 8),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            anim: anim_from(ram, a),
        }),
        PORTRAIT_PALETTE_CODE | 0x6F38 => Actor::PortraitPalette(PortraitPalette {
            started: code == PORTRAIT_PALETTE_CODE,
            wait: ram.u8(a + 0x22),
            step: ram.u16(a + 0x26),
            level: ram.u8(a + 0x2A),
        }),
        COCONUTS_LIGHT_CODE | 0x6954 => Actor::CoconutsLight(CoconutsLight {
            started: code == COCONUTS_LIGHT_CODE,
            level: ram.u8(a + 0x2A),
            counter: ram.u16(a + 0x26),
            wait: ram.u8(a + 0x22),
        }),
        0x10FDC | 0x1104C | 0x110CC => Actor::IntroScript(IntroScript {
            state: match code {
                0x10FDC => IntroScriptState::Start,
                0x1104C => IntroScriptState::Run,
                _ => IntroScriptState::Ending,
            },
            ptr: ram.u32(a + 0x32),
            wait: ram.u16(a + 0x26),
            shown: ram.u8(a + 7) != 0,
            at: ram.u16(a + 0x12),
            width: ram.u16(a + 0x0E),
            lines: ram.u16(a + 0x10),
            col: ram.u16(a + 0x0A),
            line: ram.u16(a + 0x0C),
        }),
        0x113D8 | 0x113B8 => Actor::IntroBoxPart(IntroBoxPart {
            parent: slot_of(ram.u32(a + 0x2E)),
            tail: code == 0x113B8,
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        // (Robotnik's leaving step is the opponents' too: his mappings are $2B)
        0xAADA | ROBOTNIK_INTRO_CODE => Actor::RobotnikIntro(RobotnikIntro {
            started: code == ROBOTNIK_INTRO_CODE,
            leaving: false,
            ptr: ram.u32(a + 0x32),
            time: ram.u8(a + 0x22),
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0xAAD6 if ram.u8(a + 8) == 0x2B => Actor::RobotnikIntro(RobotnikIntro {
            started: true,
            leaving: true,
            ptr: ram.u32(a + 0x32),
            time: ram.u8(a + 0x22),
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0xF296 | 0xF29E | 0xF31E | 0xA526 => Actor::LairSprite(LairSprite {
            kind: match code {
                0xF296 => LairKind::Machine,
                0xF29E => LairKind::Bubbles,
                0xF31E => LairKind::Launcher,
                _ => LairKind::Robot,
            },
            draw: ram.u8(a + 6),
            mappings: ram.u8(a + 8),
            anim: anim_from(ram, a),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            count: ram.u16(a + 0x26),
        }),
        0xF300 | 0xF38E | 0xF3B2 | 0xF3D8 => Actor::LairFlier(LairFlier {
            robot: code != 0xF300,
            phase: match code {
                0xF3B2 => 1,
                0xF3D8 => 2,
                _ => 0,
            },
            draw: ram.u8(a + 6),
            motion: motion_at(ram, a),
            anim: anim_from(ram, a),
            left: ram.u16(a + 0x26),
        }),
        LAIR_LIGHTS_CODE => Actor::LairLights(LairLights { count: ram.u16(a + 0x26), step: ram.u16(a + 0x28) }),
        0xA316 | 0xA31E | 0xA322 | 0xA368 | 0xA36C | 0xA3C2 | 0xA3C6 | 0xA3D4 | 0xA404 => Actor::ThrownPiece(ThrownPiece {
            parent: slot_of(ram.u32(a + 0x2E)),
            phase: match code {
                0xA316 => ThrowPhase::Start,
                0xA31E => ThrowPhase::Wait12,
                0xA322 => ThrowPhase::Fly,
                0xA368 => ThrowPhase::Wait8,
                0xA36C => ThrowPhase::Turn,
                0xA3C2 => ThrowPhase::Wait27,
                0xA3C6 => ThrowPhase::Sound,
                0xA3D4 => ThrowPhase::Fall,
                _ => ThrowPhase::Tug,
            },
            delay: ram.u16(a + 0x24),
            draw: ram.u8(a + 6),
            motion: motion_at(ram, a),
        }),
        GAME_OVER_TIMER_CODE | 0xDB64 => Actor::GameOverTimer(GameOverTimer {
            done: code == 0xDB64,
            wait: ram.u16(a + 0x26),
            count: ram.u16(a + 0x28),
            color: ram.u16(a + 0x2A),
        }),
        0xDC44 | 0xDCAC | 0xDCCC | 0xDD06 => Actor::GameOverLetter(GameOverLetter {
            phase: match code {
                0xDC44 => LetterPhase::Spiral,
                0xDCAC => LetterPhase::Wait,
                0xDCCC => LetterPhase::Fly,
                _ => LetterPhase::Bob,
            },
            frame: ram.u8(a + 9),
            motion: motion_at(ram, a),
            count: ram.u16(a + 0x26),
            angle: ram.u8(a + 0x36),
            radius: ram.u16(a + 0x38),
            y: ram.u32(a + 0x32),
        }),
        0xCB52 | 0xCB68 | 0xCB86 | 0xCBA4 => Actor::DefeatSounds(DefeatSounds {
            stage: match code {
                0xCB52 => 0,
                0xCB68 => 1,
                0xCB86 => 2,
                _ => 3,
            },
            count: if code == 0xCB52 { 0 } else { ram.u16(a + 0x26) },
        }),
        0xCAFC | 0xCB1C => Actor::DefeatShip(DefeatShip { flying: code == 0xCB1C, parent: slot_of(ram.u32(a + 0x2E)), motion: motion_at(ram, a) }),
        0xCBBC | 0xCBD0 | 0xCC00 | 0xCC14 => Actor::DefeatPiece(DefeatPiece {
            phase: match code {
                0xCBBC => PiecePhase::Wait,
                0xCBD0 => PiecePhase::Fall,
                0xCC00 => PiecePhase::Hold,
                _ => PiecePhase::Drag,
            },
            parent: slot_of(ram.u32(a + 0x2E)),
            draw: ram.u8(a + 6),
            motion: motion_at(ram, a),
            delay: ram.u16(a + 0x2A),
            tick: ram.u8(a + 0x28),
            step: ram.u16(a + 0x26),
        }),
        0xCCA6 | 0xCCB2 | 0xCCC4 | 0xCCCA | 0xCCDE => Actor::DefeatPart(DefeatPart {
            phase: match code {
                0xCCA6 | 0xCCCA => 0,
                0xCCB2 | 0xCCDE => 1,
                _ => 2,
            },
            draw: ram.u8(a + 6),
            anim: anim_from(ram, a),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            wait: ram.u8(a + 0x26),
            explosion: code == 0xCCCA || code == 0xCCDE,
        }),
        SKIP_CODE => Actor::Skip,
        0xC28A | 0xC2B0 | 0xC2C4 | 0xC38C | 0xC3C8 | 0xC3CE => Actor::TableNameEntry(TableNameEntry {
            phase: match code {
                0xC28A => NamePhase::Start,
                0xC2B0 => NamePhase::Show,
                0xC2C4 => NamePhase::Choose,
                0xC38C => NamePhase::Back,
                0xC3C8 => NamePhase::Wait,
                _ => NamePhase::Done,
            },
            parent: slot_of(ram.u32(a + 0x2E)),
            busy: ram.u8(a + 7) != 0,
            entry: ram.u16(a + 0x0A),
            at: ram.u16(a + 0x0E),
            letters: ram.bytes(a + 0x12, 5).try_into().unwrap(),
            blink: ram.u8(a + 0x26),
            anim: anim_from(ram, a),
            timer: ram.u16(a + 0x28),
            delay: if code == 0xC3C8 { ram.u16(a + 0x24) } else { 0 },
        }),
        TABLE_CURSOR_CODE => Actor::TableCursor(TableCursor {
            parent: slot_of(ram.u32(a + 0x2E)),
            index: ram.u16(a + 0x1E),
            draw: ram.u8(a + 6),
            anim: anim_from(ram, a),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0xFAE6 | 0xFAF6 | 0xFAFC => Actor::FireworkColors(FireworkColors {
            phase: match code {
                0xFAE6 => 0,
                0xFAF6 => 1,
                _ => 2,
            },
            delay: ram.u16(a + 0x24),
            a: ram.u16(a + 0x0A),
            b: ram.u16(a + 0x0C),
            c: ram.u16(a + 0x0E),
            d: ram.u16(a + 0x10),
        }),
        0xFBA0 | 0xFBAE | 0xFBD8 => Actor::FireworkSpawner(FireworkSpawner {
            phase: match code {
                0xFBA0 => 0,
                0xFBAE => 1,
                _ => 2,
            },
            count: ram.u16(a + 0x26),
        }),
        FIREWORK_SPARK_CODE => Actor::FireworkSpark(FireworkSpark { draw: ram.u8(a + 6), motion: motion_at(ram, a), anim: anim_from(ram, a) }),
        c if Dawn::phase_of(c).is_some() => Actor::Dawn(Dawn { phase: Dawn::phase_of(c).unwrap(), step: ram.u16(a + 0x26), delay: ram.u16(a + 0x24) }),
        c if Epilogue::phase_of(c).is_some() => Actor::Epilogue(Epilogue {
            phase: Epilogue::phase_of(c).unwrap(),
            busy: ram.u8(a + 7) != 0,
            ptr: ram.u32(a + 0x32),
            wait: ram.u16(a + 0x26),
            delay: ram.u16(a + 0x24),
        }),
        0xB60E | 0xB646 => Actor::CreditLine(CreditLine {
            stopped: code == 0xB646,
            parent: slot_of(ram.u32(a + 0x32)),
            line: ram.u8(a + 9),
            motion: motion_at(ram, a),
        }),
        c if RoleCall::step_of(c).is_some() => Actor::RoleCall(RoleCall {
            step: RoleCall::step_of(c).unwrap(),
            delay: ram.u16(a + 0x24),
            busy: ram.u8(a + 7) != 0,
        }),
        0x1066C | 0x10678 | 0x10690 | 0x106E2 | 0x106FC | 0x10712 | 0x10724 | 0x1073C | 0x1075C | 0x10776 => Actor::Walker(Walker {
            robotnik: code >= 0x10712,
            phase: match code {
                0x1066C | 0x10712 => 0,
                0x10678 | 0x10724 => 1,
                0x10690 | 0x1073C => 2,
                0x106E2 | 0x1075C => 3,
                _ => 4,
            },
            parent: slot_of(ram.u32(a + 0x2E)),
            busy: ram.u8(a + 7) != 0,
            mappings: ram.u8(a + 8),
            motion: motion_at(ram, a),
            anim: anim_from(ram, a),
            count: ram.u16(a + 0x26),
        }),
        BACKDROP_PROP_CODE => Actor::BackdropProp,
        TYPIST_CODE => Actor::Typist(Typist { tick: ram.u8(a + 0x26), left: ram.u16(a + 0x28), at: ram.u16(a + 0x2A), text: ram.u32(a + 0x2E) }),
        c if LairReveal::step_of(c).is_some() => Actor::LairReveal(LairReveal { step: LairReveal::step_of(c).unwrap(), delay: ram.u16(a + 0x24) }),
        0x10A14 | 0x10A26 | 0x10A2C | 0x10A5C | 0x10A9E => Actor::HasBean(HasBean {
            phase: match code {
                0x10A14 => 0,
                0x10A26 => 1,
                0x10A2C => 2,
                0x10A5C => 3,
                _ => 4,
            },
            delay: ram.u16(a + 0x24),
            draw: ram.u8(a + 6),
            mappings: ram.u8(a + 8),
            motion: motion_at(ram, a),
            anim: anim_from(ram, a),
        }),
        0x10AC8 | 0x10AF4 | 0x10B12 | 0x10B26 => Actor::HasBeanShadow(HasBeanShadow {
            phase: match code {
                0x10AC8 => 0,
                0x10AF4 => 1,
                0x10B12 => 2,
                _ => 3,
            },
            draw: ram.u8(a + 6),
            anim: anim_from(ram, a),
            count: ram.u16(a + 0x26),
        }),
        SPARKLE_SPAWNER_CODE => Actor::SparkleSpawner(SparkleSpawner { count: ram.u16(a + 0x26) }),
        SHIP_SPARKLE_CODE => Actor::ShipSparkle(ShipSparkle {
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            step: ram.u16(a + 0x26),
            tick: ram.u16(a + 0x28),
        }),
        0xC66A | 0xC68E | 0xC70A | 0xC710 => Actor::HighScoreTable(HighScoreTable {
            busy: ram.u8(a + 7) != 0,
            phase: match code {
                0xC66A => TablePhase::Draw,
                0xC68E => TablePhase::Slide,
                0xC70A => TablePhase::Wait,
                _ => TablePhase::Done,
            },
            scroll: ram.u16(a + 0x26),
            delay: if code >= 0xC70A { ram.u16(a + 0x24) } else { 0 },
            unnamed: ram.u8(a + 0x0F) != 0,
            released: ram.u8(a + 0x2A) != 0,
        }),
        0xEE10 | CUTSCENE_ROBOTNIK_CODE => Actor::CutsceneRobotnik(CutsceneRobotnik {
            started: code == CUTSCENE_ROBOTNIK_CODE,
            ptr: ram.u32(a + 0x32),
            time: ram.u8(a + 0x22),
            frame: ram.u8(a + 9),
            mappings: ram.u8(a + 8),
            x: ram.u32(a + 0x0A),
            y: ram.u16(a + 0x0E),
            draw: ram.u8(a + 6),
        }),
        OPPONENT_INTRO_CODE | 0xAAD6 => Actor::OpponentIntro(OpponentIntro {
            leaving: code == 0xAAD6,
            mappings: ram.u8(a + 8),
            anim: anim_from(ram, a),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
        }),
        0xA758 => Actor::IntroExtra(IntroExtra::Flash { parent: slot_of(ram.u32(a + 0x2E)), count: ram.u8(a + 0x26) }),
        0xA560 => Actor::IntroExtra(IntroExtra::Sway { parent: slot_of(ram.u32(a + 0x2E)), angle: ram.u8(a + 0x36), centre: ram.u16(a + 0x1E) }),
        0xA7C4 => Actor::IntroExtra(IntroExtra::Cycle { parent: slot_of(ram.u32(a + 0x2E)), wait: ram.u8(a + 0x26), step: ram.u16(a + 0x28) }),
        0xA5F6 => Actor::IntroExtra(IntroExtra::Orbit {
            parent: slot_of(ram.u32(a + 0x2E)),
            angle: ram.u8(a + 0x36),
            stopped: ram.u8(a + 0x28),
            cued: ram.u8(a + 0x2A),
            cx: ram.u16(a + 0x1E),
            cy: ram.u16(a + 0x20),
        }),
        0xA692 => Actor::IntroExtra(IntroExtra::OrbitPiece {
            parent: slot_of(ram.u32(a + 0x2E)),
            orbit: slot_of(ram.u32(a + 0x32)),
            index: ram.u8(a + 0x28),
            angle: ram.u8(a + 0x36),
            count: ram.u8(a + 0x26),
            draw: ram.u8(a + 6),
            frame: ram.u8(a + 9),
            x: ram.u16(a + 0x0A),
            y: ram.u16(a + 0x0E),
            cx: ram.u16(a + 0x1E),
            cy: ram.u16(a + 0x20),
        }),
        0xA85C => Actor::IntroExtra(IntroExtra::Sparks { wait: ram.u16(a + 0x26) }),
        0xA8E2 => Actor::IntroExtra(IntroExtra::Spark { draw: ram.u8(a + 6), motion: motion_at(ram, a), anim: anim_from(ram, a), life: ram.u16(a + 0x26) }),
        INTRO_CLOUDS_CODE => Actor::IntroClouds(IntroClouds {
            pos: [0x0A, 0x12, 0x1A, 0x22].map(|o| ram.u32(a + o)),
            speed: [0x0E, 0x16, 0x1E, 0x26].map(|o| ram.u32(a + o)),
        }),
        INTRO_SHIP_CODE | 0xF994 | 0xF9E0 => Actor::IntroShip(IntroShip { code, anim: anim_from(ram, a), x: ram.u16(a + 0x0A), y: ram.u16(a + 0x0E) }),
        0xAFB2 | 0xAFD6 | 0xB01A | 0xB03E => Actor::Transition(Transition {
            plane_b: code >= 0xB01A,
            started: code == 0xAFD6 || code == 0xB03E,
            y: ram.u32(a + 0x0E),
            speed: ram.u32(a + 0x16),
            left: ram.u16(a + 0x26),
        }),
        BYTECODE_DELAY_CODE => Actor::FlowWait(FlowWait::Delay { delay: ram.u16(a + 0x24), done: false }),
        0x16B8 => Actor::FlowWait(FlowWait::Delay { delay: ram.u16(a + 0x24), done: true }),
        WAIT_PAL_FADE_CODE => Actor::FlowWait(FlowWait::PaletteFades),
        0xD6A8 | OPPONENT_SCREEN_CODE => Actor::OpponentScreen(OpponentScreen {
            started: code == OPPONENT_SCREEN_CODE,
            timer: ram.u16(a + 0x26),
            draw: ram.u8(a + 6),
        }),
        0xD818 | 0xD81E | 0xD824 => Actor::OpponentPortraits(OpponentPortraits {
            step: ((code - 0xD818) / 6) as u8,
            done: ram.u8(a + 0x22),
            left: ram.u16(a + 0x2C),
            at: ram.u16(a + 0x26),
        }),
        0x7C2C | 0x7C38 | 0x7BF0 | 0x7BFC => Actor::RobotnikVoice(RobotnikVoice {
            line: if code >= 0x7C2C { RobotnikVoice::VICTORY } else { RobotnikVoice::DEFEAT },
            started: code == 0x7C38 || code == 0x7BFC,
            wait: ram.u16(a + 0x26),
        }),
        FFUZZY_EYES_CODE => Actor::FfuzzyEyes(FfuzzyEyes { draw: ram.u8(a + 6), frame: ram.u8(a + 9), wait: ram.u8(a + 0x22), counter: ram.u16(a + 0x26) }),
        FFUZZY_PALETTE_CODE | 0x6E46 => {
            Actor::FfuzzyPalette(FfuzzyPalette { started: code == FFUZZY_PALETTE_CODE, wait: ram.u8(a + 0x22), counter: ram.u16(a + 0x26) })
        }
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
        Actor::BgSlide(b) => format!(
            "bg slide draw {:02X} steps {} pause {} place {} step {:X},{:X} phase {} at {:X},{:X}",
            b.draw, b.steps, b.pause, b.place, b.dx, b.dy, b.phase, b.x, b.y
        ),
        Actor::BgRunner(r) => format!(
            "bg runner portrait {:X} draw {:02X} latch {} left {} pause {} level {} at {:X},{:X} anim {:?}",
            r.portrait, r.draw, r.latch, r.left, r.pause, r.level, r.x, r.y,
            if r.level & 3 == 2 { r.anim } else { Anim { ptr: 0, ..r.anim } }
        ),
        Actor::BgSpark(sp) => format!("bg spark frame {} left {} step {:X},{:X} at {:X},{:X}", sp.frame, sp.left, sp.dx, sp.dy, sp.x, sp.y),
        Actor::NameEntry(n) => format!(
            "name entry {:04X} parent {:X} id {} rank {} timeout {:X} cursor {} letters {:02X?} blink {} delay {}",
            n.state.code(), n.parent, n.player_id, n.rank, n.timeout, n.cursor, n.letters, n.blink, n.delay
        ),
        Actor::ShakeField(sh) => format!("shake field angle {:02X} strength {:X} scroll {:X}", sh.angle, sh.strength, sh.scroll),
        Actor::PasswordBean(b) => format!("password bean color {:02X} at {:X},{:X} anim {:?}", b.color, b.x, b.y, b.anim),
        Actor::PortraitPalette(pt) => format!("portrait palette wait {} step {:04X} level {}", pt.wait, pt.step, pt.level),
        Actor::CoconutsLight(c) => format!("coconuts light {} level {} step {:04X} wait {}", c.started, c.level, c.counter, c.wait),
        Actor::FlowWait(w) => format!("flow wait {w:?}"),
        Actor::IntroScript(s) => format!("{s:X?}"),
        Actor::IntroBoxPart(p) => format!("{p:X?}"),
        Actor::OpponentIntro(o) => format!("{o:X?}"),
        Actor::IntroExtra(e) => format!("{e:X?}"),
        Actor::IntroClouds(c) => format!("{c:X?}"),
        Actor::IntroShip(s) => format!("{s:X?}"),
        Actor::RobotnikIntro(r) => format!("{r:X?}"),
        Actor::GameOverTimer(t) => format!("{t:X?}"),
        Actor::GameOverLetter(l) => format!("{l:X?}"),
        Actor::CutsceneRobotnik(r) => format!("{r:X?}"),
        Actor::HighScoreTable(t) => format!("{t:X?}"),
        Actor::DefeatSounds(s) => format!("{s:X?}"),
        Actor::DefeatShip(s) => format!("{s:X?}"),
        Actor::DefeatPiece(p) => format!("{p:X?}"),
        Actor::DefeatPart(p) => format!("{p:X?}"),
        Actor::Skip => "skip".into(),
        Actor::TableNameEntry(n) => format!("{n:X?}"),
        Actor::TableCursor(c) => format!("{c:X?}"),
        Actor::FireworkColors(c) => format!("{c:X?}"),
        Actor::FireworkSpawner(s) => format!("{s:X?}"),
        Actor::FireworkSpark(s) => format!("{s:X?}"),
        Actor::Dawn(d) => format!("{d:X?}"),
        Actor::Epilogue(e) => format!("{e:X?}"),
        Actor::CreditLine(l) => format!("{l:X?}"),
        Actor::RoleCall(r) => format!("{r:X?}"),
        Actor::Walker(w) => format!("{w:X?}"),
        Actor::BackdropProp => "backdrop prop".into(),
        Actor::Typist(t) => format!("{t:X?}"),
        Actor::LairReveal(l) => format!("{l:X?}"),
        Actor::HasBean(b) => format!("{b:X?}"),
        Actor::HasBeanShadow(s) => format!("{s:X?}"),
        Actor::SparkleSpawner(s) => format!("{s:X?}"),
        Actor::ShipSparkle(s) => format!("{s:X?}"),
        Actor::LairSprite(s) => format!("{s:X?}"),
        Actor::LairFlier(f) => format!("{f:X?}"),
        Actor::LairLights(l) => format!("{l:X?}"),
        Actor::ThrownPiece(p) => format!("{p:X?}"),
        Actor::Transition(t) => if t.started { format!("{t:X?}") } else { format!("transition {} waiting", t.plane_b) },
        Actor::OpponentScreen(s) => format!("opponent screen {} timer {:04X} draw {:02X}", s.started, if s.started { s.timer } else { 0 }, s.draw),
        Actor::OpponentPortraits(p) => format!("opponent portraits step {} done {} left {:04X} at {:04X}", p.step, p.done, p.left, p.at),
        Actor::RobotnikVoice(v) => format!("robotnik voice {:02X} {} wait {:04X}", v.line, v.started, if v.started { v.wait } else { 0 }),
        Actor::FfuzzyEyes(e) => format!("ffuzzy eyes draw {:02X} frame {} wait {} count {:04X}", e.draw, e.frame, e.wait, e.counter),
        Actor::FfuzzyPalette(p) => format!("ffuzzy palette {} wait {} step {:04X}", p.started, p.wait, p.counter),
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

