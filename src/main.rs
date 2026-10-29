//! Dr. Robotnik's Mean Bean Machine, recompiled: the game from power-on (Sega logo,
//! title, menus, scenario mode) on the ported engine, with everything read from your own
//! ROM.
//!
//! Usage: `bean-machine [path to the ROM]` (or set `MBM_ROM`). Without either, it uses
//! the ROM from the last run, or asks for it on a screen of its own
//! ([`bean_machine::locate`]) and remembers it from then on. Arrows move, Z/X/C are
//! A/B/C (turn), Enter is Start: player 1's pad (no keys play player 2's). Esc quits. The sound is the ROM's own driver, ported;
//! `MBM_MUTE=1` silences it. `MBM_CONSOLE=jp` / `eu` plays it on a Japanese / European
//! console (this US cartridge's region lockout).
//!
//! Not in the ROM: NETWORK on the main menu plays 1P vs 2P with someone on another
//! machine (see [`bean_machine::netplay`] and [`bean_machine::netmenu`]). One player
//! hosts, the other types the host's address and connects; both press START in the lobby
//! and the match starts. Our own buttons count straight away, as offline: the other
//! player's are guessed until they arrive (see [`bean_machine::rollback`]). From the command line:
//!   `bean-machine [ROM] --host [PORT]` hosts straight away (port 7845 unless given);
//!   `bean-machine [ROM] --join IP[:PORT]` joins straight away;
//!   `--delay FRAMES` sets the host's input delay (3 unless given).

use std::path::PathBuf;

use bean_machine::game::{Game, Status};
use bean_machine::locate::{self, Locate};
use bean_machine::netmenu::{Action, Input, NetMenu, Screen};
use bean_machine::netplay::{self, Session, State};
use bean_machine::rollback::{self, Rollback};
use bean_machine::sound::Cue;
use bevy::audio::AddAudioSource;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use bean_machine::{pad, render, rom};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::window::FileDragAndDrop;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SCALE: f32 = 3.0;
/// The random seed as the boot code leaves it (the game draws from it every frame, so
/// what follows depends on when buttons are pressed, as on the console).
const BOOT_SEED: u32 = 0x7FE7_B46A;
const TITLE: &str = "Dr. Robotnik's Mean Bean Machine";

#[derive(Resource)]
struct Front {
    /// The game on this machine (under the network menus, and again afterwards).
    game: Game,
    screen: Handle<Image>,
    /// Frames shown since the game reached something not ported yet.
    linger: u32,
    menu: NetMenu,
    menu_open: bool,
    /// The menus opened this frame: the key that opened them isn't for them.
    menu_fresh: bool,
    net: Option<Online>,
    /// Keys still held from the menus: the pads read nothing until they're let go.
    mute: bool,
    rom_hash: u64,
    port: u16,
    delay: u8,
    ticks: u32,
    audio: Audio,
    title: String,
}

/// A connection, and once it's up, the lobby and then the match.
struct Online {
    session: Session,
    play: Option<Rollback<rollback::Online>>,
}

impl Online {
    fn new(session: Session) -> Online {
        Online { session, play: None }
    }

    /// The lobby or match as it stands, while connected.
    fn shown(&self) -> Option<&rollback::Online> {
        self.play.as_ref().filter(|_| *self.session.state() == State::Playing).map(Rollback::state)
    }
}

/// What the command line asks for.
#[derive(Default)]
struct Args {
    rom: Option<PathBuf>,
    host: Option<u16>,
    join: Option<String>,
    delay: Option<u8>,
}

