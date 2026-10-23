use crate::models::{AuthChallenge, ConnectionProfile, ConnectionTest, EngineStatus};
use crate::runtime_logs::RuntimeLogs;
use base64::Engine as _;
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex as StdMutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::Emitter;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct RuntimePaths {
    pub node: PathBuf,
    pub entry: PathBuf,
    pub cwd: PathBuf,
}
const NODE_INSTALL_HINT: &str =
    "未找到 Node.js，请安装 Node.js 并将 node 加入系统 PATH，或设置 ASTRALINK_NODE_BINARY 后重试";

fn node_name() -> &'static str {
    if cfg!(windows) {
        "node.exe"
    } else {
        "node"
    }
}

fn system_node() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths)
                .map(|directory| directory.join(node_name()))
                .collect()
        })
        .unwrap_or_default();
    if cfg!(windows) {
        for root in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(root) = std::env::var_os(root) {
                candidates.push(PathBuf::from(root).join("nodejs/node.exe"));
            }
        }
        if let Some(root) = std::env::var_os("LOCALAPPDATA") {
            candidates.push(PathBuf::from(root).join("Programs/nodejs/node.exe"));
        }
    } else {
        // Finder/desktop launchers may omit Homebrew and /usr/local/bin from PATH.
        if cfg!(target_os = "macos") {
            candidates.push(PathBuf::from("/opt/homebrew/bin/node"));
        }
        candidates.extend(["/usr/local/bin/node", "/usr/bin/node", "/bin/node"].map(PathBuf::from));
    }
    candidates.into_iter().find(|candidate| candidate.is_file())
}

pub fn candidate_paths(
    dev_root: Option<&Path>,
    resource_dir: &Path,
    node_override: Option<&Path>,
) -> RuntimePaths {
    let directory = node_compatible_path(resource_dir);
    let bundled = directory.join("runtime").join(node_name());
    let node = node_override.map(node_compatible_path).unwrap_or_else(|| {
        if dev_root.is_none() && bundled.is_file() {
            bundled.clone()
        } else {
            system_node().unwrap_or_else(|| bundled.clone())
        }
    });
    let cwd = dev_root
        .map(|root| root.join("engine"))
        .unwrap_or_else(|| directory.join("engine"));
    let entry = cwd.join("src/main.mjs");
    RuntimePaths { node, entry, cwd }
}
fn node_compatible_path(path: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let value = path.to_string_lossy();
        if let Some(unc) = value.strip_prefix(r"\\?\UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        if let Some(drive) = value.strip_prefix(r"\\?\") {
            let bytes = drive.as_bytes();
            if bytes.len() >= 3
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
            {
                return PathBuf::from(drive);
            }
        }
    }
    path.to_path_buf()
}
pub fn resolve_paths(
    dev_root: Option<&Path>,
    resource_dir: &Path,
    node_override: Option<&Path>,
) -> Result<RuntimePaths, String> {
    let paths = candidate_paths(dev_root, resource_dir, node_override);
    if !paths.node.is_file() {
        return Err(NODE_INSTALL_HINT.into());
    }
    if !paths.entry.is_file() {
        return Err("MCP 引擎入口不存在，请检查引擎资源".into());
    }
    Ok(paths)
}
fn token() -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
struct Runtime {
    child: Option<Child>,
    port: u16,
    mcp_token: String,
    control_token: String,
    status: EngineStatus,
    cursor: u64,
    log_cursor: u64,
    pending: HashMap<String, AuthChallenge>,
    polling: bool,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
#[derive(Clone)]
pub struct EngineManager {
    paths: RuntimePaths,
    client: reqwest::Client,
    inner: Arc<Mutex<Runtime>>,
    emitter: Arc<StdMutex<Option<tauri::AppHandle>>>,
    logs: Option<Arc<RuntimeLogs>>,
}
impl EngineManager {
    pub fn bridge_paths(&self) -> Result<(PathBuf, PathBuf), String> {
        let script = self.paths.cwd.join("src/stdio-bridge.mjs");
        if !self.paths.node.is_file() || !script.is_file() {
            return Err("本机 MCP 桥接资源不存在，请检查安装包".into());
        }
        Ok((self.paths.node.clone(), script))
    }

