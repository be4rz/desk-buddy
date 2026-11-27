//! desk-buddy: one Tauri 2 process, a menu-bar accessory (no Dock icon). The bear is a
//! native NSPanel (`panel.rs`, from spike S1) driven by `mascot_core::Brain` on the main
//! thread (`driver.rs`), drawn by `mascot-render`.
//!
//! Env: `BUDDY_HOME` (default `~/.desk-buddy`), `BUDDY_PACK=<dir>` (a pack dir; else
//! settings `pack` under `BUDDY_HOME/packs` (the studio's library), `$BUDDY_PACKS`, then the
//! repo's `mascots/` in debug builds; a settings pack found nowhere runs the bear), `BUDDY_ZOOM=1|2|3`, `BUDDY_STATS=1`, `BUDDY_DUMP=<dir>`
//! (+ `BUDDY_DUMP_EVERY=<n>`), `BUDDY_EVENTS=<file.jsonl>`, `BUDDY_SCREEN=<n>`,
//! `BUDDY_VERBOSE=1`, `BUDDY_SENSES=0` (sensors off; also off under `BUDDY_EVENTS` unless
//! `BUDDY_SENSES=1`), `BUDDY_SENSES_LOG=1` (sense changes to stderr, activity moments as kinds only).
//! Debug builds only: `BUDDY_ACTIVITY_SCALE=<n>` divides the activity thresholds by `n`.
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
//!
//! Personality: the tray's Personality ▸ picks a preset of the pack's persona, Customize… opens
//! `persona/` (page `web/persona/`); `settings.persona` is the person's layer (v3).
//!
//! Memory: `memory/` owns the store at `BUDDY_HOME/memory` (`settings.memory`, v5; `BUDDY_MEMORY=0|1`),
//! the tray's What I remember… (or `buddy memory open`) opens its window (page `web/memory/`).
//!
//! Studio (p6): the tray's Studio… opens `studio/` (page `web/studio/`) on the running pack.
//! `buddy-app --studio [<pack-dir>]` runs the studio alone: no bear, agent, lock or host.json,
//! so it can run next to the app on the same `BUDDY_HOME`; it writes only the mascot library
//! (`BUDDY_HOME/packs`) and, on "Use on my desktop", settings `pack`; closing its window quits.

mod bubble;
mod caller;
mod driver;
mod events;
mod harness;
mod idle_lines;
mod instance;
mod media_os;
mod memory;
mod panel;
mod persona;
mod screen;
mod senses;
mod settings;
mod signin;
mod stats;
mod studio;
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

/// The folders built-in packs live in: `BUDDY_PACKS`, the repo's `mascots/` (debug builds).
fn builtin_roots() -> Vec<PathBuf> {
    let mut v = vec![];
    if let Some(p) = std::env::var_os("BUDDY_PACKS").filter(|p| !p.is_empty()) {
        v.push(PathBuf::from(p));
    }
    if cfg!(debug_assertions) {
        v.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots"));
    }
    v
}

/// The folders packs live in, in search order: the library `BUDDY_HOME/packs` first (a
/// customized built-in there shadows the shipped one), then the built-in roots.
fn pack_roots(home: &Path) -> Vec<PathBuf> {
    let mut v = vec![home.join("packs")];
    v.extend(builtin_roots());
    v
}

/// The studio's view of the same folders.
fn studio_roots(home: &Path) -> studio::Roots {
    studio::Roots { library: Some(home.join("packs")), builtin: builtin_roots() }
}

/// Candidate pack dirs for a pack id, in search order.
fn pack_candidates(id: &str, home: &Path) -> Vec<PathBuf> {
    pack_roots(home).into_iter().map(|r| r.join(id)).collect()
}

