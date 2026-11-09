//! Spike S1: the bear mascot on the real macOS desktop.
//!
//! One process: Tauri 2 owns the app, tray menu and an on-demand webview
//! window; the mascot is a native NSPanel driven through objc2 on the same
//! main thread.

mod assets;
mod behaviour;
mod panel;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

use objc2::MainThreadMarker;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};

use behaviour::Size;
use panel::Mascot;

thread_local! {
    // main-thread only: menu events and Tauri commands hop here
    static MASCOT: RefCell<Option<Rc<Mascot>>> = const { RefCell::new(None) };
}

fn with_mascot(f: impl FnOnce(&Rc<Mascot>)) {
    MASCOT.with(|m| {
        if let Some(m) = m.borrow().as_ref() {
            f(m)
        }
    });
}

#[tauri::command]
fn s1_info() -> String {
    format!("pid {} (same process as the mascot panel)", std::process::id())
}

#[tauri::command]
fn s1_poke(app: tauri::AppHandle) {
    let _ = app.run_on_main_thread(|| with_mascot(|m| m.poke()));
}

/// Menu-bar template icon: the bear's own silhouette from idle_S frame 0.
fn tray_icon(lib: &assets::Library) -> Image<'static> {
    let c = &lib.clips["idle"];
    let f = &c.dirs["S"].0[0];
    let (mut x0, mut y0, mut x1, mut y1) = (c.w, c.h, 0, 0);
    for y in 0..c.h {
        for x in 0..c.w {
            if f.px[(y * c.w + x) * 4 + 3] > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    let n = 36usize; // 18 pt @2x
    let side = (x1 - x0 + 1).max(y1 - y0 + 1) as f32;
    let mut out = vec![0u8; n * n * 4];
    for oy in 0..n {
        for ox in 0..n {
            // box-sample the alpha into the square icon
            let (mut sum, mut cnt) = (0u32, 0u32);
            let s = side / n as f32;
            for sy in 0..s.ceil() as usize {
                for sx in 0..s.ceil() as usize {
                    let fx = x0 as f32 + (ox as f32 * s) + sx as f32 - (side - (x1 - x0 + 1) as f32) / 2.0;
                    let fy = y0 as f32 + (oy as f32 * s) + sy as f32 - (side - (y1 - y0 + 1) as f32) / 2.0;
                    if fx >= 0.0 && fy >= 0.0 && (fx as usize) < c.w && (fy as usize) < c.h {
                        sum += f.px[(fy as usize * c.w + fx as usize) * 4 + 3] as u32;
                    }
                    cnt += 1;
                }
            }
            out[(oy * n + ox) * 4 + 3] = (sum / cnt.max(1)) as u8;
        }
    }
    Image::new_owned(out, n as u32, n as u32)
}

fn main() {
    let lib = match assets::load(&assets::default_anim_dir()) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[s1] cannot load mascot assets: {e}");
            std::process::exit(1);
        }
    };
    let icon = tray_icon(&lib);
    let lib = RefCell::new(Some(lib));

    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![s1_info, s1_poke])
        .setup(move |app| {
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let mtm = MainThreadMarker::new().expect("setup runs on the main thread");
            let mascot = Mascot::new(mtm, lib.borrow_mut().take().unwrap());
            MASCOT.with(|m| *m.borrow_mut() = Some(mascot));

            let z1 = CheckMenuItem::with_id(app, "zoom1", "Zoom ×1", true, std::env::var("S1_ZOOM").map(|z| z == "1").unwrap_or(true), None::<&str>)?;
            let z2 = CheckMenuItem::with_id(app, "zoom2", "Zoom ×2", true, std::env::var("S1_ZOOM").map(|z| z == "2").unwrap_or(false), None::<&str>)?;
            let z3 = CheckMenuItem::with_id(app, "zoom3", "Zoom ×3", true, std::env::var("S1_ZOOM").map(|z| z == "3").unwrap_or(false), None::<&str>)?;
            let small = std::env::var("S1_SIZE").as_deref() == Ok("64");
            let s100 = CheckMenuItem::with_id(app, "size100", "Size 100 px", true, !small, None::<&str>)?;
            let s64 = CheckMenuItem::with_id(app, "size64", "Size 64 px (walk S/SE only)", true, small, None::<&str>)?;
            let zoom = Submenu::with_items(app, "Zoom", true, &[&z1, &z2, &z3])?;
            let size = Submenu::with_items(app, "Size", true, &[&s100, &s64])?;
            let show = MenuItem::with_id(app, "webview", "Show test webview", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "Hide mascot", true, None::<&str>)?;
            let next = MenuItem::with_id(app, "nextdisplay", "Move to next display", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep = || PredefinedMenuItem::separator(app);
            let menu = Menu::with_items(app, &[&zoom, &size, &sep()?, &show, &hide, &next, &sep()?, &quit])?;

            let (zs, ss, hide2) = ([z1, z2, z3], [s100, s64], hide.clone());
            TrayIconBuilder::with_id("s1")
                .icon(icon.clone())
                .icon_as_template(true)
                .tooltip("desk-buddy S1")
                .menu(&menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, ev| {
                    let id = ev.id().as_ref();
                    match id {
                        "quit" => app.exit(0),
                        "zoom1" | "zoom2" | "zoom3" => {
                            let z: u32 = id[4..].parse().unwrap();
                            for (i, it) in zs.iter().enumerate() {
                                let _ = it.set_checked(i as u32 + 1 == z);
                            }
                            with_mascot(|m| m.set_zoom(z));
                        }
                        "size100" | "size64" => {
                            let s = if id == "size64" { Size::S64 } else { Size::S100 };
                            let _ = ss[0].set_checked(s == Size::S100);
                            let _ = ss[1].set_checked(s == Size::S64);
                            with_mascot(|m| m.set_size(s));
                        }
                        "hide" => with_mascot(|m| {
                            let v = !m.is_visible();
                            m.set_visible(v);
                            let _ = hide2.set_text(if v { "Hide mascot" } else { "Show mascot" });
                        }),
                        "nextdisplay" => with_mascot(|m| m.next_display()),
                        "webview" => {
                            if let Some(w) = app.get_webview_window("test") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            } else {
                                let r = WebviewWindowBuilder::new(app, "test", WebviewUrl::App("index.html".into()))
                                    .title("desk-buddy S1 test webview")
                                    .inner_size(380.0, 240.0)
                                    .resizable(false)
                                    .build();
                                match r {
                                    Ok(w) => {
                                        let _ = w.set_focus();
                                        eprintln!("[s1] test webview opened");
                                    }
                                    Err(e) => eprintln!("[s1] test webview failed: {e}"),
                                }
                            }
                        }
                        _ => {}
                    }
                })
                .build(app)?;

            if std::env::var_os("S1_OPEN_WEBVIEW").is_some() {
                let w = WebviewWindowBuilder::new(app, "test", WebviewUrl::App("index.html".into()))
                    .title("desk-buddy S1 test webview")
                    .inner_size(380.0, 240.0)
                    .build()?;
                let _ = w.set_focus();
                eprintln!("[s1] test webview opened (S1_OPEN_WEBVIEW)");
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build tauri app");

    app.run(|_app, ev| {
        // closing the test webview must not quit the app; only Quit does
        if let RunEvent::ExitRequested { code: None, api, .. } = ev {
            api.prevent_exit();
        }
    });
}