fn args() -> Result<Args, String> {
    let mut out = Args::default();
    let mut it = std::env::args().skip(1).peekable();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--host" => {
                let port = it.next_if(|p| !p.starts_with("--") && p.parse::<u16>().is_ok());
                out.host = Some(port.map_or(netplay::DEFAULT_PORT, |p| p.parse().unwrap()));
            }
            "--join" => out.join = Some(it.next().ok_or("--join needs an address (IP or IP:PORT)")?),
            "--delay" => {
                let d = it.next().and_then(|d| d.parse().ok()).filter(|&d| d <= netplay::MAX_DELAY);
                out.delay = Some(d.ok_or(format!("--delay needs a number of frames, 0 to {}", netplay::MAX_DELAY))?);
            }
            _ if a.starts_with("--") => return Err(format!("unknown option {a}")),
            _ => out.rom = Some(PathBuf::from(a)),
        }
    }
    if out.host.is_some() && out.join.is_some() {
        return Err("--host or --join, not both".into());
    }
    Ok(out)
}

/// Where to look for the ROM: the command line, `MBM_ROM`, the last run's, then (for
/// development) the research repository's.
fn rom_path(args: &Args) -> Option<PathBuf> {
    args.rom.clone().or_else(|| std::env::var_os("MBM_ROM").map(PathBuf::from)).or_else(locate::saved).or_else(|| {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bean-machine-re/rom_path.txt");
        std::fs::read_to_string(f).ok().map(|s| PathBuf::from(s.trim()))
    })
}

fn fail(e: impl std::fmt::Display) -> ! {
    eprintln!("{e}");
    std::process::exit(2);
}

/// Hosting or joining from the command line.
#[derive(Resource)]
struct Start(Option<Action>);

fn main() {
    let args = args().unwrap_or_else(|e| fail(e));
    // Without a ROM that works, the ROM screen comes up first.
    let path = rom_path(&args);
    let finding = match path.as_deref().map(|p| (p, rom::install_from(p))) {
        Some((p, Ok(()))) => {
            remember(p);
            None
        }
        Some((p, Err(e))) => Some(Finding::new(Locate::new(Some(p), Some(e)))),
        None => Some(Finding::new(Locate::new(None, None))),
    };
    let start = match (args.host, args.join) {
        (Some(_), _) => Some(Action::Host),
        (_, Some(addr)) => Some(Action::Join(addr)),
        _ => None,
    };
    let mut app = App::new();
    if let Some(finding) = finding {
        app.insert_resource(finding);
    }
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: TITLE.into(),
                resolution: ((render::WIDTH as f32 * SCALE) as u32, (render::HEIGHT as f32 * SCALE) as u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .insert_resource(Settings { port: args.host.unwrap_or(netplay::DEFAULT_PORT), delay: args.delay.unwrap_or(netplay::DEFAULT_DELAY) })
        .insert_resource(Start(start))
        .insert_resource(Glitch { log: std::env::var_os("MBM_GLITCH_LOG").is_some(), last: vec![], shots: 0 })
        .add_audio_source::<Speaker>()
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step.run_if(resource_exists::<Front>))
        .add_systems(Update, (ui, show).chain().run_if(resource_exists::<Front>))
        .add_systems(Update, find_rom.run_if(resource_exists::<Finding>))
        .run();
}

#[derive(Resource)]
struct Settings {
    port: u16,
    delay: u8,
}

fn setup(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    mut speakers: ResMut<Assets<Speaker>>,
    settings: Res<Settings>,
    mut start: ResMut<Start>,
    finding: Option<Res<Finding>>,
) {
    let buffer = Arc::new(Mutex::new(VecDeque::new()));
    let speaker = speakers.add(Speaker { buffer: buffer.clone(), mute: std::env::var_os("MBM_MUTE").is_some() });
    commands.spawn(AudioPlayer(speaker));
    let mut image = Image::new(
        Extent3d { width: render::WIDTH as u32, height: render::HEIGHT as u32, depth_or_array_layers: 1 },
        TextureDimension::D2,
        vec![0; render::WIDTH * render::HEIGHT * 4],
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::MAIN_WORLD | RenderAssetUsages::RENDER_WORLD,
    );
    image.sampler = ImageSampler::nearest();
    let screen = images.add(image);
    commands.spawn(Camera2d);
    commands.spawn(Sprite {
        image: screen.clone(),
        custom_size: Some(Vec2::new(render::WIDTH as f32 * SCALE, render::HEIGHT as f32 * SCALE)),
        ..default()
    });
    let output = Output { screen, buffer };
    if finding.is_none() {
        commands.insert_resource(begin(&output, &settings, &mut start));
    }
    commands.insert_resource(output);
}

