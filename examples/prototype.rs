//! Bean Machine: a Puyo Puyo / Mean Bean Machine style puzzler.
//!
//! Versus mode (default): you on the left, the CPU on the right. Chains turn into garbage
//! ("refugee") beans that rain into the other well; incoming garbage is cancelled first by
//! your own chains. Tab switches between Versus and Solo.
//!
//! Set BEAN_DEMO=1 to let a CPU play your side too (attract mode / testing).
//!
//! Controls: Left/Right move, Down soft drop, X or Up rotate clockwise, Z rotate counter-clockwise,
//! Enter restarts after a match.

use std::collections::VecDeque;
use std::time::{SystemTime, UNIX_EPOCH};

use bean_machine::ai;
use bean_machine::board::{self, Bean, Board, Piece, Rng, VISIBLE_H, W};
use bevy::prelude::*;

const CELL: f32 = 40.0;
const WELL: Vec2 = Vec2::new(W as f32 * CELL, VISIBLE_H as f32 * CELL);
/// Bottom-left corner of each well in world units, per mode.
const WELL_Y: f32 = -250.0;
const VERSUS_X: [f32; 2] = [-440.0, 200.0];
const SOLO_X: f32 = -180.0;
const NUM_COLORS: usize = 4;

const SETTLE_STEP: f32 = 0.035;
const POP_TIME: f32 = 0.45;
const SOFT_DROP_STEP: f32 = 0.03;
const DAS_DELAY: f32 = 0.16;
const DAS_REPEAT: f32 = 0.05;
const READY_TIME: f32 = 1.6;

/// Chain points per garbage bean sent (Puyo Puyo's "target points").
const GARBAGE_POINTS: u64 = 70;
/// Most garbage that falls in one go (5 rows); the rest waits for the next piece.
const MAX_GARBAGE_DROP: u32 = 30;
/// Seconds between CPU "button presses". Lower = harder.
const CPU_ACT: f32 = 0.11;

/// Center of cell (x, y) relative to its well's bottom-left corner.
fn cell_local(x: f32, y: f32) -> Vec2 {
    Vec2::new((x + 0.5) * CELL, (y + 0.5) * CELL)
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Bean Machine".into(),
                resolution: (960, 680).into(),
                ..default()
            }),
            ..default()
        }))
        .insert_resource(ClearColor(Color::srgb(0.08, 0.06, 0.14)))
        .insert_resource(Game::new(Mode::Versus))
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                handle_input,
                cpu_think,
                step_game,
                layout_wells,
                draw_board,
                draw_bridges,
                draw_piece,
                draw_preview,
                draw_pending,
                draw_hud,
            )
                .chain(),
        )
        .run();
}

fn seed() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0x9E3779B97F4A7C15)
}

// ---------------------------------------------------------------- game state

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Solo,
    Versus,
}

#[derive(Clone, PartialEq)]
enum Phase {
    Falling,
    Settling,
    Popping(Vec<Vec<(i32, i32)>>),
    Dead,
}

#[derive(Default)]
struct Cpu {
    /// Where the CPU wants the current pair to land.
    target: Option<Piece>,
    act_timer: f32,
    /// Holding "down": aligned, or gave up steering.
    soft: bool,
}

struct Player {
    board: Board,
    piece: Option<Piece>,
    queue: VecDeque<(Bean, Bean)>,
    rng: Rng,
    /// Separate stream for garbage columns so both players keep the same piece sequence.
    garbage_rng: Rng,
    phase: Phase,
    timer: f32,
    fall_timer: f32,
    score: u64,
    chain: u32,
    /// Last chain length worth announcing, and how long the banner stays up.
    chain_banner: (u32, f32),
    best_chain: u32,
    pieces: u32,
    /// Held horizontal direction and how long it has been held (delayed auto-shift).
    das: (i32, f32),
    /// Garbage waiting to fall on this player.
    pending: u32,
    /// Chain points not yet converted into a whole garbage bean.
    leftover: u64,
    /// Garbage already fell after this piece, so the next settle spawns a new pair.
    garbage_dropped: bool,
    cpu: Option<Cpu>,
}

