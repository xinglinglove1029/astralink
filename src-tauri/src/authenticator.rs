use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use argon2::{Algorithm, Argon2, Params, PasswordHash, PasswordVerifier, Version};
use base64::{
    engine::general_purpose::{STANDARD, STANDARD_NO_PAD},
    Engine as _,
};
use md5::{Digest, Md5};
use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use zeroize::{Zeroize, Zeroizing};

const INVALID: &str = "认证器备份格式无效或文件已损坏";
const PASSWORD: &str = "备份密码错误或文件已损坏";
const LOCKED: &str = "备份已加密，请输入导出时使用的密码";
const MAX_SIZE: usize = 1024 * 1024;
static DECRYPTIONS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    pub id: String,
    pub issuer: String,
    pub account: String,
    pub supported: bool,
    pub reason: Option<String>,
}

#[derive(Serialize)]
pub struct Preview {
    pub accounts: Vec<Account>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub secret: Option<String>,
}

/// No source file or decrypted backup is persisted. Preview contains metadata only.
#[tauri::command]
pub async fn read_authenticator_backup(
    content: String,
    password: Option<String>,
    entry_id: Option<String>,
) -> Result<Preview, String> {
    let content = Zeroizing::new(content);
    let password = password.map(Zeroizing::new);
    if content.len() > MAX_SIZE {
        return Err("备份文件不能超过 1 MB".into());
    }
    let permit = DECRYPTIONS
        .try_acquire()
        .map_err(|_| "正在处理其他备份，请稍后重试")?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        parse(
            &content,
            password.as_ref().map(|p| p.as_str()),
            entry_id.as_deref(),
        )
    })
    .await
    .map_err(|_| INVALID.to_string())?;
    result
}

fn seed(value: &str) -> Result<String, String> {
    let seed: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .trim_end_matches('=')
        .to_ascii_uppercase();
    if !(16..=128).contains(&seed.len())
        || !seed
            .bytes()
            .all(|c| c.is_ascii_uppercase() || (b'2'..=b'7').contains(&c))
        || !matches!(seed.len() % 8, 0 | 2 | 4 | 5 | 7)
    {
        return Err("动态密钥无效".into());
    }
    let alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    if !seed.chars().all(|c| alphabet.contains(c)) {
        return Err("动态密钥无效".into());
    }
    let remaining = seed.len() % 8 * 5 % 8;
    let last = alphabet
        .find(seed.chars().last().ok_or(INVALID)?)
        .ok_or(INVALID)?;
    if remaining > 0 && last % (1 << remaining) != 0 {
        return Err("动态密钥无效".into());
    }
    Ok(seed)
}

// CryptoJS/OpenSSL passphrase envelope: EVP_BytesToKey(MD5), AES-256-CBC.
fn decrypt(ciphertext: &str, passphrase: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    let mut bytes = Zeroizing::new(STANDARD.decode(ciphertext).map_err(|_| PASSWORD)?);
    if bytes.len() < 32 || &bytes[..8] != b"Salted__" || (bytes.len() - 16) % 16 != 0 {
        return Err(PASSWORD.into());
    }
    let salt = &bytes[8..16];
    let mut key = Zeroizing::new(Vec::with_capacity(48));
    let mut previous = Zeroizing::new(Vec::new());
    while key.len() < 48 {
        let mut digest = Md5::new();
        digest.update(&*previous);
        digest.update(passphrase.as_bytes());
        digest.update(salt);
        previous.clear();
        previous.extend_from_slice(&digest.finalize());
        key.extend_from_slice(&previous);
    }
    let decoded = cbc::Decryptor::<aes::Aes256>::new_from_slices(&key[..32], &key[32..48])
        .map_err(|_| PASSWORD)?
        .decrypt_padded_mut::<Pkcs7>(&mut bytes[16..])
        .map_err(|_| PASSWORD)?;
    Ok(Zeroizing::new(decoded.to_vec()))
}

fn text(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(256)
        .collect()
}

