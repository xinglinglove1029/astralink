use crate::{models::CodexRegistration, storage::atomic_write};
use std::{fs, path::Path};
use toml_edit::{value, DocumentMut, Item, Table};

pub fn register(path: &Path, port: u16, token: &str) -> Result<CodexRegistration, String> {
    if port == 0 || token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err("MCP 端口或访问令牌无效".into());
    }
    let original = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("无法读取 Codex 配置".into()),
    };
    let text = match &original {
        Some(bytes) => std::str::from_utf8(bytes).map_err(|_| "Codex 配置不是 UTF-8")?,
        None => "",
    };
    if text.len() > 8 * 1024 * 1024 {
        return Err("Codex 配置过大".into());
    }
    let mut doc = text
        .parse::<DocumentMut>()
        .map_err(|_| "Codex TOML 格式无效，原文件已保留")?;
    if doc.get("mcp_servers").is_none() {
        doc["mcp_servers"] = Item::Table(Table::new())
    }
    let servers = doc
        .get_mut("mcp_servers")
        .and_then(Item::as_table_like_mut)
        .ok_or("Codex mcp_servers 必须是表")?;
    let owned_previous = servers
        .get(crate::migration::LEGACY_CLIENT_KEY)
        .and_then(Item::as_table_like)
        .is_some_and(|entry| {
            entry.get("command").is_none()
                && entry
                    .get("url")
                    .and_then(Item::as_str)
                    .is_some_and(crate::migration::is_previous_local_endpoint)
        });
    if owned_previous {
        if servers.get("astralink").is_none() {
            if let Some(previous) = servers.remove(crate::migration::LEGACY_CLIENT_KEY) {
                servers.insert("astralink", previous);
            }
        } else {
            servers.remove(crate::migration::LEGACY_CLIENT_KEY);
        }
    }
    if servers.get("astralink").is_none() {
        servers.insert("astralink", Item::Table(Table::new()));
    }
    let astralink = servers
        .get_mut("astralink")
        .and_then(Item::as_table_like_mut)
        .ok_or("Codex astralink 必须是表")?;
    for key in [
        "command",
        "args",
        "env",
        "env_vars",
        "cwd",
        "bearer_token_env_var",
    ] {
        astralink.remove(key);
    }
    astralink.insert("enabled", value(true));
    astralink.insert("url", value(format!("http://127.0.0.1:{port}/mcp")));
    if astralink.get("http_headers").is_none() {
        astralink.insert("http_headers", Item::Table(Table::new()));
    }
    let headers = astralink
        .get_mut("http_headers")
        .and_then(Item::as_table_like_mut)
        .ok_or("Codex http_headers 必须是表")?;
    let auth_keys: Vec<_> = headers
        .iter()
        .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        .map(|(k, _)| k.to_string())
        .collect();
    for key in auth_keys {
        headers.remove(&key);
    }
    headers.insert("Authorization", value(format!("Bearer {token}")));
    if let Some(env_headers) = astralink.get_mut("env_http_headers") {
        let env_headers = env_headers
            .as_table_like_mut()
            .ok_or("Codex env_http_headers 必须是表")?;
        let keys: Vec<_> = env_headers
            .iter()
            .filter(|(k, _)| k.eq_ignore_ascii_case("authorization"))
            .map(|(k, _)| k.to_string())
            .collect();
        for key in keys {
            env_headers.remove(&key);
        }
    }
    let output = doc.to_string();
    output
        .parse::<DocumentMut>()
        .map_err(|_| "生成的 Codex 配置无效")?;
    if output == text {
        return Ok(CodexRegistration {
            config_path: path.to_string_lossy().into(),
            backup_path: None,
            reload_required: false,
        });
    }
    let backup_path = if let Some(bytes) = &original {
        let file = path
            .file_name()
            .and_then(|p| p.to_str())
            .ok_or("Codex 配置路径无效")?;
        let backup =
            path.with_file_name(format!("{file}.astralink-backup-{}", uuid::Uuid::new_v4()));
        atomic_write(&backup, bytes)?;
        Some(backup.to_string_lossy().into())
    } else {
        None
    };
    // Do not overwrite edits another process made while preparing the registration.
    let current = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(_) => return Err("无法重新读取 Codex 配置".into()),
    };
    if current != original {
        return Err("Codex 配置已被其他程序修改，请重试".into());
    }
    atomic_write(path, output.as_bytes())?;
    Ok(CodexRegistration {
        config_path: path.to_string_lossy().into(),
        backup_path,
        reload_required: true,
    })
}
