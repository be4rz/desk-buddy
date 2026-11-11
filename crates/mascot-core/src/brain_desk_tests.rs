//! Desk tests (p3 desk-brain, `#flow-desk`): AC5 (priority at the desk), AC8 (when a turn goes to
//! the desk), AC9 (seated for the whole turn), AC10 (answer from the chair, stand on `Settled`), and
//! the failure paths. On `tests/fixtures/desk_pack` (stand-in desk art cut from the read set).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use mascot_protocol::{MascotEvent, WorkKind};

use super::*;
use crate::pack;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn desk_pack() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures/desk_pack")).unwrap_or_else(|e| panic!("{e}")))
}

fn bear() -> Arc<PackMeta> {
    Arc::new(pack::load(&repo().join("mascots/bear")).unwrap_or_else(|e| panic!("{e}")))
}

fn brain(pack: Arc<PackMeta>, seed: u64) -> Brain {
    Brain::new(pack, BrainConfig { seed, temperament: None, dnd_action: DndAction::Hide }, 0)
}

/// Ticks at every wake from `from` to `until`, each show with its time.
fn run(b: &mut Brain, from: u64, until: u64) -> Vec<(u64, Show)> {
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

/// The clips in order, repeats collapsed.
fn clips(shows: &[(u64, Show)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, s) in shows {
        if out.last() != Some(&s.clip) {
            out.push(s.clip.clone());
        }
    }
    out
}

fn first_at(shows: &[(u64, Show)], clip: &str) -> Option<u64> {
    shows.iter().find(|(_, s)| s.clip == clip).map(|(t, _)| *t)
}

fn working(kind: WorkKind) -> MascotEvent {
    MascotEvent::Working { kind, tool: "x".into() }
}

fn speaking(text: &str) -> MascotEvent {
    MascotEvent::Speaking { text: text.into() }
}

fn card() -> MascotEvent {
    MascotEvent::AwaitingPermission { call_id: "c1".into(), title: "Run cargo test?".into() }
}

fn do_computer(b: &mut Brain, t: u64) -> BodyReply {
    b.body(&BodyReq::Do { name: "computer".into(), for_ms: Some(600_000) }, t)
}

/// Feeds `(at, event)` pairs, running the brain between them; all shows up to `until`.
fn script(b: &mut Brain, evs: &[(u64, MascotEvent)], until: u64) -> Vec<(u64, Show)> {
    let mut shows = Vec::new();
    let mut t = 0;
    for (at, ev) in evs {
        shows.extend(run(b, t, *at));
        b.event(ev, *at);
        t = *at;
    }
    shows.extend(run(b, t, until));
    shows
}

/// `#ex-desk` desk_long.jsonl.
fn desk_long() -> Vec<(u64, MascotEvent)> {
    vec![
        (0, MascotEvent::Thinking),
        (1500, working(WorkKind::Files)),
        (4000, MascotEvent::Thinking),
        (6000, working(WorkKind::Shell)),
        (8000, card()),
        (10_000, MascotEvent::Settled),
        // the answer streams in (each delta re-arms the mouth's 400 ms stall)
        (12_000, speaking("All")),
        (12_300, speaking("All tests")),
        (12_600, speaking("All tests pass")),
        (12_900, speaking("All tests pass,")),
        (13_200, speaking("All tests pass, 104")),
        (13_500, speaking("All tests pass, 104 of")),
        (13_800, speaking("All tests pass, 104 of them.")),
        (14_000, MascotEvent::Idle),
        (20_000, MascotEvent::Settled),
    ]
}

fn seated(s: &Show) -> bool {
    s.behavior == "desk" && s.ori.is_some()
}

// ---- loading ----

#[test]
fn the_desk_pack_has_a_playable_desk_and_the_bear_waits_for_its_art() {
    let p = desk_pack();
    assert!(p.available("desk"), "the desk station plays");
    assert_eq!(p.work_station().map(|(n, w)| (n, w.after_ms, w.after_tools)), Some(("desk", 4000, 2)));
    let a = p.body.activity("computer").expect("computer is listed");
    assert_eq!((a.station.as_deref(), a.kind), (Some("desk"), ActivityKind::Work));
    let face = p.face.as_ref().unwrap();
    for clip in ["desk_talk", "desk_ask"] {
        for ori in [Ori::A, Ori::B] {
            assert!(face.set_face(clip, ori).is_some(), "{clip} {ori:?} has a face");
        }
    }
    // the hand-off facings come from anim_d.json (the read set's: SE for A, SW for B)
    let sit = &p.clips["desk_sit"];
    assert_eq!((sit.handoff[&Ori::A].facing, sit.handoff[&Ori::B].facing), (Dir::SE, Dir::SW));

    assert!(crate::body::prompt_text(&p).unwrap().contains("- computer: sit at the desk and use the computer"));

    // declared, but anim_d not there (the desk pack minus its anim_d source): never plays, isn't offered
    let (_t, r) = desk_temp("no-art", drop_anim_d, |_| {});
    let np = r.unwrap_or_else(|e| panic!("{e}"));
    assert!(np.station("desk").is_some() && !np.available("desk"));
    assert!(np.work_station().is_none());
    assert!(np.body.activity("computer").is_none());
    assert!(np.body_issues.is_empty(), "{:?}", np.body_issues);
    assert!(!crate::body::prompt_text(&np).unwrap().contains("computer"));

    // the bear declares the same desk; it plays once desk-art renders anim_d/
    let bear = bear();
    assert!(bear.station("desk").is_some() && bear.body_issues.is_empty(), "{:?}", bear.body_issues);
    let art = bear.available("desk");
    assert_eq!(bear.work_station().is_some(), art);
    assert_eq!(bear.body.activity("computer").is_some(), art);
}

/// The pack.json edit that makes the desk set's optional source missing (not yet rendered).
fn drop_anim_d(p: &mut serde_json::Value) {
    for s in p["sources"].as_array_mut().unwrap() {
        if s["path"].as_str().unwrap().starts_with("anim_d/") {
            assert_eq!(s["optional"], true);
            s["path"] = serde_json::json!("anim_d_not_rendered/anim_d.json");
        }
    }
}

// ---- AC8: when ----

#[test]
fn ac8_ac9_ac10_the_long_job_goes_to_the_desk_and_answers_from_the_chair() {
    for seed in [1, 2, 3, 7] {
        let mut b = brain(desk_pack(), seed);
        let shows = script(&mut b, &desk_long(), 26_000);
        let c = clips(&shows);
        let want = ["desk_sit", "desk_type", "desk_swivel_in", "desk_ask", "desk_swivel_out", "desk_type", "desk_swivel_in", "desk_talk", "desk_stand"];
        let mut it = c.iter();
        for w in want {
            assert!(it.any(|x| x == w), "seed {seed}: {w} missing in order: {c:?}");
        }
        // AC8: 1 counted tool at 1.5 s, then the 4 s mark
        let sit = first_at(&shows, "desk_sit").unwrap();
        assert!((4000..6000).contains(&sit), "seed {seed}: sat at {sit}");
        // AC9: one ori from desk_sit to desk_stand's end, no standing clip between
        let i0 = shows.iter().position(|(_, s)| s.clip == "desk_sit").unwrap();
        let i1 = shows.iter().rposition(|(_, s)| s.clip == "desk_stand").unwrap();
        let ori = shows[i0].1.ori;
        assert!(ori.is_some());
        assert!(shows[i0..=i1].iter().all(|(_, s)| s.ori == ori && s.behavior == "desk"), "seed {seed}: left the seat mid-turn: {c:?}");
        // AC10: the card from the chair, the answer's mouth flaps there, and it stands only after the bubble's Settled
        let ask = first_at(&shows, "desk_ask").unwrap();
        assert!((8000..10_000).contains(&ask), "seed {seed}: ask at {ask}");
        assert!(shows.iter().any(|(_, s)| s.clip == "desk_talk" && s.mouth != MouthState::Closed), "seed {seed}: no mouth on desk_talk");
        let stand = first_at(&shows, "desk_stand").unwrap();
        assert!(stand >= 20_000, "seed {seed}: stood at {stand}, before Settled");
        let talk = first_at(&shows, "desk_talk").unwrap();
        assert!(talk < 16_000, "seed {seed}: talk at {talk}");
        assert!(shows.iter().filter(|(t, _)| (talk..20_000).contains(t)).all(|(_, s)| s.clip == "desk_talk"), "seed {seed}: the talk pose holds after Idle");
        // after it: off the set at its exit spot, idle
        let last = &shows.last().unwrap().1;
        assert_eq!((last.ori, last.intent), (None, Intent::Idle), "seed {seed}");
        let exit = &b.pack.clips["desk_stand"].handoff[&ori.unwrap()];
        assert!(shows.iter().skip(i1 + 1).take(1).all(|(_, s)| s.dir == exit.facing), "stands at the exit hand-off facing");
    }
}

#[test]
fn ac8_a_short_turn_stays_standing() {
    for seed in [1, 2, 3] {
        let mut b = brain(desk_pack(), seed);
        let evs = vec![
            (0, MascotEvent::Thinking),
            (800, working(WorkKind::Shell)),
            (2000, speaking("Done.")),
            (3000, MascotEvent::Idle),
            (5000, MascotEvent::Settled),
        ];
        let shows = script(&mut b, &evs, 12_000);
        assert!(shows.iter().all(|(_, s)| s.behavior != "desk"), "seed {seed}: {:?}", clips(&shows));
        assert!(clips(&shows).contains(&"talk".to_string()), "talks standing as today");
    }
}

#[test]
fn ac8_buddy_and_media_tools_do_not_count() {
    let mut b = brain(desk_pack(), 4);
    let evs = vec![
        (0, MascotEvent::Thinking),
        (300, working(WorkKind::Buddy)),
        (600, working(WorkKind::Media)),
        (900, working(WorkKind::Buddy)),
        (5000, working(WorkKind::Buddy)),
    ];
    let shows = script(&mut b, &evs, 12_000);
    assert!(shows.iter().all(|(_, s)| s.behavior != "desk"), "{:?}", clips(&shows));
}

#[test]
fn ac8_two_counted_tools_go_at_once_and_one_waits_for_four_seconds() {
    let mut b = brain(desk_pack(), 5);
    let shows = script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Vcs))], 6000);
    let sit = first_at(&shows, "desk_sit").expect("two tools: desk");
    assert!(sit < 4000, "sat at {sit}");

    // one tool and nothing after it: the think loop's iteration end after 4 s
    let mut b = brain(desk_pack(), 5);
    let shows = script(&mut b, &[(0, MascotEvent::Thinking), (500, working(WorkKind::Web))], 9000);
    let sit = first_at(&shows, "desk_sit").expect("timer entry");
    assert!(sit >= 4000, "sat at {sit}, before the 4 s mark");
    assert!(shows.iter().filter(|(t, _)| *t < 4000).all(|(_, s)| s.behavior != "desk"));
}

