//! `buddy`: the agent's (and the person's) way to make the mascot act.
//!
//!   buddy say <text…> [--emotion <e>]   speak in a bubble
//!   buddy mascot emote <name>            play an emote
//!   buddy remind --in <dur> <text…>      remind later (90s, 20m, 1h30m, 2d)
//!   buddy remind --at HH:MM <text…>      remind at the next HH:MM, local time
//!   buddy remind list [--json]           the reminders waiting
//!   buddy remind cancel <id>             drop one
//!   buddy status [--json]                who the API sees, and the turn
//!   buddy --help [--json]                this list; `--json` for agents
//!
//! It reaches the host API with `BUDDY_API` + `BUDDY_TOKEN` (+ `BUDDY_TURN`),
//! which the agent's commands get; from a terminal it reads the port file
//! (`BUDDY_HOST_FILE`, else `$BUDDY_HOME/host.json`, else `~/.desk-buddy/host.json`);
//! with the app gone (no file, or nothing listening) it says "desk-buddy isn't running". Exit codes: 0 done,
//! 1 refused or unreachable, 2 usage.

use std::path::PathBuf;
use std::process::ExitCode;

use host::client::{request, Reply};
use host::{local_hhmm, parse_duration, parse_hhmm, PortFile, EMOTES, EMOTIONS, MAX_REMIND_MS};
use serde_json::{json, Value};

fn help() -> Value {
    json!({
        "name": "buddy",
        "about": "Make the desk buddy act. Commands that change what the person sees ask for approval unless allowed.",
        "commands": [
            { "usage": "buddy say <text…> [--emotion <e>]", "about": "Speak in a speech bubble. Keep it short.", "emotions": EMOTIONS, "readOnly": false },
            { "usage": "buddy mascot emote <name>", "about": "Play an emote.", "emotes": EMOTES, "readOnly": false },
            { "usage": "buddy remind --in <dur> <text…>", "about": "Remind the person later: the bear says the text then. dur is like 90s, 20m, 1h30m or 2d (10 s to 7 days). Prints the reminder's id.", "readOnly": false },
            { "usage": "buddy remind --at HH:MM <text…>", "about": "Remind at the next HH:MM, local time (today, else tomorrow).", "readOnly": false },
            { "usage": "buddy remind list [--json]", "about": "The reminders waiting, soonest first.", "readOnly": true },
            { "usage": "buddy remind cancel <id>", "about": "Drop a reminder.", "readOnly": false },
            { "usage": "buddy status [--json]", "about": "Check the connection.", "readOnly": true },
            { "usage": "buddy --help [--json]", "about": "This list.", "readOnly": true },
        ],
    })
}

fn usage(msg: &str) -> ExitCode {
    eprintln!("buddy: {msg} (see buddy --help)");
    ExitCode::from(2)
}

struct Target {
    port: u16,
    token: String,
    turn: Option<String>,
    /// Read from the port file: a refused connection means the app is gone (a stale file).
    from_file: bool,
}

const NOT_RUNNING: &str = "desk-buddy isn't running";

fn target() -> Result<Target, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let (Some(api), Some(token)) = (env("BUDDY_API"), env("BUDDY_TOKEN")) {
        let port = api.strip_prefix("http://127.0.0.1:").and_then(|p| p.trim_end_matches('/').parse().ok()).ok_or_else(|| format!("BUDDY_API={api} isn’t http://127.0.0.1:<port>"))?;
        return Ok(Target { port, token, turn: env("BUDDY_TURN"), from_file: false });
    }
    // The person's terminal: the app's port file (design §3.1), never consulted when the agent's env is set.
    let file = env("BUDDY_HOST_FILE")
        .map(PathBuf::from)
        .or_else(|| env("BUDDY_HOME").map(|h| PathBuf::from(h).join("host.json")))
        .or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".desk-buddy").join("host.json")))
        .ok_or("no BUDDY_API and no home folder")?;
    let pf = PortFile::read(&file).map_err(|e| format!("{NOT_RUNNING} ({}: {e})", file.display()))?;
    Ok(Target { port: pf.port, token: pf.token, turn: None, from_file: true })
}

fn call(method: &str, path: &str, body: Option<Value>) -> Result<Reply, String> {
    let t = target()?;
    let auth = format!("Bearer {}", t.token);
    let mut headers = vec![("Authorization", auth.as_str())];
    if let Some(turn) = &t.turn {
        headers.push(("X-Buddy-Turn", turn.as_str()));
    }
    request(t.port, method, path, &headers, body.as_ref()).map_err(|e| {
        if t.from_file && e.kind() == std::io::ErrorKind::ConnectionRefused {
            format!("{NOT_RUNNING} (nothing listens on port {}; the port file is stale)", t.port)
        } else {
            format!("couldn’t reach desk-buddy: {e}")
        }
    })
}

fn finish(r: Result<Reply, String>, json_out: bool, done: &str) -> ExitCode {
    match r {
        Err(e) => {
            eprintln!("buddy: {e}");
            ExitCode::from(1)
        }
        Ok(r) if r.status == 200 => {
            if json_out {
                println!("{}", r.body);
            } else {
                let late = if r.body["afterTurn"] == true { " (after its turn ended)" } else { "" };
                println!("{done}{late}");
            }
            ExitCode::SUCCESS
        }
        Ok(r) => {
            let msg = r.body["error"]["message"].as_str().unwrap_or("refused");
            eprintln!("buddy: {msg} (HTTP {})", r.status);
            ExitCode::from(if r.body["error"]["code"] == "usage" { 2 } else { 1 })
        }
    }
}

