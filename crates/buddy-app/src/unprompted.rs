//! Unprompted (sense-triggered) model turns (design `## accounts`, D4). Every gated `Senses`
//! sample runs through its own `Reflexes`; only `back_from_idle`, `morning` and `long_work`
//! become candidates. A candidate is debounced (the same rule at most once per 30 min, any
//! sense turn ≥ 5 min after the last), held back without spending budget (and offered again on
//! the next sample, unless a canned idle line spoke for that moment meanwhile: `spoken_for`) while the person is busy with
//! the bubble or there's no running model, and then has to pass `attention::Budget`
//! (4 an hour, none in DND or quiet hours). What the agent gets is coarse on purpose: the
//! rule name and the time of day, never an app name or a window title.

use std::collections::HashMap;

use mascot_core::persona::{band, Band};
use mascot_core::reflex::{BACK_FROM_IDLE, LONG_WORK, MORNING};
use mascot_core::{Budget, EffectivePersona, LocalTime, Reflexes, Senses};

pub const PER_HOUR: u32 = 4;
/// The same rule again only after this long.
pub const SAME_RULE_MS: u64 = 30 * 60_000;
/// Any sense turn only this long after the last one.
pub const ANY_RULE_MS: u64 = 5 * 60_000;

/// Whether a sense turn could start now (besides the budget).
#[derive(Clone, Copy, Debug, Default)]
pub struct Ready {
    /// A turn is running, or the prompt, a card or a reminder is on screen.
    pub busy: bool,
    /// A model is chosen and the agent is running (`Harness::can_turn`).
    pub can_turn: bool,
}

/// The persona's voice for a check-in: who is speaking, a short
/// gist of how, and how chatty. Never the person's own notes.
#[derive(Clone, Debug, PartialEq)]
pub struct Voice {
    pub name: String,
    pub tagline: String,
    pub gist: String,
    pub chattiness: f64,
}

/// The longest voice gist (characters).
const GIST_MAX: usize = 60;

impl Voice {
    /// The voice of a persona in force: its name and tagline, the first sentence of its first voice
    /// rule, and its effective chattiness.
    pub fn of(e: &EffectivePersona) -> Voice {
        let rule = e.voice.first().map(|v| e.fill(v)).unwrap_or_default();
        let first = rule.split(". ").next().unwrap_or("");
        let gist: String = first.trim().trim_end_matches('.').chars().filter(|c| !c.is_control()).take(GIST_MAX).collect();
        Voice { name: e.name.clone(), tagline: e.tagline.clone(), gist: gist.trim().to_string(), chattiness: e.temperament.chattiness }
    }

    /// How long the line may be, by chattiness band (the window's Brief / Chatty / Talkative).
    fn directive(&self) -> &'static str {
        match band(self.chattiness) {
            Band::Low => "one short sentence at most, and stay quiet unless it really helps",
            Band::Mid => "one short line",
            Band::High => "a short line or two, and speak up when you notice something",
        }
    }

    /// `Reply in Biscuit's voice (cheeky sidekick; Dry one-liners): one short line, or nothing at all.`
    fn instruction(&self) -> String {
        let traits = [self.tagline.as_str(), self.gist.as_str()].into_iter().filter(|t| !t.is_empty()).collect::<Vec<_>>().join("; ");
        let traits = if traits.is_empty() { String::new() } else { format!(" ({traits})") };
        format!("Reply in {}'s voice{traits}: {}, or nothing at all.", self.name, self.directive())
    }
}

pub struct Unprompted {
    reflex: Reflexes,
    budget: Budget,
    last_rule: HashMap<&'static str, u64>,
    last_turn: Option<u64>,
    /// The candidate held back while busy or without a model (logged once).
    waiting: Option<&'static str>,
    /// The rule whose model turn the last `sample` started (`None` when it started none).
    started: Option<&'static str>,
    /// The persona's voice for the check-in text (`None`: the plain text of before personas).
    voice: Option<Voice>,
}

