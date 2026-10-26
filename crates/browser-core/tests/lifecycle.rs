use browser_core::*;
use ffi_traits::mock::{HostCall, MockHost};
use ffi_traits::{HostError, MemoryPressure, TabHandle};
use proptest::prelude::*;

const MIN: u64 = 60_000;

fn open(store: &mut Store<MockHost>, url: &str, now: u64) -> TabHandle {
    store
        .dispatch(Action::OpenTab {
            url: url.into(),
            now_ms: now,
        })
        .unwrap();
    store.state().active.unwrap()
}

#[test]
fn open_tab_activates_and_hides_previous() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let b = open(&mut s, "https://b.example", 10);
    assert_eq!(s.state().active, Some(b));
    let ta = s.state().tab(a).unwrap();
    assert_eq!(ta.visibility, Visibility::Hidden);
    assert_eq!(ta.hidden_since_ms, Some(10));
}

#[test]
fn hidden_tab_freezes_after_5_minutes_then_discards_after_30() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 0);
    s.dispatch(Action::Tick {
        now_ms: 5 * MIN - 1,
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Hidden);
    s.dispatch(Action::Tick { now_ms: 5 * MIN }).unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Frozen);
    s.dispatch(Action::Tick { now_ms: 30 * MIN }).unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Discarded);
    assert!(s.host().calls.contains(&HostCall::Freeze(a)));
    assert!(s.host().calls.contains(&HostCall::Discard(a)));
}

#[test]
fn long_gap_goes_straight_to_discard() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 0);
    s.dispatch(Action::Tick { now_ms: 60 * MIN }).unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Discarded);
    assert!(!s.host().calls.contains(&HostCall::Freeze(a)));
}

#[test]
fn protected_tabs_are_never_frozen_or_discarded() {
    for flags in [
        (true, false, false),
        (false, true, false),
        (false, false, true),
    ] {
        let mut s = Store::new(MockHost::new());
        let a = open(&mut s, "https://a.example", 0);
        open(&mut s, "https://b.example", 0);
        s.dispatch(Action::SetFlags {
            now_ms: 0,
            tab: a,
            audible: flags.0,
            form_editing: flags.1,
            webrtc: flags.2,
        })
        .unwrap();
        s.dispatch(Action::Tick { now_ms: 60 * MIN }).unwrap();
        s.dispatch(Action::MemoryPressure {
            level: MemoryPressure::Critical,
        })
        .unwrap();
        assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Hidden);
    }
}

#[test]
fn frozen_tab_that_becomes_protected_is_unfrozen() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 0);
    s.dispatch(Action::Tick { now_ms: 5 * MIN }).unwrap();
    s.dispatch(Action::SetFlags {
        now_ms: 0,
        tab: a,
        audible: true,
        form_editing: false,
        webrtc: false,
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Hidden);
    assert_eq!(s.host().calls.last(), Some(&HostCall::Unfreeze(a)));
}

#[test]
fn moderate_pressure_discards_least_recently_viewed_only() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let b = open(&mut s, "https://b.example", 100);
    open(&mut s, "https://c.example", 200);
    s.dispatch(Action::MemoryPressure {
        level: MemoryPressure::Moderate,
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Discarded);
    assert_eq!(s.state().tab(b).unwrap().visibility, Visibility::Hidden);
}

#[test]
fn critical_pressure_discards_all_hidden_but_never_the_active_tab() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let b = open(&mut s, "https://b.example", 100);
    let c = open(&mut s, "https://c.example", 200);
    s.dispatch(Action::MemoryPressure {
        level: MemoryPressure::Critical,
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Discarded);
    assert_eq!(s.state().tab(b).unwrap().visibility, Visibility::Discarded);
    assert_eq!(s.state().tab(c).unwrap().visibility, Visibility::Active);
}

#[test]
fn discarded_tab_keeps_only_restorable_info_and_restores_on_activate() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    s.dispatch(Action::UpdateMeta {
        tab: a,
        title: Some("A".into()),
        scroll_y: Some(420),
        form_state: Some(Some("name=x".into())),
    })
    .unwrap();
    open(&mut s, "https://b.example", 10);
    s.dispatch(Action::Tick { now_ms: 31 * MIN }).unwrap();
    let t = s.state().tab(a).unwrap();
    assert_eq!(
        (
            t.url.as_str(),
            t.title.as_str(),
            t.scroll_y,
            t.form_state.as_deref()
        ),
        ("https://a.example", "A", 420, Some("name=x"))
    );
    s.dispatch(Action::Activate {
        tab: a,
        now_ms: 32 * MIN,
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Active);
    assert_eq!(
        s.host().calls.last(),
        Some(&HostCall::Restore {
            tab: a,
            url: "https://a.example".into(),
            scroll_y: 420,
            form_state: Some("name=x".into()),
        })
    );
}

