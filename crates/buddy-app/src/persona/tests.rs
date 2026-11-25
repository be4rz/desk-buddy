use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::*;
use crate::settings;

fn bear() -> PackMeta {
    mascot_core::pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../mascots/bear")).unwrap()
}

/// A pack with no persona block.
fn plain() -> PackMeta {
    mascot_core::pack::load(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../mascot-core/tests/fixtures/listen_pack")).unwrap()
}

fn home(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("buddy-persona-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn file(h: &Path) -> String {
    std::fs::read_to_string(settings::path(h)).unwrap_or_default()
}

/// A saved settings.json, its bytes, and the in-memory settings.
fn saved(name: &str, user: UserPersona) -> (PathBuf, Settings, String) {
    let h = home(name);
    let mut s = Settings::default();
    s.persona.user = user;
    s.save(&h).unwrap();
    let bytes = file(&h);
    (h, s, bytes)
}

fn set(pack: &PackMeta, s: &mut Settings, h: &Path, p: Value) -> Result<(Value, bool), String> {
    run(pack, "Polar bear", s, h, Op::Set(p))
}

#[test]
fn the_view_lists_the_bears_presets_in_order_with_their_gestures() {
    let pack = bear();
    assert_eq!(pack_name(&pack), "Polar bear");
    let v = view(&pack, "Polar bear", &UserPersona::default());
    assert_eq!(v["pack"], json!({ "id": "bear", "name": "Polar bear" }));
    assert_eq!(v["persona"]["default"], "biscuit");
    let ids: Vec<&str> = v["persona"]["presets"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["biscuit", "mochi", "pip", "tofu"]);
    let b = &v["persona"]["presets"][0];
    assert_eq!((b["name"].as_str(), b["tagline"].as_str(), b["catchphrase"].as_str()), (Some("Biscuit"), Some("cheeky sidekick"), Some("told you so")));
    assert_eq!((b["chattiness"].as_f64(), b["playfulness"].as_f64(), b["emotes"].as_f64()), (Some(0.5), Some(0.7), Some(0.5)));
    assert_eq!(b["gestures"]["greeting"]["emote"], "wave");
    assert!(!b["gestures"]["greeting"]["about"].as_str().unwrap().is_empty());
    assert!(b["examples"]["greeting"].as_str().unwrap().contains("{you}"), "the page fills them");
    assert_eq!(v["caps"], json!({ "name": 24, "call_me": 24, "catchphrase": 40, "notes": 500 }));
    assert_eq!(v["missing"], Value::Null);
    assert!(v["user"].as_object().unwrap().values().all(Value::is_null));
}

#[test]
fn notes_over_the_cap_are_refused_with_the_pages_words_and_nothing_changes() {
    let pack = bear();
    let (h, mut s, bytes) = saved("notes", UserPersona { call_me: Some("Sam".into()), ..Default::default() });
    let before = s.clone();
    for (n, want) in [(501, "notes: 501 characters, the limit is 500"), (540, "notes: 540 characters, the limit is 500")] {
        let e = set(&pack, &mut s, &h, json!({ "notes": "x".repeat(n), "call_me": "Alex" })).unwrap_err();
        assert_eq!(e, want);
        assert_eq!(s, before, "nothing in memory");
        assert_eq!(file(&h), bytes, "nothing on disk");
    }
    // characters, not bytes: 500 é fit
    let (v, changed) = set(&pack, &mut s, &h, json!({ "notes": "é".repeat(500) })).unwrap();
    assert!(changed);
    assert_eq!(v["user"]["notes"].as_str().unwrap().chars().count(), 500);
}

#[test]
fn bad_patches_are_refused_whole() {
    let pack = bear();
    let (h, mut s, bytes) = saved("bad", UserPersona::default());
    let cases = [
        (json!({ "colour": "red" }), "unknown field “colour”"),
        (json!({ "name": "Bun", "x": 1 }), "unknown field “x”"),
        (json!({ "chattiness": 1.5 }), "chattiness: must be between 0 and 1"),
        (json!({ "playfulness": -0.1 }), "playfulness: must be between 0 and 1"),
        (json!({ "emotes": "often" }), "emotes: must be a number or null"),
        (json!({ "name": "n".repeat(25) }), "name: 25 characters, the limit is 24"),
        (json!({ "call_me": "c".repeat(25) }), "call_me: 25 characters, the limit is 24"),
        (json!({ "catchphrase": "c".repeat(41) }), "catchphrase: 41 characters, the limit is 40"),
        (json!({ "preset": "sprout" }), "no preset “sprout” in this mascot"),
        (json!({ "name": 3 }), "name: must be text or null"),
        (json!(["name"]), "the change must be an object"),
    ];
    for (p, want) in cases {
        assert_eq!(set(&pack, &mut s, &h, p.clone()).unwrap_err(), want, "{p}");
    }
    assert_eq!(s.persona.user, UserPersona::default());
    assert_eq!(file(&h), bytes);
}

#[test]
fn a_set_is_written_and_a_preset_value_is_stored_as_null() {
    let pack = bear();
    let (h, mut s, _) = saved("set", UserPersona::default());
    let (v, changed) = set(&pack, &mut s, &h, json!({ "name": "  Bun ", "call_me": "Sam", "chattiness": 0.2, "playfulness": 0.7, "catchphrase": "told you so" })).unwrap();
    assert!(changed);
    // 0.7 is Biscuit's own playfulness and "told you so" its own catchphrase: they follow the preset
    assert_eq!(v["user"], json!({ "preset": null, "name": "Bun", "call_me": "Sam", "catchphrase": null, "chattiness": 0.2, "playfulness": null, "emotes": null, "notes": null }));
    let (back, note) = settings::load(&h);
    assert_eq!(note, None);
    assert_eq!(back.persona.user, s.persona.user, "round trip");
    // emptying a field follows the preset again; the same patch twice changes nothing
    let (_, changed) = set(&pack, &mut s, &h, json!({ "name": "" })).unwrap();
    assert!(changed);
    assert_eq!(s.persona.user.name, None);
    assert!(!set(&pack, &mut s, &h, json!({ "name": "" })).unwrap().1);
}

#[test]
fn a_pick_clears_the_preset_fields_keeps_the_persons_and_offers_undo() {
    let pack = bear();
    let mine = UserPersona { name: Some("Bun".into()), call_me: Some("Sam".into()), chattiness: Some(0.1), notes: Some("No puns.".into()), ..Default::default() };
    let (h, mut s, bytes) = saved("pick", mine.clone());
    assert_eq!(run(&pack, "Polar bear", &mut s, &h, Op::Pick("sprout".into())).unwrap_err(), "no preset “sprout” in this mascot");
    assert_eq!(file(&h), bytes);
    let (v, changed) = run(&pack, "Polar bear", &mut s, &h, Op::Pick("mochi".into())).unwrap();
    assert!(changed);
    assert_eq!(v["undo"], json!(mine));
    let u = &s.persona.user;
    assert_eq!((u.preset.as_deref(), u.name.as_deref(), u.chattiness), (Some("mochi"), None, None));
    assert_eq!((u.call_me.as_deref(), u.notes.as_deref()), (Some("Sam"), Some("No puns.")));
    // the page's Undo: the old layer back in one set
    let mut undo = Map::new();
    for k in ["preset", "name", "catchphrase", "chattiness", "playfulness", "emotes"] {
        undo.insert(k.into(), v["undo"][k].clone());
    }
    set(&pack, &mut s, &h, Value::Object(undo)).unwrap();
    assert_eq!(s.persona.user, mine);
}

#[test]
fn undo_of_a_pick_away_from_a_missing_preset_brings_it_back() {
    let pack = bear();
    let mine = UserPersona { preset: Some("sprout".into()), name: Some("Bun".into()), ..Default::default() };
    let (h, mut s, _) = saved("undo-missing", mine.clone());
    let (v, _) = run(&pack, "Polar bear", &mut s, &h, Op::Pick("mochi".into())).unwrap();
    let mut undo = Map::new();
    for k in ["preset", "name", "catchphrase", "chattiness", "playfulness", "emotes"] {
        undo.insert(k.into(), v["undo"][k].clone());
    }
    // a plain set still refuses an id the pack lacks; the page's Undo takes it back
    assert_eq!(set(&pack, &mut s, &h, Value::Object(undo.clone())).unwrap_err(), "no preset “sprout” in this mascot");
    let (v, changed) = run(&pack, "Polar bear", &mut s, &h, Op::Undo(Value::Object(undo))).unwrap();
    assert!(changed);
    assert_eq!(s.persona.user, mine);
    assert_eq!(v["missing"], "sprout");
}

#[test]
fn reset_goes_back_to_the_preset_as_written() {
    let pack = bear();
    let (h, mut s, _) = saved("reset", UserPersona { preset: Some("pip".into()), name: Some("Zip".into()), call_me: Some("Sam".into()), emotes: Some(0.1), notes: Some("Hi".into()), ..Default::default() });
    let (v, changed) = run(&pack, "Polar bear", &mut s, &h, Op::Reset).unwrap();
    assert!(changed);
    assert_eq!(v["user"], json!({ "preset": "pip", "name": null, "call_me": "Sam", "catchphrase": null, "chattiness": null, "playfulness": null, "emotes": null, "notes": "Hi" }));
    assert!(v.get("undo").is_none());
    assert!(!run(&pack, "Polar bear", &mut s, &h, Op::Reset).unwrap().1, "nothing left to reset");
}

#[test]
fn a_write_error_is_refused_and_nothing_changes() {
    let pack = bear();
    let h = home("ro");
    // a file where the home folder should be: create_dir_all fails
    let not_dir = h.join("home-is-a-file");
    std::fs::write(&not_dir, "x").unwrap();
    let mut s = Settings::default();
    let e = set(&pack, &mut s, &not_dir, json!({ "call_me": "Sam" })).unwrap_err();
    assert!(e.starts_with("couldn't write "), "{e}");
    assert_eq!(s, Settings::default());
    let e = run(&pack, "Polar bear", &mut s, &not_dir, Op::Pick("tofu".into())).unwrap_err();
    assert!(e.starts_with("couldn't write "), "{e}");
    assert_eq!(s, Settings::default());
}

#[test]
fn an_unknown_preset_in_the_settings_runs_the_default_and_says_so() {
    let pack = bear();
    let u = UserPersona { preset: Some("sprout".into()), ..Default::default() };
    let v = view(&pack, "Polar bear", &u);
    assert_eq!(v["missing"], "sprout");
    let t = tray(&pack, &u);
    assert_eq!(t.title, "Personality: Biscuit");
    assert!(t.items.iter().all(|(_, _, on)| !on), "no preset is checked");
    assert_eq!(effects(&pack, &u), effects(&pack, &UserPersona::default()));
}

#[test]
fn the_tray_lists_the_presets_and_its_title_follows_the_layer() {
    let pack = bear();
    let t = tray(&pack, &UserPersona::default());
    assert_eq!(t.title, "Personality: Biscuit");
    let items: Vec<(&str, &str, bool)> = t.items.iter().map(|(i, l, on)| (i.as_str(), l.as_str(), *on)).collect();
    assert_eq!(
        items,
        [
            ("persona:biscuit", "Biscuit · cheeky sidekick", true),
            ("persona:mochi", "Mochi · cozy homebody", false),
            ("persona:pip", "Pip · curious explorer", false),
            ("persona:tofu", "Tofu · shy, earnest helper", false),
        ]
    );
    let mochi = UserPersona { preset: Some("mochi".into()), ..Default::default() };
    assert_eq!(tray(&pack, &mochi).title, "Personality: Mochi");
    assert!(tray(&pack, &mochi).items[1].2);
    assert_eq!(tray(&pack, &UserPersona { chattiness: Some(0.9), ..Default::default() }).title, "Personality: Biscuit (customized)");
    assert_eq!(tray(&pack, &UserPersona { name: Some("Bun".into()), chattiness: Some(0.9), ..Default::default() }).title, "Personality: Bun");
    // the person's own fields are not a customization of the preset
    assert_eq!(tray(&pack, &UserPersona { call_me: Some("Sam".into()), notes: Some("x".into()), ..Default::default() }).title, "Personality: Biscuit");
}

#[test]
fn a_pack_without_a_persona_has_an_empty_view_and_refuses_a_pick() {
    let pack = plain();
    assert!(pack.persona.is_none());
    let v = view(&pack, &pack_name(&pack), &UserPersona::default());
    assert_eq!(v["persona"], Value::Null);
    assert_eq!(v["missing"], Value::Null);
    let t = tray(&pack, &UserPersona::default());
    assert_eq!((t.title.as_str(), t.items.len()), ("Personality", 0));
    let (h, mut s, bytes) = saved("plain", UserPersona::default());
    assert_eq!(run(&pack, "x", &mut s, &h, Op::Pick("biscuit".into())).unwrap_err(), "no preset “biscuit” in this mascot");
    assert_eq!(file(&h), bytes);
    let fx = effects(&pack, &UserPersona::default());
    assert_eq!((fx.temperament, fx.prompt), (pack.temperament, None));
}

#[test]
fn a_change_reaches_the_temperament_and_the_prompt() {
    let pack = bear();
    let (h, mut s, _) = saved("fx", UserPersona::default());
    let start = effects(&pack, &s.persona.user);
    assert_eq!(start.temperament.chattiness, 0.5);
    assert!(start.prompt.as_deref().unwrap().contains("Your name is Biscuit."));
    set(&pack, &mut s, &h, json!({ "chattiness": 0.1, "name": "Bun" })).unwrap();
    let fx = effects(&pack, &s.persona.user);
    assert_eq!(fx.temperament.chattiness, 0.1);
    assert_eq!((fx.temperament.energy, fx.temperament.shyness), (start.temperament.energy, start.temperament.shyness));
    let p = fx.prompt.unwrap();
    assert!(p.contains("Your name is Bun.") && p.contains("Reply length: brief."), "{p}");
    run(&pack, "Polar bear", &mut s, &h, Op::Pick("tofu".into())).unwrap();
    let fx = effects(&pack, &s.persona.user);
    assert_eq!((fx.temperament.shyness, fx.temperament.chattiness), (0.7, 0.3));
    assert!(fx.prompt.unwrap().contains("Your name is Tofu."));
}

#[test]
fn the_new_section_reaches_the_agent_for_its_next_request_and_start() {
    use crate::harness::{Config, Harness};
    let h = home("agent");
    let wanted = settings::Model::default();
    let harness: Arc<Harness> = Harness::build(&h, &wanted, Config::memory(bridge::store::MemoryStore::new()), None).unwrap();
    let pack = bear();
    let u = UserPersona { preset: Some("pip".into()), call_me: Some("Sam".into()), ..Default::default() };
    *LATEST.lock().unwrap() = Some(effects(&pack, &u).prompt);
    push_latest(harness.agent());
    let got = harness.agent().persona().unwrap();
    assert!(got.contains("Your name is Pip.") && got.contains("called Sam"), "{got}");
    // nothing pending: a second push sends nothing and keeps the last
    push_latest(harness.agent());
    assert_eq!(harness.agent().persona().as_deref(), Some(got.as_str()));
    harness.shutdown();
    let _ = std::fs::remove_dir_all(&h);
}

#[test]
fn only_the_persona_window_may_call() {
    assert!(crate::caller::is("persona", window::LABEL, "persona_set"));
    for other in ["bubble", "signin", "studio", "widget-media", ""] {
        assert!(!crate::caller::is(other, window::LABEL, "persona_set"), "{other}");
    }
}

#[test]
fn the_log_line_never_carries_the_persons_words() {
    let pack = bear();
    let u = UserPersona { name: Some("Secret Name".into()), call_me: Some("Private".into()), notes: Some("my diary".into()), ..Default::default() };
    let l = log_line(&pack, &u);
    assert_eq!(l, "persona: biscuit (customized) + notes");
}

#[test]
fn the_window_shows_the_prompts_slider_words_and_bands() {
    let js = include_str!("../../web/persona/persona.js");
    for (slider, words) in persona::SLIDER_WORDS {
        for (w, about) in words {
            assert!(js.contains(&format!("[\"{w}\", \"{about}\"]")), "{slider}: {w} · {about} is not the page's");
        }
    }
    let edges = format!("v <= {} ? 0 : v >= {} ? 2 : 1", persona::BAND_LOW, persona::BAND_HIGH);
    assert!(js.contains(&edges), "the page's band() is not `{edges}`");
}
