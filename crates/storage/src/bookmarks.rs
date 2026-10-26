use rusqlite::params;

use crate::db::{to_i64, Db, MAX_LIMIT};
use crate::error::StorageError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
    pub created_ms: u64,
}

impl Db {
    /// 同じ URL は上書き(タイトル更新)。
    pub fn add_bookmark(&self, url: &str, title: &str, now_ms: u64) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO bookmarks(url, title, created_ms) VALUES (?1, ?2, ?3)
             ON CONFLICT(url) DO UPDATE SET title = excluded.title",
            params![url, title, to_i64(now_ms)],
        )?;
        Ok(())
    }

    pub fn remove_bookmark(&self, url: &str) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .execute("DELETE FROM bookmarks WHERE url = ?1", params![url])?
            > 0)
    }

    /// 作成の新しい順に `limit` 件、`offset` 件目から。
    pub fn list_bookmarks(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Vec<Bookmark>, StorageError> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT url, title, created_ms FROM bookmarks ORDER BY created_ms DESC, id DESC LIMIT ?1 OFFSET ?2",
        )?;
        let rows = stmt.query_map(
            params![to_i64(limit as u64).min(MAX_LIMIT), to_i64(offset as u64)],
            |r| {
                Ok(Bookmark {
                    url: r.get(0)?,
                    title: r.get(1)?,
                    created_ms: r.get::<_, i64>(2)? as u64,
                })
            },
        )?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn is_bookmarked(&self, url: &str) -> Result<bool, StorageError> {
        Ok(self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM bookmarks WHERE url = ?1)",
            params![url],
            |r| r.get(0),
        )?)
    }
}
