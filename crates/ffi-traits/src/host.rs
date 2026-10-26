/// Chromium の `WebContents` を指す不透明ハンドル。ポインタではなく単なる ID。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TabHandle(pub u64);

/// OS のメモリ圧迫通知(`base::MemoryPressureListener` を shim 経由で転送)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    Moderate,
    Critical,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum HostError {
    #[error("unknown tab: {0:?}")]
    UnknownTab(TabHandle),
    #[error("host operation failed: {0}")]
    Failed(String),
}

/// 単調増加の時刻源(ミリ秒)。テストでは手動時計を注入する。
pub trait Clock {
    fn now_ms(&self) -> u64;
}

/// Chromium 側に対して Rust が下す命令。判断は Rust、実行は Chromium。
pub trait WebContentsHost {
    fn create_tab(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError>;
    fn navigate(&mut self, tab: TabHandle, url: &str) -> Result<(), HostError>;
    /// ページ凍結(JS 実行停止)。
    fn freeze(&mut self, tab: TabHandle) -> Result<(), HostError>;
    fn unfreeze(&mut self, tab: TabHandle) -> Result<(), HostError>;
    /// `WebContents` を破棄して Renderer を解放する。
    fn discard(&mut self, tab: TabHandle) -> Result<(), HostError>;
    /// 破棄済みタブを URL から再構築する。
    fn restore(
        &mut self,
        tab: TabHandle,
        url: &str,
        scroll_y: u32,
        form_state: Option<&str>,
    ) -> Result<(), HostError>;
    fn close_tab(&mut self, tab: TabHandle) -> Result<(), HostError>;
}
