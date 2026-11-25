//! Widget windows (design `## widgets`, §4.9): the app's side of `widgets::Manager`.
//!
//! Pages come from the widget server (`http://127.0.0.1:<wport>/<id>/…`). A runtime capability per
//! widget grants the `widget_*` commands, and nothing else, to the webview `widget-<id>` at that
//! exact origin; each command checks its caller's label and page path again. The media window
//! autoplays (its own `WKWebViewConfiguration`) and keeps running while hidden (no background
//! throttling). The browser widget is one window, two webviews: its loopback top strip
//! `widget-browser` (IPC) and the page `browser-page` (any http(s) page except desk-buddy's own
//! ports, no capability, so no IPC), driven by `evaluateJavaScript` (Tauri's `eval_with_callback`).
//! Debug builds only: `BUDDY_MEDIA_FAKE=1` plays a local stand-in instead of YouTube.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::MainThreadMarker;
use objc2_web_kit::{WKAudiovisualMediaTypes, WKWebViewConfiguration};
use serde_json::Value;
use tauri::ipc::CapabilityBuilder;
use tauri::utils::config::BackgroundThrottlingPolicy;
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager as _, PhysicalPosition, Url, Webview, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use widgets::server::Server;
use widgets::{Manager, Manifest, Shell};

/// The commands a widget page may call (the runtime capability grants exactly these).
pub const COMMANDS: [&str; 6] = ["widget_ready", "widget_reply", "widget_state", "widget_visibility", "widget_control", "widget_drag"];
/// The browser page's webview: no capability names it.
pub const PAGE_LABEL: &str = "browser-page";
/// The browser widget's top strip height (title, url, two banners).
const CHROME_H: f64 = 104.0;
/// How long a page load or an evaluation may take.
const LOAD_WAIT: Duration = Duration::from_secs(15);
const EVAL_WAIT: Duration = Duration::from_secs(10);

static MANAGER: OnceLock<Arc<Manager>> = OnceLock::new();
static SERVER: OnceLock<Server> = OnceLock::new();
static LOADS: Mutex<Option<mpsc::Sender<()>>> = Mutex::new(None);
/// The person or the bear used Music/Spotify through `buddy music` this session: only then does
/// `Senses.now_playing` ask that app (so the Automation prompt only follows a `buddy music` use).
static OS_USED: AtomicBool = AtomicBool::new(false);
/// How often `Senses.now_playing` is refreshed.
const NOW_PLAYING_POLL: Duration = Duration::from_secs(5);
/// How often Music/Spotify is asked while nothing plays (each ask is an osascript run).
const NOW_PLAYING_IDLE: Duration = Duration::from_secs(30);
/// Where a parked (off-screen) widget was.
static PARKED: Mutex<Option<HashMap<String, PhysicalPosition<i32>>>> = Mutex::new(None);

pub fn manager() -> Option<Arc<Manager>> {
    MANAGER.get().cloned()
}

pub fn label(widget: &str) -> String {
    format!("widget-{widget}")
}

fn fake_media() -> bool {
    #[cfg(debug_assertions)]
    {
        if std::env::var("BUDDY_MEDIA_FAKE").is_ok_and(|v| v == "1") {
            return true;
        }
    }
    false
}

/// The capability of widget `id`: its `widget_*` commands, for its own webview at the widget origin.
/// The IPC check sees only the page's origin (not its path), so the path is checked twice more:
/// the window can't navigate off its own pages (`widget_nav`) and each command checks
/// `is_widget_page` on the webview's URL (`caller`).
pub fn capability(origin: &str, id: &str) -> CapabilityBuilder {
    let mut c = CapabilityBuilder::new(format!("widget-{id}")).local(false).remote(format!("{origin}/*")).webview(label(id));
    for cmd in COMMANDS {
        c = c.permission(format!("allow-{}", cmd.replace('_', "-")));
    }
    c
}

