use ffi_traits::{MemoryPressure, TabHandle};

/// 状態を変える唯一の入口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    OpenTab {
        url: String,
        now_ms: u64,
    },
    CloseTab {
        tab: TabHandle,
        now_ms: u64,
    },
    Activate {
        tab: TabHandle,
        now_ms: u64,
    },
    /// アクティブタブのみ有効。非アクティブタブへの遷移要求は無視する。
    Navigate {
        tab: TabHandle,
        url: String,
    },
    /// ページ側で遷移が確定した(リンク・リダイレクト等)。host 呼び出しは伴わない。
    Committed {
        tab: TabHandle,
        url: String,
    },
    /// セッション復元用。host には何も要求せず、破棄状態のタブとして登録する。
    /// 実体はアクティブ化時の `Effect::Restore` で作られる。
    RestoreTab {
        url: String,
        title: String,
        scroll_y: u32,
        form_state: Option<String>,
        now_ms: u64,
    },
    UpdateMeta {
        tab: TabHandle,
        title: Option<String>,
        scroll_y: Option<u32>,
        form_state: Option<Option<String>>,
    },
    SetFlags {
        tab: TabHandle,
        audible: bool,
        form_editing: bool,
        webrtc: bool,
        now_ms: u64,
    },
    /// host の副作用が失敗した。State を実体と食い違わない状態へ戻す。`Store` が自動で発行する。
    HostFailed {
        tab: TabHandle,
        op: FailedOp,
    },
    /// 定期タイマー。時間経過による凍結・破棄を判断する。
    Tick {
        now_ms: u64,
    },
    MemoryPressure {
        level: MemoryPressure,
    },
}

/// 失敗した host 操作の種類(State の巻き戻し方が変わるもの)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailedOp {
    Create,
    Navigate,
    Restore,
}

/// reduce が返す副作用の指示。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Create {
        tab: TabHandle,
        url: String,
    },
    Navigate {
        tab: TabHandle,
        url: String,
    },
    Freeze {
        tab: TabHandle,
    },
    Unfreeze {
        tab: TabHandle,
    },
    Discard {
        tab: TabHandle,
    },
    Restore {
        tab: TabHandle,
        url: String,
        scroll_y: u32,
        form_state: Option<String>,
    },
    Close {
        tab: TabHandle,
    },
}
