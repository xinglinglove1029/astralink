use super::*;
pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let value: Value = serde_json::from_str(input).map_err(|_| "JSON 配置格式无效")?;
    let value = value.get("connections").unwrap_or(&value);
    let rows: Vec<(Option<&str>, &Value)> = match value {
        Value::Array(array) => array.iter().map(|v| (None, v)).collect(),
        Value::Object(map) if map.contains_key("host") => vec![(None, value)],
        Value::Object(map) => map
            .iter()
            .map(|(name, v)| (Some(name.as_str()), v))
            .collect(),
        _ => return Err("JSON 配置必须为连接对象或数组".into()),
    };
    if rows.len() > 512 {
        return Err("最多导入 512 个连接".into());
    }
    for (name, v) in rows {
        if !v.is_object() {
            unsupported(preview, "连接条目必须为对象", name);
            continue;
        }
        let name = text(v, &["name"]).or(name).unwrap_or("未命名连接");
        let mut p = new_profile(name, "mcp-json");
        p.host = text(v, &["host", "hostname"]).unwrap_or_default().into();
        p.username = text(v, &["username", "user"]).unwrap_or_default().into();
        p.port = match port(v.get("port")) {
            Ok(port) => port,
            Err(_) => {
                preview.stats.unsupported += 1;
                issue(preview, "error", "端口无效", Some(name));
                continue;
            }
        };
        p.group = text(v, &["group"]).unwrap_or_default().into();
        p.notes = text(v, &["notes"]).unwrap_or_default().into();
        p.password = optional(v, &["password"]);
        p.passphrase = optional(v, &["passphrase"]);
        p.agent = optional(v, &["agent"]);
        p.try_keyboard = v
            .get("tryKeyboard")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        p.enabled = v.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        if let Some(key) = text(v, &["privateKeyPath", "privateKey"]) {
            p.auth_type = AuthType::PrivateKey;
            if key.contains("-----BEGIN") || key.contains('\n') {
                disable(preview, &mut p, "内嵌私钥暂不支持，请选择本地私钥文件");
            } else {
                p.private_key_path = Some(path(key, source, home))
            }
        } else if p.agent.is_some() {
            p.auth_type = AuthType::Agent
        }
        if let Some(auth) = text(v, &["authType"]) {
            match auth {
                "password" => p.auth_type = AuthType::Password,
                "privateKey" => p.auth_type = AuthType::PrivateKey,
                "agent" => p.auth_type = AuthType::Agent,
                _ => disable(preview, &mut p, "认证方式暂不支持"),
            }
        }
        match text(v, &["transportMode"]) {
            None | Some("exec") => {}
            Some("shell") => p.transport_mode = TransportMode::Shell,
            _ => disable(preview, &mut p, "传输模式暂不支持"),
        }
        for (key, local) in [("allowedLocalPaths", true), ("allowedRemotePaths", false)] {
            if let Some(raw) = v.get(key) {
                if let Some(array) = raw.as_array() {
                    if array.iter().all(Value::is_string) {
                        let values = array
                            .iter()
                            .filter_map(Value::as_str)
                            .map(Into::into)
                            .collect();
                        if local {
                            p.allowed_local_paths = Some(values)
                        } else {
                            p.allowed_remote_paths = Some(values)
                        }
                    } else {
                        disable(preview, &mut p, "传输目录数组必须为字符串")
                    }
                } else {
                    disable(preview, &mut p, "传输目录必须为数组")
                }
            }
        }
        p.jump_host_id = optional(v, &["jumpHostId"]);
        p.target_host = optional(v, &["targetHost"]);
        p.target_username = optional(v, &["targetUsername"]);
        if let Some(id) = text(v, &["id"]) {
            if let Ok(id) = uuid::Uuid::parse_str(id) {
                p.id = id.to_string()
            }
        }
        for unsupported in [
            "proxyCommand",
            "proxy",
            "sock",
            "hostVerifier",
            "keyboardInteractive",
        ] {
            if v.get(unsupported)
                .is_some_and(|v| !v.is_null() && v != false)
            {
                disable(preview, &mut p, "代理或自定义认证选项暂不支持，请手动配置");
            }
        }
        normalize(p, preview);
    }
    let ids: HashSet<_> = preview.profiles.iter().map(|p| p.id.clone()).collect();
    for i in 0..preview.profiles.len() {
        if preview.profiles[i]
            .jump_host_id
            .as_ref()
            .is_some_and(|id| !ids.contains(id))
        {
            let name = preview.profiles[i].name.clone();
            preview.profiles[i].enabled = false;
            issue(preview, "error", "跳板连接引用未解析", Some(&name));
        }
    }
    Ok(())
}
