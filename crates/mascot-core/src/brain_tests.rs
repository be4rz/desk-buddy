//! Pack loader and brain tests (AC1, AC2). A child of `brain` so the
//! scheduler's statistics can be sampled directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_protocol::{Emotion, MascotEvent, WorkKind};
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

/// The bear with no `turning` block: facing changes snap, so a set starts on the tick it is
/// chosen (for tests of other mechanics; turning has its own tests).
fn bear_snapping() -> Arc<PackMeta> {
    let mut p = (*bear()).clone();
    p.turning = None;
    Arc::new(p)
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
        // A5: own clip first, then a borrowed fallback, then rest (mascots/bear/README.md § gestures)
        (Intent::Spin, "rest"),
        (Intent::Greet, "rest"),
        (Intent::Remind, "rest"),
        (Intent::Dodge, "dash"),
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
        (Intent::Jump, "jump"),
        (Intent::Remind, "remind"),
    ];
    for (i, b) in expect {
        assert_eq!(p.resolve(i), b, "{i:?}");
    }
    // The real face makes the bear blink while it holds a covered clip (think: a drag now
    // dangles, a baked clip with no face layers).
    let mut b = brain_with(p, 9);
    b.event(&MascotEvent::Thinking, 0);
    let shows = run_until(&mut b, 0, 60_000);
    let blinks = shows.windows(2).filter(|w| w[0].1.eyes == EyeState::Open && w[1].1.eyes == EyeState::Closed).count();
    assert!(blinks > 0, "the real bear blinks");
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
    t.edit(|v| v["pack_version"] = json!(3));
    let e = t.load().unwrap_err();
    assert!(matches!(e, PackError::Version { found: 3, supported: 2, .. }), "{e:?}");
    assert!(err_line(&e).ends_with("pack.json: pack_version 3 is not supported (this build reads 2)"));
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
    b.event(&MascotEvent::Emote { name: "nod".into() }, at);
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
    for edit in [json!(3), json!("x")] {
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
        (MascotEvent::Emote { name: "wave".into() }, Intent::Wave),
        (MascotEvent::Emote { name: "spin".into() }, Intent::Spin),
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
    // held while waiting: the ask, with approval_wait fidgets inside it, and back to the ask
    let shows = run_until(&mut b, 1000, 20_000);
    assert!(shows.iter().all(|(_, s)| s.intent == Intent::Ask || ["foot_tap", "check_watch"].contains(&s.behavior.as_str())), "held while waiting");
    assert!(shows.iter().filter(|(t, _)| *t >= 12_000).any(|(_, s)| s.intent == Intent::Ask), "back to the ask between fidgets");
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
    let p = bear_snapping();
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

/// The base weight formula alone: no cooldowns and no `variety` block (its hard rules would
/// mask the memory penalty and the traits these tests measure).
fn base_formula(p: &PackMeta) -> Arc<PackMeta> {
    let mut p = (*no_cooldowns(p)).clone();
    p.variety = None;
    Arc::new(p)
}

fn picks(b: &mut Brain, n: usize) -> Vec<String> {
    (0..n).map(|i| pick_now(b, i as u64 * 1000)).collect()
}

fn pick_now(b: &mut Brain, t: u64) -> String {
    let p = b.pick(t).0;
    b.last_start.insert(p.clone(), t);
    p
}

#[test]
fn cooldown_blocks_a_pick() {
    let mut b = brain_with(bear(), 1);
    b.last_start.insert("dash".into(), 10_000);
    let w = |b: &Brain, t| b._named_weights(t).into_iter().find(|(n, _)| n == "dash").unwrap().1;
    assert_eq!(w(&b, 10_000 + 89_000), 0.0);
    assert!(w(&b, 10_000 + 90_000) > 0.0);
    for t in 0..2000 {
        assert_ne!(pick_now(&mut b, 10_000 + t), "dash");
    }
}

#[test]
fn recent_pick_penalty_lowers_repeats() {
    let p = base_formula(&bear());
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
    let p = base_formula(&bear());
    // the entries energy feeds (wander, dash and the energetic A4 gaits), read from the pack
    let energetic: Vec<String> = p.idle_pool.iter().filter(|e| e.traits.get("energy").is_some_and(|w| *w > 0.0)).map(|e| e.name().to_string()).collect();
    assert!(energetic.iter().any(|n| n == "wander") && energetic.iter().any(|n| n == "dash"), "{energetic:?}");
    let share = |energy: f64| {
        let mut b = brain_with(p.clone(), 5);
        b.set_temperament(Temperament { energy, ..p.temperament });
        let v = picks(&mut b, 10_000);
        v.iter().filter(|x| energetic.contains(x)).count() as f64 / v.len() as f64
    };
    let (hi, lo) = (share(1.0), share(0.0));
    assert!(lo < 0.02 && hi > lo + 0.2, "energy 1: {hi:.3}, energy 0: {lo:.3}");
}

#[test]
fn quiet_hours_pool_is_nap_and_rest() {
    let mut b = brain_with(no_cooldowns(&bear()), 2);
    b.quiet = true;
    let w = b._named_weights(0);
    assert!(w.iter().all(|(n, _)| n == "nap" || n == "rest"));
    let nap = w.iter().find(|(n, _)| n == "nap").unwrap().1;
    let normal = {
        b.quiet = false;
        // past the warm-up, which keeps nap out at first (quiet hours excepted)
        b._named_weights(WARM).into_iter().find(|(n, _)| n == "nap").unwrap().1
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
    let nap = |b: &Brain, t| b._named_weights(t).into_iter().find(|(n, _)| n == "nap").unwrap().1;
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
    // the bear adds nightcap (hidden until its p5 art lands: waiting(), never a pick)
    let want = crate::pack::QuietPool { pool: vec!["nap".into(), "rest".into(), "nightcap".into()], ..Default::default() };
    assert_eq!(bear.quiet_hours, want);
    assert!(!bear.available("nightcap"));
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
        // an older pack: no body block either (the bear's own names nap, which is now an error)
        o.remove("body");
    });
    let no_nap = t.load().unwrap();
    assert_eq!(no_nap.quiet_hours.pool, ["rest"]);
    assert_eq!(no_nap.body.activity_names(), ["read"], "the default body block drops what the pack can't play");
    assert!(no_nap.quiet_hours.boost.is_empty() && no_nap.warmup.exclude.is_empty());
    let t = TempPack::new("noquiet2");
    // a pack can change them
    t.edit(|v| {
        v["quiet_hours"] = json!({ "pool": ["rest", "read"], "boost": { "read": 2 } });
        v["warmup"] = json!({ "duration_s": 0 });
    });
    let p = t.load().unwrap();
    let mut b = brain_with(no_cooldowns(&p), 3);
    assert!(b._named_weights(0).iter().find(|(n, _)| n == "nap").unwrap().1 > 0.0, "no warm-up");
    b.quiet = true;
    let w = b._named_weights(0);
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
    crate::replay::Item { at_ms, event: None, senses: Some(serde_json::from_value(patch).unwrap()), poke: None, face: None }
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
    b.event(&MascotEvent::Emote { name: "wave".into() }, 1100);
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
        b.event(&MascotEvent::Emote { name: "nod".into() }, 20);
    }
    assert_eq!(b.cue.len(), CUE_CAP);
    assert!(matches!(b.cue[0], Cue::Timed { intent: Intent::Talk, then: Some(Intent::Happy), .. }), "the playing cue is kept");
}

#[test]
fn r7_a_same_ms_tick_repeats_no_cut_or_motion() {
    let mut b = brain_with(bear_snapping(), 1);
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
    let w = vec![(0, 1.0), (1, 0.0)];
    assert_eq!(weighted(&w, 1.0), 0, "x at the total");
    assert_eq!(weighted(&w, 0.5), 0);
    assert_eq!(weighted(&[(3, 0.0), (4, 2.0), (5, 0.0)], 9.0), 4, "the last that can play");
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
    b.event(&MascotEvent::Emote { name: "wave".into() }, 10);
    assert_eq!(b.cue, before, "the emote is dropped, every reminder kept");
}

