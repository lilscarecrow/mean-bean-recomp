//! Rollback (not in the ROM): playing a network game without waiting for the other
//! player. Each frame is played as soon as our own buttons are read, with the other
//! player's guessed (whatever they held last). The state before every frame not yet
//! confirmed is kept; when their real buttons arrive and a guess was wrong, the game goes
//! back to the state before that frame and plays forward again with what they really
//! pressed, all within one tick. So our own buttons answer as fast as offline, and the
//! other player's take effect when they arrive, the frames in between redone.
//!
//! Both sides play every frame the same way once its inputs are known, so the confirmed
//! game is the same on both; [`CHECK_EVERY`] frames apart, each sends a checksum of it to
//! be sure. A side that gets [`MAX_ROLLBACK`] frames past what it has heard from the
//! other waits, and a side that's ahead of the other skips a tick now and then, so the
//! two keep pace.
//!
//! Sounds stay out of the copied state: each frame played reports its requests to a
//! [`SoundLog`], which starts only what a replayed frame didn't already start (see
//! [`crate::sound`]).

use std::collections::VecDeque;
use std::sync::Arc;

use crate::game::{Game, Status};
use crate::netmenu::Lobby;
use crate::netplay::{Session, State, CHECK_EVERY};
use crate::sound::{Cue, Sound, SoundLog};

/// Furthest we play past the other player's last known buttons (12 frames: 200ms).
pub const MAX_ROLLBACK: u32 = 12;
/// Ticks between two waits to let the other side catch up (so waiting doesn't stutter).
const WAIT_EVERY: u32 = 4;

/// Something played frame by frame from two pads, that can be copied to go back to.
pub trait Sim: Clone {
    fn step(&mut self, pads: [u8; 2]);
    /// A fingerprint of the state, for the desync check.
    fn checksum(&self) -> u32;
    /// The sounds the last step asked for.
    fn sounds(&self) -> &[Sound] {
        &[]
    }
}

pub struct Rollback<S> {
    /// After `frame` frames (the latest ones played on guesses).
    state: S,
    frame: u32,
    /// Frames before `confirmed` were played with the other player's real buttons.
    confirmed: u32,
    /// For each frame from `confirmed` on: the state before it, and the other player's
    /// buttons it was played with.
    saved: VecDeque<(S, u8)>,
    ticks: u32,
    last_wait: u32,
    sound: SoundLog,
    /// Times we went back, and frames played again.
    pub rollbacks: u32,
    pub replayed: u32,
}

impl<S: Sim> Rollback<S> {
    pub fn new(state: S) -> Rollback<S> {
        Rollback { state, frame: 0, confirmed: 0, saved: VecDeque::new(), ticks: 0, last_wait: 0, sound: SoundLog::default(), rollbacks: 0, replayed: 0 }
    }

    /// The game as it stands (what to show).
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Frames played.
    pub fn frame(&self) -> u32 {
        self.frame
    }

    /// Frames played with both players' real buttons.
    pub fn confirmed(&self) -> u32 {
        self.confirmed
    }

    /// What the sound player should start (or cancel) since last asked.
    pub fn take_cues(&mut self) -> Vec<Cue> {
        self.sound.take()
    }

    /// One tick (after [`Session::receive`], before [`Session::send`]): puts right the
    /// frames played on wrong guesses, then plays the next frame with our `buttons`.
    pub fn tick(&mut self, net: &mut Session, buttons: u8) {
        if *net.state() != State::Playing {
            return;
        }
        self.ticks += 1;
        self.correct(net);
        let (peer_frame, peer_advantage) = net.peer_progress();
        let advantage = self.frame as i32 - peer_frame as i32;
        net.set_progress(self.frame, advantage);
        // Each side sees the other behind by the time a packet takes; the difference of
        // the two views is twice how far ahead we really are.
        if advantage - peer_advantage >= 2 && self.ticks - self.last_wait >= WAIT_EVERY {
            self.last_wait = self.ticks;
            return;
        }
        if self.frame - self.confirmed >= MAX_ROLLBACK {
            return;
        }
        net.push_local(buttons);
        self.play(net);
    }

    /// Goes back to the first frame played on a wrong guess (if any) and plays on from
    /// there with what's known now, then lets go of what's confirmed.
    fn correct(&mut self, net: &mut Session) {
        let known = net.remote_len().min(self.frame);
        let wrong = (self.confirmed..known).find(|&f| net.remote(f) != Some(self.saved[(f - self.confirmed) as usize].1));
        if let Some(f) = wrong {
            let i = (f - self.confirmed) as usize;
            let end = self.frame;
            self.state = self.saved[i].0.clone();
            self.saved.truncate(i);
            self.frame = f;
            while self.frame < end {
                self.play(net);
            }
            self.rollbacks += 1;
            self.replayed += end - f;
        }
        // The checksums of confirmed states, at every CHECK_EVERY frames.
        let mut n = (self.confirmed / CHECK_EVERY + 1) * CHECK_EVERY;
        while n <= known {
            let sum = if n == self.frame { self.state.checksum() } else { self.saved[(n - self.confirmed) as usize].0.checksum() };
            net.check(n, sum);
            n += CHECK_EVERY;
        }
        self.saved.drain(..(known - self.confirmed) as usize);
        self.confirmed = known;
        self.sound.confirm(known);
    }

