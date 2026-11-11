//! The sidecar's `ev` records → `MascotEvent`s (plan §2 mapping). Only changes
//! come out: the same event twice in a row is sent once. A fired reminder is
//! a one-off, not a state: it always comes out, and leaves the dedupe alone.

use std::collections::HashMap;

use mascot_protocol::{classify, MascotEvent};
use serde_json::Value;

/// Run failures the mascot shows as an error; the rest (an abort, a reset,
/// nothing to answer) end quietly.
const ERRORS: [&str; 6] = ["model_error", "no_model", "faulted", "restart_limit", "interrupted", "orphaned"];

#[derive(Default)]
pub struct Mapper {
    last: Option<MascotEvent>,
    /// The streamed answer of the current generation.
    text: String,
    busy: bool,
    /// Approval card titles, by call id.
    titles: HashMap<String, String>,
}

impl Mapper {
    pub fn new() -> Mapper {
        Mapper::default()
    }

    /// The card the policy put up for `call_id`.
    pub fn titled(&mut self, call_id: &str, title: &str) {
        self.titles.insert(call_id.into(), title.into());
    }

    /// The sidecar ended: whatever it showed is over.
    pub fn reset(&mut self) -> Option<MascotEvent> {
        self.busy = false;
        self.text.clear();
        self.emit(MascotEvent::Idle)
    }

    /// Whether the last status had a run.
    pub fn busy(&self) -> bool {
        self.busy
    }

    fn emit(&mut self, e: MascotEvent) -> Option<MascotEvent> {
        if self.last.as_ref() == Some(&e) {
            return None;
        }
        self.last = Some(e.clone());
        Some(e)
    }

    /// One `ev` record.
    pub fn on_record(&mut self, rec: &Value) -> Option<MascotEvent> {
        if rec["t"] != "ev" {
            return None;
        }
        match rec["kind"].as_str() {
            Some("text") => {
                if rec["reset"] == true {
                    self.text.clear();
                }
                self.text.push_str(rec["delta"].as_str().unwrap_or(""));
                (self.busy && !self.text.is_empty()).then(|| MascotEvent::Speaking { text: self.text.clone() }).and_then(|e| self.emit(e))
            }
            Some("entry") => {
                let e = &rec["entry"];
                if e["kind"] == "buddy.event" && e["data"]["type"] == "reminder_fired" {
                    let (id, text) = (e["data"]["id"].as_str().unwrap_or(""), e["data"]["text"].as_str().unwrap_or(""));
                    let set_at_ms = e["data"]["setAtMs"].as_u64();
                    return (!text.is_empty()).then(|| MascotEvent::Reminder { id: id.into(), text: text.into(), set_at_ms });
                }
                if e["kind"] == "buddy.event" && e["data"]["type"] == "approval_asked" {
                    if let (Some(id), Some(title)) = (e["data"]["callId"].as_str(), e["data"]["title"].as_str()) {
                        self.titled(id, title);
                    }
                }
                // A short answer can commit before any text streamed: the
                // committed answer speaks too (the sidecar sends entries before
                // the status that ends the run).
                if self.busy {
                    let said = answer_text(e);
                    if !said.is_empty() && said != self.text {
                        self.text = said;
                        return self.emit(MascotEvent::Speaking { text: self.text.clone() });
                    }
                }
                None
            }
            Some("status") => self.on_status(&rec["status"]),
            _ => None,
        }
    }

    fn on_status(&mut self, s: &Value) -> Option<MascotEvent> {
        let busy = s["busy"] == true;
        if busy && !self.busy {
            self.text.clear();
        }
        self.busy = busy;
        let first = |k: &str| s[k].as_array().and_then(|a| a.first()).cloned();
        let event = if let Some(id) = first("asking").and_then(|v| v.as_str().map(str::to_string)) {
            let title = self.titles.get(&id).cloned().unwrap_or_else(|| "Allow this?".into());
            MascotEvent::AwaitingPermission { call_id: id, title }
        } else if let Some(t) = first("tools") {
            let tool = t["name"].as_str().unwrap_or("").to_string();
            MascotEvent::Working { kind: classify(&tool, t["command"].as_str()), tool }
        } else if busy && s["phase"] == "speaking" && !self.text.is_empty() {
            MascotEvent::Speaking { text: self.text.clone() }
        } else if busy {
            // Between a call and the next request too: the model has the turn.
            self.text.clear();
            MascotEvent::Thinking
        } else {
            self.text.clear();
            let reason = s["failure"]["reason"].as_str().unwrap_or("");
            if ERRORS.contains(&reason) {
                let message = s["failure"]["detail"].as_str().map(str::to_string).unwrap_or_else(|| default_message(reason).into());
                MascotEvent::Error { reason: reason.into(), message }
            } else {
                MascotEvent::Idle
            }
        };
        self.emit(event)
    }
}

/// The text parts of an entry's assistant messages.
fn answer_text(entry: &Value) -> String {
    let msgs = entry["model"].as_array().into_iter().flatten().filter(|m| m["role"] == "assistant");
    msgs.flat_map(|m| m["content"].as_array().into_iter().flatten()).filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("")
}

