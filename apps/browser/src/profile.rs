use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// プロファイル(履歴 DB・ポリシー・CEF キャッシュ)の置き場所。
/// `RUSTBROWSER_PROFILE` で上書きできる(テストで空のプロファイルを使うため)。
pub fn profile_dir() -> PathBuf {
    let dir = std::env::var_os("RUSTBROWSER_PROFILE")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("RustBrowser")))
        .unwrap_or_else(|| PathBuf::from("RustBrowserProfile"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        eprintln!("cannot create profile dir {}: {e}", dir.display());
    }
    dir
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}
