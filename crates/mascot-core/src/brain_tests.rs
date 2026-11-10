//! Pack loader and brain tests (AC1, AC2). A child of `brain` so the
//! scheduler's statistics can be sampled directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_protocol::{EmoteName, Emotion, MascotEvent, WorkKind};
use serde_json::{json, Value};

use super::*;
use crate::pack::{self, PackError};

// ---- fixtures ----

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bear_dir() -> PathBuf {
    repo().join("mascots/bear")
}

fn bear() -> Arc<PackMeta> {
    Arc::new(pack::load(&bear_dir()).unwrap_or_else(|e| panic!("{e}")))
}

/// The bear as it was before the art round: a temp pack holding only the
/// Stage A/B/props art, without `anim_c/` and `face/`. Keep the `TempPack`
/// alive while the pack is used.
fn pre_art_bear() -> (TempPack, Arc<PackMeta>) {
    let t = TempPack::new("pre-art");
    let p = Arc::new(t.load().unwrap_or_else(|e| panic!("{e}")));
    (t, p)
}

const FREE_CLIPS: [&str; 6] = ["think", "talk", "ask", "oops", "happy", "celebrate"];

/// A throwaway pack: the bear's art (symlinked) plus optional fake art-round
/// sources. Strip PNGs are empty files: the loader only checks they exist.
struct TempPack {
    root: PathBuf,
}

impl Drop for TempPack {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl TempPack {
    fn new(tag: &str) -> TempPack {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let root = std::env::temp_dir().join(format!("mascot-core-test-{}-{tag}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let art = root.join("art");
        std::fs::create_dir_all(&art).unwrap();
        let real = repo().join("experiments/mascot/out/bear3d");
        for d in ["anim", "anim_b", "props"] {
            std::os::unix::fs::symlink(real.join(d).canonicalize().unwrap(), art.join(d)).unwrap();
        }
        std::fs::create_dir_all(root.join("pack")).unwrap();
        let t = TempPack { root };
        let mut p = t.bear_json();
        p["art_root"] = json!(t.root.join("art"));
        let pals: Vec<Value> = p["palettes"].as_array().unwrap().iter().map(|v| json!(bear_dir().join(v.as_str().unwrap()))).collect();
        p["palettes"] = json!(pals);
        t.write_pack(&p);
        t
    }

    fn dir(&self) -> PathBuf {
        self.root.join("pack")
    }

    fn bear_json(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(bear_dir().join("pack.json")).unwrap()).unwrap()
    }

    fn pack_json(&self) -> Value {
        serde_json::from_str(&std::fs::read_to_string(self.dir().join("pack.json")).unwrap()).unwrap()
    }

    fn write_pack(&self, v: &Value) {
        std::fs::write(self.dir().join("pack.json"), serde_json::to_string_pretty(v).unwrap()).unwrap();
    }

    fn edit(&self, f: impl FnOnce(&mut Value)) {
        let mut v = self.pack_json();
        f(&mut v);
        self.write_pack(&v);
    }

    /// `anim_c/anim_c.json` with the art round's six free clips.
    fn add_anim_c(&self) {
        let dir = self.root.join("art/anim_c");
        std::fs::create_dir_all(&dir).unwrap();
        let dirs = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];
        let mut anims = serde_json::Map::new();
        for clip in FREE_CLIPS {
            let mut files = serde_json::Map::new();
            for d in dirs {
                let f = format!("{clip}_{d}.png");
                std::fs::write(dir.join(&f), b"").unwrap();
                files.insert(d.into(), json!(f));
            }
            let root_px: serde_json::Map<String, Value> = dirs.iter().map(|d| (d.to_string(), json!([0.0, 0.0]))).collect();
            let frames: Vec<Value> = (0..6).map(|_| json!({ "dur": 83, "root_world": 0.0, "root_px": root_px })).collect();
            let looping = ["think", "talk", "ask"].contains(&clip);
            anims.insert(
                clip.into(),
                json!({ "loop": looping, "fps": 12, "frame_w": 84, "frame_h": 123,
                        "anchor": [42, 107], "frames_n": 6, "files": files, "frames": frames }),
            );
        }
        let j = json!({ "fps": 12, "ppu": 40.07, "directions": {}, "anims": anims });
        std::fs::write(dir.join("anim_c.json"), serde_json::to_string(&j).unwrap()).unwrap();
    }

    /// `face/face.json` covering every free clip of this temp pack in all 8 dirs
    /// (its own clips, not the real bear's: the real face names Stage C clips).
    fn add_face(&self) {
        let bear = self.load().unwrap_or_else(|e| panic!("{e}"));
        let dir = self.root.join("art/face");
        std::fs::create_dir_all(&dir).unwrap();
        let dirs: Vec<&str> = Dir::ALL.iter().map(|d| d.as_str()).collect();
        let mut layers = serde_json::Map::new();
        for (slot, states) in [("eyes", vec!["open", "closed"]), ("mouth", vec!["closed", "open", "wide"])] {
            let mut by_state = serde_json::Map::new();
            for st in states {
                let mut by_dir = serde_json::Map::new();
                for d in &dirs {
                    let f = format!("{slot}_{st}_{d}.png");
                    std::fs::write(dir.join(&f), b"").unwrap();
                    by_dir.insert(d.to_string(), json!({ "file": f, "w": 8, "h": 4 }));
                }
                by_state.insert(st.into(), Value::Object(by_dir));
            }
            layers.insert(slot.into(), Value::Object(by_state));
        }
        let mut clips = serde_json::Map::new();
        for (name, c) in bear.clips.iter().filter(|(_, c)| c.set.is_none() && c.dirs.len() == 8) {
            let pos: Vec<Value> = (0..c.frames_n()).map(|_| json!([30, 40])).collect();
            let by_dir: serde_json::Map<String, Value> = dirs.iter().map(|d| (d.to_string(), json!({ "eyes": pos, "mouth": pos }))).collect();
            clips.insert(name.clone(), Value::Object(by_dir));
        }
        let j = json!({ "face_version": 1, "dirs": dirs,
            "slots": { "eyes": { "states": ["open", "closed"], "default": "open" },
                       "mouth": { "states": ["closed", "open", "wide"], "default": "closed" } },
            "layers": layers, "clips": clips });
        std::fs::write(dir.join("face.json"), serde_json::to_string(&j).unwrap()).unwrap();
    }

