//! The network menus (not in the ROM): what NETWORK on the main menu opens. Host or
//! join, an address to type for joining, then a lobby where both players press START.
//!
//! The screens only decide what to show and what the player asked for ([`Action`]);
//! the front end owns the connection ([`netplay::Session`](crate::netplay::Session)).
//! The lobby runs on the inputs both sides trade in lockstep, so both machines see the
//! two players ready on the same frame and start the match together.

use crate::overlay::{self, Canvas, Rect, GLYPH_H};
use crate::pad;

/// What a key, button or click means here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Up,
    Down,
    Confirm,
    Back,
    /// A character typed (only the address field takes them).
    Char(char),
    Erase,
    /// A click at this picture pixel.
    Click(i32, i32),
}

/// What the player asked for, for the front end to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Host,
    Join(String),
    /// Stop hosting or joining.
    Cancel,
    /// Back to the main menu.
    Close,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Screen {
    /// HOST / JOIN / BACK.
    Choose,
    /// Waiting for player 2 on `port`; `addresses` to tell them.
    Host { port: u16, addresses: Vec<String> },
    /// The address to join, CONNECT, BACK.
    Join,
    Connecting,
    /// Something to say (couldn't connect, the other player left, ...), then OK.
    Notice(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Item {
    Field,
    Host,
    Join,
    Connect,
    Back,
    Cancel,
    Ok,
}

impl Item {
    fn label(self) -> &'static str {
        match self {
            Item::Field => "",
            Item::Host => "HOST A GAME",
            Item::Join => "JOIN A GAME",
            Item::Connect => "CONNECT",
            Item::Back => "BACK",
            Item::Cancel => "CANCEL",
            Item::Ok => "OK",
        }
    }
}

/// The panel the screens draw in.
pub const PANEL: Rect = Rect { x: 30, y: 36, w: 260, h: 152 };
const CX: i32 = PANEL.x + PANEL.w / 2;
const ITEMS_Y: i32 = 116;
const ITEM_STEP: i32 = 16;
const FIELD: Rect = Rect { x: PANEL.x + 20, y: ITEMS_Y - 4, w: PANEL.w - 40, h: 15 };
/// Longest address the field takes.
const ADDRESS_LEN: usize = 40;

pub struct NetMenu {
    pub screen: Screen,
    line: usize,
    /// The address being typed (kept between visits).
    pub address: String,
}

impl Default for NetMenu {
    fn default() -> Self {
        Self::new()
    }
}

impl NetMenu {
    pub fn new() -> NetMenu {
        NetMenu { screen: Screen::Choose, line: 0, address: String::new() }
    }

    /// Shows `screen`, with the first item chosen.
    pub fn show(&mut self, screen: Screen) {
        self.screen = screen;
        self.line = 0;
    }

    fn items(&self) -> &'static [Item] {
        match self.screen {
            Screen::Choose => &[Item::Host, Item::Join, Item::Back],
            Screen::Host { .. } | Screen::Connecting => &[Item::Cancel],
            Screen::Join => &[Item::Field, Item::Connect, Item::Back],
            Screen::Notice(_) => &[Item::Ok],
        }
    }

    /// Where item `i` is (for drawing and clicking).
    fn rect(&self, i: usize) -> Rect {
        let items = self.items();
        // (the field sits first; the buttons below it)
        let y = ITEMS_Y + i as i32 * ITEM_STEP + if items[0] == Item::Field && i > 0 { 8 } else { 0 };
        if items[i] == Item::Field {
            return FIELD;
        }
        let w = overlay::text_width(items[i].label(), 1) + 16;
        Rect { x: CX - w / 2, y: y - 4, w, h: GLYPH_H + 8 }
    }

    pub fn input(&mut self, input: Input) -> Option<Action> {
        let items = self.items();
        let typing = items[self.line] == Item::Field;
        match input {
            Input::Up => self.line = self.line.saturating_sub(1),
            Input::Down => self.line = (self.line + 1).min(items.len() - 1),
            Input::Char(c) if typing => {
                if self.address.len() < ADDRESS_LEN && (c.is_ascii_alphanumeric() || ".:-[]".contains(c)) {
                    self.address.push(c);
                }
            }
            Input::Erase if typing => {
                self.address.pop();
            }
            Input::Confirm => return self.choose(items[self.line]),
            Input::Back => {
                let back = items.iter().copied().find(|i| matches!(i, Item::Back | Item::Cancel | Item::Ok)).unwrap_or(Item::Back);
                return self.choose(back);
            }
            Input::Click(x, y) => {
                let hit = (0..items.len()).find(|&i| self.rect(i).contains(x, y))?;
                self.line = hit;
                if items[hit] != Item::Field {
                    return self.choose(items[hit]);
                }
            }
            Input::Char(_) | Input::Erase => {}
        }
        None
    }

    fn choose(&mut self, item: Item) -> Option<Action> {
        match (item, &self.screen) {
            (Item::Host, _) => return Some(Action::Host),
            (Item::Join, _) => self.show(Screen::Join),
            (Item::Field | Item::Connect, _) => {
                let address = self.address.trim().to_string();
                if address.is_empty() {
                    self.line = 0;
                } else {
                    return Some(Action::Join(address));
                }
            }
            (Item::Back, Screen::Choose) => return Some(Action::Close),
            (Item::Back, _) => {
                self.show(Screen::Choose);
                self.line = 1;
            }
            (Item::Cancel, Screen::Connecting) => {
                self.show(Screen::Join);
                return Some(Action::Cancel);
            }
            (Item::Cancel, _) => {
                self.show(Screen::Choose);
                return Some(Action::Cancel);
            }
            (Item::Ok, _) => self.show(Screen::Choose),
        }
        None
    }

    /// Draws the screen over `picture` (dimmed). `ticks` blinks the text cursor.
    pub fn draw(&self, picture: &mut [u8], ticks: u32) {
        let mut c = Canvas(picture);
        c.dim();
        c.panel(PANEL);
        c.text_centered(CX, PANEL.y + 10, "NETWORK PLAY", overlay::YELLOW, 2);
        let lines: Vec<(String, overlay::Rgb)> = match &self.screen {
            Screen::Choose => vec![("PLAY 1P VS 2P WITH SOMEONE".into(), overlay::WHITE), ("ON ANOTHER MACHINE.".into(), overlay::WHITE)],
            Screen::Host { port, addresses } => {
                let mut l = vec![("WAITING FOR PLAYER 2...".to_string(), overlay::GREEN), (format!("PORT {port} (UDP)"), overlay::WHITE)];
                match addresses.first() {
                    Some(a) => l.push((format!("YOUR ADDRESS: {a}"), overlay::WHITE)),
                    None => l.push(("SHARE YOUR IP ADDRESS".into(), overlay::WHITE)),
                }
                l
            }
            Screen::Join => vec![("THE HOST'S ADDRESS (IP OR IP:PORT):".into(), overlay::WHITE)],
            Screen::Connecting => vec![("CONNECTING TO".into(), overlay::GREEN), (self.address.clone(), overlay::WHITE)],
            Screen::Notice(why) => wrap(why, 38).into_iter().map(|l| (l, overlay::RED)).collect(),
        };
        for (i, (l, color)) in lines.iter().enumerate() {
            c.text_centered(CX, PANEL.y + 40 + i as i32 * 12, l, *color, 1);
        }
        for (i, item) in self.items().iter().enumerate() {
            let r = self.rect(i);
            let on = i == self.line;
            if *item == Item::Field {
                c.fill(r, [0, 0, 0]);
                c.frame(r, if on { overlay::YELLOW } else { overlay::GREY });
                let cursor = if on && (ticks / 20).is_multiple_of(2) { "_" } else { "" };
                c.text(r.x + 4, r.y + 4, &format!("{}{cursor}", self.address), overlay::WHITE, 1);
                continue;
            }
            if on {
                c.frame(r, overlay::YELLOW);
            }
            c.text_centered(CX, r.y + 4, item.label(), if on { overlay::YELLOW } else { overlay::WHITE }, 1);
        }
        let hint = if self.items().contains(&Item::Field) { "TYPE, ENTER TO CONNECT, ESC TO GO BACK" } else { "ARROWS, ENTER OR CLICK. ESC: BACK" };
        c.text_centered(CX, PANEL.y + PANEL.h - 14, hint, overlay::GREY, 1);
    }
}