    pub fn new(paths: RuntimePaths, port: u16, mcp_token: String) -> Result<Self, String> {
        // Explicitly bypass proxies: credentials only travel to the owned loopback child.
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "无法创建本机 HTTP 客户端")?;
        let status = stopped(port, None);
        Ok(Self {
            paths,
            client,
            inner: Arc::new(Mutex::new(Runtime {
                child: None,
                port,
                mcp_token,
                control_token: token(),
                status,
                cursor: 0,
                log_cursor: 0,
                pending: HashMap::new(),
                polling: false,
            })),
            emitter: Arc::new(StdMutex::new(None)),
            logs: None,
        })
    }
    pub fn with_logs(mut self, logs: Arc<RuntimeLogs>) -> Self {
        self.logs = Some(logs);
        self
    }
    fn log(&self, code: &str, profile_id: Option<&str>) {
        if let Some(logs) = &self.logs {
            logs.record(code, profile_id, None);
        }
    }
    async fn collect_logs(&self, runtime: &mut Runtime) {
        let Some(logs) = &self.logs else {
            return;
        };
        let path = format!("/control/logs?since={}", runtime.log_cursor);
        if let Ok(Ok(value)) = tokio::time::timeout(
            Duration::from_millis(500),
            self.request(runtime, reqwest::Method::GET, &path, None),
        )
        .await
        {
            if let (Some(cursor), Some(entries)) = (
                value.get("cursor").and_then(Value::as_u64),
                value.get("entries").and_then(Value::as_array),
            ) {
                if cursor < runtime.log_cursor || entries.len() > 512 {
                    return;
                }
                if value.get("dropped").and_then(Value::as_bool) == Some(true) {
                    logs.record("logs_dropped", None, None);
                }
                for entry in entries {
                    logs.ingest(entry);
                }
                runtime.log_cursor = cursor;
            }
        }
    }
    pub fn attach(&self, app: tauri::AppHandle) {
        if let Ok(mut emitter) = self.emitter.lock() {
            *emitter = Some(app)
        }
    }
    pub async fn pending_challenges(&self) -> Vec<AuthChallenge> {
        let mut runtime = self.inner.lock().await;
        runtime.pending.retain(|_, event| event.expires_at > now());
        runtime.pending.values().cloned().collect()
    }
    fn emit_status(&self, status: &EngineStatus) {
        if let Ok(app) = self.emitter.lock() {
            if let Some(app) = app.as_ref() {
                let _ = app.emit("astralink-engine", status);
            }
        }
    }
    fn emit_auth(&self, event: &AuthChallenge) {
        if let Ok(app) = self.emitter.lock() {
            if let Some(app) = app.as_ref() {
                let _ = app.emit("astralink-auth", event);
            }
        }
    }
    pub async fn set_port(&self, port: u16) -> Result<(), String> {
        let mut runtime = self.inner.lock().await;
        if port == 0 {
            return Err("MCP 端口无效".into());
        }
        if runtime.child.is_some() && runtime.port != port {
            return Err("请先停止 MCP 服务再修改端口".into());
        }
        runtime.port = port;
        if runtime.child.is_none() {
            runtime.status = stopped(port, None)
        }
        Ok(())
    }
    pub async fn configure(&self, port: u16, mcp_token: String) -> Result<(), String> {
        let mut runtime = self.inner.lock().await;
        if runtime.child.is_some() {
            if runtime.port == port && runtime.mcp_token == mcp_token {
                return Ok(());
            }
            return Err("请先停止 MCP 服务再修改端口".into());
        }
        if port == 0 || mcp_token.is_empty() {
            return Err("MCP 端口或令牌无效".into());
        }
        runtime.port = port;
        runtime.mcp_token = mcp_token;
        runtime.status = stopped(port, None);
        Ok(())
    }
    async fn request(
        &self,
        runtime: &Runtime,
        method: reqwest::Method,
        path: &str,
        body: Option<Value>,
    ) -> Result<Value, String> {
        let mut request = self
            .client
            .request(method, format!("http://127.0.0.1:{}{path}", runtime.port))
            .bearer_auth(&runtime.control_token);
        if let Some(body) = body {
            request = request.json(&body)
        }
        let response = request.send().await.map_err(|_| "无法访问 MCP 控制端点")?;
        if !response.status().is_success() {
            return Err(format!(
                "MCP 控制请求失败（HTTP {}）",
                response.status().as_u16()
            ));
        }
        // Never surface arbitrary response bodies: an engine error can contain resolved credentials.
        response
            .json()
            .await
            .map_err(|_| "MCP 控制响应格式无效".into())
    }
    async fn probe(&self, runtime: &mut Runtime) -> Result<EngineStatus, String> {
        let child = runtime.child.as_mut().ok_or("MCP 引擎未运行")?;
        if child
            .try_wait()
            .map_err(|_| "无法读取引擎进程状态")?
            .is_some()
        {
            return Err("MCP 引擎已退出".into());
        }
        let pid = child.id();
        let data = self
            .request(runtime, reqwest::Method::GET, "/control/status", None)
            .await?;
        let status: EngineStatus = serde_json::from_value(data).map_err(|_| "MCP 状态响应无效")?;
        if !status.running
            || status.pid != Some(pid)
            || status.endpoint != format!("http://127.0.0.1:{}/mcp", runtime.port)
        {
            return Err("MCP 状态与受控进程不一致".into());
        }
        Ok(status)
    }
    pub async fn start(&self, profiles: Vec<ConnectionProfile>) -> EngineStatus {
        self.log("service_start", None);
        let mut runtime = self.inner.lock().await;
        if runtime.child.is_some() {
            match self.probe(&mut runtime).await {
                Ok(s) => {
                    runtime.status = s.clone();
                    return s;
                }
                Err(error) => {
                    if child_has_exited(&mut runtime) {
                        terminate(&mut runtime);
                    } else {
                        runtime.status = stopped(runtime.port, Some(error));
                        return runtime.status.clone();
                    }
                }
            }
        }
        let result = async {
            if runtime.mcp_token.is_empty() {
                return Err("缺少 MCP 访问令牌".into());
            }
            if !self.paths.node.is_file() {
                return Err(NODE_INSTALL_HINT.into());
            }
            if !self.paths.entry.is_file() {
                return Err("MCP 引擎资源不存在，请重新安装 AstraLink".into());
            }
            let listener = std::net::TcpListener::bind(("127.0.0.1", runtime.port))
                .map_err(|_| "MCP 端口已被占用")?;
            drop(listener);
            runtime.control_token = token();
            runtime.cursor = 0;
            runtime.log_cursor = 0;
            runtime.pending.clear();
            let mut command = Command::new(&self.paths.node);
            command
                .arg(&self.paths.entry)
                .current_dir(&self.paths.cwd)
                .env("ASTRALINK_PORT", runtime.port.to_string())
                .env("ASTRALINK_CONTROL_TOKEN", &runtime.control_token)
                .env("ASTRALINK_MCP_TOKEN", &runtime.mcp_token)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped());
            // Do not inherit external Node hooks capable of intercepting credential IPC.
            command.env_remove("NODE_OPTIONS").env_remove("NODE_PATH");
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x08000000);
            }
            runtime.child = Some(command.spawn().map_err(|_| "无法启动 MCP 引擎")?);
            let mut ready = None;
            let deadline = std::time::Instant::now() + Duration::from_secs(10);
            for _ in 0..50 {
                if std::time::Instant::now() >= deadline {
                    break;
                }
                if let Some(child) = runtime.child.as_mut() {
                    if let Some(exit) = child.try_wait().map_err(|_| "无法读取引擎进程")? {
                        let mut output = [0u8; 2048];
                        let bytes = child
                            .stderr
                            .as_mut()
                            .and_then(|stderr| stderr.read(&mut output).ok())
                            .unwrap_or(0);
                        return Err(boot_failure(
                            exit.code(),
                            &String::from_utf8_lossy(&output[..bytes]),
                        ));
                    }
                }
                if let Ok(Ok(status)) =
                    tokio::time::timeout(Duration::from_secs(2), self.probe(&mut runtime)).await
                {
                    ready = Some(status);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            let status = ready.ok_or("MCP 引擎启动超时")?;
            self.send_profiles(&runtime, profiles).await?;
            Ok::<EngineStatus, String>(status)
        }
        .await;
        runtime.status = match result {
            Ok(status) => status,
            Err(error) => {
                terminate(&mut runtime);
                self.log(
                    if error == NODE_INSTALL_HINT {
                        "service_node_missing"
                    } else if error == "MCP 端口已被占用" {
                        "service_port_busy"
                    } else if error == "MCP 引擎资源不存在，请重新安装 AstraLink" {
                        "service_entry_missing"
                    } else {
                        "service_failed"
                    },
                    None,
                );
                stopped(runtime.port, Some(error))
            }
        };
        let status = runtime.status.clone();
        self.log(
            if status.running {
                "service_ready"
            } else {
                "service_failed"
            },
            None,
        );
        if status.running {
            self.collect_logs(&mut runtime).await;
        }
        let start_poll = status.running && !runtime.polling;
        if start_poll {
            runtime.polling = true
        }
        drop(runtime);
        self.emit_status(&status);
        if start_poll {
            let manager = self.clone();
            tauri::async_runtime::spawn(async move {
                manager.poll_loop().await;
            });
        }
        status
    }
    async fn send_profiles(
        &self,
        runtime: &Runtime,
        profiles: Vec<ConnectionProfile>,
    ) -> Result<(), String> {
        let count = profiles.len();
        let profiles: Vec<Value> = profiles
            .into_iter()
            .map(|p| {
                let mut value = serde_json::to_value(&p).map_err(|_| "无法序列化运行配置")?;
                if let Some(password) = p.password {
                    value["password"] = json!(password)
                }
                if let Some(passphrase) = p.passphrase {
                    value["passphrase"] = json!(passphrase)
                }
                if let Some(totp_secret) = p.totp_secret {
                    value["totpSecret"] = json!(totp_secret)
                }
                Ok::<Value, String>(value)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let response = self
            .request(
                runtime,
                reqwest::Method::POST,
                "/control/profiles",
                Some(json!({"profiles":profiles})),
            )
            .await?;
        if response.get("count").and_then(Value::as_u64) != Some(count as u64) {
            return Err("MCP 未确认完整配置更新".into());
        }
        Ok(())
    }
    pub async fn sync(&self, profiles: Vec<ConnectionProfile>) -> Result<bool, String> {
        let runtime = self.inner.lock().await;
        if runtime.child.is_none() {
            return Ok(false);
        }
        self.send_profiles(&runtime, profiles).await?;
        Ok(true)
    }
    pub async fn status(&self) -> EngineStatus {
        let mut runtime = self.inner.lock().await;
        if runtime.child.is_some() {
            runtime.status = match self.probe(&mut runtime).await {
                Ok(s) => s,
                Err(error) => {
                    if child_has_exited(&mut runtime) {
                        terminate(&mut runtime);
                    }
                    stopped(runtime.port, Some(error))
                }
            };
        }
        runtime.status.clone()
    }
    pub async fn stop(&self) -> EngineStatus {
        let mut runtime = self.inner.lock().await;
        if runtime.child.is_some() {
            self.collect_logs(&mut runtime).await;
        }
        if runtime.child.is_some() {
            let _ = self
                .request(
                    &runtime,
                    reqwest::Method::POST,
                    "/control/shutdown",
                    Some(json!({})),
                )
                .await;
            for _ in 0..100 {
                if runtime
                    .child
                    .as_mut()
                    .is_some_and(|c| c.try_wait().ok().flatten().is_some())
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        }
        terminate(&mut runtime);
        runtime.status = stopped(runtime.port, None);
        let status = runtime.status.clone();
        self.log("service_stop", None);
        drop(runtime);
        self.emit_status(&status);
        status
    }
    pub async fn test(&self, id: &str) -> ConnectionTest {
        let (port, control_token) = {
            let runtime = self.inner.lock().await;
            if !runtime.status.running {
                return failed("engine", "MCP 引擎未运行");
            }
            (runtime.port, runtime.control_token.clone())
        };
        // Do not hold the runtime mutex through SSH authentication: event polling/OTP must continue.
        let response = self
            .client
            .post(format!("http://127.0.0.1:{port}/control/test"))
            .bearer_auth(control_token)
            .timeout(Duration::from_secs(120))
            .json(&json!({"profileId":id}))
            .send()
            .await;
        match response {
            Ok(response) if response.status().is_success() => {
                match response.json::<ConnectionTest>().await {
                    Ok(test)
                        if ["connected", "partial", "auth_required", "failed"]
                            .contains(&test.status.as_str()) =>
                    {
                        test
                    }
                    _ => failed("engine", "MCP 测试响应格式无效"),
                }
            }
            Ok(_) => failed("engine", "MCP 拒绝连接测试请求"),
            Err(_) => failed("engine", "连接测试超时或引擎不可用"),
        }
    }
    pub async fn auth(
        &self,
        request_id: &str,
        code: Option<String>,
        cancel: bool,
    ) -> Result<bool, String> {
        let mut runtime = self.inner.lock().await;
        let Some(event) = runtime.pending.get(request_id) else {
            return Ok(false);
        };
        if event.expires_at <= now() {
            runtime.pending.remove(request_id);
            return Ok(false);
        }
        if !cancel
            && code
                .as_ref()
                .is_none_or(|s| s.is_empty() || s.len() > 1024 || s.chars().any(char::is_control))
        {
            return Err("验证码格式无效".into());
        }
        let mut body = json!({"requestId":request_id});
        if cancel {
            body["cancel"] = json!(true)
        } else {
            body["code"] = json!(code.unwrap_or_default())
        }
        let response = self
            .request(&runtime, reqwest::Method::POST, "/control/auth", Some(body))
            .await?;
        let accepted = response
            .get("accepted")
            .and_then(Value::as_bool)
            .ok_or("MCP 认证响应格式无效")?;
        if accepted {
            runtime.pending.remove(request_id);
        }
        Ok(accepted)
    }
    async fn poll_loop(self) {
        loop {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let mut runtime = self.inner.lock().await;
            if Arc::strong_count(&self.inner) == 1 {
                terminate(&mut runtime);
                runtime.polling = false;
                break;
            }
            if runtime.child.is_none() {
                runtime.polling = false;
                break;
            }
            match self.probe(&mut runtime).await {
                Ok(status) => {
                    let recovered = !runtime.status.running;
                    runtime.status = status.clone();
                    if recovered {
                        self.log("service_recovered", None);
                        self.emit_status(&status);
                    }
                }
                Err(error) => {
                    let exited = child_has_exited(&mut runtime);
                    if exited {
                        terminate(&mut runtime);
                        runtime.polling = false;
                    }
                    let changed =
                        runtime.status.running || runtime.status.error.as_deref() != Some(&error);
                    runtime.status = stopped(runtime.port, Some(error));
                    let status = runtime.status.clone();
                    drop(runtime);
                    if changed {
                        self.log("service_lost", None);
                        self.emit_status(&status);
                    }
                    if exited {
                        break;
                    }
                    continue;
                }
            }
            let path = format!("/control/events?since={}", runtime.cursor);
            let response = self
                .request(&runtime, reqwest::Method::GET, &path, None)
                .await;
            runtime.pending.retain(|_, event| event.expires_at > now());
            if let Ok(value) = response {
                if let Ok(events) = serde_json::from_value::<EventBatch>(value) {
                    if events.cursor >= runtime.cursor {
                        runtime.cursor = events.cursor;
                        for event in events.events.into_iter().take(128) {
                            if event.expires_at <= now()
                                || event.expires_at > now() + 600_000
                                || runtime.pending.contains_key(&event.request_id)
                                || runtime.pending.len() >= 32
                            {
                                continue;
                            }
                            if uuid::Uuid::parse_str(&event.request_id).is_err()
                                || uuid::Uuid::parse_str(&event.profile_id).is_err()
                            {
                                continue;
                            }
                            runtime
                                .pending
                                .insert(event.request_id.clone(), event.clone());
                            self.emit_auth(&event);
                        }
                    }
                }
            }
            self.collect_logs(&mut runtime).await;
        }
    }
}
#[derive(Deserialize)]
struct EventBatch {
    cursor: u64,
    events: Vec<AuthChallenge>,
}
fn child_has_exited(runtime: &mut Runtime) -> bool {
    runtime
        .child
        .as_mut()
        .is_none_or(|child| child.try_wait().ok().flatten().is_some())
}
fn boot_failure(code: Option<i32>, stderr: &str) -> String {
    if stderr.contains("ERR_MODULE_NOT_FOUND") {
        return "MCP 运行依赖缺失".into();
    }
    if stderr.contains("EADDRINUSE") {
        return "MCP 端口已被占用".into();
    }
    if code == Some(-1073741511) {
        return "MCP 运行程序缺少 Windows 入口点".into();
    }
    match code {
        Some(code) => format!("MCP 引擎启动失败（退出码 {code}）"),
        None => "MCP 引擎异常退出".into(),
    }
}
fn terminate(runtime: &mut Runtime) {
    if let Some(mut child) = runtime.child.take() {
        let _ = child.kill();
        let _ = child.wait();
    }
    runtime.pending.clear();
    runtime.cursor = 0;
}
fn stopped(port: u16, error: Option<String>) -> EngineStatus {
    EngineStatus {
        running: false,
        endpoint: format!("http://127.0.0.1:{port}/mcp"),
        pid: None,
        version: None,
        error,
    }
}
fn failed(stage: &str, detail: &str) -> ConnectionTest {
    ConnectionTest {
        status: "failed".into(),
        stage: stage.into(),
        detail: detail.into(),
        elapsed_ms: 0,
        stdout: None,
    }
}
#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
