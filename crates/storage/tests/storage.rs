use std::path::PathBuf;

use storage::{Db, StorageError};

fn temp_db_path(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("rb-storage-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("db.sqlite")
}

#[test]
fn pragmas_follow_memory_budget() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.pragma_i64("cache_size").unwrap(), Some(-512));
    assert!(matches!(
        db.pragma_i64("mmap_size").unwrap(),
        None | Some(0)
    ));
    assert_eq!(db.schema_version().unwrap(), 1);
}

#[test]
fn file_db_uses_wal_and_reopens_without_remigrating() {
    let path = temp_db_path("wal");
    {
        let db = Db::open(&path).unwrap();
        db.record_visit("https://a.example/", "A", 1).unwrap();
    }
    let db = Db::open(&path).unwrap();
    assert_eq!(db.history_count().unwrap(), 1);
    let mode: String = rusqlite::Connection::open(&path)
        .unwrap()
        .pragma_query_value(None, "journal_mode", |r| r.get(0))
        .unwrap();
    assert_eq!(mode.to_lowercase(), "wal");
    drop(db);
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn newer_schema_is_rejected() {
    let path = temp_db_path("newer");
    {
        let c = rusqlite::Connection::open(&path).unwrap();
        c.pragma_update(None, "user_version", 99).unwrap();
    }
    match Db::open(&path) {
        Err(StorageError::SchemaTooNew {
            found: 99,
            supported: 1,
        }) => {}
        other => panic!("unexpected: {:?}", other.err()),
    }
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn record_visit_upserts_count_and_keeps_title_when_new_is_empty() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "Alpha", 10).unwrap();
    db.record_visit("https://a.example/", "", 20).unwrap();
    let hits = db.search_history("alpha", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(
        (
            hits[0].title.as_str(),
            hits[0].visit_count,
            hits[0].last_visit_ms
        ),
        ("Alpha", 2, 20)
    );
}

#[test]
fn search_matches_title_and_url_prefix_with_and_semantics() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit(
        "https://doc.rust-lang.org/book/",
        "The Rust Programming Language",
        1,
    )
    .unwrap();
    db.record_visit("https://example.com/", "Example Domain", 2)
        .unwrap();
    assert_eq!(db.search_history("rust prog", 10).unwrap().len(), 1);
    assert_eq!(db.search_history("exam", 10).unwrap().len(), 1);
    assert_eq!(db.search_history("rust example", 10).unwrap().len(), 0);
    assert!(db.search_history("   ", 10).unwrap().is_empty());
}

#[test]
fn search_survives_hostile_input() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "A", 1).unwrap();
    for q in [
        "\"",
        "a OR",
        "NEAR(a b)",
        "* * *",
        "a:b",
        "(",
        "title:",
        "\u{0}",
    ] {
        db.search_history(q, 5).unwrap();
    }
}

#[test]
fn search_is_newest_first_and_limited() {
    let db = Db::open_in_memory().unwrap();
    for i in 0..5 {
        db.record_visit(&format!("https://site{i}.example/"), "site", i)
            .unwrap();
    }
    let hits = db.search_history("site", 3).unwrap();
    assert_eq!(hits.len(), 3);
    assert_eq!(hits[0].url, "https://site4.example/");
}

#[test]
fn delete_history_removes_it_from_fts_too() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "Alpha", 1).unwrap();
    assert!(db.delete_history("https://a.example/").unwrap());
    assert!(!db.delete_history("https://a.example/").unwrap());
    assert!(db.search_history("alpha", 5).unwrap().is_empty());
}

#[test]
fn title_update_is_reflected_in_search() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "Old", 1).unwrap();
    db.record_visit("https://a.example/", "Fresh", 2).unwrap();
    assert!(db.search_history("old", 5).unwrap().is_empty());
    assert_eq!(db.search_history("fresh", 5).unwrap().len(), 1);
}