#[test]
fn ac8_a_pack_without_stations_thinks_and_talks_standing() {
    // stations declared, but no anim_d (the bear until desk-art): the same long job stays standing
    let (_t, r) = desk_temp("no-art-long", drop_anim_d, |_| {});
    let mut b = brain(Arc::new(r.unwrap_or_else(|e| panic!("{e}"))), 1);
    let shows = script(&mut b, &desk_long(), 26_000);
    assert!(shows.iter().all(|(_, s)| s.behavior != "desk"), "{:?}", clips(&shows));
    let c = clips(&shows);
    for w in ["think", "ask", "talk"] {
        assert!(c.iter().any(|x| x == w), "{w} in {c:?}");
    }
    // and a pack with no stations block at all
    let mut p = (*desk_pack()).clone();
    p.stations.clear();
    p.work = None;
    let mut b = brain(Arc::new(p), 1);
    let shows = script(&mut b, &desk_long(), 26_000);
    assert!(shows.iter().all(|(_, s)| s.behavior != "desk"));
}

// ---- AC5: priority at the desk ----

#[test]
fn ac5_a_click_while_typing_turns_to_listen_from_the_chair() {
    let mut b = brain(desk_pack(), 2);
    let shows = script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Shell))], 7000);
    assert!(shows.last().is_some_and(|(_, s)| s.clip == "desk_type"), "{:?}", clips(&shows));
    b.poke(Poke::Click, 7000);
    b.poke(Poke::PromptOpen, 7000);
    let shows = run(&mut b, 7000, 12_000);
    let c = clips(&shows);
    assert!(c.starts_with(&["desk_type".into(), "desk_swivel_in".into(), "desk_talk".into()]), "{c:?}");
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_talk" && s.intent == Intent::Listen));
    assert!(shows.iter().all(|(_, s)| seated(s)), "listens from the chair");
    // closed: the run goes on, back to the screen
    b.poke(Poke::PromptClosed, 12_000);
    let shows = run(&mut b, 12_000, 16_000);
    let c = clips(&shows);
    assert!(c.contains(&"desk_swivel_out".into()) && c.last() == Some(&"desk_type".into()), "{c:?}");
}

