use serde::{Deserialize, Serialize};
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthType {
    #[default]
    Password,
    PrivateKey,
    Agent,
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum TransportMode {
    #[default]
    Exec,
    Shell,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ConnectionProfile {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub group: String,
    pub source: String,
    pub auth_type: AuthType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub private_key_path: Option<String>,
    #[serde(skip_serializing)]
    pub password: Option<String>,
    #[serde(skip_serializing)]
    pub passphrase: Option<String>,
    #[serde(skip_serializing)]
    pub totp_secret: Option<String>,
    pub has_password: bool,
    pub has_passphrase: bool,
    pub has_totp_secret: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_local_paths: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_remote_paths: Option<Vec<String>>,
    pub transport_mode: TransportMode,
    pub try_keyboard: bool,
    pub enabled: bool,
    pub notes: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jump_host_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_username: Option<String>,
}
impl Default for ConnectionProfile {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            host: String::new(),
            port: 22,
            username: String::new(),
            group: String::new(),
            source: String::new(),
            auth_type: AuthType::Password,
            private_key_path: None,
            password: None,
            passphrase: None,
            totp_secret: None,
            has_password: false,
            has_passphrase: false,
            has_totp_secret: false,
            agent: None,
            allowed_local_paths: None,
            allowed_remote_paths: None,
            transport_mode: TransportMode::Exec,
            try_keyboard: false,
            enabled: true,
            notes: String::new(),
            jump_host_id: None,
            target_host: None,
            target_username: None,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    pub mcp_port: u16,
    pub start_on_launch: bool,
    pub minimize_to_tray: bool,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StorageInfo {
    pub backend: String,
    pub database_path: String,
    pub legacy_backup_path: Option<String>,
    pub server_count: usize,
}
impl Default for AppSettings {
    fn default() -> Self {
        Self {
            mcp_port: 18766,
            start_on_launch: false,
            minimize_to_tray: true,
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub running: bool,
    pub endpoint: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppState {
    pub profiles: Vec<ConnectionProfile>,
    pub settings: AppSettings,
    pub engine: EngineStatus,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportIssue {
    pub severity: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_name: Option<String>,
}
#[derive(Default, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportStats {
    pub total: usize,
    pub ready: usize,
    pub needs_credential: usize,
    pub unsupported: usize,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    pub source: String,
    pub source_path: String,
    pub profiles: Vec<ConnectionProfile>,
    pub issues: Vec<ImportIssue>,
    pub stats: ImportStats,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceCandidate {
    pub kind: String,
    pub label: String,
    pub path: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_count: Option<usize>,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthPrompt {
    pub prompt: String,
    pub echo: bool,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthChallenge {
    pub request_id: String,
    pub profile_id: String,
    pub connection_name: String,
    pub host: String,
    pub username: String,
    pub instructions: String,
    pub prompts: Vec<AuthPrompt>,
    pub expires_at: u64,
}
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionTest {
    pub status: String,
    pub stage: String,
    pub detail: String,
    pub elapsed_ms: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stdout: Option<String>,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImportStrategy {
    Merge,
    Replace,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodexRegistration {
    pub config_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_path: Option<String>,
    pub reload_required: bool,
}
