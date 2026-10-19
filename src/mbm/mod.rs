//! Ports of Dr. Robotnik's Mean Bean Machine (Mega Drive, USA, `GM MK-1706 -00`).
//!
//! Each item names the original routine it reproduces. Behavior is checked frame for
//! frame against the real ROM by `tests/original_*.rs`, using RAM recordings from the
//! private harness (see `tests/support/mod.rs`). No ROM code, graphics or sound live here:
//! only the small rule tables and animation timings the logic needs, written out by hand.

pub mod actors;
pub mod ai;
pub mod anim;
pub mod bean_order;
pub mod field;
pub mod motion;
pub mod pair;
pub mod rng;

pub use bean_order::{gen_bean_orders, BeanColor, BeanOrders};
pub use rng::Rng;
