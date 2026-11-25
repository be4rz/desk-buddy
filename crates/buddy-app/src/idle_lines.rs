//! Canned idle lines: now and then the bear says a line from its persona's
//! `idle_lines` with no model call. They are free: they never touch the check-in budget
//! (`unprompted.rs`, 4 an hour) and make no request.
//!
//! The moments: `morning`, `back` (back from a break) and `long_session` come from a cloned
//! `Reflexes` fed every sense sample (the rules `morning`, `back_from_idle`, `long_work`); `evening`
//! is its own window (18:00 to 22:00, once per local day, while the person is at the keyboard).
//!
//! A moment that fires is spent whether it speaks or not: a line is said right then or never,
//! and nothing is queued for later. It stays silent when
//! - do-not-disturb, quiet hours, fullscreen or a screen share is on (`!unprompted_allowed()`),
//! - the bear is not shown (hidden by the person or by do-not-disturb, or held still),
//! - a turn is running or any bubble is up,
//! - the cooldown (`40 - 20 * chattiness` minutes, ±10 %) has not passed since the last canned
//!   line or model check-in,
//! - the dice say no (`0.4 + 0.6 * chattiness`, a seeded `Rng`),
//! - a model check-in started for the same moment (`note_turn`), or
//! - the only line left would repeat the last one said.
//!
//! Variety memory lives in RAM only: no back-to-back repeat, unseen lines first. The lines carry
//! mood only (they are filled with the name, what it calls the person and the catchphrase, never
//! time or numbers). The log gets the moment's name and nothing else: never the line.

use std::collections::{HashMap, HashSet};

use mascot_core::reflex::{BACK_FROM_IDLE, LONG_WORK, MORNING};
use mascot_core::{EffectivePersona, LocalTime, Reflexes, Rng, Senses};

use crate::unprompted::{self, Ready, Unprompted};

/// The evening window, local minutes: from 18:00 up to 22:00.
pub const EVENING_FROM_MIN: u16 = 18 * 60;
pub const EVENING_UNTIL_MIN: u16 = 22 * 60;
/// At the keyboard: input within this long (the reflexes' `ACTIVE_MS`).
const ACTIVE_MS: u64 = mascot_core::reflex::ACTIVE_MS;
/// A model check-in for a moment keeps its canned line away this long.
const SAME_MOMENT_MS: u64 = unprompted::SAME_RULE_MS;

/// The reflex rule a persona moment is the cue for (`evening` has none).
pub fn rule_of(moment: &str) -> Option<&'static str> {
    match moment {
        "morning" => Some(MORNING),
        "back" => Some(BACK_FROM_IDLE),
        "long_session" => Some(LONG_WORK),
        _ => None,
    }
}

/// What the cooldown does around its base: `base * (0.9 + 0.2 * unit)`.
const JITTER_LOW: f64 = 0.9;
const JITTER_SPAN: f64 = 0.2;

/// The persona moment (`mascot_core::persona::MOMENTS`) a reflex rule is the cue for.
pub fn moment_of(rule: &str) -> Option<&'static str> {
    match rule {
        MORNING => Some("morning"),
        BACK_FROM_IDLE => Some("back"),
        LONG_WORK => Some("long_session"),
        _ => None,
    }
}

/// Minutes between canned lines before the jitter: 40 at chattiness 0 down to 20 at 1.
pub fn cooldown_ms(chattiness: f64) -> u64 {
    ((40.0 - 20.0 * chattiness.clamp(0.0, 1.0)) * 60_000.0) as u64
}

/// The chance a moment that may speak does: 0.4 at chattiness 0 up to 1.0 at 1.
pub fn say_probability(chattiness: f64) -> f64 {
    0.4 + 0.6 * chattiness.clamp(0.0, 1.0)
}

/// What the app knows now that the senses don't say.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    /// `Brain::unprompted_allowed()`: no do-not-disturb, no quiet hours.
    pub allowed: bool,
    /// The bear is on screen and moving: the brain shows it and the person has not hidden it.
    pub shown: bool,
    /// A bubble is up (any) or a turn is running.
    pub busy: bool,
}

/// A line to say now.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub moment: &'static str,
    pub text: String,
}

#[derive(Clone)]
pub struct IdleLines {
    /// The same rules as `Unprompted`'s own copy, so a moment fires on the same sample.
    reflex: Reflexes,
    rng: Rng,
    persona: Option<EffectivePersona>,
    /// Local days seen (bumps when the weekday changes) and the day the evening line was spent.
    weekday: Option<u8>,
    day: u32,
    evening_day: Option<u32>,
    /// When something last spoke on its own (a canned line or a model check-in), and the cooldown
    /// drawn for it.
    last_spoke: Option<u64>,
    cooldown: u64,
    /// When a model check-in last started for a moment.
    model: HashMap<&'static str, u64>,
    /// The lines said per moment since all of them were, and the last line said.
    seen: HashMap<&'static str, HashSet<String>>,
    last: Option<String>,
}

impl IdleLines {
    pub fn new(seed: u64) -> IdleLines {
        IdleLines {
            reflex: Reflexes::new(),
            rng: Rng::new(seed),
            persona: None,
            weekday: None,
            day: 0,
            evening_day: None,
            last_spoke: None,
            cooldown: 0,
            model: HashMap::new(),
            seen: HashMap::new(),
            last: None,
        }
    }

