//! A v1 file migrates to v2: the keyword index appears, is filled from the old rows and follows later ones.
mod common;
use common::*;
use memory::Store;
use rusqlite::Connection;

fn fts(dir: &std::path::Path, q: &str) -> i64 {
    count(dir, &format!("SELECT count(*) FROM facts_fts WHERE facts_fts MATCH '{q}'"))
}

/// A file exactly as v1 left it: the v2 index, its triggers and the version stamp removed.
fn v1_file(tmp: &Tmp) {
    let s = Store::open(&tmp.mem()).unwrap();
    drop(s);
    let c = db(&tmp.mem());
    c.execute_batch(
        "DROP TRIGGER facts_ai; DROP TRIGGER facts_ad; DROP TRIGGER facts_au; DROP TABLE facts_fts; PRAGMA user_version = 1;",
    )
    .unwrap();
    // rows written while the file was v1 (no triggers)
    insert_row(&c, "profile", "Lives in Hà Nội", "live");
    insert_row(&c, "episode", "Stressed about the Friday talk", "live");
    insert_row(&c, "episode", "Forgotten lunch with Lan", "forgotten");
}

fn insert_row(c: &Connection, kind: &str, text: &str, status: &str) {
    c.execute(
        "INSERT INTO facts (kind, text, importance, authority, status, observed_at)
         VALUES (?1, ?2, 6, 3, ?3, '2026-11-20T09:00:00+07:00')",
        [kind, text, status],
    )
    .unwrap();
}

#[test]
fn a_v1_file_opens_migrates_and_finds_its_old_rows() {
    let tmp = Tmp::new();
    v1_file(&tmp);
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 1);
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM sqlite_master WHERE name = 'facts_fts'"), 0);

    let s = Store::open(&tmp.mem()).unwrap();
    assert!(!s.is_newer());
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 2);
    for t in ["facts_fts", "facts_ai", "facts_ad", "facts_au"] {
        assert_eq!(count(&tmp.mem(), &format!("SELECT count(*) FROM sqlite_master WHERE name = '{t}'")), 1, "{t}");
    }
    // the rebuild indexed the old rows, accents folded
    assert_eq!(fts(&tmp.mem(), "talk"), 1);
    assert_eq!(fts(&tmp.mem(), "ha"), 1);
    let hits = s.search("friday talk", 5).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(s.search("ha noi", 5).unwrap().len(), 1);
    assert!(s.search("lunch", 5).unwrap().is_empty(), "a forgotten row is not searched");
    // the old rows are still there
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts"), 3);
}

#[test]
fn rows_written_after_the_migration_follow_the_index() {
    let tmp = Tmp::new();
    v1_file(&tmp);
    let s = Store::open(&tmp.mem()).unwrap();
    let id = episode(&s, date(2026, 11, 26), "Walked around the lantern market", 5);
    assert_eq!(fts(&tmp.mem(), "lantern"), 1);
    // an update of the text moves the index entry
    db(&tmp.mem()).execute("UPDATE facts SET text = 'Walked around the harbour' WHERE id = ?1", [id]).unwrap();
    assert_eq!(fts(&tmp.mem(), "lantern"), 0);
    assert_eq!(fts(&tmp.mem(), "harbour"), 1);
}

#[test]
fn erase_removes_the_text_from_the_index() {
    let tmp = Tmp::new();
    v1_file(&tmp);
    let s = Store::open(&tmp.mem()).unwrap();
    let id = episode(&s, date(2026, 11, 26), "Met Lan at Lanternfish Labs", 6);
    assert_eq!(fts(&tmp.mem(), "lanternfish"), 1);
    s.erase(id).unwrap();
    assert_eq!(fts(&tmp.mem(), "lanternfish"), 0);
    assert_eq!(count(&tmp.mem(), "SELECT count(*) FROM facts WHERE text LIKE '%Lanternfish%'"), 0);
    // the whole index is still consistent with the table
    db(&tmp.mem()).execute("INSERT INTO facts_fts(facts_fts, rank) VALUES ('integrity-check', 1)", []).unwrap();
}

#[test]
fn a_file_newer_than_v2_opens_read_only_and_is_not_touched() {
    let tmp = Tmp::new();
    let s = Store::open(&tmp.mem()).unwrap();
    episode(&s, date(2026, 11, 26), "Fixed the login bug", 5);
    drop(s);
    db(&tmp.mem()).pragma_update(None, "user_version", 3).unwrap();
    let s = Store::open(&tmp.mem()).unwrap();
    assert!(s.is_newer());
    assert_eq!(s.search("login", 5).unwrap().len(), 1, "reads still use the index");
    assert_eq!(s.add("profile", "x", &now()), Err(memory::Error::Newer));
    drop(s);
    assert_eq!(count(&tmp.mem(), "PRAGMA user_version"), 3);
}
