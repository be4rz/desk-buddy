//! What the bear remembers about the person (D18): a local SQLite fact store with deterministic
//! reconcile, the core section for the prompt, and the rules that keep it honest.
//!
//! Rust owns the store; the sidecar only makes the model calls. This crate never reads the clock
//! (time comes in as [`Now`] or a `Date`) and never logs: callers log `[buddy] memory <outcome> <id>`,
//! ids and counts only, never text, quotes or what the person said.

mod files;
pub mod predicates;
mod reconcile;
mod render;
mod schema;
mod secret;
mod store;
mod validate;
mod view;

pub use secret::looks_secret;
pub use store::Store;

use jiff::civil::Date;
use serde::Serialize;

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Why a call failed. The app maps these to the window's `Err` strings
/// (`secret|empty|too_long|not_found|store`) and the host to 400/404/503.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// A bad argument (the host's 400 `usage`).
    Usage(String),
    /// The text or value looks like a password or key (400 `secret`).
    Secret,
    NotFound,
    Empty,
    TooLong,
    /// The file was written by a newer version of the app: readable, never written.
    Newer,
    /// SQLite or the disk failed (503).
    Store(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Usage(m) => write!(f, "usage: {m}"),
            Error::Secret => f.write_str("looks like a secret"),
            Error::NotFound => f.write_str("not found"),
            Error::Empty => f.write_str("empty"),
            Error::TooLong => f.write_str("too long"),
            Error::Newer => f.write_str("memory was written by a newer version"),
            Error::Store(m) => write!(f, "store: {m}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<rusqlite::Error> for Error {
    fn from(e: rusqlite::Error) -> Error {
        Error::Store(e.to_string())
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::Store(e.to_string())
    }
}

/// The caller's clock: the local calendar day and the RFC 3339 timestamp.
#[derive(Debug, Clone)]
pub struct Now {
    pub today: Date,
    pub at: String,
}

impl Now {
    pub fn new(today: Date, at: impl Into<String>) -> Now {
        Now { today, at: at.into() }
    }
}

/// One remembered fact as `buddy memory list|search` and the window's rows show it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Fact {
    pub id: i64,
    pub kind: String,
    pub text: String,
    /// `pred` or `pred:qualifier`; None for a keyless note.
    pub key: Option<String>,
    pub value: Option<String>,
    /// `YYYY-MM`, when it became true.
    pub since: Option<String>,
    /// `window | chat | bear | guess`.
    pub source: &'static str,
}

/// What reconcile did with one memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Created(i64),
    Extended(i64),
    Superseded { new: i64, old: i64 },
    Disputed(i64),
    /// Not stored; the reason is a short word, never the text.
    Skipped(&'static str),
}

impl Outcome {
    /// The word for `[buddy] memory <outcome> <id>`.
    pub fn name(&self) -> &'static str {
        match self {
            Outcome::Created(_) => "created",
            Outcome::Extended(_) => "extended",
            Outcome::Superseded { .. } => "superseded",
            Outcome::Disputed(_) => "disputed",
            Outcome::Skipped(_) => "skipped",
        }
    }

    /// The row the outcome is about (the new one for a supersede).
    pub fn id(&self) -> Option<i64> {
        match *self {
            Outcome::Created(id) | Outcome::Extended(id) | Outcome::Disputed(id) => Some(id),
            Outcome::Superseded { new, .. } => Some(new),
            Outcome::Skipped(_) => None,
        }
    }
}

/// A `buddy memory remember` request.
#[derive(Debug, Clone, Default)]
pub struct RememberIn {
    /// 1–200 characters.
    pub text: String,
    /// `pred` or `pred:qualifier`. An unknown predicate makes a keyless note.
    pub key: Option<String>,
    /// Required with a known key.
    pub value: Option<String>,
    /// Defaults to the registry's kind for a known key, else `profile`.
    pub kind: Option<String>,
    /// 1–10, default 5.
    pub importance: Option<u8>,
    /// `YYYY-MM-DD` or `YYYY-MM`: when it became true.
    pub from: Option<String>,
}

/// One memory.extract item's result. Ids and reasons only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemOutcome {
    /// The request id of the turn it came from, if the reply named a known one.
    pub turn: Option<String>,
    pub outcome: Outcome,
}

/// What `apply_extract` did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Applied {
    pub items: Vec<ItemOutcome>,
    pub created: usize,
    pub extended: usize,
    pub superseded: usize,
    pub disputed: usize,
    pub skipped: usize,
    /// Pending turns cleared.
    pub cleared: usize,
}

impl Applied {
    pub(crate) fn push(&mut self, turn: Option<String>, outcome: Outcome) {
        match outcome {
            Outcome::Created(_) => self.created += 1,
            Outcome::Extended(_) => self.extended += 1,
            Outcome::Superseded { .. } => self.superseded += 1,
            Outcome::Disputed(_) => self.disputed += 1,
            Outcome::Skipped(_) => self.skipped += 1,
        }
        self.items.push(ItemOutcome { turn, outcome });
    }
}
