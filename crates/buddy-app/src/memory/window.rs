//! The "What I remember" window: a normal, activating Tauri window `memory` (page
//! `web/memory/index.html`), made on first open and hidden (not destroyed) on close, like the
//! Personality window. Opened by the tray's **What I remember…** and by `buddy memory open`. Its state
//! arrives as `memory://state` (the same State `memory_state` answers, design I6).
//!
//! The commands answer only this window (`caller.rs`, `capabilities/memory.json`). Their `Err` is one
//! of the fixed strings `not allowed|secret|empty|too_long|not_found|bad_phrase|store`; the page
//! words them. Store calls are short local SQLite operations, run on the command's own thread;
//! only the switch touches the main thread (the app owns settings.json).

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

use super::Mem;
use crate::driver::{post, Inbox};

pub const LABEL: &str = "memory";
pub const EVENT: &str = "memory://state";

static HANDLE: OnceLock<AppHandle> = OnceLock::new();

/// At setup: where the window and its events live.
pub fn init(handle: AppHandle) {
    let _ = HANDLE.set(handle);
}

fn shown() -> bool {
    HANDLE.get().is_some_and(|h| h.get_webview_window(LABEL).is_some())
}

/// After a change (any thread): the new State to the page, through the main thread, when the window exists.
pub fn emit_from(m: &Mem) {
    if !shown() {
        return;
    }
    let view = m.state();
    post(Inbox::Call(Box::new(move |_| {
        if let Some(h) = HANDLE.get() {
            let _ = h.emit_to(LABEL, EVENT, &view);
        }
    })));
}

/// Tray → What I remember… (or `buddy memory open`): show (making it the first time) and bring it front.
pub fn open() {
    let Some(h) = HANDLE.get().cloned() else { return };
    let h2 = h.clone();
    let _ = h.run_on_main_thread(move || {
        let win = match h2.get_webview_window(LABEL) {
            Some(w) => w,
            None => match WebviewWindowBuilder::new(&h2, LABEL, WebviewUrl::App("memory/index.html".into()))
                .title("What I remember")
                .inner_size(460.0, 720.0)
                .min_inner_size(380.0, 480.0)
                .resizable(true)
                .visible(false)
                .build()
            {
                Ok(w) => {
                    let w2 = w.clone();
                    // the red button hides it (every change was saved as it happened)
                    w.on_window_event(move |e| {
                        if let WindowEvent::CloseRequested { api, .. } = e {
                            api.prevent_close();
                            let _ = w2.hide();
                        }
                    });
                    w
                }
                Err(e) => {
                    eprintln!("[buddy] memory window: {e}");
                    return;
                }
            },
        };
        eprintln!("[buddy] memory: window open");
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
        if let Some(m) = super::get() {
            let _ = win.emit_to(LABEL, EVENT, m.state());
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

/// The caller is the memory window, else `not allowed`.
pub fn mine(label: &str, cmd: &str) -> Result<(), String> {
    if crate::caller::is(label, LABEL, cmd) {
        Ok(())
    } else {
        Err("not allowed".into())
    }
}

fn mem() -> Result<Arc<Mem>, String> {
    super::get().ok_or_else(|| "store".to_string())
}

/// `memory_set_enabled`: the switch, then `settings.memory.enabled` written on the main thread.
pub fn set_enabled(m: &Mem, on: bool) -> Result<Value, String> {
    m.set_enabled(on);
    let (tx, rx) = std::sync::mpsc::channel();
    post(Inbox::Call(Box::new(move |a| {
        a.settings.memory.enabled = on;
        a.save_settings();
        let _ = tx.send(());
    })));
    // the switch already took effect; a slow main thread only delays the file
    let _ = rx.recv_timeout(Duration::from_secs(5));
    Ok(json!({}))
}

// Async: they never run on (or block) the main thread.

#[tauri::command]
pub async fn memory_state(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(w.label(), "memory_state")?;
    Ok(mem()?.state())
}

#[tauri::command]
pub async fn memory_add(w: tauri::WebviewWindow, kind: String, text: String) -> Result<Value, String> {
    mine(w.label(), "memory_add")?;
    mem()?.add(&kind, &text)
}

#[tauri::command]
pub async fn memory_edit(w: tauri::WebviewWindow, id: i64, text: String, value: Option<String>) -> Result<Value, String> {
    mine(w.label(), "memory_edit")?;
    mem()?.edit(id, &text, value.as_deref())
}

#[tauri::command]
pub async fn memory_forget(w: tauri::WebviewWindow, id: i64, hard: bool) -> Result<Value, String> {
    mine(w.label(), "memory_forget")?;
    mem()?.forget(id, hard)
}

#[tauri::command]
pub async fn memory_resolve(w: tauri::WebviewWindow, id: i64, keep: String) -> Result<Value, String> {
    mine(w.label(), "memory_resolve")?;
    mem()?.resolve(id, &keep)
}

#[tauri::command]
pub async fn memory_forget_all(w: tauri::WebviewWindow, confirm: String) -> Result<Value, String> {
    mine(w.label(), "memory_forget_all")?;
    mem()?.forget_all(&confirm)
}

#[tauri::command]
pub async fn memory_set_enabled(w: tauri::WebviewWindow, on: bool) -> Result<Value, String> {
    mine(w.label(), "memory_set_enabled")?;
    let m = mem()?;
    set_enabled(&m, on)
}

#[tauri::command]
pub fn memory_close(w: tauri::WebviewWindow) -> Result<Value, String> {
    mine(w.label(), "memory_close")?;
    hide();
    Ok(json!({}))
}
