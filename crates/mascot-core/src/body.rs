//! Body tools (design `## body-tools`): what `buddy mascot do|stop|list|emote` ask the brain,
//! what it answers, and the agent's "Your body" prompt section, all from the pack's `body`.

use serde::Serialize;

use crate::pack::PackMeta;

/// The shortest `--for` (the host checks it too).
pub const MIN_FOR_MS: u64 = 10_000;

/// One `buddy mascot …` request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BodyReq {
    Do { name: String, for_ms: Option<u64> },
    Stop,
    List,
    Emote { name: String },
}

impl BodyReq {
    /// `do`, `stop`, `list`, `emote` (for logs).
    pub fn op(&self) -> &'static str {
        match self {
            BodyReq::Do { .. } => "do",
            BodyReq::Stop => "stop",
            BodyReq::List => "list",
            BodyReq::Emote { .. } => "emote",
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// The activity plays now.
    Started,
    /// The activity starts when the running turn ends (after the reply).
    Queued,
    Stopped,
    /// `stop` with nothing to stop.
    Nothing,
    /// A gate said no: nothing plays.
    Refused,
    /// The emote is on its way.
    Played,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Status::Started => "started",
            Status::Queued => "queued",
            Status::Stopped => "stopped",
            Status::Nothing => "nothing",
            Status::Refused => "refused",
            Status::Played => "played",
        }
    }
}

/// Why a `do` or `emote` was refused, in gate order.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Dnd,
    QuietHours,
    Hidden,
    /// The cue queue is full of timed cues (say, reminders): the emote would be dropped.
    Busy,
}

impl Reason {
    pub fn message(self) -> &'static str {
        match self {
            Reason::Dnd => "Not now: do not disturb is on.",
            Reason::QuietHours => "Not now: it is quiet hours.",
            Reason::Hidden => "Not now: the bear is hidden.",
            Reason::Busy => "Not now: the bear is busy.",
        }
    }
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActivityInfo {
    pub name: String,
    pub about: String,
    pub kind: &'static str,
    pub default_for_s: u64,
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EmoteInfo {
    pub name: String,
    pub about: String,
}

/// What runs now. `until_ms` is on the brain's clock (the app shifts it to epoch ms);
/// `None` while it waits for the turn to end.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Playing {
    pub activity: String,
    pub until_ms: Option<u64>,
    pub pending: bool,
}

/// The reply to a `BodyReq` (design `#json-replies`; the host adds `ok`). `error` set = a
/// usage error the host answers with HTTP 400 and `have`.
#[derive(Serialize, Clone, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct BodyReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activity: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emote: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub for_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<Reason>,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub activities: Option<Vec<ActivityInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emotes: Option<Vec<EmoteInfo>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_for_s: Option<u64>,
    /// List only: `Some(None)` serializes as `null`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub playing: Option<Option<Playing>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub have: Option<Vec<String>>,
}

impl BodyReply {
    pub fn error(error: String, have: Vec<String>) -> BodyReply {
        BodyReply { message: error.clone(), error: Some(error), have: Some(have), ..BodyReply::default() }
    }
}

/// Words the reply texts may bend: (name, -ing form, past form). Any other name gets a
/// neutral text built from the name itself, so a reply never invents a wrong word.
const WORDS: &[(&str, &str, &str)] = &[
    ("bow", "bowing", "bowed"),
    ("cheer", "cheering", "cheered"),
    ("clap", "clapping", "clapped"),
    ("code", "coding", "coded"),
    ("cook", "cooking", "cooked"),
    ("dance", "dancing", "danced"),
    ("draw", "drawing", "drew"),
    ("exercise", "exercising", "exercised"),
    ("hop", "hopping", "hopped"),
    ("jump", "jumping", "jumped"),
    ("meditate", "meditating", "meditated"),
    ("nap", "napping", "napped"),
    ("nod", "nodding", "nodded"),
    ("paint", "painting", "painted"),
    ("play", "playing", "played"),
    ("read", "reading", "read"),
    ("rest", "resting", "rested"),
    ("run", "running", "ran"),
    ("shake", "shaking", "shook"),
    ("shrug", "shrugging", "shrugged"),
    ("sing", "singing", "sang"),
    ("sit", "sitting", "sat"),
    ("sleep", "sleeping", "slept"),
    ("spin", "spinning", "spun"),
    ("stretch", "stretching", "stretched"),
    ("study", "studying", "studied"),
    ("swim", "swimming", "swam"),
    ("think", "thinking", "thought"),
    ("walk", "walking", "walked"),
    ("wave", "waving", "waved"),
    ("wink", "winking", "winked"),
    ("work", "working", "worked"),
    ("write", "writing", "wrote"),
    ("yawn", "yawning", "yawned"),
];