    fn load(&self) -> Result<PackMeta, PackError> {
        pack::load(&self.dir())
    }
}

fn brain_with(pack: Arc<PackMeta>, seed: u64) -> Brain {
    Brain::new(pack, BrainConfig { seed, temperament: None, dnd_action: DndAction::Hide }, 0)
}

/// Ticks at every wake until `until`, returning each distinct show with its time.
fn run_until(b: &mut Brain, from: u64, until: u64) -> Vec<(u64, Show)> {
    let mut out = Vec::new();
    let mut t = from;
    loop {
        out.push((t, b.tick(t)));
        let next = b.next_wake_ms().max(t + 1);
        if next > until {
            break;
        }
        t = next;
    }
    out
}

fn show_after(b: &mut Brain, ev: &MascotEvent, at: u64) -> Show {
    b.event(ev, at);
    b.tick(at + 200)
}

// ---- pack (AC1) ----

#[test]
fn bear_loads_and_every_intent_resolves_before_the_art_round() {
    let (_t, p) = pre_art_bear();
    assert_eq!(p.id, "bear");
    for c in ["idle", "walk_start", "walk", "walk_stop", "run_start", "run", "run_stop", "get_in_bed", "sleep", "wake_up", "sit_down", "read", "stand_up"] {
        assert!(p.clips.contains_key(c), "{c}");
    }
    assert_eq!(p.clips["idle"].dirs.len(), 8);
    assert_eq!(p.clips["idle"].frame_w, 84);
    assert_eq!(p.clips["sleep"].set.as_deref(), Some("bed"));
    assert_eq!(p.clips["get_in_bed"].handoff[&Ori::A].facing, Dir::SW);
    assert!(p.sets["read"].oris[&Ori::B].props.iter().any(|pr| pr.name == "book"));
    assert!(p.face.is_none());
    assert_eq!(p.palettes.len(), 3);
    for i in Intent::ALL {
        assert!(p.available(p.resolve(i)), "{i:?} → {}", p.resolve(i));
    }
    let expect = [
        (Intent::Idle, "rest"),
        (Intent::Think, "hold"),
        (Intent::Talk, "hold"),
        (Intent::Ask, "hold"),
        (Intent::Oops, "rest"),
        (Intent::Celebrate, "rest"),
        (Intent::Sleepy, "nap"),
        (Intent::Spin, "dash"),
        (Intent::Greet, "look_around"),
        (Intent::Remind, "look_around"),
        (Intent::Dodge, "wander"),
    ];
    for (i, b) in expect {
        assert_eq!(p.resolve(i), b, "{i:?}");
    }
    assert!(!p.available("celebrate") && !p.available("happy") && !p.available("think"));
}

#[test]
fn real_bear_has_a_face_and_the_six_art_round_clips() {
    let p = bear();
    for c in FREE_CLIPS {
        let clip = p.clips.get(c).unwrap_or_else(|| panic!("{c}"));
        assert_eq!(clip.dirs.len(), 8, "{c}");
        assert_eq!((clip.frame_w, clip.frame_h), (84, 123), "{c}");
        assert!(clip.set.is_none(), "{c}");
        assert!(p.available(c), "{c}");
    }
    assert!(p.face.is_some(), "face/face.json loads");
    for i in Intent::ALL {
        assert!(p.available(p.resolve(i)), "{i:?} → {}", p.resolve(i));
    }
    let expect = [
        (Intent::Think, "think"),
        (Intent::Work, "think"),
        (Intent::Talk, "talk"),
        (Intent::Ask, "ask"),
        (Intent::Oops, "oops"),
        (Intent::Happy, "happy"),
        (Intent::Celebrate, "celebrate"),
        (Intent::Jump, "celebrate"),
        (Intent::Remind, "ask"),
    ];
    for (i, b) in expect {
        assert_eq!(p.resolve(i), b, "{i:?}");
    }
    // The real face makes the bear blink while it holds a covered clip.
    let starts = blink_starts(p, 9, 60_000);
    assert!(!starts.is_empty(), "the real bear blinks");
}

#[test]
fn art_round_clips_take_over_with_no_pack_edit() {
    let t = TempPack::new("anim-c");
    t.add_anim_c();
    let p = t.load().unwrap_or_else(|e| panic!("{e}"));
    let expect = [
        (Intent::Think, "think"),
        (Intent::Work, "think"),
        (Intent::Talk, "talk"),
        (Intent::Ask, "ask"),
        (Intent::Oops, "oops"),
        (Intent::Happy, "happy"),
        (Intent::Celebrate, "celebrate"),
        (Intent::Sad, "oops"),
        (Intent::Surprised, "oops"),
        (Intent::Jump, "celebrate"),
        (Intent::Wave, "happy"),
        (Intent::Remind, "ask"),
    ];
    for (i, b) in expect {
        assert_eq!(p.resolve(i), b, "{i:?}");
    }
    // Without anim_c the same temp pack falls back (optional + absent = skipped).
    std::fs::remove_dir_all(t.root.join("art/anim_c")).unwrap();
    assert_eq!(t.load().unwrap().resolve(Intent::Celebrate), "rest");
}

fn err_line(e: &PackError) -> String {
    let s = e.to_string();
    assert!(!s.contains('\n'), "one line: {s}");
    s
}

#[test]
fn missing_pack_json() {
    let e = pack::load(Path::new("/nonexistent")).unwrap_err();
    assert!(matches!(e, PackError::Missing { .. }));
    assert_eq!(err_line(&e), "pack /nonexistent: pack.json: not found");
}

#[test]
fn bad_json_reports_line_and_column() {
    let t = TempPack::new("bad-json");
    std::fs::write(t.dir().join("pack.json"), "{\n  \"pack_version\": 1,\n  \"id\": \"bear\" oops\n}").unwrap();
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Parse { .. }), "{e:?}");
    let s = err_line(&e);
    assert!(s.contains("pack.json: bad JSON") && s.contains("line 3 column"), "{s}");
}

#[test]
fn unsupported_pack_version() {
    let t = TempPack::new("version");
    t.edit(|v| v["pack_version"] = json!(2));
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Version { found: 2, supported: 1, .. }), "{e:?}");
    assert!(err_line(&e).ends_with("pack.json: pack_version 2 is not supported (this build reads 1)"));
}

#[test]
fn required_source_missing() {
    let t = TempPack::new("source");
    t.edit(|v| v["sources"][0]["path"] = json!("anim/nope.json"));
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Missing { .. }), "{e:?}");
    assert!(err_line(&e).ends_with("anim/nope.json: not found"), "{e}");
}

#[test]
fn optional_source_present_but_invalid_is_an_error() {
    let t = TempPack::new("optional");
    std::fs::create_dir_all(t.root.join("art/anim_c")).unwrap();
    std::fs::write(t.root.join("art/anim_c/anim_c.json"), "{ nope").unwrap();
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Parse { .. }), "{e:?}");
    assert!(err_line(&e).contains("anim_c/anim_c.json: bad JSON"));
}

#[test]
fn intent_missing_from_intents() {
    let t = TempPack::new("intent");
    t.edit(|v| {
        v["intents"].as_object_mut().unwrap().remove("dodge");
    });
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Invalid { .. }), "{e:?}");
    assert!(err_line(&e).ends_with("pack.json: intents: dodge is missing"), "{e}");
}

#[test]
fn chain_with_nothing_available() {
    let t = TempPack::new("chain");
    t.edit(|v| v["intents"]["celebrate"] = json!(["celebrate", "happy"]));
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Invalid { .. }), "{e:?}");
    assert!(err_line(&e).ends_with("intents: celebrate has no available behavior in [celebrate, happy]"), "{e}");
}

