# Handover checklist (AC13)

Run these on your own Mac, at your own desk. They are the checks an agent can't do: they need a real
desktop, real displays, your eyes and ears, a real keychain, or a real account. Every other check runs
in `cargo test --workspace`, `bun test` and the `.babysit/qa.yaml` harness.

Tick each box when the **Expect** line holds. If one fails, note what you saw next to it.

Setup, once:

```sh
cd ~/orca/desk-buddy
export CARGO_TARGET_DIR=$PWD/target
cargo build -p buddy-app -p host --bin buddy -p studio
```

Unless a step says otherwise, start the app with `cargo run -p buddy-app` and quit it from the tray
(**Quit desk-buddy**). Your normal settings live in `~/.desk-buddy/`. To try something without touching them,
add `BUDDY_HOME=$(mktemp -d)` in front of the command.

## 1. Desktop (AC3)

- [ ] **Click-through.** Open a text editor and put the bear over it. Click on the transparent pixels
  around the bear, including the floor area under it.
  **Expect:** the clicks reach the editor. A click on the bear itself is caught by the bear and makes
  it react.
- [ ] **Drag and persisted position.** Drag the bear somewhere else, quit from the tray, then
  `cargo run -p buddy-app` again.
  **Expect:** it comes back where you dropped it. `~/.desk-buddy/settings.json` holds the position.
- [ ] **Tray.** In the menu-bar icon, try each item: Size ▸ Small/Medium/Large, Hide bear (and show it again),
  When busy ▸ Hold still / Hide, Do not disturb, Quiet hours, Move to next display, Ask…, Reset web caution
  (only while the chat has read a web page), Approvals ▸ Ask every time / Ask only when unsure,
  Accounts…, Studio…, and Quit desk-buddy.
  **Expect:** each one does what it says, and the size is still set after a relaunch. There is no Dock
  icon (only `buddy-app --studio`, a separate window app, has one).
- [ ] **Look and feel.** Leave the bear on your own wallpaper (try a light one and a dark one) for a
  few minutes.
  **Expect:** the soft floor vignette under it reads as a shadow on both wallpapers, not as a box or
  smudge, and the bear looks cute and toy-like (D2/D13).
- [ ] **Fullscreen Spaces.** Put a normal app (not a video or game) in fullscreen in its own Space.
  **Expect:** the bear stays on top there, and stays when you swipe between Spaces.
- [ ] **Mixed DPI.** With a Retina and a 1× display, use **Move to next display** (or drag the bear)
  to the 1× screen, then back.
  **Expect:** the pixels stay crisp, with no blur or half-pixel seams, and the bear is the same size
  on both screens at the same zoom.

## 2. Idle CPU on a quiet machine (AC3)

- [ ] Close builds and other heavy apps. Then run `BUDDY_STATS=1 cargo run -p buddy-app` and leave
  the bear alone for 2–3 minutes while it walks, naps and rests.
  **Expect:** the `[buddy] fps … cpu x.x% …` line every 10 s shows **CPU under 1%** for a held pose and for
  animated idles (walk, nap). Earlier QA read 0.6–2.1% while animating, but on a busy machine
  (load average 3–13), so this needs a quiet-machine number. The same figure in Activity Monitor
  is fine too.
- [ ] Open the studio from the tray (**Studio…**), then close it, and watch the stats for another
  minute.
  **Expect:** after the window closes, CPU is back to what it was before it opened.

## 3. Art (AC5)

Art is reviewed by eye. The pixel rules and the face invariant are already proven by `verify_b.py`
and `verify_face.py`.

- [ ] **Preview pages.** Run
  `python3 -m http.server 8771 --directory experiments/mascot/out`, then open
  <http://localhost:8771/bear3d/anim/preview.html>, `…/anim_b/preview_b.html` and
  `…/anim_c/preview_c.html` (Stage C clips and face sheets).
  **Expect:** think, talk, ask, oops, happy and celebrate in all 8 directions read as cute, chunky and
  toy-like, on-model with the reference drawings (`experiments/bear-proto/ref/`), and never like
  realistic bear anatomy.
- [ ] **The subtle "ask" paw.** In `preview_c.html`, look at `ask` in the front views (S, SE, SW).
  **Expect:** you can tell the bear is asking. The paw is raised only to cheek height, and the art
  handoff flagged it as subtle from the front. Decide whether it reads, or should be raised in a
  re-render.
