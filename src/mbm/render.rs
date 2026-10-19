//! A software VDP for the screens the port shows: two scrolling planes and the sprite
//! table, with priorities and shadow/highlight, from a 64 KiB VRAM, the four palette
//! lines and the scroll values. Output is 320 x 224.
//!
//! Each pixel comes out as a CRAM color plus a brightness (shadow, normal, highlight),
//! as 3 x 4-bit intensity levels (0-14: shadow = v, normal = 2v, highlight = 7 + v);
//! [`to_rgb`] turns those into 8-bit RGB.

pub const WIDTH: usize = 320;
pub const HEIGHT: usize = 224;

/// The VDP setup the renderer needs (the registers the game sets for a screen).
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub plane_a: u16,
    pub plane_b: u16,
    pub sprites: u16,
    pub hscroll: u16,
    /// Plane size in cells (64 x 64 on the match screen).
    pub plane_width: u16,
    pub plane_height: u16,
    /// Background color: palette line * 16 + index (register 7).
    pub backdrop: u8,
    pub shadow_highlight: bool,
    /// Vertical scroll per 2-cell column (register 11 bit 2) instead of whole planes.
    pub column_vscroll: bool,
}

impl Layout {
    /// From the VDP registers 0-23 (the game keeps a copy in RAM, `vdp_reg_0`..).
    pub fn from_registers(r: &[u8]) -> Layout {
        let size = |bits: u8| match bits & 3 {
            0 => 32,
            1 => 64,
            _ => 128,
        };
        Layout {
            plane_a: u16::from(r[2] & 0x38) << 10,
            plane_b: u16::from(r[4] & 7) << 13,
            sprites: u16::from(r[5] & 0x7F) << 9,
            hscroll: u16::from(r[13] & 0x3F) << 10,
            plane_width: size(r[16]),
            plane_height: size(r[16] >> 4),
            backdrop: r[7] & 0x3F,
            shadow_highlight: r[12] & 8 != 0,
            column_vscroll: r[11] & 4 != 0,
        }
    }

    /// The match screen (`SetupVDPRegs` table 1 with shadow/highlight on).
    pub fn match_screen() -> Layout {
        Layout::from_registers(&[0x04, 0x64, 0x30, 0x3C, 0x07, 0x5E, 0, 0, 0, 0, 0, 0x04, 0x89, 0x2E, 0, 2, 0x11, 0, 0, 0, 0, 0, 0, 0])
    }
}

/// One pixel: CRAM color (9-bit, `----bbb-ggg-rrr-`) and brightness.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Pixel {
    pub color: u16,
    /// 0 shadow, 1 normal, 2 highlight.
    pub light: u8,
}

/// The intensity levels (r, g, b), 0-14, of a pixel.
pub fn levels(p: Pixel) -> [u8; 3] {
    let level = |shift: u16| {
        let v = ((p.color >> shift) & 0x0E) as u8 >> 1;
        match p.light {
            0 => v,
            1 => v * 2,
            _ => 7 + v,
        }
    };
    [level(0), level(4), level(8)]
}

/// 8-bit RGB for a pixel.
pub fn to_rgb(p: Pixel) -> [u8; 3] {
    levels(p).map(|l| l * 17)
}

struct Cell {
    index: u8,
    line: u8,
    priority: bool,
}

fn pattern_pixel(vram: &[u8], word: u16, px: usize, py: usize) -> u8 {
    let tile = usize::from(word & 0x07FF);
    let x = if word & 0x0800 != 0 { 7 - px } else { px };
    let y = if word & 0x1000 != 0 { 7 - py } else { py };
    let byte = vram[(tile * 32 + y * 4 + x / 2) & 0xFFFF];
    if x % 2 == 0 { byte >> 4 } else { byte & 0x0F }
}

fn word(vram: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([vram[at & 0xFFFF], vram[(at + 1) & 0xFFFF]])
}

