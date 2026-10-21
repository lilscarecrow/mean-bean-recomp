//! A scenario-mode stage on the engine, as the front end drives it: the game flow from
//! `BC_Level` (which sets the match up), one engine frame per video frame with the two
//! pads, then on through the screens after the match as far as they're ported; and the
//! picture: VRAM kept up to date from the engine's plane writes and the flow's art loads,
//! the sprite table built like `DrawActors`, and the software VDP with the registers,
//! palettes and scroll the engine keeps.

use super::actors::{Actor, Engine, Inputs, Unported, VdpOp, BC_LEVEL, SLOTS};
use super::assets::{planes, screen, sprites, AssetSource, Rom, RomAssets, Vram};
use super::pad::Pad;
use super::render::{self, Layout};
use super::rng::Rng;

/// Brings `vram` up to date with what the engine did since the last call: its plane
/// writes and, in order among them, the flow's art loads and plane lists.
/// `boxes` is `plane_a_buffer` (what the intro's text box covers).
pub fn apply_vdp(e: &mut Engine, vram: &mut Vram, boxes: &mut Vec<u16>, rom: &Rom, src: &dyn AssetSource) -> Result<(), String> {
    let writes: Vec<(u16, u16)> = e.plane_writes.drain(..).collect();
    let mut done = 0;
    for ev in std::mem::take(&mut e.vdp_events) {
        for &(at, w) in &writes[done..ev.at.min(writes.len())] {
            vram.load(at, &w.to_be_bytes());
        }
        done = done.max(ev.at.min(writes.len()));
        match ev.what {
            VdpOp::Sorcery { vram: at, art } => vram.load(at, &src.sorcery(art)?),
            VdpOp::Nemesis { vram: at, art } => vram.load(at, &src.nemesis(art)?),
            VdpOp::PlaneList { list, row } => planes::run_list_rows(rom, vram, list, row)?,
            VdpOp::Font { vram: at, art } => {
                let mut font = src.nemesis(art)?;
                font.resize(0x1180, 0);
                font.iter_mut().for_each(|b| *b |= 0x11);
                vram.load(at, &font);
            }
            VdpOp::RomCopy { vram: at, from, len } => {
                let from = from as usize;
                vram.load(at, &rom.0[from..from + usize::from(len)]);
            }
            VdpOp::Words { at, words } => {
                let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_be_bytes()).collect();
                vram.load(at, &bytes);
            }
            VdpOp::SaveBox { at, cols, rows, row } => {
                boxes.clear();
                for r in 0..rows {
                    for c in 0..cols {
                        let a = usize::from(at.wrapping_add(r * row).wrapping_add(c * 2));
                        boxes.push(u16::from_be_bytes([vram.0[a], vram.0[(a + 1) & 0xFFFF]]));
                    }
                }
            }
            VdpOp::RestoreBox { at, cols, rows, row } => {
                for r in 0..rows {
                    for c in 0..cols {
                        let w = boxes.get(usize::from(r * cols + c)).copied().unwrap_or(0);
                        vram.load(at.wrapping_add(r * row).wrapping_add(c * 2), &w.to_be_bytes());
                    }
                }
            }
        }
    }
    for &(at, w) in &writes[done..] {
        vram.load(at, &w.to_be_bytes());
    }
    Ok(())
}

/// `Game_Bytecode` (after the boot code's own checks) and the VDP registers it starts
/// with.
const GAME_BYTECODE: u32 = 0x18BA;
const BOOT_VDP_REGS: [u8; 0x13] = [0x04, 0x64, 0x30, 0x3C, 0x07, 0x5E, 0, 0, 0, 0, 0, 0x03, 0x81, 0x2E, 0, 0x02, 0x03, 0, 0];

/// `Opponents`: who you meet on each level.
const OPPONENTS: u32 = 0x278E;
/// `unk_DF8E`: the CPU's `byte_FF0104` by the difficulty option.
const CPU_DIFFICULTY: [u8; 4] = [0, 2, 4, 6];

/// What the stage is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Playing,
    /// The engine reached something not ported yet.
    Stopped(String),
}

