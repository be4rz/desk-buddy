# desk-buddy: reconciled plan (v1)

Status: reconciled by two planning sessions (planner-a, planner-b) on
2026-11-05. The user answered the open questions the same day (§8), and the
plan switched to **pixel art**, which changes §3–§5 and §7. Reference
implementation for the harness: 2market (`~/Desktop/work/2market`,
`docs/designs/2market-agent-harness.md`).

### Decisions so far

| # | Decision |
|---|---|
| D1 | TS sidecar (Bun + pi-durable) for the agent loop; Rust everywhere else. |
| D2 | **Pixel art, "64-bit" style**: the user's term for fine-detail, rich-palette pixel art (not chunky 16-bit or 32-bit sprites). Size and color both matter: enough resolution for real detail, and a large, muted, hue-shifted palette, while keeping the nostalgic pixel look. The user shared a reference image (third-party art, used only as a reference, never copied). The first mascot is an agent attempt; the likely final path is an artist's base sprite that agents derive from. |
| D3 | macOS menu bar accessory app (no Dock icon). |
| D4 | 4 unprompted model turns/hour; 0 during quiet hours and DND. |
| D5 | Music: YouTube IFrame widget + OS media controls; yt-dlp opt-in. |
| D6 | Agent browser: injected-script webview; Chromium only if needed. |
| D7 | No Rive trial. **Rev2D is out** (bones and mesh warps break the pixel grid). |
| D8 | **Distributed in the future** → the product must not depend on anything we can't ship. |
| D9 | Every pi-ai provider; Claude as the default. |
| D10 | busybox-w32 as the Windows shell. |
| D11 | **Aseprite, compiled from source, is for development only** (EULA §2g allows personal builds; §2b forbids distributing them). The product depends only on the documented `.aseprite` file format and our own toolkit. |
| D12 | Before any app code: a **mascot demo** to judge whether agent-made pixel art is good enough. |
| D13 | **The mascot lives in a world; the window is a view into it.** **Fake 3D**: the runtime is 2D sprites with depth ordering (no real-time 3D engine). A follow camera keeps the mascot centered. A small fixed set of **shots** (e.g. wide room, desk, talking close-up), each with its own angle, tied to activities; the app **cuts** between them (no smooth orbiting). The brain grows into a small life sim (places, needs, routines), and packs split into mascot packs + room packs. Decided 2026-11-06 after the `experiments/scene-proto/` prototypes: **isometric** (2:1) wide shot; **floating borderless view with a follow camera** (soft-edged, Pip always centered, room scrolls behind; no frame); **3 shots** (wide room, desk, talking close-up) with **hard cuts**, all shots sharing one aspect ratio; **hybrid art**: rooms and furniture pre-rendered from Blender (toon ramps to palette, ID/depth outlines, holdout occlusion), Pip hand-drawn (artist sprite later, agent-drawn for prototypes). Real-time 3D is reconsidered only if a freely decoratable room becomes core. **Changed 2026-11-09: all art comes from Blender, including the mascot.** The mascot is now a polar bear: a 3D model built from the user's pixel-bear reference, rigged and animated in Blender, and pre-rendered through the same toon → palette → ink-outline pipeline into iso pixel sprites in 8 directions at 12 fps. The pipeline lives in `experiments/mascot/bear3d/`. Isometric, the follow camera and the 3 shots are unchanged. |
| D14 | **The S0 mascot demo (D12) passed, 2026-11-09.** The user approved the "cute" Blender bear (mochi body, flat plush face) and its Stage A clips: idle, walk, run and their start/stop transitions. The bear is about 100 px tall. S1 put it on the real desktop (spikes/s1-window: NSPanel + CALayer, 0.3% CPU idle), and the user found the 64/100 sizes and zoom ×1–×3 all fine. The default is 100 px, and zoom is a user setting. The brain and harness (S2) are worked on in a separate session. |

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
  It ships as one ~65 MB binary; no Node install. In 2market's spike it opened
  in 0.07 s with 51 MB RSS.
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
  - `bridge.rs`, `protocol.rs`
  - `policy.rs` + `rules.rs` + their tests
  - the gate extension (fail closed, decide timeout)
  - exact-env bash prepare
  - the subagent extension (research child, web tools only, max 3)
  - jobs/`wait` (durable `sleepUntil` = reminders)
  - context/compaction
  - `RustCredentialStore`
  - pi-ai sign-ins

