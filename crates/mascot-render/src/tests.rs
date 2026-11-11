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

const BARE: View = View { cam: [0, 0], floor: false, vignette: false };

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