impl Player {
    fn new(seed: u64, index: u64, cpu: bool) -> Self {
        let mut rng = Rng::new(seed);
        let queue = (0..3).map(|_| rng.pair(NUM_COLORS)).collect();
        let mut player = Player {
            board: Board::default(),
            piece: None,
            queue,
            rng,
            garbage_rng: Rng::new(seed.rotate_left(17) ^ (index + 1)),
            phase: Phase::Falling,
            timer: 0.0,
            fall_timer: 0.0,
            score: 0,
            chain: 0,
            chain_banner: (0, 0.0),
            best_chain: 0,
            pieces: 0,
            das: (0, 0.0),
            pending: 0,
            leftover: 0,
            garbage_dropped: false,
            cpu: cpu.then(Cpu::default),
        };
        player.spawn();
        player
    }

    /// Gravity speeds up every 20 pieces.
    fn fall_interval(&self) -> f32 {
        (0.7 * 0.9f32.powi((self.pieces / 20) as i32)).max(0.12)
    }

    fn spawn(&mut self) {
        let (pivot, satellite) = self.queue.pop_front().unwrap();
        self.queue.push_back(self.rng.pair(NUM_COLORS));
        let piece = Piece::new(pivot, satellite);
        self.fall_timer = 0.0;
        if let Some(cpu) = &mut self.cpu {
            *cpu = Cpu::default();
        }
        if piece.fits(&self.board) {
            self.piece = Some(piece);
            self.phase = Phase::Falling;
        } else {
            self.piece = None;
            self.phase = Phase::Dead;
        }
    }

    fn lock(&mut self) {
        if let Some(piece) = self.piece.take() {
            piece.lock(&mut self.board);
            self.pieces += 1;
            self.chain = 0;
            self.timer = 0.0;
            self.phase = Phase::Settling;
        }
    }

    /// Advances falling / settling / popping. Returns garbage this player's chain sends out.
    fn step(&mut self, dt: f32, soft: bool) -> u32 {
        self.chain_banner.1 = (self.chain_banner.1 - dt).max(0.0);
        let mut sent = 0;
        match self.phase.clone() {
            Phase::Falling => {
                let interval = if soft { SOFT_DROP_STEP } else { self.fall_interval() };
                self.fall_timer += dt;
                while self.fall_timer >= interval {
                    self.fall_timer -= interval;
                    let Some(piece) = self.piece.as_mut() else { break };
                    if piece.try_move(&self.board, 0, -1) {
                        if soft {
                            self.score += 1;
                        }
                    } else {
                        self.lock();
                        break;
                    }
                }
            }
            Phase::Settling => {
                self.timer += dt;
                while self.timer >= SETTLE_STEP {
                    self.timer -= SETTLE_STEP;
                    if self.board.gravity_step() {
                        continue;
                    }
                    let groups = self.board.find_pops();
                    if !groups.is_empty() {
                        self.chain += 1;
                        self.timer = 0.0;
                        self.phase = Phase::Popping(groups);
                    } else if self.board.is_topped_out() {
                        self.phase = Phase::Dead;
                    } else if self.pending > 0 && !self.garbage_dropped {
                        // Chain is over: incoming garbage rains down before the next pair.
                        let n = self.pending.min(MAX_GARBAGE_DROP);
                        self.pending -= n;
                        self.board.drop_garbage(n as usize, &mut self.garbage_rng);
                        self.garbage_dropped = true;
                        self.timer = 0.0;
                    } else {
                        self.garbage_dropped = false;
                        self.spawn();
                    }
                    break;
                }
            }
            Phase::Popping(groups) => {
                self.timer += dt;
                if self.timer >= POP_TIME {
                    let info = self.board.clear(&groups);
                    let points = board::score(self.chain, &info);
                    self.score += points;
                    self.leftover += points;
                    sent = (self.leftover / GARBAGE_POINTS) as u32;
                    self.leftover %= GARBAGE_POINTS;
                    if self.chain >= 2 {
                        self.chain_banner = (self.chain, 1.2);
                    }
                    self.best_chain = self.best_chain.max(self.chain);
                    self.timer = 0.0;
                    self.phase = Phase::Settling;
                }
            }
            Phase::Dead => {}
        }
        sent
    }
}

#[derive(Resource)]
struct Game {
    mode: Mode,
    players: [Player; 2],
    /// "READY?" countdown before pieces start falling.
    ready: f32,
    over: bool,
}

