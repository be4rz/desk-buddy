# Media spike (p5, AC9)

Does a YouTube IFrame embed play in a Tauri webview window served from the loopback widget
server, and does it keep playing while the window is hidden? Run 2026-11-10 on macOS 15.7.4
(Apple silicon), Tauri 2.12.4 / wry 0.57.1, debug build of `buddy-app`, temp `BUDDY_HOME`,
`BUDDY_SENSES=0`, `BUDDY_CRED_STORE=memory`. Each run was stopped with SIGTERM via `trap`.

## Setup

- The media window loads `http://127.0.0.1:<wport>/media/index.html` (`crates/widgets`, tiny_http,
  loopback only, Host must be exactly `127.0.0.1:<wport>`).
- Its `WKWebViewConfiguration` sets `mediaTypesRequiringUserActionForPlayback = None` (autoplay
  with no click), and the window has `background_throttling(Disabled)`.
- The page loads `https://www.youtube.com/iframe_api` and plays with `playerVars.origin` = the
  widget origin. Status comes from `buddy music status --json` (the page's `getCurrentTime`).
- Hiding used the manager's `visibility("media", "hide")` (what the page's hide button calls),
  through a temporary probe 8 s after load; the probe is gone.

## Results

| Question | Answer | Evidence |
|---|---|---|
| Embed loads at `http://127.0.0.1:<wport>` | **yes** | `M7lc1UVf-VE` (YouTube's IFrame API sample) played: `playing:true`, title from the player, duration 1343.7 |
| Audio while hidden (`NSWindow` ordered out) | **yes** | real embed: `time` 5.1 (shown) → 12.2, 17.4, 22.5, 27.7 over 20 s hidden, `document.hidden: true` |
| Fallback used | **none for hiding**; error 150 → browser widget | `park_offscreen` (move the window off-screen instead of ordering it out) is kept as a manifest option, off for media |
| Embed blocked (error 150) | handled | `dQw4w9WgXcQ` fails with 150 at loopback; the reply is `openedIn: "browser"` with the watch URL and "This video can't play here (error 150); opened it in the browser widget." |

## Findings

- **Muted media stops while hidden; audible media doesn't.** The first fake player (a looping
  silent WAV at `volume = 0`) froze at 7.6 s as soon as the window was hidden, both ordered out and
  parked off-screen. At `volume = 1` (the WAV is still silent) it kept going in both cases. WebKit
  pauses invisible media it considers muted. So a real song keeps playing, and the fake keeps an
  unmuted, silent element so the fake test measures the same thing.
- **JS keeps running while hidden** (status answered every 5 s), thanks to background throttling
  being off.
- **The IPC check sees only the page's origin.** Tauri checks a remote capability against
  `http://127.0.0.1:<wport>/`, not the page path, so a path-scoped pattern (`…/media/*`) refused
  every command. Each widget's capability is now the widget origin plus its own webview label; the
  path is still enforced by the navigation filter and by each command's `is_widget_page` check.
- Some videos refuse embedding even at a public origin; error 150 is not a loopback artefact
  (the sample video plays from the same page).

## Left for the person (AC13)

- Listening: real audio was not judged by ear here; play a song, hide the widget, check it keeps
  sounding.
- Music/Spotify control (`buddy music pause|next|status` with no widget playing) sends AppleScript
  to an already-running app only; the first use shows macOS's Automation prompt.
