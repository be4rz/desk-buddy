//! The bundled SQLite has FTS5 (phase 2 needs it).

#[test]
fn fts5_works_on_the_bundled_build() {
    let c = rusqlite::Connection::open_in_memory().unwrap();
    c.execute_batch("CREATE VIRTUAL TABLE t USING fts5(x); INSERT INTO t(x) VALUES ('lives in Hanoi');").unwrap();
    let n: i64 = c.query_row("SELECT count(*) FROM t WHERE t MATCH 'hanoi'", [], |r| r.get(0)).unwrap();
    assert_eq!(n, 1);
}
