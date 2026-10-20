//! Starting a match the way the game flow does (`BC_Level`): `GenPuyoOrder`, a frame
//! later `loc_3C00` (the players, the referee, the background animator, the danger
//! effect; each player then sets up its pair holder and field), and a frame after that
//! `loc_6070` (the opponent's portrait and their stage's own actors). Between the last
//! two the ROM decompresses the portrait art with interrupts off: no frames pass there
//! but one vertical blank.

use super::*;
use crate::mbm::bean_order::gen_bean_orders;
use crate::mbm::rom;

/// `unk_3F84`: the scenario speed step each level starts at.
const LEVEL_SPEED_STEPS: u32 = 0x3F84;
/// `word_5BAE`: the referee's place and intro by mode (x, y, script).
const REFEREE_START: [(u16, u16, u32); 4] = [(0x140, 0x128, 0x3316), (0x120, 0x108, 0x3316), (0x120, 0x10C, 0x3330), (0x120, 0x108, 0x3330)];
/// `word_45CA`: where each side's next-pair preview sits.
pub(super) const PREVIEW_AT: [(u16, u16); 2] = [(0x108, 0xC6), (0x138, 0xC6)];
/// `unk_690A`: Frankly's six hoppers (x, y, dx, dy, frame, place).
const FRANKLY_HOPPERS: u32 = 0x690A;

const OPP_FRANKLY: u8 = 1;
const OPP_ARMS: u8 = 3;
const OPP_COCONUTS: u8 = 7;
const OPP_SIR_FFUZZY: u8 = 9;
const OPP_HUMPTY: u8 = 14;

impl Player {
    /// A freshly spawned player actor (`loc_3C40`): everything zero but its id.
    pub fn new(id: u8) -> Player {
        Player {
            id,
            state: PlayerState::Setup,
            flags: 3,
            w26: 0,
            delay: 0,
            chain: 0,
            fall_speed: 0,
            speed_level: 0,
            pending_garbage: 0,
            speed_index: 0,
            score: 0,
            acc: 0,
            base: 0,
            bonus: 0,
            beans_total: 0,
            beans_count: 0,
            threshold: 0,
            garbage_sent: 0,
            target: (0, 0),
            move_frames: 0,
            pause_frames: 0,
            other_slot: 0,
            holder_slot: None,
            w28: 0,
            pop: None,
        }
    }
}

impl Engine {
    /// `GenPuyoOrder`: both players' bean orders for the match.
    pub fn deal_bean_orders(&mut self) {
        let orders = gen_bean_orders(&mut self.rng, self.level, self.level_mode);
        self.rng_calls += if self.level_mode == super::super::bean_order::LEVEL_MODE_SPLIT { 256 + 248 } else { 256 };
        self.bean_orders[0] = orders.p1.map(|c| c as u8);
        self.bean_orders[1] = orders.p2.map(|c| c as u8);
    }