/// Start the widget server and the manager; `host_port` is never opened by the browser widget.
pub fn start(app: &AppHandle, host_port: Option<u16>) -> Result<Arc<Manager>, String> {
    let server = Server::start().map_err(|e| format!("widget server: {e}"))?;
    let origin = server.origin();
    let m = Arc::new(Manager::new(Arc::new(AppShell { app: app.clone() }), widgets::builtin(), &origin, fake_media()));
    if let Some(p) = host_port {
        m.refuse_port(p);
    }
    for w in m.registry() {
        app.add_capability(capability(&origin, &w.id)).map_err(|e| format!("widget capability: {e}"))?;
    }
    let _ = SERVER.set(server);
    let _ = MANAGER.set(m.clone());
    eprintln!("[buddy] widgets at {origin}{}", if fake_media() { " (fake media)" } else { "" });
    Ok(m)
}

/// Whether a widget window may navigate to `url`: its own pages, and for the media widget the
/// YouTube player frames (frames go through this check too).
fn widget_nav(widget: &str, url: &Url) -> bool {
    let Some(m) = manager() else { return false };
    if m.is_widget_page(widget, url.as_str()) || url.as_str() == "about:blank" || url.as_str() == "about:srcdoc" {
        return true;
    }
    let youtube = matches!(url.host_str(), Some("www.youtube.com" | "youtube.com" | "www.youtube-nocookie.com"));
    let ok = widget == "media" && url.scheme() == "https" && youtube;
    if !ok {
        eprintln!("[buddy] widget {widget}: refused navigation to {}", url.origin().ascii_serialization());
    }
    ok
}

/// Whether the browser page may go to `url` (also every link, redirect and frame).
fn page_nav(url: &Url) -> bool {
    if url.as_str() == "about:blank" || url.as_str() == "about:srcdoc" {
        return true;
    }
    let refused = manager().map(|m| m.refused_ports()).unwrap_or_default();
    let ok = widgets::manager::check_url(url.as_str(), &refused).is_ok() && url.port_or_known_default().is_some_and(|p| !refused.contains(&p));
    if !ok {
        eprintln!("[buddy] browser: refused navigation to {}", url.origin().ascii_serialization());
    }
    ok
}

fn autoplay_config(mtm: MainThreadMarker) -> Retained<WKWebViewConfiguration> {
    // SAFETY: a fresh configuration on the main thread; the setter takes a plain bit mask
    unsafe {
        let c = WKWebViewConfiguration::new(mtm);
        c.setMediaTypesRequiringUserActionForPlayback(WKAudiovisualMediaTypes::None);
        c
    }
}

fn on_closed(widget: String) -> impl Fn(&WindowEvent) + Send + 'static {
    move |e| {
        if let WindowEvent::Destroyed = e {
            if let Some(m) = manager() {
                m.closed(&widget);
            }
        }
    }
}

