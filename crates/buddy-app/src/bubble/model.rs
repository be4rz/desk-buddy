//! What the speech bubble shows (design §4.8 `BubbleModel`), as a pure state machine: mascot
//! events, approval details, taint, `buddy say` lines and the person's clicks go in; a model to
//! show (or a hide) comes out. No Tauri, no clock: `now` is passed in. Unit-tested.

use std::collections::{HashMap, VecDeque};

use mascot_protocol::MascotEvent;
use serde::Serialize;
use serde_json::Value;

/// A finished bubble hides this long after its run ends (unless hovered).
pub const HIDE_MS: u64 = 6000;
/// While hovered, the hide is checked again after this long.
pub const HOVER_RECHECK_MS: u64 = 1000;

pub const TAINT_NOTE: &str = "This chat read a web page, so “Always” is off until you clear it (tray → Clear web taint).";

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Speech,
    Prompt,
    Card,
    Reminder,
    Error,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CardModel {
    pub call_id: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub can_always: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taint_note: Option<String>,
    /// "`index` of `total`" (additive, p3): this card's place among the run's queued cards.
    pub index: usize,
    pub total: usize,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct ErrorModel {
    pub reason: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}

/// §4.8 `BubbleModel`. `id` (additive, p3) is the shown reminder's id, for `reminder_snooze`.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BubbleModel {
    pub mode: Mode,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotion: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub card: Option<CardModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorModel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// The reminder chip's "set N min ago" (additive, p4); none for reminders stored before p4.
    #[serde(rename = "setMinAgo", skip_serializing_if = "Option::is_none")]
    pub set_min_ago: Option<u64>,
    /// When the reminder was set (epoch ms): `set_min_ago` is worked out from it each time the
    /// chip is shown, so a reminder that waited behind a card isn't stale.
    #[serde(skip)]
    pub set_at_ms: Option<u64>,
}

impl BubbleModel {
    fn new(mode: Mode) -> BubbleModel {
        BubbleModel { mode, text: None, streaming: None, emotion: None, card: None, error: None, id: None, set_min_ago: None, set_at_ms: None }
    }

    pub fn speech(text: String, streaming: bool, emotion: Option<String>) -> BubbleModel {
        BubbleModel { text: Some(text), streaming: Some(streaming), emotion, ..BubbleModel::new(Mode::Speech) }
    }

    pub fn prompt() -> BubbleModel {
        BubbleModel::new(Mode::Prompt)
    }

    pub fn error(reason: &str, message: &str) -> BubbleModel {
        // no model, or a keychain that can't be read: retrying can't help, the Sign in… window can
        let action = if matches!(reason, "no_model" | "keychain") { "sign_in" } else { "retry" };
        BubbleModel { error: Some(ErrorModel { reason: reason.into(), message: message.into(), action: Some(action.into()) }), ..BubbleModel::new(Mode::Error) }
    }

    /// The short stderr label: `speech streaming`, `card`, …
    pub fn label(&self) -> String {
        let mode = serde_json::to_value(self.mode).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_default();
        match self.streaming {
            Some(true) => format!("{mode} streaming"),
            _ => mode,
        }
    }
}

/// What the window should do.
#[derive(Clone, Debug, PartialEq)]
pub enum Out {
    Show(BubbleModel),
    Hide,
    /// Nothing visible changed.
    Keep,
}

#[derive(Default)]
pub struct Bubble {
    shown: Option<BubbleModel>,
    /// A turn is running (from a submit or the first busy event until Idle/Error).
    run: bool,
    /// The person stopped the turn (■): its remaining events stay quiet until its Idle.
    stopped: bool,
    /// The turn's streamed answer.
    text: String,
    /// The agent's own `buddy say` lines in this turn, shown above the answer.
    said: Vec<String>,
    /// Approval details by call id (`buddy.event approval_asked`), dropped when the run ends.
    approvals: HashMap<String, Value>,
    /// The card answered and not yet replaced by the run's next state.
    answered: Option<String>,
    /// Cards waiting for an answer, `(call_id, title)`; the front one is shown.
    cards: VecDeque<(String, String)>,
    /// Cards answered since the queue was last empty (the counter's offset).
    cards_done: usize,
    tainted: bool,
    hide_at: Option<u64>,
    last_prompt: Option<String>,
    /// The running (or last) turn is one the person sent from here: only its error offers
    /// Try again (which re-sends `last_prompt`); an unprompted turn's error offers Close only.
    ours: bool,
    /// Bubbles waiting for what the person has to act on (a card, the prompt, a reminder), in order.
    deferred: VecDeque<BubbleModel>,
    /// Reminder id → text, while shown.
    reminder: Option<(String, String)>,
    /// Wall clock (epoch ms) for the reminder chip, set by the caller before `event`.
    pub wall_ms: u64,
}

impl Bubble {
    pub fn new() -> Bubble {
        Bubble::default()
    }

    pub fn shown(&self) -> Option<&BubbleModel> {
        self.shown.as_ref()
    }

    pub fn hide_at(&self) -> Option<u64> {
        self.hide_at
    }

    pub fn last_prompt(&self) -> Option<&str> {
        self.last_prompt.as_deref()
    }