#[test]
fn review_hidden_drops_one_shots_so_a_reminder_is_not_blocked() {
    let mut b = brain_with(bear(), 6);
    b.tick(0);
    b.senses(&Senses { screen_share: true, ..sense_at(600, 0) }, 0);
    assert_eq!(b.tick(1).visibility, Visibility::Hidden);
    b.event(&MascotEvent::Emote { name: "wave".into() }, 10);
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
    b.event(&MascotEvent::Emote { name: "wave".into() }, 0);
    b.event(&MascotEvent::Reminder { id: "r".into(), text: "stretch".into(), set_at_ms: None }, 1);
    b.event(&MascotEvent::Emote { name: "nod".into() }, 2);
    b.event(&MascotEvent::Emote { name: "jump".into() }, 3);
    b.event(&MascotEvent::Emote { name: "spin".into() }, 4);
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

// ---- body tools (body-tools AC1–AC5) ----

fn do_(b: &mut Brain, name: &str, for_ms: Option<u64>, t: u64) -> BodyReply {
    b.body(&BodyReq::Do { name: name.into(), for_ms }, t)
}

/// The first time at or after `from` the bear shows `clip`.
fn first_clip(shows: &[(u64, Show)], clip: &str) -> Option<u64> {
    shows.iter().find(|(_, s)| s.clip == clip).map(|(t, _)| *t)
}

fn activity_notes(b: &Brain) -> Vec<String> {
    b.trace().iter().filter(|l| l.what == "activity").map(|l| l.note.clone()).collect()
}

#[test]
fn a_do_during_a_turn_waits_for_the_reply_then_holds_and_leaves() {
    let mut b = brain_with(bear(), 11);
    b.event(&MascotEvent::Thinking, 0);
    b.event(&MascotEvent::Working { kind: WorkKind::Buddy, tool: "buddy mascot do read --for 5m".into() }, 1000);
    let r = do_(&mut b, "read", Some(300_000), 1010);
    assert_eq!((r.status, r.for_ms, r.message.as_str()), (Some(Status::Queued), Some(300_000), "Started reading for 5m (after this reply)."));
    assert_eq!(b.playing(), Some(Playing { activity: "read".into(), until_ms: None, pending: true }));
    // the bridge sends Thinking again between tool calls: a pending activity survives it
    b.event(&MascotEvent::Thinking, 1500);
    assert!(b.playing().is_some_and(|p| p.pending), "pending survives a mid-turn Thinking");
    b.event(&MascotEvent::Speaking { text: "Okay! Grabbing my book.".into() }, 2000);
    let during = run_until(&mut b, 2000, 3999);
    assert!(during.iter().all(|(_, s)| s.intent != Intent::Do), "nothing plays before the reply ends");
    b.event(&MascotEvent::Idle, 4000);
    assert_eq!(b.playing(), Some(Playing { activity: "read".into(), until_ms: Some(304_000), pending: false }));
    assert_eq!(b.next_wake_ms().min(304_000), b.next_wake_ms(), "the deadline is a wake");
    let shows = run_until(&mut b, 4000, 320_000);
    let sit = first_clip(&shows, "sit_down").expect("sits down");
    assert!(sit < 6000, "starts right after the reply's loop: {sit}");
    let stand = first_clip(&shows, "stand_up").expect("stands up");
    assert_eq!(stand, 304_000, "leaves at the deadline");
    let reading: Vec<_> = shows.iter().filter(|(t, _)| *t > sit + 1500 && *t < stand).collect();
    assert!(reading.iter().all(|(_, s)| (s.intent, s.behavior.as_str(), s.clip.as_str()) == (Intent::Do, "read", "read")), "holds the read loop");
    let after = shows.iter().find(|(t, _)| *t > stand + 1500).unwrap();
    assert_eq!(after.1.intent, Intent::Idle);
    assert_eq!(b.playing(), None);
}

#[test]
fn a_click_ends_a_running_activity_through_its_exit_and_cancels_a_pending_one() {
    let mut b = brain_with(bear(), 12);
    let r = do_(&mut b, "read", None, 100);
    assert_eq!((r.status, r.for_ms), (Some(Status::Started), Some(120_000)), "no turn: starts now, default 2m");
    let shows = run_until(&mut b, 100, 10_000);
    assert!(shows.last().is_some_and(|(_, s)| s.clip == "read"), "reading");
    b.poke(Poke::Click, 10_000);
    assert_eq!(b.playing(), None);
    let shows = run_until(&mut b, 10_000, 14_000);
    assert_eq!(shows[0].1.clip, "stand_up", "the exit clip at once");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle);

    // pending: the click cancels it before the reply ends; nothing plays later
    b.event(&MascotEvent::Thinking, 20_000);
    assert_eq!(do_(&mut b, "nap", None, 20_100).status, Some(Status::Queued));
    b.poke(Poke::Click, 20_200);
    assert_eq!(b.playing(), None);
    b.event(&MascotEvent::Idle, 21_000);
    let shows = run_until(&mut b, 21_000, 30_000);
    assert!(shows.iter().all(|(_, s)| s.intent != Intent::Do), "the cancelled do never starts");
    assert!(activity_notes(&b).iter().any(|n| n == "end nap (click)"), "{:?}", activity_notes(&b));
}

#[test]
fn the_next_turn_ends_a_started_activity() {
    let mut b = brain_with(bear(), 13);
    do_(&mut b, "read", None, 0);
    run_until(&mut b, 0, 8000);
    b.event(&MascotEvent::Thinking, 8000);
    let shows = run_until(&mut b, 8000, 12_000);
    assert_eq!(shows[0].1.clip, "stand_up");
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Think), "then thinks");
    assert_eq!(b.playing(), None);
}

/// A turn whose first status is a tool, a card or text (no Thinking first) still ends a started
/// activity, so it does not sit back down after the turn.
#[test]
fn a_turn_that_opens_without_thinking_still_ends_a_started_activity() {
    let opens = [
        MascotEvent::Working { kind: mascot_protocol::WorkKind::Files, tool: "read".into() },
        MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Allow?".into() },
        MascotEvent::Speaking { text: "Hi".into() },
    ];
    for ev in opens {
        let mut b = brain_with(bear(), 13);
        do_(&mut b, "read", None, 0);
        run_until(&mut b, 0, 8000);
        assert!(b.playing().is_some_and(|p| !p.pending), "reading: {ev:?}");
        b.event(&ev, 8000);
        let shows = run_until(&mut b, 8000, 12_000);
        assert_eq!(shows[0].1.clip, "stand_up", "{ev:?}");
        assert_eq!(b.playing(), None, "{ev:?}");
        b.event(&MascotEvent::Idle, 12_000);
        let after = run_until(&mut b, 12_000, 20_000);
        assert!(after.iter().all(|(_, s)| s.intent != Intent::Do), "no sitting back down: {ev:?}");
    }
}

