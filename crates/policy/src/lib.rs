//! What the agent's tool calls may do (plan §2 "Permissions"). Every call the
//! sidecar's gate sees comes here as `policy.decide`. Ported from 2market's
//! `Production` policy: three modes, Claude Code-form rules, approval cards
//! and the taint mark, kept per agent in `<state_dir>/policy.json`. 2market's
//! spend allowances, `twomarket` tool and panel checks are not ported.

pub mod rules;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use rules::{FileTool, Pattern, Segment};

/// A refused policy op, with a code the UI acts on.
#[derive(Debug, Clone, PartialEq)]
pub struct PolicyError {
    pub code: &'static str,
    pub message: String,
}

impl PolicyError {
    pub fn new(code: &'static str, message: impl Into<String>) -> PolicyError {
        PolicyError { code, message: message.into() }
    }
}

impl fmt::Display for PolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for PolicyError {}

pub type Res<T> = Result<T, PolicyError>;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// `n` random bytes as hex.
pub fn random_hex(n: usize) -> String {
    let mut b = vec![0u8; n];
    getrandom::fill(&mut b).expect("the OS random source failed");
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The agent a request came from.
#[derive(Clone, Debug)]
pub struct AgentCtx {
    pub agent_id: String,
    /// The agent's working folder: file rules resolve against it.
    pub work_dir: PathBuf,
    /// Where the app keeps this agent's records (`policy.json`, its storage). Never writable by the agent.
    pub state_dir: PathBuf,
    /// The app's bundled skills: read-only, nothing of the person's in them.
    pub skills_dir: Option<PathBuf>,
    /// The sidecar start the request came from.
    pub gen: u64,
    /// The agent's HOME, which `~` means to its tools and shell (the app's own when unset).
    pub home: Option<PathBuf>,
    /// Files the agent never opens, by any tool (the host API's port file, with
    /// the person's token).
    pub secrets: Vec<PathBuf>,
}

impl AgentCtx {
    /// Whether `path` (from the work folder) is one of the secret files.
    fn is_secret(&self, path: &str) -> bool {
        if self.secrets.is_empty() {
            return false;
        }
        let abs = rules::folded(&rules::target(&self.work_dir, self.home.as_deref(), path).abs);
        self.secrets.iter().any(|s| rules::folded(&s.canonicalize().unwrap_or_else(|_| s.clone())) == abs)
    }
}

const SECRET: &str = "That file holds desk-buddy’s own API token: the agent never opens it.";

/// `policy.decide`'s arguments.
#[derive(Deserialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", default)]
pub struct Decide {
    pub call_id: String,
    pub tool: String,
    pub args: Value,
    /// "root" | "child"
    pub conversation: String,
    pub depth: u8,
    /// `{ requestId, source, resumed? }` of the input whose run made the call.
    pub trigger: Value,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    Allow,
    Deny { reason: String },
    /// The approval card's fields (`approval_asked` without `callId`).
    Ask(Value),
}

impl Decision {
    pub fn json(&self) -> Value {
        match self {
            Decision::Allow => json!({ "decision": "allow" }),
            Decision::Deny { reason } => json!({ "decision": "deny", "reason": reason }),
            Decision::Ask(approval) => json!({ "decision": "ask", "approval": approval }),
        }
    }
}

// ------------------------------------------------------------ policy.json

/// `full` (full access): everything runs except what needs the person's attention: ask rules, the built-in
/// risky commands and sensitive files (`rules::attention`, `rules::sensitive_path`); deny rules still deny, and
/// while the conversation is marked by the web it runs as `allowlist`.
pub const MODES: [&str; 4] = ["ask", "allowlist", "full", "skip"];

/// The default rules: computed, never stored, shown with `origin: "default"`.
/// The `buddy` commands that drive the bear's own body and reminders run without a card, marked or not.
const DEFAULT_RULES: [(&str, &str, &str); 11] = [
    ("d1", "deny", "Write(.buddy/**)"),
    ("d2", "deny", "Edit(.buddy/**)"),
    ("d3", "ask", "Shell(curl:*)"),
    ("d4", "ask", "Shell(wget:*)"),
    ("d5", "allow", "Shell(buddy say:*)"),
    ("d6", "allow", "Shell(buddy mascot:*)"),
    ("d7", "allow", "Shell(buddy status:*)"),
    ("d8", "allow", "Shell(buddy remind:*)"),
    ("d9", "allow", "Shell(buddy music:*)"),
    ("d10", "allow", "Shell(buddy widget list:*)"),
    ("d11", "allow", "Shell(buddy browser snapshot:*)"),
];
/// `buddy browser <these>` act on a page for the agent: while the conversation is marked they
/// always ask, whatever the mode or the person's rules (design `## widgets`).
const OUTBOUND: [&str; 3] = ["click", "type", "goto"];

/// `buddy browser click|type|goto …`, or `buddy widget open browser <url>` (a goto).
/// Whether `command` may drive the browser outward anywhere in it: as a plain segment, behind
/// `env`/`VAR=1`/`sh -c`/`eval`, inside `$(…)`, with flags (`--json`) between the words or
/// quotes and backslashes inside them (`brow''ser`), or built from a variable (`$b click`).
/// A token scan, so it errs toward asking (it never denies).
fn outbound(command: &str) -> bool {
    // the shell drops quotes and backslashes inside a word: `bu''ddy`, `brow\ser`
    let plain: String = command.chars().filter(|c| !matches!(c, '\'' | '"' | '\\')).collect();
    // flags anywhere (the CLI takes `--json` at any position) don't separate the words
    let w: Vec<&str> = plain.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).filter(|w| !w.is_empty() && !w.starts_with('-')).collect();
    // the words in order, others allowed between them
    let seq = |want: &[&[&str]]| {
        let mut it = w.iter();
        want.iter().all(|alts| it.any(|x| alts.contains(x)))
    };
    if seq(&[&["buddy"], &["browser"], &OUTBOUND]) || seq(&[&["buddy"], &["widget"], &["open"], &["browser"]]) {
        return true;
    }
    // a line that names the browser but builds a word at run time: `b=browser; buddy $b click`
    let built = plain.contains('$') || plain.contains('`') || w.contains(&"eval");
    built && w.iter().any(|x| ["buddy", "browser", "widget"].contains(x) || OUTBOUND.contains(x))
}
/// Approving a command these rules ask about marks the conversation.
const FETCH_RULES: [&str; 2] = ["d3", "d4"];
/// The tools a child conversation (a `task` research helper) may use.
pub const CHILD_TOOLS: [&str; 2] = ["web_fetch", "web_search"];
const MAX_PENDING: usize = 8;
const MAX_TAINT_SOURCES: usize = 20;

