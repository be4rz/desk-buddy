//! What the harness tells the mascot (plan §3): versioned and semantic, never a
//! clip name. The brain maps events to intents; packs map intents to clips.

use serde::{Deserialize, Serialize};

pub mod pack;

/// Bumped on any change a consumer must know about.
pub const VERSION: u32 = 2;

/// What kind of work a running tool call is, for `Working` (plan §2 mapping).
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum WorkKind {
    /// `buddy …`: the agent drives its own body or widgets.
    Buddy,
    /// `buddy music …`: media.
    Media,
    /// `git …`, `gh …`.
    Vcs,
    /// `read`, `write`, `edit`.
    Files,
    /// `curl`, `wget`.
    Web,
    /// Any other command.
    Shell,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MascotEvent {
    /// Nothing running.
    Idle,
    /// Waiting for the model.
    Thinking,
    /// A tool call the gate let through is running.
    Working {
        kind: WorkKind,
        /// The tool's name (`bash`, `read`, …).
        tool: String,
    },
    /// An approval card waits for the person.
    AwaitingPermission {
        #[serde(rename = "callId")]
        call_id: String,
        title: String,
    },
    /// Answer text so far, streamed into the bubble.
    Speaking { text: String },
    /// The run ended without an answer.
    Error { reason: String, message: String },
    // v2
    /// `buddy say` (proactive / out-of-turn line).
    Say { text: String, emotion: Option<Emotion> },
    /// `buddy mascot emote`.
    Emote { name: EmoteName },
    /// A durable reminder fired (P2b).
    Reminder { id: String, text: String },
    /// The person answered or dismissed what the bubble showed (clears ask/talk holds).
    Settled,
}

/// How a `Say` line feels; the brain plays it after the line.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Emotion {
    Neutral,
    Happy,
    Sad,
    Surprised,
    Thinking,
    Sleepy,
}

/// What `buddy mascot emote` can ask for.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum EmoteName {
    Wave,
    Nod,
    Shake,
    Jump,
    Spin,
}

/// The kind of work a shell command does, from its first word(s).
pub fn classify_command(command: &str) -> WorkKind {
    let mut words = command.split_whitespace().skip_while(|w| w.contains('=') && !w.starts_with('='));
    match (words.next(), words.next()) {
        (Some("buddy"), Some("music")) => WorkKind::Media,
        (Some("buddy"), _) => WorkKind::Buddy,
        (Some("git" | "gh"), _) => WorkKind::Vcs,
        (Some("curl" | "wget"), _) => WorkKind::Web,
        _ => WorkKind::Shell,
    }
}

/// The kind of work a tool call does.
pub fn classify(tool: &str, command: Option<&str>) -> WorkKind {
    match tool {
        "read" | "write" | "edit" | "look" => WorkKind::Files,
        "web_fetch" | "web_search" => WorkKind::Web,
        "bash" => classify_command(command.unwrap_or("")),
        _ => WorkKind::Shell,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_classify_by_their_first_words() {
        assert_eq!(classify_command("buddy say hi"), WorkKind::Buddy);
        assert_eq!(classify_command("  buddy music play lofi"), WorkKind::Media);
        assert_eq!(classify_command("git push"), WorkKind::Vcs);
        assert_eq!(classify_command("LANG=C git log"), WorkKind::Vcs);
        assert_eq!(classify_command("curl -s x"), WorkKind::Web);
        assert_eq!(classify_command("ls -la"), WorkKind::Shell);
        assert_eq!(classify("edit", None), WorkKind::Files);
    }

    #[test]
    fn events_serialize_tagged() {
        let e = MascotEvent::Working { kind: WorkKind::Buddy, tool: "bash".into() };
        assert_eq!(serde_json::to_value(&e).unwrap(), serde_json::json!({ "type": "working", "kind": "buddy", "tool": "bash" }));
        let a = MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run buddy?".into() };
        assert_eq!(serde_json::to_value(&a).unwrap()["callId"], "c1");
    }

    #[test]
    fn v2_events_round_trip() {
        let cases = [
            (MascotEvent::Say { text: "hi".into(), emotion: Some(Emotion::Happy) }, serde_json::json!({ "type": "say", "text": "hi", "emotion": "happy" })),
            (MascotEvent::Emote { name: EmoteName::Wave }, serde_json::json!({ "type": "emote", "name": "wave" })),
            (MascotEvent::Reminder { id: "r1".into(), text: "tea".into() }, serde_json::json!({ "type": "reminder", "id": "r1", "text": "tea" })),
            (MascotEvent::Settled, serde_json::json!({ "type": "settled" })),
        ];
        for (e, j) in cases {
            assert_eq!(serde_json::to_value(&e).unwrap(), j);
            assert_eq!(serde_json::from_value::<MascotEvent>(j).unwrap(), e);
        }
        let no_emotion: MascotEvent = serde_json::from_value(serde_json::json!({ "type": "say", "text": "x" })).unwrap();
        assert_eq!(no_emotion, MascotEvent::Say { text: "x".into(), emotion: None });
        assert_eq!(VERSION, 2);
    }
}