/// The bubble dismissed mid-turn (`Settled`) clears the turn's talk, but the agent is still working:
/// a pending activity waits for the turn's real end (Idle), and its clock starts only then.
#[test]
fn a_pending_activity_waits_for_idle_even_when_the_bubble_is_dismissed_mid_turn() {
    let mut b = brain_with(bear(), 21);
    b.event(&MascotEvent::Thinking, 1000);
    let r = do_(&mut b, "read", Some(60_000), 1200);
    assert_eq!(r.status, Some(Status::Queued));
    b.event(&MascotEvent::Speaking { text: "Sure!".into() }, 1500);
    b.event(&MascotEvent::Settled, 2000);
    run_until(&mut b, 2000, 4000);
    assert_eq!(b.playing().map(|p| p.pending), Some(true), "still pending after Settled");
    // a `do` asked now is still queued: the turn is open
    assert_eq!(do_(&mut b, "read", Some(60_000), 4000).status, Some(Status::Queued));
    b.event(&MascotEvent::Working { kind: mascot_protocol::WorkKind::Files, tool: "read".into() }, 4500);
    b.event(&MascotEvent::Idle, 6000);
    run_until(&mut b, 6000, 6100);
    assert_eq!(b.playing().and_then(|p| p.until_ms), Some(66_000), "the clock starts at Idle");
}

#[test]
fn gates_refuse_do_and_emote_and_nothing_plays() {
    for (senses, hidden, reason) in [
        (Senses { dnd_toggle: true, ..Senses::default() }, false, Reason::Dnd),
        (Senses { quiet_hours: true, dnd_toggle: true, ..Senses::default() }, false, Reason::Dnd),
        (Senses { quiet_hours: true, ..Senses::default() }, true, Reason::QuietHours),
        (Senses::default(), true, Reason::Hidden),
    ] {
        let mut b = brain_with(bear(), 14);
        b.senses(&senses, 0);
        b.set_user_hidden(hidden, 0);
        let r = do_(&mut b, "read", None, 100);
        assert_eq!((r.status, r.reason), (Some(Status::Refused), Some(reason)), "{senses:?} hidden={hidden}");
        assert_eq!(r.message, reason.message());
        let e = b.body(&BodyReq::Emote { name: "wave".into() }, 100);
        assert_eq!((e.status, e.reason, e.emote.as_deref()), (Some(Status::Refused), Some(reason), Some("wave")));
        assert_eq!(b.playing(), None);
        let shows = run_until(&mut b, 100, 5000);
        assert!(shows.iter().all(|(_, s)| s.intent != Intent::Do && s.intent != Intent::Wave), "nothing plays");
        // stop and list always answer
        assert_eq!(b.body(&BodyReq::Stop, 5000).status, Some(Status::Nothing));
        assert!(b.body(&BodyReq::List, 5000).activities.is_some());
    }
    assert_eq!(Reason::Dnd.message(), "Not now: do not disturb is on.");
    assert_eq!(Reason::QuietHours.message(), "Not now: it is quiet hours.");
    assert_eq!(Reason::Hidden.message(), "Not now: the bear is hidden.");
}

#[test]
fn unknown_names_are_errors_before_the_gates() {
    let mut b = brain_with(bear(), 15);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 0);
    let r = do_(&mut b, "juggle", None, 10);
    assert_eq!(r.error.as_deref(), Some("unknown activity \"juggle\""));
    // `have` lists what can play now (entries waiting for art are left out), in pack order
    let list = serde_json::to_value(b.body(&BodyReq::List, 10)).unwrap();
    let names = |k: &str| -> Vec<String> { list[k].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap().to_string()).collect() };
    let (acts, emotes) = (names("activities"), names("emotes"));
    assert!(r.have.as_deref().is_some_and(|h| h.starts_with(&["read", "nap", "computer", "lean"].map(String::from))), "{:?}", r.have);
    let mut have = r.have.clone().unwrap();
    have.sort();
    let mut want = acts.clone();
    want.sort();
    assert_eq!(have, want);
    assert_eq!(r.status, None);
    let r = b.body(&BodyReq::Emote { name: "explode".into() }, 10);
    assert_eq!(r.error.as_deref(), Some("unknown emote \"explode\""));
    assert_eq!(r.have.unwrap(), emotes);
}

#[test]
fn stop_list_and_the_duration_cap() {
    let mut b = brain_with(bear(), 16);
    let l = b.body(&BodyReq::List, 0);
    let v = serde_json::to_value(&l).unwrap();
    assert_eq!(v["activities"][0], json!({ "name": "read", "about": "sit down and read a book", "kind": "pastime", "defaultForS": 120, "group": "pastime" }));
    assert_eq!(v["activities"][1]["name"], "nap");
    // entries waiting for art are left out; the rest are listed (derived from the pack, so new art
    // in a later phase does not move this count)
    let p = bear();
    let waits: std::collections::BTreeSet<String> = p.waiting().into_iter().map(|w| w.name).collect();
    let listed = |k: &str| -> std::collections::BTreeSet<String> { v[k].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap().to_string()).collect() };
    let want = |names: Vec<&String>| -> std::collections::BTreeSet<String> { names.into_iter().filter(|n| !waits.contains(*n)).cloned().collect() };
    assert_eq!(listed("activities"), want(p.body.activities.iter().map(|(n, _)| n).collect()));
    assert_eq!(listed("emotes"), want(p.body.emotes.iter().map(|(n, _)| n).collect()));
    assert_eq!(v["emotes"][0], json!({ "name": "wave", "about": "a happy little wave", "group": "gesture" }));
    // and independently of `waiting()`: a copy without the fidget and posture art lists neither
    let (_tmp, r) = crate::test_pack::copy("mascots/bear", "list-without-fp", |v, _| {
        v["sources"].as_array_mut().unwrap().retain(|s| !["anim_f/", "anim_p/"].iter().any(|d| s["path"].as_str().unwrap().starts_with(d)));
    });
    let mut bw = brain_with(Arc::new(r.unwrap_or_else(|e| panic!("{e}"))), 16);
    let vw = serde_json::to_value(bw.body(&BodyReq::List, 0)).unwrap();
    let names = |v: &Value, k: &str| -> std::collections::BTreeSet<String> { v[k].as_array().unwrap().iter().map(|a| a["name"].as_str().unwrap().to_string()).collect() };
    for (k, gone) in [("emotes", "weight_shift"), ("emotes", "ear_twitch"), ("activities", "sit_floor")] {
        assert!(names(&v, k).contains(gone) && !names(&vw, k).contains(gone), "{gone}: listed with its art only");
    }
    assert!(names(&vw, "emotes").contains("wave") && names(&vw, "activities").contains("read"));
    assert_eq!((v["maxForS"].clone(), v["playing"].clone()), (json!(1800), Value::Null));
    // the text: two headed blocks, one line per listed entry, columns padded to the longest group and
    // name (the exact list grows as art lands, so the shape is checked, not a golden)
    let lines: Vec<&str> = l.message.lines().collect();
    let emotes_at = lines.iter().position(|x| *x == "Emotes").unwrap();
    assert_eq!(lines[0], "Activities");
    assert_eq!((emotes_at - 1, lines.len() - emotes_at - 1), (listed("activities").len(), listed("emotes").len()));
    let col = |x: &str| x.trim_start().find(char::is_whitespace).map(|i| i + x.len() - x.trim_start().len());
    let name_col = |x: &str| col(x).map(|c| c + x[c..].len() - x[c..].trim_start().len());
    let at = name_col(lines[1]).unwrap();
    for x in lines.iter().filter(|x| x.starts_with("  ")) {
        assert_eq!(name_col(x), Some(at), "{x:?} in\n{}", l.message);
    }
    assert_eq!(lines[1].split_whitespace().take(2).collect::<Vec<_>>(), ["pastime", "read"]);
    assert!(lines[1].ends_with("(2m default, 30m max)"), "{}", lines[1]);
    assert!(lines.iter().any(|x| x.split_whitespace().collect::<Vec<_>>().join(" ") == "posture lean lean on a toy block and rest"), "{}", l.message);
    assert!(lines.iter().any(|x| x.trim_start().starts_with("work") && x.ends_with("starts at once")), "{}", l.message);
    let r = do_(&mut b, "nap", Some(9_000_000), 10);
    assert_eq!(r.for_ms, Some(1_800_000), "capped at max_for_s");
    let v = serde_json::to_value(b.body(&BodyReq::List, 20)).unwrap();
    assert_eq!(v["playing"], json!({ "activity": "nap", "untilMs": 1_800_010, "pending": false }));
    let s = b.body(&BodyReq::Stop, 30);
    assert_eq!((s.status, s.activity.as_deref(), s.message.as_str()), (Some(Status::Stopped), Some("nap"), "Stopped napping."));
    let s = b.body(&BodyReq::Stop, 40);
    assert_eq!((s.status, s.message.as_str()), (Some(Status::Nothing), "Not doing anything."));
    // emote: played, the intent's own behavior
    let e = b.body(&BodyReq::Emote { name: "wave".into() }, 50);
    assert_eq!((e.status, e.message.as_str()), (Some(Status::Played), "Waved."));
}

