use crate::{
    cloud_backup,
    commands::{self, AppCore},
    credentials::SecretStore,
    engine,
    models::{AppSettings, ConnectionProfile, ImportStrategy},
};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path as RoutePath, Request, State},
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tower_http::services::ServeDir;

fn random_token() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn private_write(path: &Path, bytes: &[u8], create_only: bool) -> Result<(), String> {
    let parent = path.parent().ok_or("路径无效")?;
    fs::create_dir_all(parent).map_err(|_| "无法创建数据目录")?;
    let temp = if create_only {
        path.to_path_buf()
    } else {
        parent.join(format!(".{}.tmp", random_token()))
    };
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|_| "无法写入数据")?;
    let result = file.write_all(bytes).and_then(|_| file.sync_all());
    if result.is_err() {
        let _ = fs::remove_file(&temp);
        return Err("文件已存在或无法写入".into());
    }
    if !create_only && fs::rename(&temp, path).is_err() {
        let _ = fs::remove_file(&temp);
        return Err("无法保存数据".into());
    }
    Ok(())
}

pub struct FileSecretStore {
    directory: PathBuf,
    key: [u8; 32],
}
impl FileSecretStore {
    pub fn open(directory: &Path) -> Result<Self, String> {
        let bytes =
            fs::read(directory.join("master.key")).map_err(|_| "缺少服务器主密钥，请先初始化")?;
        let key: [u8; 32] = bytes.try_into().map_err(|_| "主密钥格式无效")?;
        Ok(Self {
            directory: directory.join("secrets"),
            key,
        })
    }
    fn path(&self, id: &str) -> PathBuf {
        self.directory
            .join(format!("{:x}.enc", Sha256::digest(id.as_bytes())))
    }
}
impl SecretStore for FileSecretStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        let bytes = match fs::read(self.path(id)) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("无法读取服务器凭据".into()),
        };
        let (nonce, ciphertext) = bytes.split_at_checked(24).ok_or("服务器凭据损坏")?;
        let cipher = XChaCha20Poly1305::new_from_slice(&self.key).map_err(|_| "主密钥格式无效")?;
        let clear = cipher
            .decrypt(XNonce::from_slice(nonce), ciphertext)
            .map_err(|_| "无法解密服务器凭据")?;
        String::from_utf8(clear)
            .map(Some)
            .map_err(|_| "服务器凭据损坏".into())
    }
    fn set(&self, id: &str, value: &str) -> Result<(), String> {
        let mut nonce = [0u8; 24];
        rand::rng().fill_bytes(&mut nonce);
        let cipher = XChaCha20Poly1305::new_from_slice(&self.key).map_err(|_| "主密钥格式无效")?;
        let encrypted = cipher
            .encrypt(XNonce::from_slice(&nonce), value.as_bytes())
            .map_err(|_| "无法加密服务器凭据")?;
        private_write(
            &self.path(id),
            &[nonce.as_slice(), encrypted.as_slice()].concat(),
            false,
        )
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        match fs::remove_file(self.path(id)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法删除服务器凭据".into()),
        }
    }
}

pub fn initialize(directory: &Path, password: &str) -> Result<(), String> {
    if password.chars().count() < 12 {
        return Err("管理员密码至少 12 个字符".into());
    }
    if directory.join("admin.hash").exists() || directory.join("master.key").exists() {
        return Err("实例已初始化，拒绝覆盖管理员或主密钥".into());
    }
    fs::create_dir_all(directory).map_err(|_| "无法创建数据目录")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "无法保护数据目录")?;
    }
    let mut key = [0u8; 32];
    rand::rng().fill_bytes(&mut key);
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|_| "无法生成密码哈希")?
        .to_string();
    private_write(&directory.join("master.key"), &key, true)?;
    private_write(&directory.join("admin.hash"), hash.as_bytes(), true)
}