    /// The reminder shown now: `(id, text)`.
    pub fn reminder(&self) -> Option<&(String, String)> {
        self.reminder.as_ref()
    }

    /// The shown card for `call_id` offers "Always" (it is off while the chat is tainted).
    pub fn can_always(&self, call_id: &str) -> bool {
        matches!(&self.shown, Some(BubbleModel { card: Some(c), .. }) if c.call_id == call_id && c.can_always)
    }

    /// Dismissing what's shown now stops the turn: the streaming answer's ■.
    pub fn dismiss_stops(&self) -> bool {
        self.run && matches!(&self.shown, Some(m) if m.mode == Mode::Speech && m.streaming == Some(true))
    }

    fn show(&mut self, mut m: BubbleModel) -> Out {
        if let Some(t) = m.set_at_ms {
            m.set_min_ago = Some(self.wall_ms.saturating_sub(t) / 60_000);
        }
        if self.shown.as_ref() == Some(&m) {
            return Out::Keep;
        }
        // a reminder waits for Done or Snooze: whatever replaces it (a card, the prompt) puts it back first in line
        if let Some(r) = self.shown.take().filter(|r| r.mode == Mode::Reminder && m.mode != Mode::Reminder) {
            self.deferred.push_front(r);
        }
        self.reminder = match (&m.id, &m.text) {
            (Some(id), Some(t)) if m.mode == Mode::Reminder => Some((id.clone(), t.clone())),
            _ => None,
        };
        self.shown = Some(m.clone());
        Out::Show(m)
    }

    /// Show a bubble that waited: a finished speech line gets its 6 s from now.
    fn show_later(&mut self, m: BubbleModel, now: u64) -> Out {
        self.hide_at = (m.mode == Mode::Speech && m.streaming != Some(true)).then_some(now + HIDE_MS);
        self.show(m)
    }

    /// What's shown goes; the next waiting bubble, or the running turn, comes back.
    fn hide(&mut self, now: u64) -> Out {
        self.hide_at = None;
        self.reminder = None;
        let was = self.shown.take().is_some();
        if let Some(d) = self.deferred.pop_front() {
            return self.show_later(d, now);
        }
        if self.run {
            return self.streaming();
        }
        if was {
            Out::Hide
        } else {
            Out::Keep
        }
    }

    fn mode(&self) -> Option<Mode> {
        self.shown.as_ref().map(|m| m.mode)
    }

    /// The person has to act on what's shown: other bubbles wait behind it.
    fn holding(&self) -> bool {
        matches!(self.mode(), Some(Mode::Card | Mode::Prompt | Mode::Reminder))
    }

    /// The turn's speech: the agent's own lines, then the answer.
    fn turn_text(&self) -> String {
        let mut parts: Vec<&str> = self.said.iter().map(String::as_str).collect();
        if !self.text.is_empty() {
            parts.push(&self.text);
        }
        parts.join("\n")
    }

    fn streaming(&mut self) -> Out {
        self.hide_at = None;
        let t = self.turn_text();
        self.show(BubbleModel::speech(t, true, None))
    }

    /// Click on the bear or tray → Ask… (a card on screen stays: it needs its answer first).
    pub fn open_prompt(&mut self) -> Out {
        if self.mode() == Some(Mode::Card) {
            return Out::Keep;
        }
        self.hide_at = None;
        self.show(BubbleModel::prompt())
    }

    /// A turn is running (the bubble's view).
    pub fn running(&self) -> bool {
        self.run
    }

    /// R-midturn: a new prompt replaces a running turn. The old turn's remaining events stay
    /// quiet (as after ■) until `unstop`, which the caller posts once the old turn is idle.
    pub fn supersede(&mut self, text: &str) -> Out {
        let out = self.submitted(text);
        self.stopped = true;
        out
    }

    /// The old turn is over (or the wait gave up): the new turn's events show again.
    pub fn unstop(&mut self) -> Out {
        if !self.stopped && self.run {
            return Out::Keep;
        }
        self.stopped = false;
        self.run = true;
        if self.holding() {
            return Out::Keep;
        }
        self.streaming()
    }

    /// The person sent a prompt: the bubble waits for the answer.
    pub fn submitted(&mut self, text: &str) -> Out {
        // an error held back while the prompt was open belongs to before this question: its
        // Try again would re-ask this one
        self.deferred.retain(|m| m.mode != Mode::Error);
        self.last_prompt = Some(text.to_string());
        self.ours = true;
        self.run = true;
        self.stopped = false;
        self.text.clear();
        self.said.clear();
        self.streaming()
    }

    /// The prompt couldn't be sent (no model, the agent stopped, …).
    pub fn failed(&mut self, reason: &str, message: &str) -> Out {
        self.run = false;
        self.stopped = false;
        self.error(BubbleModel::error(reason, message))
    }

    /// An error never covers the open prompt or a reminder (R-error-prompt): it waits behind them.
    fn error(&mut self, m: BubbleModel) -> Out {
        if matches!(self.mode(), Some(Mode::Prompt | Mode::Reminder)) {
            self.deferred.push_back(m);
            return Out::Keep;
        }
        self.hide_at = None;
        self.show(m)
    }