impl Game {
    fn new(mode: Mode) -> Self {
        // Same seed for both players: everyone gets the same sequence of pairs.
        let seed = seed();
        Game {
            mode,
            players: [Player::new(seed, 0, std::env::var_os("BEAN_DEMO").is_some()), Player::new(seed, 1, true)],
            ready: READY_TIME,
            over: false,
        }
    }

    fn active(&self) -> usize {
        match self.mode {
            Mode::Solo => 1,
            Mode::Versus => 2,
        }
    }
}

// ---------------------------------------------------------------- visuals

#[derive(Resource)]
struct Palette {
    colors: Vec<Handle<ColorMaterial>>,
    flash: Handle<ColorMaterial>,
}

impl Palette {
    fn bean(&self, bean: Bean) -> Handle<ColorMaterial> {
        self.colors[bean.index()].clone()
    }
}

fn bean_color(bean: Bean) -> Color {
    match bean {
        Bean::Red => Color::srgb(0.93, 0.22, 0.25),
        Bean::Green => Color::srgb(0.28, 0.85, 0.35),
        Bean::Blue => Color::srgb(0.25, 0.5, 0.98),
        Bean::Yellow => Color::srgb(0.98, 0.85, 0.2),
        Bean::Purple => Color::srgb(0.7, 0.35, 0.95),
        Bean::Garbage => Color::srgb(0.62, 0.66, 0.74),
    }
}

/// Parent of everything drawn for one player; placed at that well's bottom-left corner.
#[derive(Component)]
struct WellRoot(usize);

/// A fixed cell of a well; its color/visibility mirror that player's board.
#[derive(Component)]
struct GridBean {
    p: usize,
    x: i32,
    y: i32,
}

/// Connector drawn between a cell and its right (or upper) neighbor when they match.
#[derive(Component)]
struct Bridge {
    p: usize,
    x: i32,
    y: i32,
    vertical: bool,
}

/// Part 0 = pivot, 1 = satellite of a player's falling pair.
#[derive(Component)]
struct PieceBean {
    p: usize,
    part: usize,
}

/// Preview slot: queue index, and 0 = pivot / 1 = satellite.
#[derive(Component)]
struct PreviewBean {
    p: usize,
    slot: usize,
    part: usize,
}

/// One icon in the incoming-garbage row above a well.
#[derive(Component)]
struct PendingIcon {
    p: usize,
    i: usize,
}

#[derive(Component)]
struct ScoreText(usize);

#[derive(Component)]
struct ChainText(usize);

#[derive(Component)]
struct ResultText(usize);

#[derive(Component)]
struct InfoText;

/// Meshes/materials shared by every bean entity.
struct BeanArt {
    body: Handle<Mesh>,
    eye: Handle<Mesh>,
    pupil: Handle<Mesh>,
    body_mat: Handle<ColorMaterial>,
    white: Handle<ColorMaterial>,
    black: Handle<ColorMaterial>,
}

/// A bean with two googly eyes; the eyes are children so they follow its visibility and scale.
fn spawn_bean(commands: &mut Commands, art: &BeanArt, root: Entity, pos: Vec2, marker: impl Bundle) {
    commands
        .spawn((
            Mesh2d(art.body.clone()),
            MeshMaterial2d(art.body_mat.clone()),
            Transform::from_translation(pos.extend(1.0)),
            Visibility::Hidden,
            ChildOf(root),
            marker,
        ))
        .with_children(|parent| {
            for side in [-1.0, 1.0] {
                parent.spawn((
                    Mesh2d(art.eye.clone()),
                    MeshMaterial2d(art.white.clone()),
                    Transform::from_xyz(side * CELL * 0.16, CELL * 0.08, 0.1),
                ));
                parent.spawn((
                    Mesh2d(art.pupil.clone()),
                    MeshMaterial2d(art.black.clone()),
                    Transform::from_xyz(side * CELL * 0.16, CELL * 0.05, 0.2),
                ));
            }
        });
}

fn font(px: f32) -> TextFont {
    TextFont { font_size: bevy::text::FontSize::Px(px), ..default() }
}