/// The game's picture and the audio output's buffer: made at startup, before the ROM
/// may be in.
#[derive(Resource)]
struct Output {
    screen: Handle<Image>,
    buffer: Arc<Mutex<VecDeque<f32>>>,
}

/// The game, once the ROM is in (and hosting or joining, if the command line asked).
fn begin(output: &Output, settings: &Settings, start: &mut Start) -> Front {
    let mut front = Front {
        game: boot(BOOT_SEED).expect("starting the game"),
        screen: output.screen.clone(),
        linger: 0,
        menu: NetMenu::new(),
        menu_open: false,
        menu_fresh: false,
        net: None,
        mute: false,
        rom_hash: netplay::hash(rom::get().unwrap()),
        port: settings.port,
        delay: settings.delay,
        ticks: 0,
        audio: Audio::new(output.buffer.clone()),
        title: TITLE.into(),
    };
    if let Some(action) = start.0.take() {
        front.menu_open = true;
        if let Action::Join(addr) = &action {
            front.menu.address = addr.clone();
        }
        act(&mut front, action);
    }
    front
}

/// Saves the ROM's path for the next run (unless it's the one saved already).
fn remember(path: &std::path::Path) {
    if locate::saved().as_deref() != std::path::absolute(path).ok().as_deref()
        && let Err(e) = locate::save(path)
    {
        eprintln!("couldn't remember the ROM's path: {e}");
    }
}

/// The ROM screen while it's up: what it shows, and the file picker while that's open.
#[derive(Resource)]
struct Finding {
    screen: Locate,
    /// What the file picker (on a thread of its own) chose, once it closes.
    picker: Option<Mutex<std::sync::mpsc::Receiver<Option<PathBuf>>>>,
    ticks: u32,
}

impl Finding {
    fn new(screen: Locate) -> Finding {
        Finding { screen, picker: None, ticks: 0 }
    }
}

/// The ROM screen: keys, clicks, dropped files and the file picker, until a ROM works;
/// then the game starts (and the ROM's path is saved).
#[allow(clippy::too_many_arguments)]
fn find_rom(
    mut commands: Commands,
    typed: MessageReader<KeyboardInput>,
    mut drops: MessageReader<FileDragAndDrop>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut finding: ResMut<Finding>,
    output: Res<Output>,
    settings: Res<Settings>,
    mut start: ResMut<Start>,
    mut images: ResMut<Assets<Image>>,
    mut exit: MessageWriter<AppExit>,
) {
    let finding = &mut *finding;
    finding.ticks += 1;
    let mut actions = vec![];
    let picked = finding.picker.as_ref().map(|p| p.lock().unwrap().try_recv());
    if let Some(picked @ (Ok(_) | Err(std::sync::mpsc::TryRecvError::Disconnected))) = picked {
        let chosen = picked.ok().flatten();
        finding.picker = None;
        finding.screen.browsing = false;
        actions.extend(chosen.map(|p| finding.screen.dropped(&p)));
    }
    for drop in drops.read() {
        if let FileDragAndDrop::DroppedFile { path_buf, .. } = drop {
            actions.push(finding.screen.dropped(path_buf));
        }
    }
    for input in inputs(typed, &mouse, &windows) {
        actions.extend(finding.screen.input(input));
    }
    for action in actions {
        match action {
            locate::Action::Browse => {
                let (tx, rx) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let chosen = rfd::FileDialog::new()
                        .set_title("Your Mean Bean Machine ROM")
                        .add_filter("Mega Drive ROM", &["bin", "md", "gen"])
                        .add_filter("All files", &["*"])
                        .pick_file();
                    let _ = tx.send(chosen);
                });
                finding.picker = Some(Mutex::new(rx));
                finding.screen.browsing = true;
            }
            locate::Action::Load(path) => match rom::install_from(&path) {
                Ok(()) => {
                    remember(&path);
                    let mut front = begin(&output, &settings, &mut start);
                    // (the key that started it isn't for the game)
                    front.mute = true;
                    commands.insert_resource(front);
                    commands.remove_resource::<Finding>();
                    return;
                }
                Err(e) => finding.screen.error = Some(e),
            },
            locate::Action::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
    if let Some(mut image) = images.get_mut(&output.screen) {
        let mut picture = vec![0; render::WIDTH * render::HEIGHT * 4];
        finding.screen.draw(&mut picture, finding.ticks);
        image.data = Some(picture);
    }
}

