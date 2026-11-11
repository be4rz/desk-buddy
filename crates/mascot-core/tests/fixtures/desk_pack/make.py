"""Builds the desk_pack test fixture (p3 desk-brain) from the real bear art.

Stand-in art only, until desk-art renders `anim_d/`: every desk clip is a run of frames cut from the real
Stage B `read` set strips (no drawing), in the `#anim-d` shapes: set `desk` A/B, props_frame
`../props_d/desk_<ori>.png`, hand-offs (px, facing) copied from `sit_down` / `stand_up`, frame counts
inside the storyboard's ranges. The face (`face/face.json` v2) is the real one plus `set_clips` for
desk_talk / desk_ask: the S patches, at the idle positions of the hand-off facing moved onto the set frame.
pack.json is the bear's pack with its sources pointed at the real art, plus the desk wiring of
`#pack-stations` (sources, stations, work, the `computer` activity).
Run from anywhere: python3 make.py  (needs Pillow). Writes anim_d/, props_d/, face/ and pack.json next to this file.
"""
import json
import shutil
from pathlib import Path

from PIL import Image

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
REAL = ROOT / "experiments/mascot/out/bear3d"
REAL_REL = "../../../../../experiments/mascot/out/bear3d"  # from this dir

anim_b = json.loads((REAL / "anim_b/anim_b.json").read_text())
props_b = json.loads((REAL / "props/props.json").read_text())
read_set = anim_b["sets"]["read"]
W, H = read_set["A"]["frame_w"], read_set["A"]["frame_h"]


