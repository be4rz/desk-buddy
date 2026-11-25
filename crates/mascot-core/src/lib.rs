//! The mascot brain (plan §3, design §4.2–§4.4): the pack loader, the `Brain`
//! that turns harness events and senses into what to show, and the
//! deterministic replay driver, plus the sense reflexes. No pixels and no window here.

pub mod attention;
pub mod body;
pub mod brain;
pub mod pack;
pub mod persona;
pub mod reaction;
pub mod reflex;
pub mod replay;
pub mod rng;
pub mod room;
#[cfg(test)]
mod test_pack;

pub use attention::Budget;
pub use brain::{
    bubble_ms, AppCategory, Brain, BrainConfig, DndAction, EyeState, FrontApp, Intent, LocalTime, MouthState, NowPlaying, Placement, Poke,
    Senses, Show, TraceLine, Visibility, WinRect,
};
pub use persona::{Effective as EffectivePersona, PersonaMeta};
pub use pack::{load, load_report, ActivityMeta, BodyMeta, EmoteAct, EmoteMeta, ClipMeta, Dir, FaceMeta, Handoff, Ori, PackError, PackMeta, QuietPool, Role, SetFace, SetMeta, StationMeta, StripRef, Warmup};
pub use reaction::Cursor;
pub use reflex::Reflexes;
pub use rng::Rng;
pub use room::{FloorMeta, RoomError, RoomMeta, SceneMeta, VariantMeta};
