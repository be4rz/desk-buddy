//! desk-buddy widgets (design §4.9, `## widgets`): small web pages in their own windows that the
//! agent (and the person) drive with `buddy <widget> <action>`. This crate is pure Rust and headless:
//! `widget.json` (`manifest`), the first-party widgets' files embedded in the binary (`ASSETS`), the
//! loopback static server they're served from (`server`), the `Manager` that routes actions to a
//! widget page and waits for its reply (`manager`), and YouTube query resolution (`media`). The app
//! (`buddy-app/src/widgets.rs`) implements `manager::Shell`: windows, IPC, evaluateJavaScript.

pub mod manager;
pub mod manifest;
pub mod media;
pub mod server;

pub use manager::{Done, Manager, Shell, WidgetError};
pub use manifest::Manifest;

/// One embedded file, served at `/<path>`.
pub struct Asset {
    pub path: &'static str,
    pub bytes: &'static [u8],
}

macro_rules! asset {
    ($p:literal) => {
        Asset { path: $p, bytes: include_bytes!(concat!("../", $p)) }
    };
}

/// Every file the widget server serves.
pub const ASSETS: &[Asset] = &[
    asset!("sdk/buddy-widget.js"),
    asset!("sdk/widget.css"),
    asset!("media/widget.json"),
    asset!("media/index.html"),
    asset!("media/media.css"),
    asset!("media/media.js"),
    asset!("browser/widget.json"),
    asset!("browser/index.html"),
    asset!("browser/browser.css"),
    asset!("browser/browser.js"),
];

/// Debug builds only: the local YouTube stand-in.
#[cfg(debug_assertions)]
pub const DEBUG_ASSETS: &[Asset] = &[asset!("media/fake-yt.js")];
#[cfg(not(debug_assertions))]
pub const DEBUG_ASSETS: &[Asset] = &[];

/// The script the app evaluates in the browser widget's page (never served).
pub const INJECT_JS: &str = include_str!("../browser/inject.js");

/// The first-party widgets' manifests.
pub fn builtin() -> Vec<Manifest> {
    ["media/widget.json", "browser/widget.json"]
        .iter()
        .map(|p| {
            let a = asset(&format!("/{p}")).expect("embedded widget.json");
            Manifest::parse(a.bytes).unwrap_or_else(|e| panic!("{p}: {e}"))
        })
        .collect()
}

/// The embedded file at URL path `path` (`/media/index.html`).
pub fn asset(path: &str) -> Option<&'static Asset> {
    let p = path.strip_prefix('/')?;
    ASSETS.iter().chain(DEBUG_ASSETS.iter()).find(|a| a.path == p)
}

/// The browser script for one op (`{op:"snapshot"}` …), ready to evaluate.
pub fn inject_script(op: &serde_json::Value) -> String {
    INJECT_JS.replace("__BUDDY_OP__", &op.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_widgets_parse() {
        let all = builtin();
        let ids: Vec<_> = all.iter().map(|m| (m.id.as_str(), m.cli.as_str())).collect();
        assert_eq!(ids, [("media", "music"), ("browser", "browser")]);
        for m in &all {
            assert!(asset(&format!("/{}/{}", m.id, m.entry)).is_some(), "{} entry", m.id);
        }
    }

    #[test]
    fn the_inject_script_takes_the_op() {
        let s = inject_script(&serde_json::json!({"op": "click", "ref": "e2"}));
        assert!(!s.contains("__BUDDY_OP__"));
        assert!(s.contains(r#"({"op":"click","ref":"e2"})"#));
    }
}
