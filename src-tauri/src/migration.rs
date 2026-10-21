//! Previous product identifiers are isolated here for one-way data migration.
use crate::{
    credentials::{OsSecretStore, SecretStore},
    sqlite_store,
};
use rusqlite::{backup::Backup, Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const LEGACY_IDENTIFIER: &str = "dev.hostdock.desktop";
const LEGACY_DATABASE: &str = "hostdock.db";
pub(crate) const LEGACY_CLIENT_KEY: &str = "hostdock";

pub fn is_previous_local_endpoint(url: &str) -> bool {
    let Some(authority) = url
        .strip_prefix("http://")
        .and_then(|value| value.strip_suffix("/mcp"))
    else {
        return false;
    };
    let Some((host, port)) = authority.rsplit_once(':') else {
        return false;
    };
    matches!(host, "127.0.0.1" | "localhost") && port.parse::<u16>().is_ok_and(|value| value > 0)
}

pub fn previous_data_directory(current: &Path) -> PathBuf {
    current.with_file_name(LEGACY_IDENTIFIER)
}

fn optional_file(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() || metadata.len() > 16 * 1024 * 1024 => {
            Err("旧应用数据格式或大小无效，原文件已保留".into())
        }
        Ok(_) => fs::read(path)
            .map(Some)
            .map_err(|_| "无法读取旧应用数据，原文件已保留".into()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法检查旧应用数据，原文件已保留".into()),
    }
}

/// Uses SQLite's online backup so committed WAL pages are included. Source files
/// are retained; an existing new database always wins, even if it is corrupt.
pub fn prepare_data_directory(current: &Path, previous: Option<&Path>) -> Result<bool, String> {
    if current.join(sqlite_store::DATABASE_FILE).exists() {
        return Ok(false);
    }
    let source = if current.join(LEGACY_DATABASE).exists() {
        current
    } else if current.join("state.json").exists() || current.join("transaction.json").exists() {
        return Ok(false);
    } else if let Some(previous) = previous
        .filter(|path| path.join(LEGACY_DATABASE).exists() || path.join("state.json").exists())
    {
        previous
    } else {
        return Ok(false);
    };
    let in_place = source == current;
    if !in_place
        && current.exists()
        && fs::read_dir(current)
            .map_err(|_| "无法检查应用数据目录")?
            .next()
            .is_some()
    {
        return Err("新的应用数据目录已有文件，旧配置已保留，请先检查数据目录".into());
    }
    let parent = current.parent().ok_or("应用数据目录无效")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建应用数据目录")?;
    let staging = parent.join(format!(".astralink-migration-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&staging).map_err(|_| "无法创建迁移暂存目录")?;
    let result = (|| -> Result<(), String> {
        let journal = optional_file(&source.join("transaction.json"))?;
        let old_database = source.join(LEGACY_DATABASE);
        if old_database.exists() {
            let from = Connection::open_with_flags(&old_database, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(|_| "无法读取旧数据库，原文件已保留")?;
            from.busy_timeout(Duration::from_secs(2))
                .map_err(|_| "无法读取旧数据库")?;
            let mut to = Connection::open(staging.join(sqlite_store::DATABASE_FILE))
                .map_err(|_| "无法创建迁移数据库")?;
            let backup =
                Backup::new(&from, &mut to).map_err(|_| "无法迁移旧数据库，原文件已保留")?;
            // A single bounded wait avoids an indefinite retry while another app writes.
            match backup
                .step(-1)
                .map_err(|_| "数据库迁移失败，原文件已保留")?
            {
                rusqlite::backup::StepResult::Done => (),
                _ => return Err("旧数据库正在使用中，请退出旧版应用后重试".into()),
            }
        }
        for name in ["state.json", "transaction.json"] {
            if let Some(bytes) = if name == "transaction.json" {
                journal.clone()
            } else {
                optional_file(&source.join(name))?
            } {
                fs::write(staging.join(name), bytes)
                    .map_err(|_| "无法迁移应用配置，原文件已保留")?;
            }
        }
        if !in_place {
            for name in ["runtime.jsonl", "runtime.1.jsonl", "runtime.2.jsonl"] {
                if let Some(bytes) = optional_file(&source.join("logs").join(name))? {
                    fs::create_dir_all(staging.join("logs")).map_err(|_| "无法迁移运行日志")?;
                    fs::write(staging.join("logs").join(name), bytes)
                        .map_err(|_| "无法迁移运行日志")?;
                }
            }
        }
        // Validate schema and secret-free metadata before exposing the new directory.
        drop(sqlite_store::open(&staging)?);
        if optional_file(&source.join("transaction.json"))? != journal {
            return Err("旧应用正在更新配置，请退出旧版应用后重试".into());
        }
        if in_place {
            fs::rename(
                staging.join(sqlite_store::DATABASE_FILE),
                current.join(sqlite_store::DATABASE_FILE),
            )
            .map_err(|_| "无法完成数据库迁移，原文件已保留")?;
        } else {
            if current.exists() {
                fs::remove_dir(current).map_err(|_| "新的数据目录已变化，旧配置已保留")?;
            }
            fs::rename(&staging, current).map_err(|_| "无法完成应用数据迁移，原文件已保留")?;
        }
        Ok(())
    })();
    // staging is an exact freshly generated sibling, never the user's source.
    if staging.parent() == Some(parent)
        && staging
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with(".astralink-migration-"))
    {
        let _ = fs::remove_dir_all(&staging);
    }
    result.map(|_| true)
}

pub struct MigratingSecrets<C, P> {
    current: C,
    previous: P,
}
impl<C: SecretStore, P: SecretStore> MigratingSecrets<C, P> {
    pub fn new(current: C, previous: P) -> Self {
        Self { current, previous }
    }
}
impl<C: SecretStore, P: SecretStore> SecretStore for MigratingSecrets<C, P> {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if let Some(value) = self.current.get(key)? {
            return Ok(Some(value));
        }
        let value = self.previous.get(key)?;
        if let Some(value) = &value {
            self.current.set(key, value)?;
        }
        Ok(value)
    }
    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        self.current.set(key, value)
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        // Remove the old copy first so a retry can never resurrect a deleted seed.
        self.previous.delete(key)?;
        self.current.delete(key)
    }
}
struct PreviousOsSecrets;
impl PreviousOsSecrets {
    fn entry(key: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(LEGACY_IDENTIFIER, key).map_err(|_| "无法访问旧系统凭据库".into())
    }
}
impl SecretStore for PreviousOsSecrets {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(value)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("无法读取旧系统凭据".into()),
        }
    }
    fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
        Err("迁移不写入旧凭据库".into())
    }
    fn delete(&self, key: &str) -> Result<(), String> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("无法删除旧系统凭据".into()),
        }
    }
}
pub fn secret_store() -> impl SecretStore {
    MigratingSecrets::new(OsSecretStore, PreviousOsSecrets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{credentials::SecretStore, sqlite_store, storage::StoredState};
    use std::{
        collections::HashMap,
        fs,
        sync::{Arc, Mutex},
    };

    #[derive(Clone, Default)]
    struct Vault(Arc<Mutex<HashMap<String, String>>>);
    impl SecretStore for Vault {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(key).cloned())
        }
        fn set(&self, key: &str, value: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(key.into(), value.into());
            Ok(())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(key);
            Ok(())
        }
    }
    #[test]
    fn credentials_migrate_lazily_and_deleted_values_never_resurrect() {
        let current = Vault::default();
        let previous = Vault::default();
        previous
            .set("profile/fixture/totpSecret", "fixture-only")
            .unwrap();
        let store = MigratingSecrets::new(current.clone(), previous.clone());
        assert_eq!(
            store.get("profile/fixture/totpSecret").unwrap().as_deref(),
            Some("fixture-only")
        );
        assert_eq!(
            current
                .get("profile/fixture/totpSecret")
                .unwrap()
                .as_deref(),
            Some("fixture-only")
        );
        store
            .set("profile/fixture/totpSecret", "new-fixture")
            .unwrap();
        assert_eq!(
            store.get("profile/fixture/totpSecret").unwrap().as_deref(),
            Some("new-fixture")
        );
        store.delete("profile/fixture/totpSecret").unwrap();
        assert_eq!(store.get("profile/fixture/totpSecret").unwrap(), None);
        assert_eq!(previous.get("profile/fixture/totpSecret").unwrap(), None);
    }
    struct ReadOnlyVault(Vault);
    impl SecretStore for ReadOnlyVault {
        fn get(&self, key: &str) -> Result<Option<String>, String> {
            self.0.get(key)
        }
        fn set(&self, _key: &str, _value: &str) -> Result<(), String> {
            Err("fixture store unavailable".into())
        }
        fn delete(&self, key: &str) -> Result<(), String> {
            self.0.delete(key)
        }
    }
    #[test]
    fn credential_migration_failure_keeps_previous_secret_and_reports_error() {
        let current = Vault::default();
        let previous = Vault::default();
        previous.set("fixture-secret", "fixture-value").unwrap();
        let store = MigratingSecrets::new(ReadOnlyVault(current.clone()), previous.clone());
        assert!(store.get("fixture-secret").is_err());
        assert_eq!(current.get("fixture-secret").unwrap(), None);
        assert_eq!(
            previous.get("fixture-secret").unwrap().as_deref(),
            Some("fixture-value")
        );
    }
    #[test]
    fn existing_invalid_new_database_is_never_replaced_by_old_snapshot() {
        let root = tempfile::tempdir().unwrap();
        let previous = root.path().join("previous");
        let current = root.path().join("current");
        fs::create_dir(&previous).unwrap();
        fs::create_dir(&current).unwrap();
        fs::write(previous.join(LEGACY_DATABASE), "old-fixture").unwrap();
        fs::write(
            current.join(sqlite_store::DATABASE_FILE),
            "invalid-new-fixture",
        )
        .unwrap();
        assert!(!prepare_data_directory(&current, Some(&previous)).unwrap());
        assert_eq!(
            fs::read_to_string(current.join(sqlite_store::DATABASE_FILE)).unwrap(),
            "invalid-new-fixture"
        );
    }
    #[test]
    fn sqlite_backup_includes_wal_and_never_overwrites_new_data() {
        let root = tempfile::tempdir().unwrap();
        let previous = root.path().join(LEGACY_IDENTIFIER);
        let current = root.path().join("dev.astralink.desktop");
        fs::create_dir(&previous).unwrap();
        drop(sqlite_store::open(&previous).unwrap());
        fs::rename(
            previous.join(sqlite_store::DATABASE_FILE),
            previous.join(LEGACY_DATABASE),
        )
        .unwrap();
        let db = rusqlite::Connection::open(previous.join(LEGACY_DATABASE)).unwrap();
        db.pragma_update(None, "journal_mode", "WAL").unwrap();
        db.execute_batch("CREATE TABLE migration_probe(value TEXT); INSERT INTO migration_probe VALUES ('from-wal');").unwrap();
        fs::write(
            previous.join("state.json"),
            serde_json::to_vec(&StoredState::default()).unwrap(),
        )
        .unwrap();
        assert!(prepare_data_directory(&current, Some(&previous)).unwrap());
        assert!(previous.join(LEGACY_DATABASE).exists());
        assert!(current.join("state.json").exists());
        let migrated =
            rusqlite::Connection::open(current.join(sqlite_store::DATABASE_FILE)).unwrap();
        assert_eq!(
            migrated
                .query_row("SELECT value FROM migration_probe", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "from-wal"
        );
        migrated
            .execute("UPDATE migration_probe SET value='new-authoritative'", [])
            .unwrap();
        assert!(!prepare_data_directory(&current, Some(&previous)).unwrap());
        assert_eq!(
            migrated
                .query_row("SELECT value FROM migration_probe", [], |row| row
                    .get::<_, String>(0))
                .unwrap(),
            "new-authoritative"
        );
    }
    #[test]
    fn migration_failure_preserves_original_and_does_not_install_broken_database() {
        let root = tempfile::tempdir().unwrap();
        let previous = root.path().join("previous");
        let current = root.path().join("current");
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join(LEGACY_DATABASE), "broken database").unwrap();
        assert!(prepare_data_directory(&current, Some(&previous)).is_err());
        assert_eq!(
            fs::read_to_string(previous.join(LEGACY_DATABASE)).unwrap(),
            "broken database"
        );
        assert!(!current.exists());
    }
    #[test]
    fn explicit_old_data_directory_migrates_in_place_and_retains_journal() {
        let root = tempfile::tempdir().unwrap();
        drop(sqlite_store::open(root.path()).unwrap());
        fs::rename(
            root.path().join(sqlite_store::DATABASE_FILE),
            root.path().join(LEGACY_DATABASE),
        )
        .unwrap();
        fs::write(root.path().join("transaction.json"), "fixture-journal").unwrap();
        assert!(prepare_data_directory(root.path(), None).unwrap());
        assert_eq!(
            fs::read_to_string(root.path().join("transaction.json")).unwrap(),
            "fixture-journal"
        );
        assert!(root.path().join(LEGACY_DATABASE).exists());
    }
    #[test]
    fn client_registration_replaces_owned_old_entry_and_preserves_unrelated_settings() {
        let root = tempfile::tempdir().unwrap();
        let json_path = root.path().join("mcp.json");
        let old = serde_json::json!({"url":"http://127.0.0.1:18766/mcp", "headers":{"Authorization":"Bearer old-fixture"}});
        let original = serde_json::json!({"settings":true,"mcpServers":{LEGACY_CLIENT_KEY:old,"other":{"url":"https://example.invalid/mcp"}}});
        fs::write(&json_path, serde_json::to_vec(&original).unwrap()).unwrap();
        let result =
            crate::client_setup::register("cursor", &json_path, 18766, "new-fixture").unwrap();
        assert!(result.backup_path.is_some());
        let next: serde_json::Value =
            serde_json::from_slice(&fs::read(&json_path).unwrap()).unwrap();
        assert!(next["mcpServers"].get(LEGACY_CLIENT_KEY).is_none());
        assert_eq!(
            next["mcpServers"]["astralink"]["headers"]["Authorization"],
            "Bearer new-fixture"
        );
        assert_eq!(next["mcpServers"]["other"], original["mcpServers"]["other"]);
        assert_eq!(next["settings"], true);
        let toml_path = root.path().join("config.toml");
        fs::write(&toml_path,format!("[mcp_servers.{LEGACY_CLIENT_KEY}]\nurl='http://127.0.0.1:18766/mcp'\nenabled=false\n[mcp_servers.other]\ncommand='other'\n")).unwrap();
        crate::config_sync::register(&toml_path, 18766, "new-fixture").unwrap();
        let next = fs::read_to_string(&toml_path)
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        assert!(next["mcp_servers"].get(LEGACY_CLIENT_KEY).is_none());
        assert_eq!(
            next["mcp_servers"]["astralink"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(
            next["mcp_servers"]["other"]["command"].as_str(),
            Some("other")
        );
    }
    #[test]
    fn registration_preserves_old_key_when_it_points_to_an_unrelated_service() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("mcp.json");
        let original = serde_json::json!({"mcpServers":{LEGACY_CLIENT_KEY:{"url":"https://example.invalid/mcp"}}});
        fs::write(&path, serde_json::to_vec(&original).unwrap()).unwrap();
        crate::client_setup::register("cursor", &path, 18766, "fixture").unwrap();
        let next: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        assert_eq!(
            next["mcpServers"][LEGACY_CLIENT_KEY],
            original["mcpServers"][LEGACY_CLIENT_KEY]
        );
    }
    #[test]
    fn unrelated_destination_files_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        let previous = root.path().join("previous");
        let current = root.path().join("current");
        fs::create_dir(&previous).unwrap();
        fs::write(previous.join("state.json"), "{}").unwrap();
        fs::create_dir(&current).unwrap();
        fs::write(current.join("keep.txt"), "keep").unwrap();
        assert!(prepare_data_directory(&current, Some(&previous)).is_err());
        assert_eq!(
            fs::read_to_string(current.join("keep.txt")).unwrap(),
            "keep"
        );
    }
}