- [ ] **Face layers on the desktop.** Run
  `BUDDY_EVENTS=crates/buddy-app/tests/fixtures/events.jsonl cargo run -p buddy-app`.
  **Expect:** blinks look natural, and the mouth flaps while it speaks ("Hi there" at about 9 s).
  The face never slides off the head, and there are no stray pixels around the eyes.
- [ ] **In the studio.** Run `cargo run -p buddy-app -- --studio mascots/bear` and step through
  `ask`, `talk` and `happy` in each direction. Use the scrub bar, ←/→ and ×1–×4 zoom, and the face
  toggles for eyes open/closed and mouth closed/open/wide. Add one comment and one region on a frame.
  **Expect:** what you see matches the preview pages pixel for pixel. The comments land in
  `mascots/bear/review/comments.jsonl`, and `buddy studio comments --pack mascots/bear --open` lists
  them.

## 4. Talk (AC6)

- [ ] **Bubble keyboard focus.** Sign in first (section 5), then tray → **Ask…**.
  **Expect:** the prompt has keyboard focus at once, without a click. ⏎ sends, and Esc closes it.
  After either, the keyboard goes back to the app you were in before.
- [ ] **The bubble on the desktop.** Ask something with a longer answer. Then drag the bear near the
  top and the bottom edges of the screen.
  **Expect:** the answer streams in while the mouth flaps. The bubble's tail points at the bear, the
  bubble flips above or below near screen edges, and it follows the drag. Hovering it holds it open.
- [ ] **Approval card.** Ask for something that runs a command, such as "list the files in my
  Downloads folder".
  **Expect:** a card appears in the bubble. Deny means nothing runs. Allow runs it, and the bear shows
  its asking pose while the card waits.
- [ ] **Reminder.** Run `buddy remind --in 1m "stretch"` from your own terminal.
  **Expect:** a minute later, a bubble with the `⏰ reminder · set 1 min ago` chip. **Snooze 10 min**
  works.
- [ ] **`buddy say` from outside.** Run `buddy say "hello"` from your own terminal while the app runs,
  then again after you quit.
  **Expect:** first, a bubble saying "hello". After quitting: "desk-buddy isn't running".

## 5. Accounts (AC8)

- [ ] **Real keychain test.** Run `BUDDY_KEYCHAIN_TEST=1 cargo test -p bridge keychain`.
  **Expect:** macOS asks for keychain access (allow it), and the test passes.
- [ ] **Keychain prompts on unsigned rebuilds.** Change any line in `crates/buddy-app`, then rebuild
  and launch while signed in.
  **Expect:** macOS asks again for access to the `desk-buddy` keychain item, because the binary is
  unsigned; "Always Allow" answers it. Note whether the bear and tray freeze until you answer (known
  limitation V4).
- [ ] **Live API-key sign-in.** Tray → **Accounts…** → a provider → **Add key**, and paste a real key.
  **Expect:** the tray shows "Ready · <model>", and a question gets an answer. The key is in
  Keychain Access under service `desk-buddy`, and nowhere in `~/.desk-buddy`.
- [ ] **Live OAuth sign-in.** Sign in with one OAuth provider: the Anthropic code flow, or GitHub
  Copilot's device code.
  **Expect:** the browser opens, the code field stays visible in the window, and sign-in finishes with
  "Ready · <model>".
- [ ] **Relaunch.** Quit, then launch again.
  **Expect:** the credential loads from the keychain, with no new sign-in.
- [ ] **Looks.** The Accounts window at its native size, its focus ring, and the tray lines
  "Check-ins: N of 4 this hour" and "Check-ins paused (…)".

## 6. Do Not Disturb (AC4)

- [ ] **Real fullscreen.** Play a video fullscreen (or start a game).
  **Expect:** the bear goes still or hides, as set, and starts no unprompted turn. When you leave
  fullscreen it comes back.
- [ ] **Real screen share.** Share your screen in a real meeting app.
  **Expect:** the same as fullscreen. This heuristic has only been tested on synthetic fixtures, so
  this is its first real run.
- [ ] **Tray Do not disturb and Quiet hours.** Toggle each one.
  **Expect:** the bear stays still with no unprompted turns, and the tray line shows the pause.

## 7. Widgets (AC9, AC10)

- [ ] **Audible sound.** Run `buddy music play "lofi hip hop"`.
  **Expect:** you hear it. The fake player used in tests is silent.
- [ ] **Hidden YouTube playback.** While it plays, hide the media widget with its – button.
  **Expect:** the music keeps playing. `buddy music pause` and `buddy music play` still work, and
  `buddy music status --json` reports it.
