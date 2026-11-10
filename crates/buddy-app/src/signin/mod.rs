//! Sign in… (design `## accounts`, prototype `#signin`, `#signin-error`, `#signin-done`): a window
//! listing the providers of `providers.json`. A pasted API key goes straight to the keychain
//! (never to the sidecar). OAuth runs pi-ai's sign-in in the sidecar (`auth.login`); its
//! `auth.*` requests land here (`AuthUi`): the browser opens with `/usr/bin/open <url>` (one
//! argument, no shell, a clean environment), and the window shows the URL, the code field or the
//! device code.
//!
//! The window's state (`signin://state`) never holds a key, a token or a pasted code, and
//! neither do the commands' replies, errors or log lines.

pub mod window;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::Duration;

use bridge::agent::{safe_auth_url, AuthUi};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::harness::{self, Harness};

/// One provider of the sign-in list.
#[derive(Deserialize, Serialize, Clone, Debug, PartialEq)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub detail: String,
    /// `oauth`, `api_key`
    pub methods: Vec<String>,
    /// Its default model (when `settings.model` names another provider).
    pub model: String,
}

impl Provider {
    pub fn has(&self, method: &str) -> bool {
        self.methods.iter().any(|m| m == method)
    }
}

/// The curated list (checked against pi-ai by `agent/test/scenarios/auth.test.ts`), plus the
/// scripted `fake-oauth` provider when a debug build turned it on.
pub fn catalog(fake_oauth: bool) -> Vec<Provider> {
    #[allow(unused_mut)]
    let mut v: Vec<Provider> = serde_json::from_str(include_str!("providers.json")).expect("signin/providers.json");
    // compiled out of release builds (lead condition b): not even its name is in the binary
    #[cfg(debug_assertions)]
    if fake_oauth {
        v.push(Provider { id: "fake-oauth".into(), name: "Fake OAuth (test)".into(), detail: "scripted sign-in: the code is “good”".into(), methods: vec!["oauth".into()], model: "fake-1".into() });
    }
    #[cfg(not(debug_assertions))]
    let _ = fake_oauth;
    v
}

/// What the window shows of an OAuth sign-in in progress.
#[derive(Clone, Debug, Default, PartialEq)]
enum Phase {
    #[default]
    Starting,
    /// The browser opened at `url`.
    Browser,
    /// Waiting for the pasted code.
    Code,
    Device,
    Select,
    Error,
}

#[derive(Clone, Debug, Default)]
struct Flow {
    provider: String,
    phase: Phase,
    url: Option<String>,
    instructions: Option<String>,
    message: Option<String>,
    placeholder: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    options: Vec<(String, String)>,
    progress: Option<String>,
    error: Option<String>,
}

#[derive(Default)]
struct Inner {
    flow: Option<Flow>,
    /// The pending prompt's answer, once given.
    answer: Option<Result<String, String>>,
    /// A prompt or select is waiting for its answer.
    waiting: bool,
    /// What this sign-in was given (codes), only to scrub them from error messages.
    given: Vec<String>,
    /// The last outcome line: `(ok, text)`.
    notice: Option<(bool, String)>,
}

type Emit = Box<dyn Fn(&Value) + Send + Sync>;
type Open = Box<dyn Fn(&str) + Send + Sync>;

pub struct SignIn {
    harness: Arc<Harness>,
    inner: Mutex<Inner>,
    answered: Condvar,
    emit: Emit,
    open: Open,
}

static SIGNIN: OnceLock<Arc<SignIn>> = OnceLock::new();

/// The app's sign-in, once `install` ran.
pub fn get() -> Option<Arc<SignIn>> {
    SIGNIN.get().cloned()
}

/// Wire the window to the harness: the sidecar's `auth.*` requests come here.
pub fn install(harness: Arc<Harness>, emit: Emit) -> Arc<SignIn> {
    let s = SignIn::new(harness, emit, Box::new(open_in_browser));
    let _ = SIGNIN.set(s.clone());
    s.harness.agent().set_auth_ui(s.clone());
    s
}

/// `/usr/bin/open <url>`: one argument, no shell, nothing of the app's environment.
fn open_in_browser(url: &str) {
    let r = std::process::Command::new("/usr/bin/open")
        .arg(url)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    match r {
        Ok(mut child) => {
            std::thread::spawn(move || child.wait());
        }
        Err(e) => eprintln!("[buddy] signin: couldn't open the browser: {e}"),
    }
}