fn setup(mut commands: Commands, mut meshes: ResMut<Assets<Mesh>>, mut materials: ResMut<Assets<ColorMaterial>>) {
    commands.spawn(Camera2d);

    let palette = Palette {
        colors: Bean::ALL.iter().map(|&b| materials.add(bean_color(b))).collect(),
        flash: materials.add(Color::WHITE),
    };
    let art = BeanArt {
        body: meshes.add(Circle::new(CELL * 0.46)),
        eye: meshes.add(Circle::new(CELL * 0.12)),
        pupil: meshes.add(Circle::new(CELL * 0.06)),
        body_mat: palette.colors[0].clone(),
        white: palette.flash.clone(),
        black: materials.add(Color::BLACK),
    };
    let bridge_h = meshes.add(Rectangle::new(CELL, CELL * 0.55));
    let bridge_v = meshes.add(Rectangle::new(CELL * 0.55, CELL));
    let center = WELL / 2.0;

    for p in 0..2 {
        let root = commands
            .spawn((Transform::from_xyz(VERSUS_X[p], WELL_Y, 0.0), Visibility::Visible, WellRoot(p)))
            .id();
        // The side panel (preview, score) sits right of your well and left of the CPU's.
        let side = if p == 0 { 1.0 } else { -1.0 };
        let panel_x = if p == 0 { WELL.x + 70.0 } else { -70.0 };
        let text_x = if p == 0 { WELL.x + 100.0 } else { -100.0 };

        // Well background + frame.
        let frame = if p == 0 { Color::srgb(0.35, 0.3, 0.5) } else { Color::srgb(0.5, 0.25, 0.3) };
        commands.spawn((Sprite::from_color(frame, WELL + 16.0), Transform::from_translation(center.extend(-2.0)), ChildOf(root)));
        commands.spawn((
            Sprite::from_color(Color::srgb(0.03, 0.02, 0.07), WELL),
            Transform::from_translation(center.extend(-1.0)),
            ChildOf(root),
        ));
        // Mark the spawn column's top cell: filling it ends the game.
        let spawn = cell_local(board::SPAWN_X as f32, board::SPAWN_Y as f32);
        for angle in [0.785f32, -0.785] {
            commands.spawn((
                Sprite::from_color(Color::srgba(0.9, 0.2, 0.3, 0.35), Vec2::new(CELL * 0.8, 4.0)),
                Transform::from_translation(spawn.extend(-0.5)).with_rotation(Quat::from_rotation_z(angle)),
                ChildOf(root),
            ));
        }

        for y in 0..VISIBLE_H as i32 {
            for x in 0..W as i32 {
                let pos = cell_local(x as f32, y as f32);
                spawn_bean(&mut commands, &art, root, pos, GridBean { p, x, y });
                for vertical in [false, true] {
                    let (mesh, offset) = if vertical {
                        (bridge_v.clone(), Vec2::new(0.0, CELL * 0.5))
                    } else {
                        (bridge_h.clone(), Vec2::new(CELL * 0.5, 0.0))
                    };
                    commands.spawn((
                        Mesh2d(mesh),
                        MeshMaterial2d(palette.colors[0].clone()),
                        Transform::from_translation((pos + offset).extend(0.5)),
                        Visibility::Hidden,
                        Bridge { p, x, y, vertical },
                        ChildOf(root),
                    ));
                }
            }
        }
        for part in 0..2 {
            spawn_bean(&mut commands, &art, root, Vec2::ZERO, PieceBean { p, part });
        }
        for i in 0..6 {
            let pos = Vec2::new((i as f32 + 0.5) * CELL, WELL.y + 30.0);
            spawn_bean(&mut commands, &art, root, pos, PendingIcon { p, i });
        }

        // Next / next-next preview.
        commands.spawn((
            Text2d::new("NEXT"),
            font(20.0),
            Transform::from_xyz(panel_x + side * 20.0, WELL.y - 10.0, 0.0),
            ChildOf(root),
        ));
        for slot in 0..2 {
            let scale = if slot == 0 { 1.0 } else { 0.75 };
            for part in 0..2 {
                let pos = Vec2::new(
                    panel_x + side * slot as f32 * 55.0,
                    WELL.y - 110.0 + part as f32 * CELL * scale + slot as f32 * 10.0,
                );
                spawn_bean(&mut commands, &art, root, pos, PreviewBean { p, slot, part });
            }
        }

        commands.spawn((Text2d::new(""), font(20.0), Transform::from_xyz(text_x, WELL.y - 270.0, 0.0), ScoreText(p), ChildOf(root)));
        commands.spawn((
            Text2d::new(""),
            font(36.0),
            TextColor(Color::srgb(1.0, 0.9, 0.3)),
            Transform::from_translation(center.extend(5.0)),
            ChainText(p),
            ChildOf(root),
        ));
        commands.spawn((
            Text2d::new(""),
            font(40.0),
            Transform::from_translation((center + Vec2::new(0.0, 80.0)).extend(5.0)),
            ResultText(p),
            ChildOf(root),
        ));
        if p == 0 {
            commands.spawn((
                Text2d::new("<- ->  move\nDown  drop\nX / Up  rotate\nZ  rotate back"),
                font(15.0),
                TextColor(Color::srgb(0.6, 0.6, 0.75)),
                Transform::from_xyz(text_x, 45.0, 0.0),
                ChildOf(root),
            ));
        }
    }

    commands.spawn((
        Text2d::new(""),
        font(20.0),
        TextColor(Color::srgb(0.8, 0.8, 0.95)),
        Transform::from_xyz(0.0, WELL_Y - 45.0, 5.0),
        InfoText,
    ));
    commands.insert_resource(palette);
}

