//! Menu-bar item (prototype `#tray-menu`, `#tray-menu-error`, `#tray-dnd`). p1 items: a
//! header, an optional settings warning, Size ▸ Small/Medium/Large (the zoom, persisted), Hide/Show bear, Move
//! to next display, Quit. p2 (senses): a status line while DND or quiet hours is active,
//! "Do not disturb" (session only), "Quiet hours HH:MM–HH:MM" (writes
//! `quiet_hours.enabled`) and "When busy ▸ Hide / Hold still" (writes `dnd.action`).
//! p3 (talk): "Ask…" (opens the bubble's prompt), the agent line ("Ready · <model>",
//! stopped, problem, "Connect an AI account…", can't read the keychain) and "Reset web caution" while the chat is
//! tainted. UX pass: plain words ("Ready · <model>", "Connect an AI account…" which opens the
//! Accounts window, "Reset web caution", "Check-ins: N of 4 this hour", "Accounts…"), the three
//! sizes in a "Size" submenu, and "Approvals ▸" (the policy's ask / allowlist modes). p4 (accounts): the budget line ("Check-ins: N of 4 this hour" / "Check-ins paused
//! (…)", `#tray-budget`), "Accounts…" (the sign-in window), and Ask… reads "Ask… (bear hidden)",
//! disabled, while DND or quiet hours hide the bear (a bear the person hid comes back on Ask…).
//! Settings pass: "Hide over fullscreen apps" (writes `dnd.fullscreen`, off by default: the bear stays
//! visible), "Model ▸" (the models of the signed-in providers, from the sidecar's catalog; writes
//! `settings.model`) and "Approvals ▸ Full access" (the policy's `full` mode).

use mascot_core::{Dir, EyeState, Intent, MouthState, Show, Visibility};
use mascot_render::{Art, View};
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::TrayIconBuilder;
use tauri::{App as TauriApp, AppHandle, Wry};

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
    /// `dnd.fullscreen`: hide (or hold still) while a fullscreen app fills the bear's display
    pub dnd_fullscreen: bool,
}

const SIZES: [&str; 3] = ["Small", "Medium", "Large"];

fn size_label(zoom: u32) -> String {
    format!("Size: {}", SIZES[(zoom.clamp(1, 3) - 1) as usize])
}

/// The Approvals submenu's title for a policy mode (`skip` can only be set by editing policy.json).
pub fn approvals_label(mode: &str) -> &'static str {
    match mode {
        "ask" => "Approvals: ask every time",
        "full" => "Approvals: full access",
        "skip" => "Approvals: off (never asks)",
        _ => "Approvals: ask when unsure",
    }
}

/// The Model submenu's title: the model in use by its catalog name.
fn model_label(list: &[harness::ModelChoice], current: Option<&bridge::protocol::ModelRef>) -> String {
    match current {
        Some(c) => format!("Model: {}", list.iter().find(|m| m.provider == c.provider && m.id == c.model).map_or(c.model.as_str(), |m| m.name.as_str())),
        None => "Model".into(),
    }
}

