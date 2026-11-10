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
  sets `anim_b/anim_b.json` + `props/props.json`. Two sources are optional:
  `anim_c/anim_c.json` (new free clips) and `face/face.json` (face layers). If an optional
  source is missing, it is skipped. If it is present but broken, the pack doesn't load.
- **Behaviors** are clip sequences. A behavior is *available* when every clip it names
  exists: a free clip in at least `S`, a set clip in both orientations A and B.
- **Intents** map to fallback chains. The brain plays the first available behavior. Before
  the art round, `celebrate → happy → rest` resolves to `rest`, and `think` / `talk` / `ask`
  resolve to `hold` (idle, held). Once `anim_c` exists, the same chains pick the new clips
  without any pack edit.
- **Errors** are one line: `pack <dir>: <path>: <what>`, e.g.
  `pack /nonexistent: pack.json: not found`.

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
