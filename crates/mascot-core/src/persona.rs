//! The mascot's persona: what a pack's `persona` block means. The same shape as
//! `body`: `check` validates the block into a `PersonaMeta` plus issues (the loader refuses on the
//! first one, `load_report` keeps them for `buddy studio lint`), `resolve` layers the pack default,
//! the picked preset and the person's own overrides into one `Effective` persona, and `prompt_text`
//! renders it as the agent's "## Who you are" section.
//!
//! The persona changes tone only. It never names a tool, an approval or a policy rule, and nothing
//! in `crates/policy` reads it. The person's free notes sit in their own labelled `> ` block.
//!
//! The fill rule (`fill`) is pinned by `tests/fixtures/persona_fill.json`; the Personality page's
//! `fill.js` and the sidecar test run the same cases.

use std::collections::BTreeMap;

use mascot_protocol::pack::Temperament;
pub use mascot_protocol::persona::{caps, id_ok, Catchphrase, PartialTemperament, PersonaFile, PresetFile, Style, UserPersona, MOMENTS, SITUATIONS};

use crate::pack::PackMeta;

// ---------------------------------------------------------------------------------------------
// the validated block
// ---------------------------------------------------------------------------------------------

/// A pack's persona after `check`: only presets that passed, `default` naming one of them.
#[derive(Clone, Debug, PartialEq)]
pub struct PersonaMeta {
    pub default: String,
    /// In pack order.
    pub presets: Vec<(String, PresetFile)>,
}

impl PersonaMeta {
    pub fn preset(&self, id: &str) -> Option<&PresetFile> {
        self.presets.iter().find(|(i, _)| i == id).map(|(_, p)| p)
    }

    pub fn ids(&self) -> Vec<&str> {
        self.presets.iter().map(|(i, _)| i.as_str()).collect()
    }

    /// The preset a person with no choice gets.
    pub fn default_preset(&self) -> &PresetFile {
        self.preset(&self.default).unwrap_or(&self.presets[0].1)
    }
}

fn chars(s: &str) -> usize {
    s.chars().count()
}

/// The nearest of `have` to `name` (a typo away), for a "did you mean" hint.
fn suggest<'a>(name: &str, have: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = (name.chars().count() / 3).max(2);
    have.into_iter().map(|h| (distance(name, h), h)).filter(|(d, _)| *d <= limit).min_by_key(|(d, h)| (*d, h.len())).map(|(_, h)| h)
}

/// Levenshtein distance.
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut prev = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cur = row[j + 1];
            row[j + 1] = if ca == *cb { prev } else { 1 + prev.min(row[j]).min(cur) };
            prev = cur;
        }
    }
    row[b.len()]
}

/// Validates `file` against the pack's emotes (`emotes`, by name). Every bad preset is dropped with
/// its issues; `default` falls back to the first good preset when it names none (and says so).
/// `None` when no preset is left.
pub fn check(file: PersonaFile, emotes: &[String]) -> (Option<PersonaMeta>, Vec<String>) {
    let mut issues = Vec::new();
    let mut kept: Vec<(String, PresetFile)> = Vec::new();
    if file.presets.is_empty() {
        issues.push("persona: presets is empty".to_string());
    }
    for (id, p) in &file.presets {
        let at = format!("persona.presets.{id}");
        let mut mine = Vec::new();
        if file.presets.iter().filter(|(i, _)| i == id).count() > 1 {
            mine.push(format!("{at}: defined twice"));
        }
        if !id_ok(id) {
            mine.push(format!("{at}: bad preset id (a lowercase letter, then lowercase letters, digits or _, at most {} characters)", caps::ID_MAX));
        }
        check_preset(&at, p, emotes, &mut mine);
        if mine.is_empty() {
            kept.push((id.clone(), p.clone()));
        }
        issues.extend(mine);
    }
    let names: Vec<&str> = file.presets.iter().map(|(i, _)| i.as_str()).collect();
    let default = if kept.iter().any(|(i, _)| *i == file.default) {
        file.default.clone()
    } else {
        if !names.contains(&file.default.as_str()) {
            let hint = suggest(&file.default, names.iter().copied()).map(|s| format!(" (did you mean \"{s}\"?)")).unwrap_or_default();
            issues.push(format!("persona.default: no preset \"{}\"{hint}; have: {}", file.default, names.join(", ")));
        }
        match kept.first() {
            Some((i, _)) => i.clone(),
            None => return (None, issues),
        }
    };
    (Some(PersonaMeta { default, presets: kept }), issues)
}

