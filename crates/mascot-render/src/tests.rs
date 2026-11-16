use super::*;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use mascot_core::{FaceMeta, Intent};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bear() -> &'static PackMeta {
    static P: OnceLock<PackMeta> = OnceLock::new();
    P.get_or_init(|| mascot_core::pack::load(&repo().join("mascots/bear")).expect("mascots/bear loads"))
}

fn art() -> &'static Art {
    static A: OnceLock<Art> = OnceLock::new();
    A.get_or_init(|| Art::load(bear()).expect("bear art loads"))
}

fn show(behavior: &str, clip: &str, dir: Dir, ori: Option<Ori>, frame: u16) -> Show {
    Show {
        intent: Intent::Idle,
        behavior: behavior.into(),
        clip: clip.into(),
        dir,
        ori,
        frame,
        eyes: EyeState::Open,
        mouth: MouthState::Closed,
        effect: None,
        visibility: Visibility::Shown,
        moved_px: [0, 0],
        cut: false,
        place: None,
    }
}

const BARE: View = View { cam: [0, 0], floor: false, vignette: false, patch: None, scenes: true, hour: 12 };

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mascot-render-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// Straight RGBA of pixel (x, y) on the canvas.
fn px(c: &Canvas, x: i32, y: i32) -> [u8; 4] {
    let o = (y as usize * c.w + x as usize) * 4;
    let p = &c.px[o..o + 4];
    [p[2], p[1], p[0], p[3]]
}

/// Every pixel of frame `f` of the strip, placed with its anchor on `at`, matches the canvas.
fn assert_frame_at(c: &Canvas, strip: &Path, fw: u32, fh: u32, f: usize, anchor: [i32; 2], at: [i32; 2]) {
    let (w, _, rgba) = read_png(strip).unwrap();
    let mut opaque = 0;
    for y in 0..fh as i32 {
        for x in 0..fw as i32 {
            let s = ((y as usize) * w + f * fw as usize + x as usize) * 4;
            if rgba[s + 3] == 0 {
                continue;
            }
            opaque += 1;
            let (cx, cy) = (at[0] - anchor[0] + x, at[1] - anchor[1] + y);
            assert_eq!(px(c, cx, cy), [rgba[s], rgba[s + 1], rgba[s + 2], rgba[s + 3]], "{} frame {f} pixel {x},{y}", strip.display());
        }
    }
    assert!(opaque > 100, "frame has art");
}

#[test]
fn the_canvas_holds_every_free_frame_and_set_with_a_margin() {
    let (w, h) = canvas_size(bear());
    let [ax, ay] = art().anchor();
    for c in bear().clips.values().filter(|c| c.set.is_none()) {
        assert!(ax - c.anchor[0] >= MARGIN && ay - c.anchor[1] >= MARGIN, "{}", c.name);
        assert!(ax - c.anchor[0] + c.frame_w as i32 + MARGIN <= w as i32, "{}", c.name);
        assert!(ay - c.anchor[1] + c.frame_h as i32 + MARGIN <= h as i32, "{}", c.name);
    }
    assert!(w % 2 == 0 && h % 2 == 0);
}

#[test]
fn idle_s_frame_0_is_the_strip_pixels_at_the_anchor() {
    let c0 = &bear().clips["idle"];
    let mut out = art().canvas();
    compose(art(), &show("rest", "idle", Dir::S, None, 0), &BARE, &mut out);
    assert_frame_at(&out, &c0.dirs[&Dir::S].file, c0.frame_w, c0.frame_h, 0, c0.anchor, art().anchor());
    // and nothing else: every opaque canvas pixel is a strip pixel
    let n = out.px.chunks(4).filter(|p| p[3] > 0).count();
    let (w, _, rgba) = read_png(&c0.dirs[&Dir::S].file).unwrap();
    let mut m = 0;
    for y in 0..c0.frame_h as usize {
        for x in 0..c0.frame_w as usize {
            m += (rgba[(y * w + x) * 4 + 3] > 0) as usize;
        }
    }
    assert_eq!(n, m);
    // sprites are alpha 0/255 only, so the bear is all hit and the floor is not
    let mut floor = art().canvas();
    compose(art(), &show("rest", "idle", Dir::S, None, 0), &View::default(), &mut floor);
    let [ax, ay] = art().anchor();
    assert!(hit(&floor, ax as usize, (ay - 40) as usize));
    assert!(!hit(&floor, 1, 1));
    assert!(floor.px.chunks(4).filter(|p| p[3] > 0).count() > n, "the floor grid draws");
}

