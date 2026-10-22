use super::*;
use redb::{
    Database, DatabaseError, ReadOnlyDatabase, ReadableDatabase, ReadableTable, TableDefinition,
    TableError,
};
use std::collections::HashMap;

const GROUPS: TableDefinition<&str, &[u8]> = TableDefinition::new("groups");
const CONNECTIONS: TableDefinition<&str, &[u8]> = TableDefinition::new("connections");
const META: TableDefinition<&str, &str> = TableDefinition::new("meta");
const MAX_DATABASE_BYTES: u64 = 256 * 1024 * 1024;
const MAX_RECORD_BYTES: usize = 1024 * 1024;

fn group_path(id: &str, groups: &HashMap<String, Value>) -> String {
    let mut current = id.to_owned();
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    while !current.is_empty() && seen.insert(current.clone()) && names.len() < 32 {
        let Some(group) = groups.get(&current) else {
            break;
        };
        if let Some(name) = text(group, &["name"]) {
            names.push(name.to_owned());
        }
        current = text(group, &["parent_id"]).unwrap_or_default().to_owned();
    }
    names.reverse();
    names.join("/")
}

pub(super) fn database(path: &Path) -> Result<ImportPreview, String> {
    let meta = fs::metadata(path).map_err(|_| "NyaTerm 数据库不存在或无法读取")?;
    if !meta.is_file() || meta.len() > MAX_DATABASE_BYTES {
        return Err("NyaTerm 数据库不是有效文件或超过 256 MB".into());
    }
    match ReadOnlyDatabase::open(path) {
        Ok(db) => read_database(&db, path),
        Err(DatabaseError::RepairAborted | DatabaseError::TransactionInProgress) => {
            let temp = tempfile::Builder::new()
                .prefix("astralink-nyaterm-")
                .tempdir()
                .map_err(|_| "无法创建 NyaTerm 临时副本目录")?;
            let copy = temp.path().join("nyaterm.redb");
            fs::copy(path, &copy).map_err(|_| "无法读取 NyaTerm 数据库副本")?;
            let db = Database::open(&copy)
                .map_err(|_| "NyaTerm 临时数据库无法恢复，请在 NyaTerm 中正常退出后重试")?;
            read_database(&db, path)
        }
        Err(DatabaseError::DatabaseAlreadyOpen) => {
            Err("NyaTerm 数据库正在使用，请退出 NyaTerm 后重试".into())
        }
        Err(DatabaseError::UpgradeRequired(_)) => Err("NyaTerm 数据库格式不兼容".into()),
        Err(_) => Err("无法只读打开 NyaTerm 数据库".into()),
    }
}

