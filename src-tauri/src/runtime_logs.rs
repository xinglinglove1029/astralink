use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    pub id: u64,
    pub timestamp: u64,
    pub level: String,
    pub source: String,
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogSnapshot {
    pub entries: Vec<LogEntry>,
    pub log_path: String,
    pub error: Option<String>,
}
struct Inner {
    entries: VecDeque<LogEntry>,
    next_id: u64,
    error: Option<String>,
}
pub struct RuntimeLogs {
    directory: PathBuf,
    max_bytes: u64,
    inner: Mutex<Inner>,
}

fn clock() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn timestamp_text(milliseconds: u64) -> String {
    let seconds = milliseconds / 1000;
    let day_seconds = seconds % 86400;
    let z = (seconds / 86400) as i64 + 719468;
    let era = z / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    year += if month <= 2 { 1 } else { 0 };
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{:03}Z",
        day_seconds / 3600,
        day_seconds / 60 % 60,
        day_seconds % 60,
        milliseconds % 1000
    )
}
fn context(value: Option<&str>) -> Option<String> {
    value
        .and_then(|id| uuid::Uuid::parse_str(id).ok())
        .map(|id| id.to_string())
}
fn event(code: &str) -> Option<(&'static str, &'static str, &'static str)> {
    Some(match code {
        "app_ready" => ("info", "desktop", "软件启动，运行日志已启用"),
        "app_start" => ("info", "desktop", "软件开始启动，正在读取本机配置"),
        "app_failed" => (
            "error",
            "desktop",
            "软件启动失败，请检查本机配置与系统凭据库",
        ),
        "app_shutdown" => ("info", "desktop", "软件正在退出"),
        "service_start" => ("info", "desktop", "正在启动 MCP 服务"),
        "service_ready" => ("info", "desktop", "MCP 服务启动成功"),
        "service_node_missing" => (
            "error",
            "desktop",
            "未找到 Node.js，请检查安装或 ASTRALINK_NODE_BINARY 配置",
        ),
        "service_port_busy" => (
            "error",
            "desktop",
            "MCP 端口已被占用，请修改端口或关闭占用进程",
        ),
        "service_entry_missing" => ("error", "desktop", "MCP 引擎资源缺失，请检查安装目录"),
        "service_failed" => (
            "error",
            "desktop",
            "MCP 服务启动失败，请检查运行环境、端口和凭据配置",
        ),
        "service_stop" => ("info", "desktop", "MCP 服务已停止"),
        "service_lost" => ("error", "desktop", "MCP 引擎退出或控制通道不可用"),
        "service_recovered" => ("info", "desktop", "MCP 控制通道已恢复"),
        "logs_dropped" => ("warn", "desktop", "引擎日志缓存已轮换，部分早期记录不可用"),
        "engine_ready" => ("info", "engine", "本机 MCP 引擎已就绪"),
        "profiles_synced" => ("info", "engine", "服务器配置已同步到引擎"),
        "config_failed" => ("error", "desktop", "服务器配置同步失败"),
        "ssh_connecting" => ("info", "ssh", "开始建立 SSH 连接"),
        "ssh_reused" => ("info", "ssh", "复用已有的已认证 SSH 连接"),
        "ssh_dns_failed" => ("error", "ssh", "SSH 主机名解析失败，请检查主机地址和 DNS"),
        "ssh_refused" => (
            "error",
            "ssh",
            "SSH 端口拒绝连接，请检查端口、服务状态和防火墙",
        ),
        "ssh_reset" => ("error", "ssh", "SSH 连接被对端重置，请检查服务器或网络"),
        "ssh_authenticated" => ("info", "ssh", "SSH 身份认证成功"),
        "ssh_auth_failed" => ("error", "ssh", "SSH 服务端拒绝认证，请检查密码和动态验证码"),
        "ssh_timeout" => (
            "error",
            "ssh",
            "SSH 建连或认证等待超时，请检查网络和认证流程",
        ),
        "ssh_connection_failed" => ("error", "ssh", "SSH 连接失败、关闭或超时"),
        "auth_otp" => ("info", "auth", "服务端请求动态验证码"),
        "auth_otp_echo" => ("info", "auth", "服务端请求动态验证码，并设置了输入回显标记"),
        "auth_password" => ("info", "auth", "使用已保存密码回答明确的密码提示"),
        "auth_unknown" => ("warn", "auth", "服务端请求其他交互认证，不自动填写"),
        "otp_auto" => ("info", "auth", "动态验证码已自动生成并提交，等待服务端结果"),
        "otp_near_expiry" => ("warn", "auth", "动态验证码即将换期，回退手动输入"),
        "otp_step_used" => (
            "warn",
            "auth",
            "当前时间步已自动提交过验证码，避免重复使用，回退手动输入",
        ),
        "otp_missing" => ("warn", "auth", "未配置动态密钥，等待手动输入"),
        "otp_prompt_unknown" => ("warn", "auth", "认证提示不明确，等待手动输入"),
        "auth_manual" => ("info", "auth", "等待用户回答认证挑战"),
        "auth_manual_submitted" => ("info", "auth", "手动认证信息已提交，等待服务端结果"),
        "auth_cancelled" => ("warn", "auth", "认证已取消"),
        "auth_expired" => ("warn", "auth", "手动认证等待已超时"),
        "auth_unsupported" => (
            "error",
            "auth",
            "认证请求无法处理；可能包含多个未知提示或待处理请求过多",
        ),
        "test_started" => ("info", "test", "开始连接测试"),
        "test_command" => ("info", "test", "已连接，正在验证只读命令"),
        "test_connected" => ("info", "test", "连接测试通过，只读命令执行成功"),
        "test_target_menu" => (
            "warn",
            "test",
            "JumpServer 已登录，当前停在资产菜单；请配置目标资产和目标账号",
        ),
        "test_failed" => (
            "error",
            "test",
            "连接测试失败，请查看同一关联编号的前序记录",
        ),
        "test_auth_incomplete" => ("warn", "test", "本次测试的认证未完成，请重新测试"),
        "tool_started" => ("info", "mcp", "开始执行 MCP 工具请求"),
        "tool_finished" => ("info", "mcp", "MCP 工具请求已结束"),
        "tool_failed" => ("error", "mcp", "MCP 工具请求失败"),
        _ => return None,
    })
}
impl RuntimeLogs {
    pub fn open(directory: PathBuf) -> Self {
        Self::open_with_limit(directory, 1024 * 1024)
    }
    fn open_with_limit(directory: PathBuf, max_bytes: u64) -> Self {
        let store = Self {
            directory,
            max_bytes,
            inner: Mutex::new(Inner {
                entries: VecDeque::new(),
                next_id: 0,
                error: None,
            }),
        };
        if fs::create_dir_all(&store.directory).is_err() {
            store.inner.lock().unwrap().error =
                Some("无法创建日志目录；本次日志仅保存在内存".into());
            return store;
        }
        for name in ["runtime.2.jsonl", "runtime.1.jsonl", "runtime.jsonl"] {
            let path = store.directory.join(name);
            if let Ok(meta) = fs::metadata(&path) {
                if !meta.is_file() || meta.len() > max_bytes {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(path) {
                    for line in text.lines().filter(|line| line.len() <= 4096) {
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
                            if let Some(entry) = store.decode(&value) {
                                store.push(entry, false);
                            }
                        }
                    }
                }
            }
        }
        store
    }
    fn decode(&self, value: &serde_json::Value) -> Option<LogEntry> {
        let code = value.get("code")?.as_str()?;
        let (level, source, message) = event(code)?;
        let timestamp = value.get("timestamp")?.as_u64()?;
        if timestamp > clock() + 60_000 {
            return None;
        }
        let profile_id = value.get("profileId").and_then(|v| v.as_str());
        let request_id = value.get("requestId").and_then(|v| v.as_str());
        if profile_id.is_some_and(|id| context(Some(id)).is_none())
            || request_id.is_some_and(|id| context(Some(id)).is_none())
        {
            return None;
        }
        Some(LogEntry {
            id: 0,
            timestamp,
            code: code.into(),
            level: level.into(),
            source: source.into(),
            message: message.into(),
            profile_id: context(profile_id),
            request_id: context(request_id),
        })
    }
    pub fn record(&self, code: &str, profile_id: Option<&str>, request_id: Option<&str>) {
        self.ingest(&serde_json::json!({ "code": code, "timestamp": clock(), "profileId": profile_id, "requestId": request_id }));
    }
    pub fn ingest(&self, value: &serde_json::Value) {
        if let Some(entry) = self.decode(value) {
            self.push(entry, true);
        }
    }
    fn push(&self, mut entry: LogEntry, persist: bool) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        inner.next_id += 1;
        entry.id = inner.next_id;
        if persist && self.append(&entry).is_err() {
            inner.error = Some("写入日志失败；日志仍可在当前窗口查看和导出".into());
        }
        inner.entries.push_back(entry);
        while inner.entries.len() > 1000 {
            inner.entries.pop_front();
        }
    }
    fn append(&self, entry: &LogEntry) -> std::io::Result<()> {
        let path = self.directory.join("runtime.jsonl");
        let mut bytes = serde_json::to_vec(entry)?;
        bytes.push(b'\n');
        if fs::metadata(&path).map(|m| m.len()).unwrap_or(0) + bytes.len() as u64 > self.max_bytes {
            let oldest = self.directory.join("runtime.2.jsonl");
            if oldest.exists() {
                fs::remove_file(&oldest)?;
            }
            let previous = self.directory.join("runtime.1.jsonl");
            if previous.exists() {
                fs::rename(&previous, oldest)?;
            }
            if path.exists() {
                fs::rename(&path, previous)?;
            }
        }
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(&bytes)
    }
    pub fn snapshot(&self) -> LogSnapshot {
        let inner = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        LogSnapshot {
            entries: inner.entries.iter().cloned().collect(),
            log_path: self
                .directory
                .join("runtime.jsonl")
                .to_string_lossy()
                .into(),
            error: inner.error.clone(),
        }
    }
    pub fn export(&self, path: &Path) -> Result<(), String> {
        if !path.is_absolute()
            || !path.extension().is_some_and(|e| e == "log" || e == "txt")
            || path.starts_with(&self.directory)
        {
            return Err("请选择日志目录外的绝对路径 .log 或 .txt 文件".into());
        }
        let parent = path
            .parent()
            .and_then(|p| fs::canonicalize(p).ok())
            .ok_or("导出目录不存在或不可用")?;
        if fs::canonicalize(&self.directory)
            .ok()
            .is_some_and(|directory| parent.starts_with(directory))
        {
            return Err("请选择日志目录以外的位置".into());
        }
        let snapshot = self.snapshot();
        let text = snapshot
            .entries
            .iter()
            .map(|e| {
                format!(
                    "{} [{}] {} {} {} profile={} request={}\n",
                    timestamp_text(e.timestamp),
                    e.level,
                    e.source,
                    e.code,
                    e.message,
                    e.profile_id.as_deref().unwrap_or("-"),
                    e.request_id.as_deref().unwrap_or("-")
                )
            })
            .collect::<String>();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .map_err(|_| "无法创建导出文件，请选择一个新文件名")?;
        file.write_all(text.as_bytes())
            .map_err(|_| "无法写入导出文件")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exported_timestamps_are_readable_utc() {
        assert_eq!(timestamp_text(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp_text(1700000000123), "2023-11-14T22:13:20.123Z");
        assert_eq!(timestamp_text(951782400000), "2000-02-29T00:00:00.000Z");
    }
    #[test]
    fn tampered_persisted_messages_and_invalid_identifiers_are_never_replayed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("runtime.jsonl"), "{\"timestamp\":1700000000000,\"code\":\"ssh_authenticated\",\"message\":\"private-secret\",\"source\":\"private-secret\"}\n{\"timestamp\":1700000000000,\"code\":\"otp_auto\",\"profileId\":\"private-secret\"}\n").unwrap();
        let store = RuntimeLogs::open(dir.path().into());
        assert_eq!(store.snapshot().entries.len(), 1);
        assert!(!serde_json::to_string(&store.snapshot())
            .unwrap()
            .contains("private-secret"));
    }
    #[test]
    fn fixed_events_persist_reload_rotate_and_export_without_unknown_fields() {
        let dir = tempfile::tempdir().unwrap();
        let store = RuntimeLogs::open_with_limit(dir.path().join("logs"), 700);
        let id = "11111111-1111-4111-8111-111111111111";
        store.record("ssh_authenticated", Some(id), None);
        store.record("password=private-secret", Some(id), None);
        store.ingest(&serde_json::json!({"timestamp": 1700000000000u64, "code":"otp_auto", "profileId": id, "message":"private-secret", "password":"private-secret"}));
        for _ in 0..20 {
            store.record("test_target_menu", Some(id), None);
        }
        let snapshot = store.snapshot();
        assert!(snapshot.entries.len() <= 1000);
        assert!(!serde_json::to_string(&snapshot)
            .unwrap()
            .contains("private-secret"));
        assert!(dir.path().join("logs/runtime.1.jsonl").exists());
        let reopened = RuntimeLogs::open_with_limit(dir.path().join("logs"), 700);
        assert!(!reopened.snapshot().entries.is_empty());
        let exported = dir.path().join("export.log");
        reopened.export(&exported).unwrap();
        assert!(std::fs::read_to_string(&exported)
            .unwrap()
            .contains("JumpServer"));
        assert!(reopened.export(&exported).is_err());
    }
    #[test]
    fn unavailable_disk_keeps_memory_log_and_exposes_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, "test").unwrap();
        let logs = RuntimeLogs::open(file.join("logs"));
        logs.record("service_start", None, None);
        assert_eq!(logs.snapshot().entries.len(), 1);
        assert!(logs.snapshot().error.is_some());
    }
}
