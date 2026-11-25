# Bear pack

The polar bear, our first mascot. `pack.json` (`pack_version: 2`) points at the Blender
pipeline output in `experiments/mascot/out/bear3d/` and reads it in place. Nothing is
copied. `mascot_core::pack::load("mascots/bear")` loads it, and `mascot-replay` shows what
the brain does with it:

```sh
cargo run -p mascot-core --bin mascot-replay -- --pack mascots/bear \
  --timeline crates/mascot-core/tests/fixtures/day.json --seed 7
```

All art comes from Blender (plan D13). Never draw sprites in code. Keep the bear chunky and
toy-like.

## How the pack works

- **Sources** (paths relative to `art_root`): Stage A free clips `anim/anim.json`, Stage B
  sets `anim_b/anim_b.json` + `props/props.json`. Four sources are optional:
  `anim_c/anim_c.json` (new free clips), `anim_l/anim_l.json` (listen and turn clips, p2),
  `anim_d/anim_d.json` + `props_d/props_d.json` (the desk set, p3) and `face/face.json` (face layers). If an optional source is missing, it is skipped. If it is
  present but broken, the pack doesn't load.
- **Behaviors** are clip sequences. A behavior is *available* when every clip it names
  exists: a free clip in at least `S`, a set clip in both orientations A and B.
- **Intents** map to fallback chains. The brain plays the first available behavior. Before
  the art round, `celebrate → happy → rest` resolves to `rest`, and `think` / `talk` / `ask`
  resolve to `hold` (idle, held). Once `anim_c` exists, the same chains pick the new clips
  without any pack edit.
- **Listen** (p2): while the Ask… prompt is open the brain plays the `listen` intent
  (`["listen", "rest"]`): `listen_in` once, `listen` held, `listen_out` once (`"exit": true`, so
  sending or closing plays `listen_out` at once). The listen clips exist for `S SE E W SW` only;
  from a back view the bear turns first: NE→E, NW→W, N→E or W (seeded). Until `anim_l` exists,
  listen is `rest` facing the person.
- **Turning** (p2): `"turning": {"ccw": "turn_ccw", "cw": "turn_cw"}` names the clips that turn
  the bear 45° (Stage A clips with `"turn_steps": 1` for ccw, `-1` for cw, in all 8 dirs). Every
  facing change the brain makes (listen, a set's approach, `look_around`, `wander`) plays them,
  shortest way first. Clips not rendered yet: the bear snaps (lint warns `turn_clips_missing`).
- **Stations** (p3): `"stations": {"desk": {...}}` turns a Stage B set into a place the bear
  works from. Each names its `set` and one set clip per role: `enter` (the sit, hand-off at its
  start), `loop` (typing, held), `to_user` (swivel to the person), `talk`, `ask`, `listen`
  (held; `listen` defaults to `talk`), `to_screen` (swivel back) and `exit` (stand up, hand-off at
  its end). Every role clip must be in the set, in both oris. A station is a behavior of its
  own name (no behavior may share it); it plays only while its set is loaded, so until
  `anim_d/` exists the bear thinks and answers standing (lint warns `station_art_missing`).
- **Work** (p3): `"work": {"station": "desk", "after_ms": 4000, "after_tools": 2}`. A turn goes
  to the station once it has run `after_tools` counted tools, or 1 counted tool and `after_ms`
  have passed since the turn began. Counted = every tool except `buddy …` (body, say, widgets)
  and media. It is seated until the turn ends: a card is `ask` from the chair (`to_user` first,
  `to_screen` back to `loop` when it settles), the answer is `talk` with the mouth layers. It
  stands (`exit`) after the answer bubble goes (`Settled`), or 120 s after the turn's end. An
  error, a drag or a do-not-disturb hide stands it at once; after a hide it stays standing for
  the rest of that turn. A click while seated listens from the chair.
- **Errors** are one line: `pack <dir>: <path>: <what>`, e.g.
  `pack /nonexistent: pack.json: not found`.

## Pack v2 (idle brain, aliveness 2)

`pack_version: 2` adds the keys below; a v1 pack still loads and plays exactly as before (a
v2 key in a v1 pack is lint error `v2_key_in_v1_pack`). The loader reads versions 1–2.

- **Optional sources for art that isn't rendered yet.** Each A4 row has a Stage A source
  `anim_<x>/anim_<x>.json` + a `face_part` `anim_<x>/face_<x>.json` (x = f g p m t e x r s w z),
  and each posture set a `stage_b` `set_<s>/anim_<s>.json` with its `props` file
  `set_<s>/props_<s>.json` + `set_<s>/face_<s>.json` (s = lean sit_a sit_b play out). All are
  `"optional": true` and resolve against `art_root` like every other source. A `face_part` adds
  its clips to the face layers once `face/face.json` loaded; a clip in two parts (or in
  face.json and a part, as a free or a set clip) is a load error. A part reads in the stage
  kit's shape (`"face_part": 1`, `clips` dirs without `patch` + a top-level `patch_dirs`,
  `exact` by dir/ori) or the prototype's (`face_part_version`, `patch` per dir, flat `exact`).
