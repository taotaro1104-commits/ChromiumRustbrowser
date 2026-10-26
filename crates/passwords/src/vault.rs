use storage::{Db, LoginEntry, StorageError};
use zeroize::Zeroizing;

use crate::origin::normalize_origin;
use crate::secret::{SecretError, SecretStore};

/// ユーザー名・パスワードの上限(バイト)。巨大な入力でメモリや資格情報ストアを圧迫しないため。
pub const MAX_USERNAME_BYTES: usize = 256;
pub const MAX_PASSWORD_BYTES: usize = 1024;

#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    #[error("this site cannot store passwords")]
    UnsupportedSite,
    #[error("username or password is empty or too long")]
    InvalidCredentials,
    #[error(transparent)]
    Storage(#[from] StorageError),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveOutcome {
    Saved,
    Updated,
    Unchanged,
}

/// ログイン送信を受けて、利用者に何を尋ねるか。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// 尋ねない(対象外のサイト、「保存しない」のサイト、保存済みで同じ内容)。
    None,
    /// 新しいログインの保存を尋ねる。
    Save,
    /// 保存済みのユーザー名で、パスワードが変わった。更新を尋ねる。
    Update,
}

pub struct Vault<S: SecretStore> {
    store: S,
}

fn secret_key(origin: &str, username: &str) -> String {
    // オリジンに `|` は含まれないので、(origin, username) の組が一意に決まる。
    format!("rustbrowser/{origin}|{username}")
}

fn valid(username: &str, password: &str) -> bool {
    !username.is_empty()
        && !password.is_empty()
        && username.len() <= MAX_USERNAME_BYTES
        && password.len() <= MAX_PASSWORD_BYTES
}

impl<S: SecretStore> Vault<S> {
    pub fn new(store: S) -> Self {
        Self { store }
    }

    pub fn store(&self) -> &S {
        &self.store
    }

    /// ログイン送信に対して、保存・更新を尋ねるべきか。パスワードの値は返さず、比較だけする。
    pub fn prompt_for(
        &self,
        db: &Db,
        url: &str,
        username: &str,
        password: &str,
    ) -> Result<PromptKind, VaultError> {
        let Some(origin) = normalize_origin(url) else {
            return Ok(PromptKind::None);
        };
        if !valid(username, password) || db.is_login_exception(&origin)? {
            return Ok(PromptKind::None);
        }
        let known = db
            .logins_for(&origin)?
            .iter()
            .any(|l| l.username == username);
        if !known {
            return Ok(PromptKind::Save);
        }
        match self.store.get(&secret_key(&origin, username))? {
            Some(saved) if *saved == password => Ok(PromptKind::None),
            _ => Ok(PromptKind::Update),
        }
    }

    /// 保存する(利用者が承認した後に呼ぶ)。
    pub fn save(
        &self,
        db: &Db,
        url: &str,
        username: &str,
        password: &str,
        now_ms: u64,
    ) -> Result<SaveOutcome, VaultError> {
        let origin = normalize_origin(url).ok_or(VaultError::UnsupportedSite)?;
        if !valid(username, password) {
            return Err(VaultError::InvalidCredentials);
        }
        let key = secret_key(&origin, username);
        let unchanged = self
            .store
            .get(&key)?
            .is_some_and(|saved| *saved == password);
        if unchanged {
            db.touch_login(&origin, username, now_ms)?;
            return Ok(SaveOutcome::Unchanged);
        }
        // 秘密を先に保存し、成功してから索引を書く(索引だけ残って中身が無い状態を作らない)。
        self.store.set(&key, password)?;
        let inserted = db.upsert_login(&origin, username, now_ms)?;
        Ok(if inserted {
            SaveOutcome::Saved
        } else {
            SaveOutcome::Updated
        })
    }

    /// そのサイトのログイン一覧(ユーザー名のみ。パスワードは含まない)。対象外のサイトは空。
    pub fn logins_for(&self, db: &Db, url: &str) -> Result<Vec<LoginEntry>, VaultError> {
        match normalize_origin(url) {
            Some(origin) => Ok(db.logins_for(&origin)?),
            None => Ok(Vec::new()),
        }
    }

    /// 自動入力用にパスワードを取り出す。**利用者が入力を承認した後だけ**呼ぶこと。
    pub fn secret_for(
        &self,
        db: &Db,
        url: &str,
        username: &str,
        now_ms: u64,
    ) -> Result<Option<Zeroizing<String>>, VaultError> {
        let Some(origin) = normalize_origin(url) else {
            return Ok(None);
        };
        let secret = self.store.get(&secret_key(&origin, username))?;
        if secret.is_some() {
            db.touch_login(&origin, username, now_ms)?;
        }
        Ok(secret)
    }

    /// 管理画面からの削除。索引と秘密の両方を消す。
    pub fn delete(&self, db: &Db, origin: &str, username: &str) -> Result<bool, VaultError> {
        // 秘密の削除に失敗しても索引は消さない(消えたように見えて中身が残るのを避ける)。
        let had_secret = self.store.delete(&secret_key(origin, username))?;
        let had_index = db.delete_login(origin, username)?;
        Ok(had_secret || had_index)
    }

