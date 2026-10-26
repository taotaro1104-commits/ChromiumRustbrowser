//! テスト用モック(feature `mock`)。呼び出しを記録する。

use std::cell::Cell;

use crate::host::{Clock, HostError, TabHandle, WebContentsHost};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostCall {
    Create(TabHandle, String),
    Navigate(TabHandle, String),
    Freeze(TabHandle),
    Unfreeze(TabHandle),
    Discard(TabHandle),
    Restore {
        tab: TabHandle,
        url: String,
        scroll_y: u32,
        form_state: Option<String>,
    },
    Close(TabHandle),
}

#[derive(Debug, Default)]
pub struct MockHost {
    pub calls: Vec<HostCall>,
    /// `Some` の間は全操作がこのエラーで失敗する。
    pub fail_with: Option<HostError>,
}

impl MockHost {
    pub fn new() -> Self {
        Self::default()
    }

    fn record(&mut self, call: HostCall) -> Result<(), HostError> {
        if let Some(err) = &self.fail_with {
            return Err(match err {
                HostError::UnknownTab(t) => HostError::UnknownTab(*t),
                HostError::Failed(m) => HostError::Failed(m.clone()),
            });
        }
        self.calls.push(call);
        Ok(())
    }
}

impl WebContentsHost for MockHost {
    fn create_tab(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        self.record(HostCall::Create(tab, url.to_owned()))
    }
    fn navigate(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        self.record(HostCall::Navigate(tab, url.to_owned()))
    }
    fn freeze(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.record(HostCall::Freeze(tab))
    }
    fn unfreeze(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.record(HostCall::Unfreeze(tab))
    }
    fn discard(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.record(HostCall::Discard(tab))
    }
    fn restore(
        &mut self,
        tab: TabHandle,
        url: &str,
        scroll_y: u32,
        form_state: Option<&str>,
    ) -> Result<(), HostError> {
        self.record(HostCall::Restore {
            tab,
            url: url.to_owned(),
            scroll_y,
            form_state: form_state.map(str::to_owned),
        })
    }
    fn close_tab(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.record(HostCall::Close(tab))
    }
}

/// 手動で進める時計。
#[derive(Debug, Default)]
pub struct ManualClock(Cell<u64>);

impl ManualClock {
    pub fn new(start_ms: u64) -> Self {
        Self(Cell::new(start_ms))
    }
    pub fn advance(&self, ms: u64) {
        self.0.set(self.0.get() + ms);
    }
}

impl Clock for ManualClock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_calls_in_order() {
        let mut h = MockHost::new();
        h.create_tab(TabHandle(1), "https://a.example").unwrap();
        h.freeze(TabHandle(1)).unwrap();
        assert_eq!(
            h.calls,
            vec![
                HostCall::Create(TabHandle(1), "https://a.example".into()),
                HostCall::Freeze(TabHandle(1)),
            ]
        );
    }

    #[test]
    fn injected_failure_is_returned_and_not_recorded() {
        let mut h = MockHost::new();
        h.fail_with = Some(HostError::UnknownTab(TabHandle(9)));
        assert_eq!(
            h.discard(TabHandle(9)),
            Err(HostError::UnknownTab(TabHandle(9)))
        );
        h.fail_with = Some(HostError::Failed("boom".into()));
        assert!(h.close_tab(TabHandle(1)).is_err());
        assert!(h.calls.is_empty());
    }

    #[test]
    fn manual_clock_advances() {
        let c = ManualClock::new(10);
        c.advance(5);
        assert_eq!(c.now_ms(), 15);
    }
}
