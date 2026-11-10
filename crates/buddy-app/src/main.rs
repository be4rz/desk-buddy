//! desk-buddy: one Tauri 2 process, a menu-bar accessory (no Dock icon). The bear is a
//! native NSPanel (`panel.rs`, from spike S1) driven by `mascot_core::Brain` on the main
//! thread (`driver.rs`), drawn by `mascot-render`.
//!
//! Env: `BUDDY_HOME` (default `~/.desk-buddy`), `BUDDY_PACK=<dir>` (a pack dir; else
//! settings `pack` under `$BUDDY_PACKS`, the repo's `mascots/` in debug builds, then
//! `BUDDY_HOME/packs`), `BUDDY_ZOOM=1|2|3`, `BUDDY_STATS=1`, `BUDDY_DUMP=<dir>`
//! (+ `BUDDY_DUMP_EVERY=<n>`), `BUDDY_EVENTS=<file.jsonl>`, `BUDDY_SCREEN=<n>`,
//! `BUDDY_VERBOSE=1`, `BUDDY_SENSES=0` (sensors off; also off under `BUDDY_EVENTS` unless
//! `BUDDY_SENSES=1`), `BUDDY_SENSES_LOG=1` (sense changes to stderr).
//!
//! Talk (p3): the speech bubble (`bubble/`, page `web/bubble/`) and the agent harness
//! (`harness.rs`: host API at `BUDDY_HOME/host.json`, policy gate, one agent). Debug builds
//! only: the dev credential `BUDDY_FAKE_MODEL=<url>`, `BUDDY_PROMPT=<text>` (one prompt once the agent
//! is ready), `BUDDY_ANSWER=allow|deny`.
//!
//! Accounts (p4): keys live in the macOS login keychain (service `desk-buddy`) and in memory; the
//! tray's Sign in… opens `signin/` (page `web/signin/`). Sense-triggered model turns go through
//! `unprompted.rs` (4 an hour, none in DND or quiet hours). One app per `BUDDY_HOME`
//! (`instance.rs`): a second launch logs `[buddy] already running (pid N)` and exits 0. Debug
//! builds only: `ANTHROPIC_API_KEY` (a session key, never stored), `BUDDY_CRED_STORE=memory|fail`
//! and `BUDDY_FAKE_OAUTH=1` (a scripted `fake-oauth` provider; see `harness.rs`).

mod bubble;
mod caller;
mod driver;
mod events;
mod harness;
mod instance;
mod media_os;
mod panel;
mod senses;
mod settings;
mod signin;
mod stats;
mod tray;
mod unprompted;
mod widgets;

use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use mascot_core::{Brain, BrainConfig, PackMeta};
use objc2::MainThreadMarker;
use objc2_foundation::{NSRunLoop, NSRunLoopCommonModes, NSTimer};
use tauri::RunEvent;

/// Candidate pack dirs for a pack id, in search order (design §3.2).
fn pack_candidates(id: &str, home: &Path) -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("BUDDY_PACKS") {
        v.push(PathBuf::from(p).join(id));
    }
    if cfg!(debug_assertions) {
        v.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots").join(id));
    }
    v.push(home.join("packs").join(id));
    v
}

fn pack_dir(id: &str, home: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("BUDDY_PACK") {
        return PathBuf::from(p);
    }
    let c = pack_candidates(id, home);
    c.iter().find(|d| d.join("pack.json").is_file()).cloned().unwrap_or_else(|| c[0].clone())
}

fn fail(what: &str, e: impl std::fmt::Display) -> ! {
    eprintln!("[buddy] cannot load {what}: {e}");
    std::process::exit(1);
}

fn seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9e37_79b9_7f4a_7c15)
        | 1
}