    /// `buddy.event approval_asked`: the card's details (it may arrive before or after the event).
    pub fn approval(&mut self, call_id: &str, data: Value) -> Out {
        self.approvals.insert(call_id.to_string(), data);
        if self.answered.as_deref() != Some(call_id) && matches!(&self.shown, Some(BubbleModel { card: Some(c), .. }) if c.call_id == call_id) {
            let title = self.shown.as_ref().and_then(|m| m.card.as_ref()).map(|c| c.title.clone()).unwrap_or_default();
            return self.card(call_id, &title);
        }
        Out::Keep
    }

    fn card(&mut self, call_id: &str, title: &str) -> Out {
        self.hide_at = None;
        let a = self.approvals.get(call_id);
        let s = |k: &str| a.and_then(|a| a[k].as_str()).map(str::to_string);
        let offer = a.is_some_and(|a| a["offerAlways"] == true);
        let card = CardModel {
            call_id: call_id.into(),
            title: s("title").unwrap_or_else(|| title.to_string()),
            detail: s("detail").unwrap_or_default(),
            command: s("command").or_else(|| s("query")).or_else(|| s("path")).filter(|c| !c.is_empty()),
            can_always: offer && !self.tainted,
            taint_note: self.tainted.then(|| TAINT_NOTE.to_string()),
            index: self.cards_done + 1,
            total: self.cards_done + self.cards.len().max(1),
        };
        self.show(BubbleModel { card: Some(card), ..BubbleModel::new(Mode::Card) })
    }

    /// The person answered the card: what waited behind it, else the turn's speech.
    pub fn answered(&mut self, call_id: &str, now: u64) -> Out {
        if !matches!(&self.shown, Some(BubbleModel { card: Some(c), .. }) if c.call_id == call_id) {
            return Out::Keep;
        }
        self.answered = Some(call_id.to_string());
        self.approvals.remove(call_id);
        self.cards.retain(|(id, _)| id != call_id);
        if let Some((id, title)) = self.cards.front().cloned() {
            self.cards_done += 1;
            return self.card(&id, &title);
        }
        self.cards_done = 0;
        self.hide(now)
    }

    /// The policy view's taint changed: a card on screen updates its "Always".
    pub fn set_tainted(&mut self, tainted: bool) -> Out {
        if self.tainted == tainted {
            return Out::Keep;
        }
        self.tainted = tainted;
        match self.shown.as_ref().and_then(|m| m.card.clone()) {
            Some(c) => self.card(&c.call_id, &c.title),
            None => Out::Keep,
        }
    }

    /// A `buddy say` line. `merge`: the agent's own line during its turn joins the turn's bubble.
    pub fn say(&mut self, text: &str, emotion: Option<String>, merge: bool, now: u64) -> Out {
        if merge && self.run {
            self.said.push(text.to_string());
            if self.holding() {
                return Out::Keep;
            }
            return self.streaming();
        }
        let m = BubbleModel::speech(text.to_string(), false, emotion);
        if self.holding() {
            self.deferred.push_back(m);
            return Out::Keep;
        }
        self.show_later(m, now)
    }

    /// A mascot event from the agent.
    pub fn event(&mut self, ev: &MascotEvent, now: u64) -> Out {
        match ev {
            MascotEvent::Thinking | MascotEvent::Working { .. } | MascotEvent::Speaking { .. } if self.stopped => Out::Keep,
            MascotEvent::Thinking | MascotEvent::Working { .. } => {
                self.answered = None;
                if !self.run {
                    // a turn the person didn't start here (a resumed run, a sense turn): it shows too
                    self.ours = false;
                    self.run = true;
                    self.text.clear();
                    self.said.clear();
                }
                if self.holding() {
                    return Out::Keep; // never clobber what the person is typing or has to answer
                }
                self.streaming()
            }
            MascotEvent::Speaking { text } => {
                self.answered = None;
                self.ours &= self.run;
                self.run = true;
                self.text = text.clone();
                if self.holding() {
                    return Out::Keep;
                }
                self.streaming()
            }
            MascotEvent::AwaitingPermission { call_id, title } => {
                self.answered = None;
                self.stopped = false;
                self.ours &= self.run;
                self.run = true;
                if !self.cards.iter().any(|(id, _)| id == call_id) {
                    self.cards.push_back((call_id.clone(), title.clone()));
                }
                // the shown card stays first; a later one waits behind it (its counter grows)
                let (id, title) = self.cards.front().cloned().expect("queued");
                self.card(&id, &title)
            }
            MascotEvent::Idle => {
                let was = std::mem::replace(&mut self.run, false);
                self.answered = None;
                self.approvals.clear();
                self.cards.clear();
                self.cards_done = 0;
                if std::mem::replace(&mut self.stopped, false) {
                    return Out::Keep;
                }
                if !was {
                    // a withdrawn card (an abort, a restart) goes away
                    return if self.mode() == Some(Mode::Card) { self.hide(now) } else { Out::Keep };
                }
                let t = self.turn_text();
                let turn_on_screen = matches!(&self.shown, Some(m) if m.mode == Mode::Card || (m.mode == Mode::Speech && m.streaming == Some(true)));
                if t.is_empty() {
                    return if turn_on_screen { self.hide(now) } else { Out::Keep };
                }
                let m = BubbleModel::speech(t, false, None);
                if self.holding() {
                    self.deferred.push_back(m);
                    return Out::Keep;
                }
                self.show_later(m, now)
            }
            MascotEvent::Error { reason, message } => {
                self.run = false;
                self.answered = None;
                // the run is over, as at Idle: its cards are withdrawn
                self.approvals.clear();
                self.cards.clear();
                self.cards_done = 0;
                if std::mem::replace(&mut self.stopped, false) {
                    return Out::Keep; // the stop the person asked for
                }
                let mut m = BubbleModel::error(reason, message);
                if !self.ours {
                    // Try again would re-send the person's last prompt, not this turn
                    if let Some(e) = m.error.as_mut().filter(|e| e.action.as_deref() == Some("retry")) {
                        e.action = None;
                    }
                }
                self.error(m)
            }
            MascotEvent::Reminder { id, text, set_at_ms } => {
                // a reminder stored before p4 has no setAtMs: no chip (condition f); the age is
                // worked out when it shows (`show`)
                let m = BubbleModel { text: Some(text.clone()), id: Some(id.clone()), set_at_ms: *set_at_ms, ..BubbleModel::new(Mode::Reminder) };
                if self.holding() {
                    self.deferred.push_back(m);
                    return Out::Keep;
                }
                self.hide_at = None;
                self.show(m)
            }
            // the brain's alone (say lines come through `say`, with who said them)
            MascotEvent::Say { .. } | MascotEvent::Emote { .. } | MascotEvent::Settled => Out::Keep,
        }
    }

