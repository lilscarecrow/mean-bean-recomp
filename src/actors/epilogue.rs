//! The ending's epilogue: Robotnik's ship on the mountain under fireworks (`loc_FAAE`:
//! sparks from `loc_FBA0`, the sky's colors flickering with `loc_FAE6`), the dawn
//! (`loc_C882`: palette steps, then the credits' sky), and the last lines of text
//! rising up the screen (`loc_B4B6`: up to eight lines at once, each a sprite whose
//! mapping is built in RAM, `byte_FF1446`). Then the high score table.

use super::*;
use crate::assets::sprites::Piece;
use crate::motion;
use crate::rom;

/// `loc_FAE6`: the fireworks' colors on palette line 2, after $2D0 frames.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FireworkColors {
    pub phase: u8,
    pub delay: u16,
    /// +$0A, +$0C, +$0E, +$10: counters whose top bytes pick colors.
    pub a: u16,
    pub b: u16,
    pub c: u16,
    pub d: u16,
}

/// `loc_FBA0`: a burst every so often (`dword_FF1130`: the mask on its random wait,
/// then which colors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FireworkSpawner {
    pub phase: u8,
    pub count: u16,
}

/// `loc_FCA0`: one spark.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FireworkSpark {
    pub draw: u8,
    pub motion: Motion,
    pub anim: Anim,
}

pub const FIREWORK_SPARK_CODE: u32 = 0xFCA0;

/// `loc_C882`: the dawn's palette steps (+$26 the step).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dawn {
    pub phase: u8,
    pub step: u16,
    pub delay: u16,
}

const DAWN_CODES: [u32; 10] = [0xC882, 0xC892, 0xC898, 0xC8CE, 0xC8D4, 0xC902, 0xC90E, 0xC92C, 0xC932, 0xC946];

impl Dawn {
    pub fn code(&self) -> u32 {
        DAWN_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        DAWN_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `loc_B4B6`: the epilogue's text script (+$32 where, +$26 frames to the next line).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Epilogue {
    pub phase: u8,
    pub busy: bool,
    pub ptr: u32,
    pub wait: u16,
    pub delay: u16,
}

const EPILOGUE_CODES: [u32; 7] = [0xB4B6, 0xB4CE, 0xB4D4, 0xB510, 0xB516, 0xB52E, 0xB534];

impl Epilogue {
    pub fn code(&self) -> u32 {
        EPILOGUE_CODES[usize::from(self.phase)]
    }

    pub fn phase_of(code: u32) -> Option<u8> {
        EPILOGUE_CODES.iter().position(|&c| c == code).map(|i| i as u8)
    }
}

/// `loc_B60E`: a line of text rising (mappings 7, frame = its line slot) until y $70;
/// it stops where it is when the script ends ($B646).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreditLine {
    pub stopped: bool,
    pub parent: usize,
    pub line: u8,
    pub motion: Motion,
}

/// `loc_FAAE`'s tables: `unk_FCD6`, `unk_FCE6` (two flickering colors), `unk_FD16`
/// (six colors per burst color), `unk_FCBC` (the sparks' scripts); `word_6F3FE` /
/// `word_6F85E` (the dawn's palettes), `unk_C948`, the credits' sky; `word_B732` and
/// `off_B560` (the epilogue script, the ending's last part by difficulty).
const FLICKER_A: u32 = 0xFCD6;
const FLICKER_B: u32 = 0xFCE6;
const BURST_COLORS: u32 = 0xFD16;
const SPARK_SCRIPTS: u32 = 0xFCBC;
const DAWN_LINE_0: u32 = 0x6F3FE;
const DAWN_LINE_1: u32 = 0x6F85E;
const DAWN_LINE_2: u32 = 0xC948;
const SKY_MAP: u32 = 0x6F158;
const SKY_ART: u32 = 0x6ED46;
const EPILOGUE_SCRIPT: u32 = 0xB732;
const EPILOGUE_BY_DIFFICULTY: u32 = 0xB560;
const FONT: u32 = 0x11258;

fn rom_colors(at: u32) -> [u16; 16] {
    std::array::from_fn(|c| rom::word(at + c as u32 * 2))
}

impl Engine {
    pub(super) fn run_epilogue_routine(&mut self, at: u32) -> bool {
        match at {
            0xFAAE => {
                self.fireworks_stop = 0;
                if let Some(slot) = self.free_slot() {
                    self.slots[slot] = Some(Actor::FireworkSpawner(FireworkSpawner { phase: 0, count: 0x1E0 }));
                    self.fireworks_mask = 0x1FF;
                }
                if let Some(slot) = self.free_slot() {
                    let c = FireworkColors { phase: 0, delay: 0, a: 0, b: 0, c: 0, d: 0 };
                    self.slots[slot] = Some(Actor::FireworkColors(c));
                }
            }
            // sub_C858
            0xC858 => {
                self.line_slots = [0; 8];
                if let Some(slot) = self.find_actor_slot() {
                    let e = Epilogue { phase: 0, busy: false, ptr: 0, wait: 0, delay: 0 };
                    self.slots[slot] = Some(Actor::Epilogue(e));
                }
                if let Some(slot) = self.find_actor_slot() {
                    self.slots[slot] = Some(Actor::Dawn(Dawn { phase: 0, step: 0, delay: 0 }));
                }
            }
            _ => return false,
        }
        true
    }

