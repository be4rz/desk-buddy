//! The Sign in… window: a normal, activating Tauri window `signin` (page `web/signin/index.html`),
//! made on first open and hidden (not destroyed) on close. Its state arrives as `signin://state`.

use std::sync::OnceLock;

use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LABEL: &str = "signin";
pub const EVENT: &str = "signin://state";

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// At setup: where the window and its events live.
pub fn init(handle: AppHandle) {
    let _ = HANDLE.set(handle);
}

/// The state to the page (when its window exists).
pub fn emit(state: &Value) {
    if let Some(h) = HANDLE.get() {
        if h.get_webview_window(LABEL).is_some() {
            let _ = h.emit_to(LABEL, EVENT, state);
        }
    }
}

/// Tray → Sign in… and the bubble's Sign in…: show (making it the first time) and bring it front.
pub fn open() {
    let Some(h) = HANDLE.get().cloned() else { return };
    let h2 = h.clone();
    let _ = h.run_on_main_thread(move || {
        let win = match h2.get_webview_window(LABEL) {
            Some(w) => w,
            None => match WebviewWindowBuilder::new(&h2, LABEL, WebviewUrl::App("signin/index.html".into())).title("Accounts").inner_size(460.0, 560.0).min_inner_size(380.0, 420.0).resizable(true).visible(false).build() {
                Ok(w) => {
                    let w2 = w.clone();
                    // the red button hides it; a sign-in in progress is cancelled
                    w.on_window_event(move |e| {
                        if let WindowEvent::CloseRequested { api, .. } = e {
                            api.prevent_close();
                            let _ = w2.hide();
                            if let Some(s) = super::get() {
                                std::thread::spawn(move || s.cancel());
                            }
                        }
                    });
                    w
                }
                Err(e) => {
                    eprintln!("[buddy] signin window: {e}");
                    return;
                }
            },
        };
        eprintln!("[buddy] signin: window open");
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        if let Some(s) = super::get() {
            let _ = win.emit_to(LABEL, EVENT, s.state());
        }
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
