use ffi_traits::mock::{HostCall, MockHost};
use policy::Policy;
use shell_core::{Shell, ShellError, MAX_SESSION_BYTES};
use storage::Db;

fn shell() -> Shell<MockHost> {
    Shell::new(
        MockHost::new(),
        Db::open_in_memory().unwrap(),
        Policy::default(),
    )
}

fn shell_with(policy: &str) -> Shell<MockHost> {
    Shell::new(
        MockHost::new(),
        Db::open_in_memory().unwrap(),
        Policy::from_json(policy).unwrap(),
    )
}

#[test]
fn submit_without_tabs_opens_one_and_with_active_navigates() {
    let mut s = shell();
    s.submit("example.com", 1).unwrap();
    assert_eq!(s.store().state().tabs.len(), 1);
    assert_eq!(
        s.store().host().calls.last(),
        Some(&HostCall::Create(
            s.store().state().active.unwrap(),
            "https://example.com/".into()
        ))
    );
    s.submit("https://other.example/x", 2).unwrap();
    assert_eq!(s.store().state().tabs.len(), 1);
    assert!(matches!(
        s.store().host().calls.last(),
        Some(HostCall::Navigate(_, u)) if u == "https://other.example/x"
    ));
}

#[test]
fn search_text_goes_to_search_engine_with_encoding() {
    let mut s = shell();
    s.submit("rust & cef 入門", 1).unwrap();
    let url = &s.store().state().tabs[0].url;
    assert!(url.starts_with("https://duckduckgo.com/?q="), "{url}");
    assert!(!url.contains(' ') && !url.contains("& "), "{url}");
}

#[test]
fn http_is_upgraded_and_blocked_hosts_are_refused() {
    let mut s = shell_with(r#"{"blocked_hosts": ["ads.example"]}"#);
    s.submit("http://news.example/a", 1).unwrap();
    assert_eq!(s.store().state().tabs[0].url, "https://news.example/a");
    let err = s.submit("https://x.ads.example/", 2).unwrap_err();
    assert!(matches!(err, ShellError::Blocked));
    assert_eq!(s.store().state().tabs[0].url, "https://news.example/a");
}

#[test]
fn empty_submit_is_a_noop() {
    let mut s = shell();
    s.submit("   ", 1).unwrap();
    assert!(s.store().state().tabs.is_empty());
}

#[test]
fn load_end_records_history_title_and_url() {
    let mut s = shell();
    s.submit("example.com", 1).unwrap();
    let tab = s.store().state().active.unwrap();
    s.on_load_end(tab, "https://example.com/landing", "Example", 5)
        .unwrap();
    let t = s.store().state().tab(tab).unwrap();
    assert_eq!(
        (t.url.as_str(), t.title.as_str()),
        ("https://example.com/landing", "Example")
    );
    let hits = s.suggest("exam", 6, 5).unwrap();
    assert!(hits
        .iter()
        .any(|h| h.target == "https://example.com/landing"));
}

#[test]
fn non_http_loads_are_not_recorded_in_history() {
    let mut s = shell();
    s.submit("about:blank", 1).unwrap();
    let tab = s.store().state().active.unwrap();
    s.on_load_end(tab, "about:blank", "", 2).unwrap();
    assert_eq!(s.db().history_count().unwrap(), 0);
}

#[test]
fn internal_pages_are_not_recorded_in_history() {
    let mut s = shell();
    s.set_internal_prefix("http://127.0.0.1:4000/");
    s.submit("http://127.0.0.1:4000/", 1).unwrap();
    let tab = s.store().state().active.unwrap();
    s.on_load_end(tab, "http://127.0.0.1:4000/", "start", 2)
        .unwrap();
    assert_eq!(s.db().history_count().unwrap(), 0);
    s.on_load_end(tab, "http://127.0.0.1:40001/", "other", 3)
        .unwrap();
    assert_eq!(
        s.db().history_count().unwrap(),
        1,
        "prefix must match whole origin"
    );
}

#[test]
fn bookmark_toggle_round_trip() {
    let mut s = shell();
    assert!(
        !s.toggle_bookmark(1).unwrap(),
        "no active tab: nothing happens"
    );
    s.submit("example.com", 1).unwrap();
    let tab = s.store().state().active.unwrap();
    s.on_load_end(tab, "https://example.com/", "Example", 2)
        .unwrap();
    assert!(s.toggle_bookmark(3).unwrap());
    assert!(s.db().is_bookmarked("https://example.com/").unwrap());
    assert!(!s.toggle_bookmark(4).unwrap());
    assert!(!s.db().is_bookmarked("https://example.com/").unwrap());
}

#[test]
fn session_round_trip_restores_lazily_with_only_active_tab_created() {
    let mut a = shell();
    a.submit("a.example", 1).unwrap();
    let ta = a.store().state().active.unwrap();
    a.on_load_end(ta, "https://a.example/", "A", 2).unwrap();
    a.open_tab("https://b.example/", 3).unwrap();
    let tb = a.store().state().active.unwrap();
    a.on_load_end(tb, "https://b.example/", "B", 4).unwrap();
    a.save_session(9).unwrap();
    let saved = a.db().load_session().unwrap().unwrap();
    assert!(!saved.contains("javascript"));

    let mut b = shell();
    b.db().save_session(&saved, 10).unwrap();
    assert!(b.restore_session(20).unwrap());
    let st = b.store().state();
    assert_eq!(st.tabs.len(), 2);
    // ホストには、アクティブだった B の復元だけが送られる。
    let calls = &b.store().host().calls;
    assert_eq!(calls.len(), 1);
    assert!(matches!(&calls[0], HostCall::Restore { url, .. } if url == "https://b.example/"));
    assert_eq!(st.tab(st.active.unwrap()).unwrap().title, "B");
}

#[test]
fn restore_without_saved_session_returns_false() {
    let mut s = shell();
    assert!(!s.restore_session(1).unwrap());
}

#[test]
fn corrupt_or_oversized_session_is_rejected_without_side_effects() {
    let mut s = shell();
    s.db().save_session("not json", 1).unwrap();
    assert!(matches!(s.restore_session(2), Err(ShellError::Session(_))));
    s.db()
        .save_session(&"x".repeat(MAX_SESSION_BYTES + 1), 3)
        .unwrap();
    assert!(matches!(s.restore_session(4), Err(ShellError::Session(_))));
    assert!(s.store().state().tabs.is_empty());
}

#[test]
fn session_with_hostile_urls_drops_them() {
    let mut s = shell();
    s.db()
        .save_session(
            r#"{"version":1,"active":0,"tabs":[
                {"url":"javascript:alert(1)","title":"x","scroll_y":0,"form_state":null},
                {"url":"https://ok.example/","title":"ok","scroll_y":3,"form_state":null}]}"#,
            1,
        )
        .unwrap();
    assert!(s.restore_session(2).unwrap());
    assert_eq!(s.store().state().tabs.len(), 1);
    assert_eq!(s.store().state().tabs[0].url, "https://ok.example/");
    assert!(s.store().state().active.is_some());
}

