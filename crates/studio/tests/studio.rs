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
    // the only warnings are the walk/run cycles' sub-pixel root motion (anim_l/ has every turn clip),
    // and the desk station until anim_d/ is rendered (p3 desk-art)
    let desk_art = s.pack.available("desk");
    assert!(
        r.issues.iter().all(|i| i.code == "non_integer_root"
            || i.code == "waits_for_art"
            || (!desk_art && i.code == "station_art_missing" && i.detail.starts_with("station desk:"))),
        "{:#?}",
        r.issues
    );
}

#[test]
fn lint_on_the_listen_fixture_takes_front_only_listen_clips_and_v2_patch_dirs() {
    let p = mascot_core::pack::load(&root().join("crates/mascot-core/tests/fixtures/listen_pack")).unwrap();
    let r = lint::lint(&p);
    assert_eq!(r.errors, 0, "{:#?}", r.issues);
    assert!(r.issues.iter().all(|i| i.code == "non_integer_root"), "no missing_dir for listen, no turn warning: {:#?}", r.issues);
    // a turning block naming a clip the pack lacks: a warning, the pack still loads
    let mut q = p.clone();
    q.turning = Some(mascot_protocol::pack::TurningFile { ccw: "turn_ccw".into(), cw: "spin_cw".into() });
    let r = lint::lint(&q);
    let warn: Vec<_> = r.issues.iter().filter(|i| i.code == "turn_clips_missing").map(|i| (i.severity, i.file.as_str())).collect();
    assert_eq!(warn, vec![("warn", "spin_cw")]);
    assert_eq!(r.errors, 0);
}

/// Without a listen behavior, listen resolves to `rest`: rest's clips get no back-view exemption.
#[test]
fn lint_without_a_listen_behavior_still_checks_rest_back_views() {
    let mut p = mascot_core::pack::load(&root().join("crates/mascot-core/tests/fixtures/listen_pack")).unwrap();
    p.behaviors.remove("listen");
    p.intents.insert(mascot_core::Intent::Listen, vec!["rest".into()]);
    // a front-only clip in rest's seq: a real gap now
    p.behaviors.get_mut("rest").unwrap().seq.push(mascot_protocol::pack::StepDef { clip: "listen_in".into(), loops: Default::default() });
    let r = lint::lint(&p);
    assert!(r.issues.iter().any(|i| i.code == "missing_dir" && i.file == "listen_in"), "{:#?}", r.issues);
}

