# Room packs

A room pack holds the floors and pocket scenes drawn around a mascot. A floor is the patch the
mascot stands on. A scene is furniture and decorations only: it has no floor and no walls of its
own, it is drawn over whichever floor is selected (or over none), and its windows, shelves and
pictures hang where a wall would be. It belongs to no mascot:
any mascot pack whose behaviors or sets carry the names in a scene's `for` list gets that scene.

```
rooms/<id>/
  room.json
  floors/<floor>.png
  scenes/<scene>_<variant>.png         back layer, drawn behind the mascot
  scenes/<scene>_<variant>_front.png   optional front layer, drawn over the mascot (same size)
```

## room.json

| key | meaning |
| --- | --- |
| `room_version` | `1` |
| `id`, `name` | the folder name and a display name |
| `palettes` | palette files the art uses (read by `verify_env.py` only) |
| `floors[]` | `id` (not `grid` or `none`: those are the app's own choices), `name` (shown in the tray's Floor menu), `file`, `origin` |
| `scenes[]` | `id`, `name`, `for`, `origin`, `variants[]` |
| `variants[]` | `name`, `back`, optional `front`, optional `hours` |

- `origin` is the pixel of the image that sits on the mascot's floor anchor (its feet).
- `for` lists behavior names or set names. The scene shows for the whole behavior, on top of
  the selected floor. A name may belong to one scene only.
- `hours` is `[from, to)` in local hours, and may wrap midnight (`[20, 5]`). A scene's timed
  variants must cover the day exactly once; a scene with one variant needs no `hours`.
- For a set played in placement B (the mirror of A) the scene is mirrored about the origin.
- Pixels are alpha 0 or 255, in the listed palettes, and never touch the image edge.

The loader (`mascot_core::room::load`) refuses a pack with a missing file, a path that leaves the
folder, a duplicate or reserved id, a name that is not `a-z 0-9 _ -` (1 to 32), bad hours or a behavior
claimed by two scenes. `verify_env.py` checks the same rules (and is a little stricter). A `for` name the mascot has no behavior or
set of is not an error (rooms are mascot-independent); the app logs it at launch.

## The starter room

`rooms/starter/` is written by `experiments/env-proto/export.py` from the Blender renders there.
Do not edit its files by hand. A rebuild gives the same bytes every time:

```
cd experiments/env-proto
sh build.sh               # render (Blender), pixelize, write rooms/starter/, verify (~2 min)
python3 verify_env.py     # the check alone: exit 0 = clean
```
