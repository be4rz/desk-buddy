use std::sync::mpsc;
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
    assert_eq!(request(port, "POST", "/v1/mascot/emote", &h, Some(&json!({ "name": "explode" }))).unwrap().status, 400);
    assert_eq!(request(port, "GET", "/v1/say", &h, None).unwrap().status, 405);
    assert_eq!(request(port, "GET", "/v2/anything", &h, None).unwrap().status, 404);
    let r = request(port, "POST", "/v1/mascot/emote", &h, Some(&json!({ "name": "wave" }))).unwrap();
    assert_eq!(r.status, 200);
    assert_eq!(fx.events.recv_timeout(Duration::from_secs(2)).unwrap().action, Action::Emote { name: "wave".into() });
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
