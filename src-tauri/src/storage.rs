use crate::{
    credentials::{credential_key, SecretStore},
    models::*,
    sqlite_store,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const MAX_STATE_BYTES: u64 = 16 * 1024 * 1024;
#[derive(Deserialize, Serialize)]
struct TransactionEntry {
    target: String,
    backup: bool,
    index: usize,
}
#[derive(Deserialize, Serialize)]
struct TransactionMarker {
    id: String,
    candidate: Vec<u8>,
    entries: Vec<TransactionEntry>,
    #[serde(default)]
    rolled_back: bool,
}
fn backup_key(id: &str, index: usize) -> String {
    format!("transaction/{id}/{index}")
}
fn journal_path(directory: &Path) -> PathBuf {
    directory.join("transaction.json")
}
fn discard_journal(
    directory: &Path,
    secrets: &dyn SecretStore,
    marker: &TransactionMarker,
) -> Result<(), String> {
    // After a committed state is durable, delete old backups before the marker;
    // if interrupted, recovery will observe that state and retry cleanup.
    for entry in &marker.entries {
        if entry.backup {
            secrets.delete(&backup_key(&marker.id, entry.index))?;
        }
    }
    fs::remove_file(journal_path(directory)).map_err(|_| "无法清理凭据恢复记录".to_string())
}
fn mark_rolled_back_and_discard(
    directory: &Path,
    secrets: &dyn SecretStore,
    marker: &TransactionMarker,
) -> Result<(), String> {
    let next = TransactionMarker {
        id: marker.id.clone(),
        candidate: marker.candidate.clone(),
        entries: marker
            .entries
            .iter()
            .map(|entry| TransactionEntry {
                target: entry.target.clone(),
                backup: entry.backup,
                index: entry.index,
            })
            .collect(),
        rolled_back: true,
    };
    atomic_write(
        &journal_path(directory),
        &serde_json::to_vec(&next).map_err(|_| "无法标记凭据恢复状态")?,
    )?;
    discard_journal(directory, secrets, &next)
}
fn recover_journal(directory: &Path, secrets: &dyn SecretStore) -> Result<(), String> {
    let bytes = match fs::read(journal_path(directory)) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("无法读取凭据恢复记录".into()),
    };
    if bytes.len() > 4 * MAX_STATE_BYTES as usize {
        return Err("凭据恢复记录超过大小限制".into());
    }
    let marker: TransactionMarker =
        serde_json::from_slice(&bytes).map_err(|_| "凭据恢复记录损坏".to_string())?;
    if uuid::Uuid::parse_str(&marker.id).is_err() || marker.entries.len() > 1024 {
        return Err("凭据恢复记录标识无效".into());
    }
    for entry in &marker.entries {
        let Some((prefix, field)) = entry.target.rsplit_once('/') else {
            return Err("凭据恢复目标无效".into());
        };
        let Some(id) = prefix.strip_prefix("profile/") else {
            return Err("凭据恢复目标无效".into());
        };
        if uuid::Uuid::parse_str(id).is_err()
            || !["password", "passphrase", "totpSecret"].contains(&field)
        {
            return Err("凭据恢复目标无效".into());
        }
    }
    let committed =
        sqlite_store::committed_bytes(directory).as_deref() == Some(marker.candidate.as_slice());
    if !committed && !marker.rolled_back {
        for entry in &marker.entries {
            if entry.backup {
                let value = secrets
                    .get(&backup_key(&marker.id, entry.index))?
                    .ok_or("恢复用凭据不存在，原数据已保留")?;
                secrets.set(&entry.target, &value)?;
            } else {
                secrets.delete(&entry.target)?;
            }
        }
    }
    if !committed && !marker.rolled_back {
        mark_rolled_back_and_discard(directory, secrets, &marker)
    } else {
        discard_journal(directory, secrets, &marker)
    }
}

#[derive(Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub(crate) struct StoredState {
    pub(crate) profiles: Vec<ConnectionProfile>,
    pub(crate) settings: AppSettings,
}