#[test]
fn unknown_session_version_is_rejected() {
    let mut s = shell();
    s.db()
        .save_session(r#"{"version":99,"active":null,"tabs":[]}"#, 1)
        .unwrap();
    assert!(matches!(s.restore_session(2), Err(ShellError::Session(_))));
}

#[test]
fn close_and_activate_pass_through_to_store() {
    let mut s = shell();
    s.submit("a.example", 1).unwrap();
    let a = s.store().state().active.unwrap();
    s.open_tab("https://b.example/", 2).unwrap();
    s.activate(a, 3).unwrap();
    assert_eq!(s.store().state().active, Some(a));
    s.close_tab(a, 4).unwrap();
    assert_eq!(s.store().state().tabs.len(), 1);
}

#[test]
fn tick_and_memory_pressure_drive_lifecycle() {
    let mut s = shell();
    s.submit("a.example", 0).unwrap();
    s.open_tab("https://b.example/", 10).unwrap();
    s.tick(31 * 60_000).unwrap();
    assert!(s
        .store()
        .host()
        .calls
        .iter()
        .any(|c| matches!(c, HostCall::Discard(_))));
}

#[test]
fn title_and_address_events_update_state_without_touching_history() {
    let mut s = shell();
    s.submit("example.com", 1).unwrap();
    let tab = s.store().state().active.unwrap();
    let calls = s.store().host().calls.len();
    s.on_title_changed(tab, "Hello").unwrap();
    s.on_address_changed(tab, "https://example.com/next")
        .unwrap();
    s.on_address_changed(tab, "javascript:alert(1)").unwrap();
    let t = s.store().state().tab(tab).unwrap();
    assert_eq!(
        (t.title.as_str(), t.url.as_str()),
        ("Hello", "https://example.com/next")
    );
    assert_eq!(s.db().history_count().unwrap(), 0);
    assert_eq!(
        s.store().host().calls.len(),
        calls,
        "no host call for page-driven events"
    );
}

#[test]
fn internal_and_file_urls_are_reported_not_swallowed() {
    let mut s = shell();
    for text in ["file:///c:/secret.txt", "about:flags"] {
        assert!(
            matches!(s.submit(text, 1), Err(ShellError::UnsupportedUrl(_))),
            "{text}"
        );
    }
    assert!(s.store().state().tabs.is_empty());
}

#[test]
fn restore_skips_policy_blocked_hosts() {
    let mut a = shell();
    a.open_tab("https://ok.example/", 1).unwrap();
    a.open_tab("https://bad.example/", 2).unwrap();
    a.save_session(3).unwrap();
    let json = a.db().load_session().unwrap().unwrap();

    let mut b = shell_with(r#"{"blocked_hosts":["bad.example"]}"#);
    b.db().save_session(&json, 4).unwrap();
    assert!(b.restore_session(5).unwrap());
    let urls: Vec<_> = b
        .store()
        .state()
        .tabs
        .iter()
        .map(|t| t.url.clone())
        .collect();
    assert_eq!(urls, vec!["https://ok.example/".to_string()]);
}
