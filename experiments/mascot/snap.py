"""Snap an AI-made "pixel-looking" image to true pixel art.

Steps: remove the flat background, detect the pseudo-pixel size (or take
--cell), take the mode colour of each cell, quantize to the bear palette,
and write the 1x sprite plus an 8x preview next to the reference drawing.

usage: python3 snap.py IN.png [--cell N] [--height H] [--out NAME]
"""
import argparse, json, os, numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
BP = os.path.join(HERE, '..', 'bear-proto')
REF = os.path.join(BP, 'ref', 'original-drawing.png')

def palette():
    hexes = []
    bear = json.load(open(os.path.join(BP, 'palette-bear.json')))
    for k, v in bear.items():
        if isinstance(v, list): hexes += v
    room = json.load(open(os.path.join(HERE, '..', 'scene-proto', 'palette.json')))
    ramps = room.get('ramps', room)
    hexes += ramps.get('moss', [])          # clover green
    return np.array([[int(h[i:i+2], 16) for i in (1, 3, 5)] for h in hexes])

def bg_mask(a):
    corners = np.concatenate([a[:8, :8], a[:8, -8:], a[-8:, :8], a[-8:, -8:]]).reshape(-1, 3)
    bg = np.median(corners, 0)
    near = np.abs(a - bg).sum(-1) < 45
    reach = np.zeros_like(near)
    reach[0, :] = reach[-1, :] = reach[:, 0] = reach[:, -1] = True
    reach &= near
    while True:   # region-grow the background from the border
        p = np.pad(reach, 1)
        nx = (p[:-2, 1:-1] | p[2:, 1:-1] | p[1:-1, :-2] | p[1:-1, 2:] | reach) & near
        if (nx == reach).all(): return reach
        reach = nx

def _profile(a, axis):
    g = np.abs(np.diff(a, axis=axis)).sum(-1) > 40
    p = g.sum(1 - axis).astype(float)
    return p - p.mean()

def detect_cell(a):
    """Fractional pseudo-pixel size from the strongest period of colour edges."""
    p = _profile(a, 1)
    f = np.abs(np.fft.rfft(p)); fr = np.fft.rfftfreq(len(p))
    ok = (fr > 1 / 48) & (fr < 1 / 3)
    return 1 / fr[ok][f[ok].argmax()]

def detect_phase(a, cell, axis):
    """Offset of the grid lines along one axis (edges sit at phase + k*cell)."""
    p = _profile(a, axis)
    idx = np.arange(len(p)) + 1          # an edge between i and i+1 is at i+1
    best = max(np.arange(0, cell, 0.25),
               key=lambda ph: p[np.abs(((idx - ph) / cell) - np.round((idx - ph) / cell)) * cell < 1.0].sum())
    return best

def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('inp'); ap.add_argument('--cell', type=float)
    ap.add_argument('--height', type=int, help='force sprite height in px instead of a cell size')
    ap.add_argument('--out', default=None)
    o = ap.parse_args()
    a = np.asarray(Image.open(o.inp).convert('RGB')).astype(int)
    bgm = bg_mask(a)
    if o.height:
        ys, _ = np.where(~bgm); cell = (ys.max() - ys.min() + 1) / o.height; px = py = 0.0
        a, bgm = a[ys.min():], bgm[ys.min():]
    else:
        cell = o.cell or detect_cell(a)
        px, py = detect_phase(a, cell, 1), detect_phase(a, cell, 0)
    H, W = int((a.shape[0] - py) // cell), int((a.shape[1] - px) // cell)
    pal = palette()
    out = np.zeros((H, W, 4), np.uint8)
    q = max(1, int(cell / 4))
    for y in range(H):
        for x in range(W):
            cy, cx = int(py + (y + 0.5) * cell), int(px + (x + 0.5) * cell)
            # sample the cell's centre to dodge seams between pseudo-pixels
            sl = (slice(cy - q, cy + q + 1), slice(cx - q, cx + q + 1))
            if bgm[sl].mean() > 0.5: continue
            blk = a[sl].reshape(-1, 3)
            idx = ((blk[:, None, :] - pal[None]) ** 2).sum(-1).argmin(1)
            k = np.bincount(idx).argmax()
            out[y, x, :3] = pal[k]; out[y, x, 3] = 255
    ys, xs = np.where(out[..., 3] > 0)
    out = out[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
    H, W = out.shape[:2]
    name = o.out or os.path.splitext(os.path.basename(o.inp))[0]
    sprite = Image.fromarray(out, 'RGBA')
    sprite.save(os.path.join(HERE, 'out', f'{name}.png'))
    # preview: reference | 8x sprite on a neutral ground
    S = 8; ph = H * S
    ref = Image.open(REF).convert('RGB'); ref = ref.resize((int(ref.width * ph / ref.height), ph))
    big = sprite.resize((W * S, ph), Image.NEAREST)
    prev = Image.new('RGB', (ref.width + big.width + 48, ph + 32), (200, 196, 210))
    prev.paste(ref, (16, 16)); prev.paste(big, (ref.width + 32, 16), big)
    prev.save(os.path.join(HERE, 'out', f'{name}_preview.png'))
    print(f'cell {cell:.2f} phase ({px:.2f},{py:.2f}) -> {W}x{H} px, colours used: {len(np.unique(out[out[...,3]>0][:, :3], axis=0))}')

if __name__ == '__main__':
    main()