#[test]
fn ac5_dnd_and_quiet_hours_refuse_the_computer_and_a_click_ends_it() {
    let mut b = brain(desk_pack(), 3);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 0);
    let r = do_computer(&mut b, 100);
    assert_eq!((r.status, r.reason), (Some(Status::Refused), Some(Reason::Dnd)));
    b.senses(&Senses { quiet_hours: true, ..Senses::default() }, 200);
    let r = do_computer(&mut b, 300);
    assert_eq!((r.status, r.reason), (Some(Status::Refused), Some(Reason::QuietHours)));
    b.senses(&Senses::default(), 400);
    let r = do_computer(&mut b, 500);
    assert_eq!(r.status, Some(Status::Started));
    let shows = run(&mut b, 500, 8000);
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_type" && seated(s)), "{:?}", clips(&shows));
    b.poke(Poke::Click, 8000);
    assert_eq!(b.playing(), None, "the click ends it");
    let shows = run(&mut b, 8000, 14_000);
    let c = clips(&shows);
    assert!(c.contains(&"desk_stand".into()), "{c:?}");
    assert_eq!(shows.last().unwrap().1.ori, None);
}

#[test]
fn ac5_a_reply_during_the_computer_talks_from_the_chair_and_a_click_wins() {
    let mut b = brain(desk_pack(), 6);
    assert_eq!(do_computer(&mut b, 0).status, Some(Status::Started));
    run(&mut b, 0, 5000);
    // a turn does not end a work activity; a reply talks from the chair (Talk beats Type)
    b.event(&MascotEvent::Thinking, 5000);
    let shows = run(&mut b, 5000, 7000);
    assert!(shows.iter().all(|(_, s)| seated(s)), "{:?}", clips(&shows));
    assert!(b.playing().is_some(), "the computer keeps going through the turn");
    b.event(&speaking("Here is the answer."), 7000);
    let shows = run(&mut b, 7000, 11_000);
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_talk" && s.intent == Intent::Talk), "{:?}", clips(&shows));
    assert!(shows.iter().all(|(_, s)| seated(s)));
    // the person's click outranks it: the activity ends, the bear listens
    b.poke(Poke::Click, 11_000);
    b.poke(Poke::PromptOpen, 11_000);
    assert_eq!(b.playing(), None);
    let shows = run(&mut b, 11_000, 14_000);
    assert!(shows.iter().any(|(_, s)| s.intent == Intent::Listen), "{:?}", clips(&shows));
    // closed and the turn over (bubble gone): it stands up, no typing again
    b.poke(Poke::PromptClosed, 14_000);
    b.event(&MascotEvent::Idle, 14_000);
    b.event(&MascotEvent::Settled, 14_000);
    let shows = run(&mut b, 14_000, 20_000);
    let c = clips(&shows);
    assert!(c.contains(&"desk_stand".into()) && !c.contains(&"desk_type".into()), "{c:?}");
}

