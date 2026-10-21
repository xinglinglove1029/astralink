use crate::{
    models::*,
    storage::{validate_profiles, StoredState},
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use std::{fs, path::Path, time::Duration};
const MAX_BYTES: usize = 16 * 1024 * 1024;
pub const DATABASE_FILE: &str = "astralink.db";

fn valid_state(state: &StoredState) -> Result<(), String> {
    if state
        .profiles
        .iter()
        .any(|p| p.password.is_some() || p.passphrase.is_some() || p.totp_secret.is_some())
    {
        return Err("应用数据含明文凭据，请先迁移到系统凭据库".into());
    }
    validate_profiles(&state.profiles, false)?;
    if state.settings.mcp_port == 0 {
        return Err("MCP 端口无效".into());
    }
    Ok(())
}
fn legacy(path: &Path) -> Result<StoredState, String> {
    if !path.exists() {
        return Ok(StoredState::default());
    }
    if fs::metadata(path).map_err(|_| "无法读取旧配置")?.len() > MAX_BYTES as u64 {
        return Err("应用数据超过大小限制".into());
    }
    let bytes = fs::read(path).map_err(|_| "无法读取旧配置")?;
    let state =
        serde_json::from_slice::<StoredState>(&bytes).map_err(|_| "应用数据损坏，未覆盖原文件")?;
    valid_state(&state)?;
    Ok(state)
}
pub(crate) fn open(directory: &Path) -> Result<(Connection, StoredState), String> {
    let path = directory.join(DATABASE_FILE);
    // Validate the old file before creating a new DB. Keep it unchanged as the migration backup.
    let original = if !path.exists() {
        Some(legacy(&directory.join("state.json"))?)
    } else {
        None
    };
    let mut db = Connection::open(path).map_err(|_| "无法打开本机 SQLite 数据库")?;
    db.busy_timeout(Duration::from_secs(2))
        .map_err(|_| "无法设置数据库等待时间")?;
    db.pragma_update(None, "synchronous", "FULL")
        .map_err(|_| "无法设置数据库持久化选项")?;
    let version: i64 = db
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(|_| "无法读取数据库版本")?;
    match version {
        0 => {
            let state = match original {
                Some(state) => state,
                None => legacy(&directory.join("state.json"))?,
            };
            let tx = db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| "无法启动数据库迁移")?;
            tx.execute_batch("-- 服务器元数据；密码、私钥口令和动态密钥仅存于系统凭据库。
                CREATE TABLE servers (id TEXT PRIMARY KEY, position INTEGER NOT NULL,
                    name TEXT NOT NULL UNIQUE, host TEXT NOT NULL, port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),
                    username TEXT NOT NULL, group_name TEXT NOT NULL, source TEXT NOT NULL,
                    auth_type TEXT NOT NULL, transport_mode TEXT NOT NULL, enabled INTEGER NOT NULL,
                    try_keyboard INTEGER NOT NULL, metadata_json TEXT NOT NULL CHECK(json_valid(metadata_json)));
                CREATE INDEX servers_group_name ON servers(group_name);
                CREATE INDEX servers_host ON servers(host);
                -- 应用设置，单行持久化，与服务器修改共享事务。
                CREATE TABLE app_settings (id INTEGER PRIMARY KEY CHECK(id=1), mcp_port INTEGER NOT NULL CHECK(mcp_port BETWEEN 1 AND 65535),
                    start_on_launch INTEGER NOT NULL, minimize_to_tray INTEGER NOT NULL);
                PRAGMA user_version = 1;")
                .map_err(|_| "无法初始化 SQLite 表结构")?;
            save_rows(&tx, &state)?;
            tx.commit()
                .map_err(|_| "数据库迁移提交失败；旧配置已保留")?;
            Ok((db, state))
        }
        1 => {
            let state = read(&db)?;
            Ok((db, state))
        }
        _ => Err("数据库版本不受支持，请使用匹配版本的 Astralink".into()),
    }
}
fn save_rows(db: &Connection, state: &StoredState) -> Result<(), String> {
    let bytes = serde_json::to_vec(state).map_err(|_| "无法序列化连接配置")?;
    if bytes.len() > MAX_BYTES {
        return Err("应用配置超过大小限制".into());
    }
    db.execute("DELETE FROM servers", [])
        .map_err(|_| "无法更新数据库连接配置")?;
    for (position, profile) in state.profiles.iter().enumerate() {
        let metadata = serde_json::to_string(profile).map_err(|_| "无法序列化连接元数据")?;
        let auth = match profile.auth_type {
            AuthType::Password => "password",
            AuthType::PrivateKey => "privateKey",
            AuthType::Agent => "agent",
        };
        let transport = match profile.transport_mode {
            TransportMode::Exec => "exec",
            TransportMode::Shell => "shell",
        };
        db.execute("INSERT INTO servers(id,position,name,host,port,username,group_name,source,auth_type,transport_mode,enabled,try_keyboard,metadata_json)
            VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
            params![profile.id,position as i64,profile.name,profile.host,profile.port,profile.username,profile.group,profile.source,auth,transport,profile.enabled,profile.try_keyboard,metadata])
            .map_err(|_| "数据库写入服务器配置失败")?;
    }
    db.execute("INSERT INTO app_settings(id,mcp_port,start_on_launch,minimize_to_tray) VALUES(1,?1,?2,?3)
        ON CONFLICT(id) DO UPDATE SET mcp_port=excluded.mcp_port,start_on_launch=excluded.start_on_launch,minimize_to_tray=excluded.minimize_to_tray",
        params![state.settings.mcp_port,state.settings.start_on_launch,state.settings.minimize_to_tray])
        .map_err(|_| "数据库写入应用设置失败")?;
    Ok(())
}
pub(crate) fn save(db: &mut Connection, state: &StoredState) -> Result<(), String> {
    valid_state(state)?;
    let tx = db
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "数据库忙或无法写入，请稍后重试")?;
    save_rows(&tx, state)?;
    tx.commit()
        .map_err(|_| "数据库事务提交失败，已保留原配置".to_string())
}
pub(crate) fn read(db: &Connection) -> Result<StoredState, String> {
    let (count, bytes): (i64, i64) = db
        .query_row(
            "SELECT COUNT(*),COALESCE(SUM(LENGTH(CAST(metadata_json AS BLOB))),0) FROM servers",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(|_| "数据库表结构损坏或不完整")?;
    if count > 512 || bytes > MAX_BYTES as i64 {
        return Err("数据库配置超过大小限制".into());
    }
    let mut statement = db
        .prepare("SELECT metadata_json FROM servers ORDER BY position LIMIT 513")
        .map_err(|_| "数据库表结构损坏或不完整")?;
    let rows = statement
        .query_map([], |row| row.get::<_, String>(0))
        .map_err(|_| "无法查询服务器配置")?;
    let mut profiles = Vec::new();
    let mut size = 0;
    for row in rows {
        let text = row.map_err(|_| "无法读取服务器数据")?;
        size += text.len();
        if size > MAX_BYTES {
            return Err("应用数据超过大小限制".into());
        }
        profiles.push(serde_json::from_str(&text).map_err(|_| "数据库连接元数据损坏")?);
    }
    let settings = db
        .query_row(
            "SELECT mcp_port,start_on_launch,minimize_to_tray FROM app_settings WHERE id=1",
            [],
            |row| {
                Ok(AppSettings {
                    mcp_port: row.get(0)?,
                    start_on_launch: row.get(1)?,
                    minimize_to_tray: row.get(2)?,
                })
            },
        )
        .optional()
        .map_err(|_| "无法读取应用设置")?
        .ok_or("数据库应用设置缺失")?;
    let state = StoredState { profiles, settings };
    valid_state(&state)?;
    Ok(state)
}
pub fn committed_bytes(directory: &Path) -> Option<Vec<u8>> {
    let path = directory.join(DATABASE_FILE);
    if path.exists() {
        let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).ok()?;
        serde_json::to_vec_pretty(&read(&db).ok()?).ok()
    } else {
        fs::read(directory.join("state.json")).ok()
    }
}
