use browser_core::{Action, Store};
use ffi_traits::{HostError, MemoryPressure, TabHandle, WebContentsHost};
use net_policy::{decide, Decision, NoopBlocker, Request};
use omnibox::{classify, suggest, Input, Suggestion};
use policy::Policy;
use storage::{Db, StorageError};
use url::Url;

use crate::session::{SessionFile, SessionTabDto, MAX_RESTORED_TABS};

const SEARCH_ENGINE: &str = "https://duckduckgo.com/";

#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    #[error("host error: {0}")]
    Host(#[from] HostError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
    #[error("blocked by policy")]
    Blocked,
    #[error("session error: {0}")]
    Session(String),
}

pub struct Shell<H: WebContentsHost> {
    store: Store<H>,
    db: Db,
    policy: Policy,
}

impl<H: WebContentsHost> Shell<H> {
    pub fn new(host: H, db: Db, policy: Policy) -> Self {
        Self {
            store: Store::new(host),
            db,
            policy,
        }
    }

    pub fn store(&self) -> &Store<H> {
        &self.store
    }

    pub fn db(&self) -> &Db {
        &self.db
    }

    /// アドレスバーの確定。アクティブタブがあればそこへ、無ければ新規タブで開く。
    pub fn submit(&mut self, text: &str, now_ms: u64) -> Result<(), ShellError> {
        let target = match classify(text) {
            Input::Search(q) if q.is_empty() => return Ok(()),
            Input::Search(q) => search_url(&q),
            Input::Url(u) => u,
        };
        let url = self.apply_policy(target)?;
        match self.store.state().active {
            Some(tab) => self.store.dispatch(Action::Navigate {
                tab,
                url: url.into(),
            })?,
            None => self.store.dispatch(Action::OpenTab {
                url: url.into(),
                now_ms,
            })?,
        }
        Ok(())
    }

    /// 新規タブ(URL はポリシー検査後に開く)。
    pub fn open_tab(&mut self, url: &str, now_ms: u64) -> Result<(), ShellError> {
        let target = Url::parse(url).map_err(|e| ShellError::Session(e.to_string()))?;
        let url = self.apply_policy(target)?;
        self.store.dispatch(Action::OpenTab {
            url: url.into(),
            now_ms,
        })?;
        Ok(())
    }

    pub fn activate(&mut self, tab: TabHandle, now_ms: u64) -> Result<(), ShellError> {
        Ok(self.store.dispatch(Action::Activate { tab, now_ms })?)
    }

    pub fn close_tab(&mut self, tab: TabHandle, now_ms: u64) -> Result<(), ShellError> {
        Ok(self.store.dispatch(Action::CloseTab { tab, now_ms })?)
    }

    pub fn tick(&mut self, now_ms: u64) -> Result<(), ShellError> {
        Ok(self.store.dispatch(Action::Tick { now_ms })?)
    }

    pub fn memory_pressure(&mut self, level: MemoryPressure) -> Result<(), ShellError> {
        Ok(self.store.dispatch(Action::MemoryPressure { level })?)
    }

    /// ページの読込完了。URL・タイトルを状態へ反映し、http(s) のみ履歴に記録する。
    pub fn on_load_end(
        &mut self,
        tab: TabHandle,
        url: &str,
        title: &str,
        now_ms: u64,
    ) -> Result<(), ShellError> {
        if self.store.state().tab(tab).is_some_and(|t| t.url != url) {
            self.store.dispatch(Action::Committed {
                tab,
                url: url.to_owned(),
            })?;
        }
        self.store.dispatch(Action::UpdateMeta {
            tab,
            title: Some(title.to_owned()),
            scroll_y: None,
            form_state: None,
        })?;
        if is_web_url(url) {
            self.db.record_visit(url, title, now_ms)?;
        }
        Ok(())
    }

    /// ページのタイトル変更(履歴は読込完了時に記録する)。
    pub fn on_title_changed(&mut self, tab: TabHandle, title: &str) -> Result<(), ShellError> {
        Ok(self.store.dispatch(Action::UpdateMeta {
            tab,
            title: Some(title.to_owned()),
            scroll_y: None,
            form_state: None,
        })?)
    }