#[test]
fn a_computer_activity_starts_mid_turn_and_after_the_turn_returns_to_typing() {
    let mut b = brain(desk_pack(), 8);
    b.event(&MascotEvent::Thinking, 0);
    let r = do_computer(&mut b, 100);
    assert_eq!(r.status, Some(Status::Started), "a work activity is never queued");
    let shows = run(&mut b, 100, 5000);
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_type"), "{:?}", clips(&shows));
    b.event(&speaking("ok"), 5000);
    b.event(&MascotEvent::Idle, 6000);
    b.event(&MascotEvent::Settled, 9000);
    let shows = run(&mut b, 9000, 14_000);
    let c = clips(&shows);
    assert!(c.contains(&"desk_swivel_out".into()) && c.last() == Some(&"desk_type".into()), "{c:?}");
    assert!(b.playing().is_some());
    // emotes are refused while seated (they would stand it up)
    let e = b.body(&BodyReq::Emote { name: "wave".into() }, 14_000);
    assert_eq!((e.status, e.reason), (Some(Status::Refused), Some(Reason::Busy)));
}

// ---- failure paths ----

#[test]
fn an_error_while_seated_swivels_stands_then_oops() {
    let mut b = brain(desk_pack(), 9);
    let shows = script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Shell))], 7000);
    assert!(shows.last().is_some_and(|(_, s)| s.clip == "desk_type"));
    b.event(&MascotEvent::Error { reason: "model_error".into(), message: "boom".into() }, 7000);
    let shows = run(&mut b, 7000, 14_000);
    let c = clips(&shows);
    let i = |x: &str| c.iter().position(|y| y == x).unwrap_or_else(|| panic!("{x} in {c:?}"));
    assert!(i("desk_swivel_in") < i("desk_stand") && i("desk_stand") < i("oops"), "{c:?}");
}

