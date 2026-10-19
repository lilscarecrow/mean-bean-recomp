//! Runs a stage without a window and writes frames as PPM images (for checking the
//! picture). Usage: cargo run --release --example headless -- <rom> <frames> <every> <outdir>
use bean_machine::mbm::{game::Game, rom};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    rom::install_from(std::path::Path::new(&args[1])).expect("ROM");
    let (frames, every): (u32, u32) = (args[2].parse().unwrap(), args[3].parse().unwrap());
    let out = std::path::Path::new(&args[4]);
    std::fs::create_dir_all(out).unwrap();
    let mut game = Game::new(3, 2, 0, 0x1234_5678).expect("game");
    for f in 0..frames {
        // Hold nothing; tap down now and then so pairs come down.
        let p1 = if f % 4 == 0 { bean_machine::mbm::pad::DOWN } else { 0 };
        game.frame([p1, 0]);
        if f % every == 0 || f + 1 == frames {
            let rgba = game.picture();
            let mut ppm = format!("P6 320 224 255\n").into_bytes();
            for px in rgba.chunks(4) {
                ppm.extend_from_slice(&px[..3]);
            }
            std::fs::write(out.join(format!("f{f:05}.ppm")), ppm).unwrap();
        }
    }
    println!("status after {frames} frames: {:?}", game.status);
}