fn main() {
    let home = settings::home();
    // held (open) until the process exits
    let _app_lock = match instance::claim(&home) {
        Ok(lock) => lock,
        Err(instance::Running(pid)) => {
            eprintln!("[buddy] already running (pid {})", pid.map_or("?".into(), |p| p.to_string()));
            std::process::exit(0);
        }
    };
    let (settings, note) = settings::load(&home);
    let warning = note.map(|settings::LoadNote::Reset { bad, why }| {
        eprintln!("[buddy] settings reset to defaults: {why} (kept {})", bad.display());
        "Settings reset (see settings.json.bad)".to_string()
    });
    // a session override: shown, never written back to settings.json (the tray's Zoom is)
    let zoom = std::env::var("BUDDY_ZOOM")
        .ok()
        .and_then(|z| z.parse::<u32>().ok())
        .map_or(settings.zoom, |z| z.clamp(1, 3));

    let dir = pack_dir(&settings.pack, &home);
    let pack: Arc<PackMeta> = match mascot_core::pack::load(&dir) {
        Ok(p) => Arc::new(p),
        Err(e) => fail("mascot pack", e),
    };
    let art = match mascot_render::Art::load(&pack) {
        Ok(a) => a,
        Err(e) => fail("mascot art", e),
    };
    let items = match std::env::var_os("BUDDY_EVENTS") {
        Some(f) => match events::load(Path::new(&f)) {
            Ok(v) => Some(v),
            Err(e) => fail("events", e),
        },
        None => None,
    };
    let (cw, ch) = mascot_render::canvas_size(&pack);
    eprintln!(
        "[buddy] pack {}: {} behaviors ({} available), canvas {cw}x{ch} sprite px, from {}",
        pack.id,
        pack.behaviors.len(),
        pack.behaviors.keys().filter(|b| pack.available(b)).count(),
        dir.display()
    );
    let icon = tray::icon(&art);
    let t0 = Instant::now();
    let dnd_action = driver::dnd_action(settings.dnd.action);
    let sensors = match std::env::var("BUDDY_SENSES").ok().as_deref() {
        Some("0") => false,
        Some(_) => true,
        None => items.is_none(), // a replay drives the senses itself
    };
    let brain = Brain::new(pack.clone(), BrainConfig { seed: seed(), temperament: None, dnd_action }, 0);

    let mut boot = Some((pack, art, settings, home, brain, items, warning, icon));
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            bubble::window::prompt_submit,
            bubble::window::card_answer,
            bubble::window::bubble_dismiss,
            bubble::window::bubble_size,
            bubble::window::bubble_retry,
            bubble::window::reminder_snooze,
            signin::signin_state,
            signin::signin_save_key,
            signin::signin_start,
            signin::signin_answer,
            signin::signin_cancel,
            signin::signin_remove,
            signin::signin_reopen,
            signin::signin_close,
            widgets::widget_ready,
            widgets::widget_reply,
            widgets::widget_state,
            widgets::widget_visibility,
            widgets::widget_control,
            widgets::widget_drag,
        ])
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let (pack, art, settings, home, brain, items, warning, icon) = boot.take().unwrap();
            let mtm = MainThreadMarker::new().expect("setup runs on the main thread");
            let (w, h) = mascot_render::canvas_size(&pack);
            let panel = panel::Panel::new(mtm, w, h, zoom, settings.position.as_ref());
            let q = &settings.quiet_hours;
            let tray_init = tray::TrayInit {
                pack_id: pack.id.clone(),
                zoom,
                warning,
                icon,
                dnd_action: settings.dnd.action,
                quiet_enabled: q.enabled,
                quiet_span: format!("{}–{}", q.start, q.end),
            };
            let wanted = settings.model.clone();
            let home2 = home.clone();
            driver::start(driver::Init { pack, art, settings, home, panel, brain, t0 }, app.handle().clone());
            let win = bubble::window::BubbleWin::new(app, mtm)?;
            driver::with_app(|a| a.bubble_win = Some(win));
            tray::build(app, tray_init)?;
            signin::window::init(app.handle().clone());
            let mut host_port = None;
            match harness::Harness::start(&home2, &wanted) {
                Ok(h) => {
                    host_port = h.host_port();
                    let view = h.policy_view();
                    driver::with_app(|a| a.policy_changed(&view));
                    signin::install(h, Box::new(signin::window::emit));
                }
                Err(e) => {
                    // R-start-err: the tray says so (it would read "starting…" for ever)
                    eprintln!("[buddy] cannot start the host API: {e}");
                    let s = harness::AgentState::Error(format!("host API: {e}"));
                    driver::with_app(|a| a.agent_changed(&s));
                }
            }
            if let Err(e) = widgets::start(app.handle(), host_port) {
                eprintln!("[buddy] no widgets: {e}");
            }
            if sensors {
                senses::start();
                widgets::watch_now_playing();
            }
            if std::env::var_os("BUDDY_STATS").is_some() {
                start_stats();
            }
            quit_on_signals(app.handle().clone());
            if let Some(items) = items {
                events::replay(items, t0);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build tauri app");

    app.run(|_app, ev| match ev {
        // no window closing quits the app; only Quit does
        RunEvent::ExitRequested { code: None, api, .. } => api.prevent_exit(),
        // Quit: stop the sidecar and remove host.json (also after a sidecar crash)
        RunEvent::Exit => {
            if let Some(h) = harness::get() {
                h.shutdown();
                eprintln!("[buddy] quit: agent stopped, {} removed", h.host_file().display());
            }
        }
        _ => {}
    });
}

static SIGNAL_FD: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(-1);

extern "C" fn on_signal(_: libc::c_int) {
    let fd = SIGNAL_FD.load(std::sync::atomic::Ordering::Relaxed);
    if fd >= 0 {
        // async-signal-safe: one byte down a pipe; a thread does the quitting
        unsafe { libc::write(fd, b"q".as_ptr().cast(), 1) };
    }
}

/// SIGTERM / SIGINT / SIGHUP quit like the tray's Quit (the sidecar stops, host.json goes).
/// Handlers reset to the default in children at exec, so the sidecar is unaffected.
fn quit_on_signals(handle: tauri::AppHandle) {
    let mut fds = [0 as libc::c_int; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return;
    }
    SIGNAL_FD.store(fds[1], std::sync::atomic::Ordering::Relaxed);
    for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
        unsafe { libc::signal(sig, on_signal as *const () as libc::sighandler_t) };
    }
    let rd = fds[0];
    let _ = std::thread::Builder::new().name("buddy-signals".into()).spawn(move || {
        let mut b = [0u8; 1];
        while unsafe { libc::read(rd, b.as_mut_ptr().cast(), 1) } != 1 {}
        eprintln!("[buddy] signal: quitting");
        handle.exit(0);
    });
}

/// `BUDDY_STATS=1`: one line every 10 s, also while the bear is hidden (0 fps).
fn start_stats() {
    let block = RcBlock::new(|_t: NonNull<NSTimer>| {
        driver::with_app(|a| {
            let polls = a.panel.take_polls();
            eprintln!("{} poll {:.1} Hz", a.stats.line(), polls as f64 / stats::PERIOD.as_secs_f64());
        })
    });
    unsafe {
        let t = NSTimer::timerWithTimeInterval_repeats_block(stats::PERIOD.as_secs_f64(), true, &block);
        NSRunLoop::mainRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes);
    }
}
