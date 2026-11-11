use std::sync::mpsc;
use std::sync::Arc;
use std::time::Duration;

use super::client::request;
use super::*;

struct Fx {
    host: Host,
    events: mpsc::Receiver<HostEvent>,
    dir: PathBuf,
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

fn start(name: &str) -> Fx {
    let dir = std::env::temp_dir().join(format!("buddy-host-{name}-{}-{}", std::process::id(), random_hex(3)));
    let (tx, events) = mpsc::channel();
    let tx = Mutex::new(tx);
    let host = Host::start(&dir.join("host.json"), move |e| {
        let _ = tx.lock().unwrap().send(e);
        Ok(json!({}))
    })
    .unwrap();
    Fx { host, events, dir }
}

fn say(fx: &Fx, headers: &[(&str, &str)]) -> client::Reply {
    request(fx.host.port(), "POST", "/v1/say", headers, Some(&json!({ "text": "Hi!" }))).unwrap()
}

#[test]
fn the_port_file_is_private_and_holds_the_users_token() {
    let fx = start("file");
    let path = fx.dir.join("host.json");
    let pf = PortFile::read(&path).unwrap();
    assert_eq!(pf, PortFile { port: fx.host.port(), token: fx.host.user_token().to_string() });
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(fs::metadata(&path).unwrap().permissions().mode() & 0o777, 0o600);
    }
    let auth = format!("Bearer {}", pf.token);
    let r = request(fx.host.port(), "GET", "/v1/status", &[("Authorization", &auth)], None).unwrap();
    assert_eq!((r.status, r.body["caller"]["kind"].as_str()), (200, Some("user")));
}

#[test]
fn no_token_or_a_wrong_one_is_401() {
    let fx = start("401");
    assert_eq!(say(&fx, &[]).status, 401);
    assert_eq!(say(&fx, &[("Authorization", "Bearer ba_nope")]).status, 401);
    assert_eq!(say(&fx, &[("Authorization", "Bearer ")]).status, 401);
    assert_eq!(say(&fx, &[("Authorization", fx.host.user_token())]).status, 401, "the scheme is required");
    assert!(fx.events.try_recv().is_err());
}

#[test]
fn an_origin_header_or_a_foreign_host_is_403() {
    let fx = start("403");
    let auth = format!("Bearer {}", fx.host.user_token());
    assert_eq!(say(&fx, &[("Authorization", &auth), ("Origin", "https://evil.example")]).status, 403);
    assert_eq!(say(&fx, &[("Authorization", &auth), ("Origin", "null")]).status, 403);
    let wrong = format!("evil.example:{}", fx.host.port());
    assert_eq!(say(&fx, &[("Authorization", &auth), ("Host", &wrong)]).status, 403);
    assert_eq!(say(&fx, &[("Authorization", &auth), ("Host", "127.0.0.1:1")]).status, 403);
    let local = format!("localhost:{}", fx.host.port());
    assert_eq!(say(&fx, &[("Authorization", &auth), ("Host", &local)]).status, 200);
    assert!(matches!(fx.events.recv_timeout(Duration::from_secs(2)).unwrap().action, Action::Say { .. }));
    assert!(fx.events.try_recv().is_err(), "only the allowed request reached the mascot");
}

#[test]
fn an_agent_token_works_until_revoked_and_marks_late_turns() {
    let fx = start("agent");
    let token = fx.host.issue("a1", 3);
    let auth = format!("Bearer {token}");
    fx.host.set_turn("a1", Some("u:turn-1"));
    let r = say(&fx, &[("Authorization", &auth), ("X-Buddy-Turn", "u:turn-1")]);
    assert_eq!((r.status, &r.body["afterTurn"]), (200, &json!(false)));
    let e = fx.events.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(e.caller, Caller::Agent { agent_id: "a1".into(), gen: 3 });
    assert_eq!((e.turn.as_deref(), e.after_turn), (Some("u:turn-1"), false));

    // The turn ended (a background command still running): accepted, flagged.
    fx.host.set_turn("a1", None);
    let r = say(&fx, &[("Authorization", &auth), ("X-Buddy-Turn", "u:turn-1")]);
    assert_eq!((r.status, &r.body["afterTurn"]), (200, &json!(true)));
    assert!(fx.events.recv_timeout(Duration::from_secs(2)).unwrap().after_turn);
    // A newer turn is running: still after its own.
    fx.host.set_turn("a1", Some("u:turn-2"));
    assert_eq!(say(&fx, &[("Authorization", &auth), ("X-Buddy-Turn", "u:turn-1")]).body["afterTurn"], true);
    let _ = fx.events.recv_timeout(Duration::from_secs(2));

    fx.host.revoke_gen("a1", 3);
    assert_eq!(say(&fx, &[("Authorization", &auth)]).status, 401);
    assert!(fx.events.try_recv().is_err());
}

