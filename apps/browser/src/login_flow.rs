//! パスワード管理の流れ(`Ctx` の一部): ログイン送信 → 確認バー → 保存、保存済みログインの自動入力の確認。
//! 質問は一度に 1 つ。表示するのは、その質問を出したタブが表示中のときだけ。

use ffi_traits::TabHandle;

use crate::ctx::Ctx;
use crate::login_capture::{LoginMessage, fill_script};
use crate::profile::now_ms;
use crate::prompts::{MAX_FILL_CHOICES, Outcome, Prompt};
use crate::ui::UiAction;
use shell_core::PromptKind;

impl Ctx {
    fn tab_url(&self, tab: TabHandle) -> Option<String> {
        self.shell.store().state().tab(tab).map(|t| t.url.clone())
    }

    /// ページから届いたログイン関連のメッセージを処理する。
    pub fn on_login_message(&mut self, tab: TabHandle, msg: LoginMessage) {
        let Some(url) = self.tab_url(tab) else { return };
        let now = now_ms();
        match msg {
            LoginMessage::Submitted { username, password } => {
                match self.shell.login_prompt(&url, &username, &password) {
                    Ok(kind @ (PromptKind::Save | PromptKind::Update)) => {
                        self.set_prompt(Prompt::SaveLogin {
                            tab,
                            url,
                            username,
                            password,
                            update: kind == PromptKind::Update,
                            since_ms: now,
                        });
                    }
                    Ok(PromptKind::None) => {}
                    Err(e) => eprintln!("browser: {e}"),
                }
            }
            LoginMessage::Probe {
                has_password_field: true,
            } => {
                // 他の質問が出ている間、別のタブの質問は出さない。
                if self.prompt.is_some() || self.shell.store().state().active != Some(tab) {
                    return;
                }
                match self.shell.saved_logins(&url) {
                    Ok(logins) if !logins.is_empty() => self.set_prompt(Prompt::FillLogin {
                        tab,
                        url,
                        usernames: logins
                            .into_iter()
                            .take(MAX_FILL_CHOICES)
                            .map(|l| l.username)
                            .collect(),
                        since_ms: now,
                    }),
                    Ok(_) => {}
                    Err(e) => eprintln!("browser: {e}"),
                }
            }
            LoginMessage::Probe { .. } => {}
        }
    }

    /// 管理画面からの削除。
    pub fn delete_login(&mut self, origin: &str, username: &str) {
        if let Err(e) = self.shell.delete_login(origin, username) {
            eprintln!("browser: {e}");
        }
    }

    fn set_prompt(&mut self, prompt: Prompt) {
        self.prompt = Some(prompt);
        self.refresh_prompt();
    }

    /// 質問の表示を、今の状態に合わせる。タブが無くなったら捨て、別のタブを見ている間は隠す。
    pub(crate) fn refresh_prompt(&mut self) {
        let state = self.shell.store().state();
        if self
            .prompt
            .as_ref()
            .is_some_and(|p| state.tab(p.tab()).is_none())
        {
            self.prompt = None;
        }
        match &self.prompt {
            Some(p) if state.active == Some(p.tab()) => {
                let buttons: Vec<(String, UiAction)> = p
                    .buttons()
                    .into_iter()
                    .enumerate()
                    .map(|(i, label)| (label, UiAction::Prompt(i)))
                    .collect();
                self.ui.show_prompt(&p.message(), &buttons);
            }
            _ => self.ui.hide_prompt(),
        }
    }

    /// 期限切れの質問を捨てる(パスワードをメモリに長く置かない)。
    pub(crate) fn expire_prompt(&mut self) {
        if self.prompt.as_ref().is_some_and(|p| p.expired(now_ms())) {
            self.prompt = None;
            self.refresh_prompt();
        }
    }

    /// 確認バーのボタンが押された。
    pub(crate) fn answer_prompt(&mut self, index: usize) {
        let Some(prompt) = self.prompt.take() else {
            return;
        };
        let tab = prompt.tab();
        let now = now_ms();
        let notice = match prompt.choose(index) {
            Outcome::Save {
                url,
                username,
                password,
            } => match self.shell.save_login(&url, &username, &password, now) {
                Ok(_) => Some("パスワードを保存しました".to_owned()),
                Err(e) => {
                    eprintln!("browser: {e}");
                    Some("パスワードを保存できませんでした".to_owned())
                }
            },
            Outcome::NeverSave { url } => match self.shell.never_save_logins(&url, now) {
                Ok(()) => Some("このサイトでは保存しません".to_owned()),
                Err(e) => {
                    eprintln!("browser: {e}");
                    None
                }
            },
            Outcome::Fill { url, username } => {
                self.fill(tab, &url, &username, now);
                None
            }
            Outcome::Dismiss => None,
        };
        if let Some(text) = notice {
            self.prompt = Some(Prompt::Notice {
                tab,
                text,
                since_ms: now,
            });
        }
        self.refresh_prompt();
    }

    /// 承認された自動入力。現在のタブの URL が、質問を出したときと同じオリジンのときだけ入力する。
    fn fill(&self, tab: TabHandle, asked_url: &str, username: &str, now: u64) {
        let current = self.tab_url(tab).unwrap_or_default();
        // 質問の後に別のサイトへ移っていたら、何も入れない。
        let same_site = match (
            self.shell.saved_logins(asked_url),
            self.shell.saved_logins(&current),
        ) {
            (Ok(a), Ok(b)) => {
                !a.is_empty() && a.first().map(|l| &l.origin) == b.first().map(|l| &l.origin)
            }
            _ => false,
        };
        if !same_site {
            return;
        }
        match self.shell.login_secret(&current, username, now) {
            Ok(Some(secret)) => {
                if let Some(script) = fill_script(username, &secret) {
                    self.shell.store().host().run_script(tab, &script);
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("browser: {e}"),
        }
    }
}