    /// Plays frame `frame` with our buttons and the other player's (or the guess).
    fn play(&mut self, net: &Session) {
        let f = self.frame;
        let theirs = net.remote(f).unwrap_or_else(|| net.last_remote());
        let ours = net.local(f).unwrap_or(0);
        let pads = if net.side() == 0 { [ours, theirs] } else { [theirs, ours] };
        self.saved.push_back((self.state.clone(), theirs));
        self.state.step(pads);
        self.sound.played(f, self.state.sounds());
        self.frame += 1;
    }
}

impl Sim for Game {
    fn step(&mut self, pads: [u8; 2]) {
        self.frame(pads);
    }

    fn checksum(&self) -> u32 {
        Game::checksum(self)
    }

    fn sounds(&self) -> &[Sound] {
        Game::sounds(self)
    }
}

/// A network game: the lobby until both players are ready, then the match, which
/// starts from `start` (1P vs 2P, made once when connecting: [`Game::versus`] plays
/// thousands of frames, too many to do again when going back).
#[derive(Clone)]
pub struct Online {
    pub lobby: Lobby,
    pub game: Option<Game>,
    start: Arc<Game>,
    linger: u32,
    /// The last step's sounds (none in the lobby).
    sounds: Vec<Sound>,
}

impl Online {
    pub fn new(start: Game) -> Online {
        Online { lobby: Lobby::default(), game: None, start: Arc::new(start), linger: 0, sounds: vec![] }
    }
}

impl Sim for Online {
    fn step(&mut self, pads: [u8; 2]) {
        self.sounds.clear();
        match &mut self.game {
            None => {
                if self.lobby.step(pads) {
                    self.game = Some((*self.start).clone());
                }
            }
            Some(g) => {
                g.frame(pads);
                self.sounds.extend_from_slice(g.sounds());
                if let Status::Stopped(_) = g.status {
                    // Something not ported yet: say so for a second, then back to the
                    // lobby.
                    self.linger += 1;
                    if self.linger > 60 {
                        self.game = None;
                        self.lobby = Lobby::default();
                        self.linger = 0;
                    }
                }
            }
        }
    }

    fn checksum(&self) -> u32 {
        self.game.as_ref().map_or(self.lobby.checksum(), Game::checksum)
    }

