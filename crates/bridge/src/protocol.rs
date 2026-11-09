// The JSONL records between Rust and the `buddy-agent` sidecar; mirrors
// agent/src/protocol.ts. Ported from 2market. One JSON object per line, split
// only on "\n": CR, U+2028 and U+0085 inside strings never end a record.

use std::collections::BTreeMap;
use std::io::BufRead;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const V: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ModelRef {
    pub provider: String,
    pub model: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModel {
    pub id: String,
    pub name: String,
    pub context_window: u64,
    pub max_tokens: u64,
    pub reasoning: bool,
    pub input: Vec<String>,
    /// pi-ai's per-model `compat` flags; tests set them for fake models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compat: Option<Value>,
}

/// An endpoint pi-ai lacks, or a debug base override; never a key.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDef {
    pub id: String,
    pub name: String,
    /// "openai-completions" | "openai-responses" | "anthropic-messages"
    pub api: String,
    pub base_url: String,
    pub models: Vec<ProviderModel>,
}

/// How the agent's commands reach the host API.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct HostApi {
    /// `http://127.0.0.1:<port>`.
    pub url: String,
    /// This sidecar's agent token; revoked when it exits.
    pub token: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Init {
    pub t: &'static str,
    pub v: u32,
    pub agent_id: String,
    /// The agent's working folder: its cwd, and where file rules resolve.
    pub work_dir: String,
    pub storage_path: String,
    pub skills_dir: String,
    pub model: Option<ModelRef>,
    pub providers: Vec<ProviderDef>,
    /// The whole environment of every shell command.
    pub shell_env: BTreeMap<String, String>,
    pub shell_path: String,
    pub platform: &'static str,
    pub host: Option<HostApi>,
    /// Open: `stepLimit`, `shutdownMs`, `decideMs`, `maxRetries`, `retryDelayMs`, `lastExit`.
    pub settings: Value,
}

pub fn platform() -> &'static str {
    if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(windows) {
        "windows"
    } else {
        "linux"
    }
}

/// The only accepted request id forms (agent/src/protocol.ts), and the source each names.
pub fn request_source(id: &str) -> Option<&'static str> {
    let (prefix, rest) = id.split_once(':')?;
    let free = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "_.:-".contains(c));
    match prefix {
        "u" if rest.len() == 36 && rest.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-') => Some("chat"),
        "continue" if free(rest) => Some("continue"),
        "retry" if free(rest) => Some("retry"),
        _ => None,
    }
}

/// A fresh chat request id: `u:<uuid v4>`.
pub fn new_request_id() -> String {
    let h = policy::random_hex(16);
    let v = format!("4{}", &h[13..16]);
    let var = format!("{:x}{}", (u8::from_str_radix(&h[16..17], 16).unwrap() & 0x3) | 0x8, &h[17..20]);
    format!("u:{}-{}-{}-{}-{}", &h[0..8], &h[8..12], v, var, &h[20..32])
}

/// Reads LF-terminated records. Anything that isn't a JSON object is skipped;
/// `None` at the end of the stream.
pub struct Records<R: BufRead> {
    input: R,
    buf: Vec<u8>,
}

impl<R: BufRead> Records<R> {
    pub fn new(input: R) -> Self {
        Records { input, buf: Vec::with_capacity(64 * 1024) }
    }
}

impl<R: BufRead> Iterator for Records<R> {
    type Item = Value;

    fn next(&mut self) -> Option<Value> {
        loop {
            self.buf.clear();
            match self.input.read_until(b'\n', &mut self.buf) {
                Ok(0) | Err(_) => return None,
                Ok(_) => {
                    if let Ok(v @ Value::Object(_)) = serde_json::from_slice::<Value>(&self.buf) {
                        return Some(v);
                    }
                }
            }
        }
    }
}

/// One record as a line.
pub fn line(v: &Value) -> Vec<u8> {
    let mut out = serde_json::to_vec(v).unwrap_or_default();
    out.push(b'\n');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bridge_splits_only_on_lf() {
        // U+2028 and U+0085 raw (JSON.stringify leaves them; a CR is always escaped), then escaped.
        let a = "{\"t\":\"ev\",\"text\":\"one\\rtwo\u{2028}three\u{0085}four\"}\n";
        let b = "{\"t\":\"ack\",\"id\":\"c-1\",\"text\":\"a\\r\\nb\\u2028\"}\n";
        let input = format!("{a}not json\n\n{b}{{\"t\":\"ready\"}}");
        let got: Vec<Value> = Records::new(std::io::Cursor::new(input.into_bytes())).collect();
        assert_eq!(got.len(), 3, "{got:?}");
        assert_eq!(got[0]["text"], "one\rtwo\u{2028}three\u{0085}four");
        assert_eq!(got[1]["text"], "a\r\nb\u{2028}");
        assert_eq!(got[2]["t"], "ready", "a last record without its LF still counts");
        // What Rust writes splits the same way.
        let l = line(&serde_json::json!({ "t": "cmd", "text": "x\u{2028}y\rz" }));
        assert_eq!(l.iter().filter(|b| **b == b'\n').count(), 1);
        assert_eq!(*l.last().unwrap(), b'\n');
    }

    #[test]
    fn request_ids_take_only_the_accepted_forms() {
        assert_eq!(request_source("u:0f8fad5b-d9cb-469f-a165-70867728950e"), Some("chat"));
        assert_eq!(request_source(&new_request_id()), Some("chat"));
        assert_eq!(request_source("continue:01HX.2"), Some("continue"));
        assert_eq!(request_source("retry:r-1"), Some("retry"));
        for bad in ["u:nope", "u:0F8FAD5B-D9CB-469F-A165-70867728950E", "answer:v1", "job:j-1", "continue:", "other:1", "plain"] {
            assert_eq!(request_source(bad), None, "{bad}");
        }
    }
}
