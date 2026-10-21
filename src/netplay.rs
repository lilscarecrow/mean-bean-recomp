//! Playing over the network: the connection two copies of the game trade pad inputs
//! over. The engine is deterministic, so if both sides start from the same seed and feed
//! [`Game::frame`](crate::game::Game::frame) the same two pads every frame, they play the
//! same game; only the buttons cross the wire. [`rollback`](crate::rollback) plays the
//! frames: ours straight away, the other player's guessed until they arrive.
//!
//! Each side's input for frame `f` is the buttons it read `delay` frames earlier (0
//! unless the host asks for more: a little delay means fewer wrong guesses on a slow
//! connection). Every packet carries all the inputs the peer hasn't acknowledged yet,
//! so a lost packet costs nothing but the wait for the next one, and says how far along
//! the sender is, so the side that's ahead can wait for the other.
//!
//! The host is player 1 and picks the seed and the delay; whoever joins is player 2.
//! Both must run the same ROM (checked when they connect), and every [`CHECK_EVERY`]
//! frames each side sends a checksum of its game so a desync is caught, not played on.
//!
//! Packets (UDP): `"MBMN"`, version, kind, then
//! - hello (joiner to host): ROM hash (u64)
//! - welcome (host to joiner): seed (u32), delay (u8)
//! - reject: the reason, as text
//! - inputs: frames of ours the sender has (u32), first frame here (u32), checksum
//!   frame (u32) and value (u32), the sender's frame (u32) and how far it thinks it's
//!   ahead of us (i32), then one button byte per frame
//! - bye: nothing
//!
//! Numbers are big-endian.

use std::collections::VecDeque;
use std::io;
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

pub const DEFAULT_PORT: u16 = 7845;
/// Frames between reading a pad and playing it (rollback covers the wait for the
/// other player's).
pub const DEFAULT_DELAY: u8 = 0;
/// The most delay the host may ask for (half a second).
pub const MAX_DELAY: u8 = 30;
/// How often (in frames) the two sides compare checksums.
pub const CHECK_EVERY: u32 = 60;

const MAGIC: &[u8; 4] = b"MBMN";
const VERSION: u8 = 2;
const HELLO: u8 = 0;
const WELCOME: u8 = 1;
const REJECT: u8 = 2;
const INPUTS: u8 = 3;
const BYE: u8 = 4;

/// How often an unanswered hello is sent again.
const RESEND: Duration = Duration::from_millis(250);
/// Silence after which the peer counts as gone.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Most inputs in one packet (2 seconds' worth; a peer further behind than that waits).
const MAX_INPUTS: usize = 120;
/// Our own checksums kept to compare with the peer's.
const CHECKS_KEPT: usize = 16;

/// Where the connection is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum State {
    /// Hosting, nobody has joined yet.
    Waiting,
    /// Joining, the host hasn't answered yet.
    Connecting,
    Playing,
    /// Over, and why.
    Closed(String),
}

pub struct Session {
    sock: UdpSocket,
    peer: Option<SocketAddr>,
    host: bool,
    rom_hash: u64,
    seed: u32,
    delay: u8,
    state: State,
    /// Our buttons by frame, and the peer's (both start with `delay` frames of nothing).
    local: Vec<u8>,
    remote: Vec<u8>,
    /// How many of `local` the peer has.
    acked: u32,
    /// Our frame and how far ahead of the peer we think we are (sent), and the same
    /// from the peer.
    frame: u32,
    advantage: i32,
    peer_frame: u32,
    peer_advantage: i32,
    /// Our recent checksums (frame, value), and the peer's latest.
    checks: VecDeque<(u32, u32)>,
    peer_check: Option<(u32, u32)>,
    last_hello: Option<Instant>,
    last_heard: Instant,
    /// Tests: drop every nth packet we send.
    #[cfg(test)]
    lose_every: u32,
    #[cfg(test)]
    sent: u32,
}

impl Session {
    /// Hosts on `port` (all IPv4 interfaces) as player 1, starting the game from `seed`
    /// with `delay` frames of input delay.
    pub fn host(port: u16, rom_hash: u64, seed: u32, delay: u8) -> io::Result<Session> {
        let sock = UdpSocket::bind(("0.0.0.0", port))?;
        Self::new(sock, None, true, rom_hash, seed, delay.min(MAX_DELAY), State::Waiting)
    }