#[test]
fn bad_bodies_and_unknown_routes_are_refused() {
    let fx = start("bodies");
    let auth = format!("Bearer {}", fx.host.user_token());
    let h = [("Authorization", auth.as_str())];
    let port = fx.host.port();
    assert_eq!(request(port, "POST", "/v1/say", &h, Some(&json!({ "text": "  " }))).unwrap().status, 400);
    assert_eq!(request(port, "POST", "/v1/say", &h, Some(&json!({ "text": "hi", "emotion": "furious" }))).unwrap().status, 400);
    assert_eq!(request(port, "POST", "/v1/say", &h, Some(&json!({ "text": "x".repeat(MAX_SAY + 1) }))).unwrap().status, 400);
    assert_eq!(request(port, "POST", "/v1/say", &h, Some(&json!(["not", "an", "object"]))).unwrap().status, 400);
    assert_eq!(request(port, "POST", "/v1/mascot/emote", &h, Some(&json!({ "name": "Explode!" }))).unwrap().status, 400);
    assert_eq!(request(port, "GET", "/v1/say", &h, None).unwrap().status, 405);
    assert_eq!(request(port, "GET", "/v2/anything", &h, None).unwrap().status, 404);
    let r = request(port, "POST", "/v1/mascot/emote", &h, Some(&json!({ "name": "wave" }))).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(fx.events.recv_timeout(Duration::from_secs(2)).unwrap().action, Action::Mascot(MascotOp::Emote { name: "wave".into() }));
    assert!(fx.events.try_recv().is_err());
}

#[test]
fn the_port_file_goes_away_with_the_host() {
    let fx = start("drop");
    let path = fx.dir.join("host.json");
    assert!(path.exists());
    drop(fx);
    assert!(!path.exists());
}

/// A host whose sink keeps reminders the way the bridge's agent does.
fn start_reminders(name: &str, agent: bool) -> (Host, PathBuf, Arc<Mutex<Vec<(String, u64, String)>>>) {
    let dir = std::env::temp_dir().join(format!("buddy-host-{name}-{}-{}", std::process::id(), random_hex(3)));
    let store: Arc<Mutex<Vec<(String, u64, String)>>> = Arc::default();
    let s2 = store.clone();
    let host = Host::start(&dir.join("host.json"), move |e| {
        if !agent {
            return Err(Refusal::no_agent());
        }
        let mut s = s2.lock().unwrap();
        match e.action {
            Action::Remind(RemindOp::Add { at_ms, text }) => {
                let id = format!("r_{}", random_hex(6));
                s.push((id.clone(), at_ms, text));
                Ok(json!({ "id": id, "atMs": at_ms, "ok": "ignored" }))
            }
            Action::Remind(RemindOp::List) => Ok(json!({ "reminders": s.iter().map(|(id, at, t)| json!({ "id": id, "atMs": at, "text": t })).collect::<Vec<_>>() })),
            Action::Remind(RemindOp::Cancel { id }) => match s.iter().position(|r| r.0 == id) {
                Some(i) => {
                    s.remove(i);
                    Ok(json!({}))
                }
                None => Err(Refusal::new(404, "not_found", "No such reminder.")),
            },
            _ => Ok(json!({})),
        }
    })
    .unwrap();
    (host, dir, store)
}

