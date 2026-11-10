//! `buddy music pause|next|status` for Music or Spotify (design `## widgets`): AppleScript through
//! `/usr/bin/osascript`, sent only to a player that is already running (NSWorkspace's running
//! apps), so nothing is launched. The first such script makes macOS ask the person to let
//! desk-buddy control that app (Automation); a refusal comes back as an error. `now_playing` is
//! the same status, for `Senses.now_playing`.

use std::process::Command;

use objc2_app_kit::NSWorkspace;
use serde_json::{json, Value};

/// (bundle id, name) of the players, in preference order.
const PLAYERS: [(&str, &str); 2] = [("com.apple.Music", "Music"), ("com.spotify.client", "Spotify")];

const STATUS: &str = r#"set s to (player state as string)
set t to ""
try
  set t to (name of current track)
end try
return s & linefeed & t"#;

/// The players running now (bundle id, name).
fn running() -> Vec<(&'static str, &'static str)> {
    let apps = NSWorkspace::sharedWorkspace().runningApplications();
    let ids: Vec<String> = apps.iter().filter_map(|a| a.bundleIdentifier().map(|b| b.to_string())).collect();
    PLAYERS.iter().copied().filter(|(id, _)| ids.iter().any(|x| x == id)).collect()
}

fn osascript(bundle: &str, body: &str) -> Result<String, String> {
    let script = format!("tell application id \"{bundle}\"\n{body}\nend tell");
    let out = Command::new("/usr/bin/osascript").env_clear().arg("-e").arg(script).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        let e = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return Err(if e.contains("-1743") { "desk-buddy isn't allowed to control that app (System Settings › Privacy & Security › Automation)".into() } else { e });
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim_end().to_string())
}

/// A status script's output as `{playing, paused, title, source}`.
pub fn parse_status(out: &str, source: &str) -> Value {
    let (s, t) = out.split_once('\n').unwrap_or((out, ""));
    let s = s.trim();
    json!({ "playing": s == "playing" || s == "kPSP", "paused": s == "paused" || s == "kPSp", "title": t.trim(), "source": source })
}

fn status_of(bundle: &str, name: &str) -> Result<Value, String> {
    osascript(bundle, STATUS).map(|o| parse_status(&o, name))
}

/// `pause` / `next` / `status` for the running player (the playing one first); `None` when none runs.
pub fn control(op: &str) -> Option<Result<Value, String>> {
    let players = running();
    if players.is_empty() {
        return None;
    }
    let states: Vec<_> = players.iter().map(|(b, n)| (*b, *n, status_of(b, n))).collect();
    let pick = states.iter().find(|(_, _, s)| s.as_ref().is_ok_and(|v| v["playing"] == true)).or(states.first())?;
    let (bundle, name) = (pick.0, pick.1);
    Some(match op {
        "status" => pick.2.clone().map(|s| json!({ "state": s })),
        "pause" => osascript(bundle, "pause").and_then(|_| status_of(bundle, name)).map(|s| json!({ "state": s })),
        "next" => osascript(bundle, "next track").and_then(|_| status_of(bundle, name)).map(|s| json!({ "state": s })),
        other => Err(format!("{name} can't {other}")),
    })
}

/// What the running player plays (`None` when none runs or none plays).
pub fn now_playing() -> Option<Value> {
    for (b, n) in running() {
        if let Ok(s) = status_of(b, n) {
            if s["playing"] == true || s["paused"] == true {
                return Some(s);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_lines_parse() {
        assert_eq!(parse_status("playing\nSo What", "Music"), json!({ "playing": true, "paused": false, "title": "So What", "source": "Music" }));
        assert_eq!(parse_status("paused\n", "Spotify")["paused"], true);
        assert_eq!(parse_status("stopped", "Music")["playing"], false);
    }
}