#[test]
fn incomplete_face_is_rejected() {
    let edit_face = |t: &TempPack, f: &dyn Fn(&mut Value)| {
        let p = t.root.join("art/face/face.json");
        let mut v: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        f(&mut v);
        std::fs::write(&p, serde_json::to_string(&v).unwrap()).unwrap();
    };
    let t = TempPack::new("face-ok");
    t.add_face();
    assert!(t.load().is_ok());
    // A slot without a state the brain shows.
    let t = TempPack::new("face-state");
    t.add_face();
    edit_face(&t, &|v| v["slots"]["mouth"]["states"] = json!(["closed", "open"]));
    assert!(err_line(&t.load().unwrap_err()).ends_with("face/face.json: slot mouth has no state wide"));
    // A covered clip and direction without a patch for every state.
    let t = TempPack::new("face-layer");
    t.add_face();
    edit_face(&t, &|v| {
        v["layers"]["eyes"]["closed"].as_object_mut().unwrap().remove("S");
    });
    assert!(err_line(&t.load().unwrap_err()).contains("no eyes closed layer for S"));
    // A covered clip and direction with only one slot's positions.
    let t = TempPack::new("face-slot");
    t.add_face();
    edit_face(&t, &|v| {
        v["clips"]["idle"]["S"].as_object_mut().unwrap().remove("mouth");
    });
    assert!(err_line(&t.load().unwrap_err()).ends_with("clips: idle S: no mouth positions"));
}

#[test]
fn set_missing_an_ori_is_rejected() {
    let t = TempPack::new("set-ori");
    let real = repo().join("experiments/mascot/out/bear3d/anim_b").canonicalize().unwrap();
    let art = t.root.join("art/anim_b");
    std::fs::remove_file(&art).unwrap();
    std::fs::create_dir_all(&art).unwrap();
    for e in std::fs::read_dir(&real).unwrap() {
        let e = e.unwrap();
        if e.file_name() != "anim_b.json" {
            std::os::unix::fs::symlink(e.path(), art.join(e.file_name())).unwrap();
        }
    }
    let mut v: Value = serde_json::from_str(&std::fs::read_to_string(real.join("anim_b.json")).unwrap()).unwrap();
    let set = v["sets"].as_object().unwrap().keys().next().unwrap().clone();
    v["sets"][&set].as_object_mut().unwrap().remove("B");
    std::fs::write(art.join("anim_b.json"), serde_json::to_string(&v).unwrap()).unwrap();
    let e = t.load().unwrap_err();
    assert!(err_line(&e).ends_with(&format!("anim_b/anim_b.json: set {set}: ori B is missing")), "{e}");
}

#[test]
fn relabelled_one_shot_turn_is_not_replayed() {
    // Before the art round oops and nod both resolve to `rest`.
    let (_t, p) = pre_art_bear();
    assert_eq!((p.resolve(Intent::Oops), p.resolve(Intent::Nod)), ("rest", "rest"));
    let mut b = brain_with(p, 2);
    b.event(&MascotEvent::Error { reason: "x".into(), message: "y".into() }, 0);
    // Let the error start playing (`rest` labelled oops), then nod over it.
    let at = run_until(&mut b, 0, 5000).into_iter().find(|(_, s)| s.intent == Intent::Oops).expect("oops plays").0 + 100;
    b.event(&MascotEvent::Emote { name: EmoteName::Nod }, at);
    let shows = run_until(&mut b, at, 30_000);
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Nod), "relabelled to nod");
    let oops_after_nod = shows.iter().skip_while(|(_, s)| s.intent != Intent::Nod).any(|(_, s)| s.intent == Intent::Oops);
    assert!(!oops_after_nod, "the error turn played again after the nod");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle);
}

#[test]
fn errors_are_distinct() {
    let t = TempPack::new("distinct");
    let mut msgs = vec![pack::load(Path::new("/nonexistent")).unwrap_err().to_string()];
    for edit in [json!(2), json!("x")] {
        t.edit(|v| v["pack_version"] = edit.clone());
        msgs.push(t.load().unwrap_err().to_string());
    }
    msgs.sort();
    msgs.dedup();
    assert_eq!(msgs.len(), 3);
}

// ---- brain (AC2) ----

#[test]
fn events_map_to_intents() {
    let p = bear();
    let cases: Vec<(MascotEvent, Intent)> = vec![
        (MascotEvent::Thinking, Intent::Think),
        (MascotEvent::Working { kind: WorkKind::Shell, tool: "bash".into() }, Intent::Work),
        (MascotEvent::AwaitingPermission { call_id: "c".into(), title: "t".into() }, Intent::Ask),
        (MascotEvent::Speaking { text: "hi".into() }, Intent::Talk),
        (MascotEvent::Error { reason: "model_error".into(), message: "m".into() }, Intent::Oops),
        (MascotEvent::Say { text: "hi".into(), emotion: Some(Emotion::Happy) }, Intent::Talk),
        (MascotEvent::Emote { name: EmoteName::Wave }, Intent::Wave),
        (MascotEvent::Emote { name: EmoteName::Spin }, Intent::Spin),
        (MascotEvent::Reminder { id: "r".into(), text: "tea".into(), set_at_ms: None }, Intent::Remind),
    ];
    for (ev, want) in cases {
        let mut b = brain_with(p.clone(), 3);
        let s = show_after(&mut b, &ev, 1000);
        assert_eq!(s.intent, want, "{ev:?}");
        assert_eq!(s.behavior, p.resolve(want), "{ev:?}");
    }
}

#[test]
fn say_plays_its_emotion_after_the_bubble() {
    let mut b = brain_with(bear(), 3);
    b.event(&MascotEvent::Say { text: "Tea?".into(), emotion: Some(Emotion::Happy) }, 1000);
    let shows = run_until(&mut b, 1000, 1000 + bubble_ms("Tea?") + 3000);
    let intents: Vec<Intent> = shows.iter().map(|(_, s)| s.intent).fold(Vec::new(), |mut v, i| {
        if v.last() != Some(&i) {
            v.push(i);
        }
        v
    });
    assert_eq!(&intents[..3], &[Intent::Idle, Intent::Talk, Intent::Happy], "{intents:?}");
}

#[test]
fn say_thinking_follow_up_plays_once_then_idles() {
    let mut b = brain_with(bear(), 3);
    b.event(&MascotEvent::Say { text: "Hmm".into(), emotion: Some(Emotion::Thinking) }, 1000);
    let shows = run_until(&mut b, 1000, 1000 + bubble_ms("Hmm") + 6000);
    let intents: Vec<Intent> = shows.iter().map(|(_, s)| s.intent).fold(Vec::new(), |mut v, i| {
        if v.last() != Some(&i) {
            v.push(i);
        }
        v
    });
    assert_eq!(&intents[..], &[Intent::Idle, Intent::Talk, Intent::Think, Intent::Idle][..intents.len().min(4)], "{intents:?}");
    assert_eq!(intents.last(), Some(&Intent::Idle), "{intents:?}");
}

#[test]
fn unhide_does_not_rewind_the_mouth_or_depend_on_tick_cadence() {
    // Hidden mid-talk, shown again: time never runs backwards.
    let mut b = brain_with(bear(), 4);
    b.event(&MascotEvent::Say { text: "a long enough line to keep talking".into(), emotion: None }, 0);
    b.tick(50);
    b.senses(&Senses { fullscreen: true, ..Senses::default() }, 100);
    b.senses(&Senses::default(), 1000);
    assert!(b.mouth_at > 1000, "stale mouth timer {} would rewind time", b.mouth_at);
    b.tick(1000);
    assert_eq!(b.t, 1000);
    // Same inputs, sparse vs dense ticks: same show afterwards.
    let run = |step: u64| {
        let mut b = brain_with(bear(), 9);
        b.event(&MascotEvent::Speaking { text: "Hi".into() }, 0);
        let mut t = 0;
        while t < 60_000 {
            b.tick(t);
            t += step;
        }
        b.event(&MascotEvent::Idle, 60_000);
        let s = b.tick(90_000);
        (s.behavior, s.clip, s.dir)
    };
    assert_eq!(run(7), run(5000));
}