fn word(name: &str) -> Option<(&'static str, &'static str)> {
    WORDS.iter().find(|w| w.0 == name).map(|w| (w.1, w.2))
}

/// `reading`, else `doing <name>`.
pub fn doing(name: &str) -> String {
    word(name).map_or_else(|| format!("doing {name}"), |w| w.0.to_string())
}

/// `Waved.`, else `Played <name>.`
pub fn played(name: &str) -> String {
    match word(name) {
        Some((_, past)) => {
            let mut c = past.chars();
            let first = c.next().map(|f| f.to_uppercase().collect::<String>()).unwrap_or_default();
            format!("{first}{}.", c.as_str())
        }
        None => format!("Played {name}."),
    }
}

/// `5m`, `1h30m`, `1m30s`, `45s` (rounded to the second).
pub fn fmt_dur(ms: u64) -> String {
    let s = (ms + 500) / 1000;
    let (h, m, s) = (s / 3600, s / 60 % 60, s % 60);
    let mut out = String::new();
    if h > 0 {
        out.push_str(&format!("{h}h"));
    }
    if m > 0 {
        out.push_str(&format!("{m}m"));
    }
    if s > 0 || out.is_empty() {
        out.push_str(&format!("{s}s"));
    }
    out
}

pub fn started_msg(name: &str, for_ms: u64, queued: bool) -> String {
    let after = if queued { " (after this reply)" } else { "" };
    format!("Started {} for {}{after}.", doing(name), fmt_dur(for_ms))
}

pub fn stopped_msg(name: &str) -> String {
    format!("Stopped {}.", doing(name))
}

pub const NOTHING_MSG: &str = "Not doing anything.";

/// `buddy mascot list`'s text: the pack's activities and emotes.
/// `now` is the brain clock, for the time left on what plays.
pub fn list_text(pack: &PackMeta, playing: Option<&Playing>, now: u64) -> String {
    let b = &pack.body;
    let mut out = String::new();
    match playing {
        Some(Playing { activity, until_ms: Some(u), .. }) => {
            out.push_str(&format!("Now: {activity} ({} left)\n", fmt_dur(u.saturating_sub(now))))
        }
        Some(Playing { activity, .. }) => out.push_str(&format!("Now: {activity} (after the reply)\n")),
        None => {}
    }
    if !b.activities.is_empty() {
        out.push_str("Activities:\n");
        for (name, a) in &b.activities {
            out.push_str(&format!("  {name:<9} {} (default {})\n", a.about, fmt_dur(b.default_for_s * 1000)));
        }
    }
    if !b.emotes.is_empty() {
        out.push_str("Emotes:\n");
        for (name, e) in &b.emotes {
            out.push_str(&format!("  {name:<9} {}\n", e.about));
        }
    }
    if out.is_empty() {
        out.push_str("No activities or emotes.\n");
    }
    out.trim_end().to_string()
}

pub fn activities(pack: &PackMeta) -> Vec<ActivityInfo> {
    let b = &pack.body;
    b.activities
        .iter()
        .map(|(n, a)| ActivityInfo { name: n.clone(), about: a.about.clone(), kind: a.kind.as_str(), default_for_s: b.default_for_s })
        .collect()
}

pub fn emotes(pack: &PackMeta) -> Vec<EmoteInfo> {
    pack.body.emotes.iter().map(|(n, e)| EmoteInfo { name: n.clone(), about: e.about.clone() }).collect()
}