fn unlock_key(value: &Value, password: &str) -> Result<Zeroizing<String>, String> {
    if let Some(enc) = value.get("enc").and_then(Value::as_str) {
        let bytes = decrypt(enc, password)?;
        let mut key = Zeroizing::new(String::with_capacity(bytes.len() * 2));
        const HEX: &[u8; 16] = b"0123456789abcdef";
        for byte in bytes.iter() {
            key.push(HEX[(byte >> 4) as usize] as char);
            key.push(HEX[(byte & 0x0f) as usize] as char);
        }
        verify_key(value, &key)?;
        return Ok(key);
    }
    let salt = value.get("salt").and_then(Value::as_str).ok_or(INVALID)?;
    if !(8..=128).contains(&salt.len()) {
        return Err(INVALID.into());
    }
    let params = Params::new(19 * 1024, 2, 1, Some(32)).map_err(|_| INVALID)?;
    let mut derived = Zeroizing::new([0u8; 32]);
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(password.as_bytes(), salt.as_bytes(), &mut *derived)
        .map_err(|_| PASSWORD)?;
    let key = Zeroizing::new(STANDARD_NO_PAD.encode(&*derived));
    verify_key(value, &key)?;
    Ok(key)
}

fn verify_key(value: &Value, key: &str) -> Result<(), String> {
    let encoded = value.get("hash").and_then(Value::as_str).ok_or(INVALID)?;
    let hash = PasswordHash::new(encoded).map_err(|_| INVALID)?;
    if hash.algorithm.as_str() != "argon2id" || hash.version != Some(19) {
        return Err("备份加密算法不受支持".into());
    }
    let memory = hash.params.get_decimal("m").ok_or(INVALID)?;
    let time = hash.params.get_decimal("t").ok_or(INVALID)?;
    let lanes = hash.params.get_decimal("p").ok_or(INVALID)?;
    if memory > 65536 || time > 4 || lanes > 4 {
        return Err("备份加密参数超出支持范围".into());
    }
    Argon2::default()
        .verify_password(key.as_bytes(), &hash)
        .map_err(|_| PASSWORD)?;
    Ok(())
}

fn record(
    value: &Value,
    id: String,
    secrets: &mut HashMap<String, Zeroizing<String>>,
    root: &Value,
    password: Option<&str>,
) -> Result<(Account, Option<Zeroizing<String>>), String> {
    let owned;
    let mut value = value;
    if value.get("dataType").and_then(Value::as_str) == Some("EncOTPStorage") {
        let password = password.filter(|p| !p.is_empty()).ok_or(LOCKED)?;
        let key_id = value.get("keyId").and_then(Value::as_str).ok_or(INVALID)?;
        if !secrets.contains_key(key_id) {
            if secrets.len() >= 8 {
                return Err("备份中的加密密钥过多".into());
            }
            let key = root
                .get(key_id)
                .filter(|v| {
                    v.get("dataType").and_then(Value::as_str) == Some("Key")
                        && v.get("id").and_then(Value::as_str) == Some(key_id)
                })
                .ok_or(INVALID)?;
            secrets.insert(key_id.into(), unlock_key(key, password)?);
        }
        let decrypted = decrypt(
            value.get("data").and_then(Value::as_str).ok_or(INVALID)?,
            &secrets[key_id],
        )?;
        owned = SecretValue(serde_json::from_slice::<Value>(&decrypted).map_err(|_| PASSWORD)?);
        value = &owned.0;
    }
    let kind = value.get("type").and_then(Value::as_str).unwrap_or("");
    let supported = kind.eq_ignore_ascii_case("totp")
        && value
            .get("algorithm")
            .is_none_or(|v| v.as_str().is_some_and(|s| s.eq_ignore_ascii_case("SHA1")))
        && value.get("digits").is_none_or(|v| v.as_u64() == Some(6))
        && value.get("period").is_none_or(|v| v.as_u64() == Some(30));
    let mut raw = Zeroizing::new(
        value
            .get("secret")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    );
    if value.get("encrypted").and_then(Value::as_bool) == Some(true) {
        let password = password.filter(|p| !p.is_empty()).ok_or(LOCKED)?;
        let key_id = value.get("keyId").and_then(Value::as_str);
        let key = if let Some(id) = key_id {
            if !secrets.contains_key(id) {
                if secrets.len() >= 8 {
                    return Err("备份中的加密密钥过多".into());
                }
                secrets.insert(
                    id.into(),
                    unlock_key(root.get(id).ok_or(INVALID)?, password)?,
                );
            }
            Some(secrets.get(id).ok_or(INVALID)?)
        } else if root.get("key").is_some_and(|v| v.get("enc").is_some()) {
            if !secrets.contains_key("legacy") {
                secrets.insert("legacy".into(), unlock_key(&root["key"], password)?);
            }
            Some(secrets.get("legacy").ok_or(INVALID)?)
        } else {
            None
        };
        let decrypted = decrypt(&raw, key.map(|k| k.as_str()).unwrap_or(password))?;
        raw = Zeroizing::new(String::from_utf8(decrypted.to_vec()).map_err(|_| PASSWORD)?);
    }
    let (secret, reason) = if supported {
        match seed(&raw) {
            Ok(s) => (Some(Zeroizing::new(s)), None),
            Err(e) => (None, Some(e)),
        }
    } else {
        (None, Some("仅支持 SHA1、6 位、30 秒的 TOTP 账号".into()))
    };
    let account = Account {
        id,
        issuer: text(value, "issuer"),
        account: text(value, "account"),
        supported: secret.is_some(),
        reason,
    };
    Ok((account, secret))
}