/// The keyboard as player 1's pad (arrows, Z/X/C, Enter). Player 2's pad reads nothing.
const P1_KEYS: [(KeyCode, u8); 8] = [
    (KeyCode::ArrowUp, pad::UP),
    (KeyCode::ArrowDown, pad::DOWN),
    (KeyCode::ArrowLeft, pad::LEFT),
    (KeyCode::ArrowRight, pad::RIGHT),
    (KeyCode::KeyX, pad::B),
    (KeyCode::KeyC, pad::C),
    (KeyCode::KeyZ, pad::A),
    (KeyCode::Enter, pad::START),
];

fn buttons(keys: &ButtonInput<KeyCode>, map: &[(KeyCode, u8)]) -> u8 {
    map.iter().filter(|(k, _)| keys.pressed(*k)).fold(0, |acc, (_, b)| acc | b)
}

fn step(keys: Res<ButtonInput<KeyCode>>, mut front: ResMut<Front>, mut windows: Query<&mut Window>) {
    let front = &mut *front;
    front.ticks += 1;
    let mut pads = [buttons(&keys, &P1_KEYS), 0];
    if front.mute {
        front.mute = pads != [0, 0];
        pads = [0, 0];
    }
    if let Some(net) = front.net.as_mut() {
        net.session.receive();
        match net.session.state().clone() {
            State::Playing => {
                if front.menu_open {
                    // (just connected: the keys that got us here don't count in the lobby)
                    front.menu_open = false;
                    front.mute = true;
                    pads = [0, 0];
                }
                play_online(net, pads[0]);
                if let Some(play) = net.play.as_mut() {
                    front.audio.cues(&play.take_cues());
                }
                net.session.send();
                set_title(front, &mut windows);
                return;
            }
            State::Closed(why) => {
                front.net = None;
                front.menu_open = true;
                front.mute = true;
                front.menu.show(Screen::Notice(why));
            }
            State::Waiting | State::Connecting => net.session.send(),
        }
    }
    // This machine's game: played as usual, or running on its own under the menus.
    let local = if front.menu_open { [0, 0] } else { pads };
    play_frame(&mut front.game, &mut front.linger, BOOT_SEED, local);
    front.audio.frame(front.game.sounds());
    front.game.engine.network_line = true;
    if front.game.engine.network_chosen {
        front.game.engine.network_chosen = false;
        front.menu.show(Screen::Choose);
        front.menu_open = true;
        front.menu_fresh = true;
    }
    set_title(front, &mut windows);
}

/// The next frame over the network: the lobby until both are ready, then the match.
/// Once connected, the match's start (1P vs 2P from the host's seed) is made first.
fn play_online(net: &mut Online, mine: u8) {
    if net.play.is_none() {
        match Game::versus(net.session.seed()) {
            Ok(g) => net.play = Some(Rollback::new(rollback::Online::new(g))),
            Err(e) => return net.session.leave(&e),
        }
    }
    if let Some(play) = net.play.as_mut() {
        play.tick(&mut net.session, mine);
    }
}