/// `msg` with every given code replaced, so an error that echoes one never shows it: the whole
/// value and each part of it (pi-ai splits `code#state` and callback URLs before sending them).
fn scrub(msg: &str, given: &[String]) -> String {
    let mut parts: Vec<&str> = given.iter().map(String::as_str).filter(|g| g.len() >= 3).collect();
    for g in given {
        // code#state and a callback's query values are secret as they are; inside a part with a
        // path or a label (`Code: …`), only the token-like words (8+ chars with a digit or mixed
        // case), so the plain words of an error stay readable
        for p in g.split(['#', '&', '=', '?']) {
            if p.contains(['/', ' ']) {
                parts.extend(p.split(['/', ' ']).filter(|w| w.len() >= 8 && token_like(w)));
            } else if p.len() >= 6 {
                parts.push(p);
            }
        }
    }
    // longest first, so a part never leaves the rest of a longer match behind
    parts.sort_by_key(|p| std::cmp::Reverse(p.len()));
    let mut m = msg.to_string();
    for p in parts {
        m = m.replace(p, "…");
    }
    m
}

fn token_like(p: &str) -> bool {
    p.bytes().any(|b| b.is_ascii_digit()) || (p.bytes().any(|b| b.is_ascii_uppercase()) && p.bytes().any(|b| b.is_ascii_lowercase()))
}

/// A sign-in error as the window says it: one line, short, without the response body or the
/// stack pi-ai can append (`body=…`, `details=…`), which may hold parts of a token response.
fn failure_text(message: &str) -> String {
    if message.contains("invalid_grant") {
        return "That code didn't work (invalid_grant). Start again to get a fresh one.".into();
    }
    let mut m = message.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("").to_string();
    for cut in ["body=", "details=", "response missing fields:"] {
        if let Some(i) = m.find(cut) {
            m.truncate(i);
        }
    }
    let mut m: String = m.trim_end_matches([';', ' ', ':']).to_string();
    if m.is_empty() {
        m = "no reason given".into();
    }
    if m.chars().count() > 160 {
        m = m.chars().take(160).collect::<String>() + "…";
    }
    format!("The sign-in didn't work: {m}")
}

