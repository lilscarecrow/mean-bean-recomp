//! Pause (`CheckPause`, before the controllers are read each frame, so on the last
//! frame's presses): START toggles bit 7 of a side's `player_1_flags` (only while that
//! side plays: the flag nonzero). Outside exercise mode either pad pauses both sides,
//! and only the pad that paused (`p1_paused`) unpauses. A paused side's actors (tagged
//! $F1 / $F2 by their player) neither run nor draw, its field's tiles are saved and
//! blanked (`PausePuyoField`), PAUSE blinks over it (`ActPause`), and with both sides
//! paused the referee plays its own animation (`loc_5E86`). Each toggle waits for the
//! next vertical blank (`VSync`) before going on, and then sounds: in exercise mode
//! $54 each time, otherwise `PauseSound` / `UnpauseSound` (once each per check).

use super::*;
use crate::assets::planes::FIELD_ORIGIN;

/// `ActPause` / `ActPause_Update`.
pub const PAUSE_SIGN_CODE: u32 = 0x92A8;
pub const PAUSE_SIGN_UPDATE_CODE: u32 = 0x92D2;
/// `loc_5EB6`: the referee's pause animation (`unk_5ED6`).
pub const REFEREE_PAUSE_CODE: u32 = 0x5EB6;
const REFEREE_PAUSE_SCRIPT: u32 = 0x5ED6;
/// The field's tiles: 24 rows of 12 words, filled with $8500 while paused.
const FIELD_ROWS: u16 = 0x18;
const FIELD_COLS: u16 = 0x0C;
const BLANK: u16 = 0x8500;

/// `ActPause`: PAUSE over a side's field (+$26 its blink counter).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PauseSign {
    pub started: bool,
    pub side: u8,
    pub draw: u8,
    pub x: u16,
    pub y: u16,
    pub blink: u8,
}

/// `loc_5EB6`: while both sides are paused, the referee's frames come from here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefereePause {
    pub referee: usize,
    pub anim: Anim,
}

impl Engine {
    /// The side whose player an actor belongs to (its tag $F1 / $F2), if any.
    pub fn actor_side(a: &Actor) -> Option<u8> {
        let side = match a {
            Actor::Player(p) => p.id,
            Actor::Pivot(v) => v.player_id,
            Actor::Satellite(s) => s.player_id,
            Actor::Effect(e) => e.player,
            Actor::Burst(b) => b.player,
            Actor::Piece(p) => p.player,
            Actor::GarbageClear(g) => g.player,
            Actor::GarbageDrop(d) => d.player,
            Actor::GarbageColumn(c) => c.player,
            Actor::Glow(g) => g.player,
            Actor::Special(s) => s.id,
            Actor::Debris(d) => d.side,
            Actor::FieldShake(f) => f.side,
            _ => return None,
        };
        Some(side & 1)
    }

    /// Whether the actor in `slot` runs and draws (its side isn't paused).
    pub fn awake(&self, slot: usize) -> bool {
        match self.slots[slot].as_ref().and_then(Self::actor_side) {
            Some(side) => self.player_flags[usize::from(side)] & 0x80 == 0,
            None => true,
        }
    }

    /// `CheckPause`. True when this frame ends in its wait for the vertical blank.
    pub(super) fn check_pause(&mut self) -> bool {
        let press = self.pause_press;
        if let Some(paused) = self.pause_sound.take() {
            self.pause_sounds(paused);
        }
        let (side, d1) = match self.pause_next.take() {
            Some(rest) => rest,
            None => {
                self.pause_sounds_sent = 0; // (d2's upper bits)
                let (mut d0, mut d1) = (press[0], press[1]);
                if self.swap_controls != 0 {
                    std::mem::swap(&mut d0, &mut d1);
                }
                if self.level_mode & 2 == 0 {
                    d0 |= d1;
                    d1 = d0;
                    if self.player_flags[0] & 0x80 != 0 {
                        // (only the pad that paused)
                        d0 = if self.p1_paused != 0 { press[1] } else { press[0] };
                        d1 = d0;
                    }
                }
                if self.toggle_pause(0, d0) {
                    self.pause_next = Some((1, d1));
                    return true;
                }
                (1, d1)
            }
        };
        if side == 1 && self.toggle_pause(1, d1) {
            self.pause_next = Some((2, 0));
            return true;
        }
        false
    }

