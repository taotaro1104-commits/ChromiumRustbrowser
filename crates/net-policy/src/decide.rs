use policy::Policy;
use url::{Host, Url};

/// リクエスト1件の判断材料。
#[derive(Debug, Clone)]
pub struct Request<'a> {
    pub url: &'a Url,
    /// トップレベルフレームの URL(ナビゲーション自体なら `None`)。
    pub top_frame: Option<&'a Url>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Block(BlockReason),
    UpgradeToHttps(Url),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
    BlockedHost,
    ContentBlocker,
}

/// 広告ブロックエンジンの抽象。フェーズ2で adblock-rust を実装する。
pub trait ContentBlocker {
    fn should_block(&self, request: &Request<'_>) -> bool;
}

/// 何もブロックしない既定実装。
pub struct NoopBlocker;

impl ContentBlocker for NoopBlocker {
    fn should_block(&self, _request: &Request<'_>) -> bool {
        false
    }
}

/// 判断順: ポリシーのブロックホスト → コンテンツブロッカー → HTTPS アップグレード → 許可。
pub fn decide(request: &Request<'_>, policy: &Policy, blocker: &dyn ContentBlocker) -> Decision {
    let url = request.url;
    if !matches!(url.scheme(), "http" | "https" | "ws" | "wss" | "ftp") {
        return Decision::Allow;
    }
    if let Some(host) = url.host_str() {
        // 末尾ドット付きの FQDN 表記(example.com.)でも同じホストとして照合する。
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if policy.is_host_blocked(&host) {
            return Decision::Block(BlockReason::BlockedHost);
        }
    }
    if blocker.should_block(request) {
        return Decision::Block(BlockReason::ContentBlocker);
    }
    // 非既定ポートは HTTPS が待ち受けていない可能性が高いので対象外。
    if policy.https_upgrade && url.scheme() == "http" && url.port().is_none() && is_upgradable(url)
    {
        let mut up = url.clone();
        // http→https は special scheme 同士なので失敗しない。既定ポート 80 は parse 時に落ちている。
        if up.set_scheme("https").is_ok() {
            return Decision::UpgradeToHttps(up);
        }
    }
    Decision::Allow
}

/// 公開証明書が成立しない内部向けの名前。
const PRIVATE_SUFFIXES: &[&str] = &[
    "localhost",
    "local",
    "lan",
    "internal",
    "home.arpa",
    "corp",
    "intranet",
];

/// ローカル・プライベート宛て(localhost / IP リテラル / 単一ラベル)は証明書が成立しないので対象外。
fn is_upgradable(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(d)) => {
            let d = d.trim_end_matches('.');
            let d = d.to_ascii_lowercase();
            d.contains('.')
                && !PRIVATE_SUFFIXES
                    .iter()
                    .any(|s| d == *s || d.ends_with(&format!(".{s}")))
        }
        _ => false,
    }
}
