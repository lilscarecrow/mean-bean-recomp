//! A scenario-mode stage on the engine, as the front end drives it: the start sequence
//! the game flow runs (`GenPuyoOrder`, `loc_3C00`, `loc_6070`), one engine frame per
//! video frame with the two pads, and the picture: the match screen's VRAM kept up to
//! date from the engine's plane writes, the sprite table built like `DrawActors`, and
//! the software VDP.

use super::actors::{Actor, Engine, Inputs, Unported, SLOTS};
use super::assets::{planes, screen, sprites, Rom, RomAssets, Vram};
use super::pad::Pad;
use super::render::{self, Layout};
use super::rng::Rng;

/// `Opponents`: who you meet on each level.
const OPPONENTS: u32 = 0x278E;
/// `unk_DF8E`: the CPU's `byte_FF0104` by the difficulty option.
const CPU_DIFFICULTY: [u8; 4] = [0, 2, 4, 6];
/// `BWRITE pressed_time, $0802` in `BC_Level`: the pads' repeat times for a match.
const REPEAT_TIMES: (u8, u8) = (8, 2);

/// What the stage is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Starting(u8),
    Playing,
    /// The match is over; P1 won (next level) or not.
    Over { p1_won: bool },
    /// The engine reached something not ported yet.
    Stopped(String),
}

pub struct Game {
    pub engine: Engine,
    pub status: Status,
    pub level: u8,
    rom: Rom<'static>,
    vram: Vram,
    palettes: [[u16; 16]; 4],
    pads: [Pad; 2],
    /// `draw_order`: DrawActors walks the slots forwards and backwards on alternate frames.
    backwards: bool,
    refugee_frame: u8,
}

impl Game {
    /// A scenario stage at `level` (3 = stage 1), with the difficulty option (0-3), the
    /// score so far and a random seed.
    pub fn new(level: u8, difficulty: u8, score: u32, seed: u32) -> Result<Game, String> {
        let src = RomAssets::new()?;
        let rom = Rom(super::rom::get().ok_or("no ROM installed")?);
        let opponent = rom.byte(OPPONENTS + u32::from(level));
        let mut e = Engine::new();
        e.level_mode = 0;
        e.level = level;
        e.opponent = opponent;
        e.difficulty_option = difficulty & 3;
        e.difficulty = CPU_DIFFICULTY[usize::from(difficulty & 3)];
        e.bonus_level = 1;
        e.control_player_1 = 0xFF;
        e.result_score = score;
        e.rng = Rng::new(seed);
        let mut vram = screen::match_vram(&rom, &src, opponent)?;
        planes::match_planes(&rom, &mut vram)?;
        let palettes = screen::match_palettes(&rom, &src, opponent);
        Ok(Game {
            engine: e,
            status: Status::Starting(0),
            level,
            rom,
            vram,
            palettes,
            pads: [Pad::default(); 2],
            backwards: false,
            refugee_frame: 0xFF,
        })
    }

    /// One video frame with the buttons held on each pad (`mbm::pad` bits).
    pub fn frame(&mut self, buttons: [u8; 2]) {
        for (pad, &b) in self.pads.iter_mut().zip(&buttons) {
            pad.read(b, REPEAT_TIMES.0, REPEAT_TIMES.1);
        }
        let mut inputs = Inputs::default();
        inputs.pads = [self.pads[0].input(), self.pads[1].input()];
        let e = &mut self.engine;
        let result: Result<(), Unported> = match self.status {
            Status::Starting(step) => {
                e.vblank();
                let r = match step {
                    0 => {
                        e.deal_bean_orders();
                        Ok(())
                    }
                    1 => e.start_match().and_then(|_| e.run_slots(0..SLOTS, &inputs)),
                    2 => Ok(()), // the portrait art decompresses
                    _ => e.start_stage().and_then(|_| e.run_slots(0..SLOTS, &inputs)),
                };
                self.status = if step >= 3 { Status::Playing } else { Status::Starting(step + 1) };
                r
            }
            Status::Playing => e.frame(&inputs),
            Status::Over { .. } | Status::Stopped(_) => Ok(()),
        };
        if let Err(Unported(what)) = result {
            self.status = Status::Stopped(what.to_string());
        }
        if self.status == Status::Playing && self.engine.match_over {
            self.status = Status::Over { p1_won: self.engine.bytecode_flag == 0 };
        }
        self.update_vram();
    }

    fn update_vram(&mut self) {
        for (at, w) in self.engine.plane_writes.drain(..) {
            self.vram.load(at, &w.to_be_bytes());
        }
        // SpecPlane8A: the background animator's frame on the refugee bean's tiles.
        let frame = self.engine.slots.iter().flatten().find_map(|a| match a {
            Actor::Animator(an) if an.started => Some(an.shown),
            _ => None,
        });
        if let Some(f) = frame.filter(|&f| f != self.refugee_frame && f != 0xFF) {
            self.refugee_frame = f;
            planes::refugee_frame(&mut self.vram, f);
        }
        self.build_sprite_table();
    }

    /// `DrawActors` + `SetSpriteLinks`: the hardware sprite table at $BC00 (entry 0 a
    /// dummy; in front come layer 0's sprites in table order, then layer 1's, ...).
    fn build_sprite_table(&mut self) {
        let mut list: Vec<sprites::Sprite> = vec![];
        let order: Vec<usize> = if self.backwards { (0..SLOTS).rev().collect() } else { (0..SLOTS).collect() };
        self.backwards = !self.backwards;
        'slots: for slot in order {
            let Some(d) = self.engine.draw_of(slot).filter(|d| d.shown()) else { continue };
            for s in sprites::place(&self.rom, d.mappings, d.frame, d.x, d.y, 0) {
                if list.len() + 1 >= sprites::MAX_SPRITES {
                    break 'slots;
                }
                list.push(s);
            }
        }
        let mut chain: Vec<usize> = (1..=list.len()).collect();
        chain.sort_by_key(|&i| (list[i - 1].layer, i));
        let base = Layout::match_screen().sprites;
        let entry = |vram: &mut Vram, i: usize, y: u16, size: u8, link: u8, tile: u16, x: u16| {
            let at = base + i as u16 * 8;
            vram.load(at, &[y.to_be_bytes(), [size, link], tile.to_be_bytes(), x.to_be_bytes()].concat());
        };
        let first = chain.first().copied().unwrap_or(0) as u8;
        entry(&mut self.vram, 0, 0, 0, first, 0, 0);
        for (k, &i) in chain.iter().enumerate() {
            let s = list[i - 1];
            let link = chain.get(k + 1).copied().unwrap_or(0) as u8;
            entry(&mut self.vram, i, s.y, s.size, link, s.tile, s.x);
        }
    }

    /// The picture as RGBA (320 x 224).
    pub fn picture(&self) -> Vec<u8> {
        let cram: [u16; 64] = std::array::from_fn(|i| self.palettes[i / 16][i % 16]);
        let pixels = render::render(&Layout::match_screen(), &self.vram.0, &cram, &self.engine.vscroll);
        let mut out = Vec::with_capacity(pixels.len() * 4);
        for p in pixels {
            let [r, g, b] = render::to_rgb(p);
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }
}