    /// Joins the host at `addr` ("ip:port", or just "ip" for the default port) as
    /// player 2.
    pub fn join(addr: &str, rom_hash: u64) -> io::Result<Session> {
        let peer = resolve(addr)?;
        let any: SocketAddr = if peer.is_ipv4() { "0.0.0.0:0" } else { "[::]:0" }.parse().unwrap();
        let sock = UdpSocket::bind(any)?;
        Self::new(sock, Some(peer), false, rom_hash, 0, 0, State::Connecting)
    }

    fn new(sock: UdpSocket, peer: Option<SocketAddr>, host: bool, rom_hash: u64, seed: u32, delay: u8, state: State) -> io::Result<Session> {
        sock.set_nonblocking(true)?;
        let mut s = Session {
            sock,
            peer,
            host,
            rom_hash,
            seed,
            delay: 0,
            state,
            local: vec![],
            remote: vec![],
            acked: 0,
            frame: 0,
            advantage: 0,
            peer_frame: 0,
            peer_advantage: 0,
            checks: VecDeque::new(),
            peer_check: None,
            last_hello: None,
            last_heard: Instant::now(),
            #[cfg(test)]
            lose_every: 0,
            #[cfg(test)]
            sent: 0,
        };
        s.set_delay(delay);
        Ok(s)
    }

