#!/bin/sh
# Stage kit runner: render one stage from its manifest (stages/<id>.json, or $BEAR3D_STAGES/<id>.json) into
# out/bear3d/set_<id>/ (kind "set") or out/bear3d/anim_<id>/ (kind "clips"), or $BEAR3D_OUT/... when that is set.
#
#   sh run_stage.sh <id> [WORKDIR] [--keep] [--resume]
#
# WORKDIR (default $TMPDIR/bear3d_stage_<id>) holds the rig, the render passes and the pixel layers; it must be one
# this script made (marker .bear3d_stage_work). It is removed after a good run unless --keep, and kept after a failed
# one. --resume reuses it: the rig and every layer already rendered are kept (render_set.py / render_anim.py skip
# them), so a run continues where it stopped; if the stage module is newer than the work dir's clips json
# the run starts fresh instead (a resume would reuse the old poses). Nothing in the out dir changes unless the stage's make step, its
# verifier (verify_<id>.py --dir <new>) and verify_face.py --part <new face part> all pass; then the new dir is
# swapped in, verify_face.py runs once more over every part, and a failure there puts the old dir back.
# Renders take one of two machine-wide slots (stagekit.py lock: $TMPDIR/bear3d_render.slot0|1 + pid; a third run
# waits, a slot whose pid is gone is reclaimed). Stage A/B/C/L/D outputs are never written.
# Needs bear.blend (run.sh), out/bear3d/anim/ (run_anim.sh) and face/face.json v2 (run_face.sh + face_l.py).
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
SID=$1; [ -n "$SID" ] || { echo "usage: sh run_stage.sh <id> [WORKDIR] [--keep] [--resume]"; exit 2; }
shift
WK=""; KEEP=0; RESUME=0
for a in "$@"; do
  case "$a" in --keep) KEEP=1;; --resume) RESUME=1;; -*) echo "unknown option $a"; exit 2;; *) WK=$a;; esac
done
MAN=$(python3 stagekit.py manifest "$SID" --sh) || { echo "no stage manifest for $SID"; exit 1; }
eval "$MAN"      # ID KIND MODULE VERIFY OUT
[ -n "$OUT" ] && [ -n "$KIND" ] || { echo "stage manifest $SID: no kind / out dir"; exit 1; }
WK=${WK:-${TMPDIR:-/tmp}/bear3d_stage_$SID}
OROOT=$(python3 -c 'import stagekit; print(stagekit.out_root())')
DEST="$OROOT/$OUT"; mkdir -p "$OROOT"
if [ -e "$WK" ] && [ ! -f "$WK/.bear3d_stage_work" ]; then echo "refusing to use $WK: not a run_stage.sh work dir"; exit 1; fi
MODF=$(python3 -c "import stagekit; print(stagekit.module('$MODULE').__file__)")
if [ "$RESUME" = 1 ] && [ -f "$WK/clips_$SID.json" ] && [ "$MODF" -nt "$WK/clips_$SID.json" ]; then
  echo "$MODULE changed since $WK was made: starting fresh (a resume would reuse the old poses)"; RESUME=0
fi
if [ "$RESUME" = 1 ] && [ -d "$WK" ]; then echo "resuming in $WK"; else rm -rf "$WK"; fi
mkdir -p "$WK/raw" "$WK/px"; touch "$WK/.bear3d_stage_work"
rm -rf "$WK/new" "$WK/old"

SLOT=$(python3 stagekit.py lock acquire --pid $$)
trap 'python3 stagekit.py lock release "$SLOT" --pid $$ >/dev/null 2>&1 || true' EXIT
trap 'exit 130' INT TERM      # stop the run (the EXIT trap then frees the slot)
echo "render slot $SLOT"

RIG="$WK/rig_$SID.blend"; CJ="$WK/clips_$SID.json"
# Blender exits 0 on a Python error: a missing output is the failure signal
if [ ! -f "$RIG" ] || [ ! -f "$CJ" ]; then
  $B -b bear.blend -P rig_bear.py -- --stage k --clips-module "$MODULE" --out "$RIG" --clips "$CJ" 2>&1 \
    | grep -E "SAVED|Error|Traceback|Assertion" || true
  [ -f "$RIG" ] && [ -f "$CJ" ] || { echo "rig failed (rig_bear.py --stage k --clips-module $MODULE)"; exit 1; }
fi