/// The `task` card in ask mode: its helper then searches and reads the web without a card.
pub const TASK_CARD_DETAIL: &str = "Ask every time is on. Its helper can search and read the web without asking.";

/// The approval card for `buddy screen look` / `screen_look`.
pub const SCREEN_CARD_TITLE: &str = "Let the bear look at your screen?";
pub const SCREEN_CARD_DETAIL: &str = "It reads the text on the display the bear is on. Nothing is saved. Password managers and messengers are never read.";
/// A check-in turn (`trigger.source == "sense"`) never looks at the screen.
pub const SCREEN_SENSE_DENIED: &str = "Screen look only runs when the person asked in chat, not during a check-in";

pub const DECLINED: &str = "The person declined this call. Don’t retry it; try another way or ask them what to do.";

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Rule {
    pub id: String,
    pub effect: String,
    pub pattern: String,
    pub origin: String,
    pub at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaintSource {
    pub source: String,
    pub detail: String,
    pub at: u64,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Taint {
    pub at: u64,
    pub sources: Vec<TaintSource>,
}

/// An unanswered card. `taint` is what Allow would mark the conversation with.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub call_id: String,
    pub approval: Value,
    pub at: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taint: Option<TaintSource>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PolicyFile {
    pub version: u32,
    pub mode: String,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default)]
    pub taint: Option<Taint>,
    #[serde(default)]
    pub pending: Vec<Card>,
}

impl Default for PolicyFile {
    fn default() -> Self {
        PolicyFile { version: 1, mode: "allowlist".into(), rules: vec![], taint: None, pending: vec![] }
    }
}

pub fn policy_path(state_dir: &Path) -> PathBuf {
    state_dir.join("policy.json")
}