#[test]
fn a_set_frame_sits_so_its_handoff_is_the_bear_anchor() {
    let p = bear();
    let (name, clip) = p.clips.iter().find(|(_, c)| c.set.as_deref() == Some("bed")).expect("a bed clip");
    let behavior = p.behaviors.iter().find(|(_, b)| b.set.as_deref() == Some("bed")).unwrap().0;
    for ori in [Ori::A, Ori::B] {
        let so = &p.sets["bed"].oris[&ori];
        let h = clip.handoff.get(&ori).map(|h| h.px).or_else(|| handoff_px(p, "bed", ori)).unwrap();
        let mut out = art().canvas();
        compose(art(), &show(behavior, name, Dir::S, Some(ori), 0), &BARE, &mut out);
        let [ax, ay] = art().anchor();
        assert_frame_at(&out, &clip.oris[&ori].file, so.frame_w, so.frame_h, 0, so.anchor, [ax - h[0], ay - h[1]]);
    }
}

#[test]
fn hidden_is_fully_transparent() {
    let mut out = art().canvas();
    let mut s = show("rest", "idle", Dir::S, None, 0);
    s.visibility = Visibility::Hidden;
    compose(art(), &s, &View::default(), &mut out);
    assert!(out.px.iter().all(|&b| b == 0));
}

#[test]
fn held_frames_share_a_frame_id_and_different_ones_do_not() {
    let a = art();
    let ids: Vec<u64> = (0..bear().clips["idle"].frames_n() as u16).map(|f| a.frame_id(&show("rest", "idle", Dir::S, None, f))).collect();
    let distinct: std::collections::HashSet<_> = ids.iter().collect();
    assert!(distinct.len() > 1 && distinct.len() < ids.len(), "idle holds repeat poses: {distinct:?}");
    assert_ne!(a.frame_id(&show("rest", "idle", Dir::S, None, 0)), a.frame_id(&show("rest", "idle", Dir::N, None, 0)));
}

#[test]
fn face_patches_draw_at_their_frame_positions() {
    let d = scratch("face");
    let mut p = bear().clone();
    // a 3x2 patch per state, a colour each, so the test can tell which one drew
    let mut layers: BTreeMap<String, BTreeMap<String, BTreeMap<Dir, mascot_core::pack::FaceLayer>>> = BTreeMap::new();
    let states = [("eyes", "open", [10u8, 20, 30]), ("eyes", "closed", [40, 50, 60]), ("mouth", "closed", [70, 80, 90]), ("mouth", "open", [1, 2, 3]), ("mouth", "wide", [4, 5, 6])];
    for (slot, state, rgb) in states {
        let f = d.join(format!("{slot}_{state}.png"));
        let rgba: Vec<u8> = (0..6).flat_map(|_| [rgb[0], rgb[1], rgb[2], 255]).collect();
        write_png(&f, 3, 2, &rgba).unwrap();
        layers.entry(slot.into()).or_default().entry(state.into()).or_default().insert(Dir::S, mascot_core::pack::FaceLayer { file: f, w: 3, h: 2 });
    }
    let n = p.clips["idle"].frames_n();
    let pos = |x: i32| (0..n).map(|f| [x + f as i32, 30]).collect::<Vec<_>>();
    let slots = BTreeMap::from([("eyes".to_string(), pos(30)), ("mouth".to_string(), pos(40))]);
    p.face = Some(FaceMeta {
        dirs: vec![Dir::S],
        layers,
        clips: BTreeMap::from([("idle".to_string(), BTreeMap::from([(Dir::S, slots)]))]),
        patch_dirs: BTreeMap::new(),
        set_clips: BTreeMap::new(),
    });
    let art = Art::load(&p).unwrap();
    let c0 = &p.clips["idle"];
    let [ax, ay] = art.anchor();
    let (ox, oy) = (ax - c0.anchor[0], ay - c0.anchor[1]);
    let mut out = art.canvas();
    let mut s = show("rest", "idle", Dir::S, None, 2);
    s.eyes = EyeState::Closed;
    s.mouth = MouthState::Wide;
    compose(&art, &s, &BARE, &mut out);
    assert_eq!(px(&out, ox + 32, oy + 30), [40, 50, 60, 255], "closed eyes at frame 2's spot");
    assert_eq!(px(&out, ox + 44, oy + 31), [4, 5, 6, 255], "wide mouth, bottom-right patch pixel");
    // a direction the face doesn't cover keeps the baked face
    let mut north = art.canvas();
    compose(&art, &show("rest", "idle", Dir::N, None, 2), &BARE, &mut north);
    let mut plain = art.canvas();
    compose(self::art(), &show("rest", "idle", Dir::N, None, 2), &BARE, &mut plain);
    assert_eq!(north, plain);
    std::fs::remove_dir_all(d).ok();
}