    /// ✕ / Esc / Close / Done, and ■ on the streaming answer (the caller aborts the turn: `dismiss_stops`).
    /// A card has no ✕: it goes only with its answer (Esc there would leave the call waiting at the gate, unseen).
    pub fn dismiss(&mut self, now: u64) -> Out {
        if self.mode() == Some(Mode::Card) {
            return Out::Keep;
        }
        if self.dismiss_stops() {
            self.run = false;
            self.stopped = true;
            self.text.clear();
            self.said.clear();
        }
        self.hide(now)
    }

    /// The hide timer fired.
    pub fn tick(&mut self, now: u64, hovered: bool) -> Out {
        match self.hide_at {
            Some(t) if now >= t => {
                if hovered {
                    self.hide_at = Some(now + HOVER_RECHECK_MS);
                    Out::Keep
                } else {
                    self.hide(now)
                }
            }
            _ => Out::Keep,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mascot_protocol::WorkKind;
    use serde_json::json;

    fn labels(outs: &[Out]) -> Vec<String> {
        outs.iter()
            .filter_map(|o| match o {
                Out::Show(m) => Some(m.label()),
                Out::Hide => Some("hide".into()),
                Out::Keep => None,
            })
            .collect()
    }

    fn speaking(t: &str) -> MascotEvent {
        MascotEvent::Speaking { text: t.into() }
    }

    #[test]
    fn a_turn_streams_then_settles_then_hides_after_6s_unless_hovered() {
        let mut b = Bubble::new();
        let mut o = vec![b.open_prompt(), b.submitted("hi"), b.event(&MascotEvent::Thinking, 0)];
        o.push(b.event(&speaking("Hel"), 10));
        o.push(b.event(&speaking("Hello."), 20));
        o.push(b.event(&MascotEvent::Idle, 1000));
        assert_eq!(labels(&o), ["prompt", "speech streaming", "speech streaming", "speech streaming", "speech"]);
        assert_eq!(b.shown().unwrap().text.as_deref(), Some("Hello."));
        assert_eq!(b.hide_at(), Some(1000 + HIDE_MS));
        assert_eq!(b.tick(5000, false), Out::Keep, "not yet");
        assert_eq!(b.tick(7000, true), Out::Keep, "hovered");
        assert_eq!(b.hide_at(), Some(8000));
        assert_eq!(b.tick(8000, false), Out::Hide);
        assert!(b.shown().is_none());
    }

    #[test]
    fn a_card_shows_its_details_and_the_answer_goes_back_to_speech() {
        let mut b = Bubble::new();
        b.submitted("gated hi");
        b.approval("c1", json!({ "title": "Run make?", "detail": "It isn’t in the allowlist.", "command": "make greet && buddy say hi", "offerAlways": true }));
        let o = b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run make?".into() }, 0);
        let Out::Show(m) = o else { panic!("{o:?}") };
        let c = m.card.unwrap();
        assert_eq!((c.call_id.as_str(), c.title.as_str(), c.command.as_deref()), ("c1", "Run make?", Some("make greet && buddy say hi")));
        assert!(c.can_always);
        assert_eq!(c.taint_note, None);
        assert_eq!(b.hide_at(), None, "a card never hides by itself");
        assert_eq!(labels(&[b.answered("c1", 0)]), ["speech streaming"]);
        assert_eq!(b.answered("c1", 0), Out::Keep, "once");
        let o = [b.event(&MascotEvent::Working { kind: WorkKind::Shell, tool: "bash".into() }, 1), b.event(&speaking("Done."), 2), b.event(&MascotEvent::Idle, 3)];
        assert_eq!(labels(&o), ["speech streaming", "speech"]);
    }

    #[test]
    fn a_second_card_waits_behind_the_first_and_the_counter_says_n_of_m() {
        let mut b = Bubble::new();
        b.submitted("two things");
        let ask = |b: &mut Bubble, id: &str| b.event(&MascotEvent::AwaitingPermission { call_id: id.into(), title: format!("Run {id}?") }, 0);
        let card = |o: &Out| match o {
            Out::Show(m) => m.card.as_ref().map(|c| (c.call_id.clone(), c.index, c.total)).unwrap(),
            o => panic!("{o:?}"),
        };
        assert_eq!(card(&ask(&mut b, "a")), ("a".into(), 1, 1));
        // the shown card stays; its counter grows
        assert_eq!(card(&ask(&mut b, "b")), ("a".into(), 1, 2));
        assert_eq!(ask(&mut b, "b"), Out::Keep, "the same call again changes nothing");
        assert!(!b.can_always("b"), "only the shown card can be answered");
        assert_eq!(b.answered("b", 0), Out::Keep);
        assert_eq!(card(&b.answered("a", 0)), ("b".into(), 2, 2));
        assert_eq!(labels(&[b.answered("b", 0)]), ["speech streaming"]);
        // the next batch counts from 1 again
        assert_eq!(card(&ask(&mut b, "c")), ("c".into(), 1, 1));
        // Idle drops what's left (the run is over, so its cards are withdrawn)
        ask(&mut b, "d");
        b.event(&MascotEvent::Idle, 1);
        b.submitted("again");
        assert_eq!(card(&ask(&mut b, "e")), ("e".into(), 1, 1));
    }

    #[test]
    fn details_arriving_after_the_event_fill_the_card_in() {
        let mut b = Bubble::new();
        let o = b.event(&MascotEvent::AwaitingPermission { call_id: "c2".into(), title: "Run curl?".into() }, 0);
        assert!(matches!(&o, Out::Show(m) if m.card.as_ref().unwrap().command.is_none()));
        let o = b.approval("c2", json!({ "title": "Run curl?", "detail": "d", "command": "curl x", "offerAlways": false }));
        assert!(matches!(&o, Out::Show(m) if m.card.as_ref().unwrap().command.as_deref() == Some("curl x")));
    }

    #[test]
    fn a_tainted_card_offers_no_always_and_says_why() {
        let mut b = Bubble::new();
        b.approval("c3", json!({ "title": "Run curl?", "detail": "d", "command": "curl x", "offerAlways": true }));
        b.event(&MascotEvent::AwaitingPermission { call_id: "c3".into(), title: "Run curl?".into() }, 0);
        let Out::Show(m) = b.set_tainted(true) else { panic!() };
        let c = m.card.unwrap();
        assert!(!c.can_always);
        assert_eq!(c.taint_note.as_deref(), Some(TAINT_NOTE));
        let Out::Show(m) = b.set_tainted(false) else { panic!() };
        assert!(m.card.unwrap().can_always);
        assert_eq!(b.set_tainted(false), Out::Keep);
    }

    #[test]
    fn errors_offer_retry_or_sign_in() {
        let mut b = Bubble::new();
        b.submitted("please break");
        let Out::Show(m) = b.event(&MascotEvent::Error { reason: "model_error".into(), message: "400 bad".into() }, 0) else { panic!() };
        assert_eq!(m.mode, Mode::Error);
        assert_eq!(m.error.unwrap().action.as_deref(), Some("retry"));
        assert_eq!(b.last_prompt(), Some("please break"));
        assert!(!b.run);
        let Out::Show(m) = b.failed("no_model", "no key") else { panic!() };
        assert_eq!(m.error.unwrap().action.as_deref(), Some("sign_in"));
        assert_eq!(b.dismiss(0), Out::Hide);
    }

    #[test]
    fn the_agents_say_in_its_turn_merges_and_a_users_say_stands_alone() {
        let mut b = Bubble::new();
        b.submitted("say hi");
        b.event(&MascotEvent::Thinking, 0);
        let Out::Show(m) = b.say("Hi there", Some("happy".into()), true, 5) else { panic!() };
        assert_eq!((m.text.as_deref(), m.streaming), (Some("Hi there"), Some(true)));
        b.event(&speaking("I said hi."), 6);
        let Out::Show(m) = b.event(&MascotEvent::Idle, 7) else { panic!() };
        assert_eq!(m.text.as_deref(), Some("Hi there\nI said hi."));
        // From the person's terminal: its own bubble with the emotion, hiding after 6 s.
        let Out::Show(m) = b.say("hello", Some("happy".into()), false, 100) else { panic!() };
        assert_eq!((m.text.as_deref(), m.streaming, m.emotion.as_deref()), (Some("hello"), Some(false), Some("happy")));
        assert_eq!(b.hide_at(), Some(100 + HIDE_MS));
        // The agent's line after its turn ended is a standalone line too.
        let Out::Show(m) = b.say("late", None, true, 200) else { panic!() };
        assert_eq!(m.streaming, Some(false));
    }

    #[test]
    fn a_say_or_reminder_waits_for_the_card_or_prompt() {
        let mut b = Bubble::new();
        b.open_prompt();
        assert_eq!(b.say("hello", None, false, 0), Out::Keep);
        assert_eq!(b.event(&MascotEvent::Thinking, 0), Out::Keep, "typing is never clobbered");
        assert!(matches!(b.dismiss(40), Out::Show(m) if m.text.as_deref() == Some("hello")));
        assert_eq!(b.hide_at(), Some(40 + HIDE_MS), "a line that waited gets its 6 s once shown");
        b.event(&MascotEvent::AwaitingPermission { call_id: "c9".into(), title: "Run x?".into() }, 1);
        assert_eq!(b.open_prompt(), Out::Keep, "a card needs its answer first");
        assert_eq!(b.event(&MascotEvent::Reminder { id: "r_1".into(), text: "stretch".into(), set_at_ms: None }, 2), Out::Keep);
        assert_eq!(b.say("later", None, false, 3), Out::Keep, "queued behind the reminder, not over it");
        assert!(matches!(b.answered("c9", 50), Out::Show(m) if m.mode == Mode::Reminder && m.id.as_deref() == Some("r_1")));
        assert_eq!(b.reminder(), Some(&("r_1".to_string(), "stretch".to_string())), "Snooze finds a reminder that waited");
        assert_eq!(b.hide_at(), None);
        assert!(matches!(b.dismiss(60), Out::Show(m) if m.text.as_deref() == Some("later")));
        assert!(matches!(b.dismiss(61), Out::Show(m) if m.streaming == Some(true)), "then the turn the card belongs to");
        assert_eq!(b.event(&MascotEvent::Idle, 62), Out::Hide);
    }

    #[test]
    fn a_reminder_on_screen_comes_back_after_the_prompt_and_isnt_replaced_by_a_turn() {
        let mut b = Bubble::new();
        b.event(&MascotEvent::Reminder { id: "r_2".into(), text: "water".into(), set_at_ms: None }, 0);
        assert_eq!(labels(&[b.open_prompt()]), ["prompt"]);
        assert_eq!(b.reminder(), None);
        assert!(matches!(b.dismiss(1), Out::Show(m) if m.id.as_deref() == Some("r_2")), "Esc on the prompt: the reminder again");
        // a turn the bear runs meanwhile waits behind the reminder; its answer shows after Done
        assert_eq!(b.event(&MascotEvent::Thinking, 2), Out::Keep);
        assert_eq!(b.event(&speaking("Hi."), 3), Out::Keep);
        assert_eq!(b.event(&MascotEvent::Idle, 4), Out::Keep);
        assert!(!b.dismiss_stops(), "Done on a reminder never stops a turn");
        let Out::Show(m) = b.dismiss(10) else { panic!() };
        assert_eq!((m.text.as_deref(), m.streaming), (Some("Hi."), Some(false)));
        assert_eq!(b.hide_at(), Some(10 + HIDE_MS));
    }

    #[test]
    fn only_the_streaming_answer_stops_the_turn_and_it_stays_stopped() {
        let mut b = Bubble::new();
        b.submitted("long story");
        b.event(&speaking("Once upon"), 1);
        // the bear is clicked mid-turn: the prompt, and Esc brings the answer back without stopping it
        b.open_prompt();
        assert!(!b.dismiss_stops());
        assert!(matches!(b.dismiss(2), Out::Show(m) if m.streaming == Some(true) && m.text.as_deref() == Some("Once upon")));
        assert!(b.run);
        // ■ on the streaming answer: stopped, and the turn's last events don't bring it back
        assert!(b.dismiss_stops());
        assert_eq!(b.dismiss(3), Out::Hide);
        assert!(!b.run);
        assert_eq!(b.event(&speaking("Once upon a time"), 4), Out::Keep);
        assert_eq!(b.event(&MascotEvent::Error { reason: "aborted".into(), message: "stopped".into() }, 5), Out::Keep);
        assert_eq!(b.event(&MascotEvent::Idle, 6), Out::Keep);
        assert!(b.shown().is_none());
        // the next turn shows again
        assert_eq!(labels(&[b.submitted("again"), b.event(&MascotEvent::Thinking, 7)]), ["speech streaming"]);
    }

    #[test]
    fn esc_keeps_a_card_until_it_is_answered_and_an_error_drops_the_runs_cards() {
        let mut b = Bubble::new();
        b.submitted("gated hi");
        b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run make?".into() }, 0);
        // Esc on the card: nothing changes, the call still waits for its answer
        assert_eq!(b.dismiss(1), Out::Keep);
        assert!(matches!(b.shown(), Some(m) if m.mode == Mode::Card));
        assert!(matches!(b.answered("c1", 2), Out::Show(m) if m.streaming == Some(true)));
        // a run that ends in an error withdraws a card still queued: the next run's card is its own, 1 of 1
        b.event(&MascotEvent::AwaitingPermission { call_id: "c2".into(), title: "Run a?".into() }, 3);
        b.event(&MascotEvent::AwaitingPermission { call_id: "c3".into(), title: "Run b?".into() }, 3);
        b.event(&MascotEvent::Error { reason: "interrupted".into(), message: "x".into() }, 4);
        b.submitted("again");
        let Out::Show(m) = b.event(&MascotEvent::AwaitingPermission { call_id: "c4".into(), title: "Run c?".into() }, 5) else { panic!() };
        let c = m.card.unwrap();
        assert_eq!((c.call_id.as_str(), c.index, c.total), ("c4", 1, 1));
    }

    #[test]
    fn always_is_only_offered_while_the_card_offers_it_and_details_go_with_the_run() {
        let mut b = Bubble::new();
        b.approval("c5", json!({ "title": "Run make?", "offerAlways": true }));
        b.event(&MascotEvent::AwaitingPermission { call_id: "c5".into(), title: "Run make?".into() }, 0);
        assert!(b.can_always("c5"));
        assert!(!b.can_always("c6"));
        b.set_tainted(true);
        assert!(!b.can_always("c5"), "a web read turned it off");
        b.approval("c7", json!({ "title": "never shown" }));
        b.event(&MascotEvent::Idle, 1);
        assert!(b.approvals.is_empty(), "the run ended: its cards' details go");
    }

    #[test]
    fn a_reminder_shows_with_its_id_until_done() {
        let mut b = Bubble::new();
        let Out::Show(m) = b.event(&MascotEvent::Reminder { id: "r_ab".into(), text: "stretch".into(), set_at_ms: None }, 0) else { panic!() };
        assert_eq!(serde_json::to_value(&m).unwrap(), json!({ "mode": "reminder", "text": "stretch", "id": "r_ab" }));
        assert_eq!(b.reminder(), Some(&("r_ab".to_string(), "stretch".to_string())));
        assert_eq!(b.tick(60_000, false), Out::Keep, "a reminder waits for Done or Snooze");
        assert_eq!(b.dismiss(0), Out::Hide);
        assert_eq!(b.reminder(), None);
    }

    #[test]
    fn a_withdrawn_card_goes_away_and_an_empty_run_hides() {
        let mut b = Bubble::new();
        b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "t".into() }, 0);
        // the sidecar crashed: the mapper resets to Idle; the run ended without speech
        assert_eq!(b.event(&MascotEvent::Idle, 1), Out::Hide);
        b.submitted("x");
        assert_eq!(b.event(&MascotEvent::Idle, 2), Out::Hide);
        assert_eq!(b.event(&MascotEvent::Idle, 3), Out::Keep);
    }

