use url::{Host, Url};

/// 保存・自動入力の対象にできるオリジンを返す(対象外なら `None`)。
/// - `https://host[:port]` は対象。
/// - `http://` は開発用のループバック(`localhost` / `127.0.0.1` / `[::1]`)だけ対象。
/// - それ以外(`file:` / `data:` / 内部ページ / 平文 http の一般サイト)は対象外。
pub fn normalize_origin(url: &str) -> Option<String> {
    let u = Url::parse(url).ok()?;
    let allowed = match u.scheme() {
        "https" => true,
        "http" => match u.host()? {
            Host::Domain(d) => d.eq_ignore_ascii_case("localhost"),
            Host::Ipv4(ip) => ip.is_loopback(),
            Host::Ipv6(ip) => ip.is_loopback(),
        },
        _ => false,
    };
    allowed.then(|| u.origin().ascii_serialization())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn https_origins_drop_path_query_and_userinfo() {
        assert_eq!(
            normalize_origin("https://user:pw@A.Example:8443/login?next=/x#f").as_deref(),
            Some("https://a.example:8443")
        );
        assert_eq!(
            normalize_origin("https://a.example/").as_deref(),
            Some("https://a.example")
        );
    }

    #[test]
    fn plain_http_is_only_for_loopback() {
        assert_eq!(
            normalize_origin("http://127.0.0.1:8765/login").as_deref(),
            Some("http://127.0.0.1:8765")
        );
        assert_eq!(
            normalize_origin("http://localhost/x").as_deref(),
            Some("http://localhost")
        );
        assert_eq!(
            normalize_origin("http://[::1]:3000/").as_deref(),
            Some("http://[::1]:3000")
        );
        assert_eq!(normalize_origin("http://a.example/"), None);
        assert_eq!(normalize_origin("http://127.0.0.1.evil.example/"), None);
    }

    #[test]
    fn other_schemes_and_garbage_are_rejected() {
        for u in [
            "file:///c:/x",
            "data:text/html,x",
            "about:blank",
            "javascript:alert(1)",
            "ftp://a.example/",
            "not a url",
            "",
        ] {
            assert_eq!(normalize_origin(u), None, "{u}");
        }
    }

    #[test]
    fn different_ports_and_subdomains_are_different_origins() {
        let a = normalize_origin("https://a.example/").unwrap();
        assert_ne!(a, normalize_origin("https://a.example:444/").unwrap());
        assert_ne!(a, normalize_origin("https://www.a.example/").unwrap());
        // 既定ポートは同じオリジン。
        assert_eq!(a, normalize_origin("https://a.example:443/").unwrap());
    }
}
