//! `WebContentsHost` の CEF 実装。タブ = Views の `BrowserView`。
//! 凍結(freeze)は P0 では未実装(非表示で CEF が自動的に抑制する)。実凍結は P1 の Content API で行う。

use std::cell::RefCell;
use std::collections::HashMap;

use cef::*;
use ffi_traits::{HostError, TabHandle, WebContentsHost};

use crate::handlers::TabClient;

thread_local! {
    /// 閉じている途中のタブのビュー。ブラウザの破棄(`on_before_close`)が済むまで、親に載せたまま保持する。
    /// 先に外して手放すと CEF が異常終了することがある。
    static CLOSING: RefCell<Vec<(TabHandle, BrowserView, Panel)>> = const { RefCell::new(Vec::new()) };
}

/// このタブが「こちらから閉じている途中」か(ユーザーがウィンドウを閉じた場合と区別する)。
pub fn is_closing(tab: TabHandle) -> bool {
    CLOSING.with(|c| c.borrow().iter().any(|(t, ..)| *t == tab))
}

/// ビューを親から外して手放す(ビュー階層の取り外し)。これでブラウザの破棄が完了し、`on_before_close` が呼ばれる。
pub fn tab_closed(tab: TabHandle) {
    let entry = CLOSING.with(|c| {
        let mut v = c.borrow_mut();
        v.iter().position(|(t, ..)| *t == tab).map(|i| v.remove(i))
    });
    if let Some((_, view, parent)) = entry {
        parent.remove_child_view(Some(&mut View::from(&view)));
    }
}

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
    /// 復元したタブの読込完了後に戻す入力欄の内容。
    pending_form: RefCell<HashMap<TabHandle, String>>,
}

impl CefHost {
    pub fn new(content: Panel) -> Self {
        Self {
            content,
            views: HashMap::new(),
            pending_scroll: RefCell::new(HashMap::new()),
            pending_form: RefCell::new(HashMap::new()),
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
        // 古いスクロール位置が、次に実体化したページへ適用されないようにする。
        self.pending_scroll.borrow_mut().remove(&tab);
        self.pending_form.borrow_mut().remove(&tab);
        if let Some(view) = self.views.remove(&tab) {
            // 実体(レンダラー等)を確実に破棄する。`do_close` が届くまでビューは非表示のまま親に残し、
            // そこでビュー階層から外して完了させる(Views では DoClose が false だとウィンドウの閉じ要求になるため)。
            view.set_visible(0);
            let host = view.browser().and_then(|b| b.host());
            CLOSING.with(|c| c.borrow_mut().push((tab, view, self.content.clone())));
            if let Some(host) = host {
                host.close_browser(1);
            } else {
                tab_closed(tab);
            }
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

    pub fn take_pending_form(&self, tab: TabHandle) -> Option<String> {
        self.pending_form.borrow_mut().remove(&tab)
    }

    /// ページ状態を取得する JS を、実体のあるタブのメインフレームで実行する。結果は console 経由で届く。
    pub fn request_page_state(&self, tab: TabHandle) {
        if let Some(frame) = self.browser(tab).and_then(|b| b.main_frame()) {
            let js = CefString::from(crate::page_state::capture_script().as_str());
            frame.execute_java_script(Some(&js), None, 0);
        }
    }

    /// タブのメインフレームで JS を実行する(自動入力など)。実体が無ければ何もしない。
    pub fn run_script(&self, tab: TabHandle, script: &str) {
        if let Some(frame) = self.browser(tab).and_then(|b| b.main_frame()) {
            frame.execute_java_script(Some(&CefString::from(script)), None, 0);
        }
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

    /// 実体が無ければ作り、スクロール位置と入力欄の内容は読込完了後に戻す。
    fn restore(
        &mut self,
        tab: TabHandle,
        url: &str,
        scroll_y: u32,
        form_state: Option<&str>,
    ) -> Result<(), HostError> {
        self.despawn(tab);
        self.spawn(tab, url)?;
        if scroll_y > 0 {
            self.pending_scroll.borrow_mut().insert(tab, scroll_y);
        }
        if let Some(f) = form_state {
            self.pending_form.borrow_mut().insert(tab, f.to_owned());
        }
        Ok(())
    }

    fn close_tab(&mut self, tab: TabHandle) -> Result<(), HostError> {
        self.despawn(tab);
        self.pending_scroll.borrow_mut().remove(&tab);
        Ok(())
    }
}