#[test]
fn timeline_senses_null_clears_a_field() {
    let p: crate::replay::SensesPatch = serde_json::from_str(r#"{"frontmost":null}"#).unwrap();
    assert_eq!(p.frontmost, Some(None));
    let p: crate::replay::SensesPatch = serde_json::from_str("{}").unwrap();
    assert_eq!(p.frontmost, None);
    // Write → read keeps "unchanged" and "clear" apart.
    let p = crate::replay::SensesPatch { user_idle_ms: Some(5), panel: Some(None), ..Default::default() };
    let text = serde_json::to_string(&p).unwrap();
    assert_eq!(text, r#"{"user_idle_ms":5,"panel":null}"#);
    assert_eq!(serde_json::from_str::<crate::replay::SensesPatch>(&text).unwrap(), p);
}

#[test]
fn held_intents_end_on_settled_and_idle() {
    let p = bear();
    // Ask ends on Settled.
    let mut b = brain_with(p.clone(), 1);
    assert_eq!(show_after(&mut b, &MascotEvent::AwaitingPermission { call_id: "c".into(), title: "t".into() }, 1000).intent, Intent::Ask);
    assert_eq!(b.tick(20_000).intent, Intent::Ask, "held while waiting");
    b.event(&MascotEvent::Settled, 20_000);
    assert_eq!(b.tick(24_000).intent, Intent::Idle);
    // Talk is held through stalls (lead decision) and ends on Idle.
    let mut b = brain_with(p.clone(), 1);
    show_after(&mut b, &MascotEvent::Speaking { text: "Hel".into() }, 1000);
    assert_eq!(b.tick(15_000).intent, Intent::Talk, "a stall keeps talk");
    b.event(&MascotEvent::Idle, 15_000);
    assert_eq!(b.tick(19_000).intent, Intent::Idle);
    // Think ignores Settled, ends on Idle.
    let mut b = brain_with(p, 1);
    show_after(&mut b, &MascotEvent::Thinking, 1000);
    b.event(&MascotEvent::Settled, 2000);
    assert_eq!(b.tick(9000).intent, Intent::Think);
    b.event(&MascotEvent::Idle, 9000);
    assert_eq!(b.tick(13_000).intent, Intent::Idle);
}

#[test]
fn harness_interrupts_a_set_behavior_through_its_exit_clip() {
    let p = bear();
    let mut b = brain_with(p, 5);
    b.start("nap", Intent::Idle, 0);
    assert!(b.tick(0).cut);
    let s = b.tick(5000);
    assert_eq!((s.clip.as_str(), s.ori.is_some()), ("sleep", true));
    b.event(&MascotEvent::Thinking, 5000);
    assert_eq!(b.tick(5100).clip, "wake_up");
    let shows = run_until(&mut b, 5100, 12_000);
    let free = shows.iter().find(|(_, s)| s.ori.is_none()).expect("leaves the set");
    assert!(free.1.cut && free.1.intent == Intent::Think, "{:?}", free.1);
    let exit = &b.pack.clips["wake_up"].handoff;
    assert!(exit.values().any(|h| h.facing == free.1.dir));
}

fn no_cooldowns(p: &PackMeta) -> Arc<PackMeta> {
    let mut p = p.clone();
    for e in &mut p.idle_pool {
        e.cooldown_s = 0.0;
    }
    Arc::new(p)
}

fn picks(b: &mut Brain, n: usize) -> Vec<String> {
    (0..n).map(|i| pick_now(b, i as u64 * 1000)).collect()
}

fn pick_now(b: &mut Brain, t: u64) -> String {
    let p = b.pick(t);
    b.last_start.insert(p.clone(), t);
    p
}

#[test]
fn cooldown_blocks_a_pick() {
    let mut b = brain_with(bear(), 1);
    b.last_start.insert("dash".into(), 10_000);
    let w = |b: &Brain, t| b.pool_weights(t).into_iter().find(|(n, _)| n == "dash").unwrap().1;
    assert_eq!(w(&b, 10_000 + 89_000), 0.0);
    assert!(w(&b, 10_000 + 90_000) > 0.0);
    for t in 0..2000 {
        assert_ne!(pick_now(&mut b, 10_000 + t), "dash");
    }
}

#[test]
fn recent_pick_penalty_lowers_repeats() {
    let p = no_cooldowns(&bear());
    let repeats = |penalty: f64| {
        let mut q = (*p).clone();
        q.memory.penalty = penalty;
        let mut b = brain_with(Arc::new(q), 11);
        let v = picks(&mut b, 10_000);
        v.windows(2).filter(|w| w[0] == w[1]).count()
    };
    let (with, without) = (repeats(0.25), repeats(1.0));
    assert!((with as f64) < 0.6 * without as f64, "repeats with penalty {with}, without {without}");
}

#[test]
fn energy_shifts_wander_and_dash_share() {
    let p = no_cooldowns(&bear());
    let share = |energy: f64| {
        let mut b = brain_with(p.clone(), 5);
        b.set_temperament(Temperament { energy, ..p.temperament });
        let v = picks(&mut b, 10_000);
        v.iter().filter(|x| *x == "wander" || *x == "dash").count() as f64 / v.len() as f64
    };
    let (hi, lo) = (share(1.0), share(0.0));
    assert!(lo < 0.02 && hi > lo + 0.2, "energy 1: {hi:.3}, energy 0: {lo:.3}");
}

#[test]
fn quiet_hours_pool_is_nap_and_rest() {
    let mut b = brain_with(no_cooldowns(&bear()), 2);
    b.quiet = true;
    let w = b.pool_weights(0);
    assert!(w.iter().all(|(n, _)| n == "nap" || n == "rest"));
    let nap = w.iter().find(|(n, _)| n == "nap").unwrap().1;
    let normal = {
        b.quiet = false;
        // past the warm-up, which keeps nap out at first (quiet hours excepted)
        b.pool_weights(WARM).into_iter().find(|(n, _)| n == "nap").unwrap().1
    };
    assert!((nap - 4.0 * normal).abs() < 1e-9);
    b.quiet = true;
    let v = picks(&mut b, 2000);
    assert!(v.iter().all(|x| x == "nap" || x == "rest"));
}

/// Blink starts (Open → Closed) seen while dragging (a covered idle hold).
fn blink_starts(p: Arc<PackMeta>, seed: u64, until: u64) -> Vec<u64> {
    let mut b = brain_with(p, seed);
    b.poke(Poke::DragStart, 0);
    let shows = run_until(&mut b, 0, until);
    shows.windows(2).filter(|w| w[0].1.eyes == EyeState::Open && w[1].1.eyes == EyeState::Closed).map(|w| w[1].0).collect()
}

#[test]
fn blink_intervals_are_jittered_inside_bounds() {
    let t = TempPack::new("blink");
    t.add_face();
    let mut p = t.load().unwrap_or_else(|e| panic!("{e}"));
    p.timers.blink.double_p = 0.0;
    let bt = p.timers.blink;
    let starts = blink_starts(Arc::new(p.clone()), 9, 600_000);
    assert!(starts.len() > 50, "{}", starts.len());
    let iv: Vec<u64> = starts.windows(2).map(|w| w[1] - w[0]).collect();
    let mean = bt.mean_ms as f64; // sleepiness 0.5 → ×1
    let (lo, hi) = (mean * (1.0 - bt.jitter), mean * (1.0 + bt.jitter));
    for i in &iv {
        assert!((lo.floor() as u64..=hi.ceil() as u64).contains(i), "{i} outside [{lo}, {hi}]");
    }
    let (min, max) = (iv.iter().min().unwrap(), iv.iter().max().unwrap());
    assert!(max - min > 1000, "intervals vary: {min}..{max}");
    // Eyes stay closed for closed_frames frames.
    let mut b = brain_with(Arc::new(p.clone()), 9);
    b.poke(Poke::DragStart, 0);
    let s0 = starts[0];
    b.tick(s0);
    assert_eq!(b.tick(s0 + 1).eyes, EyeState::Closed);
    assert_eq!(b.tick(s0 + bt.closed_frames as u64 * 83 + 1).eyes, EyeState::Open);
    // Sleepier blinks less often.
    p.temperament.sleepiness = 1.0;
    let sleepy = blink_starts(Arc::new(p.clone()), 9, 600_000);
    assert!(sleepy.len() < starts.len() * 8 / 10, "{} vs {}", sleepy.len(), starts.len());
    // Double blinks happen with double_p.
    p.temperament.sleepiness = 0.5;
    p.timers.blink.double_p = 1.0;
    let d = blink_starts(Arc::new(p), 9, 60_000);
    assert!(d.windows(2).any(|w| w[1] - w[0] < 500), "{d:?}");
}

#[test]
fn bear_without_face_never_blinks_or_wakes_for_it() {
    let (_t, p) = pre_art_bear();
    let mut b = brain_with(p, 9);
    b.poke(Poke::DragStart, 0);
    let shows = run_until(&mut b, 0, 60_000);
    assert!(shows.iter().all(|(_, s)| s.eyes == EyeState::Open && s.mouth == MouthState::Closed));
}

#[test]
fn mouth_flaps_while_text_grows_and_closes_on_stall() {
    let t = TempPack::new("mouth");
    t.add_face();
    let p = Arc::new(t.load().unwrap());
    let mut b = brain_with(p, 4);
    b.poke(Poke::DragStart, 0); // stay on a covered clip
    b.event(&MascotEvent::Speaking { text: "Hel".into() }, 1000);
    let mut shapes = Vec::new();
    for i in 0..12u64 {
        let t = 1000 + i * 100;
        b.event(&MascotEvent::Speaking { text: "Hel".to_string() + &"lo".repeat(i as usize + 1) }, t);
        shapes.push(b.tick(t + 50).mouth);
    }
    assert!(shapes.contains(&MouthState::Open) || shapes.contains(&MouthState::Wide), "{shapes:?}");
    assert!(shapes.windows(2).any(|w| w[0] != w[1]));
    let s = b.tick(1000 + 1200 + 401);
    assert_eq!((s.mouth, s.intent), (MouthState::Closed, Intent::Talk), "stalled: closed but still talking");
}

#[test]
fn dnd_hide_freezes_and_resumes() {
    let mut b = brain_with(bear(), 6);
    b.tick(0);
    b.senses(&Senses { fullscreen: true, ..Senses::default() }, 1000);
    let s = b.tick(1000);
    assert_eq!(s.visibility, Visibility::Hidden);
    assert_eq!(b.next_wake_ms(), u64::MAX, "no timer while hidden");
    b.event(&MascotEvent::Thinking, 2000);
    assert_eq!(b.tick(30_000).visibility, Visibility::Hidden);
    b.senses(&Senses::default(), 30_000);
    let s = b.tick(30_200);
    assert_eq!((s.visibility, s.intent), (Visibility::Shown, Intent::Think));
}

#[test]
fn dnd_still_pins_idle_frame_zero_and_lets_turns_play() {
    let (_t, p) = pre_art_bear();
    let mut b = Brain::new(p, BrainConfig { seed: 6, temperament: None, dnd_action: DndAction::Still }, 0);
    b.tick(0);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 1000);
    let dir = b.tick(1000).dir;
    for t in [1000, 30_000, 300_000] {
        let s = b.tick(t);
        assert_eq!((s.visibility, s.clip.as_str(), s.frame, s.dir), (Visibility::Still, "idle", 0, dir), "t={t}");
    }
    assert_eq!(b.next_wake_ms(), u64::MAX, "no face: nothing to wake for");
    b.event(&MascotEvent::Thinking, 300_000);
    let s = b.tick(300_500);
    assert_eq!((s.intent, s.visibility), (Intent::Think, Visibility::Still));
    assert!(s.frame > 0, "a turn plays normally while still");
    b.event(&MascotEvent::Idle, 301_000);
    let s = b.tick(301_000);
    assert_eq!((s.intent, s.frame), (Intent::Idle, 0));
    b.senses(&Senses::default(), 302_000);
    assert_eq!(b.tick(302_000).visibility, Visibility::Shown);
    assert!(b.next_wake_ms() < u64::MAX);
}

#[test]
fn tick_is_idempotent_and_runs_are_deterministic() {
    let run = |seed| {
        let mut b = brain_with(bear(), seed);
        b.event(&MascotEvent::Thinking, 50_000);
        b.event(&MascotEvent::Idle, 60_000);
        let shows = run_until(&mut b, 0, 400_000);
        assert_eq!(b.tick(400_000), b.tick(400_000));
        shows
    };
    assert_eq!(run(1), run(1));
    assert_ne!(run(1), run(2));
}

#[test]
fn wander_moves_and_root_motion_is_whole_pixels() {
    let mut b = brain_with(bear(), 1);
    b.start("wander", Intent::Idle, 0);
    let shows = run_until(&mut b, 0, 6000);
    let total: [i32; 2] = shows.iter().fold([0, 0], |a, (_, s)| [a[0] + s.moved_px[0], a[1] + s.moved_px[1]]);
    assert!(total[0].abs() + total[1].abs() > 20, "{total:?}");
}

#[test]
fn bubble_time_is_clamped() {
    assert_eq!(bubble_ms(""), 1500);
    assert_eq!(bubble_ms("abcd"), 1700);
    assert_eq!(bubble_ms(&"x".repeat(1000)), 8000);
}

// ---- p2 senses: reflexes, DND, quiet hours, p1 brain findings ----

/// Past the bear's warm-up (180 s).
const WARM: u64 = 181_000;

fn sense_at(minutes: u16, idle: u64) -> Senses {
    Senses { local: LocalTime { minutes, weekday: 0 }, user_idle_ms: idle, ..Senses::default() }
}

#[test]
fn warmup_keeps_nap_out_of_the_first_minutes() {
    let p = no_cooldowns(&bear());
    let b = brain_with(p.clone(), 1);
    let nap = |b: &Brain, t| b.pool_weights(t).into_iter().find(|(n, _)| n == "nap").unwrap().1;
    assert_eq!(nap(&b, 0), 0.0);
    assert_eq!(nap(&b, 179_999), 0.0);
    assert!(nap(&b, 180_000) > 0.0);
    let mut q = brain_with(p, 1);
    q.quiet = true;
    assert!(nap(&q, 0) > 0.0, "quiet hours: nap at once");
    // the launch symptom: in 8/8 launches the bear napped 20-30 s after start
    for seed in 1..=60 {
        let mut b = brain_with(bear(), seed);
        let shows = run_until(&mut b, 0, 180_000);
        assert!(shows.iter().all(|(_, s)| s.behavior != "nap"), "seed {seed} napped during the warm-up");
        let kinds: std::collections::BTreeSet<&str> = shows.iter().map(|(_, s)| s.behavior.as_str()).collect();
        assert!(kinds.len() >= 2, "seed {seed}: awake variety, got {kinds:?}");
    }
}

#[test]
fn quiet_pool_and_warmup_come_from_pack_data() {
    let bear = bear();
    assert_eq!(bear.quiet_hours, crate::pack::QuietPool::default());
    assert_eq!(bear.warmup, crate::pack::Warmup::default());
    // an older pack without the keys loads with today's values
    let t = TempPack::new("noquiet");
    t.edit(|v| {
        v.as_object_mut().unwrap().remove("quiet_hours");
        v.as_object_mut().unwrap().remove("warmup");
    });
    let old = t.load().unwrap();
    assert_eq!((old.quiet_hours.clone(), old.warmup.clone()), (crate::pack::QuietPool::default(), crate::pack::Warmup::default()));
    // …and a pack with no nap at all still loads: the defaults name only its behaviors
    t.edit(|v| {
        let o = v.as_object_mut().unwrap();
        o["behaviors"].as_object_mut().unwrap().remove("nap");
        o["intents"]["sleepy"] = json!(["rest"]);
        o["idle_pool"].as_array_mut().unwrap().retain(|e| e["behavior"] != "nap");
    });
    let no_nap = t.load().unwrap();
    assert_eq!(no_nap.quiet_hours.pool, ["rest"]);
    assert!(no_nap.quiet_hours.boost.is_empty() && no_nap.warmup.exclude.is_empty());
    let t = TempPack::new("noquiet2");
    // a pack can change them
    t.edit(|v| {
        v["quiet_hours"] = json!({ "pool": ["rest", "read"], "boost": { "read": 2 } });
        v["warmup"] = json!({ "duration_s": 0 });
    });
    let p = t.load().unwrap();
    let mut b = brain_with(no_cooldowns(&p), 3);
    assert!(b.pool_weights(0).iter().find(|(n, _)| n == "nap").unwrap().1 > 0.0, "no warm-up");
    b.quiet = true;
    let w = b.pool_weights(0);
    assert_eq!(w.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(), ["rest", "read"]);
    // bad data is a load error
    t.edit(|v| v["quiet_hours"] = json!({ "pool": ["snooze"] }));
    assert!(err_line(&t.load().unwrap_err()).contains("undefined behavior snooze"));
    t.edit(|v| v["quiet_hours"] = json!({ "pool": ["nap"], "boost": { "nap": -1 } }));
    assert!(err_line(&t.load().unwrap_err()).contains("boost for nap"));
}

#[test]
fn quiet_hours_play_the_sleepy_pool_with_no_reflex() {
    let mut b = brain_with(bear(), 4);
    let mut s = Senses { quiet_hours: true, ..sense_at(23 * 60 + 30, 0) };
    b.senses(&s, 0);
    s.frontmost = Some(FrontApp { bundle_id: "com.valvesoftware.steam".into(), category: AppCategory::Game });
    s.user_idle_ms = 0;
    b.senses(&s, 1000);
    assert!(b.cue.is_empty(), "no reflex in quiet hours");
    assert!(!b.unprompted_allowed());
    let shows = run_until(&mut b, 1000, 3_600_000);
    assert!(shows.iter().all(|(_, s)| ["rest", "nap"].contains(&s.behavior.as_str())), "sleepy pool only");
    assert!(shows.iter().any(|(_, s)| s.behavior == "nap"));
    assert_eq!(shows.last().unwrap().1.visibility, Visibility::Shown, "quiet hours stay shown");
}

#[test]
fn dnd_with_a_thinking_turn_stays_hidden_and_blocks_unprompted() {
    let mut b = brain_with(bear(), 6);
    b.senses(&sense_at(600, 0), 0);
    assert!(b.unprompted_allowed());
    b.senses(&Senses { screen_share: true, ..sense_at(600, 0) }, 1000);
    b.event(&MascotEvent::Thinking, 2000);
    for t in [2000, 2500, 60_000] {
        assert_eq!(b.tick(t).visibility, Visibility::Hidden, "t={t}");
    }
    assert!(!b.unprompted_allowed());
    assert_eq!(b.next_wake_ms(), u64::MAX);
    // switching the busy action to Still shows the turn, still blocking unprompted turns
    b.set_dnd_action(DndAction::Still, 61_000);
    let s = b.tick(61_500);
    assert_eq!((s.visibility, s.intent), (Visibility::Still, Intent::Think));
    assert!(!b.unprompted_allowed());
    b.senses(&sense_at(600, 0), 62_000);
    assert!(b.unprompted_allowed());
    assert_eq!(b.tick(62_000).visibility, Visibility::Shown);
}

#[test]
fn reflexes_are_off_in_dnd() {
    let mut b = Brain::new(bear(), BrainConfig { seed: 1, temperament: Some(Temperament { shyness: 1.0, ..bear().temperament }), dnd_action: DndAction::Still }, 0);
    b.senses(&Senses { dnd_toggle: true, ..sense_at(300, 0) }, 0);
    let game = Some(FrontApp { bundle_id: "g".into(), category: AppCategory::Game });
    b.senses(&Senses { dnd_toggle: true, frontmost: game.clone(), ..sense_at(300, 0) }, 1000);
    assert!(b.cue.is_empty());
    // DND ends on the same app: no stale edge fires
    b.senses(&Senses { frontmost: game, ..sense_at(300, 0) }, 2000);
    assert!(b.cue.is_empty());
}

#[test]
fn a_game_switch_dodges() {
    let shy = Temperament { shyness: 1.0, ..bear().temperament };
    let mut b = Brain::new(bear(), BrainConfig { seed: 9, temperament: Some(shy), dnd_action: DndAction::Hide }, 0);
    b.senses(&sense_at(300, 0), 0);
    b.senses(&Senses { frontmost: Some(FrontApp { bundle_id: "g".into(), category: AppCategory::Game }), ..sense_at(300, 0) }, 1000);
    let shows = run_until(&mut b, 1000, 20_000);
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Dodge), "dodged");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle, "and back to idle");
}