/// Refill the Model submenu: one check item per model (id `model:<provider>/<model id>`), under its
/// provider's name when more than one provider is signed in. Returns the check items by id.
fn fill_models(app: &AppHandle<Wry>, sub: &Submenu<Wry>, list: &[harness::ModelChoice]) -> Vec<(String, CheckMenuItem<Wry>)> {
    while sub.items().is_ok_and(|i| !i.is_empty()) {
        if sub.remove_at(0).is_err() {
            break;
        }
    }
    let mut out = Vec::new();
    if list.is_empty() {
        if let Ok(none) = MenuItem::with_id(app, "model_none", "Connect an AI account to choose a model", false, None::<&str>) {
            let _ = sub.append(&none);
        }
        return out;
    }
    let mut providers: Vec<&str> = Vec::new();
    for m in list {
        if !providers.contains(&m.provider.as_str()) {
            providers.push(&m.provider);
        }
    }
    let harness = harness::get();
    for (i, p) in providers.iter().enumerate() {
        if providers.len() > 1 {
            if i > 0 {
                if let Ok(sep) = PredefinedMenuItem::separator(app) {
                    let _ = sub.append(&sep);
                }
            }
            let name = harness.as_ref().and_then(|h| h.catalog().iter().find(|c| c.id == *p).map(|c| c.name.clone())).unwrap_or_else(|| p.to_string());
            if let Ok(head) = MenuItem::with_id(app, format!("model_head:{p}"), name, false, None::<&str>) {
                let _ = sub.append(&head);
            }
        }
        for m in list.iter().filter(|m| m.provider == *p) {
            let id = format!("model:{}/{}", m.provider, m.id);
            if let Ok(it) = CheckMenuItem::with_id(app, &id, &m.name, true, false, None::<&str>) {
                let _ = sub.append(&it);
                out.push((id, it));
            }
        }
    }
    out
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
        .map(|z| CheckMenuItem::with_id(app, format!("zoom{z}"), SIZES[(z - 1) as usize], true, z == t.zoom, None::<&str>));
    let [z1, z2, z3] = zooms;
    let zs = [z1?, z2?, z3?];
    let size = Submenu::with_id_and_items(app, "size", size_label(t.zoom), true, &[&zs[0], &zs[1], &zs[2]])?;
    let mode_ask = CheckMenuItem::with_id(app, "mode_ask", "Ask every time", true, false, None::<&str>)?;
    let mode_list = CheckMenuItem::with_id(app, "mode_allowlist", "Ask when unsure (safe commands just run)", true, true, None::<&str>)?;
    let mode_full = CheckMenuItem::with_id(app, "mode_full", "Full access (asks only before risky things)", true, false, None::<&str>)?;
    let approvals = Submenu::with_id_and_items(app, "approvals", approvals_label("allowlist"), true, &[&mode_ask, &mode_list, &mode_full])?;
    let model = Submenu::with_id(app, "model", "Model", true)?;
    fill_models(app.handle(), &model, &[]);
    let fs_hide = CheckMenuItem::with_id(app, "fs_hide", "Hide over fullscreen apps", true, t.dnd_fullscreen, None::<&str>)?;
    let ask = MenuItem::with_id(app, "ask", "Ask…", true, None::<&str>)?;
    let agent = MenuItem::with_id(app, "agent", harness::AgentState::Starting.line(), false, None::<&str>)?;
    let taint = MenuItem::with_id(app, "taint", "Reset web caution", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide bear", true, None::<&str>)?;
    let next = MenuItem::with_id(app, "nextdisplay", "Move to next display", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit desk-buddy", true, None::<&str>)?;
    let budget = MenuItem::with_id(app, "budget", "Check-ins: 0 of 4 this hour", false, None::<&str>)?;
    let signin = MenuItem::with_id(app, "signin", "Accounts…", true, None::<&str>)?;
    let studio = MenuItem::with_id(app, "studio", "Studio…", true, None::<&str>)?;
    let sep5 = PredefinedMenuItem::separator(app)?;
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
    // "Reset web caution" goes in under the agent line while tainted
    let taint_at = items.len();
    items.extend([&sep4 as &dyn IsMenuItem<Wry>, &size, &hide, &next, &sep2, &dnd, &quiet, &busy, &fs_hide, &budget, &sep5, &model, &approvals, &signin, &studio, &sep3, &quit]);
    let menu = Menu::with_items(app, &items)?;

    let hide2 = hide.clone();
    let ask2 = ask.clone();
    let (menu2, status2) = (menu.clone(), status.clone());
    let (menu3, taint2) = (menu.clone(), taint.clone());
    let (dnd2, quiet2, bh2, bs2, busy2) = (dnd.clone(), quiet.clone(), busy_hide.clone(), busy_still.clone(), busy.clone());
    let fs2 = fs_hide.clone();
    let apph = app.handle().clone();
    with_app(move |a| {
        a.on_agent = Some(Box::new(move |s| {
            let _ = agent.set_text(s.line());
            // with no account the line is the way in: it opens the Accounts window
            let _ = agent.set_enabled(s.needs_account());
        }));
        // the list last shown and its check items: refilled only when the list itself changes
        let shown = std::cell::RefCell::new((Vec::<harness::ModelChoice>::new(), Vec::<(String, CheckMenuItem<Wry>)>::new()));
        a.on_models = Some(Box::new(move |list, current| {
            let mut st = shown.borrow_mut();
            if st.0 != list {
                st.1 = fill_models(&apph, &model, list);
                st.0 = list.to_vec();
            }
            let on = current.map(|c| format!("model:{}/{}", c.provider, c.model));
            for (id, it) in &st.1 {
                let _ = it.set_checked(on.as_deref() == Some(id.as_str()));
            }
            let _ = model.set_text(model_label(list, current));
        }));
        let (ma, ml, mf, ap) = (mode_ask.clone(), mode_list.clone(), mode_full.clone(), approvals.clone());
        a.on_mode = Some(Box::new(move |mode| {
            let _ = ma.set_checked(mode == "ask");
            let _ = ml.set_checked(mode == "allowlist");
            let _ = mf.set_checked(mode == "full");
            let _ = ap.set_text(approvals_label(mode));
        }));
        let _ = budget.set_text(a.budget_line());
        a.on_budget = Some(Box::new(move |line| {
            let _ = budget.set_text(line);
        }));
        a.on_ask = Some(Box::new(move |brain_hidden| {
            let _ = ask2.set_text(if brain_hidden { "Ask… (bear hidden)" } else { "Ask…" });
            let _ = ask2.set_enabled(!brain_hidden);
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
            let _ = fs2.set_checked(s.dnd.fullscreen);
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
                "signin" | "agent" => crate::signin::window::open(),
                "mode_ask" | "mode_allowlist" | "mode_full" => {
                    let mode = id[5..].to_string();
                    // the checkmarks follow the policy's own view (`on_mode`), also when this fails
                    with_app(|a| a.policy_mode_refresh());
                    std::thread::spawn(move || {
                        if let Some(h) = harness::get() {
                            eprintln!("[buddy] approvals: {mode}");
                            if let Err(e) = h.agent().policy_ui(&serde_json::json!({ "op": "set_mode", "mode": mode })) {
                                eprintln!("[buddy] approvals: {e}");
                            }
                        }
                    });
                }
                "studio" => crate::studio::window::open(false),
                "taint" => {
                    std::thread::spawn(|| {
                        if let Some(h) = harness::get() {
                            eprintln!("[buddy] reset web caution (clear taint)");
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
                    let _ = size.set_text(size_label(z));
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
                "fs_hide" => with_app(|a| {
                    let on = !a.settings.dnd.fullscreen;
                    a.set_dnd_fullscreen(on);
                    let _ = fs_hide.set_checked(on);
                }),
                m if m.starts_with("model:") => {
                    // provider ids have no slash; model ids may (`anthropic/claude-…` on OpenRouter)
                    if let Some((provider, model)) = m[6..].split_once('/') {
                        let (provider, model) = (provider.to_string(), model.to_string());
                        std::thread::spawn(move || {
                            if let Some(h) = harness::get() {
                                h.pick_model(&provider, &model);
                            }
                        });
                    }
                }
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