/// The pack to run: `BUDDY_PACK` (a broken one stops the launch), else settings `pack` in
/// `pack_roots`; a settings pack that is nowhere (deleted in the studio, or a bad id) runs the
/// bear and says so. A library pack that doesn't load (edited by hand) is skipped the same way,
/// so a broken customized bear falls back to the shipped one.
fn pack_dir(id: &str, home: &Path) -> PathBuf {
    if let Some(p) = std::env::var_os("BUDDY_PACK") {
        return PathBuf::from(p);
    }
    let lib = home.join("packs");
    let found = |id: &str| {
        pack_candidates(id, home).into_iter().find(|d| {
            if !d.join("pack.json").is_file() {
                return false;
            }
            if !d.starts_with(&lib) {
                return true;
            }
            match mascot_core::pack::load(d) {
                Ok(_) => true,
                Err(e) => {
                    eprintln!("[buddy] mascot \"{id}\" in {} doesn't load ({e}); skipping it", lib.display());
                    false
                }
            }
        })
    };
    if let Some(d) = Some(id).filter(|i| ::studio::package::valid_id(i)).and_then(found) {
        return d;
    }
    if id != "bear" {
        eprintln!("[buddy] mascot \"{id}\" not found; running the bear");
    }
    found("bear").unwrap_or_else(|| pack_candidates("bear", home).pop().unwrap_or_else(|| home.join("packs/bear")))
}

/// Sets settings `pack` from the studio: through the driver in the app (its settings are the
/// ones it saves), straight into settings.json in `--studio` mode.
fn desktop_setter(home: PathBuf, in_app: bool) -> Box<dyn Fn(&str) -> Result<(), String> + Send> {
    Box::new(move |id: &str| {
        let id = id.to_string();
        if in_app {
            driver::post(driver::Inbox::Call(Box::new(move |a| {
                a.settings.pack = id;
                a.save_settings();
            })));
            Ok(())
        } else {
            // the running app owns settings.json (it rewrites it from memory): only write it
            // while no app runs, holding the app lock so none starts meanwhile
            let _lock = instance::claim(&home).map_err(|_| "the desk-buddy app is running: choose the mascot in its own Studio (tray ▸ Studio…), or quit it first".to_string())?;
            let (mut s, _) = settings::load(&home);
            s.pack = id;
            s.save(&home).map_err(|e| format!("settings: {e}"))
        }
    })
}