// ---- replay timelines (the brief's reflex checks) ----

fn timeline(start: &str, duration_ms: u64, items: Vec<crate::replay::Item>) -> crate::replay::Timeline {
    crate::replay::Timeline { timeline_version: 1, seed: 11, duration_ms, start_local: start.into(), weekday: 0, items }
}

fn sense_item(at_ms: u64, patch: Value) -> crate::replay::Item {
    crate::replay::Item { at_ms, event: None, senses: Some(serde_json::from_value(patch).unwrap()), poke: None }
}

/// Times at which the log enters `intent`.
fn entries(log: &[crate::replay::LogLine], intent: Intent) -> Vec<u64> {
    log.windows(2).filter(|w| w[0].intent != intent && w[1].intent == intent).map(|w| w[1].t_ms).collect()
}

#[test]
fn replay_game_app_dodges_code_does_not() {
    let run = |cat: &str, seed| {
        let mut tl = timeline("05:00", 30_000, vec![sense_item(5000, json!({ "frontmost": { "bundle_id": "x", "category": cat } }))]);
        tl.seed = seed;
        entries(&crate::replay::run(bear(), &tl), Intent::Dodge)
    };
    let fired = (1..=20).filter(|s| !run("game", *s).is_empty()).count();
    assert!(fired >= 10, "shyness 0.3 → p 0.8, fired {fired}/20");
    assert!((1..=20).all(|s| run("code", s).is_empty()));
    assert!((1..=20).filter(|s| !run("video", *s).is_empty()).count() >= 10);
}

