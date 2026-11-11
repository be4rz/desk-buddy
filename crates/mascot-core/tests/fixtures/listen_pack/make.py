"""Builds the listen_pack test fixture (p2 listen-brain) from the real Stage A idle strips.

Stand-in art only, until listen-art renders `anim_l/`: every listen/turn frame is a crop of an idle frame
(no drawing). Turn clips: frame 0 = idle_<from>#0, last = idle_<to>#0 (the #anim-l hand-offs).
Run from anywhere: python3 make.py  (needs Pillow). Writes anim_l/ and face/face.json next to this file.
"""
import json
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
REAL = HERE.parents[4] / "experiments/mascot/out/bear3d"
REAL_REL = "../../../../../experiments/mascot/out/bear3d"  # from this dir
DIRS = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"]
FACE_DIRS = ["S", "SE", "E", "W", "SW"]
PATCH = {"S": "S", "SE": "S", "SW": "S", "E": "SE", "W": "SW"}  # design §1.4
W, H = 84, 123

anim = json.loads((REAL / "anim/anim.json").read_text())
idle = anim["anims"]["idle"]
strips = {d: Image.open(REAL / "anim" / idle["files"][d]).convert("RGBA") for d in DIRS}


def frame(d, k):
    return strips[d].crop((k * W, 0, (k + 1) * W, H))


def strip(frames):
    out = Image.new("RGBA", (W * len(frames), H), (0, 0, 0, 0))
    for i, f in enumerate(frames):
        out.paste(f, (i * W, 0))
    return out


def clip(name, dirs, n, loop, pick, turn_steps=0):
    files = {}
    for d in dirs:
        fn = f"{name}_{d}.png"
        strip([pick(d, k) for k in range(n)]).save(HERE / "anim_l" / fn)
        files[d] = fn
    c = {
        "loop": loop, "fps": 12, "frame_w": W, "frame_h": H, "anchor": idle["anchor"], "frames_n": n, "files": files,
        "frames": [{"dur": 83, "root_world": 0.0, "root_px": {d: [0.0, 0.0] for d in dirs}} for _ in range(n)],
    }
    if turn_steps:
        c["turn_steps"] = turn_steps
    return c


def step(d, by):
    return DIRS[(DIRS.index(d) + by) % 8]


(HERE / "anim_l").mkdir(exist_ok=True)
anims = {
    "listen_in": clip("listen_in", FACE_DIRS, 3, False, lambda d, k: frame(d, 0 if k == 0 else k)),
    "listen": clip("listen", FACE_DIRS, 12, True, lambda d, k: frame(d, k)),
    "listen_out": clip("listen_out", FACE_DIRS, 3, False, lambda d, k: frame(d, 0 if k == 2 else k + 1)),
    "turn_ccw": clip("turn_ccw", DIRS, 3, False, lambda d, k: frame(step(d, 1) if k == 2 else d, k if k < 2 else 0), 1),
    "turn_cw": clip("turn_cw", DIRS, 3, False, lambda d, k: frame(step(d, -1) if k == 2 else d, k if k < 2 else 0), -1),
}
(HERE / "anim_l/anim_l.json").write_text(json.dumps(
    {"note": "listen_pack stand-in (make.py): crops of idle frames.", "fps": 12, "ppu": anim["ppu"],
     "directions": anim["directions"], "anims": anims}, indent=1) + "\n")

face = json.loads((REAL / "face/face.json").read_text())
for slot in face["layers"].values():
    for state in slot.values():
        for d, layer in state.items():
            layer["file"] = f"../{REAL_REL}/face/{layer['file']}"
face["face_version"] = 2
face["note"] = "listen_pack stand-in (make.py): the real face layers; listen positions = idle's first 12."
face["clips"] = {
    "idle": face["clips"]["idle"],
    "listen": {d: {s: pos[:12] for s, pos in face["clips"]["idle"][d].items()} for d in FACE_DIRS},
}
face["patch_dirs"] = {"listen": PATCH}
(HERE / "face").mkdir(exist_ok=True)
(HERE / "face/face.json").write_text(json.dumps(face) + "\n")
print("ok")