/// The room pack's folder (floors and pocket scenes): `BUDDY_ROOM`, else `<id>` under the repo's
/// `rooms/` (debug builds) or `BUDDY_HOME/rooms`. `None` when no such folder has a `room.json`:
/// the app then runs with the grid only.
fn room_dir(id: &str, home: &Path) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("BUDDY_ROOM").filter(|p| !p.is_empty()) {
        return Some(PathBuf::from(p));
    }
    let mut roots = vec![];
    if cfg!(debug_assertions) {
        roots.push(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../rooms"));
    }
    roots.push(home.join("rooms"));
    // an id is one folder name, never a path out of the roots
    if id.is_empty() || id.contains(['/', '\\']) || id.starts_with('.') {
        return None;
    }
    roots.into_iter().map(|r| r.join(id)).find(|d| d.join("room.json").is_file())
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--studio") {
        return studio_main(args.get(i + 1).filter(|a| !a.starts_with("--")).map(PathBuf::from));
    }
    let home = settings::home();
    // held (open) until the process exits
    let _app_lock = match instance::claim(&home) {
        Ok(lock) => lock,
        Err(instance::Running(pid)) => {
            eprintln!("[buddy] already running (pid {})", pid.map_or("?".into(), |p| p.to_string()));
            std::process::exit(0);
        }
    };
    let (mut settings, note) = settings::load(&home);
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
    // the person's personality layer belongs to the mascot it was made for
    if let Some(line) = settings.persona.for_pack(&pack.id) {
        eprintln!("[buddy] {line}");
    }
    // a room that is there but broken stops the launch like a broken pack; none at all is fine
    let room = match room_dir(&settings.room, &home) {
        Some(d) => match mascot_core::room::load(&d) {
            Ok(r) => Some(r),
            Err(e) => fail("room", e),
        },
        None => {
            eprintln!("[buddy] no room pack \"{}\": floors and scenes are off", settings.room);
            None
        }
    };
    let art = match mascot_render::Art::load_with(&pack, room.as_ref()) {
        Ok(a) => a,
        Err(e) => fail(if room.is_some() { "mascot or room art" } else { "mascot art" }, e),
    };
    if let Some(r) = &room {
        eprintln!("[buddy] room {}: {} floors, {} scenes, from {}", r.id, r.floors.len(), r.scenes.len(), r.dir.display());
        // a room is mascot-independent, so a name this mascot lacks is fine, but say it: a typo never shows
        for (sc, n) in r.scenes.iter().flat_map(|sc| sc.for_.iter().map(move |n| (sc, n))) {
            if !pack.behaviors.contains_key(n) && !pack.sets.contains_key(n) && !pack.stations.contains_key(n) {
                eprintln!("[buddy] room {}: scene {} is for \"{n}\", which this mascot has no behavior or set of", r.id, sc.id);
            }
        }
    }
    let items = match std::env::var_os("BUDDY_EVENTS") {
        Some(f) => match events::load(Path::new(&f)) {
            Ok(v) => Some(v),
            Err(e) => fail("events", e),
        },
        None => None,
    };
    let (cw, ch) = art.size();
    eprintln!(
        "[buddy] pack {}: {} behaviors ({} available), canvas {cw}x{ch} sprite px, from {}",
        pack.id,
        pack.behaviors.len(),
        pack.behaviors.keys().filter(|b| pack.available(b)).count(),
        dir.display()
    );
    // wired entries whose art has not landed yet: one line, then they are skipped (`#hidden-log`)
    if let Some(line) = pack.waiting_line() {
        eprintln!("[buddy] {line}");
    }
    let icon = tray::icon(&art);
    let t0 = Instant::now();
    let dnd_action = driver::dnd_action(settings.dnd.action);
    let sensors = match std::env::var("BUDDY_SENSES").ok().as_deref() {
        Some("0") => false,
        Some(_) => true,
        None => items.is_none(), // a replay drives the senses itself
    };
    // the persona's temperament (pack, preset, the person's chattiness): the same one a change sets later
    let temperament = Some(persona::effects(&pack, &settings.persona.user).temperament);
    let brain = Brain::new(pack.clone(), BrainConfig { seed: seed(), temperament, dnd_action }, 0);

    studio::configure(Some(dir.clone()), studio_roots(&home), studio::Desktop { current: settings.pack.clone(), set: desktop_setter(home.clone(), true) });
    let mut boot = Some((pack, art, settings, home, brain, items, warning, icon));
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            bubble::window::prompt_submit,
            bubble::window::card_answer,
            bubble::window::bubble_dismiss,
            bubble::window::bubble_size,
            bubble::window::bubble_retry,
            bubble::window::bubble_recall,
            bubble::window::bubble_reply,
            bubble::window::reminder_snooze,
            signin::signin_state,
            signin::signin_save_key,
            signin::signin_start,
            signin::signin_answer,
            signin::signin_cancel,
            signin::signin_remove,
            signin::signin_reopen,
            signin::signin_close,
            persona::persona_state,
            persona::persona_set,
            persona::persona_pick,
            persona::persona_reset,
            persona::persona_open_studio,
            persona::persona_close,
            memory::window::memory_state,
            memory::window::memory_add,
            memory::window::memory_edit,
            memory::window::memory_forget,
            memory::window::memory_resolve,
            memory::window::memory_forget_all,
            memory::window::memory_set_enabled,
            memory::window::memory_close,
            widgets::widget_ready,
            widgets::widget_reply,
            widgets::widget_state,
            widgets::widget_visibility,
            widgets::widget_control,
            widgets::widget_drag,
            studio::studio_state,
            studio::studio_poll,
            studio::studio_open,
            studio::studio_frames,
            studio::studio_lint,
            studio::studio_audit,
            studio::studio_comments,
            studio::studio_comment_add,
            studio::studio_comment_status,
            studio::studio_sim,
            studio::studio_library,
            studio::studio_thumb,
            studio::studio_edit,
            studio::studio_save,
            studio::studio_import,
            studio::studio_remove_group,
            studio::studio_create,
            studio::studio_customize,
            studio::studio_restore,
            studio::studio_delete,
            studio::studio_use,
            studio::studio_reveal,
        ])
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let (pack, art, settings, home, brain, items, warning, icon) = boot.take().unwrap();
            let mtm = MainThreadMarker::new().expect("setup runs on the main thread");
            let (w, h) = art.size();
            let panel = panel::Panel::new(mtm, w, h, zoom, settings.position.as_ref(), art.feet(), mascot_render::feet_without_room(&pack));
            let q = &settings.quiet_hours;
            let tray_init = tray::TrayInit {
                pack_id: pack.id.clone(),
                zoom,
                warning,
                icon,
                dnd_action: settings.dnd.action,
                quiet_enabled: q.enabled,
                quiet_span: format!("{}–{}", q.start, q.end),
                dnd_fullscreen: settings.dnd.fullscreen,
                floors: art.room().map(|r| r.floors.iter().map(|f| (f.id.clone(), f.name.clone())).collect()).unwrap_or_default(),
                floor: driver::floor_choice(&art, &settings.floor).to_string(),
                scenes: art.room().is_some_and(|r| !r.scenes.is_empty()).then_some(settings.scenes),
                activity: settings.activity.enabled,
                screen_look: settings.screen_look.enabled,
                screen_look_missing: settings.screen_look.enabled && screen::permission_missing(),
                persona: persona::tray(&pack, &settings.persona.user),
            };
            let wanted = settings.model.clone();
            let memory_on = settings.memory.enabled_now();
            let home2 = home.clone();
            driver::start(driver::Init { pack, art, settings, home, panel, brain, t0 }, app.handle().clone());
            let win = bubble::window::BubbleWin::new(app, mtm)?;
            driver::with_app(|a| a.bubble_win = Some(win));
            tray::build(app, tray_init)?;
            signin::window::init(app.handle().clone());
            persona::window::init(app.handle().clone());
            studio::window::init(app.handle().clone());
            memory::window::init(app.handle().clone());
            // before the harness: the agent starts with the memory section (a failed open is the window's error)
            let mem = memory::start(&home2, memory_on);
            let mut host_port = None;
            match harness::Harness::start(&home2, &wanted) {
                Ok(h) => {
                    memory::jobs::start(mem);
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

/// `--studio [<pack-dir>]`: the studio window alone, on `<pack-dir>` or `BUDDY_PACK` (else the
/// library, listing the packs in `pack_roots`). It takes no lock and writes no host.json; it
/// writes the library (`BUDDY_HOME/packs`) and, on "Use on my desktop" or deleting the desktop's
/// mascot, settings `pack` (read, changed, saved). It starts no sidecar and shows no bear;
/// closing the window quits. Its handler list is the studio's commands, checked against
/// build.rs by the `widgets` ACL test.
fn studio_main(dir: Option<PathBuf>) {
    let home = settings::home();
    let start = dir.or_else(|| std::env::var_os("BUDDY_PACK").map(PathBuf::from)).map(|d| if d.is_relative() { std::env::current_dir().map(|c| c.join(&d)).unwrap_or(d) } else { d });
    match &start {
        Some(d) => eprintln!("[buddy] studio: pack {}", d.display()),
        None => eprintln!("[buddy] studio: no pack given (the window offers the packs it finds)"),
    }
    // read only: a bad settings.json is left for the app to report (and set aside)
    let current = settings::read(&home).ok().flatten().unwrap_or_default().pack;
    studio::configure(start.clone(), studio_roots(&home), studio::Desktop { current, set: desktop_setter(home.clone(), false) });
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            studio::studio_state,
            studio::studio_poll,
            studio::studio_open,
            studio::studio_frames,
            studio::studio_lint,
            studio::studio_audit,
            studio::studio_comments,
            studio::studio_comment_add,
            studio::studio_comment_status,
            studio::studio_sim,
            studio::studio_library,
            studio::studio_thumb,
            studio::studio_edit,
            studio::studio_save,
            studio::studio_import,
            studio::studio_remove_group,
            studio::studio_create,
            studio::studio_customize,
            studio::studio_restore,
            studio::studio_delete,
            studio::studio_use,
            studio::studio_reveal
        ])
        .setup(|app| {
            app.set_activation_policy(tauri::ActivationPolicy::Regular);
            studio::window::init(app.handle().clone());
            studio::window::open(true);
            quit_on_signals(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build tauri app");
    app.run(|_, _| {});
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
