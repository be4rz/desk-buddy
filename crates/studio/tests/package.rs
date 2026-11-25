//! The package standard (AC1) and the studio's package writes (AC2–AC5, headless).

use std::path::{Path, PathBuf};
use std::process::Command;

use mascot_core::{EyeState, MouthState};
use studio::package::{self, Op};
use studio::session::Session;
use studio::RenderOpts;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("studio-pkg-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn blob_bytes() -> Vec<u8> {
    std::fs::read(root().join("crates/studio/tests/fixtures/blob.aseprite")).unwrap()
}

fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_buddy-studio"))
}

/// sha256 of every file under `d`, by relative path.
fn hashes(d: &Path) -> Vec<(String, String)> {
    let mut out = vec![];
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap().flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push((path.strip_prefix(d).unwrap().display().to_string(), studio::sha256_hex(&std::fs::read(&path).unwrap())));
            }
        }
    }
    out.sort();
    out
}

/// Every clip, every view, first + middle + last frame, and one face variant: the same bytes.
fn same_pixels(a: &Session, b: &Session) {
    assert_eq!(a.pack.clips.keys().collect::<Vec<_>>(), b.pack.clips.keys().collect::<Vec<_>>());
    let mut n = 0;
    for (name, c) in &a.pack.clips {
        let views: Vec<String> = if c.set.is_some() { c.oris.keys().map(|o| format!("{o:?}")).collect() } else { c.dirs.keys().map(|d| d.as_str().to_string()).collect() };
        let last = c.durs_ms.len().saturating_sub(1);
        for v in views {
            for (frame, eyes, mouth) in [(0, EyeState::Open, MouthState::Closed), (last / 2, EyeState::Closed, MouthState::Open), (last, EyeState::Open, MouthState::Closed)] {
                let o = RenderOpts { clip: name.clone(), view: v.clone(), frame, zoom: 1, eyes, mouth };
                let (x, y) = (studio::render(&a.pack, &a.art, &o), studio::render(&b.pack, &b.art, &o));
                match (x, y) {
                    (Ok(x), Ok(y)) => {
                        assert!(x == y, "{name} {v} #{frame} differs");
                        n += 1;
                    }
                    (Err(x), Err(y)) => assert_eq!(x, y),
                    (x, y) => panic!("{name} {v} #{frame}: {x:?} vs {y:?}"),
                }
            }
        }
    }
    assert!(n > 1000, "rendered {n}");
    eprintln!("same pixels: {n} renders");
}