#[derive(Clone)]
struct Session {
    csrf: String,
    expires: Instant,
}
struct LoginState {
    sessions: HashMap<String, Session>,
    failures: u8,
    blocked_until: Option<Instant>,
}
#[derive(Clone)]
pub struct WebState {
    core: Arc<AppCore>,
    directory: PathBuf,
    assets: PathBuf,
    origin: String,
    secure: bool,
    auth: Arc<Mutex<LoginState>>,
    uploads: Arc<Mutex<HashMap<String, (String, PathBuf, Instant)>>>,
}
fn error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({"error": message}))).into_response()
}
async fn security_headers(request: Request, next: Next) -> Response {
    let api = request.uri().path().starts_with("/api/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    headers.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; connect-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"));
    if api {
        headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    }
    response
}
fn cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("astralink_session="))
}
impl WebState {
    fn session(&self, headers: &HeaderMap) -> Option<Session> {
        let mut auth = self.auth.lock().ok()?;
        auth.sessions
            .retain(|_, session| session.expires > Instant::now());
        auth.sessions.get(cookie(headers)?).cloned()
    }
    fn origin_ok(&self, headers: &HeaderMap) -> bool {
        headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(self.origin.as_str())
    }
    fn authorized(&self, headers: &HeaderMap) -> Result<(), Response> {
        if self.session(headers).is_some() {
            Ok(())
        } else {
            Err(error(StatusCode::UNAUTHORIZED, "请先登录"))
        }
    }
    fn mutation_allowed(&self, headers: &HeaderMap) -> Result<(), Response> {
        let session = self
            .session(headers)
            .ok_or_else(|| error(StatusCode::UNAUTHORIZED, "请先登录"))?;
        if !self.origin_ok(headers)
            || headers
                .get("x-astralink-csrf")
                .and_then(|v| v.to_str().ok())
                != Some(&session.csrf)
        {
            return Err(error(StatusCode::FORBIDDEN, "请求校验失败"));
        }
        Ok(())
    }
}

async fn auth_status(State(state): State<WebState>, headers: HeaderMap) -> Json<Value> {
    Json(match state.session(&headers) {
        Some(session) => json!({"authenticated": true, "csrf": session.csrf}),
        None => json!({"authenticated": false}),
    })
}
async fn login(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if !state.origin_ok(&headers) {
        return error(StatusCode::FORBIDDEN, "请求校验失败");
    }
    let Some(password) = body.get("password").and_then(Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "请输入密码");
    };
    let mut auth = match state.auth.lock() {
        Ok(auth) => auth,
        Err(_) => return error(StatusCode::INTERNAL_SERVER_ERROR, "登录服务不可用"),
    };
    if auth
        .blocked_until
        .is_some_and(|until| until > Instant::now())
    {
        return error(StatusCode::TOO_MANY_REQUESTS, "登录过于频繁，请稍后重试");
    }
    let hash = fs::read_to_string(state.directory.join("admin.hash")).unwrap_or_default();
    let valid = PasswordHash::new(&hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(password.as_bytes(), &hash)
            .is_ok()
    });
    if !valid {
        auth.failures = auth.failures.saturating_add(1);
        if auth.failures >= 5 {
            auth.failures = 0;
            auth.blocked_until = Some(Instant::now() + Duration::from_secs(900));
        }
        return error(StatusCode::UNAUTHORIZED, "密码不正确");
    }
    auth.failures = 0;
    auth.blocked_until = None;
    if auth.sessions.len() >= 32 {
        auth.sessions.clear();
    }
    let token = random_token();
    let csrf = random_token();
    auth.sessions.insert(
        token.clone(),
        Session {
            csrf: csrf.clone(),
            expires: Instant::now() + Duration::from_secs(12 * 3600),
        },
    );
    let mut response = Json(json!({"authenticated": true, "csrf": csrf})).into_response();
    let secure = if state.secure { "; Secure" } else { "" };
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "astralink_session={token}; HttpOnly; SameSite=Strict; Path=/; Max-Age=43200{secure}"
        ))
        .unwrap(),
    );
    response
}
async fn logout(State(state): State<WebState>, headers: HeaderMap) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    if let Some(token) = cookie(&headers) {
        if let Ok(mut auth) = state.auth.lock() {
            auth.sessions.remove(token);
        }
    }
    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "astralink_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0",
        ),
    );
    response
}
async fn change_password(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    let (Some(old), Some(new)) = (
        body.get("oldPassword").and_then(Value::as_str),
        body.get("newPassword").and_then(Value::as_str),
    ) else {
        return error(StatusCode::BAD_REQUEST, "请输入原密码和新密码");
    };
    if new.chars().count() < 12 {
        return error(StatusCode::BAD_REQUEST, "新密码至少 12 个字符");
    }
    let hash = fs::read_to_string(state.directory.join("admin.hash")).unwrap_or_default();
    if !PasswordHash::new(&hash).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(old.as_bytes(), &hash)
            .is_ok()
    }) {
        return error(StatusCode::UNAUTHORIZED, "原密码不正确");
    }
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    let Ok(hash) = Argon2::default().hash_password(new.as_bytes(), &salt) else {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "无法更新密码");
    };
    if private_write(
        &state.directory.join("admin.hash"),
        hash.to_string().as_bytes(),
        false,
    )
    .is_err()
    {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "无法更新密码");
    }
    if let Ok(mut auth) = state.auth.lock() {
        auth.sessions.clear();
    }
    Json(json!({"ok": true})).into_response()
}

