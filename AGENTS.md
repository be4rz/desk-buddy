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
- **Spike S1 (macOS window):** `spikes/s1-window/` (Rust, Tauri 2 + NSPanel).
  Run with `cargo run` there; env switches `S1_MODE=idle|walk|run`, `S1_ZOOM`,
  `S1_SIZE=64`, `S1_STATS=1`, `S1_VERBOSE=1`. It loads sprites from
  `experiments/mascot/out/bear3d/anim/`, so re-rendering Stage A changes what
  it shows. Its `target/` grows to ~3 GB and the disk is tight.
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
- `python3 verify_b.py`: Stage B hand-off frames must be pixel-identical to props + the idle sprite; run it after any re-render.
- Pixel rules: iso 2:1 ortho camera, 8 directions, 12 fps, ppu 40.07, alpha 0/255 only, 1px ink outline, palette =
  `bear-proto/palette-bear.json` + `scene-proto/palette.json` (+ `bear3d/palette-ext.json` for furniture).
- Preview: `python3 -m http.server 8771 --directory experiments/mascot/out`, then open
  `bear3d/anim/preview.html` or `bear3d/anim_b/preview_b.html`.

## Gotchas

- Never screenshot the desktop: it can capture the user's private windows. Check sprites with Pillow contact sheets instead.
- Subagents can't write `.md` files here; have them return reports as text.
- `timeout` isn't installed (macOS).

## 2build

This repo is configured for 2build autonomous runs.

- Git: autopilot leaves work uncommitted; the lead commits once per phase and lands it on the base branch
- QA harness: `.babysit/qa.yaml`. This is a native desktop app with no web target: checks are `cargo check && cargo test` in `spikes/s1-window` and `python3 verify_b.py` in `experiments/mascot/bear3d`; mascot visuals are judged by the user on the real desktop.
- Browser: only for the HTML sprite previews (`experiments/mascot/out/bear3d/**/preview*.html`); invoke `/bbs:browse` (or `/bbs:qa` for a full loop). These drive a real Chromium via `agent-browser`; there is no separate browser *tool* to look for, and `WebFetch` is not a substitute. It cannot drive the Tauri/NSPanel windows. One-time: `npm install -g agent-browser cloakbrowser`.
- Browser hygiene: automation uses its own browser (Chrome for Testing, Playwright or cloakbrowser), never the installed Chrome. End every browser run with the tool's own close (`agent-browser --session "$AGENT_BROWSER_SESSION" close`), never `kill`/`pkill`/`close --all`, and check what still uses a stale browser or profile clone before reaping it. See `/bbs:browse` § Browser hygiene.
- Default run: `/goal "STATUS: DONE or STATUS: BLOCKED appears" /bbs:autopilot "<task>"`
- Search: use `rg`, never `grep`. `rg -uu` includes gitignored and hidden files; skip noise with `-g '!node_modules' -g '!.git'`. List files with `rg --files -uu`.

QA must prove the local target or name the blocker, and must include at least one non-happy-path case before PASS.
