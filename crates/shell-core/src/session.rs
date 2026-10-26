use serde::{Deserialize, Serialize};

use crate::shell::ShellError;

/// 保存・読込するセッション JSON の上限(バイト)。
pub const MAX_SESSION_BYTES: usize = 4 * 1024 * 1024;
/// 復元するタブ数の上限。
pub(crate) const MAX_RESTORED_TABS: usize = 500;
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
pub(crate) struct SessionFile {
    version: u32,
    pub(crate) active: Option<usize>,
    pub(crate) tabs: Vec<SessionTabDto>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct SessionTabDto {
    pub(crate) url: String,
    pub(crate) title: String,
    pub(crate) scroll_y: u32,
    pub(crate) form_state: Option<String>,
}

impl SessionFile {
    pub(crate) fn new(active: Option<usize>, tabs: Vec<SessionTabDto>) -> Self {
        Self {
            version: VERSION,
            active,
            tabs,
        }
    }

    pub(crate) fn to_json(&self) -> Result<String, ShellError> {
        serde_json::to_string(self).map_err(|e| ShellError::Session(e.to_string()))
    }

    /// 上限(`MAX_SESSION_BYTES`)に収まるまで古いタブから落として JSON にする。
    /// 保存できないほど大きいセッションが、次回起動の復元失敗につながるのを防ぐ。
    pub(crate) fn into_fitted_json(mut self) -> Result<String, ShellError> {
        loop {
            let json = self.to_json()?;
            if json.len() <= MAX_SESSION_BYTES || self.tabs.len() <= 1 {
                return Ok(json);
            }
            self.tabs.remove(0);
            self.active = self.active.and_then(|a| a.checked_sub(1));
        }
    }

    /// 境界チェック付きで読み込む(サイズ・版・形式)。
    pub(crate) fn parse(json: &str) -> Result<Self, ShellError> {
        if json.len() > MAX_SESSION_BYTES {
            return Err(ShellError::Session(format!(
                "session too large ({} bytes)",
                json.len()
            )));
        }
        let file: Self =
            serde_json::from_str(json).map_err(|e| ShellError::Session(e.to_string()))?;
        if file.version != VERSION {
            return Err(ShellError::Session(format!(
                "unsupported session version {}",
                file.version
            )));
        }
        Ok(file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(n: usize, size: usize) -> SessionTabDto {
        SessionTabDto {
            url: format!("https://a.example/{n}"),
            title: "x".repeat(size),
            scroll_y: 0,
            form_state: None,
        }
    }

    #[test]
    fn oversized_session_drops_oldest_tabs_and_stays_restorable() {
        let tabs: Vec<_> = (0..10).map(|n| tab(n, MAX_SESSION_BYTES / 4)).collect();
        let json = SessionFile::new(Some(9), tabs).into_fitted_json().unwrap();
        assert!(json.len() <= MAX_SESSION_BYTES);
        let file = SessionFile::parse(&json).unwrap();
        assert!(file.tabs.len() < 10);
        // 最後(アクティブ)のタブは残り、active は同じタブを指し続ける。
        let last = file.tabs.len() - 1;
        assert_eq!(file.active, Some(last));
        assert!(file.tabs[last].url.ends_with("/9"));
    }

    #[test]
    fn small_session_is_untouched() {
        let json = SessionFile::new(Some(0), vec![tab(0, 10)])
            .into_fitted_json()
            .unwrap();
        assert_eq!(SessionFile::parse(&json).unwrap().tabs.len(), 1);
    }
}
