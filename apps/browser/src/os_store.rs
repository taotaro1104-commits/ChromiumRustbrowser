//! パスワードの保存先: Windows の資格情報マネージャー(`keyring` 経由)。
//! 資格情報の名前にはサイトやユーザー名をそのまま使わず、ハッシュ化した固定長の名前にする
//! (名前の長さ制限を避け、資格情報の一覧からサイト名が読み取られないようにするため)。
//! どのサイトのどのユーザー名かの索引は、ブラウザの DB(パスワードを含まない)が持つ。

use keyring::Entry;
use sha2::{Digest, Sha256};
use shell_core::{SecretError, SecretStore, Zeroizing};

const SERVICE: &str = "RustBrowser";

pub struct OsSecretStore;

/// `passwords` が作る識別子から、資格情報の名前を作る。
pub fn credential_name(key: &str) -> String {
    let digest = Sha256::digest(key.as_bytes());
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("pw-{hex}")
}

fn entry(key: &str) -> Result<Entry, SecretError> {
    Entry::new(SERVICE, &credential_name(key)).map_err(|e| SecretError::Store(e.to_string()))
}

impl SecretStore for OsSecretStore {
    fn set(&self, key: &str, secret: &str) -> Result<(), SecretError> {
        entry(key)?
            .set_password(secret)
            .map_err(|e| SecretError::Store(e.to_string()))
    }

    fn get(&self, key: &str) -> Result<Option<Zeroizing<String>>, SecretError> {
        match entry(key)?.get_password() {
            Ok(p) => Ok(Some(Zeroizing::new(p))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }

    fn delete(&self, key: &str) -> Result<bool, SecretError> {
        match entry(key)?.delete_credential() {
            Ok(()) => Ok(true),
            Err(keyring::Error::NoEntry) => Ok(false),
            Err(e) => Err(SecretError::Store(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credential_names_are_fixed_length_and_hide_the_site() {
        let a = credential_name("rustbrowser/https://bank.example|alice");
        let b = credential_name("rustbrowser/https://bank.example|bob");
        assert_eq!(a.len(), "pw-".len() + 64);
        assert_ne!(a, b);
        assert!(!a.contains("bank") && !a.contains("alice"));
        assert_eq!(a, credential_name("rustbrowser/https://bank.example|alice"));
    }

    /// 実際の資格情報マネージャーへの書き込みを伴うため、通常のテストでは走らせない
    /// (`cargo test -- --ignored os_round_trip` で手動実行)。
    #[test]
    #[ignore]
    fn os_round_trip() {
        let s = OsSecretStore;
        let key = format!("rustbrowser/test|{}", std::process::id());
        assert_eq!(s.get(&key).unwrap(), None);
        s.set(&key, "pw-é日本").unwrap();
        assert_eq!(&*s.get(&key).unwrap().unwrap(), "pw-é日本");
        assert!(s.delete(&key).unwrap());
        assert!(!s.delete(&key).unwrap());
    }
}