/// Where the game's sounds go: the ported sound driver and chips
/// ([`bean_machine::audio`]), a frame of samples each step, into the buffer the audio
/// output plays from. With `MBM_SOUND_LOG` set the requests are printed too;
/// `MBM_MUTE` keeps it quiet.
struct Audio {
    log: bool,
    /// Frames played offline (to number their cues).
    frame: u32,
    chips: bean_machine::audio::Audio,
    out: Arc<Mutex<VecDeque<f32>>>,
}

/// The output's rate, and the rate the chips are rendered at: a frame of the console's
/// (59.92 Hz) in exactly a step's samples (60 Hz), the 0.13% the game runs fast.
const OUTPUT_HZ: u32 = 48_000;
const RENDER_HZ: u32 = (OUTPUT_HZ as f64 / 60.0 * (bean_machine::audio::MASTER_HZ / bean_machine::audio::FRAME_CLOCKS as f64)) as u32;
/// At most this many frames of samples wait to be played (more and the oldest go).
const BUFFERED_FRAMES: usize = 6;

impl Audio {
    fn new(out: Arc<Mutex<VecDeque<f32>>>) -> Audio {
        Audio { log: std::env::var_os("MBM_SOUND_LOG").is_some(), frame: 0, chips: bean_machine::audio::Audio::new(RENDER_HZ), out }
    }

    /// An offline frame's requests: all of them, once.
    fn frame(&mut self, sounds: &[bean_machine::sound::Sound]) {
        let frame = self.frame;
        let cues: Vec<Cue> = sounds.iter().map(|&sound| Cue { frame, sound, cancel: false }).collect();
        self.cues(&cues);
    }

    /// A frame's worth of what to start or cancel (over the network, after rollback has
    /// sorted out replays: a sound already started can't be taken back, so cancels only
    /// show in the log).
    fn cues(&mut self, cues: &[Cue]) {
        self.frame += 1;
        if self.log {
            for c in cues {
                eprintln!("sound: frame {} {}{:?}", c.frame, if c.cancel { "cancel " } else { "" }, c.sound);
            }
        }
        let sounds: Vec<bean_machine::sound::Sound> = cues.iter().filter(|c| !c.cancel).map(|c| c.sound).collect();
        let samples = self.chips.frame(&sounds);
        let mut out = self.out.lock().unwrap();
        out.extend(samples.iter().copied());
        let most = BUFFERED_FRAMES * (OUTPUT_HZ as usize / 60) * 2;
        while out.len() > most {
            out.pop_front();
        }
    }
}

/// The audio output's source: whatever the game has rendered (silence when it's behind).
#[derive(Asset, TypePath, Clone)]
struct Speaker {
    buffer: Arc<Mutex<VecDeque<f32>>>,
    mute: bool,
}

struct SpeakerStream(Speaker);

impl Iterator for SpeakerStream {
    type Item = bevy::audio::Sample;

    fn next(&mut self) -> Option<Self::Item> {
        let s = self.0.buffer.lock().unwrap().pop_front().unwrap_or(0.0);
        Some(if self.0.mute { 0.0 } else { s })
    }
}

impl bevy::audio::Source for SpeakerStream {
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> bevy::audio::ChannelCount {
        bevy::audio::ChannelCount::new(2).unwrap()
    }

    fn sample_rate(&self) -> bevy::audio::SampleRate {
        bevy::audio::SampleRate::new(OUTPUT_HZ).unwrap()
    }

    fn total_duration(&self) -> Option<std::time::Duration> {
        None
    }
}

impl bevy::audio::Decodable for Speaker {
    type Decoder = SpeakerStream;

    fn decoder(&self) -> Self::Decoder {
        SpeakerStream(self.clone())
    }
}