#[test]
fn the_bear_is_not_a_package_and_its_bundle_is_one_with_the_same_pixels() {
    let bear = root().join("mascots/bear");
    let before = hashes(&bear);
    let f = package::check(&bear);
    let all = f.iter().map(|f| format!("{} {}", f.path, f.problem)).collect::<Vec<_>>().join("\n");
    assert!(all.contains("art_root"), "{all}");
    assert!(all.contains("read from outside the package"), "{all}");

    let d = tmp("bundle-bear");
    let out = d.join("bear");
    let r = package::bundle(&bear, &out, None, None).unwrap();
    assert_eq!(r.id, "bear");
    assert!(r.files > 500 && r.bytes > 10_000_000, "{r:?}");
    assert_eq!(package::check(&out), vec![], "the bundle is a standard package");
    let text = std::fs::read_to_string(out.join("pack.json")).unwrap();
    assert!(text.contains("\"art_root\": \"art\""));
    assert!(text.contains("\"palettes\": [\"palettes/palette-bear.json\", \"palettes/palette.json\", \"palettes/palette-ext.json\", \"palettes/palette-ext-out.json\"]"), "{}", &text[..600]);
    // hand formatting survives: the behaviors block is byte-identical
    let src = std::fs::read_to_string(bear.join("pack.json")).unwrap();
    let block = |t: &str| t[t.find("\"behaviors\"").unwrap()..].to_string();
    assert_eq!(block(&text), block(&src));
    assert!(!out.join("review").exists(), "comments are not part of the mascot");
    same_pixels(&Session::open(&bear).unwrap(), &Session::open(&out).unwrap());
    // bundling a package again gives the same bytes
    let again = d.join("again").join("bear");
    package::bundle(&out, &again, None, None).unwrap();
    assert_eq!(hashes(&out), hashes(&again));
    // a copy under a new id
    let copy = d.join("polar-two");
    package::bundle(&out, &copy, Some("polar-two"), Some("Polar two")).unwrap();
    let pf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(copy.join("pack.json")).unwrap()).unwrap();
    assert_eq!((pf["id"].as_str(), pf["name"].as_str()), (Some("polar-two"), Some("Polar two")));
    assert_eq!(package::check(&copy), vec![]);
    // refusals: the folder must be named after the id and must not exist
    assert!(package::bundle(&bear, &d.join("wrong"), None, None).unwrap_err().0.contains("named after its id"));
    assert!(package::bundle(&bear, &out, None, None).unwrap_err().0.contains("already exists"));
    assert!(package::bundle(&bear, &d.join("Bad Id"), Some("Bad Id"), None).is_err());
    assert_eq!(hashes(&bear), before, "the source is never written");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn check_names_each_problem() {
    let d = tmp("check");
    let p = package::create_from_aseprite(&d, "blobby", "Blobby", &blob_bytes(), "blob.aseprite").unwrap();
    assert_eq!(package::check(&p), vec![]);
    // stray file, a link out, an off-standard art_root, a renamed folder
    std::fs::write(p.join("notes.txt"), "x").unwrap();
    std::os::unix::fs::symlink("/etc", p.join("art/out")).unwrap();
    let f = package::check(&p);
    assert!(f.iter().any(|f| f.path == "notes.txt" && f.problem.contains("isn't part")), "{f:?}");
    assert!(f.iter().any(|f| f.path == "art/out" && f.problem.contains("outside")), "{f:?}");
    std::fs::remove_file(p.join("notes.txt")).unwrap();
    std::fs::remove_file(p.join("art/out")).unwrap();
    let moved = d.join("other");
    std::fs::rename(&p, &moved).unwrap();
    assert!(package::check(&moved).iter().any(|f| f.problem.contains("folder's name")));
    std::fs::rename(&moved, &p).unwrap();
    std::fs::remove_file(p.join("art/blobby/blobby_walk_S.png")).unwrap();
    assert!(package::check(&p).iter().any(|f| f.problem.contains("doesn't")), "{:?}", package::check(&p));
    assert!(package::check(&d.join("none")).iter().any(|f| f.problem == "not a folder"));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn create_refuses_bad_input_and_writes_nothing() {
    let d = tmp("create");
    assert!(package::create_from_aseprite(&d, "Bad", "x", &blob_bytes(), "b").is_err());
    assert!(package::create_from_aseprite(&d, "ok", " ", &blob_bytes(), "b").is_err());
    assert!(package::create_from_aseprite(&d, "ok", "Ok", b"not an aseprite file", "junk.aseprite").unwrap_err().0.contains("nothing written"));
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0, "no staging left behind");
    let p = package::create_from_aseprite(&d, "9-lives", "Nine", &blob_bytes(), "b").unwrap();
    assert_eq!(package::check(&p), vec![]);
    assert!(package::create_from_aseprite(&d, "9-lives", "Nine", &blob_bytes(), "b").unwrap_err().0.contains("already exists"));
    let _ = std::fs::remove_dir_all(&d);
}

fn op(json: &str) -> Vec<Op> {
    serde_json::from_str(json).unwrap()
}