#[test]
fn no_settled_within_the_cap_stands_up_at_120_s() {
    let mut b = brain(desk_pack(), 10);
    let evs = vec![
        (0, MascotEvent::Thinking),
        (300, working(WorkKind::Files)),
        (600, working(WorkKind::Shell)),
        (6000, speaking("Here.")),
        (8000, MascotEvent::Idle),
    ];
    let shows = script(&mut b, &evs, 8000 + SEAT_HOLD_MS + 6000);
    let stand = first_at(&shows, "desk_stand").expect("the cap stands it up");
    assert!(stand >= 8000 + SEAT_HOLD_MS, "stood at {stand}");
    assert!(stand < 8000 + SEAT_HOLD_MS + 3000, "soon after the cap: {stand}");
    assert!(shows.iter().filter(|(t, _)| (9000..8000 + SEAT_HOLD_MS).contains(t)).all(|(_, s)| s.clip == "desk_talk"));
}

#[test]
fn a_stopped_answer_settles_at_idle_and_the_bear_stands() {
    // ■ on the streaming answer: the bubble goes (Settled) mid-reply, then the turn's Idle comes while
    // the talk clip still plays; the app settles again when no answer shows, so it stands at once
    let mut b = brain(desk_pack(), 4);
    let mut evs: Vec<(u64, MascotEvent)> = desk_long().into_iter().filter(|(t, _)| *t < 13_000).collect();
    evs.push((13_000, MascotEvent::Settled));
    evs.push((13_100, MascotEvent::Idle));
    let mut shows = script(&mut b, &evs, 13_100);
    // what driver.rs does at Idle with no Speech bubble showing
    if b.holds_seat() {
        b.event(&MascotEvent::Settled, 13_100);
    }
    assert!(!b.holds_seat());
    shows.extend(run(&mut b, 13_100, 20_000));
    let stand = first_at(&shows, "desk_stand").expect("stands without a bubble");
    assert!(stand < 17_000, "stood at {stand}: held for a bubble that was gone");
}