fn default_message(reason: &str) -> &'static str {
    match reason {
        "model_error" => "The model returned an error.",
        "no_model" => "No model is chosen.",
        "interrupted" => "The run was cut off.",
        _ => "Something went wrong.",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mascot_protocol::WorkKind;
    use serde_json::json;

    fn status(s: Value) -> Value {
        json!({ "t": "ev", "seq": 1, "kind": "status", "status": s })
    }

    #[test]
    fn a_gated_buddy_call_maps_to_the_expected_sequence() {
        let mut m = Mapper::new();
        m.titled("c1", "Run buddy say?");
        let recs = [
            status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })),
            status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })),
            status(json!({ "busy": true, "phase": "tool", "tools": [], "asking": ["c1"] })),
            status(json!({ "busy": true, "phase": "tool", "tools": [{ "callId": "c1", "name": "bash", "command": "buddy say hi" }], "asking": [] })),
            status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })),
            json!({ "t": "ev", "kind": "text", "delta": "Do" }),
            json!({ "t": "ev", "kind": "text", "delta": "ne." }),
            status(json!({ "busy": true, "phase": "speaking", "tools": [], "asking": [] })),
            status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [] })),
        ];
        let got: Vec<MascotEvent> = recs.iter().filter_map(|r| m.on_record(r)).collect();
        assert_eq!(
            got,
            vec![
                MascotEvent::Thinking,
                MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run buddy say?".into() },
                MascotEvent::Working { kind: WorkKind::Buddy, tool: "bash".into() },
                MascotEvent::Thinking,
                MascotEvent::Speaking { text: "Do".into() },
                MascotEvent::Speaking { text: "Done.".into() },
                MascotEvent::Idle,
            ]
        );
    }

    #[test]
    fn failures_map_to_error_but_an_abort_is_idle() {
        let mut m = Mapper::new();
        m.on_record(&status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })));
        let e = m.on_record(&status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [], "failure": { "reason": "model_error", "detail": "400 bad" } })));
        assert_eq!(e, Some(MascotEvent::Error { reason: "model_error".into(), message: "400 bad".into() }));
        m.on_record(&status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })));
        let e = m.on_record(&status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [], "failure": { "reason": "aborted" } })));
        assert_eq!(e, Some(MascotEvent::Idle));
        // Text outside a run (a late record) doesn't make it speak.
        assert_eq!(m.on_record(&json!({ "t": "ev", "kind": "text", "delta": "late" })), None);
    }

    #[test]
    fn a_fired_reminder_maps_to_reminder_every_time_without_touching_the_dedupe() {
        let mut m = Mapper::new();
        assert_eq!(m.on_record(&status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [] }))), Some(MascotEvent::Idle));
        let fired = |id: &str| json!({ "t": "ev", "kind": "entry", "entry": { "kind": "buddy.event", "data": { "type": "reminder_fired", "id": id, "text": "stretch", "atMs": 1 } } });
        assert_eq!(m.on_record(&fired("r_1")), Some(MascotEvent::Reminder { id: "r_1".into(), text: "stretch".into(), set_at_ms: None }));
        assert_eq!(m.on_record(&fired("r_2")), Some(MascotEvent::Reminder { id: "r_2".into(), text: "stretch".into(), set_at_ms: None }));
        // p4: when it was set rides along; a reminder stored before p4 has none
        let mut with = fired("r_3");
        with["entry"]["data"]["setAtMs"] = json!(1_700_000_000_000u64);
        assert_eq!(m.on_record(&with), Some(MascotEvent::Reminder { id: "r_3".into(), text: "stretch".into(), set_at_ms: Some(1_700_000_000_000) }));
        // Still idle as far as the dedupe knows.
        assert_eq!(m.on_record(&status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [] }))), None);
        // Other buddy events map to nothing.
        let other = json!({ "t": "ev", "kind": "entry", "entry": { "kind": "buddy.event", "data": { "type": "taint_set", "source": "web_fetch" } } });
        assert_eq!(m.on_record(&other), None);
    }

    /// `host::EMOTIONS` (what the CLI validates) is exactly the protocol's enum. Emotes are pack data now
    /// (`buddy mascot list`), so there is no static emote list to pin.
    #[test]
    fn the_hosts_emotion_list_equals_the_protocol_enum() {
        use mascot_protocol::Emotion;
        // Exhaustive: a new variant fails to compile here until it is listed.
        let emotion_name = |e: Emotion| match e {
            Emotion::Neutral | Emotion::Happy | Emotion::Sad | Emotion::Surprised | Emotion::Thinking | Emotion::Sleepy => serde_json::to_value(e).unwrap().as_str().unwrap().to_string(),
        };
        let emotions: Vec<String> = [Emotion::Neutral, Emotion::Happy, Emotion::Sad, Emotion::Surprised, Emotion::Thinking, Emotion::Sleepy].into_iter().map(emotion_name).collect();
        assert_eq!(emotions, host::EMOTIONS);
        for s in host::EMOTIONS {
            assert!(serde_json::from_value::<Emotion>(json!(s)).is_ok(), "{s}");
        }
    }

    #[test]
    fn a_committed_answer_speaks_when_nothing_streamed() {
        let mut m = Mapper::new();
        m.on_record(&status(json!({ "busy": true, "phase": "thinking", "tools": [], "asking": [] })));
        let entry = json!({ "t": "ev", "kind": "entry", "entry": { "kind": "pi.assistant", "model": [{ "role": "assistant", "content": [{ "type": "text", "text": "Hi." }] }] } });
        assert_eq!(m.on_record(&entry), Some(MascotEvent::Speaking { text: "Hi.".into() }));
        assert_eq!(m.on_record(&entry), None, "the same answer once");
        assert_eq!(m.on_record(&status(json!({ "busy": false, "phase": "idle", "tools": [], "asking": [] }))), Some(MascotEvent::Idle));
        // After the run, an entry doesn't speak.
        assert_eq!(m.on_record(&entry), None);
    }
}