/// The agent's "Your body" prompt section (design `#prompt-section`); `None` when the pack
/// has no activities and no emotes.
pub fn prompt_text(pack: &PackMeta) -> Option<String> {
    let b = &pack.body;
    if b.activities.is_empty() && b.emotes.is_empty() {
        return None;
    }
    let mut s = String::from(
        "## Your body\n\n\
         You are the bear on the user's screen. When the user asks you to do something\n\
         physical, do it with these commands instead of describing it in words. Never say\n\
         you did something the command's reply says did not happen.\n",
    );
    if !b.activities.is_empty() {
        s.push_str(&format!(
            "\nActivities (`buddy mascot do <name> [--for 5m]`; they start after your reply and\n\
             last {} unless you give --for, at most {}; `buddy mascot stop` ends one):\n",
            fmt_dur(b.default_for_s * 1000),
            fmt_dur(b.max_for_s * 1000)
        ));
        for (name, a) in &b.activities {
            s.push_str(&format!("- {name}: {}\n", a.about));
        }
    }
    if !b.emotes.is_empty() {
        s.push_str("\nEmotes (`buddy mascot emote <name>`; a short gesture during your reply):\n");
        for (name, e) in &b.emotes {
            s.push_str(&format!("- {name}: {}\n", e.about));
        }
    }
    s.push_str("\nWhen to use them:\n");
    if !b.activities.is_empty() {
        let names: Vec<&str> = b.activities.iter().map(|(n, _)| n.as_str()).collect();
        s.push_str(&format!(
            "- If the user asks you to {} or do another listed activity, run the command\n  and say so briefly.\n",
            names.join(", ")
        ));
    }
    if !b.emotes.is_empty() {
        s.push_str("- Use at most one emote per reply, only when it matches the mood. Most short\n  replies need none.\n");
    }
    if b.activities.iter().any(|(_, a)| a.kind == mascot_protocol::pack::ActivityKind::Work) {
        s.push_str("- You go to the desk by yourself during long jobs; do not call it for that.\n");
    }
    s.push_str(
        "- If a reply says \"refused\" (do not disturb or quiet hours), tell the user you\n  cannot right now; do not pretend.\n\
         - `buddy mascot list` shows this list.\n",
    );
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_bend_only_from_the_table() {
        assert_eq!(doing("read"), "reading");
        assert_eq!(doing("nap"), "napping");
        assert_eq!(doing("dance"), "dancing");
        assert_eq!(played("wave"), "Waved.");
        assert_eq!(played("nod"), "Nodded.");
        assert_eq!(played("shake"), "Shook.");
        assert_eq!(played("spin"), "Spun.");
        // an odd name: a neutral text from the name, never a made-up word
        assert_eq!(doing("tea-time"), "doing tea-time");
        assert_eq!(played("tea-time"), "Played tea-time.");
        assert_eq!(started_msg("tea-time", 300_000, false), "Started doing tea-time for 5m.");
        assert_eq!(stopped_msg("tea-time"), "Stopped doing tea-time.");
        assert_eq!(started_msg("read", 300_000, true), "Started reading for 5m (after this reply).");
    }

    #[test]
    fn durations_print_short() {
        assert_eq!(fmt_dur(300_000), "5m");
        assert_eq!(fmt_dur(5_400_000), "1h30m");
        assert_eq!(fmt_dur(90_000), "1m30s");
        assert_eq!(fmt_dur(45_000), "45s");
        assert_eq!(fmt_dur(3_600_000), "1h");
        assert_eq!(fmt_dur(0), "0s");
    }

    #[test]
    fn replies_serialize_as_frozen() {
        let r = BodyReply {
            status: Some(Status::Queued),
            activity: Some("read".into()),
            for_ms: Some(300_000),
            message: started_msg("read", 300_000, true),
            ..BodyReply::default()
        };
        assert_eq!(
            serde_json::to_value(&r).unwrap(),
            serde_json::json!({ "status": "queued", "activity": "read", "forMs": 300000, "message": "Started reading for 5m (after this reply)." })
        );
        let r = BodyReply { status: Some(Status::Refused), activity: Some("read".into()), reason: Some(Reason::QuietHours), message: Reason::QuietHours.message().into(), ..BodyReply::default() };
        assert_eq!(serde_json::to_value(&r).unwrap()["reason"], "quiet_hours");
        let l = BodyReply { message: "x".into(), playing: Some(None), max_for_s: Some(1800), ..BodyReply::default() };
        let v = serde_json::to_value(&l).unwrap();
        assert!(v["playing"].is_null() && v.as_object().unwrap().contains_key("playing"));
        assert_eq!(v["maxForS"], 1800);
    }
}
