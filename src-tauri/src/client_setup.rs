use crate::{config_sync, models::CodexRegistration, storage::atomic_write};
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn default_path(client: &str, home: &Path, roaming: Option<&Path>) -> Result<PathBuf, String> {
    let path = match client {
        "claude-code" => home.join(".claude.json"),
        "workbuddy" => home.join(".workbuddy/mcp.json"),
        "claude-desktop" if cfg!(target_os = "macos") => {
            home.join("Library/Application Support/Claude/claude_desktop_config.json")
        }
        "claude-desktop" if cfg!(target_os = "linux") => {
            home.join(".config/Claude/claude_desktop_config.json")
        }
        "claude-desktop" => roaming
            .ok_or("无法定位 Claude Desktop 配置，请选择配置文件")?
            .join("Claude/claude_desktop_config.json"),
        "cursor" => home.join(".cursor/mcp.json"),
        "vscode" if cfg!(target_os = "macos") => {
            home.join("Library/Application Support/Code/User/mcp.json")
        }
        "vscode" if cfg!(target_os = "linux") => home.join(".config/Code/User/mcp.json"),
        "vscode" => roaming
            .ok_or("无法定位 VS Code 用户配置，请选择配置文件")?
            .join("Code/User/mcp.json"),
        "antigravity" => home.join(".gemini/config/mcp_config.json"),
        "opencode" => {
            let json = home.join(".config/opencode/opencode.json");
            let jsonc = json.with_extension("jsonc");
            if jsonc.exists() {
                jsonc
            } else {
                json
            }
        }
        "openclaw" => std::env::var_os("OPENCLAW_CONFIG_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".openclaw/openclaw.json")),
        "hermes" => std::env::var_os("HERMES_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".hermes"))
            .join("config.yaml"),
        "windsurf" => return Err("请先选择 Windsurf 当前使用的 MCP 配置文件".into()),
        _ => return Err("该客户端暂不支持一键配置".into()),
    };
    Ok(path)
}
fn read(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("无法读取客户端配置".into()),
    }
}
fn object_mut<'a>(
    value: &'a mut Value,
    path: &[&str],
) -> Result<&'a mut Map<String, Value>, String> {
    let mut current = value;
    for key in path {
        current = current
            .as_object_mut()
            .ok_or("客户端配置节点必须是对象，原文件已保留")?
            .entry((*key).to_string())
            .or_insert_with(|| json!({}));
    }
    current
        .as_object_mut()
        .ok_or("客户端配置节点必须是对象，原文件已保留".into())
}
/// Updates only the AstraLink entry. Never includes config contents or secrets in errors.
pub fn register(
    client: &str,
    path: &Path,
    port: u16,
    token: &str,
) -> Result<CodexRegistration, String> {
    register_with_bridge(client, path, port, token, None)
}

pub fn register_with_bridge(
    client: &str,
    path: &Path,
    port: u16,
    token: &str,
    bridge: Option<(&Path, &Path)>,
) -> Result<CodexRegistration, String> {
    if port == 0 || token.is_empty() || token.len() > 4096 || token.chars().any(char::is_control) {
        return Err("MCP 端口或访问令牌无效".into());
    }
    if client == "codex" {
        return config_sync::register(path, port, token);
    }
    let keys: &[&str] = match client {
        "claude-code" | "claude-desktop" | "workbuddy" | "cursor" | "windsurf" | "antigravity" => {
            &["mcpServers"]
        }
        "vscode" => &["servers"],
        "opencode" => &["mcp"],
        "openclaw" => &["mcp", "servers"],
        "hermes" => &["mcp_servers"],
        _ => return Err("该客户端暂不支持一键配置".into()),
    };
    let original = read(path)?;
    let text = match &original {
        Some(bytes) if bytes.len() > 8 * 1024 * 1024 => return Err("客户端配置过大".into()),
        Some(bytes) => std::str::from_utf8(bytes)
            .map_err(|_| "客户端配置不是 UTF-8")?
            .trim_start_matches('\u{feff}'),
        None => "{}",
    };
    let mut data: Value = if client == "hermes" {
        serde_yaml_ng::from_str(text).map_err(|_| "YAML 配置格式无效，原文件已保留")?
    } else {
        serde_json::from_str(text)
            .map_err(|_| "需要标准 JSON 配置；含注释或 JSON5 的文件请使用手动配置，原文件已保留")?
    };
    let url = format!("http://127.0.0.1:{port}/mcp");
    let headers = json!({"Authorization": format!("Bearer {token}")});
    let astralink = match client {
        "claude-desktop" | "workbuddy" => {
            let (node, script) = bridge.ok_or("本机 MCP 桥接资源不存在")?;
            if !node.is_file() || !script.is_file() {
                return Err("本机 MCP 桥接资源不存在，请检查安装包".into());
            }
            json!({"command": node, "args": [script], "env": {
                "ASTRALINK_MCP_PORT": port.to_string(), "ASTRALINK_MCP_TOKEN": token
            }})
        }
        "claude-code" | "vscode" => json!({"type": "http", "url": url, "headers": headers}),
        "windsurf" | "antigravity" => json!({"serverUrl": url, "headers": headers}),
        "opencode" => {
            json!({"type": "remote", "url": url, "enabled": true, "oauth": false, "headers": headers})
        }
        "openclaw" => {
            json!({"url": url, "transport": "streamable-http", "enabled": true, "headers": headers})
        }
        _ => json!({"url": url, "headers": headers}),
    };
    let servers = object_mut(&mut data, keys)?;
    if servers
        .get("astralink")
        .is_some_and(|entry| !entry.is_object())
    {
        return Err("已有 AstraLink 配置必须是对象，原文件已保留".into());
    }
    let owned_previous = servers
        .get(crate::migration::LEGACY_CLIENT_KEY)
        .and_then(Value::as_object)
        .is_some_and(|entry| {
            entry.get("command").is_none()
                && entry
                    .get("url")
                    .or_else(|| entry.get("serverUrl"))
                    .and_then(Value::as_str)
                    .is_some_and(crate::migration::is_previous_local_endpoint)
        });
    if owned_previous {
        servers.remove(crate::migration::LEGACY_CLIENT_KEY);
    }
    if !owned_previous && servers.get("astralink") == Some(&astralink) {
        return Ok(CodexRegistration {
            config_path: path.to_string_lossy().into(),
            backup_path: None,
            reload_required: false,
        });
    }
    servers.insert("astralink".into(), astralink);
    let output = if client == "hermes" {
        serde_yaml_ng::to_string(&data).map_err(|_| "无法生成 YAML 配置")?
    } else {
        serde_json::to_string_pretty(&data).map_err(|_| "无法生成 JSON 配置")? + "\n"
    };
    let backup_path = if let Some(bytes) = &original {
        let file = path
            .file_name()
            .and_then(|p| p.to_str())
            .ok_or("配置路径无效")?;
        let backup =
            path.with_file_name(format!("{file}.astralink-backup-{}", uuid::Uuid::new_v4()));
        atomic_write(&backup, bytes)?;
        Some(backup.to_string_lossy().into())
    } else {
        None
    };
    if read(path)? != original {
        return Err("客户端配置已被其他程序修改，请重试".into());
    }
    atomic_write(path, output.as_bytes())?;
    Ok(CodexRegistration {
        config_path: path.to_string_lossy().into(),
        backup_path,
        reload_required: true,
    })
}
