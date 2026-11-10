//! The app manifest lists every app command, so Tauri's ACL scopes them: a window may call only
//! the commands its capability grants (`capabilities/*.json`; widget windows get theirs at run
//! time, `src/widgets.rs`). Each command also checks its caller (`src/caller.rs`).
const BUBBLE: [&str; 6] = ["prompt_submit", "card_answer", "bubble_dismiss", "bubble_size", "bubble_retry", "reminder_snooze"];
const SIGNIN: [&str; 8] = ["signin_state", "signin_save_key", "signin_start", "signin_answer", "signin_cancel", "signin_remove", "signin_reopen", "signin_close"];
const WIDGET: [&str; 6] = ["widget_ready", "widget_reply", "widget_state", "widget_visibility", "widget_control", "widget_drag"];
const STUDIO: [&str; 10] = ["studio_state", "studio_poll", "studio_open", "studio_frames", "studio_lint", "studio_audit", "studio_comments", "studio_comment_add", "studio_comment_status", "studio_sim"];

fn main() {
    let all: &'static [&'static str] = Box::leak(BUBBLE.iter().chain(SIGNIN.iter()).chain(WIDGET.iter()).chain(STUDIO.iter()).copied().collect::<Vec<_>>().into_boxed_slice());
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(all))).expect("tauri build");
}