// ---------------------------------------------------------------- systems

fn handle_input(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut game: ResMut<Game>) {
    if keys.just_pressed(KeyCode::Tab) {
        let mode = if game.mode == Mode::Versus { Mode::Solo } else { Mode::Versus };
        *game = Game::new(mode);
        return;
    }
    if game.over {
        if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::Space) {
            let best = game.players[0].best_chain;
            *game = Game::new(game.mode);
            game.players[0].best_chain = best;
        }
        return;
    }
    if game.ready > 0.0 {
        return;
    }
    let me = &mut game.players[0];
    let Some(piece) = me.piece.as_mut().filter(|_| me.phase == Phase::Falling) else { return };

    // Horizontal movement with delayed auto-shift.
    let dir = match (keys.pressed(KeyCode::ArrowLeft), keys.pressed(KeyCode::ArrowRight)) {
        (true, false) => -1,
        (false, true) => 1,
        _ => 0,
    };
    if dir != me.das.0 {
        me.das = (dir, 0.0);
        if dir != 0 {
            piece.try_move(&me.board, dir, 0);
        }
    } else if dir != 0 {
        me.das.1 += time.delta_secs();
        while me.das.1 >= DAS_DELAY {
            piece.try_move(&me.board, dir, 0);
            me.das.1 -= DAS_REPEAT;
        }
    }

    if keys.just_pressed(KeyCode::KeyX) || keys.just_pressed(KeyCode::ArrowUp) {
        piece.try_rotate(&me.board, 1);
    }
    if keys.just_pressed(KeyCode::KeyZ) {
        piece.try_rotate(&me.board, -1);
    }
}

/// CPUs pick a target once per pair, then steer toward it one "button press" at a time.
fn cpu_think(time: Res<Time>, mut game: ResMut<Game>) {
    if game.over || game.ready > 0.0 {
        return;
    }
    let active = game.active();
    for player in &mut game.players[..active] {
        let (Some(piece), Some(cpu)) = (player.piece.as_mut(), player.cpu.as_mut()) else { continue };
        if player.phase != Phase::Falling || cpu.soft {
            continue;
        }
        if cpu.target.is_none() {
            let next = player.queue[0];
            cpu.target = ai::best_move(&player.board, (piece.pivot, piece.satellite), next, &mut player.garbage_rng);
            if cpu.target.is_none() {
                cpu.soft = true; // No safe move: just drop it.
                continue;
            }
        }
        cpu.act_timer += time.delta_secs();
        if cpu.act_timer < CPU_ACT {
            continue;
        }
        cpu.act_timer = 0.0;
        let target = cpu.target.unwrap();
        let ok = if piece.rot != target.rot {
            let dir = if (target.rot + 4 - piece.rot) % 4 == 3 { -1 } else { 1 };
            piece.try_rotate(&player.board, dir)
        } else if piece.x != target.x {
            piece.try_move(&player.board, (target.x - piece.x).signum(), 0)
        } else {
            false
        };
        // Aligned (or blocked on the way): hold down.
        cpu.soft = !ok;
    }
}

