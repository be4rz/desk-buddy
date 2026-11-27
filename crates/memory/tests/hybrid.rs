//! Vectors in the store (p3): hybrid recall, keyword-only without the module, the model mark, and erase.
//! Embeddings here are [`concept_vec`] put in directly; `embed_local.rs` goes through the HTTP embedder.
mod common;
use common::*;
use memory::{pick, Opts, Recalled, Store, DIMS};

const TALK: &str = "Stressed about Friday's talk at the meetup; practised the intro twice";
const MODEL: &str = "nomic-embed-text";

fn fixture() -> (Tmp, Store) {
    let (t, s) = store();
    episode(&s, date(2026, 11, 20), TALK, 7);
    episode(&s, date(2026, 11, 25), "Cooked noodles for lunch and fixed a bike tyre", 4);
    episode(&s, date(2026, 11, 26), "Fixed the login bug at Lanternfish Labs", 6);
    episode(&s, date(2026, 11, 26), "Walked to the lake with Lan", 5);
    (t, s)
}

/// Embeds every row waiting, as the jobs thread does. Returns how many.
fn backfill(s: &Store) -> usize {
    s.set_embed_model(MODEL).unwrap();
    let todo = s.vectors_todo(32).unwrap();
    let rows: Vec<(i64, String, Vec<f32>)> = todo.iter().map(|(id, text)| (*id, text.clone(), concept_vec(text))).collect();
    s.put_vectors(MODEL, &rows).unwrap()
}

fn hybrid(s: &Store, msg: &str) -> Vec<String> {
    s.recall_with(msg, &now(), Some(&concept_vec(msg))).unwrap()
}

#[test]
fn a_message_sharing_no_word_finds_the_episode_by_meaning() {
    let (_t, s) = fixture();
    assert!(s.vectors_on());
    assert_eq!(backfill(&s), 4);
    assert_eq!(s.vector_count(), 4);
    assert!(s.vectors_todo(32).unwrap().is_empty(), "all embedded");
    let msg = "How do I calm down before speaking?";
    assert!(s.recall(msg, &now()).unwrap().is_empty(), "no keyword in common");
    assert_eq!(hybrid(&s, msg), [format!("2026-11-20: {TALK}")]);
}

#[test]
fn hybrid_still_finds_keywords_and_an_unrelated_message_brings_nothing() {
    let (_t, s) = fixture();
    backfill(&s);
    assert_eq!(hybrid(&s, "I'm nervous about speaking at the meetup again"), [format!("2026-11-20: {TALK}")]);
    assert!(hybrid(&s, "Which train goes to the airport?").is_empty());
    assert!(hybrid(&s, "").is_empty());
    // a date word still wins, vectors or not
    let got = hybrid(&s, "what did we do yesterday?");
    assert_eq!(got.len(), 2, "{got:?}");
    assert!(got.iter().all(|l| l.starts_with("2026-11-26: ")));
}

#[test]
fn a_row_not_embedded_yet_keeps_its_keyword_score() {
    let (_t, s) = fixture();
    backfill(&s);
    let id = episode(&s, date(2026, 11, 26), "Booked the dentist for Thursday", 6);
    assert_eq!(s.vectors_todo(32).unwrap(), [(id, "Booked the dentist for Thursday".to_string())]);
    assert_eq!(hybrid(&s, "When is the dentist?"), ["2026-11-26: Booked the dentist for Thursday"]);
}

#[test]
fn core_rows_never_come_back_through_vectors() {
    let (_t, s) = store();
    let core = s.add("profile", "Gets nervous before speaking in public", &now()).unwrap();
    assert!(s.section().contains("nervous"));
    backfill(&s);
    assert!(hybrid(&s, "How do I calm down before a talk?").is_empty(), "core id {core} is in the prompt already");
}