    fn set_delay(&mut self, delay: u8) {
        self.delay = delay;
        self.local = vec![0; usize::from(delay)];
        self.remote = vec![0; usize::from(delay)];
        self.acked = u32::from(delay);
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The seed both sides start the game from (the host's; known to the joiner once
    /// playing).
    pub fn seed(&self) -> u32 {
        self.seed
    }

    pub fn delay(&self) -> u8 {
        self.delay
    }

    /// Our pad: 0 (player 1) when hosting, 1 when joined.
    pub fn side(&self) -> usize {
        if self.host { 0 } else { 1 }
    }

    /// The port we're on (useful when hosting on port 0).
    pub fn port(&self) -> io::Result<u16> {
        Ok(self.sock.local_addr()?.port())
    }

    pub fn peer(&self) -> Option<SocketAddr> {
        self.peer
    }

    /// Reads whatever has arrived. Call once a tick, before playing.
    pub fn receive(&mut self) {
        let mut buf = [0u8; 512];
        loop {
            match self.sock.recv_from(&mut buf) {
                Ok((n, from)) => self.handle(&buf[..n], from),
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                // (on some systems an ICMP "port unreachable" shows up here: the host
                // isn't up yet, or has gone; the timeout handles both)
                Err(_) => break,
            }
        }
        let quiet = matches!(self.state, State::Playing) || (!self.host && self.state == State::Connecting);
        if quiet && self.last_heard.elapsed() > TIMEOUT {
            self.close(if self.state == State::Playing { "the other player stopped answering" } else { "no answer from the host" });
        }
    }

    /// Records our buttons, read now, as our input `delay` frames on: call once for
    /// every frame played.
    pub fn push_local(&mut self, buttons: u8) {
        self.local.push(buttons);
    }

    /// Our buttons for `frame`, once recorded.
    pub fn local(&self, frame: u32) -> Option<u8> {
        self.local.get(frame as usize).copied()
    }

    /// The other player's buttons for `frame`, once they've arrived.
    pub fn remote(&self, frame: u32) -> Option<u8> {
        self.remote.get(frame as usize).copied()
    }

    /// How many frames of the other player's buttons have arrived (all of them, from
    /// frame 0: a gap is waited for).
    pub fn remote_len(&self) -> u32 {
        self.remote.len() as u32
    }

    /// The other player's latest buttons (nothing before any arrive): the best guess
    /// for the frames still to come.
    pub fn last_remote(&self) -> u8 {
        self.remote.last().copied().unwrap_or(0)
    }

    /// Where we are, for the peer: our frame and how far ahead of theirs we think it is.
    pub fn set_progress(&mut self, frame: u32, advantage: i32) {
        self.frame = frame;
        self.advantage = advantage;
    }

    /// The peer's frame and how far ahead of ours it thought it was, as last heard.
    pub fn peer_progress(&self) -> (u32, i32) {
        (self.peer_frame, self.peer_advantage)
    }

    /// Our checksum of the game after `frame` frames, to compare with the peer's.
    pub fn check(&mut self, frame: u32, value: u32) {
        if self.checks.len() == CHECKS_KEPT {
            self.checks.pop_front();
        }
        self.checks.push_back((frame, value));
        self.compare();
    }

    /// Sends what the peer needs. Call once a tick, after playing.
    pub fn send(&mut self) {
        let Some(peer) = self.peer else { return };
        match self.state {
            State::Connecting => {
                if self.last_hello.is_none_or(|t| t.elapsed() >= RESEND) {
                    self.last_hello = Some(Instant::now());
                    let mut p = header(HELLO);
                    p.extend_from_slice(&self.rom_hash.to_be_bytes());
                    self.send_to(&p, peer);
                }
            }
            State::Playing => {
                let mut p = header(INPUTS);
                let from = (self.acked as usize).min(self.local.len());
                let to = self.local.len().min(from + MAX_INPUTS);
                let (cf, cv) = self.checks.back().copied().unwrap_or((0, 0));
                for n in [self.remote.len() as u32, from as u32, cf, cv, self.frame, self.advantage as u32] {
                    p.extend_from_slice(&n.to_be_bytes());
                }
                p.extend_from_slice(&self.local[from..to]);
                self.send_to(&p, peer);
            }
            State::Waiting | State::Closed(_) => {}
        }
    }

    /// Tests: drops every `n`th packet we send (0: none).
    #[cfg(test)]
    pub(crate) fn lose_every(&mut self, n: u32) {
        self.lose_every = n;
    }

    /// Tells the peer we're leaving, and closes with `why`.
    pub fn leave(&mut self, why: &str) {
        if let (Some(peer), State::Playing | State::Connecting) = (self.peer, &self.state) {
            self.send_to(&header(BYE), peer);
        }
        self.close(why);
    }

    fn close(&mut self, why: &str) {
        if !matches!(self.state, State::Closed(_)) {
            self.state = State::Closed(why.into());
        }
    }

    fn send_to(&mut self, p: &[u8], to: SocketAddr) {
        #[cfg(test)]
        {
            self.sent += 1;
            if self.lose_every != 0 && self.sent.is_multiple_of(self.lose_every) {
                return;
            }
        }
        // (a full send buffer just loses the packet, like the network would)
        let _ = self.sock.send_to(p, to);
    }

    fn reject(&mut self, to: SocketAddr, why: &str) {
        let mut p = header(REJECT);
        p.extend_from_slice(why.as_bytes());
        self.send_to(&p, to);
    }

    fn handle(&mut self, p: &[u8], from: SocketAddr) {
        if p.len() < 6 || &p[..4] != MAGIC {
            return;
        }
        let (version, kind, body) = (p[4], p[5], &p[6..]);
        if self.host && kind == HELLO && matches!(self.state, State::Closed(_)) {
            return;
        }
        if self.host && kind == HELLO && self.peer.is_none_or(|peer| peer == from) {
            if version != VERSION {
                return self.reject(from, "a different version of the game");
            }
            let Some(hash) = body.get(..8) else { return };
            if u64::from_be_bytes(hash.try_into().unwrap()) != self.rom_hash {
                return self.reject(from, "a different ROM");
            }
            if self.state == State::Waiting {
                self.peer = Some(from);
                self.state = State::Playing;
                self.last_heard = Instant::now();
            }
            // (again for every hello: the last welcome may have been lost)
            let mut w = header(WELCOME);
            w.extend_from_slice(&self.seed.to_be_bytes());
            w.push(self.delay);
            self.send_to(&w, from);
            return;
        }
        if self.host && kind == HELLO {
            return self.reject(from, "the host already has a player");
        }
        if Some(from) != self.peer || version != VERSION {
            return;
        }
        self.last_heard = Instant::now();
        match kind {
            WELCOME if self.state == State::Connecting && body.len() >= 5 => {
                self.seed = u32::from_be_bytes(body[..4].try_into().unwrap());
                self.set_delay(body[4].min(MAX_DELAY));
                self.state = State::Playing;
            }
            REJECT if self.state == State::Connecting => {
                self.close(&format!("the host turned us away: {}", String::from_utf8_lossy(body)));
            }
            INPUTS if self.state == State::Playing && body.len() >= 24 => {
                let word = |i: usize| u32::from_be_bytes(body[i * 4..i * 4 + 4].try_into().unwrap());
                let (ack, start, cf, cv) = (word(0), word(1), word(2), word(3));
                // (packets can arrive out of order: keep the latest)
                if word(4) >= self.peer_frame {
                    (self.peer_frame, self.peer_advantage) = (word(4), word(5) as i32);
                }
                let inputs = &body[24..];
                self.acked = self.acked.max(ack.min(self.local.len() as u32));
                // Only what follows on from what we have (a gap is sent again).
                let have = self.remote.len();
                let start = start as usize;
                if start <= have && start + inputs.len() > have {
                    self.remote.extend_from_slice(&inputs[have - start..]);
                }
                if cf != 0 {
                    self.peer_check = Some((cf, cv));
                    self.compare();
                }
            }
            BYE if matches!(self.state, State::Playing | State::Connecting) => self.close("the other player left"),
            _ => {}
        }
    }

    fn compare(&mut self) {
        let Some((frame, theirs)) = self.peer_check else { return };
        if let Some(&(_, ours)) = self.checks.iter().find(|(f, _)| *f == frame)
            && ours != theirs
        {
            self.close(&format!("out of sync at frame {frame}"));
        }
    }
}

fn header(kind: u8) -> Vec<u8> {
    let mut p = MAGIC.to_vec();
    p.extend_from_slice(&[VERSION, kind]);
    p
}

fn resolve(addr: &str) -> io::Result<SocketAddr> {
    let with_port = |a: &str| a.to_socket_addrs().ok().and_then(|mut i| i.next());
    with_port(addr)
        .or_else(|| with_port(&format!("{addr}:{DEFAULT_PORT}")))
        .or_else(|| with_port(&format!("[{addr}]:{DEFAULT_PORT}")))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("can't find {addr}")))
}