    /// 「このサイトでは保存しない」。
    pub fn never_save(&self, db: &Db, url: &str, now_ms: u64) -> Result<(), VaultError> {
        let origin = normalize_origin(url).ok_or(VaultError::UnsupportedSite)?;
        db.add_login_exception(&origin, now_ms)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret::MemorySecretStore;

    fn setup() -> (Vault<MemorySecretStore>, Db) {
        (
            Vault::new(MemorySecretStore::new()),
            Db::open_in_memory().unwrap(),
        )
    }

    const SITE: &str = "https://a.example/login";

    #[test]
    fn new_login_prompts_save_then_stores_secret_outside_the_db() {
        let (v, db) = setup();
        assert_eq!(
            v.prompt_for(&db, SITE, "alice", "pw1").unwrap(),
            PromptKind::Save
        );
        assert_eq!(
            v.save(&db, SITE, "alice", "pw1", 1).unwrap(),
            SaveOutcome::Saved
        );
        assert_eq!(v.store().len(), 1);
        // DB の索引にパスワードは無い。
        let l = db.logins_for("https://a.example").unwrap();
        assert_eq!(l.len(), 1);
        assert!(!format!("{l:?}").contains("pw1"));
        // 同じ内容ならもう尋ねない。
        assert_eq!(
            v.prompt_for(&db, SITE, "alice", "pw1").unwrap(),
            PromptKind::None
        );
    }

    #[test]
    fn changed_password_prompts_update() {
        let (v, db) = setup();
        v.save(&db, SITE, "alice", "old", 1).unwrap();
        assert_eq!(
            v.prompt_for(&db, SITE, "alice", "new").unwrap(),
            PromptKind::Update
        );
        assert_eq!(
            v.save(&db, SITE, "alice", "new", 2).unwrap(),
            SaveOutcome::Updated
        );
        let got = v.secret_for(&db, SITE, "alice", 3).unwrap().unwrap();
        assert_eq!(&*got, "new");
        assert_eq!(
            v.save(&db, SITE, "alice", "new", 4).unwrap(),
            SaveOutcome::Unchanged
        );
    }

    #[test]
    fn unsupported_sites_never_prompt_or_store() {
        let (v, db) = setup();
        for url in ["http://a.example/", "file:///x", "about:blank"] {
            assert_eq!(
                v.prompt_for(&db, url, "alice", "pw").unwrap(),
                PromptKind::None
            );
            assert!(matches!(
                v.save(&db, url, "alice", "pw", 1),
                Err(VaultError::UnsupportedSite)
            ));
        }
        assert!(v.store().is_empty());
    }

    #[test]
    fn invalid_credentials_are_rejected() {
        let (v, db) = setup();
        let long = "x".repeat(MAX_PASSWORD_BYTES + 1);
        for (u, p) in [("", "pw"), ("alice", ""), ("alice", long.as_str())] {
            assert_eq!(v.prompt_for(&db, SITE, u, p).unwrap(), PromptKind::None);
            assert!(matches!(
                v.save(&db, SITE, u, p, 1),
                Err(VaultError::InvalidCredentials)
            ));
        }
    }

    #[test]
    fn never_save_suppresses_prompts_for_that_origin_only() {
        let (v, db) = setup();
        v.never_save(&db, SITE, 1).unwrap();
        assert_eq!(
            v.prompt_for(&db, "https://a.example/other", "alice", "pw")
                .unwrap(),
            PromptKind::None
        );
        assert_eq!(
            v.prompt_for(&db, "https://b.example/", "alice", "pw")
                .unwrap(),
            PromptKind::Save
        );
    }

    #[test]
    fn secrets_are_not_shared_across_origins() {
        let (v, db) = setup();
        v.save(&db, "https://a.example/", "alice", "pw", 1).unwrap();
        for other in [
            "https://b.example/",
            "https://a.example:8443/",
            "https://www.a.example/",
            "http://a.example/",
        ] {
            assert!(v.logins_for(&db, other).unwrap().is_empty(), "{other}");
            assert!(v.secret_for(&db, other, "alice", 2).unwrap().is_none());
        }
    }

    #[test]
    fn delete_removes_index_and_secret() {
        let (v, db) = setup();
        v.save(&db, SITE, "alice", "pw", 1).unwrap();
        assert!(v.delete(&db, "https://a.example", "alice").unwrap());
        assert!(v.store().is_empty());
        assert!(v.logins_for(&db, SITE).unwrap().is_empty());
        assert!(!v.delete(&db, "https://a.example", "alice").unwrap());
    }

    #[test]
    fn usernames_with_separators_do_not_collide() {
        let (v, db) = setup();
        v.save(&db, SITE, "a|b", "one", 1).unwrap();
        v.save(&db, SITE, "a", "two", 1).unwrap();
        assert_eq!(&*v.secret_for(&db, SITE, "a|b", 2).unwrap().unwrap(), "one");
        assert_eq!(&*v.secret_for(&db, SITE, "a", 2).unwrap().unwrap(), "two");
    }
}
