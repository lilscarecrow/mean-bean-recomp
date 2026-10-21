//! Bean Machine: a recompilation of Dr. Robotnik's Mean Bean Machine (Mega Drive, USA,
//! `GM MK-1706 -00`) in Rust, with a Bevy front end (`src/main.rs`).
//!
//! Each item names the original routine it reproduces. Behavior is checked frame for
//! frame against the real ROM by `tests/original_*.rs`, using RAM recordings from the
//! private harness (see `tests/support/mod.rs`). No ROM code, graphics or sound live here:
//! only the small rule tables the logic needs, written out by hand. Data such as
//! animation scripts is read at runtime from the user's ROM ([`rom`]).

pub mod actors;
pub mod ai;
pub mod anim;
pub mod assets;
pub mod bean_order;
pub mod field;
pub mod game;
pub mod motion;
pub mod netmenu;
pub mod netplay;
pub mod overlay;
pub mod pad;
pub mod pair;
pub mod render;
pub mod rng;
pub mod rom;

pub use bean_order::{gen_bean_orders, BeanColor, BeanOrders};
pub use rng::Rng;
