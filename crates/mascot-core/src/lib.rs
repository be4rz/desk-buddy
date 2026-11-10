//! The mascot brain (plan §3, design §4.2–§4.4): the pack loader, the `Brain`
//! that turns harness events and senses into what to show, and the
//! deterministic replay driver, plus the sense reflexes. No pixels and no window here.

pub mod attention;
pub mod brain;
pub mod pack;
pub mod reflex;
pub mod replay;
pub mod rng;

pub use attention::Budget;
pub use brain::{
    bubble_ms, AppCategory, Brain, BrainConfig, DndAction, EyeState, FrontApp, Intent, LocalTime, MouthState, NowPlaying, Placement, Poke,
    Senses, Show, TraceLine, Visibility, WinRect,
};
pub use pack::{load, ClipMeta, Dir, FaceMeta, Handoff, Ori, PackError, PackMeta, QuietPool, SetMeta, StripRef, Warmup};
pub use reflex::Reflexes;
pub use rng::Rng;
