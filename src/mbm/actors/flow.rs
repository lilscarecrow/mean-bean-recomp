//! The game flow: the ROM's bytecode (`RunBytecode`), run before the actors every frame
//! while it isn't disabled. An op is a word opcode and a word operand, some with more
//! words after; ops without an operand step back over it. `BDISABLE`, `BDELAY` and
//! `BFADEW` hand over to actors, which clear `bytecode_disabled` when they're done.
//!
//! What the bytecode does to video memory (art, VDP registers, plane lists) comes out as
//! [`VdpEvent`]s for whoever keeps the VRAM; sound ops do nothing here. `BRUN` calls the
//! ported routines by their ROM address; any other stops with `Unported`.

use super::*;
use crate::mbm::rom;

/// Something the flow does to the VDP, in order with `Engine::plane_writes`: `at` is
/// how many plane writes came before it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VdpEvent {
    pub at: usize,
    pub what: VdpOp,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VdpOp {
    /// `PuyoDec` (Sorcery) / `NemDec` of the art at ROM address `art` to VRAM `vram`.
    Sorcery { vram: u16, art: u32 },
    Nemesis { vram: u16, art: u32 },
    /// A `PlaneCmdLists` entry, run by the vertical blank (`ProcPlaneCommands`).
    PlaneList { list: u16, row: u16 },
    /// The intro's font (`NemDecRAM`, every long ORed with $11111111, DMAed to `vram`).
    Font { vram: u16, art: u32 },
    /// `SpecPlane8C`/`9F` save the plane words under the text box (`plane_a_buffer`),
    /// `SpecPlane8D` puts them back: `cols` x `rows` from `at`, rows `row` bytes apart.
    SaveBox { at: u16, cols: u16, rows: u16, row: u16 },
    RestoreBox { at: u16, cols: u16, rows: u16, row: u16 },
    /// A DMA of `len` bytes of ROM (uncompressed tiles) to `vram`.
    RomCopy { vram: u16, from: u32, len: u16 },
    /// Words written straight to VRAM (pattern pixels).
    Words { at: u16, words: Vec<u16> },
}

/// Bytecode addresses (`Game_Bytecode`): the stage, the scenario's next opponent.
pub const BC_LEVEL: u32 = 0x1E70;
pub const BC_SCENARIO: u32 = 0x1CC8;
/// `SetupVDPregs`' register tables (`.VDPRegTables`, longs).
const VDP_REG_TABLES: u32 = 0x0F34;
/// `Opponents` (by level), `Palette_Table`.
const OPPONENTS: u32 = 0x278E;
const PALETTE_TABLE: u32 = 0x27D6;
/// `ArtPuyo_LevelBG` (the operand of `LoadLevelBGArt`'s `lea`).
const LEVEL_BG_ART: u32 = 0x2724;

/// The bytecode's actors: `ActBytecodeDelay` (waits its delay, then re-enables the
/// bytecode) and `ActWaitPalFade` (waits for every palette fade to end).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowWait {
    Delay { delay: u16, done: bool },
    PaletteFades,
}

pub const BYTECODE_DELAY_CODE: u32 = 0x16B4;
pub const WAIT_PAL_FADE_CODE: u32 = 0x16E2;

impl FlowWait {
    pub fn code(&self) -> u32 {
        match self {
            FlowWait::Delay { done: false, .. } => BYTECODE_DELAY_CODE,
            FlowWait::Delay { done: true, .. } => BYTECODE_DELAY_CODE + 4,
            FlowWait::PaletteFades => WAIT_PAL_FADE_CODE,
        }
    }
}

