"""verify_env: python3 verify_env.py [--room DIR] | --self-test (exit 1 = works).
Checks a room pack: room.json shape, every file exists, alpha 0/255 only, every opaque colour is in the pack's
palettes, the art keeps 1 px off the canvas edge, the origin is inside the image, hours are whole hours 0..24.
The room.json rules mirror the app's loader (crates/mascot-core/src/room.rs): what passes here must load there
(this is stricter in two places: a floor and a scene may not share an id, and "front": null is refused)."""
import os, re, sys, json, shutil, tempfile
from PIL import Image
HERE = os.path.dirname(os.path.abspath(__file__))
ROOM = os.path.normpath(os.path.join(HERE, '..', '..', 'rooms', 'starter'))


def colours(room, d):
    cs = set()
    for p in room.get('palettes', []):
        def walk(v):
            if isinstance(v, str) and v.startswith('#') and len(v) == 7: cs.add(tuple(int(v[i:i + 2], 16) for i in (1, 3, 5)))
            elif isinstance(v, dict): [walk(x) for x in v.values()]
            elif isinstance(v, list): [walk(x) for x in v]
        walk(json.load(open(os.path.join(d, p))))
    return cs


def check(d):
    errs = []
    try: room = json.load(open(os.path.join(d, 'room.json')))
    except Exception as e: return ['room.json: %s' % e]
    if room.get('room_version') != 1: errs.append('room_version is not 1')
    try: pal = colours(room, d)
    except Exception as e: return ['palettes: %s' % e]
    seen = set(); claimed = set()

    def name(what, v):                              # the loader's name rule
        if not (isinstance(v, str) and re.fullmatch(r'[a-z0-9_-]{1,32}', v)): errs.append('%s %r is not a-z 0-9 _ - (1 to 32)' % (what, v))
    name('room id', room.get('id'))

    def image(what, rel, origin):
        if not isinstance(rel, str): errs.append('%s: missing file %s' % (what, rel)); return
        if os.path.isabs(rel) or '..' in rel.replace('\\', '/').split('/'): errs.append('%s: file %s must be a path inside the room pack' % (what, rel)); return
        p = os.path.join(d, rel)
        if not rel or not os.path.isfile(p): errs.append('%s: missing file %s' % (what, rel)); return
        im = Image.open(p).convert('RGBA'); w, h = im.size; px = im.load(); bad_a = off = edge = 0
        for y in range(h):
            for x in range(w):
                r, g, b, a = px[x, y]
                if a == 0: continue
                if a != 255: bad_a += 1; continue
                if (r, g, b) not in pal: off += 1
                if x in (0, w - 1) or y in (0, h - 1): edge += 1
        if bad_a: errs.append('%s: %d px with partial alpha' % (what, bad_a))
        if off: errs.append('%s: %d px off palette' % (what, off))
        if edge: errs.append('%s: %d px touch the canvas edge' % (what, edge))
        if not (isinstance(origin, list) and len(origin) == 2 and all(type(c) is int for c in origin) and 0 <= origin[0] < w and 0 <= origin[1] < h): errs.append('%s: origin %s outside %dx%d' % (what, origin, w, h))
        return (w, h)

    for f in room.get('floors', []):
        if f.get('id') in seen: errs.append('duplicate id %s' % f.get('id'))
        name('floor id', f.get('id'))
        if f.get('id') in ('grid', 'none'): errs.append('floor id %s is reserved' % f.get('id'))
        seen.add(f.get('id')); image('floor ' + str(f.get('id')), f.get('file'), f.get('origin'))
    for s in room.get('scenes', []):
        sid = str(s.get('id'))
        if sid in seen: errs.append('duplicate id %s' % sid)
        seen.add(sid); name('scene id', s.get('id'))
        fors = s.get('for')
        if not (isinstance(fors, list) and fors): errs.append('scene %s: no "for"' % sid); fors = []
        for n in fors:
            if not (isinstance(n, str) and n.strip()): errs.append('scene %s: "for" has an empty name' % sid)
            if isinstance(n, str) and n in claimed: errs.append('scene %s: %s already has a scene' % (sid, n))
            if isinstance(n, str): claimed.add(n)
        if not s.get('variants'): errs.append('scene %s: no variants' % sid)
        sizes = set()
        for v in s.get('variants', []):
            what = 'scene %s/%s' % (sid, v.get('name'))
            if not isinstance(v.get('name'), str): errs.append('%s: no name' % what)
            sizes.add(image(what, v.get('back'), s.get('origin')))
            if 'front' in v: sizes.add(image(what + ' front', v.get('front'), s.get('origin')))
            hrs = v.get('hours')
            if hrs is not None and not (isinstance(hrs, list) and len(hrs) == 2 and all(type(x) is int and 0 <= x <= 24 for x in hrs) and hrs[0] % 24 != hrs[1] % 24):
                errs.append('%s: bad hours %s' % (what, hrs))
        if len(sizes - {None}) > 1: errs.append('scene %s: variants differ in size %s' % (sid, sorted(sizes - {None})))
        timed = [v for v in s.get('variants', []) if isinstance(v.get('hours'), list) and len(v['hours']) == 2 and all(type(x) is int for x in v['hours'])]
        if timed:                                   # timed variants must cover the whole day, once
            cover = [0] * 24
            for v in timed:
                a, b = v['hours']; h = a % 24
                while h != b % 24: cover[h] += 1; h = (h + 1) % 24
            if any(c != 1 for c in cover) or len(timed) != len(s['variants']): errs.append('scene %s: hours do not cover the day exactly once' % sid)
    if not room.get('floors') and not room.get('scenes'): errs.append('no floors or scenes')
    return errs