pub struct AppStore {
    pub directory: PathBuf,
    secrets: Box<dyn SecretStore>,
    state: StoredState,
    database: rusqlite::Connection,
}
impl AppStore {
    pub fn open(directory: PathBuf, secrets: Box<dyn SecretStore>) -> Result<Self, String> {
        fs::create_dir_all(&directory).map_err(|_| "无法创建应用数据目录")?;
        recover_journal(&directory, secrets.as_ref())?;
        let (database, state) = sqlite_store::open(&directory)?;
        Ok(Self {
            directory,
            secrets,
            state,
            database,
        })
    }
    pub fn profiles(&self) -> Vec<ConnectionProfile> {
        self.state.profiles.clone()
    }
    pub fn settings(&self) -> AppSettings {
        self.state.settings.clone()
    }
    pub fn storage_info(&self) -> StorageInfo {
        let backup = self.directory.join("state.json");
        StorageInfo {
            backend: "sqlite".into(),
            database_path: self
                .directory
                .join(sqlite_store::DATABASE_FILE)
                .to_string_lossy()
                .into(),
            legacy_backup_path: backup.is_file().then(|| backup.to_string_lossy().into()),
            server_count: self.state.profiles.len(),
        }
    }
    pub fn update_settings(&mut self, settings: AppSettings) -> Result<AppSettings, String> {
        recover_journal(&self.directory, self.secrets.as_ref())?;
        if settings.mcp_port == 0 {
            return Err("MCP 端口应为 1–65535".into());
        }
        let candidate = StoredState {
            profiles: self.profiles(),
            settings: settings.clone(),
        };
        self.write_state(&candidate)?;
        self.state = candidate;
        Ok(settings)
    }
    fn write_state(&mut self, state: &StoredState) -> Result<(), String> {
        sqlite_store::save(&mut self.database, state)
    }
    pub fn apply(
        &mut self,
        incoming: Vec<ConnectionProfile>,
        strategy: ImportStrategy,
    ) -> Result<Vec<ConnectionProfile>, String> {
        self.apply_with_settings(incoming, strategy, None)
    }
    pub fn apply_with_settings(
        &mut self,
        mut incoming: Vec<ConnectionProfile>,
        strategy: ImportStrategy,
        settings: Option<AppSettings>,
    ) -> Result<Vec<ConnectionProfile>, String> {
        recover_journal(&self.directory, self.secrets.as_ref())?;
        if settings.as_ref().is_some_and(|value| value.mcp_port == 0) {
            return Err("MCP 端口应为 1–65535".into());
        }
        let mut incoming_ids = HashSet::new();
        for p in &mut incoming {
            if p.id.is_empty() {
                p.id = uuid::Uuid::new_v4().to_string()
            }
            p.id = uuid::Uuid::parse_str(&p.id)
                .map_err(|_| "连接 ID 无效")?
                .to_string();
            p.name = p.name.trim().into();
            p.host = p.host.trim().into();
            p.username = p.username.trim().into();
            if !incoming_ids.insert(p.id.clone()) {
                return Err("导入中存在重复连接 ID".into());
            }
        }
        let mut profiles = match strategy {
            ImportStrategy::Merge => self.profiles(),
            ImportStrategy::Replace => Vec::new(),
        };
        for p in incoming {
            match profiles.iter().position(|x| x.id == p.id) {
                Some(index) => profiles[index] = p,
                None => profiles.push(p),
            }
        }
        validate_profiles(&profiles, true)?;
        // Validate the entire proposed set before touching credentials or metadata.
        let mut changes: Vec<(String, Option<String>, Option<String>)> = Vec::new();
        for p in &mut profiles {
            for field in ["password", "passphrase", "totpSecret"] {
                let input = if field == "password" {
                    p.password.take()
                } else if field == "passphrase" {
                    p.passphrase.take()
                } else {
                    p.totp_secret.take()
                };
                let key = credential_key(&p.id, field);
                let old = self.secrets.get(&key)?;
                let value = match input {
                    Some(s) if s.is_empty() => None,
                    Some(s) => Some(s),
                    None => old.clone(),
                };
                if field == "password" {
                    p.has_password = value.is_some()
                } else if field == "passphrase" {
                    p.has_passphrase = value.is_some()
                } else {
                    p.has_totp_secret = value.is_some()
                }
                if old != value {
                    changes.push((key, old, value))
                }
            }
        }
        let ids: HashSet<_> = profiles.iter().map(|p| p.id.as_str()).collect();
        for old in &self.state.profiles {
            if !ids.contains(old.id.as_str()) {
                for field in ["password", "passphrase", "totpSecret"] {
                    let key = credential_key(&old.id, field);
                    if let Some(value) = self.secrets.get(&key)? {
                        changes.push((key, Some(value), None))
                    }
                }
            }
        }
        let candidate = StoredState {
            profiles,
            settings: settings.unwrap_or_else(|| self.settings()),
        };
        let marker = if changes.is_empty() {
            None
        } else {
            let id = uuid::Uuid::new_v4().to_string();
            let candidate_bytes =
                serde_json::to_vec_pretty(&candidate).map_err(|_| "无法序列化应用数据")?;
            let entries = changes
                .iter()
                .enumerate()
                .map(|(index, (target, old, _))| TransactionEntry {
                    target: target.clone(),
                    backup: old.is_some(),
                    index,
                })
                .collect();
            let marker = TransactionMarker {
                id,
                candidate: candidate_bytes,
                entries,
                rolled_back: false,
            };
            let mut staged: Vec<String> = Vec::new();
            for (index, (_, old, _)) in changes.iter().enumerate() {
                if let Some(old) = old {
                    let key = backup_key(&marker.id, index);
                    if self.secrets.set(&key, old).is_err() {
                        for staged_key in staged {
                            let _ = self.secrets.delete(&staged_key);
                        }
                        return Err("无法备份现有凭据，未修改连接".into());
                    }
                    staged.push(key);
                }
            }
            if let Err(error) = atomic_write(
                &journal_path(&self.directory),
                &serde_json::to_vec(&marker).map_err(|_| "无法序列化恢复记录")?,
            ) {
                for staged_key in staged {
                    let _ = self.secrets.delete(&staged_key);
                }
                return Err(error);
            }
            Some(marker)
        };
        let mut applied = 0;
        for (key, _, new) in &changes {
            if let Err(error) = write_secret(self.secrets.as_ref(), key, new.as_deref()) {
                let rollback = self.rollback(&changes[..=applied]);
                if rollback.is_ok() {
                    if let Some(marker) = &marker {
                        let _ = mark_rolled_back_and_discard(
                            &self.directory,
                            self.secrets.as_ref(),
                            marker,
                        );
                    }
                }
                return Err(with_rollback(error, rollback));
            }
            applied += 1;
        }
        if let Err(error) = self.write_state(&candidate) {
            let rollback = self.rollback(&changes);
            if rollback.is_ok() {
                if let Some(marker) = &marker {
                    let _ = mark_rolled_back_and_discard(
                        &self.directory,
                        self.secrets.as_ref(),
                        marker,
                    );
                }
            }
            return Err(with_rollback(error, rollback));
        }
        self.state = candidate;
        if let Some(marker) = &marker {
            let _ = discard_journal(&self.directory, self.secrets.as_ref(), marker);
        }
        Ok(self.profiles())
    }
    fn rollback(&self, changes: &[(String, Option<String>, Option<String>)]) -> Result<(), String> {
        let mut failed = false;
        for (key, old, _) in changes.iter().rev() {
            if write_secret(self.secrets.as_ref(), key, old.as_deref()).is_err() {
                failed = true
            }
        }
        if failed {
            Err("凭据回滚失败，请检查系统凭据库".into())
        } else {
            Ok(())
        }
    }
    pub fn delete(&mut self, id: &str) -> Result<Vec<ConnectionProfile>, String> {
        self.delete_many(&[id.to_string()])
    }
    pub fn delete_many(&mut self, ids: &[String]) -> Result<Vec<ConnectionProfile>, String> {
        let ids: HashSet<&str> = ids.iter().map(String::as_str).collect();
        if ids.is_empty() {
            return Err("请选择要删除的服务器".into());
        }
        if ids
            .iter()
            .any(|id| !self.state.profiles.iter().any(|p| p.id == *id))
        {
            return Err("连接不存在".into());
        }
        if self.state.profiles.iter().any(|p| {
            !ids.contains(p.id.as_str())
                && p.jump_host_id.as_deref().is_some_and(|id| ids.contains(id))
        }) {
            return Err("连接仍被用作跳板，请先修改关联连接".into());
        }
        self.apply(
            self.profiles()
                .into_iter()
                .filter(|p| !ids.contains(p.id.as_str()))
                .collect(),
            ImportStrategy::Replace,
        )
    }
    pub fn resolve(&self, id: &str) -> Result<ConnectionProfile, String> {
        let mut p = self
            .state
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("连接不存在")?;
        p.password = self.secrets.get(&credential_key(id, "password"))?;
        p.passphrase = self.secrets.get(&credential_key(id, "passphrase"))?;
        p.totp_secret = self.secrets.get(&credential_key(id, "totpSecret"))?;
        p.has_password = p.password.is_some();
        p.has_passphrase = p.passphrase.is_some();
        p.has_totp_secret = p.totp_secret.is_some();
        if p.auth_type == AuthType::Agent && p.agent.is_none() {
            p.agent = std::env::var("SSH_AUTH_SOCK").ok()
        }
        Ok(p)
    }
    pub fn read_secret(&self, id: &str, field: &str) -> Result<Option<String>, String> {
        if uuid::Uuid::parse_str(id).is_err()
            || !["password", "passphrase", "totpSecret"].contains(&field)
        {
            return Err("凭据请求无效".into());
        }
        if !self.state.profiles.iter().any(|profile| profile.id == id) {
            return Err("连接不存在".into());
        }
        self.secrets.get(&credential_key(id, field))
    }
    pub fn runtime_profiles(&self) -> Result<Vec<ConnectionProfile>, String> {
        self.state
            .profiles
            .iter()
            .filter(|p| p.enabled)
            .map(|p| self.resolve(&p.id))
            .collect()
    }
    pub fn mcp_token(&self) -> Result<String, String> {
        if let Some(token) = self.secrets.get("mcp-token")? {
            if token.is_empty() {
                return Err("MCP 令牌无效".into());
            }
            return Ok(token);
        }
        use base64::Engine;
        let token =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        self.secrets.set("mcp-token", &token)?;
        Ok(token)
    }
}
fn write_secret(store: &dyn SecretStore, key: &str, value: Option<&str>) -> Result<(), String> {
    match value {
        Some(v) => store.set(key, v),
        None => store.delete(key),
    }
}
fn with_rollback(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => error,
        Err(r) => format!("{error}；{r}"),
    }
}

