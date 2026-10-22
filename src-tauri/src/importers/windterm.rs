use super::*;
use aes::Aes256;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use pbkdf2::pbkdf2_hmac;
use sha3::Sha3_512;

struct Crypto {
    key: [u8; 32],
    iv: [u8; 16],
}

fn crypto(source: &Path, master: Option<&str>) -> Result<Option<Crypto>, String> {
    let Some(config) = source
        .parent()
        .and_then(Path::parent)
        .map(|parent| parent.join("user.config"))
    else {
        return Ok(None);
    };
    if !config.is_file() {
        return Ok(None);
    }
    let contents = read_text(&config, false)?;
    let data: Value =
        serde_json::from_str(&contents).map_err(|_| "WindTerm user.config 格式无效")?;
    let fingerprint = text(&data, &["application.fingerprint"]).ok_or("WindTerm 缺少应用指纹")?;
    if data.get("application.masterPassword") == Some(&Value::Bool(true))
        && master.unwrap_or_default().is_empty()
    {
        return Err("WindTerm 已启用主密码，请输入主密码后预览".into());
    }
    let mut material = [0u8; 48];
    pbkdf2_hmac::<Sha3_512>(
        master.unwrap_or_default().as_bytes(),
        fingerprint.as_bytes(),
        100_000,
        &mut material,
    );
    let mut key = [0u8; 32];
    key.copy_from_slice(&material[..32]);
    let mut iv = [0u8; 16];
    iv.copy_from_slice(&material[32..]);
    Ok(Some(Crypto { key, iv }))
}

fn auto_login(entry: &Value, crypto: Option<&Crypto>) -> Result<Option<Value>, String> {
    let Some(raw) = text(entry, &["session.autoLogin"]) else {
        return Ok(None);
    };
    if let Ok(value) = serde_json::from_str::<Value>(raw) {
        return Ok(value.is_object().then_some(value));
    }
    let Some(crypto) = crypto else {
        return Err("WindTerm 认证数据已加密，但缺少 user.config".into());
    };
    let mut bytes = STANDARD
        .decode(raw)
        .map_err(|_| "WindTerm 认证数据编码无效")?;
    let plain = cbc::Decryptor::<Aes256>::new(&crypto.key.into(), &crypto.iv.into())
        .decrypt_padded_mut::<Pkcs7>(&mut bytes)
        .map_err(|_| "WindTerm 主密码不正确或认证数据损坏")?;
    let value: Value = serde_json::from_slice(plain).map_err(|_| "WindTerm 认证数据格式无效")?;
    Ok(value.is_object().then_some(value))
}

fn target(value: &str) -> (String, String) {
    let value = value.trim();
    if let Some((user, host)) = value.rsplit_once('@') {
        if !user.is_empty() && !host.is_empty() {
            return (host.into(), user.into());
        }
    }
    (value.into(), "root".into())
}

fn key_path(raw: &str, source: &Path, home: &Path) -> String {
    for variable in ["$(HomeDir)", "${HomeDir}"] {
        if let Some(rest) = raw.trim().strip_prefix(variable) {
            return home
                .join(rest.trim_start_matches(['/', '\\']))
                .to_string_lossy()
                .into_owned();
        }
    }
    let home_text = home.to_string_lossy();
    let expanded = raw
        .trim()
        .replace("$(HomeDir)", &home_text)
        .replace("${HomeDir}", &home_text);
    path(&expanded, source, home)
}

pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    master: Option<&str>,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let entries: Value =
        serde_json::from_str(input).map_err(|_| "WindTerm user.sessions 格式无效")?;
    let entries = entries.as_array().ok_or("WindTerm 会话文件应为数组")?;
    if entries.len() > 512 {
        return Err("最多导入 512 个 WindTerm 会话".into());
    }
    let crypto = crypto(source, master)?;
    for entry in entries {
        if !text(entry, &["session.protocol"])
            .is_some_and(|value| value.eq_ignore_ascii_case("SSH"))
        {
            unsupported(
                preview,
                "仅导入 WindTerm SSH 会话",
                text(entry, &["session.label"]),
            );
            continue;
        }
        let (host, username) = target(text(entry, &["session.target"]).unwrap_or_default());
        let mut p = new_profile(text(entry, &["session.label"]).unwrap_or(&host), "windterm");
        p.host = host;
        p.username = username;
        p.group = text(entry, &["session.group"])
            .unwrap_or_default()
            .split('>')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("/");
        p.notes = text(entry, &["session.description"])
            .unwrap_or_default()
            .into();
        match port(entry.get("session.port")) {
            Ok(number) => p.port = number,
            Err(_) => {
                unsupported(preview, "WindTerm 端口无效", Some(&p.name));
                continue;
            }
        }
        let login = match auto_login(entry, crypto.as_ref()) {
            Ok(login) => login,
            Err(_) => {
                issue(
                    preview,
                    "warning",
                    "WindTerm 自动登录凭据无法解密，请补充凭据",
                    Some(&p.name),
                );
                None
            }
        };
        if let Some(user) = login
            .as_ref()
            .and_then(|data| text(data, &["session.user"]))
        {
            p.username = user.into();
        }
        if login.as_ref().and_then(|data| data.get("PasswordEnabled")) == Some(&Value::Bool(true)) {
            p.password = login
                .as_ref()
                .and_then(|data| text(data, &["Password"]).map(Into::into));
        }
        let key = login
            .as_ref()
            .and_then(|data| {
                data.pointer("/Public Key/windows.path")
                    .and_then(Value::as_str)
            })
            .or_else(|| text(entry, &["ssh.identityFilePath.windows"]));
        if p.password.is_none() {
            if let Some(key) = key {
                p.auth_type = AuthType::PrivateKey;
                p.private_key_path = Some(key_path(key, source, home));
                if !p
                    .private_key_path
                    .as_ref()
                    .is_some_and(|path| Path::new(path).is_file())
                {
                    disable(
                        preview,
                        &mut p,
                        "WindTerm 私钥文件不存在，请重新选择本地私钥",
                    );
                }
            } else {
                issue(
                    preview,
                    "warning",
                    "WindTerm 会话需要补充凭据",
                    Some(&p.name),
                );
            }
        }
        normalize(p, preview);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use cbc::cipher::BlockEncryptMut;

    #[test]
    fn home_directory_key_placeholders_resolve_without_copying_private_keys() {
        let dir = tempfile::tempdir().unwrap();
        let key = dir.path().join("id_ed25519");
        fs::write(&key, "fixture-key").unwrap();
        let sessions = dir.path().join("user.sessions");
        let content = r#"[{"session.protocol":"SSH","session.target":"ops@web.example.org","session.label":"Web","ssh.identityFilePath.windows":"$(HomeDir)/id_ed25519"}]"#;
        let preview = super::super::parse("windterm", content, &sessions, dir.path()).unwrap();
        assert_eq!(preview.profiles.len(), 1);
        assert_eq!(
            preview.profiles[0].private_key_path.as_deref(),
            key.to_str()
        );
        assert!(preview.profiles[0].enabled);
    }

    #[test]
    fn encrypted_auto_login_requires_master_password_and_stays_in_preview_memory() {
        let dir = tempfile::tempdir().unwrap();
        let terminal = dir.path().join("terminal");
        fs::create_dir_all(&terminal).unwrap();
        fs::write(dir.path().join("user.config"), r#"{"application.fingerprint":"fixture-fingerprint","application.masterPassword":true}"#).unwrap();
        let sessions = terminal.join("user.sessions");
        let crypto = crypto(&sessions, Some("fixture-master")).unwrap().unwrap();
        let plain =
            r#"{"PasswordEnabled":true,"Password":"fixture-secret","session.user":"deploy"}"#;
        let mut buffer = vec![0u8; plain.len() + 16];
        buffer[..plain.len()].copy_from_slice(plain.as_bytes());
        let encrypted = cbc::Encryptor::<Aes256>::new(&crypto.key.into(), &crypto.iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buffer, plain.len())
            .unwrap();
        let content = serde_json::json!([{
            "session.protocol":"SSH", "session.target":"web.example.org", "session.label":"Web",
            "session.autoLogin":STANDARD.encode(encrypted)
        }])
        .to_string();
        fs::write(&sessions, content).unwrap();
        assert!(
            super::super::preview_file("windterm", &sessions, dir.path())
                .err()
                .is_some_and(|e| e.contains("主密码"))
        );
        let preview = super::super::preview_file_with_password(
            "windterm",
            &sessions,
            dir.path(),
            Some("fixture-master"),
        )
        .unwrap();
        assert_eq!(preview.profiles[0].username, "deploy");
        assert_eq!(
            preview.profiles[0].password.as_deref(),
            Some("fixture-secret")
        );
        let wrong = super::super::preview_file_with_password(
            "windterm",
            &sessions,
            dir.path(),
            Some("wrong-password"),
        )
        .unwrap();
        assert!(wrong.profiles[0].password.is_none());
        assert!(wrong
            .issues
            .iter()
            .any(|issue| issue.message.contains("无法解密")));
    }
}
