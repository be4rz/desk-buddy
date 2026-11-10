"""Build blob.aseprite, the import test fixture, from existing Blender frames (no pixel is
drawn here): layer "body" holds the first 6 frames of walk_64_S then of walk_64_SE, tags
`walk` (S) and `walk_SE`, a slice `anchor` with the Stage A anchor as its pivot, and a hidden
layer "eyes" with the eyes_closed_S face patch (linked cels after frame 0), which the import
must skip. Run from the repo root: python3 crates/studio/tests/fixtures/make_blob.py
"""
import struct, zlib
from PIL import Image

ANIM = "experiments/mascot/out/bear3d/anim/"
FACE = "experiments/mascot/out/bear3d/face/eyes_closed_S.png"
OUT = "crates/studio/tests/fixtures/blob.aseprite"
FW, FH, N, ANCHOR, DUR = 52, 76, 6, (26, 65), 83


def frames(png):
    im = Image.open(png).convert("RGBA")
    return [im.crop((k * FW, 0, (k + 1) * FW, FH)).tobytes() for k in range(N)]


def s(t):
    b = t.encode()
    return struct.pack("<H", len(b)) + b


def chunk(kind, data):
    return struct.pack("<IH", 6 + len(data), kind) + data


def layer(name, visible):
    return chunk(0x2004, struct.pack("<HHHHHHB3x", 1 if visible else 0, 0, 0, 0, 0, 0, 255) + s(name))


def cel(layer, x, y, w, h, rgba):
    return chunk(0x2005, struct.pack("<HhhBHh5x", layer, x, y, 255, 2, 0) + struct.pack("<HH", w, h) + zlib.compress(rgba, 9))


def link(layer, x, y, to):
    return chunk(0x2005, struct.pack("<HhhBHh5xH", layer, x, y, 255, 1, 0, to))


def tags(ts):
    body = struct.pack("<H8x", len(ts))
    for name, a, b in ts:
        body += struct.pack("<HHBH6x3Bx", a, b, 0, 0, 0, 0, 0) + s(name)
    return chunk(0x2018, body)


def slice_anchor():
    body = struct.pack("<III", 1, 2, 0) + s("anchor")
    body += struct.pack("<IiiII", 0, 0, 0, FW, FH) + struct.pack("<ii", *ANCHOR)
    return chunk(0x2022, body)


body = frames(ANIM + "walk_64_S.png") + frames(ANIM + "walk_64_SE.png")
eyes = Image.open(FACE).convert("RGBA")
out = b""
for f, px in enumerate(body):
    cs = []
    if f == 0:
        cs += [layer("body", True), layer("eyes", False), tags([("walk", 0, N - 1), ("walk_SE", N, 2 * N - 1)]), slice_anchor()]
    cs.append(cel(0, 0, 0, FW, FH, px))
    cs.append(cel(1, 10, 10, eyes.width, eyes.height, eyes.tobytes()) if f == 0 else link(1, 10, 10, 0))
    data = b"".join(cs)
    out += struct.pack("<IHHH2xI", 16 + len(data), 0xF1FA, len(cs), DUR, len(cs)) + data
header = struct.pack("<IHHHHHIHIIB3xHBBhhHH84x", 128 + len(out), 0xA5E0, len(body), FW, FH, 32, 1, DUR, 0, 0, 0, 0, 1, 1, 0, 0, 16, 16)
assert len(header) == 128
open(OUT, "wb").write(header + out)
print(OUT, 128 + len(out), "bytes")