    #[test]
    fn the_model_serializes_as_section_4_8() {
        let mut b = Bubble::new();
        b.approval("c1", json!({ "title": "Run make?", "detail": "d", "command": "make", "offerAlways": false }));
        let Out::Show(m) = b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run make?".into() }, 0) else { panic!() };
        assert_eq!(
            serde_json::to_value(&m).unwrap(),
            json!({ "mode": "card", "card": { "callId": "c1", "title": "Run make?", "detail": "d", "command": "make", "canAlways": false, "index": 1, "total": 1 } })
        );
        let e = serde_json::to_value(BubbleModel::error("model_error", "x")).unwrap();
        assert_eq!(e, json!({ "mode": "error", "error": { "reason": "model_error", "message": "x", "action": "retry" } }));
        assert_eq!(serde_json::to_value(BubbleModel::speech("a".into(), true, None)).unwrap(), json!({ "mode": "speech", "text": "a", "streaming": true }));
    }

    #[test]
    fn r_error_prompt_an_error_waits_behind_the_open_prompt_and_a_reminder() {
        let mut b = Bubble::new();
        b.open_prompt();
        // a sense turn (or a resumed run) fails while the person types: the prompt stays
        assert_eq!(b.event(&MascotEvent::Error { reason: "model_error".into(), message: "529".into() }, 1), Out::Keep);
        assert_eq!(b.failed("no_model", "Sign in first."), Out::Keep);
        assert_eq!(b.shown().map(|m| m.mode), Some(Mode::Prompt));
        // Esc: the errors, in order
        assert!(matches!(b.dismiss(2), Out::Show(m) if m.error.as_ref().unwrap().reason == "model_error"));
        assert!(matches!(b.dismiss(3), Out::Show(m) if m.error.as_ref().unwrap().reason == "no_model"));
        // behind a reminder too
        b.event(&MascotEvent::Reminder { id: "r".into(), text: "tea".into(), set_at_ms: None }, 4);
        assert_eq!(b.event(&MascotEvent::Error { reason: "interrupted".into(), message: "x".into() }, 5), Out::Keep);
        assert!(matches!(b.dismiss(6), Out::Show(m) if m.mode == Mode::Error));
        // submitting moves on: an error held back behind the prompt never pops up after the answer
        let mut b = Bubble::new();
        b.open_prompt();
        assert_eq!(b.event(&MascotEvent::Error { reason: "model_error".into(), message: "529".into() }, 1), Out::Keep);
        b.submitted("hi");
        b.event(&speaking("hello"), 2);
        b.event(&MascotEvent::Idle, 3);
        assert!(!matches!(b.dismiss(4), Out::Show(m) if m.mode == Mode::Error));
        // a keychain error offers the Sign in… window (which shows why), not a Try again loop
        assert_eq!(BubbleModel::error("keychain", "Can't read the keychain: access denied.").error.unwrap().action.as_deref(), Some("sign_in"));
    }