#[test]
fn candidates_and_pick_follow_admission() {
    let (_t, s) = fixture();
    backfill(&s);
    let n = now();
    let msg = "nervous talk, then noodles for lunch and the login bug";
    let cands = s.recall_candidates(msg, &n, Some(&concept_vec(msg))).unwrap();
    assert!(cands.len() >= 3, "{cands:?}");
    assert!(cands.windows(2).all(|w| w[0].score >= w[1].score));
    let ids: Vec<i64> = cands.iter().map(|c| c.id).collect();
    // admission keeps one: only that line; keeps none: no lines; no answer: the plain top 3
    assert_eq!(pick(&cands, Some(&[ids[1]])).iter().map(|c| c.id).collect::<Vec<_>>(), [ids[1]]);
    assert!(pick(&cands, Some(&[])).is_empty());
    assert!(pick(&cands, Some(&[9999])).is_empty(), "an unknown id keeps nothing");
    assert_eq!(pick(&cands, None).iter().map(|c| c.id).collect::<Vec<_>>(), ids[..3]);
    // reading candidates is not a recall
    assert_eq!(count(&tmp_dir(&s), "SELECT count(*) FROM facts WHERE recall_n > 0"), 0);
    s.mark_recalled(&[ids[0]], &n);
    assert_eq!(count(&tmp_dir(&s), "SELECT count(*) FROM facts WHERE recall_n > 0"), 1);
}

fn tmp_dir(s: &Store) -> std::path::PathBuf {
    s.dir().to_path_buf()
}

#[test]
fn pick_skips_a_line_that_does_not_fit() {
    let long = Recalled { id: 1, line: "x".repeat(590), score: 0.9, dated: false };
    let a = Recalled { dated: false, id: 2, line: "a".repeat(20), score: 0.8 };
    let b = Recalled { dated: false, id: 3, line: "b".repeat(20), score: 0.7 };
    let got: Vec<i64> = pick(&[long, a, b], None).iter().map(|c| c.id).collect();
    assert_eq!(got, [1]);
    let c = Recalled { dated: false, id: 4, line: "c".repeat(5), score: 0.6 };
    let got: Vec<i64> = pick(&[Recalled { id: 1, line: "x".repeat(590), score: 0.9, dated: false }, c], None).iter().map(|c| c.id).collect();
    assert_eq!(got, [1, 4]);
}

#[test]
fn without_the_module_recall_is_keyword_only() {
    let tmp = Tmp::new();
    let s = Store::open_opts(&tmp.mem(), Opts { vec: false }).unwrap();
    assert!(!s.vectors_on());
    episode(&s, date(2026, 11, 20), TALK, 7);
    assert!(s.vectors_todo(32).unwrap().is_empty());
    assert!(!s.set_embed_model(MODEL).unwrap());
    assert_eq!(s.put_vectors(MODEL, &[(1, TALK.to_string(), concept_vec(TALK))]).unwrap(), 0);
    assert_eq!(s.vector_count(), 0);
    // the p2 keyword recall (A12), a query vector ignored
    let msg = "I'm nervous about speaking at the meetup again";
    assert_eq!(s.recall_with(msg, &now(), Some(&concept_vec(msg))).unwrap(), [format!("2026-11-20: {TALK}")]);
    assert!(s.recall_with("How do I calm down before speaking?", &now(), Some(&concept_vec("calm speaking"))).unwrap().is_empty());
}

#[test]
fn a_vector_store_opened_without_the_module_still_erases_and_forgets() {
    let tmp = Tmp::new();
    let s = Store::open(&tmp.mem()).unwrap();
    let id = episode(&s, date(2026, 11, 20), TALK, 7);
    let other = episode(&s, date(2026, 11, 21), "Walked to the lake with Lan", 5);
    backfill(&s);
    assert_eq!(s.vector_count(), 2);
    drop(s);

    let s = Store::open_opts(&tmp.mem(), Opts { vec: false }).unwrap();
    s.erase(id).unwrap();
    s.soft_forget(other).unwrap();
    assert!(s.list(None).unwrap().is_empty());
    // a new row takes the erased id (no AUTOINCREMENT): it must not inherit the old vector
    let reused = episode(&s, date(2026, 11, 26), "Fixed the login bug at Lanternfish Labs", 6);
    drop(s);

    let s = Store::open(&tmp.mem()).unwrap();
    assert!(s.vectors_on());
    assert_eq!(s.vector_count(), 0, "both vectors dropped on the next open with the module");
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts_vec"), 0);
    assert_eq!(s.vectors_todo(32).unwrap().iter().map(|r| r.0).collect::<Vec<_>>(), [reused]);
}

