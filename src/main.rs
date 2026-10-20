//! Dr. Robotnik's Mean Bean Machine, rebuilt: scenario mode on the ported engine, with
//! the graphics read from your own ROM.
//!
//! Usage: `bean-machine [path to the ROM]` (or set `MBM_ROM`). Arrows move, Z/X/C are
//! A/B/C (turn), Enter is Start. Esc quits.

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use bean_machine::mbm::game::{Game, Status};
use bean_machine::mbm::{pad, render, rom};
use bevy::asset::RenderAssetUsages;
use bevy::image::ImageSampler;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};

const SCALE: f32 = 3.0;
/// The first scenario stage (levels 0-2 are the lessons).
const FIRST_LEVEL: u8 = 3;
/// The difficulty option (0 easiest .. 3 hardest).
const DIFFICULTY: u8 = 2;

#[derive(Resource)]
struct Front {
    game: Game,
    screen: Handle<Image>,
    /// Frames left showing a finished match before the next one starts.
    linger: u32,
}

fn rom_path() -> Option<PathBuf> {
    std::env::args_os().nth(1).map(PathBuf::from).or_else(|| std::env::var_os("MBM_ROM").map(PathBuf::from)).or_else(|| {
        let f = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../bean-machine-re/rom_path.txt");
        std::fs::read_to_string(f).ok().map(|s| PathBuf::from(s.trim()))
    })
}

fn seed() -> u32 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.subsec_nanos() ^ d.as_secs() as u32).unwrap_or(0x2A6D_365A)
}

fn main() {
    let Some(path) = rom_path() else {
        eprintln!("Pass the path to your Mean Bean Machine ROM (or set MBM_ROM).");
        std::process::exit(2);
    };
    if let Err(e) = rom::install_from(&path) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Dr. Robotnik's Mean Bean Machine".into(),
                resolution: ((render::WIDTH as f32 * SCALE) as u32, (render::HEIGHT as f32 * SCALE) as u32).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::BLACK))
        .insert_resource(Time::<Fixed>::from_hz(60.0))
        .add_systems(Startup, setup)
        .add_systems(FixedUpdate, step)
        .add_systems(Update, (show, quit))
        .run();
}

fn setup(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
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
    let game = Game::new(FIRST_LEVEL, DIFFICULTY, 0, seed()).expect("starting the game");
    commands.insert_resource(Front { game, screen, linger: 0 });
}

/// The keyboard as player 1's pad.
fn buttons(keys: &ButtonInput<KeyCode>) -> u8 {
    [
        (KeyCode::ArrowUp, pad::UP),
        (KeyCode::ArrowDown, pad::DOWN),
        (KeyCode::ArrowLeft, pad::LEFT),
        (KeyCode::ArrowRight, pad::RIGHT),
        (KeyCode::KeyX, pad::B),
        (KeyCode::KeyC, pad::C),
        (KeyCode::KeyZ, pad::A),
        (KeyCode::Enter, pad::START),
    ]
    .into_iter()
    .filter(|(k, _)| keys.pressed(*k))
    .fold(0, |acc, (_, b)| acc | b)
}

fn step(keys: Res<ButtonInput<KeyCode>>, mut front: ResMut<Front>, mut windows: Query<&mut Window>) {
    let p1 = buttons(&keys);
    front.game.frame([p1, 0]);
    if let Status::Stopped(why) = front.game.status.clone() {
        // The flow reached a screen that isn't ported yet (the stage intro, game over):
        // start the stage it was heading for. LevelEnd has already moved the level on
        // after a win, and the score carries over.
        front.linger += 1;
        if front.linger == 1 {
            if let Ok(mut w) = windows.single_mut() {
                w.title = format!("Mean Bean Machine ({why}: not ported yet)");
            }
        }
        if front.linger > 60 {
            let e = &front.game.engine;
            let won = e.last_winner == 0;
            let score = if won { e.result_score } else { 0 };
            if let Ok(g) = Game::new(e.level.clamp(3, 15), DIFFICULTY, score, seed()) {
                front.game = g;
            }
            front.linger = 0;
        }
    }
}

fn show(front: Res<Front>, mut images: ResMut<Assets<Image>>) {
    if let Some(mut image) = images.get_mut(&front.screen) {
        image.data = Some(front.game.picture());
    }
}

fn quit(keys: Res<ButtonInput<KeyCode>>, mut exit: MessageWriter<AppExit>) {
    if keys.just_pressed(KeyCode::Escape) {
        exit.write(AppExit::Success);
    }
}