    #[test]
    fn r_midturn_the_old_turns_late_text_never_lands_in_the_new_turn() {
        let mut b = Bubble::new();
        b.submitted("tell me a long story");
        b.event(&speaking("Once upon a"), 1);
        assert!(b.running());
        // a new prompt mid-turn: the bubble waits for the new answer…
        assert_eq!(labels(&[b.supersede("actually, a haiku")]), ["speech streaming"]);
        assert_eq!(b.last_prompt(), Some("actually, a haiku"));
        // …and the old turn's tail (after the abort) stays quiet
        assert_eq!(b.event(&speaking("Once upon a time there"), 2), Out::Keep);
        assert_eq!(b.event(&MascotEvent::Idle, 3), Out::Keep);
        assert_eq!(b.shown().unwrap().text.as_deref(), Some(""));
        // the old turn is idle: unstop, then the new turn's events show
        assert!(matches!(b.unstop(), Out::Keep | Out::Show(_)));
        assert!(b.running());
        assert_eq!(b.event(&MascotEvent::Thinking, 4), Out::Keep, "same streaming bubble");
        let Out::Show(m) = b.event(&speaking("Leaves fall"), 5) else { panic!() };
        assert_eq!(m.text.as_deref(), Some("Leaves fall"));
        let Out::Show(m) = b.event(&MascotEvent::Idle, 6) else { panic!() };
        assert_eq!((m.text.as_deref(), m.streaming), (Some("Leaves fall"), Some(false)));
        // ■ on the new turn still works afterwards
        b.submitted("again");
        assert!(b.dismiss_stops());
    }

