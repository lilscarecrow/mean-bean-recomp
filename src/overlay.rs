//! Drawing over the game's picture (not in the ROM): a small pixel font, boxes and
//! dimming, on the RGBA frame [`Game::picture`](crate::game::Game::picture) returns.
//! The network menus use it; nothing here comes from the game's own art.

use crate::render::{HEIGHT, WIDTH};

pub type Rgb = [u8; 3];

pub const WHITE: Rgb = [0xEE, 0xEE, 0xEE];
pub const YELLOW: Rgb = [0xEE, 0xCC, 0x22];
pub const GREY: Rgb = [0x88, 0x88, 0x88];
pub const GREEN: Rgb = [0x44, 0xDD, 0x66];
pub const RED: Rgb = [0xEE, 0x55, 0x44];
pub const PANEL: Rgb = [0x10, 0x18, 0x40];
pub const BORDER: Rgb = [0x66, 0x88, 0xEE];

/// Glyph cell: 5 x 7 pixels, drawn 6 apart.
pub const GLYPH_W: i32 = 6;
pub const GLYPH_H: i32 = 7;

/// The font: each glyph is 7 rows of 5 pixels, `#` lit.
const FONT: &[(char, [&str; 7])] = &[
    ('A', [" ### ", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('B', ["#### ", "#   #", "#   #", "#### ", "#   #", "#   #", "#### "]),
    ('C', [" ### ", "#   #", "#    ", "#    ", "#    ", "#   #", " ### "]),
    ('D', ["#### ", "#   #", "#   #", "#   #", "#   #", "#   #", "#### "]),
    ('E', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#####"]),
    ('F', ["#####", "#    ", "#    ", "#### ", "#    ", "#    ", "#    "]),
    ('G', [" ### ", "#   #", "#    ", "# ###", "#   #", "#   #", " ####"]),
    ('H', ["#   #", "#   #", "#   #", "#####", "#   #", "#   #", "#   #"]),
    ('I', [" ### ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('J', ["  ###", "   # ", "   # ", "   # ", "   # ", "#  # ", " ##  "]),
    ('K', ["#   #", "#  # ", "# #  ", "##   ", "# #  ", "#  # ", "#   #"]),
    ('L', ["#    ", "#    ", "#    ", "#    ", "#    ", "#    ", "#####"]),
    ('M', ["#   #", "## ##", "# # #", "# # #", "#   #", "#   #", "#   #"]),
    ('N', ["#   #", "#   #", "##  #", "# # #", "#  ##", "#   #", "#   #"]),
    ('O', [" ### ", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('P', ["#### ", "#   #", "#   #", "#### ", "#    ", "#    ", "#    "]),
    ('Q', [" ### ", "#   #", "#   #", "#   #", "# # #", "#  # ", " ## #"]),
    ('R', ["#### ", "#   #", "#   #", "#### ", "# #  ", "#  # ", "#   #"]),
    ('S', [" ####", "#    ", "#    ", " ### ", "    #", "    #", "#### "]),
    ('T', ["#####", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('U', ["#   #", "#   #", "#   #", "#   #", "#   #", "#   #", " ### "]),
    ('V', ["#   #", "#   #", "#   #", "#   #", "#   #", " # # ", "  #  "]),
    ('W', ["#   #", "#   #", "#   #", "# # #", "# # #", "# # #", " # # "]),
    ('X', ["#   #", "#   #", " # # ", "  #  ", " # # ", "#   #", "#   #"]),
    ('Y', ["#   #", "#   #", " # # ", "  #  ", "  #  ", "  #  ", "  #  "]),
    ('Z', ["#####", "    #", "   # ", "  #  ", " #   ", "#    ", "#####"]),
    ('0', [" ### ", "#   #", "#  ##", "# # #", "##  #", "#   #", " ### "]),
    ('1', ["  #  ", " ##  ", "  #  ", "  #  ", "  #  ", "  #  ", " ### "]),
    ('2', [" ### ", "#   #", "    #", "   # ", "  #  ", " #   ", "#####"]),
    ('3', ["#####", "   # ", "  #  ", "   # ", "    #", "#   #", " ### "]),
    ('4', ["   # ", "  ## ", " # # ", "#  # ", "#####", "   # ", "   # "]),
    ('5', ["#####", "#    ", "#### ", "    #", "    #", "#   #", " ### "]),
    ('6', ["  ## ", " #   ", "#    ", "#### ", "#   #", "#   #", " ### "]),
    ('7', ["#####", "    #", "   # ", "  #  ", " #   ", " #   ", " #   "]),
    ('8', [" ### ", "#   #", "#   #", " ### ", "#   #", "#   #", " ### "]),
    ('9', [" ### ", "#   #", "#   #", " ####", "    #", "   # ", " ##  "]),
    ('.', ["     ", "     ", "     ", "     ", "     ", " ##  ", " ##  "]),
    (',', ["     ", "     ", "     ", "     ", " ##  ", "  #  ", " #   "]),
    (':', ["     ", " ##  ", " ##  ", "     ", " ##  ", " ##  ", "     "]),
    ('-', ["     ", "     ", "     ", "#####", "     ", "     ", "     "]),
    ('_', ["     ", "     ", "     ", "     ", "     ", "     ", "#####"]),
    ('/', ["    #", "    #", "   # ", "  #  ", " #   ", "#    ", "#    "]),
    ('(', ["   # ", "  #  ", " #   ", " #   ", " #   ", "  #  ", "   # "]),
    (')', [" #   ", "  #  ", "   # ", "   # ", "   # ", "  #  ", " #   "]),
    ('[', [" ### ", " #   ", " #   ", " #   ", " #   ", " #   ", " ### "]),
    (']', [" ### ", "   # ", "   # ", "   # ", "   # ", "   # ", " ### "]),
    ('!', ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "     ", "  #  "]),
    ('?', [" ### ", "#   #", "    #", "   # ", "  #  ", "     ", "  #  "]),
    ('\'', ["  #  ", "  #  ", " #   ", "     ", "     ", "     ", "     "]),
    ('>', [" #   ", "  #  ", "   # ", "    #", "   # ", "  #  ", " #   "]),
    ('<', ["   # ", "  #  ", " #   ", "#    ", " #   ", "  #  ", "   # "]),
    ('|', ["  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  ", "  #  "]),
];

fn glyph(c: char) -> Option<&'static [&'static str; 7]> {
    let c = c.to_ascii_uppercase();
    FONT.iter().find(|(g, _)| *g == c).map(|(_, rows)| rows)
}

/// A rectangle in picture pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, x: i32, y: i32) -> bool {
        x >= self.x && x < self.x + self.w && y >= self.y && y < self.y + self.h
    }
}

/// An RGBA picture to draw on (`WIDTH` x `HEIGHT`).
pub struct Canvas<'a>(pub &'a mut [u8]);

impl Canvas<'_> {
    pub fn pixel(&mut self, x: i32, y: i32, c: Rgb) {
        if (0..WIDTH as i32).contains(&x) && (0..HEIGHT as i32).contains(&y) {
            let at = (y as usize * WIDTH + x as usize) * 4;
            self.0[at..at + 3].copy_from_slice(&c);
        }
    }

    pub fn fill(&mut self, r: Rect, c: Rgb) {
        for y in r.y..r.y + r.h {
            for x in r.x..r.x + r.w {
                self.pixel(x, y, c);
            }
        }
    }

    pub fn frame(&mut self, r: Rect, c: Rgb) {
        self.fill(Rect { h: 1, ..r }, c);
        self.fill(Rect { y: r.y + r.h - 1, h: 1, ..r }, c);
        self.fill(Rect { w: 1, ..r }, c);
        self.fill(Rect { x: r.x + r.w - 1, w: 1, ..r }, c);
    }

    /// A box with a border.
    pub fn panel(&mut self, r: Rect) {
        self.fill(r, PANEL);
        self.frame(r, BORDER);
    }

    /// Darkens the whole picture (to a quarter), so a panel stands out.
    pub fn dim(&mut self) {
        for px in self.0.chunks_mut(4) {
            for v in &mut px[..3] {
                *v /= 4;
            }
        }
    }

    /// Text at (x, y), `scale` pixels per font pixel, with a one-pixel shadow.
    /// Characters the font lacks draw as nothing.
    pub fn text(&mut self, x: i32, y: i32, s: &str, c: Rgb, scale: i32) {
        for (i, ch) in s.chars().enumerate() {
            let Some(rows) = glyph(ch) else { continue };
            let gx = x + i as i32 * GLYPH_W * scale;
            for (row, bits) in rows.iter().enumerate() {
                for (col, b) in bits.bytes().enumerate() {
                    if b != b'#' {
                        continue;
                    }
                    let (px, py) = (gx + col as i32 * scale, y + row as i32 * scale);
                    self.fill(Rect { x: px + 1, y: py + 1, w: scale, h: scale }, [0, 0, 0]);
                    self.fill(Rect { x: px, y: py, w: scale, h: scale }, c);
                }
            }
        }
    }

    /// Text centered on x = `cx`.
    pub fn text_centered(&mut self, cx: i32, y: i32, s: &str, c: Rgb, scale: i32) {
        self.text(cx - text_width(s, scale) / 2, y, s, c, scale);
    }
}

/// How wide `s` draws at `scale`.
pub fn text_width(s: &str, scale: i32) -> i32 {
    (s.chars().count() as i32 * GLYPH_W - 1).max(0) * scale
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glyphs_are_5_wide() {
        for (c, rows) in FONT {
            assert!(rows.iter().all(|r| r.len() == 5), "{c}");
        }
    }

    #[test]
    fn text_lands_where_asked_and_stays_on_the_picture() {
        let mut px = vec![0u8; WIDTH * HEIGHT * 4];
        let mut c = Canvas(&mut px);
        c.text(10, 20, "I", WHITE, 1);
        // 'I' has its top bar at columns 1-3 of row 0.
        let at = |x: usize, y: usize| px[(y * WIDTH + x) * 4];
        assert_eq!(at(12, 20), WHITE[0]);
        assert_eq!(at(10, 20), 0);
        // Off the edges: nothing panics.
        Canvas(&mut px).text(-20, -3, "HELLO", WHITE, 2);
        Canvas(&mut px).text(315, 220, "HELLO", WHITE, 2);
    }

    #[test]
    fn lower_case_draws_as_upper() {
        assert_eq!(glyph('a'), glyph('A'));
        assert!(glyph('~').is_none());
    }
}