fn build(app: &AppHandle, m: &Manifest, url: &str) -> Result<(), String> {
    let mtm = MainThreadMarker::new().ok_or("not on the main thread")?;
    let u: Url = url.parse().map_err(|e| format!("{e}"))?;
    let (w, h) = (m.window.w as f64, m.window.h as f64);
    let id = m.id.clone();
    if m.id == "browser" {
        let win = tauri::window::WindowBuilder::new(app, label(&id))
            .title(&m.name)
            .inner_size(w, h)
            .decorations(false)
            .resizable(false)
            .always_on_top(true)
            .skip_taskbar(true)
            .visible_on_all_workspaces(true)
            .focused(false)
            .build()
            .map_err(|e| e.to_string())?;
        let id2 = id.clone();
        let chrome = WebviewBuilder::new(label(&id), WebviewUrl::External(u)).on_navigation(move |u| widget_nav(&id2, u));
        win.add_child(chrome, LogicalPosition::new(0.0, 0.0), LogicalSize::new(w, CHROME_H)).map_err(|e| e.to_string())?;
        let page = WebviewBuilder::new(PAGE_LABEL, WebviewUrl::External("about:blank".parse().unwrap()))
            .incognito(true)
            .on_navigation(page_nav)
            .on_page_load(|_, p| {
                // the first about:blank never ends a goto that is waiting for its page
                if p.event() == PageLoadEvent::Finished && p.url().scheme() != "about" {
                    if let Some(tx) = LOADS.lock().unwrap().take() {
                        let _ = tx.send(());
                    }
                }
            });
        win.add_child(page, LogicalPosition::new(0.0, CHROME_H), LogicalSize::new(w, h - CHROME_H)).map_err(|e| e.to_string())?;
        win.on_window_event(on_closed(id));
        let _ = win.center();
        return Ok(());
    }
    let id2 = id.clone();
    let win = WebviewWindowBuilder::new(app, label(&id), WebviewUrl::External(u))
        .title(&m.name)
        .inner_size(w, h)
        .decorations(false)
        .transparent(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .focused(false)
        // a hidden media widget keeps playing (the media spike, docs/spikes/media.md)
        .background_throttling(BackgroundThrottlingPolicy::Disabled)
        .with_webview_configuration(autoplay_config(mtm))
        .on_navigation(move |u| widget_nav(&id2, u))
        .build()
        .map_err(|e| e.to_string())?;
    win.on_window_event(on_closed(id));
    let _ = win.center();
    Ok(())
}

struct AppShell {
    app: AppHandle,
}

impl AppShell {
    /// Run `f` on the main thread (inline when already there) and wait for it.
    fn on_main<T: Send + 'static>(&self, f: impl FnOnce(&AppHandle) -> T + Send + 'static) -> Result<T, String> {
        if MainThreadMarker::new().is_some() {
            return Ok(f(&self.app));
        }
        let (tx, rx) = mpsc::channel();
        let app = self.app.clone();
        self.app
            .run_on_main_thread(move || {
                let _ = tx.send(f(&app));
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(EVAL_WAIT).map_err(|_| "the app didn't answer".to_string())
    }

    fn page(app: &AppHandle) -> Result<Webview, String> {
        app.get_webview(PAGE_LABEL).ok_or_else(|| "no browser widget is open".to_string())
    }
}

impl Shell for AppShell {
    fn open(&self, m: &Manifest, url: &str) -> Result<(), String> {
        let (m, url) = (m.clone(), url.to_string());
        self.on_main(move |app| build(app, &m, &url))?
    }

    fn close(&self, widget: &str) {
        let l = label(widget);
        let _ = self.on_main(move |app| {
            if let Some(w) = app.get_window(&l) {
                let _ = w.destroy();
            }
        });
    }

    fn set_visible(&self, m: &Manifest, visible: bool) {
        let (l, park) = (label(&m.id), m.window.park_offscreen);
        let _ = self.on_main(move |app| {
            let Some(w) = app.get_window(&l) else { return };
            let mut parked = PARKED.lock().unwrap();
            let parked = parked.get_or_insert_with(HashMap::new);
            if visible {
                if let Some(p) = parked.remove(&l) {
                    let _ = w.set_position(p);
                }
                let _ = w.show();
            } else if park {
                if let Ok(p) = w.outer_position() {
                    parked.insert(l.clone(), p);
                }
                let _ = w.set_position(PhysicalPosition::new(-32000, -32000));
            } else {
                let _ = w.hide();
            }
        });
    }

    fn send(&self, widget: &str, msg: &Value) -> Result<(), String> {
        let l = label(widget);
        let js = format!("window.__buddyWidget && window.__buddyWidget.dispatch({msg})");
        self.on_main(move |app| app.get_webview(&l).ok_or_else(|| "not open".to_string())?.eval(js).map_err(|e| e.to_string()))?
    }

    fn browser_goto(&self, url: &str) -> Result<(), String> {
        let u: Url = url.parse().map_err(|e| format!("{e}"))?;
        // the same guard as every page navigation, before it starts: a refused goto is an error
        // (the caller exits 1), never a silent "ok" on a page that didn't move
        if !page_nav(&u) {
            return Err(format!("refused navigation to {}", u.origin().ascii_serialization()));
        }
        let (tx, rx) = mpsc::channel();
        *LOADS.lock().unwrap() = Some(tx);
        self.on_main(move |app| Self::page(app)?.navigate(u).map_err(|e| e.to_string()))??;
        // loaded, or slow: either way the page is the widget's now
        let _ = rx.recv_timeout(LOAD_WAIT);
        Ok(())
    }

    fn browser_eval(&self, script: &str) -> Result<String, String> {
        let (tx, rx) = mpsc::channel::<String>();
        let script = script.to_string();
        self.on_main(move |app| {
            Self::page(app)?
                .eval_with_callback(script, move |s| {
                    let _ = tx.send(s);
                })
                .map_err(|e| e.to_string())
        })??;
        let raw = rx.recv_timeout(EVAL_WAIT).map_err(|_| format!("no answer in {} s", EVAL_WAIT.as_secs()))?;
        // the callback gets the result as JSON: our script returns a string
        Ok(serde_json::from_str::<String>(&raw).unwrap_or(raw))
    }

    fn os_media(&self, op: &str) -> Option<Result<Value, String>> {
        let r = crate::media_os::control(op);
        if matches!(r, Some(Ok(_))) {
            OS_USED.store(true, Ordering::Relaxed);
        }
        r
    }
}

// ---- Senses.now_playing ----

/// `{playing, title, source, …}` (widget state or an OS player's status) as the brain's `NowPlaying`.
pub fn now_playing_of(v: &Value) -> Option<mascot_core::NowPlaying> {
    let playing = v["playing"].as_bool().unwrap_or(false);
    if !playing && v["paused"] != true {
        return None;
    }
    let source = match v["source"].as_str().unwrap_or("") {
        "youtube" => "YouTube".to_string(),
        s => s.to_string(),
    };
    Some(mascot_core::NowPlaying { source, title: v["title"].as_str().unwrap_or("").chars().take(200).collect(), playing })
}

/// What plays now: the media widget's video, else (after a `buddy music` use) Music or Spotify.
/// `last` is the previous OS answer and when it was asked.
fn now_playing(last: &mut Option<(Instant, Option<mascot_core::NowPlaying>)>) -> Option<mascot_core::NowPlaying> {
    if let Some(v) = manager().and_then(|m| m.now_playing()).and_then(|v| now_playing_of(&v)) {
        return Some(v);
    }
    if !OS_USED.load(Ordering::Relaxed) {
        return None;
    }
    let due = last.as_ref().is_none_or(|(at, was)| at.elapsed() >= if was.as_ref().is_some_and(|n| n.playing) { NOW_PLAYING_POLL } else { NOW_PLAYING_IDLE });
    if due {
        *last = Some((Instant::now(), crate::media_os::now_playing().and_then(|v| now_playing_of(&v))));
    }
    last.as_ref().and_then(|(_, n)| n.clone())
}

/// The sensor: `Senses.now_playing` every 5 s, posted on change (in memory only).
pub fn watch_now_playing() {
    std::thread::Builder::new()
        .name("buddy-nowplaying".into())
        .spawn(|| {
            let mut prev = None;
            let mut os = None;
            loop {
                let now = now_playing(&mut os);
                if now != prev {
                    crate::driver::post(crate::driver::Inbox::Senses(mascot_core::replay::SensesPatch { now_playing: Some(now.clone()), ..Default::default() }));
                    prev = now;
                }
                std::thread::sleep(NOW_PLAYING_POLL);
            }
        })
        .expect("spawn now-playing thread");
}

// ---- the widget pages' commands ----

/// The widget a command came from: its webview is `widget-<id>` on that widget's own page.
fn caller(webview: &Webview, cmd: &str) -> Option<(Arc<Manager>, String)> {
    let m = manager()?;
    let url = webview.url().map(|u| u.to_string()).unwrap_or_default();
    match webview.label().strip_prefix("widget-") {
        Some(w) if m.manifest(w).is_some_and(|x| x.id == w) && m.is_widget_page(w, &url) => Some((m, w.to_string())),
        _ => {
            eprintln!("[buddy] refused {cmd} from webview {:?}", webview.label());
            None
        }
    }
}

#[tauri::command]
pub fn widget_ready(webview: Webview) {
    if let Some((m, w)) = caller(&webview, "widget_ready") {
        m.ready(&w);
    }
}

#[tauri::command]
pub fn widget_reply(webview: Webview, req: String, reply: Value) {
    if let Some((m, w)) = caller(&webview, "widget_reply") {
        m.reply(&w, &req, reply);
    }
}

#[tauri::command]
pub fn widget_state(webview: Webview, state: Value) {
    if let Some((m, w)) = caller(&webview, "widget_state") {
        m.state(&w, state);
    }
}

#[tauri::command]
pub fn widget_visibility(webview: Webview, op: String) {
    if let Some((m, w)) = caller(&webview, "widget_visibility") {
        // closing destroys the calling webview: not from inside its own command
        std::thread::spawn(move || m.visibility(&w, &op));
    }
}

#[tauri::command]
pub fn widget_control(webview: Webview, op: String) -> Result<(), String> {
    let (m, w) = caller(&webview, "widget_control").ok_or("refused")?;
    m.control(&w, &op)
}

#[tauri::command]
pub fn widget_drag(webview: Webview) {
    if caller(&webview, "widget_drag").is_some() {
        let _ = webview.window().start_dragging();
    }
}

/// Lead condition (1): the runtime widget capabilities grant only `widget_*` commands, only to
/// `widget-<id>` at the widget origin; the browser's page webview has no IPC at all.
#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn now_playing_comes_from_widget_or_player_state() {
        let np = super::now_playing_of(&json!({ "playing": true, "title": "lofi", "source": "youtube", "videoId": "abc" })).unwrap();
        assert_eq!((np.source.as_str(), np.title.as_str(), np.playing), ("YouTube", "lofi", true));
        let np = super::now_playing_of(&json!({ "playing": false, "paused": true, "title": "Song", "source": "Music" })).unwrap();
        assert_eq!((np.source.as_str(), np.playing), ("Music", false));
        assert_eq!(super::now_playing_of(&json!({ "playing": false })), None);
        assert_eq!(super::now_playing_of(&json!({})), None);
    }
}

