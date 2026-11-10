//! The manager against a fake app: windows are records, pages answer from a script.
use super::*;
use std::sync::{OnceLock, Weak};

#[derive(Default)]
struct Fake {
    mgr: OnceLock<Weak<Manager>>,
    log: Mutex<Vec<String>>,
    /// what the browser page's script returns
    page: Mutex<Option<String>>,
    os: Mutex<Option<Value>>,
    /// the media page never answers
    mute: Mutex<bool>,
    /// the page never calls ready
    no_ready: Mutex<bool>,
}

impl Fake {
    fn m(&self) -> Arc<Manager> {
        self.mgr.get().and_then(Weak::upgrade).unwrap()
    }
    fn log(&self) -> Vec<String> {
        self.log.lock().unwrap().clone()
    }
    fn note(&self, s: String) {
        self.log.lock().unwrap().push(s);
    }
}

impl Shell for Fake {
    fn open(&self, m: &Manifest, url: &str) -> Result<(), String> {
        self.note(format!("open {} {url}", m.id));
        if !*self.no_ready.lock().unwrap() {
            let (mgr, id) = (self.m(), m.id.clone());
            std::thread::spawn(move || mgr.ready(&id));
        }
        Ok(())
    }
    fn close(&self, widget: &str) {
        self.note(format!("close {widget}"));
    }
    fn set_visible(&self, m: &Manifest, visible: bool) {
        self.note(format!("visible {} {visible}", m.id));
    }
    fn send(&self, widget: &str, msg: &Value) -> Result<(), String> {
        if msg["kind"] != "action" {
            self.note(format!("send {widget} {}", msg["kind"].as_str().unwrap_or("")));
            return Ok(());
        }
        self.note(format!("action {widget} {} {}", msg["name"].as_str().unwrap(), msg["args"]));
        if *self.mute.lock().unwrap() {
            return Ok(());
        }
        let (mgr, w, req, name, args) = (self.m(), widget.to_string(), msg["req"].as_str().unwrap().to_string(), msg["name"].as_str().unwrap().to_string(), msg["args"].clone());
        std::thread::spawn(move || {
            let reply = match name.as_str() {
                "play" if args["videoId"].as_str().unwrap().starts_with("blocked") => json!({ "ok": true, "value": { "state": { "playing": false }, "embedError": 150 } }),
                "play" => {
                    let st = json!({ "playing": true, "videoId": args["videoId"], "title": "t", "source": "youtube" });
                    mgr.state(&w, st.clone());
                    json!({ "ok": true, "value": { "state": st } })
                }
                "status" => json!({ "ok": true, "value": { "state": { "playing": true, "hidden": true } } }),
                _ => json!({ "ok": false, "code": "nothing_playing", "message": "nothing is playing" }),
            };
            mgr.reply(&w, &req, reply);
        });
        Ok(())
    }
    fn browser_goto(&self, url: &str) -> Result<(), String> {
        self.note(format!("goto {url}"));
        Ok(())
    }
    fn browser_eval(&self, script: &str) -> Result<String, String> {
        assert!(script.contains("data-buddy-ref"), "evaluates the inject script");
        let op = script.rsplit("})(").next().unwrap().trim_end_matches(");\n").to_string();
        self.note(format!("eval {op}"));
        if op.contains("e999") {
            return Ok(r#"{"ok":false,"code":"no_element","message":"no such element: e999 (take a new snapshot)"}"#.into());
        }
        Ok(self.page.lock().unwrap().clone().unwrap_or_else(|| r#"{"ok":true,"url":"http://127.0.0.1:9/page","title":"Order","text":"[e1] link “Track”","refs":1}"#.into()))
    }
    fn os_media(&self, op: &str) -> Option<Result<Value, String>> {
        self.note(format!("os {op}"));
        self.os.lock().unwrap().clone().map(Ok)
    }
}

const ORIGIN: &str = "http://127.0.0.1:47001";

fn setup() -> (Arc<Fake>, Arc<Manager>) {
    let fake = Arc::new(Fake::default());
    let search: Search = Box::new(|q| Ok(vec![Video { id: "aaaaaaaaaa1".into(), title: q.into() }, Video { id: "aaaaaaaaaa2".into(), title: q.into() }]));
    let m = Arc::new(Manager::new(fake.clone(), crate::builtin(), ORIGIN, false).with_search(search).with_timeouts(Duration::from_millis(800), Duration::from_millis(800)));
    m.refuse_port(47002);
    fake.mgr.set(Arc::downgrade(&m)).ok();
    (fake, m)
}

#[test]
fn open_list_close() {
    let (fake, m) = setup();
    assert_eq!(m.open("media", None).unwrap(), json!({ "id": "w_1" }));
    assert_eq!(m.open("music", None).unwrap(), json!({ "id": "w_1" }), "one per widget; the CLI word works too");
    assert_eq!(fake.log()[0], format!("open media {ORIGIN}/media/index.html"));
    let l = m.list();
    assert_eq!(l["widgets"][0]["kind"], "media");
    assert_eq!(l["widgets"][0]["visible"], true);
    m.visibility("media", "hide");
    assert_eq!(m.list()["widgets"][0]["visible"], false);
    assert_eq!(m.close(false, "w_1").unwrap(), json!({}));
    assert_eq!(m.list()["widgets"], json!([]));
    assert_eq!(m.close(false, "w_1").unwrap_err().code, "no_widget");
    assert_eq!(m.open("nope", None).unwrap_err().code, "no_widget");
    assert_eq!(m.open("media", Some("https://example.com")).unwrap_err().code, "usage");
}

#[test]
fn commands_list_every_action() {
    let (_, m) = setup();
    let c = m.commands();
    let usages: Vec<&str> = c["commands"].as_array().unwrap().iter().map(|c| c["usage"].as_str().unwrap()).collect();
    assert!(usages.contains(&"buddy music play <query…>"), "{usages:?}");
    assert!(usages.contains(&"buddy browser type <ref> <text…>"), "{usages:?}");
    assert!(m.action(false, "music", "dance", &json!({})).unwrap_err().message.contains("play, pause, next, status"));
}

#[test]
fn music_play_routes_to_the_page_and_status_works_hidden() {
    let (fake, m) = setup();
    let d = m.action(true, "music", "play", &json!({ "query": "lofi" })).unwrap();
    assert_eq!(d.value["state"]["playing"], true);
    assert_eq!(d.taint, None);
    assert!(fake.log().iter().any(|l| l.starts_with("action media play") && l.contains("aaaaaaaaaa1") && l.contains("\"by\":\"agent\"")), "{:?}", fake.log());
    m.visibility("media", "hide");
    let s = m.action(false, "music", "status", &json!({})).unwrap();
    assert_eq!(s.value["state"]["playing"], true);
    assert_eq!(m.now_playing().unwrap()["videoId"], "aaaaaaaaaa1");
    // next walks the search results, then runs out
    m.action(false, "music", "next", &json!({})).unwrap();
    assert!(fake.log().iter().any(|l| l.contains("aaaaaaaaaa2")));
    assert_eq!(m.action(false, "music", "next", &json!({})).unwrap_err().code, "nothing_next");
}

#[test]
fn embed_error_150_opens_the_browser_widget() {
    let (fake, m) = setup();
    let d = m.action(true, "music", "play", &json!({ "query": "https://youtu.be/blocked0150" })).unwrap();
    assert_eq!(d.value["openedIn"], "browser");
    assert_eq!(d.value["state"]["playing"], false);
    assert!(d.value["message"].as_str().unwrap().contains("error 150"), "{}", d.value);
    assert!(fake.log().contains(&"goto https://www.youtube.com/watch?v=blocked0150".to_string()), "{:?}", fake.log());
    assert_eq!(m.list()["widgets"].as_array().unwrap().len(), 2);
}

#[test]
fn pause_without_the_widget_goes_to_a_running_os_player_only() {
    let (fake, m) = setup();
    assert_eq!(m.action(false, "music", "pause", &json!({})).unwrap_err().code, "nothing_playing");
    assert_eq!(m.action(false, "music", "status", &json!({})).unwrap().value, json!({ "state": { "playing": false } }));
    *fake.os.lock().unwrap() = Some(json!({ "state": { "playing": false, "source": "Music" } }));
    assert_eq!(m.action(false, "music", "pause", &json!({})).unwrap().value["state"]["source"], "Music");
    assert!(!fake.log().iter().any(|l| l.starts_with("open")), "no widget opened for an OS player");
}

#[test]
fn browser_needs_an_open_widget_and_names_missing_refs() {
    let (_, m) = setup();
    let e = m.action(true, "browser", "snapshot", &json!({})).unwrap_err();
    assert_eq!((e.status, e.message.as_str()), (404, "no browser widget is open (buddy widget open browser)"));
    m.action(true, "browser", "goto", &json!({ "url": "http://127.0.0.1:9/page" })).unwrap();
    let e = m.action(true, "browser", "click", &json!({ "ref": "e999" })).unwrap_err();
    assert_eq!((e.status, e.code, e.message.as_str()), (404, "no_element", "no such element: e999 (take a new snapshot)"));
    assert_eq!(m.action(true, "browser", "click", &json!({})).unwrap_err().code, "usage");
}

#[test]
fn an_agent_read_taints_and_a_person_read_does_not() {
    let (fake, m) = setup();
    m.open("browser", Some("http://127.0.0.1:9/page")).unwrap();
    let d = m.action(false, "browser", "snapshot", &json!({})).unwrap();
    assert_eq!(d.taint, None);
    assert_eq!(m.list()["widgets"][0]["state"]["tainted"], false);
    let d = m.action(true, "browser", "snapshot", &json!({})).unwrap();
    assert_eq!(d.taint.as_deref(), Some("http://127.0.0.1:9/page"));
    assert_eq!(d.value["text"], "[e1] link “Track”");
    assert_eq!(m.list()["widgets"][0]["state"]["tainted"], true, "the banner shows it");
    m.action(true, "browser", "goto", &json!({ "url": "https://example.org" })).unwrap();
    assert_eq!(m.list()["widgets"][0]["state"]["tainted"], true, "the conversation stays marked after a goto");
    assert!(fake.log().iter().filter(|l| *l == "send browser state").count() >= 2);
}

#[test]
fn goto_refuses_desk_buddys_own_ports_and_other_schemes() {
    let (fake, m) = setup();
    for u in ["http://127.0.0.1:47001/media/index.html", "http://localhost:47002/v1/status", "http://[::1]:47001/", "http://2130706433:47001/", "http://x@127.0.0.1:47002/", "file:///etc/passwd", "javascript:alert(1)", "ipc://localhost/widget_reply"] {
        let e = m.action(true, "browser", "goto", &json!({ "url": u })).unwrap_err();
        assert_eq!(e.status, 403, "{u}: {e:?}");
    }
    assert_eq!(m.open("browser", Some("http://127.0.0.1:47001/sdk/buddy-widget.js")).unwrap_err().code, "forbidden_url");
    assert!(!fake.log().iter().any(|l| l.starts_with("goto")));
    assert!(check_url("https://example.com/a?b=c", &[47001]).is_ok());
    assert!(check_url("http://127.0.0.1:8123/fixture.html", &[47001, 47002]).is_ok());
}

#[test]
fn take_over_stops_the_agent_not_the_person() {
    let (_, m) = setup();
    m.open("browser", Some("https://example.com")).unwrap();
    m.control("browser", "take_over").unwrap();
    assert_eq!(m.action(true, "browser", "click", &json!({ "ref": "e1" })).unwrap_err().code, "taken_over");
    assert_eq!(m.action(true, "browser", "goto", &json!({ "url": "https://example.org" })).unwrap_err().code, "taken_over");
    assert_eq!(m.open("browser", Some("https://example.org")).unwrap_err().code, "taken_over", "open <url> navigates too");
    assert!(m.action(false, "browser", "click", &json!({ "ref": "e1" })).is_ok());
    assert!(m.action(true, "browser", "snapshot", &json!({})).is_ok(), "reading is still fine");
    // closing and reopening doesn't hand it back (review p5 S3)
    assert_eq!(m.close(true, "browser").unwrap_err().code, "taken_over");
    assert_eq!(m.close(true, "w_1").unwrap_err().code, "taken_over");
    assert_eq!(m.action(true, "browser", "goto", &json!({ "url": "https://example.org" })).unwrap_err().code, "taken_over", "still taken over");
    m.control("browser", "hand_back").unwrap();
    assert!(m.action(true, "browser", "click", &json!({ "ref": "e1" })).is_ok());
    assert!(m.control("browser", "launch").is_err());
}

#[test]
fn a_silent_page_times_out_and_a_page_that_never_loads_is_closed() {
    let (fake, m) = setup();
    *fake.mute.lock().unwrap() = true;
    let e = m.action(false, "music", "play", &json!({ "query": "jfKfPfyJRdk" })).unwrap_err();
    assert_eq!(e.code, "widget_timeout");
    // a late reply is dropped
    m.reply("media", "r1", json!({ "ok": true }));
    let (fake2, m2) = setup();
    *fake2.no_ready.lock().unwrap() = true;
    assert_eq!(m2.open("browser", None).unwrap_err().code, "widget_timeout");
    assert!(fake2.log().contains(&"close browser".to_string()));
    assert_eq!(m2.list()["widgets"], json!([]));
}

#[test]
fn page_checks() {
    let (_, m) = setup();
    assert!(m.is_widget_page("media", "http://127.0.0.1:47001/media/index.html?fake=1"));
    assert!(!m.is_widget_page("media", "http://127.0.0.1:47001/browser/index.html"));
    assert!(!m.is_widget_page("media", "http://127.0.0.1:470011/media/index.html"));
    assert!(!m.is_widget_page("media", "http://127.0.0.1:47001/mediax/index.html"));
    assert!(!m.is_widget_page("media", "https://www.youtube.com/media/"));
    // state is capped and must be an object
    m.open("media", None).unwrap();
    m.state("media", json!("x"));
    m.state("media", json!({ "big": "x".repeat(9000) }));
    assert_eq!(m.list()["widgets"][0]["state"], json!({}));
}
