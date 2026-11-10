//! desk-buddy: one Tauri 2 process, a menu-bar accessory (no Dock icon). The bear is a
//! native NSPanel (`panel.rs`, from spike S1) driven by `mascot_core::Brain` on the main
//! thread (`driver.rs`), drawn by `mascot-render`.
//!
//! Env: `BUDDY_HOME` (default `~/.desk-buddy`), `BUDDY_PACK=<dir>` (a pack dir; else
//! settings `pack` under `$BUDDY_PACKS`, the repo's `mascots/` in debug builds, then
//! `BUDDY_HOME/packs`), `BUDDY_ZOOM=1|2|3`, `BUDDY_STATS=1`, `BUDDY_DUMP=<dir>`
//! (+ `BUDDY_DUMP_EVERY=<n>`), `BUDDY_EVENTS=<file.jsonl>`, `BUDDY_SCREEN=<n>`,
//! `BUDDY_VERBOSE=1`.

mod driver;
mod events;
mod panel;
mod settings;
mod stats;
mod tray;

use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::Arc;
use std::time::Instant;

use block2::RcBlock;
use mascot_core::{Brain, BrainConfig, DndAction, PackMeta};
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
    let icon = tray::icon(&pack);
    let t0 = Instant::now();
    let dnd_action = match settings.dnd.action {
        settings::DndAct::Hide => DndAction::Hide,
        settings::DndAct::Still => DndAction::Still,
    };
    let brain = Brain::new(pack.clone(), BrainConfig { seed: seed(), temperament: None, dnd_action }, 0);

    let mut boot = Some((pack, art, settings, home, brain, items, warning, icon));
    let app = tauri::Builder::default()
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);
            let (pack, art, settings, home, brain, items, warning, icon) = boot.take().unwrap();
            let mtm = MainThreadMarker::new().expect("setup runs on the main thread");
            let (w, h) = mascot_render::canvas_size(&pack);
            let panel = panel::Panel::new(mtm, w, h, zoom, settings.position.as_ref());
            let tray_init = tray::TrayInit { pack_id: pack.id.clone(), zoom, warning, icon };
            driver::start(driver::Init { pack, art, settings, home, panel, brain, t0 }, app.handle().clone());
            tray::build(app, tray_init)?;
            if std::env::var_os("BUDDY_STATS").is_some() {
                start_stats();
            }
            if let Some(items) = items {
                events::replay(items, t0);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build tauri app");

    app.run(|_app, ev| {
        // no window closing quits the app; only Quit does
        if let RunEvent::ExitRequested { code: None, api, .. } = ev {
            api.prevent_exit();
        }
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
