use net_policy::BlockReason as Reason;
use net_policy::*;
use policy::Policy;
use url::Url;

fn u(s: &str) -> Url {
    Url::parse(s).unwrap()
}

fn policy(json: &str) -> Policy {
    Policy::from_json(json).unwrap()
}

fn decide_for(url: &str, p: &Policy) -> Decision {
    let url = u(url);
    decide(
        &Request {
            url: &url,
            top_frame: None,
        },
        p,
        &NoopBlocker,
    )
}

#[test]
fn http_is_upgraded_by_default_and_default_port_is_dropped() {
    let p = Policy::default();
    assert_eq!(
        decide_for("http://example.com:80/a?b=1#c", &p),
        Decision::UpgradeToHttps(u("https://example.com/a?b=1#c"))
    );
}

#[test]
fn upgrade_can_be_disabled_by_policy() {
    let p = policy(r#"{"https_upgrade": false}"#);
    assert_eq!(decide_for("http://example.com/", &p), Decision::Allow);
}

#[test]
fn local_and_ip_and_single_label_hosts_are_not_upgraded() {
    let p = Policy::default();
    for url in [
        "http://localhost:3000/",
        "http://dev.localhost/",
        "http://127.0.0.1/",
        "http://[::1]/",
        "http://intranet/",
    ] {
        assert_eq!(decide_for(url, &p), Decision::Allow, "{url}");
    }
}

#[test]
fn https_and_non_web_schemes_are_allowed() {
    let p = policy(r#"{"blocked_hosts": ["example.com"]}"#);
    assert_eq!(decide_for("about:blank", &p), Decision::Allow);
    assert_eq!(decide_for("file:///tmp/x.html", &p), Decision::Allow);
    assert_eq!(decide_for("https://other.example/", &p), Decision::Allow);
}

#[test]
fn blocked_hosts_block_host_and_subdomains_before_upgrade() {
    let p = policy(r#"{"blocked_hosts": ["ads.example"]}"#);
    assert_eq!(
        decide_for("http://ads.example/x", &p),
        Decision::Block(Reason::BlockedHost)
    );
    assert_eq!(
        decide_for("https://a.b.ADS.example/x", &p),
        Decision::Block(Reason::BlockedHost)
    );
    assert_eq!(decide_for("https://notads.example/x", &p), Decision::Allow);
}

#[test]
fn content_blocker_is_consulted_and_policy_block_wins() {
    struct BlockAll;
    impl ContentBlocker for BlockAll {
        fn should_block(&self, _: &Request<'_>) -> bool {
            true
        }
    }
    let url = u("https://cdn.example/ad.js");
    let req = Request {
        url: &url,
        top_frame: None,
    };
    assert_eq!(
        decide(&req, &Policy::default(), &BlockAll),
        Decision::Block(Reason::ContentBlocker)
    );
    let p = policy(r#"{"blocked_hosts": ["cdn.example"]}"#);
    assert_eq!(
        decide(&req, &p, &BlockAll),
        Decision::Block(Reason::BlockedHost)
    );
}

#[test]
fn third_party_detection() {
    let top = u("https://www.example.com/");
    assert!(!is_third_party(&u("https://www.example.com/x"), &top));
    assert!(!is_third_party(&u("https://img.www.example.com/x"), &top));
    assert!(is_third_party(&u("https://tracker.net/x"), &top));
    assert!(is_third_party(&u("https://example.com.evil.net/x"), &top));
    assert!(is_third_party(&u("https://cdn.example.com/x"), &top)); // 近似: 兄弟は第三者側に倒す
}

#[test]
fn cookie_policies() {
    let top = u("https://site.example/");
    let tp = u("https://tracker.net/p");
    let fp = u("https://site.example/p");
    let allow = policy(r#"{"cookie_policy": "allow"}"#);
    let block3 = Policy::default();
    let block_all = policy(r#"{"cookie_policy": "block_all"}"#);
    assert_eq!(
        cookie_decision(&tp, Some(&top), &allow),
        CookieDecision::Allow
    );
    assert_eq!(
        cookie_decision(&tp, Some(&top), &block3),
        CookieDecision::Block
    );
    assert_eq!(
        cookie_decision(&fp, Some(&top), &block3),
        CookieDecision::Allow
    );
    assert_eq!(cookie_decision(&tp, None, &block3), CookieDecision::Allow);
    assert_eq!(
        cookie_decision(&fp, Some(&top), &block_all),
        CookieDecision::Block
    );
}

#[test]
fn trailing_dot_and_websocket_do_not_bypass_blocking() {
    let p = policy(r#"{"blocked_hosts": ["example.com"]}"#);
    for url in [
        "https://ads.example.com./x",
        "wss://ads.example.com/s",
        "ws://example.com/s",
    ] {
        assert_eq!(
            decide_for(url, &p),
            Decision::Block(Reason::BlockedHost),
            "{url}"
        );
    }
}

#[test]
fn unicode_pattern_matches_punycode_host() {
    let p = policy(r#"{"blocked_hosts": ["例え.jp"]}"#);
    assert_eq!(
        decide_for("https://www.xn--r8jz45g.jp/", &p),
        Decision::Block(Reason::BlockedHost)
    );
}

#[test]
fn upgrade_skips_custom_ports_and_private_suffixes() {
    let p = Policy::default();
    assert_eq!(decide_for("http://example.com:8080/", &p), Decision::Allow);
    for url in [
        "http://printer.lan/",
        "http://x.local/",
        "http://a.corp/",
        "http://svc.internal/",
    ] {
        assert_eq!(decide_for(url, &p), Decision::Allow, "{url}");
    }
}
