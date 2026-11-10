//! Menu-bar item (prototype `#tray-menu`, `#tray-menu-error`, `#tray-dnd`). p1 items: a
//! header, an optional settings warning, Zoom ×1/×2/×3 (persisted), Hide/Show bear, Move
//! to next display, Quit. p2 (senses): a status line while DND or quiet hours is active,
//! "Do not disturb" (session only), "Quiet hours HH:MM–HH:MM" (writes
//! `quiet_hours.enabled`) and "When busy ▸ Hide / Hold still" (writes `dnd.action`).
//! p3 (talk): "Ask…" (opens the bubble's prompt), the agent line ("Agent: running · <model>",
//! stopped, error, no key) and "Clear web taint" while the chat is tainted.

use mascot_core::{Dir, EyeState, Intent, MouthState, Show, Visibility};
use mascot_render::{Art, View};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{App as TauriApp, Wry};

use crate::driver::with_app;
use crate::harness;
use crate::settings::DndAct;

/// Menu-bar template icon: the bear's own silhouette from idle S frame 0, composed from
/// the already-decoded art (no floor), one bbox scan, box-sampled alpha.
pub fn icon(art: &Art) -> Option<Image<'static>> {
    let show = Show {
        intent: Intent::Idle,
        behavior: String::new(),
        clip: "idle".into(),
        dir: Dir::S,
        ori: None,
        frame: 0,
        eyes: EyeState::Open,
        mouth: MouthState::Closed,
        effect: None,
        visibility: Visibility::Shown,
        moved_px: [0, 0],
        cut: false,
        place: None,
    };
    let mut c = art.canvas();
    mascot_render::compose(art, &show, &View { cam: [0, 0], floor: false, vignette: false }, &mut c);
    silhouette(c.w, c.h, |x, y| c.px[(y * c.w + x) * 4 + 3])
}

/// A 36×36 (18 pt @2x) alpha-only image of the opaque bbox of `a`, centred and square.
fn silhouette(fw: usize, fh: usize, a: impl Fn(usize, usize) -> u8) -> Option<Image<'static>> {
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
    let n = 36usize;
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
    pub dnd_action: DndAct,
    pub quiet_enabled: bool,
    /// "22:30–07:30" (times are edited in settings.json)
    pub quiet_span: String,
}