    pub(super) fn run_firework_colors(&mut self, mut f: FireworkColors) -> Option<FireworkColors> {
        match f.phase {
            0 => {
                f.delay = 0x2D0;
                f.phase = 1;
            }
            1 => {
                f.delay -= 1;
                if f.delay == 0 {
                    f.phase = 2;
                }
            }
            _ => {
                f.a = f.a.wrapping_add(0x80);
                f.b = f.b.wrapping_add(0xC0);
                f.c = f.c.wrapping_add(0x60);
                f.d = f.d.wrapping_add(0x100);
                self.palettes[2][4] = rom::word(FLICKER_A + u32::from((f.a >> 8) & 7) * 2);
                self.palettes[2][6] = rom::word(FLICKER_B + u32::from((f.c >> 8) & 7) * 2);
                let at = BURST_COLORS + u32::from(self.fireworks_color) * 12;
                for (i, c) in [3, 10, 12, 13, 14, 15].into_iter().enumerate() {
                    self.palettes[2][c] = rom::word(at + i as u32 * 2);
                }
                self.load_palette(2, PaletteLoad::Buffer);
                if self.fireworks_stop != 0 {
                    return None;
                }
            }
        }
        Some(f)
    }

    pub(super) fn run_firework_spawner(&mut self, mut s: FireworkSpawner) -> Option<FireworkSpawner> {
        match s.phase {
            0 => {
                s.count = s.count.wrapping_sub(1);
                if s.count & 0x8000 != 0 {
                    s.phase = 1;
                }
            }
            1 => self.firework_pause(&mut s),
            _ => {
                if self.fireworks_stop != 0 {
                    return None;
                }
                s.count = s.count.wrapping_sub(1);
                if s.count & 0x8000 == 0 {
                    return Some(s);
                }
                if let Some(slot) = self.find_actor_slot() {
                    let x = (self.random() & 0x3F).wrapping_sub(0x1F).wrapping_add(0x160);
                    let y = (self.random() & 0x1F).wrapping_sub(0x0F).wrapping_sub(0x50);
                    let script = SPARK_SCRIPTS + u32::from(self.random() & 3) * 2;
                    let scale = (self.random() & 0x1FF) + 0x100;
                    let angle = ((self.random() & 0x3F) + 0xC9) as u8;
                    let motion = Motion {
                        x: u32::from(x) << 16,
                        y: u32::from(y) << 16,
                        vx: motion::sin(angle, scale),
                        vy: motion::cos(angle, scale),
                        ..Motion::default()
                    };
                    let anim = Anim { ptr: script, time: 1, frame: 0x49 };
                    self.slots[slot] = Some(Actor::FireworkSpark(FireworkSpark { draw: 0x80, motion, anim }));
                }
                self.firework_pause(&mut s);
            }
        }
        Some(s)
    }

    /// $FBAE: the next burst's color and wait.
    fn firework_pause(&mut self, s: &mut FireworkSpawner) {
        self.fireworks_color = self.random() & 7;
        s.count = (self.random() & self.fireworks_mask) + 0x3C;
        s.phase = 2;
    }

    pub(super) fn run_firework_spark(&mut self, mut s: FireworkSpark) -> Option<FireworkSpark> {
        s.draw = 0xBF;
        s.motion.step(s.draw);
        if s.anim.step_with(&mut || self.random()) {
            return None;
        }
        Some(s)
    }