## 2. Harness (shell-first)

### Tools

- `bash`, `read`/`write`/`edit`, `web_fetch`/`web_search`, `look`, `task`,
  `wait`.
- `look` is offered only to image-capable models.
- `web_search` = 2market's keyless DuckDuckGo path unless pi-ai 1.5 has
  provider-native search (check in S2).
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

- The agent learns the CLI from skills (`SKILL.md`) plus `buddy --help --json`.
  There are no extra tool schemas, so the cached prompt prefix stays stable.
- A typed tool is a later option, only if something needs streamed structured
  output.

### Host API auth and loopback hygiene

- **Agent token.** bash prepare injects a per-agent token and
  `BUDDY_TURN=<requestId>` into each command's exact env.
  - Auth is bound to the token, which is revoked when the agent tab closes.
  - `BUDDY_TURN` is attribution only. A stale turn id is accepted, logged and
    shown as "after turn", so a script may outlive its turn.
  - Research subagents never get the token.
- **User token**, for the user's own terminal and own CLI agents (Claude Code,
  Codex), so `buddy say/music/remind` works outside any sidecar turn.
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
  - `manifest`, `art.aseprite` (or `sheet.png` + `sheet.json`), `behavior` map
  - an intent → behavior map with fallback chains (`celebrate → happy → idle`)
  - declared capabilities (look-at, lip-sync, …)
  - **temperament** params (energy, curiosity, shyness, chattiness, sleepiness)
    that scale the brain's weights and timers, so each mascot has its own
    personality without code
- Packs are data only in v1. Sandboxed Rhai scripting is a later, optional
  extension.
- **The pack's art is an `.aseprite` file** (or a PNG sheet + JSON in
  Aseprite's export layout, for users without Aseprite). It maps one-to-one:

  | Aseprite | Pack |
  |---|---|
  | layers | paper-doll parts |
  | tags | clips |
  | slices | anchors (bubble, eyes, hit area) |
  | palette | palette |
  | user data | metadata |

  Rust reads it directly with `asefile` (read only) or `aseprite-io` (read and
  write; maturity to be spiked).
- **Backend in v1: `sprite` with paper-doll layers.**
  - Layers move by **whole-pixel offsets and flips only**; everything else is a
    cel swap.
  - blink = eye-layer frames; look-at = 8-direction eye cels; speech = mouth
    flaps; squash and stretch = pre-drawn body frames; bob = whole-sprite
    integer offset.
  - Effects (hearts, zzz, sweat drop) are sprites too.
  - Liveliness comes from scheduling plus combinatorics (eyes × mouths ×
    bodies × accessories). The brain's continuous params are quantized to the
    pixel grid and to the clip fps (8–12).
- **Later:** a smooth `rig` backend through the same trait, only if a
  non-pixel mascot is ever wanted.