#[test]
fn events_route_to_the_body_and_reply_in_the_trace() {
    let mut b = brain_with(bear(), 17);
    b.event(&MascotEvent::Activity { name: "read".into(), for_ms: Some(60_000) }, 0);
    assert_eq!(b.playing().map(|p| p.until_ms), Some(Some(60_000)));
    b.event(&MascotEvent::ActivityStop, 10);
    assert_eq!(b.playing(), None);
    b.event(&MascotEvent::Activity { name: "juggle".into(), for_ms: None }, 20);
    let notes: Vec<String> = b.trace().iter().filter(|l| l.what == "body").map(|l| l.note.clone()).collect();
    assert_eq!(notes, ["do -> started", "stop -> stopped", "do -> unknown activity \"juggle\""]);
}

#[test]
fn the_idle_pool_waits_for_an_activity_and_a_newer_do_replaces_it() {
    let mut b = brain_with(bear(), 18);
    run_until(&mut b, 0, 30_000);
    assert_eq!(do_(&mut b, "nap", Some(60_000), 30_000).status, Some(Status::Started));
    let shows = run_until(&mut b, 30_000, 40_000);
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Do && s.clip == "get_in_bed"), "the pool waits; nap starts");
    assert_eq!(do_(&mut b, "read", Some(60_000), 40_000).status, Some(Status::Started));
    let shows = run_until(&mut b, 40_000, 50_000);
    let wake = first_clip(&shows, "wake_up").expect("leaves the bed first");
    let sit = first_clip(&shows, "sit_down").expect("then reads");
    assert!(wake < sit);
    assert_eq!(b.playing().unwrap().activity, "read");
}

#[test]
fn dnd_or_quiet_hours_end_an_activity() {
    let mut b = brain_with(bear(), 19);
    do_(&mut b, "read", None, 0);
    run_until(&mut b, 0, 8000);
    b.senses(&Senses { quiet_hours: true, ..Senses::default() }, 8000);
    assert_eq!(b.playing(), None);
    let shows = run_until(&mut b, 8000, 12_000);
    assert_eq!(shows[0].1.clip, "stand_up", "quiet: the exit clip, then idle");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle);

    let mut b = brain_with(bear(), 19);
    do_(&mut b, "read", None, 0);
    run_until(&mut b, 0, 8000);
    b.senses(&Senses { fullscreen: true, ..Senses::default() }, 8000);
    assert_eq!(b.playing(), None);
    assert_eq!(b.tick(8000).visibility, Visibility::Hidden);
}

/// A pack with its own body: an activity that plays a free one-shot, and a `plays` emote.
fn custom_body_pack() -> (TempPack, Arc<PackMeta>) {
    let t = TempPack::new("body");
    t.add_anim_c();
    t.edit(|v| {
        v["body"] = json!({ "default_for_s": 60, "max_for_s": 600,
            "activities": { "dance": { "plays": ["happy"], "about": "a happy little dance" } },
            "emotes": { "tea-time": { "plays": ["celebrate"], "about": "a cheer for tea" } } });
    });
    let p = Arc::new(t.load().unwrap_or_else(|e| panic!("{e}")));
    (t, p)
}

#[test]
fn a_pack_can_add_activities_and_emotes_with_odd_names() {
    let (_t, p) = custom_body_pack();
    assert_eq!(p.body.activity_names(), ["dance"]);
    let text = body::prompt_text(&p).unwrap();
    assert!(text.contains("- dance: a happy little dance\n") && text.contains("- tea-time: a cheer for tea\n"), "{text}");
    assert!(text.contains("They last 1m unless you give --for, at most 10m."), "{text}");
    let mut b = brain_with(p, 21);
    let r = do_(&mut b, "dance", None, 0);
    assert_eq!((r.status, r.message.as_str()), (Some(Status::Started), "Started dancing for 1m."));
    // a behavior with no loop restarts until the deadline
    let shows = run_until(&mut b, 0, 59_000);
    let starts = b.trace().iter().filter(|l| l.what == "start" && l.behavior == "happy").count();
    assert!(starts >= 3, "happy restarted {starts} times");
    assert!(shows.iter().skip(2).all(|(_, s)| s.intent == Intent::Do), "dances throughout");
    b.body(&BodyReq::Stop, 59_000);
    let e = b.body(&BodyReq::Emote { name: "tea-time".into() }, 60_000);
    assert_eq!((e.status, e.message.as_str()), (Some(Status::Played), "Played tea-time."), "an odd name gets a neutral text");
    let shows = run_until(&mut b, 60_000, 66_000);
    assert!(shows.iter().any(|(_, s)| (s.intent, s.behavior.as_str()) == (Intent::Emote, "celebrate")), "the gesture plays its behavior");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle);
    let r = do_(&mut b, "tea-time", None, 66_000);
    assert_eq!(r.have.unwrap(), ["dance"], "an emote isn't an activity");
}

#[test]
fn a_pack_without_body_gets_the_default_block() {
    let t = TempPack::new("nobody");
    t.edit(|v| {
        v.as_object_mut().unwrap().remove("body");
    });
    let p = t.load().unwrap();
    assert_eq!(p.body.activity_names(), ["read", "nap"]);
    assert_eq!(p.body.emote_names(), ["wave", "nod", "shake", "jump", "spin"]);
    assert_eq!((p.body.default_for_s, p.body.max_for_s), (120, 1800));
    assert!(p.body_issues.is_empty());
    // without the nap behavior the default leaves nap out instead of failing
    t.edit(|v| {
        let o = v.as_object_mut().unwrap();
        o["behaviors"].as_object_mut().unwrap().remove("nap");
        o["idle_pool"] = json!(o["idle_pool"].as_array().unwrap().iter().filter(|e| e["behavior"] != "nap").cloned().collect::<Vec<_>>());
        o["intents"]["sleepy"] = json!(["rest"]);
        o["quiet_hours"] = json!({ "pool": ["rest"] });
        o["warmup"] = json!({ "duration_s": 180 });
    });
    let p = t.load().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(p.body.activity_names(), ["read"]);
    // a pack with neither: no prompt section
    t.edit(|v| v["body"] = json!({}));
    assert_eq!(body::prompt_text(&t.load().unwrap()), None);
}