#[test]
fn a_thinking_say_after_a_long_turn_does_not_go_to_the_desk() {
    // the tally is the last turn's after Idle: a Say's `thinking` follow-up is no turn
    let mut b = brain(desk_pack(), 5);
    let mut evs = desk_long();
    evs.push((27_000, MascotEvent::Say { text: "hmm".into(), emotion: Some(Emotion::Thinking) }));
    let shows = script(&mut b, &evs, 40_000);
    assert!(shows.iter().filter(|(t, _)| *t >= 27_000).all(|(_, s)| s.behavior != "desk"), "{:?}", clips(&shows));
}

#[test]
fn dnd_hidden_mid_turn_leaves_the_desk_and_it_stays_standing_this_turn() {
    let mut b = brain(desk_pack(), 11);
    let shows = script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Shell))], 7000);
    let ori = shows.last().unwrap().1.ori.expect("seated");
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 7000);
    let s = b.tick(7000);
    assert_eq!((s.visibility, s.ori), (Visibility::Hidden, None));
    assert_eq!(s.dir, b.pack.clips["desk_stand"].handoff[&ori].facing, "at the exit hand-off");
    b.event(&working(WorkKind::Shell), 8000);
    b.senses(&Senses::default(), 9000);
    b.event(&working(WorkKind::Shell), 9500);
    let shows = run(&mut b, 9000, 16_000);
    assert!(shows.iter().all(|(_, s)| s.behavior != "desk" && s.visibility == Visibility::Shown), "{:?}", clips(&shows));
    // the next turn may go again
    b.event(&MascotEvent::Idle, 16_000);
    let shows = script(&mut b, &[(17_000, MascotEvent::Thinking), (17_300, working(WorkKind::Files)), (17_600, working(WorkKind::Shell))], 24_000);
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_type"), "{:?}", clips(&shows));
}

#[test]
fn a_drag_takes_the_bear_off_the_chair_and_the_drop_sits_it_again() {
    let mut b = brain(desk_pack(), 12);
    script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Shell))], 7000);
    b.poke(Poke::DragStart, 7000);
    let s = b.tick(7000);
    assert_eq!((s.behavior.as_str(), s.ori), ("hold", None));
    b.poke(Poke::DragEnd, 8000);
    let shows = run(&mut b, 8000, 14_000);
    assert!(shows.iter().any(|(_, s)| s.clip == "desk_type"), "{:?}", clips(&shows));
}

#[test]
fn still_dnd_keeps_the_turn_at_the_desk() {
    let mut b = Brain::new(desk_pack(), BrainConfig { seed: 14, temperament: None, dnd_action: DndAction::Still }, 0);
    script(&mut b, &[(0, MascotEvent::Thinking), (300, working(WorkKind::Files)), (600, working(WorkKind::Shell))], 7000);
    b.senses(&Senses { dnd_toggle: true, ..Senses::default() }, 7000);
    let shows = run(&mut b, 7000, 10_000);
    assert!(shows.iter().all(|(_, s)| seated(s) && s.visibility == Visibility::Still), "{:?}", clips(&shows));
}

// ---- p2 leftover: the head pop ----

