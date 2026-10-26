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
    /// 定期タイマー。時間経過による凍結・破棄を判断する。
    Tick {
        now_ms: u64,
    },
    MemoryPressure {
        level: MemoryPressure,
    },
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
