# desk-buddy: reconciled plan (v1)

Status: reconciled by two planning sessions (planner-a, planner-b) on
2026-11-05. The user answered the open questions the same day (§8), and the
plan switched to **pixel art**, which changes §3–§5 and §7. Reference
implementation for the harness: 2market (`~/Desktop/work/2market`,
`docs/designs/2market-agent-harness.md`).

Last updated 2026-11-10 against the code: all art now comes from Blender (D13),
S0, S1 and S2 are done on macOS (D14), and S3 is dropped (D15). §7's roadmap
shows what is done and what comes next.

### Decisions so far

| # | Decision |
|---|---|
| D1 | TS sidecar (Bun + pi-durable) for the agent loop; Rust everywhere else. |
| D2 | **Pixel art, "64-bit" style**: the user's term for fine-detail, rich-palette pixel art (not chunky 16-bit or 32-bit sprites). Size and color both matter: enough resolution for real detail, and a large, muted, hue-shifted palette, while keeping the nostalgic pixel look. The user shared a reference image (third-party art, used only as a reference, never copied). "64-bit" names the level of detail, not a 64×64 canvas: the bear is about 100 px tall (D14). ~~The first mascot is an agent attempt; the likely final path is an artist's base sprite that agents derive from.~~ Superseded by D13: the sprites are rendered from a Blender model, and the user's own drawings (`experiments/bear-proto/ref/`) are the reference. |
| D3 | macOS menu bar accessory app (no Dock icon). |
| D4 | 4 unprompted model turns/hour; 0 during quiet hours and DND. |
| D5 | Music: YouTube IFrame widget + OS media controls; yt-dlp opt-in. |
| D6 | Agent browser: injected-script webview; Chromium only if needed. |
| D7 | No Rive trial. **Rev2D is out** (bones and mesh warps break the pixel grid). |
| D8 | **Distributed in the future** → the product must not depend on anything we can't ship. |
| D9 | Every pi-ai provider; Claude as the default. |
| D10 | busybox-w32 as the Windows shell. |
| D11 | **Aseprite, compiled from source, is for development only** (EULA §2g allows personal builds; §2b forbids distributing them). The product depends only on the documented `.aseprite` file format and our own toolkit. Since D13/D15 the bear doesn't use Aseprite at all; this rule now applies only to `.aseprite` import for user-made mascots in P4. |
| D12 | Before any app code: a **mascot demo** to judge whether agent-made pixel art is good enough. |
| D13 | **The mascot lives in a world; the window is a view into it.** **Fake 3D**: the runtime is 2D sprites with depth ordering (no real-time 3D engine). A follow camera keeps the mascot centered. A small fixed set of **shots** (e.g. wide room, desk, talking close-up), each with its own angle, tied to activities; the app **cuts** between them (no smooth orbiting). The brain grows into a small life sim (places, needs, routines), and packs split into mascot packs + room packs. Decided 2026-11-06 after the `experiments/scene-proto/` prototypes: **isometric** (2:1) wide shot; **floating borderless view with a follow camera** (soft-edged, Pip always centered, room scrolls behind; no frame); **3 shots** (wide room, desk, talking close-up) with **hard cuts**, all shots sharing one aspect ratio; **hybrid art**: rooms and furniture pre-rendered from Blender (toon ramps to palette, ID/depth outlines, holdout occlusion), Pip hand-drawn (artist sprite later, agent-drawn for prototypes). Real-time 3D is reconsidered only if a freely decoratable room becomes core. **Changed 2026-11-09: all art comes from Blender, including the mascot.** The mascot is now a polar bear: a 3D model built from the user's pixel-bear reference, rigged and animated in Blender, and pre-rendered through the same toon → palette → ink-outline pipeline into iso pixel sprites in 8 directions at 12 fps. The pipeline lives in `experiments/mascot/bear3d/`. Isometric, the follow camera and the 3 shots are unchanged. |
| D14 | **The S0 mascot demo (D12) passed, 2026-11-09.** The user approved the "cute" Blender bear (mochi body, flat plush face) and its Stage A clips: idle, walk, run and their start/stop transitions. The bear is about 100 px tall. S1 put it on the real desktop (spikes/s1-window: NSPanel + CALayer, 0.3% CPU idle), and the user found the 64/100 sizes and zoom ×1–×3 all fine. The default is 100 px, and zoom is a user setting. The brain and harness (S2) are worked on in a separate session. |
| D15 | **S3 is dropped, 2026-11-10.** D13 moved all art to Blender, so the bear ships as PNG strips + JSON from `experiments/mascot/bear3d/`, not `.aseprite`. That removes S3's open question (`asefile` vs `aseprite-io`), and S1 already loads and composites the pipeline output, so no spike is left to run. The versioned **pack format** (manifest + loader over the pipeline output, intent → clip map) is built in P1a. Headless composite, `lint` and `diff` move to P4 with the studio; P1b may pull the composite forward. `.aseprite` import for user-made mascots also waits for P4. §3 and §5 were updated to match. |
| D16 | **Face layers for blinking and talking; no eye look-at in v1, 2026-11-10.** The Blender pipeline renders the bear as whole-body frames, so the eyes and mouth come as separate small **face layers** per direction (eyes open/closed, a few mouth shapes), drawn over the body at a **per-frame face position** that Blender exports with each clip (as it does `root_px`), so the face follows head bobs. The brain then picks blink timing at random (jittered timers) and flaps the mouth while `Speaking` streams. Faces are rendered only for the directions where the face shows (not N, NE, NW). A pipeline check, like `verify_b.py`, requires body + open-eyes/closed-mouth layer to match the original render pixel for pixel. Baking blinks and talking into the clips was rejected: blinks would repeat on the idle loop's fixed beat (30 frames, 2.5 s), and a baked mouth can't follow streamed speech. **Look-at in v1 = turning the body** toward the target in the 8 directions. Moving the eyes within the face waits until blinking and talking have been judged on the desktop. P1a's pack format carries the face layers and positions; the art round renders them. |

