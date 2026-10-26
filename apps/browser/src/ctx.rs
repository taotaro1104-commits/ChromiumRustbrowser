//! UI スレッド専用のアプリ状態。CEF のコールバックから `with` 経由でのみ触る。
//! 再入(コールバック中の再コールバック)で借用が衝突した場合は、その呼び出しを黙って捨てる。

use std::cell::{Cell, RefCell};

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
    /// UI 更新の予約済みフラグ。`Ctx` の外に置くのは、借用に失敗した回でも必ず戻せるようにするため。
    static SYNC_PENDING: Cell<bool> = const { Cell::new(false) };
}

pub fn install(ctx: Ctx) {
    CTX.with(|c| *c.borrow_mut() = Some(ctx));
}

pub fn clear() {
    CTX.with(|c| *c.borrow_mut() = None);
}

pub fn with<R>(f: impl FnOnce(&mut Ctx) -> R) -> Option<R> {
    CTX.with(|c| {
        let Ok(mut guard) = c.try_borrow_mut() else {
            eprintln!("browser: reentrant ctx call dropped");
            return None;
        };
        guard.as_mut().map(f)
    })
}

pub struct Ctx {
    pub shell: Shell<CefHost>,
    pub ui: Ui,
    home_url: String,
    /// 前回のセッション復元に失敗した。古いセッションを上書きしないよう、自動保存を止める。
    save_disabled: bool,
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
            home_url,
            save_disabled: false,
        }
    }

    /// 起動: 前回のセッションを復元し、無ければホームを開く。起動時の URL(`--url` / `--urls-file`)があれば新規タブで開く。
    pub fn start(&mut self, startup_urls: &[String]) {
        let restored = match self.shell.restore_session(now_ms()) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("browser: session restore failed: {e}; autosave disabled for this run");
                self.save_disabled = true;
                false
            }
        };
        if !startup_urls.is_empty() {
            for url in startup_urls {
                report(self.shell.open_tab(url, now_ms()));
            }
        } else if !restored || self.shell.store().state().tabs.is_empty() {
            report(self.shell.open_tab(&self.home_url, now_ms()));
        }
        self.sync();
    }

    pub fn act(&mut self, action: UiAction) {
        let now = now_ms();
        let active = self.shell.store().state().active;
        // 切り替える前に、今のタブの状態(スクロール・入力中)を取得しておく。
        if let (Some(a), UiAction::NewTab | UiAction::ActivateTab(_)) = (active, action) {
            self.shell.store().host().request_page_state(a);
        }
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
        if let Err(e) = self.shell.submit(text, now_ms()) {
            eprintln!("browser: {e}");
            // 開けなかった入力を残さず、表示中のページの URL に戻す。
            self.ui.reset_address(&self.active_url());
        }
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

    fn active_url(&self) -> String {
        let state = self.shell.store().state();
        state
            .active
            .and_then(|a| state.tab(a))
            .map(|t| t.url.clone())
            .unwrap_or_default()
    }

    /// 音声の再生開始・停止。再生中のタブは凍結・破棄から保護される。
    pub fn on_audible(&mut self, tab: TabHandle, audible: bool) {
        report(self.shell.set_audible(tab, audible, now_ms()));
    }

    /// ページから届いた状態(スクロール・入力中・入力欄)を反映する。
    pub fn on_page_state(&mut self, tab: TabHandle, state: shell_core::PageState) {
        report(self.shell.update_page_state(tab, state, now_ms()));
    }

    pub fn take_pending_form(&self, tab: TabHandle) -> Option<String> {
        self.shell.store().host().take_pending_form(tab)
    }

    /// 実体のあるタブすべてに、ページ状態の取得を依頼する(結果は非同期に届く)。
    fn request_page_states(&self) {
        let state = self.shell.store().state();
        for t in state
            .tabs
            .iter()
            .filter(|t| t.visibility != browser_core::Visibility::Discarded)
        {
            self.shell.store().host().request_page_state(t.handle);
        }
    }

    /// ダウンロードの完了を知らせる(今はウィンドウのタイトルに出す。次の更新で元に戻る)。
    pub fn on_download_complete(&mut self, name: &str) {
        self.ui
            .set_window_title(&format!("ダウンロード完了: {name}"));
    }

    pub fn take_pending_scroll(&self, tab: TabHandle) -> Option<u32> {
        self.shell.store().host().take_pending_scroll(tab)
    }

    pub fn tick(&mut self) {
        self.request_page_states();
        self.apply_manual_memory_saver();
        report(self.shell.tick(now_ms()));
        self.save_session();
        self.request_sync();
    }

    /// 診断用: プロファイル直下に `discard-now` ファイルがあれば、非表示のタブを今すぐ破棄する
    /// (メモリ圧迫時と同じ動作)。ファイルは処理後に消す。メモリ削減効果の実測に使う。
    fn apply_manual_memory_saver(&mut self) {
        let trigger = crate::profile::profile_dir().join("discard-now");
        if trigger.exists() && std::fs::remove_file(&trigger).is_ok() {
            report(
                self.shell
                    .memory_pressure(ffi_traits::MemoryPressure::Critical),
            );
        }
    }

    pub fn save_session(&self) {
        if self.save_disabled {
            return;
        }
        report(self.shell.save_session(now_ms()));
    }

    /// UI の更新は後で一度だけ行う(押下中のボタンを作り直さないため)。
    fn request_sync(&mut self) {
        if SYNC_PENDING.with(|p| p.replace(true)) {
            return;
        }
        post_ui(|| {
            // 借用に失敗して `with` が走らなくても、予約フラグは必ず戻す。
            SYNC_PENDING.with(|p| p.set(false));
            crate::ctx::with(Ctx::sync);
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
        let bookmarked = !url.is_empty() && self.shell.is_bookmarked(&url).unwrap_or(false);
        self.ui.set_tabs(&labels);
        self.ui.set_address(&url);
        self.ui.set_bookmarked(bookmarked);
        self.ui.set_window_title(&title);
    }
}