#[test]
fn reminders_add_list_and_cancel_through_the_sink() {
    let (host, dir, store) = start_reminders("remind", true);
    let auth = format!("Bearer {}", host.user_token());
    let h = [("Authorization", auth.as_str())];
    let port = host.port();
    let before = now_ms();
    let r = request(port, "POST", "/v1/remind", &h, Some(&json!({ "inMs": 60_000, "text": " stretch " }))).unwrap();
    assert_eq!(r.status, 200, "{:?}", r.body);
    assert_eq!(r.body["ok"], true, "the envelope's ok wins");
    let at = r.body["atMs"].as_u64().unwrap();
    assert!(at >= before + 60_000 && at <= now_ms() + 60_000);
    let id = r.body["id"].as_str().unwrap().to_string();
    assert_eq!(store.lock().unwrap()[0].2, "stretch");
    let l = request(port, "GET", "/v1/reminders", &h, None).unwrap();
    assert_eq!(l.body["reminders"][0]["id"], id.as_str());
    // HH:MM is the next such local time.
    let r = request(port, "POST", "/v1/remind", &h, Some(&json!({ "at": "07:05", "text": "tea" }))).unwrap();
    assert_eq!(r.status, 200, "{:?}", r.body);
    let at = r.body["atMs"].as_u64().unwrap();
    assert!(at > now_ms() && at <= now_ms() + 24 * 3600 * 1000);
    assert_eq!(local_hhmm(at).as_deref(), Some("07:05"));

    assert_eq!(request(port, "POST", "/v1/remind/cancel", &h, Some(&json!({ "id": id }))).unwrap().status, 200);
    // Unknown id: the sink's 404.
    let r = request(port, "POST", "/v1/remind/cancel", &h, Some(&json!({ "id": id }))).unwrap();
    assert_eq!((r.status, r.body["error"]["code"].as_str()), (404, Some("not_found")));
    drop(host);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn bad_reminders_are_usage_errors_and_no_agent_is_503() {
    let (host, dir, store) = start_reminders("remind-bad", true);
    let auth = format!("Bearer {}", host.user_token());
    let h = [("Authorization", auth.as_str())];
    let port = host.port();
    let bad = [
        json!({ "inMs": 0, "text": "x" }),
        json!({ "inMs": 9_999, "text": "x" }),
        json!({ "inMs": MAX_REMIND_MS + 1, "text": "x" }),
        json!({ "inMs": 60_000 }),
        json!({ "inMs": 60_000, "text": "x".repeat(MAX_REMIND_TEXT + 1) }),
        json!({ "text": "x" }),
        json!({ "inMs": 60_000, "at": "10:00", "text": "x" }),
        json!({ "at": "25:00", "text": "x" }),
        json!({ "at": "9:5", "text": "x" }),
    ];
    for b in bad {
        let r = request(port, "POST", "/v1/remind", &h, Some(&b)).unwrap();
        assert_eq!((r.status, r.body["error"]["code"].as_str()), (400, Some("usage")), "{b}");
    }
    assert_eq!(request(port, "POST", "/v1/remind/cancel", &h, Some(&json!({}))).unwrap().status, 400);
    assert_eq!(request(port, "GET", "/v1/remind", &h, None).unwrap().status, 405);
    assert!(store.lock().unwrap().is_empty());
    // A lower floor for tests and QA.
    host.set_min_remind_ms(1000);
    assert_eq!(request(port, "POST", "/v1/remind", &h, Some(&json!({ "inMs": 1000, "text": "x" }))).unwrap().status, 200);
    drop(host);
    let _ = fs::remove_dir_all(dir);

    let (host, dir, _) = start_reminders("remind-none", false);
    let auth = format!("Bearer {}", host.user_token());
    let r = request(host.port(), "POST", "/v1/remind", &[("Authorization", &auth)], Some(&json!({ "inMs": 60_000, "text": "x" }))).unwrap();
    assert_eq!((r.status, r.body["error"]["code"].as_str(), r.body["ok"].as_bool()), (503, Some("no_agent"), Some(false)));
    assert_eq!(r.body["error"]["message"], NO_AGENT);
    drop(host);
    let _ = fs::remove_dir_all(dir);
}

#[test]
fn durations_and_times_parse_strictly() {
    for (s, ms) in [("90s", 90_000), ("20m", 1_200_000), ("1h30m", 5_400_000), ("2d", 172_800_000), ("1d12h", 129_600_000), ("0s", 0)] {
        assert_eq!(parse_duration(s), Some(ms), "{s}");
    }
    for s in ["", "20", "m", "1m1h", "1m1m", "1.5h", "-5m", "5 m", "10x", "9999999999s"] {
        assert_eq!(parse_duration(s), None, "{s}");
    }
    assert_eq!(parse_hhmm("14:30"), Some((14, 30)));
    assert_eq!(parse_hhmm("7:05"), Some((7, 5)));
    for s in ["24:00", "12:60", "12:5", "1230", ":30", "a:bc", "123:00"] {
        assert_eq!(parse_hhmm(s), None, "{s}");
    }
}

#[cfg(unix)]
#[test]
fn next_local_is_today_or_tomorrow() {
    let now = now_ms();
    let hhmm = local_hhmm(now).unwrap();
    let (h, m) = parse_hhmm(&hhmm).unwrap();
    // This very minute has started: the next one is tomorrow's (give or take a DST hour).
    let next = next_local(h, m, now).unwrap();
    assert!(next > now + 22 * 3600 * 1000 && next <= now + 25 * 3600 * 1000, "{next} vs {now}");
    let soon = next_local((h + 1) % 24, m, now).unwrap();
    assert!(soon > now && soon <= now + 25 * 3600 * 1000);
    assert_eq!(local_hhmm(soon).unwrap(), format!("{:02}:{:02}", (h + 1) % 24, m));
}

#[test]
fn widget_endpoints_reach_the_sink_as_widget_ops() {
    let dir = std::env::temp_dir().join(format!("buddy-host-widget-{}-{}", std::process::id(), random_hex(3)));
    let host = Host::start(&dir.join("host.json"), |e| match e.action {
        Action::Widget(WidgetOp::Open { kind, url }) => Ok(json!({ "id": "w_1", "kind": kind, "url": url })),
        Action::Widget(WidgetOp::List) => Ok(json!({ "widgets": [] })),
        Action::Widget(WidgetOp::Commands) => Ok(json!({ "commands": [{ "usage": "buddy music play <query…>" }] })),
        Action::Widget(WidgetOp::Close { id }) if id == "w_1" => Ok(json!({})),
        Action::Widget(WidgetOp::Close { .. }) => Err(Refusal::new(404, "no_widget", "no open widget")),
        Action::Widget(WidgetOp::Action { widget, action, args }) if action == "click" => Err(Refusal::new(404, "no_element", format!("no such element: {} ({widget})", args["ref"].as_str().unwrap_or("")))),
        Action::Widget(WidgetOp::Action { widget, action, args }) => Ok(json!({ "widget": widget, "action": action, "args": args })),
        _ => Ok(json!({})),
    })
    .unwrap();
    let auth = format!("Bearer {}", host.user_token());
    let h = [("Authorization", auth.as_str())];
    let p = host.port();
    let r = request(p, "POST", "/v1/widget/open", &h, Some(&json!({ "kind": "browser", "url": "https://example.com" }))).unwrap();
    assert_eq!(r.body, json!({ "ok": true, "id": "w_1", "kind": "browser", "url": "https://example.com" }));
    assert_eq!(request(p, "GET", "/v1/widget/list", &h, None).unwrap().body, json!({ "ok": true, "widgets": [] }));
    assert_eq!(request(p, "GET", "/v1/widget/commands", &h, None).unwrap().body["commands"][0]["usage"], "buddy music play <query…>");
    assert_eq!(request(p, "POST", "/v1/widget/close", &h, Some(&json!({ "id": "w_1" }))).unwrap().body, json!({ "ok": true }));
    let r = request(p, "POST", "/v1/widget/close", &h, Some(&json!({ "id": "w_9" }))).unwrap();
    assert_eq!((r.status, r.body["error"]["code"].as_str()), (404, Some("no_widget")));
    let r = request(p, "POST", "/v1/widget/action", &h, Some(&json!({ "widget": "music", "action": "play", "args": { "query": "lofi" } }))).unwrap();
    assert_eq!(r.body["args"]["query"], "lofi");
    let r = request(p, "POST", "/v1/widget/action", &h, Some(&json!({ "widget": "browser", "action": "click", "args": { "ref": "e999" } }))).unwrap();
    assert_eq!((r.status, r.body["error"]["message"].as_str()), (404, Some("no such element: e999 (browser)")));
    // usage errors never reach the sink
    for (path, body) in [("/v1/widget/open", json!({})), ("/v1/widget/close", json!({ "id": " " })), ("/v1/widget/action", json!({ "widget": "music" })), ("/v1/widget/action", json!({ "widget": "music", "action": "play", "args": [1] }))] {
        let r = request(p, "POST", path, &h, Some(&body)).unwrap();
        assert_eq!((r.status, r.body["error"]["code"].as_str()), (400, Some("usage")), "{path} {body}");
    }
    assert_eq!(request(p, "POST", "/v1/widget/list", &h, Some(&json!({}))).unwrap().status, 405);
    drop(host);
    let _ = fs::remove_dir_all(&dir);
}

/// A host whose sink answers mascot ops like the app: by name, with refusals and a busy brain.
fn start_mascot(name: &str) -> (Host, PathBuf, mpsc::Receiver<HostEvent>) {
    let dir = std::env::temp_dir().join(format!("buddy-host-{name}-{}-{}", std::process::id(), random_hex(3)));
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let host = Host::start(&dir.join("host.json"), move |e| {
        let _ = tx.lock().unwrap().send(e.clone());
        let Action::Mascot(op) = e.action else { return Ok(json!({})) };
        match op {
            MascotOp::Do { name, .. } if name == "juggle" => Ok(json!({ "error": "unknown activity \"juggle\"", "have": ["read", "nap"], "message": "x" })),
            MascotOp::Do { name, .. } if name == "nap" => {
                Ok(json!({ "status": "refused", "activity": "nap", "reason": "dnd", "message": "Not now: do not disturb is on." }))
            }
            MascotOp::Do { name, for_ms } if name == "busy" => {
                let _ = for_ms;
                Err(Refusal::new(503, "busy", "busy"))
            }
            MascotOp::Do { name, for_ms } => Ok(json!({ "status": "queued", "activity": name, "forMs": for_ms.unwrap_or(120_000), "message": "Started reading for 5m (after this reply)." })),
            MascotOp::Stop => Ok(json!({ "status": "nothing", "message": "Not doing anything." })),
            MascotOp::List => Ok(json!({ "activities": [], "emotes": [], "maxForS": 1800, "playing": null, "message": "" })),
            MascotOp::Emote { name } => Ok(json!({ "status": "played", "emote": name, "message": "Waved." })),
        }
    })
    .unwrap();
    (host, dir, rx)
}

#[test]
fn mascot_routes_answer_in_the_frozen_shapes() {
    let (host, dir, rx) = start_mascot("mascot");
    let auth = format!("Bearer {}", host.user_token());
    let h = [("Authorization", auth.as_str())];
    let port = host.port();
    let post = |path: &str, body: Value| request(port, "POST", path, &h, Some(&body)).unwrap();

    let r = post("/v1/mascot/do", json!({ "name": "read", "forMs": 300_000 }));
    assert_eq!((r.status, r.body.clone()), (200, json!({ "ok": true, "status": "queued", "activity": "read", "forMs": 300000, "message": "Started reading for 5m (after this reply)." })));
    assert_eq!(rx.recv_timeout(Duration::from_secs(2)).unwrap().action, Action::Mascot(MascotOp::Do { name: "read".into(), for_ms: Some(300_000) }));
    let r = post("/v1/mascot/do", json!({ "name": "nap" }));
    assert_eq!((r.status, r.body["status"].as_str(), r.body["reason"].as_str()), (200, Some("refused"), Some("dnd")), "a refusal is an answer");
    let r = post("/v1/mascot/do", json!({ "name": "juggle" }));
    assert_eq!((r.status, r.body.clone()), (400, json!({ "ok": false, "error": "unknown activity \"juggle\"", "have": ["read", "nap"] })));
    let r = post("/v1/mascot/do", json!({ "name": "busy" }));
    assert_eq!((r.status, r.body.clone()), (503, json!({ "ok": false, "error": "busy" })));
    while rx.try_recv().is_ok() {}

    // the host's own checks never reach the app
    for (body, err) in [
        (json!({ "name": "read", "forMs": 5000 }), FOR_RANGE),
        (json!({ "name": "read", "forMs": 1_800_001 }), FOR_RANGE),
        (json!({ "name": "read", "forMs": "5m" }), FOR_RANGE),
        (json!({ "name": "Read Book" }), "bad name \"Read Book\" (a-z, 0-9, _ and -, at most 32)"),
        (json!({}), "name is missing"),
    ] {
        let r = post("/v1/mascot/do", body.clone());
        assert_eq!((r.status, r.body["ok"].clone(), r.body["error"].as_str()), (400, json!(false), Some(err)), "{body}");
    }
    assert_eq!(post("/v1/mascot/do", json!({ "name": "x".repeat(33) })).status, 400);
    assert!(rx.try_recv().is_err());
    assert_eq!(post("/v1/mascot/do", json!({ "name": "read", "forMs": 10_000 })).status, 200, "10 s is the floor");

    let r = post("/v1/mascot/stop", json!({}));
    assert_eq!((r.status, r.body["status"].as_str()), (200, Some("nothing")));
    let r = request(port, "GET", "/v1/mascot/list", &h, None).unwrap();
    assert_eq!((r.status, r.body["maxForS"].clone(), r.body["playing"].clone()), (200, json!(1800), Value::Null));
    let r = post("/v1/mascot/emote", json!({ "name": "tea-time" }));
    assert_eq!((r.status, r.body["emote"].as_str()), (200, Some("tea-time")), "the host checks the shape only");
    for path in ["/v1/mascot/do", "/v1/mascot/stop", "/v1/mascot/emote"] {
        assert_eq!(request(port, "GET", path, &h, None).unwrap().status, 405, "{path}");
    }
    assert_eq!(request(port, "POST", "/v1/mascot/list", &h, None).unwrap().status, 405);
    drop(host);
    let _ = fs::remove_dir_all(dir);
}

/// The on-time rule of `say` (plan §2): a `do`, `stop` or `emote` from a turn that already ended still
/// reaches the app, flagged, and its reply carries `afterTurn: true`; `list` and the person's own calls
/// (no turn) answer as always.
#[test]
fn mascot_routes_follow_says_after_turn_rule() {
    let (host, dir, rx) = start_mascot("mascot-late");
    let token = host.issue("a1", 1);
    let auth = format!("Bearer {token}");
    let user = format!("Bearer {}", host.user_token());
    let port = host.port();
    let on = |h: &[(&str, &str)], method: &str, path: &str, body: Option<Value>| request(port, method, path, h, body.as_ref()).unwrap();
    let calls: [(&str, &str, Option<Value>); 3] = [
        ("POST", "/v1/mascot/do", Some(json!({ "name": "read" }))),
        ("POST", "/v1/mascot/stop", Some(json!({}))),
        ("POST", "/v1/mascot/emote", Some(json!({ "name": "wave" }))),
    ];

    // on time: no flag, the frozen shape as is
    host.set_turn("a1", Some("u:t1"));
    let agent = [("Authorization", auth.as_str()), ("X-Buddy-Turn", "u:t1")];
    for (m, p, b) in calls.clone() {
        let r = on(&agent, m, p, b);
        assert_eq!((r.status, r.body.get("afterTurn")), (200, None), "{p}");
        assert!(!rx.recv_timeout(Duration::from_secs(2)).unwrap().after_turn);
    }

    // the turn ended: still acts, flagged
    host.set_turn("a1", None);
    for (m, p, b) in calls.clone() {
        let r = on(&agent, m, p, b);
        assert_eq!((r.status, r.body["ok"].clone(), r.body["afterTurn"].clone()), (200, json!(true), json!(true)), "{p}");
        assert!(rx.recv_timeout(Duration::from_secs(2)).unwrap().after_turn, "{p} reached the app");
    }
    // a newer turn running: still after its own
    host.set_turn("a1", Some("u:t2"));
    assert_eq!(on(&agent, "POST", "/v1/mascot/do", Some(json!({ "name": "read" }))).body["afterTurn"], true);
    let _ = rx.recv_timeout(Duration::from_secs(2));
    // list always answers, unflagged
    let r = on(&agent, "GET", "/v1/mascot/list", None);
    assert_eq!((r.status, r.body.get("afterTurn")), (200, None));
    let _ = rx.recv_timeout(Duration::from_secs(2));
    // a refusal from a late turn is still an answer, flagged
    let r = on(&agent, "POST", "/v1/mascot/do", Some(json!({ "name": "nap" })));
    assert_eq!((r.status, r.body["status"].as_str(), r.body["afterTurn"].clone()), (200, Some("refused"), json!(true)));
    let _ = rx.recv_timeout(Duration::from_secs(2));
    // the person's terminal: no turn id, never late
    for (m, p, b) in calls {
        let r = on(&[("Authorization", user.as_str())], m, p, b);
        assert_eq!((r.status, r.body.get("afterTurn")), (200, None), "{p}");
        assert!(!rx.recv_timeout(Duration::from_secs(2)).unwrap().after_turn);
    }
    drop(host);
    let _ = fs::remove_dir_all(dir);
}
