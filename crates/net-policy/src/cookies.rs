use policy::{CookiePolicy, Policy};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieDecision {
    Allow,
    Block,
}

/// 同一サイト判定の簡易版: ホストが同一、または一方が他方のサブドメイン。
/// Public Suffix List を使う厳密な eTLD+1 判定はフェーズ2(//net の判定へ委譲する)で置き換える。
/// この近似は兄弟サブドメイン(a.example.com と b.example.com)を第三者と見なす側に倒れる。
pub fn is_third_party(request: &Url, top_frame: &Url) -> bool {
    let (Some(a), Some(b)) = (request.host_str(), top_frame.host_str()) else {
        return true;
    };
    let (a, b) = (
        a.trim_end_matches('.').to_ascii_lowercase(),
        b.trim_end_matches('.').to_ascii_lowercase(),
    );
    !(a == b || a.ends_with(&format!(".{b}")) || b.ends_with(&format!(".{a}")))
}

/// トップフレーム不明(ナビゲーション自体)は第一者として扱う。
pub fn cookie_decision(request: &Url, top_frame: Option<&Url>, policy: &Policy) -> CookieDecision {
    match policy.cookie_policy {
        CookiePolicy::Allow => CookieDecision::Allow,
        CookiePolicy::BlockAll => CookieDecision::Block,
        CookiePolicy::BlockThirdParty => match top_frame {
            Some(top) if is_third_party(request, top) => CookieDecision::Block,
            _ => CookieDecision::Allow,
        },
    }
}