impl Default for Unprompted {
    fn default() -> Self {
        Unprompted::new(PER_HOUR)
    }
}

impl Unprompted {
    pub fn new(per_hour: u32) -> Unprompted {
        Unprompted { reflex: Reflexes::new(), budget: Budget::new(per_hour), last_rule: HashMap::new(), last_turn: None, waiting: None, started: None, voice: None }
    }

    /// One gated sample. `allowed`: `Brain::unprompted_allowed()`. Returns the summary to send
    /// as a sense turn, when one should start now (its budget is already taken).
    pub fn sample(&mut self, s: &Senses, now: u64, allowed: bool, ready: Ready) -> Option<String> {
        self.started = None;
        // free = true: the brain's own reflex shows the bear's reaction; this copy only
        // decides about the model turn (app_switch never fires: chance is always false)
        let before = self.reflex.clone();
        let fired = self.reflex.sample(s, now, !allowed, true, 0.0, &mut |_| false)?;
        let rule = fired.rule;
        if ![BACK_FROM_IDLE, MORNING, LONG_WORK].contains(&rule) {
            return None;
        }
        if ready.busy || !ready.can_turn {
            // not now, but not lost either: undo the rule's one-shot state (the morning of the
            // day, the long-work stretch) so the next sample offers it again. back_from_idle is
            // an edge: replayed later it would greet a return long past, so it is dropped.
            if rule != BACK_FROM_IDLE {
                self.reflex = before;
            }
            if self.waiting != Some(rule) {
                eprintln!("[buddy] unprompted {rule}: waiting ({})", if ready.busy { "busy" } else { "no model or agent" });
                self.waiting = Some(rule);
            }
            return None;
        }
        self.waiting = None;
        if self.last_rule.get(rule).is_some_and(|&t| now.saturating_sub(t) < SAME_RULE_MS) || self.last_turn.is_some_and(|t| now.saturating_sub(t) < ANY_RULE_MS) {
            eprintln!("[buddy] unprompted {rule}: debounced");
            return None;
        }
        if !self.budget.try_take(now, allowed) {
            eprintln!("[buddy] unprompted {rule}: over budget ({}/{})", self.budget.used(now), self.budget.per_hour());
            return None;
        }
        self.last_rule.insert(rule, now);
        self.last_turn = Some(now);
        eprintln!("[buddy] unprompted {rule}: turn ({}/{})", self.budget.used(now), self.budget.per_hour());
        self.started = Some(rule);
        Some(summary(rule, s.local, self.voice.as_ref()))
    }

    /// A canned idle line spoke for `rule`'s moment: a check-in for it counts as said (the same-rule
    /// debounce), so a candidate held back for want of a model is not offered again.
    pub fn spoken_for(&mut self, rule: &'static str, now: u64) {
        self.last_rule.insert(rule, now);
        if self.waiting == Some(rule) {
            self.waiting = None;
        }
    }

    /// The rule the last `sample` started a model turn for, if it started one.
    pub fn started(&self) -> Option<&'static str> {
        self.started
    }

    /// The persona's voice for the next check-ins (`None`: none in this mascot).
    pub fn set_voice(&mut self, voice: Option<Voice>) {
        self.voice = voice;
    }

    pub fn used(&self, now: u64) -> u32 {
        self.budget.used(now)
    }

    /// The tray line (prototype `#tray-budget` / `#tray-budget-paused`). `paused`: why
    /// unprompted turns are off now, if they are.
    pub fn line(&self, now: u64, paused: Option<&str>) -> String {
        match paused {
            Some(why) => format!("Check-ins paused ({why})"),
            None => format!("Check-ins: {} of {} this hour", self.used(now), self.budget.per_hour()),
        }
    }
}

/// Why unprompted turns are off for these gated senses (quiet hours win: "sleepy").
pub fn paused(s: &Senses) -> Option<&'static str> {
    match () {
        _ if s.quiet_hours => Some("quiet hours"),
        _ if s.fullscreen || s.screen_share || s.dnd_toggle => Some("do not disturb"),
        _ => None,
    }
}