#[test]
fn a_new_model_drops_every_vector_and_a_late_batch_is_refused() {
    let (t, s) = fixture();
    assert!(s.set_embed_model("model-a").unwrap(), "first mark");
    assert!(!s.set_embed_model("model-a").unwrap(), "same model");
    let todo = s.vectors_todo(32).unwrap();
    let rows: Vec<(i64, String, Vec<f32>)> = todo.iter().map(|(id, text)| (*id, text.clone(), concept_vec(text))).collect();
    assert_eq!(s.put_vectors("model-a", &rows).unwrap(), 4);
    assert_eq!(meta(&t.mem(), "embed_model").as_deref(), Some("model-a"));

    assert!(s.set_embed_model("model-b").unwrap());
    assert_eq!(s.vector_count(), 0);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts_vec"), 0);
    assert_eq!(s.put_vectors("model-a", &rows).unwrap(), 0, "made with the old model");
    assert_eq!(s.vectors_todo(32).unwrap().len(), 4);
    // a wrong-sized embedding is refused, not stored
    assert!(matches!(s.put_vectors("model-b", &[(rows[0].0, rows[0].1.clone(), vec![0.1; DIMS - 1])]), Err(memory::Error::Usage(_))));
}

#[test]
fn forgotten_and_superseded_rows_lose_their_vector() {
    let (_t, s) = fixture();
    backfill(&s);
    let talk = s.recall("nervous about the meetup talk", &now()).unwrap();
    assert_eq!(talk.len(), 1);
    let id = s.list(Some("episode")).unwrap().into_iter().find(|f| f.text == TALK).unwrap().id;
    s.soft_forget(id).unwrap();
    assert!(hybrid(&s, "How do I calm down before speaking?").is_empty(), "never recalled once forgotten");
    assert_eq!(s.vector_count(), 3, "its vector went with the forget, not at the next backfill");
    assert_eq!(count(&_t.mem(), "SELECT count(*) FROM facts_vec"), 3);
    assert!(s.vectors_todo(5).unwrap().is_empty(), "and it is not embedded again");
}

#[test]
fn a_forgotten_rows_vector_bytes_are_gone_from_the_file_at_once() {
    let (t, s) = store();
    let id = episode(&s, date(2026, 11, 20), TALK, 7);
    s.set_embed_model(MODEL).unwrap();
    let mut v: Vec<f32> = (0..DIMS).map(|i| 0.011 + i as f32 * 0.0009).collect();
    memory::embed::normalise(&mut v);
    assert_eq!(s.put_vectors(MODEL, &[(id, TALK.to_string(), v.clone())]).unwrap(), 1);
    s.soft_forget(id).unwrap();
    assert_eq!(s.vector_count(), 0);
    db(&t.mem()).query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |_| Ok(())).unwrap();
    assert!(!file_has(&t.mem(), &needle(&v)), "forgotten: the vector's bytes are gone from the file");
}