/// AC7: face v2 `patch_dirs` — a `listen` frame facing E draws the SE patches (the head turned
/// toward the camera) at E's positions; the idle clip keeps its own dir's patches.
#[test]
fn ac7_listen_draws_the_patch_dir_at_the_body_dirs_positions() {
    let p = mascot_core::pack::load(&repo().join("crates/mascot-core/tests/fixtures/listen_pack")).expect("listen fixture loads");
    let art = Art::load(&p).unwrap();
    let face = p.face.as_ref().unwrap();
    let c0 = &p.clips["listen"];
    let [ax, ay] = art.anchor();
    let (ox, oy) = (ax - c0.anchor[0], ay - c0.anchor[1]);
    for (clip, dir, patch) in [("listen", Dir::E, Dir::SE), ("listen", Dir::W, Dir::SW), ("listen", Dir::SE, Dir::S), ("idle", Dir::E, Dir::E)] {
        let f = 3;
        let mut out = art.canvas();
        let mut s = show(clip, clip, dir, None, f as u16);
        s.eyes = EyeState::Closed;
        compose(&art, &s, &BARE, &mut out);
        let at = face.clips[clip][&dir]["eyes"][f];
        let layer = &face.layers["eyes"]["closed"][&patch];
        let (w, h, rgba) = read_png(&layer.file).unwrap();
        let mut opaque = 0;
        for y in 0..h {
            for x in 0..w {
                let o = (y * w + x) * 4;
                if rgba[o + 3] == 255 {
                    opaque += 1;
                    assert_eq!(px(&out, ox + at[0] + x as i32, oy + at[1] + y as i32), [rgba[o], rgba[o + 1], rgba[o + 2], 255], "{clip} {dir:?}: {patch:?} patch pixel ({x},{y})");
                }
            }
        }
        assert!(opaque > 0, "{clip} {dir:?}");
    }
}

#[test]
fn upscale_repeats_each_pixel_k_by_k() {
    let c = Canvas { w: 2, h: 1, px: vec![1, 2, 3, 4, 5, 6, 7, 8] };
    let u = upscale(&c, 3);
    assert_eq!((u.w, u.h), (6, 3));
    for y in 0..3 {
        assert_eq!(&u.px[y * 24..y * 24 + 24], [[1, 2, 3, 4]; 3].concat().into_iter().chain([[5, 6, 7, 8]; 3].concat()).collect::<Vec<u8>>().as_slice());
    }
    assert_eq!(upscale(&c, 0), c, "k < 1 is 1");
}

#[test]
fn to_rgba_unpremultiplies() {
    let c = Canvas { w: 2, h: 1, px: vec![0, 64, 128, 128, 9, 9, 9, 0] };
    assert_eq!(to_rgba(&c), vec![255, 128, 0, 128, 0, 0, 0, 0]);
}

#[test]
fn a_missing_strip_names_the_file() {
    let mut p = bear().clone();
    let gone = PathBuf::from("/nonexistent/desk-buddy/idle_S.png");
    p.clips.get_mut("idle").unwrap().dirs.get_mut(&Dir::S).unwrap().file = gone.clone();
    let e = Art::load(&p).err().expect("load fails");
    assert!(e.to_string().starts_with(&format!("art {}: ", gone.display())), "{e}");
}

#[test]
fn a_strip_too_small_for_its_frames_is_a_size_error() {
    let d = scratch("size");
    let f = d.join("tiny.png");
    write_png(&f, 4, 4, &[255; 64]).unwrap();
    let mut p = bear().clone();
    p.clips.get_mut("idle").unwrap().dirs.get_mut(&Dir::S).unwrap().file = f.clone();
    let e = Art::load(&p).err().expect("load fails");
    assert!(matches!(e, RenderError::Size { .. }), "{e}");
    assert!(e.to_string().contains("does not fit"), "{e}");
    std::fs::remove_dir_all(d).ok();
}

