# AGENTS.md

**desk-buddy**: an animated desktop assistant character (inspired by the
classic Office Assistant era) that floats over other apps, reacts to events,
and talks through speech bubbles, powered by an LLM.

## Ground rules

- Decisions D1–D16 in the plan (below) are settled. Re-read them before
  proposing anything, and don't reopen one without the user.
- All art comes from Blender, pre-rendered to iso pixel art (D13). Never draw
  sprites in code. Keep the bear cute and toy-like, never realistic bear anatomy.
- Commit and push straight to `main` when asked; no feature branches.
- Use an **original character and art**. Do not copy Clippy or any other
  Microsoft Office Assistant characters, art, sounds or names.
- Never commit secrets (`.env`, API keys); use example placeholders only.

## Project map

- **Architecture and decisions:** [docs/designs/desk-buddy-plan.md](docs/designs/desk-buddy-plan.md).
  Read §1 (runtime split), §3 (mascot contract) and §6 (workspace layout) before
  adding code or crates, and §7 for spike pass criteria and the roadmap (what is done and what comes next).
- **Art and visual design:** the same plan, D2/D13 and §5 (pixel style, iso
  world, shots, Blender → palette → ink-outline pipeline). Read it before any
  mascot, room or UI art change. Pipeline: `experiments/mascot/bear3d/`;
  original reference drawings: `experiments/bear-proto/ref/`.