    /// The persona in force (`None`: this mascot has none, so no canned lines). Cooldown and variety
    /// memory carry over: a changed voice does not make the bear chattier.
    pub fn set_persona(&mut self, persona: Option<EffectivePersona>) {
        self.persona = persona;
    }

    fn chattiness(&self) -> f64 {
        self.persona.as_ref().map_or(0.5, |p| p.temperament.chattiness)
    }

    fn start_cooldown(&mut self, now: u64) {
        let base = cooldown_ms(self.chattiness()) as f64;
        self.cooldown = (base * (JITTER_LOW + JITTER_SPAN * self.rng.unit())) as u64;
        self.last_spoke = Some(now);
    }

    /// A model check-in started for `rule` (`Unprompted::started`): its canned line is dropped, and
    /// the cooldown starts, so nothing canned follows right behind it.
    pub fn note_turn(&mut self, rule: &str, now: u64) {
        if self.persona.is_none() {
            return;
        }
        if let Some(m) = moment_of(rule) {
            self.model.insert(m, now);
        }
        self.start_cooldown(now);
    }

    /// One sense sample (the gated `Senses`). `Some` = say this line now.
    pub fn sample(&mut self, s: &Senses, now: u64, env: Env) -> Option<Line> {
        self.track_day(s.local);
        // the rules run with nothing off and nothing busy: a moment is fired, then kept or dropped
        // below, never held for later (an edge replayed late would greet a return long past)
        let fired = self.reflex.sample(s, now, false, true, 0.0, &mut |_| false);
        let mut moment = fired.and_then(|f| moment_of(f.rule));
        if moment.is_none() && self.evening_due(s) {
            self.evening_day = Some(self.day);
            moment = Some("evening");
        }
        let moment = moment?;
        if !env.allowed || unprompted::paused(s).is_some() || !env.shown || env.busy {
            return None;
        }
        self.persona.as_ref()?;
        if self.model.get(moment).is_some_and(|&t| now.saturating_sub(t) < SAME_MOMENT_MS) {
            return None;
        }
        if self.last_spoke.is_some_and(|t| now.saturating_sub(t) < self.cooldown) {
            return None;
        }
        // the dice come before the pick: a "no" costs one draw and starts no cooldown
        if !self.rng.chance(say_probability(self.chattiness())) {
            return None;
        }
        let text = self.pick(moment)?;
        self.start_cooldown(now);
        Some(Line { moment, text })
    }

    fn track_day(&mut self, local: LocalTime) {
        match self.weekday {
            Some(w) if w != local.weekday => self.day += 1,
            _ => {}
        }
        self.weekday = Some(local.weekday);
    }

    /// The evening window is open for a person at the keyboard and today's evening is unspent.
    fn evening_due(&self, s: &Senses) -> bool {
        (EVENING_FROM_MIN..EVENING_UNTIL_MIN).contains(&s.local.minutes) && s.user_idle_ms < ACTIVE_MS && self.evening_day != Some(self.day)
    }

    /// A line of `moment`: never the one said last, unseen ones first. `None` when there is nothing
    /// but a repeat (or no line at all).
    fn pick(&mut self, moment: &'static str) -> Option<String> {
        let mut lines = self.persona.as_ref()?.idle(moment);
        lines.dedup();
        let fresh: Vec<String> = lines.into_iter().filter(|l| self.last.as_ref() != Some(l)).collect();
        if fresh.is_empty() {
            return None;
        }
        let seen = self.seen.entry(moment).or_default();
        let unseen: Vec<&String> = fresh.iter().filter(|l| !seen.contains(*l)).collect();
        let pool: Vec<&String> = if unseen.is_empty() {
            // all of them have had their turn: start the round again
            seen.clear();
            fresh.iter().collect()
        } else {
            unseen
        };
        let text = pool[self.rng.below(pool.len() as u64) as usize].clone();
        seen.insert(text.clone());
        self.last = Some(text.clone());
        Some(text)
    }
}

/// What one sense sample does: a model check-in to start (budgeted, `Unprompted`) and a canned line
/// to say (free, `IdleLines`). The two never both speak for the same moment.
#[derive(Debug, Default, PartialEq)]
pub struct Step {
    pub check_in: Option<String>,
    pub line: Option<Line>,
}

/// The driver's sense path: the budgeted check-in first, then the canned line (dropped when the
/// check-in started for the same moment). A canned line that speaks spends its moment for the
/// check-in too, so one held back for want of a model is not said again once one is running.
pub fn step(u: &mut Unprompted, il: &mut IdleLines, s: &Senses, now: u64, env: Env, ready: Ready) -> Step {
    let check_in = u.sample(s, now, env.allowed, ready);
    if let Some(rule) = u.started() {
        il.note_turn(rule, now);
    }
    let line = il.sample(s, now, Env { busy: env.busy || ready.busy, ..env });
    if let Some(rule) = line.as_ref().and_then(|l| rule_of(l.moment)) {
        u.spoken_for(rule, now);
    }
    Step { check_in, line }
}

#[cfg(test)]
mod tests;
