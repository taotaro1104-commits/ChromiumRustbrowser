use ffi_traits::{MemoryPressure, TabHandle};

use crate::action::{Action, Effect};
use crate::state::{State, Tab, Visibility};

/// 非表示になってから凍結するまで(5分)。
pub const FREEZE_AFTER_MS: u64 = 5 * 60 * 1000;
/// 非表示になってから破棄するまで(30分)。
pub const DISCARD_AFTER_MS: u64 = 30 * 60 * 1000;

/// フォーム状態として保持する上限(バイト)。
pub const MAX_FORM_STATE_BYTES: usize = 64 * 1024;

/// タブに渡してよい URL スキーム。`javascript:` / `data:` / `file:` 等は拒否する。
fn is_allowed_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    ["http://", "https://", "about:"]
        .iter()
        .any(|p| lower.starts_with(p))
}

/// 純粋関数。入力の State は変更せず、新しい State と副作用を返す。
/// 実行時の経路は複製を避ける `apply` を使い、こちらはテスト・検証用。
pub fn reduce(state: &State, action: &Action) -> (State, Vec<Effect>) {
    let mut next = state.clone();
    let fx = apply(&mut next, action);
    (next, fx)
}

/// State をその場で更新し、副作用を返す(`reduce` と同じ結果。全タブの複製を避ける)。
pub(crate) fn apply(next: &mut State, action: &Action) -> Vec<Effect> {
    let mut fx = Vec::new();
    match action {
        Action::OpenTab { url, now_ms } => open_tab(next, url, *now_ms, &mut fx),
        Action::CloseTab { tab, now_ms } => close_tab(next, *tab, *now_ms, &mut fx),
        Action::Activate { tab, now_ms } => activate(next, *tab, *now_ms, &mut fx),
        Action::Navigate { tab, url } => navigate(next, *tab, url, &mut fx),
        Action::Committed { tab, url } => committed(next, *tab, url),
        Action::RestoreTab {
            url,
            title,
            scroll_y,
            form_state,
            now_ms,
        } => restore_tab(next, url, title, *scroll_y, form_state.as_deref(), *now_ms),
        Action::UpdateMeta {
            tab,
            title,
            scroll_y,
            form_state,
        } => {
            if let Some(t) = next.tab_mut(*tab) {
                if let Some(v) = title {
                    t.title.clone_from(v);
                }
                if let Some(v) = scroll_y {
                    t.scroll_y = *v;
                }
                if let Some(v) = form_state {
                    // 巨大な入力は保持しない(メモリとセッション永続化の上限)。
                    if v.as_ref().is_none_or(|f| f.len() <= MAX_FORM_STATE_BYTES) {
                        t.form_state.clone_from(v);
                    }
                }
            }
        }
        Action::SetFlags {
            tab,
            audible,
            form_editing,
            webrtc,
            now_ms,
        } => set_flags(
            next,
            *tab,
            (*audible, *form_editing, *webrtc),
            *now_ms,
            &mut fx,
        ),
        Action::Tick { now_ms } => tick(next, *now_ms, &mut fx),
        Action::MemoryPressure { level } => memory_pressure(next, *level, &mut fx),
    }
    fx
}

fn open_tab(s: &mut State, url: &str, now: u64, fx: &mut Vec<Effect>) {
    if !is_allowed_url(url) {
        return;
    }
    hide_active(s, now);
    let handle = TabHandle(s.next_id);
    s.next_id += 1;
    s.tabs.push(Tab {
        handle,
        url: url.to_owned(),
        title: String::new(),
        scroll_y: 0,
        form_state: None,
        visibility: Visibility::Active,
        hidden_since_ms: None,
        last_active_ms: now,
        audible: false,
        form_editing: false,
        webrtc: false,
    });
    s.active = Some(handle);
    fx.push(Effect::Create {
        tab: handle,
        url: url.to_owned(),
    });
}

fn committed(s: &mut State, tab: TabHandle, url: &str) {
    if !is_allowed_url(url) {
        return;
    }
    if let Some(t) = s.tab_mut(tab) {
        t.url = url.to_owned();
        t.scroll_y = 0;
        t.form_state = None;
    }
}

fn restore_tab(
    s: &mut State,
    url: &str,
    title: &str,
    scroll_y: u32,
    form_state: Option<&str>,
    now: u64,
) {
    if !is_allowed_url(url) {
        return;
    }
    let handle = TabHandle(s.next_id);
    s.next_id += 1;
    s.tabs.push(Tab {
        handle,
        url: url.to_owned(),
        title: title.to_owned(),
        scroll_y,
        form_state: form_state
            .filter(|f| f.len() <= MAX_FORM_STATE_BYTES)
            .map(str::to_owned),
        visibility: Visibility::Discarded,
        hidden_since_ms: Some(now),
        last_active_ms: now,
        audible: false,
        form_editing: false,
        webrtc: false,
    });
}