/// The bytes of one vector as the file stores them.
fn needle(v: &[f32]) -> Vec<u8> {
    v[..8].iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn file_has(dir: &std::path::Path, needle: &[u8]) -> bool {
    ["memory.sqlite", "memory.sqlite-wal"].iter().any(|f| {
        std::fs::read(dir.join(f)).is_ok_and(|b| b.windows(needle.len()).any(|w| w == needle))
    })
}

#[test]
fn erase_leaves_no_copy_of_the_vector_in_the_file() {
    let (t, s) = store();
    let id = episode(&s, date(2026, 11, 20), TALK, 7);
    s.set_embed_model(MODEL).unwrap();
    let mut v: Vec<f32> = (0..DIMS).map(|i| 0.013 + i as f32 * 0.0007).collect();
    memory::embed::normalise(&mut v);
    assert_eq!(s.put_vectors(MODEL, &[(id, TALK.to_string(), v.clone())]).unwrap(), 1);
    assert!(file_has(&t.mem(), &needle(&v)), "the test can see the vector");
    s.erase(id).unwrap();
    assert_eq!(s.vector_count(), 0);
    assert_eq!(count(&t.mem(), "SELECT count(*) FROM facts_vec"), 0);
    assert!(!file_has(&t.mem(), &needle(&v)), "erased: the vector's bytes are gone from the file");
}

#[test]
fn a_v2_file_migrates_to_v3_and_backfills_its_old_rows() {
    let tmp = Tmp::new();
    let s = Store::open(&tmp.mem()).unwrap();
    episode(&s, date(2026, 11, 20), TALK, 7);
    drop(s);
    db(&tmp.mem())
        .execute_batch("DROP TRIGGER facts_vec_ad; DROP TABLE facts_vec; DROP TRIGGER vec_rows_ad; DROP TABLE vec_rows; PRAGMA user_version = 2;")
        .unwrap();
    let s = Store::open(&tmp.mem()).unwrap();
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 3);
    assert!(s.vectors_on());
    assert_eq!(backfill(&s), 1);
    assert_eq!(hybrid(&s, "How do I calm down before speaking?").len(), 1);
}

#[test]
fn admission_never_drops_a_row_the_message_named_the_day_of() {
    let (_t, s) = fixture();
    backfill(&s);
    let msg = "what did we do yesterday?";
    let cands = s.recall_candidates(msg, &now(), Some(&concept_vec(msg))).unwrap();
    let dated: Vec<i64> = cands.iter().filter(|c| c.dated).map(|c| c.id).collect();
    assert_eq!(dated.len(), 2, "{cands:?}");
    // the model keeps none of them: the day's rows still go
    assert_eq!(pick(&cands, Some(&[])).iter().map(|c| c.id).collect::<Vec<_>>(), dated);
}

#[test]
fn a_keyword_hit_is_never_scored_below_keywords_alone() {
    let (_t, s) = fixture();
    s.set_embed_model(MODEL).unwrap();
    // the talk's vector points somewhere else entirely: cosine adds nothing
    let todo = s.vectors_todo(32).unwrap();
    let rows: Vec<(i64, String, Vec<f32>)> = todo.iter().map(|(id, t)| (*id, t.clone(), concept_vec("the lake walk"))).collect();
    s.put_vectors(MODEL, &rows).unwrap();
    let msg = "I'm nervous about speaking at the meetup again";
    let kw = s.recall_candidates(msg, &now(), None).unwrap();
    let hy = s.recall_candidates(msg, &now(), Some(&concept_vec(msg))).unwrap();
    let score = |c: &[Recalled], id| c.iter().find(|r| r.id == id).map(|r| r.score);
    let talk = kw.first().expect("keywords find the talk").id;
    assert!(score(&hy, talk) >= score(&kw, talk), "{kw:?} vs {hy:?}");
    assert_eq!(hybrid(&s, msg), [format!("2026-11-20: {TALK}")]);
}

#[test]
fn a_vector_side_error_falls_back_to_keywords() {
    let (_t, s) = fixture();
    backfill(&s);
    let msg = "I'm nervous about speaking at the meetup again";
    // a query vector sqlite-vec can't compare (wrong size): keyword recall, not an error
    assert_eq!(s.recall_with(msg, &now(), Some(&[0.5; 10])).unwrap(), [format!("2026-11-20: {TALK}")]);
}

#[test]
fn a_vector_for_text_the_row_no_longer_holds_is_not_stored() {
    let (_t, s) = store();
    let id = episode(&s, date(2026, 11, 20), TALK, 7);
    s.set_embed_model(MODEL).unwrap();
    // the row was erased during the embed call and its id went to a new row
    assert_eq!(s.put_vectors(MODEL, &[(id, "Walked to the lake with Lan".into(), concept_vec("lake"))]).unwrap(), 0);
    assert_eq!(s.vector_count(), 0);
    assert_eq!(s.put_vectors(MODEL, &[(id, TALK.into(), concept_vec(TALK))]).unwrap(), 1);
}