/// p1 leftover: import pre-checks with `load_report`, so a pack whose body block has a problem
/// (the app refuses it; lint reports it) still takes new art.
#[test]
fn import_into_a_pack_with_a_body_issue_still_works() {
    let d = tmp("import-body");
    let p = d.join("p");
    import_blob(&p, "blob");
    let pj = p.join("pack.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
    v["body"] = serde_json::json!({ "activities": { "juggle": { "plays": ["juggle"], "kind": "pastime", "about": "juggle" } } });
    std::fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    assert!(mascot_core::pack::load(&p).is_err(), "the strict load refuses it");
    assert!(!mascot_core::pack::load_report(&p).unwrap().body_issues.is_empty());
    let r = import_blob(&p, "blob2");
    assert!(!r.created_pack);
    assert!(mascot_core::pack::load_report(&p).unwrap().clips.contains_key("blob2_walk"));
    let _ = std::fs::remove_dir_all(&d);
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
    let order: Vec<String> = s.pack.idle_pool.iter().map(|e| e.name().to_string()).collect();
    assert_eq!(audit::chart(&a, &order), audit::chart(&b, &order));
    let c = audit::run(s.pack.clone(), &audit::quiet(30.0, 4)).unwrap();
    assert_ne!(a.sequence, c.sequence, "another seed, another day");
    // the CLI writes both files, the same bytes twice
    let d = tmp("audit");
    for o in ["x", "y"] {
        let out = bin().args(["audit", "--pack"]).arg(root().join("mascots/bear")).args(["--minutes", "30", "--seed", "3", "--out"]).arg(d.join(o)).output().unwrap();
        // the bear keeps its own variety rule and targets on this seed (AC17): exit 0
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        assert!(String::from_utf8_lossy(&out.stdout).contains("seed 3: "));
    }
    for f in ["audit.json", "audit.png"] {
        assert_eq!(std::fs::read(d.join("x").join(f)).unwrap(), std::fs::read(d.join("y").join(f)).unwrap());
    }
    let j: serde_json::Value = serde_json::from_slice(&std::fs::read(d.join("x/audit.json")).unwrap()).unwrap();
    for k in ["pack", "minutes", "seeds", "pass", "runs"] {
        assert!(j.get(k).is_some(), "{k}");
    }
    let run = &j["runs"][0];
    for k in ["seed", "start", "picks", "distinct", "relaxed", "sequence", "time", "behaviors", "kinds", "long", "violations", "targets", "pass"] {
        assert!(run.get(k).is_some(), "runs[0].{k}");
    }
    assert_eq!((j["seeds"].clone(), run["start"].clone()), (serde_json::json!([3]), serde_json::json!("2026-11-16T10:00")));
    // only idle pool entries with art are picked (hidden entries never are), derived from the pack
    // so new art in later phases does not move this test
    let names: Vec<&str> = run["sequence"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
    assert!(!names.is_empty());
    for n in &names {
        let e = s.pack.idle_pool.iter().find(|e| e.name() == *n);
        assert!(e.is_some_and(|e| e.station.is_some() || s.pack.available(n)), "{n}: picked but not an idle entry with art");
    }
    assert_eq!(run["pass"], true, "{run}");
    // usage errors exit 2
    for bad in [&["--seeds", "1,x"][..], &["--start", "monday"], &["--temperament", "grumpy=1"], &["--seed", "1", "--seeds", "2"]] {
        let o = bin().args(["audit", "--pack"]).arg(root().join("mascots/bear")).args(bad).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{bad:?}");
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

// ---- idle-brain (aliveness 2, p1): hidden art, v2 lint codes, audit v2 ----

fn fixture(name: &str) -> PathBuf {
    root().join("crates/mascot-core/tests/fixtures").join(name)
}

/// `#hidden-log`: entries the bear wires before their art lands are warnings only (exit 0), one per
/// waiting entry with its name and missing clip. The expected list comes from the pack itself, so
/// the test holds while later phases add art (and is empty once every wired name is rendered).
#[test]
fn lint_lists_entries_waiting_for_art_as_warnings() {
    // the bear has all its A4 art (p2), so lint a copy without stage f's sources: its fidgets wait
    let real = root().join("mascots/bear");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(real.join("pack.json")).unwrap()).unwrap();
    let abs = |s: &serde_json::Value| serde_json::json!(real.join(s.as_str().unwrap()));
    v["art_root"] = abs(&v["art_root"]);
    v["palettes"] = serde_json::Value::Array(v["palettes"].as_array().unwrap().iter().map(abs).collect());
    v["sources"].as_array_mut().unwrap().retain(|s| !s["path"].as_str().unwrap().starts_with("anim_f/"));
    let dir = tmp("waits-for-art");
    std::fs::write(dir.join("pack.json"), v.to_string()).unwrap();
    // the real bear waits only for nightcap's p5 art (react-brain p4: hidden, never a pick)
    for (pack, waits_some) in [(real.clone(), false), (dir.clone(), true)] {
        let out = bin().args(["lint", "--pack"]).arg(&pack).arg("--json").output().unwrap();
        assert_eq!(out.status.code(), Some(0), "{}", String::from_utf8_lossy(&out.stdout));
        let j: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
        let waits: Vec<&serde_json::Value> = j["issues"].as_array().unwrap().iter().filter(|i| i["code"] == "waits_for_art").collect();
        assert!(waits.iter().all(|i| i["severity"] == "warn" && i["name"].is_string() && i["clip"].is_string()), "{waits:?}");
        let p = mascot_core::pack::load(&pack).unwrap();
        let want: Vec<(String, String)> = p.waiting().into_iter().map(|w| (w.name, w.clip)).collect();
        let got: Vec<(String, String)> = waits.iter().map(|i| (i["name"].as_str().unwrap().to_string(), i["clip"].as_str().unwrap().to_string())).collect();
        assert_eq!(got, want, "{}", pack.display());
        let others = got.iter().filter(|(n, _)| n != "nightcap").count();
        assert_eq!(others > 0, waits_some, "{}: {got:?}", pack.display());
        assert!(got.iter().any(|(n, _)| n == "nightcap"), "nightcap waits for its art: {got:?}");
        // a waiting entry is one whose art is missing, never one that has it; lean (set_lean, p1) never waits
        for (name, clip) in &got {
            assert!(!p.clips.contains_key(clip), "{name} waits for {clip}, which is loaded");
            assert!(name != "lean");
        }
        if waits_some {
            assert!(got.iter().any(|(n, c)| n == "weight_shift" && c == "fid_weight_shift"), "{got:?}");
        }
        assert_eq!(j["errors"], 0);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn lint_v2_codes() {
    let mut p = mascot_core::pack::load(&fixture("variety_pack")).unwrap();
    let r = lint::lint(&p);
    assert_eq!(r.errors, 0, "front-only and back-only behaviors turn first: {:#?}", r.issues);
    // v2 keys under pack_version 1
    p.pack_version = 1;
    let r = lint::lint(&p);
    let v: Vec<&str> = r.issues.iter().filter(|i| i.code == "v2_key_in_v1_pack").map(|i| i.detail.as_str()).collect();
    assert!(v.iter().any(|d| d.starts_with("variety ")) && v.iter().any(|d| d.starts_with("idle_pool[0].kind ")), "{v:?}");
    assert!(r.errors >= v.len());
    p.pack_version = 2;
    // a canvas above 176x160
    p.clips.get_mut("fid_front").unwrap().frame_w = 177;
    let r = lint::lint(&p);
    assert!(r.issues.iter().any(|i| i.code == "canvas_too_big" && i.file == "fid_front" && i.severity == "error"), "{:#?}", r.issues);
    p.clips.get_mut("fid_front").unwrap().frame_w = 84;
    // one behavior drawn in two dir sets
    p.behaviors.get_mut("peek").unwrap().seq.push(mascot_protocol::pack::StepDef { clip: "fid_front".into(), loops: Default::default() });
    let r = lint::lint(&p);
    let m: Vec<&str> = r.issues.iter().filter(|i| i.code == "dir_set_mismatch").map(|i| i.detail.as_str()).collect();
    assert_eq!(m, ["behavior peek: fid_front has S SE E W SW but fid_back has NE N NW"]);
}

/// AC17: the happy fixture keeps the variety rule on 3 seeds (exit 0); with 3 entries the rules
/// cannot hold: relaxed picks and violations, exit 1 (non-happy).
#[test]
fn audit_v2_passes_the_variety_pack_and_fails_the_three_entry_one() {
    let ok = bin().args(["audit", "--pack"]).arg(fixture("variety_pack")).args(["--minutes", "60", "--seeds", "1,2,3", "--json"]).output().unwrap();
    assert_eq!(ok.status.code(), Some(0), "{}", String::from_utf8_lossy(&ok.stdout));
    let j: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert_eq!(j["pass"], true);
    for run in j["runs"].as_array().unwrap() {
        assert_eq!((run["relaxed"].clone(), run["violations"].clone()), (serde_json::json!(0), serde_json::json!([])), "{run}");
        assert!(run["picks"].as_u64().unwrap() > 600, "every pick of the hour, not the 256-line trace tail: {}", run["picks"]);
        let t = &run["time"];
        let sum = t["stand"].as_f64().unwrap() + t["walk_run"].as_f64().unwrap() + t["turn"].as_f64().unwrap() + t["other"].as_f64().unwrap();
        assert!((sum - 1.0).abs() < 0.01, "{t}");
        assert!(run["kinds"]["fidget"].as_u64().unwrap() > 0 && run["kinds"]["move"].as_u64().unwrap() > 0);
        let long = run["long"].as_array().unwrap();
        assert!(long.iter().all(|l| l["name"] == "sit_long" && (20.0..=45.0).contains(&l["len_s"].as_f64().unwrap())), "{long:?}");
    }
    let chart = tmp("audit-v2").join("c.png");
    let bad = bin().args(["audit", "--pack"]).arg(fixture("variety_pack_3")).args(["--minutes", "10", "--seed", "4", "--json", "--chart"]).arg(&chart).output().unwrap();
    assert_eq!(bad.status.code(), Some(1));
    let j: serde_json::Value = serde_json::from_slice(&bad.stdout).unwrap();
    let run = &j["runs"][0];
    assert_eq!(j["pass"], false);
    assert!(run["relaxed"].as_u64().unwrap() > 0);
    let rules: BTreeSet<&str> = run["violations"].as_array().unwrap().iter().map(|v| v["rule"].as_str().unwrap()).collect();
    assert!(rules.contains("relaxed") && rules.contains("max_in_window"), "{rules:?}");
    assert!(!rules.contains("repeat"), "no_repeat is never relaxed");
    assert!(chart.is_file());
}