/// Splits `s` into lines of at most `width` characters, at spaces where it can.
fn wrap(s: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![];
    for word in s.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.len() + 1 + word.len() <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.chars().take(width).collect()),
        }
    }
    lines
}

/// Once connected: each player presses START when ready (B takes it back); the match
/// starts when both are. Runs on the lockstep inputs, so it's the same on both sides.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lobby {
    pub ready: [bool; 2],
    held: [u8; 2],
}

impl Lobby {
    /// One frame with both pads; true when both players are ready.
    pub fn step(&mut self, pads: [u8; 2]) -> bool {
        for (side, &held) in pads.iter().enumerate() {
            let press = held & !self.held[side];
            self.held[side] = held;
            if press & pad::START != 0 {
                self.ready[side] = true;
            } else if press & pad::B != 0 {
                self.ready[side] = false;
            }
        }
        self.ready == [true, true]
    }

    /// For the desync check while in the lobby.
    pub fn checksum(&self) -> u32 {
        0x4C0B_0000 | u32::from(self.ready[0]) | u32::from(self.ready[1]) << 1 | u32::from(self.held[0]) << 8 | u32::from(self.held[1]) << 16
    }

    /// Draws the lobby over `picture` (dimmed); `side` is ours, `peer` the other player.
    pub fn draw(&self, picture: &mut [u8], side: usize, peer: &str) {
        let mut c = Canvas(picture);
        c.dim();
        c.panel(PANEL);
        c.text_centered(CX, PANEL.y + 10, "CONNECTED", overlay::GREEN, 2);
        c.text_centered(CX, PANEL.y + 36, &format!("WITH {peer}"), overlay::GREY, 1);
        for p in 0..2 {
            let who = format!("PLAYER {}{}", p + 1, if p == side { " (YOU)" } else { "" });
            let y = PANEL.y + 60 + p as i32 * 18;
            c.text(PANEL.x + 24, y, &who, overlay::WHITE, 1);
            let (state, color) = if self.ready[p] { ("READY!", overlay::GREEN) } else { ("PRESS START", overlay::YELLOW) };
            c.text(PANEL.x + PANEL.w - 24 - overlay::text_width(state, 1), y, state, color, 1);
        }
        c.text_centered(CX, PANEL.y + 108, "THE MATCH STARTS WHEN BOTH ARE READY.", overlay::WHITE, 1);
        c.text_centered(CX, PANEL.y + PANEL.h - 14, "B (X OR K): NOT READY. ESC: LEAVE", overlay::GREY, 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::{HEIGHT, WIDTH};

    #[test]
    fn host_from_the_first_screen() {
        let mut m = NetMenu::new();
        assert_eq!(m.input(Input::Confirm), Some(Action::Host));
    }

    #[test]
    fn join_takes_an_address_and_connects() {
        let mut m = NetMenu::new();
        m.input(Input::Down);
        assert_eq!(m.input(Input::Confirm), None);
        assert_eq!(m.screen, Screen::Join);
        // Connecting with nothing typed goes back to the field.
        m.input(Input::Down);
        assert_eq!(m.input(Input::Confirm), None);
        for c in "192.168.1.20:7845 ~x".chars() {
            m.input(Input::Char(c));
        }
        m.input(Input::Erase);
        assert_eq!(m.address, "192.168.1.20:7845");
        assert_eq!(m.input(Input::Confirm), Some(Action::Join("192.168.1.20:7845".into())));
    }

    #[test]
    fn typing_only_goes_into_the_field() {
        let mut m = NetMenu::new();
        m.input(Input::Char('1'));
        assert_eq!(m.address, "");
        m.show(Screen::Join);
        m.input(Input::Down);
        m.input(Input::Char('1'));
        assert_eq!(m.address, "");
    }

    #[test]
    fn escape_backs_out_step_by_step() {
        let mut m = NetMenu::new();
        m.show(Screen::Join);
        assert_eq!(m.input(Input::Back), None);
        assert_eq!(m.screen, Screen::Choose);
        assert_eq!(m.input(Input::Back), Some(Action::Close));
        m.show(Screen::Host { port: 7845, addresses: vec![] });
        assert_eq!(m.input(Input::Back), Some(Action::Cancel));
        assert_eq!(m.screen, Screen::Choose);
        m.show(Screen::Connecting);
        assert_eq!(m.input(Input::Back), Some(Action::Cancel));
        assert_eq!(m.screen, Screen::Join);
        m.show(Screen::Notice("the other player left".into()));
        assert_eq!(m.input(Input::Back), None);
        assert_eq!(m.screen, Screen::Choose);
    }

    #[test]
    fn clicking_a_button_chooses_it() {
        let mut m = NetMenu::new();
        let r = m.rect(1);
        assert_eq!(m.input(Input::Click(r.x + 2, r.y + 2)), None);
        assert_eq!(m.screen, Screen::Join);
        m.address = "10.0.0.2".into();
        let r = m.rect(1);
        assert_eq!(m.input(Input::Click(r.x + 2, r.y + 2)), Some(Action::Join("10.0.0.2".into())));
        // Off every button: nothing.
        assert_eq!(m.input(Input::Click(0, 0)), None);
    }

    #[test]
    fn items_fit_in_the_panel_and_dont_overlap() {
        let mut m = NetMenu::new();
        for s in [Screen::Choose, Screen::Join, Screen::Connecting, Screen::Notice("x".into())] {
            m.show(s);
            let rects: Vec<Rect> = (0..m.items().len()).map(|i| m.rect(i)).collect();
            for (i, r) in rects.iter().enumerate() {
                assert!(r.x >= PANEL.x && r.x + r.w <= PANEL.x + PANEL.w && r.y + r.h <= PANEL.y + PANEL.h - 16, "{:?} {r:?}", m.screen);
                for q in &rects[i + 1..] {
                    assert!(r.y + r.h <= q.y, "{:?}: {r:?} overlaps {q:?}", m.screen);
                }
            }
            let mut px = vec![0u8; WIDTH * HEIGHT * 4];
            m.draw(&mut px, 0);
        }
    }

    #[test]
    fn lobby_starts_when_both_press_start() {
        let mut l = Lobby::default();
        assert!(!l.step([pad::START, 0]));
        // Holding START isn't pressing it again; B takes ready back.
        assert!(!l.step([pad::START, pad::B]));
        assert!(!l.step([pad::B, 0]));
        assert_eq!(l.ready, [false, false]);
        assert!(!l.step([pad::START, 0]));
        assert!(!l.step([0, 0]));
        assert!(l.step([0, pad::START]));
    }

    #[test]
    fn long_notices_wrap() {
        let lines = wrap("the host turned us away: a different version of the game", 20);
        assert!(lines.iter().all(|l| l.len() <= 20));
        assert_eq!(lines.join(" "), "the host turned us away: a different version of the game");
    }
}