    /// After the toggle's `VSync`.
    fn pause_sounds(&mut self, paused: bool) {
        if self.level_mode & 2 != 0 {
            self.play_sample(0x54); // SFX_GARBAGE_1
            return;
        }
        let bit = if paused { 1 } else { 2 };
        if self.pause_sounds_sent & bit == 0 {
            self.pause_sounds_sent |= bit;
            self.sound_command(0xFF); // PauseSound / UnpauseSound
        }
    }

    /// `CheckPlayerPause`. True when the side's pause changed.
    fn toggle_pause(&mut self, side: usize, press: u8) -> bool {
        if self.player_flags[side] == 0 || press & 0x80 == 0 {
            return false;
        }
        self.player_flags[side] ^= 0x80;
        self.pause_sound = Some(self.player_flags[side] & 0x80 != 0);
        let at = FIELD_ORIGIN[(side ^ usize::from(self.swap_controls)) & 1];
        let event = |e: &mut Engine, what: VdpOp| e.vdp_events.push(VdpEvent { at: e.plane_writes.len(), what });
        let field = VdpOp::SaveBox { at, cols: FIELD_COLS, rows: FIELD_ROWS, row: 0x80, buffer: 1 + side as u8 };
        if self.player_flags[side] & 0x80 != 0 {
            self.p1_paused = (self.pause_press[0] & 0x80) ^ 0x80;
            if let Some(slot) = self.find_actor_slot() {
                self.slots[slot] = Some(Actor::PauseSign(PauseSign { started: false, side: side as u8, draw: 0, x: 0, y: 0, blink: 0 }));
            }
            // PausePuyoField
            event(self, field);
            for r in 0..FIELD_ROWS {
                for c in 0..FIELD_COLS {
                    self.plane_writes.push((at.wrapping_add(r * 0x80).wrapping_add(c * 2), BLANK));
                }
            }
        } else if let VdpOp::SaveBox { at, cols, rows, row, buffer } = field {
            // UnpausePuyoField
            event(self, VdpOp::RestoreBox { at, cols, rows, row, buffer });
        }
        true
    }

    pub(super) fn run_pause_sign(&mut self, mut s: PauseSign) -> Option<PauseSign> {
        let side = usize::from(s.side & 1);
        if !s.started {
            s.started = true;
            s.x = FIELD_X[(side ^ usize::from(self.swap_controls)) & 1] + 0x30;
            s.y = FIELD_TOP_Y + 0x50;
            return Some(s);
        }
        if self.player_flags[side] & 0x80 == 0 {
            return None;
        }
        s.blink = (s.blink + 1) & 0x3F;
        s.draw = if s.blink < 0x30 { 0x80 } else { 0 };
        Some(s)
    }

    /// `loc_5E86`: both sides paused; the referee waits (False when there's no slot).
    pub(super) fn pause_referee(&mut self, referee: usize) -> bool {
        let Some(slot) = self.find_actor_slot() else { return false };
        self.slots[slot] = Some(Actor::RefereePause(RefereePause { referee, anim: Anim::new(REFEREE_PAUSE_SCRIPT) }));
        true
    }

    pub(super) fn both_paused(&self) -> bool {
        self.player_flags[0] & self.player_flags[1] & 0x80 != 0
    }

    pub(super) fn run_referee_pause(&mut self, mut p: RefereePause) -> Option<RefereePause> {
        if !self.both_paused() {
            return None;
        }
        p.anim.step_with(&mut || 0);
        if let Some(Actor::Referee(r)) = &mut self.slots[p.referee] {
            r.anim.frame = p.anim.frame;
        }
        Some(p)
    }
}