#[test]
fn replay_six_minutes_idle_then_input_greets() {
    let tl = timeline(
        "05:00",
        400_000,
        vec![sense_item(60_000, json!({ "user_idle_ms": 60_000 })), sense_item(360_000, json!({ "user_idle_ms": 360_000 })), sense_item(361_000, json!({ "user_idle_ms": 500 }))],
    );
    let g = entries(&crate::replay::run(bear(), &tl), Intent::Greet);
    assert_eq!(g.len(), 1, "{g:?}");
    assert!(g[0] >= 361_000);
    // 4 minutes away is not long enough
    let tl = timeline("05:00", 400_000, vec![sense_item(240_000, json!({ "user_idle_ms": 240_000 })), sense_item(241_000, json!({ "user_idle_ms": 0 }))]);
    assert!(entries(&crate::replay::run(bear(), &tl), Intent::Greet).is_empty());
}

#[test]
fn replay_two_hours_of_work_stretches_once() {
    let minute = 60_000;
    let mk = |mins: u64, break_at: Option<u64>| {
        let items = (1..=mins)
            .map(|m| {
                let idle = if Some(m) == break_at { 11 * minute } else { 1000 };
                sense_item(m * minute, json!({ "user_idle_ms": idle }))
            })
            .collect();
        timeline("05:00", mins * minute + 30_000, items)
    };
    let s = entries(&crate::replay::run(bear(), &mk(130, None)), Intent::Stretch);
    assert_eq!(s.len(), 1, "{s:?}");
    assert!(s[0] >= 120 * minute);
    // a 10-minute break in the middle resets the streak
    assert!(entries(&crate::replay::run(bear(), &mk(130, Some(60))), Intent::Stretch).is_empty());
}

