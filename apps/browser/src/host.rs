//! `WebContentsHost` の CEF 実装。タブ = Views の `BrowserView`。
//! 凍結(freeze)は P0 では未実装(非表示で CEF が自動的に抑制する)。実凍結は P1 の Content API で行う。

use std::cell::RefCell;
use std::collections::HashMap;

use cef::*;
use ffi_traits::{HostError, TabHandle, WebContentsHost};

use crate::handlers::TabClient;

wrap_browser_view_delegate! {
    struct TabViewDelegate {
        _unused: u8,
    }

    impl ViewDelegate {}

    impl BrowserViewDelegate {
        /// Chrome スタイルのウィンドウは Chrome スタイルの BrowserView を 1 つしか持てないため、
        /// タブは Alloy スタイルで作る。
        fn browser_runtime_style(&self) -> RuntimeStyle {
            RuntimeStyle::ALLOY
        }
    }
}

pub struct CefHost {
    content: Panel,
    views: HashMap<TabHandle, BrowserView>,
    /// 復元したタブの読込完了後に戻すスクロール位置。
    pending_scroll: RefCell<HashMap<TabHandle, u32>>,
}

impl CefHost {
    pub fn new(content: Panel) -> Self {
        Self {
            content,
            views: HashMap::new(),
            pending_scroll: RefCell::new(HashMap::new()),
        }
    }

    fn spawn(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        let mut client = TabClient::new(tab);
        let settings = BrowserSettings::default();
        let url = CefString::from(url);
        let mut delegate = TabViewDelegate::new(0);
        let view = browser_view_create(
            Some(&mut client),
            Some(&url),
            Some(&settings),
            None,
            None,
            Some(&mut delegate),
        )
        .ok_or_else(|| HostError::Failed("browser_view_create returned none".into()))?;
        let mut as_view = View::from(&view);
        self.content.add_child_view(Some(&mut as_view));
        self.views.insert(tab, view);
        Ok(())
    }

    fn despawn(&mut self, tab: TabHandle) {
        if let Some(view) = self.views.remove(&tab) {
            let mut as_view = View::from(&view);
            // Views 上のブラウザは close_browser を呼ぶとウィンドウごと閉じるため、
            // ビューを外して参照を手放すことで破棄する。
            self.content.remove_child_view(Some(&mut as_view));
        }
    }

    /// 状態の `active` に合わせて表示するタブを切り替える。
    pub fn show_only(&self, active: Option<TabHandle>) {
        for (tab, view) in &self.views {
            view.set_visible(i32::from(Some(*tab) == active));
        }
        self.content.invalidate_layout();
    }

    pub fn focus(&self, tab: TabHandle) {
        if let Some(view) = self.views.get(&tab) {
            view.request_focus();
        }
    }

    pub fn browser(&self, tab: TabHandle) -> Option<Browser> {
        self.views.get(&tab).and_then(|v| v.browser())
    }

    pub fn take_pending_scroll(&self, tab: TabHandle) -> Option<u32> {
        self.pending_scroll.borrow_mut().remove(&tab)
    }
}

impl WebContentsHost for CefHost {
    fn create_tab(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        self.spawn(tab, url)
    }

    fn navigate(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError> {
        let frame = self
            .browser(tab)
            .and_then(|b| b.main_frame())
            .ok_or(HostError::UnknownTab(tab))?;
        frame.load_url(Some(&CefString::from(url)));
        Ok(())
    }

    fn freeze(&mut self, _tab: TabHandle) -> Result<(), HostError> {
        Ok(())
    }

    fn unfreeze(&mut self, _tab: TabHandle) -> Result<(), HostError> {
        Ok(())
    }

    fn discard(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.despawn(tab);
        Ok(())
    }

    /// 実体が無ければ作り、スクロール位置は読込完了後に戻す。フォーム内容の復元は P1。
    fn restore(
        &mut self,
        tab: TabHandle,
        url: &str,
        scroll_y: u32,
        _form_state: Option<&str>,
    ) -> Result<(), HostError> {
        self.despawn(tab);
        self.spawn(tab, url)?;
        if scroll_y > 0 {
            self.pending_scroll.borrow_mut().insert(tab, scroll_y);
        }
        Ok(())
    }

    fn close_tab(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.despawn(tab);
        self.pending_scroll.borrow_mut().remove(&tab);
        Ok(())
    }
}
