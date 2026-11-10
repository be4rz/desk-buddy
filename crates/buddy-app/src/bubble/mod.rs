//! The speech bubble (design `## talk`, §4.8): `model` is the pure state machine, `window` the
//! Tauri webview window and the page's commands.

pub mod model;
pub mod window;

pub use model::{Bubble, Out};
