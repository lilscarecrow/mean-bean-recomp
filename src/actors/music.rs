//! The stages' music: each level's intro, stage and win tune (`PlayLevelIntroMusic`,
//! `PlayLevelMusic`, `PlayLevelWinMusic`), and the danger tune while P1's field is
//! crowded in scenario mode (`HandleLevelMusic`). `cur_level_music` remembers what's
//! playing, so the same tune isn't started twice.

use super::*;
use crate::rom;

/// `LevelIntroMusicIDs`, `LevelMusicIDs`, `LevelWinMusicIDs`: per level.
const INTRO_MUSIC: u32 = 0x26A4;
const STAGE_MUSIC: u32 = 0x26DA;
const WIN_MUSIC: u32 = 0x2710;
const BGM_DANGER: u8 = 0x04;
/// P1's field turns dangerous at this many beans, and back under the other.
const DANGER_AT: u16 = 60;
const DANGER_OVER: u16 = 54;

impl Engine {
    fn level_tune(&self, table: u32) -> u8 {
        rom::byte(table + u32::from(self.level))
    }

    /// `PlayLevelIntroMusic`
    pub(super) fn play_level_intro_music(&mut self) {
        let id = self.level_tune(INTRO_MUSIC);
        self.play_sound(id);
    }

    /// `PlayLevelMusic`
    pub(super) fn play_level_music(&mut self) {
        self.play_new_tune(self.level_tune(STAGE_MUSIC));
    }

    /// `PlayLevelWinMusic`
    pub(super) fn play_level_win_music(&mut self) {
        self.play_new_tune(self.level_tune(WIN_MUSIC));
    }

    fn play_new_tune(&mut self, id: u8) {
        if id != self.level_music {
            self.level_music = id;
            self.play_sound(id);
        }
    }

    /// `HandleLevelMusic`: P1's, in scenario mode.
    pub(super) fn handle_level_music(&mut self, player: u8) {
        if self.level_mode != 0 || player != 0 {
            return;
        }
        let count = self.ai_fields[0].count();
        if self.level_music != BGM_DANGER {
            if count >= DANGER_AT {
                self.level_music = BGM_DANGER;
                self.play_sound(BGM_DANGER);
            }
        } else if count < DANGER_OVER {
            self.play_level_music();
        }
    }
}
