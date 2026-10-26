use std::collections::VecDeque;

use ffi_traits::{HostError, TabHandle, WebContentsHost};

use crate::action::{Action, Effect, FailedOp};
use crate::reducer::apply;
use crate::state::State;

/// 診断ログにユーザー入力(フォーム内容)を残さない。
fn redact(action: Action) -> Action {
    match action {
        Action::UpdateMeta {
            tab,
            title,
            scroll_y,
            form_state: Some(Some(_)),
        } => Action::UpdateMeta {
            tab,
            title,
            scroll_y,
            form_state: Some(Some("<redacted>".into())),
        },
        Action::RestoreTab {
            url,
            title,
            scroll_y,
            form_state: Some(_),
            now_ms,
        } => Action::RestoreTab {
            url,
            title,
            scroll_y,
            form_state: Some("<redacted>".into()),
            now_ms,
        },
        other => other,
    }
}

/// 診断用ログの上限。常駐メモリを増やさないため有限リングにする。
const LOG_CAPACITY: usize = 1024;

/// 1 回の dispatch で行う巻き戻しの上限(失敗の連鎖で止まらなくなるのを防ぐ)。
const MAX_ROLLBACKS: usize = 16;

/// 失敗したときに巻き戻しが必要な副作用か。
fn failed_op(fx: &Effect) -> Option<(TabHandle, FailedOp)> {
    match fx {
        Effect::Create { tab, .. } => Some((*tab, FailedOp::Create)),
        Effect::Navigate { tab, .. } => Some((*tab, FailedOp::Navigate)),
        Effect::Restore { tab, .. } => Some((*tab, FailedOp::Restore)),
        _ => None,
    }
}

/// State と host を束ねる。状態更新は reduce のみ、host 呼び出しは Effect のみ。
pub struct Store<H: WebContentsHost> {
    state: State,
    host: H,
    log: VecDeque<Action>,
}

impl<H: WebContentsHost> Store<H> {
    pub fn new(host: H) -> Self {
        Self {
            state: State::default(),
            host,
            log: VecDeque::new(),
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn host(&self) -> &H {
        &self.host
    }

    pub fn host_mut(&mut self) -> &mut H {
        &mut self.host
    }

    /// 直近の Action(Tick を除く、最大 1024 件)。診断・テスト用で、復元には `State::snapshot` を使う。
    pub fn log(&self) -> impl Iterator<Item = &Action> {
        self.log.iter()
    }

    /// Action を適用し副作用を実行する。host が失敗しても残りの副作用は実行し、最初のエラーを返す。
    /// 実体を作れなかった等の失敗は、`HostFailed` で State を巻き戻す(実体の無い Active タブを残さない)。
    pub fn dispatch(&mut self, action: Action) -> Result<(), HostError> {
        let mut first_err = None;
        let mut queue = VecDeque::from([action]);
        let mut budget = MAX_ROLLBACKS;
        while let Some(action) = queue.pop_front() {
            let effects = apply(&mut self.state, &action);
            if !matches!(action, Action::Tick { .. }) {
                if self.log.len() == LOG_CAPACITY {
                    self.log.pop_front();
                }
                self.log.push_back(redact(action));
            }
            for fx in effects {
                let failure = failed_op(&fx);
                if let Err(e) = self.run(fx) {
                    first_err.get_or_insert(e);
                    if let Some((tab, op)) = failure.filter(|_| budget > 0) {
                        budget -= 1;
                        queue.push_back(Action::HostFailed { tab, op });
                    }
                }
            }
        }
        first_err.map_or(Ok(()), Err)
    }

    fn run(&mut self, fx: Effect) -> Result<(), HostError> {
        match fx {
            Effect::Create { tab, url } => self.host.create_tab(tab, &url),
            Effect::Navigate { tab, url } => self.host.navigate(tab, &url),
            Effect::Freeze { tab } => self.host.freeze(tab),
            Effect::Unfreeze { tab } => self.host.unfreeze(tab),
            Effect::Discard { tab } => self.host.discard(tab),
            Effect::Restore {
                tab,
                url,
                scroll_y,
                form_state,
            } => self
                .host
                .restore(tab, &url, scroll_y, form_state.as_deref()),
            Effect::Close { tab } => self.host.close_tab(tab),
        }
    }
}
