//! The Personality window: a normal, activating Tauri window `persona` (page `web/persona/index.html`),
//! made on first open and hidden (not destroyed) on close, like the Accounts window. Its state arrives
//! as `persona://state` (the same view `persona_state` answers).

use std::sync::OnceLock;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LABEL: &str = "persona";
pub const EVENT: &str = "persona://state";

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// At setup: where the window and its events live.
pub fn init(handle: AppHandle) {
    let _ = HANDLE.set(handle);
}

/// The view to the page (when its window exists).
pub fn emit(view: &Value) {
    if let Some(h) = HANDLE.get() {
        if h.get_webview_window(LABEL).is_some() {
            let _ = h.emit_to(LABEL, EVENT, view);
        }
    }
}

/// Tray → Personality ▸ Customize…: show (making it the first time) and bring it front, with the
/// current view.
pub fn open() {
    let Some(h) = HANDLE.get().cloned() else { return };
    let h2 = h.clone();
    let _ = h.run_on_main_thread(move || {
        let win = match h2.get_webview_window(LABEL) {
            Some(w) => w,
            None => match WebviewWindowBuilder::new(&h2, LABEL, WebviewUrl::App("persona/index.html".into())).title("Personality").inner_size(460.0, 720.0).min_inner_size(380.0, 480.0).resizable(true).visible(false).build() {
                Ok(w) => {
                    let w2 = w.clone();
                    // the red button hides it (the page has saved as it went)
                    w.on_window_event(move |e| {
                        if let WindowEvent::CloseRequested { api, .. } = e {
                            api.prevent_close();
                            let _ = w2.hide();
                        }
                    });
                    w
                }
                Err(e) => {
                    eprintln!("[buddy] persona window: {e}");
                    return;
                }
            },
        };
        eprintln!("[buddy] persona: window open");
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        crate::driver::with_app(|a| {
            let _ = win.emit_to(LABEL, EVENT, a.persona_view());
        });
    });
}

pub fn hide() {
    let Some(h) = HANDLE.get().cloned() else { return };
    let h2 = h.clone();
    let _ = h.run_on_main_thread(move || {
        if let Some(w) = h2.get_webview_window(LABEL) {
            let _ = w.hide();
        }
    });
}