pub fn validate_profiles(profiles: &[ConnectionProfile], check_files: bool) -> Result<(), String> {
    if profiles.len() > 512 {
        return Err("最多支持 512 个连接".into());
    }
    let mut ids = HashSet::new();
    let mut names = HashSet::new();
    for p in profiles {
        let id = uuid::Uuid::parse_str(&p.id)
            .map_err(|_| "连接 ID 无效")?
            .to_string();
        if !ids.insert(id) || !names.insert(p.name.clone()) {
            return Err("连接 ID 或名称重复".into());
        }
        for (value, max, label) in [
            (&p.name, 128, "名称"),
            (&p.host, 253, "主机"),
            (&p.username, 256, "用户名"),
        ] {
            if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
                return Err(format!("{label}无效"));
            }
        }
        if p.port == 0
            || p.host
                .chars()
                .any(|c| c.is_whitespace() || "/@?#".contains(c))
        {
            return Err("主机或端口无效".into());
        }
        if let Some(seed) = p.totp_secret.as_deref().filter(|seed| !seed.is_empty()) {
            let seed = seed.trim().to_ascii_uppercase();
            let padding = seed.trim_end_matches('=').len();
            if seed.len() > 128
                || seed.len() < 16
                || seed[..padding]
                    .chars()
                    .any(|c| !matches!(c, 'A'..='Z' | '2'..='7'))
                || !matches!(padding % 8, 0 | 2 | 4 | 5 | 7)
                || seed[padding..].chars().any(|c| c != '=')
                || seed[..padding].chars().last().is_some_and(|last| {
                    let remainder = (padding % 8) * 5 % 8;
                    let value = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".find(last).unwrap_or(0);
                    remainder > 0 && value % (1 << remainder) != 0
                })
            {
                return Err("TOTP 密钥格式无效，请输入 Base32 密钥".into());
            }
        }
        if p.group.len() > 256 || p.source.len() > 128 || p.notes.len() > 4096 {
            return Err("连接描述过长".into());
        }
        for s in [&p.password, &p.passphrase].into_iter().flatten() {
            if s.len() > 16384 || s.contains('\0') {
                return Err("凭据格式无效".into());
            }
        }
        for value in [
            &p.private_key_path,
            &p.agent,
            &p.jump_host_id,
            &p.target_host,
            &p.target_username,
        ]
        .into_iter()
        .flatten()
        {
            if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
                return Err("连接可选字段无效".into());
            }
        }
        for (roots, local) in [
            (&p.allowed_local_paths, true),
            (&p.allowed_remote_paths, false),
        ] {
            if let Some(roots) = roots {
                if roots.len() > 32 {
                    return Err("传输目录最多 32 个".into());
                }
                for root in roots {
                    if root.is_empty()
                        || root.len() > 4096
                        || root.chars().any(char::is_control)
                        || root.split(['/', '\\']).any(|part| part == "..")
                    {
                        return Err("传输目录无效".into());
                    }
                    if local {
                        if !Path::new(root).is_absolute()
                            || (check_files && !Path::new(root).is_dir())
                        {
                            return Err("本地传输目录必须是存在的绝对目录".into());
                        }
                    } else {
                        let bytes = root.as_bytes();
                        let absolute = root.starts_with('/')
                            || (bytes.len() >= 3
                                && bytes[0].is_ascii_alphabetic()
                                && bytes[1] == b':'
                                && [b'/', b'\\'].contains(&bytes[2]));
                        if !absolute {
                            return Err("远端传输目录必须为绝对路径".into());
                        }
                    }
                }
            }
        }
        if p.target_host.is_some() != p.target_username.is_some() {
            return Err("目标主机和目标用户名必须同时填写".into());
        }
        if let (Some(target_host), Some(target_username)) = (&p.target_host, &p.target_username) {
            for value in [&p.username, target_host, target_username] {
                if value.chars().any(|c| c.is_whitespace() || "@#".contains(c)) {
                    return Err("目标登录字段无效".into());
                }
            }
        }
        if check_files && p.enabled && p.auth_type == AuthType::PrivateKey {
            let key = p.private_key_path.as_ref().ok_or("缺少私钥文件")?;
            let path = Path::new(key);
            if !path.is_absolute() || !path.is_file() {
                return Err("私钥文件不存在或路径不是绝对路径".into());
            }
            if fs::metadata(path).map_err(|_| "无法读取私钥文件")?.len() > 1024 * 1024 {
                return Err("私钥文件过大".into());
            }
        }
    }
    let by_id: HashMap<_, _> = profiles.iter().map(|p| (p.id.as_str(), p)).collect();
    for p in profiles {
        let mut visited = HashSet::from([p.id.as_str()]);
        let mut current = p;
        while let Some(jump) = &current.jump_host_id {
            current = *by_id.get(jump.as_str()).ok_or("跳板连接不存在")?;
            if p.enabled && !current.enabled {
                return Err("跳板连接未启用".into());
            }
            if !visited.insert(current.id.as_str()) || visited.len() > 8 {
                return Err("跳板连接循环或层级过多".into());
            }
        }
    }
    Ok(())
}

