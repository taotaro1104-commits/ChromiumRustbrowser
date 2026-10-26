use storage::{Db, HistoryEntry, StorageError};

use crate::classify::{classify, Input};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuggestionKind {
    /// 入力そのものが URL。
    Navigate,
    History,
    /// 既定の検索エンジンで検索。
    Search,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Suggestion {
    pub kind: SuggestionKind,
    /// 表示テキスト(URL、タイトル、検索語)。
    pub text: String,
    /// 選択時の遷移先(Search は検索語そのまま)。
    pub target: String,
    pub score: f64,
}

const DAY_MS: f64 = 86_400_000.0;
/// FTS から候補を多めに取り、スコアで並べ替えて `limit` に絞る。
const OVERFETCH: usize = 3;

/// 頻度(対数)と新しさ(1/(1+日数))の合成スコア。
fn history_score(e: &HistoryEntry, now_ms: u64) -> f64 {
    let age_days = now_ms.saturating_sub(e.last_visit_ms) as f64 / DAY_MS;
    f64::from(e.visit_count).ln_1p() + 2.0 / (1.0 + age_days)
}

/// 入力に対する候補を最大 `limit` 件、スコア降順で返す。
pub fn suggest(
    db: &Db,
    input: &str,
    now_ms: u64,
    limit: usize,
) -> Result<Vec<Suggestion>, StorageError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    match classify(input) {
        Input::Url(u) => out.push(Suggestion {
            kind: SuggestionKind::Navigate,
            text: u.to_string(),
            target: u.to_string(),
            score: f64::MAX,
        }),
        Input::Search(q) if q.is_empty() => return Ok(Vec::new()),
        Input::Search(q) => out.push(Suggestion {
            kind: SuggestionKind::Search,
            text: q.clone(),
            target: q,
            // 履歴の強い一致より下、弱い一致より上に置く。
            score: 2.0,
        }),
    }
    let mut hist: Vec<Suggestion> = db
        .search_history(input, limit.saturating_mul(OVERFETCH))?
        .into_iter()
        .map(|e| Suggestion {
            kind: SuggestionKind::History,
            score: history_score(&e, now_ms),
            text: if e.title.is_empty() {
                e.url.clone()
            } else {
                e.title
            },
            target: e.url,
        })
        .collect();
    out.append(&mut hist);
    out.sort_by(|a, b| b.score.total_cmp(&a.score));
    // 同じ遷移先は最高スコアの1件だけ残す。
    let mut seen: Vec<String> = Vec::new();
    out.retain(|s| {
        let fresh = !seen.contains(&s.target);
        if fresh {
            seen.push(s.target.clone());
        }
        fresh
    });
    out.truncate(limit);
    Ok(out)
}
