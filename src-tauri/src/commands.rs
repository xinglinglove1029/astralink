use crate::{
    client_setup, config_sync,
    credentials::SecretStore,
    discovery,
    engine::{EngineManager, RuntimePaths},
    importers,
    models::*,
    runtime_logs::{LogSnapshot, RuntimeLogs},
    storage::AppStore,
};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
    time::{Duration, Instant},
};
use tauri::State;
struct PendingPreview {
    expires: Instant,
    profiles: Vec<ConnectionProfile>,
}
pub struct AppCore {
    store: Mutex<AppStore>,
    pub engine: EngineManager,
    pub logs: Arc<RuntimeLogs>,
    pending: Mutex<Option<PendingPreview>>,
    home: PathBuf,
    roaming: Option<PathBuf>,
    documents: Option<PathBuf>,
    codex_config: PathBuf,
    operations: tokio::sync::Mutex<()>,
    shutting_down: std::sync::atomic::AtomicBool,
}
impl AppCore {
    pub fn open(
        directory: PathBuf,
        secrets: Box<dyn SecretStore>,
        paths: RuntimePaths,
        home: PathBuf,
        roaming: Option<PathBuf>,
        documents: Option<PathBuf>,
        codex_config: PathBuf,
    ) -> Result<Self, String> {
        let logs = Arc::new(RuntimeLogs::open(directory.join("logs")));
        logs.record("app_start", None, None);
        let store = AppStore::open(directory, secrets).map_err(|error| {
            logs.record("app_failed", None, None);
            error
        })?;
        let engine = EngineManager::new(paths, store.settings().mcp_port, String::new())?
            .with_logs(logs.clone());
        logs.record("app_ready", None, None);
        Ok(Self {
            store: Mutex::new(store),
            engine,
            logs,
            pending: Mutex::new(None),
            home,
            roaming,
            documents,
            codex_config,
            operations: tokio::sync::Mutex::new(()),
            shutting_down: std::sync::atomic::AtomicBool::new(false),
        })
    }
    pub fn begin_shutdown(&self) {
        self.shutting_down
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
    fn ensure_active(&self) -> Result<(), String> {
        if self.shutting_down.load(std::sync::atomic::Ordering::SeqCst) {
            Err("应用正在退出".into())
        } else {
            Ok(())
        }
    }
    pub async fn stop_service(&self) -> EngineStatus {
        let _operation = self.operations.lock().await;
        self.engine.stop().await
    }
    pub async fn shutdown(&self) {
        self.logs.record("app_shutdown", None, None);
        self.begin_shutdown();
        self.stop_service().await;
    }
    pub(crate) fn store(&self) -> Result<MutexGuard<'_, AppStore>, String> {
        self.store.lock().map_err(|_| "应用数据锁不可用".into())
    }
    pub fn preview(&self, kind: &str, path: &Path) -> Result<ImportPreview, String> {
        let mut preview = importers::preview_file(kind, path, &self.home)?;
        let cached = preview.profiles.clone();
        for p in &mut preview.profiles {
            p.password = None;
            p.passphrase = None;
            p.totp_secret = None;
            p.has_totp_secret = false;
        }
        *self.pending.lock().map_err(|_| "导入预览锁不可用")? = Some(PendingPreview {
            expires: Instant::now() + Duration::from_secs(600),
            profiles: cached,
        });
        Ok(preview)
    }
    pub fn apply(
        &self,
        mut profiles: Vec<ConnectionProfile>,
        strategy: ImportStrategy,
    ) -> Result<Vec<ConnectionProfile>, String> {
        let mut pending = self.pending.lock().map_err(|_| "导入预览锁不可用")?;
        if pending
            .as_ref()
            .is_some_and(|p| p.expires <= Instant::now())
        {
            *pending = None
        }
        let mut store = self.store()?;
        let existing = store.profiles();
        for p in &mut profiles {
            if let Some(source) = pending.as_ref().and_then(|pending| {
                pending
                    .profiles
                    .iter()
                    .find(|cached| same_identity(cached, p))
            }) {
                if p.password.is_none() {
                    p.password = source.password.clone()
                }
                if p.passphrase.is_none() {
                    p.passphrase = source.passphrase.clone()
                }
            }
            let old = existing.iter().find(|old| old.id == p.id);
            if p.has_password && p.password.is_none() && !old.is_some_and(|p| p.has_password) {
                return Err("导入密码已失效或连接身份已修改，请重新预览或填写密码".into());
            }
            if p.has_passphrase && p.passphrase.is_none() && !old.is_some_and(|p| p.has_passphrase)
            {
                return Err("导入私钥口令已失效，请重新预览或填写口令".into());
            }
        }
        let saved = store.apply(profiles, strategy)?;
        *pending = None;
        Ok(saved)
    }
    async fn commit_and_sync(
        &self,
        mutate: impl FnOnce(&Self) -> Result<Vec<ConnectionProfile>, String>,
    ) -> Result<Vec<ConnectionProfile>, String> {
        let previous = {
            let store = self.store()?;
            store
                .profiles()
                .iter()
                .map(|p| store.resolve(&p.id))
                .collect::<Result<Vec<_>, _>>()?
        };
        let saved = mutate(self)?;
        if let Err(error) = self.sync().await {
            self.logs.record("config_failed", None, None);
            let rollback = self.store()?.apply(previous, ImportStrategy::Replace);
            match rollback {
                Ok(_) => {
                    let resync = self.sync().await;
                    return Err(if resync.is_ok() {
                        format!("MCP 未接受配置，已撤销本次保存：{error}")
                    } else {
                        "本地更改已撤销，但 MCP 状态尚未确认；请停止并重启服务".into()
                    });
                }
                Err(_) => return Err("MCP 更新失败且凭据回滚失败；请检查本地配置和服务状态".into()),
            }
        }
        Ok(saved)
    }
    async fn sync(&self) -> Result<(), String> {
        let profiles = self.store()?.runtime_profiles()?;
        self.engine
            .sync(profiles)
            .await
            .map_err(|error| format!("配置已保存，但 MCP 更新失败：{error}"))?;
        Ok(())
    }
}
fn same_identity(a: &ConnectionProfile, b: &ConnectionProfile) -> bool {
    a.id == b.id
        && a.host == b.host
        && a.port == b.port
        && a.username == b.username
        && a.auth_type == b.auth_type
        && a.private_key_path == b.private_key_path
        && a.agent == b.agent
        && a.jump_host_id == b.jump_host_id
        && a.target_host == b.target_host
        && a.target_username == b.target_username
}
#[tauri::command]
pub fn get_runtime_logs(state: State<'_, AppCore>) -> LogSnapshot {
    state.logs.snapshot()
}
#[tauri::command]
pub fn get_storage_info(state: State<'_, AppCore>) -> Result<StorageInfo, String> {
    Ok(state.store()?.storage_info())
}
#[tauri::command]
pub fn export_runtime_logs(state: State<'_, AppCore>, file_path: String) -> Result<(), String> {
    state.logs.export(Path::new(&file_path))
}
#[tauri::command]
pub async fn get_app_state(state: State<'_, AppCore>) -> Result<AppState, String> {
    let (profiles, settings) = {
        let store = state.store()?;
        (store.profiles(), store.settings())
    };
    Ok(AppState {
        profiles,
        settings,
        engine: state.engine.status().await,
    })
}
#[tauri::command]
pub fn reveal_profile_secret(
    state: State<'_, AppCore>,
    profile_id: String,
    field: String,
) -> Result<Option<String>, String> {
    state.ensure_active()?;
    state.store()?.read_secret(&profile_id, &field)
}
#[tauri::command]
pub fn discover_sources(state: State<'_, AppCore>) -> Vec<SourceCandidate> {
    discovery::discover(
        &state.home,
        state.roaming.as_deref(),
        state.documents.as_deref(),
    )
}
#[tauri::command]
pub fn preview_import(
    state: State<'_, AppCore>,
    source_kind: String,
    file_path: String,
) -> Result<ImportPreview, String> {
    state.preview(&source_kind, Path::new(&file_path))
}
#[tauri::command]
pub async fn apply_import(
    state: State<'_, AppCore>,
    mut profiles: Vec<ConnectionProfile>,
    strategy: ImportStrategy,
) -> Result<Vec<ConnectionProfile>, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    for profile in &mut profiles {
        profile.totp_secret = None;
        profile.has_totp_secret = false;
    }
    state
        .commit_and_sync(|core| core.apply(profiles, strategy))
        .await
}
#[tauri::command]
pub async fn upsert_profile(
    state: State<'_, AppCore>,
    profile: ConnectionProfile,
) -> Result<Vec<ConnectionProfile>, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    state
        .commit_and_sync(|core| core.apply(vec![profile], ImportStrategy::Merge))
        .await
}
#[tauri::command]
pub async fn delete_profile(
    state: State<'_, AppCore>,
    profile_id: String,
) -> Result<Vec<ConnectionProfile>, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    state
        .commit_and_sync(|core| core.store()?.delete(&profile_id))
        .await
}
#[tauri::command]
pub async fn delete_profiles(
    state: State<'_, AppCore>,
    profile_ids: Vec<String>,
) -> Result<Vec<ConnectionProfile>, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    state
        .commit_and_sync(|core| core.store()?.delete_many(&profile_ids))
        .await
}
#[tauri::command]
pub async fn update_settings(
    state: State<'_, AppCore>,
    settings: AppSettings,
) -> Result<AppSettings, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    let old = state.store()?.settings();
    if old.mcp_port != settings.mcp_port && state.engine.status().await.running {
        return Err("请先停止 MCP 服务再修改端口".into());
    }
    let settings = state.store()?.update_settings(settings)?;
    state.engine.set_port(settings.mcp_port).await?;
    Ok(settings)
}
#[tauri::command]
pub async fn start_engine(state: State<'_, AppCore>) -> Result<EngineStatus, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    let (port, token, profiles) = {
        let store = state.store()?;
        (
            store.settings().mcp_port,
            store.mcp_token()?,
            store.runtime_profiles()?,
        )
    };
    state.engine.configure(port, token).await?;
    Ok(state.engine.start(profiles).await)
}
#[tauri::command]
pub async fn stop_engine(state: State<'_, AppCore>) -> Result<EngineStatus, String> {
    let _operation = state.operations.lock().await;
    Ok(state.engine.stop().await)
}
#[tauri::command]
pub async fn get_engine_status(state: State<'_, AppCore>) -> Result<EngineStatus, String> {
    Ok(state.engine.status().await)
}
#[tauri::command]
pub async fn test_connection(
    state: State<'_, AppCore>,
    profile_id: String,
) -> Result<ConnectionTest, String> {
    {
        let store = state.store()?;
        let profile = store
            .profiles()
            .into_iter()
            .find(|p| p.id == profile_id)
            .ok_or("连接不存在")?;
        if !profile.enabled {
            return Err("连接未启用".into());
        }
    }
    Ok(state.engine.test(&profile_id).await)
}
#[tauri::command]
pub async fn submit_otp(
    state: State<'_, AppCore>,
    request_id: String,
    code: String,
) -> Result<bool, String> {
    state.engine.auth(&request_id, Some(code), false).await
}
#[tauri::command]
pub async fn cancel_auth(state: State<'_, AppCore>, request_id: String) -> Result<bool, String> {
    state.engine.auth(&request_id, None, true).await
}
#[tauri::command]
pub async fn register_codex(state: State<'_, AppCore>) -> Result<CodexRegistration, String> {
    let _operation = state.operations.lock().await;
    let store = state.store()?;
    config_sync::register(
        &state.codex_config,
        store.settings().mcp_port,
        &store.mcp_token()?,
    )
}

#[tauri::command]
pub async fn get_mcp_token(state: State<'_, AppCore>) -> Result<String, String> {
    state.ensure_active()?;
    state.store()?.mcp_token()
}
#[cfg(test)]
#[path = "commands_tests.rs"]
mod tests;

#[tauri::command]
pub async fn register_mcp_client(
    state: State<'_, AppCore>,
    client_id: String,
    file_path: Option<String>,
) -> Result<CodexRegistration, String> {
    let _operation = state.operations.lock().await;
    state.ensure_active()?;
    let path = match file_path {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("请选择绝对路径的配置文件".into());
            }
            path
        }
        None if client_id == "codex" => state.codex_config.clone(),
        None => client_setup::default_path(&client_id, &state.home, state.roaming.as_deref())?,
    };
    let store = state.store()?;
    client_setup::register(
        &client_id,
        &path,
        store.settings().mcp_port,
        &store.mcp_token()?,
    )
}
