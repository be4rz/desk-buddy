//! The loopback embedder end to end (A16, A17) against a stand-in Ollama (`common/ollama.rs`): no real model in CI.
//! The real-model check is `real_ollama_finds_the_talk_by_meaning` (ignored; needs `ollama pull nomic-embed-text`).
mod common;
use common::ollama::{closed_url, Cfg, Stub};
use common::*;
use memory::embed::{Embedder, DEFAULT_MODEL, DEFAULT_URL};
use memory::Store;
use std::time::{Duration, Instant};

const TALK: &str = "Stressed about Friday's talk at the meetup; practised the intro twice";
const CALM: &str = "How do I calm down before speaking?";
const T: Duration = Duration::from_secs(2);

fn fixture() -> (Tmp, Store) {
    let (t, s) = store();
    episode(&s, date(2026, 11, 20), TALK, 7);
    episode(&s, date(2026, 11, 25), "Cooked noodles for lunch and fixed a bike tyre", 4);
    episode(&s, date(2026, 11, 26), "Fixed the login bug at Lanternfish Labs", 6);
    episode(&s, date(2026, 11, 26), "Walked to the lake with Lan", 5);
    (t, s)
}

/// What the jobs thread does when the probe says on: mark the model, embed what's waiting, store it.
fn backfill(s: &Store, e: &Embedder) -> usize {
    s.set_embed_model(e.model()).unwrap();
    let todo = s.vectors_todo(32).unwrap();
    let texts: Vec<String> = todo.iter().map(|(_, t)| t.clone()).collect();
    let vecs = e.documents(&texts, Duration::from_secs(10)).unwrap();
    s.put_vectors(e.model(), &todo.into_iter().zip(vecs).map(|((id, t), v)| (id, t, v)).collect::<Vec<_>>()).unwrap()
}

/// What `recall_for` does: the message's embedding (if any), then hybrid recall.
fn recall(s: &Store, e: &Embedder, msg: &str) -> Vec<String> {
    let q = e.query(msg, Duration::from_millis(500)).ok();
    s.recall_with(msg, &now(), q.as_deref()).unwrap()
}

#[test]
fn a16_a_message_with_no_shared_word_recalls_the_talk_through_local_embeddings() {
    let stub = Stub::start(Cfg::default());
    let e = Embedder::new(&stub.url, DEFAULT_MODEL).unwrap();
    assert!(e.probe(T), "the model is listed (as name:latest)");
    let (_t, s) = fixture();
    assert_eq!(backfill(&s, &e), 4);
    assert!(s.recall(CALM, &now()).unwrap().is_empty(), "keywords alone find nothing");
    assert_eq!(recall(&s, &e, CALM), [format!("2026-11-20: {TALK}")]);
    assert!(recall(&s, &e, "Which train goes to the airport?").is_empty(), "an unrelated message brings nothing");
    // nomic models get their task prefixes; documents first, then the queries
    let inputs = stub.inputs.lock().unwrap().clone();
    assert_eq!(inputs.len(), 6);
    assert!(inputs[..4].iter().all(|i| i.starts_with("search_document: ")), "{inputs:?}");
    assert_eq!(inputs[4], format!("search_query: {CALM}"));
}

#[test]
fn a17_no_ollama_means_keyword_recall_and_nothing_sent() {
    let e = Embedder::new(&closed_url(), DEFAULT_MODEL).unwrap();
    let t0 = Instant::now();
    assert!(!e.probe(T));
    assert!(e.query(CALM, Duration::from_millis(500)).is_err());
    assert!(t0.elapsed() < Duration::from_secs(2), "a refused connection fails fast");
    let (_t, s) = fixture();
    // p2 recall exactly: the keyword match, and nothing for a meaning-only message
    assert_eq!(recall(&s, &e, "I'm nervous about speaking at the meetup again"), [format!("2026-11-20: {TALK}")]);
    assert!(recall(&s, &e, CALM).is_empty());
}