#[test]
fn activating_frozen_tab_unfreezes() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 0);
    s.dispatch(Action::Tick { now_ms: 6 * MIN }).unwrap();
    s.dispatch(Action::Activate {
        tab: a,
        now_ms: 7 * MIN,
    })
    .unwrap();
    assert_eq!(s.host().calls.last(), Some(&HostCall::Unfreeze(a)));
}

#[test]
fn closing_active_tab_activates_most_recently_used() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let b = open(&mut s, "https://b.example", 10);
    let c = open(&mut s, "https://c.example", 20);
    s.dispatch(Action::CloseTab { tab: c, now_ms: 30 }).unwrap();
    assert_eq!(s.state().active, Some(b));
    assert!(s.state().tab(a).is_some() && s.state().tab(c).is_none());
}

#[test]
fn navigate_only_applies_to_active_tab_and_resets_scroll() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let b = open(&mut s, "https://b.example", 1);
    s.dispatch(Action::Navigate {
        tab: a,
        url: "https://x.example".into(),
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().url, "https://a.example");
    s.dispatch(Action::UpdateMeta {
        tab: b,
        title: None,
        scroll_y: Some(9),
        form_state: None,
    })
    .unwrap();
    s.dispatch(Action::Navigate {
        tab: b,
        url: "https://y.example".into(),
    })
    .unwrap();
    assert_eq!(s.state().tab(b).unwrap().scroll_y, 0);
    assert_eq!(s.state().tab(b).unwrap().url, "https://y.example");
}

#[test]
fn unknown_tab_actions_are_noops() {
    let mut s = Store::new(MockHost::new());
    open(&mut s, "https://a.example", 0);
    let before = s.state().clone();
    for act in [
        Action::CloseTab {
            tab: TabHandle(99),
            now_ms: 1,
        },
        Action::Activate {
            tab: TabHandle(99),
            now_ms: 1,
        },
        Action::UpdateMeta {
            tab: TabHandle(99),
            title: Some("x".into()),
            scroll_y: None,
            form_state: None,
        },
        Action::SetFlags {
            now_ms: 0,
            tab: TabHandle(99),
            audible: true,
            form_editing: false,
            webrtc: false,
        },
    ] {
        s.dispatch(act).unwrap();
    }
    assert_eq!(*s.state(), before);
}

#[test]
fn host_failure_is_reported_but_state_is_still_updated() {
    let mut s = Store::new(MockHost::new());
    s.host_mut().fail_with = Some(HostError::Failed("x".into()));
    assert!(s
        .dispatch(Action::OpenTab {
            url: "https://a.example".into(),
            now_ms: 0
        })
        .is_err());
    assert_eq!(s.state().tabs.len(), 1);
}

#[test]
fn log_is_bounded_and_skips_ticks() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    for i in 0..2000 {
        s.dispatch(Action::Tick { now_ms: i }).unwrap();
    }
    assert_eq!(s.log().count(), 1);
    for i in 0..2000 {
        s.dispatch(Action::UpdateMeta {
            tab: a,
            title: Some(i.to_string()),
            scroll_y: None,
            form_state: None,
        })
        .unwrap();
    }
    assert_eq!(s.log().count(), 1024);
}

#[test]
fn reduce_does_not_mutate_input_state() {
    let (s1, _) = reduce(
        &State::default(),
        &Action::OpenTab {
            url: "https://a.example".into(),
            now_ms: 0,
        },
    );
    let before = s1.clone();
    let _ = reduce(&s1, &Action::Tick { now_ms: 99 * MIN });
    assert_eq!(s1, before);
}

#[test]
fn snapshot_lists_session_tabs() {
    let mut s = Store::new(MockHost::new());
    open(&mut s, "https://a.example", 0);
    let snap = s.state().snapshot();
    assert_eq!(snap.len(), 1);
    assert_eq!(snap[0].url, "https://a.example");
}

