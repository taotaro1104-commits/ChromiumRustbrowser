use policy::*;

#[test]
fn empty_object_gives_secure_defaults() {
    let p = Policy::from_json("{}").unwrap();
    assert_eq!(p, Policy::default());
    assert!(p.https_upgrade);
    assert_eq!(p.cookie_policy, CookiePolicy::BlockThirdParty);
    assert_eq!(p.site_isolation, SiteIsolation::Full);
}

#[test]
fn full_policy_parses() {
    let p = Policy::from_json(
        r#"{
            "https_upgrade": false,
            "cookie_policy": "block_all",
            "blocked_hosts": ["Ads.Example.com", "*.tracker.net"],
            "homepage": "https://start.example/",
            "memory_saver": true,
            "site_isolation": "partial_logged_in"
        }"#,
    )
    .unwrap();
    assert!(!p.https_upgrade && p.memory_saver);
    assert_eq!(p.cookie_policy, CookiePolicy::BlockAll);
    assert_eq!(p.site_isolation, SiteIsolation::PartialLoggedIn);
    assert_eq!(p.blocked_hosts[0].as_str(), "ads.example.com");
    assert_eq!(p.blocked_hosts[1].as_str(), "tracker.net");
    assert_eq!(p.homepage.unwrap().as_str(), "https://start.example/");
}

#[test]
fn unknown_keys_and_bad_values_are_errors() {
    for bad in [
        r#"{"https_upgrde": true}"#,
        r#"{"cookie_policy": "maybe"}"#,
        r#"{"https_upgrade": "yes"}"#,
        r#"{"blocked_hosts": ["http://a.com"]}"#,
        r#"{"blocked_hosts": [""]}"#,
        "[]",
        "not json",
        "",
    ] {
        assert!(
            matches!(Policy::from_json(bad), Err(PolicyError::Json(_))),
            "{bad}"
        );
    }
}

#[test]
fn homepage_must_be_http_or_https() {
    for bad in [
        r#"{"homepage": "javascript:alert(1)"}"#,
        r#"{"homepage": "file:///etc/passwd"}"#,
        r#"{"homepage": "nope"}"#,
    ] {
        assert!(
            matches!(Policy::from_json(bad), Err(PolicyError::Homepage(_))),
            "{bad}"
        );
    }
}

#[test]
fn oversized_input_is_rejected_before_parsing() {
    let huge = format!(
        "{{\"blocked_hosts\": [{}]}}",
        "\"a.com\",".repeat(MAX_POLICY_BYTES / 8)
    );
    assert!(matches!(
        Policy::from_json(&huge),
        Err(PolicyError::TooLarge(_))
    ));
}

#[test]
fn json_source_implements_policy_source() {
    let src: Box<dyn PolicySource> = Box::new(JsonSource(r#"{"memory_saver": true}"#.into()));
    assert!(src.load().unwrap().memory_saver);
}