#[test]
fn a17_a_server_without_the_model_is_off() {
    let stub = Stub::start(Cfg { models: Some(vec!["some-other-model:latest".into()]), ..Cfg::default() });
    assert!(!Embedder::new(&stub.url, DEFAULT_MODEL).unwrap().probe(T));
    assert!(Embedder::new(&stub.url, "some-other-model").unwrap().probe(T), "name:latest counts");
    assert!(!Embedder::new(&stub.url, "some-other-model:7b").unwrap().probe(T), "a tag must match exactly");
}

#[test]
fn a17_a_slow_ollama_is_given_up_on_and_recall_goes_on() {
    let stub = Stub::start(Cfg { delay: Duration::from_secs(2), ..Cfg::default() });
    let e = Embedder::new(&stub.url, DEFAULT_MODEL).unwrap();
    let (_t, s) = fixture();
    let t0 = Instant::now();
    let got = recall(&s, &e, "I'm nervous about speaking at the meetup again");
    assert!(t0.elapsed() < Duration::from_millis(1500), "the 500 ms query budget held: {:?}", t0.elapsed());
    assert_eq!(got, [format!("2026-11-20: {TALK}")], "keyword recall still answers");
}

#[test]
fn a17_a_wrong_sized_reply_stores_nothing() {
    let stub = Stub::start(Cfg { dims: Some(384), ..Cfg::default() });
    let e = Embedder::new(&stub.url, DEFAULT_MODEL).unwrap();
    assert_eq!(e.query(CALM, T), Err("dims"));
    assert_eq!(e.documents(&[TALK.to_string()], T).map(|v| v.len()), Err("dims"));
}

#[test]
fn a17_only_http_on_loopback_is_accepted() {
    for bad in [
        "https://127.0.0.1:11434",
        "http://192.0.2.10:11434",
        "http://ollama.example.com:11434",
        "http://127.0.0.1.example.com:11434",
        "http://me@127.0.0.1:11434",
        "http://127.0.0.1:11434/v1",
    ] {
        assert!(Embedder::new(bad, DEFAULT_MODEL).is_err(), "{bad}");
    }
    assert!(Embedder::new(DEFAULT_URL, DEFAULT_MODEL).is_ok());
    // `localhost` is resolved to loopback addresses only, and reaches the stub
    let stub = Stub::start(Cfg::default());
    let port = stub.url.rsplit(':').next().unwrap();
    let e = Embedder::new(&format!("http://localhost:{port}"), DEFAULT_MODEL).unwrap();
    assert!(e.probe(T));
    assert!(stub.calls() >= 1);
}

/// A16 with a real model (the human's desktop check): `ollama serve` + `ollama pull nomic-embed-text`, then
/// `cargo test -p memory --test embed_local -- --ignored`.
#[test]
#[ignore]
fn real_ollama_finds_the_talk_by_meaning() {
    let e = Embedder::new(DEFAULT_URL, DEFAULT_MODEL).unwrap();
    assert!(e.probe(T), "Ollama on 127.0.0.1:11434 with {DEFAULT_MODEL} pulled");
    let (_t, s) = fixture();
    assert_eq!(backfill(&s, &e), 4);
    let q = e.query(CALM, Duration::from_secs(5)).unwrap();
    let cands = s.recall_candidates(CALM, &now(), Some(&q)).unwrap();
    eprintln!("candidates: {:?}", cands.iter().map(|c| (c.id, (c.score * 1000.0).round() / 1000.0)).collect::<Vec<_>>());
    assert_eq!(s.recall_with(CALM, &now(), Some(&q)).unwrap().first(), Some(&format!("2026-11-20: {TALK}")));
    let q = e.query("Which train goes to the airport?", Duration::from_secs(5)).unwrap();
    assert!(s.recall_with("Which train goes to the airport?", &now(), Some(&q)).unwrap().is_empty());
}