/// Golden (design app-shell checks): idle S frame 0 composed with the floor and vignette
/// on the pack's fixed canvas equals the checked-in PNG. A deliberate re-render of the
/// bear changes it: regenerate with `UPDATE_GOLDEN=1 cargo test -p mascot-render golden`
/// and look at the new PNG before committing it.
#[test]
fn golden_idle_s_frame_0_matches_the_checked_in_png() {
    let golden = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/idle_S_0.png");
    let mut out = art().canvas();
    compose(art(), &show("rest", "idle", Dir::S, None, 0), &View::default(), &mut out);
    let rgba = to_rgba(&out);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        write_png(&golden, out.w, out.h, &rgba).unwrap();
    }
    let (w, h, want) = read_png(&golden).expect("tests/golden/idle_S_0.png (UPDATE_GOLDEN=1 writes it)");
    assert_eq!((w, h), (out.w, out.h), "canvas size");
    let diff = rgba.chunks(4).zip(want.chunks(4)).filter(|(a, b)| a != b).count();
    assert_eq!(diff, 0, "{diff} pixels differ from {}", golden.display());
}

#[test]
fn hit_mask_agrees_with_hit() {
    let mut out = art().canvas();
    compose(art(), &show("rest", "idle", Dir::S, None, 0), &View::default(), &mut out);
    let mut mask = vec![false; out.w * out.h];
    hit_mask(&out, &mut mask);
    assert!(mask.iter().any(|&m| m) && mask.iter().any(|&m| !m));
    for (i, &m) in mask.iter().enumerate() {
        assert_eq!(m, hit(&out, i % out.w, i / out.w));
    }
}

/// Desk (p3): face v2 `set_clips` — a seated desk_talk frame draws the named patch dir's layers
/// at the set_clips positions, measured from the baked set frame's top-left; desk_type (no
/// set face) stays the baked frame.
#[test]
fn desk_talk_draws_the_set_face_on_the_set_frame() {
    let p = mascot_core::pack::load(&repo().join("crates/mascot-core/tests/fixtures/desk_pack")).expect("desk fixture loads");
    let art = Art::load(&p).unwrap();
    let face = p.face.as_ref().unwrap();
    let [ax, ay] = art.anchor();
    for (clip, ori) in [("desk_talk", Ori::A), ("desk_talk", Ori::B), ("desk_ask", Ori::A)] {
        let so = &p.sets["desk"].oris[&ori];
        let h = handoff_px(&p, "desk", ori).expect("desk hand-off");
        let (ox, oy) = (ax - h[0] - so.anchor[0], ay - h[1] - so.anchor[1]);
        let sf = face.set_face(clip, ori).expect("set face");
        let f = 5;
        let mut out = art.canvas();
        let mut s = show("desk", clip, Dir::S, Some(ori), f as u16);
        s.eyes = EyeState::Closed;
        s.mouth = MouthState::Wide;
        compose(&art, &s, &BARE, &mut out);
        for (slot, state) in [("eyes", "closed"), ("mouth", "wide")] {
            let at = sf.slots[slot][f];
            let layer = &face.layers[slot][state][&sf.patch];
            let (w, hh, rgba) = read_png(&layer.file).unwrap();
            let mut opaque = 0;
            for y in 0..hh {
                for x in 0..w {
                    let o = (y * w + x) * 4;
                    if rgba[o + 3] == 255 {
                        opaque += 1;
                        assert_eq!(px(&out, ox + at[0] + x as i32, oy + at[1] + y as i32), [rgba[o], rgba[o + 1], rgba[o + 2], 255], "{clip} {ori:?} {slot}: pixel ({x},{y})");
                    }
                }
            }
            assert!(opaque > 0, "{clip} {ori:?} {slot}");
        }
        // the default face (open, closed) on a talk frame changes the baked frame only inside the patches
        let mut plain = art.canvas();
        compose(&art, &show("desk", clip, Dir::S, Some(ori), f as u16), &BARE, &mut plain);
        assert_ne!(plain, out, "{clip} {ori:?}: the face state shows");
    }
    // desk_type has no set face: eyes/mouth states do not change it
    let mut a = art.canvas();
    let mut b = art.canvas();
    compose(&art, &show("desk", "desk_type", Dir::S, Some(Ori::A), 3), &BARE, &mut a);
    let mut s = show("desk", "desk_type", Dir::S, Some(Ori::A), 3);
    s.mouth = MouthState::Wide;
    s.eyes = EyeState::Closed;
    compose(&art, &s, &BARE, &mut b);
    assert_eq!(a, b);
    let so = &p.sets["desk"].oris[&Ori::A];
    let h = handoff_px(&p, "desk", Ori::A).unwrap();
    assert_frame_at(&a, &p.clips["desk_type"].oris[&Ori::A].file, so.frame_w, so.frame_h, 3, so.anchor, [ax - h[0], ay - h[1]]);
}

