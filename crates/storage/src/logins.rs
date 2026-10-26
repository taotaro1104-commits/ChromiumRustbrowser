//! 保存済みログインの **索引**(どのサイトにどのユーザー名があるか)。パスワード本体はここに置かず、
//! OS の資格情報ストアへ保存する(`passwords` クレート)。索引だけなので、DB が漏れてもパスワードは出ない。

use rusqlite::params;

use crate::db::{to_i64, Db, MAX_LIMIT};
use crate::error::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginEntry {
    pub origin: String,
    pub username: String,
    pub created_ms: u64,
    pub last_used_ms: u64,
}

impl Db {
    /// 登録する。新規なら `true`、既存(同じ origin とユーザー名)なら使用日時だけ更新して `false`。
    pub fn upsert_login(
        &self,
        origin: &str,
        username: &str,
        now_ms: u64,
    ) -> Result<bool, StorageError> {
        let changed = self.conn.execute(
            "INSERT INTO logins(origin, username, created_ms, last_used_ms) VALUES (?1, ?2, ?3, ?3)
             ON CONFLICT(origin, username) DO NOTHING",
            params![origin, username, to_i64(now_ms)],
        )?;
        if changed == 0 {
            self.touch_login(origin, username, now_ms)?;
        }
        Ok(changed > 0)
    }

    pub fn touch_login(
        &self,
        origin: &str,
        username: &str,
        now_ms: u64,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "UPDATE logins SET last_used_ms = ?3 WHERE origin = ?1 AND username = ?2",
            params![origin, username, to_i64(now_ms)],
        )?;
        Ok(())
    }

    /// そのサイトのログイン。最近使った順。
    pub fn logins_for(&self, origin: &str) -> Result<Vec<LoginEntry>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT origin, username, created_ms, last_used_ms FROM logins
             WHERE origin = ?1 ORDER BY last_used_ms DESC, username",
        )?;
        let rows = stmt.query_map(params![origin], row_to_entry)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// 全サイトのログイン(管理画面用)。サイト名、ユーザー名の順。
    pub fn list_logins(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<LoginEntry>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT origin, username, created_ms, last_used_ms FROM logins
             ORDER BY origin, username LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(
            params![to_i64(limit as u64).min(MAX_LIMIT), to_i64(offset as u64)],
            row_to_entry,
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn delete_login(&self, origin: &str, username: &str) -> Result<bool, StorageError> {
        Ok(self.conn.execute(
            "DELETE FROM logins WHERE origin = ?1 AND username = ?2",
            params![origin, username],
        )? > 0)
    }

    /// 「このサイトでは保存しない」の登録。
    pub fn add_login_exception(&self, origin: &str, now_ms: u64) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO login_exceptions(origin, created_ms) VALUES (?1, ?2)",
            params![origin, to_i64(now_ms)],
        )?;
        Ok(())
    }

    pub fn is_login_exception(&self, origin: &str) -> Result<bool, StorageError> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM login_exceptions WHERE origin = ?1)",
            params![origin],
            |r| r.get(0),
        )?)
    }

    pub fn remove_login_exception(&self, origin: &str) -> Result<bool, StorageError> {
        Ok(self.conn.execute(
            "DELETE FROM login_exceptions WHERE origin = ?1",
            params![origin],
        )? > 0)
    }
}

fn row_to_entry(r: &rusqlite::Row<'_>) -> rusqlite::Result<LoginEntry> {
    Ok(LoginEntry {
        origin: r.get(0)?,
        username: r.get(1)?,
        created_ms: r.get::<_, i64>(2)? as u64,
        last_used_ms: r.get::<_, i64>(3)? as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_reports_new_versus_existing_and_touches() {
        let db = Db::open_in_memory().unwrap();
        assert!(db.upsert_login("https://a.example", "alice", 10).unwrap());
        assert!(!db.upsert_login("https://a.example", "alice", 20).unwrap());
        let l = db.logins_for("https://a.example").unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!((l[0].created_ms, l[0].last_used_ms), (10, 20));
    }

    #[test]
    fn logins_are_scoped_by_origin_and_ordered_by_recent_use() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_login("https://a.example", "old", 1).unwrap();
        db.upsert_login("https://a.example", "new", 5).unwrap();
        db.upsert_login("https://b.example", "other", 3).unwrap();
        let names: Vec<_> = db
            .logins_for("https://a.example")
            .unwrap()
            .into_iter()
            .map(|e| e.username)
            .collect();
        assert_eq!(names, ["new", "old"]);
        assert!(db.logins_for("https://c.example").unwrap().is_empty());
        // 別オリジン(ポート違い・サブドメイン)は別物。
        assert!(db.logins_for("https://a.example:8443").unwrap().is_empty());
        assert!(db.logins_for("https://www.a.example").unwrap().is_empty());
    }

    #[test]
    fn delete_and_listing() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_login("https://b.example", "x", 1).unwrap();
        db.upsert_login("https://a.example", "y", 1).unwrap();
        let all = db.list_logins(10, 0).unwrap();
        assert_eq!(all[0].origin, "https://a.example");
        assert!(db.delete_login("https://a.example", "y").unwrap());
        assert!(!db.delete_login("https://a.example", "y").unwrap());
        assert_eq!(db.list_logins(10, 0).unwrap().len(), 1);
    }

    #[test]
    fn exceptions_round_trip() {
        let db = Db::open_in_memory().unwrap();
        assert!(!db.is_login_exception("https://a.example").unwrap());
        db.add_login_exception("https://a.example", 1).unwrap();
        db.add_login_exception("https://a.example", 2).unwrap();
        assert!(db.is_login_exception("https://a.example").unwrap());
        assert!(db.remove_login_exception("https://a.example").unwrap());
        assert!(!db.is_login_exception("https://a.example").unwrap());
    }

    #[test]
    fn sql_metacharacters_are_data() {
        let db = Db::open_in_memory().unwrap();
        let evil = "x'); DROP TABLE logins;--";
        db.upsert_login("https://a.example", evil, 1).unwrap();
        assert_eq!(
            db.logins_for("https://a.example").unwrap()[0].username,
            evil
        );
    }
}
