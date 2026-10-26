use ffi_traits::TabHandle;

/// タブのライフサイクル状態(設計書 §4)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Visibility {
    Active,
    Hidden,
    Frozen,
    Discarded,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tab {
    pub handle: TabHandle,
    pub url: String,
    pub title: String,
    pub scroll_y: u32,
    pub form_state: Option<String>,
    pub visibility: Visibility,
    /// 非表示になった時刻。Active の間は `None`。
    pub hidden_since_ms: Option<u64>,
    /// 最後に表示されていた時刻(メモリ圧迫時の破棄順に使う)。
    pub last_active_ms: u64,
    pub audible: bool,
    pub form_editing: bool,
    pub webrtc: bool,
}

impl Tab {
    /// 音声再生中・フォーム入力中・WebRTC 通話中は凍結・破棄の対象外。
    pub fn is_protected(&self) -> bool {
        self.audible || self.form_editing || self.webrtc
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub tabs: Vec<Tab>,
    pub active: Option<TabHandle>,
    pub(crate) next_id: u64,
}

/// セッション復元に必要な最小情報。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionTab {
    pub url: String,
    pub title: String,
    pub scroll_y: u32,
    pub form_state: Option<String>,
}

impl State {
    pub fn tab(&self, handle: TabHandle) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.handle == handle)
    }

    pub(crate) fn tab_mut(&mut self, handle: TabHandle) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.handle == handle)
    }

    pub fn snapshot(&self) -> Vec<SessionTab> {
        self.tabs
            .iter()
            .map(|t| SessionTab {
                url: t.url.clone(),
                title: t.title.clone(),
                scroll_y: t.scroll_y,
                form_state: t.form_state.clone(),
            })
            .collect()
    }
}
