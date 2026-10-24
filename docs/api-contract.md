# AstraLink API contract

JSON uses camelCase. IDs are UUIDs. All user-facing labels are concise Chinese. Runtime secrets do not appear in diagnostic output or persisted metadata; optional password/passphrase inputs are extracted into OS credentials before saving.

## Shared types

```ts
interface ConnectionProfile {
 id:string; name:string; host:string; port:number; username:string;
 group:string; source:string; authType:'password'|'privateKey'|'agent';
 privateKeyPath?:string; password?:string; passphrase?:string; totpSecret?:string;
 hasPassword:boolean; hasPassphrase:boolean; hasTotpSecret:boolean; agent?:string;
 transportMode:'exec'|'shell'; tryKeyboard:boolean; enabled:boolean; notes:string;
 jumpHostId?:string; targetHost?:string; targetUsername?:string;
 allowedLocalPaths?:string[]; allowedRemotePaths?:string[];
}
interface AppSettings { mcpPort:number; startOnLaunch:boolean; minimizeToTray:boolean }
interface EngineStatus { running:boolean; endpoint:string; pid?:number; version?:string; error?:string }
interface AppState { profiles:ConnectionProfile[]; settings:AppSettings; engine:EngineStatus; mcpClientHost?:string }
interface ImportIssue { severity:'warning'|'error'; message:string; profileName?:string }
interface ImportPreview { source:string; sourcePath:string; profiles:ConnectionProfile[]; issues:ImportIssue[]; stats:{total:number;ready:number;needsCredential:number;unsupported:number} }
interface SourceCandidate { kind:string; label:string; path:string; exists:boolean; profileCount?:number }
interface AuthChallenge { requestId:string; profileId:string; connectionName:string; host:string; username:string; instructions:string; prompts:{prompt:string;echo:boolean}[]; expiresAt:number }
interface ConnectionTest { status:'connected'|'partial'|'auth_required'|'failed'; stage:string; detail:string; elapsedMs:number; stdout?:string }
```

## Tauri commands

桌面与服务器均支持 AstraLink 自身加密快照。`export_backup_file`、`preview_backup_file`、`restore_backup_file` 在桌面使用本地绝对路径；服务器版通过登录保护的下载/上传 API 传输文件，不接受浏览器提供的任意服务器路径。快照 v2 包含 profiles 和 settings；v1 仍可恢复连接。导入时缺失的私钥或本地传输目录会使对应连接停用。管理员登录信息、MCP 令牌和私钥文件不随备份迁移。

服务器入口：`GET /api/auth/status` 查询当前会话；`POST /api/auth/login|logout|password` 管理单管理员会话。其余 `POST /api/commands/{name}` 复用配置、测试和引擎核心；登录要求 Origin 与实际请求 Host 同源，写请求另外校验 HttpOnly 会话 Cookie 与 `X-AstraLink-CSRF`。HTTPS Origin 的登录 Cookie 带 Secure。`POST /api/upload/{kind}/{ext}` 上传浏览器所选的单个导入文件（十分钟预览期）或私钥，`POST /api/backup/export|preview|import` 处理加密备份。客户端注册与 MCP 访问令牌接口不暴露给浏览器。

服务器 `get_app_state` 的 `mcpClientHost` 来自 `ASTRALINK_MCP_CLIENT_HOST`（合法 IP；未配置为 `127.0.0.1`），只用于网页客户端配置片段的地址展示；SSH 隧道模式继续使用本机回环地址。它不改变 MCP 引擎的回环监听或 Docker 端口映射。