    /// ページ側で遷移が確定した(リンク・リダイレクト)。同じ URL なら何もしない。
    pub fn on_address_changed(&mut self, tab: TabHandle, url: &str) -> Result<(), ShellError> {
        if self.store.state().tab(tab).is_some_and(|t| t.url != url) {
            self.store.dispatch(Action::Committed {
                tab,
                url: url.to_owned(),
            })?;
        }
        Ok(())
    }

    /// アクティブタブのブックマークを切り替える。追加したら `true`、削除/対象なしは `false`。
    pub fn toggle_bookmark(&mut self, now_ms: u64) -> Result<bool, ShellError> {
        let state = self.store.state();
        let Some(tab) = state.active.and_then(|h| state.tab(h)) else {
            return Ok(false);
        };
        let (url, title) = (tab.url.clone(), tab.title.clone());
        if self.db.is_bookmarked(&url)? {
            self.db.remove_bookmark(&url)?;
            Ok(false)
        } else {
            self.db.add_bookmark(&url, &title, now_ms)?;
            Ok(true)
        }
    }

    pub fn suggest(
        &self,
        input: &str,
        now_ms: u64,
        limit: usize,
    ) -> Result<Vec<Suggestion>, ShellError> {
        Ok(suggest(&self.db, input, now_ms, limit)?)
    }

    pub fn save_session(&self, now_ms: u64) -> Result<(), ShellError> {
        let state = self.store.state();
        let active = state
            .active
            .and_then(|a| state.tabs.iter().position(|t| t.handle == a));
        let tabs = state
            .snapshot()
            .into_iter()
            .map(|t| SessionTabDto {
                url: t.url,
                title: t.title,
                scroll_y: t.scroll_y,
                form_state: t.form_state,
            })
            .collect();
        let json = SessionFile::new(active, tabs).to_json()?;
        self.db.save_session(&json, now_ms)?;
        Ok(())
    }

    /// 保存済みセッションを復元する。保存が無ければ `false`。
    /// 全タブを破棄状態で登録し、アクティブだったタブだけを実体化する(メモリ最小化)。
    pub fn restore_session(&mut self, now_ms: u64) -> Result<bool, ShellError> {
        let Some(json) = self.db.load_session()? else {
            return Ok(false);
        };
        let file = SessionFile::parse(&json)?;
        let mut handles: Vec<Option<TabHandle>> = Vec::new();
        for t in file.tabs.into_iter().take(MAX_RESTORED_TABS) {
            let before = self.store.state().tabs.len();
            self.store.dispatch(Action::RestoreTab {
                url: t.url,
                title: t.title,
                scroll_y: t.scroll_y,
                form_state: t.form_state,
                now_ms,
            })?;
            // core が拒否した(危険な URL 等)場合はタブが増えない。
            let added = self.store.state().tabs.len() > before;
            handles.push(added.then(|| self.store.state().tabs[before].handle));
        }
        let target = file
            .active
            .and_then(|i| handles.get(i).copied().flatten())
            .or_else(|| handles.iter().rev().flatten().next().copied());
        if let Some(tab) = target {
            self.store.dispatch(Action::Activate { tab, now_ms })?;
        }
        Ok(true)
    }

    /// ポリシー適用: ブロックなら拒否、HTTPS アップグレードがあれば反映。
    fn apply_policy(&self, url: Url) -> Result<Url, ShellError> {
        let req = Request {
            url: &url,
            top_frame: None,
        };
        match decide(&req, &self.policy, &NoopBlocker) {
            Decision::Allow => Ok(url),
            Decision::UpgradeToHttps(u) => Ok(u),
            Decision::Block(_) => Err(ShellError::Blocked),
        }
    }
}

fn is_web_url(url: &str) -> bool {
    Url::parse(url).is_ok_and(|u| matches!(u.scheme(), "http" | "https"))
}

fn search_url(query: &str) -> Url {
    // 定数の URL なので parse は失敗しない。
    let mut url = Url::parse(SEARCH_ENGINE).expect("constant search engine url");
    url.query_pairs_mut().append_pair("q", query);
    url
}
