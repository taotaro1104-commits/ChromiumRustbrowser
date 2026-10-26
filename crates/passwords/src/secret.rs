use std::collections::HashMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    /// OS の資格情報ストアが失敗した。詳細(OS のメッセージ)のみで、秘密の値は含めない。
    #[error("credential store error: {0}")]
    Store(String),
}

/// 秘密(パスワード)の保存先。実機では OS の資格情報ストア(Windows 資格情報マネージャー)を使う。
/// `key` はオリジンとユーザー名から作る識別子で、秘密ではない。
pub trait SecretStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError>;
    fn get(&self, key: &str) -> Result<Option<Zeroizing<String>>, SecretError>;
    /// 削除できたら `true`、元から無ければ `false`。
    fn delete(&self, key: &str) -> Result<bool, SecretError>;
}

/// テストと、OS ストアが使えないときの一時的な代替(プロセスを終えると消える)。
#[derive(Default)]
pub struct MemorySecretStore {
    map: Mutex<HashMap<String, Zeroizing<String>>>,
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.map.lock().map_or(0, |m| m.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl SecretStore for MemorySecretStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        let mut m = self
            .map
            .lock()
            .map_err(|_| SecretError::Store("poisoned".into()))?;
        m.insert(key.to_owned(), Zeroizing::new(secret.to_owned()));
        Ok(())
    }

    fn get(&self, key: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        let m = self
            .map
            .lock()
            .map_err(|_| SecretError::Store("poisoned".into()))?;
        Ok(m.get(key).map(|s| Zeroizing::new((**s).clone())))
    }

    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        let mut m = self
            .map
            .lock()
            .map_err(|_| SecretError::Store("poisoned".into()))?;
        Ok(m.remove(key).is_some())
    }
}

impl<T: SecretStore + ?Sized> SecretStore for Box<T> {
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        (**self).set(key, secret)
    }

    fn get(&self, key: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        (**self).get(key)
    }

    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        (**self).delete(key)
    }
}
