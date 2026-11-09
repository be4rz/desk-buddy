#!/bin/sh
# Stage A: rig the cute bear, render every clip in 8 directions (~100 px) + walk at ~64 px (S, SE),
# pixelize, assemble ../out/bear3d/anim/. Needs bear.blend from `sh run.sh` (the cute model). ~5 min.
#   sh run_anim.sh [RAWDIR]     (RAWDIR = scratch for the render passes; each clip's passes are deleted once pixelized)
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
RAW=${1:-${TMPDIR:-/tmp}/bear3d_anim_raw}
OUT=../out/bear3d/anim
rm -rf "$RAW"; mkdir -p "$RAW/px"
$B -b bear.blend -P rig_bear.py -- --out bear_rig.blend --clips clips.json
for c in idle walk_start walk walk_stop run_start run run_stop; do
  $B -b bear_rig.blend -P render_anim.py -- --raw "$RAW" --tag big --clip $c
  python3 pixelize_anim.py "$RAW/big_$c" "$RAW/px"; rm -rf "$RAW/big_$c"
done
$B -b bear_rig.blend -P render_anim.py -- --raw "$RAW" --tag small --clip walk --fit 64 --yaws 45,90
python3 pixelize_anim.py "$RAW/small_walk" "$RAW/px"; rm -rf "$RAW/small_walk"
rm -rf "$OUT"; python3 make_anim.py "$RAW/px" "$OUT" clips.json
rm -rf "$RAW"