- [ ] **Music/Spotify Automation prompt.** With Music or Spotify already open and playing, run
  `buddy music pause`.
  **Expect:** the first time, macOS asks to let desk-buddy control the app. Allow pauses it. Then
  deny it in System Settings → Privacy & Security → Automation and run the command again: it fails
  with a clear message and nothing crashes.
- [ ] **Edge sitting.** Drop the bear on the top edge of a real window.
  **Expect:** it sits on the edge but never over the middle of the title bar, and follows the window
  when you move it. It gets off when the window closes or goes fullscreen.
- [ ] **Widget windows.** The media and browser widget windows at native size, including **Take over**
  and **Hand back** in the browser widget.

## 8. Studio (AC11): looks only

The agent checked the studio page with mocks in a browser, and every studio command headless. These
need eyes on the real window.

- [ ] Run `cargo run -p buddy-app -- --studio mascots/bear`.
  **Expect:** one "desk-buddy studio" window. Its title bar shows "‹ Mascots", the name, a Built-in
  pill and a Review | Edit switch, with a green notice that offers **Customize…**. Below are the clip
  picker on the left, the preview with its scrub bar and the matching `buddy studio …` commands in the
  middle, and the Comments, Simulator, Lint and Audit tabs on the right. Closing the window quits it.
  The normal app can run at the same time. `--studio` writes nothing under `~/.desk-buddy` until you
  create, save, delete, customize, restore or put a mascot on the desktop.
- [ ] **Library.** Use a scratch home: `BUDDY_HOME=/tmp/bh cargo run -p buddy-app -- --studio`.
  **Expect:** the Mascots page with Built-in (the polar bear, with a thumbnail) and Yours (only the
  dashed "New mascot" card). **New mascot ▸ From an .aseprite file**: drop or choose
  `crates/studio/tests/fixtures/blob.aseprite` and name it "Blob". It opens in Edit, and walks in
  Review. Back in the library, the Blob card offers Delete…, and the polar bear's card doesn't.
- [ ] **Edit.** On Blob: change the name and save (⌘S). The title changes. In Advanced, rename a
  clip in a behavior to one that doesn't exist. **Expect:** a red "This edit doesn't load" with the
  reason, Save is refused, and `/tmp/bh/packs/blob/pack.json` is unchanged. Discard brings the last
  saved text back.
- [ ] **Customize and restore.** Open the polar bear, press **Customize…** and confirm (copying ~36 MB
  takes a few seconds). **Expect:** a Customized pill, and the edit sections unlocked. Change the
  blink timer and save. Then **Restore original…**: the pill goes back to Built-in and
  `mascots/bear/pack.json` was never touched (`git status mascots/` is clean).
- [ ] **Delete.** Delete Blob after its confirmation. **Expect:** the folder is gone from
  `/tmp/bh/packs`. If Blob was on the desktop ("Put on desktop"), settings go back to the bear.
  Remove `/tmp/bh` afterwards.
- [ ] **Hot reload.** With the studio open, edit `mascots/bear/pack.json` to break it (for example,
  delete a `"` character), then save.
  **Expect:** within a second, a pink "Reload failed: …" banner, while the last good pack keeps
  playing. Undo the edit: the banner goes and the "reloaded hh:mm:ss" pill appears.
- [ ] **Simulator.** In the Simulator tab, press Run, then click **thinking**, **say happy** and
  **DND**, with "preview follows" on.
  **Expect:** the preview switches to the clip the brain picks, the trace lists the events, and the
  live bear on your desktop does nothing (the simulator has its own brain).
- [ ] **Audit.** Run the Audit tab with 30 min and seed 3.
  **Expect:** the tab is the one-seed quiet-day chart (a v1 audit: distinct, max same in a row,
  entropy, starts). Its numbers match the v1 CLI run on an empty 10:00 timeline:
  `echo '{"timeline_version":1,"seed":3,"duration_ms":1800000,"start_local":"10:00","weekday":0,"items":[]}' > /tmp/q.json`,
  then `buddy studio audit --pack mascots/bear --timeline /tmp/q.json --out /tmp/a` (`/tmp/a/audit.json` +
  `audit.png`). Without `--timeline` the CLI writes the v2 variety report instead
  (`{pack, minutes, seeds, pass, runs[]}`, exit 0 pass / 1 findings); for the bear,
  `buddy studio audit --pack mascots/bear --minutes 30 --seeds 1,2,3` exits 0.

## Known limitations to judge

