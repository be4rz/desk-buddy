"""quick look: python3 view.py out.png name[:variant] ... -> a 3x contact sheet with the bear composited (dev tool)."""
import sys, os, json
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__)); B3 = os.path.join(HERE, '..', 'mascot', 'out', 'bear3d')
BG = (58, 62, 78, 255)


def bear_frame(b, f=0):
    """(image, anchor) of a bear clip frame. b = dict(clip, dir | set+ori+file, at)."""
    if 'file' in b:
        j = json.load(open(os.path.join(B3, b['json']))); st = j['sets'][b['set']][b['ori']]
        im = Image.open(os.path.join(B3, os.path.dirname(b['json']), b['file'])).convert('RGBA')
        w, h, an = st['frame_w'], st['frame_h'], st['anchor']
    else:
        j = json.load(open(os.path.join(B3, b.get('json', 'anim/anim.json')))); a = j['anims'][b['clip']]
        im = Image.open(os.path.join(B3, os.path.dirname(b.get('json', 'anim/anim.json')), a['files'][b['dir']])).convert('RGBA')
        w, h, an = a['frame_w'], a['frame_h'], a['anchor']
    n = im.width // w; f %= n
    return im.crop((f * w, 0, f * w + w, h)), an, n


def compose(name, variant=None, f=0, bear=True):
    od = os.path.join(HERE, 'out', name); meta = json.load(open(os.path.join(od, 'meta.json')))
    v = variant or list(meta['variants'])[0] or 'base'
    src = Image.open(os.path.join(od, v + '__all.png')).convert('RGBA')
    b = meta.get('bear'); top = 0
    if b:                                   # room above the art for the bear (same canvas with or without it)
        fr, an, _ = bear_frame(b, f); ppu = meta['ppu']; x, y = b.get('at', [0, 0])
        px = meta['origin'][0] + round((x + y) * ppu * 0.5 ** 0.5); py = meta['origin'][1] - round((y - x) * ppu * 0.5 ** 0.5 * 0.5)
        top = max(0, an[1] - py + 2)
    im = Image.new('RGBA', (src.width, src.height + top), (0, 0, 0, 0)); im.alpha_composite(src, (0, top))
    if bear and b:
        im.alpha_composite(fr, (px - an[0], py - an[1] + top))
        for l in meta['layers']:
            if l != 'all': im.alpha_composite(Image.open(os.path.join(od, f'{v}__{l}.png')).convert('RGBA'), (0, top))
    return im


if __name__ == '__main__':
    out = sys.argv[1]; ims = []
    for a in sys.argv[2:]:
        nb = a.endswith('!'); a = a.rstrip('!')
        n, _, v = a.partition(':'); ims.append(compose(n, v or None, bear=not nb))
    z = int(os.environ.get('Z', 3)); W = sum(i.width for i in ims) + 8 * (len(ims) + 1); H = max(i.height for i in ims) + 16
    sh = Image.new('RGBA', (W, H), BG); x = 8
    for i in ims: sh.alpha_composite(i, (x, 8)); x += i.width + 8
    sh.resize((W * z, H * z), Image.NEAREST).save(out); print(out, sh.size)