    pub(super) fn run_dawn(&mut self, mut d: Dawn) -> Option<Dawn> {
        match d.phase {
            0 => {
                d.step = 0;
                d.delay = 0xF0;
                d.phase = 1;
            }
            // ActorBookmark_SetDelay's waits.
            1 | 3 | 7 => {
                d.delay -= 1;
                if d.delay == 0 {
                    d.phase += 1;
                }
            }
            2 => self.dawn_step(&mut d),
            4 => {
                d.step += 1;
                if d.step != 0x1B {
                    self.dawn_step(&mut d);
                } else {
                    self.load_palette(2, PaletteLoad::Colors(rom_colors(DAWN_LINE_2)));
                    // sub_BDCA: plane B's scroll cleared, the clouds gone.
                    for i in 0..0x81 {
                        self.hscroll[1 + i * 2] = 0;
                    }
                    if let Some(c) = self.slots.get_mut(self.clouds_slot) {
                        *c = None;
                    }
                    // sub_F794
                    self.eni_dec_safe(SKY_MAP, 0x4000, 0x27, 9, 0xF600);
                    d.phase = 5;
                }
            }
            5 => {
                // sub_F7B8
                self.long_step();
                self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Nemesis { vram: 0, art: SKY_ART } });
                d.phase = 6;
            }
            6 => self.dawn_sky_step(&mut d),
            8 => {
                d.step += 1;
                if d.step != 0x23 {
                    self.dawn_sky_step(&mut d);
                } else {
                    d.phase = 9;
                }
            }
            _ => {}
        }
        Some(d)
    }

    /// $C898: the step's colors on lines 0 and 1, then $0C frames.
    fn dawn_step(&mut self, d: &mut Dawn) {
        let at = u32::from(d.step) * 0x20;
        self.load_palette(0, PaletteLoad::Colors(rom_colors(DAWN_LINE_0 + at)));
        self.load_palette(1, PaletteLoad::Colors(rom_colors(DAWN_LINE_1 + at)));
        d.delay = 0x0C;
        d.phase = 3;
    }

    /// $C90E: the step's colors on line 2, then $1E frames.
    fn dawn_sky_step(&mut self, d: &mut Dawn) {
        let at = u32::from(d.step) * 0x20;
        self.load_palette(2, PaletteLoad::Colors(rom_colors(DAWN_LINE_1 + at)));
        d.delay = 0x1E;
        d.phase = 7;
    }

    pub(super) fn run_epilogue(&mut self, slot: usize, mut e: Epilogue) -> Option<Epilogue> {
        match e.phase {
            0 => {
                e.busy = true;
                e.ptr = EPILOGUE_SCRIPT;
                e.delay = 0x300;
                e.phase = 1;
            }
            1 | 3 | 5 => {
                e.delay -= 1;
                if e.delay == 0 {
                    e.phase += 1;
                }
            }
            2 => {
                if e.wait != 0 {
                    e.wait -= 1;
                    return Some(e);
                }
                let n = rom::word(e.ptr);
                if n == 0 {
                    // loc_B500: the script's end; the lines stop.
                    e.busy = false;
                    e.delay = 0xBB8;
                    e.phase = 3;
                } else if n & 0x8000 != 0 {
                    let d = u32::from(self.difficulty_option & 3);
                    e.ptr = rom::long(EPILOGUE_BY_DIFFICULTY + d * 4);
                } else {
                    e.wait = n * 2;
                    let text = rom::long(e.ptr + 2);
                    e.ptr += 6;
                    self.spawn_credit_line(slot, text);
                }
            }
            4 => {
                self.fireworks_mask = 0x3F;
                e.delay = 0x12C;
                e.phase = 5;
            }
            _ => {
                self.fireworks_stop = 0xFFFF;
                self.bytecode_disabled = false;
                self.bytecode_flag = 0;
                return None;
            }
        }
        Some(e)
    }

    /// `loc_B570`: a line in the highest free line slot, its mapping built from the text
    /// (x, count, characters: spaces only move on 8 pixels).
    fn spawn_credit_line(&mut self, parent: usize, text: u32) {
        let Some(line) = (0..8).rev().find(|&i| self.line_slots[i] == 0) else { return };
        self.line_slots[line] = 0xFF;
        let Some(slot) = self.find_actor_slot() else { return };
        let motion = Motion { x: u32::from(rom::word(text)) << 16, y: 0x160 << 16, vy: 0xFFFF_8000, ..Motion::default() };
        self.slots[slot] = Some(Actor::CreditLine(CreditLine { stopped: false, parent, line: line as u8, motion }));
        let count = rom::word(text + 2);
        let mut pieces = vec![];
        let (mut at, mut x) = (text + 4, 0u16);
        while pieces.len() < usize::from(count) {
            let tile = rom::byte(FONT + u32::from(rom::byte(at)));
            at += 1;
            if tile != 0 {
                pieces.push(Piece { y: 0x120, size: 0, layer: 0, tile: 0x8500 | u16::from(tile), x });
            }
            x = x.wrapping_add(8);
        }
        self.line_maps[line] = pieces;
    }

    pub(super) fn run_credit_line(&mut self, mut l: CreditLine) -> Option<CreditLine> {
        if l.stopped {
            return Some(l);
        }
        let busy = matches!(&self.slots[l.parent], Some(Actor::Epilogue(e)) if e.busy);
        if !busy {
            l.stopped = true;
            return Some(l);
        }
        l.motion.step(0x91);
        if ((l.motion.y >> 16) as u16) < 0x70 {
            self.line_slots[usize::from(l.line)] = 0;
            return None;
        }
        Some(l)
    }
}