// ---------- room art: floors and pocket scenes ----------

fn room() -> &'static RoomMeta {
    static R: OnceLock<RoomMeta> = OnceLock::new();
    R.get_or_init(|| mascot_core::room::load(&repo().join("rooms/starter")).expect("rooms/starter loads"))
}

fn room_art() -> &'static Art {
    static A: OnceLock<Art> = OnceLock::new();
    A.get_or_init(|| Art::load_with(bear(), Some(room())).expect("bear + starter room art loads"))
}

fn compose_room(s: &Show, v: &View) -> Canvas {
    let mut c = room_art().canvas();
    compose(room_art(), s, v, &mut c);
    c
}

/// `px`, un-premultiplied (room pixels are composed at alpha 254).
fn straight(c: &Canvas, x: i32, y: i32) -> [u8; 4] {
    let p = px(c, x, y);
    let a = p[3] as u32;
    let un = |v: u8| if a == 0 { 0 } else { ((v as u32 * 255 + a / 2) / a).min(255) as u8 };
    [un(p[0]), un(p[1]), un(p[2]), p[3]]
}

/// Same colour? Alpha 254 cannot hold every 8-bit value exactly, so a channel may be 1 off.
fn close(a: &[u8], b: &[u8]) -> bool {
    a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 1)
}

/// Straight RGBA of every pixel of a room PNG.
fn room_png(rel: &str) -> (i32, i32, Vec<u8>) {
    let (w, h, rgba) = read_png(&repo().join("rooms/starter").join(rel)).unwrap();
    (w as i32, h as i32, rgba)
}

/// How many opaque pixels of `rel` (origin on the anchor, mirrored or not) show their own colour at
/// alpha 254 on the canvas, and how many are covered by the mascot (alpha 255).
fn room_pixels(c: &Canvas, rel: &str, origin: [i32; 2], flip: bool) -> (usize, usize, usize) {
    let (w, h, rgba) = room_png(rel);
    let [ax, ay] = room_art().anchor();
    let (mut same, mut covered, mut wrong) = (0, 0, 0);
    for y in 0..h {
        for x in 0..w {
            let p = &rgba[((y * w + x) * 4) as usize..][..4];
            if p[3] == 0 {
                continue;
            }
            let d = x - origin[0];
            let got = straight(c, if flip { ax - d } else { ax + d }, ay + y - origin[1]);
            match got[3] {
                254 if close(&got[..3], &p[..3]) => same += 1,
                255 => covered += 1,
                _ => wrong += 1,
            }
        }
    }
    (same, covered, wrong)
}

const LAPTOP: (&str, &str) = ("laptop", "laptop_type");

#[test]
fn the_canvas_holds_all_room_art_and_the_bear_alone_keeps_its_size() {
    let (w, h) = room_art().size();
    let [ax, ay] = room_art().anchor();
    for f in &room().floors {
        let (fw, fh, _) = room_png(f.file.strip_prefix(&room().dir).unwrap().to_str().unwrap());
        assert!(ax - f.origin[0] >= 0 && ay - f.origin[1] >= 0 && ax - f.origin[0] + fw <= w as i32 && ay - f.origin[1] + fh <= h as i32, "floor {}", f.id);
    }
    for s in &room().scenes {
        let (sw, sh, _) = room_png(s.variants[0].back.strip_prefix(&room().dir).unwrap().to_str().unwrap());
        let (l, r) = (s.origin[0], sw - 1 - s.origin[0]);
        // both placements: as made (A) and mirrored (B)
        assert!(ax - l.max(r) >= 0 && ax + l.max(r) < w as i32 && ay - s.origin[1] >= 0 && ay - s.origin[1] + sh <= h as i32, "scene {}", s.id);
    }
    assert_eq!(art().size(), canvas_size(bear()), "without a room the canvas is the pack's own");
    assert!(w > art().size().0 && h > art().size().1);
}

