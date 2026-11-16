use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use super::Desk;

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("buddy-studio-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A small real pack: the blob fixture (Blender frames) imported into `dir/<name>`.
fn blob_pack(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(name);
    let fx = Path::new(env!("CARGO_MANIFEST_DIR")).join("../studio/tests/fixtures/blob.aseprite");
    ::studio::aseprite::import(&fx, &p, "blob").unwrap();
    p
}

fn bump_mtime(f: &Path) {
    // a later mtime even on a coarse clock
    let t = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options().write(true).open(f).unwrap().set_modified(t).unwrap();
}

#[test]
fn hot_reload_keeps_the_last_good_pack_and_reports_the_error() {
    let d = tmp("reload");
    let p = blob_pack(&d, "p");
    let mut desk = Desk::new(Some(p.clone()), vec![p.clone()]);
    let s = desk.handle("studio_state", &Value::Null).unwrap();
    assert_eq!(s["pack"]["id"], "blob");
    assert_eq!(s["pack"]["clips"][0]["name"], "blob_walk");
    assert_eq!(desk.handle("studio_poll", &Value::Null).unwrap()["changed"], false);
    // break pack.json: the poll fails, the pack stays, the error is shown
    let pj = p.join("pack.json");
    let good = std::fs::read_to_string(&pj).unwrap();
    std::fs::write(&pj, good.replace("\"blob_walk\"", "\"nope_walk\"")).unwrap();
    bump_mtime(&pj);
    let r = desk.handle("studio_poll", &Value::Null).unwrap();
    assert_eq!((r["changed"].clone(), r["reloaded"].clone()), (json!(true), json!(false)));
    assert!(r["error"].as_str().unwrap().contains("pack.json"), "{r}");
    let s = desk.handle("studio_state", &Value::Null).unwrap();
    assert_eq!(s["pack"]["clips"][0]["name"], "blob_walk", "the last good pack");
    assert!(s["error"].is_string());
    // frames still come from the last good pack
    let f = desk.handle("studio_frames", &json!({ "clip": "blob_walk", "dir": "S" })).unwrap();
    assert_eq!(f["frames"].as_array().unwrap().len(), 6);
    assert!(f["frames"][0].as_str().unwrap().starts_with("data:image/png;base64,iVBOR"));
    // a strip edit is seen too, once fixed
    std::fs::write(&pj, &good).unwrap();
    bump_mtime(&pj);
    let r = desk.handle("studio_poll", &Value::Null).unwrap();
    assert_eq!(r["reloaded"], true);
    assert!(r["error"].is_null());
    assert_eq!(r["loads"], 2);
    let strip = p.join("art/blob/blob_walk_S.png");
    std::fs::write(&strip, std::fs::read(&strip).unwrap()).unwrap();
    bump_mtime(&strip);
    assert_eq!(desk.handle("studio_poll", &Value::Null).unwrap()["reloaded"], true);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_strip_added_after_a_failed_reload_is_picked_up() {
    let d = tmp("newstrip");
    let p = blob_pack(&d, "p");
    let mut desk = Desk::new(Some(p.clone()), vec![p.clone()]);
    desk.handle("studio_state", &Value::Null).unwrap();
    // anim.json now names a strip that isn't there yet: the reload fails
    let aj = p.join("art/blob/anim.json");
    let txt = std::fs::read_to_string(&aj).unwrap();
    std::fs::write(&aj, txt.replace("\"blob_walk_S.png\"", "\"blob_walk_S2.png\"")).unwrap();
    bump_mtime(&aj);
    let r = desk.handle("studio_poll", &Value::Null).unwrap();
    assert_eq!((r["changed"].clone(), r["reloaded"].clone()), (json!(true), json!(false)), "{r}");
    assert_eq!(desk.handle("studio_poll", &Value::Null).unwrap()["changed"], false);
    // the artist exports it: the next poll loads
    std::fs::copy(p.join("art/blob/blob_walk_S.png"), p.join("art/blob/blob_walk_S2.png")).unwrap();
    let t = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::open(p.join("art/blob")).unwrap().set_modified(t).unwrap();
    let r = desk.handle("studio_poll", &Value::Null).unwrap();
    assert_eq!(r["reloaded"], true, "{r}");
    assert!(r["error"].is_null());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn comments_round_trip_with_the_frozen_schema() {
    let d = tmp("comments");
    let p = blob_pack(&d, "p");
    let mut desk = Desk::new(Some(p.clone()), vec![p.clone()]);
    let c = desk.handle("studio_comment_add", &json!({ "clip": "blob_walk", "dir": "SE", "frame": 3, "t_ms": 249, "region": [10, 20, 8, 6], "text": "  ear tip loses its ink  " })).unwrap();
    let c2 = desk.handle("studio_comment_add", &json!({ "clip": "blob_walk", "dir": "S", "frame": 0, "t_ms": 0, "text": "ok" })).unwrap();
    let file = p.join("review/comments.jsonl");
    let lines: Vec<Value> = std::fs::read_to_string(&file).unwrap().lines().map(|l| serde_json::from_str(l).unwrap()).collect();
    assert_eq!(lines.len(), 2);
    let keys: Vec<&str> = lines[0].as_object().unwrap().keys().map(String::as_str).collect();
    let mut want = vec!["v", "id", "clip", "dir", "frame", "t_ms", "region", "text", "author", "at", "status"];
    want.sort();
    let mut keys = keys;
    keys.sort();
    assert_eq!(keys, want);
    assert_eq!(lines[0]["v"], 1);
    assert_eq!(lines[0]["text"], "ear tip loses its ink");
    assert_eq!(lines[0]["region"], json!([10, 20, 8, 6]));
    assert_eq!(lines[1]["region"], Value::Null);
    assert_eq!((lines[0]["author"].as_str(), lines[0]["status"].as_str()), (Some("user"), Some("open")));
    let id = c["id"].as_str().unwrap();
    assert!(id.starts_with("c_") && id.len() == 10 && id != c2["id"].as_str().unwrap());
    let at = lines[0]["at"].as_str().unwrap();
    assert!(at.len() == 20 && at.ends_with('Z') && at.as_bytes()[10] == b'T', "{at}");
    // resolve, then read back
    desk.handle("studio_comment_status", &json!({ "id": id, "status": "resolved" })).unwrap();
    let all = desk.handle("studio_comments", &Value::Null).unwrap();
    assert_eq!(all["comments"][0]["status"], "resolved");
    assert_eq!(all["comments"][1]["status"], "open");
    // the headless list reads the same file
    let read = ::studio::comments::read(&p).unwrap();
    assert_eq!(serde_json::to_value(&read).unwrap(), all["comments"]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn bad_input_from_the_page_is_refused() {
    let d = tmp("refuse");
    let p = blob_pack(&d, "p");
    let mut desk = Desk::new(Some(p.clone()), vec![p.clone()]);
    let ok = json!({ "clip": "blob_walk", "dir": "S", "frame": 0, "t_ms": 0, "text": "x" });
    let with = |k: &str, v: Value| {
        let mut o = ok.clone();
        o[k] = v;
        o
    };
    for (what, args) in [
        ("a path", with("path", json!("../../etc/x.jsonl"))),
        ("a file", with("file", json!("/tmp/x"))),
        ("no clip", with("clip", json!("../../x"))),
        ("no view", with("dir", json!("Q"))),
        ("a frame past the end", with("frame", json!(6))),
        ("a region outside", with("region", json!([50, 70, 10, 10]))),
        ("no text", with("text", json!("   "))),
        ("too long", with("text", json!("x".repeat(2001)))),
    ] {
        assert!(desk.handle("studio_comment_add", &args).is_err(), "{what}");
    }
    assert!(!p.join("review").exists(), "nothing written");
    for (id, st) in [("../../x", "resolved"), ("c_zzzzzzzz", "resolved"), ("c_00000000", "resolved"), ("c_00000000", "deleted")] {
        assert!(desk.handle("studio_comment_status", &json!({ "id": id, "status": st })).is_err(), "{id} {st}");
    }
    // packs are opened by id from the allowed list only
    for id in ["../p", "/etc", "bear", ""] {
        assert!(desk.handle("studio_open", &json!({ "id": id })).is_err(), "{id}");
    }
    assert!(desk.handle("studio_frames", &json!({ "clip": "blob_walk", "dir": "../S" })).is_err());
    assert!(desk.handle("studio_sim", &json!({ "op": "event", "event": { "type": "rm_rf" } })).is_err());
    assert!(desk.handle("studio_audit", &json!({ "minutes": 100000 })).is_err());
    assert!(desk.handle("studio_nope", &Value::Null).is_err());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn empty_state_then_open_by_id() {
    let d = tmp("empty");
    let a = blob_pack(&d, "a");
    let mut desk = Desk::new(None, super::allowed_packs(None, &[d.clone()]));
    let s = desk.handle("studio_state", &Value::Null).unwrap();
    assert!(s["pack"].is_null());
    assert_eq!(s["packs"], json!(["blob"]));
    assert_eq!(desk.handle("studio_lint", &Value::Null).unwrap_err(), "no pack open");
    let s = desk.handle("studio_open", &json!({ "id": "blob" })).unwrap();
    assert_eq!(s["pack"]["dir"], a.display().to_string());
    // closing drops the session; the next call opens the same pack again
    desk.close();
    assert_eq!(desk.handle("studio_state", &Value::Null).unwrap()["pack"]["id"], "blob");
    // a start pack that doesn't load: the empty state with the reason
    let mut bad = Desk::new(Some(d.join("missing")), vec![]);
    let s = bad.handle("studio_state", &Value::Null).unwrap();
    assert!(s["pack"].is_null() && s["error"].as_str().unwrap().contains("no pack.json"), "{s}");
    // a start dir named like a real pack but without a pack.json (a typo) doesn't hide it
    let typo = d.join("x").join("blob");
    std::fs::create_dir_all(&typo).unwrap();
    let mut t = Desk::new(Some(typo.clone()), super::allowed_packs(Some(&typo), &[d.clone()]));
    assert_eq!(t.handle("studio_open", &json!({ "id": "blob" })).unwrap()["pack"]["dir"], a.display().to_string());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_simulator_takes_frozen_event_shapes_and_never_the_live_agent() {
    let bear = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear");
    let mut desk = Desk::new(Some(bear.clone()), vec![bear]);
    let r = desk.handle("studio_sim", &json!({ "op": "tick", "dt_ms": 1000 })).unwrap();
    assert_eq!(r["t_ms"], 1000);
    let mut r = desk.handle("studio_sim", &json!({ "op": "event", "event": { "type": "thinking" } })).unwrap();
    // the greeting finishes, then it thinks
    for _ in 0..20 {
        if r["show"]["intent"] == "think" {
            break;
        }
        r = desk.handle("studio_sim", &json!({ "op": "tick", "dt_ms": 500 })).unwrap();
    }
    assert_eq!(r["show"]["intent"], "think", "{r}");
    desk.handle("studio_sim", &json!({ "op": "temperament", "temperament": { "energy": 0.9 } })).unwrap();
    desk.handle("studio_sim", &json!({ "op": "senses", "patch": { "dnd_toggle": true } })).unwrap();
    let tl = json!({ "timeline_version": 1, "seed": 5, "duration_ms": 10000, "start_local": "10:00", "weekday": 0, "items": [{ "at_ms": 2000, "event": { "type": "say", "text": "hi", "emotion": "happy" } }] });
    let r = desk.handle("studio_sim", &json!({ "op": "timeline", "timeline": tl })).unwrap();
    assert_eq!(r["pending"], 1);
    let r = desk.handle("studio_sim", &json!({ "op": "tick", "dt_ms": 2500 })).unwrap();
    assert_eq!(r["pending"], 0);
    assert!(r["trace"].as_array().unwrap().iter().any(|l| l["what"] == "event"), "{r}");
    // a start time that isn't HH:MM is refused (it used to overflow, or be ignored)
    for bad in ["2000:00", "25:00", "10"] {
        let tl = json!({ "timeline_version": 1, "seed": 5, "duration_ms": 10000, "start_local": bad, "weekday": 0, "items": [] });
        let e = desk.handle("studio_sim", &json!({ "op": "timeline", "timeline": tl })).unwrap_err();
        assert!(e.contains("start_local"), "{e}");
    }
    assert!(desk.handle("studio_sim", &json!({ "op": "tick", "dt_ms": 0 })).is_ok(), "the simulator still runs");
    // no harness was ever started by any of that
    assert!(crate::harness::get().is_none());
}

/// sha256 of every file under `d`.
fn hashes(d: &Path) -> Vec<(PathBuf, String)> {
    let mut out = vec![];
    let mut stack = vec![d.to_path_buf()];
    while let Some(p) = stack.pop() {
        for e in std::fs::read_dir(&p).unwrap().flatten() {
            if e.path().is_dir() {
                stack.push(e.path());
            } else {
                out.push((e.path(), ::studio::sha256_hex(&std::fs::read(e.path()).unwrap())));
            }
        }
    }
    out.sort();
    out
}

type Calls = std::sync::Arc<std::sync::Mutex<Vec<String>>>;

/// A studio with a library `d/packs` and one built-in, `d/builtin/blob`.
fn library_desk(d: &Path) -> (Desk, PathBuf, PathBuf, Calls) {
    let builtin = blob_pack(&d.join("builtin"), "blob");
    let lib = d.join("packs");
    let calls: Calls = Default::default();
    let c = calls.clone();
    let desktop = super::Desktop { current: "blob".into(), set: Box::new(move |id| {
        c.lock().unwrap().push(id.to_string());
        Ok(())
    }) };
    let desk = Desk::with(None, super::Roots { library: Some(lib.clone()), builtin: vec![d.join("builtin")] }, desktop);
    (desk, lib, builtin, calls)
}

fn blob_b64() -> String {
    ::studio::base64(&std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../studio/tests/fixtures/blob.aseprite")).unwrap())
}

fn kinds(desk: &mut Desk) -> Vec<(String, String)> {
    let l = desk.handle("studio_library", &Value::Null).unwrap();
    l["packs"].as_array().unwrap().iter().map(|p| (p["id"].as_str().unwrap().to_string(), p["kind"].as_str().unwrap().to_string())).collect()
}

#[test]
fn built_ins_are_customized_through_a_copy_and_restored_never_written() {
    let d = tmp("builtin");
    let (mut desk, lib, builtin, _) = library_desk(&d);
    let before = hashes(&builtin);
    assert_eq!(kinds(&mut desk), vec![("blob".into(), "builtin".into())]);
    let l = desk.handle("studio_library", &Value::Null).unwrap();
    let can = &l["packs"][0]["can"];
    assert_eq!((can["delete"].as_bool(), can["customize"].as_bool(), can["edit"].as_bool()), (Some(false), Some(true), Some(false)), "{l}");
    assert_eq!(l["packs"][0]["desktop"], true);
    // the backend refuses what no control offers
    assert!(desk.handle("studio_delete", &json!({ "id": "blob" })).unwrap_err().contains("can't be deleted"));
    assert!(desk.handle("studio_restore", &json!({ "id": "blob" })).is_err());
    let s = desk.handle("studio_open", &json!({ "id": "blob" })).unwrap();
    assert_eq!((s["pack"]["kind"].as_str(), s["pack"]["writable"].as_bool()), (Some("builtin"), Some(false)));
    let e = desk.handle("studio_edit", &Value::Null).unwrap();
    assert_eq!(e["writable"], false);
    let ops = json!([{ "op": "set", "path": ["name"], "json": "\"Mine\"" }]);
    assert!(desk.handle("studio_save", &json!({ "sha": e["sha"], "ops": ops })).unwrap_err().contains("built-in"));
    assert!(desk.handle("studio_import", &json!({ "name": "extra", "bytes": blob_b64() })).unwrap_err().contains("built-in"));
    assert!(desk.handle("studio_remove_group", &json!({ "group": "blob" })).is_err());
    // make my copy: same id, in the library, shadows the built-in
    let s = desk.handle("studio_customize", &json!({ "id": "blob" })).unwrap();
    assert_eq!(s["pack"]["kind"], "customized");
    assert_eq!(s["pack"]["dir"], lib.join("blob").display().to_string());
    assert!(desk.handle("studio_customize", &json!({ "id": "blob" })).is_err(), "already a copy");
    assert_eq!(kinds(&mut desk), vec![("blob".into(), "customized".into())]);
    let e = desk.handle("studio_edit", &Value::Null).unwrap();
    assert_eq!(e["findings"], json!([]), "the copy is a standard package");
    let e = desk.handle("studio_save", &json!({ "sha": e["sha"], "ops": ops })).unwrap();
    assert!(e["text"].as_str().unwrap().contains("\"name\": \"Mine\""));
    assert_eq!(desk.handle("studio_state", &Value::Null).unwrap()["pack"]["name"], "Mine");
    assert!(desk.handle("studio_delete", &json!({ "id": "blob" })).unwrap_err().contains("Restore original"));
    // restore: the copy goes, the shipped one is back as it was
    let s = desk.handle("studio_restore", &json!({ "id": "blob" })).unwrap();
    assert_eq!((s["pack"]["kind"].as_str(), s["pack"]["name"].as_str()), (Some("builtin"), Some("blob")));
    assert!(!lib.join("blob").exists());
    assert_eq!(hashes(&builtin), before, "the built-in's files never changed");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn create_edit_and_delete_a_user_mascot() {
    let d = tmp("library");
    let (mut desk, lib, _, calls) = library_desk(&d);
    let s = desk.handle("studio_create", &json!({ "from": "aseprite", "name": "Wobble Bot", "file_name": "/Users/x/wobble.aseprite", "bytes": blob_b64() })).unwrap();
    assert_eq!((s["pack"]["id"].as_str(), s["pack"]["kind"].as_str(), s["pack"]["name"].as_str()), (Some("wobble-bot"), Some("yours"), Some("Wobble Bot")));
    assert!(lib.join("wobble-bot/art/wobble_bot/anim.json").is_file());
    assert!(desk.handle("studio_create", &json!({ "from": "aseprite", "name": "Wobble Bot", "bytes": blob_b64() })).unwrap_err().contains("already exists"));
    let s = desk.handle("studio_create", &json!({ "from": "copy", "source": "blob", "name": "Blob two" })).unwrap();
    assert_eq!((s["pack"]["id"].as_str(), s["pack"]["kind"].as_str()), (Some("blob-two"), Some("yours")));
    for bad in [
        json!({ "from": "copy", "source": "../x", "name": "y" }),
        json!({ "from": "copy", "source": "blob", "name": "z", "id": "../evil" }),
        json!({ "from": "aseprite", "name": "q", "id": "Q Q", "bytes": blob_b64() }),
        json!({ "from": "aseprite", "name": "q", "bytes": "%%%" }),
        json!({ "from": "aseprite", "name": "q", "bytes": ::studio::base64(b"junk") }),
        json!({ "from": "aseprite", "name": "  ", "bytes": blob_b64() }),
        json!({ "from": "path", "name": "q", "path": "/etc" }),
    ] {
        assert!(desk.handle("studio_create", &bad).is_err(), "{bad}");
    }
    assert!(!d.join("evil").exists());
    assert_eq!(kinds(&mut desk).len(), 3);
    // edit the open one (blob-two): a refused save writes nothing
    let e = desk.handle("studio_edit", &Value::Null).unwrap();
    let pj = lib.join("blob-two/pack.json");
    let text = std::fs::read_to_string(&pj).unwrap();
    assert!(desk.handle("studio_save", &json!({ "sha": "stale", "raw": text })).unwrap_err().contains("changed since"));
    let err = desk.handle("studio_save", &json!({ "sha": e["sha"], "ops": [{ "op": "remove", "path": ["behaviors", "rest"] }] })).unwrap_err();
    assert!(err.contains("stop the mascot loading"), "{err}");
    assert!(desk.handle("studio_save", &json!({ "sha": e["sha"], "raw": "{" })).is_err());
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), text, "nothing written");
    let dry = desk.handle("studio_save", &json!({ "sha": e["sha"], "dry": true, "ops": [{ "op": "set", "path": ["author"], "json": "\"me\"" }] })).unwrap();
    assert_eq!(dry["ok"], true);
    assert_eq!(std::fs::read_to_string(&pj).unwrap(), text, "a dry run writes nothing");
    // clips: add a group, remove it again
    let e = desk.handle("studio_import", &json!({ "name": "extra", "file_name": "extra.aseprite", "bytes": blob_b64() })).unwrap();
    assert_eq!(e["imported"], json!(["extra_walk"]));
    assert!(desk.handle("studio_state", &Value::Null).unwrap()["pack"]["clips"].as_array().unwrap().iter().any(|c| c["name"] == "extra_walk"), "reloaded at once");
    let e = desk.handle("studio_remove_group", &json!({ "group": "extra" })).unwrap();
    assert!(e["groups"].as_array().unwrap().iter().all(|g| g["name"] != "extra"));
    assert!(desk.handle("studio_remove_group", &json!({ "group": "blob" })).unwrap_err().contains("used by"));
    assert!(desk.handle("studio_import", &json!({ "name": "big", "bytes": "A".repeat(50 << 20) })).unwrap_err().contains("too big"));
    // thumbnails, Finder
    assert!(desk.handle("studio_thumb", &json!({ "id": "wobble-bot" })).unwrap()["png"].as_str().unwrap().starts_with("data:image/png;base64,"));
    assert!(desk.handle("studio_thumb", &json!({ "id": "../x" })).is_err());
    desk.handle("studio_reveal", &json!({ "id": "blob" })).unwrap();
    // on the desktop, then deleted: the desktop goes back to the bear
    desk.handle("studio_use", &json!({ "id": "wobble-bot" })).unwrap();
    desk.handle("studio_open", &json!({ "id": "wobble-bot" })).unwrap();
    let s = desk.handle("studio_delete", &json!({ "id": "wobble-bot" })).unwrap();
    assert!(s["pack"].is_null(), "the open mascot was deleted: back to the library");
    assert!(!lib.join("wobble-bot").exists());
    assert_eq!(*calls.lock().unwrap(), vec!["wobble-bot".to_string(), "bear".to_string()]);
    assert!(desk.handle("studio_delete", &json!({ "id": "wobble-bot" })).is_err());
    assert_eq!(kinds(&mut desk), vec![("blob-two".into(), "yours".into()), ("blob".into(), "builtin".into())]);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_library_folder_whose_name_is_not_an_id_is_read_only() {
    let d = tmp("badname");
    let (mut desk, lib, _, calls) = library_desk(&d);
    let p = blob_pack(&lib, "My Copy");
    let before = hashes(&p);
    assert_eq!(kinds(&mut desk), vec![("My Copy".into(), "folder".into()), ("blob".into(), "builtin".into())]);
    let l = desk.handle("studio_library", &Value::Null).unwrap();
    let can = &l["packs"][0]["can"];
    assert_eq!((can["edit"].as_bool(), can["delete"].as_bool(), can["use"].as_bool()), (Some(false), Some(false), Some(false)), "{l}");
    let s = desk.handle("studio_open", &json!({ "id": "My Copy" })).unwrap();
    assert_eq!((s["pack"]["kind"].as_str(), s["pack"]["writable"].as_bool()), (Some("folder"), Some(false)));
    let e = desk.handle("studio_edit", &Value::Null).unwrap();
    assert!(desk.handle("studio_save", &json!({ "sha": e["sha"], "raw": e["text"] })).is_err());
    assert!(desk.handle("studio_delete", &json!({ "id": "My Copy" })).is_err());
    assert!(desk.handle("studio_use", &json!({ "id": "My Copy" })).is_err());
    assert!(calls.lock().unwrap().is_empty());
    assert_eq!(hashes(&p), before);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_studio_without_a_library_writes_nothing() {
    let d = tmp("nolib");
    let p = blob_pack(&d, "p");
    let mut desk = Desk::new(Some(p.clone()), vec![p.clone()]);
    let s = desk.handle("studio_state", &Value::Null).unwrap();
    assert_eq!((s["pack"]["kind"].as_str(), s["library"].as_bool()), (Some("folder"), Some(false)));
    let e = desk.handle("studio_edit", &Value::Null).unwrap();
    assert!(desk.handle("studio_save", &json!({ "sha": e["sha"], "raw": e["text"] })).unwrap_err().contains("folder"));
    for cmd in ["studio_customize", "studio_create"] {
        assert!(desk.handle(cmd, &json!({ "id": "blob", "name": "x", "from": "copy", "source": "blob" })).unwrap_err().contains("library"), "{cmd}");
    }
    assert!(desk.handle("studio_delete", &json!({ "id": "blob" })).is_err());
    assert!(desk.handle("studio_use", &json!({ "id": "blob" })).is_err(), "a folder pack can't go on the desktop");
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(&d);
}

#[cfg(test)]
mod acl {
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::test::{get_ipc_response, mock_builder, INVOKE_KEY};
    use tauri::webview::InvokeRequest;
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

    fn call(app: &tauri::App<tauri::test::MockRuntime>, label: &str, url: &str, cmd: &str) -> Result<(), String> {
        let w = app.get_webview_window(label).map(Ok).unwrap_or_else(|| WebviewWindowBuilder::new(app, label, WebviewUrl::External(url.parse().unwrap())).build()).unwrap();
        let req = InvokeRequest { cmd: cmd.into(), callback: CallbackFn(0), error: CallbackFn(1), url: url.parse().unwrap(), body: InvokeBody::default(), headers: Default::default(), invoke_key: INVOKE_KEY.into() };
        get_ipc_response(&w, req).map(|_| ()).map_err(|e| e.to_string())
    }

    #[test]
    fn only_the_studio_window_on_the_app_origin_reaches_studio_commands() {
        let app = mock_builder()
            .invoke_handler(|i| {
                i.resolver.resolve(());
                true
            })
            .build(tauri::generate_context!(test = true))
            .unwrap();
        let local = "tauri://localhost/studio/index.html";
        assert_eq!(call(&app, "studio", local, "studio_state"), Ok(()));
        assert_eq!(call(&app, "studio", local, "studio_comment_add"), Ok(()));
        // nothing beyond its own commands
        assert!(call(&app, "studio", local, "signin_state").unwrap_err().contains("not allowed"));
        assert!(call(&app, "studio", local, "card_answer").unwrap_err().contains("not allowed"));
        // other windows can't reach the studio's
        for label in ["signin", "bubble", "widget-browser", "browser-page"] {
            assert!(call(&app, label, local, "studio_comment_add").unwrap_err().contains("not allowed"), "{label}");
        }
        // the studio label on another origin: refused
        for url in ["https://example.com/", "http://127.0.0.1:61234/studio/index.html"] {
            let app2 = mock_builder().invoke_handler(|i| {
                i.resolver.resolve(());
                true
            }).build(tauri::generate_context!(test = true)).unwrap();
            assert!(call(&app2, "studio", url, "studio_state").unwrap_err().contains("not allowed"), "{url}");
        }
    }

    #[test]
    fn the_command_refuses_other_callers_too() {
        assert!(crate::caller::is("studio", super::super::window::LABEL, "studio_state"));
        assert!(!crate::caller::is("signin", super::super::window::LABEL, "studio_state"));
        assert!(!crate::caller::is("widget-media", super::super::window::LABEL, "studio_comment_add"));
    }
}
