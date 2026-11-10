//! Frontmost app → `AppCategory`, from a bundle-id table (local choice; unknown → Other).

use mascot_core::{AppCategory, FrontApp};

/// Exact bundle ids first, then prefixes (`*` suffix).
const TABLE: &[(&str, AppCategory)] = &[
    // code
    ("com.apple.dt.Xcode", AppCategory::Code),
    ("com.microsoft.VSCode", AppCategory::Code),
    ("com.microsoft.VSCodeInsiders", AppCategory::Code),
    ("com.todesktop.230313mzl4w4u92", AppCategory::Code), // Cursor
    ("dev.zed.Zed", AppCategory::Code),
    ("com.sublimetext.4", AppCategory::Code),
    ("com.panic.Nova", AppCategory::Code),
    ("com.jetbrains.*", AppCategory::Code),
    ("com.apple.Terminal", AppCategory::Code),
    ("com.googlecode.iterm2", AppCategory::Code),
    ("dev.warp.Warp-Stable", AppCategory::Code),
    ("com.mitchellh.ghostty", AppCategory::Code),
    ("net.kovidgoyal.kitty", AppCategory::Code),
    ("org.alacritty", AppCategory::Code),
    ("com.github.wez.wezterm", AppCategory::Code),
    // browsers
    ("com.apple.Safari", AppCategory::Browser),
    ("com.google.Chrome*", AppCategory::Browser), // + .beta, .dev, .canary
    ("org.mozilla.firefox", AppCategory::Browser),
    ("com.microsoft.edgemac", AppCategory::Browser),
    ("company.thebrowser.Browser", AppCategory::Browser), // Arc
    ("com.brave.Browser", AppCategory::Browser),
    ("com.vivaldi.Vivaldi", AppCategory::Browser),
    ("com.operasoftware.Opera", AppCategory::Browser),
    // meetings
    ("us.zoom.xos", AppCategory::Meeting),
    ("com.microsoft.teams", AppCategory::Meeting),
    ("com.microsoft.teams2", AppCategory::Meeting),
    ("com.apple.FaceTime", AppCategory::Meeting),
    ("com.cisco.webexmeetingsapp", AppCategory::Meeting),
    ("Cisco-Systems.Spark", AppCategory::Meeting),
    // chat
    ("com.tinyspeck.slackmacgap", AppCategory::Chat),
    ("com.hnc.Discord", AppCategory::Chat),
    ("com.apple.MobileSMS", AppCategory::Chat),
    ("ru.keepcoder.Telegram", AppCategory::Chat),
    ("net.whatsapp.WhatsApp", AppCategory::Chat),
    ("com.apple.mail", AppCategory::Chat),
    // presentations
    ("com.apple.iWork.Keynote", AppCategory::Presentation),
    ("com.microsoft.Powerpoint", AppCategory::Presentation),
    // video
    ("com.apple.TV", AppCategory::Video),
    ("com.apple.QuickTimePlayerX", AppCategory::Video),
    ("org.videolan.vlc", AppCategory::Video),
    ("com.colliderli.iina", AppCategory::Video),
    ("com.netflix.Netflix", AppCategory::Video),
    // games
    ("com.valvesoftware.steam", AppCategory::Game),
    ("com.blizzard.*", AppCategory::Game),
    ("com.riotgames.*", AppCategory::Game),
    ("com.mojang.*", AppCategory::Game),
    ("com.epicgames.*", AppCategory::Game),
    ("com.apple.Chess", AppCategory::Game),
];

pub fn category(bundle_id: &str) -> AppCategory {
    TABLE
        .iter()
        .find(|(k, _)| match k.strip_suffix('*') {
            Some(prefix) => bundle_id.starts_with(prefix),
            None => *k == bundle_id,
        })
        .map_or(AppCategory::Other, |(_, c)| *c)
}

pub fn front_app(bundle_id: &str) -> FrontApp {
    FrontApp { bundle_id: bundle_id.to_string(), category: category(bundle_id) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table() {
        assert_eq!(category("com.microsoft.VSCode"), AppCategory::Code);
        assert_eq!(category("com.jetbrains.rustrover"), AppCategory::Code);
        assert_eq!(category("com.apple.iWork.Keynote"), AppCategory::Presentation);
        assert_eq!(category("com.blizzard.worldofwarcraft"), AppCategory::Game);
        assert_eq!(category("us.zoom.xos"), AppCategory::Meeting);
        assert_eq!(category("com.google.Chrome.beta"), AppCategory::Browser);
        assert_eq!(category("org.example.unknown"), AppCategory::Other);
        assert_eq!(category(""), AppCategory::Other);
        assert_eq!(category("com.apple.Safari.extra"), AppCategory::Other, "exact ids are exact");
    }
}