fn step_game(keys: Res<ButtonInput<KeyCode>>, time: Res<Time>, mut game: ResMut<Game>) {
    let dt = time.delta_secs();
    if game.over {
        return;
    }
    if game.ready > 0.0 {
        game.ready -= dt;
        return;
    }
    for p in 0..game.active() {
        let soft = match &game.players[p].cpu {
            Some(cpu) => cpu.soft,
            None => keys.pressed(KeyCode::ArrowDown),
        };
        let sent = game.players[p].step(dt, soft);
        if sent > 0 && game.mode == Mode::Versus {
            // Your chain first cancels garbage headed your way; the rest crosses over.
            let me = &mut game.players[p];
            let cancelled = sent.min(me.pending);
            me.pending -= cancelled;
            game.players[1 - p].pending += sent - cancelled;
        }
    }
    game.over = game.players[..game.active()].iter().any(|pl| pl.phase == Phase::Dead);
}

fn layout_wells(game: Res<Game>, mut roots: Query<(&WellRoot, &mut Transform, &mut Visibility)>) {
    for (root, mut transform, mut visibility) in &mut roots {
        let x = match game.mode {
            Mode::Versus => VERSUS_X[root.0],
            Mode::Solo => SOLO_X,
        };
        transform.translation.x = x;
        *visibility = if root.0 < game.active() { Visibility::Visible } else { Visibility::Hidden };
    }
}

fn is_popping(player: &Player, x: i32, y: i32) -> bool {
    match &player.phase {
        Phase::Popping(groups) => groups.iter().flatten().any(|&c| c == (x, y)),
        _ => false,
    }
}

