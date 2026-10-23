//! Sound requests. The game logic doesn't make noise: it asks for sounds (the
//! original's `PlaySound` / `StopSound`), and [`Game::sounds`](crate::game::Game::sounds)
//! hands over the requests each frame made. Whatever plays them sits outside the game's
//! state, so copying the game (rollback) never copies or replays audio.
//!
//! Offline, every frame's requests go straight to the player. Over the network, frames
//! can be played again (a wrong guess put right), so [`SoundLog`] remembers what each
//! recent frame already started: a replayed frame starts only what's new, and cancels
//! what it no longer asks for (a sound from a guess that didn't happen).
//!
//! Every request the ROM makes is made, in the same order: the tests check the engine's
//! copy of the 68k `sound_queue` against RAM each frame. No sound driver is ported yet;
//! the front end only logs what it's asked for.

use std::collections::VecDeque;

/// What the game asks of the sound driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Sound {
    /// `PlaySound` with this ID (music, effects and voice samples share the numbering;
    /// `StopSound` is $6F then $FE).
    Play(u8),
    /// A byte written straight to `sound_playing`, skipping the queue: `FadeSound` $FD,
    /// `PauseSound` / `UnpauseSound` $FF, `StopSFX` $6F.
    Command(u8),
}

/// What the sound player should do, for the frame that asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cue {
    pub frame: u32,
    pub sound: Sound,
    /// False: start it. True: a frame played again no longer asks for it (stop it if it
    /// can be, or ignore it).
    pub cancel: bool,
}

/// What recent frames started, so frames played again don't start things twice.
#[derive(Default)]
pub struct SoundLog {
    /// (frame, its requests), oldest first, for frames not yet confirmed.
    frames: VecDeque<(u32, Vec<Sound>)>,
    cues: Vec<Cue>,
}

impl SoundLog {
    /// Frame `frame` was played (again, maybe) and asked for `sounds`.
    pub fn played(&mut self, frame: u32, sounds: &[Sound]) {
        let before = match self.frames.iter_mut().find(|(f, _)| *f == frame) {
            Some((_, s)) => std::mem::replace(s, sounds.to_vec()),
            None => {
                self.frames.push_back((frame, sounds.to_vec()));
                vec![]
            }
        };
        // What's asked for now and wasn't before starts; what was and isn't any more is
        // cancelled (as many of each as asked for).
        let mut left = before.clone();
        for &s in sounds {
            match left.iter().position(|&b| b == s) {
                Some(i) => {
                    left.swap_remove(i);
                }
                None => self.cues.push(Cue { frame, sound: s, cancel: false }),
            }
        }
        self.cues.extend(left.into_iter().map(|sound| Cue { frame, sound, cancel: true }));
    }

    /// Frames before `frame` won't be played again.
    pub fn confirm(&mut self, frame: u32) {
        while self.frames.front().is_some_and(|(f, _)| *f < frame) {
            self.frames.pop_front();
        }
    }

    /// The cues since last time.
    pub fn take(&mut self) -> Vec<Cue> {
        std::mem::take(&mut self.cues)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const POP: Sound = Sound::Play(0x63);
    const VOICE: Sound = Sound::Play(0x6B);

    fn start(frame: u32, sound: Sound) -> Cue {
        Cue { frame, sound, cancel: false }
    }

    #[test]
    fn a_frame_played_again_starts_nothing_twice() {
        let mut log = SoundLog::default();
        log.played(5, &[POP]);
        assert_eq!(log.take(), [start(5, POP)]);
        log.played(5, &[POP]);
        assert_eq!(log.take(), []);
    }

    #[test]
    fn what_a_guess_asked_for_and_reality_doesnt_is_cancelled() {
        let mut log = SoundLog::default();
        log.played(5, &[POP, VOICE]);
        log.take();
        log.played(5, &[POP]);
        assert_eq!(log.take(), [Cue { frame: 5, sound: VOICE, cancel: true }]);
        // And the other way round.
        log.played(5, &[POP, POP]);
        assert_eq!(log.take(), [start(5, POP)]);
    }

    #[test]
    fn confirmed_frames_are_forgotten() {
        let mut log = SoundLog::default();
        log.played(1, &[POP]);
        log.played(2, &[POP]);
        log.confirm(2);
        assert_eq!(log.frames.len(), 1);
        // (frame 1 can't come back; if it did, it'd count as new)
        log.take();
        log.played(2, &[POP]);
        assert_eq!(log.take(), []);
    }
}