- get_app_state {} -> AppState
- get_storage_info {} -> {backend:'sqlite',databasePath:string,legacyBackupPath:string|null,serverCount:number}; shows actual per-platform data location in Settings.
- get_runtime_logs {} -> {entries:RuntimeLogEntry[],logPath:string,error:string|null}; at most 1000 entries in chronological order. Each entry has id,timestamp,level,source,code,message and optional profileId/requestId UUIDs. Fixed messages only; no credentials, raw prompts, remote command text/output or arbitrary errors.
- export_runtime_logs {filePath:string} -> void; native save dialog chooses a new absolute .log/.txt filename outside the log directory. Refuses existing files; exports validated recent records with UTC timestamps and UUID correlation.
- discover_sources {} -> SourceCandidate[]
- preview_import {sourceKind,filePath} -> ImportPreview
- sourceKind 包含 xshell、mobaxterm（`.mxtsessions`）、windterm（`user.sessions`）、securecrt（XML）、finalshell（`conn` 目录或单个 JSON）、termius（本机 IndexedDB 目录）、electerm（书签 JSON）、nyaterm（本机 `.redb` 或 v1 JSON）、openssh、mcp-json、tabby、netcatty、nya-backup（`.nya`）。仅转换 SSH 会话。`.redb` 只读连接与分组，必要时只在临时副本恢复；Termius 使用当前用户系统凭据库 localKey 解密本机 IndexedDB 副本；不写回来源。
- preview_import {sourceKind,filePath,importPassword?} -> ImportPreview；WindTerm 主密码和 `.nya` 备份密码仅用于当次预览，敏感字段不会回传 UI。
- preview_webdav_backup {url,username,password,passphrase} -> {fingerprint,serverCount}
- backup_to_webdav {url,username,password,passphrase,expectedFingerprint?} -> {fingerprint,serverCount}; 首次只创建，覆盖需远端原始指纹与 ETag 匹配。
- restore_webdav_backup {url,username,password,passphrase,expectedFingerprint} -> ConnectionProfile[]; 重新下载并核对指纹，替换本机连接。只接受 HTTPS WebDAV 文件地址；口令不保存。
- apply_import {profiles,strategy:'merge'|'replace'} -> ConnectionProfile[]
- upsert_profile {profile} / delete_profile {profileId} -> ConnectionProfile[]
- reveal_profile_secret {profileId,field:'password'|'passphrase'|'totpSecret'} -> string|null. Available only through an explicit click in the desktop editor; never included in AppState, list views or diagnostic output. The display value is cleared when hidden or the editor closes, and is not merged into the editable profile.
- delete_profiles {profileIds: string[]} -> ConnectionProfile[]；原子批量删除，保留连接依赖被删跳板时整批拒绝，同批目标和跳板可一起删除。
- update_settings {settings} -> AppSettings
- start_engine {} / stop_engine {} / get_engine_status {} -> EngineStatus
- test_connection {profileId} -> ConnectionTest
- submit_otp {requestId,code} / cancel_auth {requestId} -> boolean
- register_codex {} -> {configPath:string;backupPath?:string;reloadRequired:boolean}
- get_mcp_token {} -> string. Explicit user-initiated clipboard copy only; never render or log the returned token.

Native file selection uses @tauri-apps/plugin-dialog open(). Events: astralink-auth -> AuthChallenge; astralink-engine -> EngineStatus. Browser preview must indicate desktop startup is required; never fake operational success.

`test_connection` blocks until the attempt succeeds or ends; `auth_required` is a terminal incomplete authentication result, not a request still awaiting an answer. Clear that test's challenges on every result. Render completed `auth_required` as 认证未完成 and `partial` as 堡垒机已登录 in both the list and detail panel; only active tests show waiting/progress indicators. Authentication cancellation, expiry, and server rejection have distinct safe details.

## Node engine protocol

Start engine/src/main.mjs. Env: ASTRALINK_PORT (18766 default), ASTRALINK_CONTROL_TOKEN, ASTRALINK_MCP_TOKEN. Both tokens mandatory, never logged. Bind 127.0.0.1. Reject non-loopback Origins and use constant-time token validation.

- /mcp: Streamable HTTP, bearer MCP token; retain all 10 upstream tools and their argument names, including execute-command/connectionName.
- GET /control/status: control bearer -> {running:true,version:'0.1.0',pid,endpoint}
- POST /control/profiles: {profiles:RuntimeProfile[]} -> {count}; validate atomically and update incrementally, preserving unrelated connections.
- POST /control/test: {profileId} -> ConnectionTest; readonly hostname/id command only. A successful SSH login followed by a failed command probe remains failed at stage command, with an explicit authentication-success detail. A confirmed JumpServer banner and Opt> asset menu reports partial at stage target and asks for targetHost/targetUsername in advanced settings; reaching the menu proves SSH authentication but not asset command access.
- GET /control/events?since=N -> {cursor:number,events:AuthChallenge[]}; bounded history.
- GET /control/logs?since=N -> {cursor:number,dropped:boolean,entries:{seq,timestamp,code,profileId?,requestId?}[]}; authenticated control token only, maximum 512 retained events. Rust validates event codes and UUIDs and supplies human-readable Chinese messages before writing JSONL. Reset the log cursor when the child starts; dropped history produces an explicit warning.
- POST /control/auth: {requestId,code?:string,cancel?:boolean} -> {accepted:boolean}
- POST /control/shutdown -> acknowledgement and clean shutdown.