def self_test():
    """each mutation of a copy of the real pack must be caught"""
    def mutate(fn):
        t = tempfile.mkdtemp(); d = os.path.join(t, 'a', 'b', 'room'); shutil.copytree(ROOM, d)
        room = json.load(open(os.path.join(d, 'room.json')))
        room['palettes'] = [os.path.normpath(os.path.join(ROOM, p)) for p in room['palettes']]
        fn(room, d); json.dump(room, open(os.path.join(d, 'room.json'), 'w')); e = check(d); shutil.rmtree(t); return e

    def px(rel, xy, rgba):
        def f(room, d):
            p = os.path.join(d, rel); im = Image.open(p).convert('RGBA'); im.putpixel(xy, rgba); im.save(p)
        return f

    def opaque_xy(rel):
        im = Image.open(os.path.join(ROOM, rel)).convert('RGBA'); w, h = im.size
        return next((x, y) for y in range(h) for x in range(w) if im.getpixel((x, y))[3] == 255)

    xy = opaque_xy('floors/tatami.png')
    cases = [
        ('clean copy passes', lambda r, d: None, False),
        ('partial alpha', px('floors/tatami.png', xy, (10, 20, 30, 128)), True),
        ('off palette', px('floors/tatami.png', xy, (1, 2, 3, 255)), True),
        ('edge pixel', px('scenes/kitchen_base.png', (0, 0), (0, 0, 0, 255)), True),
        ('missing file', lambda r, d: os.remove(os.path.join(d, 'scenes/tearoom_base_front.png')), True),
        ('origin outside', lambda r, d: r['floors'][0].update(origin=[9999, 0]), True),
        ('hours gap', lambda r, d: r['scenes'][0]['variants'][0].update(hours=[6, 8]), True),
        ('no for', lambda r, d: r['scenes'][1].update({'for': []}), True),
        ('duplicate id', lambda r, d: r['floors'][1].update(id='tatami'), True),
        ('reserved floor id', lambda r, d: r['floors'][1].update(id='grid'), True),
        ('bad id', lambda r, d: r['scenes'][1].update(id='Mini Kitchen'), True),
        ('empty for name', lambda r, d: r['scenes'][1].update({'for': ['']}), True),
        ('for claimed twice', lambda r, d: r['scenes'][1].update({'for': ['laptop']}), True),
        ('hours 0 to 24', lambda r, d: r['scenes'][0]['variants'].append(dict(r['scenes'][0]['variants'][0], name='all', hours=[0, 24])), True),
        ('path leaves the pack', lambda r, d: r['floors'][0].update(file='../room/floors/tatami.png'), True),
        ('float origin', lambda r, d: r['floors'][0].update(origin=[float(r['floors'][0]['origin'][0]), r['floors'][0]['origin'][1]]), True),
    ]
    ok = True
    for name, fn, want in cases:
        got = bool(mutate(fn)); print('%-20s %s' % (name, 'caught' if got else 'clean')); ok &= got == want
    print('self-test', 'works' if ok else 'BROKEN'); return ok


if __name__ == '__main__':
    if '--self-test' in sys.argv: sys.exit(1 if self_test() else 2)
    d = sys.argv[sys.argv.index('--room') + 1] if '--room' in sys.argv else ROOM
    errs = check(d)
    for e in errs: print('FAIL', e)
    print('verify_env:', d, 'ok' if not errs else '%d problems' % len(errs)); sys.exit(1 if errs else 0)
