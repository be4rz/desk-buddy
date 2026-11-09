#!/bin/sh
# Full rebuild: model -> data passes -> pixel art -> deliverables (~40 s).
#   sh run.sh [cute] [RAWDIR]
#     cute   (default) bear3d/build_bear.py + pixelize.py  -> ../out/bear3d/cute/
#   OUT=dir overrides the output folder. RAWDIR = scratch for the big intermediate passes, deleted at the end.
set -e
cd "$(dirname "$0")"
B=${BLENDER:-/Applications/Blender.app/Contents/MacOS/Blender}
VAR=${1:-cute}
RAW=${2:-${TMPDIR:-/tmp}/bear3d_raw}
case "$VAR" in
  cute)   SRC=.;      OUT=${OUT:-../out/bear3d/cute} ;;
  *) echo "usage: sh run.sh [cute] [RAWDIR]"; exit 1 ;;
esac
rm -rf "$RAW"; mkdir -p "$RAW"
$B -b -P $SRC/build_bear.py -- --out "$SRC/bear.blend"
# ~105 px tall at the reference's 3/4 angle (yaw 112.5), 32 angles (the 8 directions + turntable in-betweens)
$B -b "$SRC/bear.blend" -P render_bear.py -- --raw "$RAW" --name big --fit 105 --fit-yaw 112.5 --yaws 32
# ~64 px tall, 8 directions
$B -b "$SRC/bear.blend" -P render_bear.py -- --raw "$RAW" --name small --fit 64 --fit-yaw 112.5 --yaws 8
python3 $SRC/pixelize.py "$RAW/big" "$RAW/px" --prefix big
python3 $SRC/pixelize.py "$RAW/small" "$RAW/px" --prefix small
python3 make_outputs.py "$RAW/px" "$OUT" "$RAW"
rm -rf "$RAW"