#[cfg(test)]
mod acl {
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    const ORIGIN: &str = "http://127.0.0.1:61234";

    fn call(app: &tauri::App<tauri::test::MockRuntime>, label: &str, url: &str, cmd: &str) -> Result<(), String> {
        let w = app.get_webview_window(label).map(Ok).unwrap_or_else(|| WebviewWindowBuilder::new(app, label, WebviewUrl::External(url.parse().unwrap())).build()).unwrap();
        let req = InvokeRequest { cmd: cmd.into(), callback: CallbackFn(0), error: CallbackFn(1), url: url.parse().unwrap(), body: InvokeBody::default(), headers: Default::default(), invoke_key: INVOKE_KEY.into() };
        get_ipc_response(&w, req).map(|_| ()).map_err(|e| e.to_string())
    }

    #[test]
    fn widget_pages_get_only_their_commands_and_the_browser_page_none() {
        let app = mock_builder().invoke_handler(|i| {
            // any command the ACL let through answers ok; the ACL runs before this
            i.resolver.resolve(());
            true
        }).build(tauri::generate_context!(test = true)).unwrap();
        for id in ["media", "browser"] {
            app.add_capability(super::capability(ORIGIN, id)).unwrap();
        }
        let page = format!("{ORIGIN}/media/index.html");
        assert_eq!(call(&app, "widget-media", &page, "widget_ready"), Ok(()));
        assert_eq!(call(&app, "widget-media", &page, "widget_state"), Ok(()));
        // nothing beyond its own commands
        assert!(call(&app, "widget-media", &page, "signin_state").unwrap_err().contains("not allowed"));
        // the same webview on any other origin (it navigated away, or a frame)
        assert!(call(&app, "widget-media", "https://www.youtube.com/embed/x", "widget_ready").unwrap_err().contains("not allowed"));
        assert!(call(&app, "widget-media", "http://127.0.0.1:61235/media/index.html", "widget_ready").unwrap_err().contains("not allowed"));
        // the browser's page webview: no capability names it, at any URL, even the widget origin
        for url in ["https://example.com/", "http://127.0.0.1:8793/page.html", &format!("{ORIGIN}/browser/index.html")] {
            for cmd in ["widget_ready", "widget_state", "signin_state"] {
                assert!(call(&app, super::PAGE_LABEL, url, cmd).unwrap_err().contains("not allowed"), "{cmd} at {url}");
            }
        }
        // its top strip may
        assert_eq!(call(&app, "widget-browser", &format!("{ORIGIN}/browser/index.html"), "widget_ready"), Ok(()));
    }

