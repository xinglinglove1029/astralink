use super::*;
use std::collections::HashMap;
fn merge(base: &mut Value, extra: &Value) {
    if let (Some(a), Some(b)) = (base.as_object_mut(), extra.as_object()) {
        for (k, v) in b {
            if let Some(old) = a.get_mut(k) {
                merge(old, v)
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    } else {
        *base = extra.clone()
    }
}
pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    parse_with_reader(
        input,
        source,
        home,
        preview,
        &crate::tabby_credentials::WindowsCredentialReader,
    )
}
fn parse_with_reader(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
    reader: &dyn crate::tabby_credentials::CredentialReader,
) -> Result<(), String> {
    let data: Value = serde_yaml_ng::from_str(input).map_err(|_| "Tabby YAML 格式无效")?;
    let profiles = data
        .get("profiles")
        .and_then(Value::as_array)
        .ok_or("Tabby 配置缺少 profiles 数组")?;
    if profiles.len() > 512 {
        return Err("最多导入 512 个连接".into());
    }
    let defaults = data
        .pointer("/profileDefaults/ssh")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({}));
    let groups = data.get("groups").and_then(Value::as_array);
    let mut ids = HashMap::new();
    let mut jumps = Vec::new();
    for raw in profiles {
        if text(raw, &["type"]) != Some("ssh") || raw.get("isTemplate") == Some(&Value::Bool(true))
        {
            unsupported(preview, "仅导入 SSH 连接", text(raw, &["name"]));
            continue;
        }
        let mut full = defaults.clone();
        if let Some(group) = text(raw, &["group"]) {
            if let Some(groups) = groups {
                if let Some(group) = groups.iter().find(|g| text(g, &["id"]) == Some(group)) {
                    if let Some(d) = group.pointer("/defaults/ssh") {
                        merge(&mut full, d)
                    }
                }
            }
        }
        merge(&mut full, raw);
        let options = full.get("options").unwrap_or(&Value::Null);
        let mut p = new_profile(text(&full, &["name"]).unwrap_or("未命名连接"), "tabby");
        p.host = text(options, &["host"]).unwrap_or_default().into();
        // Tabby SSHProfilesService.configDefaults.options.user is root; explicit empty/null overrides it.
        p.username = match options.get("user").or_else(|| options.get("username")) {
            None => "root".into(),
            Some(Value::String(user)) => user.clone(),
            _ => String::new(),
        };
        p.port = match port(options.get("port")) {
            Ok(p) => p,
            Err(_) => {
                preview.stats.unsupported += 1;
                issue(preview, "error", "端口无效", Some(&p.name));
                continue;
            }
        };
        p.group = text(&full, &["group"]).unwrap_or_default().into();
        if let Some(groups) = groups {
            if let Some(g) = groups
                .iter()
                .find(|g| text(g, &["id"]) == Some(p.group.as_str()))
            {
                p.group = text(g, &["name"]).unwrap_or(&p.group).into()
            }
        }
        if let Some(password) = text(options, &["password"]) {
            if password.starts_with("vault:")
                || password.starts_with("encrypted:")
                || password.starts_with("safeStorage:")
                || password.starts_with("$")
            {
                issue(
                    preview,
                    "warning",
                    "Tabby 凭据库引用未解密，请补充密码",
                    Some(&p.name),
                )
            } else {
                p.password = Some(password.into())
            }
        } else if options.get("password").is_some_and(|p| !p.is_null()) {
            issue(
                preview,
                "warning",
                "Tabby 凭据库引用未解密，请补充密码",
                Some(&p.name),
            )
        }
        if vault_enabled(&data) {
            p.password = None;
        }
        if let Some(keys) = options.get("privateKeys").and_then(Value::as_array) {
            if let Some(key) = keys.first().and_then(Value::as_str) {
                p.auth_type = AuthType::PrivateKey;
                if key.starts_with("vault://") || key.contains("://") {
                    disable(
                        preview,
                        &mut p,
                        "Tabby 保险库私钥引用暂不支持，请选择本地私钥文件",
                    );
                } else {
                    p.private_key_path = Some(path(key, source, home));
                }
            } else if !keys.is_empty() {
                disable(preview, &mut p, "Tabby 内嵌或凭据库私钥暂不支持")
            }
            if keys.len() > 1 {
                issue(preview, "warning", "多把私钥仅使用第一把", Some(&p.name))
            }
        }
        match text(options, &["auth"]) {
            Some("agent") => {
                p.auth_type = AuthType::Agent;
                p.agent = std::env::var("SSH_AUTH_SOCK").ok()
            }
            Some("password") | None => {}
            Some("publicKey") | Some("privateKey") => p.auth_type = AuthType::PrivateKey,
            Some("keyboardInteractive") => p.try_keyboard = true,
            Some(_) => disable(preview, &mut p, "Tabby 认证方式暂不支持"),
        }
        if let Some(jump) = text(options, &["jumpHost"]) {
            jumps.push((p.id.clone(), jump.to_string()))
        }
        for key in ["proxyCommand", "socksProxyHost", "httpProxyHost"] {
            if options
                .get(key)
                .is_some_and(|v| !v.is_null() && v.as_str() != Some(""))
            {
                disable(preview, &mut p, "Tabby 代理配置暂不支持")
            }
        }
        if let Some(id) = text(raw, &["id"]) {
            ids.insert(id.to_string(), p.id.clone());
        }
        if p.auth_type == AuthType::Password && p.password.is_none() {
            if vault_enabled(&data) {
                issue(
                    preview,
                    "warning",
                    "Tabby 凭据库已启用，无法解密，请补充密码",
                    Some(&p.name),
                )
            } else {
                let host = text(options, &["host"]).unwrap_or(&p.host);
                let targets =
                    if options.get("port").is_none() || options.get("port") == Some(&Value::Null) {
                        vec![Some(p.port), None]
                    } else {
                        vec![Some(p.port)]
                    };
                let mut missing = true;
                for port in targets {
                    if let Ok(target) = crate::tabby_credentials::target(host, port, &p.username) {
                        match reader.read_utf8(&target) {
                            Ok(Some(password)) if !password.is_empty() => {
                                p.password = Some(password);
                                missing = false;
                                break;
                            }
                            Ok(_) => {}
                            Err(_) => {
                                issue(
                                    preview,
                                    "warning",
                                    "无法读取 Tabby 系统凭据，请补充密码",
                                    Some(&p.name),
                                );
                                break;
                            }
                        }
                    }
                }
                if missing {
                    issue(
                        preview,
                        "warning",
                        "Tabby 系统密码缺失，请补充认证",
                        Some(&p.name),
                    )
                }
            }
        }
        normalize(p, preview);
    }
    for (id, jump) in jumps {
        let Some(index) = preview.profiles.iter().position(|p| p.id == id) else {
            continue;
        };
        if let Some(target) = ids.get(&jump) {
            preview.profiles[index].jump_host_id = Some(target.clone())
        } else {
            preview.profiles[index].enabled = false;
            let name = preview.profiles[index].name.clone();
            issue(preview, "error", "Tabby 跳板连接引用未解析", Some(&name));
        }
    }
    if vault_enabled(&data) {
        issue(
            preview,
            "warning",
            "Tabby 凭据库未解密，缺失凭据需手动补充",
            None,
        )
    }
    Ok(())
}