/// An agent's file, or the defaults. A file that can't be read keeps the agent
/// marked, so a damaged file never lifts the taint mark.
pub fn load(state_dir: &Path) -> PolicyFile {
    let path = policy_path(state_dir);
    let bytes = match fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return PolicyFile::default(),
        Err(_) => vec![],
    };
    match serde_json::from_slice::<PolicyFile>(&bytes) {
        Ok(mut f) if f.version == 1 => {
            if !MODES.contains(&f.mode.as_str()) {
                f.mode = "allowlist".into();
            }
            f
        }
        _ => {
            eprintln!("buddy: {} couldn’t be read; starting from the defaults, marked", path.display());
            let at = now_ms();
            PolicyFile { taint: Some(Taint { at, sources: vec![TaintSource { source: "unknown".into(), detail: "policy.json couldn’t be read".into(), at }] }), ..Default::default() }
        }
    }
}

fn save(state_dir: &Path, f: &PolicyFile) -> io::Result<()> {
    let path = policy_path(state_dir);
    fs::create_dir_all(state_dir)?;
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(f)?)?;
    fs::rename(tmp, path)
}

// ------------------------------------------------------------- production

struct Tab {
    file: PolicyFile,
    /// Cards answered whose wait hasn't taken the answer yet (the sidecar may
    /// have ended first): `(approval, result)`.
    answers: HashMap<String, (Value, Value)>,
    /// Sidecar gens that ended; their waits return `cancelled`.
    released: HashSet<u64>,
}

type Notify = Box<dyn Fn(&AgentCtx, Value) + Send + Sync>;

/// The policy for every agent of the app.
pub struct Production {
    tabs: Mutex<HashMap<String, Tab>>,
    answered: Condvar,
    /// Sends a changed `PolicyView` to the UI.
    notify: Notify,
}

struct Rules {
    deny: Vec<(String, Pattern)>,
    ask: Vec<(String, Pattern)>,
    allow: Vec<(String, Pattern)>,
}

impl Rules {
    fn of(f: &PolicyFile, shell_person: bool) -> Rules {
        let mut r = Rules { deny: vec![], ask: vec![], allow: vec![] };
        let defaults = DEFAULT_RULES.iter().map(|(id, e, p)| (id.to_string(), e.to_string(), p.to_string(), true));
        let person = f.rules.iter().map(|x| (x.id.clone(), x.effect.clone(), x.pattern.clone(), false));
        for (id, effect, pattern, default) in defaults.chain(person) {
            let Some(p) = rules::parse(&pattern) else { continue };
            // While marked, the person's own allow rules stop applying to shell.
            if !default && !shell_person && effect == "allow" && matches!(p, Pattern::ShellPrefix(_) | Pattern::ShellExact(_)) {
                continue;
            }
            match effect.as_str() {
                "deny" => r.deny.push((id, p)),
                "ask" => r.ask.push((id, p)),
                "allow" => r.allow.push((id, p)),
                _ => {}
            }
        }
        r
    }
}

fn first<'a>(list: &'a [(String, Pattern)], f: impl Fn(&Pattern) -> bool) -> Option<&'a (String, Pattern)> {
    list.iter().find(|(_, p)| f(p))
}

fn pattern_text(id: &str, f: &PolicyFile) -> String {
    DEFAULT_RULES.iter().find(|(d, _, _)| *d == id).map(|(_, _, p)| p.to_string()).or_else(|| f.rules.iter().find(|r| r.id == id).map(|r| r.pattern.clone())).unwrap_or_default()
}

fn tool_label(t: FileTool) -> &'static str {
    match t {
        FileTool::Read => "Read",
        FileTool::Write => "Write",
        FileTool::Edit => "Edit",
    }
}

/// The decision before any card bookkeeping, plus what Allow would mark.
enum Verdict {
    Allow,
    Deny(String),
    Ask(Value, Option<TaintSource>),
}

impl Production {
    pub fn new(notify: impl Fn(&AgentCtx, Value) + Send + Sync + 'static) -> Production {
        Production { tabs: Mutex::new(HashMap::new()), answered: Condvar::new(), notify: Box::new(notify) }
    }