These are known and unfixed. Each needs your call, not more QA. Some are design changes, others are
small follow-ups.

**Security and agent**
- `web_fetch` DNS-rebind TOCTOU: the host name is resolved once to check it and again to fetch it.
- After taint, the default `buddy say`, `mascot` (do, stop, list, emote), `status` and `remind` run without a card.
  Outside ask mode, `web_fetch` and `web_search` also run without one, so a query string can carry
  data out.
- In ask mode, a research helper's own web calls run without a card, and the `task` card doesn't say so.
- Commands that the sidecar starts detached can outlive a killed sidecar.
- Sense turns run with the full tool set and policy, so a card can appear unprompted (V13).
- The policy accepts `allow_always` on a file Write/Edit card in a tainted chat. Only the bubble refuses it.
- Widgets: tainted agent browser actions are gated only by the policy's shell-command scan (L1). The
  agent can drive the browser widget while it's hidden (L2). Music and now-playing titles reach the
  agent without marking the chat (L3).

**Accounts**
- Fixed in p5, each with a unit test:
  - the keychain loads off the main thread (V4);
  - a foreign keychain item is skipped and logged (V3);
  - sign-out during a refresh stays signed out (V6);
  - Try again after a failed sense turn retries that turn (V5);
  - the reminder chip's age is computed at show time (V11).
- Still open, by design: if the keychain refuses to save a *refreshed* token, it's kept in memory for this session
  and saved at the next refresh, so a rotated token is never lost (V7/V8). A first sign-in that can't be saved is not
  kept. Only a real keychain shows this (§5).

**Talk**
- Fixed in p4, each with a test: an Error waits while you type; Ask… shows a hidden bear first; a failed harness start
  shows its error in the tray; a submit mid-turn stops the old answer first. Nothing open here beyond the "not proven"
  items under Widgets.

**Brain and senses**
- The first nap often comes soon after the 3-minute warm-up, so a first look may find the bear
  asleep. Judge whether that's charming or too early.
- A hide driven by fullscreen or screen share prints nothing on stderr.
- `cue` holds one entry, so an emote or reminder can replace a timed Say's follow-up emotion.
- The panel keeps its own fullscreen observer and CGWindowList read, apart from the senses (a
  senses follow-up).

**Widgets (p5 nits)**
- `buddy browser type --enter` strips `--enter` anywhere in the text. Edge-follow can hop to another
  window of the same app. After a failed harness start the widgets wait 5 s. A widget that fails to
  load is skipped with only a stderr line.
- The media error page centres "Pick another"; the prototype right-aligns it.
- `buddy music status` says `hidden: true` (the page's own view) while `buddy widget list` says
  `visible: true`.
- Not proven: the bubble's `ours` flag isn't reset at Idle/Error (V5 residual), and the 75 s client
  timeout applies to every widget call.
- Fixed in p6: `buddy browser goto` to a refused address now errors ("refused navigation to …"), and
  now-playing titles keep only plain words.

**Distribution**
- `macOSPrivateApi: true` (for the transparent bubble) rules out the Mac App Store. This matters only
  when distribution (D8) arrives.

**Studio (p6)**
- Lint on the bear: 0 errors, no stray or off-palette pixels, and 26 `non_integer_root` warnings
  (walk and run move a fractional number of pixels per frame). They are warnings, not errors; judge
  whether the root motion should be whole pixels.
- The studio's library lists the packs it found (yours, built-in, and the start folder), with no
  "Open folder…" button: no path ever comes from the window. To open another folder, start it with
  `buddy-app --studio <dir>`; it opens read-only, and **Duplicate** copies it into your library.
- "Put on desktop" applies at the bear's next launch; the running bear doesn't swap.
- There is no pixel editor (plan §5). Art comes in as `.aseprite` clip groups; retouch PNG strips
  outside the studio via "Show in Finder".
- `audit.png` has no text labels (no font in the renderer). The studio's Audit tab and `audit.json`
  carry the names.
- `--studio` with a folder that isn't a pack opens the studio's library with the error, and
  doesn't exit 1. The normal app still exits 1 on a bad pack.
- Studio follow-ups from the p6 review (small): a file rewritten during the ~1 s reload can be missed
  until the next change; if the start pack failed to load, fixing it doesn't refresh the window (pick
  it again); closing the window mid-lint can stall the bear for under half a second; a pack that ships
  a symlinked `review/` folder has its comments written through the link; `import-aseprite` reads
  the whole file before checking its size; `--json` usage errors (exit 2) print plain text.