RuntimeProfile is ConnectionProfile with optional resolved password/passphrase/totpSecret. Rust owns credential resolution and events. TOTP secrets are Base32 values stored in the OS credential vault and omitted from metadata, imports, and events. The editor can decode an image file locally and extract a standard TOTP otpauth secret; images and decoded payloads are never uploaded or logged. The engine generates six-digit RFC 6238 SHA-1 codes for explicit OTP/MFA/2FA prompts regardless of their echo flag; less explicit token prompts must be non-echoing. When fewer than five seconds remain in the step or that profile's step was already submitted, the existing manual challenge is used. Reject duplicate profile IDs/names, bad ports and missing key files without replacing a good config. For JumpServer targetHost/targetUsername, map its direct-login username format; unresolved jumpHostId relations must be explicit errors, not ignored.

The editor's QR/secret entry appears next to the interactive-authentication toggle. Scanning fills an unsaved field; `upsert_profile` must preserve and vault-store `totpSecret`, while `apply_import` must discard imported TOTP material. On reopen only `hasTotpSecret` is returned, never the seed itself.

User data lives in OS app-data, outside Git. Rust manager resolves secrets, controls child process, polls auth events, and emits Tauri events. SourceKind values: xshell, tabby, netcatty, mcp-json, openssh. Xshell encrypted password strings are never treated as plain passwords; Netcatty CSV is the primary import entry; Tabby metadata/vault limitations are reported accurately.

Server metadata and AppSettings now persist transactionally in app-data/astralink.db (SQLite schema version 1). `servers` has one row per connection, indexed main fields and metadata_json for optional configuration; `app_settings` stores application preferences. First startup migrates validated state.json and preserves the original file as a backup. An initialized database is authoritative; corrupt or unsupported databases produce an error instead of re-importing stale JSON. Credentials remain exclusively in the OS vault. Credential journals use the committed SQLite snapshot as their recovery witness, preserving crash rollback behavior.

The server table paginates filtered metadata at 20/50/100 rows per page, default 20. All search/group/source/auth filters run before pagination. Selection/all-select/deletion are restricted to the visible page; switching pages or filters removes hidden selection. Filter/page-size changes reset page one, deletion clamps the last page, and selecting a newly saved item moves to its page. The complete metadata set remains available for the editor, jump dependency validation and MCP synchronization.

Runtime logs live in app-data/logs/runtime.jsonl (1 MiB current file plus runtime.1.jsonl and runtime.2.jsonl). Last 1000 entries reload after restart. Disk failures leave memory logging available and show a warning. The sidebar log page refreshes every 1.5 seconds while open, supports pause/level/profile/keyword filters, copies filtered records, and exports the retained snapshot. An old log file's message/source fields are reconstructed from the allowlist on reload/export, not trusted.

## 客户端快捷接入

- register_mcp_client {clientId: string, filePath?: string} -> {configPath, backupPath?, reloadRequired}
- get_mcp_bridge_paths {} -> {nodePath:string,scriptPath:string}; returns installed local paths only, never credentials. Desktop snippets use them with a token placeholder.
- clientId: codex、claude-code、claude-desktop、workbuddy、cursor、vscode、windsurf、antigravity、opencode、openclaw、hermes。Claude Desktop / WorkBuddy 通过打包的 Node 和 stdio 桥接连接本机 HTTP 服务；Cherry Studio 可在自身界面导入桥接片段（桌面版填写绝对路径，令牌手动复制）；云端 ChatGPT 需 Secure MCP Tunnel 或独立 HTTPS 网关，暂不支持本机快捷接入。
- 未指定路径时写入对应用户级配置；Codex 继续遵循已有 CODEX_HOME 配置位置。Windsurf 要求用户先选择当前实际使用的配置文件。
- 只更新 AstraLink 节点，其余设置保留。写入前创建旁路备份，再校验文件未发生变化并原子替换；错误不包含配置内容或令牌。
- JSON/YAML 配置按结构重新序列化；格式可能调整，YAML 注释不保留。含注释的 JSON、JSON5 和非对象节点拒绝自动写入，保留原文件，使用手动合并。
- 自动填入本机 MCP 访问令牌；界面只回传配置和备份路径，不回传令牌。各客户端仍需重新加载 MCP。

## AstraLink naming and migration

All active identifiers use astralink: dev.astralink.desktop, astralink.db, astralink-auth / astralink-engine events, ASTRALINK_* environment variables and the astralink MCP client entry. Previous identifier literals exist only in migration.rs for one-way migration.

On first launch without a new database, the desktop copies the previous app directory into a validated staging directory using SQLite online backup (including committed WAL pages). It retains the previous metadata and transaction journal, refuses to overwrite existing destination data, and preserves originals on migration failure. Explicit development directories can migrate their old database filename in place. New data is authoritative, including when invalid. OS credentials migrate on demand; explicit deletion removes the previous copy before the new one so a deleted credential cannot return. Client registration backs up and replaces only an owned previous localhost HTTP MCP entry, retaining unrelated endpoints and client settings.