## 0. Goals (from the request)

1. Always-on-screen mascot on macOS and Windows. That is the only idea taken
   from the Office Assistant era; original character and art only.
2. 2D mascot that feels alive: many idle actions, reactions, status display,
   no visible repetition.
3. An agent harness on pi-durable, shell-first, with Claude Code's three
   permission modes, context management and the rest.
4. Rust everywhere it pays off; best performance.
5. A standard mascot interface: many mascots later, user-made mascots, one
   clean contract between mascot and harness.
6. Widgets the mascot opens (media player, browser) that the user and the agent
   can both control.
7. A studio where agents create and animate mascots and the user reviews and
   gives feedback.

## 1. Runtime split

- **Tauri 2, one process.** Rust owns windows, the mascot runtime, brain and
  renderer, senses, widgets, permission policy, credentials (OS keychain), the
  host API with the `buddy` CLI, and storage.
- **Agent loop = a Bun-compiled TypeScript sidecar** (pi-durable 1.5 + pi-ai).
  It ships as one ~68 MB binary (`agent/dist/buddy-agent`); no Node install.
  In 2market's spike it opened in 0.07 s with 51 MB RSS.
- **Why not all Rust.** pi-durable is TypeScript. The existing Rust ports
  (pi-agent, rpi-ai, nowa/ri) cover pi-agent-core, not pi-durable: no crash
  recovery, durable timers or subagents. 2market built a Rust loop
  (`fc63a02`) and reverted it (`5b58062`) because of three streaming dialects,
  compaction, the model catalog and OAuth sign-ins. The loop waits on the
  network, so Rust gains nothing there. The always-on mascot is where
  performance matters, and that part is pure Rust.
- **The JSONL-over-stdio protocol is the seam**, so a future Rust harness can
  replace the sidecar.
- **Ported from 2market:**
  - done in S2: `bridge.rs`, `protocol.rs` (`crates/bridge`)
  - done in S2: `policy.rs` + `rules.rs` + their tests (`crates/policy`;
    2market's spend allowances and panel checks are not ported)
  - done in S2: the gate extension (fail closed, decide timeout)
  - done in S2: exact-env bash prepare
  - P2b: the subagent extension (research child, web tools only, max 3)
  - P2b: jobs/`wait` (durable `sleepUntil` = reminders)
  - P2b: context/compaction
  - P2c: `RustCredentialStore` (S2 keeps keys in Rust memory only, in
    `crates/bridge/src/credentials.rs`)
  - P2c: pi-ai sign-ins

## 2. Harness (shell-first)

### Tools

- `bash`, `read`/`write`/`edit`, `web_fetch`/`web_search`, `look`, `task`,
  `wait`.
- S2 offers only `bash`, `read`/`write`/`edit` (`agent/src/ext/tools`); the
  rest come in P2b. On Windows `bash` stays off until a busybox-w32
  `shellPath` is set.
- `look` is offered only to image-capable models.
- `web_search` = 2market's keyless DuckDuckGo path. S2 found that pi-ai 1.5
  has no provider-native search.
- **Every desk-buddy capability is the `buddy` Rust CLI** talking to the
  loopback host API. For example:

  ```
  buddy say "Done!" --emotion happy
  buddy mascot emote celebrate
  buddy music play "lofi" | pause | next | status --json
  buddy widget open browser --url … | list | close <id>
  buddy browser goto | snapshot --json | click <ref> | type <ref> …
  buddy remind --in 20m "…"   ·   buddy notify "…"
  buddy studio render | sheet | gif | diff | audit --json
  ```

  S2 built `buddy say`, `buddy mascot emote`, `buddy status` and
  `buddy --help [--json]` (`crates/host/src/bin/buddy.rs`); the rest arrive
  with the phases that need them.
- The agent learns the CLI from skills (`SKILL.md`) plus `buddy --help --json`.
  There are no extra tool schemas, so the cached prompt prefix stays stable.
- A typed tool is a later option, only if something needs streamed structured
  output.

### Host API auth and loopback hygiene

- **Agent token.** bash prepare injects a per-agent token and
  `BUDDY_TURN=<requestId>` into each command's exact env.
  - Auth is bound to the token. Each sidecar start gets a new one, which is
    revoked when that sidecar exits.
  - `BUDDY_TURN` is attribution only. A stale turn id is accepted, logged and
    shown as "after turn", so a script may outlive its turn.
  - Research subagents never get the token.
- **User token**, for the user's own terminal and own CLI agents (Claude Code,
  Codex), so `buddy say/music/remind` works outside any sidecar turn. The host
  writes it with the port to `~/.desk-buddy/host.json` (0600), and `buddy`
  reads that file when it has no `BUDDY_TOKEN`.
