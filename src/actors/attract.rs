//! The attract mode: after the title's countdown, the game's prologue (`BC_GameIntro`:
//! Robotnik in his lair, with the intro text script for "opponent" $10, sending two
//! robots off, which dissolve), then in turn the tutorial or a demo match, each
//! followed by the high score table (`BC_HighScores_2`). A button goes back to the title.

use super::*;
use crate::assets::nemesis;
use crate::rom;

/// `sub_F074`: the prologue's controller (`word_FF112C` its step).
pub const PROLOGUE_CONTROL_CODE: u32 = 0xF074;

/// `loc_A212` / `loc_A25C`: a robot dissolving: its art, kept in RAM
/// (`misc_buffer_1`), masked a step further every frame and DMAed back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dissolve {
    pub second: bool,
    pub step: u16,
}

impl Dissolve {
    pub fn code(&self) -> u32 {
        if self.second { 0xA25C } else { 0xA212 }
    }
}

/// `ArtNem_IntroBadniks` (decompressed into `misc_buffer_1` too); `unk_B224` (the
/// dissolve's masks); `unk_EF22` (Robotnik's script); `byte_F12A` (opponents by level).
const BADNIKS_ART: u32 = 0x4C432;
const DISSOLVE_MASKS: u32 = 0xB224;
const ROBOTNIK_PROLOGUE: u32 = 0xEF22;
const DEMO_OPPONENTS: u32 = 0xF12A;
/// The lair's maps (`LoadLairMachineMap` etc.).
const LAIR_MACHINE: u32 = 0x6432E;
const LAIR_PRIORITY: u32 = 0x6463A;
const LAIR_WALL: u32 = 0x644B4;
const LAIR_FLOOR: u32 = 0x645D8;

impl Engine {
    pub(super) fn run_attract_routine(&mut self, at: u32) -> bool {
        match at {
            0xF84A => self.eni_dec_prio_safe(LAIR_MACHINE, 0x200, 0x27, 0x12, 0xC000, LAIR_PRIORITY),
            0xF88E => self.eni_dec_safe(LAIR_WALL, 0x6200, 0x27, 0x12, 0xE000),
            0xF8CC => self.eni_dec_safe(LAIR_FLOOR, 0x200, 0x27, 8, 0xE980),
            0xEFFE => self.start_prologue(),
            // sub_DDD8: an arcade "insert coin" sign; it deletes itself when it first runs.
            0xDDD8 => {
                if let Some(slot) = self.free_slot() {
                    self.slots[slot] = Some(Actor::CoinSign);
                }
            }
            // LoadTimerCtrlSkip / LoadCtrlWait
            0xDD64 | 0xDD7E => {
                if let Some(slot) = self.find_actor_slot() {
                    let timer = if at == 0xDD64 { 0xA00 } else { 0 };
                    self.slots[slot] = Some(Actor::CtrlSkip(CtrlSkip { timer }));
                }
            }
            _ => return false,
        }
        true
    }

    /// `sub_EFFE`.
    fn start_prologue(&mut self) {
        if let Some(Ok((art, _))) = rom::get().map(|r| nemesis::decompress(&r[BADNIKS_ART as usize..])) {
            self.misc_buffer = art;
        }
        self.long_step();
        self.opponent = 0x10;
        self.opponent_x = 0xDA;
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::IntroScript(IntroScript {
                state: IntroScriptState::Start,
                ptr: 0,
                wait: 0,
                shown: false,
                at: 0,
                width: 0,
                lines: 0,
                col: 0,
                line: 0,
            }));
        }
        self.vscroll[0] = 0;
        self.hscroll[0] = 0;
        self.word_112c = 0;
        self.fireworks_mask = 0;
        self.fireworks_color = 0;
        self.use_lair = 0;
        self.spawn_lair_parts();
        // sub_ED62
        self.intro_cue = 0;
        if let Some(slot) = self.find_actor_slot() {
            let r = CutsceneRobotnik { started: false, ptr: ROBOTNIK_PROLOGUE, time: 0, frame: 0, mappings: 0x2B, x: 0x90 << 16, y: 0xE0, draw: 0x80 };
            self.slots[slot] = Some(Actor::CutsceneRobotnik(r));
        }
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::PrologueControl);
        }
    }

    /// `sub_F074`.
    pub(super) fn run_prologue_control(&mut self, inputs: &Inputs) -> bool {
        if self.word_112c == 0 {
            self.word_112c = 4;
            return true;
        }
        // (dword_FF1130 counts along, unused)
        self.fireworks_mask = self.fireworks_mask.wrapping_add(4);
        if (inputs.pads[0].press | inputs.pads[1].press) & 0xF0 != 0 {
            self.bytecode_disabled = false;
            self.bytecode_flag = 0;
            return false;
        }
        if self.opponent != 0 {
            return true;
        }
        // The prologue's over: the tutorial and a demo match in turn.
        self.bytecode_disabled = false;
        let next = self.attract_turn.wrapping_add(1);
        self.bytecode_flag = next;
        self.attract_turn = next & 1;
        if next == 2 {
            self.demo_stage = self.demo_stage.wrapping_add(1) & 3;
            self.level = self.demo_stage + 3;
            self.opponent = rom::byte(DEMO_OPPONENTS + u32::from(self.level));
        }
        false
    }

    /// `sub_A1C2` / `sub_A1EA`.
    pub(super) fn spawn_dissolve(&mut self, second: bool) {
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Dissolve(Dissolve { second, step: 0 }));
        }
    }

    pub(super) fn run_dissolve(&mut self, mut d: Dissolve) -> Option<Dissolve> {
        let step = d.step;
        if step == 0x100 {
            return None;
        }
        d.step += 0x10;
        // sub_A2A6: $63 groups of 16 bytes, each ANDed with the step's mask.
        let (from, to) = if d.second { (0, 0x1200) } else { (0x620, 0x1820) };
        let mask: Vec<u8> = (0..16).map(|i| rom::byte(DISSOLVE_MASKS + u32::from(step) + i)).collect();
        for g in 0..0x63usize {
            for (i, m) in mask.iter().enumerate() {
                if let Some(b) = self.misc_buffer.get_mut(from + g * 16 + i) {
                    *b &= m;
                }
            }
        }
        let words: Vec<u16> = (0..0x310).map(|i| {
            let at = from + i * 2;
            u16::from_be_bytes([self.misc_buffer.get(at).copied().unwrap_or(0), self.misc_buffer.get(at + 1).copied().unwrap_or(0)])
        }).collect();
        self.dma_queue.push(VdpOp::Words { at: to, words });
        // (it leaves RunActors' actor pointer in the DMA queue: the slots after it
        // don't run this frame)
        self.actors_derailed = true;
        Some(d)
    }
}