async fn command(
    State(state): State<WebState>,
    RoutePath(name): RoutePath<String>,
    headers: HeaderMap,
    Json(args): Json<Value>,
) -> Response {
    let read_only = matches!(
        name.as_str(),
        "get_app_state"
            | "get_storage_info"
            | "get_runtime_logs"
            | "get_engine_status"
            | "discover_sources"
            | "get_auth_challenges"
    );
    let access = if read_only {
        state.authorized(&headers)
    } else {
        state.mutation_allowed(&headers)
    };
    if let Err(response) = access {
        return response;
    }
    let core = &state.core;
    let result: Result<Value, String> = async {
        match name.as_str() {
            "get_app_state" => {
                let (profiles, settings) = { let store = core.store()?; (store.profiles(), store.settings()) };
                Ok(json!({"profiles": profiles, "settings": settings, "engine": core.engine.status().await}))
            }
            "get_storage_info" => Ok(json!(core.store()?.storage_info())),
            "get_runtime_logs" => Ok(json!(core.logs.snapshot())),
            "get_engine_status" => Ok(json!(core.engine.status().await)),
            "get_auth_challenges" => Ok(json!(core.engine.pending_challenges().await)),
            "discover_sources" => Ok(json!([])),
            "reveal_profile_secret" => Ok(json!(core.store()?.read_secret(arg(&args, "profileId")?, arg(&args, "field")?)?)),
            "get_mcp_token" => { core.ensure_active()?; Ok(json!(core.store()?.mcp_token()?)) },
            "preview_import" => {
                let kind = arg(&args, "sourceKind")?;
                let upload_id = arg(&args, "filePath")?;
                let path = {
                    let mut uploads = state.uploads.lock().map_err(|_| "上传服务不可用")?;
                    uploads.retain(|_, (_, path, until)| {
                        if *until > Instant::now() { true } else { let _ = fs::remove_file(path); false }
                    });
                    let (owner, path, _) = uploads.get(upload_id).ok_or("上传文件已过期，请重新选择")?;
                    if owner != cookie(&headers).unwrap_or_default() { return Err("上传文件无效".into()); }
                    path.clone()
                };
                Ok(json!(core.preview_with_password(kind, &path, args.get("importPassword").and_then(Value::as_str))?))
            }
            "upsert_profile" | "apply_import" => {
                let _lock = core.operations.lock().await;
                core.ensure_active()?;
                let strategy = if name == "upsert_profile" { ImportStrategy::Merge } else {
                    serde_json::from_value(args.get("strategy").cloned().unwrap_or(Value::Null)).map_err(|_| "导入方式无效")?
                };
                let mut profiles: Vec<ConnectionProfile> = if name == "upsert_profile" {
                    vec![serde_json::from_value(args.get("profile").cloned().unwrap_or(Value::Null)).map_err(|_| "连接配置无效")?]
                } else {
                    serde_json::from_value(args.get("profiles").cloned().unwrap_or(Value::Null)).map_err(|_| "连接配置无效")?
                };
                if name == "apply_import" { for profile in &mut profiles { profile.totp_secret = None; profile.has_totp_secret = false; } }
                Ok(json!(core.commit_and_sync(|core| core.apply(profiles, strategy)).await?))
            }
            "delete_profile" | "delete_profiles" => {
                let _lock = core.operations.lock().await;
                core.ensure_active()?;
                let ids: Vec<String> = if name == "delete_profile" {
                    vec![arg(&args, "profileId")?.to_string()]
                } else { serde_json::from_value(args.get("profileIds").cloned().unwrap_or(Value::Null)).map_err(|_| "连接 ID 无效")? };
                Ok(json!(core.commit_and_sync(|core| core.store()?.delete_many(&ids)).await?))
            }
            "update_settings" => {
                let _lock = core.operations.lock().await;
                core.ensure_active()?;
                let settings: AppSettings = serde_json::from_value(args.get("settings").cloned().unwrap_or(Value::Null)).map_err(|_| "设置无效")?;
                let old = core.store()?.settings();
                if old.mcp_port != settings.mcp_port && core.engine.status().await.running { return Err("请先停止 MCP 服务再修改端口".into()); }
                let saved = core.store()?.update_settings(settings)?;
                core.engine.set_port(saved.mcp_port).await?;
                Ok(json!(saved))
            }
            "start_engine" => {
                let _lock = core.operations.lock().await;
                core.ensure_active()?;
                let (port, token, profiles) = {
                    let store = core.store()?;
                    (store.settings().mcp_port, store.mcp_token()?, store.runtime_profiles()?)
                };
                core.engine.configure(port, token).await?;
                Ok(json!(core.engine.start(profiles).await))
            }
            "stop_engine" => { let _lock = core.operations.lock().await; Ok(json!(core.engine.stop().await)) }
            "test_connection" => {
                let id = arg(&args, "profileId")?;
                if !core.store()?.profiles().iter().any(|profile| profile.id == id && profile.enabled) { return Err("连接不存在或未启用".into()); }
                Ok(json!(core.engine.test(id).await))
            }
            "submit_otp" => Ok(json!(core.engine.auth(arg(&args, "requestId")?, Some(arg(&args, "code")?.to_string()), false).await?)),
            "cancel_auth" => Ok(json!(core.engine.auth(arg(&args, "requestId")?, None, true).await?)),
            _ => Err("网页不支持此操作".into()),
        }
    }.await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}