fn read_database(db: &impl ReadableDatabase, path: &Path) -> Result<ImportPreview, String> {
    let read = db.begin_read().map_err(|_| "无法读取 NyaTerm 数据库")?;
    match read.open_table(META) {
        Ok(table) => {
            if let Some(version) = table
                .get("schema_version")
                .map_err(|_| "无法读取 NyaTerm 数据库版本")?
            {
                let number = version
                    .value()
                    .parse::<u32>()
                    .map_err(|_| "NyaTerm 数据库版本无效")?;
                if number != 3 {
                    return Err("NyaTerm 数据库版本不受支持，请先在 NyaTerm 更新数据".into());
                }
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(_) => return Err("无法读取 NyaTerm 数据库版本".into()),
    }
    let mut groups = HashMap::new();
    match read.open_table(GROUPS) {
        Ok(table) => {
            for entry in table.iter().map_err(|_| "无法读取 NyaTerm 分组")? {
                let (key, raw) = entry.map_err(|_| "NyaTerm 分组记录损坏")?;
                if !key.value().starts_with("groups/") {
                    continue;
                }
                if raw.value().len() > MAX_RECORD_BYTES {
                    return Err("NyaTerm 分组记录过大".into());
                }
                let value: Value =
                    serde_json::from_slice(raw.value()).map_err(|_| "NyaTerm 分组格式无效")?;
                if let Some(id) = text(&value, &["id"]) {
                    groups.insert(id.to_owned(), value);
                }
            }
        }
        Err(TableError::TableDoesNotExist(_)) => {}
        Err(_) => return Err("无法读取 NyaTerm 分组".into()),
    }
    let mut preview = ImportPreview {
        source: "nyaterm".into(),
        source_path: path.to_string_lossy().into(),
        profiles: Vec::new(),
        issues: Vec::new(),
        stats: ImportStats::default(),
    };
    let table = read
        .open_table(CONNECTIONS)
        .map_err(|_| "NyaTerm 数据库没有连接表，请先在 NyaTerm 完成数据迁移")?;
    for entry in table.iter().map_err(|_| "无法读取 NyaTerm 连接")? {
        let (key, raw) = entry.map_err(|_| "NyaTerm 连接记录损坏")?;
        if !key.value().starts_with("connections/") {
            continue;
        }
        if preview.profiles.len() + preview.stats.unsupported >= 512
            || raw.value().len() > MAX_RECORD_BYTES
        {
            return Err("NyaTerm 连接超过 512 个或单条记录过大".into());
        }
        let value: Value =
            serde_json::from_slice(raw.value()).map_err(|_| "NyaTerm 连接格式无效")?;
        if text(&value, &["type"]) != Some("ssh") {
            unsupported(
                &mut preview,
                "仅导入 NyaTerm SSH 连接",
                text(&value, &["name"]),
            );
            continue;
        }
        let mut p = new_profile(text(&value, &["name"]).unwrap_or_default(), "nyaterm");
        p.host = text(&value, &["host"]).unwrap_or_default().into();
        p.username = text(&value, &["username"]).unwrap_or_default().into();
        p.notes = text(&value, &["description"]).unwrap_or_default().into();
        p.group = group_path(text(&value, &["group_id"]).unwrap_or_default(), &groups);
        match port(value.get("port")) {
            Ok(number) => p.port = number,
            Err(_) => {
                unsupported(&mut preview, "NyaTerm 端口无效", Some(&p.name));
                continue;
            }
        }
        let auth = value.get("auth").unwrap_or(&Value::Null);
        match text(auth, &["mode"]).unwrap_or("password") {
            "password" => issue(
                &mut preview,
                "warning",
                "NyaTerm 数据库凭据不读取，请补充密码",
                Some(&p.name),
            ),
            "key" => {
                p.auth_type = AuthType::PrivateKey;
                disable(&mut preview, &mut p, "NyaTerm 私钥不导出，请选择本地私钥");
            }
            "agent" => {
                p.auth_type = AuthType::Agent;
                p.agent = std::env::var("SSH_AUTH_SOCK").ok();
                if p.agent.is_none() {
                    disable(&mut preview, &mut p, "未检测到 SSH Agent，请重新配置认证");
                }
            }
            _ => disable(&mut preview, &mut p, "NyaTerm 认证方式不支持，请手动配置"),
        }
        if value
            .get("network")
            .is_some_and(|network| !network.is_null())
            || value.get("post_login").is_some_and(|post| !post.is_null())
        {
            disable(
                &mut preview,
                &mut p,
                "代理或登录脚本暂不支持，请手动核对连接",
            );
        }
        normalize(p, &mut preview);
    }
    finalize(&mut preview);
    Ok(preview)
}

pub(super) fn parse(
    input: &str,
    _source: &Path,
    _home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let data: Value = serde_json::from_str(input).map_err(|_| "NyaTerm JSON 格式无效")?;
    if data.get("version").and_then(Value::as_u64) != Some(1) {
        return Err("仅支持 NyaTerm v1 JSON 配置".into());
    }
    let sessions = data
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or("NyaTerm 配置缺少 sessions 数组")?;
    if sessions.len() > 512 {
        return Err("最多导入 512 个 NyaTerm 会话".into());
    }
    let passwords = data.get("passwords").and_then(Value::as_array);
    let ssh_keys = data.get("ssh_keys").and_then(Value::as_array);
    for session in sessions {
        if text(session, &["type"]) != Some("ssh") {
            unsupported(preview, "仅导入 NyaTerm SSH 会话", text(session, &["name"]));
            continue;
        }
        let mut profile = new_profile(text(session, &["name"]).unwrap_or_default(), "nyaterm");
        profile.host = text(session, &["host"]).unwrap_or_default().into();
        profile.username = text(session, &["username"]).unwrap_or("root").into();
        profile.notes = text(session, &["description"]).unwrap_or_default().into();
        profile.group = session
            .get("group_path")
            .and_then(Value::as_array)
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("/")
            })
            .unwrap_or_default();
        match port(session.get("port")) {
            Ok(value) => profile.port = value,
            Err(_) => {
                unsupported(preview, "NyaTerm 端口无效", Some(&profile.name));
                continue;
            }
        }
        let auth = session.get("auth").unwrap_or(&Value::Null);
        match text(auth, &["mode"]).or_else(|| auth.is_null().then_some("password")) {
            Some("password") => {
                profile.password = text(auth, &["password"]).map(Into::into).or_else(|| {
                    let reference = text(auth, &["password_ref"])?;
                    passwords?.iter().find_map(|entry| {
                        (text(entry, &["ref"]) == Some(reference))
                            .then(|| text(entry, &["password"]).map(Into::into))
                            .flatten()
                    })
                });
                if profile.password.is_none() {
                    issue(
                        preview,
                        "warning",
                        "NyaTerm 密码引用未解析，请补充密码",
                        Some(&profile.name),
                    );
                }
            }
            Some("key") => {
                profile.auth_type = AuthType::PrivateKey;
                let key_known = text(auth, &["key_ref"]).and_then(|reference| {
                    ssh_keys?
                        .iter()
                        .find(|entry| text(entry, &["ref"]) == Some(reference))
                });
                if key_known.is_none() {
                    disable(
                        preview,
                        &mut profile,
                        "NyaTerm 私钥引用未解析，请选择本地私钥",
                    );
                } else {
                    disable(
                        preview,
                        &mut profile,
                        "NyaTerm 内联私钥不会写入文件，请重新选择本地私钥",
                    );
                }
            }
            Some("agent") => {
                profile.auth_type = AuthType::Agent;
                profile.agent = std::env::var("SSH_AUTH_SOCK").ok();
            }
            _ => disable(preview, &mut profile, "NyaTerm 认证方式不支持，请手动配置"),
        }
        normalize(profile, preview);
    }
    Ok(())
}