- **Widget tokens.** Widget webviews get no token by default. A widget that
  needs agent actions declares them and gets a narrower capability token.
- **Loopback hygiene.** The browser widget runs arbitrary pages on the same
  loopback network, so:
  - bind 127.0.0.1 only
  - random port, written to a 0600 file
  - token in a header, never a cookie
  - reject requests with an `Origin` header unless it's a first-party widget
    origin
  - check `Host` against DNS rebinding

### Permissions (Claude Code style)

- **Modes:** `ask` / `allowlist` (default) / `skip`. Deny rules apply in every
  mode. Precedence deny > ask > allow. Fail closed on error or timeout.
- **Rules match shell text:** `Shell(buddy music:*)`, `Shell(git status:*)`,
  `Read|Write|Edit(glob)`.
- **Matching is best-effort, as in 2market's `rules.rs`:**
  - compound commands, pipes and `$()` are split, and every segment must match
    an allow rule
  - anything the parser can't read as plain words asks in `allowlist` mode and
    runs only in `skip` mode
- **Taint** (2market D10) is set by web_fetch, web_search and agent-browser
  reads. While tainted:
  - no "Always" rule can be created
  - outbound actions (send, post, pay, agent-browser form submits) ask even in
    `allowlist` mode
  - `skip` still skips
- **Windows:** bundle **busybox-w32** as the shell (pi-durable `shellPath`), so
  there's one shell syntax and one rule parser.
- **Approval card = a mascot behavior.** The card renders in the speech bubble
  (Allow once / Always / Deny), and the mascot plays its "attention" behavior
  while it waits.

### Harness → mascot mapping (no tool-specific code)

| pi-durable event | MascotEvent |
|---|---|
| run_start | `Thinking` |
| tool_execution_start | `Working{kind}` (bridge classifies: `buddy music`→media, `git push`→vcs, …) |
| approval_asked | `AwaitingPermission` |
| message deltas | `Speaking` (streamed from `watchEvents` straight into the bubble) |
| error | `Error` |
| idle | `Idle` |

`buddy say` is only for proactive or out-of-turn lines.

### Senses, reflexes and the attention budget

- **Senses:** active app/window, idle time, now playing, time of day, and
  clipboard (opt-in only).
- **Reflex layer (zero tokens).**
  - Rust-side rules map sense → MascotEvent inside the brain; packs still only
    map event → behavior.
  - Examples: you switch to a game → the mascot gets out of the way; 2 h of
    nonstop work → it stretches.
  - Most of the "aliveness" never touches the LLM.
- **Built-in reflexes:**
  - **Do-not-disturb:** triggered by a fullscreen frontmost app,
    Keynote/PowerPoint/Zoom screen share, or a user toggle. The mascot hides or
    goes still, and no unprompted model turn fires.
  - **Quiet hours.**
- **Attention budget.** Only debounced events that pass the budget become model
  inputs. Default: **4 unprompted model turns per hour**, 0 during quiet hours
  and DND, with the count visible in the UI.
- **Privacy.** Sense events live in a short in-memory ring buffer. They are
  never written to disk unless the user opts into a log. They leave the machine
  only as the summarized event inside a budgeted turn.

### Context

Carried over from 2market:
- tool output capped at 200 lines / 16 KB, with the full output saved to a file
- old tool outputs cleared in batches, so the cached prompt prefix changes
  rarely
- compaction at 80% of the window
- skill index in the prompt; system prompt sections fixed for the session

## 3. Mascot contract

```
Harness ──MascotEvent + params──▶ Brain (shared, Rust) ──Intent──▶ Pack (data only) ──▶ MascotBackend (trait)
```

- **MascotEvent** is versioned and semantic, and never contains clip names. It
  also carries continuous params: look-at point, energy, speech amplitude.
- **Brain** is shared by all mascots:
  - idle scheduler with weights, cooldowns and memory of recent picks
  - jittered blink/breath timers
  - eye and head follow, springs on accessories, blend layers
  - reflex reactions
  - rate-limited spontaneous moments driven by the LLM, under the attention
    budget
- **Pack** = a text-first folder that agents and users can author and git can
  diff:
  - `manifest`, the art (PNG strips + their JSON, below), `behavior` map
  - an intent → behavior map with fallback chains (`celebrate → happy → idle`)
  - declared capabilities (look-at, lip-sync, …)
  - **temperament** params (energy, curiosity, shyness, chattiness, sleepiness)
    that scale the brain's weights and timers, so each mascot has its own
    personality without code
- Packs are data only in v1. Sandboxed Rhai scripting is a later, optional
  extension.
