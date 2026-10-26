//! 保存用の URL から秘匿情報を取り除く。履歴・ブックマーク・セッションに残す URL に使う。
//! 表示中のタブの URL(メモリ上)は変えない。取り除くのは認証情報・トークン類だけで、他のクエリは保つ。

use url::Url;

/// 名前にこの語を含むクエリ/フラグメントのキーは秘匿対象。
const SENSITIVE_PARTS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "pwd",
    "apikey",
    "api_key",
    "api-key",
    "auth",
    "credential",
    "signature",
    "session",
    "jwt",
    "otp",
];
/// 名前が完全一致したときだけ秘匿対象にする短いキー(部分一致だと誤爆が多い)。
const SENSITIVE_EXACT: &[&str] = &["code", "key", "sig", "sid", "state", "ticket"];

fn is_sensitive(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    SENSITIVE_EXACT.contains(&k.as_str()) || SENSITIVE_PARTS.iter().any(|p| k.contains(p))
}

/// `key=value&...` 形式から秘匿キーを除いた文字列。残りが無ければ `None`。
fn filter_pairs(raw: &str) -> Option<String> {
    let kept: Vec<&str> = raw
        .split('&')
        .filter(|pair| {
            let key = pair.split_once('=').map_or(*pair, |(k, _)| k);
            !pair.is_empty() && !is_sensitive(key)
        })
        .collect();
    (!kept.is_empty()).then(|| kept.join("&"))
}

/// ユーザー名・パスワード、秘匿キーのクエリ、トークンを含むフラグメントを除いた URL を返す。
/// 解析できない文字列はそのまま返す(保存の可否は呼び出し側が決める)。
pub fn scrub_url(raw: &str) -> String {
    let Ok(mut url) = Url::parse(raw) else {
        return raw.to_owned();
    };
    // 認証情報の除去は失敗しうる(cannot-be-a-base)が、その場合そもそも保持していない。
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let query = url.query().and_then(filter_pairs);
    url.set_query(query.as_deref());
    // `#access_token=...` のようなフラグメントはキー付きのときだけ同様に処理する。
    if let Some(frag) = url.fragment().filter(|f| f.contains('=')) {
        let kept = filter_pairs(frag);
        url.set_fragment(kept.as_deref());
    }
    url.into()
}

#[cfg(test)]
mod tests {
    use super::scrub_url;

    #[test]
    fn userinfo_is_removed() {
        assert_eq!(
            scrub_url("https://alice:hunter2@example.com/a"),
            "https://example.com/a"
        );
    }

    #[test]
    fn sensitive_query_keys_are_dropped_and_others_kept() {
        assert_eq!(
            scrub_url("https://a.example/cb?code=abc&state=xyz&q=rust&page=2&access_token=t"),
            "https://a.example/cb?q=rust&page=2"
        );
        assert_eq!(
            scrub_url("https://a.example/?API_KEY=1&Password=2&sessionId=3&sig=4&sid=5"),
            "https://a.example/"
        );
    }

    #[test]
    fn harmless_lookalike_keys_survive() {
        // `code` 完全一致のみ対象。`zipcode` や `keyboard` は残す。
        assert_eq!(
            scrub_url("https://a.example/?zipcode=100&keyboard=jp&q=x"),
            "https://a.example/?zipcode=100&keyboard=jp&q=x"
        );
    }

    #[test]
    fn token_fragments_are_removed() {
        assert_eq!(
            scrub_url("https://a.example/cb#access_token=abc&token_type=bearer&section=top"),
            "https://a.example/cb#section=top"
        );
        assert_eq!(
            scrub_url("https://a.example/cb#id_token=abc"),
            "https://a.example/cb"
        );
        // `=` を含まない普通のアンカーは保つ。
        assert_eq!(
            scrub_url("https://a.example/doc#intro"),
            "https://a.example/doc#intro"
        );
    }

    #[test]
    fn unparsable_or_clean_urls_are_unchanged() {
        assert_eq!(scrub_url("not a url"), "not a url");
        assert_eq!(
            scrub_url("https://a.example/x?q=1"),
            "https://a.example/x?q=1"
        );
    }
}
