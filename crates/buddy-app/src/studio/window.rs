//! The studio window: a normal, activating Tauri window `studio` (page `web/studio/index.html`),
//! made on open and destroyed on close, so its poll and preview loop stop and the session is
//! dropped. In `--studio` mode closing it quits the app.

use std::sync::OnceLock;

use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

pub const LABEL: &str = "studio";

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

pub fn init(handle: AppHandle) {
    let _ = HANDLE.set(handle);
}

/// Tray → Studio… (and `--studio` at launch): show it, making it when needed.
pub fn open(quit_on_close: bool) {
    let Some(h) = HANDLE.get().cloned() else { return };
    let h2 = h.clone();
    let _ = h.run_on_main_thread(move || {
        if let Some(w) = h2.get_webview_window(LABEL) {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
            return;
        }
        match WebviewWindowBuilder::new(&h2, LABEL, WebviewUrl::App("studio/index.html".into())).title("desk-buddy studio").inner_size(1180.0, 760.0).min_inner_size(900.0, 560.0).resizable(true).build() {
            Ok(w) => {
                let h3 = h2.clone();
                w.on_window_event(move |e| {
                    if let WindowEvent::Destroyed = e {
                        super::close();
                        eprintln!("[buddy] studio: closed");
                        if quit_on_close {
                            h3.exit(0);
                        }
                    }
                });
                eprintln!("[buddy] studio: window open");
                let _ = w.set_focus();
            }
            Err(e) => eprintln!("[buddy] studio window: {e}"),
        }
    });
}