#[test]
fn bad_body_entries_are_reported_and_refused() {
    let t = TempPack::new("badbody");
    t.edit(|v| {
        v["body"] = json!({ "default_for_s": 4000, "max_for_s": 1800,
            "activities": {
                "read": { "plays": ["read"], "about": "sit down and read a book" },
                "fly": { "plays": ["soar"], "about": "fly away" },
                "both": { "plays": ["read"], "station": "desk", "about": "two keys" },
                "Bad Name": { "plays": ["read"], "about": "bad name" },
                "long": { "plays": ["read"], "about": "x".repeat(121) },
                "loud": { "plays": ["read"], "about": "Shouty start." },
                "garage": { "station": "garage", "about": "fix the car" } },
            "emotes": { "boom": { "intent": "explode", "about": "boom" },
                "ponder": { "intent": "think", "about": "a long think" },
                "loaf": { "intent": "idle", "about": "do nothing" },
                "hum": { "plays": ["hold"], "about": "stand and hum" },
                "nod": { "intent": "nod", "about": "a small yes" } } });
    });
    let p = pack::load_report(&t.dir()).unwrap();
    assert_eq!(p.body.emote_names(), ["nod"], "an emote that never ends by itself is dropped");
    assert_eq!(p.body.activity_names(), ["read"], "bad entries are dropped");
    let all = p.body_issues.join("\n");
    for want in [
        "default_for_s 4000 is more than max_for_s 1800",
        "activities.fly: unknown behavior soar",
        "activities.both: needs exactly one of plays or station",
        "activities.Bad Name: a name is 1-32",
        "activities.long: about is longer than 120",
        "activities.loud: about starts lower-case",
        "activities.garage: unknown station garage",
        "emotes.boom: unknown intent explode",
        "emotes.ponder: ",
        "emotes.loaf: intent idle is not a gesture",
        "emotes.hum: hold holds until stopped",
    ] {
        assert!(all.contains(want), "missing {want:?} in\n{all}");
    }
    // think resolves to think or hold (whichever the art has): either one holds
    assert!(all.lines().any(|l| l.starts_with("body: emotes.ponder: ") && l.ends_with("holds until stopped; an emote must end by itself")), "{all}");
    let e = t.load().unwrap_err().to_string();
    assert!(e.contains("pack.json: body: default_for_s"), "{e}");
    // max_for_s is capped at the CLI's 30m, so the prompt never advertises a time the host refuses
    t.edit(|v| v["body"] = json!({ "max_for_s": 7200, "default_for_s": 3600, "activities": { "read": { "plays": ["read"], "about": "sit down and read a book" } } }));
    let p = pack::load_report(&t.dir()).unwrap();
    assert!(p.body_issues.iter().any(|i| i == "body: max_for_s 7200 is more than 1800"), "{:?}", p.body_issues);
    assert_eq!((p.body.max_for_s, p.body.default_for_s), (1800, 1800));
}

#[test]
fn the_bear_prompt_section_is_golden() {
    let want = "## Your body

You are the bear on the user's screen. When the user asks you to do something
physical, do it with these commands instead of describing it in words. Never say
you did something the command's reply says did not happen.

Activities (`buddy mascot do <name> [--for 5m]`). Work activities start at once; the
others start after your reply. They last 2m unless you give --for, at most 30m.
`buddy mascot stop` ends one.
- read: sit down and read a book
- nap: get into bed and sleep
- dance: dance
- lean: lean on a toy block and rest
- sit_floor: sit on the floor
- sit_sway: sit on the floor and sway
- lie_belly: lie on its tummy and kick its feet
- lie_back: lie on its back
- floor_nap: curl up for a nap on the floor
- computer (work): sit at the desk and use the computer

Gestures (`buddy mascot emote <name>`; a short gesture during your reply):
- wave: a happy little wave
- nod: a small yes
- shake: a sad little no
- jump: a bounce of joy
- spin: a twirl
- greet: a big two-paw hello
- stretch: a big stretch
- sad: a sad little droop
- surprised: a surprised little hop
- dodge: a quick step aside
- remind: a raised paw: don't forget
- shrug: a shrug: who knows
- facepalm: facepalm at a silly mistake
- thumbs_up: a paw up: well done
- clap: clap its paws
- point_bubble: point at what you just said
- shy_hide: hide its face, shy
- laugh: a happy laugh
- pout: a little pout
- proud: puff its chest, proud
- head_scratch: scratch its head, confused

Fidgets: weight_shift, ear_twitch, look_up, look_down, tummy_rub, wave_small, peek, butt_wiggle, rock_heels, head_tilt, scratch, yawn, shake_off, sneeze, nod_off, squat
Moves: tiptoe, skip, shuffle, back_waddle, hop, roll, trip, chase, slide
Reactions: giggle, head_bob, foot_tap, flinch, check_watch, dizzy, morning_stretch, evening_yawn

When to use them:
- If the user asks you to read, nap, dance, lean, sit_floor, sit_sway, lie_belly, lie_back, floor_nap, computer or do another listed activity, run the command
  and say so briefly.
- Use at most one emote per reply, only when it matches the mood. Most short
  replies need none.
- You go to the desk by yourself during long jobs; do not call it for that.
- If a reply says \"refused\" (do not disturb or quiet hours), tell the user you
  cannot right now; do not pretend.
- `buddy mascot list` shows this list.
";
    assert_eq!(body::prompt_text(&bear()).unwrap(), want);
}

/// AC5: the `#priority` table, row by row (DND/quiet > click > agent > automatic).
#[test]
fn the_priority_table() {
    let reading = |seed| {
        let mut b = brain_with(bear(), seed);
        do_(&mut b, "read", None, 0);
        run_until(&mut b, 0, 8000);
        assert_eq!(b.tick(8000).clip, "read");
        b
    };
    // read | click → stand_up exit, then idle (p1)
    let mut b = reading(30);
    b.poke(Poke::Click, 8000);
    let s = run_until(&mut b, 8000, 12_000);
    assert_eq!((s[0].1.clip.as_str(), s.last().unwrap().1.intent), ("stand_up", Intent::Idle));
    // activity | DND (Hide) → hidden, activity gone; quiet → exit, idle (dnd_or_quiet_hours_end_an_activity)
    let mut b = reading(31);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 8000);
    assert_eq!((b.tick(8000).visibility, b.playing()), (Visibility::Hidden, None));
    // DND / quiet | do / emote → refused, nothing plays (gates_refuse_do_and_emote_and_nothing_plays)
    assert_eq!(do_(&mut b, "read", None, 8100).status, Some(Status::Refused));
    // quiet hours | click → nothing to cancel, no error (listen is p2)
    let mut b = brain_with(bear(), 32);
    b.senses(&Senses { quiet_hours: true, ..Senses::default() }, 0);
    b.poke(Poke::Click, 100);
    assert_eq!(b.playing(), None);
    // DND-Still | click → no clip change
    let mut b = Brain::new(bear(), BrainConfig { seed: 33, temperament: None, dnd_action: DndAction::Still }, 0);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 0);
    let before = b.tick(100);
    b.poke(Poke::Click, 200);
    let after = b.tick(300);
    assert_eq!((after.clip, after.frame, after.visibility), (before.clip, before.frame, Visibility::Still));
    // a turn (listen in p2) | do read → queued, starts on Idle
    let mut b = brain_with(bear(), 34);
    b.event(&MascotEvent::Thinking, 0);
    assert_eq!(do_(&mut b, "read", None, 100).status, Some(Status::Queued));
    b.event(&MascotEvent::Idle, 2000);
    assert!(run_until(&mut b, 2000, 6000).iter().any(|(_, s)| s.intent == Intent::Do));
    // idle pool | do read → started, the pool waits
    let mut b = brain_with(bear(), 35);
    run_until(&mut b, 0, 20_000);
    assert_eq!(do_(&mut b, "read", None, 20_000).status, Some(Status::Started));
    // activity | next turn's Thinking → exit step, then think
    let mut b = reading(36);
    b.event(&MascotEvent::Thinking, 8000);
    let s = run_until(&mut b, 8000, 12_000);
    assert_eq!(s[0].1.clip, "stand_up");
    assert_eq!(s.last().unwrap().1.intent, Intent::Think);
    // both orders: DND first, then the agent's do → refused; do first, then DND → ended
    let mut b = brain_with(bear(), 37);
    do_(&mut b, "nap", None, 0);
    b.senses(&Senses { screen_share: true, ..Senses::default() }, 10);
    assert_eq!(b.playing(), None);
    // reflexes stay quiet while an activity runs
    let mut b = reading(38);
    for i in 0..20 {
        b.senses(&Senses { user_idle_ms: 600_000, ..sense_at(600, 0) }, 8000 + i * 1000);
    }
    assert!(b.trace().iter().all(|l| l.what != "reflex" || l.note.contains("late")), "no reflex during an activity");
}