/// `buddy remind …`.
fn remind(rest: &[&str], json_out: bool) -> ExitCode {
    match rest {
        ["list" | "ls"] => match call("GET", "/v1/reminders", None) {
            Ok(r) if r.status == 200 && !json_out => {
                let list = r.body["reminders"].as_array().cloned().unwrap_or_default();
                if list.is_empty() {
                    println!("no reminders");
                }
                for x in list {
                    let at = x["atMs"].as_u64().and_then(local_hhmm).unwrap_or_else(|| "?".into());
                    println!("{}  {}  {}", x["id"].as_str().unwrap_or("?"), at, x["text"].as_str().unwrap_or(""));
                }
                ExitCode::SUCCESS
            }
            other => finish(other, json_out, ""),
        },
        ["cancel", id] => finish(call("POST", "/v1/remind/cancel", Some(json!({ "id": id }))), json_out, "cancelled"),
        ["cancel", ..] => usage("buddy remind cancel <id>"),
        ["list" | "ls", ..] => usage("buddy remind list [--json]"),
        _ => {
            let (mut inn, mut at, mut text) = (None, None, Vec::new());
            let mut it = rest.iter();
            while let Some(w) = it.next() {
                match *w {
                    "--in" | "--at" => match it.next() {
                        Some(v) if *w == "--in" => inn = Some(v.to_string()),
                        Some(v) => at = Some(v.to_string()),
                        None => return usage(&format!("{w} needs a value")),
                    },
                    w if w.starts_with("--in=") => inn = Some(w["--in=".len()..].to_string()),
                    w if w.starts_with("--at=") => at = Some(w["--at=".len()..].to_string()),
                    w => text.push(w),
                }
            }
            let text = text.join(" ");
            if text.trim().is_empty() {
                return usage("remind of what? buddy remind --in 20m <text>");
            }
            let mut body = json!({ "text": text });
            match (inn, at) {
                (Some(d), None) => match parse_duration(&d) {
                    Some(ms) if ms > 0 && ms <= MAX_REMIND_MS => body["inMs"] = json!(ms),
                    Some(_) => return usage("a reminder is 10 s to 7 days away"),
                    None => return usage(&format!("“{d}” isn’t a duration like 90s, 20m, 1h30m or 2d")),
                },
                (None, Some(t)) => {
                    if !cfg!(unix) {
                        return usage("--at isn’t supported here");
                    }
                    if parse_hhmm(&t).is_none() {
                        return usage(&format!("“{t}” isn’t a time like 14:30"));
                    }
                    body["at"] = json!(t);
                }
                _ => return usage("give one of --in <dur> or --at HH:MM"),
            }
            match call("POST", "/v1/remind", Some(body)) {
                Ok(r) if r.status == 200 && !json_out => {
                    let at = r.body["atMs"].as_u64().and_then(local_hhmm).unwrap_or_else(|| "?".into());
                    println!("reminder {} set for {at}", r.body["id"].as_str().unwrap_or("?"));
                    ExitCode::SUCCESS
                }
                other => finish(other, json_out, "reminder set"),
            }
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let json_out = args.iter().any(|a| a == "--json");
    let words: Vec<&str> = args.iter().map(String::as_str).filter(|a| *a != "--json").collect();
    match words.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            let h = help();
            if json_out {
                println!("{h}");
            } else {
                println!("{}", h["about"].as_str().unwrap_or(""));
                for c in h["commands"].as_array().into_iter().flatten() {
                    println!("  {:<38} {}", c["usage"].as_str().unwrap_or(""), c["about"].as_str().unwrap_or(""));
                }
            }
            ExitCode::SUCCESS
        }
        ["status"] => match call("GET", "/v1/status", None) {
            Ok(r) if r.status == 200 && !json_out => {
                println!("connected as {}", r.body["caller"]["kind"].as_str().unwrap_or("?"));
                ExitCode::SUCCESS
            }
            other => finish(other, json_out, "connected"),
        },
        ["say", rest @ ..] => {
            let mut text = Vec::new();
            let mut emotion = None;
            let mut it = rest.iter();
            while let Some(w) = it.next() {
                match *w {
                    "--emotion" => match it.next() {
                        Some(e) => emotion = Some(e.to_string()),
                        None => return usage("--emotion needs a value"),
                    },
                    w if w.starts_with("--emotion=") => emotion = Some(w["--emotion=".len()..].to_string()),
                    w => text.push(w),
                }
            }
            let text = text.join(" ");
            if text.trim().is_empty() {
                return usage("say what? buddy say <text>");
            }
            let mut body = json!({ "text": text });
            if let Some(e) = emotion {
                body["emotion"] = json!(e);
            }
            finish(call("POST", "/v1/say", Some(body)), json_out, "said")
        }
        ["mascot", "emote", name] => finish(call("POST", "/v1/mascot/emote", Some(json!({ "name": name }))), json_out, "emoted"),
        ["mascot", ..] => usage("buddy mascot emote <name>"),
        ["remind", rest @ ..] => remind(rest, json_out),
        [other, ..] => usage(&format!("unknown command “{other}”")),
    }
}