    #[test]
    fn r_chip_set_min_ago_comes_from_set_at_ms_and_an_old_reminder_has_no_chip() {
        let mut b = Bubble::new();
        b.wall_ms = 1_700_000_000_000 + 20 * 60_000 + 30_000;
        let Out::Show(m) = b.event(&MascotEvent::Reminder { id: "r_1".into(), text: "stretch".into(), set_at_ms: Some(1_700_000_000_000) }, 0) else { panic!() };
        assert_eq!(m.set_min_ago, Some(20));
        assert_eq!(serde_json::to_value(&m).unwrap()["setMinAgo"], 20);
        b.dismiss(1);
        // condition f: stored before p4 (no setAtMs) — it still shows, without the chip
        let Out::Show(m) = b.event(&MascotEvent::Reminder { id: "r_0".into(), text: "old".into(), set_at_ms: None }, 2) else { panic!() };
        assert_eq!(serde_json::to_value(&m).unwrap(), json!({ "mode": "reminder", "text": "old", "id": "r_0" }));
        // a clock that went backwards never shows a negative age
        b.dismiss(3);
        let Out::Show(m) = b.event(&MascotEvent::Reminder { id: "r_2".into(), text: "x".into(), set_at_ms: Some(b.wall_ms + 60_000) }, 4) else { panic!() };
        assert_eq!(m.set_min_ago, Some(0));
    }

