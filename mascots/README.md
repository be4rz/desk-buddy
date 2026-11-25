# Mascots

Each mascot is one folder. Its manifest `pack.json` and its schema are described in
[bear/README.md](bear/README.md). This file is the **package standard**: the shape a mascot
folder must have to be shared, kept in the studio's library, edited or deleted there.

## Package standard

A standard package is one self-contained folder:

```
<id>/
  pack.json        the manifest (schema: bear/README.md); "art_root": "art"
  art/<group>/     the art: source JSON + PNG strips (one group per import or Blender stage)
  palettes/*.json  the palettes pack.json lists
  README.md        optional
  review/          optional; studio comments (review/comments.jsonl), not part of the mascot
```

Rules (each one is a finding of `buddy studio package check`):

- The **folder name is the id**: `[a-z0-9][a-z0-9_-]{0,31}`, and `pack.json`'s `id` equals it.
- `name` isn't empty. `description` and `author` are optional strings the studio shows and edits.
- `art_root` is `"art"`. Every `sources[].path` / `props` is relative to it: no absolute paths,
  `..`, backslashes or drive letters.
- `palettes` are `palettes/<name>.json`.
- Nothing else at the top level (dotfiles are ignored). No link points outside the folder, and
  every file the pack reads resolves inside it.
- The pack loads (`mascot_core::pack`), has no `body` or `persona` problems and draws (`mascot-render`).
  `bundle` refuses a source with `persona` problems.

The repo's `bear/` is a **linked dev pack**, not a package: its `art_root` points at the Blender
output in `experiments/mascot/out/bear3d/`, so a re-render shows up without a copy. Bundling it
makes a package that draws the same pixels.

## Commands

```sh
buddy studio package check <dir> [--json]          # exit 0 = standard, 1 = findings (each named), 2 = usage
buddy studio package bundle --pack <dir> --out <dir>/<id> [--id <id>] [--name <name>] [--json]
```

`bundle` turns any loadable pack into a package. It copies only the files the pack reads: art
keeps its layout under `art/`, palettes go to `palettes/`, and `README.md` comes along, `review/`
doesn't. It rewrites `art_root`, `palettes` and the source paths, then checks the copy in a
staging folder before moving it into place. `--id` and `--name` rename the copy. When anything
fails, nothing is written.

## The library

The studio keeps your mascots in `BUDDY_HOME/packs/<id>/` (default `~/.desk-buddy/packs`). The app
and the studio look for a pack id there first, then in the built-in roots (`$BUDDY_PACKS`, and the
repo's `mascots/` in debug builds).

- **Yours**: a library folder with no built-in of the same id. You can edit it or delete it
  (after a confirmation).
- **Built-in**: shipped and never written. **Customize** bundles it into the library under the
  same id; that copy (**customized**) is used instead of the shipped one. **Restore original**
  deletes the copy. A built-in can't be deleted: the studio doesn't offer it and the backend
  refuses it.
- **From a folder** (`--studio <dir>` outside the roots, or a library folder whose name isn't an
  id): read-only and never put on the desktop; **Duplicate** makes a copy in the library.

Every studio write is staged and loaded first. An edit that would stop the pack loading, such as
a behavior naming a missing clip, broken JSON, a changed id or removing a clip group still in
use, is refused, and the file on disk stays as it was. Deleting the mascot on your desktop sets
settings `pack` back to `bear`. A settings pack found nowhere at launch runs the bear with
`[buddy] mascot "<id>" not found; running the bear`; a library pack that doesn't load (edited by
hand) is skipped with a log line, so a broken customized built-in runs the shipped one. A broken
`BUDDY_PACK` still exits 1. **Put on desktop** in a studio started with `--studio` writes settings
only while the app isn't running; with the app running it says to use the app's own Studio…
(the running app would overwrite the file).
