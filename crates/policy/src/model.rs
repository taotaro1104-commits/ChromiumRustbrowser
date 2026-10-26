use serde::Deserialize;
use url::Url;

use crate::host_pattern::{HostPattern, HostSet};

/// ポリシー JSON の上限。巨大入力でメモリを使い切らないための境界チェック。
pub const MAX_POLICY_BYTES: usize = 1024 * 1024;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PolicyError {
    #[error("policy is too large ({0} bytes, max {MAX_POLICY_BYTES})")]
    TooLarge(usize),
    #[error("invalid policy json: {0}")]
    Json(String),
    #[error("invalid homepage: {0}")]
    Homepage(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CookiePolicy {
    Allow,
    #[default]
    BlockThirdParty,
    BlockAll,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SiteIsolation {
    #[default]
    Full,
    /// ログインのあるサイトのみ分離する省メモリモード(設計書 §4)。
    PartialLoggedIn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    pub https_upgrade: bool,
    pub cookie_policy: CookiePolicy,
    pub blocked_hosts: Vec<HostPattern>,
    /// `blocked_hosts` のハッシュ索引。照合は `is_host_blocked` を使う。
    blocked_index: HostSet,
    pub homepage: Option<Url>,
    pub memory_saver: bool,
    pub site_isolation: SiteIsolation,
}

impl Default for Policy {
    fn default() -> Self {
        Self {
            https_upgrade: true,
            cookie_policy: CookiePolicy::default(),
            blocked_hosts: Vec::new(),
            blocked_index: HostSet::default(),
            homepage: None,
            memory_saver: false,
            site_isolation: SiteIsolation::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, default)]
struct RawPolicy {
    https_upgrade: bool,
    cookie_policy: CookiePolicy,
    blocked_hosts: Vec<HostPattern>,
    homepage: Option<String>,
    memory_saver: bool,
    site_isolation: SiteIsolation,
}

impl Default for RawPolicy {
    fn default() -> Self {
        let d = Policy::default();
        Self {
            https_upgrade: d.https_upgrade,
            cookie_policy: d.cookie_policy,
            blocked_hosts: d.blocked_hosts,
            homepage: None,
            memory_saver: d.memory_saver,
            site_isolation: d.site_isolation,
        }
    }
}

impl Policy {
    /// JSON から読み込む。未知のキー・不正な値はエラー(設定ミスを黙って無視しない)。
    pub fn from_json(json: &str) -> Result<Self, PolicyError> {
        if json.len() > MAX_POLICY_BYTES {
            return Err(PolicyError::TooLarge(json.len()));
        }
        // serde は `[]` を空の構造体として受理してしまうため、オブジェクトであることを先に確認する。
        if !json.trim_start().starts_with('{') {
            return Err(PolicyError::Json("policy must be a JSON object".into()));
        }
        let raw: RawPolicy =
            serde_json::from_str(json).map_err(|e| PolicyError::Json(e.to_string()))?;
        let homepage = raw.homepage.as_deref().map(parse_homepage).transpose()?;
        Ok(Self {
            https_upgrade: raw.https_upgrade,
            cookie_policy: raw.cookie_policy,
            blocked_index: HostSet::new(&raw.blocked_hosts),
            blocked_hosts: raw.blocked_hosts,
            homepage,
            memory_saver: raw.memory_saver,
            site_isolation: raw.site_isolation,
        })
    }

    /// 正規化済み(小文字・末尾ドットなし)のホストがブロック対象か。
    pub fn is_host_blocked(&self, host: &str) -> bool {
        self.blocked_index.matches(host)
    }
}

fn parse_homepage(s: &str) -> Result<Url, PolicyError> {
    let url = Url::parse(s).map_err(|e| PolicyError::Homepage(format!("{s:?}: {e}")))?;
    match url.scheme() {
        "http" | "https" => Ok(url),
        other => Err(PolicyError::Homepage(format!(
            "unsupported scheme {other:?}"
        ))),
    }
}

/// ポリシーの取得元(JSON ファイル、Windows GPO、macOS plist …)。
pub trait PolicySource {
    fn load(&self) -> Result<Policy, PolicyError>;
}

/// 文字列として渡された JSON を読む取得元。
pub struct JsonSource(pub String);

impl PolicySource for JsonSource {
    fn load(&self) -> Result<Policy, PolicyError> {
        Policy::from_json(&self.0)
    }
}