#[test]
fn closing_the_prompt_during_listen_in_lets_it_finish_before_listen_out() {
    let p = Arc::new(pack::load(&repo().join("crates/mascot-core/tests/fixtures/listen_pack")).unwrap_or_else(|e| panic!("{e}")));
    for seed in [1, 2, 3] {
        let mut b = brain(p.clone(), seed);
        b.face(Dir::S, 0);
        b.poke(Poke::PromptOpen, 100);
        let mut t = 100;
        while b.tick(t).clip != "listen_in" {
            t = b.next_wake_ms().max(t + 1);
            assert!(t < 5000, "seed {seed}: listen_in never starts");
        }
        // one frame in, then closed
        t = b.next_wake_ms();
        assert_eq!(b.tick(t).clip, "listen_in");
        b.poke(Poke::PromptClosed, t);
        let n = b.pack.clips["listen_in"].frames_n() as u16;
        let shows = run(&mut b, t, t + 3000);
        let seq: Vec<(String, u16)> = shows.iter().map(|(_, s)| (s.clip.clone(), s.frame)).collect();
        let out = seq.iter().position(|(c, _)| c == "listen_out").unwrap_or_else(|| panic!("seed {seed}: no listen_out: {seq:?}"));
        assert!(out > 0, "seed {seed}: {seq:?}");
        // no head pop: listen_in reaches its last frame before anything else plays
        let first_other = seq.iter().position(|(c, _)| c != "listen_in").unwrap();
        assert_eq!(seq[first_other - 1], ("listen_in".to_string(), n - 1), "seed {seed}: listen_in played to its end: {seq:?}");
        let ln = b.pack.clips["listen"].frames_n() as u16;
        assert!(seq[out - 1] == ("listen_in".to_string(), n - 1) || seq[out - 1] == ("listen".to_string(), ln - 1), "seed {seed}: {seq:?}");
        assert_eq!(seq[out].1, 0);
    }
}

// ---- the loader on bad desk wiring ----

/// desk_pack rewritten into a temp dir (absolute art paths), edited by `f` (pack.json) and `g` (face.json).
struct DeskTemp(PathBuf);