- **The app (`crates/buddy-app`):** Tauri 2 menu-bar accessory + the native NSPanel bear
  (`panel.rs`, moved from spike S1, which is gone; its source is at `0b1f66e`), driven by
  `mascot-core`'s `Brain` on the main thread (`driver.rs`: one `Inbox` for every other thread),
  drawn by `crates/mascot-render` (pack PNGs → BGRA canvas, headless and unit-tested).
  `cargo run -p buddy-app`; env `BUDDY_HOME` (default `~/.desk-buddy`, holds `settings.json`),
  `BUDDY_PACK=<dir>`, `BUDDY_ZOOM=1|2|3`, `BUDDY_STATS=1` (fps + CPU every 10 s),
  `BUDDY_DUMP=<dir>` (+ `BUDDY_DUMP_EVERY`, canvas PNGs), `BUDDY_EVENTS=<file.jsonl>` (replays
  MascotEvents, e.g. `crates/buddy-app/tests/fixtures/events.jsonl`), `BUDDY_SCREEN`,
  `BUDDY_VERBOSE=1`, `BUDDY_SENSES=0|1` (sensors; off under `BUDDY_EVENTS` unless `=1`),
  `BUDDY_SENSES_LOG=1` (sense changes to stderr). Talk (p3): the speech bubble (`src/bubble/`,
  page `web/bubble/index.html`, `?mock=<state>` in a plain browser) and `harness.rs` (host API at
  `BUDDY_HOME/host.json`, policy, agent "main"); dev credential `BUDDY_FAKE_MODEL=<url>` (debug builds only).
  Accounts (p4): tray **Accounts…** (and the agent line "Connect an AI account…" when signed out) opens `src/signin/` (page `web/signin/index.html`, `?mock=<state>`);
  keys and OAuth tokens live only in the macOS keychain (service `desk-buddy`) and Rust memory, never
  in the sidecar's or a command's env. Unprompted sense turns (`unprompted.rs`): 4 an hour, tray line
  "Check-ins: N of 4 this hour". A second launch on the same `BUDDY_HOME` (lock: `BUDDY_HOME/app.lock`) logs
  `[buddy] already running (pid N)` and exits 0. Debug builds only: `ANTHROPIC_API_KEY` (session-only, never stored),
  `BUDDY_CRED_STORE=memory|fail` (no real keychain), `BUDDY_FAKE_OAUTH=1`, `BUDDY_PROMPT=<text>`,
  `BUDDY_ANSWER=allow|deny`. Real keychain test (prompts): `BUDDY_KEYCHAIN_TEST=1 cargo test -p bridge keychain`.
  SIGTERM/SIGINT quit like the tray's Quit (host.json removed).
  Widgets (p5): `crates/widgets` (manifests `crates/widgets/<id>/widget.json`, the loopback widget server on a random
  port, logged `[buddy] widgets at http://127.0.0.1:<wport>`; pages `<id>/index.html?mock=<state>`) + `src/widgets.rs`
  (windows, run-time capabilities: each widget page gets only its own `widget_*` commands, the browser's page none).
  CLI `buddy widget list|open|close`, `buddy music play|pause|next|status`, `buddy browser goto|snapshot|click|type`
  (browser reads taint the chat; click/type then ask). Body tools: `buddy mascot list|do <activity> [--for 5m]|stop|emote <name>`
  (activities and emotes from the pack's `body` block, `mascots/bear/README.md`; the prompt's "Your body" section is
  `mascot_core::body::prompt_text`; replay `crates/buddy-app/tests/fixtures/activity.jsonl`). Music/Spotify control is AppleScript to an already-running app
  only (`src/media_os.rs`). Edges: drop the bear on a window's top edge (`settings.edges.enabled`), `src/senses/windows.rs`
  polls window bounds only while it sits (never window names). Debug only: `BUDDY_MEDIA_FAKE=1` (local stand-in for
  YouTube). Real-window browser test: `cargo build -p host --bin buddy && cargo test -p buddy-app --test browser_widget -- --ignored`.
  Spike record: `docs/spikes/media.md`.
  Studio (p6): `crates/studio` (headless, pixel-stable: `buddy studio render|sheet|gif|diff|lint|audit|comments|import-aseprite`,
  also the `buddy-studio` binary; `--json` everywhere, exit 0 ok / 1 findings or error / 2 usage) + `src/studio/` (window
  `studio`, page `web/studio/index.html?mock=1|lint-error|empty`, only it may call `studio_*`). Tray **Studio…**, or
  `cargo run -p buddy-app -- --studio [<pack-dir>]` (only the studio window: no lock, settings, host.json or sidecar).
  It only shows and checks art; comments go to `<pack>/review/comments.jsonl`. Human desktop checks: `docs/handover.md`.
  A bad pack or art dir exits 1 with `[buddy] cannot load mascot …`. Art comes
  from `experiments/mascot/out/bear3d/` via `mascots/bear/pack.json`, so re-rendering changes it.
  Always `export CARGO_TARGET_DIR=/Users/chantran/orca/desk-buddy/target` (one shared target;
  a Tauri build needs ~3 GB and the disk is tight).
- **Spike S2 (agent harness):** `agent/` is the Bun-compiled pi-durable 1.5 sidecar
  (`bun run typecheck && bun test`; `bun run build` → `agent/dist/buddy-agent`, ~68 MB).
  The root Cargo workspace holds `crates/mascot-protocol` (MascotEvent), `crates/policy`
  (tool-call rules, approval cards, `policy.json`), `crates/host` (loopback API + the `buddy`
  CLI) and `crates/bridge` (runs the sidecar; its e2e tests need bun), plus the driver
  `spikes/s2-harness` (run it after `cargo build -p host --bin buddy`, which it puts on the
  agent's PATH). Check with `cargo test --workspace`. Model keys stay in Rust memory:
  never put one in the sidecar's or a command's env. pi-durable starts shell commands detached,
  so a killed sidecar can leave them running (tests reap them).
- **Deployment / release:** N/A for now. Nothing is distributed yet (Cargo
  `publish = false`, Tauri bundling off); D8 says it will be, so add a release
  profile when the app crate exists.

## Mascot pipeline (`experiments/mascot/bear3d/`)

- Blender 5.0 headless: `/Applications/Blender.app/Contents/MacOS/Blender -b -P <script>.py` (override with `BLENDER=`).
- Build order, each step needing the previous one's .blend:
  `sh run.sh` (model → `out/bear3d/cute/`, ~40 s) → `sh run_anim.sh` (Stage A → `out/bear3d/anim/`, ~5 min)
  → `sh run_b.sh` (Stage B → `out/bear3d/anim_b/` + `props/`).
  Stage C (needs `bear.blend` + `out/bear3d/anim/`): `sh run_c.sh` (`rig_bear.py --stage c` → `bear_rig_c.blend` +
  `clips_c.json`; think/talk/ask loop + oops/happy/celebrate one-shot in Stage A's 84×123 frame → `out/bear3d/anim_c/`,
  ~7 min) → `sh run_face.sh` (D16 face layers → `out/bear3d/face/`, ~40 s; ends with `verify_face.py`).
- `python3 verify_b.py`: Stage B hand-off frames must be pixel-identical to props + the idle sprite; run it after any re-render.
- `python3 verify_face.py` (`--self-test`: two in-memory mutations, each must be caught, exit 1): the face invariant
  "T" (amends design §3.4). Default patches drawn at each frame's position must equal the expected frame (the anim_c
  frame; for idle the Blender reference `face/ref/idle_<dir>.png`, rigid face, no baked blink) exactly on idle's rest
  frames and on Stage C frames with arms and squash at rest (all hand-offs). Elsewhere they may differ by ≤ 16 px per
  frame, only inside the masks + 1 px, only ink/fur/blush outline or shade flips, no cluster of 5+ px. Paws must never
  cross the face in S/SE/E/W/SW. walk*/run* have no face. Stage C has no roll, lean or squash (face bones shift by
  whole pixels). It prints a residual table and writes `face/residual_worst.png`. Stage A/B/props bytes never change.
- Pixel rules: iso 2:1 ortho camera, 8 directions, 12 fps, ppu 40.07, alpha 0/255 only, 1px ink outline, palette =
  `bear-proto/palette-bear.json` + `scene-proto/palette.json` (+ `bear3d/palette-ext.json` for furniture).
- Preview: `python3 -m http.server 8771 --directory experiments/mascot/out`, then open
  `bear3d/anim/preview.html`, `bear3d/anim_b/preview_b.html` or `bear3d/anim_c/preview_c.html` (Stage C + face sheets).

## Gotchas

- Never screenshot the desktop: it can capture the user's private windows. Check sprites with Pillow contact sheets instead.
- Subagents can't write `.md` files here; have them return reports as text.
- `timeout` isn't installed (macOS).

## 2build

This repo is configured for 2build autonomous runs.

- Git: autopilot leaves work uncommitted; the lead commits once per phase and lands it on the base branch
- QA harness: `.babysit/qa.yaml`. This is a native desktop app with no web target: checks are `cargo test -p mascot-render -p buddy-app` (plus `cargo test --workspace` per phase) and `python3 verify_b.py` in `experiments/mascot/bear3d`; mascot visuals are judged by the user on the real desktop.
- Browser: only for the HTML sprite previews (`experiments/mascot/out/bear3d/**/preview*.html`); invoke `/bbs:browse` (or `/bbs:qa` for a full loop). These drive a real Chromium via `agent-browser`; there is no separate browser *tool* to look for, and `WebFetch` is not a substitute. It cannot drive the Tauri/NSPanel windows. One-time: `npm install -g agent-browser cloakbrowser`.
- Browser hygiene: automation uses its own browser (Chrome for Testing, Playwright or cloakbrowser), never the installed Chrome. End every browser run with the tool's own close (`agent-browser --session "$AGENT_BROWSER_SESSION" close`), never `kill`/`pkill`/`close --all`, and check what still uses a stale browser or profile clone before reaping it. See `/bbs:browse` § Browser hygiene.
- Default run: `/goal "STATUS: DONE or STATUS: BLOCKED appears" /bbs:autopilot "<task>"`
- Search: use `rg`, never `grep`. `rg -uu` includes gitignored and hidden files; skip noise with `-g '!node_modules' -g '!.git'`. List files with `rg --files -uu`.

QA must prove the local target or name the blocker, and must include at least one non-happy-path case before PASS.