fn scrub(value: &mut Value) {
    match value {
        Value::String(s) => s.zeroize(),
        Value::Array(a) => a.iter_mut().for_each(scrub),
        Value::Object(o) => o.values_mut().for_each(scrub),
        _ => {}
    }
}

struct SecretValue(Value);
impl Drop for SecretValue {
    fn drop(&mut self) {
        scrub(&mut self.0);
    }
}

fn parse(content: &str, password: Option<&str>, entry_id: Option<&str>) -> Result<Preview, String> {
    if content.len() > MAX_SIZE {
        return Err("备份文件不能超过 1 MB".into());
    }
    if password.is_some_and(|p| p.len() > 1024) {
        return Err("备份密码过长".into());
    }
    let content = content.trim_start_matches('\u{feff}').trim();
    let mut root = if content.starts_with('{') {
        serde_json::from_str::<Value>(content).map_err(|_| INVALID)?
    } else {
        Value::Null
    };
    let mut entries = Vec::new();
    let result = (|| {
        let mut secrets = HashMap::new();
        if let Some(object) = root.as_object() {
            for (name, value) in object {
                if name.len() > 128 {
                    return Err(INVALID.into());
                }
                if name == "key"
                    || name == "UserSettings"
                    || value.get("dataType").and_then(Value::as_str) == Some("Key")
                {
                    continue;
                }
                if !value.is_object() {
                    return Err(INVALID.into());
                }
                if entries.len() >= 256 {
                    return Err("一次最多导入 256 个账号".into());
                }
                entries.push(record(
                    value,
                    entries.len().to_string(),
                    &mut secrets,
                    &root,
                    password,
                )?);
            }
        } else {
            for line in content.lines().map(str::trim).filter(|s| !s.is_empty()) {
                if entries.len() >= 256 {
                    return Err("一次最多导入 256 个账号".into());
                }
                let url = reqwest::Url::parse(line).map_err(|_| INVALID)?;
                if url.scheme() != "otpauth" || !matches!(url.host_str(), Some("totp" | "hotp")) {
                    return Err(INVALID.into());
                }
                let label = decode_label(url.path().trim_start_matches('/'))?;
                let (issuer, account) = label.split_once(':').unwrap_or(("", &label));
                let mut value = SecretValue(
                    serde_json::json!({"type":url.host_str(),"issuer":issuer,"account":account}),
                );
                let mut seen = std::collections::HashSet::new();
                for (k, v) in url.query_pairs() {
                    if !seen.insert(k.to_string()) {
                        return Err(INVALID.into());
                    }
                    if matches!(&*k, "digits" | "period") {
                        value.0[k.as_ref()] = Value::from(v.parse::<u64>().map_err(|_| INVALID)?);
                    } else if matches!(&*k, "secret" | "issuer" | "algorithm") {
                        value.0[k.as_ref()] = Value::from(v.as_ref());
                    }
                }
                if let Some(url_issuer) = value
                    .0
                    .get("issuer")
                    .and_then(Value::as_str)
                    .filter(|s| !s.is_empty())
                {
                    let label_issuer = label.split_once(':').map(|(i, _)| i).unwrap_or("");
                    if !label_issuer.is_empty() && label_issuer != url_issuer {
                        return Err(INVALID.into());
                    }
                }
                let item = record(
                    &value.0,
                    entries.len().to_string(),
                    &mut secrets,
                    &root,
                    password,
                );
                entries.push(item?);
            }
        }
        if entries.is_empty() {
            return Err("备份中没有认证器账号".into());
        }
        if let Some(id) = entry_id {
            let selected = entries
                .iter_mut()
                .find(|(a, _)| a.id == id)
                .ok_or("请选择有效的认证器账号")?;
            let secret = selected.1.take().ok_or("此账号类型暂不支持")?;
            Ok(Preview {
                accounts: vec![],
                secret: Some(secret.to_string()),
            })
        } else {
            Ok(Preview {
                accounts: entries.drain(..).map(|(a, _)| a).collect(),
                secret: None,
            })
        }
    })();
    scrub(&mut root);
    result
}

