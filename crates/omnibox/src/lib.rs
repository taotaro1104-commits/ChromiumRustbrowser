//! オムニボックス: 入力の URL/検索判定とサジェスト(設計書 §6 omnibox)。
//! メモリ内インデックスは持たず、候補は `storage` の FTS5 から都度取得する。

mod classify;
mod suggest;

pub use classify::{classify, Input};
pub use suggest::{suggest, Suggestion, SuggestionKind};