#[test]
fn edits_are_checked_and_a_bad_one_writes_nothing() {
    let d = tmp("edit");
    let p = package::create_from_aseprite(&d, "blobby", "Blobby", &blob_bytes(), "blob.aseprite").unwrap();
    let pj = p.join("pack.json");
    let text = std::fs::read_to_string(&pj).unwrap();
    // details + temperament
    let t = package::apply(&text, &op(r#"[{"op":"set","path":["description"],"json":"\"A small blob\""},{"op":"set","path":["temperament"],"json":"{\"energy\": 0.9, \"curiosity\": 0.5, \"shyness\": 0.5, \"chattiness\": 0.5, \"sleepiness\": 0.1}"}]"#)).unwrap();
    let pack = package::save(&p, &t).unwrap();
    assert_eq!(pack.temperament.energy, 0.9);
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), t);
    // refused: each leaves pack.json as it was
    let refused = [
        (r#"[{"op":"set","path":["id"],"json":"\"x\""}]"#, "id can't change"),
        (r#"[{"op":"remove","path":["behaviors","rest"]}]"#, "stop the mascot loading"),
        (r#"[{"op":"set","path":["behaviors","bounce"],"json":"{\"seq\": [{\"clip\": \"nope\"}]}"}]"#, "clip “nope”"),
        (r#"[{"op":"set","path":["art_root"],"json":"\"..\""}]"#, "art_root"),
        (r#"[{"op":"set","path":["palettes"],"json":"[\"../x.json\"]"}]"#, "palettes[0]"),
        (r#"[{"op":"set","path":["temperament","energy"],"json":"\"high\""}]"#, "pack.json"),
    ];
    for (ops, why) in refused {
        let t2 = package::apply(&t, &op(ops)).unwrap();
        let e = package::save(&p, &t2).unwrap_err().0;
        assert!(e.contains(why), "{ops}: {e}");
        assert_eq!(std::fs::read_to_string(&pj).unwrap(), t, "{ops} wrote nothing");
    }
    assert!(package::save(&p, "{ not json").is_err());
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), t);
    // a clip group: add, it's unused, remove it; a used one can't go
    studio::aseprite::import_bytes(&blob_bytes(), "blob.aseprite", &p, "extra").unwrap();
    assert_eq!(package::check(&p), vec![]);
    let pack = mascot_core::pack::load_report(&p).unwrap();
    let g = package::groups(&p, &pack).unwrap();
    let names: Vec<_> = g.iter().map(|g| (g.name.as_str(), g.used_by.is_empty())).collect();
    assert_eq!(names, vec![("blobby", false), ("extra", true)]);
    assert!(package::remove_group(&p, "blobby").unwrap_err().0.contains("behavior rest"));
    assert!(package::remove_group(&p, "../x").is_err());
    package::remove_group(&p, "extra").unwrap();
    assert!(!p.join("art/extra").exists());
    assert_eq!(package::check(&p), vec![]);
    // delete
    package::delete(&p).unwrap();
    assert!(!p.exists());
    assert!(package::delete(&d.join("nothing")).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bundle_carries_the_source_body_problems_and_a_refused_group_removal_keeps_the_folder() {
    let d = tmp("carry");
    let p = package::create_from_aseprite(&d, "blobby", "Blobby", &blob_bytes(), "blob.aseprite").unwrap();
    // a pack with a body problem (written by hand, past the studio's checks) still bundles
    let pj = p.join("pack.json");
    let mut v: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&pj).unwrap()).unwrap();
    v["body"] = serde_json::json!({ "max_for_s": 7200, "default_for_s": 60 });
    std::fs::write(&pj, serde_json::to_string_pretty(&v).unwrap()).unwrap();
    let issues = mascot_core::pack::load_report(&p).unwrap().body_issues;
    assert!(!issues.is_empty(), "the fixture has a body problem");
    let out = d.join("lib").join("blob-copy");
    package::bundle(&p, &out, Some("blob-copy"), None).unwrap();
    assert_eq!(mascot_core::pack::load_report(&out).unwrap().body_issues, issues, "carried, not added");
    // the group in use can't go, and its folder stays whole
    let before = hashes(&out.join("art/blobby"));
    assert!(package::remove_group(&out, "blobby").is_err());
    assert_eq!(hashes(&out.join("art/blobby")), before);
    assert!(!std::fs::read_dir(out.join("art")).unwrap().flatten().any(|e| e.file_name().to_string_lossy().starts_with('.')), "nothing left aside");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cli_package_check_and_bundle() {
    let d = tmp("cli");
    let o = bin().args(["package", "check", root().join("mascots/bear").to_str().unwrap(), "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "the linked dev bear isn't a package");
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(v["findings"].as_array().unwrap().iter().any(|f| f["problem"].as_str().unwrap().contains("art_root")), "{v}");
    let out = d.join("bear");
    let o = bin().args(["package", "bundle", "--pack", root().join("mascots/bear").to_str().unwrap(), "--out", out.to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    let o = bin().args(["package", "check", out.to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    let o = bin().args(["package", "bundle", "--pack", root().join("mascots/bear").to_str().unwrap(), "--out", out.to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "the target exists");
    let o = bin().args(["package", "frob"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2));
    let o = bin().args(["package", "bundle", "--pack", "x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "--out is required");
    let _ = std::fs::remove_dir_all(&d);
}

// ---------------------------------------------------------------------------------------------
// the persona block
// ---------------------------------------------------------------------------------------------

/// The page's starter personality ("+ Add a personality"), with a gesture on one emote.
fn persona_json(gesture: &str) -> String {
    serde_json::json!({
        "default": "blobby",
        "presets": { "blobby": {
            "name": "Blobby", "tagline": "friendly helper",
            "identity": "A small desk buddy who keeps you company while you work.",
            "voice": ["Short and friendly: a sentence or two.", "Plain words, no jargon.", "Honest: says so when something looks wrong."],
            "likes": [],
            "gestures": { "greeting": gesture },
            "examples": { "greeting": "Hi, {you}! I'm here if you need me.", "done": "Done. It works now.", "error": "That didn't work. Want me to look into it?", "checkin": "You've been at it a while. Maybe a short break?", "disagree": "I'd hold off on that: it could break something." },
            "idle_lines": { "morning": ["Morning!"], "evening": ["Long day. Nearly there."], "back": ["Welcome back."], "long_session": ["Still here."] },
            "temperament": { "energy": 0.5, "curiosity": 0.5, "shyness": 0.5, "chattiness": 0.5, "sleepiness": 0.5 },
            "style": { "playfulness": 0.5, "emotes": 0.5 }
        } }
    })
    .to_string()
}

/// A package whose body has two emotes (`wave`, `nod`), ready for a persona.
fn blob_with_emotes(d: &Path) -> (PathBuf, String) {
    let p = package::create_from_aseprite(d, "blobby", "Blobby", &blob_bytes(), "blob.aseprite").unwrap();
    let text = std::fs::read_to_string(p.join("pack.json")).unwrap();
    let body = r#"{"default_for_s": 60, "max_for_s": 600, "activities": {}, "emotes": {"wave": {"plays": ["rest"], "about": "a wave"}, "nod": {"plays": ["rest"], "about": "a nod"}}}"#;
    let t = package::apply(&text, &[Op { op: "set".into(), path: vec!["body".into()], json: Some(body.into()) }]).unwrap();
    package::save(&p, &t).unwrap();
    (p.clone(), t)
}

fn set(path: &[&str], json: &str) -> Vec<Op> {
    vec![Op { op: "set".into(), path: path.iter().map(|s| (*s).into()).collect(), json: Some(json.into()) }]
}

#[test]
fn a_save_that_adds_a_persona_problem_is_refused_and_writes_nothing() {
    let d = tmp("persona-edit");
    let (p, body_text) = blob_with_emotes(&d);
    let pj = p.join("pack.json");
    // the page's starter loads, a dry check passes, the save writes it
    let good = package::apply(&body_text, &set(&["persona"], &persona_json("wave"))).unwrap();
    assert_eq!(package::validate_edit(&p, &good).unwrap().persona.as_ref().unwrap().default, "blobby");
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), body_text, "a dry check writes nothing");
    let pack = package::save(&p, &good).unwrap();
    assert!(pack.persona_issues.is_empty(), "{:?}", pack.persona_issues);
    assert_eq!(package::check(&p), vec![]);

    // each of these adds a persona issue: refused with its text, the file as it was
    let refused: [(&[&str], String, &str); 7] = [
        (&["persona", "presets", "blobby", "gestures", "greeting"], "\"wvae\"".into(), "did you mean \"wave\""),
        (&["persona", "default"], "\"blobbi\"".into(), "persona.default: no preset \"blobbi\" (did you mean \"blobby\"?)"),
        (&["persona", "presets", "blobby", "voice"], serde_json::json!(["a", "b", "c", "d", "e", "f", "g"]).to_string(), "7 rules, it needs 3 to 6"),
        (&["persona", "presets", "blobby", "identity"], format!("\"{}\"", "x".repeat(121)), "121 characters, the limit is 120"),
        (&["persona", "presets", "blobby", "examples", "done"], "\"Done, {nope}.\"".into(), "unknown placeholder {nope}"),
        (&["persona", "presets", "Bad-Id"], serde_json::from_str::<serde_json::Value>(&persona_json("wave")).unwrap()["presets"]["blobby"].to_string(), "bad preset id"),
        (&["persona", "presets", "blobby", "style", "playfulness"], "1.5".into(), "not between 0 and 1"),
    ];
    for (path, json, why) in refused {
        let bad = package::apply(&good, &set(path, &json)).unwrap();
        let e = package::validate_edit(&p, &bad).unwrap_err().0;
        assert!(e.contains(why), "{path:?}: {e}");
        let e = package::save(&p, &bad).unwrap_err().0;
        assert!(e.contains(why), "{path:?}: {e}");
        assert_eq!(std::fs::read_to_string(&pj).unwrap(), good, "{path:?} wrote nothing");
    }
    // a missing example, and a block that does not parse (a stray field)
    let bad = package::apply(&good, &[Op { op: "remove".into(), path: vec!["persona".into(), "presets".into(), "blobby".into(), "examples".into(), "error".into()], json: None }]).unwrap();
    assert!(package::save(&p, &bad).unwrap_err().0.contains("examples.error: missing"));
    let bad = package::apply(&good, &set(&["persona", "presets", "blobby", "mood"], "\"x\"")).unwrap();
    let e = package::save(&p, &bad).unwrap_err().0;
    assert!(e.contains("mood"), "{e}");
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), good);
    // taking the personality away is fine
    let none = package::apply(&good, &[Op { op: "remove".into(), path: vec!["persona".into()], json: None }]).unwrap();
    assert!(package::save(&p, &none).unwrap().persona.is_none());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn persona_problems_already_on_disk_do_not_block_other_edits_but_new_ones_do() {
    let d = tmp("persona-disk");
    let (p, body_text) = blob_with_emotes(&d);
    let pj = p.join("pack.json");
    // written by hand, past the studio's checks: one gesture names no emote
    let broken = package::apply(&body_text, &set(&["persona"], &persona_json("waev"))).unwrap();
    std::fs::write(&pj, &broken).unwrap();
    let issues = mascot_core::pack::load_report(&p).unwrap().persona_issues;
    assert_eq!(issues.len(), 1, "{issues:?}");
    assert!(mascot_core::pack::load(&p).is_err(), "the app refuses it");
    // an unrelated edit still saves, the old issue stays
    let t = package::apply(&broken, &set(&["description"], "\"hi\"")).unwrap();
    assert_eq!(package::save(&p, &t).unwrap().persona_issues, issues);
    // a new one does not
    let worse = package::apply(&t, &set(&["persona", "presets", "blobby", "voice"], "[\"a\", \"b\"]")).unwrap();
    assert!(package::save(&p, &worse).unwrap_err().0.contains("2 rules, it needs 3 to 6"));
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), t);
    // fixing it is allowed
    let fixed = package::apply(&t, &set(&["persona", "presets", "blobby", "gestures", "greeting"], "\"nod\"")).unwrap();
    assert!(package::save(&p, &fixed).unwrap().persona_issues.is_empty());
    // package check lists it, and a bundle refuses to carry it
    std::fs::write(&pj, &t).unwrap();
    let f = package::check(&p);
    assert!(f.iter().any(|f| f.path == "pack.json" && f.problem.contains("persona.presets.blobby.gestures.greeting") && f.problem.contains("did you mean \"wave\"")), "{f:?}");
    let e = package::bundle(&p, &d.join("lib/blob-copy"), Some("blob-copy"), None).unwrap_err().0;
    assert!(e.contains("fix the personality before bundling"), "{e}");
    assert!(!d.join("lib/blob-copy").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cli_lint_and_package_check_report_persona_problems() {
    let d = tmp("persona-cli");
    let (p, body_text) = blob_with_emotes(&d);
    let ok = bin().args(["lint", "--pack", p.to_str().unwrap()]).output().unwrap();
    assert_eq!(ok.status.code(), Some(0), "{}", String::from_utf8_lossy(&ok.stdout));
    std::fs::write(p.join("pack.json"), package::apply(&body_text, &set(&["persona"], &persona_json("waev"))).unwrap()).unwrap();
    let o = bin().args(["lint", "--pack", p.to_str().unwrap(), "--json"]).output().unwrap();
    assert_eq!(o.status.code(), Some(1), "{}", String::from_utf8_lossy(&o.stdout));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    let i = v["issues"].as_array().unwrap().iter().find(|i| i["code"] == "bad_persona_gesture").unwrap_or_else(|| panic!("{v}"));
    assert_eq!((i["severity"].as_str(), i["file"].as_str(), i["name"].as_str()), (Some("error"), Some("pack.json"), Some("blobby")));
    assert!(i["detail"].as_str().unwrap().contains("did you mean \"wave\""), "{i}");
    let o = bin().args(["package", "check", p.to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stdout).contains("persona.presets.blobby.gestures.greeting"), "{}", String::from_utf8_lossy(&o.stdout));
    let o = bin().args(["package", "bundle", "--pack", p.to_str().unwrap(), "--out", d.join("lib/blobby").to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("personality"), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn cli_persona_render_prints_the_agents_section_and_audit_takes_a_preset() {
    let bear = root().join("mascots/bear");
    let pack = mascot_core::pack::load(&bear).unwrap();
    let user = r#"{"call_me":"Sam","chattiness":0.9,"notes":"Call me captain."}"#;
    for preset in ["biscuit", "mochi", "pip", "tofu"] {
        let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap(), "--preset", preset, "--user", user, "--json"]).output().unwrap();
        assert_eq!(o.status.code(), Some(0), "{preset}: {}", String::from_utf8_lossy(&o.stderr));
        let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
        let mut u: mascot_core::persona::UserPersona = serde_json::from_str(user).unwrap();
        u.preset = Some(preset.into());
        let want = mascot_core::persona::prompt_text(&pack, &u).unwrap();
        assert_eq!(v["text"].as_str(), Some(want.as_str()), "{preset}");
        assert_eq!(v["preset"], preset);
        assert!(want.starts_with("## Who you are"), "{want}");
        // the plain output is the same text
        let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap(), "--preset", preset, "--user", user]).output().unwrap();
        assert_eq!(String::from_utf8(o.stdout).unwrap(), format!("{want}\n"));
    }
    // no --preset: the pack's default; --user may be a file
    let d = tmp("persona-user");
    std::fs::write(d.join("u.json"), r#"{"preset":"pip"}"#).unwrap();
    let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap(), "--user", d.join("u.json").to_str().unwrap(), "--json"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()["preset"], "pip");
    let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap(), "--json"]).output().unwrap();
    assert_eq!(serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()["preset"], "biscuit");
    // refusals: an unknown preset and a bad --user are usage errors, a pack without a persona is an error
    for args in [vec!["--preset", "nope"], vec!["--user", "{not json"], vec!["--user", r#"{"chattiness":"loud"}"#]] {
        let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap()]).args(&args).output().unwrap();
        assert_eq!(o.status.code(), Some(2), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    }
    let o = bin().args(["persona", "render", "--pack", bear.to_str().unwrap(), "--preset", "nope"]).output().unwrap();
    assert!(String::from_utf8_lossy(&o.stderr).contains("have: biscuit, mochi, pip, tofu"), "{}", String::from_utf8_lossy(&o.stderr));
    assert_eq!(bin().args(["persona", "render"]).output().unwrap().status.code(), Some(2));
    assert_eq!(bin().args(["persona", "frob", "--pack", "x"]).output().unwrap().status.code(), Some(2));
    let blob = package::create_from_aseprite(&d, "blobby", "Blobby", &blob_bytes(), "blob.aseprite").unwrap();
    let o = bin().args(["persona", "render", "--pack", blob.to_str().unwrap()]).output().unwrap();
    assert_eq!(o.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&o.stderr).contains("no persona"), "{}", String::from_utf8_lossy(&o.stderr));
    // audit --preset: the preset's temperament over the pack's, --temperament over that
    let a = |args: &[&str]| bin().args(["audit", "--pack", bear.to_str().unwrap(), "--minutes", "1", "--seed", "1", "--json"]).args(args).output().unwrap();
    let o = a(&["--preset", "tofu"]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    let v: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!((v["preset"].as_str(), v["temperament"]["shyness"].as_f64(), v["temperament"]["energy"].as_f64()), (Some("tofu"), Some(0.7), Some(0.4)));
    let v: serde_json::Value = serde_json::from_slice(&a(&["--preset", "tofu", "--temperament", "shyness=0.2"]).stdout).unwrap();
    assert_eq!((v["temperament"]["shyness"].as_f64(), v["temperament"]["energy"].as_f64()), (Some(0.2), Some(0.4)));
    let o = a(&["--preset", "nope"]);
    assert_eq!(o.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&o.stderr).contains("--preset nope: no such preset in this pack (have: biscuit, mochi, pip, tofu)"), "{}", String::from_utf8_lossy(&o.stderr));
    let o = bin().args(["audit", "--pack", blob.to_str().unwrap(), "--minutes", "1", "--preset", "x"]).output().unwrap();
    assert_eq!(o.status.code(), Some(2), "a pack with no persona has no presets");
    assert!(String::from_utf8_lossy(&o.stderr).contains("no persona"), "{}", String::from_utf8_lossy(&o.stderr));
    let _ = std::fs::remove_dir_all(&d);
}