/// This machine's address on its network (the one a route to the internet would
/// use), to tell the other player. Sends nothing.
pub fn local_address() -> Option<std::net::IpAddr> {
    let s = UdpSocket::bind("0.0.0.0:0").ok()?;
    s.connect("8.8.8.8:53").ok()?;
    Some(s.local_addr().ok()?.ip()).filter(|ip| !ip.is_unspecified())
}

/// FNV-1a: tells two ROMs (or two games' states) apart, nothing more.
pub fn hash(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xCBF2_9CE4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01B3))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_different_rom_is_turned_away() {
        let mut host = Session::host(0, 1, 0, DEFAULT_DELAY).unwrap();
        let mut join = Session::join(&format!("127.0.0.1:{}", host.port().unwrap()), 2).unwrap();
        for _ in 0..200 {
            join.send();
            host.receive();
            join.receive();
            if matches!(join.state(), State::Closed(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*host.state(), State::Waiting);
        assert!(matches!(join.state(), State::Closed(why) if why.contains("different ROM")), "{:?}", join.state());
    }

    #[test]
    fn a_desync_is_noticed() {
        let mut host = Session::host(0, 1, 0, 0).unwrap();
        let mut join = Session::join(&format!("127.0.0.1:{}", host.port().unwrap()), 1).unwrap();
        for _ in 0..200 {
            for s in [&mut host, &mut join] {
                s.receive();
                s.send();
            }
            if host.state() == &State::Playing && join.state() == &State::Playing {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        host.check(60, 1);
        join.check(60, 2);
        for _ in 0..200 {
            for s in [&mut host, &mut join] {
                s.send();
                s.receive();
            }
            if matches!(host.state(), State::Closed(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*host.state(), State::Closed("out of sync at frame 60".into()));
    }

    #[test]
    fn leaving_tells_the_other_side() {
        let mut host = Session::host(0, 1, 0, 0).unwrap();
        let mut join = Session::join(&format!("127.0.0.1:{}", host.port().unwrap()), 1).unwrap();
        for _ in 0..200 {
            for s in [&mut host, &mut join] {
                s.receive();
                s.send();
            }
            if join.state() == &State::Playing {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        join.leave("left");
        for _ in 0..200 {
            host.receive();
            if matches!(host.state(), State::Closed(_)) {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert_eq!(*host.state(), State::Closed("the other player left".into()));
    }

    #[test]
    fn junk_is_ignored() {
        let mut host = Session::host(0, 99, 0, 0).unwrap();
        let junk = UdpSocket::bind("127.0.0.1:0").unwrap();
        let to = ("127.0.0.1", host.port().unwrap());
        for p in [&b""[..], b"MBMN", b"MBMN\x01\x00", b"MBMN\x01\x03\x00", b"hello there", b"MBMN\x01\x00\x00\x00\x00\x00\x00\x00\x00\x01"] {
            junk.send_to(p, to).unwrap();
        }
        std::thread::sleep(Duration::from_millis(20));
        host.receive();
        assert_eq!(*host.state(), State::Waiting);
    }

    #[test]
    fn addresses_without_a_port_get_the_default() {
        assert_eq!(resolve("127.0.0.1").unwrap(), "127.0.0.1:7845".parse().unwrap());
        assert_eq!(resolve("127.0.0.1:9000").unwrap(), "127.0.0.1:9000".parse().unwrap());
        assert_eq!(resolve("::1").unwrap(), "[::1]:7845".parse().unwrap());
    }
}