fn arb_action() -> impl Strategy<Value = Action> {
    let tab = (0u64..6).prop_map(TabHandle);
    prop_oneof![
        (0u64..10_000_000).prop_map(|n| Action::OpenTab {
            url: "https://x.example".into(),
            now_ms: n
        }),
        (tab.clone(), 0u64..10_000_000).prop_map(|(t, n)| Action::CloseTab { tab: t, now_ms: n }),
        (tab.clone(), 0u64..10_000_000).prop_map(|(t, n)| Action::Activate { tab: t, now_ms: n }),
        (tab, any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(t, a, f, w)| {
            Action::SetFlags {
                now_ms: 0,
                tab: t,
                audible: a,
                form_editing: f,
                webrtc: w,
            }
        }),
        (0u64..10_000_000).prop_map(|n| Action::Tick { now_ms: n }),
        prop_oneof![
            Just(MemoryPressure::Moderate),
            Just(MemoryPressure::Critical)
        ]
        .prop_map(|level| Action::MemoryPressure { level }),
    ]
}

proptest! {
    /// 任意の Action 列でも「保護中の Active/Hidden タブは時間・圧迫で Frozen/Discarded にならない」
    /// 「active は常に実在し Active」「Active は高々1枚」。
    #[test]
    fn invariants_hold_for_any_action_sequence(
        actions in proptest::collection::vec(arb_action(), 0..60)
    ) {
        let mut state = State::default();
        for a in &actions {
            let protected: Vec<_> = state
                .tabs
                .iter()
                .filter(|t| t.is_protected() && matches!(t.visibility, Visibility::Active | Visibility::Hidden))
                .map(|t| t.handle)
                .collect();
            let (next, _) = reduce(&state, a);
            if matches!(a, Action::Tick { .. } | Action::MemoryPressure { .. }) {
                for h in protected {
                    let t = next.tab(h).unwrap();
                    prop_assert!(matches!(t.visibility, Visibility::Active | Visibility::Hidden));
                }
            }
            if let Some(h) = next.active {
                prop_assert_eq!(next.tab(h).map(|t| t.visibility), Some(Visibility::Active));
            }
            prop_assert!(next.tabs.iter().filter(|t| t.visibility == Visibility::Active).count() <= 1);
            state = next;
        }
    }
}

#[test]
fn dangerous_urls_are_rejected_at_the_core_boundary() {
    let mut s = Store::new(MockHost::new());
    for url in [
        "javascript:alert(1)",
        "data:text/html,x",
        "file:///etc/passwd",
        "  JavaScript:x",
    ] {
        s.dispatch(Action::OpenTab {
            url: url.into(),
            now_ms: 0,
        })
        .unwrap();
    }
    assert!(s.state().tabs.is_empty() && s.host().calls.is_empty());
    let a = open(&mut s, "https://a.example", 0);
    s.dispatch(Action::Navigate {
        tab: a,
        url: "javascript:x".into(),
    })
    .unwrap();
    assert_eq!(s.state().tab(a).unwrap().url, "https://a.example");
}

#[test]
fn form_state_is_capped_and_redacted_in_log() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    let meta = |f: String| Action::UpdateMeta {
        tab: a,
        title: None,
        scroll_y: None,
        form_state: Some(Some(f)),
    };
    s.dispatch(meta("secret".into())).unwrap();
    s.dispatch(meta("x".repeat(MAX_FORM_STATE_BYTES + 1)))
        .unwrap();
    assert_eq!(
        s.state().tab(a).unwrap().form_state.as_deref(),
        Some("secret")
    );
    assert!(!format!("{:?}", s.log().collect::<Vec<_>>()).contains("secret"));
}

#[test]
fn grace_period_restarts_when_protection_ends() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 0);
    s.dispatch(Action::SetFlags {
        now_ms: 0,
        tab: a,
        audible: false,
        form_editing: true,
        webrtc: false,
    })
    .unwrap();
    s.dispatch(Action::SetFlags {
        now_ms: 40 * MIN,
        tab: a,
        audible: false,
        form_editing: false,
        webrtc: false,
    })
    .unwrap();
    s.dispatch(Action::Tick { now_ms: 41 * MIN }).unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Hidden);
    s.dispatch(Action::Tick { now_ms: 45 * MIN }).unwrap();
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Frozen);
}

#[test]
fn apply_path_matches_pure_reduce() {
    let mut s = Store::new(MockHost::new());
    let a = open(&mut s, "https://a.example", 0);
    open(&mut s, "https://b.example", 10);
    let before = s.state().clone();
    let act = Action::Tick { now_ms: 31 * MIN };
    let (pure_next, _) = reduce(&before, &act);
    s.dispatch(act).unwrap();
    assert_eq!(s.state(), &pure_next);
    assert_eq!(s.state().tab(a).unwrap().visibility, Visibility::Discarded);
}
