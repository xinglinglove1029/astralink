use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebUser {
    pub id: String,
    pub username: String,
    pub role: Option<String>,
    pub enabled: bool,
    pub revision: i64,
    pub has_token: bool,
}

pub struct WebAccess {
    db: Mutex<Connection>,
    identities: PathBuf,
    audit_path: PathBuf,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn password_hash(password: &str) -> Result<String, String> {
    let salt = SaltString::generate(&mut argon2::password_hash::rand_core::OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| "无法生成密码哈希".into())
}

fn row_user(row: &rusqlite::Row<'_>) -> rusqlite::Result<WebUser> {
    Ok(WebUser {
        id: row.get(0)?,
        username: row.get(1)?,
        role: row.get(2)?,
        enabled: row.get::<_, i64>(3)? != 0,
        revision: row.get(4)?,
        has_token: row.get::<_, i64>(5)? != 0,
    })
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditQuery {
    page: Option<usize>,
    page_size: Option<usize>,
    actor: Option<String>,
    channel: Option<String>,
    action: Option<String>,
    outcome: Option<String>,
    profile_id: Option<String>,
    from: Option<i64>,
    to: Option<i64>,
}

impl AuditQuery {
    fn validate(&self) -> Result<(usize, usize), String> {
        let page = self.page.unwrap_or(1);
        let size = self.page_size.unwrap_or(20);
        if page == 0
            || page > 1_000_000
            || ![20, 50, 100].contains(&size)
            || self.from.zip(self.to).is_some_and(|(from, to)| from > to)
            || [
                self.actor.as_deref(),
                self.action.as_deref(),
                self.profile_id.as_deref(),
            ]
            .iter()
            .flatten()
            .any(|value| value.len() > 128)
            || self
                .channel
                .as_deref()
                .is_some_and(|value| !["web", "mcp"].contains(&value))
            || self
                .outcome
                .as_deref()
                .is_some_and(|value| !["success", "failure", "denied"].contains(&value))
        {
            return Err("审计筛选参数无效".into());
        }
        Ok((page, size))
    }

    fn matches(&self, event: &Value) -> bool {
        let text = |key| event.get(key).and_then(Value::as_str).unwrap_or_default();
        let time = event
            .get("timestamp")
            .and_then(Value::as_i64)
            .unwrap_or_default();
        self.actor
            .as_deref()
            .is_none_or(|value| text("actor").to_lowercase().contains(&value.to_lowercase()))
            && self
                .channel
                .as_deref()
                .is_none_or(|value| text("channel") == value)
            && self.action.as_deref().is_none_or(|value| {
                text("action")
                    .to_lowercase()
                    .contains(&value.to_lowercase())
            })
            && self
                .outcome
                .as_deref()
                .is_none_or(|value| text("outcome") == value)
            && self
                .profile_id
                .as_deref()
                .is_none_or(|value| text("profileId") == value)
            && self.from.is_none_or(|value| time >= value)
            && self.to.is_none_or(|value| time <= value)
    }
}

impl WebAccess {
    pub fn open(directory: &Path) -> Result<Self, String> {
        if !directory.join("admin.hash").is_file() {
            return Err("管理员未初始化".into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .map_err(|_| "无法保护服务器数据目录")?;
        }
        let db_path = directory.join("web-access.db");
        let mut db = Connection::open_with_flags(
            &db_path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE,
        )
        .map_err(|_| "无法打开用户数据库")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&db_path, fs::Permissions::from_mode(0o600))
                .map_err(|_| "无法保护用户数据库")?;
        }
        db.busy_timeout(Duration::from_secs(2))
            .map_err(|_| "无法设置用户数据库等待时间")?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;
            -- 网页用户；管理员的密码仍保存在原 admin.hash，避免升级重置。
            CREATE TABLE IF NOT EXISTS web_users (
                id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT,
                role TEXT CHECK(role IN ('admin','operator','viewer')),
                enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), revision INTEGER NOT NULL DEFAULT 0,
                token_hash TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
            -- 只存非敏感的操作元数据；不保存命令、参数、响应或凭据。
            CREATE TABLE IF NOT EXISTS web_audit (
                id INTEGER PRIMARY KEY AUTOINCREMENT, timestamp INTEGER NOT NULL,
                actor_id TEXT NOT NULL, actor_name TEXT NOT NULL, channel TEXT NOT NULL,
                action TEXT NOT NULL, outcome TEXT NOT NULL, profile_id TEXT);
            CREATE INDEX IF NOT EXISTS web_audit_timestamp ON web_audit(timestamp DESC);")
            .map_err(|_| "无法初始化用户数据库")?;
        let role_required: i64 = db
            .query_row(
                "SELECT \"notnull\" FROM pragma_table_info('web_users') WHERE name='role'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "无法检查用户数据库结构")?;
        if role_required != 0 {
            let tx = db.transaction().map_err(|_| "无法迁移用户数据库")?;
            tx.execute_batch("CREATE TABLE web_users_next (
                id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT,
                role TEXT CHECK(role IN ('admin','operator','viewer')),
                enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), revision INTEGER NOT NULL DEFAULT 0,
                token_hash TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);
                INSERT INTO web_users_next(id,username,password_hash,role,enabled,revision,token_hash,created_at,updated_at)
                    SELECT id,username,password_hash,role,enabled,revision,token_hash,created_at,updated_at FROM web_users;
                DROP TABLE web_users;
                ALTER TABLE web_users_next RENAME TO web_users;")
                .map_err(|_| "无法迁移用户数据库")?;
            tx.commit().map_err(|_| "无法迁移用户数据库")?;
        }
        db.execute("INSERT OR IGNORE INTO web_users(id,username,password_hash,role,enabled,revision,created_at,updated_at)
            VALUES('admin','admin',NULL,'admin',1,0,?1,?1)", [now()])
            .map_err(|_| "无法初始化管理员用户")?;
        let access = Self {
            db: Mutex::new(db),
            identities: directory.join("mcp-identities.json"),
            audit_path: directory.join("mcp-audit.jsonl"),
        };
        access.sync_tokens()?;
        Ok(access)
    }

    pub fn user(&self, id: &str) -> Result<Option<WebUser>, String> {
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.query_row("SELECT id,username,role,enabled,revision,token_hash IS NOT NULL FROM web_users WHERE id=?1", [id], row_user)
            .optional().map_err(|_| "无法读取用户".into())
    }

    pub fn authenticate(
        &self,
        username: &str,
        password: &str,
        directory: &Path,
    ) -> Result<Option<WebUser>, String> {
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let record = db.query_row("SELECT id,username,role,enabled,revision,token_hash IS NOT NULL,password_hash FROM web_users WHERE username=?1", [username], |row| {
            Ok((row_user(row)?, row.get::<_, Option<String>>(6)?))
        }).optional().map_err(|_| "无法读取用户")?;
        let Some((user, stored)) = record else {
            return Ok(None);
        };
        if !user.enabled || user.role.is_none() {
            return Ok(None);
        }
        let hash = if user.id == "admin" {
            fs::read_to_string(directory.join("admin.hash")).map_err(|_| "无法读取管理员密码")?
        } else {
            stored.unwrap_or_default()
        };
        Ok(PasswordHash::new(&hash)
            .is_ok_and(|hash| {
                Argon2::default()
                    .verify_password(password.as_bytes(), &hash)
                    .is_ok()
            })
            .then_some(user))
    }

    pub fn list(&self) -> Result<Vec<WebUser>, String> {
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let mut query = db.prepare("SELECT id,username,role,enabled,revision,token_hash IS NOT NULL FROM web_users ORDER BY username")
            .map_err(|_| "无法读取用户")?;
        let users = query
            .query_map([], row_user)
            .map_err(|_| "无法读取用户")?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|_| "无法读取用户".to_string())?;
        Ok(users)
    }

    pub fn save(
        &self,
        id: Option<&str>,
        username: &str,
        password: Option<&str>,
        role: &str,
        enabled: bool,
    ) -> Result<WebUser, String> {
        if username.len() < 2
            || username.len() > 32
            || !username
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
            || !matches!(role, "admin" | "operator" | "viewer")
        {
            return Err("用户名或角色无效".into());
        }
        let hash = match password {
            Some(p) if p.chars().count() >= 12 => Some(password_hash(p)?),
            Some(_) => return Err("密码至少 12 个字符".into()),
            None => None,
        };
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        if let Some(id) = id {
            if id == "admin" {
                return Err("初始管理员只能通过修改密码入口管理".into());
            }
            let exists = db
                .query_row("SELECT 1 FROM web_users WHERE id=?1", [id], |row| {
                    row.get::<_, i64>(0)
                })
                .optional()
                .map_err(|_| "无法读取用户")?;
            if exists.is_none() {
                return Err("用户不存在".into());
            }
        } else {
            if hash.is_none() {
                return Err("新用户必须设置密码".into());
            }
            let count: i64 = db
                .query_row("SELECT COUNT(*) FROM web_users", [], |row| row.get(0))
                .map_err(|_| "无法读取用户")?;
            if count >= 64 {
                return Err("用户数量已达上限".into());
            }
        }
        let owner = db
            .query_row(
                "SELECT id FROM web_users WHERE username=?1",
                [username],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| "无法读取用户")?;
        if owner.as_deref().is_some_and(|owner| Some(owner) != id) {
            return Err("用户名已存在".into());
        }
        self.invalidate_identities()?;
        if let Some(id) = id {
            let changed = db.execute("UPDATE web_users SET username=?1,role=?2,enabled=?3,password_hash=COALESCE(?4,password_hash),revision=revision+1,token_hash=NULL,updated_at=?5 WHERE id=?6",
                params![username,role,enabled as i64,hash,now(),id]).map_err(|_| "用户更新失败，请检查用户名是否重复")?;
            if changed == 0 {
                return Err("用户不存在".into());
            }
        } else {
            let hash = hash.expect("checked above");
            let id = uuid::Uuid::new_v4().to_string();
            db.execute("INSERT INTO web_users(id,username,password_hash,role,enabled,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?6)",
                params![id,username,hash,role,enabled as i64,now()]).map_err(|_| "用户创建失败，请检查用户名是否重复")?;
            drop(db);
            self.sync_tokens()?;
            return self.user(&id)?.ok_or("无法读取用户".into());
        }
        drop(db);
        self.sync_tokens()?;
        self.user(id.unwrap())?.ok_or("用户不存在".into())
    }

    pub fn save_account(
        &self,
        id: &str,
        username: &str,
        password: Option<&str>,
        enabled: bool,
    ) -> Result<WebUser, String> {
        if id == "admin" {
            return Err("初始管理员只能通过修改密码入口管理".into());
        }
        if username.len() < 2
            || username.len() > 32
            || !username
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("用户名无效".into());
        }
        let hash = match password {
            Some(value) if value.chars().count() >= 12 => Some(password_hash(value)?),
            Some(_) => return Err("密码至少 12 个字符".into()),
            None => None,
        };
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let exists = db
            .query_row("SELECT 1 FROM web_users WHERE id=?1", [id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map_err(|_| "无法读取用户")?;
        if exists.is_none() {
            return Err("用户不存在".into());
        }
        let owner = db
            .query_row(
                "SELECT id FROM web_users WHERE username=?1",
                [username],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|_| "无法读取用户")?;
        if owner.as_deref().is_some_and(|owner| owner != id) {
            return Err("用户名已存在".into());
        }
        self.invalidate_identities()?;
        db.execute("UPDATE web_users SET username=?1,enabled=?2,password_hash=COALESCE(?3,password_hash),revision=revision+1,token_hash=NULL,updated_at=?4 WHERE id=?5",
            params![username,enabled as i64,hash,now(),id]).map_err(|_| "用户更新失败，请检查用户名是否重复")?;
        drop(db);
        self.sync_tokens()?;
        self.user(id)?.ok_or("用户不存在".into())
    }

    pub fn assign_role(&self, id: &str, role: Option<&str>) -> Result<WebUser, String> {
        if id == "admin" {
            return Err("不能调整初始管理员角色".into());
        }
        if role.is_some_and(|role| !matches!(role, "admin" | "operator" | "viewer")) {
            return Err("角色无效".into());
        }
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let current = db
            .query_row("SELECT role FROM web_users WHERE id=?1", [id], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()
            .map_err(|_| "无法读取用户")?
            .ok_or("用户不存在")?;
        if current.as_deref() == role {
            drop(db);
            return self.user(id)?.ok_or("用户不存在".into());
        }
        self.invalidate_identities()?;
        db.execute("UPDATE web_users SET role=?1,revision=revision+1,token_hash=NULL,updated_at=?2 WHERE id=?3",
            params![role, now(), id]).map_err(|_| "无法调整用户角色")?;
        drop(db);
        self.sync_tokens()?;
        self.user(id)?.ok_or("用户不存在".into())
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        if id == "admin" {
            return Err("不能删除初始管理员".into());
        }
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let exists = db
            .query_row("SELECT 1 FROM web_users WHERE id=?1", [id], |row| {
                row.get::<_, i64>(0)
            })
            .optional()
            .map_err(|_| "无法读取用户")?;
        if exists.is_none() {
            return Err("用户不存在".into());
        }
        self.invalidate_identities()?;
        if db
            .execute("DELETE FROM web_users WHERE id=?1", [id])
            .map_err(|_| "无法删除用户")?
            == 0
        {
            return Err("用户不存在".into());
        }
        drop(db);
        self.sync_tokens()
    }

    pub fn issue_token(&self, user_id: &str) -> Result<String, String> {
        let user = self.user(user_id)?.ok_or("用户不存在")?;
        if !user.enabled || !matches!(user.role.as_deref(), Some("admin" | "operator")) {
            return Err("当前用户无权使用 MCP".into());
        }
        self.invalidate_identities()?;
        let token = URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>());
        let digest = format!("{:x}", Sha256::digest(token.as_bytes()));
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.execute(
            "UPDATE web_users SET token_hash=?1,updated_at=?2 WHERE id=?3",
            params![digest, now(), user_id],
        )
        .map_err(|_| "无法保存访问令牌")?;
        drop(db);
        self.sync_tokens()?;
        Ok(token)
    }

    pub fn revoke_token(&self, id: &str) -> Result<(), String> {
        self.invalidate_identities()?;
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.execute(
            "UPDATE web_users SET token_hash=NULL,updated_at=?1 WHERE id=?2",
            params![now(), id],
        )
        .map_err(|_| "无法撤销访问令牌")?;
        drop(db);
        self.sync_tokens()
    }

    pub fn change_password(&self, user_id: &str, new_password: &str) -> Result<(), String> {
        if user_id == "admin" {
            return Err("初始管理员密码由兼容入口更新".into());
        }
        let hash = password_hash(new_password)?;
        self.invalidate_identities()?;
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.execute(
            "UPDATE web_users SET password_hash=?1,revision=revision+1,token_hash=NULL,updated_at=?2 WHERE id=?3",
            params![hash, now(), user_id],
        )
        .map_err(|_| "无法更新密码")?;
        drop(db);
        self.sync_tokens()
    }
    pub fn bump_revision(&self, id: &str) -> Result<(), String> {
        self.invalidate_identities()?;
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.execute(
            "UPDATE web_users SET revision=revision+1,token_hash=NULL,updated_at=?1 WHERE id=?2",
            params![now(), id],
        )
        .map_err(|_| "无法使旧会话失效")?;
        drop(db);
        self.sync_tokens()
    }

    fn sync_tokens(&self) -> Result<(), String> {
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        let mut query = db.prepare("SELECT id,username,role,token_hash FROM web_users WHERE enabled=1 AND role IN ('admin','operator') AND token_hash IS NOT NULL")
            .map_err(|_| "无法读取访问令牌")?;
        let entries = query.query_map([], |row| Ok(json!({"id":row.get::<_,String>(0)?,"username":row.get::<_,String>(1)?,"role":row.get::<_,String>(2)?,"hash":row.get::<_,String>(3)?})))
            .map_err(|_| "无法读取访问令牌")?.collect::<rusqlite::Result<Vec<Value>>>().map_err(|_| "无法读取访问令牌")?;
        drop(query);
        drop(db);
        let temp = self.identities.with_extension("json.tmp");
        fs::write(
            &temp,
            serde_json::to_vec(&entries).map_err(|_| "无法编码访问令牌")?,
        )
        .map_err(|_| "无法同步访问令牌")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&temp, fs::Permissions::from_mode(0o600))
                .map_err(|_| "无法保护访问令牌")?;
        }
        fs::rename(&temp, &self.identities).map_err(|_| "无法同步访问令牌".into())
    }