impl SignIn {
    pub fn new(harness: Arc<Harness>, emit: Emit, open: Open) -> Arc<SignIn> {
        Arc::new(SignIn { harness, inner: Mutex::default(), answered: Condvar::new(), emit, open })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The window's whole state: providers and their status, the sign-in in progress, the
    /// keychain error, the last outcome. Never a secret.
    pub fn state(&self) -> Value {
        let h = &self.harness;
        let creds = h.creds();
        let kinds: std::collections::HashMap<String, String> = creds.kinds().into_iter().collect();
        let current = h.model();
        let providers: Vec<Value> = h
            .catalog()
            .iter()
            .map(|p| {
                let status = match kinds.get(&p.id).map(String::as_str) {
                    _ if creds.is_session(&p.id) => "session",
                    Some("oauth") => "signed_in",
                    Some("api_key") => "key_saved",
                    _ => "none",
                };
                let model = current.as_ref().filter(|m| m.provider == p.id).map(|m| m.model.clone());
                json!({ "id": p.id, "name": p.name, "detail": p.detail, "methods": p.methods, "status": status, "model": model })
            })
            .collect();
        let g = self.lock();
        let flow = g.flow.as_ref().map(|f| {
            let name = h.catalog().iter().find(|p| p.id == f.provider).map_or(f.provider.clone(), |p| p.name.clone());
            json!({
                "providerId": f.provider,
                "name": name,
                "phase": match f.phase { Phase::Starting => "starting", Phase::Browser => "browser", Phase::Code => "code", Phase::Device => "device", Phase::Select => "select", Phase::Error => "error" },
                "url": f.url,
                "instructions": f.instructions,
                "message": f.message,
                "placeholder": f.placeholder,
                "userCode": f.user_code,
                "verificationUri": f.verification_uri,
                "options": f.options.iter().map(|(id, label)| json!({ "id": id, "label": label })).collect::<Vec<_>>(),
                "progress": f.progress,
                "error": f.error,
            })
        });
        let keychain = match h.state() {
            harness::AgentState::Keychain(why) => Some(why),
            _ => None,
        };
        json!({
            "providers": providers,
            "flow": flow,
            "keychain": keychain,
            "canOAuth": h.can_oauth(),
            "notice": g.notice.as_ref().map(|(ok, text)| json!({ "ok": ok, "text": text })),
        })
    }

    fn changed(&self) {
        (self.emit)(&self.state());
    }

    fn notice(&self, ok: bool, text: String) {
        eprintln!("[buddy] signin: {text}");
        self.lock().notice = Some((ok, text));
        self.changed();
    }

    fn provider(&self, id: &str) -> Option<Provider> {
        self.harness.catalog().iter().find(|p| p.id == id).cloned()
    }

    /// A pasted key → the keychain. Blocks on the keychain (call off the main thread).
    pub fn save_key(&self, provider_id: &str, key: &str) {
        let Some(p) = self.provider(provider_id).filter(|p| p.has("api_key")) else {
            return self.notice(false, format!("{provider_id} takes no API key here."));
        };
        match self.harness.save_api_key(&p.id, key) {
            Ok(()) => self.notice(true, format!("{}: key saved in your keychain.", p.name)),
            Err(e) => self.notice(false, e),
        }
    }

    /// Start an OAuth sign-in and wait for it to end (call off the main thread).
    pub fn oauth(&self, provider_id: &str) {
        let Some(p) = self.provider(provider_id).filter(|p| p.has("oauth")) else {
            return self.notice(false, format!("{provider_id} has no browser sign-in here."));
        };
        {
            let mut g = self.lock();
            if g.flow.as_ref().is_some_and(|f| f.phase != Phase::Error) {
                drop(g);
                return self.notice(false, "Another sign-in is in progress; cancel it first.".into());
            }
            *g = Inner { flow: Some(Flow { provider: p.id.clone(), ..Flow::default() }), ..Inner::default() };
        }
        self.changed();
        eprintln!("[buddy] signin: {} (OAuth) started", p.id);
        let r = self.harness.oauth_login(&p.id);
        let mut g = self.lock();
        let given = std::mem::take(&mut g.given);
        g.waiting = false;
        g.answer = None;
        match r {
            Ok(()) => {
                g.flow = None;
                drop(g);
                self.notice(true, format!("Signed in to {}.", p.name));
            }
            Err(e) if e.code == "cancelled" => {
                g.flow = None;
                drop(g);
                eprintln!("[buddy] signin: {} cancelled", p.id);
                self.changed();
            }
            Err(e) => {
                let text = failure_text(&scrub(&e.message, &given));
                eprintln!("[buddy] signin: {} failed: {text}", p.id);
                if let Some(f) = g.flow.as_mut() {
                    f.phase = Phase::Error;
                    f.error = Some(text);
                }
                drop(g);
                self.changed();
            }
        }
    }

    /// The pasted code (or an option's id) for the waiting prompt.
    pub fn answer(&self, value: String) {
        let mut g = self.lock();
        if !g.waiting {
            return;
        }
        g.given.push(value.trim().to_string());
        g.answer = Some(Ok(value.trim().to_string()));
        self.answered.notify_all();
    }

    /// Cancel / closing the window: the waiting prompt fails, and the sidecar ends the sign-in.
    pub fn cancel(&self) {
        let active = {
            let mut g = self.lock();
            let active = g.flow.is_some();
            if g.waiting {
                g.answer = Some(Err("cancelled".into()));
                self.answered.notify_all();
            }
            if g.flow.as_ref().is_some_and(|f| f.phase == Phase::Error) {
                g.flow = None;
            }
            active
        };
        if active {
            self.harness.cancel_login();
        }
        self.changed();
    }

    /// Sign out / Remove (call off the main thread).
    pub fn remove(&self, provider_id: &str) {
        let Some(p) = self.provider(provider_id) else { return };
        match self.harness.sign_out(&p.id) {
            Ok(()) => self.notice(true, format!("{}: signed out, keychain item removed.", p.name)),
            Err(e) => self.notice(false, e),
        }
    }

    /// Open the sign-in page again (or the device-code page): only what the sidecar sent and
    /// `safe_auth_url` passed, never a URL from the page.
    pub fn reopen(&self) {
        let url = self.lock().flow.as_ref().and_then(|f| f.url.clone().or_else(|| f.verification_uri.clone()));
        if let Some(u) = url.filter(|u| safe_auth_url(u)) {
            (self.open)(&u);
        }
    }

    fn update(&self, f: impl FnOnce(&mut Flow)) {
        if let Some(flow) = self.lock().flow.as_mut() {
            f(flow);
        }
        self.changed();
    }

    /// Ask the window a question: show it (`f`) and start waiting under one lock, so an early
    /// answer isn't dropped and a late `open_url` / `device_code` can't hide the question.
    fn ask(&self, f: impl FnOnce(&mut Flow), cancelled: &AtomicBool) -> Result<String, String> {
        {
            let mut g = self.lock();
            g.waiting = true;
            g.answer = None;
            if let Some(flow) = g.flow.as_mut() {
                f(flow);
            }
        }
        self.changed();
        self.wait_answer(cancelled)
    }

    /// Wait for `answer` or `cancel` (or the bridge's own cancel); `ask` started the wait.
    fn wait_answer(&self, cancelled: &AtomicBool) -> Result<String, String> {
        let mut g = self.lock();
        loop {
            if let Some(a) = g.answer.take() {
                g.waiting = false;
                return a;
            }
            if cancelled.load(Ordering::SeqCst) || g.flow.is_none() {
                g.waiting = false;
                return Err("cancelled".into());
            }
            g = self.answered.wait_timeout(g, Duration::from_millis(200)).unwrap_or_else(|e| e.into_inner()).0;
        }
    }
}

impl AuthUi for SignIn {
    fn open_url(&self, url: &str, instructions: Option<&str>) {
        // the bridge checked it; checked again here, as this is what runs `open`
        if !safe_auth_url(url) {
            return;
        }
        eprintln!("[buddy] signin: opening the sign-in page");
        (self.open)(url);
        // the bridge runs `auth.prompt` on its own thread: a code prompt sent right after this
        // may already be waiting, and its field must stay on screen
        let waiting = self.lock().waiting;
        self.update(|f| {
            if !waiting {
                f.phase = Phase::Browser;
            }
            f.url = Some(url.to_string());
            f.instructions = instructions.map(str::to_string);
        });
    }