fn draw_board(
    game: Res<Game>,
    palette: Res<Palette>,
    mut beans: Query<(&GridBean, &mut MeshMaterial2d<ColorMaterial>, &mut Visibility, &mut Transform)>,
) {
    for (cell, mut material, mut visibility, mut transform) in &mut beans {
        let player = &game.players[cell.p];
        // Popping beans blink white and puff up before vanishing.
        let blink = (player.timer * 14.0) as i32 % 2 == 0;
        match player.board.get(cell.x, cell.y) {
            Some(bean) => {
                let popping = is_popping(player, cell.x, cell.y);
                material.0 = if popping && blink { palette.flash.clone() } else { palette.bean(bean) };
                transform.scale = Vec3::splat(if popping { 1.0 + player.timer * 0.5 } else { 1.0 });
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn draw_bridges(
    game: Res<Game>,
    palette: Res<Palette>,
    mut bridges: Query<(&Bridge, &mut MeshMaterial2d<ColorMaterial>, &mut Visibility)>,
) {
    for (bridge, mut material, mut visibility) in &mut bridges {
        let player = &game.players[bridge.p];
        let (nx, ny) = if bridge.vertical { (bridge.x, bridge.y + 1) } else { (bridge.x + 1, bridge.y) };
        let here = player.board.get(bridge.x, bridge.y);
        let linked = ny < VISIBLE_H as i32
            && here.is_some_and(|b| b != Bean::Garbage)
            && here == player.board.get(nx, ny)
            && !is_popping(player, bridge.x, bridge.y);
        match here {
            Some(bean) if linked => {
                material.0 = palette.bean(bean);
                *visibility = Visibility::Inherited;
            }
            _ => *visibility = Visibility::Hidden,
        }
    }
}

fn draw_piece(
    game: Res<Game>,
    palette: Res<Palette>,
    time: Res<Time>,
    mut beans: Query<(&PieceBean, &mut MeshMaterial2d<ColorMaterial>, &mut Visibility, &mut Transform)>,
) {
    for (bean_ref, mut material, mut visibility, mut transform) in &mut beans {
        let player = &game.players[bean_ref.p];
        let piece = player.piece.filter(|_| player.phase == Phase::Falling);
        let Some((x, y, bean)) = piece.map(|p| p.cells()[bean_ref.part]).filter(|c| c.1 < VISIBLE_H as i32) else {
            *visibility = Visibility::Hidden;
            continue;
        };
        material.0 = palette.bean(bean);
        transform.translation = cell_local(x as f32, y as f32).extend(2.0);
        // Pulse the pivot so you can see which bean the pair turns around.
        let pulse = if bean_ref.part == 0 { 1.0 + 0.05 * (time.elapsed_secs() * 8.0).sin() } else { 1.0 };
        transform.scale = Vec3::splat(pulse);
        *visibility = Visibility::Inherited;
    }
}

fn draw_preview(
    game: Res<Game>,
    palette: Res<Palette>,
    mut beans: Query<(&PreviewBean, &mut MeshMaterial2d<ColorMaterial>, &mut Visibility, &mut Transform)>,
) {
    for (slot, mut material, mut visibility, mut transform) in &mut beans {
        let Some(&(pivot, satellite)) = game.players[slot.p].queue.get(slot.slot) else { continue };
        material.0 = palette.bean(if slot.part == 0 { pivot } else { satellite });
        transform.scale = Vec3::splat(if slot.slot == 0 { 1.0 } else { 0.75 });
        *visibility = Visibility::Inherited;
    }
}

/// Incoming garbage shown as icons: a big red bean = 30, a big refugee = 6, a small one = 1.
fn pending_icons(mut n: u32) -> Vec<u32> {
    let mut icons = Vec::new();
    for size in [30, 6, 1] {
        while n >= size && icons.len() < 6 {
            icons.push(size);
            n -= size;
        }
    }
    icons
}

fn draw_pending(
    game: Res<Game>,
    palette: Res<Palette>,
    mut icons: Query<(&PendingIcon, &mut MeshMaterial2d<ColorMaterial>, &mut Visibility, &mut Transform)>,
) {
    let lists = [pending_icons(game.players[0].pending), pending_icons(game.players[1].pending)];
    for (icon, mut material, mut visibility, mut transform) in &mut icons {
        match lists[icon.p].get(icon.i) {
            Some(&size) => {
                material.0 = palette.bean(if size == 30 { Bean::Red } else { Bean::Garbage });
                transform.scale = Vec3::splat(if size == 1 { 0.55 } else { 0.85 });
                *visibility = Visibility::Inherited;
            }
            None => *visibility = Visibility::Hidden,
        }
    }
}

fn draw_hud(
    game: Res<Game>,
    mut score: Query<(&ScoreText, &mut Text2d), (Without<ChainText>, Without<ResultText>, Without<InfoText>)>,
    mut chain: Query<(&ChainText, &mut Text2d), (Without<ScoreText>, Without<ResultText>, Without<InfoText>)>,
    mut result: Query<(&ResultText, &mut Text2d), (Without<ScoreText>, Without<ChainText>, Without<InfoText>)>,
    mut info: Query<&mut Text2d, (With<InfoText>, Without<ScoreText>, Without<ChainText>, Without<ResultText>)>,
) {
    for (ScoreText(p), mut text) in &mut score {
        let pl = &game.players[*p];
        let name = if *p == 0 { "YOU" } else { "CPU" };
        text.0 = format!("{name}\n\nSCORE\n{:08}\n\nBEST CHAIN\n{}", pl.score, pl.best_chain);
    }
    for (ChainText(p), mut text) in &mut chain {
        let pl = &game.players[*p];
        text.0 = if pl.chain_banner.1 > 0.0 { format!("{} CHAIN!", pl.chain_banner.0) } else { String::new() };
    }
    for (ResultText(p), mut text) in &mut result {
        let pl = &game.players[*p];
        text.0 = if game.ready > 0.0 {
            if game.ready > 0.5 { "READY?" } else { "GO!" }.into()
        } else if !game.over {
            String::new()
        } else if game.mode == Mode::Solo {
            "GAME OVER".into()
        } else if pl.phase == Phase::Dead {
            "LOSE".into()
        } else {
            "WIN!".into()
        };
    }
    if let Ok(mut text) = info.single_mut() {
        let other = if game.mode == Mode::Versus { "Solo" } else { "Versus" };
        text.0 = if game.over {
            format!("Enter: play again     Tab: switch to {other}")
        } else {
            format!("Tab: switch to {other}")
        };
    }
}