/// Renders a frame: `vram`, the 64 CRAM colors, and `vsram` (2 words per 2-cell column:
/// plane A then B; or the first two for whole planes).
pub fn render(layout: &Layout, vram: &[u8], cram: &[u16; 64], vsram: &[u16]) -> Vec<Pixel> {
    let (pw, ph) = (usize::from(layout.plane_width), usize::from(layout.plane_height));
    let hscroll = |plane: usize| word(vram, usize::from(layout.hscroll) + plane * 2) & 0x3FF;
    let plane_cell = |plane: usize, x: usize, y: usize| -> Cell {
        let base = usize::from(if plane == 0 { layout.plane_a } else { layout.plane_b });
        let column = if layout.column_vscroll { (x / 16) * 2 } else { 0 };
        let vs = usize::from(vsram.get(column + plane).copied().unwrap_or(0) & 0x3FF);
        let px = (x + 0x400 - usize::from(hscroll(plane))) % (pw * 8);
        let py = (y + vs) % (ph * 8);
        let w = word(vram, base + ((py / 8) * pw + px / 8) * 2);
        Cell { index: pattern_pixel(vram, w, px % 8, py % 8), line: ((w >> 13) & 3) as u8, priority: w & 0x8000 != 0 }
    };
    // The sprite table in link order (the first drawn is in front).
    let mut sprites = vec![];
    let mut link = 0usize;
    for _ in 0..80 {
        let at = usize::from(layout.sprites) + link * 8;
        let y = usize::from(word(vram, at) & 0x3FF);
        let size = vram[(at + 2) & 0xFFFF];
        let next = usize::from(vram[(at + 3) & 0xFFFF] & 0x7F);
        let attr = word(vram, at + 4);
        let x = usize::from(word(vram, at + 6) & 0x1FF);
        sprites.push((x, y, size, attr));
        if next == 0 {
            break;
        }
        link = next;
    }
    let mut out = Vec::with_capacity(WIDTH * HEIGHT);
    for y in 0..HEIGHT {
        for x in 0..WIDTH {
            let (b, a) = (plane_cell(1, x, y), plane_cell(0, x, y));
            // Sprite pixel: the first opaque one in link order.
            let mut sprite: Option<Cell> = None;
            for &(sx, sy, size, attr) in &sprites {
                let (w, h) = (usize::from((size >> 2) & 3) + 1, usize::from(size & 3) + 1);
                let (rx, ry) = ((x + 128).wrapping_sub(sx), (y + 128).wrapping_sub(sy));
                if rx >= w * 8 || ry >= h * 8 {
                    continue;
                }
                let (cx, cy) = (rx / 8, ry / 8);
                let cx = if attr & 0x0800 != 0 { w - 1 - cx } else { cx };
                let cy = if attr & 0x1000 != 0 { h - 1 - cy } else { cy };
                let tile = (attr & 0x07FF).wrapping_add((cx * h + cy) as u16);
                let index = pattern_pixel(vram, (attr & 0xF800) | (tile & 0x07FF), rx % 8, ry % 8);
                if index != 0 {
                    sprite = Some(Cell { index, line: ((attr >> 13) & 3) as u8, priority: attr & 0x8000 != 0 });
                    break;
                }
            }
            let color = |c: &Cell| cram[usize::from(c.line) * 16 + usize::from(c.index)];
            // Back to front: backdrop, B low, A low, sprites low, B high, A high, sprites high.
            let mut top = cram[usize::from(layout.backdrop)];
            let mut from_sprite: Option<&Cell> = None;
            for pri in [false, true] {
                if b.priority == pri && b.index != 0 {
                    top = color(&b);
                    from_sprite = None;
                }
                if a.priority == pri && a.index != 0 {
                    top = color(&a);
                    from_sprite = None;
                }
                if let Some(s) = sprite.as_ref().filter(|s| s.priority == pri) {
                    top = color(s);
                    from_sprite = Some(s);
                }
            }
            let mut light = 1;
            if layout.shadow_highlight {
                let planes_low = !a.priority && !b.priority;
                light = if planes_low { 0 } else { 1 };
                if let Some(s) = from_sprite {
                    if s.line == 3 && s.index >= 14 {
                        // Operators: 14 highlights, 15 shadows what is underneath.
                        let under = Pixel { color: 0, light };
                        light = if s.index == 14 { (under.light + 1).min(2) } else { 0 };
                        top = {
                            // The color under the operator.
                            let mut t = cram[usize::from(layout.backdrop)];
                            for pri in [false, true] {
                                if b.priority == pri && b.index != 0 {
                                    t = color(&b);
                                }
                                if a.priority == pri && a.index != 0 {
                                    t = color(&a);
                                }
                            }
                            t
                        };
                    } else if s.priority {
                        light = 1;
                    }
                }
            }
            out.push(Pixel { color: top, light });
        }
    }
    out
}