#[test]
fn morning_greets_on_the_first_input_after_six() {
    let tl = timeline("05:58", 600_000, vec![sense_item(60_000, json!({ "local": { "minutes": 359, "weekday": 0 } })), sense_item(120_000, json!({ "local": { "minutes": 360, "weekday": 0 } }))]);
    let g = entries(&crate::replay::run(bear(), &tl), Intent::Greet);
    assert_eq!(g.len(), 1, "{g:?}");
    assert!(g[0] >= 120_000);
}

// ---- p1 brain findings ----

#[test]
fn r2_cues_queue_instead_of_overwriting_a_follow_up() {
    let mut b = brain_with(bear(), 2);
    b.event(&MascotEvent::Say { text: "hi".into(), emotion: Some(Emotion::Happy) }, 1000);
    b.event(&MascotEvent::Emote { name: EmoteName::Wave }, 1100);
    b.event(&MascotEvent::Reminder { id: "r1".into(), text: "stand up".into(), set_at_ms: None }, 1200);
    let shows = run_until(&mut b, 1000, 30_000);
    let mut seq: Vec<Intent> = shows.iter().map(|(_, s)| s.intent).collect();
    seq.dedup();
    let pos = |i| seq.iter().position(|x| *x == i).unwrap_or_else(|| panic!("{i:?} missing from {seq:?}"));
    assert!(pos(Intent::Talk) < pos(Intent::Happy), "{seq:?}");
    assert!(pos(Intent::Happy) < pos(Intent::Wave), "{seq:?}");
    assert!(pos(Intent::Wave) < pos(Intent::Remind), "{seq:?}");
    // the queued reminder still gets its whole bubble time once it plays
    let start = shows.iter().find(|(_, s)| s.intent == Intent::Remind).unwrap().0;
    let end = shows.iter().skip_while(|(_, s)| s.intent != Intent::Remind).find(|(_, s)| s.intent != Intent::Remind).unwrap().0;
    assert!(end - start + 200 >= bubble_ms("stand up"), "{start}..{end}");
    assert_eq!(seq.last(), Some(&Intent::Idle));
}

#[test]
fn r2_a_new_say_replaces_the_queued_line_and_the_queue_is_bounded() {
    let mut b = brain_with(bear(), 2);
    b.event(&MascotEvent::Say { text: "one".into(), emotion: Some(Emotion::Sad) }, 0);
    b.event(&MascotEvent::Say { text: "two".into(), emotion: Some(Emotion::Happy) }, 10);
    assert_eq!(b.cue.len(), 1);
    for _ in 0..10 {
        b.event(&MascotEvent::Emote { name: EmoteName::Nod }, 20);
    }
    assert_eq!(b.cue.len(), CUE_CAP);
    assert!(matches!(b.cue[0], Cue::Timed { intent: Intent::Talk, then: Some(Intent::Happy), .. }), "the playing cue is kept");
}

#[test]
fn r7_a_same_ms_tick_repeats_no_cut_or_motion() {
    let mut b = brain_with(bear(), 1);
    b.tick(0);
    b.start("nap", Intent::Idle, 0);
    b.dirty = true;
    let first = b.tick(0);
    assert!(first.cut);
    let again = b.tick(0);
    assert!(!again.cut);
    assert_eq!(again.moved_px, [0, 0]);
    assert_eq!(Show { cut: true, moved_px: first.moved_px, ..again }, first);
}

#[test]
fn r10_the_pick_fallback_never_returns_weight_zero() {
    let w = vec![("rest".to_string(), 1.0), ("nap".to_string(), 0.0)];
    assert_eq!(weighted(&w, 1.0), "rest", "x at the total");
    assert_eq!(weighted(&w, 0.5), "rest");
    assert_eq!(weighted(&[("a".to_string(), 0.0)], 0.0), "rest");
}

#[test]
fn r4_a_long_gap_is_not_replayed_frame_by_frame() {
    let mut b = brain_with(bear(), 5);
    b.tick(0);
    b.event(&MascotEvent::Say { text: "bye".into(), emotion: None }, 10);
    let gap = 8 * 3_600_000;
    let s = b.tick(gap);
    assert_eq!(s.intent, Intent::Idle, "the cue expired in the gap");
    assert_eq!(s.mouth, MouthState::Closed);
    let replayed = b.trace().iter().filter(|l| l.t_ms > 100 && l.t_ms < gap - CATCHUP_MS).count();
    assert_eq!(replayed, 0, "{:?}", b.trace().iter().filter(|l| l.t_ms > 100).take(5).collect::<Vec<_>>());
    // and it keeps animating normally afterwards
    let shows = run_until(&mut b, gap, gap + 20_000);
    assert!(shows.len() > 20);
    assert!(shows.windows(2).all(|w| w[0].0 <= w[1].0));
}