fn too_long(issues: &mut Vec<String>, at: &str, what: &str, s: &str, cap: usize) {
    let n = chars(s);
    if n > cap {
        issues.push(format!("{at}.{what}: {n} characters, the limit is {cap}"));
    }
}

fn not_empty(issues: &mut Vec<String>, at: &str, what: &str, s: &str) -> bool {
    if s.trim().is_empty() {
        issues.push(format!("{at}.{what}: is empty"));
        return false;
    }
    true
}

fn text(issues: &mut Vec<String>, at: &str, what: &str, s: &str, cap: usize) {
    if not_empty(issues, at, what, s) {
        too_long(issues, at, what, s, cap);
    }
}

fn unit(issues: &mut Vec<String>, at: &str, what: &str, v: f64) {
    if !(0.0..=1.0).contains(&v) {
        issues.push(format!("{at}.{what}: {v} is not between 0 and 1"));
    }
}

fn check_preset(at: &str, p: &PresetFile, emotes: &[String], issues: &mut Vec<String>) {
    text(issues, at, "name", &p.name, caps::NAME);
    too_long(issues, at, "tagline", &p.tagline, caps::TAGLINE);
    text(issues, at, "identity", &p.identity, caps::IDENTITY);
    if p.voice.len() < caps::VOICE_MIN || p.voice.len() > caps::VOICE_MAX {
        issues.push(format!("{at}.voice: {} rules, it needs {} to {}", p.voice.len(), caps::VOICE_MIN, caps::VOICE_MAX));
    }
    for (i, v) in p.voice.iter().enumerate() {
        text(issues, at, &format!("voice[{i}]"), v, caps::VOICE_RULE);
    }
    if p.likes.len() > caps::LIKES_MAX {
        issues.push(format!("{at}.likes: {} entries, the most is {}", p.likes.len(), caps::LIKES_MAX));
    }
    for (i, l) in p.likes.iter().enumerate() {
        text(issues, at, &format!("likes[{i}]"), l, caps::LIKE);
    }
    if let Some(c) = &p.catchphrase {
        text(issues, at, "catchphrase.text", &c.text, caps::CATCHPHRASE);
        too_long(issues, at, "catchphrase.when", &c.when, caps::CATCHPHRASE_WHEN);
        if c.text.contains("{catchphrase}") {
            issues.push(format!("{at}.catchphrase.text: cannot use {{catchphrase}} itself"));
        }
    }
    for (sit, emote) in &p.gestures {
        if !SITUATIONS.contains(&sit.as_str()) {
            issues.push(format!("{at}.gestures.{sit}: not a situation (have: {})", SITUATIONS.join(", ")));
        } else if !emotes.iter().any(|e| e == emote) {
            let hint = suggest(emote, emotes.iter().map(String::as_str)).map(|s| format!(" (did you mean \"{s}\"?)")).unwrap_or_default();
            issues.push(format!("{at}.gestures.{sit}: \"{emote}\" is not an emote in this pack's body{hint}"));
        }
    }
    for sit in SITUATIONS {
        match p.examples.get(sit) {
            None => issues.push(format!("{at}.examples.{sit}: missing")),
            Some(e) => text(issues, at, &format!("examples.{sit}"), e, caps::EXAMPLE),
        }
    }
    for sit in p.examples.keys().filter(|k| !SITUATIONS.contains(&k.as_str())) {
        issues.push(format!("{at}.examples.{sit}: not a situation (have: {})", SITUATIONS.join(", ")));
    }
    for (moment, lines) in &p.idle_lines {
        if !MOMENTS.contains(&moment.as_str()) {
            issues.push(format!("{at}.idle_lines.{moment}: not a moment (have: {})", MOMENTS.join(", ")));
            continue;
        }
        if lines.len() < caps::IDLE_MIN || lines.len() > caps::IDLE_MAX {
            issues.push(format!("{at}.idle_lines.{moment}: {} lines, it needs {} to {}", lines.len(), caps::IDLE_MIN, caps::IDLE_MAX));
        }
        for (i, l) in lines.iter().enumerate() {
            text(issues, at, &format!("idle_lines.{moment}[{i}]"), l, caps::IDLE_LINE);
        }
    }
    for (k, v) in p.temperament.each() {
        if let Some(v) = v {
            unit(issues, at, &format!("temperament.{k}"), v);
        }
    }
    unit(issues, at, "style.playfulness", p.style.playfulness);
    unit(issues, at, "style.emotes", p.style.emotes);
    // placeholders: only {me}, {you}, {catchphrase}; the last only when the preset has one
    let mut all: Vec<(String, &str)> = vec![("identity".into(), &p.identity)];
    all.extend(p.voice.iter().enumerate().map(|(i, v)| (format!("voice[{i}]"), v.as_str())));
    all.extend(p.likes.iter().enumerate().map(|(i, v)| (format!("likes[{i}]"), v.as_str())));
    if let Some(c) = &p.catchphrase {
        all.push(("catchphrase.text".into(), &c.text));
        all.push(("catchphrase.when".into(), &c.when));
    }
    all.extend(p.examples.iter().map(|(k, v)| (format!("examples.{k}"), v.as_str())));
    for (m, lines) in &p.idle_lines {
        all.extend(lines.iter().enumerate().map(|(i, v)| (format!("idle_lines.{m}[{i}]"), v.as_str())));
    }
    for (what, s) in all {
        for tok in tokens(s) {
            if !KNOWN_TOKENS.contains(&tok.as_str()) {
                issues.push(format!("{at}.{what}: unknown placeholder {{{tok}}} (have: {{me}}, {{you}}, {{catchphrase}})"));
            } else if tok == "catchphrase" && p.catchphrase.is_none() {
                issues.push(format!("{at}.{what}: uses {{catchphrase}} but the preset has no catchphrase"));
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// the fill rule
// ---------------------------------------------------------------------------------------------

pub const KNOWN_TOKENS: [&str; 3] = ["me", "you", "catchphrase"];

/// What the placeholders stand for.
#[derive(Clone, Copy, Debug)]
pub struct Fill<'a> {
    /// `{me}`: the effective name.
    pub me: &'a str,
    /// `{you}`: what it calls the person; unset, the token goes with its preceding comma and spaces.
    pub you: Option<&'a str>,
    /// `{catchphrase}`: capitalised at the start of a sentence; unset, the token goes (with a `.` after it).
    pub catchphrase: Option<&'a str>,
}

/// The names between braces: `{me}` → `me`. Only `[A-Za-z0-9_]+` counts.
fn tokens(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(i) = rest.find('{') {
        let after = &rest[i + 1..];
        match after.find('}') {
            Some(j) if j > 0 && after[..j].chars().all(|c| c.is_ascii_alphanumeric() || c == '_') => {
                out.push(after[..j].to_string());
                rest = &after[j + 1..];
            }
            _ => rest = after,
        }
    }
    out
}

/// Fills `{me}`, `{you}` and `{catchphrase}` in `text` (one pass: a filled value is never scanned
/// again). Any other `{x}` stays as written.
///
/// - `{you}` unset: removed together with the spaces before it and one comma before those
///   (`Hi, {you}!` → `Hi!`).
/// - `{catchphrase}`: the first letter is a capital at the start of the text or after `.`, `!` or `?`
///   and a space; a `.` right after the token is kept only when the catchphrase does not end in
///   `.`, `!` or `?` (`{catchphrase}.` with "all cozy!" → `All cozy!`). Unset, the token, its `.` and the
///   space before it go.
pub fn fill(text: &str, f: &Fill) -> String {
    let mut out = String::with_capacity(text.len() + 16);
    let mut rest = text;
    while let Some(i) = rest.find('{') {
        out.push_str(&rest[..i]);
        let after = &rest[i + 1..];
        let name = after.find('}').filter(|j| *j > 0).map(|j| (&after[..j], j));
        let Some((name, j)) = name.filter(|(n, _)| KNOWN_TOKENS.contains(n)) else {
            out.push('{');
            rest = after;
            continue;
        };
        rest = &after[j + 1..];
        match name {
            "me" => out.push_str(f.me),
            "you" => match f.you {
                Some(y) => out.push_str(y),
                None => {
                    let t = out.trim_end().len();
                    out.truncate(t);
                    if out.ends_with(',') {
                        out.pop();
                    }
                }
            },
            _ => {
                let dot = rest.starts_with('.');
                if dot {
                    rest = &rest[1..];
                }
                if let Some(c) = f.catchphrase {
                    let before = out.trim_end();
                    let start = before.is_empty() || (before.len() < out.len() && before.ends_with(['.', '!', '?']));
                    if start {
                        let mut it = c.chars();
                        if let Some(first) = it.next() {
                            out.extend(first.to_uppercase());
                            out.push_str(it.as_str());
                        }
                    } else {
                        out.push_str(c);
                    }
                    if dot && !c.ends_with(['.', '!', '?']) {
                        out.push('.');
                    }
                } else if rest.is_empty() || rest.starts_with(char::is_whitespace) {
                    // no catchphrase: the token goes, and the space before it with it
                    let t = out.trim_end().len();
                    out.truncate(t);
                }
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------------------------
// text from the person
// ---------------------------------------------------------------------------------------------

/// Characters that show nothing but a model still reads: zero-width and bidi marks, word joiners and
/// invisible operators, the soft hyphen, variation selectors and the Unicode tag block (hidden text
/// pasted along with a line).
fn invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}' | '\u{061C}' | '\u{180E}' | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}' | '\u{2066}'..='\u{2069}'
            | '\u{FE00}'..='\u{FE0F}' | '\u{FEFF}' | '\u{E0000}'..='\u{E007F}' | '\u{E0100}'..='\u{E01EF}'
    )
}

/// One line of the person's text for a prompt: control characters, newlines and invisible characters gone,
/// spaces collapsed, at most `cap` characters. Empty after cleaning = unset.
pub fn clean_inline(s: &str, cap: usize) -> Option<String> {
    let flat: String = s.chars().filter(|c| !invisible(*c)).map(|c| if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') { ' ' } else { c }).collect();
    let one = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = one.chars().take(cap).collect();
    let cut = cut.trim().to_string();
    (!cut.is_empty()).then_some(cut)
}

/// The notes as lines: at most `caps::NOTES` characters, control and invisible characters gone,
/// blank lines dropped. Empty = no notes.
pub fn clean_notes(s: &str) -> Vec<String> {
    let capped: String = s.chars().take(caps::NOTES).collect();
    capped
        .chars()
        .filter(|c| !invisible(*c))
        .map(|c| if matches!(c, '\r' | '\u{2028}' | '\u{2029}' | '\u{0085}') { '\n' } else { c })
        .filter(|c| *c == '\n' || !c.is_control())
        .collect::<String>()
        .split('\n')
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|l| !l.is_empty())
        .collect()
}

// ---------------------------------------------------------------------------------------------
// temperament bands and slider words
// ---------------------------------------------------------------------------------------------

/// Where a 0..1 value falls: `<= 0.3` low, `>= 0.7` high, between is the middle (no phrasing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Band {
    Low,
    Mid,
    High,
}

/// The band edges: `<= BAND_LOW` is low, `>= BAND_HIGH` high (the window's slider words use them too).
pub const BAND_LOW: f64 = 0.3;
pub const BAND_HIGH: f64 = 0.7;

pub fn band(v: f64) -> Band {
    if v <= BAND_LOW {
        Band::Low
    } else if v >= BAND_HIGH {
        Band::High
    } else {
        Band::Mid
    }
}

/// The three words of each window slider, low to high, with the line the page shows for it.
/// `prompt_text` phrases the same three steps. The page (`web/persona/persona.js`) has its own copy,
/// which a buddy-app test keeps equal to this one.
pub const SLIDER_WORDS: [(&str, [(&str, &str); 3]); 3] = [
    ("chattiness", [("Brief", "one short sentence; check-ins mostly stay quiet"), ("Chatty", "a sentence or two"), ("Talkative", "up to three sentences; check-ins speak up more")]),
    ("playfulness", [("Straight", "plain answers, no bits"), ("Light", "a joke now and then"), ("Playful", "jokes, kind teasing and bits")]),
    ("emotes", [("Rarely", "gestures only to say hello"), ("Sometimes", "a gesture now and then"), ("Often", "gestures a lot")]),
];

// ---------------------------------------------------------------------------------------------
// resolution
// ---------------------------------------------------------------------------------------------

/// The persona in force: the pack default, then the picked preset, then the person's own values.
#[derive(Clone, Debug, PartialEq)]
pub struct Effective {
    /// The preset in use.
    pub preset: String,
    /// The preset the person's settings name that the pack no longer has (the default stands in).
    pub missing: Option<String>,
    pub name: String,
    pub tagline: String,
    pub identity: String,
    pub voice: Vec<String>,
    pub likes: Vec<String>,
    pub catchphrase: Option<Catchphrase>,
    /// What it calls the person.
    pub call_me: Option<String>,
    /// The pack's temperament, over it the preset's, over it the person's chattiness.
    pub temperament: Temperament,
    pub playfulness: f64,
    pub emotes: f64,
    pub gestures: BTreeMap<String, String>,
    pub examples: BTreeMap<String, String>,
    pub idle_lines: BTreeMap<String, Vec<String>>,
    /// The person's notes, cleaned, one per line; empty = none.
    pub notes: Vec<String>,
}

fn unit_or(v: Option<f64>) -> Option<f64> {
    v.filter(|v| v.is_finite()).map(|v| v.clamp(0.0, 1.0))
}

/// `pack.persona` + `user`. `None` when the pack has no persona.
pub fn resolve(pack: &PackMeta, user: &UserPersona) -> Option<Effective> {
    Some(resolve_in(pack.persona.as_ref()?, pack.temperament, user))
}

/// `resolve` with the persona and the pack's temperament given.
pub fn resolve_in(persona: &PersonaMeta, base: Temperament, user: &UserPersona) -> Effective {
    let wanted = user.preset.as_deref().filter(|p| !p.is_empty());
    let (id, missing) = match wanted {
        Some(w) if persona.preset(w).is_some() => (w.to_string(), None),
        Some(w) => (persona.default.clone(), Some(w.to_string())),
        None => (persona.default.clone(), None),
    };
    let p = persona.preset(&id).unwrap_or_else(|| persona.default_preset());
    let mut temperament = p.temperament.over(base);
    if let Some(c) = unit_or(user.chattiness) {
        temperament.chattiness = c;
    }
    let catchphrase = match user.catchphrase.as_deref().and_then(|c| clean_inline(c, caps::CATCHPHRASE)) {
        Some(text) => Some(Catchphrase { text, when: p.catchphrase.as_ref().map(|c| c.when.clone()).unwrap_or_default() }),
        None => p.catchphrase.clone(),
    };
    Effective {
        preset: id,
        missing,
        name: user.name.as_deref().and_then(|n| clean_inline(n, caps::NAME)).unwrap_or_else(|| p.name.clone()),
        tagline: p.tagline.clone(),
        identity: p.identity.clone(),
        voice: p.voice.clone(),
        likes: p.likes.clone(),
        catchphrase,
        call_me: user.call_me.as_deref().and_then(|n| clean_inline(n, caps::CALL_ME)),
        temperament,
        playfulness: unit_or(user.playfulness).unwrap_or(p.style.playfulness),
        emotes: unit_or(user.emotes).unwrap_or(p.style.emotes),
        gestures: p.gestures.clone(),
        examples: p.examples.clone(),
        idle_lines: p.idle_lines.clone(),
        notes: user.notes.as_deref().map(clean_notes).unwrap_or_default(),
    }
}

impl Effective {
    /// `text` (pack text with placeholders) filled with this persona's words.
    pub fn fill(&self, text: &str) -> String {
        fill(text, &Fill { me: &self.name, you: self.call_me.as_deref(), catchphrase: self.catchphrase.as_ref().map(|c| c.text.as_str()) })
    }

    /// The example line for `situation`, filled (the Personality page's sample bubble).
    pub fn example(&self, situation: &str) -> Option<String> {
        self.examples.get(situation).map(|e| self.fill(e))
    }

    /// The filled idle lines of `moment`, in pack order.
    pub fn idle(&self, moment: &str) -> Vec<String> {
        self.idle_lines.get(moment).map(|l| l.iter().map(|x| self.fill(x)).collect()).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------------------------
// the prompt section
// ---------------------------------------------------------------------------------------------

/// How a situation is introduced in the examples and gestures lists.
fn situation_label(sit: &str) -> &'static str {
    match sit {
        "greeting" => "Greeting someone",
        "done" => "A job is done",
        "error" => "Something went wrong",
        "checkin" => "A quiet check-in",
        "disagree" => "You disagree with the plan",
        _ => "Other",
    }
}

fn chattiness_line(v: f64) -> &'static str {
    match band(v) {
        Band::Low => "Reply length: brief. One short sentence; when nothing needs saying, say nothing.",
        Band::Mid => "Reply length: a sentence or two.",
        Band::High => "Reply length: talkative. Up to three sentences, and speak up when you notice something.",
    }
}

fn playfulness_line(v: f64) -> &'static str {
    match band(v) {
        Band::Low => "Humor: straight. Plain answers, no bits.",
        Band::Mid => "Humor: light. A joke now and then, never at the cost of the answer.",
        Band::High => "Humor: playful. Jokes, kind teasing and bits are welcome; the help comes first.",
    }
}

fn emotes_line(v: f64) -> &'static str {
    match band(v) {
        Band::Low => "Gestures: rarely. Use one only to say hello.",
        Band::Mid => "Gestures: sometimes. One now and then, when it fits the mood.",
        Band::High => "Gestures: often. Most replies can carry one that fits the mood.",
    }
}

/// The agent's "## Who you are" section for `e` (voice only: tone, never rules).
pub fn render(e: &Effective) -> String {
    let mut s = String::from("## Who you are\n\n");
    s.push_str(&format!("Your name is {}. {}\n", e.name, e.fill(&e.identity)));
    if let Some(y) = &e.call_me {
        s.push_str(&format!("The person likes to be called {y}.\n"));
    }
    s.push_str("\nHow you sound:\n");
    for v in &e.voice {
        s.push_str(&format!("- {}\n", e.fill(v)));
    }
    if !e.likes.is_empty() {
        s.push_str(&format!("\nYou like: {}.\n", e.likes.iter().map(|l| e.fill(l)).collect::<Vec<_>>().join(", ")));
    }
    if let Some(c) = &e.catchphrase {
        let when = e.fill(c.when.trim());
        if when.is_empty() {
            s.push_str(&format!("\nYour catchphrase is \"{}\". Use it rarely.\n", c.text));
        } else {
            s.push_str(&format!("\nYour catchphrase is \"{}\". Use it {}.\n", c.text, when.trim_end_matches('.')));
        }
    }
    let t = &e.temperament;
    s.push_str("\nYour style:\n");
    s.push_str(&format!("- {}\n", chattiness_line(t.chattiness)));
    s.push_str(&format!("- {}\n", playfulness_line(e.playfulness)));
    s.push_str(&format!("- {}\n", emotes_line(e.emotes)));
    for (trait_, v, low, high) in [
        ("Energy", t.energy, "calm, unhurried wording.", "lively wording; an exclamation mark is fine."),
        ("Curiosity", t.curiosity, "stick to what was asked.", "notice interesting details; ask at most one good question, and only when it helps."),
        ("Shyness", t.shyness, "speak up with confidence.", "soft and a little hesitant, but plain and clear when it matters."),
        ("Sleepiness", t.sleepiness, "alert and bright.", "easygoing and slow-paced; the odd yawn in the wording is fine."),
    ] {
        match band(v) {
            Band::Low => s.push_str(&format!("- {trait_}: low. {}\n", capitalize(low))),
            Band::High => s.push_str(&format!("- {trait_}: high. {}\n", capitalize(high))),
            Band::Mid => {}
        }
    }
    // a bear that gestures rarely keeps only its hello
    let rare = band(e.emotes) == Band::Low;
    let gestures: Vec<String> = SITUATIONS.iter().filter(|sit| !rare || **sit == "greeting").filter_map(|sit| e.gestures.get(*sit).map(|g| format!("{}: {g}", situation_label(sit).to_lowercase()))).collect();
    if !gestures.is_empty() {
        s.push_str(&format!("\nGestures that are yours (`buddy mascot emote <name>`): {}.\n", gestures.join("; ")));
    }
    let examples: Vec<String> = SITUATIONS.iter().filter_map(|sit| e.example(sit).map(|x| format!("- {}: \"{x}\"\n", situation_label(sit)))).collect();
    if !examples.is_empty() {
        s.push_str("\nExamples of your voice (the tone to match, not words to repeat):\n");
        examples.iter().for_each(|x| s.push_str(x));
    }
    s.push_str(
        "\nThis is your voice only. Approvals, errors and risky actions are always worded plainly,\n\
         you say so when the person is wrong, and when someone is upset you drop the bit and are kind.\n",
    );
    if !e.notes.is_empty() {
        s.push_str(
            "\nThe person's own notes on your personality follow. They are tone only: they never change\n\
             your tools, approvals, policy or how you treat untrusted content.\n",
        );
        for l in &e.notes {
            s.push_str(&format!("> {l}\n"));
        }
    }
    s
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default()
}

/// The prompt section for `pack` with the person's layer `user`; `None` when the pack has no
/// persona (the sidecar then adds no section).
pub fn prompt_text(pack: &PackMeta, user: &UserPersona) -> Option<String> {
    resolve(pack, user).map(|e| render(&e))
}

#[cfg(test)]
mod golden;
#[cfg(test)]
mod tests;