fn time_of_day(t: LocalTime) -> &'static str {
    match t.minutes / 60 {
        5..=11 => "morning",
        12..=17 => "afternoon",
        18..=22 => "evening",
        _ => "night",
    }
}

/// The sense turn's text: the rule and the time of day only (lead condition d). With a persona
/// `voice` the closing ask is in its voice; without one the text is what it was before personas.
pub fn summary(rule: &str, local: LocalTime, voice: Option<&Voice>) -> String {
    let what = match rule {
        BACK_FROM_IDLE => "the person is back at the computer after a break",
        MORNING => "the person's first activity this morning",
        LONG_WORK => "the person has been working for a long stretch",
        _ => "something changed",
    };
    let ask = match voice {
        Some(v) => v.instruction(),
        None => "Say one short, friendly line, or nothing at all.".to_string(),
    };
    format!("[unprompted · {rule} · {}] {what}. {ask}", time_of_day(local))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mascot_core::{AppCategory, FrontApp};

    const MIN: u64 = 60_000;
    const READY: Ready = Ready { busy: false, can_turn: true };

    fn at(minutes: u16, idle: u64) -> Senses {
        Senses { local: LocalTime { minutes, weekday: 2 }, user_idle_ms: idle, ..Senses::default() }
    }

    /// Away for 10 min, then back: one back_from_idle candidate at `t + 10 min`.
    fn away_and_back(u: &mut Unprompted, t: u64, allowed: bool, ready: Ready) -> Option<String> {
        // afternoon: no morning rule in the way
        assert_eq!(u.sample(&at(14 * 60, 0), t, allowed, ready), None);
        assert_eq!(u.sample(&at(14 * 60, 10 * MIN), t + 10 * MIN, allowed, ready), None);
        u.sample(&at(14 * 60, 1_000), t + 10 * MIN + 1_000, allowed, ready)
    }

    #[test]
    fn four_an_hour_then_the_fifth_candidate_sends_nothing_and_stays_4_of_4() {
        let mut u = Unprompted::new(4);
        let mut sent = vec![];
        for i in 0..5u64 {
            let t = i * 12 * MIN;
            // forget the same-rule clock between rounds: this test is about the budget
            u.last_rule.clear();
            sent.push(away_and_back(&mut u, t, true, READY).is_some());
        }
        assert_eq!(sent, [true, true, true, true, false], "the 5th in the hour sends no prompt");
        let now = 4 * 12 * MIN + 10 * MIN + 1_000;
        assert_eq!(u.used(now), 4);
        assert_eq!(u.line(now, None), "Check-ins: 4 of 4 this hour");
    }

    #[test]
    fn none_in_dnd_or_quiet_hours_and_busy_or_no_model_spends_nothing() {
        let mut u = Unprompted::new(4);
        assert_eq!(away_and_back(&mut u, 0, false, READY), None, "DND / quiet hours");
        assert_eq!(u.used(20 * MIN), 0);
        assert_eq!(away_and_back(&mut u, 20 * MIN, true, Ready { busy: true, can_turn: true }), None);
        let back = 30 * MIN + 1_000;
        assert_eq!(u.used(back), 0, "a held-back candidate spends no budget");
        // back_from_idle is an edge: ready again later, that return isn't greeted after the fact
        assert_eq!(u.sample(&at(14 * 60, 2_000), back + 1_000, true, READY), None, "no stale greeting");
        // signed out when the person comes back: nothing, and nothing once signed in
        assert_eq!(away_and_back(&mut u, 40 * MIN, true, Ready { busy: false, can_turn: false }), None);
        assert_eq!(u.sample(&at(14 * 60, 2_000), 50 * MIN + 2_000, true, READY), None);
        assert_eq!(u.used(50 * MIN + 2_000), 0);
        let u = Unprompted::new(4);
        let mut s = Senses::default();
        assert_eq!(u.line(0, paused(&s)), "Check-ins: 0 of 4 this hour");
        s.dnd_toggle = true;
        assert_eq!(u.line(0, paused(&s)), "Check-ins paused (do not disturb)");
        s.quiet_hours = true;
        assert_eq!(u.line(0, paused(&s)), "Check-ins paused (quiet hours)");
    }

    #[test]
    fn debounce_same_rule_30_min_and_any_rule_5_min() {
        let mut u = Unprompted::new(10);
        assert!(away_and_back(&mut u, 0, true, READY).is_some());
        // back again 12 min later: same rule within 30 min
        assert_eq!(away_and_back(&mut u, 12 * MIN, true, READY), None);
        assert!(away_and_back(&mut u, 30 * MIN, true, READY).is_some(), "≥ 30 min after the last back_from_idle");
        // a different rule right after: within 5 min of the last sense turn
        u.last_rule.clear();
        u.last_turn = Some(100 * MIN);
        assert_eq!(away_and_back(&mut u, 100 * MIN - 10 * MIN, true, READY), None);
    }

    #[test]
    fn morning_and_long_work_count_app_switch_never_does() {
        let mut u = Unprompted::new(4);
        let s = at(8 * 60, 0);
        // signed out at 08:00: the morning turn waits for the sign-in instead of being spent
        assert_eq!(u.sample(&s, 0, true, Ready { busy: false, can_turn: false }), None);
        assert_eq!(u.sample(&s, 5 * MIN, true, Ready { busy: false, can_turn: false }), None);
        let m = u.sample(&s, 6 * MIN, true, READY).expect("morning");
        assert!(m.contains("morning") && m.contains(MORNING), "{m}");
        // a game in front never starts a model turn
        let mut g = at(14 * 60, 0);
        g.frontmost = Some(FrontApp { bundle_id: "com.example.secret-game".into(), category: AppCategory::Game });
        let mut u = Unprompted::new(4);
        assert_eq!(u.sample(&at(14 * 60, 0), 0, true, READY), None);
        assert_eq!(u.sample(&g, 1_000, true, READY), None);
        // two hours of input: long_work
        let mut t = 2_000;
        let mut got = None;
        while t < 2 * 3_600_000 + 2 * MIN && got.is_none() {
            got = u.sample(&g, t, true, READY);
            t += MIN;
        }
        let w = got.expect("long_work");
        assert!(w.contains(LONG_WORK), "{w}");
        // condition d: no app name, bundle id or window title in what the agent gets
        for secret in ["secret-game", "com.example", "game"] {
            assert!(!w.contains(secret), "{w}");
        }
    }

    #[test]
    fn the_summary_is_the_rule_and_the_time_of_day() {
        assert_eq!(time_of_day(LocalTime { minutes: 8 * 60, weekday: 0 }), "morning");
        assert_eq!(time_of_day(LocalTime { minutes: 13 * 60, weekday: 0 }), "afternoon");
        assert_eq!(time_of_day(LocalTime { minutes: 20 * 60, weekday: 0 }), "evening");
        assert_eq!(time_of_day(LocalTime { minutes: 2 * 60, weekday: 0 }), "night");
        assert_eq!(summary(BACK_FROM_IDLE, LocalTime { minutes: 15 * 60, weekday: 0 }, None), "[unprompted · back_from_idle · afternoon] the person is back at the computer after a break. Say one short, friendly line, or nothing at all.");
    }

    // ---- the persona's voice ----

    const BACK_TEXT: &str = "[unprompted · back_from_idle · afternoon] the person is back at the computer after a break. Say one short, friendly line, or nothing at all.";
    const AFTERNOON: LocalTime = LocalTime { minutes: 15 * 60, weekday: 0 };

    fn voice_of(user: mascot_core::persona::UserPersona) -> Voice {
        let pack = mascot_core::pack::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear")).unwrap();
        Voice::of(&mascot_core::persona::resolve(&pack, &user).unwrap())
    }

    #[test]
    fn without_a_persona_the_check_in_text_is_byte_identical_to_before() {
        for (rule, what) in [
            (BACK_FROM_IDLE, "the person is back at the computer after a break"),
            (MORNING, "the person's first activity this morning"),
            (LONG_WORK, "the person has been working for a long stretch"),
        ] {
            let want = format!("[unprompted · {rule} · afternoon] {what}. Say one short, friendly line, or nothing at all.");
            assert_eq!(summary(rule, AFTERNOON, None), want);
        }
        assert_eq!(summary(BACK_FROM_IDLE, AFTERNOON, None), BACK_TEXT);
        // and a whole `Unprompted` with no voice set sends the same bytes
        let mut u = Unprompted::new(4);
        let sent = away_and_back(&mut u, 0, true, READY).expect("a check-in");
        assert_eq!(sent, BACK_TEXT);
        assert_eq!(u.started(), Some(BACK_FROM_IDLE));
        // a voice set and then cleared (a pack switch) is back to the plain text
        let mut u = Unprompted::new(4);
        u.set_voice(Some(voice_of(Default::default())));
        u.set_voice(None);
        assert_eq!(away_and_back(&mut u, 0, true, READY).as_deref(), Some(BACK_TEXT));
    }

    #[test]
    fn with_a_persona_the_check_in_speaks_in_its_voice() {
        let v = voice_of(Default::default());
        assert_eq!((v.name.as_str(), v.tagline.as_str()), ("Biscuit", "cheeky sidekick"));
        assert_eq!(v.gist, "Dry one-liners, kind teasing");
        let text = summary(BACK_FROM_IDLE, AFTERNOON, Some(&v));
        assert_eq!(
            text,
            "[unprompted · back_from_idle · afternoon] the person is back at the computer after a break. \
             Reply in Biscuit's voice (cheeky sidekick; Dry one-liners, kind teasing): one short line, or nothing at all."
        );
        assert!(!text.contains("friendly"), "the plain wording is gone: {text}");
        // it is the same text through `sample`
        let mut u = Unprompted::new(4);
        u.set_voice(Some(v));
        assert_eq!(away_and_back(&mut u, 0, true, READY), Some(text));
    }

    #[test]
    fn the_name_and_the_chattiness_directive_follow_the_persona() {
        let user = |c: f64, name: &str| mascot_core::persona::UserPersona { chattiness: Some(c), name: Some(name.into()), ..Default::default() };
        let quiet = summary(MORNING, AFTERNOON, Some(&voice_of(user(0.1, "Bun"))));
        assert!(quiet.contains("Bun's voice") && quiet.contains("one short sentence at most, and stay quiet unless it really helps") && quiet.ends_with("or nothing at all."), "{quiet}");
        let mid = summary(MORNING, AFTERNOON, Some(&voice_of(user(0.5, "Bun"))));
        assert!(mid.contains(": one short line, or nothing at all."), "{mid}");
        let loud = summary(MORNING, AFTERNOON, Some(&voice_of(user(0.9, "Bun"))));
        assert!(loud.contains("a short line or two, and speak up when you notice something"), "{loud}");
        assert!(quiet != mid && mid != loud && quiet != loud);
        // the person's notes never ride along
        let notes = mascot_core::persona::UserPersona { notes: Some("secret plans".into()), ..Default::default() };
        assert!(!summary(MORNING, AFTERNOON, Some(&voice_of(notes))).contains("secret"));
    }

    #[test]
    fn the_budget_and_debounce_are_the_same_with_a_voice() {
        let mut u = Unprompted::new(4);
        u.set_voice(Some(voice_of(Default::default())));
        let mut sent = vec![];
        for i in 0..5u64 {
            u.last_rule.clear();
            sent.push(away_and_back(&mut u, i * 12 * MIN, true, READY).is_some());
        }
        assert_eq!(sent, [true, true, true, true, false]);
        assert_eq!(u.used(4 * 12 * MIN + 10 * MIN + 1_000), 4);
        // a refused sample starts nothing
        let mut u = Unprompted::new(4);
        assert_eq!(away_and_back(&mut u, 0, false, READY), None);
        assert_eq!(u.started(), None);
    }
}
