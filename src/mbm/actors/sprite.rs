//! What `DrawActors` sees of each actor: draw flags (+$06, bit 7 = drawn), mapping list
//! (+$08), frame (+$09) and position (+$0A, +$0E). The renderer turns these into
//! hardware sprites with `assets::sprites::place`.

use super::*;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Draw {
    pub flags: u8,
    pub mappings: u8,
    pub frame: u8,
    pub x: u16,
    pub y: u16,
}

impl Draw {
    pub fn shown(&self) -> bool {
        self.flags & 0x80 != 0
    }
}

fn hi(v: u32) -> u16 {
    (v >> 16) as u16
}

/// `sub_543C`: where a pair's pivot is drawn.
pub fn pivot_x(v: &Pivot) -> u16 {
    FIELD_X[usize::from(v.player_id & 1)].wrapping_add(v.pair.col.wrapping_mul(16)).wrapping_add(v.pair.slide).wrapping_add(8)
}

impl Engine {
    /// The draw state of the actor in `slot` (None for an empty slot).
    pub fn draw_of(&self, slot: usize) -> Option<Draw> {
        let d = |flags: u8, mappings: u8, frame: u8, x: u16, y: u16| Some(Draw { flags, mappings, frame, x, y });
        let field_x = |player: u8, col: u16| FIELD_X[usize::from(player & 1)].wrapping_add(col.wrapping_mul(16)).wrapping_add(8);
        match self.slots[slot].as_ref()? {
            Actor::Pivot(v) => match v.state {
                PivotState::Hidden => d(0, v.color as u8, v.pair.pivot_anim.frame, pivot_x(v), hi(v.y)),
                _ => d(0x80, v.color as u8, v.pair.pivot_anim.frame, pivot_x(v), hi(v.y)),
            },
            Actor::Satellite(s) => {
                let frame = match (s.state, &self.slots[s.pivot_slot]) {
                    (SatelliteState::Init | SatelliteState::Follow, Some(Actor::Pivot(p))) => p.pair.sat_anim.frame,
                    _ => s.anim.frame,
                };
                let hidden = matches!(&self.slots[s.pivot_slot], Some(Actor::Pivot(p)) if p.state == PivotState::Hidden);
                d(if hidden { 0 } else { 0x80 }, s.color as u8, frame, s.x, hi(s.y))
            }
            Actor::Effect(e) => d(0x80, e.color, e.frame, e.x, hi(e.y)),
            Actor::Burst(b) => d(0x80, b.color, b.anim.frame, b.x, b.y),
            Actor::Piece(p) => d(p.draw, p.color, p.anim.frame, hi(p.motion.x), hi(p.motion.y)),
            Actor::GarbageClear(g) => d(0x80, 6, g.anim.frame, g.x, g.y),
            Actor::Glow(g) => d(g.draw, 2, g.anim.frame, hi(g.motion.x), hi(g.motion.y)),
            Actor::GarbageColumn(c) => d(0x85, 6, c.frame, field_x(c.player, c.col), hi(c.fall.y)),
            Actor::Icon(i) => d(0xA2, 2, i.frame, hi(i.motion.x), hi(i.motion.y)),
            Actor::PreviewBean(v) => d(v.draw, v.color, v.anim.frame, v.x, v.y),
            Actor::Referee(r) => d(r.draw, 0x19, r.anim.frame, r.x, r.y),
            Actor::Sparkle(s) => d(s.draw, 6, 0x0E, hi(s.motion.x), hi(s.motion.y)),
            Actor::DangerSpark(s) => d(s.draw, 0x25, s.anim.frame, hi(s.motion.x), hi(s.motion.y)),
            Actor::Star(s) => d(s.draw, s.color, s.frame, hi(s.motion.x), hi(s.motion.y)),
            Actor::StarSpark(s) => d(s.draw, 0x25, s.anim.frame, hi(s.motion.x), hi(s.motion.y)),
            Actor::PasswordBean(b) => d(0x80, b.color, b.anim.frame, b.x, b.y),
            Actor::ContinueSign(c) => d(0x80, 0x29, c.anim.frame, c.x, c.y),
            Actor::BouncingBean(b) => d(b.draw, b.color, b.frame, hi(b.motion.x), hi(b.motion.y)),
            Actor::BounceSparkle(s) => d(0x80, 6, s.anim.frame, s.x, s.y),
            Actor::BgSlide(b) => d(b.draw, 0x2C, b.frame, b.x, b.y),
            Actor::BgRunner(r) => d(r.draw, 0x2D, r.anim.frame, r.x, r.y),
            Actor::BgSpark(s) => d(0x80, 0x2D, s.frame, s.x, s.y),
            Actor::IntroBoxPart(p) => d(0x80, 0x20, p.frame, p.x, p.y),
            Actor::OpponentIntro(o) => d(0x80, o.mappings, o.anim.frame, o.x, o.y),
            Actor::IntroExtra(IntroExtra::Spark { draw, motion, anim, .. }) => d(*draw, 0x12, anim.frame, hi(motion.x), hi(motion.y)),
            Actor::IntroExtra(IntroExtra::OrbitPiece { draw, frame, x, y, .. }) => d(*draw, 0x0E, *frame, *x, *y),
            Actor::RobotnikIntro(r) => d(0x80, 0x2B, r.frame, r.x, r.y),
            Actor::LairSprite(s) => d(s.draw, s.mappings, s.anim.frame, s.x, s.y),
            Actor::LairFlier(f) => d(f.draw, 0x40, f.anim.frame, hi(f.motion.x), hi(f.motion.y)),
            Actor::ThrownPiece(p) => d(p.draw, 0x2B, 0x1E, hi(p.motion.x), hi(p.motion.y)),
            Actor::IntroShip(s) => d(0x80, 0x40, s.anim.frame, 0xF1, 0xFFE0),
            Actor::OpponentScreen(s) => d(s.draw, OPPONENT_FLASH_MAPPINGS, 1, OPPONENT_FLASH_AT.0, OPPONENT_FLASH_AT.1),
            Actor::FfuzzyEyes(e) => d(e.draw, FFUZZY_EYES_MAPPINGS, e.frame, FFUZZY_EYES_AT.0, FFUZZY_EYES_AT.1),
            // Not sprites: controllers, timers, palette and plane work.
            _ => d(0, 0, 0, 0, 0),
        }
    }
}