impl Engine {
    /// `RunBytecode`: ops until one ends the frame.
    pub fn run_bytecode(&mut self, inputs: &Inputs) -> Result<(), Unported> {
        if self.bytecode_disabled {
            return Ok(());
        }
        loop {
            let pc = self.bytecode_pc;
            if pc == BC_TITLE {
                return Err(Unported("the title screen (bytecode $195C)"));
            }
            let (op, arg) = (rom::word(pc), rom::word(pc + 2));
            self.bytecode_pc = pc + 4;
            let next_long = |e: &Engine| u32::from(arg) << 16 | u32::from(rom::word(e.bytecode_pc));
            match op {
                // BDISABLE / BFRMEND
                0 | 1 => {
                    self.bytecode_pc -= 2;
                    if op == 0 {
                        self.bytecode_disabled = true;
                    }
                    return Ok(());
                }
                // BDELAY
                2 => {
                    self.bytecode_disabled = true;
                    if let Some(slot) = self.find_actor_slot() {
                        self.slots[slot] = Some(Actor::FlowWait(FlowWait::Delay { delay: arg, done: false }));
                    }
                    return Ok(());
                }
                // BFADEW
                3 => {
                    self.bytecode_pc -= 2;
                    self.bytecode_disabled = true;
                    if let Some(slot) = self.find_actor_slot() {
                        self.slots[slot] = Some(Actor::FlowWait(FlowWait::PaletteFades));
                    }
                    return Ok(());
                }
                // BWRITE
                4 => {
                    let at = next_long(self);
                    let value = rom::word(self.bytecode_pc + 2);
                    self.bytecode_pc += 4;
                    self.write_word(at, value)?;
                }
                // BRUN
                5 => {
                    let at = next_long(self);
                    self.bytecode_pc += 2;
                    self.run_routine(at, inputs)?;
                }
                // BJMP / BJCLR / BJSET
                6 => self.bytecode_pc = next_long(self),
                7 | 8 => {
                    if (self.bytecode_flag == 0) == (op == 7) {
                        self.bytecode_pc = next_long(self);
                    } else {
                        self.bytecode_pc += 2;
                    }
                }
                // BJTBL
                9 => {
                    let i = u16::from(self.bytecode_flag).min(arg.wrapping_sub(1));
                    self.bytecode_pc = rom::long(self.bytecode_pc + u32::from(i) * 4);
                }
                // BVDP
                0x0A => self.set_vdp_registers(arg),
                // BPUYO / BPUYOI (the last stage's intro puts it at $2000) / BNEM
                0x0B | 0x16 | 0x14 => {
                    self.long_step();
                    let art = rom::long(self.bytecode_pc);
                    self.bytecode_pc += 4;
                    let vram = if op == 0x16 && self.level == 0x0F { 0x2000 } else { arg };
                    let what = if op == 0x14 { VdpOp::Nemesis { vram, art } } else { VdpOp::Sorcery { vram, art } };
                    self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what });
                }
                // BPCMD
                0x0C => self.queue_panel(PanelCommand::List(arg)),
                // BPAL
                0x0D => {
                    let colors = self.table_palette_at(arg);
                    self.load_palette(usize::from(arg & 3), PaletteLoad::Colors(colors));
                }
                // BFADE
                0x0E => {
                    let speed = rom::byte(self.bytecode_pc);
                    self.bytecode_pc += 2;
                    let colors = self.table_palette_at(arg);
                    self.fade_palette(arg & 3, speed, 8, &colors);
                }
                // Sound: BSND / BSNDP / BSNDP2 (operand), BSFADE / BSSTOP (none).
                0x0F | 0x10 | 0x13 => {}
                0x11 | 0x12 => self.bytecode_pc -= 2,
                // BFADEI
                0x15 => {
                    let (speed, lair) = (rom::byte(self.bytecode_pc), rom::byte(self.bytecode_pc + 1));
                    self.bytecode_pc += 2;
                    self.fade_intro(arg, speed, lair);
                }
                _ => return Err(Unported("this bytecode op")),
            }
        }
    }

    /// A palette word's `Palette_Table` entry (offset << 3 | line).
    fn table_palette_at(&self, word: u16) -> [u16; 16] {
        let at = PALETTE_TABLE + u32::from(word >> 3);
        std::array::from_fn(|c| rom::word(at + c as u32 * 2))
    }

    /// `BWRITE`: the RAM words the flow sets.
    fn write_word(&mut self, at: u32, value: u16) -> Result<(), Unported> {
        match at {
            0xFF_1108 => self.repeat_times = ((value >> 8) as u8, value as u8),
            0xFF_1122 => self.input_lock = value,
            0xFF_1974 => self.level_transition = value,
            0xFF_FC02 => self.sound_test = value,
            0xFF_0112 => {
                self.level = (value >> 8) as u8;
                self.opponent = value as u8;
            }
            0xFF_112A => self.use_lair = (value >> 8) as u8,
            0xFF_1972 => {
                self.high_score_table = (value >> 8) as u8;
                self.high_score_both = value as u8 != 0;
            }
            0xFF_05D2..=0xFF_0620 if at & 1 == 0 => self.vscroll[((at - 0xFF_05D2) / 2) as usize] = value,
            _ => return Err(Unported(Box::leak(format!("a bytecode write to ${at:06X}").into_boxed_str()))),
        }
        Ok(())
    }

    /// `SetupVDPRegs_DisplayOn`: a register table into the mirror (display on), scroll
    /// cleared.
    pub fn set_vdp_registers(&mut self, table: u16) {
        let at = rom::long(VDP_REG_TABLES + u32::from(table) * 4);
        for i in 0..0x13 {
            self.vdp_regs[i] = rom::byte(at + i as u32 * 2 + 1);
        }
        self.vdp_regs[1] |= 0x40;
        self.vscroll[0] = 0;
        self.vscroll[1] = 0;
        self.hscroll[0] = 0;
        self.hscroll[1] = 0;
    }

    /// The routines `BRUN` calls on the way between matches.
    fn run_routine(&mut self, at: u32, inputs: &Inputs) -> Result<(), Unported> {
        if self.run_game_over_routine(at) || self.run_high_score_routine(at) || self.run_finale_routine(at) || self.run_credits_routine(at) || self.run_epilogue_routine(at) || self.run_intro_routine(at)? {
            return Ok(());
        }
        match at {
            // InitPalette_Safe: the buffer cleared and every line marked for copying.
            0x116C => {
                self.palettes = [[0; 16]; 4];
                self.palette_loads = [Some(PaletteLoad::Buffer); 4];
            }
            // InitActors
            0x35BC => {
                self.slots.iter_mut().for_each(|s| *s = None);
                self.pal_fade_gen = [0; 4];
            }
            // EnableSHMode / DisableSHMode
            0x0D7A => self.vdp_regs[0x0C] |= 8,
            0x0D90 => self.vdp_regs[0x0C] &= !8,
            // ClearScroll
            0x0D58 => {
                self.vscroll = [0; 40];
                self.hscroll = [0; 0x200];
            }
            // LevelEnd
            0x3B62 => {
                if self.level >= 0x0F {
                    self.bytecode_flag = 1;
                } else {
                    self.level += 1;
                    self.opponents_defeated[usize::from(self.opponent) & 0x0F] = 0xFF;
                    self.bytecode_flag = if self.level == 3 { 2 } else { 0 };
                }
            }
            // SetOpponent
            0x2764 => {
                self.bytecode_flag = u8::from(self.level == 0x0F);
                self.opponent = rom::byte(OPPONENTS + u32::from(self.level));
            }
            // CheckFinalLevel
            0x27C0 => self.bytecode_flag = u8::from(self.level == 0x0F),
            // LoadLevelBGArt / LoadLevelBGPal
            0x2722 => {
                self.long_step();
                let art = rom::long(LEVEL_BG_ART);
                self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Sorcery { vram: 0, art } });
                self.queue_panel(PanelCommand::List(3));
            }
            0x273C => self.load_palette(2, PaletteLoad::Colors(table_palette(3))),
            // CheckTutorialPalInit (the lessons only)
            0x2752 if self.level >= 3 => {}
            // The stage: GenPuyoOrder, loc_3C00, loc_6070, Level_DrawSmallText.
            0x3A64 => self.deal_bean_orders(),
            0x3C00 => self.start_match()?,
            0x6070 => {
                self.long_step(); // the portrait's NemDec
                self.start_stage()?
            }
            0xD608 => self.draw_small_text(),
            // The opponent screen.
            0xD620 => self.spawn_opponent_screen(),
            0xD4D2 => self.draw_opponent_boxes(),
            _ => return Err(Unported(Box::leak(format!("game flow routine ${at:06X}").into_boxed_str()))),
        }
        let _ = inputs;
        Ok(())
    }

    pub(super) fn run_flow_wait(&mut self, w: FlowWait) -> Option<FlowWait> {
        match w {
            // ActBytecodeDelay: ActorBookmark counts the delay down, then .Done.
            FlowWait::Delay { delay, done: false } => {
                let delay = delay.saturating_sub(1);
                Some(FlowWait::Delay { delay, done: delay == 0 })
            }
            FlowWait::Delay { done: true, .. } => {
                self.bytecode_disabled = false;
                None
            }
            // ActWaitPalFade: CheckPaletteFade.
            FlowWait::PaletteFades => {
                if self.pal_fade_gen.iter().any(|&g| g != 0) {
                    return Some(w);
                }
                self.bytecode_disabled = false;
                None
            }
        }
    }
}
