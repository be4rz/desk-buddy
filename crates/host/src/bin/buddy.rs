//! `buddy`: the agent's (and the person's) way to make the mascot act.
//!
//!   buddy say <text…> [--emotion <e>]   speak in a bubble
//!   buddy mascot emote <name>            play an emote
//!   buddy status [--json]                who the API sees, and the turn
//!   buddy --help [--json]                this list; `--json` for agents
//!
//! It reaches the host API with `BUDDY_API` + `BUDDY_TOKEN` (+ `BUDDY_TURN`),
//! which the agent's commands get; from a terminal it reads the port file
//! (`BUDDY_HOST_FILE`, else `~/.desk-buddy/host.json`). Exit codes: 0 done,
//! 1 refused or unreachable, 2 usage.

use std::path::PathBuf;
use std::process::ExitCode;

use host::client::{request, Reply};
use host::{PortFile, EMOTES, EMOTIONS};
use serde_json::{json, Value};

fn help() -> Value {
    json!({
        "name": "buddy",
        "about": "Make the desk buddy act. Commands that change what the person sees ask for approval unless allowed.",
        "commands": [
            { "usage": "buddy say <text…> [--emotion <e>]", "about": "Speak in a speech bubble. Keep it short.", "emotions": EMOTIONS, "readOnly": false },
            { "usage": "buddy mascot emote <name>", "about": "Play an emote.", "emotes": EMOTES, "readOnly": false },
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
}

fn target() -> Result<Target, String> {
    let env = |k: &str| std::env::var(k).ok().filter(|v| !v.is_empty());
    if let (Some(api), Some(token)) = (env("BUDDY_API"), env("BUDDY_TOKEN")) {
        let port = api.strip_prefix("http://127.0.0.1:").and_then(|p| p.trim_end_matches('/').parse().ok()).ok_or_else(|| format!("BUDDY_API={api} isn’t http://127.0.0.1:<port>"))?;
        return Ok(Target { port, token, turn: env("BUDDY_TURN") });
    }
    let file = env("BUDDY_HOST_FILE").map(PathBuf::from).or_else(|| env("HOME").map(|h| PathBuf::from(h).join(".desk-buddy").join("host.json"))).ok_or("no BUDDY_API and no home folder")?;
    let pf = PortFile::read(&file).map_err(|e| format!("desk-buddy isn’t running ({}: {e})", file.display()))?;
    Ok(Target { port: pf.port, token: pf.token, turn: None })
}

fn call(method: &str, path: &str, body: Option<Value>) -> Result<Reply, String> {
    let t = target()?;
    let auth = format!("Bearer {}", t.token);
    let mut headers = vec![("Authorization", auth.as_str())];
    if let Some(turn) = &t.turn {
        headers.push(("X-Buddy-Turn", turn.as_str()));
    }
    request(t.port, method, path, &headers, body.as_ref()).map_err(|e| format!("couldn’t reach desk-buddy: {e}"))
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
        [other, ..] => usage(&format!("unknown command “{other}”")),
    }
}