/// A cue queue full of timed cues drops a new emote, so the reply must say it did not happen.
#[test]
fn an_emote_the_full_queue_drops_is_refused_not_played() {
    let mut b = brain_with(bear(), 22);
    for i in 0..4 {
        b.event(&MascotEvent::Reminder { id: format!("r{i}"), text: "stand up".into(), set_at_ms: None }, 0);
    }
    let e = b.body(&BodyReq::Emote { name: "wave".into() }, 10);
    assert_eq!((e.status, e.reason), (Some(Status::Refused), Some(Reason::Busy)));
    assert_eq!(e.message, "Not now: the bear is busy.");
}

/// The plain `list` text names what plays now and how long is left, or that it waits for the reply.
#[test]
fn list_text_says_what_plays_now() {
    let mut b = brain_with(bear(), 23);
    assert!(!b.body(&BodyReq::List, 0).message.contains("Now:"));
    b.event(&MascotEvent::Thinking, 100);
    do_(&mut b, "read", Some(300_000), 200);
    assert!(b.body(&BodyReq::List, 300).message.starts_with("Now: read (after the reply)\n"));
    b.event(&MascotEvent::Idle, 1000);
    run_until(&mut b, 1000, 1100);
    let m = b.body(&BodyReq::List, 61_000).message;
    assert!(m.starts_with("Now: read (4m left)\n"), "{m}");
}

// ---- listen + turns (p2 listen-brain, AC5–AC7) ----

fn listen_pack() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures/listen_pack")).unwrap_or_else(|e| panic!("{e}")))
}

/// The listen fixture with only `rest` in its idle pool, so idle never turns on its own.
fn listen_pack_still_idle() -> Arc<PackMeta> {
    let mut p = (*listen_pack()).clone();
    p.idle_pool.retain(|e| e.name() == "rest");
    Arc::new(p)
}

/// Consecutive shows collapsed to (clip, dir) segments.
fn segments(shows: &[(u64, Show)]) -> Vec<(String, Dir)> {
    let mut out: Vec<(String, Dir)> = Vec::new();
    for (_, s) in shows {
        if out.last().is_none_or(|l| l.0 != s.clip || l.1 != s.dir) {
            out.push((s.clip.clone(), s.dir));
        }
    }
    out
}

/// Every facing change happens as a turn clip ends: the show before it plays a turn clip
/// from the old dir, and the new dir is exactly one step on.
fn assert_turns_only(shows: &[(u64, Show)], ctx: &str) {
    for w in shows.windows(2) {
        let (a, b) = (&w[0].1, &w[1].1);
        if a.dir != b.dir {
            let by = match a.clip.as_str() {
                "turn_ccw" => 1,
                "turn_cw" => -1,
                other => panic!("{ctx}: dir {:?} -> {:?} at {} without a turn clip (was {other})", a.dir, b.dir, w[1].0),
            };
            assert_eq!(a.dir.step(by), b.dir, "{ctx}: one step a turn");
        }
    }
}

/// Clicks from `from`, opens the prompt, closes it at 6 s; returns the shows up to 9 s.
fn listen_from(p: &Arc<PackMeta>, from: Dir, seed: u64) -> Vec<(u64, Show)> {
    let mut b = brain_with(p.clone(), seed);
    b.face(from, 0);
    b.poke(Poke::Click, 100);
    b.poke(Poke::PromptOpen, 100);
    let mut shows = run_until(&mut b, 100, 6000);
    b.poke(Poke::PromptClosed, 6000);
    shows.extend(run_until(&mut b, 6000, 9000));
    shows
}

#[test]
fn ac6_listen_from_each_of_the_8_dirs_turns_the_exact_path() {
    let p = listen_pack_still_idle();
    assert!(p.turning.is_some());
    for from in Dir::ALL {
        for seed in [1, 2, 3, 4] {
            let shows = listen_from(&p, from, seed);
            let ctx = format!("from {from:?} seed {seed}");
            assert_turns_only(&shows, &ctx);
            let seg = segments(&shows);
            let start = seg.iter().position(|s| s.0 != "idle").unwrap_or_else(|| panic!("{ctx}: never listens: {seg:?}"));
            let lin = seg.iter().position(|s| s.0 == "listen_in").unwrap_or_else(|| panic!("{ctx}: {seg:?}"));
            let turns: Vec<&(String, Dir)> = seg[start..lin].iter().collect();
            let to = seg[lin].1;
            let want: Vec<(&str, Dir)> = match from {
                Dir::NE => vec![("turn_cw", Dir::NE)],
                Dir::NW => vec![("turn_ccw", Dir::NW)],
                Dir::N if to == Dir::E => vec![("turn_cw", Dir::N), ("turn_cw", Dir::NE)],
                Dir::N => vec![("turn_ccw", Dir::N), ("turn_ccw", Dir::NW)],
                _ => vec![],
            };
            let got: Vec<(&str, Dir)> = turns.iter().map(|s| (s.0.as_str(), s.1)).collect();
            assert_eq!(got, want, "{ctx}: path");
            assert_eq!(to, from.listen_target().unwrap_or(to), "{ctx}: listen dir");
            assert!(matches!(to, Dir::S | Dir::SE | Dir::E | Dir::W | Dir::SW), "{ctx}");
            let rest: Vec<&str> = seg[lin..].iter().map(|s| s.0.as_str()).collect();
            assert_eq!(&rest[..4], ["listen_in", "listen", "listen_out", "idle"], "{ctx}");
            assert!(seg[lin..lin + 4].iter().all(|s| s.1 == to), "{ctx}: listens facing {to:?}");
            // held until the prompt closed
            let out_at = shows.iter().find(|(_, s)| s.clip == "listen_out").unwrap().0;
            assert!(out_at >= 6000 && out_at < 6200, "{ctx}: listen_out at {out_at}");
        }
    }
    // N's two ways both come up over seeds (seeded tie-break)
    let tos: std::collections::BTreeSet<Dir> = (0..16)
        .map(|s| segments(&listen_from(&p, Dir::N, s)).iter().find(|x| x.0 == "listen_in").unwrap().1)
        .collect();
    assert_eq!(tos, [Dir::E, Dir::W].into_iter().collect());
}

#[test]
fn ac6_prompt_sent_goes_from_listen_straight_to_think() {
    let mut b = brain_with(listen_pack(), 3);
    b.face(Dir::S, 0);
    b.poke(Poke::PromptOpen, 100);
    run_until(&mut b, 100, 3000);
    assert_eq!(b.tick(3000).clip, "listen");
    b.poke(Poke::PromptSent, 3000);
    b.event(&MascotEvent::Thinking, 3000);
    let shows = run_until(&mut b, 3000, 5000);
    let out = shows.iter().position(|(_, s)| s.clip == "listen_out").expect("listen_out");
    let after: Vec<Intent> = shows[out..].iter().map(|(_, s)| s.intent).filter(|i| *i != Intent::Listen).collect();
    assert!(!after.is_empty() && after.iter().all(|i| *i == Intent::Think), "no idle between: {after:?}");
}

#[test]
fn ac6_prompt_sent_without_a_thinking_event_still_thinks() {
    let mut b = brain_with(listen_pack(), 3);
    b.face(Dir::SE, 0);
    b.poke(Poke::PromptOpen, 100);
    run_until(&mut b, 100, 2000);
    b.poke(Poke::PromptSent, 2000);
    let shows = run_until(&mut b, 2000, 4000);
    assert_eq!(shows.last().unwrap().1.intent, Intent::Think);
    b.event(&MascotEvent::Idle, 4000);
    let shows = run_until(&mut b, 4000, 6000);
    assert_eq!(shows.last().unwrap().1.intent, Intent::Idle);
}