fn vault_enabled(data: &Value) -> bool {
    match data.get("vault") {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::Object(map))
            if map.get("enabled") == Some(&Value::Bool(false)) && !map.contains_key("contents") =>
        {
            false
        }
        _ => true,
    }
}
#[cfg(test)]
mod credential_tests {
    use super::*;
    use std::{cell::RefCell, collections::HashMap};
    struct Reader {
        targets: RefCell<Vec<String>>,
        values: HashMap<String, String>,
    }
    impl crate::tabby_credentials::CredentialReader for Reader {
        fn read_utf8(&self, target: &str) -> Result<Option<String>, String> {
            self.targets.borrow_mut().push(target.into());
            Ok(self.values.get(target).cloned())
        }
    }
    fn preview() -> ImportPreview {
        ImportPreview {
            source: "tabby".into(),
            source_path: "fixture.yaml".into(),
            profiles: vec![],
            issues: vec![],
            stats: ImportStats::default(),
        }
    }
    #[test]
    fn imports_exact_keytar_password_without_serializing_it() {
        let reader = Reader {
            targets: RefCell::new(vec![]),
            values: HashMap::from([("ssh@db.test:2222/ops".into(), "utf8-密码-test".into())]),
        };
        let mut p = preview();
        parse_with_reader("vault: null\nprofiles:\n  - type: ssh\n    name: db\n    options: {host: db.test, port: 2222, user: ops}\n",Path::new("fixture.yaml"),Path::new("C:/Users/test"),&mut p,&reader).unwrap();
        assert_eq!(reader.targets.borrow().as_slice(), ["ssh@db.test:2222/ops"]);
        assert_eq!(p.profiles[0].password.as_deref(), Some("utf8-密码-test"));
        assert!(!serde_json::to_string(&p)
            .unwrap()
            .contains("utf8-密码-test"));
    }
    #[test]
    fn vault_private_key_reference_is_not_a_local_file() {
        let reader = Reader {
            targets: RefCell::new(vec![]),
            values: HashMap::new(),
        };
        let mut p = preview();
        parse_with_reader("profiles:\n  - type: ssh\n    name: vault-key\n    options: {host: db.test, user: ops, privateKeys: ['vault://synthetic-id']}\n",Path::new("fixture.yaml"),Path::new("C:/Users/test"),&mut p,&reader).unwrap();
        assert!(!p.profiles[0].enabled);
        assert!(p.profiles[0].private_key_path.is_none());
        assert!(p.issues.iter().any(|i| i.message.contains("私钥")));
    }
    #[test]
    fn enabled_vault_does_not_read_stale_windows_password() {
        let reader = Reader {
            targets: RefCell::new(vec![]),
            values: HashMap::from([("ssh@db.test:22/ops".into(), "stale".into())]),
        };
        let mut p = preview();
        parse_with_reader("vault: {contents: encrypted, version: 1}\nprofiles:\n  - type: ssh\n    name: db\n    options: {host: db.test, user: ops}\n",Path::new("fixture.yaml"),Path::new("C:/Users/test"),&mut p,&reader).unwrap();
        assert!(reader.targets.borrow().is_empty());
        assert!(p.profiles[0].password.is_none());
        assert!(p.issues.iter().any(|i| i.message.contains("无法解密")));
    }
}