- **The pack's art is the Blender pipeline's output (D15)**: one PNG strip per
  clip and direction, plus a JSON file describing them. The versioned pack
  manifest and loader around it are built in P1a. Today's files
  (`experiments/mascot/out/bear3d/`):
  - `anim/anim.json` (Stage A: idle, walk, run and their start/stop
    transitions): per clip `loop`, `fps` (12), `frame_w`/`frame_h` (84×123),
    `anchor` (the floor point between the feet), one strip per direction
    (S, SE, E, NE, N, NW, W, SW), and per frame `dur` plus `root_px` (how far
    to move the sprite after that frame, so walks don't slide).
  - `anim_b/anim_b.json` + `props/props.json` (Stage B: the bed and reading
    sets, with `get_in_bed → sleep → wake_up` and `sit_down → read → stand_up`).
    Each set comes in two placements: A, and B, its true 3D mirror. During a
    Stage B clip the props and the bear are baked into one frame. Outside
    those clips the props and the bear are separate sprites, sorted by floor
    depth. Hand-off frames match the props + idle sprite pixel for pixel
    (`verify_b.py`).
  - `.aseprite` packs (layers → parts, tags → clips, slices → anchors) are
    for user-made mascots and wait for P4 (D15).
- **Backend in v1: `sprite` with face layers (D16).**
  - The bear's frames are whole-body sprites per direction. Moves are
    **whole-pixel offsets only**, and there are no flips, because every
    direction is its own render.
  - squash and stretch, bob and gait are in the rendered clips.
  - blink = eyes-closed face layer; speech = mouth-shape layers flapped while
    `Speaking` streams. Both are drawn at the clip's per-frame face position,
    in the directions where the face shows. Not rendered yet (art round).
  - look-at = turning the body toward the target in the 8 directions; no eye
    movement within the face in v1.
  - Effects (hearts, zzz, sweat drop) are sprites too. Stage B already
    renders `zzz_<ori>.png` as an overlay.
  - Liveliness comes from scheduling, clip variety and face layers on top of
    any clip. The brain's continuous params are quantized to
    the pixel grid and to the clip fps (12).
- **Later:** a smooth `rig` backend through the same trait, only if a
  non-pixel mascot is ever wanted.
- **Screen awareness** (walking along window edges, sitting on title bars)
  needs other apps' window geometry, which brings permission prompts:
  - macOS: CGWindowList (titles need Screen Recording permission; bounds don't)
  - Windows: EnumWindows + DwmGetWindowAttribute

  Moved to **P3 or later**.

## 4. Windows, widgets, media, browser

### Mascot window

S1 (`spikes/s1-window`) built this on macOS; Windows is still unbuilt.

- **CPU composite by default** (a few blits per frame on an ~84×123 canvas),
  because pixel art doesn't need a GPU vector pipeline. S1 composites a faint
  iso floor grid that scrolls with the follow camera, a soft vignette, and the
  bear centered on top, at 0.3% CPU idle.
  - Windows: a layered window (`WS_EX_LAYERED` + `UpdateLayeredWindow`), which
    passes clicks through alpha-0 pixels by itself. Not built yet.
  - macOS: a CALayer fed from a CPU buffer (premultiplied BGRA).
  - wgpu and Vello leave v1, and the renderer trait stays.
- **Display scale is always a whole multiple of sprite pixels**: backing scale
  × user zoom, floored. Never fractional, or Windows at 125%/150% blurs it.
  Zoom is a user setting, ×1–×3 (D14), so the ~100 px bear is 200–600 device
  px on a Retina display.
- **Per-pixel hit test from our own alpha** on macOS: toggle
  `ignoresMouseEvents` when the cursor is over opaque pixels (as in Bevy's
  desk_toy). In S1 a press on the bear drags it.
- Low fps when idle, 0 when hidden or occluded.
- **macOS:**
  - a non-activating NSPanel
  - `collectionBehavior` = canJoinAllSpaces + fullScreenAuxiliary
  - accessory activation policy → a **menu bar app, no Dock icon** (D3), with
    a tray menu
  - window level just above *floating*; raised to the fullscreen-capable level
    only while the frontmost app is in a fullscreen Space (a level above
    statusBar would also paint over Spotlight, menus and Notification Center)
  - all done with objc2, as in 2market
- **Last-resort fallback:** a transparent webview mascot (the WindowPet route).

### Bubbles and widgets

- **Bubbles** are anchored webview windows.
- **Widgets** are frameless webviews run by a Rust widget manager, each with a
  `widget.json` manifest.
  - The actions a widget declares become `buddy` subcommands, and its state
    goes into the agent's context.
  - The widget manifest and JS widget SDK are written on day one, and the
    first-party widgets use them.
  - v1 ships **first-party widgets only**, because user widgets are code (unlike
    packs). Opening them up later = Tauri capability scoping, a narrow token
    and an install-time prompt. That's a policy change, not a refactor.
  - Widget pages are served from the loopback HTTP server over
    `http://127.0.0.1:<port>`, not a custom scheme or `file://`, because
    YouTube embeds check the embedding origin.

### Music

1. **YouTube IFrame player in a media widget**, visible or hidden. That is the
   "hidden browser".
   - Autoplay must be allowed on that webview.
   - If a video has embedding disabled (error 101/150), it opens in the browser
     widget instead.
   - **Spike check:** does audio keep playing when the widget is hidden? If
     WKWebView throttles it, park the window at 1×1 off-screen instead.
2. **OS media controls** for native players, used for pause/next/now-playing
   (now-playing also feeds senses):
   - macOS: AppleScript to Music and Spotify, which triggers one Automation
     permission prompt per target app
   - Windows: GlobalSystemMediaTransportControls, no permission needed
3. **yt-dlp** only as an explicit opt-in "personal/offline" mode. It breaks
   often and is a terms-of-service grey zone.

### Browser

- **One engine (the Tauri webview), used two ways:** the user-facing browser
  widget and a hidden agent instance.
- **The agent drives it through an injected script:**
  - a text DOM snapshot with element refs
  - click/type by ref
  - screenshots (WKWebView snapshot via objc2; CDP on WebView2)
- Agent-browser reads set taint.
- chromiumoxide (Chromium over CDP, ~150 MB) is the escalation path only.

## 5. Studio

- **Pattern** from HyperFrames Studio: shared files; you point at a frame and
  leave a note for the agent.
- **Art pipeline (D13, D14): Blender → palette → ink outline.** All of the
  bear's art comes from `experiments/mascot/bear3d/`. Nothing is drawn in code.
  - A 3D bear built from the user's drawings (`experiments/bear-proto/ref/`),
    kept cute and toy-like (mochi body, flat plush face), rigged and animated
    in Blender 5.0 headless.
  - Build order: `run.sh` (model) → `run_anim.sh` (Stage A clips) →
    `run_b.sh` (Stage B sets and clips), each needing the previous step's
    `.blend`. `verify_b.py` checks the Stage B hand-offs after a re-render.
  - Pixel rules: iso 2:1 orthographic camera, 8 directions, 12 fps, 40.07 px
    per world unit, alpha only 0 or 255, a 1 px ink outline, and colors only
    from `bear-proto/palette-bear.json` + `scene-proto/palette.json` (+
    `bear3d/palette-ext.json` for furniture).
  - Review: GIFs and `preview.html` / `preview_b.html` pages, plus Pillow
    contact sheets, and the real desktop through S1.
  - New clips (think, talk, ask, oops, happy, celebrate) are the roadmap's
    art round.
- **Pixel-art survey (2026-11).** Written before D13, when the bear was
  meant to be drawn by hand. It still matters for user-made mascots in P4:

  | Tool | Verdict |
  |---|---|
  | **Aseprite** (CLI + Lua; community MCPs such as `aseprite-live-mcp`, where the agent draws in the open editor with one undo per edit) | **The human editor and the file format.** Source build for dev only (D11). |
  | PixelAOI (MCP, reads and writes `.aseprite`) | Windows and Linux only, v0.6. Watch it. |
  | Pixelorama | No CLI (#579). Out. |
  | Piskel MCP, pixel-art-tutor (WebMCP) | Prototypes. |
  | PixelLab, Retro Diffusion | **Generators**, paid credits: concept drafts, rotations, style-matched variants that go through review; never the source of truth. |

  None of them has a real review step (comments, approval), so that part is
  ours. **Verdict: no GUI pixel editor of our own.** Aseprite is for human
  edits of user-made mascots; our studio handles behavior and review, plus a
  headless pixel toolkit for agents.
- **Headless pixel toolkit, `buddy studio px …` (P4): the agent path that
  always works**, with no Aseprite needed (D8/D11). It touches up sprites; it
  doesn't replace the Blender pipeline:
  - `grid`: a layer/cel as a palette-indexed text grid (a 64×64 cel ≈ 4 KB),
    edited and written back
  - layer ops: integer shift, mirror, recolor within the palette, outline,
    flood fill, copy frame/tag, swap cels
  - `sheet`: contact sheets with zoom, onion skin, and a mock desktop
    background at true display scale
  - `diff`: highlights changed pixels
  - `lint`: off-palette colors, stray pixels, non-integer offsets, missing tags
    or anchor slices, frame counts per tag
  - Aseprite's own `-b --script` Lua bridge is an optional extra when it's
    found on the machine.
- **Sprite spec:** one page covering canvas, palette, directions, required
  clips, anchors and fps. For the bear the pipeline's pixel rules and its JSON
  files are that spec; P1a's pack README writes it down, and P4 hands it to
  people who make their own mascots.
- **The studio is the app's own binary in studio mode**, so the preview is
  exactly what the app renders. Its UI is a webview inside that binary.
  - hot reload of the pack folder
  - state/clip pickers, param and temperament sliders
  - **harness simulator** with scripted event timelines and simulated senses
  - scrub/loop, lint
  - **comments pinned to clip + time (+ region)**, written as files the agent
    reads
  - **liveliness audit:** simulate 30+ minutes of the brain and chart what
    played and how often, so repetition is measurable
- **Headless rendering.** `buddy studio render | sheet | gif | diff | audit
  --json` renders offscreen on the CPU, so output is pixel-stable across
  machines and CI. `audit` outputs JSON plus a PNG chart. Agents run check →
  stills → look → fix without a window, the same loop as in tiktok-motion.
- The studio doubles as the **user-facing "make your own mascot" tool**, so its
  UX matters beyond agents.
- **Art (the hardest problem).** Solved for now by the Blender pipeline: the
  S0 demo passed (D14), and agents add clips by animating the same rig. An
  artist is no longer on the plan.

## 6. Workspace layout

✅ = exists today; the rest is planned (the roadmap step that adds it is in
brackets).

```
Cargo.toml             workspace; spikes/s1-window is excluded (own lockfile and target/)
crates/
  buddy-app/           Tauri shell                                    [P1b]
  mascot-protocol/ ✅  MascotEvent (versioned); params + pack schema [P1a]
  mascot-core/         brain + runtime (scheduler, reflexes, temperament) [P1a]
  mascot-render/       CPU composite + integer upscale (trait for later backends) [P1b]
  host/ ✅             buddy CLI + loopback API (tokens, origin/host checks)
  policy/ ✅           modes, rules, taint (ported from 2market)
  bridge/ ✅           sidecar process + JSONL protocol, MascotEvent mapping, in-memory credentials
  widgets/             widget manager, manifest, JS SDK               [P3]
  studio/              studio mode + headless render/audit            [P4]
agent/ ✅              TS sidecar (pi-durable + extensions; skills [P2])
mascots/<name>/        packs                                          [P1a]
spikes/
  s1-window/ ✅        S1: NSPanel mascot window, demo behaviour loop (folds into buddy-app in P1b)
  s2-harness/ ✅       S2: driver for one prompt through the whole harness
experiments/
  mascot/bear3d/ ✅    the Blender pipeline; output in mascot/out/bear3d/
  bear-proto/ref/ ✅   the user's original bear drawings + palette-bear.json
  scene-proto/ ✅      palette.json from the D13 scene prototypes
```

## 7. Spikes (with pass criteria), then phases

- **S0 mascot demo (D12): passed 2026-11-09 (D14).** The plan was an
  agent-drawn 64×64 `.aseprite` mascot. What passed is the Blender polar bear
  (§5 art pipeline): Stage A clips in 8 directions, reviewed as GIFs, preview
  pages and on the desktop through S1. Stage B (bed and reading sets) was
  rendered after it.
- **S1 window**
  - transparent + topmost + per-pixel click-through on macOS, including over
    fullscreen Spaces with the dynamic window level
  - the same on Windows 11 (CPU layered window)
  - idle < 1% CPU
  - coexists with a Tauri webview window in the same process
  - multi-monitor with mixed DPI: dragging the mascot across displays causes no
    blur and no jumps
  - on failure, walk the fallback ladder (§4)
- **S2 harness**
  - Bun sidecar round trip
  - a `buddy` command passes the policy gate (ask, then allow)
  - `BUDDY_TURN` reaches the host API
  - a crash mid-turn resumes
  - **Windows:** pi-durable's bash tool runs under busybox-w32 via `shellPath`
    (check for assumptions like `-lc`)
  - check whether pi-ai 1.5 has native search (it doesn't)
- **S3 studio/format: dropped (D15).** The pack format moves to P1a; headless
  composite, `lint`, `diff` and `.aseprite` reading move to P4.
- **Media spike** (before P3)
  - IFrame embed from the loopback origin
  - audio keeps playing while hidden (or the off-screen parking fallback works)

**Phases:**
- **P1:** protocol + brain + one mascot + senses/reflexes (DND, quiet hours)
- **P2:** harness + permissions + bubbles + attention budget + user token
- **P3:** widgets (media, browser) + screen awareness
- **P4:** studio + mascot SDK for users

### Roadmap (status as of 2026-11-12)

Each step is one autopilot ticket unless it says otherwise. Do them in order,
because each one builds on the one before it.

| # | Step | Status | Builds on |
|---|---|---|---|
| 1 | **S0** mascot demo: the Blender bear, Stage A clips (D12, D14) | ✅ done | — |
| 2 | **S1** window on macOS: NSPanel, click-through, fullscreen Spaces, mixed-DPI display moves, 0.3% CPU idle, webview in the same process (`0b1f66e`) | ✅ macOS · ⏳ Windows 11 deferred (no Windows machine) | S0 |
| 3 | **S2** harness: sidecar, policy gate, host API + `buddy`, `BUDDY_TURN`, crash resume, MascotEvents (`e1df770`). pi-ai 1.5 has no native search. | ✅ macOS · ⏳ busybox-w32 check deferred | — |
| — | ~~S3 studio/format~~ | dropped (D15) | — |
| 4 | **P1a** brain + intents: the versioned pack format (manifest + loader over the pipeline output, with room for face layers and per-frame face positions, D16) with the bear pack's intent → clip map and fallbacks; `crates/mascot-core` (event → intent, idle scheduler, timers, temperament, DND/quiet-hours inputs); a replay driver | ⬜ next | S0 art, S2 events |
| 5 | **P1b** app shell: `crates/buddy-app` (Tauri) folds S1 in, with the brain driving the window from real events; S1's demo loop retires | ⬜ | S1, P1a |
| 6 | **P1c** senses + reflexes: frontmost app, idle time, time of day, DND (fullscreen, screen share, toggle), quiet hours | ⬜ | P1b |
| 7 | **Art round:** Blender clips for think, talk, ask, oops, happy and celebrate (asked for by P1a's pack README), plus the D16 face layers (eyes open/closed, mouth shapes, per-frame face positions) and their pixel-match check | ⬜ can run any time after P1a | S0 pipeline |
| 8 | **P2a** harness in the app: `Agent` + `Host` in buddy-app, the speech bubble streaming `Speaking`, `buddy say` lines, the approval card as a mascot behavior, the user token | ⬜ | P1b, S2 |
| 9 | **P2b** harness depth (S2b): context capping and compaction, `web_fetch`/`web_search` (DuckDuckGo), `look`, research subagent (`task`), jobs/`wait` reminders | ⬜ | S2 |
| 10 | **P2c** credentials (S2c): OS keychain + pi-ai sign-ins, and the attention budget (D4: 4 unprompted turns/hour, 0 in DND and quiet hours) | ⬜ | P2a, P1c |
| 11 | **Media spike:** IFrame embed from the loopback origin, hidden audio | ⬜ | P2a |
| 12 | **P3** widgets (manager, manifest, JS SDK, media, browser) + screen awareness | ⬜ | media spike |
| 13 | **P4** studio + mascot SDK: studio mode, `buddy studio …` (headless composite, `lint`, `diff`), liveliness audit, `.aseprite` import (D15) | ⬜ | P1a |
| 14 | **Windows pass:** S1 layered window + S2 busybox-w32 on Windows 11 | ⏳ needs a Windows machine | S1, S2 |
| 15 | Release profile + distribution (D8) | ⬜ | P2 |

### Aliveness round (added 2026-11-11)

The user's request: the bear mostly stands and walks, which does not read as a
living mascot. It needs many more animations, and the agent must be able to
drive its own body. Effort and risk are not a constraint in this round; aim
for the best result. Do the steps in order: A1 first, because every later
animation plugs into it.

| # | Step | Status | Builds on |
|---|---|---|---|
| A1 | **Body tools + system prompt** (code only) | ✅ built (`buddy mascot do\|stop\|list\|emote`, pack `body` block) | p3 harness |
| A2 | **Listen:** face the user on click | ✅ built (prompt pokes, listen slot, turn planner, face v2 `patch_dirs`, `anim_l` listen + turn clips) | A1 |
| A3 | **Working animations** for long turns | ✅ built (desk station: pack `stations` + `work`, seated turns, seated `talk`/`ask` with face v2 `set_clips`, `buddy mascot do computer`, `anim_d` desk set) | A1, A2 |
| A4 | **Idle life:** fidgets and postures | ✅ built (pack v2 idle brain: `variety` rule, `for_s` holds, turn first, hidden entries; stage kit; art stages f g p m t + lean, all in the bear's idle pool; `buddy studio audit` v2 passes 10 and 30 min on seeds 1,2,3) | A2's turn-in-place clip |
| A5 | **Real clips for the borrowed intents** | ✅ built (stages e + x, 19 one-shots; the 11 intents play their own clip first with the old chain behind; 16 new gesture emotes in `mascot list` and the prompt; `emotes.jsonl` replay) | A1 |
| A6 | **Desktop reactions, prop activities, rare surprises** | ✅ built (p4 reactions: pack `contexts` + `reactions`, cursor sense, drag dangle/land/dizzy, flinch/pet/follow, morning, evening, away, approval_wait, edge entries; stages r s w. p5: 16 prop activities in set stages sit_a sit_b play out + nightcap, idle `long` entries and `buddy mascot do`; 11 rare surprises in stage z on 2400–3600 s cooldowns, seasonal hats by month; sheet index `out/bear3d/sheets.html`) | A4 |

**Who decides an animation** (applies to every step):

| Layer | Decides | Covers |
|---|---|---|
| Automatic | The brain, no model | Click → listen; waiting → think; tool running → a working animation picked from the tool's `WorkKind`; answer streaming → talk |
| Agent tools | The agent | What it wants to express or was asked to do ("read a book"), and a specific working animation before a long job |
| System prompt | Us | Tells the agent it has a body, what it can do and when to use it |

Priority: do-not-disturb and quiet hours > the user's click > the agent's
choice > the automatic default.

**A1. Body tools + system prompt**
- Today the agent has only `buddy mascot emote <wave|nod|shake|jump|spin>`
  (one-shots), so "read a book" gets a text reply and no animation.
- Add `buddy mascot do <activity> [--for 5m]`, `buddy mascot stop` and
  `buddy mascot list`.
- The activity and emote lists come from the pack, so a new animation reaches
  the agent with no code change. Names are semantic (activity → behavior in
  the pack); events still never carry clip names (§3).
- An activity starts after the bear finishes its reply, and holds for its
  duration or until the user clicks the bear or a new turn starts.
- Each command replies with what happened ("reading for 5 min", or "can't,
  do-not-disturb is on"), so the agent never claims an animation that did not
  play.
- The agent can chain a work animation with the work in one shell command
  (`buddy mascot do research && curl …`), so it costs no extra round trip.
- System prompt section, generated from the pack: you have a body; do physical
  requests with the tool instead of describing them; the list with one line
  per entry; match the answer's mood; pick a fitting work animation for long
  jobs; don't overdo it on short replies.
- Works with today's `read` and `nap` before any new art exists.

**A2. Listen**
- Today `Poke::Click` does nothing to the body, and think/talk/ask play in the
  current facing, so the bear can answer with its back turned.
- New `listen` state: starts when the prompt opens, holds until the user sends
  or dismisses it.
- Art: a `listen` clip per direction with the head turned to the camera (small
  ear perk or lean-in), with face layers at the turned head's position.
- From N, NE and NW the head can't reach: the bear first turns its body to the
  nearest side or front direction, then turns its head.
- Art: a turn-in-place clip, because direction changes are instant today.
- This extends D16's "look-at = turning the body" with a rendered head turn
  for listening only (user request, 2026-11-11). Eyes still don't move within
  the face.
- First step with no new art: snap to the nearest front-facing direction on
  click.

**A3. Working animations**
- Today every tool call plays `think`; the brain ignores `WorkKind`.
- First set: a desk with a computer, built like the bed and reading sets:
  `sit_at_desk → type (loop) → stand_up`.
- Short turns stay standing: go to the desk only when the turn runs on (about
  4 s, or a second tool call).
- Once seated, stay seated through the Thinking/Working alternation until the
  turn ends.
- Listen → work: head turns back, think pose, walk and sit.
- Work → answer: the bear turns its head from the screen to the user and talks
  from the chair, then stands up after the bubble is dismissed (needs seated
  `talk` and `ask` clips). An approval card mid-work uses the seated `ask`.
- Later: other loops per `WorkKind` (papers for files, a telescope for web, …).

**A4. Idle life** (breaks up `rest`, which carries most of the idle weight)
- Fidgets, 1–3 s, in place: weight shift, ear twitch, head tilt, look up or
  down, scratch, stretch, yawn, rock on heels, tail wiggle (back views),
  shake off, sneeze, tummy rub, nod off and jolt awake, unprompted wave, peek
  at the cursor.
- Postures: sit on the floor, sit and sway, lie on belly kicking feet, lie on
  back, curl up for a floor nap, squat to inspect, lean on an edge.
- Moving: tiptoe, skip, sleepy shuffle, backwards waddle, hop, roll, spin,
  trip and get up, chase something invisible, slide to a stop.

**A5. Real clips for the borrowed intents**
- These play another intent's clip today: `wave`, `nod`, `greet`, `stretch`
  (→ happy); `sad`, `shake` (→ oops); `surprised`, `jump`, `spin`, `dodge`,
  `remind`.
- New: shrug, facepalm, thumbs up, clap, point at the bubble, shy hide, laugh,
  pout, proud chest-puff, confused head-scratch.

**A6. Desktop reactions, props, rare surprises**
- Reactions: music playing (head bob, foot tap, dance); sitting on a window
  edge (dangle legs, peer over, doze); being dragged (dangle, squash landing,
  dizzy); cursor (follow, flinch, giggle when petted); time of day (morning
  stretch, evening yawns, nightcap in quiet hours); long user idle (tap on the
  glass, wave, give up and nap); waiting on an approval (foot tap, check
  watch).
- Prop activities, long loops like read and nap: tea or cocoa, snack, laptop,
  notebook, ball, bubbles, plant, knitting, yo-yo, block tower, ice fishing,
  snowman, sweeping, headphones, umbrella or balloon.
- Rare, long cooldown: windmill arms, hiccups, find a coin, snowflake on the
  nose, butterfly on the ear, peekaboo, moonwalk, seasonal hats.

**Art constraints for A2–A6**
- Paws must not cross the face in S/SE/E/W/SW (`verify_face.py`). Yawn with a
  paw, eye rub, facepalm and peekaboo need a baked face like walk/run, or a
  pose with the paws beside the face.
- Fidgets and gestures fit the 84×123 Stage C frame. Lying down, rolling and
  props need a wider frame or a set, as Stage B does.
- Keep the bear cute and toy-like in every clip (ground rules).

## 8. Open questions

All ten earlier questions are answered (D1–D10). Answered since:
- ~~Does "64-bit pixel art" mean a 64×64 canvas?~~ No. "64-bit" is the user's
  word for the level of detail and color (D2), not a canvas size. Bit depth
  counts colors; canvas size counts pixels. The bear is about 100 px tall on
  an 84×123 frame (D14).
- ~~Does LibreSprite handle tags and slices well enough to recommend to users
  without Aseprite?~~ No longer relevant: the bear comes from Blender (D13),
  and `.aseprite` import waits for P4 (D15). Check it again then, if P4 still
  wants a free editor to point users at.
- ~~Blink, look-at and speech for the Blender bear?~~ Face layers for blinking
  and talking; look-at by turning the body (D16).

Nothing else is open right now.

## Sources

- Pi Durable: https://redreamality.com/blog/pi-durable-agent-harness-durable-execution/ ·
  https://developers.cloudflare.com/agents/harnesses/pi/ ·
  https://alternativeto.net/news/2026/10/pi-1-0-brings-native-mcp-support-and-pi-durable-for-long-running-crash-resistant-ai-agents/
- HyperFrames Studio: https://runtimewire.com/article/heygen-hyperframes-studio-agent-video-editor
- Rev2D: https://glama.ai/mcp/servers/RevStudio/Rev2D
- Rive Editor MCP: https://rive.app/docs/editor/ai/mcp
- rive-bevy / rive-rs: https://github.com/rive-app/rive-bevy
- Spine agent tooling: https://glama.ai/mcp/servers/nff3x1nh98 ·
  https://github.com/GenielabsOpenSource/spine-animation-ai
