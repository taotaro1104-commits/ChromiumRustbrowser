use rusqlite::params;

use crate::db::{to_i64, Db, MAX_LIMIT};
use crate::error::StorageError;

const UPSERT_VISIT: &str =
    "INSERT INTO history(url, title, visit_count, last_visit_ms) VALUES (?1, ?2, 1, ?3)
     ON CONFLICT(url) DO UPDATE SET
         visit_count = visit_count + 1,
         last_visit_ms = MAX(last_visit_ms, excluded.last_visit_ms),
         title = CASE WHEN excluded.title <> '' THEN excluded.title ELSE title END";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visit_count: u32,
    pub last_visit_ms: u64,
}

impl Db {
    /// 訪問を記録する。既存 URL は回数を加算し、空でないタイトルで更新する。
    pub fn record_visit(&self, url: &str, title: &str, now_ms: u64) -> Result<(), StorageError> {
        self.conn
            .prepare_cached(UPSERT_VISIT)?
            .execute(params![url, title, to_i64(now_ms)])?;
        Ok(())
    }

    /// 複数の訪問を1トランザクションで記録する(履歴インポート等)。途中で失敗したら全件ロールバック。
    pub fn record_visits<'a>(
        &self,
        visits: impl IntoIterator<Item = (&'a str, &'a str, u64)>,
    ) -> Result<(), StorageError> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut stmt = self.conn.prepare_cached(UPSERT_VISIT)?;
            for (url, title, now_ms) in visits {
                stmt.execute(params![url, title, to_i64(now_ms)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// URL・タイトルの前方一致 AND 検索(FTS5)。新しい順に最大 `limit` 件。
    pub fn search_history(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<HistoryEntry>, StorageError> {
        let Some(fts) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare_cached(
            "SELECT h.url, h.title, h.visit_count, h.last_visit_ms
             FROM history_fts JOIN history h ON h.id = history_fts.rowid
             WHERE history_fts MATCH ?1
             ORDER BY h.last_visit_ms DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![fts, to_i64(limit as u64).min(MAX_LIMIT)], |r| {
            Ok(HistoryEntry {
                url: r.get(0)?,
                title: r.get(1)?,
                visit_count: r.get(2)?,
                last_visit_ms: r.get::<_, i64>(3)? as u64,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn history_count(&self) -> Result<u64, StorageError> {
        Ok(self
            .conn
            .query_row("SELECT COUNT(*) FROM history", [], |r| r.get::<_, i64>(0))?
            as u64)
    }

    /// 履歴を刈り込む。`older_than_ms` より古い行を消し、残りが `max_rows` を超えたら
    /// 最終訪問が古い順に削除する。削除した行数を返す。
    pub fn prune_history(&self, older_than_ms: u64, max_rows: u64) -> Result<u64, StorageError> {
        let tx = self.conn.unchecked_transaction()?;
        let by_age = tx.execute(
            "DELETE FROM history WHERE last_visit_ms < ?1",
            params![to_i64(older_than_ms)],
        )?;
        let by_count = tx.execute(
            "DELETE FROM history WHERE id IN (
                 SELECT id FROM history ORDER BY last_visit_ms DESC LIMIT -1 OFFSET ?1)",
            params![to_i64(max_rows)],
        )?;
        tx.commit()?;
        Ok((by_age + by_count) as u64)
    }

    pub fn delete_history(&self, url: &str) -> Result<bool, StorageError> {
        Ok(self
            .conn
            .execute("DELETE FROM history WHERE url = ?1", params![url])?
            > 0)
    }
}

/// 利用者入力を FTS5 クエリへ安全に変換する。英数字トークンのみ抽出し、各トークンを
/// 引用符で囲んで前方一致にする(演算子・特殊構文の注入を防ぐ)。
/// ASCII 1文字だけの入力は全行に近い数が一致するため検索しない(非 ASCII は1文字でも意味を持つ)。
fn fts_query(input: &str) -> Option<String> {
    let tokens: Vec<&str> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let selective = tokens
        .iter()
        .any(|t| t.chars().nth(1).is_some() || !t.is_ascii());
    selective.then(|| {
        tokens
            .iter()
            .map(|t| format!("\"{t}\"*"))
            .collect::<Vec<_>>()
            .join(" ")
    })
}

#[cfg(test)]
mod tests {
    use super::fts_query;

    #[test]
    fn fts_query_strips_operators_and_quotes_tokens() {
        assert_eq!(
            fts_query("rust lang").as_deref(),
            Some("\"rust\"* \"lang\"*")
        );
        assert_eq!(
            fts_query("a\" OR \"b").as_deref(),
            Some("\"a\"* \"OR\"* \"b\"*")
        );
        assert_eq!(fts_query("  ***  "), None);
        assert_eq!(fts_query("a"), None);
        assert_eq!(fts_query("a b"), None);
        assert!(fts_query("a rust").is_some());
        assert!(fts_query("例").is_some());
    }
}