    /// Only the Personality window reaches the persona commands (the agent's pages and the bubble never).
    #[test]
    fn only_the_persona_window_gets_the_persona_commands() {
        let app = mock_builder().invoke_handler(|i| {
            i.resolver.resolve(());
            true
        }).build(tauri::generate_context!(test = true)).unwrap();
        let local = "tauri://localhost/persona/index.html";
        for cmd in ["persona_state", "persona_set", "persona_pick", "persona_reset", "persona_open_studio", "persona_close"] {
            assert_eq!(call(&app, "persona", local, cmd), Ok(()), "{cmd}");
            for other in ["bubble", "signin", "studio", "widget-media"] {
                assert!(call(&app, other, local, cmd).unwrap_err().contains("not allowed"), "{cmd} from {other}");
            }
        }
        // and the persona window gets nothing else
        for cmd in ["signin_state", "card_answer", "studio_save", "widget_ready"] {
            assert!(call(&app, "persona", local, cmd).unwrap_err().contains("not allowed"), "{cmd}");
        }
    }

    /// Every `"word"` in `text`.
    fn quoted(text: &str) -> std::collections::BTreeSet<String> {
        text.split('"').skip(1).step_by(2).map(str::to_string).collect()
    }

    /// The app manifest (build.rs), the commands the capabilities grant (bubble, signin, studio,
    /// the widgets' runtime ones) and the handler (main.rs) name the same commands: a command left
    /// out of build.rs would skip the ACL, one left out of a capability would never answer.
    #[test]
    fn build_rs_capabilities_and_the_handler_list_the_same_commands() {
        let build: std::collections::BTreeSet<String> = include_str!("../build.rs").lines().filter(|l| l.starts_with("const ")).flat_map(quoted).collect();
        let mut granted = std::collections::BTreeSet::new();
        for cap in [include_str!("../capabilities/bubble.json"), include_str!("../capabilities/signin.json"), include_str!("../capabilities/persona.json"), include_str!("../capabilities/studio.json")] {
            let v: serde_json::Value = serde_json::from_str(cap).unwrap();
            for p in v["permissions"].as_array().unwrap().iter().filter_map(|p| p.as_str()?.strip_prefix("allow-")) {
                assert!(granted.insert(p.replace('-', "_")), "{p} granted twice");
            }
        }
        granted.extend(super::COMMANDS.iter().map(|c| c.to_string()));
        let main = include_str!("main.rs");
        let lists: Vec<std::collections::BTreeSet<String>> = main
            .split("generate_handler![")
            .skip(1)
            .map(|list| list[..list.find(']').unwrap()].split(',').filter_map(|w| w.trim().rsplit("::").next()).filter(|w| !w.is_empty()).map(str::to_string).collect())
            .collect();
        assert_eq!(lists.len(), 2, "the app's list and `--studio`'s");
        assert_eq!(build, granted, "build.rs vs capabilities");
        assert_eq!(build, lists[0], "build.rs vs main.rs generate_handler!");
        let studio: std::collections::BTreeSet<String> = build.iter().filter(|c| c.starts_with("studio_")).cloned().collect();
        assert_eq!(studio.len(), 22);
        assert_eq!(studio, lists[1], "`--studio` registers exactly the studio commands");
        assert_eq!(build.len(), 50);
    }
}
