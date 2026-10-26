//! 履歴・ブックマーク・セッションの永続化(SQLite / WAL / FTS5)。
//!
//! 省メモリ方針(設計書 §4): 全件をメモリへ載せず都度クエリし、接続の `cache_size` を小さく、
//! `mmap_size=0`、接続数は最小にする。

mod bookmarks;
mod db;
mod error;
mod history;
mod logins;
mod session;

pub use bookmarks::Bookmark;
pub use db::Db;
pub use error::StorageError;
pub use history::HistoryEntry;
pub use logins::LoginEntry;
