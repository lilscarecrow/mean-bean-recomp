//! Sprite motion: the sine table (`Sin`, `Cos`) and `sub_3810` (velocity, steering
//! toward a target, leaving the screen).

/// `SineTable`: sin over half a turn (128 steps) × 256.
#[rustfmt::skip]
const SINE: [u16; 128] = [
    0x0000, 0x0006, 0x000D, 0x0013, 0x0019, 0x001F, 0x0026, 0x002C, 0x0032, 0x0038, 0x003E, 0x0044, 0x004A, 0x0050, 0x0056, 0x005C,
    0x0062, 0x0068, 0x006D, 0x0073, 0x0079, 0x007E, 0x0084, 0x0089, 0x008E, 0x0093, 0x0098, 0x009D, 0x00A2, 0x00A7, 0x00AC, 0x00B1,
    0x00B5, 0x00B9, 0x00BE, 0x00C2, 0x00C6, 0x00CA, 0x00CE, 0x00D1, 0x00D5, 0x00D8, 0x00DC, 0x00DF, 0x00E2, 0x00E5, 0x00E7, 0x00EA,
    0x00ED, 0x00EF, 0x00F1, 0x00F3, 0x00F5, 0x00F7, 0x00F8, 0x00FA, 0x00FB, 0x00FC, 0x00FD, 0x00FE, 0x00FF, 0x00FF, 0x0100, 0x0100,
    0x0100, 0x0100, 0x0100, 0x00FF, 0x00FF, 0x00FE, 0x00FD, 0x00FC, 0x00FB, 0x00FA, 0x00F8, 0x00F7, 0x00F5, 0x00F3, 0x00F1, 0x00EF,
    0x00ED, 0x00EA, 0x00E7, 0x00E5, 0x00E2, 0x00DF, 0x00DC, 0x00D8, 0x00D5, 0x00D1, 0x00CE, 0x00CA, 0x00C6, 0x00C2, 0x00BE, 0x00B9,
    0x00B5, 0x00B1, 0x00AC, 0x00A7, 0x00A2, 0x009D, 0x0098, 0x0093, 0x008E, 0x0089, 0x0084, 0x007E, 0x0079, 0x0073, 0x006D, 0x0068,
    0x0062, 0x005C, 0x0056, 0x0050, 0x004A, 0x0044, 0x003E, 0x0038, 0x0032, 0x002C, 0x0026, 0x001F, 0x0019, 0x0013, 0x000D, 0x0006,
];

/// `Sin`: sin(angle) × 256 × `scale` as a long (angle: 256 per turn).
pub fn sin(angle: u8, scale: u16) -> u32 {
    let v = u32::from(SINE[(angle & 0x7F) as usize]) * u32::from(scale);
    if angle & 0x80 != 0 { v.wrapping_neg() } else { v }
}

/// `Cos`: a quarter turn ahead of `sin`.
pub fn cos(angle: u8, scale: u16) -> u32 {
    sin(angle.wrapping_add(0x40), scale)
}

/// Draw-flag bits that drive `sub_3810` (actor +$06).
pub mod draw {
    pub const MOVE_Y: u8 = 0x01;
    pub const MOVE_X: u8 = 0x02;
    pub const STEER_Y: u8 = 0x04;
    pub const STEER_X: u8 = 0x08;
    pub const NO_CLIP_Y: u8 = 0x10;
    pub const NO_CLIP_X: u8 = 0x20;
}

/// Position, velocity and steering of a sprite (all 16.16 except targets/steps).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Motion {
    /// aX, aY (+$0A, +$0E).
    pub x: u32,
    pub y: u32,
    /// +$12, +$16.
    pub vx: u32,
    pub vy: u32,
    /// Steering steps (+$1A, +$1C) toward targets (+$1E, +$20).
    pub ax: u16,
    pub ay: u16,
    pub tx: u16,
    pub ty: u16,
}

impl Motion {
    /// `sub_3810`. Returns true when the sprite would leave the screen (sprite space:
    /// x 128-447, y 128-351); the remaining updates are then skipped.
    pub fn step(&mut self, flags: u8) -> bool {
        if flags & draw::MOVE_X != 0 {
            let x = self.x.wrapping_add(self.vx);
            let px = (x >> 16) as u16;
            if flags & draw::NO_CLIP_X == 0 && !(128..448).contains(&px) {
                return true;
            }
            self.x = x;
        }
        if flags & draw::MOVE_Y != 0 {
            let y = self.y.wrapping_add(self.vy);
            let py = (y >> 16) as u16;
            if flags & draw::NO_CLIP_Y == 0 && !(128..352).contains(&py) {
                return true;
            }
            self.y = y;
        }
        if flags & draw::STEER_X != 0 {
            let px = (self.x >> 16) as u16;
            if px < self.tx {
                self.vx = self.vx.wrapping_add(u32::from(self.ax));
            } else if px != self.tx {
                self.vx = self.vx.wrapping_sub(u32::from(self.ax));
            }
        }
        if flags & draw::STEER_Y != 0 {
            let py = (self.y >> 16) as u16;
            if py < self.ty {
                self.vy = self.vy.wrapping_add(u32::from(self.ay));
            } else if py != self.ty {
                self.vy = self.vy.wrapping_sub(u32::from(self.ay));
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_turns() {
        assert_eq!(sin(0x40, 0x1000), 0x10_0000);
        assert_eq!(sin(0xC0, 0x1000), 0x10_0000u32.wrapping_neg());
        assert_eq!(cos(0, 0x1000), 0x10_0000);
        assert_eq!(cos(0x80, 0x1000), 0x10_0000u32.wrapping_neg());
    }

    #[test]
    fn leaving_the_screen_stops_the_update() {
        let mut m = Motion { x: 129 << 16, y: 200 << 16, vx: 0xFFFE_0000, ..Motion::default() };
        assert!(m.step(draw::MOVE_X | draw::MOVE_Y));
        assert_eq!(m.x, 129 << 16);
    }
}