fn busy_label(a: DndAct) -> &'static str {
    match a {
        DndAct::Hide => "When busy: Hide",
        DndAct::Still => "When busy: Hold still",
    }
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
    let ask = MenuItem::with_id(app, "ask", "Ask…", true, None::<&str>)?;
    let agent = MenuItem::with_id(app, "agent", harness::AgentState::Starting.line(), false, None::<&str>)?;
    let taint = MenuItem::with_id(app, "taint", "Clear web taint", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide bear", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "nextdisplay", "Move to next display", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit desk-buddy", true, None::<&str>)?;
    let status = MenuItem::with_id(app, "status", "", false, None::<&str>)?;
    let dnd = CheckMenuItem::with_id(app, "dnd", "Do not disturb", true, false, None::<&str>)?;
    let quiet = CheckMenuItem::with_id(app, "quiet", format!("Quiet hours {}", t.quiet_span), true, t.quiet_enabled, None::<&str>)?;
    let busy_hide = CheckMenuItem::with_id(app, "busy_hide", "Hide", true, t.dnd_action == DndAct::Hide, None::<&str>)?;
    let busy_still = CheckMenuItem::with_id(app, "busy_still", "Hold still", true, t.dnd_action == DndAct::Still, None::<&str>)?;
    let busy = Submenu::with_id_and_items(app, "busy", busy_label(t.dnd_action), true, &[&busy_hide, &busy_still])?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let sep3 = PredefinedMenuItem::separator(app)?;
    let sep4 = PredefinedMenuItem::separator(app)?;
    let mut items: Vec<&dyn IsMenuItem<Wry>> = vec![&header];
    if let Some(w) = &warn {
        items.push(w);
    }
    // the status line goes in at index `items.len()` (under the header/warning) when active
    let status_at = items.len();
    items.extend([&sep1 as &dyn IsMenuItem<Wry>, &ask, &agent]);
    // "Clear web taint" goes in under the agent line while tainted
    let taint_at = items.len();
    items.extend([&sep4 as &dyn IsMenuItem<Wry>, &zs[0], &zs[1], &zs[2], &hide, &next, &sep2, &dnd, &quiet, &busy, &sep3, &quit]);
    let menu = Menu::with_items(app, &items)?;

    let hide2 = hide.clone();
    let (menu2, status2) = (menu.clone(), status.clone());
    let (menu3, taint2) = (menu.clone(), taint.clone());
    let (dnd2, quiet2, bh2, bs2, busy2) = (dnd.clone(), quiet.clone(), busy_hide.clone(), busy_still.clone(), busy.clone());
    with_app(move |a| {
        a.on_agent = Some(Box::new(move |s| {
            let _ = agent.set_text(s.line());
        }));
        a.on_taint = Some(Box::new(move |t| {
            let shown = menu3.get("taint").is_some();
            // the status line may sit above: count it in
            let at = taint_at + usize::from(menu3.get("status").is_some());
            match (t, shown) {
                (true, false) => {
                    let _ = menu3.insert(&taint2, at);
                }
                (false, true) => {
                    let _ = menu3.remove(&taint2);
                }
                _ => {}
            }
        }));
        // settings changed under the menu (Inbox::Settings): the checkmarks follow
        a.on_settings = Some(Box::new(move |s| {
            let _ = quiet2.set_checked(s.quiet_hours.enabled);
            let _ = bh2.set_checked(s.dnd.action == DndAct::Hide);
            let _ = bs2.set_checked(s.dnd.action == DndAct::Still);
            let _ = busy2.set_text(busy_label(s.dnd.action));
        }));
        let _ = dnd2.set_checked(a.dnd_toggle());
        a.on_hidden = Some(Box::new(move |h| {
            let _ = hide2.set_text(if h { "Show bear" } else { "Hide bear" });
        }));
        a.on_status = Some(Box::new(move |st| match st {
            Some(text) => {
                let _ = status2.set_text(text);
                // insert once; a second insert of the same item would duplicate it
                if menu2.get("status").is_none() {
                    let _ = menu2.insert(&status2, status_at);
                }
            }
            None => {
                let _ = menu2.remove(&status2);
            }
        }));
    });
    let mut b = TrayIconBuilder::with_id("buddy")
        .tooltip("desk-buddy")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(move |app, ev| {
            let id = ev.id().as_ref();
            match id {
                "quit" => app.exit(0),
                "ask" => with_app(|a| a.open_prompt()),
                "taint" => {
                    std::thread::spawn(|| {
                        if let Some(h) = harness::get() {
                            eprintln!("[buddy] clear web taint");
                            if let Err(e) = h.agent().policy_ui(&serde_json::json!({ "op": "clear_taint" })) {
                                eprintln!("[buddy] clear taint: {e}");
                            }
                        }
                    });
                }
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
                "dnd" => with_app(|a| {
                    let on = !a.dnd_toggle();
                    a.set_dnd_toggle(on);
                    let _ = dnd.set_checked(on);
                }),
                "quiet" => with_app(|a| {
                    let on = !a.settings.quiet_hours.enabled;
                    a.set_quiet_enabled(on);
                    let _ = quiet.set_checked(on);
                }),
                "busy_hide" | "busy_still" => {
                    let act = if id == "busy_hide" { DndAct::Hide } else { DndAct::Still };
                    let _ = busy_hide.set_checked(act == DndAct::Hide);
                    let _ = busy_still.set_checked(act == DndAct::Still);
                    let _ = busy.set_text(busy_label(act));
                    with_app(|a| a.set_dnd_action(act));
                }
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
