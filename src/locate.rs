//! The ROM screen (not in the ROM): what the front end shows at startup when it can't
//! find the player's ROM, or the one it found won't do. Drop the file on the window,
//! type its path or BROWSE for it; once it's in, the path is remembered ([`save`]) so
//! later runs start straight away.
//!
//! Like [`netmenu`](crate::netmenu), the screen only decides what to show and what the
//! player asked for ([`Action`]); the front end opens the file picker and installs the
//! ROM.

use std::path::{Path, PathBuf};

use crate::netmenu::Input;
use crate::overlay::{self, Canvas, Rect, GLYPH_H, GLYPH_W};

/// What the player asked for, for the front end to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Open the system's file picker.
    Browse,
    /// Try the ROM at this path.
    Load(PathBuf),
    Quit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Field,
    Browse,
    Start,
    Quit,
}

const ITEMS: [Item; 4] = [Item::Field, Item::Browse, Item::Start, Item::Quit];

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Field => "",
            Item::Browse => "BROWSE...",
            Item::Start => "START",
            Item::Quit => "QUIT",
        }
    }
}

const PANEL: Rect = Rect { x: 16, y: 20, w: 288, h: 184 };
const CX: i32 = PANEL.x + PANEL.w / 2;
const FIELD: Rect = Rect { x: PANEL.x + 12, y: 92, w: PANEL.w - 24, h: 15 };
const BUTTONS_Y: i32 = 136;
const BUTTON_STEP: i32 = 16;
/// Longest path the field takes.
const PATH_LEN: usize = 260;

pub struct Locate {
    /// The path in the field.
    pub path: String,
    /// Why the last try didn't work.
    pub error: Option<String>,
    /// The file picker is open (the screen waits for it).
    pub browsing: bool,
    line: usize,
}

impl Locate {
    /// The screen, with `path` (the one that didn't work, if any) in the field and
    /// `error` saying why.
    pub fn new(path: Option<&Path>, error: Option<String>) -> Locate {
        let path = path.map(|p| p.display().to_string()).unwrap_or_default();
        // (the cursor starts on BROWSE: most players won't type a path)
        Locate { path, error, browsing: false, line: 1 }
    }

    /// Where item `i` is (for drawing and clicking).
    fn rect(&self, i: usize) -> Rect {
        if ITEMS[i] == Item::Field {
            return FIELD;
        }
        let w = overlay::text_width(ITEMS[i].label(), 1) + 16;
        Rect { x: CX - w / 2, y: BUTTONS_Y + (i as i32 - 1) * BUTTON_STEP - 4, w, h: GLYPH_H + 8 }
    }

    /// A file dropped on the window: tried straight away.
    pub fn dropped(&mut self, path: &Path) -> Action {
        self.path = path.display().to_string();
        Action::Load(path.to_path_buf())
    }

    pub fn input(&mut self, input: Input) -> Option<Action> {
        if self.browsing {
            return None;
        }
        let typing = ITEMS[self.line] == Item::Field;
        match input {
            Input::Up => self.line = self.line.saturating_sub(1),
            Input::Down => self.line = (self.line + 1).min(ITEMS.len() - 1),
            Input::Char(c) if typing => {
                if self.path.chars().count() < PATH_LEN {
                    self.path.push(c);
                }
            }
            Input::Erase if typing => {
                self.path.pop();
            }
            Input::Confirm => return self.choose(ITEMS[self.line]),
            Input::Back => return Some(Action::Quit),
            Input::Click(x, y) => {
                let hit = (0..ITEMS.len()).find(|&i| self.rect(i).contains(x, y))?;
                self.line = hit;
                if ITEMS[hit] != Item::Field {
                    return self.choose(ITEMS[hit]);
                }
            }
            Input::Char(_) | Input::Erase => {}
        }
        None
    }

    fn choose(&mut self, item: Item) -> Option<Action> {
        match item {
            Item::Browse => Some(Action::Browse),
            Item::Field | Item::Start => {
                // (pasted paths often come quoted)
                let path = self.path.trim().trim_matches('"');
                if path.is_empty() {
                    self.line = 0;
                    self.error = Some("type the ROM's path first, or BROWSE for it".into());
                    None
                } else {
                    Some(Action::Load(PathBuf::from(path)))
                }
            }
            Item::Quit => Some(Action::Quit),
        }
    }