#[cfg(test)]
mod builtin_default_tests {
    use super::*;
    struct Reader;
    impl crate::tabby_credentials::CredentialReader for Reader {
        fn read_utf8(&self, target: &str) -> Result<Option<String>, String> {
            Ok((target == "ssh@default.test:22/root").then(|| "synthetic-root".into()))
        }
    }
    fn fixture(input: &str) -> ImportPreview {
        let mut p = ImportPreview {
            source: "tabby".into(),
            source_path: "fixture".into(),
            profiles: vec![],
            issues: vec![],
            stats: ImportStats::default(),
        };
        parse_with_reader(
            input,
            Path::new("fixture.yaml"),
            Path::new("C:/Users/test"),
            &mut p,
            &Reader,
        )
        .unwrap();
        p
    }
    #[test]
    fn official_builtin_user_root_is_used_when_absent() {
        let p=fixture("profileDefaults:\n  ssh:\n    disableDynamicTitle: true\nprofiles:\n  - type: ssh\n    name: default\n    options: {host: default.test}\n");
        assert_eq!(p.profiles[0].username, "root");
        assert_eq!(p.profiles[0].password.as_deref(), Some("synthetic-root"));
    }
    #[test]
    fn explicitly_empty_user_is_not_replaced_with_root() {
        let p=fixture("profiles:\n  - type: ssh\n    name: empty\n    options: {host: default.test, user: ''}\n");
        assert!(p.profiles[0].username.is_empty());
        assert!(!p.profiles[0].enabled);
        assert!(p.profiles[0].password.is_none());
    }
}
