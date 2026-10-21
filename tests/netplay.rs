//! Two copies of the game, each driven through its own netplay session over localhost
//! with its own player's buttons, stay identical: same checksums along the way, same
//! picture at the end.

mod support;

use std::time::{Duration, Instant};

use bean_machine::game::Game;
use bean_machine::netplay::{self, Session, State};
use bean_machine::{pad, rom};

const FRAMES: u32 = 3000;

/// A player who taps START now and then (through the menus and into a match) and
/// otherwise mashes, differently on each side.
fn buttons(side: usize, tick: u32) -> u8 {
    if tick % 90 < 3 {
        return pad::START;
    }
    let r = (tick.wrapping_add(side as u32 * 977)).wrapping_mul(2_654_435_761) >> 24;
    [pad::LEFT, pad::RIGHT, pad::DOWN, pad::A, pad::B, 0, 0, 0][(r % 8) as usize]
}

#[test]
fn two_games_over_the_network_stay_in_step() {
    if !support::install_rom() {
        return;
    }
    let hash = netplay::hash(rom::get().unwrap());
    let mut host = Session::host(0, hash, 0x7FE7_B46A, netplay::DEFAULT_DELAY).unwrap();
    let join = Session::join(&format!("127.0.0.1:{}", host.port().unwrap()), hash).unwrap();
    host.send();
    let mut sides = [(host, None::<Game>, 0u32), (join, None, 0)];
    let deadline = Instant::now() + Duration::from_secs(120);
    let mut tick = 0;
    while sides.iter().any(|(_, _, f)| *f < FRAMES) {
        assert!(Instant::now() < deadline, "stuck at frames {} / {}", sides[0].2, sides[1].2);
        tick += 1;
        for (side, (net, game, frame)) in sides.iter_mut().enumerate() {
            net.receive();
            assert!(!matches!(net.state(), State::Closed(_)), "side {side}: {:?}", net.state());
            if *net.state() == State::Playing && game.is_none() {
                *game = Some(Game::boot(net.seed()).unwrap());
            }
            if let Some(g) = game.as_mut().filter(|_| *frame < FRAMES)
                && let Some(both) = net.step(*frame, buttons(side, tick))
            {
                g.frame(both);
                *frame += 1;
                if frame.is_multiple_of(netplay::CHECK_EVERY) {
                    net.check(*frame, g.checksum());
                }
            }
            net.send();
        }
    }
    // Let the last checksums cross.
    for _ in 0..50 {
        for (side, (net, _, _)) in sides.iter_mut().enumerate() {
            net.send();
            net.receive();
            assert!(!matches!(net.state(), State::Closed(_)), "side {side}: {:?}", net.state());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    let [a, b] = [&sides[0].1, &sides[1].1].map(|g| g.as_ref().unwrap());
    assert_eq!(a.checksum(), b.checksum());
    assert!(a.picture() == b.picture(), "the two pictures differ");
}

/// Taps START from power-on until the main menu takes input.
fn to_main_menu(g: &mut Game) {
    for t in 0..6000 {
        if g.main_menu_line().is_some() {
            return;
        }
        g.frame([if t % 8 < 2 { pad::START } else { 0 }, 0]);
    }
    panic!("never reached the main menu");
}

fn tap(g: &mut Game, b: u8) {
    g.frame([b, 0]);
    g.frame([0, 0]);
}

#[test]
fn the_network_line_is_only_there_when_asked_for() {
    if !support::install_rom() {
        return;
    }
    // As the ROM has it: the cursor stops at OPTIONS.
    let mut g = Game::boot(0x7FE7_B46A).unwrap();
    to_main_menu(&mut g);
    g.frame([0, 0]);
    for _ in 0..6 {
        tap(&mut g, pad::DOWN);
    }
    assert_eq!(g.main_menu_line(), Some(3));
    // With the extra line: one further, and START there leaves the menu up.
    let mut g = Game::boot(0x7FE7_B46A).unwrap();
    g.engine.network_line = true;
    to_main_menu(&mut g);
    g.frame([0, 0]);
    for _ in 0..6 {
        tap(&mut g, pad::DOWN);
    }
    assert_eq!(g.main_menu_line(), Some(bean_machine::actors::NETWORK_LINE));
    tap(&mut g, pad::START);
    assert!(g.engine.network_chosen);
    assert_eq!(g.main_menu_line(), Some(bean_machine::actors::NETWORK_LINE));
    // And back up to the ROM's lines.
    tap(&mut g, pad::UP);
    assert_eq!(g.main_menu_line(), Some(3));
}

#[test]
fn versus_starts_the_same_way_every_time() {
    if !support::install_rom() {
        return;
    }
    let a = Game::versus(0x1234_5678).unwrap();
    let b = Game::versus(0x1234_5678).unwrap();
    assert_eq!(a.engine.level_mode, 1, "not in 1P vs 2P");
    assert_eq!(a.checksum(), b.checksum());
    assert!(a.picture() == b.picture());
}
