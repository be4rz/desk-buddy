//! Menu-bar item (prototype `#tray-menu`, `#tray-menu-error`). p1 items: a header, an
//! optional settings warning, Zoom ×1/×2/×3 (persisted), Hide/Show bear, Move to next
//! display, separator, Quit. Later phases add their items here.

use mascot_core::{Dir, PackMeta};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{App as TauriApp, Wry};

use crate::driver::with_app;

/// Menu-bar template icon: the bear's own silhouette from idle S frame 0 (pack art,
/// box-sampled alpha; S1's icon).
pub fn icon(pack: &PackMeta) -> Option<Image<'static>> {
    let c = pack.clips.get("idle")?;
    let strip = c.dirs.get(&Dir::S)?;
    let (w, _, rgba) = mascot_render::read_png(&strip.file).ok()?;
    let (fw, fh) = (c.frame_w as usize, c.frame_h as usize);
    let a = |x: usize, y: usize| rgba[(y * w + x) * 4 + 3];
    let (mut x0, mut y0, mut x1, mut y1) = (fw, fh, 0, 0);
    for y in 0..fh {
        for x in 0..fw {
            if a(x, y) > 0 {
                x0 = x0.min(x);
                y0 = y0.min(y);
                x1 = x1.max(x);
                y1 = y1.max(y);
            }
        }
    }
    if x1 < x0 {
        return None;
    }
    let n = 36usize; // 18 pt @2x
    let side = (x1 - x0 + 1).max(y1 - y0 + 1) as f32;
    let s = side / n as f32;
    let mut out = vec![0u8; n * n * 4];
    for oy in 0..n {
        for ox in 0..n {
            let (mut sum, mut cnt) = (0u32, 0u32);
            for sy in 0..s.ceil() as usize {
                for sx in 0..s.ceil() as usize {
                    let fx = x0 as f32 + (ox as f32 * s) + sx as f32 - (side - (x1 - x0 + 1) as f32) / 2.0;
                    let fy = y0 as f32 + (oy as f32 * s) + sy as f32 - (side - (y1 - y0 + 1) as f32) / 2.0;
                    if fx >= 0.0 && fy >= 0.0 && (fx as usize) < fw && (fy as usize) < fh {
                        sum += a(fx as usize, fy as usize) as u32;
                    }
                    cnt += 1;
                }
            }
            out[(oy * n + ox) * 4 + 3] = (sum / cnt.max(1)) as u8;
        }
    }
    Some(Image::new_owned(out, n as u32, n as u32))
}

pub struct TrayInit {
    pub pack_id: String,
    pub zoom: u32,
    pub warning: Option<String>,
    pub icon: Option<Image<'static>>,
}

pub fn build(app: &TauriApp<Wry>, t: TrayInit) -> tauri::Result<()> {
    let header = MenuItem::with_id(app, "header", format!("desk-buddy · {} pack", t.pack_id), false, None::<&str>)?;
    let warn = match &t.warning {
        Some(w) => Some(MenuItem::with_id(app, "warning", format!("⚠ {w}"), false, None::<&str>)?),
        None => None,
    };
    let zooms = [1u32, 2, 3]
        .map(|z| CheckMenuItem::with_id(app, format!("zoom{z}"), format!("Zoom ×{z}"), true, z == t.zoom, None::<&str>));
    let [z1, z2, z3] = zooms;
    let zs = [z1?, z2?, z3?];
    let hide = MenuItem::with_id(app, "hide", "Hide bear", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "nextdisplay", "Move to next display", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit desk-buddy", true, None::<&str>)?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&header];
    if let Some(w) = &warn {
        items.push(w);
    }
    items.extend([&sep1 as &dyn IsMenuItem<Wry>, &zs[0], &zs[1], &zs[2], &hide, &next, &sep2, &quit]);
    let menu = Menu::with_items(app, &items)?;

    let hide2 = hide.clone();
    with_app(move |a| {
        a.on_hidden = Some(Box::new(move |h| {
            let _ = hide2.set_text(if h { "Show bear" } else { "Hide bear" });
        }))
    });
    let mut b = TrayIconBuilder::with_id("buddy")
        .tooltip("desk-buddy")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, ev| {
            let id = ev.id().as_ref();
            match id {
                "quit" => app.exit(0),
                "zoom1" | "zoom2" | "zoom3" => {
                    let z: u32 = id[4..].parse().unwrap_or(1);
                    for (i, it) in zs.iter().enumerate() {
                        let _ = it.set_checked(i as u32 + 1 == z);
                    }
                    with_app(|a| a.set_zoom(z));
                }
                "hide" => with_app(|a| {
                    let h = !a.user_hidden();
                    a.set_user_hidden(h);
                }),
                "nextdisplay" => with_app(|a| a.next_display()),
                _ => {}
            }
        });
    // a pack without an idle S silhouette still gets a visible item (the only way to Quit)
    b = match t.icon {
        Some(icon) => b.icon(icon).icon_as_template(true),
        None => b.title("desk-buddy"),
    };
    b.build(app)?;
    Ok(())
}
