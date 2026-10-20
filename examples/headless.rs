//! Runs the game without a window and writes frames as PPM images (for checking the
//! picture). Usage:
//!   cargo run --release --example headless -- <rom> <frames> <every> <outdir> [level] [a_from]
//! plays a stage (`a_from`: from that frame on, tap A instead: name entry letters, the
//! game over countdown); `MBM_START_PC=1FC4` starts the game flow at that bytecode
//! address instead (the ending).
//!   MBM_BOOT_INPUTS=<script> cargo run --release --example headless -- <rom> <frames> <every> <outdir>
//! powers on and plays player 1's pad from a harness input script ("frame BUTTON,..." or
//! "frame -" lines; frame 0 is power-on; the engine takes over after frame 8, like the boot
//! code), naming frames by power-on frame.
use bean_machine::{game::Game, pad, rom};

/// The harness's input script: (frame, buttons) changes for player 1.
fn script(path: &str) -> Vec<(u32, u8)> {
    let mut out = vec![];
    for line in std::fs::read_to_string(path).expect("input script").lines() {
        let line = line.split('#').next().unwrap().trim();
        let Some((frame, rest)) = line.split_once(' ') else { continue };
        let p1 = rest.split('|').next().unwrap().trim();
        let mut b = 0;
        if p1 != "-" {
            for name in p1.split(',') {
                b |= match name.trim().to_uppercase().as_str() {
                    "UP" => pad::UP,
                    "DOWN" => pad::DOWN,
                    "LEFT" => pad::LEFT,
                    "RIGHT" => pad::RIGHT,
                    "A" => pad::A,
                    "B" => pad::B,
                    "C" => pad::C,
                    "START" => pad::START,
                    _ => 0,
                };
            }
        }
        out.push((frame.parse().unwrap(), b));
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    rom::install_from(std::path::Path::new(&args[1])).expect("ROM");
    let (frames, every): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let out = std::path::Path::new(&args[4]);
    std::fs::create_dir_all(out).unwrap();
    let boot = std::env::var("MBM_BOOT_INPUTS").ok().map(|p| script(&p));
    let level = args.get(5).and_then(|l| l.parse().ok()).unwrap_or(3);
    let a_from: u32 = args.get(6).and_then(|l| l.parse().ok()).unwrap_or(u32::MAX);
    let mut game = if boot.is_some() { Game::boot(0x7FE7_B46A) } else { Game::new(level, 2, 0, 0x1234_5678) }.expect("game");
    if let Some(pc) = std::env::var("MBM_START_PC").ok().and_then(|v| u32::from_str_radix(&v, 16).ok()) {
        game.engine.bytecode_pc = pc;
    }
    let first = if boot.is_some() { 9 } else { 0 };
    let mut held = 0u8;
    for f in first..frames {
        let p1 = match &boot {
            Some(changes) => {
                if let Some(&(_, b)) = changes.iter().find(|(at, _)| *at == f) {
                    held = b;
                }
                held
            }
            // Hold nothing; tap down now and then so pairs come down.
            None if f >= a_from => {
                if f % 16 == 0 {
                    pad::A
                } else {
                    0
                }
            }
            None if f % 4 == 0 => pad::DOWN,
            None => 0,
        };
        game.frame([p1, 0]);
        if f % every == 0 || f + 1 == frames {
            let rgba = game.picture();
            let mut ppm = "P6 320 224 255\n".to_string().into_bytes();
            for px in rgba.chunks(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write(out.join(format!("f{f:05}.ppm")), ppm).unwrap();
        }
    }
    println!("status after {frames} frames: {:?}", game.status);
}