impl Drop for DeskTemp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn desk_temp(tag: &str, f: impl FnOnce(&mut serde_json::Value), g: impl FnOnce(&mut serde_json::Value)) -> (DeskTemp, Result<PackMeta, pack::PackError>) {
    use serde_json::{json, Value};
    let src = repo().join("crates/mascot-core/tests/fixtures/desk_pack").canonicalize().unwrap();
    let root = std::env::temp_dir().join(format!("mascot-desk-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let t = DeskTemp(root.clone());
    let mut p: Value = serde_json::from_str(&std::fs::read_to_string(src.join("pack.json")).unwrap()).unwrap();
    p["art_root"] = json!(src);
    let pals: Vec<Value> = p["palettes"].as_array().unwrap().iter().map(|v| json!(src.join(v.as_str().unwrap()))).collect();
    p["palettes"] = json!(pals);
    let mut face: Value = serde_json::from_str(&std::fs::read_to_string(src.join("face/face.json")).unwrap()).unwrap();
    for slot in face["layers"].as_object_mut().unwrap().values_mut() {
        for state in slot.as_object_mut().unwrap().values_mut() {
            for layer in state.as_object_mut().unwrap().values_mut() {
                let abs = src.join("face").join(layer["file"].as_str().unwrap());
                layer["file"] = json!(abs);
            }
        }
    }
    g(&mut face);
    std::fs::write(root.join("face.json"), face.to_string()).unwrap();
    for s in p["sources"].as_array_mut().unwrap() {
        if s["kind"] == "face" {
            s["path"] = json!(root.join("face.json"));
        }
    }
    f(&mut p);
    std::fs::write(root.join("pack.json"), p.to_string()).unwrap();
    let r = pack::load(&root);
    (t, r)
}

fn load_err(tag: &str, f: impl FnOnce(&mut serde_json::Value), g: impl FnOnce(&mut serde_json::Value)) -> String {
    let (_t, r) = desk_temp(tag, f, g);
    match r {
        Ok(_) => panic!("{tag}: loaded"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn the_temp_copy_of_the_desk_pack_loads() {
    let (_t, r) = desk_temp("ok", |_| {}, |_| {});
    assert!(r.unwrap_or_else(|e| panic!("{e}")).available("desk"));
}

#[test]
fn bad_desk_wiring_is_refused_with_a_reason() {
    use serde_json::json;
    let cases: Vec<(&str, Box<dyn FnOnce(&mut serde_json::Value)>, &str)> = vec![
        ("missing-clip", Box::new(|p| p["stations"]["desk"]["talk"] = json!("desk_nope")), "stations: desk: talk clip desk_nope is missing"),
        ("free-clip", Box::new(|p| p["stations"]["desk"]["loop"] = json!("think")), "stations: desk: loop clip think is not in set desk"),
        ("no-enter-handoff", Box::new(|p| p["stations"]["desk"]["enter"] = json!("desk_type")), "enter clip desk_type needs a hand-off at start"),
        ("exit-at-start", Box::new(|p| p["stations"]["desk"]["exit"] = json!("desk_sit")), "exit clip desk_sit needs a hand-off at end"),
        ("name-clash", Box::new(|p| { let d = p["stations"]["desk"].take(); p["stations"] = json!({ "rest": d }); p["work"]["station"] = json!("rest"); }), "stations: rest: a behavior has the same name"),
        ("unknown-work", Box::new(|p| p["work"]["station"] = json!("garage")), "work: unknown station garage"),
        ("zero-tools", Box::new(|p| p["work"]["after_tools"] = json!(0)), "work: after_tools must be at least 1"),
        ("unknown-key", Box::new(|p| p["stations"]["desk"]["sleep"] = json!("desk_type")), "sleep"),
    ];
    for (tag, f, want) in cases {
        let e = load_err(tag, f, |_| {});
        assert!(e.contains(want), "{tag}: {e}");
    }
}

#[test]
fn bad_set_faces_are_refused_with_a_reason() {
    use serde_json::json;
    let cases: Vec<(&str, Box<dyn FnOnce(&mut serde_json::Value)>, &str)> = vec![
        ("free-clip-face", Box::new(|f| { let v = f["set_clips"]["desk_talk"].clone(); f["set_clips"]["think"] = v; }), "set_clips: think is not a set clip"),
        ("short", Box::new(|f| { f["set_clips"]["desk_talk"]["A"]["mouth"].as_array_mut().unwrap().pop(); }), "set_clips: desk_talk A"),
        ("bad-patch", Box::new(|f| f["set_clips"]["desk_ask"]["B"]["patch"] = json!("N")), "set_clips: desk_ask B"),
        ("v1", Box::new(|f| { f["face_version"] = json!(1); f.as_object_mut().unwrap().remove("patch_dirs"); f["clips"].as_object_mut().unwrap().remove("listen"); }), "set_clips needs face_version 2"),
    ];
    for (tag, g, want) in cases {
        let e = load_err(tag, |_| {}, g);
        assert!(e.contains(want), "{tag}: {e}");
    }
}

#[test]
fn a_computer_activity_naming_an_unknown_station_fails_the_app_and_the_studio_reports_it() {
    use serde_json::json;
    let e = load_err("act", |p| p["body"]["activities"]["computer"]["station"] = json!("garage"), |_| {});
    assert!(e.contains("activities.computer: unknown station garage"), "{e}");
    // load_report (studio): the art still loads, the activity is dropped and reported
    let (t, _) = desk_temp("act2", |p| p["body"]["activities"]["computer"]["station"] = json!("garage"), |_| {});
    let p = pack::load_report(&t.0).unwrap_or_else(|e| panic!("{e}"));
    assert!(p.body.activity("computer").is_none() && p.available("desk"));
    assert!(p.body_issues.iter().any(|i| i.contains("unknown station garage")), "{:?}", p.body_issues);
}
