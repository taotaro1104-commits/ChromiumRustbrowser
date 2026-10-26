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
