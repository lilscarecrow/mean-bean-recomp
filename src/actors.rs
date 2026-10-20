//! The original's actor system, as far as one player's pieces need it.
//!
//! The game runs up to 64 actors every frame in slot order (`RunActors`). Each actor is a
//! coroutine: it runs until it calls `ActorBookmark` (yield until next frame, optionally
//! after a delay) and resumes at the instruction after it. Every state here is named by
//! that resume address (`code()`), so it can be compared with the game's RAM.
//!
//! Ported so far: the player's field-controller loop between pieces (wait for the pair,
//! find pops, redraw, delay, spawn the next pair), the pair's pivot and satellite
//! (controlled, landing, physics fall, squash, write to the field) and the little
//! post-landing effect, pops and chains, garbage, and the CPU opponent (`ai`). Game
//! over and the CPU's pattern play stop the engine with `Unported`.

use super::ai::{self, AiField};
use super::anim::{script, Anim};
use super::bean_order::BeanColor;
use super::field::{Field, Group, COLS, DIR, HIDDEN_ROWS, ROWS};
use super::motion::{cos, sin, Motion};
use super::pair::{PadInput, Pair, RotateKeys, Step};
use super::assets::planes::FIELD_ORIGIN;
use super::rng::Rng;

mod draw;
mod credits;
mod ending;
mod epilogue;
mod finale;
mod flow;
mod game_over;
mod high_scores;
mod intro;
mod lair;
mod menu;
mod opponent;
mod options;
mod palette;
mod panel;
mod password;
mod setup;
mod sprite;
mod stage;
mod title;
pub use draw::*;
pub use credits::*;
pub use ending::*;
pub use epilogue::*;
pub use finale::*;
pub use flow::*;
pub use game_over::*;
pub use high_scores::*;
pub use intro::*;
pub use lair::*;
pub use menu::*;
pub use opponent::*;
pub use options::*;
pub use palette::*;
pub use panel::*;
pub use password::*;
pub use sprite::*;
pub use stage::*;
pub use title::*;

pub const SLOTS: usize = 64;
/// Screen x of each field's left edge (`word_5B34`).
pub const FIELD_X: [u16; 2] = [0x90, 0x150];
/// Screen y of a field's top visible row (`word_5B34`), same for both players.
pub const FIELD_TOP_Y: u16 = 0x90;
/// Frames to wait after a piece settles, by the player's +$2B (`word_3B58`).
const SETTLE_DELAY: [u16; 5] = [12, 8, 4, 2, 0];
/// Bounce tables after a post-pop fall, by color group (`unk_1A928`..): where their
/// end marker sits. Red/yellow, teal/green, purple/blue, garbage.
const BOUNCE_LEN: [u8; 4] = [22, 23, 22, 1];
/// Chain, color and group bonuses (`sub_9AC2`, `sub_9AEE`, `sub_9B34`).
const CHAIN_BONUS: [u16; 9] = [0, 8, 16, 32, 64, 128, 256, 512, 1024];
const COLOR_BONUS: [u16; 7] = [0, 0, 3, 6, 12, 24, 48];
const GROUP_BONUS: [u16; 12] = [0, 0, 0, 0, 0, 2, 3, 4, 5, 6, 7, 10];
const SCORE_CAP: u32 = 99_999_999;
const GARBAGE_POINTS: u32 = 70;
/// Scenario mode fall rate and beans to the next speed-up, by speed step (`unk_9DC4`).
const SCENARIO_SPEEDS: [(u16, u16); 24] = [
    (0x0400, 0x20), (0x0400, 0x20), (0x0500, 0x20), (0x0500, 0x20), (0x0600, 0x20), (0x0800, 0x20),
    (0x0A00, 0x20), (0x0C00, 0x28), (0x1000, 0x28), (0x2000, 0x30), (0x3000, 0x30), (0x4000, 0x38),
    (0x4800, 0x38), (0x5000, 0x40), (0x6000, 0x40), (0x8000, 0xFFFF), (0x8000, 0xFFFF), (0x8000, 4),
    (0x8000, 4), (0x8000, 4), (0xA000, 4), (0xC000, 4), (0xE000, 4), (0xFE00, 0xFFFF),
];
/// Player flag set while beans fall after a pop.
const FALLING_AFTER_POP: u8 = 0x10;

/// Physics fall (`sub_4948`): start speed 1 px/frame, + $3000/65536 per frame, max 8.
const FALL_START_VEL: u32 = 0x1_0000;
const FALL_ACCEL: u16 = 0x3000;
const FALL_MAX_VEL: u32 = 0x8_0000;

/// Player flags (+$07 of the player actor).
pub mod flag {
    pub const PIVOT_ACTIVE: u8 = 0x01;
    pub const SATELLITE_ACTIVE: u8 = 0x02;
    pub const GARBAGE_FALLING: u8 = 0x04;
    pub const SOFT_DROP_POINT: u8 = 0x08;
}

/// Something the port can't run yet; the engine stops instead of guessing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unported(pub &'static str);

#[derive(Clone, Debug, PartialEq)]
pub enum Actor {
    /// A slot used by an actor this port doesn't model (keeps slot allocation right).
    Opaque,
    /// Like `Opaque`, but allocated by `FindActorSlotQuick` (bit 7 of +$01 clear), so a
    /// full table lets `FindActorSlot` take its slot.
    OpaqueQuick,
    /// An unmodelled actor that deletes itself when its turn comes this frame (its slot
    /// is free for anything spawned later in the frame).
    Vanishing,
    Player(Player),
    Pivot(Pivot),
    Satellite(Satellite),
    Effect(Effect),
    Burst(Burst),
    Piece(Piece),
    GarbageClear(GarbageClear),
    Glow(Glow),
    GarbageDrop(GarbageDrop),
    GarbageColumn(GarbageColumn),
    Icon(Icon),
    PairHolder(PairHolder),
    PreviewBean(PreviewBean),
    Referee(Referee),
    Sparkle(Sparkle),
    Animator(Animator),
    Danger(Danger),
    DangerSpark(DangerSpark),
    Portrait(Portrait),
    PortraitPalette(PortraitPalette),
    CoconutsLight(CoconutsLight),
    FfuzzyEyes(FfuzzyEyes),
    FfuzzyPalette(FfuzzyPalette),
    PaletteFade(PaletteFade),
    Star(Star),
    StarSpark(StarSpark),
    Banner(Banner),
    FieldWipe(FieldWipe),
    FallingColumn(FallingColumn),
    PasswordBean(PasswordBean),
    ContinueSign(ContinueSign),
    BouncingBean(BouncingBean),
    BounceSparkle(BounceSparkle),
    BgSlide(BgSlide),
    BgRunner(BgRunner),
    BgSpark(BgSpark),
    ShakeField(ShakeField),
    NameEntry(NameEntry),
    RobotnikVoice(RobotnikVoice),
    FlowWait(FlowWait),
    OpponentScreen(OpponentScreen),
    OpponentPortraits(OpponentPortraits),
    IntroScript(IntroScript),
    IntroBoxPart(IntroBoxPart),
    OpponentIntro(OpponentIntro),
    IntroExtra(IntroExtra),
    IntroClouds(IntroClouds),
    IntroShip(IntroShip),
    Transition(Transition),
    RobotnikIntro(RobotnikIntro),
    LairSprite(LairSprite),
    LairFlier(LairFlier),
    LairLights(LairLights),
    ThrownPiece(ThrownPiece),
    GameOverTimer(GameOverTimer),
    GameOverLetter(GameOverLetter),
    CutsceneRobotnik(CutsceneRobotnik),
    HighScoreTable(HighScoreTable),
    DefeatSounds(DefeatSounds),
    DefeatShip(DefeatShip),
    DefeatPiece(DefeatPiece),
    DefeatPart(DefeatPart),
    /// `loc_10486`: a button skips the ending's current part.
    Skip,
    SparkleSpawner(SparkleSpawner),
    ShipSparkle(ShipSparkle),
    RoleCall(RoleCall),
    Walker(Walker),
    /// `sub_104E2`.
    BackdropProp,
    Typist(Typist),
    LairReveal(LairReveal),
    HasBean(HasBean),
    HasBeanShadow(HasBeanShadow),
    FireworkColors(FireworkColors),
    FireworkSpawner(FireworkSpawner),
    FireworkSpark(FireworkSpark),
    Dawn(Dawn),
    Epilogue(Epilogue),
    CreditLine(CreditLine),
    TableNameEntry(TableNameEntry),
    TableCursor(TableCursor),
    SegaLogo(SegaLogo),
    CtrlSkip(CtrlSkip),
    /// `ActTitleHandler`.
    TitleHandler,
    TitleFade(TitleFade),
    TitleRobotnik(TitleRobotnik),
    MachineText(MachineText),
    TitlePiece(TitlePiece),
    TitleParticle(TitleParticle),
    TitleBean(TitleBean),
    /// `ActTitleRobotnikText` (its y follows the machine's shake).
    RobotnikText { y: u16 },
    /// The title's trademark sign.
    Trademark,
    MainMenu(MainMenu),
    MenuClouds(MenuClouds),
    Options(Options),
    PasswordEntry(PasswordEntry),
    PasswordSprite(PasswordSprite),
    PasswordColors(PasswordColors),
}

impl Actor {
    /// Allocated by `FindActorSlotQuick` (pop visuals): `FindActorSlot` may evict it.
    pub fn evictable(&self) -> bool {
        matches!(
            self,
            Actor::Burst(_)
                | Actor::Piece(_)
                | Actor::GarbageClear(_)
                | Actor::Icon(_)
                | Actor::OpaqueQuick
                | Actor::Sparkle(_)
                | Actor::DangerSpark(_)
                | Actor::Star(_)
                | Actor::StarSpark(_)
                | Actor::BouncingBean(_)
                | Actor::BounceSparkle(_)
                | Actor::RobotnikVoice(_)
                | Actor::IntroBoxPart(_)
                | Actor::LairLights(_)
                | Actor::IntroExtra(IntroExtra::Sparks { .. })
                | Actor::ShakeField(_)
                | Actor::GameOverLetter(_)
                | Actor::SparkleSpawner(_)
                | Actor::Walker(_)
                | Actor::BackdropProp
                | Actor::Typist(_)
                | Actor::HasBean(_)
                | Actor::FireworkColors(_)
                | Actor::FireworkSpawner(_)
        )
    }