    #[test]
    fn v11_a_reminder_that_waited_behind_a_card_shows_its_age_at_show_time() {
        let mut b = Bubble::new();
        let set = 1_700_000_000_000;
        b.wall_ms = set + 2 * 60_000;
        b.event(&MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run it?".into() }, 0);
        assert_eq!(b.event(&MascotEvent::Reminder { id: "r_1".into(), text: "stretch".into(), set_at_ms: Some(set) }, 1), Out::Keep, "it waits behind the card");
        // the card is answered 15 minutes later
        b.wall_ms = set + 17 * 60_000;
        let Out::Show(m) = b.answered("c1", 2) else { panic!() };
        assert_eq!(m.mode, Mode::Reminder);
        assert_eq!(m.set_min_ago, Some(17), "worked out when shown, not when it arrived");
    }

    #[test]
    fn v5_an_unprompted_turns_error_offers_close_only() {
        let mut b = Bubble::new();
        b.submitted("what's the weather");
        b.event(&MascotEvent::Idle, 1);
        b.dismiss(2);
        // a sense turn: it starts with no submit here
        b.event(&MascotEvent::Thinking, 3);
        let Out::Show(m) = b.event(&MascotEvent::Error { reason: "model_error".into(), message: "529".into() }, 4) else { panic!() };
        assert_eq!(m.error.as_ref().unwrap().action, None, "no Try again: it would re-send the last user prompt");
        assert!(serde_json::to_value(&m).unwrap()["error"].get("action").is_none());
        b.dismiss(5);
        // the person's own turn still offers it
        b.submitted("again?");
        let Out::Show(m) = b.event(&MascotEvent::Error { reason: "model_error".into(), message: "529".into() }, 6) else { panic!() };
        assert_eq!(m.error.unwrap().action.as_deref(), Some("retry"));
        // and an unprompted turn that only speaks then errors is still not ours
        b.dismiss(7);
        b.event(&MascotEvent::Idle, 8);
        b.event(&MascotEvent::Speaking { text: "hm".into() }, 9);
        let Out::Show(m) = b.event(&MascotEvent::Error { reason: "model_error".into(), message: "x".into() }, 10) else { panic!() };
        assert_eq!(m.error.unwrap().action, None);
    }
}
