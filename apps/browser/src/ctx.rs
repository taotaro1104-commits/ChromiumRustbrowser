//! UI スレッド専用のアプリ状態。CEF のコールバックから `with` 経由でのみ触る。
//! 再入(コールバック中の再コールバック)で借用が衝突した場合は、その呼び出しを黙って捨てる。

use std::cell::RefCell;

use cef::ImplBrowser;
use ffi_traits::TabHandle;
use shell_core::Shell;

use crate::handlers::post_ui;
use crate::host::CefHost;
use crate::profile::now_ms;
use crate::ui::{TabLabel, Ui, UiAction};

/// スタートページを起動できなかったときのホーム。
pub const FALLBACK_HOME_URL: &str = "https://duckduckgo.com/";

thread_local! {
    static CTX: RefCell<Option<Ctx>> = const { RefCell::new(None) };
}

pub fn install(ctx: Ctx) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

pub fn clear() {
    CTX.with(|c| *c.borrow_mut() = None);
}

pub fn with<R>(f: impl FnOnce(&mut Ctx) -> R) -> Option<R> {
    CTX.with(|c| {
        let mut guard = c.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

pub struct Ctx {
    pub shell: Shell<CefHost>,
    pub ui: Ui,
    sync_pending: bool,
    home_url: String,
}

fn report<T, E: std::fmt::Display>(r: Result<T, E>) {
    if let Err(e) = r {
        eprintln!("browser: {e}");
    }
}

impl Ctx {
    pub fn new(shell: Shell<CefHost>, ui: Ui, home_url: String) -> Self {
        Self {
            shell,
            ui,
            sync_pending: false,
            home_url,
        }
    }

    /// 起動: 前回のセッションを復元し、無ければホームを開く。`--url` があれば新規タブで開く。
    pub fn start(&mut self, initial_url: Option<&str>) {
        let restored = match self.shell.restore_session(now_ms()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("browser: session restore failed: {e}");
                false
            }
        };
        if let Some(url) = initial_url.filter(|u| !u.is_empty()) {
            report(self.shell.open_tab(url, now_ms()));
        } else if !restored || self.shell.store().state().tabs.is_empty() {
            report(self.shell.open_tab(&self.home_url, now_ms()));
        }
        self.sync();
    }

    pub fn act(&mut self, action: UiAction) {
        let now = now_ms();
        let active = self.shell.store().state().active;
        match action {
            UiAction::Back => self.with_browser(active, |b| b.go_back()),
            UiAction::Forward => self.with_browser(active, |b| b.go_forward()),
            UiAction::Reload => self.with_browser(active, |b| b.reload()),
            UiAction::Bookmark => report(self.shell.toggle_bookmark(now)),
            UiAction::NewTab => report(self.shell.open_tab(&self.home_url, now)),
            UiAction::ActivateTab(tab) => {
                report(self.shell.activate(tab, now));
                self.shell.store().host().focus(tab);
            }
            UiAction::CloseTab(tab) => {
                report(self.shell.close_tab(tab, now));
                if self.shell.store().state().tabs.is_empty() {
                    report(self.shell.open_tab(&self.home_url, now));
                }
            }
        }
        self.request_sync();
    }

    fn with_browser(&self, tab: Option<TabHandle>, f: impl FnOnce(&cef::Browser)) {
        if let Some(b) = tab.and_then(|t| self.shell.store().host().browser(t)) {
            f(&b);
        }
    }

    pub fn submit_address(&mut self, text: &str) {
        report(self.shell.submit(text, now_ms()));
        self.request_sync();
    }

    pub fn open_url_in_new_tab(&mut self, url: &str) {
        report(self.shell.open_tab(url, now_ms()));
        self.request_sync();
    }

    pub fn on_title(&mut self, tab: TabHandle, title: &str) {
        report(self.shell.on_title_changed(tab, title));
        self.request_sync();
    }

    pub fn on_address(&mut self, tab: TabHandle, url: &str) {
        report(self.shell.on_address_changed(tab, url));
        self.request_sync();
    }

    pub fn on_load_end(&mut self, tab: TabHandle, url: &str) {
        let title = self
            .shell
            .store()
            .state()
            .tab(tab)
            .map(|t| t.title.clone())
            .unwrap_or_default();
        report(self.shell.on_load_end(tab, url, &title, now_ms()));
        self.request_sync();
    }

    pub fn take_pending_scroll(&self, tab: TabHandle) -> Option<u32> {
        self.shell.store().host().take_pending_scroll(tab)
    }

    pub fn tick(&mut self) {
        report(self.shell.tick(now_ms()));
        report(self.shell.save_session(now_ms()));
        self.request_sync();
    }

    pub fn save_session(&self) {
        report(self.shell.save_session(now_ms()));
    }

    /// UI の更新は後で一度だけ行う(押下中のボタンを作り直さないため)。
    fn request_sync(&mut self) {
        if self.sync_pending {
            return;
        }
        self.sync_pending = true;
        post_ui(|| {
            crate::ctx::with(|c| {
                c.sync_pending = false;
                c.sync();
            });
        });
    }

    /// 状態を画面へ反映する(表示タブ・タブ列・アドレス・ブックマーク・タイトル)。
    pub fn sync(&mut self) {
        let state = self.shell.store().state();
        let active = state.active;
        self.shell.store().host().show_only(active);
        let labels: Vec<TabLabel> = state
            .tabs
            .iter()
            .map(|t| TabLabel {
                tab: t.handle,
                text: if t.title.is_empty() {
                    t.url.clone()
                } else {
                    t.title.clone()
                },
                url: t.url.clone(),
                active: Some(t.handle) == active,
            })
            .collect();
        let active_tab = active.and_then(|a| state.tab(a));
        let url = active_tab.map(|t| t.url.clone()).unwrap_or_default();
        let title = active_tab.map(|t| t.title.clone()).unwrap_or_default();
        let bookmarked = !url.is_empty() && self.shell.db().is_bookmarked(&url).unwrap_or(false);
        self.ui.set_tabs(&labels);
        self.ui.set_address(&url);
        self.ui.set_bookmarked(bookmarked);
        self.ui.set_window_title(&title);
    }
}