#[test]
fn bookmarks_crud_and_paging() {
    let db = Db::open_in_memory().unwrap();
    for i in 0..5u64 {
        db.add_bookmark(&format!("https://b{i}.example/"), "t", i)
            .unwrap();
    }
    db.add_bookmark("https://b0.example/", "renamed", 99)
        .unwrap();
    let page = db.list_bookmarks(2, 0).unwrap();
    assert_eq!(page[0].url, "https://b4.example/");
    assert_eq!(db.list_bookmarks(10, 4).unwrap()[0].title, "renamed");
    assert!(db.is_bookmarked("https://b1.example/").unwrap());
    assert!(db.remove_bookmark("https://b1.example/").unwrap());
    assert!(!db.remove_bookmark("https://b1.example/").unwrap());
    assert!(!db.is_bookmarked("https://b1.example/").unwrap());
}

#[test]
fn session_keeps_latest_only() {
    let db = Db::open_in_memory().unwrap();
    assert_eq!(db.load_session().unwrap(), None);
    db.save_session("{\"tabs\":1}", 1).unwrap();
    db.save_session("{\"tabs\":2}", 2).unwrap();
    assert_eq!(db.load_session().unwrap().as_deref(), Some("{\"tabs\":2}"));
}

#[test]
fn record_visits_batches_and_merges_with_existing_rows() {
    let db = Db::open_in_memory().unwrap();
    db.record_visits([
        ("https://a.example/", "A", 1),
        ("https://b.example/", "B", 2),
    ])
    .unwrap();
    assert_eq!(db.history_count().unwrap(), 2);
    db.record_visits([("https://a.example/", "", 3)]).unwrap();
    let hit = &db.search_history("a.example", 5).unwrap()[0];
    assert_eq!(
        (hit.title.as_str(), hit.visit_count, hit.last_visit_ms),
        ("A", 2, 3)
    );
}

#[test]
fn older_visit_does_not_rewind_last_visit_and_huge_limits_are_safe() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "A", 100).unwrap();
    db.record_visit("https://a.example/", "A", 5).unwrap();
    let hit = &db.search_history("a.example", usize::MAX).unwrap()[0];
    assert_eq!(hit.last_visit_ms, 100);
    db.record_visit("https://b.example/", "B", u64::MAX)
        .unwrap();
    assert_eq!(db.list_bookmarks(usize::MAX, 0).unwrap().len(), 0);
}

#[test]
fn prune_history_drops_old_rows_then_caps_by_recency_and_cleans_fts() {
    let db = Db::open_in_memory().unwrap();
    for i in 0..10u64 {
        db.record_visit(&format!("https://site{i}.example/"), "t", 1_000 + i)
            .unwrap();
    }
    // 古い2件を期限で、残り8件のうち最古の3件を件数上限で削除する。
    assert_eq!(db.prune_history(1_002, 5).unwrap(), 5);
    assert_eq!(db.history_count().unwrap(), 5);
    assert!(db.search_history("site0", 10).unwrap().is_empty());
    assert!(db.search_history("site4", 10).unwrap().is_empty());
    assert_eq!(db.search_history("site9", 10).unwrap().len(), 1);
    assert_eq!(db.prune_history(0, 100).unwrap(), 0);
}

#[test]
fn single_ascii_char_query_returns_nothing() {
    let db = Db::open_in_memory().unwrap();
    db.record_visit("https://a.example/", "a", 1).unwrap();
    assert!(db.search_history("a", 10).unwrap().is_empty());
}

#[test]
fn concurrent_opens_migrate_once() {
    let path = temp_db_path("concurrent_migrate");
    let handles: Vec<_> = (0..4)
        .map(|_| {
            let p = path.clone();
            std::thread::spawn(move || Db::open(&p).map(|d| d.schema_version().unwrap()))
        })
        .collect();
    for h in handles {
        assert_eq!(h.join().unwrap().unwrap(), 1);
    }
}