if [ "$KIND" = set ]; then
  SETS=$(python3 -c "import json; print(' '.join(json.load(open('$CJ'))['sets']))")
  for s in $SETS; do
    for o in A B; do
      A="--module $MODULE --clips $CJ --set $s --ori $o --raw $WK/raw --px $WK/px"
      for m in layout props clip; do
        $B -b "$RIG" -P render_set.py -- $A --mode $m 2>&1 | grep -E "LAYOUT|DONE|Error|Traceback" || true
      done
      for c in $(python3 -c "import json; print(' '.join(json.load(open('$CJ'))['sets']['$s']['clips']))"); do
        [ -f "$WK/px/${s}_$o/clip_$c.json" ] || { echo "render failed: $c $o"; exit 1; }
      done
      python3 pixelize_set.py set "$WK/raw/${s}_$o" "$WK/px/${s}_$o" --module "$MODULE"
    done
  done
  python3 make_set.py "$SID" "$WK/px" "$WK/new" --clips "$CJ"   # exits 1 on a hand-off or pixel failure
else
  KIT=$(python3 -c "import json; k=json.load(open('$CJ'))['kit']; print(','.join(k['bear_meshes'])+' '+(','.join(k['objects']) or '-'))")
  BEARM=${KIT% *}; OBJS=${KIT#* }
  for c in $(python3 -c "import json; print(' '.join(json.load(open('$CJ'))['order']))"); do
    YAWS=$(python3 -c "import json,stagekit; print(','.join(str(float(stagekit.YAW[d])) for d in json.load(open('$CJ'))['clips']['$c']['dirs'] or stagekit.DIRS))")
    if [ "$OBJS" = - ]; then HB=""; else HB="--hide $OBJS"; fi
    $B -b "$RIG" -P render_anim.py -- --raw "$WK/raw" --tag bear --clip "$c" --canvas 84,123,107 --yaws "$YAWS" $HB 2>&1 \
      | grep -E "DONE|Error|Traceback|leaves" || true
    [ -f "$WK/raw/bear_$c/meta.json" ] || [ -f "$WK/px/bear_$c/meta.json" ] || { echo "render failed: bear $c"; exit 1; }
    [ -f "$WK/px/bear_$c/meta.json" ] && [ ! -f "$WK/raw/bear_$c/meta.json" ] || \
      python3 pixelize_set.py anim "$WK/raw/bear_$c" "$WK/px" --module "$MODULE" --depth
    if [ "$OBJS" != - ]; then
      $B -b "$RIG" -P render_anim.py -- --raw "$WK/raw" --tag obj --clip "$c" --canvas 84,123,107 --yaws "$YAWS" \
        --hide "$BEARM" --show "$OBJS" 2>&1 | grep -E "DONE|Error|Traceback|leaves" || true
      [ -f "$WK/raw/obj_$c/meta.json" ] || [ -f "$WK/px/obj_$c/meta.json" ] || { echo "render failed: obj $c"; exit 1; }
      [ -f "$WK/px/obj_$c/meta.json" ] && [ ! -f "$WK/raw/obj_$c/meta.json" ] || \
        python3 pixelize_set.py anim "$WK/raw/obj_$c" "$WK/px" --module "$MODULE" --depth
    fi
  done
  python3 make_stage.py "$SID" "$WK/px" "$WK/new" --clips "$CJ"
fi

# check the new dir before it goes anywhere near the out root
python3 "$VERIFY" --dir "$WK/new"
FP="$WK/new/face_$SID.json"
if [ -f "$FP" ]; then python3 verify_face.py --part "$FP"; fi
# swap in, keeping the old dir in WK until verify_face.py passes over every part
mkdir -p "$WK/old"
[ -d "$DEST" ] && mv "$DEST" "$WK/old/$OUT"
mv "$WK/new" "$DEST"
if [ -f "$DEST/face_$SID.json" ] && [ "$OROOT" = "$(python3 -c 'import stagekit; print(stagekit.REAL_OUT)')" ]; then
  if ! python3 verify_face.py; then
    echo "verify_face.py failed with the new $OUT: restoring the previous one"
    rm -rf "$DEST"; [ -d "$WK/old/$OUT" ] && mv "$WK/old/$OUT" "$DEST"
    exit 1
  fi
fi
echo "stage $SID: ok -> $DEST"
if [ "$KEEP" = 1 ]; then rm -rf "$WK/old"; echo "kept $WK"; else rm -rf "$WK"; fi
