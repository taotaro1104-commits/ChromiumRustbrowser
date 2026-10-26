//! ブラウザコア: 状態(State)と判断(reduce)。副作用は `Effect` として返し、
//! `ffi-traits` 経由で実行する(設計書 §6 browser-core)。

mod action;
mod reducer;
mod state;
mod store;

pub use action::{Action, Effect};
pub use reducer::{reduce, DISCARD_AFTER_MS, FREEZE_AFTER_MS, MAX_FORM_STATE_BYTES};
pub use state::{SessionTab, State, Tab, Visibility};
pub use store::Store;