    /// The original resume address of this actor's current state (None for `Opaque`).
    pub fn code(&self) -> Option<u32> {
        match self {
            Actor::Opaque | Actor::OpaqueQuick | Actor::Vanishing => None,
            Actor::Player(p) => Some(p.state.code()),
            Actor::Pivot(p) => Some(p.state.code()),
            Actor::Satellite(s) => Some(s.state.code()),
            Actor::Effect(e) => Some(e.state.code()),
            Actor::Burst(b) => Some(b.state.code()),
            Actor::Piece(p) => Some(p.state.code()),
            Actor::GarbageClear(_) => Some(0x4DA4),
            Actor::Glow(g) => Some(g.state.code()),
            Actor::GarbageDrop(d) => Some(if d.started { 0x48BA } else { 0x48A2 }),
            Actor::GarbageColumn(c) => Some(if c.landed { 0x5044 } else { 0x486A }),
            Actor::Icon(_) => Some(0x9962),
            Actor::PairHolder(_) => Some(PAIR_HOLDER_CODE),
            Actor::PreviewBean(_) => Some(PREVIEW_BEAN_CODE),
            Actor::Referee(r) => Some(r.state.code()),
            Actor::Sparkle(_) => Some(SPARKLE_CODE),
            Actor::Animator(a) => Some(if a.started { ANIMATOR_CODE } else { 0x3BBA }),
            Actor::Danger(_) => Some(DANGER_CODE),
            Actor::DangerSpark(s) => Some(s.state.code()),
            Actor::Portrait(p) => Some(if p.started { PORTRAIT_CODE } else { 0x60F4 }),
            Actor::PortraitPalette(p) => Some(if p.started { PORTRAIT_PALETTE_CODE } else { 0x6F38 }),
            Actor::CoconutsLight(c) => Some(if c.started { COCONUTS_LIGHT_CODE } else { 0x6954 }),
            Actor::FfuzzyEyes(_) => Some(FFUZZY_EYES_CODE),
            Actor::RobotnikVoice(v) => Some(v.code()),
            Actor::FlowWait(w) => Some(w.code()),
            Actor::OpponentScreen(s) => Some(if s.started { OPPONENT_SCREEN_CODE } else { 0xD6A8 }),
            Actor::OpponentPortraits(p) => Some(p.code()),
            Actor::IntroScript(s) => Some(s.state.code()),
            Actor::IntroBoxPart(p) => Some(if p.tail { 0x113B8 } else { 0x113D8 }),
            Actor::OpponentIntro(o) => Some(if o.leaving { 0xAAD6 } else { OPPONENT_INTRO_CODE }),
            Actor::IntroExtra(e) => Some(match e {
                IntroExtra::Flash { .. } => 0xA758,
                IntroExtra::Sway { .. } => 0xA560,
                IntroExtra::Cycle { .. } => 0xA7C4,
                IntroExtra::Sparks { .. } => 0xA85C,
                IntroExtra::Spark { .. } => 0xA8E2,
                IntroExtra::Orbit { .. } => 0xA5F6,
                IntroExtra::OrbitPiece { .. } => 0xA692,
            }),
            Actor::IntroClouds(_) => Some(INTRO_CLOUDS_CODE),
            Actor::IntroShip(s) => Some(s.code),
            Actor::Transition(t) => Some(t.code()),
            Actor::RobotnikIntro(r) => Some(match (r.started, r.leaving) {
                (false, _) => 0xAADA,
                (true, false) => ROBOTNIK_INTRO_CODE,
                (true, true) => 0xAAD6,
            }),
            Actor::LairSprite(s) => Some(s.kind.code()),
            Actor::LairFlier(f) => Some(f.code()),
            Actor::LairLights(_) => Some(LAIR_LIGHTS_CODE),
            Actor::ThrownPiece(p) => Some(p.code()),
            Actor::GameOverTimer(t) => Some(if t.done { 0xDB64 } else { GAME_OVER_TIMER_CODE }),
            Actor::GameOverLetter(l) => Some(l.phase.code()),
            Actor::CutsceneRobotnik(r) => Some(if r.started { CUTSCENE_ROBOTNIK_CODE } else { 0xEE10 }),
            Actor::HighScoreTable(t) => Some(t.phase.code()),
            Actor::DefeatSounds(s) => Some([0xCB52, 0xCB68, 0xCB86, 0xCBA4][usize::from(s.stage.min(3))]),
            Actor::DefeatShip(s) => Some(if s.flying { 0xCB1C } else { 0xCAFC }),
            Actor::DefeatPiece(p) => Some(p.phase.code()),
            Actor::DefeatPart(p) => Some(p.code()),
            Actor::Skip => Some(SKIP_CODE),
            Actor::SparkleSpawner(_) => Some(SPARKLE_SPAWNER_CODE),
            Actor::ShipSparkle(_) => Some(SHIP_SPARKLE_CODE),
            Actor::RoleCall(r) => Some(r.code()),
            Actor::Walker(w) => Some(w.code()),
            Actor::BackdropProp => Some(BACKDROP_PROP_CODE),
            Actor::Typist(_) => Some(TYPIST_CODE),
            Actor::LairReveal(l) => Some(l.code()),
            Actor::HasBean(b) => Some(b.code()),
            Actor::HasBeanShadow(s) => Some(s.code()),
            Actor::FireworkColors(c) => Some([0xFAE6, 0xFAF6, 0xFAFC][usize::from(c.phase.min(2))]),
            Actor::FireworkSpawner(s) => Some([0xFBA0, 0xFBAE, 0xFBD8][usize::from(s.phase.min(2))]),
            Actor::FireworkSpark(_) => Some(FIREWORK_SPARK_CODE),
            Actor::Dawn(d) => Some(d.code()),
            Actor::Epilogue(e) => Some(e.code()),
            Actor::CreditLine(l) => Some(if l.stopped { 0xB646 } else { 0xB60E }),
            Actor::TableNameEntry(n) => Some(n.phase.code()),
            Actor::TableCursor(_) => Some(TABLE_CURSOR_CODE),
            Actor::SegaLogo(l) => Some(l.code()),
            Actor::CtrlSkip(_) => Some(CTRL_SKIP_CODE),
            Actor::TitleHandler => Some(TITLE_HANDLER_CODE),
            Actor::TitleFade(t) => Some(t.code()),
            Actor::TitleRobotnik(_) => Some(TITLE_ROBOTNIK_CODE),
            Actor::MachineText(m) => Some(m.code()),
            Actor::TitlePiece(p) => Some(p.code()),
            Actor::TitleParticle(_) => Some(TITLE_PARTICLE_CODE),
            Actor::TitleBean(b) => Some(b.code()),
            Actor::RobotnikText { .. } => Some(ROBOTNIK_TEXT_CODE),
            Actor::Trademark => Some(TRADEMARK_CODE),
            Actor::MainMenu(m) => Some(m.code()),
            Actor::MenuClouds(_) => Some(MENU_CLOUDS_CODE),
            Actor::Options(o) => Some(o.code()),
            Actor::PasswordEntry(p) => Some(p.code()),
            Actor::PasswordSprite(s) => Some(s.kind.code()),
            Actor::PasswordColors(c) => Some(if c.started { PASSWORD_COLORS_CODE } else { 0xD3B4 }),
            Actor::FfuzzyPalette(p) => Some(if p.started { FFUZZY_PALETTE_CODE } else { 0x6E46 }),
            Actor::PaletteFade(_) => Some(PALETTE_FADE_CODE),
            Actor::Star(s) => Some(s.state.code()),
            Actor::StarSpark(_) => Some(STAR_SPARK_CODE),
            Actor::Banner(b) => Some(b.state.code()),
            Actor::FieldWipe(_) => Some(FIELD_WIPE_CODE),
            Actor::FallingColumn(c) => Some(if c.falling { 0x838A } else { 0x8386 }),
            Actor::PasswordBean(_) => Some(PASSWORD_BEAN_CODE),
            Actor::ContinueSign(_) => Some(CONTINUE_SIGN_CODE),
            Actor::BouncingBean(b) => Some(b.state.code()),
            Actor::BounceSparkle(_) => Some(BOUNCE_SPARKLE_CODE),
            Actor::BgSlide(_) => Some(BG_SLIDE_CODE),
            Actor::BgRunner(_) => Some(BG_RUNNER_CODE),
            Actor::BgSpark(_) => Some(BG_SPARK_CODE),
            Actor::ShakeField(_) => Some(SHAKE_FIELD_CODE),
            Actor::NameEntry(n) => Some(n.state.code()),
        }
    }
}

// ---------------------------------------------------------------- player

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayerState {
    /// $3D1C: just spawned (`loc_3C00`): set up the pair holder and the field.
    Setup,
    /// $3D80: until flag bit 0 clears, then take the fall rate for the speed step.
    WaitReady,
    /// $3DA0: before the match starts, until the referee clears flag bit 1.
    WaitStart,
    /// $3E6E: pair in play; count frames, wait for both beans to be written.
    WaitPair,
    /// $3E9A: look for pops (`sub_5960`).
    FindPops,
    /// $3EA6: redraw links (`sub_5782`).
    Redraw,
    /// $3EB6: pops â†’ chain; otherwise start the settle delay.
    Decide,
    /// $3EC6: settle delay (`ActorBookmark` with `aDelay`).
    SettleDelay,
    /// $3ECA: send garbage if a chain happened, then drop incoming garbage.
    AfterDelay,
    /// $3DD2: wait while garbage falls.
    WaitGarbage,
    /// $3DF6, $3E08, $3E1A: bookkeeping frames before the next pair.
    PreSpawn1,
    PreSpawn2,
    PreSpawn3,
    /// $3E44: spawn the next pair (`sub_4E24`).
    Spawn,
    /// $3EFA: pops found; score this step (`sub_9A56`).
    PopScore,
    /// $7A12: the pair couldn't spawn; who won, stars, the field falls away.
    TopOut,
    /// $7A1E: until the field's actors are done, then clear it.
    Wipe,
    /// $7A3C: on to the results.
    Cleared,
    /// $7D90 / $7D96: a pause (a button skips it), then the time is shown.
    ResultsWait,
    ResultsShown,
    /// $7DA4 / $7DAA: then the bonus, if any.
    BonusWait,
    Bonus,
    /// $7DEE / $7DF4: count the bonus into the score (a button finishes it).
    TallyWait,
    Tally,
    /// $7E50 / $7E56: no bonus: a pause, then the password.
    NoBonusWait,
    NoBonus,
    /// $7EA4: wait for a button.
    Continue,
    /// `loc_7E68` (the last stage won): the ending text shows, then the game flow.
    FinalWait,
    FinalLeave,
    FinalDone,
    /// $7EB0 / $7EB8: done; the game flow takes over.
    Leave,
    Done,
    /// $7ED6: P1 lost: "continue?" for $80 frames (a button skips).
    ContinueWait,
    /// $7EDC: the high score table; $7EF6: then clear the field; $7F1A: done.
    HighScore,
    ContinueCheck,
    ContinueDone,
    /// $3F06: back up the field, start blinking.
    PopBackup,
    /// $3F12: blink the popping beans for 24 frames (`sub_49D2`).
    Blink,
    /// $3F1A: add the points, spawn the pop effects (`sub_9BBA`, `CheckPuyoPop`).
    PopSpawn,
    /// $3F30: let the pop effects play (24 frames).
    PopWait,
    /// $3F34: close the gaps (`sub_4DB8`) and start the fall animation.
    Compact,
    /// $3F42: wait for the fall animation (plane command $89).
    FallWait,
    /// $3F5C: next chain step, look for pops again.
    ChainNext,
}

impl PlayerState {
    pub fn code(self) -> u32 {
        match self {
            PlayerState::Setup => 0x3D1C,
            PlayerState::WaitReady => 0x3D80,
            PlayerState::WaitStart => 0x3DA0,
            PlayerState::WaitPair => 0x3E6E,
            PlayerState::FindPops => 0x3E9A,
            PlayerState::Redraw => 0x3EA6,
            PlayerState::Decide => 0x3EB6,
            PlayerState::SettleDelay => 0x3EC6,
            PlayerState::AfterDelay => 0x3ECA,
            PlayerState::WaitGarbage => 0x3DD2,
            PlayerState::PreSpawn1 => 0x3DF6,
            PlayerState::PreSpawn2 => 0x3E08,
            PlayerState::PreSpawn3 => 0x3E1A,
            PlayerState::Spawn => 0x3E44,
            PlayerState::PopScore => 0x3EFA,
            PlayerState::TopOut => 0x7A12,
            PlayerState::Wipe => 0x7A1E,
            PlayerState::Cleared => 0x7A3C,
            PlayerState::ResultsWait => 0x7D90,
            PlayerState::ResultsShown => 0x7D96,
            PlayerState::BonusWait => 0x7DA4,
            PlayerState::Bonus => 0x7DAA,
            PlayerState::TallyWait => 0x7DEE,
            PlayerState::Tally => 0x7DF4,
            PlayerState::NoBonusWait => 0x7E50,
            PlayerState::NoBonus => 0x7E56,
            PlayerState::Continue => 0x7EA4,
            PlayerState::Leave => 0x7EB0,
            PlayerState::Done => 0x7EB8,
            PlayerState::FinalWait => 0x7E86,
            PlayerState::FinalLeave => 0x7E8C,
            PlayerState::FinalDone => 0x7E94,
            PlayerState::ContinueWait => 0x7ED6,
            PlayerState::HighScore => 0x7EDC,
            PlayerState::ContinueCheck => 0x7EF6,
            PlayerState::ContinueDone => 0x7F1A,
            PlayerState::PopBackup => 0x3F06,
            PlayerState::Blink => 0x3F12,
            PlayerState::PopSpawn => 0x3F1A,
            PlayerState::PopWait => 0x3F30,
            PlayerState::Compact => 0x3F34,
            PlayerState::FallWait => 0x3F42,
            PlayerState::ChainNext => 0x3F5C,
        }
    }
}

/// A player's field controller (tag $F1/$F2).
#[derive(Clone, Debug, PartialEq)]
pub struct Player {
    pub id: u8,
    pub state: PlayerState,
    pub flags: u8,
    /// +$26 word: frame counter (high byte) while a pair is in play, pop count after.
    pub w26: u16,
    /// aDelay (+$24).
    pub delay: u16,
    /// Chain step (+$09).
    pub chain: u8,
    /// Fall rate (+$1A).
    pub fall_speed: u16,
    /// +$2B: indexes the settle delay.
    pub speed_level: u8,
    /// Pending garbage (+$14).
    pub pending_garbage: u16,
    /// Speed step (+$08): bumped each time `beans_count` reaches `threshold`.
    pub speed_index: u8,
    /// Score (+$0A long) and garbage accumulator (+$0E long).
    pub score: u32,
    pub acc: u32,
    /// This step's base points (+$12) and bonus (+$1E).
    pub base: u16,
    pub bonus: u16,
    /// Beans popped (+$16, capped 9999) and since the last speed-up (+$18).
    pub beans_total: u16,
    pub beans_count: u16,
    /// Beans to pop before the next speed-up (+$1C).
    pub threshold: u16,
    /// Garbage this player's chains sent while no opponent actor is modelled.
    pub garbage_sent: u16,
    /// The CPU's plan (`sub_127BA`): target column (+$20) and rotation (+$21), frames
    /// to move (+$2C) and to pause (+$2D). Its timer and phase are `w26`'s two bytes.
    pub target: (u8, u8),
    pub move_frames: u8,
    pub pause_frames: u8,
    /// The other player's actor (+$2E) and this player's pair holder (+$32).
    pub other_slot: usize,
    pub holder_slot: Option<usize>,
    /// +$28: the bonus counted per frame in the results.
    pub w28: u16,
    /// Work for the chain step in progress.
    pub pop: Option<PopWork>,
}

/// One chain step in progress.
#[derive(Clone, Debug, PartialEq)]
pub struct PopWork {
    pub groups: Vec<Group>,
    /// Popping beans plus the garbage they clear (both blink).
    pub cells: Vec<(usize, usize)>,
    /// Field as it was before blinking (`pPuyosCopy`), and its cell bytes.
    pub backup: Field,
    pub backup_bytes: [u8; COLS * ROWS],
    /// Beans dropping after the gaps closed (`pUnk6`).
    pub falling: Vec<FallingBean>,
}

/// A bean animating down after a pop: 2 steps per row, then a bounce (`SpecPlane89`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallingBean {
    /// Where it was before the gaps closed.
    pub col: usize,
    pub row: usize,
    pub rows: u8,
    /// Steps taken so far (low byte of its `pUnk6` word).
    pub counter: u8,
    /// Bounce length: index of the end marker in its table (by color group).
    pub bounce: u8,
}