pub struct Game {
    pub engine: Engine,
    pub status: Status,
    pub level: u8,
    rom: Rom<'static>,
    src: RomAssets,
    vram: Vram,
    boxes: Vec<u16>,
    pads: [Pad; 2],
    /// `draw_order`: DrawActors walks the slots forwards and backwards on alternate frames.
    backwards: bool,
    refugee_frame: u8,
    /// CRAM and VSRAM as the last vertical blank left them.
    cram: [u16; 64],
    vsram: [u16; 40],
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
        e.load_default_high_scores();
        // The screen as the level transition leaves it (board art and maps); the flow
        // from BC_Level loads the rest.
        let mut vram = screen::match_vram(&rom, &src, opponent)?;
        planes::match_planes(&rom, &mut vram)?;
        e.set_vdp_registers(1);
        e.bytecode_pc = BC_LEVEL;
        e.bytecode_disabled = false;
        Ok(Game {
            engine: e,
            status: Status::Playing,
            level,
            rom,
            src,
            vram,
            boxes: vec![],
            pads: [Pad::default(); 2],
            backwards: false,
            refugee_frame: 0xFF,
            cram: [0; 64],
            vsram: [0; 40],
        })
    }

    /// Power-on: the game flow from its first routine (`Game_Bytecode`), as the boot code
    /// leaves things (VDP registers, controller repeat times, default high scores and
    /// options; nothing in VRAM yet).
    pub fn boot(seed: u32) -> Result<Game, String> {
        let src = RomAssets::new()?;
        let rom = Rom(super::rom::get().ok_or("no ROM installed")?);
        let mut e = Engine::new();
        e.rng = Rng::new(seed);
        e.load_default_high_scores();
        e.vdp_regs = BOOT_VDP_REGS;
        e.repeat_times = (0x10, 3);
        e.bytecode_pc = GAME_BYTECODE;
        e.bytecode_disabled = false;
        Ok(Game {
            engine: e,
            status: Status::Playing,
            level: 0,
            rom,
            src,
            vram: Vram(vec![0; 0x10000]),
            boxes: vec![],
            pads: [Pad::default(); 2],
            backwards: false,
            refugee_frame: 0xFF,
            cram: [0; 64],
            vsram: [0; 40],
        })
    }

    /// One video frame with the buttons held on each pad (`mbm::pad` bits).
    pub fn frame(&mut self, buttons: [u8; 2]) {
        if self.status != Status::Playing {
            return;
        }
        let (pressed, unpressed) = self.engine.repeat_times;
        for (pad, &b) in self.pads.iter_mut().zip(&buttons) {
            pad.read(b, pressed, unpressed);
        }
        let mut inputs = Inputs::default();
        inputs.pads = [self.pads[0].input(), self.pads[1].input()];
        // The picture is what the vertical blank sends the VDP: the art, maps, sprite
        // table, scroll and colors from the logic before it (DMAed tiles and the
        // sprites that use them arrive together).
        self.engine.vblank();
        self.update_vram();
        self.cram = std::array::from_fn(|i| self.engine.palettes[i / 16][i % 16]);
        self.vsram = self.engine.vscroll;
        if let Err(Unported(what)) = self.engine.logic(&inputs) {
            self.status = Status::Stopped(what.to_string());
        }
        self.engine.match_over = false;
    }

    /// A cheap fingerprint of where the game is (the random seed, how often it was drawn
    /// from and the game flow's place): two copies that differ here have desynced. The
    /// game draws from the generator every frame, so a difference shows up quickly.
    pub fn checksum(&self) -> u32 {
        let e = &self.engine;
        e.rng.seed ^ e.rng_calls.rotate_left(16) ^ e.bytecode_pc.wrapping_mul(0x9E37_79B9)
    }

    /// The VDP layout the registers set.
    fn layout(&self) -> Layout {
        let mut regs = [0u8; 24];
        regs[..0x13].copy_from_slice(&self.engine.vdp_regs);
        Layout::from_registers(&regs)
    }

    fn update_vram(&mut self) {
        if let Err(e) = apply_vdp(&mut self.engine, &mut self.vram, &mut self.boxes, &self.rom, &self.src) {
            self.status = Status::Stopped(e);
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
        // TransferHScroll: the planes' horizontal scroll.
        let at = self.layout().hscroll;
        let words: Vec<u8> = self.engine.hscroll.iter().flat_map(|w| w.to_be_bytes()).collect();
        self.vram.load(at, &words);
    }

    /// `DrawActors` + `SetSpriteLinks`: the hardware sprite table at $BC00 (entry 0 a
    /// dummy; in front come layer 0's sprites in table order, then layer 1's, ...).
    fn build_sprite_table(&mut self) {
        let mut list: Vec<sprites::Sprite> = vec![];
        let order: Vec<usize> = if self.backwards { (0..SLOTS).rev().collect() } else { (0..SLOTS).collect() };
        // (sprites move with plane A's vertical scroll)
        let vscroll = self.engine.vscroll[0];
        self.backwards = !self.backwards;
        'slots: for slot in order {
            let Some(d) = self.engine.draw_of(slot).filter(|d| d.shown()) else { continue };
            // (mapping list 7's frames are the epilogue's lines, built in RAM)
            let placed = match self.engine.line_maps.get(usize::from(d.frame)).filter(|_| d.mappings == 7) {
                Some(pieces) => sprites::place_pieces(pieces.clone(), d.x, d.y, vscroll),
                None => sprites::place(&self.rom, d.mappings, d.frame, d.x, d.y, vscroll),
            };
            for s in placed {
                if list.len() + 1 >= sprites::MAX_SPRITES {
                    break 'slots;
                }
                list.push(s);
            }
        }
        let mut chain: Vec<usize> = (1..=list.len()).collect();
        chain.sort_by_key(|&i| (list[i - 1].layer, i));
        let base = self.layout().sprites;
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
        let pixels = render::render(&self.layout(), &self.vram.0, &self.cram, &self.vsram);
        let mut out = Vec::with_capacity(pixels.len() * 4);
        for p in pixels {
            let [r, g, b] = render::to_rgb(p);
            out.extend_from_slice(&[r, g, b, 255]);
        }
        out
    }
}