#[test]
fn review_settled_cuts_the_old_mouth_but_the_next_say_still_talks() {
    let mut b = brain_with(bear(), 5);
    b.tick(0);
    b.event(&MascotEvent::Say { text: "first line".into(), emotion: None }, 0);
    b.event(&MascotEvent::Settled, 5);
    assert!(b.cue.is_empty(), "Settled ends the turn's Say: {:?}", b.cue);
    b.event(&MascotEvent::Say { text: "hello there, friend".into(), emotion: None }, 10);
    b.event(&MascotEvent::Settled, 20);
    b.event(&MascotEvent::Say { text: "and one more".into(), emotion: None }, 30);
    assert!(matches!(b.cue.front(), Some(Cue::Timed { intent: Intent::Talk, .. })));
    assert!(b.talking_until > 30, "the Say after Settled has its mouth");
}

#[test]
fn p2_settled_does_not_cut_a_reminder_that_is_showing() {
    let mut b = brain_with(bear(), 5);
    b.tick(0);
    b.event(&MascotEvent::Reminder { id: "r".into(), text: "stand up".into(), set_at_ms: None }, 0);
    b.event(&MascotEvent::Say { text: "hello there, friend".into(), emotion: None }, 10);
    assert_eq!(b.cue.len(), 2);
    b.event(&MascotEvent::Settled, 20);
    assert!(matches!(b.cue.front(), Some(Cue::Timed { intent: Intent::Remind, .. })), "{:?}", b.cue);
    assert_eq!(b.cue.len(), 2, "the queued Say waits behind the reminder");
}

#[test]
fn p2_a_full_queue_of_timed_cues_drops_an_incoming_emote_not_a_reminder() {
    let mut b = brain_with(bear(), 7);
    b.tick(0);
    for (i, r) in ["one", "two", "three", "four"].iter().enumerate() {
        b.event(&MascotEvent::Reminder { id: (*r).into(), text: (*r).into(), set_at_ms: None }, i as u64);
    }
    assert_eq!(b.cue.len(), CUE_CAP);
    let before = b.cue.clone();
    b.event(&MascotEvent::Emote { name: EmoteName::Wave }, 10);
    assert_eq!(b.cue, before, "the emote is dropped, every reminder kept");
}

#[test]
fn review_hidden_drops_one_shots_so_a_reminder_is_not_blocked() {
    let mut b = brain_with(bear(), 6);
    b.tick(0);
    b.senses(&Senses { screen_share: true, ..sense_at(600, 0) }, 0);
    assert_eq!(b.tick(1).visibility, Visibility::Hidden);
    b.event(&MascotEvent::Emote { name: EmoteName::Wave }, 10);
    assert!(b.cue.is_empty(), "no emote queued while hidden");
    b.event(&MascotEvent::Reminder { id: "r".into(), text: "drink water".into(), set_at_ms: None }, 20);
    assert!(matches!(b.cue.front(), Some(Cue::Timed { until: Some(_), .. })), "its clock runs at once");
    b.tick(60_000); // hidden arms no timer: the next wake (any inbox message) catches up
    assert!(b.cue.is_empty(), "the reminder ran out while hidden instead of waiting: {:?}", b.cue);
}

#[test]
fn review_a_full_queue_evicts_an_emote_before_a_reminder() {
    let mut b = brain_with(bear(), 7);
    b.tick(0);
    b.event(&MascotEvent::Emote { name: EmoteName::Wave }, 0);
    b.event(&MascotEvent::Reminder { id: "r".into(), text: "stretch".into(), set_at_ms: None }, 1);
    b.event(&MascotEvent::Emote { name: EmoteName::Nod }, 2);
    b.event(&MascotEvent::Emote { name: EmoteName::Jump }, 3);
    b.event(&MascotEvent::Emote { name: EmoteName::Spin }, 4);
    assert_eq!(b.cue.len(), CUE_CAP);
    assert!(b.cue.iter().any(|c| matches!(c, Cue::Timed { intent: Intent::Remind, .. })), "{:?}", b.cue);
}

#[test]
fn review_quiet_hours_starting_drops_a_queued_reflex() {
    let mut b = brain_with(bear(), 8);
    b.tick(0);
    b.cue.push_back(Cue::Once(Intent::Wave));
    b.cue.push_back(Cue::Reflex(Intent::Greet));
    b.senses(&Senses { quiet_hours: true, ..sense_at(22 * 60 + 30, 0) }, 10);
    assert!(!b.cue.iter().any(|c| matches!(c, Cue::Reflex(_))));
}

#[test]
fn edges_dropping_on_a_top_edge_places_the_panel_follows_and_goes_back() {
    let mut b = Brain::new(bear(), BrainConfig { seed: 1, temperament: None, dnd_action: DndAction::Hide }, 0);
    let feet = [140, 150];
    let win = WinRect { x: 100, y: 300, w: 800, h: 500, owner_pid: 42, layer: 0 };
    let start = WinRect { x: 900, y: 900, w: 284, h: 198, owner_pid: 1, layer: 3 };
    let dropped = WinRect { x: 250 - feet[0], y: 290 - feet[1], ..start };
    let sense = |panel, windows: Vec<WinRect>| Senses { panel: Some(panel), windows, ..Senses::default() };
    // off by default: a drop on the edge places nothing
    b.senses(&sense(start, vec![win]), 10);
    b.poke(Poke::DragStart, 20);
    b.senses(&sense(dropped, vec![win]), 30);
    b.poke(Poke::DragEnd, 40);
    assert_eq!(b.tick(40).place, None);
    assert!(!b.sitting());
    // on
    b.set_edges(Some(feet));
    b.senses(&sense(start, vec![win]), 50);
    b.poke(Poke::DragStart, 60);
    b.senses(&sense(dropped, vec![win]), 70);
    b.poke(Poke::DragEnd, 80);
    let at = Placement { x: 250 - feet[0], y: 300 - feet[1], follow_pid: Some(42) };
    assert_eq!(b.tick(80).place, Some(at));
    assert_eq!(b.tick(90).place, None, "placed once");
    assert!(b.sitting());
    // the window moves: the panel follows
    let moved = WinRect { x: 120, y: 280, ..win };
    b.senses(&sense(Rect::at(at, start), vec![moved]), 100);
    assert_eq!(b.tick(100).place, Some(Placement { x: at.x + 20, y: at.y - 20, ..at }));
    // it closes: back to where the drag started
    b.senses(&sense(start, vec![]), 110);
    assert_eq!(b.tick(110).place, Some(Placement { x: start.x, y: start.y, follow_pid: None }));
    assert!(!b.sitting());
    assert!(b.trace().iter().any(|l| l.what == "edge"));
}

struct Rect;
impl Rect {
    fn at(p: Placement, r: WinRect) -> WinRect {
        WinRect { x: p.x, y: p.y, ..r }
    }
}