#[test]
fn a_floor_patch_is_drawn_under_the_bear_and_never_takes_a_click() {
    let s = show("idle", "idle", Dir::S, None, 0);
    for (i, f) in room().floors.iter().enumerate() {
        assert_eq!(room_art().floor_index(&f.id), Some(i));
        let c = compose_room(&s, &View { patch: Some(i), ..View::default() });
        let rel = format!("floors/{}.png", f.id);
        let (same, covered, wrong) = room_pixels(&c, &rel, f.origin, false);
        assert_eq!(wrong, 0, "{rel}: every floor pixel shows or is behind the bear");
        assert!(same > 3000 && covered > 50, "{rel}: {same} shown, {covered} behind the bear");
        // the hit mask is the bear's alone: the same as with no floor at all
        let bare = compose_room(&s, &View { floor: false, ..View::default() });
        let (mut a, mut b) = (vec![false; c.w * c.h], vec![false; c.w * c.h]);
        hit_mask(&c, &mut a);
        hit_mask(&bare, &mut b);
        assert!(a == b && a.iter().any(|h| *h), "{rel}: clicks reach only the bear");
    }
    assert_eq!(room_art().floor_index("lava"), None);
    // an index the room does not have draws the grid's fallback (nothing), not a panic
    let c = compose_room(&s, &View { patch: Some(99), floor: false, ..View::default() });
    assert_eq!(c, compose_room(&s, &View { floor: false, ..View::default() }));
}

#[test]
fn a_scene_shows_during_its_activity_only_and_follows_the_hour() {
    let v = |hour| View { hour, ..View::default() };
    let s = show(LAPTOP.0, LAPTOP.1, Dir::SE, Some(Ori::A), 0);
    let nook = &room().scenes[0];
    for (hour, name) in [(6, "dawn"), (12, "day"), (18, "dusk"), (23, "night"), (2, "night")] {
        let c = compose_room(&s, &v(hour));
        let (same, covered, wrong) = room_pixels(&c, &format!("scenes/nook_{name}.png"), nook.origin, false);
        assert_eq!(wrong, 0, "nook {name} at {hour}h");
        assert!(same > 20_000 && covered > 500, "nook {name}: {same} shown, {covered} behind the bear");
    }
    assert_ne!(compose_room(&s, &v(12)), compose_room(&s, &v(23)));
    // the scene has no floor of its own: it sits on the selected floor patch, which shows around it
    let on = compose_room(&s, &View { patch: Some(0), ..v(12) });
    let (same, _, wrong) = room_pixels(&on, "scenes/nook_day.png", nook.origin, false);
    assert!(wrong == 0 && same > 5_000, "nook over a floor: {same} shown, {wrong} wrong");
    let f = &room().floors[0];
    let (floor_px, _, _) = room_pixels(&on, f.file.strip_prefix(&room().dir).unwrap().to_str().unwrap(), f.origin, false);
    assert!(floor_px > 5_000, "the floor shows around the scene: {floor_px}");
    assert_ne!(on, compose_room(&s, &View { floor: false, ..v(12) }));
    // scenes off: the activity as before (no room pixel at all)
    let off = compose_room(&s, &View { scenes: false, floor: false, ..v(12) });
    assert!(off.px.chunks_exact(4).all(|p| p[3] != 254));
    // not this scene's activity: nothing
    let idle = show("idle", "idle", Dir::S, None, 0);
    assert_eq!(room_art().scene(&idle, &v(12)), None);
    assert!(compose_room(&idle, &View { floor: false, ..v(12) }).px.chunks_exact(4).all(|p| p[3] != 254));
    // every scene is wired to a behavior the bear has
    for sc in &room().scenes {
        for b in &sc.for_ {
            assert!(bear().behaviors.contains_key(b), "scene {} is for {b}, which the bear does not have", sc.id);
        }
    }
    // floor sitting is a free clip (no set): the grove shows in any direction, never mirrored
    let sit = show("sit_floor", "sit_floor", Dir::W, None, 3);
    assert_eq!(room_art().scene(&sit, &v(12)), Some((3, 0, false)));
    assert_eq!(room_art().scene(&sit, &v(22)), Some((3, 1, false)));
    let mut hidden = s.clone();
    hidden.visibility = Visibility::Hidden;
    assert_eq!(room_art().scene(&hidden, &v(12)), None);
}