/// Power-on, on the console `MBM_CONSOLE` names: `us` (the default), `jp` (the region
/// lockout, which a code on pad 2 gets past; then the sound test is in the options) or
/// `eu` (the lockout for good).
fn boot(seed: u32) -> Result<Game, String> {
    let mut g = Game::boot(seed)?;
    g.engine.console = match std::env::var("MBM_CONSOLE").unwrap_or_default().to_lowercase().as_str() {
        "jp" => 0x00,
        "eu" => 0xC0,
        _ => 0x80,
    };
    Ok(g)
}

/// One frame of the game. When the flow reaches something not ported yet, it says so
/// (in the title) for a second, then powers on again.
fn play_frame(game: &mut Game, linger: &mut u32, seed: u32, pads: [u8; 2]) {
    game.frame(pads);
    if let Status::Stopped(_) = game.status {
        *linger += 1;
        if *linger > 60 {
            if let Ok(g) = boot(seed) {
                *game = g;
            }
            *linger = 0;
        }
    }
}

/// Does what the network menus ask.
fn act(front: &mut Front, action: Action) {
    match action {
        Action::Host => match Session::host(front.port, front.rom_hash, random_seed(), front.delay) {
            Ok(s) => {
                let port = s.port().unwrap_or(front.port);
                let addresses = netplay::local_address().map(|a| a.to_string()).into_iter().collect();
                front.menu.show(Screen::Host { port, addresses });
                front.net = Some(Online::new(s));
            }
            Err(e) => front.menu.show(Screen::Notice(format!("can't host on port {}: {e}", front.port))),
        },
        Action::Join(addr) => match Session::join(&addr, front.rom_hash) {
            Ok(s) => {
                front.menu.show(Screen::Connecting);
                front.net = Some(Online::new(s));
            }
            Err(e) => front.menu.show(Screen::Notice(format!("can't join {addr}: {e}"))),
        },
        Action::Cancel => {
            if let Some(mut net) = front.net.take() {
                net.session.leave("cancelled");
            }
        }
        Action::Close => {
            front.menu_open = false;
            front.mute = true;
        }
    }
}

/// A seed for a network game (the host's choice; the joiner gets it when connecting).
fn random_seed() -> u32 {
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (t.as_nanos() as u32) ^ std::process::id().rotate_left(16) | 1
}

/// Keys and clicks for the menus (and Esc: back, leave the network game, or quit).
fn ui(
    typed: MessageReader<KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut front: ResMut<Front>,
    mut exit: MessageWriter<AppExit>,
) {
    let inputs = inputs(typed, &mouse, &windows);
    let front = &mut *front;
    if std::mem::take(&mut front.menu_fresh) {
        return;
    }
    for input in inputs {
        let playing = front.net.as_ref().is_some_and(|n| *n.session.state() == State::Playing);
        if playing {
            if input == Input::Back
                && let Some(net) = front.net.as_mut()
            {
                net.session.leave("you left the game");
            }
        } else if front.menu_open {
            if let Some(action) = front.menu.input(input) {
                act(front, action);
            }
        } else if input == Input::Back {
            exit.write(AppExit::Success);
        }
    }
}

/// This frame's keys and clicks, as the menus take them.
fn inputs(mut typed: MessageReader<KeyboardInput>, mouse: &ButtonInput<MouseButton>, windows: &Query<&Window>) -> Vec<Input> {
    let mut inputs = vec![];
    for ev in typed.read().filter(|ev| ev.state == ButtonState::Pressed) {
        let input = match ev.key_code {
            KeyCode::ArrowUp => Input::Up,
            KeyCode::ArrowDown | KeyCode::Tab => Input::Down,
            KeyCode::Enter | KeyCode::NumpadEnter => Input::Confirm,
            KeyCode::Escape => Input::Back,
            KeyCode::Backspace => Input::Erase,
            _ => {
                inputs.extend(ev.text.iter().flat_map(|t| t.chars()).filter(|c| !c.is_control()).map(Input::Char));
                continue;
            }
        };
        inputs.push(input);
    }
    if mouse.just_pressed(MouseButton::Left)
        && let Ok(w) = windows.single()
        && let Some(at) = w.cursor_position()
    {
        // The picture sits in the middle of the window, SCALE times its size.
        let x = (at.x - (w.width() - render::WIDTH as f32 * SCALE) / 2.0) / SCALE;
        let y = (at.y - (w.height() - render::HEIGHT as f32 * SCALE) / 2.0) / SCALE;
        inputs.push(Input::Click(x.floor() as i32, y.floor() as i32));
    }
    inputs
}