#[test]
fn ac6_closed_while_turning_ends_at_the_current_dir() {
    let mut b = brain_with(listen_pack_still_idle(), 1);
    b.face(Dir::N, 0);
    b.poke(Poke::PromptOpen, 100);
    let mut shows = run_until(&mut b, 100, 200);
    assert!(shows.last().unwrap().1.clip.starts_with("turn_"), "{:?}", segments(&shows));
    b.poke(Poke::PromptClosed, 200);
    shows.extend(run_until(&mut b, 200, 3000));
    assert_turns_only(&shows, "closed mid-turn");
    let seg = segments(&shows);
    assert!(seg.iter().all(|s| !s.0.starts_with("listen")), "{seg:?}");
    let last = seg.last().unwrap();
    assert_eq!(last.0, "idle");
    assert!(matches!(last.1, Dir::NE | Dir::NW), "one step on, then idle there: {seg:?}");
}

#[test]
fn ac6_a_click_alone_does_not_listen() {
    // the driver posts no PromptOpen when a card is up: the click alone plays no listen
    let mut b = brain_with(listen_pack_still_idle(), 2);
    b.face(Dir::NE, 0);
    b.poke(Poke::Click, 100);
    let shows = run_until(&mut b, 100, 4000);
    assert!(shows.iter().all(|(_, s)| s.intent != Intent::Listen && !s.clip.starts_with("turn_")), "{:?}", segments(&shows));
}

#[test]
fn ac6_dnd_still_opens_no_listen_clip_and_hidden_plays_nothing() {
    let mut b = Brain::new(listen_pack(), BrainConfig { seed: 4, temperament: None, dnd_action: DndAction::Still }, 0);
    b.face(Dir::NE, 0);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 50);
    b.poke(Poke::Click, 100);
    b.poke(Poke::PromptOpen, 100);
    let shows = run_until(&mut b, 100, 4000);
    assert!(shows.iter().all(|(_, s)| s.intent != Intent::Listen && s.clip == "idle" && s.frame == 0 && s.dir == Dir::NE), "pinned");
    b.poke(Poke::PromptClosed, 4000);
    assert_eq!(b.tick(4100).clip, "idle");

    let mut b = brain_with(listen_pack(), 4);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 50);
    b.poke(Poke::PromptOpen, 100);
    let shows = run_until(&mut b, 100, 4000);
    assert!(shows.iter().all(|(_, s)| s.visibility == Visibility::Hidden && s.intent != Intent::Listen));
}

#[test]
fn ac6_quiet_hours_still_listen() {
    let mut b = brain_with(listen_pack(), 5);
    b.face(Dir::W, 0);
    b.senses(&Senses { quiet_hours: true, ..Senses::default() }, 50);
    b.poke(Poke::PromptOpen, 100);
    let shows = run_until(&mut b, 100, 2000);
    assert!(shows.iter().any(|(_, s)| s.clip == "listen" && s.dir == Dir::W), "{:?}", segments(&shows));
}

#[test]
fn ac6_without_turning_the_bear_snaps_and_still_listens() {
    let mut p = (*listen_pack_still_idle()).clone();
    p.turning = None;
    let p = Arc::new(p);
    for from in Dir::ALL {
        let shows = listen_from(&p, from, 6);
        let seg = segments(&shows);
        assert!(seg.iter().all(|s| !s.0.starts_with("turn_")), "{from:?}: {seg:?}");
        let lin = seg.iter().find(|s| s.0 == "listen_in").unwrap_or_else(|| panic!("{from:?}: {seg:?}"));
        assert_eq!(Some(lin.1), from.listen_target().or(Some(lin.1)), "{from:?}");
    }
    // the real bear without its turning block: it snaps NE → E and listens with the real clips
    let mut b = brain_with(bear_snapping(), 6);
    b.face(Dir::NE, 0);
    b.poke(Poke::PromptOpen, 100);
    let shows = run_until(&mut b, 100, 2000);
    assert!(segments(&shows).iter().all(|s| !s.0.starts_with("turn_")), "{:?}", segments(&shows));
    let last = &shows.last().unwrap().1;
    assert_eq!((last.intent, last.dir, last.clip.as_str()), (Intent::Listen, Dir::E, "listen"));
}

#[test]
fn ac6_the_real_bear_turns_from_every_facing_and_listens() {
    let p = bear();
    assert!(p.turn_clip(Dir::NE, -1).is_some() && p.turn_clip(Dir::NW, 1).is_some(), "the bear has anim_l");
    for from in Dir::ALL {
        let mut b = brain_with(p.clone(), 6);
        b.face(from, 0);
        b.poke(Poke::Click, 100);
        b.poke(Poke::PromptOpen, 100);
        let shows = run_until(&mut b, 100, 6000);
        let seg = segments(&shows);
        let back = matches!(from, Dir::N | Dir::NE | Dir::NW);
        assert_eq!(seg.iter().any(|s| s.0.starts_with("turn_")), back, "{from:?}: {seg:?}");
        let last = &shows.last().unwrap().1;
        let want = from.listen_target().map_or(vec![Dir::E, Dir::W], |d| vec![d]);
        assert!(last.intent == Intent::Listen && last.clip == "listen" && want.contains(&last.dir), "{from:?}: {seg:?}");
    }
}

#[test]
fn idle_pool_behaviors_finish_their_turn_on_the_real_bear() {
    // look_around / wander turn first; the turn must not end them (idle resolves to `rest`)
    let mut b = brain_with(bear(), 1);
    b.start("look_around", Intent::Idle, 0);
    let shows = run_until(&mut b, 0, 3000);
    assert!(shows.iter().any(|(_, s)| s.behavior == "look_around" && s.clip == "idle"), "{:?}", segments(&shows));
}

/// A v2 face naming `listen` while the optional `anim_l` source is missing: the pack still loads
/// (listen falls back to `rest`, the bear snaps); the same face with no optional source missing
/// still rejects an unknown clip.
#[test]
fn ac6_face_v2_without_the_anim_l_source_still_loads() {
    let t = TempPack::new("face-v2-no-anim-l");
    t.add_anim_c();
    t.add_face();
    let path = t.root.join("art/face/face.json");
    let mut f: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    f["face_version"] = json!(2);
    let pos: Vec<Value> = (0..16).map(|_| json!([30, 40])).collect();
    f["clips"]["listen"] = json!({ "E": { "eyes": pos, "mouth": pos } });
    f["patch_dirs"] = json!({ "listen": { "E": "SE" } });
    std::fs::write(&path, serde_json::to_string(&f).unwrap()).unwrap();
    assert!(!t.root.join("art/anim_l").exists());
    let p = t.load().unwrap_or_else(|e| panic!("{e}"));
    let face = p.face.as_ref().expect("the face still loads");
    assert!(!face.clips.contains_key("listen"));
    assert_eq!(face.patch_dir("listen", Dir::E), Dir::E, "its patch_dirs went with it");
    assert_eq!(p.resolve(Intent::Listen), p.resolve(Intent::Idle), "listen falls back to rest");

    // nothing optional missing (the wired-but-waiting A4 sources dropped too): an unknown face clip is still an error
    let art = p.art_root.clone();
    t.edit(|v| {
        let srcs: Vec<Value> = v["sources"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|s| !s["path"].as_str().unwrap().contains("anim_l") && !s["path"].as_str().unwrap().contains("anim_d"))
            .filter(|s| art.join(s["path"].as_str().unwrap()).exists())
            .cloned()
            .collect();
        v["sources"] = json!(srcs);
    });
    let e = t.load().expect_err("unknown clip listen");
    assert!(e.to_string().contains("unknown clip listen"), "{e}");
}

