//! Runs one agent's sidecar for the app: the JSONL bridge, its `cred.*` and
//! `policy.*` requests, a host API token per start, the running turn, restarts
//! with `lastExit`, and the `MascotEvent`s its records map to.

pub mod agent;
pub mod bridge;
pub mod credentials;
pub mod mascot;
pub mod protocol;
pub mod store;

use std::fmt;

pub use agent::{Agent, AgentEvent, AgentSpec};

/// A refused command or request, with a code the app acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeError {
    pub code: &'static str,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: &'static str, message: impl Into<String>) -> BridgeError {
        BridgeError { code, message: message.into() }
    }
}

impl fmt::Display for BridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for BridgeError {}

pub type Res<T> = Result<T, BridgeError>;