/// Flush a sibling temporary file, then replace the destination in one filesystem operation.
/// Windows rename cannot overwrite; MoveFileExW handles replacement without deleting the old file first.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .ok_or("文件必须包含父目录")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建文件目录")?;
    if let Ok(metadata) = fs::symlink_metadata(path) {
        if metadata.is_dir() || metadata.file_type().is_symlink() {
            return Err("目标必须为普通文件".into());
        }
    }
    let temporary = parent.join(format!(".astralink-{}.tmp", uuid::Uuid::new_v4()));
    let result: Result<(), String> = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|_| "无法创建临时文件")?;
        file.write_all(bytes).map_err(|_| "无法写入临时文件")?;
        file.sync_all().map_err(|_| "无法刷新临时文件")?;
        drop(file);
        replace_file(&temporary, path).map_err(|_| "无法原子替换文件，原文件已保留".to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}
#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(existing: *const u16, new: *const u16, flags: u32) -> i32;
    }
    let from: Vec<u16> = source.as_os_str().encode_wide().chain(Some(0)).collect();
    let to: Vec<u16> = target.as_os_str().encode_wide().chain(Some(0)).collect();
    // Both UTF-16 buffers are terminated and remain live for this synchronous call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0x1 | 0x8) } == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}
#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> std::io::Result<()> {
    fs::rename(source, target)
}

