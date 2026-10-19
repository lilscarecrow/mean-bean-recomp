//! Ports of Dr. Robotnik's Mean Bean Machine (Mega Drive, USA, `GM MK-1706 -00`).
//!
//! Each item names the original routine it reproduces. Behavior is checked frame for
//! frame against the real ROM by `tests/original_*.rs`, using RAM recordings from the
//! private harness (see `tests/support/ramrec.rs`). No ROM code or data lives here.

pub mod bean_order;
pub mod field;
pub mod pair;
pub mod rng;

pub use bean_order::{gen_bean_orders, BeanColor, BeanOrders};
pub use rng::Rng;