// ---------------------------------------------------------------- pair

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PivotState {
    /// $4F06: just spawned.
    Init,
    /// $4F14: under the player's control.
    Controlled,
    /// $4F4E: stopped and hidden (its player is gone).
    Hidden,
    /// $4F8A: dropping further on its own (`sub_4948`).
    Falling,
    /// $4FA8: landing animation, then written to the field.
    Squash,
}

impl PivotState {
    pub fn code(self) -> u32 {
        match self {
            PivotState::Init => 0x4F06,
            PivotState::Controlled => 0x4F14,
            PivotState::Hidden => 0x4F4E,
            PivotState::Falling => 0x4F8A,
            PivotState::Squash => 0x4FA8,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SatelliteState {
    /// $5474: just spawned.
    Init,
    /// $5482: following the pivot.
    Follow,
    /// $54D4: the pivot landed; supported or not?
    Check,
    /// $54FA: dropping further on its own.
    Falling,
    /// $5514: landing animation, then written to the field.
    Squash,
}

impl SatelliteState {
    pub fn code(self) -> u32 {
        match self {
            SatelliteState::Init => 0x5474,
            SatelliteState::Follow => 0x5482,
            SatelliteState::Check => 0x54D4,
            SatelliteState::Falling => 0x54FA,
            SatelliteState::Squash => 0x5514,
        }
    }
}

/// A bean dropping under gravity (`sub_4948`): y and speed are 16.16 pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fall {
    pub y: u32,
    pub vel: u32,
    /// +$1E: added to `vel` every frame ($3000 for the pair, per column for garbage).
    pub accel: u16,
    /// +$20 word: y where the fall started, minus 15.
    pub base: u16,
}

impl Fall {
    fn start(y: u32) -> Fall {
        Fall::with(y, FALL_START_VEL, FALL_ACCEL)
    }

    fn with(y: u32, vel: u32, accel: u16) -> Fall {
        Fall { y, vel, accel, base: ((y >> 16) as u16).wrapping_sub(15) }
    }

    /// One frame. Returns true once the bean rests (y snapped to 8 px). `row` advances
    /// each time the bean crosses into the next cell.
    fn step(&mut self, field: &Field, col: u16, row: &mut u16) -> bool {
        let next = self.y.wrapping_add(self.vel);
        let before = ((self.y >> 16) as u16).wrapping_sub(self.base);
        let after = ((next >> 16) as u16).wrapping_sub(self.base);
        if (before ^ after) & 0x10 != 0 {
            let (dc, dr) = DIR[2];
            if field.blocked(col as i16 + dc, *row as i16 + dr) {
                // Only the pixel word is rewritten: the fraction stays.
                let pixel = ((next >> 16) as u16) & !7;
                self.y = (u32::from(pixel) << 16) | (self.y & 0xFFFF);
                return true;
            }
            *row = row.wrapping_add(1);
        }
        self.y = next;
        self.vel = self.vel.wrapping_add(u32::from(self.accel)).min(FALL_MAX_VEL);
        false
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pivot {
    pub player_slot: usize,
    /// +$2A: the player it belongs to (which field it lands in).
    pub player_id: u8,
    pub satellite_slot: usize,
    pub color: BeanColor,
    pub state: PivotState,
    /// Controlled-pair state (also carries the satellite's turn and animation).
    pub pair: Pair,
    /// Own +$07 after landing: bit 0 pivot supported, bit 1 satellite, bit 2 landed.
    pub flags: u8,
    /// Screen y (16.16), recomputed from row/fall while controlled (`sub_543C`).
    pub y: u32,
    pub fall: Fall,
}

impl Pivot {
    /// `sub_543C` (y only): top of field + (row-2)*16, +8 in the lower half of a cell.
    fn place(&mut self) {
        let half = if self.pair.fall & 0x8000 != 0 { 8 } else { 0 };
        let y = FIELD_TOP_Y.wrapping_add(self.pair.row.wrapping_sub(2).wrapping_mul(16)).wrapping_add(half);
        self.y = (u32::from(y) << 16) | (self.y & 0xFFFF);
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Satellite {
    pub pivot_slot: usize,
    pub player_slot: usize,
    pub player_id: u8,
    pub color: BeanColor,
    pub state: SatelliteState,
    /// After the pivot lands: own column/row, copied flags, animation, fall.
    pub col: u16,
    pub row: u16,
    pub flags: u8,
    /// Screen x (+$0A, from `sub_5552` while it follows the pivot) and y (16.16).
    pub x: u16,
    pub y: u32,
    pub anim: Anim,
    pub fall: Fall,
}

// ---------------------------------------------------------------- effect

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EffectState {
    /// $4FCE: wait 24 frames.
    Wait,
    /// $4FE6: start rising.
    Start,
    /// $4FF4: rise 1 px every 4 frames, 6 times, then vanish.
    Rise,
}

impl EffectState {
    pub fn code(self) -> u32 {
        match self {
            EffectState::Wait => 0x4FCE,
            EffectState::Start => 0x4FE6,
            EffectState::Rise => 0x4FF4,
        }
    }
}

/// What's left of a pair bean after it is written into the field (`loc_4FC0`).
#[derive(Clone, Debug, PartialEq)]
pub struct Effect {
    pub player: u8,
    /// The landed bean's color (+$08), frame (+$09) and x (+$0A): same slot, same sprite.
    pub color: u8,
    pub frame: u8,
    pub x: u16,
    pub state: EffectState,
    pub delay: u16,
    pub steps_left: u8,
    pub ticks: u8,
    pub y: u32,
}

impl Effect {
    fn new(player: u8, y: u32, color: BeanColor, frame: u8, x: u16) -> Effect {
        // loc_4FC0: frame 7 (the landed bean's shine).
        let _ = frame;
        Effect { player, color: color as u8, frame: 7, x, state: EffectState::Wait, delay: 0x18, steps_left: 0, ticks: 0, y }
    }
}

// ---------------------------------------------------------------- pop visuals

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BurstState {
    /// $4B72: staggered start (`aDelay` 0, 4, 8, 12 by pop order).
    Wait,
    /// $4B76: burst into pieces.
    Burst,
    /// $4B7E: pop animation, then gone.
    Animate,
}

impl BurstState {
    pub fn code(self) -> u32 {
        match self {
            BurstState::Wait => 0x4B72,
            BurstState::Burst => 0x4B76,
            BurstState::Animate => 0x4B7E,
        }
    }
}

/// A popping bean (`ActPuyoPop`).
#[derive(Clone, Debug, PartialEq)]
pub struct Burst {
    pub player: u8,
    pub color: u8,
    pub x: u16,
    pub y: u16,
    pub delay: u16,
    pub anim: Anim,
    pub state: BurstState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceState {
    /// $4D74: just thrown.
    Start,
    /// $4D7C: wait 4 frames.
    Wait,
    /// $4D80: fly under gravity until the animation ends or it leaves the screen.
    Fly,
}

impl PieceState {
    pub fn code(self) -> u32 {
        match self {
            PieceState::Start => 0x4D74,
            PieceState::Wait => 0x4D7C,
            PieceState::Fly => 0x4D80,
        }
    }
}

/// A piece flying off a popped bean (`ActPuyoPoppedPiece`).
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub player: u8,
    pub color: u8,
    pub motion: Motion,
    pub draw: u8,
    pub delay: u16,
    pub anim: Anim,
    pub state: PieceState,
}

/// Garbage cleared by a pop (`ActGarbageRemove`, $4DA4): plays its animation, then gone.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageClear {
    pub player: u8,
    pub x: u16,
    pub y: u16,
    pub anim: Anim,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlowState {
    /// $23628: just spawned (first chain step: straight to `End`).
    Start,
    /// $23642, $2364C: hover for 8 frames.
    Hover,
    HoverWait,
    /// $23658: aim at the other side.
    Launch,
    /// $2369A: fly there in 33 frames, wobbling.
    Fly,
    /// $23736: final animation, then gone.
    End,
}

impl GlowState {
    pub fn code(self) -> u32 {
        match self {
            GlowState::Start => 0x23628,
            GlowState::Hover => 0x23642,
            GlowState::HoverWait => 0x2364C,
            GlowState::Launch => 0x23658,
            GlowState::Fly => 0x2369A,
            GlowState::End => 0x23736,
        }
    }
}

/// The chain glow (`ActGarbageGlow`).
#[derive(Clone, Debug, PartialEq)]
pub struct Glow {
    pub player: u8,
    /// The chaining player's actor (+$2E).
    pub player_slot: usize,
    /// Chain step it was spawned on (+$2B; 0 = first step).
    pub chain: u8,
    pub motion: Motion,
    pub draw: u8,
    /// Un-wobbled x (+$1E word) and flight counter (+$26 word).
    pub base_x: u16,
    pub counter: u16,
    pub delay: u16,
    pub anim: Anim,
    pub state: GlowState,
}

// ---------------------------------------------------------------- garbage

/// Per-column acceleration of falling garbage (`word_47BA`).
const GARBAGE_ACCEL: [u16; 6] = [0x2400, 0x2600, 0x2000, 0x2A00, 0x2200, 0x2800];
/// Start speed of falling garbage, px/frame, by difficulty (`word_4730`).
const GARBAGE_START_SPEED: [u16; 3] = [3, 2, 0];
/// Small-icon nudge on P1's side, by size (`unk_9924`).
const ICON_NUDGE: [i16; 5] = [4, -8, -20, -32, -44];

/// A garbage drop in progress (`ActGarbagePuyo`): counts the falling columns, shakes
/// the field each time one lands, and clears the player's "garbage falling" flag when
/// everything is down.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageDrop {
    pub player_slot: usize,
    /// +$2A: the player's id (its field's columns wobble).
    pub player: u8,
    /// False before its first run ($48A2), true in the update loop ($48BA).
    pub started: bool,
    /// Columns still falling (+$26) and the count last seen (+$28).
    pub falling: u16,
    pub seen: u16,
    /// +$20: (1 + columns) × $100, the shake strength when a column lands.
    pub strength: u16,
    /// +$38: current shake strength, -$40 per frame; +$36 shake phase.
    pub shake: u16,
    pub phase: u8,
    /// +$07 bit 0: the "incoming!" voice played.
    pub voiced: bool,
    /// +$09: copied from the last column to land.
    pub frame: u8,
}

/// One column of falling garbage (`sub_486A`): the whole stack drops as one sprite and
/// is written into the field when it lands.
#[derive(Clone, Debug, PartialEq)]
pub struct GarbageColumn {
    pub player: u8,
    pub drop_slot: usize,
    pub col: u16,
    /// Row of the stack's bottom bean (+$1C).
    pub row: u16,
    /// +$09: beans in the stack + $13 (and +6 while a single bean lingers).
    pub frame: u8,
    pub fall: Fall,
    /// Landed: a single bean lingers 48 frames ($5044); taller stacks vanish at once.
    pub landed: bool,
    pub linger: u16,
}

/// An incoming-garbage icon above a field (`sub_9962`): slides into place over 16
/// frames, vanishes when its side's icons are redrawn.
#[derive(Clone, Debug, PartialEq)]
pub struct Icon {
    /// Which generation counter it watches (0 = over P2's field, 1 = over P1's).
    pub side: u8,
    pub generation: u8,
    /// +$09: 5 = 6 beans, 6 = 30 beans, 0-4 = 1-5 beans.
    pub frame: u8,
    pub motion: Motion,
    /// Frames of sliding left (+$26).
    pub slide: u16,
}

// ---------------------------------------------------------------- engine

/// What the engine needs from outside each frame.
#[derive(Clone, Debug, Default)]
pub struct Inputs {
    pub pads: [PadInput; 2],
    pub keys: [RotateKeys; 2],
    /// The next pair (pivot, satellite) for a player with no pair holder in the engine.
    pub next_pair: [Option<(BeanColor, BeanColor)>; 2],
    /// Slots the next spawned pivot/satellite get, when the caller wants to force them
    /// (slots of actors this port doesn't model can't be predicted yet).
    pub spawn_slots: [Option<(usize, usize)>; 2],
}

impl Default for RotateKeys {
    fn default() -> Self {
        RotateKeys([2, 1, 2])
    }
}

/// `ActPuyoPoppedPiece`.
fn run_piece(mut p: Piece) -> Option<Piece> {
    match p.state {
        PieceState::Start => {
            p.delay = 4;
            p.state = PieceState::Wait;
        }
        PieceState::Wait => {
            if wait(&mut p.delay) {
                p.state = PieceState::Fly;
            }
        }
        PieceState::Fly => {
            p.draw = 0x87;
            if p.anim.step() || p.motion.step(p.draw) {
                return None;
            }
        }
    }
    Some(p)
}

/// One `SpecPlane89` step of the falling beans: whether any were left, and the beans to
/// draw this step (from the bottom right cell back to the top left, like the game).
fn fall_steps(work: &mut PopWork) -> (bool, Vec<(FallingBean, BeanColor)>) {
    let mut any = false;
    let mut steps = vec![];
    let backup = &work.backup;
    work.falling.retain_mut(|b| {
        any = true;
        let drop_steps = b.rows * 2;
        if b.counter >= drop_steps && b.counter - drop_steps + 1 >= b.bounce {
            return false; // reached the end marker
        }
        let color = backup.get(b.col as i16, b.row as i16).unwrap_or(BeanColor::Garbage);
        steps.push((*b, color));
        b.counter += 1;
        true
    });
    steps.sort_by_key(|(b, _)| std::cmp::Reverse((b.row, b.col)));
    (any, steps)
}

/// `sub_9C64`: points go to the score (capped) and the garbage accumulator (capped
/// at 65,535).
fn add_points(p: &mut Player, points: u32) {
    p.acc = p.acc.wrapping_add(points);
    if p.acc >> 16 != 0 {
        p.acc = 0xFFFF;
    }
    p.score = p.score.checked_add(points).unwrap_or(SCORE_CAP).min(SCORE_CAP);
}

/// `sub_4DB8`: close the gaps in rows 1-13 at once (row 0 stays put), noting how far
/// each bean dropped and which bounce its color plays.
fn close_gaps(field: &mut Field, backup: &Field) -> Vec<FallingBean> {
    let mut falling = vec![];
    for col in (0..COLS).rev() {
        let mut target = ROWS - 1;
        let mut gap = 0u8;
        for row in (1..ROWS).rev() {
            gap += 1;
            let Some(bean) = field.get(col as i16, row as i16) else { continue };
            field.set(col, row, None);
            field.set(col, target, Some(bean));
            target -= 1;
            gap -= 1;
            if gap != 0 {
                let color = backup.get(col as i16, row as i16).unwrap_or(bean);
                let group = if color == BeanColor::Garbage { 3 } else { ((color as usize) >> 1) & 3 };
                falling.push(FallingBean { col, row, rows: gap, counter: 0, bounce: BOUNCE_LEN[group] });
            }
        }
    }
    falling
}

/// The CPU's plan as the player actor holds it (timer and phase in `w26`).
fn player_plan(p: &Player) -> ai::Plan {
    ai::Plan {
        col: p.target.0,
        rot: p.target.1,
        timer: (p.w26 >> 8) as u8,
        phase: p.w26 as u8,
        move_frames: p.move_frames,
        pause_frames: p.pause_frames,
    }
}

fn set_player_plan(p: &mut Player, plan: ai::Plan) {
    p.target = (plan.col, plan.rot);
    p.w26 = u16::from(plan.timer) << 8 | u16::from(plan.phase);
    p.move_frames = plan.move_frames;
    p.pause_frames = plan.pause_frames;
}

/// `ActorBookmark` with a delay: true when the wait is over.
fn wait(delay: &mut u16) -> bool {
    if *delay == 0 {
        return true;
    }
    *delay -= 1;
    *delay == 0
}

#[derive(Clone)]
pub struct Engine {
    pub slots: Vec<Option<Actor>>,
    pub fields: [Field; 2],
    /// `puyos_popping` per player (MarkPuyoSpot does nothing while set).
    pub popping: [bool; 2],
    /// `level_mode` (0 scenario, 1 vs, 2 exercise, ...).
    pub level_mode: u8,
    /// `byte_FF0104`: offsets the scenario speed table (from the difficulty).
    pub difficulty: u8,
    /// Plane commands $89 (post-pop fall animation) queued this frame, by player slot;
    /// the vertical-blank handler runs them before the next frame's actors.
    pub fall_commands: Vec<usize>,
    /// A deleted player's falling beans whose `$89` step was still queued (the ROM keeps
    /// them in the field, `pUnk6`): (its slot, player id, the work).
    pub orphan_fall: Option<(usize, u8, PopWork)>,
    /// The game's random number generator (`rng_seed`).
    pub rng: Rng,
    /// `Random` calls made during the last frame (vertical blank included).
    pub rng_calls: u32,
    /// `garbage_glow_x`/`_y`: where the last popped bean was; the glow starts there.
    pub glow_xy: (u16, u16),
    /// Spawns of actors this port doesn't have yet.
    pub unported_spawns: u32,
    /// `byte_FF19B2`/`_B3`: a player is in the middle of a chain.
    pub chaining: [bool; 2],
    /// `byte_FF19B0`/`_B1`: icon generations; icons of an older generation vanish.
    /// Index 0 = icons over P2's field, 1 = over P1's.
    pub icon_gen: [u8; 2],
    /// Pending garbage of a player with no actor in this engine (set by the caller).
    pub external_pending: [u16; 2],
    /// Sides whose icons this engine redrew during the last frame.
    pub icons_drawn: [bool; 2],
    /// `Random` calls made by unported actors, as (before slot, count): drawn just
    /// before that slot runs (used when checking against recordings).
    pub foreign_random: Vec<(usize, u32)>,
    /// Slots whose actors drew random numbers last frame, in order.
    pub rng_users: Vec<usize>,
    current_slot: usize,
    /// The CPU's byte image of each field: cells with links, group marks, its tables.
    pub ai_fields: [AiField; 2],
    /// Per-CPU memory between pieces (`byte_FF1D4E` / `byte_FF1D58`).
    pub cpu_memory: [ai::CpuMemory; 2],
    /// Pad reads since the last plan (`byte_FF1D0A` / `_0B`).
    pub cpu_reads: [u8; 2],
    /// `byte_FF1D0C` / `byte_FF1D0D`: for the CPU's controller sprites (not ported).
    pub cpu_pressed: u8,
    pub cpu_lean: u8,
    /// `word_FF198C` (scenario mode): 2 if the CPU has 36+ beans, 1 if the player has.
    pub crowding: u16,
    /// `opponent`, `level`, `time_minutes`, `control_player_1`.
    pub opponent: u8,
    pub level: u8,
    pub time_minutes: u16,
    pub control_player_1: u8,
    /// `frame_count`: counted by every vertical blank (soft-drop points on odd ones).
    pub frame_count: u16,
    /// `time_frames` (`HandleTime`, counted in the vertical blank while neither player
    /// is out); `time_minutes` follows from it.
    pub time_frames: u16,
    /// `player_1_flags` / `player_2_flags`: $7F once a player has started, bit 7 when out.
    pub player_flags: [u8; 2],
    /// `p1_puyo_order` / `p2_puyo_order`: the pairs dealt, two bytes each.
    pub bean_orders: [[u8; 256]; 2],
    /// `word_FF19A8`: high byte bit 7 = something hit the referee (not ported).
    pub referee_hit: u16,
    /// `byte_FF1121`: the danger effect is flashing (the portrait palette waits).
    pub danger_flash: u8,
    /// `word_FF1124`: high byte set = the portrait actors end.
    pub portrait_lock: u16,
    /// The generation word of each palette line's fade (`pal_fade_data`, $82 apart).
    pub pal_fade_gen: [u16; 4],
    /// `palette_buffer` (4 lines), the lines `LoadPalette` marked for the next vertical
    /// blank (`palette_pointers`), and each line's fade (`pal_fade_data`).
    pub palettes: [[u16; 16]; 4],
    pub palette_loads: [Option<PaletteLoad>; 4],
    pub pal_fades: [FadeData; 4],
    /// The fields' cell bytes as the drawing sees them, and the plane A tile writes
    /// (VRAM address, pattern word) made since the caller last took them.
    pub field_bytes: [FieldBytes; 2],
    pub plane_writes: Vec<(u16, u16)>,
    /// The centre panel's plane commands queued for the next vertical blank, and the
    /// portrait map waiting in the Enigma queue.
    pub panel_commands: Vec<PanelCommand>,
    /// `vscroll_buffer`: per 2-cell column, plane A's and plane B's vertical scroll
    /// (the vertical blank copies it to VSRAM).
    pub vscroll: [u16; 40],
    /// `eni_tilemap_buffer` (words) and `eni_tilemap_queue` (width - 1, height - 1, plane
    /// address), copied to the planes in the vertical blank.
    pub eni_buffer: Vec<u16>,
    pub eni_queue: Vec<(u16, u16, u16)>,
    /// The game flow: `bytecode_addr`, `bytecode_disabled` (the flag is
    /// `bytecode_flag`), and what it does to the VDP for the front end.
    pub bytecode_pc: u32,
    pub bytecode_disabled: bool,
    /// Vertical blanks that come in the middle of this logic frame (when its art
    /// decompression outlasts a video frame): taken at its first long step. Only the
    /// tests set it, from the original's timing.
    pub deferred_vblanks: u16,
    /// Of those, the ones that came after the frame's last long step (taken once its
    /// bytecode is done).
    pub deferred_tail: u16,
    pub vdp_events: Vec<VdpEvent>,
    /// `vdp_reg_0`.. (the register mirror), `hscroll_buffer` (planes A and B).
    pub vdp_regs: [u8; 0x13],
    pub hscroll: [u16; 0x200],
    /// `pressed_time`/`unpressed_time`, `word_FF1122` (buttons ignored while set),
    /// `level_transition_flag`, `byte_FF0115` (the last match's winner),
    /// `opponents_defeated` (by opponent).
    pub repeat_times: (u8, u8),
    pub input_lock: u16,
    pub level_transition: u16,
    pub last_winner: u8,
    pub opponents_defeated: [u8; 18],
    /// The intro: `use_lair_background`, `word_FF1990` (the script's cue to the
    /// opponent), `word_FF198E`, `word_FF1994` (the opponent's x, for the box's tail),
    /// `word_FF1126` (its high byte: clouds paused; the title keeps its state in the
    /// word), `byte_FF1128` (typing sound toggle), and an extra
    /// cued that isn't ported.
    pub use_lair: u8,
    pub intro_cue: u16,
    pub intro_198e: u16,
    pub opponent_x: u16,
    pub word_1126: u16,
    pub text_tick: u8,
    pub unported_extra: Option<u8>,
    /// `hblank_buffer_1`'s first word: the game over wave's phase.
    pub hblank_count: u16,
    /// Something reached that isn't ported (checked after the frame's actors).
    pub unported_note: Option<&'static str>,
    /// `dma_queue`: transfers the next vertical blank makes (before its plane commands).
    pub dma_queue: Vec<VdpOp>,
    /// `difficulty` (the option: 0-3) and `byte_FF0114` (stages the time bonus allows for).
    pub difficulty_option: u8,
    pub bonus_level: u8,
    /// Results: `bytecode_flag` (the winner), `dword_FF195C` (score with the bonus),
    /// `dword_FF1960` (beans popped), `current_password`.
    pub bytecode_flag: u8,
    pub result_score: u32,
    pub result_beans: u16,
    pub password: u16,
    /// Plane command $9E (`SpecPlane9E`) queued for the next vertical blank.
    pub password_command: bool,
    /// The match is over and the game flow has taken over (`bytecode_disabled` cleared).
    pub match_over: bool,
    /// `high_scores` (a table of 5 x 16 bytes per mode) and `high_score_table_id`.
    pub high_scores: Vec<u8>,
    pub high_score_table: u8,
    /// `byte_FF1973`: show the next mode's table too.
    pub high_score_both: bool,
    /// `sound_test_enabled` ($FFFC02): set by beating the game.
    pub sound_test: u16,
    /// `dword_FF112C`: the clouds' slot.
    pub clouds_slot: usize,
    /// The slot of the actor that took a long step (decompression) this frame, for
    /// the tests: the original may show it halfway through.
    pub long_actor: Option<usize>,
    /// The epilogue: `word_FF1134` (stops the fireworks), `dword_FF1130` (the bursts'
    /// wait mask, their color), `byte_FF143E` (line slots in use) and the lines' sprite
    /// mappings (`byte_FF1446`, mapping list 7's frames).
    pub fireworks_stop: u16,
    pub fireworks_mask: u16,
    pub fireworks_color: u16,
    pub line_slots: [u8; 8],
    pub line_maps: Vec<Vec<super::assets::sprites::Piece>>,
    /// `swap_controls` (pad 2 plays player 1), `byte_FF196A`, `hblank_count` (raster
    /// interrupts: none of the ported screens use them).
    pub swap_controls: u8,
    pub byte_196a: u8,
    pub hblank_lines: u16,
    /// `byte_FF0105` (the main menu's last mode) and the options (`$FFFCA4..`).
    pub last_mode: u8,
    pub options: [u8; 9],
    /// `plane_a_buffer` ($700 words) and `use_plane_a_buffer`: while set, the vertical
    /// blank copies the buffer to plane A ($C000).
    pub plane_a_buffer: Vec<u16>,
    pub use_plane_a_buffer: bool,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            slots: vec![None; SLOTS],
            fields: [Field::default(), Field::default()],
            popping: [false; 2],
            level_mode: 0,
            difficulty: 0,
            fall_commands: vec![],
            orphan_fall: None,
            rng: Rng::new(0),
            rng_calls: 0,
            glow_xy: (0, 0),
            unported_spawns: 0,
            chaining: [false; 2],
            icon_gen: [0; 2],
            external_pending: [0; 2],
            icons_drawn: [false; 2],
            foreign_random: vec![],
            rng_users: vec![],
            current_slot: usize::MAX,
            ai_fields: [AiField::default(), AiField::default()],
            cpu_memory: [[0; 10]; 2],
            cpu_reads: [0; 2],
            cpu_pressed: 0,
            cpu_lean: 0,
            crowding: 0,
            opponent: 0,
            level: 0,
            time_minutes: 0,
            control_player_1: 0xFF,
            frame_count: 0,
            time_frames: 0,
            player_flags: [0; 2],
            bean_orders: [[0; 256]; 2],
            referee_hit: 0,
            danger_flash: 0,
            portrait_lock: 0,
            pal_fade_gen: [0; 4],
            palettes: [[0; 16]; 4],
            palette_loads: [None; 4],
            pal_fades: [FadeData::default(); 4],
            field_bytes: [FieldBytes::default(), FieldBytes::default()],
            plane_writes: vec![],
            panel_commands: vec![],
            vscroll: [0; 40],
            eni_buffer: vec![0; 0x800],
            eni_queue: vec![],
            bytecode_pc: 0,
            bytecode_disabled: true,
            deferred_vblanks: 0,
            deferred_tail: 0,
            vdp_events: vec![],
            vdp_regs: [0; 0x13],
            hscroll: [0; 0x200],
            repeat_times: (8, 2),
            input_lock: 0,
            level_transition: 0,
            last_winner: 0,
            opponents_defeated: [0; 18],
            use_lair: 0,
            intro_cue: 0,
            intro_198e: 0,
            opponent_x: 0,
            word_1126: 0,
            text_tick: 0,
            hblank_count: 0,
            unported_note: None,
            high_score_both: false,
            sound_test: 0,
            clouds_slot: 0,
            long_actor: None,
            fireworks_stop: 0,
            fireworks_mask: 0,
            fireworks_color: 0,
            line_slots: [0; 8],
            line_maps: vec![vec![]; 8],
            swap_controls: 0,
            byte_196a: 0,
            hblank_lines: 0,
            last_mode: 0,
            options: DEFAULT_OPTIONS,
            plane_a_buffer: vec![0; 0x700],
            use_plane_a_buffer: false,
            unported_extra: None,
            dma_queue: vec![],
            difficulty_option: 0,
            bonus_level: 0,
            bytecode_flag: 0,
            result_score: 0,
            result_beans: 0,
            password: 0,
            password_command: false,
            match_over: false,
            high_scores: vec![0; 0x140],
            high_score_table: 0,
        }
    }

    /// `sub_12646`: the CPU plays this player.
    pub fn is_cpu(&self, id: u8) -> bool {
        ai::is_cpu(self.level_mode, id, self.control_player_1)
    }

    /// Count the draws code outside `random()` made from `before` on (at most a few).
    fn note_draws(&mut self, before: Rng) {
        let mut r = before;
        for _ in 0..64 {
            if r == self.rng {
                return;
            }
            r.next();
            self.rng_calls += 1;
            if self.current_slot != usize::MAX && self.rng_users.last() != Some(&self.current_slot) {
                self.rng_users.push(self.current_slot);
            }
        }
        panic!("lost track of random draws");
    }

    fn random(&mut self) -> u16 {
        self.rng_calls += 1;
        if self.current_slot != usize::MAX && self.rng_users.last() != Some(&self.current_slot) {
            self.rng_users.push(self.current_slot);
        }
        self.rng.next()
    }

    /// `SpecPlane89`: one step of every bean falling after a pop. The flag stays set
    /// while any bean was still moving at the start of the step.
    fn run_fall_command(&mut self, player_slot: usize) {
        if !matches!(self.slots[player_slot], Some(Actor::Player(_))) {
            // The player was deleted after queueing it: the step still runs on its field.
            if let Some((_, id, mut work)) = self.orphan_fall.take().filter(|o| o.0 == player_slot) {
                let (_, steps) = fall_steps(&mut work);
                for (b, color) in steps {
                    self.draw_fall_step(usize::from(id & 1), &b, color);
                }
            }
            return;
        }
        let p = self.player_mut(player_slot);
        p.flags &= !FALLING_AFTER_POP;
        let id = p.id as usize & 1;
        let Some(work) = p.pop.as_mut() else { return };
        let (any, steps) = fall_steps(work);
        if any {
            p.flags |= FALLING_AFTER_POP;
        }
        for (b, color) in steps {
            self.draw_fall_step(id, &b, color);
        }
    }

    /// `FindActorSlot`: the first free slot.
    fn free_slot(&self) -> Option<usize> {
        self.slots.iter().position(|s| s.is_none())
    }

    /// `FindActorSlot`: a free slot, or, with the table full, the first one held by a
    /// `FindActorSlotQuick` actor (which is wiped and replaced).
    fn find_actor_slot(&self) -> Option<usize> {
        self.free_slot().or_else(|| self.slots.iter().position(|s| s.as_ref().is_some_and(Actor::evictable)))
    }

    fn player_mut(&mut self, slot: usize) -> &mut Player {
        match self.slots[slot].as_mut() {
            Some(Actor::Player(p)) => p,
            other => panic!("slot {slot} is not a player: {other:?}"),
        }
    }

    fn pivot_mut(&mut self, slot: usize) -> &mut Pivot {
        match self.slots[slot].as_mut() {
            Some(Actor::Pivot(p)) => p,
            other => panic!("slot {slot} is not a pivot: {other:?}"),
        }
    }

    /// One frame: every actor in slot order (`RunActors`).
    pub fn frame(&mut self, inputs: &Inputs) -> Result<(), Unported> {
        self.vblank();
        self.logic(inputs)
    }

    /// A frame's logic after its vertical blank: the game flow, then the actors.
    pub fn logic(&mut self, inputs: &Inputs) -> Result<(), Unported> {
        self.run_bytecode(inputs)?;
        self.deferred_vblanks += std::mem::take(&mut self.deferred_tail);
        self.long_step();
        self.long_actor = None;
        self.run_slots(0..SLOTS, inputs)?;
        if let Some(what) = self.unported_note.take() {
            return Err(Unported(what));
        }
        match self.unported_extra.take() {
            Some(_) => Err(Unported("this opponent's intro extra (off_A15E)")),
            None => Ok(()),
        }
    }

    /// A long step of the logic (decompression): the vertical blanks it outlasts.
    pub fn long_step(&mut self) {
        if self.current_slot != usize::MAX && self.long_actor.is_none() {
            self.long_actor = Some(self.current_slot);
        }
        for _ in 0..std::mem::take(&mut self.deferred_vblanks) {
            self.vblank();
        }
    }

    /// The vertical blank before a frame's logic: queued plane commands, then one
    /// `Random`. (When the logic overruns a video frame, the ROM's vertical blank lands
    /// in the middle of `RunActors`; see `run_slots`.)
    pub fn vblank(&mut self) {
        self.vblank_part(true);
    }

    /// A vertical blank whose `Random` (its last step) hasn't run yet: the original's
    /// handler can still be busy when the next frame's snapshot is taken (the tests).
    pub fn vblank_before_random(&mut self) {
        self.vblank_part(false);
    }

    /// `Random`, for a vertical blank that finishes late (the tests).
    pub fn random_draw(&mut self) {
        self.random();
    }

    fn vblank_part(&mut self, with_random: bool) {
        self.frame_count = self.frame_count.wrapping_add(1);
        self.rng_calls = 0;
        self.icons_drawn = [false; 2];
        self.rng_users.clear();
        self.current_slot = usize::MAX;
        // TransferDMA (before the plane commands)
        for what in std::mem::take(&mut self.dma_queue) {
            self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what });
        }
        for slot in std::mem::take(&mut self.fall_commands) {
            self.run_fall_command(slot);
        }
        if std::mem::take(&mut self.password_command) {
            self.run_password_command();
        }
        self.run_panel_commands();
        self.copy_palettes();
        if with_random {
            self.random();
        }
        // TransferPlaneABuffer
        if self.use_plane_a_buffer {
            let words = self.plane_a_buffer.clone();
            self.vdp_events.push(VdpEvent { at: self.plane_writes.len(), what: VdpOp::Words { at: 0xC000, words } });
        }
        // HandleTime
        if (self.player_flags[0] | self.player_flags[1]) & 0x80 == 0 {
            self.time_frames = self.time_frames.saturating_add(1);
        }
        self.time_minutes = self.time_frames / 3600;
    }

    /// `RunActors` over some of the slots (all of them in a normal frame).
    pub fn run_slots(&mut self, slots: std::ops::Range<usize>, inputs: &Inputs) -> Result<(), Unported> {
        let foreign = std::mem::take(&mut self.foreign_random);
        for slot in slots {
            for &(_, n) in foreign.iter().filter(|(s, _)| *s == slot) {
                (0..n).for_each(|_| {
                    self.rng.next();
                });
            }
            self.current_slot = slot;
            // Leave a placeholder so the running actor's slot still counts as used.
            let Some(actor) = self.slots[slot].replace(Actor::Opaque) else {
                self.slots[slot] = None;
                continue;
            };
            let next = match actor {
                Actor::Opaque => Some(Actor::Opaque),
                Actor::OpaqueQuick => Some(Actor::OpaqueQuick),
                Actor::Vanishing => None,
                Actor::Player(p) => self.run_player(slot, p, inputs)?.map(Actor::Player),
                Actor::Pivot(p) => self.run_pivot(slot, p, inputs)?,
                Actor::Satellite(s) => self.run_satellite(slot, s)?,
                Actor::Effect(e) => self.run_effect(e).map(Actor::Effect),
                Actor::Burst(b) => self.run_burst(b).map(Actor::Burst),
                Actor::Piece(p) => run_piece(p).map(Actor::Piece),
                Actor::GarbageClear(mut g) => (!g.anim.step()).then_some(Actor::GarbageClear(g)),
                Actor::Glow(g) => self.run_glow(g).map(Actor::Glow),
                Actor::GarbageDrop(d) => self.run_garbage_drop(d).map(Actor::GarbageDrop),
                Actor::GarbageColumn(c) => self.run_garbage_column(c).map(Actor::GarbageColumn),
                Actor::Icon(i) => self.run_icon(i).map(Actor::Icon),
                Actor::PairHolder(h) => Some(Actor::PairHolder(h)),
                Actor::PreviewBean(v) => self.run_preview_bean(v).map(Actor::PreviewBean),
                Actor::Referee(r) => self.run_referee(r, inputs)?.map(Actor::Referee),
                Actor::Sparkle(sp) => self.run_sparkle(sp).map(Actor::Sparkle),
                Actor::Animator(a) => self.run_animator(a).map(Actor::Animator),
                Actor::Danger(d) => self.run_danger(d).map(Actor::Danger),
                Actor::DangerSpark(sp) => self.run_danger_spark(sp).map(Actor::DangerSpark),
                Actor::Portrait(pt) => self.run_portrait(pt).map(Actor::Portrait),
                Actor::PortraitPalette(pt) => self.run_portrait_palette(pt).map(Actor::PortraitPalette),
                Actor::CoconutsLight(c) => self.run_coconuts_light(c).map(Actor::CoconutsLight),
                Actor::FfuzzyEyes(e) => self.run_ffuzzy_eyes(e).map(Actor::FfuzzyEyes),
                Actor::RobotnikVoice(v) => self.run_voice(v).map(Actor::RobotnikVoice),
                Actor::FlowWait(w) => self.run_flow_wait(w).map(Actor::FlowWait),
                Actor::OpponentScreen(s) => self.run_opponent_screen(s, inputs).map(Actor::OpponentScreen),
                Actor::OpponentPortraits(p) => self.run_opponent_portraits(p).map(Actor::OpponentPortraits),
                Actor::IntroScript(s) => self.run_intro_script(slot, s, inputs).map(Actor::IntroScript),
                Actor::IntroBoxPart(p) => self.run_intro_box_part(p).map(Actor::IntroBoxPart),
                Actor::OpponentIntro(o) => self.run_opponent_intro(slot, o).map(Actor::OpponentIntro),
                Actor::IntroExtra(e) => self.run_intro_extra(e).map(Actor::IntroExtra),
                Actor::IntroClouds(c) => self.run_intro_clouds(c).map(Actor::IntroClouds),
                Actor::IntroShip(mut s) => {
                    s.anim.step_with(&mut || self.random());
                    Some(Actor::IntroShip(s))
                }
                Actor::Transition(t) => self.run_transition(t).map(Actor::Transition),
                Actor::RobotnikIntro(r) => self.run_robotnik_intro(slot, r).map(Actor::RobotnikIntro),
                Actor::LairSprite(s) => self.run_lair_sprite(s).map(Actor::LairSprite),
                Actor::LairFlier(f) => self.run_lair_flier(f).map(Actor::LairFlier),
                Actor::LairLights(l) => self.run_lair_lights(l).map(Actor::LairLights),
                Actor::ThrownPiece(p) => self.run_thrown_piece(p).map(Actor::ThrownPiece),
                Actor::GameOverTimer(t) => self.run_game_over_timer(t, inputs).map(Actor::GameOverTimer),
                Actor::GameOverLetter(l) => self.run_game_over_letter(l).map(Actor::GameOverLetter),
                Actor::CutsceneRobotnik(r) => self.run_cutscene_robotnik(r).map(Actor::CutsceneRobotnik),
                Actor::HighScoreTable(t) => self.run_high_score_table(slot, t, inputs).map(Actor::HighScoreTable),
                Actor::TableNameEntry(n) => self.run_table_name_entry(slot, n, inputs).map(Actor::TableNameEntry),
                Actor::TableCursor(c) => self.run_table_cursor(c).map(Actor::TableCursor),
                Actor::SegaLogo(l) => self.run_sega_logo(l).map(Actor::SegaLogo),
                Actor::CtrlSkip(c) => self.run_ctrl_skip(c, inputs).map(Actor::CtrlSkip),
                Actor::TitleHandler => self.run_title_handler(inputs).then_some(Actor::TitleHandler),
                Actor::TitleFade(t) => self.run_title_fade(t).map(Actor::TitleFade),
                Actor::TitleRobotnik(r) => self.run_title_robotnik(r).map(Actor::TitleRobotnik),
                Actor::MachineText(m) => self.run_machine_text(slot, m).map(Actor::MachineText),
                Actor::TitlePiece(p) => self.run_title_piece(p).map(Actor::TitlePiece),
                Actor::TitleParticle(p) => self.run_title_particle(p).map(Actor::TitleParticle),
                Actor::TitleBean(b) => self.run_title_bean(b, inputs).map(Actor::TitleBean),
                Actor::RobotnikText { .. } => Some(Actor::RobotnikText { y: 0x90u16.wrapping_add(self.vscroll[0]) }),
                Actor::Trademark => Some(Actor::Trademark),
                Actor::MainMenu(m) => self.run_main_menu(m, inputs).map(Actor::MainMenu),
                Actor::MenuClouds(c) => self.run_menu_clouds(c).map(Actor::MenuClouds),
                Actor::Options(o) => self.run_options(o, inputs).map(Actor::Options),
                Actor::PasswordEntry(p) => self.run_password_entry(p, inputs).map(Actor::PasswordEntry),
                Actor::PasswordSprite(s) => self.run_password_sprite(s).map(Actor::PasswordSprite),
                Actor::PasswordColors(c) => self.run_password_colors(c).map(Actor::PasswordColors),
                Actor::DefeatSounds(s) => self.run_defeat_sounds(s).map(Actor::DefeatSounds),
                Actor::DefeatShip(s) => self.run_defeat_ship(s).map(Actor::DefeatShip),
                Actor::DefeatPiece(p) => self.run_defeat_piece(p).map(Actor::DefeatPiece),
                Actor::DefeatPart(p) => self.run_defeat_part(p).map(Actor::DefeatPart),
                Actor::Skip => self.run_skip(inputs).then_some(Actor::Skip),
                Actor::SparkleSpawner(s) => self.run_sparkle_spawner(s).map(Actor::SparkleSpawner),
                Actor::ShipSparkle(s) => self.run_ship_sparkle(s).map(Actor::ShipSparkle),
                Actor::RoleCall(r) => self.run_role_call(slot, r).map(Actor::RoleCall),
                Actor::Walker(w) => self.run_walker(w).map(Actor::Walker),
                Actor::BackdropProp => self.run_backdrop_prop().then_some(Actor::BackdropProp),
                Actor::Typist(t) => self.run_typist(t).map(Actor::Typist),
                Actor::LairReveal(l) => self.run_lair_reveal(l).map(Actor::LairReveal),
                Actor::HasBean(b) => self.run_has_bean(b).map(Actor::HasBean),
                Actor::HasBeanShadow(s) => self.run_has_bean_shadow(s).map(Actor::HasBeanShadow),
                Actor::FireworkColors(c) => self.run_firework_colors(c).map(Actor::FireworkColors),
                Actor::FireworkSpawner(s) => self.run_firework_spawner(s).map(Actor::FireworkSpawner),
                Actor::FireworkSpark(s) => self.run_firework_spark(s).map(Actor::FireworkSpark),
                Actor::Dawn(d) => self.run_dawn(d).map(Actor::Dawn),
                Actor::Epilogue(e) => self.run_epilogue(slot, e).map(Actor::Epilogue),
                Actor::CreditLine(l) => self.run_credit_line(l).map(Actor::CreditLine),
                Actor::FfuzzyPalette(p) => self.run_ffuzzy_palette(p).map(Actor::FfuzzyPalette),
                Actor::PaletteFade(pf) => self.run_palette_fade(pf).map(Actor::PaletteFade),
                Actor::Star(st) => self.run_star(st).map(Actor::Star),
                Actor::StarSpark(sp) => self.run_star_spark(sp).map(Actor::StarSpark),
                Actor::Banner(b) => self.run_banner(b).map(Actor::Banner),
                Actor::FieldWipe(w) => self.run_field_wipe(w).map(Actor::FieldWipe),
                Actor::FallingColumn(c) => self.run_falling_column(c)?.map(Actor::FallingColumn),
                Actor::PasswordBean(b) => self.run_password_bean(b).map(Actor::PasswordBean),
                Actor::ContinueSign(c) => self.run_continue_sign(c).map(Actor::ContinueSign),
                Actor::BouncingBean(b) => self.run_bouncing_bean(b).map(Actor::BouncingBean),
                Actor::BounceSparkle(sp) => self.run_bounce_sparkle(sp).map(Actor::BounceSparkle),
                Actor::BgSlide(b) => self.run_bg_slide(b).map(Actor::BgSlide),
                Actor::BgRunner(r) => self.run_bg_runner(r).map(Actor::BgRunner),
                Actor::BgSpark(sp) => self.run_bg_spark(sp).map(Actor::BgSpark),
                Actor::ShakeField(sh) => self.run_shake_field(sh).map(Actor::ShakeField),
                Actor::NameEntry(n) => self.run_name_entry(n, inputs).map(Actor::NameEntry),
            };
            self.slots[slot] = next;
        }
        self.current_slot = usize::MAX;
        Ok(())
    }

    fn run_player(&mut self, slot: usize, mut p: Player, inputs: &Inputs) -> Result<Option<Player>, Unported> {
        use PlayerState::*;
        match p.state {
            WaitPair => {
                p.w26 = (p.w26 & 0x00FF) | (u16::from(((p.w26 >> 8) as u8).wrapping_add(1)) << 8);
                if p.flags & (flag::PIVOT_ACTIVE | flag::SATELLITE_ACTIVE) == 0 {
                    p.state = FindPops; // loc_3E96 bookmarks straight away
                } else if p.flags & flag::SOFT_DROP_POINT != 0 {
                    p.flags &= !flag::SOFT_DROP_POINT;
                    add_points(&mut p, 1); // sub_9C4A
                    self.queue_panel(PanelCommand::Score(p.id));
                }
            }
            FindPops => {
                // sub_5960 also leaves links and group marks, which the CPU reads later.
                self.ai_fields[p.id as usize].render(&self.fields[p.id as usize], true);
                self.relink_bytes(p.id as usize & 1);
                p.w26 = self.fields[p.id as usize].find_pops().len() as u16;
                p.state = Redraw;
            }
            Redraw => {
                self.redraw_field(p.id as usize & 1); // sub_5782
                p.state = Decide;
            }
            Decide => {
                if p.w26 != 0 {
                    // loc_3EDA: the popping beans and the garbage they clear (sub_58C8).
                    let field = &self.fields[p.id as usize];
                    let groups = field.find_pops();
                    let mut cells: Vec<(usize, usize)> = groups.iter().flat_map(|g| g.cells.iter().copied()).collect();
                    for (c, r) in cells.clone() {
                        for (dc, dr) in DIR {
                            let (nc, nr) = (c as i16 + dc, r as i16 + dr);
                            let cell = (nc as usize, nr as usize);
                            if nr >= HIDDEN_ROWS as i16 && field.get(nc, nr) == Some(BeanColor::Garbage) && !cells.contains(&cell) {
                                cells.push(cell);
                            }
                        }
                    }
                    let backup_bytes = self.field_bytes[p.id as usize & 1].cells;
                    p.pop = Some(PopWork { groups, cells, backup: field.clone(), backup_bytes, falling: vec![] });
                    self.chaining[p.id as usize & 1] = true;
                    self.cpu_memory[p.id as usize & 1][1] = 0; // sub_11ECE
                    p.state = PopScore;
                } else {
                    p.delay = SETTLE_DELAY[p.speed_level.min(4) as usize];
                    p.state = SettleDelay;
                }
            }
            PopScore => {
                self.score_step(&mut p);
                self.queue_panel(PanelCommand::Chain(p.id)); // sub_9A40
                p.state = PopBackup;
            }
            PopBackup => {
                let backup = self.fields[p.id as usize].clone();
                let backup_bytes = self.field_bytes[p.id as usize & 1].cells;
                let work = p.pop.as_mut().unwrap();
                work.backup = backup;
                work.backup_bytes = backup_bytes;
                p.delay = 0x18;
                p.state = Blink;
            }
            Blink => {
                // sub_49D2: shown on even delay values, hidden on odd ones.
                let work = p.pop.as_ref().unwrap();
                let field = &mut self.fields[p.id as usize];
                for &(c, r) in &work.cells {
                    let shown = if p.delay & 1 == 0 { work.backup.get(c as i16, r as i16) } else { None };
                    field.set(c, r, shown);
                }
                let (cells, backup_bytes) = (work.cells.clone(), work.backup_bytes);
                self.blink_bytes(p.id as usize & 1, &cells, &backup_bytes, p.delay & 1 == 0);
                if wait(&mut p.delay) {
                    p.state = PopSpawn;
                }
            }
            PopSpawn => {
                let points = if p.bonus == 0 { u32::from(p.base) } else { u32::from(p.base) * u32::from(p.bonus) };
                add_points(&mut p, points);
                self.spawn_pops(&p);
                if self.level_mode & 2 == 0 {
                    self.spawn_glow(slot, &p);
                }
                p.delay = 0x18;
                p.state = PopWait;
            }
            PopWait => {
                if wait(&mut p.delay) {
                    p.state = Compact;
                }
            }
            Compact => {
                let id = p.id as usize;
                let work = p.pop.as_mut().unwrap();
                work.falling = close_gaps(&mut self.fields[id], &work.backup);
                self.compact_bytes(id & 1);
                p.flags |= FALLING_AFTER_POP;
                p.state = FallWait;
            }
            FallWait => {
                if p.flags & FALLING_AFTER_POP != 0 {
                    self.fall_commands.push(slot); // loc_4E14
                } else {
                    // loc_3F50: the score again in place of the chain's points.
                    self.queue_panel(PanelCommand::Score(p.id));
                    self.speed_up(&mut p)?;
                    p.state = ChainNext;
                }
            }
            ChainNext => {
                p.chain = p.chain.saturating_add(1);
                p.pop = None;
                p.state = FindPops; // loc_3E96 bookmarks straight away
            }
            SettleDelay => {
                if wait(&mut p.delay) {
                    p.state = AfterDelay;
                }
            }
            Setup => self.set_up_player(&mut p)?,
            WaitReady => self.player_ready(&mut p)?,
            WaitStart => {
                if p.flags & 2 == 0 {
                    // loc_3DAC: sub_3CFA
                    if self.level_mode & 4 == 0 {
                        self.player_flags[p.id as usize & 1] = 0x7F;
                    }
                    self.drop_garbage_and_wait(slot, &mut p);
                }
            }
            AfterDelay => {
                if p.chain != 0 && self.level_mode & 2 == 0 {
                    self.send_garbage(slot, &mut p);
                }
                self.drop_garbage_and_wait(slot, &mut p);
            }
            WaitGarbage => {
                if p.flags & flag::GARBAGE_FALLING == 0 {
                    // loc_3DDE: sub_12BAA
                    let id = p.id as usize;
                    self.ai_fields[id].render(&self.fields[id], false);
                    ai::analyze(&mut self.ai_fields[id]);
                    if self.level_mode & 3 == 0 && id == 1 {
                        self.crowding = ai::crowding(&self.ai_fields[1], &self.ai_fields[0]);
                    }
                    p.state = PreSpawn1;
                }
            }
            PreSpawn1 => {
                if self.is_cpu(p.id) {
                    ai::placement_table(&mut self.ai_fields[p.id as usize], false);
                }
                p.state = PreSpawn2;
            }
            PreSpawn2 => {
                if self.is_cpu(p.id) {
                    ai::placement_table(&mut self.ai_fields[p.id as usize], true);
                }
                p.state = PreSpawn3;
            }
            PreSpawn3 => {
                self.cpu_reads[p.id as usize & 1] = 0;
                if self.is_cpu(p.id) {
                    self.plan(&mut p, inputs)?;
                }
                p.state = Spawn;
            }
            Spawn => {
                if self.spawn_pair(slot, &mut p, inputs)? {
                    p.state = WaitPair;
                } else {
                    self.top_out(&mut p)?;
                }
            }
            TopOut => self.announce_result(&mut p)?,
            Wipe | Cleared | ResultsWait | ResultsShown | BonusWait | Bonus | TallyWait | Tally | NoBonusWait | NoBonus | FinalWait | FinalLeave | FinalDone
            | Continue | Leave | Done | ContinueWait | HighScore | ContinueCheck | ContinueDone => {
                if !self.run_results(&mut p, inputs)? {
                    return Ok(None);
                }
            }
        }
        Ok(Some(p))
    }

    /// `loc_4F3A`: the pair stops (hidden) when its player's pivot flag goes; from then
    /// on it just waits ($4F4E).
    fn hide_pair(&mut self, mut v: Pivot) -> Pivot {
        if v.state == PivotState::Controlled {
            v.state = PivotState::Hidden;
        }
        v
    }

    /// `loc_3DB0`: the chain (if any) is over; drop this player's pending garbage,
    /// show its icons, then wait for the garbage to land.
    fn drop_garbage_and_wait(&mut self, slot: usize, p: &mut Player) {
        p.chain = 0;
        self.chaining[p.id as usize & 1] = false;
        self.spawn_garbage(slot, p);
        if self.level_mode & 2 == 0 && !self.chaining[(p.id as usize & 1) ^ 1] {
            // sub_9814: this player's own incoming garbage.
            self.show_icons(p.id ^ 1, u32::from(p.pending_garbage));
        }
        p.state = PlayerState::WaitGarbage;
    }

    /// `sub_127BA`: where the CPU puts the pair about to spawn.
    fn plan(&mut self, p: &mut Player, inputs: &Inputs) -> Result<(), Unported> {
        let id = p.id as usize;
        let holder = p.holder_slot.and_then(|h| match &self.slots[h] {
            Some(Actor::PairHolder(h)) => Some((h.pairs[0], h.pairs[1])),
            _ => None,
        });
        let (pivot, sat) = match holder {
            Some(pair) => pair,
            None => inputs.next_pair[id].map(|(a, b)| (a as u8, b as u8)).ok_or(Unported("no next pair given"))?,
        };
        let ctx = ai::PlanContext {
            player_id: p.id,
            opponent: self.opponent,
            difficulty: self.difficulty,
            time_minutes: self.time_minutes,
            pair: (pivot, sat),
        };
        let mut plan = player_plan(p);
        let mut scratch = ai::PlanScratch::default();
        let before = self.rng;
        // Pattern play may rewrite the pairs in the holder and the bean order.
        let deal = match p.holder_slot.and_then(|h| self.slots[h].as_mut()) {
            Some(Actor::PairHolder(h)) => {
                let next = (h.w20 >> 8) as u8;
                Some(ai::Deal { pairs: &mut h.pairs, order: &mut self.bean_orders[id & 1], next })
            }
            _ => None,
        };
        let res = ai::plan(&self.ai_fields[id], &ctx, &mut self.cpu_memory[id], &mut plan, &mut scratch, &mut self.rng, deal);
        self.note_draws(before);
        res.map_err(Unported)?;
        set_player_plan(p, plan);
        Ok(())
    }

    /// `CheckPuyoPop`: a burst for every popping bean and a clear effect for every
    /// cleared garbage bean, in reading order; bursts start 0, 4, 8, 12 frames apart.
    fn spawn_pops(&mut self, p: &Player) {
        let id = p.id as usize;
        let work = p.pop.as_ref().unwrap();
        let mut stagger = 0u16;
        for row in HIDDEN_ROWS..ROWS {
            for col in 0..COLS {
                if !work.cells.contains(&(col, row)) {
                    continue;
                }
                let x = FIELD_X[id] + 8 + col as u16 * 16;
                let y = FIELD_TOP_Y + 8 + (row - HIDDEN_ROWS) as u16 * 16;
                let Some(slot) = self.free_slot() else { continue };
                let color = work.backup.get(col as i16, row as i16).map_or(0, |c| c as u8);
                if color == BeanColor::Garbage as u8 {
                    self.slots[slot] = Some(Actor::GarbageClear(GarbageClear { player: p.id, x, y, anim: Anim::new(script::GARBAGE_CLEAR) }));
                } else {
                    self.glow_xy = (x, y);
                    self.slots[slot] = Some(Actor::Burst(Burst {
                        player: p.id,
                        color,
                        x,
                        y,
                        delay: stagger,
                        anim: Anim { ptr: script::POP, time: 0, frame: 8 },
                        state: BurstState::Wait,
                    }));
                    stagger = (stagger + 4) & 0xF;
                }
            }
        }
    }

    /// `SpawnGarbageGlow`.
    fn spawn_glow(&mut self, player_slot: usize, p: &Player) {
        let Some(slot) = self.find_actor_slot() else { return };
        let (x, y) = self.glow_xy;
        self.slots[slot] = Some(Actor::Glow(Glow {
            player: p.id,
            player_slot,
            chain: p.chain,
            motion: Motion { x: u32::from(x) << 16, y: u32::from(y) << 16, ..Motion::default() },
            draw: 0,
            base_x: 0,
            counter: 0,
            delay: 0,
            anim: Anim::new(script::GLOW[p.id as usize & 1]),
            state: GlowState::Start,
        }));
    }

    fn run_burst(&mut self, mut b: Burst) -> Option<Burst> {
        match b.state {
            BurstState::Wait => {
                if wait(&mut b.delay) {
                    b.state = BurstState::Burst;
                }
            }
            BurstState::Burst => {
                // ActPuyoPop_PopNormal: four pieces, roughly up-left, up-right, ...
                for i in (0..4u8).rev() {
                    let Some(slot) = self.free_slot() else { continue };
                    let angle = (i << 4).wrapping_add(0x64).wrapping_add((self.random() & 7) as u8);
                    self.slots[slot] = Some(Actor::Piece(Piece {
                        player: b.player,
                        color: b.color,
                        motion: Motion {
                            x: u32::from(b.x) << 16,
                            y: u32::from(b.y) << 16,
                            vx: sin(angle, 0x400),
                            vy: cos(angle, 0x400),
                            ay: 0x4000,
                            ty: 0xFFFF,
                            ..Motion::default()
                        },
                        draw: 0,
                        delay: 0,
                        anim: Anim { ptr: script::POPPED_PIECE, time: 0, frame: 6 },
                        state: PieceState::Start,
                    }));
                }
                b.state = BurstState::Animate;
            }
            BurstState::Animate => {
                if b.anim.step() {
                    return None;
                }
            }
        }
        Some(b)
    }

    fn run_glow(&mut self, mut g: Glow) -> Option<Glow> {
        match g.state {
            GlowState::Start => {
                if g.chain == 0 {
                    self.glow_finish(&mut g);
                } else {
                    g.anim.step();
                    g.draw = 0x83;
                    g.state = GlowState::Hover;
                }
            }
            GlowState::Hover => {
                g.delay = 8;
                g.state = GlowState::HoverWait;
            }
            GlowState::HoverWait => {
                g.anim.step();
                if wait(&mut g.delay) {
                    g.state = GlowState::Launch;
                }
            }
            GlowState::Launch => {
                // Aim at the other player's side in 32 frames.
                g.counter = 0x1F;
                let target_x: u32 = if g.player == 0 { 0x180 << 16 } else { 0xC0 << 16 };
                g.motion.vx = ((target_x.wrapping_sub(g.motion.x) as i32) >> 5) as u32;
                g.motion.vy = ((0x88_0000u32.wrapping_sub(g.motion.y) as i32) >> 5) as u32;
                g.base_x = (g.motion.x >> 16) as u16;
                g.state = GlowState::Fly;
            }
            GlowState::Fly => {
                g.anim.step();
                g.motion.x = (u32::from(g.base_x) << 16) | (g.motion.x & 0xFFFF);
                g.motion.step(g.draw);
                g.base_x = (g.motion.x >> 16) as u16;
                let side: u8 = if g.player == 0 { 0x80 } else { 0 };
                let angle = side | ((g.counter as u8) << 2);
                let wobble = (sin(angle, 0x6000) >> 16) as u16;
                g.motion.x = (u32::from(((g.motion.x >> 16) as u16).wrapping_add(wobble)) << 16) | (g.motion.x & 0xFFFF);
                let done = g.counter == 0;
                g.counter = g.counter.wrapping_sub(1);
                if done {
                    g.anim.ptr = script::GLOW_END[g.player as usize & 1];
                    self.glow_finish(&mut g);
                }
            }
            GlowState::End => {
                if g.anim.step() {
                    return None;
                }
            }
        }
        Some(g)
    }

    /// `loc_2371C`: refresh the opponent's incoming-garbage icons (not ported: they
    /// are separate actors), then play out the animation.
    fn glow_finish(&mut self, g: &mut Glow) {
        // loc_984A: the opponent's icons, showing what this chain will send.
        let (acc, id) = match &self.slots[g.player_slot] {
            Some(Actor::Player(p)) => (p.acc, p.id),
            _ => (0, g.player),
        };
        let total = (acc & 0xFFFF) / GARBAGE_POINTS + u32::from(self.pending_of(id ^ 1));
        self.show_icons(id, total & 0xFFFF);
        g.state = GlowState::End;
    }

    fn pending_of(&self, id: u8) -> u16 {
        self.slots
            .iter()
            .find_map(|s| match s {
                Some(Actor::Player(p)) if p.id == id => Some(p.pending_garbage),
                _ => None,
            })
            .unwrap_or(self.external_pending[id as usize & 1])
    }

    /// `SpawnGarbage`: shuffle the columns, deal up to 30 pending beans into them, and
    /// start one falling stack per column plus the drop's controller.
    fn spawn_garbage(&mut self, player_slot: usize, p: &mut Player) {
        if p.pending_garbage == 0 {
            return;
        }
        let id = p.id as usize & 1;
        // SetGarbageOrder
        let mut order = [0u8, 1, 2, 3, 4, 5];
        for i in (0..COLS).rev() {
            let r = self.random();
            let j = ((u32::from(r) * 6) >> 16) as usize;
            order.swap(i, j);
        }
        // GetGarbageFreeSpace / SetGarbageCounts
        let room = self.fields[id].garbage_room();
        let mut counts = [0u8; COLS];
        let mut placed = 0u16;
        for i in 0..p.pending_garbage.min(super::field::MAX_GARBAGE_DROP) as usize {
            let c = order[i % COLS] as usize;
            if counts[c] < room[c] {
                counts[c] += 1;
                placed += 1;
            }
        }
        p.pending_garbage -= placed;
        let Some(drop_slot) = self.find_actor_slot() else { return };
        self.slots[drop_slot] = Some(Actor::GarbageDrop(GarbageDrop {
            player_slot,
            player: p.id,
            started: false,
            falling: 0,
            seen: 0,
            strength: 0x0100,
            shake: 0,
            phase: 0,
            voiced: false,
            frame: 0,
        }));
        p.flags |= flag::GARBAGE_FALLING;
        let speed_index = if self.level_mode == 0 && self.difficulty < 3 { self.difficulty as usize } else { 2 };
        let speed = u32::from(GARBAGE_START_SPEED[speed_index]) << 16;
        let y = FIELD_TOP_Y - 8;
        for col in (0..COLS).rev() {
            if counts[col] == 0 {
                continue;
            }
            let Some(slot) = self.find_actor_slot() else { continue };
            if let Some(Actor::GarbageDrop(d)) = self.slots[drop_slot].as_mut() {
                d.falling += 1;
                d.strength = d.strength.wrapping_add(0x100);
            }
            // sub_11E90: garbage landing in the columns the CPU was aiming at.
            let mem = &mut self.cpu_memory[id];
            if (mem[8]..=mem[9]).contains(&((col as u8) << 1)) {
                mem[1] = 0; // sub_11ECE
            }
            self.slots[slot] = Some(Actor::GarbageColumn(GarbageColumn {
                player: p.id,
                drop_slot,
                col: col as u16,
                row: 1,
                frame: counts[col] + 0x13,
                fall: Fall::with(u32::from(y) << 16, speed, GARBAGE_ACCEL[col]),
                landed: false,
                linger: 0,
            }));
        }
    }

    fn run_garbage_drop(&mut self, mut d: GarbageDrop) -> Option<GarbageDrop> {
        if !d.started {
            d.seen = d.falling;
            d.started = true;
            return Some(d);
        }
        if d.falling != d.seen {
            d.seen = d.falling;
            d.shake = d.strength;
            d.voiced = true;
        }
        // The field's 6 columns of plane A wobble (from +$2C: GetPuyoField's plane address).
        let first = usize::from(FIELD_ORIGIN[usize::from(d.player & 1)] & 0x7F) / 2;
        let mut angle = d.phase;
        for col in 0..6 {
            self.vscroll[(first + col * 2) % 40] = (sin(angle & 0x7F, d.shake) >> 16) as u16;
            angle = angle.wrapping_add(0x20);
        }
        d.phase = d.phase.wrapping_add(0x18);
        if d.shake != 0 {
            d.shake = d.shake.wrapping_sub(0x40);
            return Some(d);
        }
        if d.falling != 0 {
            return Some(d);
        }
        if let Some(Actor::Player(p)) = self.slots[d.player_slot].as_mut() {
            p.flags &= !flag::GARBAGE_FALLING;
        }
        None
    }

    fn run_garbage_column(&mut self, mut c: GarbageColumn) -> Option<GarbageColumn> {
        let id = c.player as usize & 1;
        if c.landed {
            if self.popping[id] {
                return None;
            }
            c.linger -= 1;
            return (c.linger != 0).then_some(c);
        }
        let mut row = c.row;
        let down = c.fall.step(&self.fields[id], c.col, &mut row);
        c.row = row;
        if !down {
            return Some(c);
        }
        // Write the stack bottom-up.
        for _ in 0..c.frame.saturating_sub(0x13) {
            self.mark(id, c.col, c.row, BeanColor::Garbage);
            c.row = c.row.wrapping_sub(1);
        }
        if let Some(Actor::GarbageDrop(d)) = self.slots[c.drop_slot].as_mut() {
            d.falling = d.falling.wrapping_sub(1);
            d.frame = c.frame;
        }
        // loc_502C
        if c.frame >= 0x15 {
            return None;
        }
        c.frame += 6;
        c.linger = 0x30;
        c.landed = true;
        Some(c)
    }

    /// `loc_9858`: redraw one side's incoming-garbage icons for `total` beans:
    /// up to six big icons (30 or 6 beans), then a small one for the rest.
    fn show_icons(&mut self, side: u8, total: u32) {
        if self.popping.iter().any(|&p| p) {
            return;
        }
        let side = side & 1;
        self.icons_drawn[side as usize] = true;
        self.icon_gen[side as usize] = self.icon_gen[side as usize].wrapping_add(1);
        let generation = self.icon_gen[side as usize];
        let (start_x, mut target_x, step): (u16, u16, u16) =
            if side == 0 { (0x180, 0x150, 0x10) } else { (0xC0, 0xE0, 0xFFF0) };
        let (mut big, small) = (total / 6, total % 6);
        let mut index = 0;
        let icon = |engine: &mut Engine, target: u16, frame: u8, nudge: i16| -> bool {
            let Some(slot) = engine.free_slot() else { return false };
            let vx = ((u32::from(target.wrapping_sub(start_x)) << 16) as i32 >> 4) as u32;
            let x = start_x.wrapping_add(nudge as u16);
            engine.slots[slot] = Some(Actor::Icon(Icon {
                side,
                generation,
                frame,
                motion: Motion { x: u32::from(x) << 16, y: 0x88 << 16, vx, ..Motion::default() },
                slide: 0x10,
            }));
            true
        };
        while big != 0 && index < 6 {
            // sub_98B4
            let frame = if big - 1 >= 4 { 6 } else { 5 };
            if icon(self, target_x, frame, 0) {
                big -= 1;
                if frame == 6 {
                    big -= 4;
                }
            }
            target_x = target_x.wrapping_add(step);
            index += 1;
        }
        if index < 6 && small != 0 {
            // sub_98F2
            let frame = (small - 1) as u8;
            let nudge = if start_x == 0xC0 { ICON_NUDGE[frame as usize] } else { 0 };
            icon(self, target_x, frame, nudge);
        }
    }

    fn run_icon(&mut self, mut i: Icon) -> Option<Icon> {
        if i.slide != 0 {
            i.motion.step(0xA2);
            i.slide -= 1;
        }
        (self.icon_gen[i.side as usize] == i.generation).then_some(i)
    }

    /// `sub_9A56`: base points and bonus for this chain step, and the bean counters.
    fn score_step(&self, p: &mut Player) {
        let groups = &p.pop.as_ref().unwrap().groups;
        let beans: usize = groups.iter().map(|g| g.cells.len()).sum();
        let mut colors: Vec<u8> = groups.iter().map(|g| g.color as u8).collect();
        colors.sort_unstable();
        colors.dedup();
        let bonus = CHAIN_BONUS[p.chain.min(8) as usize]
            + COLOR_BONUS[colors.len().min(6)]
            + groups.iter().map(|g| GROUP_BONUS[g.cells.len().min(11)]).sum::<u16>();
        let mut multiplier = 1u32;
        if self.level_mode & 2 != 0 {
            let level = if p.speed_level >= 0x62 { 0x63 } else { p.speed_level };
            multiplier = u32::from(level) + 1;
        }
        p.beans_total = (p.beans_total as usize + beans).min(9999) as u16;
        p.beans_count = (p.beans_count as usize + beans).min(0xFFFF) as u16;
        p.base = (10 * beans as u32 * multiplier).min(0xFFFF) as u16;
        p.bonus = bonus.min(999);
    }

    /// `sub_9CA2`: enough beans popped â†’ next speed step.
    fn speed_up(&mut self, p: &mut Player) -> Result<(), Unported> {
        if p.beans_count < p.threshold {
            return Ok(());
        }
        p.beans_count = 0;
        p.speed_index = p.speed_index.wrapping_add(1);
        if self.level_mode & 3 != 0 {
            return Err(Unported("speed-ups outside scenario mode (unk_9E24, unk_9E7C)"));
        }
        let step = p.speed_index.saturating_sub(self.difficulty) as usize;
        let &(speed, threshold) = SCENARIO_SPEEDS.get(step).ok_or(Unported("speed step past unk_9DC4"))?;
        p.fall_speed = speed;
        p.threshold = threshold;
        Ok(())
    }

    /// `sub_998A`: a finished chain sends acc / 70 garbage beans; the rest stays.
    fn send_garbage(&mut self, slot: usize, p: &mut Player) {
        let acc = p.acc & 0xFFFF;
        let sent = (acc / GARBAGE_POINTS) as u16;
        p.acc = acc % GARBAGE_POINTS;
        let id = p.id;
        let opponent = (0..SLOTS).find(|&s| s != slot && matches!(&self.slots[s], Some(Actor::Player(o)) if o.id != id));
        match opponent {
            Some(o) => {
                let o = self.player_mut(o);
                o.pending_garbage = o.pending_garbage.wrapping_add(sent);
            }
            None => p.garbage_sent = p.garbage_sent.wrapping_add(sent),
        }
    }

    /// `sub_4E24`: the next pair at column 2, row 2, satellite above.
    /// `sub_4E24`: deal the next pair and spawn it. Ok(false) when the spawn cell is
    /// taken (the pair was still dealt).
    fn spawn_pair(&mut self, player_slot: usize, p: &mut Player, inputs: &Inputs) -> Result<bool, Unported> {
        let id = p.id as usize;
        let holder = p.holder_slot.filter(|&h| matches!(self.slots[h], Some(Actor::PairHolder(_))));
        let (pivot_color, sat_color) = match holder {
            Some(h) => {
                let (a, b) = self.deal_pair(h, p)?;
                let color = |c: u8| BeanColor::from_id(c).ok_or(Unported("special pieces (sub_4E24)"));
                (color(a)?, color(b)?)
            }
            None => inputs.next_pair[id].ok_or(Unported("no next pair given"))?,
        };
        if self.fields[id].get(2, 2).is_some() {
            return Ok(false);
        }
        let (pivot_slot, sat_slot) = match inputs.spawn_slots[id] {
            Some(slots) => slots,
            None => {
                let a = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
                self.slots[a] = Some(Actor::Opaque);
                let b = self.find_actor_slot().ok_or(Unported("no free actor slot"))?;
                (a, b)
            }
        };
        let pair = Pair {
            col: 2,
            row: 2,
            pivot_anim: Anim::new(script::IDLE),
            sat_anim: Anim::new(script::NONE),
            ..Pair::default()
        };
        self.slots[pivot_slot] = Some(Actor::Pivot(Pivot {
            player_slot,
            player_id: p.id,
            satellite_slot: sat_slot,
            color: pivot_color,
            state: PivotState::Init,
            pair,
            flags: 0,
            y: 0,
            fall: Fall::default(),
        }));
        self.slots[sat_slot] = Some(Actor::Satellite(Satellite {
            pivot_slot,
            player_slot,
            player_id: p.id,
            color: sat_color,
            state: SatelliteState::Init,
            col: 0,
            row: 0,
            flags: 0,
            x: 0,
            y: 0,
            anim: Anim::new(script::NONE),
            fall: Fall::default(),
        }));
        p.flags |= flag::PIVOT_ACTIVE | flag::SATELLITE_ACTIVE;
        Ok(true)
    }

    fn run_pivot(&mut self, _slot: usize, mut v: Pivot, inputs: &Inputs) -> Result<Option<Actor>, Unported> {
        // The player can be gone (deleted when the other side won): a controlled pair
        // then sees its flags as 0 and stops; a landing one carries on.
        let player = match &self.slots[v.player_slot] {
            Some(Actor::Player(p)) => Some((p.flags, p.fall_speed, player_plan(p))),
            _ => None,
        };
        if player.is_none() && matches!(v.state, PivotState::Controlled | PivotState::Hidden) {
            return Ok(Some(Actor::Pivot(self.hide_pair(v))));
        }
        let (player_flags, fall_speed, mut plan) = player.unwrap_or_default();
        let id = v.player_id as usize & 1;
        let cpu = self.is_cpu(id as u8);
        match v.state {
            PivotState::Hidden => {}
            PivotState::Init => {
                v.place();
                v.state = PivotState::Controlled;
            }
            PivotState::Controlled => {
                if player_flags & flag::PIVOT_ACTIVE == 0 {
                    return Ok(Some(Actor::Pivot(self.hide_pair(v))));
                }
                let step = if cpu {
                    let before = self.rng;
                    let pad = ai::CpuPad {
                        human: inputs.pads[id],
                        plan: &mut plan,
                        player_id: id as u8,
                        level: self.level,
                        reads: &mut self.cpu_reads[id & 1],
                        pressed: &mut self.cpu_pressed,
                        lean: &mut self.cpu_lean,
                        rng: &mut self.rng,
                    };
                    let step = v.pair.step_pivot(&self.fields[id], pad, ai::CPU_KEYS, fall_speed, self.frame_count);
                    self.note_draws(before);
                    set_player_plan(self.player_mut(v.player_slot), plan);
                    step
                } else {
                    v.pair.step_pivot(&self.fields[id], inputs.pads[id], inputs.keys[id], fall_speed, self.frame_count)
                };
                let (step, events) = step;
                if events.touched_down {
                    self.land_effects(id as u8);
                }
                if events.soft_drop_point {
                    self.player_mut(v.player_slot).flags |= flag::SOFT_DROP_POINT;
                }
                v.place();
                if step == Step::Landed {
                    // loc_4F50: which beans are supported, and tell the satellite.
                    let resting = {
                        let field = &self.fields[id];
                        let below = |c: i16, r: i16| field.blocked(c, r + 1);
                        let (dc, dr) = DIR[(v.pair.rot & 3) as usize];
                        let (c, r) = (v.pair.col as i16, v.pair.row as i16);
                        let mut bits = u8::from(below(c, r)) | (u8::from(below(c + dc, r + dr)) << 1);
                        if bits != 0 && v.pair.rot & 1 == 0 {
                            bits |= 3;
                        }
                        bits
                    };
                    v.flags = resting | 4;
                    if resting & 1 != 0 {
                        v.pair.pivot_anim.time = 0;
                        v.state = PivotState::Squash;
                    } else {
                        v.fall = Fall::start(v.y);
                        v.pair.pivot_anim.frame = 0;
                        v.state = PivotState::Falling;
                    }
                }
            }
            PivotState::Falling => {
                let mut row = v.pair.row;
                if v.fall.step(&self.fields[id], v.pair.col, &mut row) {
                    v.pair.pivot_anim.ptr = script::SQUASH;
                    v.pair.pivot_anim.time = 0;
                    v.state = PivotState::Squash;
                }
                v.pair.row = row;
                v.y = v.fall.y;
            }
            PivotState::Squash => {
                if v.pair.pivot_anim.step() {
                    if let Some(Actor::Player(p)) = self.slots[v.player_slot].as_mut() {
                        p.flags &= !flag::PIVOT_ACTIVE;
                    }
                    self.mark(id, v.pair.col, v.pair.row, v.color);
                    let x = pivot_x(&v);
                    return Ok(Some(Actor::Effect(Effect::new(id as u8, v.y, v.color, v.pair.pivot_anim.frame, x))));
                }
            }
        }
        Ok(Some(Actor::Pivot(v)))
    }

    fn run_satellite(&mut self, _slot: usize, mut s: Satellite) -> Result<Option<Actor>, Unported> {
        match s.state {
            SatelliteState::Init => {
                self.follow(&mut s);
                s.state = SatelliteState::Follow;
            }
            SatelliteState::Follow => {
                let landed = {
                    let pivot = self.pivot_mut(s.pivot_slot);
                    pivot.pair.step_satellite();
                    pivot.flags & 4 != 0
                };
                self.follow(&mut s);
                if landed {
                    // loc_549E: take over position, flags and animation from the pivot.
                    let pivot = self.pivot_mut(s.pivot_slot).clone();
                    let (dc, dr) = DIR[(pivot.pair.rot & 3) as usize];
                    s.col = pivot.pair.col.wrapping_add(dc as u16);
                    s.row = pivot.pair.row.wrapping_add(dr as u16);
                    s.player_slot = pivot.player_slot;
                    s.flags = pivot.flags;
                    s.anim = pivot.pair.sat_anim;
                    s.state = SatelliteState::Check;
                }
            }
            SatelliteState::Check => {
                if s.flags & 2 != 0 {
                    s.state = SatelliteState::Squash;
                } else {
                    s.fall = Fall::start(s.y);
                    s.state = SatelliteState::Falling;
                }
            }
            SatelliteState::Falling => {
                let id = s.player_id as usize & 1;
                let mut row = s.row;
                if s.fall.step(&self.fields[id], s.col, &mut row) {
                    s.anim.ptr = script::SQUASH;
                    s.state = SatelliteState::Squash;
                }
                s.row = row;
                s.y = s.fall.y;
            }
            SatelliteState::Squash => {
                if s.anim.step() {
                    if let Some(Actor::Player(p)) = self.slots[s.player_slot].as_mut() {
                        p.flags &= !flag::SATELLITE_ACTIVE;
                    }
                    let id = s.player_id as usize & 1;
                    self.mark(id, s.col, s.row, s.color);
                    return Ok(Some(Actor::Effect(Effect::new(id as u8, s.y, s.color, s.anim.frame, s.x))));
                }
            }
        }
        Ok(Some(Actor::Satellite(s)))
    }

    /// `sub_5552` (y only): 16 px from the pivot on the satellite's side.
    /// `sub_5552`: the satellite circles the pivot (16 pixels out, at its angle).
    fn follow(&mut self, s: &mut Satellite) {
        let pivot = self.pivot_mut(s.pivot_slot);
        let angle = pivot.pair.sat_angle;
        // A pivot in a later slot that hasn't run yet still has x (and y) 0.
        let px = if pivot.state == PivotState::Init { 0 } else { pivot_x(pivot) };
        let py = (pivot.y >> 16) as u16;
        s.x = px.wrapping_add((sin(angle, 0x1000) >> 16) as u16);
        let y = py.wrapping_sub((cos(angle, 0x1000) >> 16) as u16);
        s.y = (u32::from(y) << 16) | (s.y & 0xFFFF);
    }

    /// `MarkPuyoSpot`.
    fn mark(&mut self, player: usize, col: u16, row: u16, color: BeanColor) {
        if !self.popping[player] {
            self.fields[player].set(col as usize, row as usize, Some(color));
            self.mark_bytes(player, col as usize, row as usize, color);
        }
    }

    fn run_effect(&mut self, mut e: Effect) -> Option<Effect> {
        let popping = self.popping[e.player as usize];
        match e.state {
            EffectState::Wait => {
                if popping {
                    return None;
                }
                if wait(&mut e.delay) {
                    e.state = EffectState::Start;
                }
            }
            EffectState::Start => {
                e.steps_left = 6;
                e.ticks = 0;
                e.state = EffectState::Rise;
            }
            EffectState::Rise => {
                if popping {
                    return None;
                }
                e.ticks += 1;
                if e.ticks >= 4 {
                    e.ticks = 0;
                    e.x = e.x.wrapping_sub(1);
                    e.y = e.y.wrapping_sub(0x1_0000);
                    e.steps_left -= 1;
                    if e.steps_left == 0 {
                        return None;
                    }
                }
            }
        }
        Some(e)
    }
}

impl Default for Engine {
    fn default() -> Self {
        Engine::new()
    }
}