    fn invalidate_identities(&self) -> Result<(), String> {
        match fs::remove_file(&self.identities) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err("无法撤销旧访问令牌".into()),
        }
    }

    pub fn identity_file(&self) -> &Path {
        &self.identities
    }
    pub fn audit_file(&self) -> &Path {
        &self.audit_path
    }

    pub fn audit(
        &self,
        actor: &WebUser,
        action: &str,
        outcome: &str,
        profile: Option<&str>,
    ) -> Result<(), String> {
        self.audit_actor(&actor.id, &actor.username, action, outcome, profile)
    }

    pub fn audit_actor(
        &self,
        actor_id: &str,
        actor_name: &str,
        action: &str,
        outcome: &str,
        profile: Option<&str>,
    ) -> Result<(), String> {
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        db.execute("INSERT INTO web_audit(timestamp,actor_id,actor_name,channel,action,outcome,profile_id) VALUES(?1,?2,?3,'web',?4,?5,?6)",
            params![now(),actor_id,actor_name,action,outcome,profile]).map_err(|_| "无法记录操作审计")?;
        db.execute(
            "DELETE FROM web_audit WHERE timestamp < ?1",
            [now() - 90 * 86_400_000],
        )
        .map_err(|_| "无法维护审计保留期")?;
        Ok(())
    }

    pub fn audit_query(&self, query: &AuditQuery) -> Result<Value, String> {
        let (requested_page, size) = query.validate()?;
        let mut events = Vec::new();
        for file in [
            self.audit_path.with_extension("jsonl.1"),
            self.audit_path.clone(),
        ] {
            if let Ok(metadata) = fs::metadata(&file) {
                if metadata.len() > 9 * 1024 * 1024 {
                    continue;
                }
                if let Ok(text) = fs::read_to_string(&file) {
                    for line in text.lines() {
                        if let Ok(value) = serde_json::from_str::<Value>(line) {
                            if value.get("channel").and_then(Value::as_str) == Some("mcp")
                                && value.get("timestamp").and_then(Value::as_i64).is_some()
                                && value.get("actorId").and_then(Value::as_str).is_some()
                                && value.get("action").and_then(Value::as_str).is_some()
                                && value.get("outcome").and_then(Value::as_str) != Some("started")
                                && query.matches(&value)
                            {
                                events.push(value);
                            }
                        }
                    }
                }
            }
        }
        let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
        const FILTER: &str = " FROM web_audit WHERE outcome != 'started' AND (?1 IS NULL OR instr(lower(actor_name),lower(?1)) > 0) AND (?2 IS NULL OR instr(lower(action),lower(?2)) > 0) AND (?3 IS NULL OR outcome=?3) AND (?4 IS NULL OR profile_id=?4) AND (?5 IS NULL OR timestamp>=?5) AND (?6 IS NULL OR timestamp<=?6)";
        let actor = query.actor.as_deref();
        let action = query.action.as_deref();
        let outcome = query.outcome.as_deref();
        let profile = query.profile_id.as_deref();
        let web_count: i64 = if query.channel.as_deref() == Some("mcp") {
            0
        } else {
            db.query_row(
                &format!("SELECT count(*){FILTER}"),
                params![actor, action, outcome, profile, query.from, query.to],
                |row| row.get(0),
            )
            .map_err(|_| "无法读取审计记录")?
        };
        let total = web_count as usize + events.len();
        let page = requested_page.min(total.saturating_sub(1) / size + 1);
        let offset = (page - 1) * size;
        let web_skip = offset.saturating_sub(events.len());
        if web_count > 0 {
            let sql = format!("SELECT timestamp,actor_id,actor_name,channel,action,outcome,profile_id{FILTER} ORDER BY timestamp DESC,id DESC LIMIT ?7 OFFSET ?8");
            let mut stmt = db.prepare(&sql).map_err(|_| "无法读取审计记录")?;
            let rows = stmt.query_map(params![actor,action,outcome,profile,query.from,query.to,(offset-web_skip+size) as i64,web_skip as i64], |row| Ok(json!({
                "timestamp": row.get::<_,i64>(0)?, "actorId":row.get::<_,String>(1)?, "actor":row.get::<_,String>(2)?,
                "channel":row.get::<_,String>(3)?, "action":row.get::<_,String>(4)?, "outcome":row.get::<_,String>(5)?, "profileId":row.get::<_,Option<String>>(6)?
            }))).map_err(|_| "无法读取审计记录")?;
            events.extend(
                rows.collect::<rusqlite::Result<Vec<_>>>()
                    .map_err(|_| "无法读取审计记录")?,
            );
        }
        events.sort_by_key(|event| {
            std::cmp::Reverse(event["timestamp"].as_i64().unwrap_or_default())
        });
        let items: Vec<_> = events
            .into_iter()
            .skip(offset - web_skip)
            .take(size)
            .collect();
        Ok(json!({"items":items,"total":total,"page":page,"pageSize":size}))
    }

    #[cfg(test)]
    pub fn audit_list(&self, limit: usize) -> Result<Vec<Value>, String> {
        let query = AuditQuery {
            page_size: Some(100),
            ..Default::default()
        };
        let items = self.audit_query(&query)?["items"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        Ok(items.into_iter().take(limit).collect())
    }

    pub fn summary(&self) -> Result<Value, String> {
        let since = now() - 7 * 86_400_000;
        let events = {
            let db = self.db.lock().map_err(|_| "用户数据库不可用")?;
            let mut stmt = db.prepare("SELECT timestamp,actor_id,actor_name,channel,action,outcome,profile_id FROM web_audit WHERE timestamp >= ?1 ORDER BY id")
                .map_err(|_| "无法读取审计记录")?;
            let rows = stmt.query_map([since], |row| Ok(json!({
                "timestamp":row.get::<_,i64>(0)?,"actorId":row.get::<_,String>(1)?,"actor":row.get::<_,String>(2)?,
                "channel":row.get::<_,String>(3)?,"action":row.get::<_,String>(4)?,"outcome":row.get::<_,String>(5)?,"profileId":row.get::<_,Option<String>>(6)?
            }))).map_err(|_| "无法读取审计记录")?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|_| "无法读取审计记录".to_string())?
        };
        let mut events = events;
        for file in [
            self.audit_path.with_extension("jsonl.1"),
            self.audit_path.clone(),
        ] {
            if fs::metadata(&file).is_ok_and(|meta| meta.len() <= 9 * 1024 * 1024) {
                if let Ok(text) = fs::read_to_string(file) {
                    for line in text.lines() {
                        if let Ok(event) = serde_json::from_str::<Value>(line) {
                            if event.get("channel").and_then(Value::as_str) == Some("mcp")
                                && event
                                    .get("timestamp")
                                    .and_then(Value::as_i64)
                                    .is_some_and(|time| time >= since)
                            {
                                events.push(event);
                            }
                        }
                    }
                }
            }
        }
        let mut by_day = std::collections::BTreeMap::<String, usize>::new();
        let mut by_action = std::collections::BTreeMap::<String, usize>::new();
        let mut by_actor = std::collections::BTreeMap::<String, usize>::new();
        let mut actor_names = std::collections::BTreeMap::<String, String>::new();
        let mut by_channel = std::collections::BTreeMap::<String, usize>::new();
        let mut by_profile = std::collections::BTreeMap::<String, usize>::new();
        let mut failures = 0;
        let mut total = 0;
        for event in events.iter().filter(|event| {
            event
                .get("timestamp")
                .and_then(Value::as_i64)
                .unwrap_or_default()
                >= since
                && event.get("outcome").and_then(Value::as_str) != Some("started")
        }) {
            total += 1;
            if matches!(
                event.get("outcome").and_then(Value::as_str),
                Some("failure" | "denied")
            ) {
                failures += 1;
            }
            let time = event
                .get("timestamp")
                .and_then(Value::as_i64)
                .unwrap_or_default();
            let day = (time / 86_400_000).to_string();
            *by_day.entry(day).or_default() += 1;
            let action = event
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            *by_action.entry(action.to_string()).or_default() += 1;
            if let Some(actor_id) = event
                .get("actorId")
                .and_then(Value::as_str)
                .filter(|id| *id != "anonymous")
            {
                let actor = event
                    .get("actor")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                *by_actor.entry(actor_id.to_string()).or_default() += 1;
                actor_names.insert(actor_id.to_string(), actor.to_string());
            }
            let channel = event
                .get("channel")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            *by_channel.entry(channel.to_string()).or_default() += 1;
            if let Some(profile) = event.get("profileId").and_then(Value::as_str) {
                *by_profile.entry(profile.to_string()).or_default() += 1;
            }
        }
        Ok(
            json!({"total":total,"failures":failures,"byDay":by_day,"byAction":by_action,"byActor":by_actor,"actorNames":actor_names,"byChannel":by_channel,"byProfile":by_profile,"windowDays":7,"mcpRetention":"latest 16 MiB"}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_admin_and_role_changes_keep_credentials_isolated() {
        let dir = tempfile::tempdir().unwrap();
        let legacy = password_hash("existing-admin-password").unwrap();
        fs::write(dir.path().join("admin.hash"), &legacy).unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        assert!(access
            .authenticate("admin", "existing-admin-password", dir.path())
            .unwrap()
            .is_some());
        let user = access
            .save(
                None,
                "operator_one",
                Some("operator-password-1234"),
                "operator",
                true,
            )
            .unwrap();
        assert!(access
            .authenticate("operator_one", "operator-password-1234", dir.path())
            .unwrap()
            .is_some());
        let token = access.issue_token(&user.id).unwrap();
        let identities = fs::read_to_string(access.identity_file()).unwrap();
        assert!(!identities.contains(&token));
        assert!(identities.contains(&user.id));
        assert!(access
            .save(
                None,
                "operator_one",
                Some("another-password-1234"),
                "operator",
                true
            )
            .is_err());
        assert!(access
            .save(Some("missing"), "missing", None, "viewer", true)
            .is_err());
        assert!(access.delete("missing").is_err());
        assert_eq!(
            fs::read_to_string(access.identity_file()).unwrap(),
            identities
        );
        let updated = access
            .save(Some(&user.id), "operator_one", None, "viewer", true)
            .unwrap();
        assert!(updated.revision > user.revision);
        assert!(!updated.has_token);
        assert!(access.issue_token(&user.id).is_err());
        assert!(!fs::read_to_string(access.identity_file())
            .unwrap()
            .contains(&user.id));
        assert!(access.delete("admin").is_err());
        let reopened = WebAccess::open(dir.path()).unwrap();
        assert_eq!(
            reopened.user(&user.id).unwrap().unwrap().role.as_deref(),
            Some("viewer")
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("admin.hash")).unwrap(),
            legacy
        );
    }

    #[test]
    fn editing_account_cannot_overwrite_a_newer_role_assignment() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        let user = access
            .save(
                None,
                "account_one",
                Some("account-password-1234"),
                "operator",
                true,
            )
            .unwrap();
        access.issue_token(&user.id).unwrap();
        let reassigned = access
            .save(Some(&user.id), "account_one", None, "viewer", true)
            .unwrap();
        let edited = access
            .save_account(&user.id, "account_renamed", None, false)
            .unwrap();
        assert_eq!(edited.role.as_deref(), Some("viewer"));
        assert!(!edited.enabled);
        assert!(!edited.has_token);
        assert!(edited.revision > reassigned.revision);
        assert!(access.save_account(&user.id, "admin", None, true).is_err());
        assert!(access.save_account("admin", "admin", None, true).is_err());
    }

    #[test]
    fn assigning_role_preserves_account_fields_and_revokes_token() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        let user = access
            .save(
                None,
                "role_member",
                Some("member-password-1234"),
                "operator",
                false,
            )
            .unwrap();
        access
            .save_account(&user.id, "renamed_member", None, true)
            .unwrap();
        access.issue_token(&user.id).unwrap();
        let changed = access.assign_role(&user.id, Some("viewer")).unwrap();
        assert_eq!(changed.username, "renamed_member");
        assert!(changed.enabled);
        assert!(!changed.has_token);
        assert!(changed.revision > user.revision);
        assert!(access.assign_role("admin", Some("viewer")).is_err());
        assert!(access.assign_role(&user.id, Some("unknown")).is_err());
    }

    #[test]
    fn removing_role_revokes_access_and_can_be_reassigned() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        let user = access
            .save(
                None,
                "member",
                Some("member-password-1234"),
                "operator",
                true,
            )
            .unwrap();
        access.issue_token(&user.id).unwrap();
        let removed = access.assign_role(&user.id, None).unwrap();
        assert_eq!(removed.role, None);
        assert!(!removed.has_token);
        assert!(removed.revision > user.revision);
        assert!(!fs::read_to_string(access.identity_file())
            .unwrap()
            .contains(&user.id));
        assert!(access
            .authenticate("member", "member-password-1234", dir.path())
            .unwrap()
            .is_none());
        assert!(access.issue_token(&user.id).is_err());
        assert!(access.assign_role("admin", None).is_err());
        drop(access);
        let access = WebAccess::open(dir.path()).unwrap();
        assert_eq!(access.user(&user.id).unwrap().unwrap().role, None);
        access.assign_role(&user.id, Some("viewer")).unwrap();
        assert!(access
            .authenticate("member", "member-password-1234", dir.path())
            .unwrap()
            .is_some());
    }

    #[test]
    fn old_user_table_migrates_without_losing_accounts_or_tokens() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let db = Connection::open(dir.path().join("web-access.db")).unwrap();
        db.execute_batch(
            "CREATE TABLE web_users (
            id TEXT PRIMARY KEY, username TEXT NOT NULL UNIQUE, password_hash TEXT,
            role TEXT NOT NULL CHECK(role IN ('admin','operator','viewer')),
            enabled INTEGER NOT NULL CHECK(enabled IN (0,1)), revision INTEGER NOT NULL DEFAULT 0,
            token_hash TEXT, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL);",
        )
        .unwrap();
        let hash = password_hash("member-password-1234").unwrap();
        let token_hash = format!("{:x}", Sha256::digest(b"old-token"));
        db.execute(
            "INSERT INTO web_users VALUES (?1,?2,?3,?4,1,7,?5,10,11)",
            params!["old-user", "member", hash, "operator", token_hash],
        )
        .unwrap();
        drop(db);
        let access = WebAccess::open(dir.path()).unwrap();
        let user = access.user("old-user").unwrap().unwrap();
        assert_eq!(user.role.as_deref(), Some("operator"));
        assert_eq!(user.revision, 7);
        assert!(user.has_token);
        assert!(access
            .authenticate("member", "member-password-1234", dir.path())
            .unwrap()
            .is_some());
        assert!(fs::read_to_string(access.identity_file())
            .unwrap()
            .contains(&token_hash));
        access.assign_role("old-user", None).unwrap();
        drop(access);
        assert_eq!(
            WebAccess::open(dir.path())
                .unwrap()
                .user("old-user")
                .unwrap()
                .unwrap()
                .role,
            None
        );
    }

    #[test]
    fn audit_omits_sensitive_input_and_reports_real_events() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        let admin = access.user("admin").unwrap().unwrap();
        access
            .audit(&admin, "upsert_profile", "success", Some("profile-1"))
            .unwrap();
        access
            .audit(&admin, "test_connection", "failure", None)
            .unwrap();
        access
            .audit_actor("anonymous", "mistyped", "login", "failure", None)
            .unwrap();
        let rows = access.audit_list(10).unwrap();
        assert_eq!(rows.len(), 3);
        let summary = access.summary().unwrap();
        assert_eq!(summary["failures"], 2);
        assert_eq!(summary["byActor"]["admin"], 2);
        assert!(summary["byActor"].get("anonymous").is_none());
        assert!(!fs::read_to_string(dir.path().join("web-access.db"))
            .unwrap_or_default()
            .contains("existing-admin-password"));
    }

    #[test]
    fn audit_filters_before_pagination_across_web_and_mcp() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("admin.hash"),
            password_hash("existing-admin-password").unwrap(),
        )
        .unwrap();
        let access = WebAccess::open(dir.path()).unwrap();
        for i in 0..35 {
            access
                .audit_actor(
                    "user",
                    "alice",
                    "connect",
                    if i % 2 == 0 { "success" } else { "failure" },
                    Some("profile-1"),
                )
                .unwrap();
        }
        let timestamp = now() + 1000;
        let mcp = (0..25).map(|_| json!({"timestamp":timestamp,"actorId":"user","actor":"alice","channel":"mcp","action":"connect","outcome":"success","profileId":"profile-1"}).to_string()).collect::<Vec<_>>().join("\n");
        fs::write(access.audit_file(), format!("{mcp}\n")).unwrap();
        let query = AuditQuery {
            page: Some(2),
            page_size: Some(20),
            actor: Some("ALI".into()),
            outcome: Some("success".into()),
            ..Default::default()
        };
        let result = access.audit_query(&query).unwrap();
        assert_eq!(result["total"], 43);
        assert_eq!(result["items"].as_array().unwrap().len(), 20);
        let third = access
            .audit_query(&AuditQuery {
                page: Some(3),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(third["total"], 60);
        assert_eq!(third["items"].as_array().unwrap().len(), 20);
        assert!(third["items"]
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["channel"] == "web"));
        assert_eq!(
            result["items"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["channel"] == "web")
                .count(),
            15
        );
        let web = access
            .audit_query(&AuditQuery {
                channel: Some("web".into()),
                profile_id: Some("profile-1".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(web["total"], 35);
        let last = access
            .audit_query(&AuditQuery {
                page: Some(99),
                channel: Some("mcp".into()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(last["page"], 2);
        assert_eq!(last["items"].as_array().unwrap().len(), 5);
        assert!(access
            .audit_query(&AuditQuery {
                page_size: Some(1000),
                ..Default::default()
            })
            .is_err());
    }
}