/// Debugging: with `MBM_GLITCH_LOG` set, says what draws the score's font inside a
/// playfield (and saves the picture) whenever that starts or changes; F9 saves the
/// picture any time. Pictures go to `capture-N.ppm` in the current directory.
#[derive(Resource)]
struct Glitch {
    log: bool,
    last: Vec<String>,
    shots: u32,
}

impl Glitch {
    fn save(&mut self, picture: &[u8]) -> String {
        self.shots += 1;
        let name = format!("capture-{}.ppm", self.shots);
        let mut ppm = format!("P6 {} {} 255\n", render::WIDTH, render::HEIGHT).into_bytes();
        for px in picture.chunks(4) {
            ppm.extend_from_slice(&px[..3]);
        }
        match std::fs::write(&name, ppm) {
            Ok(()) => name,
            Err(e) => format!("(couldn't save {name}: {e})"),
        }
    }
}

fn show(front: Res<Front>, mut images: ResMut<Assets<Image>>, mut glitch: ResMut<Glitch>, keys: Res<ButtonInput<KeyCode>>) {
    let online = front.net.as_ref().and_then(|n| Some((n, n.shown()?)));
    let game = online.and_then(|(_, o)| o.game.as_ref()).unwrap_or(&front.game);
    let mut picture = game.picture();
    if glitch.log {
        let report = game.font_in_fields();
        if !report.is_empty() && report != glitch.last {
            let shot = glitch.save(&picture);
            eprintln!("font in a playfield ({shot}):");
            for line in &report {
                eprintln!("  {line}");
            }
        }
        glitch.last = report;
    }
    if keys.just_pressed(KeyCode::F9) {
        let shot = glitch.save(&picture);
        eprintln!("saved {shot}");
    }
    if let Some((net, o)) = online {
        match &o.game {
            None => {
                let peer = net.session.peer().map(|p| p.ip().to_string()).unwrap_or_default();
                o.lobby.draw(&mut picture, net.session.side(), &peer);
            }
            Some(g) => {
                if let Some(ready) = g.engine.continue_ready {
                    bean_machine::netmenu::draw_ready(&mut picture, ready, net.session.side());
                }
            }
        }
    } else if front.menu_open {
        front.menu.draw(&mut picture, front.ticks);
    }
    if let Some(mut image) = images.get_mut(&front.screen) {
        image.data = Some(picture);
    }
}

/// The window title: the game's, who we're playing, and anything not ported yet.
fn set_title(front: &mut Front, windows: &mut Query<&mut Window>) {
    let online = front.net.as_ref().and_then(|n| Some((n, n.shown()?)));
    let mut title = TITLE.to_string();
    let mut game = &front.game;
    if let Some((net, o)) = online {
        let peer = net.session.peer().map(|p| p.to_string()).unwrap_or_default();
        title = format!("{title} (network: player {} with {peer})", net.session.side() + 1);
        match &o.game {
            Some(g) => game = g,
            None => return set(front, windows, title),
        }
    }
    if let Status::Stopped(why) = &game.status {
        title = format!("{title} ({why}: not ported yet)");
    }
    set(front, windows, title);

    fn set(front: &mut Front, windows: &mut Query<&mut Window>, title: String) {
        if title != front.title {
            if let Ok(mut w) = windows.single_mut() {
                w.title = title.clone();
            }
            front.title = title;
        }
    }
}