    /// `loc_3C00`: spawn the match (the bytecode runs it before `RunActors`, so the new
    /// actors also run that frame).
    pub fn start_match(&mut self) -> Result<(), Unported> {
        if self.level_mode & 3 != 0 {
            return Err(Unported("starting a match outside scenario mode"));
        }
        self.time_frames = 0;
        self.time_minutes = 0;
        self.match_over = false;
        let p1 = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
        let mut player1 = Player::new(0);
        player1.score = self.result_score; // the score so far, carried between stages
        player1.beans_total = self.result_beans;
        self.slots[p1] = Some(Actor::Player(player1));
        let p2 = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
        let mut player2 = Player::new(1);
        player2.other_slot = p1;
        self.slots[p2] = Some(Actor::Player(player2));
        self.player_mut(p1).other_slot = p2;
        // sub_5B54
        self.referee_hit = 0;
        if let Some(slot) = self.find_actor_slot() {
            let (x, y, script) = REFEREE_START[usize::from(self.level_mode & 3)];
            let mut anim = Anim::new(script);
            anim.frame = 9;
            let referee = Referee {
                state: RefereeState::Spawned,
                link: actor_addr(p1),
                anim,
                draw: 0x80,
                flags: 0,
                x,
                y,
                base_y: y,
                vx: 0,
                repeats: 0,
                bob: 0,
                last_frame: 0xFF,
            };
            self.slots[slot] = Some(Actor::Referee(referee));
        }
        // sub_3BB0
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Animator(Animator { started: false, anim: Anim::default(), shown: 0 }));
        }
        // sub_1233A
        self.cpu_memory = [[0; 10]; 2];
        self.popping = [false, false];
        let count = super::super::ai::off::COUNT;
        self.ai_fields[1].bytes[count..count + 2].fill(0);
        if let Some(slot) = self.find_actor_slot() {
            self.slots[slot] = Some(Actor::Danger(Danger { spark_wait: 0, flash_wait: 0, flash: 0 }));
        }
        // sub_3CDA: NEXT and SCORE.
        self.queue_panel(PanelCommand::Text { entry: PANEL_LABELS, blink: 0, player: 0 });
        Ok(())
    }

    /// `loc_3D1C`: the player's first frame: pair holder, empty field, start flags.
    pub(super) fn set_up_player(&mut self, p: &mut Player) -> Result<(), Unported> {
        let id = usize::from(p.id & 1);
        if self.level_mode | self.level | p.id == 0 {
            return Err(Unported("the lesson's hint message (sub_11EF2)"));
        }
        self.spawn_pair_holder(p);
        // loc_3D44
        self.popping[id] = false;
        self.fields[id] = Field::default(); // ResetPuyoField
        self.reset_field_bytes(id);
        add_points(p, 0); // sub_9C4A: draws the score
        self.queue_panel(PanelCommand::Score(p.id));
        // sub_3F94 (scenario): ready at once (bit 0 set, then cleared again), at this
        // level's speed step.
        p.flags &= !1;
        p.speed_index = rom::byte(LEVEL_SPEED_STEPS + u32::from(self.level));
        p.state = PlayerState::WaitReady;
        Ok(())
    }

    /// `sub_44EA`: the holder of the next pairs (filled back to front) and the two
    /// preview beans.
    fn spawn_pair_holder(&mut self, p: &mut Player) {
        let Some(slot) = self.find_actor_slot() else { return };
        p.holder_slot = Some(slot);
        let mut h = PairHolder { w20: 0, pairs: [0; 6], dealt: 0, b07: 0 };
        for at in [4, 2, 0] {
            let order = &self.bean_orders[usize::from(p.id != 0)];
            let i = usize::from((h.w20 >> 8) as u8);
            h.pairs[at] = order[i];
            h.pairs[at + 1] = order[(i + 1) & 0xFF];
            h.w20 = h.w20.wrapping_add(0x200);
        }
        self.slots[slot] = Some(Actor::PairHolder(h));
        let (x, y) = PREVIEW_AT[usize::from(p.id & 1)];
        for index in 0..2u8 {
            let Some(bean) = self.find_actor_slot() else { return };
            let y = y.wrapping_sub(u16::from(index) * 0x10);
            let preview = PreviewBean { holder_slot: slot, index, color: 0, last: 0xFF, draw: 0x80, anim: Anim::new(0), x, y };
            self.slots[bean] = Some(Actor::PreviewBean(preview));
        }
    }

    /// $3D80: wait for flag bit 0, then the fall rate for the speed step (`loc_9CF8`).
    pub(super) fn player_ready(&mut self, p: &mut Player) -> Result<(), Unported> {
        if p.flags & 1 != 0 {
            return Ok(());
        }
        let step = p.speed_index.saturating_sub(self.difficulty) as usize;
        let &(speed, threshold) = SCENARIO_SPEEDS.get(step).ok_or(Unported("speed step past unk_9DC4"))?;
        p.fall_speed = speed;
        p.threshold = threshold;
        p.state = PlayerState::WaitStart;
        Ok(())
    }

    /// `loc_6070` + `sub_678E`: the opponent's portrait and their stage's actors (the
    /// bytecode runs it, then `RunActors`).
    pub fn start_stage(&mut self) -> Result<(), Unported> {
        // Level_DrawSmallText runs next in the bytecode; nothing else it queues overlaps.
        self.queue_panel(PanelCommand::StageText);
        self.queue_panel(PanelCommand::Names);
        self.portrait_lock = 0;
        self.danger_flash = 0;
        // sub_61E0: the portrait's palette on line 3.
        let colors = self.opponent_palette();
        self.load_palette(3, PaletteLoad::Colors(colors));
        self.crowding = 0;
        let Some(portrait) = self.find_actor_slot() else { return Ok(()) };
        let p = Portrait { started: false, level: 0, base: 0, wait: 0, shown: 0, next: 0, map: 0 };
        self.slots[portrait] = Some(Actor::Portrait(p));
        if self.level_mode & 3 != 0 {
            return Ok(());
        }
        match self.opponent {
            OPP_ARMS => {
                if let Some(slot) = self.find_actor_slot() {
                    let palette = PortraitPalette { started: false, wait: 1, step: 0, level: 0 };
                    self.slots[slot] = Some(Actor::PortraitPalette(palette));
                }
            }
            OPP_FRANKLY => {
                for i in 0..6u32 {
                    let Some(slot) = self.find_actor_slot() else { continue };
                    let at = FRANKLY_HOPPERS + i * 10;
                    let hopper = BgSlide {
                        draw: 0,
                        frame: rom::byte(at + 8),
                        steps: 0,
                        pause: 0,
                        place: rom::byte(at + 9),
                        dx: rom::word(at + 4),
                        dy: rom::word(at + 6),
                        phase: 0,
                        x: rom::word(at),
                        y: rom::word(at + 2),
                    };
                    self.slots[slot] = Some(Actor::BgSlide(hopper));
                }
            }
            OPP_HUMPTY => {
                if let Some(slot) = self.find_actor_slot() {
                    let runner = BgRunner {
                        portrait,
                        draw: 0,
                        latch: 0,
                        left: 0,
                        pause: 0,
                        level: 0,
                        x: 0x102,
                        y: 0xE6,
                        anim: Anim { ptr: 0, time: 0, frame: 0 },
                    };
                    self.slots[slot] = Some(Actor::BgRunner(runner));
                }
            }
            OPP_COCONUTS | OPP_SIR_FFUZZY => return Err(Unported("this opponent's stage actors (sub_678E)")),
            _ => {}
        }
        Ok(())
    }
}
