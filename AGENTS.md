# AGENTS.md

**desk-buddy** (product name **2poke**): an animated desktop assistant character (inspired by the
classic Office Assistant era) that floats over other apps, reacts to events,
and talks through speech bubbles, powered by an LLM. The repo is public and open source.

## Ground rules

- Decisions D1–D16 in the plan (below) are settled. Re-read them before
  proposing anything, and don't reopen one without the user.
- All art comes from Blender, pre-rendered to iso pixel art (D13). Never draw
  sprites in code. Keep the bear cute and toy-like, never realistic bear anatomy.
- Commit and push straight to `main` when asked; no feature branches.
- Use an **original character and art**. Do not copy Clippy or any other
  Microsoft Office Assistant characters, art, sounds or names.
- Never commit secrets (`.env`, API keys); use example placeholders only.
- Licenses: code is MIT (`LICENSE`), the bear and every image or `.blend`/`.aseprite` file is
  CC BY-NC 4.0 (`ARTWORK.md`). Add third-party code, fonts or images only under a compatible license,
  with its license file beside it (e.g. `site/assets/fonts/OFL.txt`). Everything committed is public,
  history included: no personal paths, names or private notes.

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
  `BUDDY_SENSES_LOG=1` (sense changes to stderr; never the cursor). Reactions (A6 p4): the pack's `contexts` + `reactions`
  (`mascots/bear/README.md` § reactions); the cursor sense (`src/senses/cursor.rs`) is the panel's existing near-poll only
  (no new permission), feet-relative, 8 pt, ≤ 10 Hz, to the brain alone (never logged, prompted or stored); replays
  `crates/buddy-app/tests/fixtures/{music,edge_react,drag,cursor,time,away,approval_wait}.jsonl`. Talk (p3): the speech bubble (`src/bubble/`,
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
  Tray settings: **Hide over fullscreen apps** (`dnd.fullscreen`, off by default; only a fullscreen app on the bear's own
  display counts; settings.json is v2, a v1 file is read with it off once), **Model ▸** (the signed-in providers' models,
  listed by the sidecar's `models` command; writes `settings.model`), **Approvals ▸ Full access** (policy mode `full`:
  everything runs except ask rules, `policy::rules::attention` commands and `sensitive_path` files; deny rules still
  deny, and it runs as `allowlist` while the chat is marked by the web).
  Personality (A7, D17): the pack's `persona` presets (`mascots/bear/README.md` § personality; Biscuit is the bear's
  default) + the person's `settings.persona` (settings.json v3) → `mascot_core::persona` (check, resolve, `fill`,
  `prompt_text` = the prompt's "Who you are" section, sent as `Init.persona` / `set_persona`, no restart). Tray
  **Personality ▸** (`persona:<id>`, `persona_customize`) and the `persona` window (`src/persona/`, page
  `web/persona/index.html?mock=<state>`, only it may call `persona_*`; `web/persona/fill.js` must match the Rust fill,
  fixture `crates/mascot-core/tests/fixtures/persona_fill.json`). Canned idle lines `src/idle_lines.rs` (no model call,
  outside the check-in budget, log `[buddy] idle line <moment>` only). Tone only: the agent has no command to change it
  and `crates/policy` never reads it. `buddy studio persona render --pack <dir> [--preset <id>]`, `audit --preset <id>`;
  live eval `BUDDY_EVAL=1 bun run agent/scripts/persona-eval.ts` (real key, never in CI).
  Rooms (floors and pocket scenes, D13): a room pack is `rooms/<id>/room.json` + PNGs, mascot-independent
  (`rooms/README.md`; loader `mascot_core::room`, drawn by `mascot-render` via `Art::load_with`). `settings.room` (default
  `starter`), `BUDDY_ROOM=<dir>` overrides; a broken room exits 1 with `[buddy] cannot load room …`, none at all runs with
  the grid only. Tray **Floor ▸** (Grid, None, then the room's floors; `settings.floor`) and **Scenes around activities**
  (`settings.scenes`, on by default): a scene (`for` = behavior or set names; furniture and decorations only, no floor or walls of its own) is
  drawn over the selected floor while that behavior plays, mirrored for set placement B, its timed variant picked by the local hour. Room pixels are composed at alpha 254,
  so they never take clicks (the hit mask is alpha 255 only). The canvas grows to hold the room art; a saved
  `position` carries `feet` so the bear stays put when the canvas changes. Replay
  `crates/buddy-app/tests/fixtures/scenes.jsonl`. Art: `experiments/env-proto/` (`sh build.sh`, ~2 min: renders,
  pixelizes, writes `rooms/starter/` and verifies it, byte-identical each run; `python3 verify_env.py [--room DIR]` checks palette, alpha 0/255, edges and hours;
  `--self-test`, exit 1 = works; `python3 review.py` → `out/review/index.html`).
  Widgets (p5): `crates/widgets` (manifests `crates/widgets/<id>/widget.json`, the loopback widget server on a random
  port, logged `[buddy] widgets at http://127.0.0.1:<wport>`; pages `<id>/index.html?mock=<state>`) + `src/widgets.rs`
  (windows, run-time capabilities: each widget page gets only its own `widget_*` commands, the browser's page none).
  CLI `buddy widget list|open|close`, `buddy music play|pause|next|status`, `buddy browser goto|snapshot|click|type`
  (browser reads taint the chat; click/type then ask). Body tools: `buddy mascot list|do <activity> [--for 5m]|stop|emote <name>`
  (activities and emotes from the pack's `body` block, `mascots/bear/README.md`; the prompt's "Your body" section is
  `mascot_core::body::prompt_text`; replay `crates/buddy-app/tests/fixtures/activity.jsonl`, gestures `emotes.jsonl`). Music/Spotify control is AppleScript to an already-running app
  only (`src/media_os.rs`). Edges: drop the bear on a window's top edge (`settings.edges.enabled`), `src/senses/windows.rs`
  polls window bounds only while it sits (never window names). Debug only: `BUDDY_MEDIA_FAKE=1` (local stand-in for
  YouTube). Real-window browser test: `cargo build -p host --bin buddy && cargo test -p buddy-app --test browser_widget -- --ignored`.
  Spike record: `docs/spikes/media.md`.
  Screen awareness (D18): activity sense `src/senses/activity.rs` (pure RAM log of front-app display names + minutes, 30 min;
  moments `long_dwell` `flip_loop` `late_hour` become check-ins in `unprompted.rs`, `back_after_away` enriches `back_from_idle`;
  names ride `SensesPatch.front_name`, taken out in the driver: never the brain, ring, log or disk; tray **Activity awareness**,
  `settings.activity.enabled`, settings.json v4; debug `BUDDY_ACTIVITY_SCALE=<n>`; `BUDDY_SENSES_LOG=1` prints kinds only;
  replay `crates/buddy-app/tests/fixtures/activity_sense.jsonl`). Screen look `src/screen/` (ScreenCaptureKit + Vision OCR behind
  `Capture`/`Ocr` traits with fakes; tests never call the real `grab`): `buddy screen look [--json]` (text only; `--image` is a
  usage error) and the sidecar's typed `screen_look` tool (`agent/src/ext/screen/`, picture for the asking turn only,
  `replay: "unsafe"`), host `POST /v1/screen/look`; tray **Screen look** (off; asks for Screen Recording when ticked),
  `settings.screen_look.{enabled,deny}`; every use asks, Full access too, denied in sense turns, taints the chat (source `screen`),
  the bear peeks first; skill `agent/skills/screen/SKILL.md`. Debug only: `BUDDY_SCREEN_FAKE=1`. Desktop checks: handover §9.
  Studio (p6): `crates/studio` (headless, pixel-stable: `buddy studio render|sheet|gif|diff|lint|audit|comments|import-aseprite`,
  also the `buddy-studio` binary; `--json` everywhere, exit 0 ok / 1 findings or error / 2 usage;
  `audit --pack <dir> [--minutes 10] [--seed N | --seeds 1,2,3] [--start 2026-11-16T10:00] [--temperament energy=1.0,…]`
  exits 1 when a run breaks the pack's `variety` rules or targets: the bear's gate is `--minutes 10` and `30` on seeds
  1,2,3; `package check <dir>` / `package bundle --pack <dir> --out <dir>/<id>`: the package standard,
  `mascots/README.md`) + `src/studio/` (window `studio`, page `web/studio/index.html?mock=<state>`: `1 lint-error
  customized library new customize delete restore edit edit-clips edit-behaviors edit-intents edit-idle edit-package
  edit-advanced edit-error`; only it may call `studio_*`, and the page never sends a path). Tray **Studio…**, or
  `cargo run -p buddy-app -- --studio [<pack-dir>]` (only the studio window: no lock, host.json or sidecar; no pack =
  the library view). The studio is the mascot library (`BUDDY_HOME/packs/<id>/`, searched before the built-ins by the
  app too): create (from `.aseprite` or a copy), edit (details, clips, behaviors, intents, idle pool, raw pack.json;
  every save is staged and loaded first, a breaking one is refused and writes nothing), delete (yours only), Customize
  / Restore original for built-ins (the shipped folder is never written), "Put on desktop" (settings `pack`, next
  launch; a settings pack found nowhere runs the bear). Comments go to `<pack>/review/comments.jsonl`. Human desktop
  checks: `docs/handover.md`.
  A bad pack or art dir exits 1 with `[buddy] cannot load mascot …`. Art comes
  from `experiments/mascot/out/bear3d/` via `mascots/bear/pack.json`, so re-rendering changes it.
  Always `export CARGO_TARGET_DIR="$(git rev-parse --path-format=absolute --git-common-dir)/../target"` (one shared target;
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
  Stage L (needs `bear.blend`, `out/bear3d/anim/` and `face/`): `sh run_l.sh` (`rig_bear.py --stage l`: Stage C rig +
  a `head` bone twisting the whole round head mass, rig in a temp dir → `clips_l.json`; listen_in/listen/listen_out for
  S SE E W SW (head turns 45° toward the camera) + turn_ccw/turn_cw for 8 dirs → `out/bear3d/anim_l/`, then
  `face_l.py` makes `face/face.json` v2 (additive: `clips.listen` + `patch_dirs`), ~2 min). `python3 verify_l.py`
  (`--self-test`, exit 1 = works): hand-offs = idle frame 0, alpha, palette, 1 px ink outline. Sheet: `anim_l/sheet_listen.png`.
  `run_c.sh` removes face/; `run_face.sh` rewrites face.json as v1 and then runs `face_l.py` and `face_d.py` itself.
  Stage D (needs `bear.blend`, `anim/` and face.json v2): `sh run_d.sh` (Stage L rig in a temp dir; desk set = low desk,
  computer + keyboard, swivel chair; clips `clips_d.py` desk_sit/type/swivel_in/talk/ask/swivel_out/stand, ori A types NE,
  ori B = 3D mirror types NW; render_d → pixelize_d → make_d → `out/bear3d/anim_d/anim_d.json` + `props_d/props_d.json`,
  then `face_d.py` adds `set_clips` to face.json (additive; seated talk/ask draw patch S), `verify_d.py` + `verify_face.py`,
  ~3 min). `python3 verify_d.py` (`--self-test`, exit 1 = works): hand-offs = props + idle frame 0, every join, loop seams,
  alpha, palette, 1 px ink outline. Sheet: `anim_d/sheet_desk.png`.
- Stage kit (`stagekit.py`, docs in its header: #stage-manifest, #anim-stage, #face-part, #verify-stage, #sha-freeze): new
  stages are a manifest `stages/<id>.json` (`id`, `kind` set|clips, `module`, `verify`) + a pure-Python module, rendered by
  `sh run_stage.sh <id> [WORKDIR] [--keep] [--resume]` (rig `rig_bear.py --stage k --clips-module <m>` in WORKDIR, Stage L rig +
  hp ±20°, hr ±15°, mouth closed/open/wide/tongue, ears ±15°, paw sockets, `add_objects(kit)`; sets: render_set →
  pixelize_set → make_set; clips: render_anim → pixelize_set → make_stage). Output `out/bear3d/set_<id>/` or
  `anim_<id>/`, swapped in only after `verify_<id>.py --dir <new>` and `verify_face.py --part <new face part>` pass; the
  failed run keeps WORKDIR for `--resume` and leaves the out dir as it was. Renders take one of 2 machine-wide slots
  (`$TMPDIR/bear3d_render.slot0|1` + pid; a third run waits, a dead pid's slot is reclaimed). `BEAR3D_STAGES` /
  `BEAR3D_OUT` / `BEAR3D_LOCK_DIR` point it elsewhere (tests). Clip stages write root motion in two forms,
  `frames[i].root_px[dir]` and `root_px[dir]` (the same list): they are ONE track, a loader applies it once (the
  verifier fails if they differ). Face parts `out/bear3d/*/face_<id>.json` are separate files (face.json is never
  edited by the kit); verify_face.py reads them all, and a clip in face.json and a part, or in two parts, is an error.
  `python3 stagekit.py --self-test` (exit 1 = works), `sheet|preview <id>`, `freeze`, `check <sha> [--own <dir>…]`.
  A clip may set `frame` [W,H] / `anchor` [W/2,y] (default 84×123 [42,107]; `run_stage.sh` passes `--canvas W,H,y` per clip);
  `handoff` ends may be `idle0` or `<Stage A clip>#<frame>` (e.g. `run_stop#0`); Rig K `tumble` (deg, about the body
  centre) and `lift` (world z), plus lean/roll up to ±90, are allowed only for face baked/none clips.
  Freeze: `stages/freeze_p1.sha` = sha256 of every file in `anim anim_b anim_c anim_l anim_d props props_d cute face
  set_lean`; `python3 stagekit.py check stages/freeze_p1.sha` must be clean (a later stage passes `--own <its dir>`;
  the one allowed extra is the p5 index `out/bear3d/sheets.html`, shown as `added`).
  Stage lean (`stages/lean.json`, `set_lean.py`): `sh run_stage.sh lean` (~1.5 min) → `out/bear3d/set_lean/`:
  `anim_lean.json` (Stage B format), `props_lean.json` (Stage B props format), `face_lean.json` (set_clips `lean`, patch
  SE / SW, + exact frames), `sheet_lean.png`, `preview_lean.html`. lean_in 10 → lean 30 (loop) → lean_out 10; a side lean
  (a back lean hides the block behind the round body): ori A faces SE with the left mitten on a toy block's top (NE on
  screen), ori B = 3D mirror (SW). Canvas 108×116. `python3 verify_lean.py` (`--self-test`, exit 1 = works): the kit's
  set checks + the block never moves + art never touches the canvas edge.
  The A4 clip stages (p2) are `kind` clips handing off at idle#0 (except where Stage t says otherwise) and render with
  `sh run_stage.sh <id>` → `out/bear3d/anim_<id>/` (`anim_<id>.json`, strips + gifs, `sheet_<id>.png` (too big for Pillow's
  default limit: read strips), `preview_<id>.html`); each `python3 verify_<id>.py` (`--self-test`, exit 1 = works) runs the
  kit checks plus its own. Freeze check after p2: `check stages/freeze_p1.sha --own anim_f anim_g anim_p anim_m anim_t`.
  Stage f (`stages/f.json`, `clips_f.py`, fidgets): 8 one-shots 84×123, S SE E W SW (`fid_butt_wiggle` N NE NW): weight_shift,
  ear_twitch, tummy_rub, wave_small (face layers, `face_f.json`), look_up, look_down, peek (baked), butt_wiggle (none);
  `verify_f.py`: no root motion, art off the canvas edge, the face part covers exactly the layers clips.
  Stage g (`stages/g.json`, `clips_g.py`, fidgets): 8 baked one-shots 84×123, S SE E W SW: rock_heels, head_tilt, scratch,
  stretch, yawn, shake_off, sneeze, nod_off; no face part; `verify_g.py`: no root motion, idle0 both ends.
  Stage p (`stages/p.json`, `clips_p.py`, floor postures): 16 clips in S SE E W SW, 132×123 [66,107] (squat 84×123):
  down → loop → up for sit_floor (layers, `face_p.json`), sit_sway (chains on sit_floor_down/up), belly, back, curl (nap),
  squat; `verify_p.py`: down[last] = loop[0] = up[0], loop seams.
  Stage m (`stages/m.json`, `clips_m.py`, gaits): tiptoe, skip, shuffle, back_waddle as start → loop → stop in 8 dirs,
  84×123, face none, root motion like Stage A's walk; `verify_m.py`: planted feet never slide (≤ 0.5 px), travel sign
  (back_waddle moves opposite to the facing).
  Stage t (`stages/t.json`, `clips_t.py`, tumbles): `sh run_stage.sh t` (~25 min, 848 frames), 6 one-shots in 8 dirs,
  face none: hop, spin 84×123; roll, trip 132×135; pounce, slide_stop 132×123 (anchors [W/2,107]); pounce and slide_stop
  start at Stage A `run_stop#0` (chase = run_start → run → pounce; slide = … → slide_stop). `python3 verify_t.py`: kit
  checks + forward/in-place root tracks + art ≥ 2 px from every canvas edge.
  The A5 gesture stages (p3), `kind` clips, one-shots in S SE E W SW, 84×123, idle#0 both ends: Stage e (`stages/e.json`,
  `clips_e.py`): wave, nod, shake, greet, sad, surprised, jump, dodge, remind (wave/remind layers, `face_e.json`; dodge
  side-steps by root_px and ends displaced); Stage x (`stages/x.json`, `clips_x.py`): shrug, facepalm, thumbs_up, clap,
  point_bubble, shy_hide, laugh, pout, proud, head_scratch (shrug/thumbs_up/point_bubble/proud layers, `face_x.json`).
  `python3 verify_e.py` / `verify_x.py` (`--self-test`, exit 1 = works). Freeze check after p3: add `--own anim_e anim_x`.
  The pack wires them as the 11 intents' own clips and 16 gesture emotes (`mascots/bear/README.md` § gestures).
  The A6 stages: p4 clip stages r, s (reactions, 84×123) and w (window edge, 84×139 [42,107], legs hang below the anchor),
  `sh run_stage.sh r|s|w` → `anim_<id>/`; p5 set stages (`kind` set, ori A faces SE, B = 3D mirror SW, canvases ≤ 176×160,
  each set enter → loop → exit, hand-offs = props + idle#0): sit_a (`set_sit_a.py`: tea, snack, knit on a floor cushion),
  sit_b (`set_sit_b.py`: notebook and laptop on a low toy tray-table, headphones `phones`, umbrella with baked rain),
  play (`set_play.py`: ball, bubbles, yoyo, blocks, tethered balloon), out (`set_out.py`: plant, sweep, snowman, fishing,
  `bed_cap` = the nightcap bed; its colours are `palette-ext-out.json`, listed in pack.json `palettes`), each
  `sh run_stage.sh <id>` → `set_<id>/` (`anim_<id>.json`, `props_<id>.json`, `face_<id>.json`, `sheet_<id>.png`,
  `preview_<id>.html`) and `python3 verify_<id>.py` (`--self-test`, exit 1 = works); p5 clip stage z (`clips_z.py`, rare
  surprises + four seasonal hats, F5 84×123, moonwalk 8 dirs) → `anim_z/`. Freeze check after p5: `check
  stages/freeze_p1.sha --own anim_f anim_g anim_p anim_m anim_t anim_e anim_x anim_r anim_s anim_w set_sit_a set_sit_b
  set_play set_out anim_z`; it reports `added: sheets.html` (the index below; `--own` takes dirs only), nothing else. The pack wires them as idle `long` / `rare` entries, activities and `surprise` emotes
  (`mascots/bear/README.md` § props and surprises). Every sheet and preview of the run: `out/bear3d/sheets.html`
  (`python3 make_sheets.py` rewrites it).
- `python3 verify_b.py`: Stage B hand-off frames must be pixel-identical to props + the idle sprite; run it after any re-render.
- `python3 verify_face.py [--part FILE]` (`--self-test`: in-memory mutations (3 with face.json v2, 4 with `set_clips`, 5 with a face part), each must be caught, exit 1): the face invariant
  "T" (amends design §3.4). Default patches drawn at each frame's position must equal the expected frame (the anim_c
  frame; for idle the Blender reference `face/ref/idle_<dir>.png`, rigid face, no baked blink) exactly on idle's rest
  frames and on Stage C frames with arms and squash at rest (all hand-offs). Elsewhere they may differ by ≤ 16 px per
  frame, only inside the masks + 1 px, only ink/fur/blush outline or shade flips, no cluster of 5+ px. Paws must never
  cross the face in S/SE/E/W/SW. walk*/run* have no face. Stage C has no roll, lean or squash (face bones shift by
  whole pixels). It prints a residual table and writes `face/residual_worst.png`. Stage A/B/props bytes never change.
- Legs: the leg stubs are part of the body mesh; the white pads (fL/fR/hL/hR) are only for steps. Sitting, lying and
  dangling poses swing the real legs with `legL` / `legR` (deg, + = forward, hip joints in `rig_bear.py` LEGS: always in
  Stage K, Stage D via `--legs`, Stage B in `render_b.py`) and keep the pads at 0, never a far fL/hL (it reads as a second,
  detached pair of legs). The shared floor sit is `anim_clips.SIT_LEG` / `SIT_BOB`.
- Pixel rules: iso 2:1 ortho camera, 8 directions, 12 fps, ppu 40.07, alpha 0/255 only, 1px ink outline, palette =
  `bear-proto/palette-bear.json` + `scene-proto/palette.json` (+ `bear3d/palette-ext.json` for furniture).
- Preview: `python3 -m http.server 8771 --directory experiments/mascot/out`, then open
  `bear3d/anim/preview.html`, `bear3d/anim_b/preview_b.html` or `bear3d/anim_c/preview_c.html` (Stage C + face sheets) or `bear3d/anim_l/preview_l.html` (listen + turns) or `bear3d/anim_d/preview_d.html` (desk set) or `bear3d/set_lean/preview_lean.html` (lean) or `bear3d/anim_<f|g|p|m|t>/preview_<id>.html` (A4 clips).

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