#[cfg(test)]
mod recovery_tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Clone, Default)]
    struct Vault(Arc<Mutex<HashMap<String, String>>>);
    impl SecretStore for Vault {
        fn get(&self, k: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(k).cloned())
        }
        fn set(&self, k: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(k.into(), v.into());
            Ok(())
        }
        fn delete(&self, k: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(k);
            Ok(())
        }
    }
    #[test]
    fn migrates_json_to_sqlite_and_preserves_vault_references_without_remigrating_backup() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::default();
        let id = uuid::Uuid::new_v4().to_string();
        let old = StoredState {
            profiles: vec![ConnectionProfile {
                id: id.clone(),
                name: "JumpServer".into(),
                host: "example.test".into(),
                username: "ops".into(),
                group: "测试".into(),
                has_password: true,
                has_totp_secret: true,
                try_keyboard: true,
                ..Default::default()
            }],
            settings: AppSettings {
                mcp_port: 18888,
                start_on_launch: true,
                minimize_to_tray: false,
            },
        };
        vault
            .set(&credential_key(&id, "password"), "vault-only-password")
            .unwrap();
        vault
            .set(
                &credential_key(&id, "totpSecret"),
                "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ",
            )
            .unwrap();
        let original = serde_json::to_vec_pretty(&old).unwrap();
        fs::write(dir.path().join("state.json"), &original).unwrap();
        let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        assert!(dir.path().join("astralink.db").exists());
        assert_eq!(store.profiles()[0].id, id);
        assert_eq!(store.settings().mcp_port, 18888);
        assert!(store.profiles()[0].has_totp_secret);
        assert_eq!(
            store.resolve(&id).unwrap().password.as_deref(),
            Some("vault-only-password")
        );
        let mut updated = store.profiles();
        updated[0].notes = "数据库更新".into();
        store.apply(updated, ImportStrategy::Merge).unwrap();
        assert_eq!(fs::read(dir.path().join("state.json")).unwrap(), original);
        let db = rusqlite::Connection::open(dir.path().join("astralink.db")).unwrap();
        let metadata: String = db
            .query_row(
                "SELECT metadata_json FROM servers WHERE id=?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!metadata.contains("vault-only-password"));
        assert!(!metadata.contains("GEZDGNBV"));
        drop(store);
        fs::write(dir.path().join("state.json"), "broken stale backup").unwrap();
        let reopened = AppStore::open(dir.path().into(), Box::new(vault)).unwrap();
        assert_eq!(reopened.profiles()[0].notes, "数据库更新");
    }
    #[test]
    fn damaged_or_newer_database_never_falls_back_to_legacy_backup() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
        drop(store);
        fs::write(
            dir.path().join("state.json"),
            "{\"profiles\":[],\"settings\":{}}",
        )
        .unwrap();
        let db = rusqlite::Connection::open(dir.path().join("astralink.db")).unwrap();
        db.pragma_update(None, "user_version", 2).unwrap();
        drop(db);
        assert!(
            matches!(AppStore::open(dir.path().into(),Box::new(Vault::default())),Err(error) if error.contains("版本"))
        );
        fs::write(dir.path().join("astralink.db"), "not a database").unwrap();
        assert!(AppStore::open(dir.path().into(), Box::new(Vault::default())).is_err());
    }
    #[test]
    fn plaintext_credentials_in_tampered_database_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
        store
            .apply(
                vec![ConnectionProfile {
                    id: uuid::Uuid::new_v4().to_string(),
                    name: "test".into(),
                    host: "example.test".into(),
                    username: "ops".into(),
                    ..Default::default()
                }],
                ImportStrategy::Merge,
            )
            .unwrap();
        drop(store);
        let db = rusqlite::Connection::open(dir.path().join("astralink.db")).unwrap();
        db.execute(
            "UPDATE servers SET metadata_json=json_set(metadata_json,'$.password',?1)",
            ["do-not-load-plaintext"],
        )
        .unwrap();
        drop(db);
        assert!(
            matches!(AppStore::open(dir.path().into(),Box::new(Vault::default())),Err(error) if error.contains("明文凭据"))
        );
    }
    fn fixture(committed: bool) -> (tempfile::TempDir, Vault, String) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::default();
        let id = uuid::Uuid::new_v4().to_string();
        let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        store
            .apply(
                vec![ConnectionProfile {
                    id: id.clone(),
                    name: "old".into(),
                    host: "test".into(),
                    username: "ops".into(),
                    password: Some("old-secret".into()),
                    enabled: true,
                    ..Default::default()
                }],
                ImportStrategy::Merge,
            )
            .unwrap();
        let target = credential_key(&id, "password");
        let prior = store.profiles();
        let mut changed = prior.clone();
        changed[0].name = "new".into();
        let candidate = StoredState {
            profiles: changed,
            settings: store.settings(),
        };
        let marker = TransactionMarker {
            id: uuid::Uuid::parse_str("109c99c0-4b51-4226-91f3-603dcc7d42e1")
                .unwrap()
                .to_string(),
            candidate: serde_json::to_vec_pretty(&candidate).unwrap(),
            entries: vec![TransactionEntry {
                target: target.clone(),
                backup: true,
                index: 0,
            }],
            rolled_back: false,
        };
        vault.set(&backup_key(&marker.id, 0), "old-secret").unwrap();
        atomic_write(
            &dir.path().join("transaction.json"),
            &serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        vault.set(&target, "new-secret").unwrap();
        if committed {
            store.write_state(&candidate).unwrap();
        }
        drop(store);
        (dir, vault, id)
    }
    #[test]
    fn crash_before_metadata_commit_restores_old_credential() {
        let (dir, vault, id) = fixture(false);
        let store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        assert_eq!(
            store.resolve(&id).unwrap().password.as_deref(),
            Some("old-secret")
        );
        assert_eq!(store.profiles()[0].name, "old");
        assert!(!dir.path().join("transaction.json").exists());
    }
    #[test]
    fn interrupted_backup_cleanup_after_rollback_does_not_require_deleted_backup() {
        let (dir, vault, id) = fixture(false);
        let mut marker: TransactionMarker =
            serde_json::from_slice(&fs::read(dir.path().join("transaction.json")).unwrap())
                .unwrap();
        vault
            .set(&credential_key(&id, "password"), "old-secret")
            .unwrap();
        vault.delete(&backup_key(&marker.id, 0)).unwrap();
        marker.rolled_back = true;
        atomic_write(
            &dir.path().join("transaction.json"),
            &serde_json::to_vec(&marker).unwrap(),
        )
        .unwrap();
        let store = AppStore::open(dir.path().into(), Box::new(vault)).unwrap();
        assert_eq!(
            store.resolve(&id).unwrap().password.as_deref(),
            Some("old-secret")
        );
        assert!(!dir.path().join("transaction.json").exists());
    }
    #[test]
    fn crash_after_metadata_commit_keeps_new_credential() {
        let (dir, vault, id) = fixture(true);
        let store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        assert_eq!(
            store.resolve(&id).unwrap().password.as_deref(),
            Some("new-secret")
        );
        assert_eq!(store.profiles()[0].name, "new");
        assert!(!dir.path().join("transaction.json").exists());
    }
    #[test]
    fn totp_seed_is_vault_only_and_survives_update_until_explicitly_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::default();
        let id = uuid::Uuid::new_v4().to_string();
        let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        store
            .apply(
                vec![ConnectionProfile {
                    id: id.clone(),
                    name: "mfa".into(),
                    host: "test".into(),
                    username: "ops".into(),
                    totp_secret: Some("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ".into()),
                    ..Default::default()
                }],
                ImportStrategy::Merge,
            )
            .unwrap();
        assert!(store.profiles()[0].has_totp_secret);
        assert!(
            !String::from_utf8(sqlite_store::committed_bytes(dir.path()).unwrap())
                .unwrap()
                .contains("GEZDG")
        );
        assert_eq!(
            store.resolve(&id).unwrap().totp_secret.as_deref(),
            Some("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ")
        );
        let mut update = store.profiles();
        update[0].name = "renamed".into();
        store.apply(update, ImportStrategy::Replace).unwrap();
        assert_eq!(
            store.resolve(&id).unwrap().totp_secret.as_deref(),
            Some("GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ")
        );
        let mut clear = store.profiles();
        clear[0].totp_secret = Some(String::new());
        store.apply(clear, ImportStrategy::Replace).unwrap();
        assert!(!store.profiles()[0].has_totp_secret);
        assert_eq!(store.resolve(&id).unwrap().totp_secret, None);
        assert_eq!(vault.get(&credential_key(&id, "totpSecret")).unwrap(), None);
    }
    #[test]
    fn explicit_secret_read_is_scoped_to_existing_profiles_and_known_fields() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::default();
        let id = uuid::Uuid::new_v4().to_string();
        let other = uuid::Uuid::new_v4().to_string();
        let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
        store
            .apply(
                vec![ConnectionProfile {
                    id: id.clone(),
                    name: "test".into(),
                    host: "example.test".into(),
                    username: "ops".into(),
                    password: Some("saved-secret".into()),
                    ..Default::default()
                }],
                ImportStrategy::Merge,
            )
            .unwrap();
        assert_eq!(
            store.read_secret(&id, "password").unwrap().as_deref(),
            Some("saved-secret")
        );
        assert_eq!(store.read_secret(&id, "passphrase").unwrap(), None);
        assert!(store.read_secret(&id, "mcp-token").is_err());
        assert!(store.read_secret(&other, "password").is_err());
        assert!(store.read_secret("not-a-uuid", "password").is_err());
    }
    #[test]
    fn opening_metadata_with_plaintext_totp_seed_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("state.json"),
            r#"{"profiles":[{"id":"11111111-1111-4111-8111-111111111111","name":"mfa","host":"test","port":22,"username":"ops","totpSecret":"GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ"}],"settings":{}}"#,
        ).unwrap();
        assert!(matches!(
            AppStore::open(dir.path().into(), Box::new(Vault::default())),
            Err(error) if error.contains("明文凭据")
        ));
    }
}