- **Hidden entries.** A pool, activity or emote entry whose clips are missing is *waiting for
  art*: weight 0, never picked, not counted as a behavior, left out of `buddy mascot list`
  (text and `--json`) and the prompt. The app logs one line at start, e.g.
  `[buddy] pack: N entries wait for art: <the first 12 names>, … (idle I, emotes E, activities A)`,
  and `buddy studio lint` lists each as a warning. Rendering the art turns an entry on with no
  pack edit. The pack picks and audits exactly as if they weren't there. Since p2 every A4
  entry of the bear has its art (nothing waits; `tests/bear_pack.rs` keeps it so).
- **`idle_pool` entries** gain `kind` (`stand fidget posture move long rare react other`,
  default `other`), `group` (the variety key; default the entry's name), `when`
  (`{"hours": [from, to)` local, wrapping midnight; `"months": [1..12]`; `"context": name}` —
  a context is eligible only while the brain holds it (p4, below), an unknown month never matches) and, instead of
  `behavior`, a station visit `{"station": "desk", "hold_s": [a, b]}`: the bear sits, types
  for a seeded draw of `hold_s` seconds (whole loops), then stands; traced as behavior `desk`,
  intent idle. A turn that opens while it types takes the seat over (the usual seated turn).
- **`"loops": {"for_s": [a, b]}`** on a behavior step holds whole loops until a seeded draw in
  `[a, b]` s has passed (`read` 40–180 s, `nap` 60–240 s). An activity's `--for` deadline wins.
- **`"variety"`** (optional; the bear's is below): `no_repeat` (never the same
  group twice in a row), `window` + `max_in_window` (≤ N picks of a group in any W picks in a row, this one included),
  `max_moves_in_row` (0 = off), and `targets` (what the audit checks: `minutes`, `stand_max`,
  `stand_move_max`, `distinct_min`, `share_max`, `long_len_s`). The rules are hard: when they
  bar every entry the brain relaxes `max_in_window`, then cooldowns, then plays `rest`
  (`look_around` after a rest), tracing `pick relaxed <rule>`; `no_repeat` is never relaxed.
  Every pick traces `pick <name> total <weight>`.
- **Turn first.** An idle behavior whose first clip lacks the current dir turns to its nearest
  dir through the turn clips before it plays (sets are skipped; no turn clips = snap).
- **Body.** `plays` is a string or an array, `about` is optional, and entries take a `group`:
  activities `pastime | play | posture | work`, emotes `gesture | fidget | move | reaction |
  surprise` (default pastime / gesture). `mascot list` and the prompt group by it.
- **Studio.** `buddy studio audit --pack <dir> [--minutes 10] [--seed N | --seeds 1,2,3]
  [--start 2026-11-16T10:00] [--temperament <json>] [--json] [--chart] [--out <dir>]` prints the
  v2 report `{pack, minutes, seeds, pass, runs[]}` (per run: picks, distinct, relaxed, sequence,
  time shares, behaviors, kinds, `long[]` lengths, violations, targets) and exits 0 pass / 1
  findings / 2 usage; `--timeline` keeps the v1 timeline. Lint adds `waits_for_art` (warn),
  `v2_key_in_v1_pack`, `canvas_too_big` (> 176×160) and `dir_set_mismatch` (errors); a bad
  station pool entry is a load error.

## The bear's idle life (A4, tuned in p2)

The idle pool holds every A4 behavior (stages f g p m t + lean) next to `rest`, `look_around`,
`wander`, `dash`, `read`, `nap` and the desk. Its `variety` block:

```json
"variety": {"no_repeat": true, "window": 20, "max_in_window": 2, "max_moves_in_row": 2,
            "targets": {"minutes": 10, "stand_max": 0.25, "stand_move_max": 0.45,
                        "distinct_min": 20, "share_max": 0.15, "long_len_s": [20, 300]}}
```

Tuning (start values from the storyboard's idle column, then):

- `rest` weight 2, `loops [1, 2]` (it was 6 and `[1, 3]`, most of the idle time).
- The long ones share `"group": "long"` (`read` 0.7 / 420 s cooldown, `nap` 0.6, desk 0.9, `lean`
  0.8): two long visits never follow each other and at most 2 in 20 picks, so a 10-minute
  stretch is never spent in two long ones back to back. The lying postures share
  `"group": "lie"` (`lie_belly`, `lie_back`, `floor_nap`) for the same reason; `look_up` /
  `look_down` share `look`, and `sit_floor` / `sit_sway` share `sit` (both sit down and stand up
  again, so one never follows the other).
- The six postures (`sit_floor`, `sit_sway`, `lie_belly`, `lie_back`, `floor_nap`, `squat`) carry
  `"exit": true`: a click, an agent turn or a reaction plays their get-up clip (`*_up`) before
  `listen_in`, instead of jumping from the floor to standing (`bear_pack.rs` checks it).
- Moves are `kind: "move"` (Stage A wander, dash and every A4 gait and tumble), at most 2 in a
  row.

Check it with `buddy studio audit --pack mascots/bear --minutes 10 --seeds 1,2,3` (AC11: stand
< 25 %, stand + walk + run < 45 %, ≥ 20 behaviors) and `--minutes 30` (AC17: no repeat, no
group > 15 %, ≤ 2 moves in a row, 0 relaxed picks); both exit 0. The gate is seeds 1,2,3: on
seeds 1–20 every 30-minute run passes, but 10 minutes is short enough that a seed drawing several
long visits can land under `distinct_min` (seed 14: 17 behaviors). An energetic
(`--temperament energy=1.0`) or sleepy run keeps 0 violations too. Quiet hours play only `nap`
and `rest`: those two can't keep 2 in 20, so the brain relaxes `max_in_window` (and a cooldown)
and traces `pick relaxed <rule>`, but never repeats one back to back (`bear_pack.rs`).

Contract notes (lead decisions for p1):

- Posture sets are `stage_b` sources with their `props` field and `art_root`-relative paths,
  not a separate `props` source kind; art-kit writes the same files either way.
- Lint keeps its report shape `{pack, files, errors, warnings, issues[]}` and the word
  `"severity": "warn"`; `waits_for_art` issues add `name`, `clip` and `source`. Warnings never
  change the exit code. The studio window (`studio.js`) reads it unchanged.

## The bear's gestures (A5, p3)

Stage e (`anim_e/`, gestures-a) and stage x (`anim_x/`, gestures-b) are one-shot clips in
`S SE E W SW` (84×123, idle#0 at both ends); each has a behavior of the same name.

- **Intents with their own clip.** `wave nod shake greet sad surprised jump dodge remind` play
  stage e's clip, `stretch` the p2 fidget (stage g) and `spin` the p2 tumble (stage t). Each
  chain is own behavior first, then a borrowed fallback, then `rest` (`wave nod → happy`,
  `shake sad surprised → oops`, `greet → wave → happy`, `jump spin → celebrate`,
  `dodge → dash`, `remind → ask`, `stretch → rest`), so a missing stage falls back with no
  pack edit. `bear_pack.rs` checks that each of the 11 resolves to its own behavior playing
  only its own clip (so no two share one).
- **Reminders.** A `reminder` event holds intent `remind` for the bubble's time, and `remind`
  is a one-shot (1.7 s, idle#0 at both ends), so the bear repeats the paw wag until the bubble
  goes (it held `ask` before).
- **Dodge** steps aside by its `root_px` (the floor camera scrolls under the bear, as for walks) and ends on idle#0 at
  the new spot. Lint warns `non_integer_root` on dodge S/E/W (sub-pixel iso steps), a warning only.
- **Gesture emotes** (group `gesture`): the intent emotes `wave nod shake jump spin greet
  stretch sad surprised dodge` and the `plays` emotes `remind` and stage x's `shrug facepalm
  thumbs_up clap point_bubble shy_hide laugh pout proud head_scratch`. `remind` is a `plays`
  emote because its intent falls back to `ask`, which holds, and an emote must end by itself.
  Without its art a `plays` emote waits (left out of `mascot list` and the prompt, named in the
  load line); an intent emote stays listed and plays its fallback.
- `point_bubble` and `thumbs_up` raise the left mitten (screen right in S, toward the bubble).
  `proud` is the subtlest gesture. Replay: `BUDDY_EVENTS=crates/buddy-app/tests/fixtures/emotes.jsonl`
  (every gesture emote once, 5 s apart).

## The bear's reactions (A6, p4)

Stages r (reactions), s (moments) and w (edge) give the bear clips that answer the desktop.
Two pack blocks wire them; both are data, so a pack without them reacts to nothing.

- **`contexts`** (names fixed in code): `music` (a player is playing), `edge` (sitting on a
  window's top edge; `"only": true`: nothing but `edge_*` entries), `evening` (`hours`
  [20, 23)), `away` (`after_min` 5 of user idle) and `away_long` (`after_min` 15, `only`,
  `"hold": "floor_nap"`: the nap's middle loop holds until input returns, then it gets up
  through `curl_up`). An `idle_pool` entry with `when.context` is eligible only while that
  context holds; the bear's are `kind: "react"` (`head_bob foot_tap dance` for music,
  `edge_dangle edge_peer edge_doze`, `evening_yawn`, `tap_glass` and `wave` for away).
- **`reactions`** (names fixed in code), each `plays` a behavior or a `chain` of them:
  `drag_hold` → `dangle` (held while dragged), `drag_land` → `land_squash`, `dizzy` (after a
  drag of `min_ms` 1500 and `min_pt` 600), `flinch` (a cursor rushing within `near_pt` 60 at
  `speed` 1500 pt/s; `cooldown_s` 20), `pet` → `giggle` (3 reversals over the bear within 2 s;
  cooldown 10), `follow` → `cursor_watch` (a still cursor within 120 pt for 1.5 s: the bear
  faces it, at most `max_s` 20, then `cooldown_s` 60), `morning` (`morning_stretch`, then the
  `greet` intent), `approval_wait` (`foot_tap` / `check_watch`, no repeat, `after_s` 8 into a
  standing permission card, then every 6–12 s; never at the desk, which keeps `desk_ask`),
  and `edge_land` → `edge_sit` (`edge_stand` on leaving).
- **Priority.** A click, a drag and the agent always win. A reaction waits for the running
  behavior's exit (a posture or the desk gets up first). Do-not-disturb and quiet hours stop
  every reaction and context entry except the drag dangle and the landing (with DND set to
  `Still` nothing plays at all: the frame stays pinned).
- **The cursor** is read only by the panel's existing near-poll (no new permission), as
  points from the bear's feet rounded to 8 pt, at most 10 Hz, and only within 180 pt. It goes
  to the brain alone: never to the sense ring, `BUDDY_SENSES_LOG`, a prompt, the agent or disk.
- **Reaction emotes** (group `reaction`): `giggle head_bob foot_tap flinch check_watch dizzy
  morning_stretch evening_yawn`; the activity `dance` (group `play`).
- **Nightcap** is in `quiet_hours.pool` (set `bed_cap` from stage out, p5: `cap_get_in_bed` →
  `cap_sleep` for 120–270 s → `cap_wake_up`), next to `nap` and `rest`, both boosted ×4. The quiet pool keeps
  only `idle_pool` entries, so nightcap has one too: `long`, `when.hours [23, 7]` (inside the default quiet hours
  22:30–07:30, so it is a quiet-hours pick only; quiet hours set elsewhere leave it out), kept out of the warm-up like `nap`.
- Replays: `crates/buddy-app/tests/fixtures/{music,edge_react,drag,cursor,time,away,approval_wait}.jsonl`
  (checked by `crates/mascot-core/tests/react_replays.rs`, also `BUDDY_EVENTS=`). A senses
  patch's `cursor` is read from a replay but never written back out (logs, the sense ring).

## The bear's props and surprises (A6, p5)

Set stages sit_a, sit_b, play and out (`set_<s>/`, ori A faces SE, ori B is its 3D mirror)
give 16 prop activities; stage z (`anim_z/`) gives 11 rare surprises. All are pack data.

- **Prop behaviors** are one set each, enter → loop (`loops.for_s`) → exit, exactly as
  `read` and `lean`: `tea` (tea_sit / tea_sip / tea_stand), `snack`, `knit` (floor cushion),
  `notebook` and `laptop` (a low toy tray-table is part of the set's prop layer), `headphones`
  (set `phones`), `umbrella` (baked rain inside the canvas), `ball`, `bubbles`, `yoyo`,
  `blocks` (the tower topples back to its first frame each loop), `balloon` (tethered),
  `plant`, `sweep`, `snowman`, `fishing`. Their colours outside the base palettes are
  `bear3d/palette-ext-out.json` (terracotta, straw, snow, carrot, ice, fish), listed in
  `palettes`.
- **In idle** each is a `kind: "long"` entry in `"group": "long"` (with read, nap, the desk and
  lean), so two long visits never follow each other and at most 2 come in 20 picks. Weights
  0.3–0.8 and cooldowns 600–1800 s are the storyboard's; `for_s` lower bounds are at least
  20 s so every visit lasts 20–300 s (`targets.long_len_s`). Conditions: `tea` only
  `when.hours` [14, 18), `snowman` and `fishing` only `when.months` [12, 1, 2]; `headphones`
  has a second entry `when.context: "music"` (the same behavior, so the same cooldown): twice
  as likely while music plays.
- **For the agent** each is an activity (`buddy mascot do tea [--for 5m]`, group `pastime` or
  `play`), plus `nightcap`. An activity ignores the idle conditions (the agent may ask for a
  snowman in July); do-not-disturb and quiet hours refuse it as any other.
- **Rare surprises** (`kind: "rare"`, one clip each, idle#0 at both ends): `windmill`,
  `hiccups`, `find_coin`, `snowflake_nose` (months 12–2), `butterfly_ear` (4–9), `peekaboo`,
  and the hats `hat_spring` (3–5), `hat_summer` (6–8), `hat_autumn` (9–11), `hat_winter`
  (12–2). Weights 0.15–0.3, cooldowns 2400–3600 s, so one shows up about once an hour at
  most. `moonwalk` (8 dirs, `loops [2, 3]`, backwards root motion) is `kind: "move"` so the
  two-moves-in-a-row rule counts it. Each is also an emote in group `surprise`.
- **Checks.** `crates/mascot-core/tests/bear_pack.rs` pins all of the above;
  `buddy studio audit --pack mascots/bear --minutes 120 --seeds 1,2,3 --start 2026-12-15T15:00`
  shows every long entry (the audit holds the start's hour and month for the whole run, so the
  start must be inside tea's hours and the winter months); `--start 2026-07-01T10:00` never
  plays a non-summer hat. The sheets: `experiments/mascot/out/bear3d/sheets.html`.

## The body block (what the agent can do)

`body` in `pack.json` lists what the agent may ask the bear to do. The agent sees it through
`buddy mascot list` and the prompt's "Your body" section (`mascot_core::body::prompt_text`),
so a new entry reaches the agent with no code change. Names are semantic; clip names never
leave the pack.

- **Activities** (`buddy mascot do <name> [--for 5m]`): `plays` is a fallback chain of
  behaviors (the first available one plays; its looping steps hold until the time is up),
  `kind` is `pastime` (default) or `work`, `about` is one line for the agent. An activity
  asked during a reply starts after it, lasts `default_for_s` (120) unless `--for` says
  otherwise, at most `max_for_s` (1800), and ends early on a click, `buddy mascot stop`, the
  next turn, or do-not-disturb / quiet hours. A behavior with `"exit": true` plays its last
  step (e.g. `read`'s `stand_up`) when it is cut short.
- **Emotes** (`buddy mascot emote <name>`): a one-shot, either `intent` (one of the brain's
  intents, as `wave`) or `plays` (a behavior chain). What it plays must end by itself: no
  `"hold"` step, and not `idle`.
- Rules: a name is 1–32 of `a-z`, `0-9`, `_`, `-`; `about` starts lower-case, has no
  trailing period and is at most 120 characters; `max_for_s` is at most 1800 (the CLI's 30m); every behavior named must exist and one of
  a chain must be available. A pack with no `body` gets the default block (`read`, `nap` and
  the five intent emotes, minus any it can't play). `buddy studio lint` lists each problem
  as a `bad_body` error, and the app refuses such a pack.
- A `"station": "<name>"` activity (the bear's `computer`, `kind: "work"`) sits at that station
  at once, even mid-turn (a work activity is never queued), and types there for its time. A turn
  doesn't end it: a reply is still answered from the chair, then back to typing. A click ends
  it (the bear listens from the chair). Until the station's art is loaded it isn't listed.
- Do-not-disturb, quiet hours and the tray's hide refuse `do` and `emote` (status
  `refused`, nothing plays); `stop` and `list` always answer.

## The personality (A7, D17)

`persona` in `pack.json` is how the mascot talks. It is optional: a pack without it talks in the
app's plain voice and its check-ins keep the old wording. It changes tone only, never tools,
approvals or the policy.

```json
"persona": {"default": "biscuit", "presets": {"biscuit": {
  "name": "Biscuit", "tagline": "cheeky sidekick", "identity": "…",
  "voice": ["3 to 6 short rules"], "likes": ["…"],
  "catchphrase": {"text": "told you so", "when": "…"},
  "gestures": {"greeting": "wave", "done": "thumbs_up", "error": "head_scratch", "checkin": "stretch", "disagree": "shrug"},
  "examples": {"greeting": "…", "done": "…", "error": "…", "checkin": "…", "disagree": "…"},
  "idle_lines": {"morning": ["…"], "evening": ["…"], "back": ["…"], "long_session": ["…"]},
  "temperament": {"energy": 0.6, "curiosity": 0.6, "shyness": 0.1, "chattiness": 0.5, "sleepiness": 0.4},
  "style": {"playfulness": 0.7, "emotes": 0.5}}}}
```

- **Presets.** The bear has four, all original: **Biscuit** (default; cheeky sidekick: dry
  one-liners, kind teasing, honest to a fault, warm underneath), **Mochi** (cozy homebody),
  **Pip** (curious explorer), **Tofu** (shy, earnest helper). A preset sets the voice, examples,
  idle lines and temperament together. The pack's top-level `temperament` equals Biscuit's.
- **Where it goes.** `mascot_core::persona::prompt_text` renders the prompt's "## Who you are"
  section: identity, voice rules, likes, catchphrase and when to use it, style bands, gestures and
  the examples (bubble length, the tone to match). `examples` are also the Customize window's
  preview. `idle_lines` are canned lines the app says with no model call (below).
- **Placeholders** in example and idle lines: `{me}` (the name), `{you}` (what the person asked to
  be called; dropped with its comma when unset: "Hi, {you}!" → "Hi!") and `{catchphrase}`
  (capitalised at a sentence start). The rule is in `mascot_core::persona::fill`, and
  `crates/buddy-app/web/persona/fill.js` must match it (`crates/mascot-core/tests/fixtures/persona_fill.json`
  pins both).
- **Rules.** Preset ids are `^[a-z][a-z0-9_]{0,23}$`, and `default` names one. `name` and
  `identity` aren't empty. `voice` has 3–6 rules, all five `examples` are set, each moment has 1–8
  idle lines, and every `gestures` entry is an emote in `body`. Caps: name 24, tagline 32, identity
  120, voice rule 80, like 32 (at most 8), catchphrase 40, `when` 80, example 160, idle line 100.
  An unknown `{x}` placeholder is a problem. `buddy studio lint` reports `bad_persona`,
  `bad_persona_gesture` (with "did you mean"), `bad_persona_default`, `bad_persona_length` and
  `bad_persona_placeholder`. The app refuses such a pack, and the studio refuses a save that adds one.
- **The person's layer** (`settings.persona`, settings v3): `preset`, `name`, `call_me`,
  `catchphrase`, `chattiness`, `playfulness`, `emotes` (0–1) and `notes` (≤ 500 characters). `null`
  follows the preset. The preset owns name, catchphrase and the three sliders; `call_me` and the
  notes are the person's and survive a preset switch. `pack` records the mascot the layer was made
  for: when another mascot is on the desktop at launch, the preset and the fields it owns are
  cleared (`call_me` and the notes stay). The notes go into the prompt as a quoted,
  tone-only block. Effective temperament = the pack's, then the preset's, then the person's
  chattiness; it drives the brain too.
- **Idle lines.** Morning, evening (18:00–22:00, once a day), back from away and a long session.
  Cooldown `40 − 20 × chattiness` minutes (±10%), said with probability `0.4 + 0.6 × chattiness`, no
  line twice in a row. Never during do-not-disturb, quiet hours, fullscreen or a screen share, a
  hidden bear, a turn or an open bubble, and never queued for later. They don't count against the
  4-an-hour check-ins. The log says only `[buddy] idle line <moment>`. Lines carry mood, never facts.
- **Tools.**
  - `buddy studio persona render --pack <dir> [--preset <id>] [--user '<json>' | <file>] [--json]`
    prints the exact "Who you are" text.
  - `buddy studio audit --pack <dir> --preset <id> …` audits with that preset's temperament. All
    four bear presets pass at `--minutes 10` and `30` on seeds 1,2,3.
  - `bun run agent/scripts/persona-eval.ts` (needs `BUDDY_EVAL=1` and a real key; never in CI)
    tries each preset on a bad plan, an error and a sad message.

## Sprite spec (every clip the art round adds)

| Rule | Value |
|---|---|
| Canvas | 84 × 123 px per frame, frames side by side in one strip PNG per direction |
| Anchor | `[42, 107]`: the floor point between the feet (same as `idle`) |
| Camera | iso 2:1 ortho, X60 Z45, the Stage A camera; ppu 40.07 |
| Directions | 8 screen directions `S SE E NE N NW W SW` (S faces the viewer), one strip each |
| Timing | 12 fps; per-frame `dur` in ms (83) |
| Root motion | per frame and direction `root_px` = screen-pixel move after showing that frame (y down); in-place clips use `[0, 0]` |
| Alpha | 0 or 255 only |
| Outline | 1 px ink outline |
| Palette | `experiments/bear-proto/palette-bear.json` + `experiments/scene-proto/palette.json` (+ `bear3d/palette-ext.json` for furniture) |
| Hand-off | frame 0 of every clip, and the last frame of every one-shot, is pixel-identical to `idle` frame 0 in that direction (rest pose) |

`anim_c/anim_c.json` uses the exact Stage A schema of `anim/anim.json` (`fps`, `ppu`,
`directions`; per clip `loop`, `fps`, `frame_w`, `frame_h`, `anchor`, `frames_n`, `files` for all 8
directions, `frames[].dur` + `frames[].root_px`).

## Face layers (D16, `face/face.json`)

The body strips keep their baked default face, so `verify_b.py` hand-offs stay
pixel-identical. Eyes and mouth are **opaque patches** drawn over it:

- Directions with a face: `S SE E W SW`. `N NE NW`, the Stage B sets and `walk_64` draw none.
- Slots: `eyes` (`open`, `closed`; default `open`) and `mouth` (`closed`, `open`, `wide`;
  default `closed`). Every state of one slot and direction has the same w × h and the same
  opaque mask, with alpha 0/255 and palette colors only.
- `layers[slot][state][dir]` = one PNG patch. `clips[clip][dir][slot]` = one integer top-left
  position per body frame (length `frames_n`), in frame pixels. This covers every free clip of
  `anim` and `anim_c`.
- v2 (`"face_version": 2`, p2) adds `patch_dirs[clip][body dir] = patch dir`: a clip whose head
  turns to the camera draws another dir's patches at its own positions. `listen`: S→S, SE→S,
  SW→S, E→SE, W→SW. v1 files still load.
- v2 also adds `set_clips[clip][ori] = {"patch": <dir>, <slot>: [[x, y], …]}` for set clips the
  bear talks in (`desk_talk`, `desk_ask`): the patch dir's layers (the desk's are `S`, the head
  turned to the camera), drawn at one top-left per frame measured on the set clip's frame. Counts
  equal the clip's `frames_n`; only set clips may be listed.
- Invariant (`verify_face.py`): body ∘ default eyes ∘ default mouth at those positions ==
  body, pixel for pixel.

The brain blinks (about every 4.2 s, jittered, sometimes twice) and flaps the mouth while
text streams, but only on clips and directions that have positions. Until the art round
delivers `face/`, the bear's eyes stay open.

## Art-round requests

| Clip | Kind | Plays for |
|---|---|---|
| `think` | loop | thinking, working (held) |
| `talk` | loop | streaming an answer, `buddy say` (held) |
| `ask` | loop | an approval card or a reminder waiting (held) |
| `oops` | one-shot | an error, sad, shake |
| `happy` | one-shot | happy, wave, nod, greet |
| `celebrate` | one-shot | celebrate, jump |

Also needed:

- **Rest-pose hand-off** for all six clips (see the sprite spec).
- **Face layers** per the section above, re-exporting face positions for the existing
  `anim` clips too (`idle`, `walk*`, `run*`).