fn arg<'a>(value: &'a Value, key: &str) -> Result<&'a str, String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| "请求参数无效".into())
}

async fn upload(
    State(state): State<WebState>,
    RoutePath((kind, ext)): RoutePath<(String, String)>,
    headers: HeaderMap,
    bytes: Bytes,
) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return error(StatusCode::BAD_REQUEST, "上传文件大小无效");
    }
    if !ext.is_ascii() || ext.len() > 16 || !ext.chars().all(|ch| ch.is_ascii_alphanumeric()) {
        return error(StatusCode::BAD_REQUEST, "文件扩展名无效");
    }
    let key = kind == "key";
    if key && bytes.len() > 1024 * 1024 {
        return error(StatusCode::BAD_REQUEST, "私钥文件超过大小限制");
    }
    if !key
        && !matches!(
            kind.as_str(),
            "xshell"
                | "finalshell"
                | "nyaterm"
                | "mobaxterm"
                | "securecrt"
                | "electerm"
                | "windterm"
                | "termius"
                | "nya-backup"
                | "tabby"
                | "netcatty"
                | "mcp-json"
                | "openssh"
        )
    {
        return error(StatusCode::BAD_REQUEST, "导入来源不受支持");
    }
    let id = random_token();
    let folder = state.directory.join(if key { "keys" } else { "uploads" });
    let path = folder.join(format!("{id}.{ext}"));
    if private_write(&path, &bytes, true).is_err() {
        return error(StatusCode::INTERNAL_SERVER_ERROR, "无法保存上传文件");
    }
    if key {
        return Json(json!({"path": path.to_string_lossy()})).into_response();
    }
    let owner = cookie(&headers).unwrap_or_default().to_string();
    if let Ok(mut uploads) = state.uploads.lock() {
        uploads.retain(|_, (_, path, until)| {
            if *until > Instant::now() {
                true
            } else {
                let _ = fs::remove_file(path);
                false
            }
        });
        if uploads.len() >= 32 {
            let _ = fs::remove_file(&path);
            return error(
                StatusCode::TOO_MANY_REQUESTS,
                "临时导入文件已达上限，请稍后重试",
            );
        }
        uploads.insert(
            id.clone(),
            (owner, path, Instant::now() + Duration::from_secs(600)),
        );
    } else {
        let _ = fs::remove_file(path);
        return error(StatusCode::INTERNAL_SERVER_ERROR, "上传服务不可用");
    }
    Json(json!({"id": id})).into_response()
}

