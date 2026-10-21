//! Dr. Robotnik's Mean Bean Machine, recompiled: the game from power-on (Sega logo,
//! title, menus, scenario mode) on the ported engine, with everything read from your own
//! ROM.
//!
//! Usage: `bean-machine [path to the ROM]` (or set `MBM_ROM`). Arrows move, Z/X/C are
//! A/B/C (turn), Enter is Start. Esc quits.
//!
//! Not in the ROM: NETWORK on the main menu plays 1P vs 2P with someone on another
//! machine (see [`bean_machine::netplay`] and [`bean_machine::netmenu`]). One player
//! hosts, the other types the host's address and connects; both press START in the lobby
//! and the match starts. Our own buttons count straight away, as offline: the other
//! player's are guessed until they arrive (see [`bean_machine::rollback`]). Each side plays with either set of keys. From the command line:
//!   `bean-machine [ROM] --host [PORT]` hosts straight away (port 7845 unless given);
//!   `bean-machine [ROM] --join IP[:PORT]` joins straight away;
//!   `--delay FRAMES` sets the host's input delay (3 unless given).

use std::path::PathBuf;

use bean_machine::game::{Game, Status};
use bean_machine::netmenu::{Action, Input, NetMenu, Screen};
use bean_machine::netplay::{self, Session, State};
use bean_machine::overlay::{self, Canvas};
use bean_machine::rollback::{self, Rollback};
use bean_machine::sound::Cue;
use bean_machine::{pad, render, rom};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::input::ButtonState;
use bevy::input::keyboard::KeyboardInput;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SCALE: f32 = 3.0;
/// The random seed as the boot code leaves it (the game draws from it every frame, so
/// what follows depends on when buttons are pressed, as on the console).
const BOOT_SEED: u32 = 0x7FE7_B46A;
const TITLE: &str = "Dr. Robotnik's Mean Bean Machine";

/// Where the NETWORK line's label goes on the main menu: just right of the bean cursor
/// on its fifth line (the cursor's sprite is at x $C0, y $D0 + line * $18).
const NETWORK_LABEL: (i32, i32) = (0xC0 - 0x80 + 24, 0xD0 + 4 * 0x18 - 0x80 - 6);

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

fn rom_path(args: &Args) -> Option<PathBuf> {
    args.rom.clone().or_else(|| std::env::var_os("MBM_ROM").map(PathBuf::from)).or_else(|| {
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
    let Some(path) = rom_path(&args) else {
        fail("Pass the path to your Mean Bean Machine ROM (or set MBM_ROM).");
    };
    if let Err(e) = rom::install_from(&path) {
        fail(e);
    }
    let start = match (args.host, args.join) {
        (Some(_), _) => Some(Action::Host),
        (_, Some(addr)) => Some(Action::Join(addr)),
        _ => None,
    };
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
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
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step)
        .add_systems(Update, (ui, show).chain())
        .run();
}

#[derive(Resource)]
struct Settings {
    port: u16,
    delay: u8,
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, settings: Res<Settings>, mut start: ResMut<Start>) {
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
    let mut front = Front {
        game: Game::boot(BOOT_SEED).expect("starting the game"),
        screen,
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
        audio: Audio::new(),
        title: TITLE.into(),
    };
    if let Some(action) = start.0.take() {
        front.menu_open = true;
        if let Action::Join(addr) = &action {
            front.menu.address = addr.clone();
        }
        act(&mut front, action);
    }
    commands.insert_resource(front);
}

/// The keyboard as player 1's pad (arrows, Z/X/C, Enter).
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

/// And player 2's (WASD, J/K/L, Space), for 1P vs 2P.
const P2_KEYS: [(KeyCode, u8); 8] = [
    (KeyCode::KeyW, pad::UP),
    (KeyCode::KeyS, pad::DOWN),
    (KeyCode::KeyA, pad::LEFT),
    (KeyCode::KeyD, pad::RIGHT),
    (KeyCode::KeyK, pad::B),
    (KeyCode::KeyL, pad::C),
    (KeyCode::KeyJ, pad::A),
    (KeyCode::Space, pad::START),
];

fn buttons(keys: &ButtonInput<KeyCode>, map: &[(KeyCode, u8)]) -> u8 {
    map.iter().filter(|(k, _)| keys.pressed(*k)).fold(0, |acc, (_, b)| acc | b)
}

fn step(keys: Res<ButtonInput<KeyCode>>, mut front: ResMut<Front>, mut windows: Query<&mut Window>) {
    let front = &mut *front;
    front.ticks += 1;
    let mut pads = [buttons(&keys, &P1_KEYS), buttons(&keys, &P2_KEYS)];
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
                play_online(net, pads[0] | pads[1]);
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

/// Where the game's sounds go (see [`bean_machine::sound`]). No sound driver is ported
/// yet: with `MBM_SOUND_LOG` set, the requests are printed instead.
struct Audio {
    log: bool,
    /// Frames played offline (to number their cues).
    frame: u32,
}

impl Audio {
    fn new() -> Audio {
        Audio { log: std::env::var_os("MBM_SOUND_LOG").is_some(), frame: 0 }
    }

    /// An offline frame's requests: all of them, once.
    fn frame(&mut self, sounds: &[bean_machine::sound::Sound]) {
        let frame = self.frame;
        self.frame += 1;
        let cues: Vec<Cue> = sounds.iter().map(|&sound| Cue { frame, sound, cancel: false }).collect();
        self.cues(&cues);
    }

    /// What to start or cancel (over the network, after rollback has sorted out replays).
    fn cues(&mut self, cues: &[Cue]) {
        if self.log {
            for c in cues {
                eprintln!("sound: frame {} {}{:?}", c.frame, if c.cancel { "cancel " } else { "" }, c.sound);
            }
        }
    }
}

/// One frame of the game. When the flow reaches something not ported yet, it says so
/// (in the title) for a second, then powers on again.
fn play_frame(game: &mut Game, linger: &mut u32, seed: u32, pads: [u8; 2]) {
    game.frame(pads);
    if let Status::Stopped(_) = game.status {
        *linger += 1;
        if *linger > 60 {
            if let Ok(g) = Game::boot(seed) {
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
    mut typed: MessageReader<KeyboardInput>,
    mouse: Res<ButtonInput<MouseButton>>,
    windows: Query<&Window>,
    mut front: ResMut<Front>,
    mut exit: MessageWriter<AppExit>,
) {
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

fn show(front: Res<Front>, mut images: ResMut<Assets<Image>>) {
    let online = front.net.as_ref().and_then(|n| Some((n, n.shown()?)));
    let mut picture = match online.and_then(|(_, o)| o.game.as_ref()) {
        Some(g) => g.picture(),
        None => front.game.picture(),
    };
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
    } else if let Some(line) = front.game.main_menu_line() {
        let color = if line == bean_machine::actors::NETWORK_LINE { overlay::YELLOW } else { overlay::WHITE };
        Canvas(&mut picture).text(NETWORK_LABEL.0, NETWORK_LABEL.1, "NETWORK", color, 2);
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