def frames_of(clip, ori):
    im = Image.open(REAL / "anim_b" / anim_b["anims"][clip]["files"][ori]).convert("RGBA")
    return [im.crop((k * W, 0, (k + 1) * W, H)) for k in range(im.width // W)]


def strip(frames):
    out = Image.new("RGBA", (W * len(frames), H), (0, 0, 0, 0))
    for i, f in enumerate(frames):
        out.paste(f, (i * W, 0))
    return out


# desk clip ← (real clip, frame indices, loop, hand-off source)
READ = list(range(48))
CLIPS = {
    "desk_sit": ("sit_down", list(range(12)), False, "sit_down"),
    "desk_type": ("read", READ[0:24], True, None),
    "desk_swivel_in": ("read", READ[24:30], False, None),
    "desk_talk": ("read", READ[30:46], True, None),
    "desk_ask": ("read", READ[30:46], True, None),
    "desk_swivel_out": ("read", READ[24:30][::-1], False, None),
    "desk_stand": ("stand_up", list(range(11)), False, "stand_up"),
}

for d in ("anim_d", "props_d", "face"):
    (HERE / d).mkdir(exist_ok=True)

sets = {"desk": {}}
for ori in ("A", "B"):
    s = dict(read_set[ori])
    s["props_frame"] = f"../props_d/desk_{ori}.png"
    s["orientation"] = f"desk_pack stand-in: the read set ({ori})"
    sets["desk"][ori] = s
    shutil.copyfile(REAL / "anim_b" / read_set[ori]["props_frame"], HERE / "props_d" / f"desk_{ori}.png")

anims = {}
for name, (src, idx, loop, hand) in CLIPS.items():
    files = {}
    for ori in ("A", "B"):
        fr = frames_of(src, ori)
        fn = f"{name}_{ori}.png"
        strip([fr[i] for i in idx]).save(HERE / "anim_d" / fn)
        files[ori] = fn
    c = {"set": "desk", "loop": loop, "fps": 12, "frames_n": len(idx), "files": files, "frames": [{"dur": 83} for _ in idx]}
    if hand:
        h = anim_b["anims"][hand]["handoff"]
        c["handoff"] = {o: {"at": h[o]["at"], "frame": 0 if h[o]["at"] == "start" else len(idx) - 1, "px": h[o]["px"], "facing": h[o]["facing"]} for o in ("A", "B")}
    anims[name] = c
(HERE / "anim_d/anim_d.json").write_text(json.dumps(
    {"note": "desk_pack stand-in (make.py): frames of the real read set.", "fps": 12, "ppu": anim_b.get("ppu", 40.07), "sets": sets, "anims": anims}, indent=1) + "\n")

props = {"note": "desk_pack stand-in (make.py): the read set's props.", "ppu": props_b["ppu"], "props": {}}
for pname, p in props_b["props"].items():
    if p["set"] != "read":
        continue
    q = json.loads(json.dumps(p))
    q["set"] = "desk"
    for ori in ("A", "B"):
        fn = f"{pname}_{ori}.png"
        shutil.copyfile(REAL / "props" / p[ori]["file"], HERE / "props_d" / fn)
        q[ori]["file"] = fn
    props["props"][pname] = q
(HERE / "props_d/props_d.json").write_text(json.dumps(props, indent=1) + "\n")

# face: the real layers and clips, plus set_clips
face = json.loads((REAL / "face/face.json").read_text())
for slot in face["layers"].values():
    for state in slot.values():
        for d, layer in state.items():
            layer["file"] = f"../{REAL_REL}/face/{layer['file']}"
anim_a = json.loads((REAL / "anim/anim.json").read_text())
idle_anchor = anim_a["anims"]["idle"]["anchor"]
face["face_version"] = 2
face["note"] = "desk_pack stand-in (make.py): the real face; set_clips = S patches at idle positions on the set frame."
set_clips = {}
for name in ("desk_talk", "desk_ask"):
    n = anims[name]["frames_n"]
    by_ori = {}
    for ori in ("A", "B"):
        h = anim_b["anims"]["sit_down"]["handoff"][ori]
        # the standing bear's anchor on the set frame: set anchor + hand-off px
        dx = read_set[ori]["anchor"][0] + h["px"][0] - idle_anchor[0]
        dy = read_set[ori]["anchor"][1] + h["px"][1] - idle_anchor[1]
        idle = face["clips"]["idle"][h["facing"]]
        by_ori[ori] = {"patch": "S", **{slot: [[x + dx, y + dy] for x, y in pos[:n]] for slot, pos in idle.items()}}
    set_clips[name] = by_ori
face["set_clips"] = set_clips
(HERE / "face/face.json").write_text(json.dumps(face) + "\n")

# pack.json: the bear's, pointed at the real art, plus the desk
pack = json.loads((ROOT / "mascots/bear/pack.json").read_text())
pack["id"] = "desk_pack"
pack["name"] = "Desk fixture"
pack["art_root"] = "."
pack["palettes"] = [f"../../../../../{p.removeprefix('../../')}" for p in pack["palettes"]]
sources = []
for s in pack["sources"]:
    if s["path"].startswith("anim_d/") or s["kind"] == "face":
        continue
    s = dict(s)
    s["path"] = f"{REAL_REL}/{s['path']}"
    if "props" in s:
        s["props"] = f"{REAL_REL}/{s['props']}"
    sources.append(s)
sources.append({"kind": "stage_b", "path": "anim_d/anim_d.json", "props": "props_d/props_d.json", "optional": True})
sources.append({"kind": "face", "path": "face/face.json", "optional": True})
pack["sources"] = sources
pack["stations"] = {"desk": {"set": "desk", "enter": "desk_sit", "loop": "desk_type", "to_user": "desk_swivel_in", "talk": "desk_talk",
                             "ask": "desk_ask", "listen": "desk_talk", "to_screen": "desk_swivel_out", "exit": "desk_stand"}}
pack["work"] = {"station": "desk", "after_ms": 4000, "after_tools": 2}
pack["body"]["activities"]["computer"] = {"station": "desk", "kind": "work", "about": "sit at the desk and use the computer"}
(HERE / "pack.json").write_text(json.dumps(pack, indent=1) + "\n")
print("ok")