    fn sounds(&self) -> &[Sound] {
        &self.sounds
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A game that remembers every pair of pads it was played with, and asks for a
    /// sound on the frames the two pads' low bits differ.
    #[derive(Clone, Default)]
    struct Toy {
        played: Vec<[u8; 2]>,
        sounds: Vec<Sound>,
    }

    fn toy_sounds(pads: [u8; 2]) -> Vec<Sound> {
        if (pads[0] ^ pads[1]) & 1 != 0 { vec![Sound::Play(pads[1])] } else { vec![] }
    }

    impl Sim for Toy {
        fn step(&mut self, pads: [u8; 2]) {
            self.played.push(pads);
            self.sounds = toy_sounds(pads);
        }

        fn sounds(&self) -> &[Sound] {
            &self.sounds
        }

        fn checksum(&self) -> u32 {
            self.played.iter().fold(7u32, |h, p| h.wrapping_mul(31).wrapping_add(u32::from(p[0]) << 8 | u32::from(p[1])))
        }
    }

    fn connect(delay: u8, lose_every: u32) -> (Session, Session) {
        let mut host = Session::host(0, 7, 0xDEAD_BEEF, delay).unwrap();
        let mut join = Session::join(&format!("127.0.0.1:{}", host.port().unwrap()), 7).unwrap();
        for _ in 0..500 {
            for s in [&mut host, &mut join] {
                s.receive();
                s.send();
            }
            if *host.state() == State::Playing && *join.state() == State::Playing {
                host.lose_every(lose_every);
                join.lose_every(lose_every);
                return (host, join);
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        panic!("never connected");
    }

    /// Buttons that change often, differently on each side.
    fn buttons(side: usize, tick: u32) -> u8 {
        ((tick / 3).wrapping_mul(2_654_435_761).wrapping_add(side as u32 * 977) >> 24) as u8
    }

    /// Plays both sides until each has `frames` confirmed frames; returns them.
    fn play(frames: u32, delay: u8, lose_every: u32) -> [Rollback<Toy>; 2] {
        let (host, join) = connect(delay, lose_every);
        let mut sides = [(host, Rollback::new(Toy::default())), (join, Rollback::new(Toy::default()))];
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut tick = 0;
        while sides.iter().any(|(_, r)| r.confirmed() < frames) {
            assert!(Instant::now() < deadline, "stuck at {} / {} confirmed", sides[0].1.confirmed(), sides[1].1.confirmed());
            tick += 1;
            for (side, (net, r)) in sides.iter_mut().enumerate() {
                net.receive();
                r.tick(net, buttons(side, tick));
                net.send();
                assert!(!matches!(net.state(), State::Closed(_)), "side {side}: {:?}", net.state());
                // Never further ahead than allowed.
                assert!(r.frame() - r.confirmed() <= MAX_ROLLBACK);
            }
            if tick % 2 == 0 {
                std::thread::sleep(Duration::from_micros(300));
            }
        }
        let [(hn, h), (jn, j)] = sides;
        // What each side played, once confirmed, is what both really pressed.
        for (net, r) in [(&hn, &h), (&jn, &j)] {
            for f in 0..frames {
                let (ours, theirs) = (net.local(f).unwrap(), net.remote(f).unwrap());
                let real = if net.side() == 0 { [ours, theirs] } else { [theirs, ours] };
                assert_eq!(r.state().played[f as usize], real, "side {} frame {f}", net.side());
            }
        }
        assert_eq!(h.state().played[..frames as usize], j.state().played[..frames as usize]);
        [h, j]
    }

    #[test]
    fn both_sides_end_up_playing_the_same_game() {
        let [h, j] = play(600, 0, 0);
        // The guesses were wrong now and then, and put right.
        assert!(h.rollbacks + j.rollbacks > 0);
    }

    #[test]
    fn each_frame_sounds_once_whatever_was_guessed() {
        let [mut h, mut j] = play(600, 0, 3);
        let mut cancelled = false;
        for r in [&mut h, &mut j] {
            let cues = r.take_cues();
            // Started minus cancelled, frame by frame, is what the real game asked for.
            for f in 0..r.confirmed() {
                let mut net: Vec<Sound> = vec![];
                for c in cues.iter().filter(|c| c.frame == f) {
                    if c.cancel {
                        let i = net.iter().position(|s| *s == c.sound).expect("cancelled something never started");
                        net.swap_remove(i);
                    } else {
                        net.push(c.sound);
                    }
                }
                assert_eq!(net, toy_sounds(r.state().played[f as usize]), "frame {f}");
            }
            cancelled |= cues.iter().any(|c| c.cancel);
        }
        assert!(cancelled, "no guess ever sounded wrong: not much of a test");
    }

    #[test]
    fn lost_packets_only_mean_more_going_back() {
        play(600, 0, 3);
    }

    #[test]
    fn with_some_input_delay_too() {
        play(300, 2, 0);
    }

    #[test]
    fn our_buttons_count_on_the_next_frame_without_waiting() {
        let (mut host, _join) = connect(0, 0);
        let mut r = Rollback::new(Toy::default());
        // The other side is silent: we still play, on guesses, up to MAX_ROLLBACK (now
        // and then skipping a tick, as we're ahead), each frame with the buttons just
        // read.
        for i in 0..MAX_ROLLBACK * 2 {
            host.receive();
            let before = r.frame();
            r.tick(&mut host, 0x40 | i as u8);
            if r.frame() > before {
                assert_eq!(r.state().played.last().unwrap()[0], 0x40 | i as u8);
            }
        }
        assert_eq!(r.frame(), MAX_ROLLBACK);
    }

    #[test]
    fn a_wrong_guess_is_put_right() {
        let (mut host, mut join) = connect(0, 0);
        let mut h = Rollback::new(Toy::default());
        let mut j = Rollback::new(Toy::default());
        // The host plays 5 frames guessing the joiner holds nothing...
        for _ in 0..5 {
            host.receive();
            h.tick(&mut host, 0);
            host.send();
        }
        assert!(h.state().played.iter().all(|p| *p == [0, 0]));
        let guessed = h.frame();
        // ...but the joiner held START from frame 0.
        for _ in 0..5 {
            join.receive();
            j.tick(&mut join, 0x80);
            join.send();
        }
        std::thread::sleep(Duration::from_millis(20));
        host.receive();
        h.tick(&mut host, 0);
        assert_eq!(h.rollbacks, 1);
        assert_eq!(h.replayed, guessed);
        assert!(h.state().played.iter().all(|p| *p == [0, 0x80]), "{:?}", h.state().played);
    }
}