#[test]
fn ac6_old_face_v1_loads_with_no_patch_dirs() {
    let t = TempPack::new("face-v1");
    t.add_anim_c();
    t.add_face();
    let p = t.load().unwrap_or_else(|e| panic!("{e}"));
    let f = p.face.as_ref().expect("a v1 face");
    assert!(f.patch_dirs.is_empty());
    assert_eq!(f.patch_dir("idle", Dir::E), Dir::E);
    let l = listen_pack();
    let f = l.face.as_ref().unwrap();
    assert_eq!(f.patch_dir("listen", Dir::E), Dir::SE);
    assert_eq!(f.patch_dir("listen", Dir::SW), Dir::S);
    assert_eq!(f.patch_dir("idle", Dir::E), Dir::E);
}

#[test]
fn ac5_click_and_prompt_outrank_a_started_activity() {
    let mut b = brain_with(bear(), 13);
    do_(&mut b, "read", None, 0);
    run_until(&mut b, 0, 8000);
    assert!(b.playing().is_some_and(|p| !p.pending));
    b.poke(Poke::Click, 8000);
    b.poke(Poke::PromptOpen, 8000);
    assert_eq!(b.playing(), None);
    let shows = run_until(&mut b, 8000, 12_000);
    assert_eq!(shows[0].1.clip, "stand_up");
    assert_eq!(shows.last().unwrap().1.intent, Intent::Listen, "then listens");
}

#[test]
fn priority_a_do_from_the_sent_turn_queues_and_starts_after_idle() {
    let mut b = brain_with(bear(), 7);
    b.poke(Poke::PromptOpen, 0);
    run_until(&mut b, 0, 1000);
    b.poke(Poke::PromptSent, 1000);
    assert_eq!(do_(&mut b, "read", None, 1200).status, Some(Status::Queued));
    b.event(&MascotEvent::Idle, 3000);
    let shows = run_until(&mut b, 3000, 6000);
    assert!(shows.iter().any(|(_, s)| s.clip == "sit_down"));
}

#[test]
fn turning_packs_turn_for_look_around_and_without_turning_nothing_turns() {
    // (a pack without turn clips: the second half below, nothing turns)
    let p = listen_pack();
    let mut b = brain_with(p.clone(), 9);
    let shows = run_until(&mut b, 0, 120_000);
    assert_turns_only(&shows, "idle day");
    assert!(shows.iter().any(|(_, s)| s.behavior == "look_around" && s.clip.starts_with("turn_")), "look_around turns");
    let mut np = (*p).clone();
    np.turning = None;
    let mut b2 = brain_with(Arc::new(np), 9);
    let shows2 = run_until(&mut b2, 0, 120_000);
    assert!(shows2.iter().all(|(_, s)| !s.clip.starts_with("turn_")));
}

// ---- p1 leftovers ----

#[test]
fn leftover_hiding_from_the_tray_ends_the_activity_with_its_exit_step() {
    let mut b = brain_with(bear(), 13);
    do_(&mut b, "read", None, 0);
    run_until(&mut b, 0, 8000);
    b.set_user_hidden(true, 8000);
    assert_eq!(b.playing(), None);
    assert!(activity_notes(&b).iter().any(|n| n == "end read (hidden)"), "{:?}", activity_notes(&b));
    run_until(&mut b, 8000, 12_000);
    b.set_user_hidden(false, 12_000);
    let shows = run_until(&mut b, 12_000, 20_000);
    assert!(shows.iter().all(|(_, s)| s.intent != Intent::Do), "not still reading when shown again");
}

#[test]
fn leftover_a_say_during_a_set_activity_keeps_the_seat() {
    let mut b = brain_with(bear(), 20);
    do_(&mut b, "read", Some(120_000), 0);
    run_until(&mut b, 0, 8000);
    b.event(&MascotEvent::Say { text: "Tea time!".into(), emotion: Some(Emotion::Happy) }, 8000);
    let shows = run_until(&mut b, 8000, 20_000);
    assert!(shows.iter().all(|(_, s)| s.intent == Intent::Do && s.clip == "read"), "{:?}", segments(&shows));
    let e = b.body(&BodyReq::Emote { name: "wave".into() }, 20_000);
    assert_eq!((e.status, e.reason), (Some(Status::Refused), Some(Reason::Busy)));
    let shows = run_until(&mut b, 20_000, 22_000);
    assert!(shows.iter().all(|(_, s)| s.clip == "read"));
}

#[test]
fn leftover_a_repeated_do_only_moves_the_deadline() {
    let mut b = brain_with(bear(), 13);
    do_(&mut b, "read", Some(60_000), 0);
    run_until(&mut b, 0, 8000);
    let until = b.playing().unwrap().until_ms.unwrap();
    let r = do_(&mut b, "read", Some(60_000), 8000);
    assert_eq!(r.status, Some(Status::Started));
    assert_eq!(b.playing().unwrap().until_ms, Some(68_000));
    assert!(until < 68_000);
    let shows = run_until(&mut b, 8000, 20_000);
    assert!(shows.iter().all(|(_, s)| s.clip == "read"), "no restart: {:?}", segments(&shows));
}

/// Stage kit clip stages write root motion twice (`frames[i].root_px[dir]` and the clip's `root_px[dir]`, `#anim-stage`):
/// ONE track. The loader takes the per-frame form once and never adds the clip-level copy.
#[test]
fn a_kit_clip_with_both_root_px_forms_moves_once() {
    let t = TempPack::new("rootpx");
    let dir = t.root.join("art/anim_k");
    std::fs::create_dir_all(&dir).unwrap();
    let dirs = ["S", "SE", "E", "NE", "N", "NW", "W", "SW"];
    let steps: Vec<[f32; 2]> = vec![[1.0, 0.0], [2.0, 1.0], [0.0, -1.0], [0.0, 0.0]];
    let mut files = serde_json::Map::new();
    let mut clip_px = serde_json::Map::new();
    for d in dirs {
        let f = format!("fid_k_{d}.png");
        std::fs::write(dir.join(&f), b"").unwrap();
        files.insert(d.into(), json!(f));
        clip_px.insert(d.into(), json!(steps));
    }
    let frames: Vec<Value> = steps.iter().map(|s| {
        let per: serde_json::Map<String, Value> = dirs.iter().map(|d| (d.to_string(), json!(s))).collect();
        json!({ "dur": 83, "root_world": 0.0, "root_px": per })
    }).collect();
    let anims = json!({ "fid_k": { "loop": false, "fps": 12, "frame_w": 84, "frame_h": 123, "anchor": [42, 107], "frames_n": 4,
        "files": files, "frames": frames, "root_px": clip_px, "root_track": [[0.0, 0.0]], "face": "none", "handoff": {"start": "idle0", "end": "idle0"} } });
    // as make_stage.py writes it: Stage A's `directions` map, plus root_track and the informational keys
    let yaws: serde_json::Map<String, Value> = dirs.iter().enumerate().map(|(i, d)| (d.to_string(), json!({ "yaw_deg": (45 + 45 * i) % 360 }))).collect();
    let j = json!({ "note": "kit", "stage": "k", "fps": 12, "ppu": 40.07, "directions": yaws, "anims": anims });
    std::fs::write(dir.join("anim_k.json"), j.to_string()).unwrap();
    t.edit(|p| p["sources"].as_array_mut().unwrap().push(json!({"kind": "stage_a", "path": "anim_k/anim_k.json"})));
    let p = t.load().unwrap_or_else(|e| panic!("{e}"));
    for d in Dir::ALL {
        assert_eq!(p.clips["fid_k"].dirs[&d].root_px, steps, "{d:?}");
    }
}
