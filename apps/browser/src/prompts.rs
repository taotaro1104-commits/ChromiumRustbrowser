//! 確認バーに出す質問と、ボタンを押したときの結果(CEF に依存しない)。
//! パスワードは利用者が答えるまで `Zeroizing` で保持し、メッセージ文やログには含めない。
//! 答えが無いまま一定時間が過ぎたものは捨てる(パスワードを長くメモリに置かないため)。

use ffi_traits::TabHandle;
use zeroize::Zeroizing;

/// 質問の有効期間(ミリ秒)。
pub const PROMPT_TTL_MS: u64 = 5 * 60_000;
/// 自動入力の候補として並べるユーザー名の最大数(ボタンが溢れないように)。
pub const MAX_FILL_CHOICES: usize = 2;

pub enum Prompt {
    /// ログインの保存(または更新)を尋ねる。
    SaveLogin {
        tab: TabHandle,
        url: String,
        username: String,
        password: Zeroizing<String>,
        update: bool,
        since_ms: u64,
    },
    /// 保存済みのログインを入力するか尋ねる。
    FillLogin {
        tab: TabHandle,
        url: String,
        usernames: Vec<String>,
        since_ms: u64,
    },
    /// 結果の通知(「保存しました」など)。
    Notice {
        tab: TabHandle,
        text: String,
        since_ms: u64,
    },
}

/// ボタンを押した結果。
pub enum Outcome {
    Save {
        url: String,
        username: String,
        password: Zeroizing<String>,
    },
    NeverSave {
        url: String,
    },
    Fill {
        url: String,
        username: String,
    },
    Dismiss,
}

impl Prompt {
    pub fn tab(&self) -> TabHandle {
        match self {
            Prompt::SaveLogin { tab, .. }
            | Prompt::FillLogin { tab, .. }
            | Prompt::Notice { tab, .. } => *tab,
        }
    }

    fn since_ms(&self) -> u64 {
        match self {
            Prompt::SaveLogin { since_ms, .. }
            | Prompt::FillLogin { since_ms, .. }
            | Prompt::Notice { since_ms, .. } => *since_ms,
        }
    }

    pub fn expired(&self, now_ms: u64) -> bool {
        now_ms.saturating_sub(self.since_ms()) > PROMPT_TTL_MS
    }

    /// バーに出す文。パスワードは含めない。
    pub fn message(&self) -> String {
        match self {
            Prompt::SaveLogin {
                username, update, ..
            } => {
                if *update {
                    format!("「{username}」のパスワードを更新しますか?")
                } else {
                    format!("「{username}」のパスワードを保存しますか?")
                }
            }
            Prompt::FillLogin { .. } => "保存済みのログイン情報があります".to_owned(),
            Prompt::Notice { text, .. } => text.clone(),
        }
    }

    /// ボタンのラベル(左から順)。`choose` の添字と対応する。
    pub fn buttons(&self) -> Vec<String> {
        match self {
            Prompt::SaveLogin { update, .. } => {
                let mut b = vec![
                    if *update { "更新" } else { "保存" }.to_owned(),
                    "今回はしない".to_owned(),
                ];
                if !*update {
                    b.push("このサイトでは保存しない".to_owned());
                }
                b
            }
            Prompt::FillLogin { usernames, .. } => usernames
                .iter()
                .take(MAX_FILL_CHOICES)
                .map(|u| format!("「{u}」で入力"))
                .chain(std::iter::once("閉じる".to_owned()))
                .collect(),
            Prompt::Notice { .. } => vec!["閉じる".to_owned()],
        }
    }

    /// `index` 番目のボタンが押されたときの結果。範囲外は何もせず閉じる。
    pub fn choose(self, index: usize) -> Outcome {
        match self {
            Prompt::SaveLogin {
                url,
                username,
                password,
                update,
                ..
            } => match (index, update) {
                (0, _) => Outcome::Save {
                    url,
                    username,
                    password,
                },
                (2, false) => Outcome::NeverSave { url },
                _ => Outcome::Dismiss,
            },
            Prompt::FillLogin { url, usernames, .. } => usernames
                .into_iter()
                .take(MAX_FILL_CHOICES)
                .nth(index)
                .map_or(Outcome::Dismiss, |username| Outcome::Fill { url, username }),
            Prompt::Notice { .. } => Outcome::Dismiss,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: TabHandle = TabHandle(1);

    fn save(update: bool) -> Prompt {
        Prompt::SaveLogin {
            tab: T,
            url: "https://a.example/login".into(),
            username: "alice".into(),
            password: Zeroizing::new("s3cret".into()),
            update,
            since_ms: 1000,
        }
    }

    #[test]
    fn save_prompt_offers_save_skip_and_never() {
        let p = save(false);
        assert_eq!(
            p.buttons(),
            ["保存", "今回はしない", "このサイトでは保存しない"]
        );
        assert!(matches!(p.choose(0), Outcome::Save { .. }));
        assert!(matches!(save(false).choose(1), Outcome::Dismiss));
        assert!(matches!(save(false).choose(2), Outcome::NeverSave { .. }));
        assert!(matches!(save(false).choose(9), Outcome::Dismiss));
    }

    #[test]
    fn update_prompt_has_no_never_button() {
        let p = save(true);
        assert_eq!(p.buttons(), ["更新", "今回はしない"]);
        assert!(matches!(save(true).choose(2), Outcome::Dismiss));
    }

    #[test]
    fn messages_never_contain_the_password() {
        for p in [save(false), save(true)] {
            let m = p.message();
            assert!(m.contains("alice") && !m.contains("s3cret"), "{m}");
        }
    }

    #[test]
    fn fill_prompt_maps_buttons_to_usernames_and_caps_the_choices() {
        let fill = || Prompt::FillLogin {
            tab: T,
            url: "https://a.example/".into(),
            usernames: vec!["a".into(), "b".into(), "c".into()],
            since_ms: 0,
        };
        assert_eq!(fill().buttons(), ["「a」で入力", "「b」で入力", "閉じる"]);
        assert!(matches!(fill().choose(1), Outcome::Fill { username, .. } if username == "b"));
        // 3 番目のユーザー名は候補に出ないので、押しても入力されない。
        assert!(matches!(fill().choose(2), Outcome::Dismiss));
        assert!(matches!(fill().choose(3), Outcome::Dismiss));
    }

    #[test]
    fn prompts_expire() {
        let p = save(false);
        assert!(!p.expired(1000 + PROMPT_TTL_MS));
        assert!(p.expired(1000 + PROMPT_TTL_MS + 1));
        assert!(!p.expired(0), "clock going backwards must not expire");
    }

    #[test]
    fn notice_closes() {
        let n = Prompt::Notice {
            tab: T,
            text: "保存しました".into(),
            since_ms: 0,
        };
        assert_eq!(n.message(), "保存しました");
        assert_eq!(n.buttons(), ["閉じる"]);
        assert!(matches!(n.choose(0), Outcome::Dismiss));
    }
}
