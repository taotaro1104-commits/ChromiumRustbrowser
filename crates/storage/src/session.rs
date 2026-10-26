use rusqlite::{params, OptionalExtension};

use crate::db::{to_i64, Db};
use crate::error::StorageError;

impl Db {
    /// セッションを不透明な文字列(呼び出し側が JSON 等で直列化)として最新1件だけ保存する。
    pub fn save_session(&self, data: &str, now_ms: u64) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO session(id, data, saved_ms) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET data = excluded.data, saved_ms = excluded.saved_ms",
            params![data, to_i64(now_ms)],
        )?;
        Ok(())
    }

    pub fn load_session(&self) -> Result<Option<String>, StorageError> {
        Ok(self
            .conn
            .query_row("SELECT data FROM session WHERE id = 1", [], |r| r.get(0))
            .optional()?)
    }
}