fn close_tab(s: &mut State, tab: TabHandle, now: u64, fx: &mut Vec<Effect>) {
    let Some(pos) = s.tabs.iter().position(|t| t.handle == tab) else {
        return;
    };
    s.tabs.remove(pos);
    fx.push(Effect::Close { tab });
    if s.active == Some(tab) {
        s.active = None;
        let successor = s
            .tabs
            .iter()
            .max_by_key(|t| t.last_active_ms)
            .map(|t| t.handle);
        if let Some(h) = successor {
            activate(s, h, now, fx);
        }
    }
}

fn hide_active(s: &mut State, now: u64) {
    let Some(cur) = s.active.take() else { return };
    if let Some(t) = s.tab_mut(cur) {
        t.visibility = Visibility::Hidden;
        t.hidden_since_ms = Some(now);
        t.last_active_ms = now;
    }
}

fn activate(s: &mut State, tab: TabHandle, now: u64, fx: &mut Vec<Effect>) {
    if s.tab(tab).is_none() || s.active == Some(tab) {
        return;
    }
    hide_active(s, now);
    let Some(t) = s.tab_mut(tab) else { return };
    match t.visibility {
        Visibility::Frozen => fx.push(Effect::Unfreeze { tab }),
        Visibility::Discarded => fx.push(Effect::Restore {
            tab,
            url: t.url.clone(),
            scroll_y: t.scroll_y,
            form_state: t.form_state.clone(),
        }),
        Visibility::Active | Visibility::Hidden => {}
    }
    t.visibility = Visibility::Active;
    t.hidden_since_ms = None;
    t.last_active_ms = now;
    s.active = Some(tab);
}

fn navigate(s: &mut State, tab: TabHandle, url: &str, fx: &mut Vec<Effect>) {
    if s.active != Some(tab) || !is_allowed_url(url) {
        return;
    }
    if let Some(t) = s.tab_mut(tab) {
        t.url = url.to_owned();
        t.scroll_y = 0;
        t.form_state = None;
        fx.push(Effect::Navigate {
            tab,
            url: url.to_owned(),
        });
    }
}

fn set_flags(
    s: &mut State,
    tab: TabHandle,
    flags: (bool, bool, bool),
    now: u64,
    fx: &mut Vec<Effect>,
) {
    let Some(t) = s.tab_mut(tab) else { return };
    let was_protected = t.is_protected();
    (t.audible, t.form_editing, t.webrtc) = flags;
    // 保護が終わった時点から改めて猶予時間を数える(直後に即破棄しない)。
    if was_protected && !t.is_protected() && t.hidden_since_ms.is_some() {
        t.hidden_since_ms = Some(now);
    }
    // 凍結後に保護対象になったら即座に解凍する。
    if t.is_protected() && t.visibility == Visibility::Frozen {
        t.visibility = Visibility::Hidden;
        fx.push(Effect::Unfreeze { tab });
    }
}

fn tick(s: &mut State, now: u64, fx: &mut Vec<Effect>) {
    for t in &mut s.tabs {
        let Some(since) = t.hidden_since_ms else {
            continue;
        };
        if t.is_protected() {
            continue;
        }
        let age = now.saturating_sub(since);
        match t.visibility {
            Visibility::Hidden | Visibility::Frozen if age >= DISCARD_AFTER_MS => {
                t.visibility = Visibility::Discarded;
                fx.push(Effect::Discard { tab: t.handle });
            }
            Visibility::Hidden if age >= FREEZE_AFTER_MS => {
                t.visibility = Visibility::Frozen;
                fx.push(Effect::Freeze { tab: t.handle });
            }
            _ => {}
        }
    }
}

/// Moderate は最も長く見ていない1枚、Critical は破棄可能な全タブを破棄する。
fn memory_pressure(s: &mut State, level: MemoryPressure, fx: &mut Vec<Effect>) {
    let mut candidates: Vec<(u64, TabHandle)> = s
        .tabs
        .iter()
        .filter(|t| {
            matches!(t.visibility, Visibility::Hidden | Visibility::Frozen) && !t.is_protected()
        })
        .map(|t| (t.last_active_ms, t.handle))
        .collect();
    candidates.sort();
    let take = match level {
        MemoryPressure::Moderate => 1,
        MemoryPressure::Critical => candidates.len(),
    };
    for (_, handle) in candidates.into_iter().take(take) {
        if let Some(t) = s.tab_mut(handle) {
            t.visibility = Visibility::Discarded;
            fx.push(Effect::Discard { tab: handle });
        }
    }
}
