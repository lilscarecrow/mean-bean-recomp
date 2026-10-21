//! Dr. Robotnik's Mean Bean Machine, recompiled: the game from power-on (Sega logo,
//! title, menus, scenario mode) on the ported engine, with everything read from your own
//! ROM.
//!
//! Usage: `bean-machine [path to the ROM]` (or set `MBM_ROM`). Arrows move, Z/X/C are
//! A/B/C (turn), Enter is Start. Esc quits.
//!
//! Over the network (see [`bean_machine::netplay`]):
//!   `bean-machine [ROM] --host [PORT] [--delay FRAMES]` waits for a player (port 7845
//!   unless given) and plays as player 1;
//!   `bean-machine [ROM] --join IP[:PORT]` plays as player 2 on that host.
//! Each side plays with either set of keys; the game starts from power-on once both are
//! in.

use std::path::PathBuf;

use bean_machine::game::{Game, Status};
use bean_machine::netplay::{self, Session, State};
use bean_machine::{pad, render, rom};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SCALE: f32 = 3.0;
/// The random seed as the boot code leaves it (the game draws from it every frame, so
/// what follows depends on when buttons are pressed, as on the console).
const BOOT_SEED: u32 = 0x7FE7_B46A;

#[derive(Resource)]
struct Front {
    game: Game,
    screen: Handle<Image>,
    /// Frames shown since the game reached something not ported yet.
    linger: u32,
    /// The seed the game powers on with (the host's, over the network).
    seed: u32,
    /// Playing over the network: the connection and the next frame to play.
    net: Option<Session>,
    frame: u32,
    title: String,
}

const TITLE: &str = "Dr. Robotnik's Mean Bean Machine";

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

fn main() {
    let args = args().unwrap_or_else(|e| fail(e));
    let Some(path) = rom_path(&args) else {
        fail("Pass the path to your Mean Bean Machine ROM (or set MBM_ROM).");
    };
    if let Err(e) = rom::install_from(&path) {
        fail(e);
    }
    let rom_hash = netplay::hash(rom::get().unwrap());
    let delay = args.delay.unwrap_or(netplay::DEFAULT_DELAY);
    let net = match (args.host, &args.join) {
        (Some(port), _) => Some(Session::host(port, rom_hash, BOOT_SEED, delay).unwrap_or_else(|e| fail(format!("can't host on port {port}: {e}")))),
        (_, Some(addr)) => Some(Session::join(addr, rom_hash).unwrap_or_else(|e| fail(format!("can't join {addr}: {e}")))),
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
        .insert_resource(Pending(net))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step)
        .add_systems(Update, (show, quit))
        .run();
}

/// The connection, until `setup` moves it into [`Front`].
#[derive(Resource)]
struct Pending(Option<Session>);

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>, mut pending: ResMut<Pending>) {
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
    let game = Game::boot(BOOT_SEED).expect("starting the game");
    let net = pending.0.take();
    commands.insert_resource(Front { game, screen, linger: 0, seed: BOOT_SEED, net, frame: 0, title: TITLE.into() });
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
    let pads = [buttons(&keys, &P1_KEYS), buttons(&keys, &P2_KEYS)];
    let front = &mut *front;
    let Some(net) = front.net.as_mut() else {
        play_frame(&mut front.game, &mut front.linger, front.seed, pads);
        set_title(front, &mut windows);
        return;
    };
    net.receive();
    if *net.state() == State::Playing && front.frame == 0 && front.seed != net.seed() {
        // (joined: power on with the host's seed)
        front.seed = net.seed();
        front.game = Game::boot(front.seed).expect("starting the game");
    }
    // Either set of keys is our pad. Play the next frame once the other side's buttons
    // for it are here, and a second one when they're ahead of us.
    let mine = pads[0] | pads[1];
    for _ in 0..2 {
        let Some(both) = net.step(front.frame, mine) else { break };
        front.frame += 1;
        play_frame(&mut front.game, &mut front.linger, front.seed, both);
        if front.frame.is_multiple_of(netplay::CHECK_EVERY) {
            net.check(front.frame, front.game.checksum());
        }
        if !net.behind(front.frame) {
            break;
        }
    }
    net.send();
    set_title(front, &mut windows);
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

/// The window title: the game's, the connection's state, and anything not ported yet.
fn set_title(front: &mut Front, windows: &mut Query<&mut Window>) {
    let mut title = TITLE.to_string();
    if let Some(net) = &front.net {
        let port = net.port().map(|p| p.to_string()).unwrap_or_default();
        let peer = net.peer().map(|p| p.to_string()).unwrap_or_default();
        title = match net.state() {
            State::Waiting => format!("{title} (hosting on port {port}: waiting for player 2)"),
            State::Connecting => format!("{title} (joining {peer}...)"),
            State::Playing if net.side() == 0 => format!("{title} (player 1, {peer} is player 2)"),
            State::Playing => format!("{title} (player 2, hosted by {peer})"),
            State::Closed(why) => format!("{title} (disconnected: {why})"),
        };
    }
    if let Status::Stopped(why) = &front.game.status {
        title = format!("{title} ({why}: not ported yet)");
    }
    if title != front.title {
        if let Ok(mut w) = windows.single_mut() {
            w.title = title.clone();
        }
        front.title = title;
    }
}

fn show(front: Res<Front>, mut images: ResMut<Assets<Image>>) {
    if let Some(mut image) = images.get_mut(&front.screen) {
        image.data = Some(front.game.picture());
    }
}

fn quit(keys: Res<ButtonInput<KeyCode>>, mut front: ResMut<Front>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        if let Some(net) = front.net.as_mut() {
            net.leave();
        }
        exit.write(AppExit::Success);
    }
}
