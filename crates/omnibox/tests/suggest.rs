use omnibox::{suggest, SuggestionKind};
use storage::Db;

const DAY: u64 = 86_400_000;

fn db_with(entries: &[(&str, &str, u32, u64)]) -> Db {
    let db = Db::open_in_memory().unwrap();
    for (url, title, visits, last) in entries {
        for i in 0..*visits {
            let t = if i + 1 == *visits { *last } else { 0 };
            db.record_visit(url, title, t).unwrap();
        }
    }
    db
}

#[test]
fn empty_input_has_no_suggestions() {
    let db = db_with(&[]);
    assert!(suggest(&db, "  ", 0, 5).unwrap().is_empty());
    assert!(suggest(&db, "rust", 0, 0).unwrap().is_empty());
}

#[test]
fn search_entry_is_offered_for_plain_words() {
    let db = db_with(&[]);
    let s = suggest(&db, "rust", 0, 5).unwrap();
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].kind, SuggestionKind::Search);
    assert_eq!(s[0].target, "rust");
}

#[test]
fn url_input_is_first() {
    let db = db_with(&[("https://example.com/a", "Example A", 5, 10 * DAY)]);
    let s = suggest(&db, "example.com", 10 * DAY, 5).unwrap();
    assert_eq!(s[0].kind, SuggestionKind::Navigate);
    assert_eq!(s[0].target, "https://example.com/");
    assert!(s.iter().any(|x| x.kind == SuggestionKind::History));
}

#[test]
fn frequent_and_recent_pages_rank_higher() {
    let now = 100 * DAY;
    let db = db_with(&[
        ("https://old.example/", "rust old", 1, 10 * DAY),
        ("https://fresh.example/", "rust fresh", 1, now),
        ("https://often.example/", "rust often", 30, now),
    ]);
    let s = suggest(&db, "rust", now, 10).unwrap();
    let order: Vec<&str> = s
        .iter()
        .filter(|x| x.kind == SuggestionKind::History)
        .map(|x| x.target.as_str())
        .collect();
    assert_eq!(
        order,
        [
            "https://often.example/",
            "https://fresh.example/",
            "https://old.example/"
        ]
    );
}

#[test]
fn limit_is_respected_and_scores_descend() {
    let now = 5 * DAY;
    let entries: Vec<(String, String)> = (0..20)
        .map(|i| (format!("https://s{i}.example/"), "site".to_owned()))
        .collect();
    let db = Db::open_in_memory().unwrap();
    for (i, (u, t)) in entries.iter().enumerate() {
        db.record_visit(u, t, i as u64 * 1000).unwrap();
    }
    let s = suggest(&db, "site", now, 4).unwrap();
    assert_eq!(s.len(), 4);
    assert!(s.windows(2).all(|w| w[0].score >= w[1].score));
}

#[test]
fn untitled_history_shows_url_as_text() {
    let db = db_with(&[("https://untitled.example/", "", 1, 0)]);
    let s = suggest(&db, "untitled", 0, 5).unwrap();
    let h = s
        .iter()
        .find(|x| x.kind == SuggestionKind::History)
        .unwrap();
    assert_eq!(h.text, "https://untitled.example/");
}

#[test]
fn hostile_input_does_not_error() {
    let db = db_with(&[("https://a.example/", "a", 1, 0)]);
    for q in ["\"", "a OR b", "NEAR(", "javascript:alert(1)", "%00"] {
        suggest(&db, q, 0, 5).unwrap();
    }
}

/// 履歴10万件でも候補検索が許容時間内(設計書 §4: メモリ内インデックスなし、FTS5 で都度検索)。
#[test]
fn suggestions_stay_fast_with_100k_history() {
    let path = std::env::temp_dir().join(format!("rb-omnibox-100k-{}.sqlite", std::process::id()));
    let db = Db::open(&path).unwrap();
    let rows: Vec<(String, String, u64)> = (0..100_000u64)
        .map(|i| {
            (
                format!("https://host{}.example/page/{i}", i % 1000),
                format!("Page {i} about topic{}", i % 50),
                i,
            )
        })
        .collect();
    db.record_visits(rows.iter().map(|(u, t, n)| (u.as_str(), t.as_str(), *n)))
        .unwrap();
    assert_eq!(db.history_count().unwrap(), 100_000);
    let start = std::time::Instant::now();
    for q in ["page", "topic7", "host42 page", "about topic3"] {
        let s = suggest(&db, q, 200_000, 8).unwrap();
        assert!(!s.is_empty(), "{q}");
    }
    let per_query = start.elapsed() / 4;
    assert!(
        per_query.as_millis() < 250,
        "slow suggestions: {per_query:?}"
    );
    drop(db);
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", path.display()));
    }
}
