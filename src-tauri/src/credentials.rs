/// Secret storage is separate from JSON metadata. Errors never include secret values.
pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>, String>;
    fn set(&self, key: &str, value: &str) -> Result<(), String>;
    fn delete(&self, key: &str) -> Result<(), String>;
}

pub struct OsSecretStore;
impl OsSecretStore {
    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new("dev.astralink.desktop", key).map_err(|_| "无法访问系统凭据库".into())
    }
}
impl SecretStore for OsSecretStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取系统凭据".into()),
        }
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        Self::entry(key)?
            .set_password(value)
            .map_err(|_| "无法保存系统凭据".into())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法删除系统凭据".into()),
        }
    }
}

pub fn credential_key(id: &str, field: &str) -> String {
    format!("profile/{id}/{field}")
}