async fn backup_export(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    let Some(passphrase) = body.get("passphrase").and_then(Value::as_str) else {
        return error(StatusCode::BAD_REQUEST, "请输入备份口令");
    };
    match state
        .core
        .backup_snapshot()
        .and_then(|snapshot| cloud_backup::encrypt(&snapshot, passphrase))
    {
        Ok(bytes) => (
            [
                (header::CONTENT_TYPE, "application/octet-stream"),
                (
                    header::CONTENT_DISPOSITION,
                    "attachment; filename=astralink.backup",
                ),
                (header::CACHE_CONTROL, "no-store"),
            ],
            bytes,
        )
            .into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}
fn decode_backup(body: &Value) -> Result<(Vec<u8>, &str), String> {
    let encoded = arg(body, "data")?;
    if encoded.len() > 44 * 1024 * 1024 {
        return Err("备份文件超过大小限制".into());
    }
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| "备份格式无效")?;
    Ok((bytes, arg(body, "passphrase")?))
}
async fn backup_preview(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    let result = decode_backup(&body)
        .and_then(|(bytes, passphrase)| commands::backup_preview(&bytes, passphrase));
    match result {
        Ok(preview) => Json(json!(preview)).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}
async fn backup_import(
    State(state): State<WebState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Response {
    if let Err(response) = state.mutation_allowed(&headers) {
        return response;
    }
    let result: Result<Value, String> = async {
        let (bytes, passphrase) = decode_backup(&body)?;
        if cloud_backup::fingerprint(&bytes) != arg(&body, "expectedFingerprint")? {
            return Err("备份文件已改变，请重新预览".into());
        }
        let strategy: ImportStrategy =
            serde_json::from_value(body.get("strategy").cloned().unwrap_or(Value::Null))
                .map_err(|_| "导入方式无效")?;
        let snapshot = cloud_backup::decrypt(&bytes, passphrase)?;
        let _lock = state.core.operations.lock().await;
        state.core.ensure_active()?;
        Ok(json!(
            state.core.restore_snapshot(snapshot, strategy).await?
        ))
    }
    .await;
    match result {
        Ok(value) => Json(value).into_response(),
        Err(message) => error(StatusCode::BAD_REQUEST, &message),
    }
}

async fn index(State(state): State<WebState>) -> Response {
    let path = state.assets.join("index.html");
    match fs::read_to_string(path) {
        Ok(html) => axum::response::Html(html.replace(
            "</head>",
            "<meta name=\"astralink-web\" content=\"1\"></head>",
        ))
        .into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE, "网页资源缺失"),
    }
}

pub async fn run(
    directory: PathBuf,
    web_assets: PathBuf,
    address: &str,
    origin: String,
    paths: engine::RuntimePaths,
) -> Result<(), String> {
    let parsed = reqwest::Url::parse(&origin).map_err(|_| "公开访问地址无效")?;
    if parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || parsed.username() != ""
        || parsed.password().is_some()
    {
        return Err("公开访问地址必须是根路径".into());
    }
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .map_err(|_| "无法监听管理端口")?;
    let loopback = listener
        .local_addr()
        .map_err(|_| "无法读取监听地址")?
        .ip()
        .is_loopback();
    let local_http = parsed.scheme() == "http"
        && parsed
            .host_str()
            .is_some_and(|host| host == "127.0.0.1" || host == "localhost")
        && (loopback || std::env::var("ASTRALINK_LOCAL_HTTP").as_deref() == Ok("1"));
    if parsed.scheme() != "https" && !local_http {
        return Err("远程访问必须使用 HTTPS 公开地址".into());
    }
    let home = directory.clone();
    let core = Arc::new(AppCore::open(
        directory.clone(),
        Box::new(FileSecretStore::open(&directory)?),
        paths,
        home.clone(),
        None,
        None,
        home.join("config.toml"),
    )?);
    let source = web_assets.canonicalize().map_err(|_| "网页资源缺失")?;
    let upload_dir = directory.join("uploads");
    if upload_dir.exists() {
        if fs::symlink_metadata(&upload_dir)
            .map_err(|_| "上传目录无效")?
            .file_type()
            .is_symlink()
        {
            return Err("上传目录不能是符号链接".into());
        }
        for entry in fs::read_dir(&upload_dir).map_err(|_| "无法读取上传目录")? {
            let entry = entry.map_err(|_| "无法读取上传目录")?;
            if entry.file_type().map_err(|_| "无法检查上传文件")?.is_file() {
                fs::remove_file(entry.path()).map_err(|_| "无法清理过期上传文件")?;
            }
        }
    }
    let state = WebState {
        core,
        directory,
        assets: source.clone(),
        origin,
        secure: parsed.scheme() == "https",
        auth: Arc::new(Mutex::new(LoginState {
            sessions: HashMap::new(),
            failures: 0,
            blocked_until: None,
        })),
        uploads: Arc::new(Mutex::new(HashMap::new())),
    };
    if state.core.store()?.settings().start_on_launch {
        let (port, token, profiles) = {
            let store = state.core.store()?;
            (
                store.settings().mcp_port,
                store.mcp_token()?,
                store.runtime_profiles()?,
            )
        };
        state.core.engine.configure(port, token).await?;
        let status = state.core.engine.start(profiles).await;
        if !status.running {
            eprintln!("AstraLink MCP 启动失败，请在管理页查看状态");
        }
    }
    let uploads = state.uploads.clone();
    let sweep = tokio::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            if let Ok(mut uploads) = uploads.lock() {
                uploads.retain(|_, (_, path, until)| {
                    if *until > Instant::now() {
                        true
                    } else {
                        let _ = fs::remove_file(path);
                        false
                    }
                });
            }
        }
    });
    // Static assets are public; every API call is independently authenticated.
    let app = Router::new()
        .route("/", get(index))
        .nest_service("/assets", ServeDir::new(source.join("assets")))
        .route("/api/auth/status", get(auth_status))
        .route("/api/auth/login", post(login))
        .route("/api/auth/logout", post(logout))
        .route("/api/auth/password", post(change_password))
        .route("/api/commands/{name}", post(command))
        .route(
            "/api/upload/{kind}/{ext}",
            post(upload).layer(DefaultBodyLimit::max(16 * 1024 * 1024)),
        )
        .route("/api/backup/export", post(backup_export))
        .route(
            "/api/backup/preview",
            post(backup_preview).layer(DefaultBodyLimit::max(44 * 1024 * 1024)),
        )
        .route(
            "/api/backup/import",
            post(backup_import).layer(DefaultBodyLimit::max(44 * 1024 * 1024)),
        )
        .layer(middleware::from_fn(security_headers))
        .layer(DefaultBodyLimit::max(4 * 1024 * 1024))
        .with_state(state.clone());
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await;
    sweep.abort();
    state.core.shutdown().await;
    result.map_err(|_| "Web 服务异常停止".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initialized_server_encrypts_secrets_and_rejects_reinitialization() {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path(), "only-for-test-1234").unwrap();
        assert!(initialize(directory.path(), "only-for-test-1234").is_err());
        let hash = fs::read_to_string(directory.path().join("admin.hash")).unwrap();
        assert!(!hash.contains("only-for-test-1234"));
        let store = FileSecretStore::open(directory.path()).unwrap();
        store
            .set("profile/test/password", "synthetic-secret")
            .unwrap();
        assert_eq!(
            store.get("profile/test/password").unwrap().as_deref(),
            Some("synthetic-secret")
        );
        let path = store.path("profile/test/password");
        assert!(!fs::read(&path)
            .unwrap()
            .windows(16)
            .any(|part| part == b"synthetic-secret"));
        store.delete("profile/test/password").unwrap();
        assert_eq!(store.get("profile/test/password").unwrap(), None);
    }

    #[tokio::test]
    async fn login_requires_origin_and_mutations_require_csrf_and_password_change_revokes_sessions()
    {
        let directory = tempfile::tempdir().unwrap();
        initialize(directory.path(), "first-password-1234").unwrap();
        let core = Arc::new(
            AppCore::open(
                directory.path().to_path_buf(),
                Box::new(FileSecretStore::open(directory.path()).unwrap()),
                engine::candidate_paths(Some(directory.path()), directory.path(), None),
                directory.path().to_path_buf(),
                None,
                None,
                directory.path().join("config.toml"),
            )
            .unwrap(),
        );
        let state = WebState {
            core,
            directory: directory.path().to_path_buf(),
            assets: directory.path().to_path_buf(),
            origin: "http://127.0.0.1:1421".into(),
            secure: false,
            auth: Arc::new(Mutex::new(LoginState {
                sessions: HashMap::new(),
                failures: 0,
                blocked_until: None,
            })),
            uploads: Arc::new(Mutex::new(HashMap::new())),
        };
        let mut headers = HeaderMap::new();
        let rejected = login(
            State(state.clone()),
            headers.clone(),
            Json(json!({"password":"first-password-1234"})),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:1421"),
        );
        let response = login(
            State(state.clone()),
            headers.clone(),
            Json(json!({"password":"first-password-1234"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = response.headers()[header::SET_COOKIE].to_str().unwrap();
        assert!(cookie.contains("HttpOnly; SameSite=Strict"));
        headers.insert(
            header::COOKIE,
            HeaderValue::from_str(cookie.split(';').next().unwrap()).unwrap(),
        );
        assert_eq!(
            state.mutation_allowed(&headers).unwrap_err().status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            command(
                State(state.clone()),
                RoutePath("get_mcp_token".into()),
                headers.clone(),
                Json(json!({})),
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let csrf = state.session(&headers).unwrap().csrf;
        headers.insert("x-astralink-csrf", HeaderValue::from_str(&csrf).unwrap());
        assert!(state.mutation_allowed(&headers).is_ok());
        assert_eq!(
            command(
                State(state.clone()),
                RoutePath("get_mcp_token".into()),
                headers.clone(),
                Json(json!({})),
            )
            .await
            .status(),
            StatusCode::OK
        );
        let changed = change_password(
            State(state.clone()),
            headers.clone(),
            Json(json!({"oldPassword":"first-password-1234","newPassword":"second-password-1234"})),
        )
        .await;
        assert_eq!(changed.status(), StatusCode::OK);
        assert!(state.session(&headers).is_none());
        assert_eq!(
            login(
                State(state.clone()),
                headers.clone(),
                Json(json!({"password":"first-password-1234"}))
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            login(
                State(state),
                headers,
                Json(json!({"password":"second-password-1234"}))
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
}
