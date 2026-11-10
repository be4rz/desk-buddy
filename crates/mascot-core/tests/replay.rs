//! Replay (AC2): deterministic logs, the golden fixture and the CLI's exits.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use mascot_core::{pack, replay};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures").join(name)
}

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mascot-replay")).current_dir(repo()).args(args).output().unwrap()
}

const DAY: &str = "crates/mascot-core/tests/fixtures/day.json";

#[test]
fn library_run_matches_the_golden_log() {
    let p = Arc::new(pack::load(&repo().join("mascots/bear")).unwrap());
    let mut tl = replay::parse(&std::fs::read_to_string(fixture("day.json")).unwrap()).unwrap();
    tl.seed = 7;
    let got = replay::to_jsonl(&replay::run(p.clone(), &tl));
    assert_eq!(got, std::fs::read_to_string(fixture("day.seed7.jsonl")).unwrap());
    let behaviors: std::collections::BTreeSet<String> = replay::run(p, &tl).into_iter().map(|l| l.behavior).collect();
    for b in ["rest", "look_around", "wander", "dash", "nap", "read", "hold"] {
        assert!(behaviors.contains(b), "the day shows {b}");
    }
}

#[test]
fn cli_is_byte_identical_per_seed() {
    let args = ["--pack", "mascots/bear", "--timeline", DAY, "--seed", "7"];
    let a = cli(&args);
    let b = cli(&args);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stderr));
    assert_eq!(a.stdout, b.stdout);
    assert_eq!(a.stdout, std::fs::read(fixture("day.seed7.jsonl")).unwrap());
    let other = cli(&["--pack", "mascots/bear", "--timeline", DAY, "--seed", "8"]);
    assert!(other.status.success());
    assert_ne!(other.stdout, a.stdout);
    // Without --seed the timeline's own seed (1) is used.
    let own = cli(&["--pack", "mascots/bear", "--timeline", DAY]);
    assert!(own.status.success());
    assert_ne!(own.stdout, a.stdout);
}

fn scratch(name: &str, text: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("mascot-replay-{}-{name}", std::process::id()));
    std::fs::write(&p, text).unwrap();
    p
}

#[test]
fn unsupported_timeline_version_exits_1() {
    let p = scratch("v9.json", r#"{"timeline_version":9,"seed":1,"duration_ms":1000,"start_local":"09:00","weekday":0,"items":[]}"#);
    let out = cli(&["--pack", "mascots/bear", "--timeline", p.to_str().unwrap()]);
    let _ = std::fs::remove_file(&p);
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("timeline_version 9 is not supported"), "{err}");
    assert_eq!(err.trim_end().lines().count(), 1);
    assert!(out.stdout.is_empty());
}

#[test]
fn bad_timelines_are_rejected() {
    for (name, text, want) in [
        ("json", "{ nope", "bad JSON"),
        ("order", r#"{"timeline_version":1,"seed":1,"duration_ms":9000,"start_local":"09:00","weekday":0,"items":[{"at_ms":5000},{"at_ms":10}]}"#, "sorted"),
        ("past", r#"{"timeline_version":1,"seed":1,"duration_ms":10,"start_local":"09:00","weekday":0,"items":[{"at_ms":50}]}"#, "past duration_ms"),
        ("time", r#"{"timeline_version":1,"seed":1,"duration_ms":10,"start_local":"9am","weekday":0,"items":[]}"#, "HH:MM"),
        ("event", r#"{"timeline_version":1,"seed":1,"duration_ms":10,"start_local":"09:00","weekday":0,"items":[{"at_ms":1,"event":{"type":"dance"}}]}"#, "bad timeline"),
        ("typo", r#"{"timeline_version":1,"seed":1,"duration_ms":10,"start_local":"09:00","weekday":0,"items":[{"at_ms":1,"evnt":{"type":"thinking"}}]}"#, "unknown field `evnt`"),
        ("top", r#"{"timeline_version":1,"seed":1,"duration_ms":10,"start_local":"09:00","weekday":0,"itemz":[],"items":[]}"#, "unknown field `itemz`"),
    ] {
        let err = replay::parse(text).unwrap_err();
        assert!(err.contains(want), "{name}: {err}");
    }
}

#[test]
fn bad_pack_exits_1_and_usage_exits_2() {
    let out = cli(&["--pack", "/nonexistent", "--timeline", DAY]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&out.stderr).trim_end(), "mascot-replay: pack /nonexistent: pack.json: not found");
    assert_eq!(cli(&[]).status.code(), Some(2));
    assert_eq!(cli(&["--pack", "mascots/bear", "--timeline", DAY, "--seed", "x"]).status.code(), Some(2));
    assert_eq!(cli(&["--bogus"]).status.code(), Some(2));
    let missing = cli(&["--pack", "mascots/bear", "--timeline", "/nonexistent.json"]);
    assert_eq!(missing.status.code(), Some(1));
}
