//! Renders sound commands to a WAV file through the ported driver and chips:
//!
//!     cargo run --release --example render_sounds -- ROM CUES FRAMES OUT.wav
//!
//! CUES is a text file of "frame hex-id" lines (as `<recording>.sndrec.cues`); each ID
//! goes into `sound_playing` before its frame, as the harness's pokes do. Or
//!
//!     cargo run --release --example render_sounds -- ROM --game INPUTS FRAMES OUT.wav
//!
//! powers the game on and plays player 1's pad from a harness input script, rendering
//! the sounds the game asks for.

use bean_machine::audio::Audio;
use bean_machine::sound::Sound;
use bean_machine::{game::Game, pad};
use std::io::Write as _;

/// A harness input script: (frame, player 1's buttons) changes.
fn script(path: &str) -> Vec<(usize, u8)> {
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
    if args.len() < 5 {
        eprintln!("usage: render_sounds ROM CUES FRAMES OUT.wav");
        std::process::exit(2);
    }
    bean_machine::rom::install_from(std::path::Path::new(&args[1])).expect("ROM");
    if args[2] == "--game" {
        let changes = script(&args[3]);
        let frames: usize = args[4].parse().expect("frames");
        let mut game = Game::boot(0x7FE7_B46A).expect("game");
        let mut audio = Audio::new(44_100);
        let mut pcm: Vec<i16> = vec![];
        let mut held = 0;
        for f in 0..frames {
            if let Some(&(_, b)) = changes.iter().find(|(at, _)| *at == f) {
                held = b;
            }
            // (the engine takes over after the boot code's 8 frames)
            let sounds: Vec<Sound> = if f >= 9 {
                game.frame([held, 0]);
                if std::env::var_os("MBM_SOUND_LOG").is_some() && !game.sounds().is_empty() {
                    eprintln!("frame {f}: {:X?}", game.sounds());
                }
                game.sounds().to_vec()
            } else {
                vec![]
            };
            for &s in audio.frame(&sounds) {
                pcm.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
        }
        write_wav(&args[5], 44_100, &pcm);
        return;
    }
    let cues: std::collections::HashMap<usize, u8> = std::fs::read_to_string(&args[2])
        .expect("cues")
        .lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            Some((it.next()?.parse().ok()?, u8::from_str_radix(it.next()?, 16).ok()?))
        })
        .collect();
    let frames: usize = args[3].parse().expect("frames");
    let rate = 44_100;
    let mut audio = Audio::new(rate);
    let mut pcm: Vec<i16> = vec![];
    for f in 0..frames {
        // (heard from the next frame's vertical blank)
        let sounds: Vec<Sound> = cues.get(&(f + 1)).map(|&id| vec![Sound::Command(id)]).unwrap_or_default();
        for &s in audio.frame(&sounds) {
            pcm.push((s.clamp(-1.0, 1.0) * 32767.0) as i16);
        }
    }
    write_wav(&args[4], rate, &pcm);
}

fn write_wav(path: &str, rate: u32, pcm: &[i16]) {
    let mut out = std::fs::File::create(path).expect("out");
    let data = (pcm.len() * 2) as u32;
    out.write_all(b"RIFF").unwrap();
    out.write_all(&(36 + data).to_le_bytes()).unwrap();
    out.write_all(b"WAVEfmt ").unwrap();
    out.write_all(&16u32.to_le_bytes()).unwrap();
    out.write_all(&1u16.to_le_bytes()).unwrap();
    out.write_all(&2u16.to_le_bytes()).unwrap();
    out.write_all(&rate.to_le_bytes()).unwrap();
    out.write_all(&(rate * 4).to_le_bytes()).unwrap();
    out.write_all(&4u16.to_le_bytes()).unwrap();
    out.write_all(&16u16.to_le_bytes()).unwrap();
    out.write_all(b"data").unwrap();
    out.write_all(&data.to_le_bytes()).unwrap();
    for s in pcm {
        out.write_all(&s.to_le_bytes()).unwrap();
    }
}