fn decode_label(value: &str) -> Result<String, String> {
    let bytes = value.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err(INVALID.into());
            }
            let hex = |b: u8| (b as char).to_digit(16).map(|v| v as u8).ok_or(INVALID);
            out.push(hex(bytes[i + 1])? * 16 + hex(bytes[i + 2])?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| INVALID.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    const SEED: &str = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

    #[test]
    fn preview_redacts_secrets_and_selection_is_explicit_for_same_issuer() {
        let content = format!("otpauth://totp/JumpServer:alice?secret={SEED}\notpauth://totp/JumpServer:bob?secret={SEED}");
        let preview = parse(&content, None, None).unwrap();
        assert_eq!(preview.accounts.len(), 2);
        assert_eq!(preview.accounts[0].account, "alice");
        assert_eq!(preview.accounts[1].account, "bob");
        assert!(!serde_json::to_string(&preview).unwrap().contains(SEED));
        assert_eq!(
            parse(&content, None, Some("1")).unwrap().secret.as_deref(),
            Some(SEED)
        );
        assert!(parse(&content, None, Some("9")).is_err());
    }

    #[test]
    fn json_keeps_unsupported_accounts_visible_without_importing_them() {
        let content = serde_json::json!({"a":{"type":"totp","issuer":"JumpServer","account":"alice","secret":SEED},"b":{"type":"hotp","account":"counter","secret":SEED}}).to_string();
        let result = parse(&content, None, None).unwrap();
        assert!(result.accounts[0].supported);
        assert!(!result.accounts[1].supported);
        assert!(parse(&content, None, Some("1")).is_err());
    }

    #[test]
    fn malformed_and_oversized_backups_return_bounded_errors() {
        for content in [
            "sensitive-invalid-backup".to_string(),
            "[1,2]".to_string(),
            "x".repeat(1024 * 1024 + 1),
        ] {
            let error = parse(&content, None, None).err().unwrap();
            assert!(!error.contains("sensitive-invalid-backup"));
        }
    }

    #[test]
    fn encrypted_exports_match_independent_cryptojs_and_argon2_fixtures() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../test-fixtures/authenticator-synthetic.json"
        ))
        .unwrap();
        for kind in ["legacy", "modern", "modernSecret", "oldKey"] {
            let input = fixture[kind].to_string();
            let password = fixture["password"].as_str();
            assert!(parse(&input, None, None).err().unwrap().contains("已加密"));
            assert!(parse(&input, Some("wrong-test-password"), None).is_err());
            let preview = parse(&input, password, None).unwrap();
            assert_eq!(preview.accounts.len(), 1);
            assert!(preview.accounts[0].supported);
            assert!(!serde_json::to_string(&preview).unwrap().contains(SEED));
            assert_eq!(
                parse(&input, password, Some("0"))
                    .unwrap()
                    .secret
                    .as_deref(),
                Some(SEED)
            );
        }
    }

    #[test]
    fn backup_with_untrusted_kdf_or_nonstandard_otp_is_rejected_safely() {
        let mut fixture: Value = serde_json::from_str(include_str!(
            "../test-fixtures/authenticator-synthetic.json"
        ))
        .unwrap();
        fixture["modern"]["fixture-key"]["hash"] =
            Value::from("$argon2id$v=19$m=999999999,t=2,p=1$c2FsdC1zYWx0$MTIzNDU2Nzg");
        assert!(parse(
            &fixture["modern"].to_string(),
            fixture["password"].as_str(),
            None
        )
        .is_err());
        for extra in ["&algorithm=SHA256", "&digits=8", "&period=60"] {
            let result = parse(
                &format!("otpauth://totp/test?secret={SEED}{extra}"),
                None,
                None,
            )
            .unwrap();
            assert!(!result.accounts[0].supported);
        }
        assert!(parse(&format!("otpauth://totp/%中?secret={SEED}"), None, None).is_err());
        assert!(parse(
            &format!("otpauth://totp/test?secret={SEED}&secret={SEED}"),
            None,
            None
        )
        .is_err());
    }
}
