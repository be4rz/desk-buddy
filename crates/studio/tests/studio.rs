//! AC11 checks of the headless studio on the real bear pack and the blob fixture.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mascot_core::{EyeState, MouthState};
use studio::{aseprite, audit, diff, lint, session::Session, RenderOpts};

/// `buddy studio render --pack mascots/bear --clip idle --dir SE --frame 7 --zoom 4`. A change
/// here means the bear's idle art (or the renderer) changed: re-check the art, then update.
const GOLDEN_IDLE_SE_7_X4: &str = "49fc734862d6a50b22f8aacb87bfcdae7488e0f8045cd2c223c64256a07dc0c3";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bear() -> Session {
    Session::open(&root().join("mascots/bear")).expect("bear pack")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("studio-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_buddy-studio"))
}

fn tree(d: &Path) -> BTreeSet<PathBuf> {
    let mut out = BTreeSet::new();
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(&p) {
            for e in rd.flatten() {
                out.insert(e.path());
                stack.push(e.path());
            }
        }
    }
    out
}

#[test]
fn render_is_byte_stable_and_matches_the_golden_hash() {
    let s = bear();
    let o = RenderOpts { clip: "idle".into(), view: "SE".into(), frame: 7, zoom: 4, eyes: EyeState::Open, mouth: MouthState::Closed };
    let a = studio::render(&s.pack, &s.art, &o).unwrap();
    let b = studio::render(&s.pack, &s.art, &o).unwrap();
    assert_eq!((a.w, a.h), (336, 492));
    let (pa, pb) = (a.png(), b.png());
    assert_eq!(pa, pb, "two renders give the same bytes");
    assert_eq!(studio::sha256_hex(&pa), GOLDEN_IDLE_SE_7_X4);
    // and through the CLI, twice
    let d = tmp("render");
    for f in ["a.png", "b.png"] {
        let out = bin().args(["render", "--pack"]).arg(root().join("mascots/bear")).args(["--clip", "idle", "--dir", "SE", "--frame", "7", "--zoom", "4", "--out"]).arg(d.join(f)).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        let line = String::from_utf8_lossy(&out.stdout);
        assert!(line.starts_with("ok ") && line.contains("336×492 sha256 ") && line.trim_end().ends_with(GOLDEN_IDLE_SE_7_X4), "{line}");
    }
    assert_eq!(std::fs::read(d.join("a.png")).unwrap(), pa);
    let out = bin().args(["diff"]).arg(d.join("a.png")).arg(d.join("b.png")).output().unwrap();
    assert_eq!(out.status.code(), Some(0));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bad_render_args() {
    let s = bear();
    let base = RenderOpts { clip: "idle".into(), view: "SE".into(), frame: 0, zoom: 1, eyes: EyeState::Open, mouth: MouthState::Closed };
    assert!(studio::render(&s.pack, &s.art, &RenderOpts { clip: "nope".into(), ..base.clone() }).is_err());
    assert!(studio::render(&s.pack, &s.art, &RenderOpts { view: "Q".into(), ..base.clone() }).is_err());
    assert!(studio::render(&s.pack, &s.art, &RenderOpts { frame: 9999, ..base.clone() }).is_err());
    let out = bin().args(["render", "--pack", "x"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2), "a missing --clip is usage");
    let out = bin().args(["frobnicate"]).output().unwrap();
    assert_eq!(out.status.code(), Some(2));
}

#[test]
fn diff_of_a_one_pixel_change() {
    let s = bear();
    let o = RenderOpts { clip: "idle".into(), view: "S".into(), frame: 0, zoom: 1, eyes: EyeState::Open, mouth: MouthState::Closed };
    let a = studio::render(&s.pack, &s.art, &o).unwrap();
    let mut b = a.rgba.clone();
    let i = (40 * a.w + 30) * 4;
    b[i] ^= 0x10;
    let (r, ov) = diff::diff((a.w, a.h, &a.rgba), (a.w, a.h, &b), true).unwrap();
    assert_eq!(r.changed_px, 1);
    assert_eq!(r.bbox, Some([30, 40, 1, 1]));
    assert!(ov.is_some());
    let j = serde_json::to_value(&r).unwrap();
    assert_eq!(j["changed_px"], 1);
    // the CLI: exit 1 (a finding), JSON on --json
    let d = tmp("diff");
    std::fs::write(d.join("a.png"), a.png()).unwrap();
    std::fs::write(d.join("b.png"), studio::png_bytes(a.w, a.h, &b)).unwrap();
    let out = bin().arg("diff").arg(d.join("a.png")).arg(d.join("b.png")).arg("--json").args(["--out"]).arg(d.join("o.png")).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(j["changed_px"], 1);
    assert_eq!(j["bbox"], serde_json::json!([30, 40, 1, 1]));
    assert!(d.join("o.png").is_file());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn lint_on_the_bear_has_no_errors() {
    let s = bear();
    let r = lint::lint(&s.pack);
    let errs: Vec<_> = r.issues.iter().filter(|i| i.severity == "error").collect();
    assert_eq!(r.errors, 0, "{errs:#?}");
    assert!(r.files > 100);
    // the only warnings are the walk/run cycles' sub-pixel root motion
    assert!(r.issues.iter().all(|i| i.code == "non_integer_root"), "{:#?}", r.issues);
}

fn import_blob(pack: &Path, name: &str) -> aseprite::Imported {
    aseprite::import(&root().join("crates/studio/tests/fixtures/blob.aseprite"), pack, name).unwrap()
}

#[test]
fn lint_fixture_one_off_palette_one_partial_alpha() {
    let d = tmp("lintfix");
    let p = d.join("p");
    import_blob(&p, "blob");
    // the bear's palettes, then two pixels of the S strip changed in the middle of the body
    let pj = p.join("pack.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
    let ex = root().join("experiments");
    v["palettes"] = serde_json::json!([ex.join("bear-proto/palette-bear.json"), ex.join("scene-proto/palette.json"), ex.join("mascot/bear3d/palette-ext.json")]);
    std::fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let clean = lint::lint(&mascot_core::pack::load(&p).unwrap());
    assert_eq!(clean.errors, 0, "{:#?}", clean.issues);
    let strip = p.join("art/blob/blob_walk_S.png");
    let (w, h, mut px) = mascot_render::read_png(&strip).unwrap();
    let opaque = |px: &[u8], x: usize, y: usize| (0..9).all(|k| px[(((y + k / 3 - 1) * w) + x + k % 3 - 1) * 4 + 3] == 255);
    let mut spots = (1..h - 1).flat_map(|y| (1..51).map(move |x| (x, y))).filter(|&(x, y)| opaque(&px, x, y));
    let (x1, y1) = spots.next().unwrap();
    let (x2, y2) = spots.find(|&(x, y)| x.abs_diff(x1) > 2 || y.abs_diff(y1) > 2).unwrap();
    let i = (y1 * w + x1) * 4;
    px[i..i + 4].copy_from_slice(&[0x12, 0xfe, 0x34, 255]); // not in any palette
    let i = (y2 * w + x2) * 4;
    px[i + 3] = 128;
    std::fs::write(&strip, studio::png_bytes(w, h, &px)).unwrap();
    let r = lint::lint(&mascot_core::pack::load(&p).unwrap());
    let codes: Vec<_> = r.issues.iter().map(|i| (i.code, i.x, i.y)).collect();
    assert_eq!(codes, vec![("off_palette", Some(x1), Some(y1)), ("partial_alpha", Some(x2), Some(y2))], "{:#?}", r.issues);
    assert_eq!(r.errors, 2);
    let out = bin().args(["lint", "--json", "--pack"]).arg(&p).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(j["issues"].as_array().unwrap().len(), 2);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn import_copies_pixels_unchanged_and_the_pack_replays() {
    let d = tmp("import");
    let p = d.join("p");
    let r = import_blob(&p, "blob");
    assert!(r.created_pack);
    assert_eq!(r.clips, vec!["blob_walk"]);
    let pack = Arc::new(mascot_core::pack::load(&p).unwrap());
    mascot_render::Art::load(&pack).unwrap();
    let c = &pack.clips["blob_walk"];
    assert_eq!((c.frame_w, c.frame_h, c.durs_ms.len()), (52, 76, 6));
    // pixels: the first 6 frames of the Blender strips, byte for byte (the hidden layer skipped)
    for (dir, src) in [("S", "walk_64_S.png"), ("SE", "walk_64_SE.png")] {
        let (sw, _, s) = mascot_render::read_png(&root().join("experiments/mascot/out/bear3d/anim").join(src)).unwrap();
        let (w, h, o) = mascot_render::read_png(&p.join(format!("art/blob/blob_walk_{dir}.png"))).unwrap();
        assert_eq!((w, h), (52 * 6, 76));
        for y in 0..h {
            assert_eq!(&o[y * w * 4..(y + 1) * w * 4], &s[y * sw * 4..y * sw * 4 + w * 4], "{dir} row {y}");
        }
    }
    // the brain plays it, like mascot-replay
    let a = audit::run(pack.clone(), &audit::quiet(2.0, 1)).unwrap();
    assert!(a.starts > 0);
    // a second import into the existing pack adds a source; the same name is refused
    let before = std::fs::read_to_string(p.join("pack.json")).unwrap();
    let r2 = import_blob(&p, "blob2");
    assert!(!r2.created_pack);
    let pack = mascot_core::pack::load(&p).unwrap();
    assert!(pack.clips.contains_key("blob2_walk") && pack.clips.contains_key("blob_walk"));
    let snap = tree(&p);
    let pj = std::fs::read_to_string(p.join("pack.json")).unwrap();
    let e = aseprite::import(&root().join("crates/studio/tests/fixtures/blob.aseprite"), &p, "blob").unwrap_err();
    assert!(e.0.contains("already exists") && e.0.ends_with("nothing written"), "{e}");
    assert_eq!(tree(&p), snap);
    assert_eq!(std::fs::read_to_string(p.join("pack.json")).unwrap(), pj);
    assert_ne!(before, pj);
    // the CLI
    let out = bin().args(["import-aseprite"]).arg(root().join("crates/studio/tests/fixtures/blob.aseprite")).arg("--pack").arg(d.join("q")).args(["--name", "blob"]).output().unwrap();
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_corrupt_aseprite_writes_nothing() {
    let d = tmp("corrupt");
    let good = std::fs::read(root().join("crates/studio/tests/fixtures/blob.aseprite")).unwrap();
    // an existing pack must stay byte-identical; a new pack dir must not appear
    let p = d.join("p");
    import_blob(&p, "blob");
    let snap: Vec<_> = tree(&p).into_iter().map(|f| (std::fs::read(&f).ok(), f)).collect();
    let mut bad = good.clone();
    bad[128 + 4] = 0; // the first frame's magic
    let cases: Vec<(&str, Vec<u8>)> = vec![("truncated", good[..good.len() / 2].to_vec()), ("badmagic", bad), ("garbage", (0..4000u32).map(|i| (i.wrapping_mul(2654435761) >> 13) as u8).collect()), ("empty", vec![])];
    for (name, bytes) in cases {
        let f = d.join(format!("{name}.aseprite"));
        std::fs::write(&f, bytes).unwrap();
        for pack in [p.clone(), d.join("new")] {
            let e = aseprite::import(&f, &pack, "x").unwrap_err();
            assert!(e.0.ends_with("; nothing written"), "{name}: {e}");
        }
        assert!(!d.join("new").exists(), "{name}: a new pack dir was made");
        let now: Vec<_> = tree(&p).into_iter().map(|f| (std::fs::read(&f).ok(), f)).collect();
        assert_eq!(now, snap, "{name}: the pack changed");
        let out = bin().arg("import-aseprite").arg(&f).arg("--pack").arg(d.join("new")).args(["--name", "x"]).output().unwrap();
        assert_eq!(out.status.code(), Some(1));
        let msg = String::from_utf8_lossy(&out.stderr);
        assert!(msg.starts_with("import-aseprite: ") && msg.trim_end().ends_with("nothing written"), "{msg}");
    }
    assert!(!d.join("new").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn audit_is_reproducible_and_lively() {
    let s = bear();
    let a = audit::run(s.pack.clone(), &audit::quiet(30.0, 3)).unwrap();
    let b = audit::run(s.pack.clone(), &audit::quiet(30.0, 3)).unwrap();
    assert_eq!(a, b);
    assert!(a.distinct >= 5, "{a:?}");
    assert!(a.minutes >= 30.0);
    let order: Vec<String> = s.pack.idle_pool.iter().map(|e| e.behavior.clone()).collect();
    assert_eq!(audit::chart(&a, &order), audit::chart(&b, &order));
    let c = audit::run(s.pack.clone(), &audit::quiet(30.0, 4)).unwrap();
    assert_ne!(a.sequence, c.sequence, "another seed, another day");
    // the CLI writes both files, the same bytes twice
    let d = tmp("audit");
    for o in ["x", "y"] {
        let out = bin().args(["audit", "--pack"]).arg(root().join("mascots/bear")).args(["--minutes", "30", "--seed", "3", "--out"]).arg(d.join(o)).output().unwrap();
        assert!(out.status.success());
        assert!(String::from_utf8_lossy(&out.stdout).contains("distinct="));
    }
    for f in ["audit.json", "audit.png"] {
        assert_eq!(std::fs::read(d.join("x").join(f)).unwrap(), std::fs::read(d.join("y").join(f)).unwrap());
    }
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("x/audit.json")).unwrap()).unwrap();
    for k in ["plays", "distinct", "max_same_in_row", "entropy"] {
        assert!(j.get(k).is_some(), "{k}");
    }
    // --timeline goes through the same checks as mascot-replay
    let day = root().join("crates/mascot-core/tests/fixtures/day.json");
    let good = bin().args(["audit", "--pack"]).arg(root().join("mascots/bear")).arg("--timeline").arg(&day).arg("--out").arg(d.join("t")).output().unwrap();
    assert!(good.status.success(), "{}", String::from_utf8_lossy(&good.stderr));
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&day).unwrap()).unwrap();
    v["timeline_version"] = 9.into();
    std::fs::write(d.join("v9.json"), v.to_string()).unwrap();
    let bad = bin().args(["audit", "--pack"]).arg(root().join("mascots/bear")).arg("--timeline").arg(d.join("v9.json")).arg("--out").arg(d.join("v9")).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&bad.stderr).contains("timeline_version 9"), "{}", String::from_utf8_lossy(&bad.stderr));
    assert!(!d.join("v9").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn sheet_and_gif() {
    let s = bear();
    let (f, rows) = studio::sheet(&s.pack, &s.art, Some("idle"), 1, studio::Bg::Grid).unwrap();
    assert_eq!(rows.len(), 8);
    assert!(f.w > 0 && f.h > 0);
    let g1 = studio::gif(&s.pack, &s.art, "idle", "S", 2).unwrap();
    let g2 = studio::gif(&s.pack, &s.art, "idle", "S", 2).unwrap();
    assert_eq!(g1, g2);
    assert_eq!(&g1[..6], b"GIF89a");
}
