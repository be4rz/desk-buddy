# Bear pack

The polar bear, our first mascot. `pack.json` (`pack_version: 1`) points at the Blender
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
  sets `anim_b/anim_b.json` + `props/props.json`. Three sources are optional:
  `anim_c/anim_c.json` (new free clips), `anim_l/anim_l.json` (listen and turn clips, p2) and
  `face/face.json` (face layers). If an optional source is missing, it is skipped. If it is
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
- **Errors** are one line: `pack <dir>: <path>: <what>`, e.g.
  `pack /nonexistent: pack.json: not found`.

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
- Do-not-disturb, quiet hours and the tray's hide refuse `do` and `emote` (status
  `refused`, nothing plays); `stop` and `list` always answer.

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