#[test]
fn the_tea_table_is_drawn_over_the_bear_and_scenes_never_take_a_click() {
    let s = show("tea", "tea_sip", Dir::SE, Some(Ori::A), 0);
    let c = compose_room(&s, &View::default());
    let tea = &room().scenes[2];
    // every front pixel shows its own colour, over the bear (255) or over the room (254)
    let (w, h, rgba) = room_png("scenes/tearoom_base_front.png");
    let [ax, ay] = room_art().anchor();
    let (mut n, mut over_bear) = (0, 0);
    for y in 0..h {
        for x in 0..w {
            let p = &rgba[((y * w + x) * 4) as usize..][..4];
            if p[3] == 255 {
                let got = straight(&c, ax + x - tea.origin[0], ay + y - tea.origin[1]);
                assert!(close(&got[..3], &p[..3]), "front pixel {x},{y}: {got:?} is not {p:?}");
                n += 1;
                over_bear += (got[3] == 255) as usize;
            }
        }
    }
    assert!(n > 500, "the front layer has art ({n} px)");
    // clicks: exactly where the bear's own frame is opaque
    let bare = compose_room(&s, &View { scenes: false, floor: false, ..View::default() });
    let (mut a, mut b) = (vec![false; c.w * c.h], vec![false; c.w * c.h]);
    hit_mask(&c, &mut a);
    hit_mask(&bare, &mut b);
    assert!(a == b, "the scene adds no clickable pixel ({over_bear} front px sit over the bear)");
}

#[test]
fn placement_b_mirrors_the_scene_about_the_bear() {
    let s = show(LAPTOP.0, LAPTOP.1, Dir::SW, Some(Ori::B), 0);
    assert_eq!(room_art().scene(&s, &View::default()), Some((0, 1, true)));
    let c = compose_room(&s, &View::default());
    let (same, covered, wrong) = room_pixels(&c, "scenes/nook_day.png", room().scenes[0].origin, true);
    assert_eq!(wrong, 0);
    assert!(same > 20_000 && covered > 500, "{same} shown, {covered} behind the bear");
    // and not as made
    let (_, _, wrong) = room_pixels(&c, "scenes/nook_day.png", room().scenes[0].origin, false);
    assert!(wrong > 1000);
    // still turning toward the set (no placement yet): no scene, so it never shows unmirrored first
    let turning = show(LAPTOP.0, "idle", Dir::SW, None, 0);
    assert_eq!(room_art().scene(&turning, &View::default()), None);
    assert!(compose_room(&turning, &View { floor: false, ..View::default() }).px.chunks_exact(4).all(|p| p[3] != 254));
}

#[test]
fn bad_room_art_is_refused() {
    let d = scratch("badroom");
    let write = |name: &str, w: usize, h: usize, rgba: &[u8]| write_png(&d.join(name), w, h, rgba).unwrap();
    write("ok.png", 2, 2, &[10, 20, 30, 255, 0, 0, 0, 0, 0, 0, 0, 0, 10, 20, 30, 255]);
    write("soft.png", 2, 2, &[10, 20, 30, 128, 0, 0, 0, 0, 0, 0, 0, 0, 10, 20, 30, 255]);
    write("big.png", 3, 2, &[10, 20, 30, 255].repeat(6));
    let load = |json: &str| {
        std::fs::write(d.join("room.json"), json).unwrap();
        Art::load_with(bear(), Some(&mascot_core::room::load(&d).unwrap())).err().map(|e| e.to_string())
    };
    let floor = |file: &str, origin: &str| format!(r#"{{"room_version":1,"id":"t","floors":[{{"id":"f","file":"{file}","origin":{origin}}}]}}"#);
    assert_eq!(load(&floor("ok.png", "[1,1]")), None);
    assert!(load(&floor("soft.png", "[1,1]")).unwrap().contains("alpha 0 or 255"));
    assert!(load(&floor("ok.png", "[2,0]")).unwrap().contains("outside the 2x2 image"));
    let scene = r#"{"room_version":1,"id":"t","scenes":[{"id":"s","for":["tea"],"origin":[0,0],"variants":[{"name":"v","back":"ok.png","front":"big.png"}]}]}"#;
    assert!(load(scene).unwrap().contains("front is 3x2, back is 2x2"));
    std::fs::write(d.join("junk.png"), b"not a png").unwrap();
    assert!(load(&floor("junk.png", "[0,0]")).is_some());
}