- **Screen awareness** (walking along window edges, sitting on title bars)
  needs other apps' window geometry, which brings permission prompts:
  - macOS: CGWindowList (titles need Screen Recording permission; bounds don't)
  - Windows: EnumWindows + DwmGetWindowAttribute

  Moved to **P3 or later**.

## 4. Windows, widgets, media, browser

### Mascot window

- **CPU composite by default** (a few blits per frame at 64×64), because pixel
  art doesn't need a GPU vector pipeline.
  - Windows: a layered window (`WS_EX_LAYERED` + `UpdateLayeredWindow`), which
    passes clicks through alpha-0 pixels by itself.
  - macOS: a CALayer fed from a CPU buffer.
  - wgpu and Vello leave v1, and the renderer trait stays.
- **Display scale is always a whole multiple of sprite pixels**: backing scale
  × user zoom, floored. Never fractional, or Windows at 125%/150% blurs it.
  64×64 at ×4–×8 is 256–512 device px.
- **Per-pixel hit test from our own alpha** on macOS: toggle cursor
  hit-testing when the cursor is over opaque pixels (as in Bevy's desk_toy).
- Low fps when idle, 0 when hidden or occluded.
- **macOS:**
  - a non-activating NSPanel
  - `collectionBehavior` = canJoinAllSpaces + fullScreenAuxiliary
  - accessory activation policy → a **menu bar app, no Dock icon** (open
    question)
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
- **Pixel-art survey (2026-11):**

  | Tool | Verdict |
  |---|---|
  | **Aseprite** (CLI + Lua; community MCPs such as `aseprite-live-mcp`, where the agent draws in the open editor with one undo per edit) | **The human editor and the file format.** Source build for dev only (D11). |
  | PixelAOI (MCP, reads and writes `.aseprite`) | Windows and Linux only, v0.6. Watch it. |
  | Pixelorama | No CLI (#579). Out. |
  | Piskel MCP, pixel-art-tutor (WebMCP) | Prototypes. |
  | PixelLab, Retro Diffusion | **Generators**, paid credits: concept drafts, rotations, style-matched variants that go through review; never the source of truth. |

  None of them has a real review step (comments, approval), so that part is
  ours. **Verdict: no GUI pixel editor of our own.** Aseprite is for human
  edits; our studio handles behavior and review, plus a headless pixel toolkit
  for agents.
- **Headless pixel toolkit, `buddy studio px …`: the agent path that always
  works**, with no Aseprite needed (D8/D11):
  - `grid`: a layer/cel as a palette-indexed text grid (64×64 ≈ 4 KB), edited
    and written back
  - layer ops: integer shift, mirror, recolor within the palette, outline,
    flood fill, copy frame/tag, swap cels
  - `sheet`: contact sheets with zoom, onion skin, and a mock desktop
    background at true display scale
  - `diff`: highlights changed pixels
  - `lint`: off-palette colors, stray pixels, non-integer offsets, missing tags
    or anchor slices, frame counts per tag
  - Aseprite's own `-b --script` Lua bridge is an optional extra when it's
    found on the machine.
- **Sprite spec for the artist:** one page covering canvas, palette, layer
  names, required tags, anchor slices and fps. It's the contract that lets
  agents derive the rest from the artist's base sprite.
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
- **Art (the hardest problem).** First an agent attempt (the D12 demo); most
  likely an artist's base sprite after that, with agents deriving clips, eye
  and mouth cels, effects and variants from it under the sprite spec.

## 6. Workspace layout

```
crates/
  buddy-app/           Tauri shell
  mascot-protocol/     MascotEvent, params, pack schema (versioned)
  mascot-core/         brain + runtime (scheduler, reflexes, temperament)
  mascot-render/       CPU composite + integer upscale (trait for later backends)
  host/                buddy CLI + loopback API (tokens, origin/host checks)
  policy/              modes, rules, taint (ported from 2market)
  bridge/              sidecar process + JSONL protocol
  widgets/             widget manager, manifest, JS SDK
  studio/              studio mode + headless render/audit
agent/                 TS sidecar (pi-durable + extensions + skills)
mascots/<name>/        packs
```

## 7. Spikes (with pass criteria), then phases

- **S0 mascot demo (D12, now):** an agent-drawn 64×64 original mascot as an
  `.aseprite` file with paper-doll layers and a handful of clips, rendered to
  GIFs and sheets at display scale for review. Its job is to judge whether
  agent pixel art is good enough, which shapes how much depends on an artist.
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
  - check whether pi-ai 1.5 has native search
- **S3 studio/format**
  - `asefile` vs `aseprite-io`: reading layers, tags, slices and user data;
    a byte-stable write round trip
  - headless composite of the S0 mascot to PNG/GIF, plus `diff` and `lint`
- **Media spike** (before P3)
  - IFrame embed from the loopback origin
  - audio keeps playing while hidden (or the off-screen parking fallback works)

**Phases:**
- **P1:** protocol + brain + one mascot + senses/reflexes (DND, quiet hours)
- **P2:** harness + permissions + bubbles + attention budget + user token
- **P3:** widgets (media, browser) + screen awareness
- **P4:** studio + mascot SDK for users

## 8. Open questions

All ten earlier questions are answered (D1–D10). Still open:
1. Does "64-bit pixel art" mean a 64×64 canvas? Assumed yes.
2. Does LibreSprite (a free fork of an older Aseprite) handle tags and slices
   well enough to recommend to users without Aseprite? Unverified.

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
