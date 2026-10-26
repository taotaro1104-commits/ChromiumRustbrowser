use std::path::Path;

use rusqlite::{Connection, TransactionBehavior};

use crate::error::StorageError;

/// スキーマ移行。インデックス = 移行後の `user_version - 1`。追記のみ、既存は編集しない。
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE history (
    id            INTEGER PRIMARY KEY,
    url           TEXT NOT NULL UNIQUE,
    title         TEXT NOT NULL DEFAULT '',
    visit_count   INTEGER NOT NULL DEFAULT 1,
    last_visit_ms INTEGER NOT NULL
);
CREATE INDEX history_last_visit ON history(last_visit_ms DESC);
CREATE VIRTUAL TABLE history_fts USING fts5(url, title, content='history', content_rowid='id');
CREATE TRIGGER history_ai AFTER INSERT ON history BEGIN
    INSERT INTO history_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;
CREATE TRIGGER history_ad AFTER DELETE ON history BEGIN
    INSERT INTO history_fts(history_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
END;
CREATE TRIGGER history_au AFTER UPDATE ON history BEGIN
    INSERT INTO history_fts(history_fts, rowid, url, title) VALUES ('delete', old.id, old.url, old.title);
    INSERT INTO history_fts(rowid, url, title) VALUES (new.id, new.url, new.title);
END;
CREATE TABLE bookmarks (
    id         INTEGER PRIMARY KEY,
    url        TEXT NOT NULL UNIQUE,
    title      TEXT NOT NULL,
    created_ms INTEGER NOT NULL
);
CREATE TABLE session (
    id       INTEGER PRIMARY KEY CHECK (id = 1),
    data     TEXT NOT NULL,
    saved_ms INTEGER NOT NULL
);
"#];

/// 接続ごとのページキャッシュ(KiB)。数百 KB に抑える。
const CACHE_SIZE_KIB: i64 = 512;

/// WAL ファイルの残存上限(バイト)。
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

const WAL_RETRIES: u32 = 100;

const JOURNAL_SIZE_LIMIT_BYTES: i64 = 1024 * 1024;

/// u64 → SQLite の INTEGER(i64)。範囲外は飽和させ、負値への折り返しを防ぐ。
pub(crate) fn to_i64(v: u64) -> i64 {
    i64::try_from(v).unwrap_or(i64::MAX)
}

/// 1回のクエリで返す最大件数。
pub(crate) const MAX_LIMIT: i64 = 10_000;

pub struct Db {
    pub(crate) conn: Connection,
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        Self::init(Connection::open(path)?)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StorageError> {
        // 他プロセスの書き込み・移行と競合しても即失敗せず待つ(WAL 切替や移行より前に設定する)。
        conn.busy_timeout(BUSY_TIMEOUT)?;
        conn.pragma_update(None, "cache_size", -CACHE_SIZE_KIB)?;
        conn.pragma_update(None, "mmap_size", 0)?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // WAL ファイルがチェックポイント後に肥大したまま残らないようにする。
        conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT_BYTES)?;
        // メモリ DB は WAL にできず別のモード名が返るため、結果は検査しない。
        // WAL への切替は排他ロックを要し busy_timeout で待たれないため、有限回リトライする。
        let mut attempt = 0;
        loop {
            match conn.pragma_update_and_check(None, "journal_mode", "WAL", |_| Ok(())) {
                Err(rusqlite::Error::SqliteFailure(e, _))
                    if e.code == rusqlite::ErrorCode::DatabaseBusy && attempt < WAL_RETRIES =>
                {
                    attempt += 1;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
                other => break other?,
            }
        }
        let mut db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    pub fn schema_version(&self) -> Result<u32, StorageError> {
        Ok(self
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))?)
    }

    fn migrate(&mut self) -> Result<(), StorageError> {
        let current = self.schema_version()?;
        let supported = MIGRATIONS.len() as u32;
        if current > supported {
            return Err(StorageError::SchemaTooNew {
                found: current,
                supported,
            });
        }
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
            // 複数プロセスが同時に起動しても二重適用しないよう、書き込みロックを先に取り版を再確認する。
            let tx = self
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let version: u32 = tx.pragma_query_value(None, "user_version", |r| r.get(0))?;
            if version > i as u32 {
                continue;
            }
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as u32 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    /// 現在の接続設定(テスト・診断用)。値を返さない PRAGMA(mmap が無効なビルド等)は `None`。
    pub fn pragma_i64(&self, name: &str) -> Result<Option<i64>, StorageError> {
        match self.conn.pragma_query_value(None, name, |r| r.get(0)) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
}