    /// Draws the screen on `picture` (cleared first). `ticks` blinks the text cursor.
    pub fn draw(&self, picture: &mut [u8], ticks: u32) {
        for px in picture.chunks_mut(4) {
            px.copy_from_slice(&[0, 0, 0, 0xFF]);
        }
        let mut c = Canvas(picture);
        c.panel(PANEL);
        c.text_centered(CX, PANEL.y + 10, "MEAN BEAN MACHINE", overlay::YELLOW, 2);
        let intro = [
            "THIS NEEDS YOUR OWN COPY OF THE GAME:",
            "THE MEGA DRIVE ROM (USA, REV 00).",
            "DROP THE FILE ON THIS WINDOW, BROWSE",
            "FOR IT, OR TYPE ITS PATH BELOW.",
        ];
        for (i, l) in intro.iter().enumerate() {
            c.text_centered(CX, PANEL.y + 32 + i as i32 * 10, l, overlay::WHITE, 1);
        }
        for (i, item) in ITEMS.iter().enumerate() {
            let r = self.rect(i);
            let on = i == self.line && !self.browsing;
            if *item == Item::Field {
                c.fill(r, [0, 0, 0]);
                c.frame(r, if on { overlay::YELLOW } else { overlay::GREY });
                let cursor = if on && (ticks / 20).is_multiple_of(2) { "_" } else { "" };
                // (the end of a long path: the file's name matters most)
                let room = ((r.w - 8) / GLYPH_W) as usize - 1;
                let chars: Vec<char> = self.path.chars().collect();
                let shown: String = chars[chars.len().saturating_sub(room)..].iter().collect();
                c.text(r.x + 4, r.y + 4, &format!("{shown}{cursor}"), overlay::WHITE, 1);
                continue;
            }
            if on {
                c.frame(r, overlay::YELLOW);
            }
            c.text_centered(CX, r.y + 4, item.label(), if on { overlay::YELLOW } else { overlay::WHITE }, 1);
        }
        let status: Vec<(String, overlay::Rgb)> = if self.browsing {
            vec![("CHOOSE THE ROM IN THE FILE PICKER...".into(), overlay::GREEN)]
        } else if let Some(e) = &self.error {
            crate::netmenu::wrap(e, 44).into_iter().take(2).map(|l| (l, overlay::RED)).collect()
        } else {
            vec![]
        };
        for (i, (l, color)) in status.iter().enumerate() {
            c.text_centered(CX, FIELD.y + FIELD.h + 4 + i as i32 * 9, l, *color, 1);
        }
        c.text_centered(CX, PANEL.y + PANEL.h - 12, "ARROWS, ENTER OR CLICK. ESC: QUIT", overlay::GREY, 1);
    }
}

/// Where the ROM's path is remembered: `bean-machine/rom_path.txt` in the user's
/// settings folder (`%APPDATA%` on Windows, `~/Library/Application Support` on macOS,
/// `$XDG_CONFIG_HOME` or `~/.config` elsewhere).
pub fn saved_file() -> Option<PathBuf> {
    let env = |k| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let base = if cfg!(windows) {
        env("APPDATA")
    } else if cfg!(target_os = "macos") {
        env("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        env("XDG_CONFIG_HOME").or_else(|| env("HOME").map(|h| h.join(".config")))
    }?;
    Some(base.join("bean-machine").join("rom_path.txt"))
}

/// The ROM's path from an earlier run, if one was saved.
pub fn saved() -> Option<PathBuf> {
    let s = std::fs::read_to_string(saved_file()?).ok()?;
    Some(PathBuf::from(s.trim())).filter(|p| !p.as_os_str().is_empty())
}

/// Remembers `path` for later runs (made absolute, so it works from anywhere).
pub fn save(path: &Path) -> Result<(), String> {
    let file = saved_file().ok_or("no settings folder to save the ROM's path in")?;
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let write = || -> std::io::Result<()> {
        std::fs::create_dir_all(file.parent().unwrap())?;
        std::fs::write(&file, path.display().to_string())
    };
    write().map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{HEIGHT, WIDTH};

    #[test]
    fn browse_is_first_and_start_needs_a_path() {
        let mut l = Locate::new(None, None);
        assert_eq!(l.input(Input::Confirm), Some(Action::Browse));
        l.input(Input::Down);
        assert_eq!(l.input(Input::Confirm), None);
        assert!(l.error.is_some());
        // (back on the field)
        for c in "\"C:\\roms\\mbm.bin\"".chars() {
            l.input(Input::Char(c));
        }
        assert_eq!(l.input(Input::Confirm), Some(Action::Load(PathBuf::from("C:\\roms\\mbm.bin"))));
    }

    #[test]
    fn a_path_that_failed_is_kept_to_fix() {
        let mut l = Locate::new(Some(Path::new("/roms/mbm.bi")), Some("no such file".into()));
        l.input(Input::Up);
        l.input(Input::Char('n'));
        assert_eq!(l.input(Input::Confirm), Some(Action::Load(PathBuf::from("/roms/mbm.bin"))));
    }

    #[test]
    fn nothing_while_the_picker_is_open() {
        let mut l = Locate::new(None, None);
        l.browsing = true;
        assert_eq!(l.input(Input::Confirm), None);
        assert_eq!(l.input(Input::Back), None);
    }

    #[test]
    fn dropping_a_file_tries_it() {
        let mut l = Locate::new(None, None);
        assert_eq!(l.dropped(Path::new("/x/mbm.md")), Action::Load(PathBuf::from("/x/mbm.md")));
        assert_eq!(l.path, PathBuf::from("/x/mbm.md").display().to_string());
    }

    #[test]
    fn clicking_and_escape() {
        let mut l = Locate::new(None, None);
        let r = l.rect(3);
        assert_eq!(l.input(Input::Click(r.x + 2, r.y + 2)), Some(Action::Quit));
        assert_eq!(l.input(Input::Click(0, 0)), None);
        assert_eq!(l.input(Input::Back), Some(Action::Quit));
    }

    #[test]
    fn items_fit_in_the_panel_and_dont_overlap() {
        let l = Locate::new(Some(Path::new(&"a/very/long/path/".repeat(20))), Some("an error ".repeat(20)));
        let rects: Vec<Rect> = (0..ITEMS.len()).map(|i| l.rect(i)).collect();
        for (i, r) in rects.iter().enumerate() {
            assert!(r.x >= PANEL.x && r.x + r.w <= PANEL.x + PANEL.w && r.y + r.h <= PANEL.y + PANEL.h - 14, "{r:?}");
            for q in &rects[i + 1..] {
                assert!(r.y + r.h <= q.y, "{r:?} overlaps {q:?}");
            }
        }
        // (two lines of error fit between the field and the first button)
        assert!(FIELD.y + FIELD.h + 4 + 2 * 9 <= rects[1].y);
        let mut px = vec![0u8; WIDTH * HEIGHT * 4];
        l.draw(&mut px, 0);
    }
}
