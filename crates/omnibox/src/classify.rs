use std::net::Ipv4Addr;

use url::{Host, Url};

/// 入力の解釈結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Input {
    Url(Url),
    Search(String),
}

/// 明示スキームとして受け付けるもの。`javascript:` / `data:` 等は URL として扱わず検索にする。
const EXPLICIT_PREFIXES: &[&str] = &["http://", "https://", "file://", "about:"];

pub fn classify(raw: &str) -> Input {
    let text = raw.trim();
    if text.is_empty() {
        return Input::Search(String::new());
    }
    let lower = text.to_ascii_lowercase();
    if EXPLICIT_PREFIXES.iter().any(|p| lower.starts_with(p)) {
        return Url::parse(text).map_or_else(|_| Input::Search(text.to_owned()), Input::Url);
    }
    // `///evil.com` や `\\host` を url クレートがホストとして解釈するのを防ぐ。
    if text.starts_with(['/', '\\']) || text.contains('\\') || text.chars().any(char::is_whitespace)
    {
        return Input::Search(text.to_owned());
    }
    match parse_bare_host(text) {
        Some(url) => Input::Url(url),
        None => Input::Search(text.to_owned()),
    }
}

/// `example.com/path`, `localhost:3000`, `192.168.0.1`, `[::1]` のようなスキーム無しの入力。
fn parse_bare_host(text: &str) -> Option<Url> {
    let url = Url::parse(&format!("https://{text}")).ok()?;
    if !url.username().is_empty() || url.password().is_some() {
        return None;
    }
    let looks_like_host = match url.host()? {
        Host::Domain(d) => d.eq_ignore_ascii_case("localhost") || has_valid_dotted_name(d),
        // "1.2.3" のような省略形が IPv4 に正規化されるのを避け、入力どおりの4組だけ受け付ける。
        Host::Ipv4(a) => bare_host_part(text).parse::<Ipv4Addr>() == Ok(a),
        Host::Ipv6(_) => text.starts_with('['),
    };
    looks_like_host.then_some(url)
}

/// パス・クエリ・フラグメント・ポートを除いたホスト部分。
fn bare_host_part(text: &str) -> &str {
    text.split(['/', '?', '#', ':']).next().unwrap_or(text)
}

/// ファイル名として打たれやすい拡張子。実在の TLD(.rs, .md, .py, .sh …)でも、スキーム無しの入力は
/// 検索として扱い、意図しない第三者ドメインへのアクセスを避ける。
const FILE_EXTENSIONS: &[&str] = &[
    "rs", "md", "py", "sh", "txt", "html", "htm", "js", "ts", "json", "toml", "yaml", "yml", "css",
    "cpp", "java", "go", "rb", "php", "pdf", "png", "jpg", "jpeg", "gif", "zip", "exe", "log",
    "csv", "xml", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "mp3", "mp4", "tar", "gz",
];

/// 「a.b」形式で全ラベルが空でなく、最後のラベル(TLD)が2文字以上の英字(または punycode)。
fn has_valid_dotted_name(domain: &str) -> bool {
    let labels: Vec<&str> = domain.trim_end_matches('.').split('.').collect();
    if labels.len() < 2 || labels.iter().any(|l| l.is_empty()) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    !FILE_EXTENSIONS.contains(&tld.to_ascii_lowercase().as_str())
        && tld.len() >= 2
        && (tld.chars().all(|c| c.is_ascii_alphabetic()) || tld.starts_with("xn--"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_of(s: &str) -> String {
        match classify(s) {
            Input::Url(u) => u.to_string(),
            Input::Search(q) => panic!("expected url, got search {q:?}"),
        }
    }

    fn is_search(s: &str) -> bool {
        matches!(classify(s), Input::Search(_))
    }

    #[test]
    fn explicit_schemes_are_urls() {
        assert_eq!(url_of("https://example.com"), "https://example.com/");
        assert_eq!(
            url_of("  HTTP://Example.com/a b "),
            "http://example.com/a%20b"
        );
        assert_eq!(url_of("about:blank"), "about:blank");
    }

    #[test]
    fn bare_hosts_become_https() {
        assert_eq!(url_of("example.com"), "https://example.com/");
        assert_eq!(
            url_of("example.com/path?q=1"),
            "https://example.com/path?q=1"
        );
        assert_eq!(
            url_of("sub.example.co.jp:8443/x"),
            "https://sub.example.co.jp:8443/x"
        );
        assert_eq!(url_of("localhost"), "https://localhost/");
        assert_eq!(url_of("localhost:3000"), "https://localhost:3000/");
        assert_eq!(url_of("192.168.0.1"), "https://192.168.0.1/");
        assert_eq!(url_of("[::1]"), "https://[::1]/");
    }

    #[test]
    fn plain_words_and_sentences_are_searches() {
        for s in [
            "rust",
            "rust lang",
            "what is example.com",
            "foo.",
            ".com",
            "a..b",
            "file.txt1",
        ] {
            assert!(is_search(s), "{s}");
        }
        assert_eq!(classify(""), Input::Search(String::new()));
        assert_eq!(classify("   "), Input::Search(String::new()));
    }

    #[test]
    fn dangerous_schemes_are_never_urls() {
        for s in [
            "javascript:alert(1)",
            "data:text/html,<b>x</b>",
            "vbscript:x",
            "blob:https://a/b",
        ] {
            assert!(is_search(s), "{s}");
        }
    }

    #[test]
    fn credentials_in_bare_input_are_rejected() {
        assert!(is_search("user:pass@example.com"));
    }

    #[test]
    fn numbers_that_are_not_ips_are_searches() {
        assert!(is_search("12345"));
        assert!(is_search("1.2.3"));
    }

    #[test]
    fn invalid_explicit_url_falls_back_to_search() {
        assert!(is_search("http://"));
    }

    #[test]
    fn idn_tld_is_accepted() {
        assert!(matches!(classify("例え.jp"), Input::Url(_)));
    }
}