    fn device_code(&self, user_code: &str, verification_uri: &str) {
        let uri = Some(verification_uri.to_string()).filter(|u| safe_auth_url(u));
        let waiting = self.lock().waiting;
        self.update(|f| {
            if !waiting {
                f.phase = Phase::Device;
            }
            f.user_code = Some(user_code.to_string());
            f.verification_uri = uri;
        });
    }

    fn progress(&self, message: &str) {
        self.update(|f| f.progress = Some(message.to_string()));
    }

    fn prompt(&self, message: &str, placeholder: Option<&str>, cancelled: &AtomicBool) -> Result<String, String> {
        self.ask(
            |f| {
                f.phase = Phase::Code;
                f.message = Some(message.to_string());
                f.placeholder = placeholder.map(str::to_string);
            },
            cancelled,
        )
    }

    fn select(&self, message: &str, options: &[(String, String)], cancelled: &AtomicBool) -> Result<String, String> {
        let id = self.ask(
            |f| {
                f.phase = Phase::Select;
                f.message = Some(message.to_string());
                f.options = options.to_vec();
            },
            cancelled,
        )?;
        if options.iter().any(|(o, _)| *o == id) {
            Ok(id)
        } else {
            Err("cancelled".into())
        }
    }
}

// --- the page's commands (they never return or log a secret) -------------------------------

fn with(f: impl FnOnce(Arc<SignIn>) + Send + 'static) {
    if let Some(s) = get() {
        // the keychain and the sidecar can block: never on the main thread
        std::thread::spawn(move || f(s));
    }
}

/// The Sign in… window only (`caller`): the bubble page never reaches a key or a sign-in.
fn mine(w: &tauri::WebviewWindow, cmd: &str) -> bool {
    crate::caller::is(w.label(), window::LABEL, cmd)
}

#[tauri::command]
pub fn signin_state(w: tauri::WebviewWindow) -> Value {
    if !mine(&w, "signin_state") {
        return Value::Null;
    }
    get().map_or(Value::Null, |s| s.state())
}

#[tauri::command]
pub fn signin_save_key(w: tauri::WebviewWindow, provider_id: String, key: String) {
    if mine(&w, "signin_save_key") {
        with(move |s| s.save_key(&provider_id, &key));
    }
}

#[tauri::command]
pub fn signin_start(w: tauri::WebviewWindow, provider_id: String) {
    if mine(&w, "signin_start") {
        with(move |s| s.oauth(&provider_id));
    }
}

#[tauri::command]
pub fn signin_answer(w: tauri::WebviewWindow, value: String) {
    if mine(&w, "signin_answer") {
        with(move |s| s.answer(value));
    }
}

#[tauri::command]
pub fn signin_cancel(w: tauri::WebviewWindow) {
    if mine(&w, "signin_cancel") {
        with(|s| s.cancel());
    }
}

#[tauri::command]
pub fn signin_remove(w: tauri::WebviewWindow, provider_id: String) {
    if mine(&w, "signin_remove") {
        with(move |s| s.remove(&provider_id));
    }
}

#[tauri::command]
pub fn signin_reopen(w: tauri::WebviewWindow) {
    if mine(&w, "signin_reopen") {
        with(|s| s.reopen());
    }
}

#[tauri::command]
pub fn signin_close(w: tauri::WebviewWindow) {
    if mine(&w, "signin_close") {
        window::hide();
        with(|s| s.cancel());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::Config;
    use bridge::store::MemoryStore;

    type Seen = Arc<Mutex<Vec<String>>>;

    fn fixture(name: &str) -> (Arc<SignIn>, Seen, Seen, std::path::PathBuf) {
        let home = std::env::temp_dir().join(format!("buddy-signin-{name}-{}-{}", std::process::id(), host::random_hex(3)));
        let wanted = crate::settings::Model { provider: "anthropic".into(), model: "claude-sonnet-5-5".into(), ..Default::default() };
        let h = Harness::build(&home, &wanted, Config::memory(MemoryStore::new()), None).unwrap();
        let states: Seen = Arc::default();
        let opened: Seen = Arc::default();
        let (s2, o2) = (states.clone(), opened.clone());
        let s = SignIn::new(h, Box::new(move |v| s2.lock().unwrap().push(v.to_string())), Box::new(move |u| o2.lock().unwrap().push(u.to_string())));
        (s, states, opened, home)
    }

    #[test]
    fn the_list_is_pi_ais_and_the_fake_is_debug_only() {
        let c = catalog(false);
        assert_eq!(c[0].id, "anthropic");
        assert!(c.iter().all(|p| !p.methods.is_empty() && !p.model.is_empty()));
        assert!(!c.iter().any(|p| p.id == "fake-oauth"));
        assert_eq!(catalog(true).iter().any(|p| p.id == "fake-oauth"), cfg!(debug_assertions));
    }

    #[test]
    fn a_pasted_key_is_saved_and_never_in_the_state_or_a_notice() {
        let (s, states, _, home) = fixture("key");
        let key = "sk-or-v1-PASTED-c0ffee-123";
        s.save_key("openrouter", key);
        let last: Value = serde_json::from_str(states.lock().unwrap().last().unwrap()).unwrap();
        let row = last["providers"].as_array().unwrap().iter().find(|p| p["id"] == "openrouter").unwrap();
        assert_eq!(row["status"], "key_saved");
        assert_eq!(last["notice"]["ok"], true);
        // a provider with no API-key method, and an empty key: refused, the key never echoed
        s.save_key("github-copilot", key);
        s.save_key("mistral", "  ");
        let all = states.lock().unwrap().join("\n");
        assert!(!all.contains(key) && !all.contains("PASTED"), "{all}");
        assert!(all.contains("Paste a key first."));
        assert_eq!(s.harness.creds().kinds(), vec![("openrouter".to_string(), "api_key".to_string())]);
        s.remove("openrouter");
        assert!(!s.harness.creds().has("openrouter"));
        s.harness.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn a_code_prompt_that_overtakes_its_open_url_keeps_the_code_field() {
        // the bridge runs `auth.prompt` on its own thread, `auth.open_url` on the pool: either may land first
        let (s, states, opened, home) = fixture("reorder");
        s.lock().flow = Some(Flow { provider: "anthropic".into(), ..Flow::default() });
        let s2 = s.clone();
        let t = std::thread::spawn(move || s2.prompt("Paste the code", None, &AtomicBool::new(false)));
        while !s.lock().waiting {
            std::thread::sleep(Duration::from_millis(5));
        }
        s.open_url("https://claude.ai/oauth/authorize?code=true", Some("Paste the code"));
        assert_eq!(*opened.lock().unwrap(), vec!["https://claude.ai/oauth/authorize?code=true"]);
        let st: Value = serde_json::from_str(states.lock().unwrap().last().unwrap()).unwrap();
        assert_eq!(st["flow"]["phase"], "code", "the late open_url must not hide the code field");
        assert_eq!(st["flow"]["url"], "https://claude.ai/oauth/authorize?code=true");
        s.device_code("ABCD-EFGH", "https://github.com/login/device");
        let st: Value = serde_json::from_str(states.lock().unwrap().last().unwrap()).unwrap();
        assert_eq!(st["flow"]["phase"], "code");
        s.answer("abc".into());
        assert_eq!(t.join().unwrap(), Ok("abc".into()));
        // once answered, the next page opens as usual
        s.open_url("https://claude.ai/next", None);
        let st: Value = serde_json::from_str(states.lock().unwrap().last().unwrap()).unwrap();
        assert_eq!(st["flow"]["phase"], "browser");
        s.harness.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn the_browser_opens_only_safe_pages_and_a_code_prompt_waits_for_its_answer() {
        let (s, states, opened, home) = fixture("oauth");
        s.lock().flow = Some(Flow { provider: "anthropic".into(), ..Flow::default() });
        s.open_url("https://claude.ai/oauth/authorize?code=true", Some("Paste the code"));
        s.open_url("file:///etc/passwd", None);
        assert_eq!(*opened.lock().unwrap(), vec!["https://claude.ai/oauth/authorize?code=true"]);
        let st: Value = serde_json::from_str(states.lock().unwrap().last().unwrap()).unwrap();
        assert_eq!(st["flow"]["phase"], "browser");
        assert_eq!(st["flow"]["url"], "https://claude.ai/oauth/authorize?code=true", "the URL is shown as text too");
        // the prompt blocks until the window answers; the code never reaches the state
        let s2 = s.clone();
        let t = std::thread::spawn(move || s2.prompt("Paste the code", Some("code#state"), &AtomicBool::new(false)));
        while !s.lock().waiting {
            std::thread::sleep(Duration::from_millis(5));
        }
        s.answer(" abc123#secretstate ".into());
        assert_eq!(t.join().unwrap(), Ok("abc123#secretstate".into()));
        assert!(!states.lock().unwrap().join("\n").contains("secretstate"));
        // an error that echoes the code shows it scrubbed
        assert_eq!(failure_text(&scrub("invalid_grant for abc123#secretstate", &s.lock().given)), "That code didn't work (invalid_grant). Start again to get a fresh one.");
        assert_eq!(scrub("bad code abc123#secretstate", &s.lock().given), "bad code …");
        // a part of what was pasted (pi-ai sends only the code of `code#state`)
        assert_eq!(scrub("invalid code secretstate", &["abc123#secretstate".into()]), "invalid code …");
        assert_eq!(scrub("code abcdef123 rejected", &["https://x.test/cb?code=abcdef123&state=zz".into()]), "code … rejected");
        // the window gets one short line, never a response body or a stack
        assert_eq!(failure_text("HTTP request failed. status=500; url=https://x.test/token; body={\"access_token\":\"AT-1\"}\n at f()"), "The sign-in didn't work: HTTP request failed. status=500; url=https://x.test/token");
        assert_eq!(failure_text("token exchange failed: response missing fields: {\"access_token\":\"AT-2\"}"), "The sign-in didn't work: token exchange failed");
        assert!(failure_text(&"x".repeat(400)).chars().count() < 200);
        // a body with nothing before it, a leading blank line, nothing at all
        assert_eq!(failure_text("body={\"access_token\":\"AT-3\"}"), "The sign-in didn't work: no reason given");
        assert_eq!(failure_text("\n  token request failed\n at f()"), "The sign-in didn't work: token request failed");
        assert_eq!(failure_text("bad; details={\"refresh_token\":\"RT\"}"), "The sign-in didn't work: bad");
        // words of a pasted sentence aren't blanked in the error, the code part is
        assert_eq!(scrub("the code expired", &["https://x.test/auth/callback?code=AbCdEf12345".into()]), "the code expired");
        assert_eq!(scrub("rejected AbCdEf12345", &["https://x.test/auth/callback?code=AbCdEf12345".into()]), "rejected …");
        // a labelled paste and a code in a path segment
        assert_eq!(scrub("invalid code AbCdEf123456", &["Code: AbCdEf123456#StAtE9876".into()]), "invalid code …");
        assert_eq!(scrub("no such code Zq8w7e6r5t", &["https://x.test/cb/Zq8w7e6r5t".into()]), "no such code …");
        // a cancelled prompt
        let s3 = s.clone();
        let t = std::thread::spawn(move || s3.prompt("Paste the code", None, &AtomicBool::new(false)));
        while !s.lock().waiting {
            std::thread::sleep(Duration::from_millis(5));
        }
        s.cancel();
        assert_eq!(t.join().unwrap(), Err("cancelled".into()));
        // the bridge's own cancel flag
        assert_eq!(s.prompt("x", None, &AtomicBool::new(true)), Err("cancelled".into()));
        s.harness.shutdown();
        let _ = std::fs::remove_dir_all(&home);
    }
}