    fn tabs(&self) -> MutexGuard<'_, HashMap<String, Tab>> {
        self.tabs.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn tab<'a>(tabs: &'a mut HashMap<String, Tab>, at: &AgentCtx) -> &'a mut Tab {
        tabs.entry(at.agent_id.clone()).or_insert_with(|| Tab { file: load(&at.state_dir), answers: HashMap::new(), released: HashSet::new() })
    }

    fn persist(at: &AgentCtx, t: &Tab) {
        if let Err(e) = save(&at.state_dir, &t.file) {
            eprintln!("buddy: couldn’t write policy.json for {}: {e}", at.agent_id);
        }
    }

    /// Always built under the tabs lock, so `rev` orders views as the state
    /// changed, whichever thread delivers them first.
    fn view_of(f: &PolicyFile) -> Value {
        static REV: AtomicU64 = AtomicU64::new(0);
        let mut rules: Vec<Value> = DEFAULT_RULES.iter().map(|(id, effect, pattern)| json!({ "id": id, "effect": effect, "pattern": pattern, "origin": "default" })).collect();
        rules.extend(f.rules.iter().map(|r| json!({ "id": r.id, "effect": r.effect, "pattern": r.pattern, "origin": r.origin })));
        json!({
            "mode": f.mode,
            "taint": f.taint,
            "rules": rules,
            "pending": f.pending.iter().map(|c| c.call_id.clone()).collect::<Vec<_>>(),
            "rev": REV.fetch_add(1, Ordering::SeqCst) + 1,
        })
    }

    fn verdict(at: &AgentCtx, f: &PolicyFile, req: &Decide) -> Verdict {
        let child = req.conversation == "child" || req.depth > 0;
        if child && !CHILD_TOOLS.contains(&req.tool.as_str()) {
            return Verdict::Deny(format!("A helper conversation can’t use “{}”.", req.tool));
        }
        // Looking at the screen is only ever the person's ask in chat: a check-in turn never does it.
        let looks = req.tool == "screen_look" || (req.tool == "bash" && rules::screen_look(req.args["command"].as_str().unwrap_or("")));
        if looks && req.trigger["source"] == "sense" {
            return Verdict::Deny(SCREEN_SENSE_DENIED.into());
        }
        let tainted = f.taint.is_some();
        let rules = Rules::of(f, !tainted);
        // Full access pauses while the conversation is marked: a web page must not steer a bear that runs anything.
        let mode = if f.mode == "full" && tainted { "allowlist" } else { f.mode.as_str() };
        match req.tool.as_str() {
            "bash" => Self::shell(at, f, &rules, mode, tainted, req.args["command"].as_str().unwrap_or("")),
            // The sidecar's typed tool is judged as the command it stands for, so modes, allow rules and the
            // "Always" rule `Shell(buddy screen look:*)` act the same on both paths.
            "screen_look" => {
                let command = if req.args["image"] == true { "buddy screen look --image" } else { "buddy screen look" };
                match Self::shell(at, f, &rules, mode, tainted, command) {
                    Verdict::Ask(mut a, t) => {
                        a["tool"] = json!("screen_look");
                        Verdict::Ask(a, t)
                    }
                    v => v,
                }
            }
            "read" | "write" | "edit" => {
                let tool = FileTool::of(&req.tool).unwrap();
                Self::file(at, &rules, mode, tainted, tool, req.args["path"].as_str().unwrap_or(""))
            }
            // Showing the model an image is reading that file.
            "look" => Self::file(at, &rules, mode, tainted, FileTool::Read, req.args["path"].as_str().unwrap_or("")),
            "web_fetch" | "web_search" | "task" => Self::web(mode, &req.tool, &req.args, child),
            t => Verdict::Deny(format!("“{t}” isn’t one of desk-buddy’s tools.")),
        }
    }

    fn shell(at: &AgentCtx, f: &PolicyFile, rules: &Rules, mode: &str, tainted: bool, command: &str) -> Verdict {
        if command.trim().is_empty() {
            return Verdict::Deny("Give a command to run.".into());
        }
        let line = rules::split(command);
        for seg in &line.segments {
            // Named outright (`cat ../host.json`); a glob that happens to match is the OS sandbox's job.
            if seg.words.iter().any(|w| at.is_secret(w.split_once('=').map_or(w, |(_, v)| v))) {
                return Verdict::Deny(SECRET.into());
            }
            if let Some((id, _)) = first(&rules.deny, |p| p.matches_segment(seg)) {
                return Verdict::Deny(format!("A rule denies this command: {}.", pattern_text(id, f)));
            }
        }
        if tainted && outbound(command) {
            return Verdict::Ask(
                json!({
                    "kind": "command",
                    "tool": "bash",
                    "title": "Let the bear use the page?",
                    "detail": "This conversation read a web page, so clicking, typing or opening pages asks every time.",
                    "command": command,
                    "suggestedRule": null,
                    "offerAlways": false,
                }),
                None,
            );
        }
        // `skip` (policy.json only) never asks, except for a look at the screen: D18 asks every time.
        let looks = rules::screen_look(command);
        if mode == "skip" && !looks {
            return Verdict::Allow;
        }
        // Every ask rule a segment matches.
        let hits: Vec<&String> = line.segments.iter().flat_map(|s| rules.ask.iter().filter(|(_, p)| p.matches_segment(s)).map(|(id, _)| id)).collect();
        let asked = hits.first().map(|id| (*id).clone());
        // Full access: only an ask rule or a risky command (`rules::attention`) asks.
        // A look beside a riskier command names the riskier one (`buddy screen look && rm -rf …` is about the `rm`).
        let risky = (mode == "full")
            .then(|| {
                let found: Vec<(Option<&Segment>, &str)> = line.segments.iter().filter_map(|s| rules::attention(s).map(|why| (Some(s), why))).collect();
                found.iter().find(|(_, why)| *why != rules::SCREEN_REASON).or(found.first()).copied().or_else(|| if line.complex { rules::attention_raw(command).map(|why| (None, why)) } else { None })
            })
            .flatten();
        if mode == "full" && asked.is_none() && risky.is_none() {
            return Verdict::Allow;
        }
        // An allow rule names a command, not where it writes: a redirect still asks.
        let runs = |s: &Segment| !line.complex && (rules::read_only(s) || (!s.writes && first(&rules.allow, |p| p.matches_segment(s)).is_some()));
        let needs: Vec<&Segment> = line.segments.iter().filter(|s| !runs(s)).collect();
        if mode == "allowlist" && asked.is_none() && needs.is_empty() {
            return Verdict::Allow;
        }
        let name = risky.and_then(|(s, _)| s).or(needs.first().copied()).or(line.segments.first()).map(|s| {
            // `buddy say` and `git push` read better than `buddy` or `git`.
            match (s.words.first(), s.words.get(1)) {
                (Some(a), Some(b)) if (a == "buddy" || a == "git") && !b.starts_with('-') => format!("{a} {b}"),
                (Some(a), _) => a.clone(),
                _ => "this command".into(),
            }
        });
        let name = name.unwrap_or_else(|| "this command".into());
        let suggested = match needs.as_slice() {
            [one] if mode == "allowlist" && !tainted && !line.complex && asked.is_none() && !one.writes => rules::suggest(one),
            _ => None,
        };
        let detail = if let Some(id) = &asked {
            format!("A rule asks before {} runs.", pattern_text(id, f))
        } else if mode == "ask" {
            "Ask every time is on.".to_string()
        } else if let Some((_, why)) = risky {
            format!("Full access still asks before this: {why}.")
        } else if tainted {
            "This conversation read the web, so commands outside the built-in allowlist ask every time.".to_string()
        } else if line.complex {
            "It runs a command inside another, which the allowlist can’t check.".to_string()
        } else {
            "It isn’t in the allowlist.".to_string()
        };
        // A fetch the split can't see as its own segment (`"$(curl …)"`) marks too.
        let fetches = hits.iter().any(|id| FETCH_RULES.contains(&id.as_str())) || (line.complex && command.split(|c: char| !(c.is_ascii_alphanumeric() || c == '-')).any(|w| w == "curl" || w == "wget"));
        let taint = fetches.then(|| TaintSource { source: "shell".into(), detail: command.to_string(), at: 0 });
        // A look on its own has its own card, whatever made it ask (mode, a rule, the web mark, Full access's
        // attention list). A look inside a longer line keeps the line's card and says it also reads the screen,
        // so a harmless-looking card never stands for `buddy screen look && rm -rf …`.
        let (title, detail) = if looks && !line.complex && line.segments.len() == 1 {
            (SCREEN_CARD_TITLE.to_string(), SCREEN_CARD_DETAIL.to_string())
        } else if looks {
            (format!("Run {name}?"), format!("{detail} It also looks at your screen."))
        } else {
            (format!("Run {name}?"), detail)
        };
        let approval = json!({
            "kind": "command",
            "tool": "bash",
            "title": title,
            "detail": detail,
            "command": command,
            "suggestedRule": suggested,
            "offerAlways": suggested.is_some(),
        });
        Verdict::Ask(approval, taint)
    }

    /// The web tools and `task`: allowed unless every call asks. Their results mark the conversation through
    /// `policy.taint` when they arrive, so a card here has nothing to mark and no "Always" (asking is the mode).
    fn web(mode: &str, tool: &str, args: &Value, child: bool) -> Verdict {
        if mode != "ask" || child {
            return Verdict::Allow;
        }
        let (title, what) = match tool {
            "web_fetch" => ("Open a web page?".to_string(), args["url"].as_str().unwrap_or("").to_string()),
            "web_search" => ("Search the web?".to_string(), args["query"].as_str().unwrap_or("").to_string()),
            _ => ("Send a helper to research this?".to_string(), args["prompt"].as_str().unwrap_or("").to_string()),
        };
        let what: String = what.chars().take(300).collect();
        // A helper's own web calls never ask (children get the web tools only): the card says so.
        let detail = if tool == "task" { TASK_CARD_DETAIL } else { "Ask every time is on." };
        Verdict::Ask(
            json!({
                "kind": "web",
                "tool": tool,
                "title": title,
                "detail": detail,
                "query": what,
                "suggestedRule": null,
                "offerAlways": false,
            }),
            None,
        )
    }

    fn file(at: &AgentCtx, rules: &Rules, mode: &str, tainted: bool, tool: FileTool, path: &str) -> Verdict {
        if path.trim().is_empty() {
            return Verdict::Deny("Name a file.".into());
        }
        if at.is_secret(path) {
            return Verdict::Deny(SECRET.into());
        }
        let t = rules::target(&at.work_dir, at.home.as_deref(), path);
        // A repo's `.git` holds config whose programs `git status|diff|log` run.
        if tool != FileTool::Read && t.abs.components().any(|c| c.as_os_str().eq_ignore_ascii_case(".git")) {
            return Verdict::Deny(format!("A rule denies {}({}). A repository’s .git folder can name programs git runs.", tool_label(tool), t.display));
        }
        let in_state = rules::folded(&t.abs).starts_with(rules::folded(&at.state_dir.canonicalize().unwrap_or_else(|_| at.state_dir.clone())));
        let own_records = t.display.to_ascii_lowercase().starts_with(".buddy") || in_state;
        // The app's records for this agent, wherever they live: never written by the agent.
        if tool != FileTool::Read && in_state {
            return Verdict::Deny(format!("A rule denies {}({}). That folder holds desk-buddy’s own records.", tool_label(tool), t.display));
        }
        if first(&rules.deny, |p| p.matches_file(tool, &t)).is_some() {
            let why = if own_records { " That folder holds desk-buddy’s own records." } else { "" };
            return Verdict::Deny(format!("A rule denies {}({}).{why}", tool_label(tool), t.display));
        }
        if mode == "skip" {
            return Verdict::Allow;
        }
        let asked = first(&rules.ask, |p| p.matches_file(tool, &t)).is_some();
        // Full access: only an ask rule or a file that holds keys, passwords or startup scripts asks.
        if mode == "full" && !asked && !rules::sensitive_path(&t.abs.to_string_lossy()) {
            return Verdict::Allow;
        }
        if mode == "allowlist" && !asked {
            let inside = t.inside && !own_records;
            // The bundled skills its prompt points it at hold nothing of the person's.
            let bundled = tool == FileTool::Read && at.skills_dir.as_ref().and_then(|d| d.canonicalize().ok()).is_some_and(|d| t.abs.starts_with(d));
            // A capped tool result's full text (`.buddy/outputs/`): the sidecar wrote it, the agent can't, and it is
            // already the conversation's own, so paging through it never asks, marked or not.
            let capped = tool == FileTool::Read && t.inside && t.display.replace('\\', "/").to_ascii_lowercase().starts_with(".buddy/outputs/");
            // Once the conversation read the web, a read outside the folder could carry a file out: it asks.
            if (tool == FileTool::Read && !tainted) || inside || bundled || capped || first(&rules.allow, |p| p.matches_file(tool, &t)).is_some() {
                return Verdict::Allow;
            }
        }
        let verb = tool_label(tool);
        let detail = if mode == "ask" {
            "Ask every time is on."
        } else if asked {
            "A rule asks before this file changes."
        } else if mode == "full" {
            "Full access still asks before files that hold keys, passwords or startup scripts."
        } else if tainted && tool == FileTool::Read {
            "This conversation read the web, so reading files outside the work folder asks every time."
        } else {
            "It’s outside the work folder."
        };
        let suggested = (mode == "allowlist" && !asked && !(tainted && tool == FileTool::Read)).then(|| format!("{verb}({})", t.display));
        Verdict::Ask(
            json!({
                "kind": "file",
                "tool": verb.to_lowercase(),
                "title": format!("{verb} {}?", t.display),
                "detail": detail,
                "path": t.display,
                "suggestedRule": suggested,
                "offerAlways": suggested.is_some(),
            }),
            None,
        )
    }

    /// The result `policy.wait` hands back for an answer, applied to the agent.
    fn answer(t: &mut Tab, card: &Card, answer: &str) -> Res<Value> {
        let a = &card.approval;
        let now = now_ms();
        let mut out = match answer {
            "allow" => json!({ "answer": "allow", "decision": "allow" }),
            "allow_always" => {
                let Some(pattern) = a["suggestedRule"].as_str().filter(|_| a["offerAlways"] == true) else {
                    return Err(PolicyError::new("usage", "This card doesn’t offer “from now on”."));
                };
                if !t.file.rules.iter().any(|r| r.effect == "allow" && r.pattern == pattern) {
                    t.file.rules.push(Rule { id: format!("r{}", random_hex(4)), effect: "allow".into(), pattern: pattern.into(), origin: "person".into(), at: now });
                }
                json!({ "answer": "allow_always", "decision": "allow", "rule": pattern })
            }
            "deny" => json!({ "answer": "deny", "decision": "deny", "reason": DECLINED }),
            _ => return Err(PolicyError::new("usage", "Answer allow, allow_always or deny.")),
        };
        if out["decision"] == "allow" {
            if let Some(src) = &card.taint {
                let src = TaintSource { at: now, ..src.clone() };
                Self::mark(&mut t.file, src.clone());
                out["taint"] = json!({ "source": src.source, "detail": src.detail });
            }
        }
        Ok(out)
    }

    /// Returns whether this made the agent marked.
    fn mark(f: &mut PolicyFile, src: TaintSource) -> bool {
        match &mut f.taint {
            Some(t) => {
                if !t.sources.iter().any(|s| s.source == src.source && s.detail == src.detail) {
                    t.sources.push(src);
                    let over = t.sources.len().saturating_sub(MAX_TAINT_SOURCES);
                    t.sources.drain(..over);
                }
                false
            }
            None => {
                f.taint = Some(Taint { at: src.at, sources: vec![src] });
                true
            }
        }
    }

    fn answer_card(&self, at: &AgentCtx, call_id: &str, answer: &str) -> Res<Value> {
        let view = {
            let mut tabs = self.tabs();
            let t = Self::tab(&mut tabs, at);
            let Some(i) = t.file.pending.iter().position(|c| c.call_id == call_id) else {
                return Err(PolicyError::new("not_found", "That question was already answered."));
            };
            let card = t.file.pending[i].clone();
            let out = Self::answer(t, &card, answer)?;
            t.file.pending.remove(i);
            t.answers.insert(call_id.to_string(), (card.approval, out));
            Self::persist(at, t);
            Self::view_of(&t.file)
        };
        self.answered.notify_all();
        (self.notify)(at, view.clone());
        Ok(json!({ "policy": view }))
    }

    /// `policy.decide`: allow, deny, or put up a card and ask.
    pub fn decide(&self, at: &AgentCtx, req: &Decide) -> Decision {
        let (view, approval) = {
            let mut tabs = self.tabs();
            let t = Self::tab(&mut tabs, at);
            // A call cut off while its card was up comes again with the same id.
            if let Some(c) = t.file.pending.iter().find(|c| c.call_id == req.call_id) {
                return Decision::Ask(c.approval.clone());
            }
            if let Some((approval, _)) = t.answers.get(&req.call_id) {
                return Decision::Ask(approval.clone());
            }
            match Self::verdict(at, &t.file, req) {
                Verdict::Allow => return Decision::Allow,
                Verdict::Deny(reason) => return Decision::Deny { reason },
                Verdict::Ask(approval, taint) => {
                    if req.call_id.is_empty() {
                        return Decision::Deny { reason: "A call that asks needs its call id.".into() };
                    }
                    if t.file.pending.len() >= MAX_PENDING {
                        return Decision::Deny { reason: "Too many approvals are waiting; answer one first.".into() };
                    }
                    t.file.pending.push(Card { call_id: req.call_id.clone(), approval: approval.clone(), at: now_ms(), taint });
                    Self::persist(at, t);
                    (Self::view_of(&t.file), approval)
                }
            }
        };
        (self.notify)(at, view);
        Decision::Ask(approval)
    }

    /// `policy.wait`: blocks until the person answers the card, or the sidecar ends.
    pub fn wait(&self, at: &AgentCtx, call_id: &str) -> Value {
        let cancelled = json!({ "answer": "cancelled", "decision": "deny", "reason": "The question was withdrawn before the person answered." });
        let mut tabs = self.tabs();
        loop {
            let t = Self::tab(&mut tabs, at);
            if let Some((_, out)) = t.answers.remove(call_id) {
                return out;
            }
            if t.released.contains(&at.gen) || !t.file.pending.iter().any(|c| c.call_id == call_id) {
                return cancelled;
            }
            tabs = self.answered.wait(tabs).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// The sidecar withdrew a card (an abort): it goes away unanswered.
    pub fn cancel(&self, at: &AgentCtx, call_id: &str) {
        let view = {
            let mut tabs = self.tabs();
            let t = Self::tab(&mut tabs, at);
            let before = t.file.pending.len();
            t.file.pending.retain(|c| c.call_id != call_id);
            t.answers.remove(call_id);
            if t.file.pending.len() == before {
                None
            } else {
                Self::persist(at, t);
                Some(Self::view_of(&t.file))
            }
        };
        self.answered.notify_all();
        if let Some(v) = view {
            (self.notify)(at, v);
        }
    }

    /// The sidecar of `at.gen` ended: its waits return; its cards stay pending for the next one.
    pub fn release(&self, at: &AgentCtx) {
        Self::tab(&mut self.tabs(), at).released.insert(at.gen);
        self.answered.notify_all();
    }

    /// The UI's policy ops: `approval_answer`, `deny_pending`, `set_mode`, `clear_taint`, `remove_rule`.
    pub fn ui(&self, at: &AgentCtx, op: &Value) -> Res<Value> {
        let name = op["op"].as_str().unwrap_or("");
        if name == "approval_answer" {
            let call_id = op["callId"].as_str().unwrap_or("");
            return self.answer_card(at, call_id, op["answer"].as_str().unwrap_or(""));
        }
        if name == "deny_pending" {
            let ids: Vec<String> = Self::tab(&mut self.tabs(), at).file.pending.iter().map(|c| c.call_id.clone()).collect();
            for id in ids {
                let _ = self.answer_card(at, &id, "deny");
            }
            return Ok(json!({ "policy": self.view(at) }));
        }
        let view = {
            let mut tabs = self.tabs();
            let t = Self::tab(&mut tabs, at);
            let f = &mut t.file;
            match name {
                "set_mode" => {
                    let mode = op["mode"].as_str().unwrap_or("");
                    if !MODES.contains(&mode) {
                        return Err(PolicyError::new("usage", "Pick ask, allowlist, full or skip."));
                    }
                    f.mode = mode.into();
                }
                "clear_taint" => f.taint = None,
                "remove_rule" => {
                    let id = op["ruleId"].as_str().unwrap_or("");
                    if DEFAULT_RULES.iter().any(|(d, _, _)| *d == id) {
                        return Err(PolicyError::new("usage", "Default rules can’t be removed."));
                    }
                    let before = f.rules.len();
                    f.rules.retain(|r| r.id != id);
                    if f.rules.len() == before {
                        return Err(PolicyError::new("not_found", "No such rule."));
                    }
                }
                _ => return Err(PolicyError::new("usage", format!("Unknown policy op “{name}”."))),
            }
            Self::persist(at, t);
            Self::view_of(&t.file)
        };
        (self.notify)(at, view.clone());
        Ok(json!({ "policy": view }))
    }

    /// The agent's `PolicyView`.
    pub fn view(&self, at: &AgentCtx) -> Value {
        Self::view_of(&Self::tab(&mut self.tabs(), at).file)
    }

    /// Something the agent read came from the web (S2b's web tools): mark it. Returns whether this newly marked it.
    pub fn taint(&self, at: &AgentCtx, source: &str, detail: &str) -> bool {
        let detail: String = detail.chars().take(500).collect();
        let (newly, view) = {
            let mut tabs = self.tabs();
            let t = Self::tab(&mut tabs, at);
            let newly = Self::mark(&mut t.file, TaintSource { source: source.into(), detail, at: now_ms() });
            Self::persist(at, t);
            (newly, Self::view_of(&t.file))
        };
        (self.notify)(at, view);
        newly
    }
}

#[cfg(test)]
mod tests;
