// Adapted from NyaTerm Termius importer, MIT License, Copyright (c) 2026 Kang.
// Source commit: 6af18f59b865d5eae596876ffc159acd14826abb. See docs/licenses/nyaterm-MIT.txt.
use super::*;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::{Key, Nonce, XSalsa20Poly1305};
use rusty_leveldb::LdbIterator;
use std::collections::{BTreeMap, HashMap};
use zeroize::{Zeroize, Zeroizing};
fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.filter(|s| !s.trim().is_empty())
}

fn local_key() -> Result<Zeroizing<[u8; 32]>, String> {
    let mut candidates = Vec::new();
    #[cfg(windows)]
    if let Ok(entry) = keyring::Entry::new_with_target("Termius/localKey", "Termius", "localKey") {
        if let Ok(secret) = entry.get_secret() {
            candidates.push(Zeroizing::new(secret));
        }
    }
    for (service, user) in [("Termius/localKey", "localKey"), ("Termius", "localKey")] {
        if let Ok(entry) = keyring::Entry::new(service, user) {
            if let Ok(secret) = entry.get_secret() {
                candidates.push(Zeroizing::new(secret));
            }
        }
    }
    for candidate in &candidates {
        if let Some(key) = normalize_key(candidate, 0) {
            return Ok(Zeroizing::new(key));
        }
    }
    Err("无法读取当前用户的 Termius localKey，请先登录本机 Termius".into())
}

fn normalize_key(bytes: &[u8], depth: usize) -> Option<[u8; 32]> {
    if depth > 4 || bytes.len() > 4096 {
        return None;
    }
    if bytes.len() == 32 {
        return bytes.try_into().ok();
    }
    if bytes.len() % 2 == 0 {
        let units: Vec<_> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .collect();
        if let Ok(text) = String::from_utf16(&units) {
            if let Some(key) = normalize_key(text.as_bytes(), depth + 1) {
                return Some(key);
            }
        }
    }
    let raw = std::str::from_utf8(bytes).ok()?.trim();
    if let Ok(decoded) = STANDARD.decode(raw) {
        if decoded.len() == 32 {
            return decoded.as_slice().try_into().ok();
        }
    }
    if raw.len() == 64 && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        let mut key = [0u8; 32];
        for (index, pair) in raw.as_bytes().chunks_exact(2).enumerate() {
            key[index] = u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok()?;
        }
        return Some(key);
    }
    if let Ok(data) = serde_json::from_str::<Value>(raw) {
        if let Some(value) = data.as_str() {
            return normalize_key(value.as_bytes(), depth + 1);
        }
        for name in ["localKey", "key", "secret", "value"] {
            if let Some(value) = data.get(name).and_then(Value::as_str) {
                if let Some(key) = normalize_key(value.as_bytes(), depth + 1) {
                    return Some(key);
                }
            }
        }
    }
    None
}

fn records(source: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    let source = if source.join("file__0.indexeddb.leveldb").is_dir() {
        source.join("file__0.indexeddb.leveldb")
    } else {
        source.to_path_buf()
    };
    if !source.is_dir() || !source.join("CURRENT").is_file() {
        return Err("请选择 Termius IndexedDB 中的 file__0.indexeddb.leveldb 目录".into());
    }
    let temp = tempfile::Builder::new()
        .prefix("astralink-termius-")
        .tempdir()
        .map_err(|_| "无法创建 Termius 读取副本")?;
    let mut bytes = 0u64;
    let mut count = 0usize;
    for entry in fs::read_dir(&source).map_err(|_| "无法读取 Termius IndexedDB")? {
        let entry = entry.map_err(|_| "无法读取 Termius 文件")?;
        let ty = entry.file_type().map_err(|_| "无法读取 Termius 文件类型")?;
        if !ty.is_file() || ty.is_symlink() {
            continue;
        }
        let size = entry.metadata().map_err(|_| "无法读取 Termius 文件")?.len();
        bytes += size;
        count += 1;
        if bytes > 128 * 1024 * 1024 || count > 1024 {
            return Err("Termius 数据库超过读取限制".into());
        }
        fs::copy(entry.path(), temp.path().join(entry.file_name()))
            .map_err(|_| "无法复制 Termius 数据库，请退出 Termius 后重试")?;
    }
    let options = rusty_leveldb::Options {
        create_if_missing: false,
        ..Default::default()
    };
    let mut db = rusty_leveldb::DB::open(temp.path(), options)
        .map_err(|_| "Termius 数据库副本无效或版本不兼容")?;
    let mut iter = db.new_iter().map_err(|_| "无法读取 Termius 数据库副本")?;
    let mut result = Zeroizing::new(Vec::new());
    iter.seek_to_first();
    while iter.valid() {
        let mut key = Vec::new();
        let mut value = Vec::new();
        if iter.current(&mut key, &mut value) {
            if result.len() + key.len() + value.len() + 2 > 128 * 1024 * 1024 {
                return Err("Termius 记录超过读取限制".into());
            }
            result.extend_from_slice(&key);
            result.push(0);
            result.extend_from_slice(&value);
            result.push(0);
        }
        iter.advance();
    }
    Ok(result)
}

pub(super) fn database(path: &Path) -> Result<ImportPreview, String> {
    let bytes = records(path)?;
    let key = local_key()?;
    parse_records(&bytes, &key, path)
}

fn parse_records(bytes: &[u8], key: &[u8; 32], path: &Path) -> Result<ImportPreview, String> {
    let mut values = parse_tagged_values(bytes);
    decrypt_termius_values(&mut values, key)
        .map_err(|_| "Termius 凭据解密失败，请确认当前用户与数据匹配")?;
    let store = collect_termius_store(&values);
    let hosts = dedupe_latest_hosts(store.hosts);
    let identities = dedupe_latest_identities(store.identities);
    let ssh_configs = dedupe_latest_ssh_configs(store.ssh_configs);
    let groups = dedupe_latest_groups(store.groups);
    let keys = dedupe_latest_ssh_keys(store.ssh_keys);
    let key_aliases: HashSet<String> = keys
        .iter()
        .flat_map(|item| termius_record_aliases(&item.id, item.local_id.as_deref()))
        .collect();
    if hosts.len() > 512 {
        return Err("Termius 主机超过 512 个".into());
    }
    let group_paths = build_termius_group_paths(&groups);
    let mut configs = HashMap::new();
    for config in &ssh_configs {
        for alias in termius_record_aliases(&config.id, config.local_id.as_deref()) {
            configs.insert(alias, config);
        }
    }
    let mut accounts = HashMap::new();
    for identity in &identities {
        for alias in termius_record_aliases(&identity.id, identity.local_id.as_deref()) {
            accounts.insert(alias, identity);
        }
    }
    let mut preview = ImportPreview {
        source: "termius".into(),
        source_path: path.to_string_lossy().into(),
        profiles: Vec::new(),
        issues: Vec::new(),
        stats: ImportStats::default(),
    };
    for host in hosts {
        let Some(address) = normalize_optional_string(host.address) else {
            continue;
        };
        let config = host
            .ssh_config_id
            .as_deref()
            .and_then(|id| configs.get(id).copied());
        let account = host
            .identity_id
            .as_deref()
            .or_else(|| config.and_then(|c| c.identity_id.as_deref()))
            .and_then(|id| accounts.get(id).copied());
        let mut p = new_profile(
            host.label
                .as_deref()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or(&address),
            "termius",
        );
        p.host = address;
        p.username = host
            .username
            .or_else(|| account.and_then(|a| a.username.clone()))
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| "root".into());
        p.port = host
            .port
            .or_else(|| config.and_then(|c| c.port))
            .filter(|p| *p > 0)
            .unwrap_or(22);
        p.password = host
            .password
            .or_else(|| account.and_then(|a| a.password.clone()))
            .filter(|s| !s.is_empty());
        p.group = host
            .group_id
            .as_deref()
            .and_then(|id| group_paths.get(id))
            .map(|p| p.join("/"))
            .unwrap_or_default();
        if let Some(reference) = account.and_then(|a| a.ssh_key_id.as_deref()) {
            p.auth_type = AuthType::PrivateKey;
            disable(
                &mut preview,
                &mut p,
                if key_aliases.contains(reference) {
                    "Termius 内联私钥不写入文件，请选择本地私钥"
                } else {
                    "Termius 私钥引用未解析，请选择本地私钥"
                },
            );
        } else if p.password.is_none() {
            issue(
                &mut preview,
                "warning",
                "Termius 连接需要补充密码",
                Some(&p.name),
            );
        }
        normalize(p, &mut preview);
    }
    finalize(&mut preview);
    Ok(preview)
}
#[derive(Clone, Default)]
struct TermiusRawStore {
    hosts: Vec<TermiusRawHost>,
    ssh_configs: Vec<TermiusRawSshConfig>,
    identities: Vec<TermiusRawIdentity>,
    ssh_keys: Vec<TermiusRawSshKey>,
    groups: Vec<TermiusRawGroup>,
}

#[derive(Clone, Default)]
struct TermiusRawHost {
    id: String,
    local_id: Option<String>,
    label: Option<String>,
    address: Option<String>,
    username: Option<String>,
    password: Option<String>,
    ssh_config_id: Option<String>,
    identity_id: Option<String>,
    group_id: Option<String>,
    port: Option<u16>,
    updated_at: Option<String>,
}

#[derive(Clone, Default)]
struct TermiusRawSshConfig {
    id: String,
    local_id: Option<String>,
    identity_id: Option<String>,
    port: Option<u16>,
    updated_at: Option<String>,
}

#[derive(Clone, Default)]
struct TermiusRawIdentity {
    id: String,
    local_id: Option<String>,
    username: Option<String>,
    password: Option<String>,
    ssh_key_id: Option<String>,
    updated_at: Option<String>,
}

#[derive(Clone, Default)]
struct TermiusRawSshKey {
    id: String,
    local_id: Option<String>,
    updated_at: Option<String>,
}

#[derive(Clone, Default)]
struct TermiusRawGroup {
    id: String,
    local_id: Option<String>,
    label: Option<String>,
    parent_id: Option<String>,
    updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TermiusTaggedValue {
    String(String),
    Integer(i64),
}

impl TermiusTaggedValue {
    fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(value) => Some(value),
            Self::Integer(_) => None,
        }
    }

    fn to_field_value(&self) -> String {
        match self {
            Self::String(value) => value.clone(),
            Self::Integer(value) => value.to_string(),
        }
    }
}

fn parse_tagged_values(bytes: &[u8]) -> Vec<TermiusTaggedValue> {
    let mut strings = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'I' {
            if let Some((value, consumed)) = read_v8_signed_integer(&bytes[index + 1..]) {
                strings.push(TermiusTaggedValue::Integer(value));
                index += 1 + consumed;
                continue;
            }
        }

        if bytes[index] != b'"' {
            index += 1;
            continue;
        }
        let Some((length, consumed)) = read_v8_varint(&bytes[index + 1..]) else {
            index += 1;
            continue;
        };
        let start = index + 1 + consumed;
        let end = start.saturating_add(length);
        if end > bytes.len() {
            index += 1;
            continue;
        }
        if let Ok(value) = std::str::from_utf8(&bytes[start..end]) {
            strings.push(TermiusTaggedValue::String(value.to_string()));
        }
        index = end;
    }

    strings
}

fn read_v8_varint(bytes: &[u8]) -> Option<(usize, usize)> {
    let mut value = 0_usize;
    let mut shift = 0_usize;
    for (index, byte) in bytes.iter().copied().take(5).enumerate() {
        value |= usize::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some((value, index + 1));
        }
        shift += 7;
    }
    None
}

fn read_v8_signed_integer(bytes: &[u8]) -> Option<(i64, usize)> {
    let (raw, consumed) = read_v8_varint(bytes)?;
    let value = ((raw >> 1) as i64) ^ -((raw & 1) as i64);
    Some((value, consumed))
}

fn decrypt_termius_values(
    values: &mut [TermiusTaggedValue],
    local_key: &[u8; 32],
) -> Result<(), String> {
    for value in values {
        let TermiusTaggedValue::String(text) = value else {
            continue;
        };
        if !is_termius_encrypted_value(text) {
            continue;
        }
        let decrypted = decrypt_termius_secret(text, local_key)?;
        *text = decrypted.to_string();
    }
    Ok(())
}

fn is_termius_encrypted_value(value: &str) -> bool {
    value.starts_with("BA")
        && value.len() >= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='))
}

fn decrypt_termius_secret(value: &str, local_key: &[u8; 32]) -> Result<Zeroizing<String>, String> {
    let mut decoded = base64::engine::general_purpose::STANDARD
        .decode(value)
        .map_err(|_| String::from("Cannot decode Termius encrypted value".to_string()))?;
    if decoded.len() < 26 + 16 {
        decoded.zeroize();
        return Err(String::from(
            "Termius encrypted value is too short".to_string(),
        ));
    }
    if decoded[..2] != [4, 0] {
        decoded.zeroize();
        return Err("Termius 密文版本不受支持".into());
    }

    let nonce = Nonce::from_slice(&decoded[2..26]);
    let cipher = XSalsa20Poly1305::new(Key::from_slice(local_key));
    let mut plaintext = cipher
        .decrypt(nonce, &decoded[26..])
        .map_err(|_| String::from("Cannot decrypt Termius encrypted value".to_string()))?;
    decoded.zeroize();

    let text = String::from_utf8(std::mem::take(&mut plaintext)).map_err(|_| {
        plaintext.zeroize();
        String::from("Termius encrypted value is not valid UTF-8".to_string())
    })?;
    plaintext.zeroize();
    Ok(Zeroizing::new(text))
}

fn collect_termius_store(strings: &[TermiusTaggedValue]) -> TermiusRawStore {
    let mut store = TermiusRawStore::default();

    for (index, value) in strings.iter().enumerate() {
        if value.as_str() == Some("resource_uri")
            && strings
                .get(index + 1)
                .and_then(TermiusTaggedValue::as_str)
                .is_some_and(|uri| uri.contains("/terminal/ssh/config/"))
        {
            if let Some(config) = collect_ssh_config_record(strings, index) {
                store.ssh_configs.push(config);
            }
        }

        if !is_record_marker(strings, index) {
            continue;
        }
        match value.as_str() {
            Some("ssh_config") => {
                if let Some(host) = collect_host_record(strings, index) {
                    store.hosts.push(host);
                }
            }
            Some("identity") => {
                if let Some(identity) = collect_identity_record(strings, index) {
                    store.identities.push(identity);
                }
            }
            Some("ssh_key") => {
                if let Some(key) = collect_ssh_key_record(strings, index) {
                    store.ssh_keys.push(key);
                } else if let Some(identity) = collect_identity_record(strings, index) {
                    store.identities.push(identity);
                }
            }
            Some("group") => {
                if let Some(group) = collect_group_record(strings, index) {
                    store.groups.push(group);
                }
            }
            _ => {}
        }
    }

    store
}

fn is_record_marker(strings: &[TermiusTaggedValue], index: usize) -> bool {
    strings
        .get(index + 1)
        .and_then(TermiusTaggedValue::as_str)
        .is_some_and(|value| value == "id")
}

fn collect_host_record(strings: &[TermiusTaggedValue], index: usize) -> Option<TermiusRawHost> {
    let record = collect_fields(
        strings,
        index,
        &["address", "label", "username", "password"],
        80,
    );
    let address = record.get("address").cloned();
    let label = record.get("label").cloned();
    if address.as_ref().is_none_or(|value| value.trim().is_empty())
        && label.as_ref().is_none_or(|value| value.trim().is_empty())
    {
        return None;
    }

    Some(TermiusRawHost {
        id: record_id(strings, index),
        local_id: record.get("local_id").cloned(),
        label,
        address,
        username: record.get("username").cloned(),
        password: record.get("password").cloned(),
        ssh_config_id: nested_record_id(strings, index, "ssh_config"),
        identity_id: first_non_empty_field(&record, &["identity", "identity_id", "ssh_key"]),
        group_id: first_non_empty_field(&record, &["group", "group_id"]),
        port: first_non_empty_field(&record, &["port"]).and_then(|value| value.parse().ok()),
        updated_at: record.get("updated_at").cloned(),
    })
}

fn collect_ssh_config_record(
    strings: &[TermiusTaggedValue],
    resource_uri_index: usize,
) -> Option<TermiusRawSshConfig> {
    let uri = strings.get(resource_uri_index + 1)?.as_str()?;
    let id = termius_resource_id(uri)?;
    let record = collect_fields(strings, resource_uri_index, &["port"], 80);

    Some(TermiusRawSshConfig {
        id,
        local_id: record.get("local_id").cloned(),
        identity_id: nested_record_id(strings, resource_uri_index, "identity"),
        port: first_non_empty_field(&record, &["port"]).and_then(|value| value.parse().ok()),
        updated_at: record.get("updated_at").cloned(),
    })
}

fn collect_identity_record(
    strings: &[TermiusTaggedValue],
    index: usize,
) -> Option<TermiusRawIdentity> {
    let record = collect_fields(strings, index, &["label", "username", "password"], 80);
    if !record.contains_key("username")
        && !record.contains_key("password")
        && !record.contains_key("ssh_key")
    {
        return None;
    }

    Some(TermiusRawIdentity {
        id: record_id(strings, index),
        local_id: record.get("local_id").cloned(),
        username: record.get("username").cloned(),
        password: record.get("password").cloned(),
        ssh_key_id: nested_record_id(strings, index, "ssh_key")
            .or_else(|| first_non_empty_field(&record, &["ssh_key", "ssh_key_id", "key"])),
        updated_at: record.get("updated_at").cloned(),
    })
}

fn collect_ssh_key_record(
    strings: &[TermiusTaggedValue],
    index: usize,
) -> Option<TermiusRawSshKey> {
    let record = collect_fields(strings, index, &["private_key"], 80);
    if !record.contains_key("private_key") {
        return None;
    }

    Some(TermiusRawSshKey {
        id: record_id(strings, index),
        local_id: record.get("local_id").cloned(),
        updated_at: record.get("updated_at").cloned(),
    })
}

fn collect_group_record(strings: &[TermiusTaggedValue], index: usize) -> Option<TermiusRawGroup> {
    let record = collect_fields(strings, index, &["label"], 80);
    let label = record.get("label").cloned()?;
    Some(TermiusRawGroup {
        id: record_id(strings, index),
        local_id: record.get("local_id").cloned(),
        label: Some(label),
        parent_id: first_non_empty_field(&record, &["parent", "parent_id", "group"]),
        updated_at: record.get("updated_at").cloned(),
    })
}

fn collect_fields(
    strings: &[TermiusTaggedValue],
    index: usize,
    required_keys: &[&str],
    max_fields: usize,
) -> HashMap<String, String> {
    let mut fields = HashMap::new();
    let start = record_start(strings, index);
    let mut cursor = start;
    let end = record_end(strings, index, max_fields);

    while cursor < end {
        let Some(key) = strings[cursor].as_str() else {
            cursor += 1;
            continue;
        };
        if is_termius_field_name(key) {
            let value = &strings[cursor + 1];
            let is_nested_name = value
                .as_str()
                .is_some_and(|text| is_termius_field_name(text) || is_record_type_name(text));
            if !is_nested_name {
                fields
                    .entry(key.to_string())
                    .or_insert(value.to_field_value());
            }
        }
        cursor += 1;
    }

    if required_keys.iter().any(|key| fields.contains_key(*key)) {
        fields
    } else {
        HashMap::new()
    }
}

fn record_start(strings: &[TermiusTaggedValue], index: usize) -> usize {
    let start = index.saturating_sub(120);
    for cursor in (start..index).rev() {
        if strings[cursor].as_str() == Some("status") {
            let mut next = (cursor + 2).min(strings.len());
            while next < strings.len() && matches!(strings[next], TermiusTaggedValue::Integer(_)) {
                next += 1;
            }
            return next.min(index);
        }
    }
    index.saturating_sub(8)
}

fn record_end(strings: &[TermiusTaggedValue], index: usize, max_fields: usize) -> usize {
    let hard_end = (record_start(strings, index) + max_fields).min(strings.len().saturating_sub(1));
    for cursor in (index + 1)..hard_end {
        if strings[cursor].as_str() == Some("status") {
            return (cursor + 2).min(strings.len().saturating_sub(1));
        }
    }
    hard_end
}

fn is_record_type_name(value: &str) -> bool {
    matches!(value, "ssh_config" | "identity" | "ssh_key" | "group")
}

fn is_termius_field_name(value: &str) -> bool {
    matches!(
        value,
        "id" | "local_id"
            | "updated_at"
            | "label"
            | "address"
            | "username"
            | "password"
            | "private_key"
            | "passphrase"
            | "group"
            | "group_id"
            | "identity"
            | "identity_id"
            | "ssh_key"
            | "ssh_key_id"
            | "key"
            | "parent"
            | "parent_id"
            | "port"
    )
}

fn record_id(strings: &[TermiusTaggedValue], index: usize) -> String {
    let start = record_start(strings, index);
    let end = record_end(strings, index, 80);
    for cursor in start..end {
        if strings[cursor].as_str() == Some("resource_uri") {
            let Some(uri) = strings[cursor + 1].as_str().map(str::trim) else {
                continue;
            };
            if !uri.is_empty() {
                return uri.to_string();
            }
        }
    }
    for cursor in start..end {
        if strings[cursor].as_str() == Some("id") {
            let id = strings[cursor + 1].to_field_value();
            let id = id.trim();
            if !id.is_empty() && !is_termius_field_name(id) {
                return id.to_string();
            }
        }
    }
    uuid::Uuid::new_v4().to_string()
}

fn nested_record_id(strings: &[TermiusTaggedValue], index: usize, field: &str) -> Option<String> {
    let start = record_start(strings, index);
    let end = record_end(strings, index, 80);
    for cursor in start..end.saturating_sub(2) {
        if strings[cursor].as_str() == Some(field) && strings[cursor + 1].as_str() == Some("id") {
            let value = strings[cursor + 2].to_field_value();
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

fn termius_resource_id(uri: &str) -> Option<String> {
    uri.trim_matches('/')
        .rsplit('/')
        .next()
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

fn termius_record_aliases(id: &str, local_id: Option<&str>) -> Vec<String> {
    let mut aliases = vec![id.to_string()];
    if let Some(resource_id) = termius_resource_id(id) {
        aliases.push(resource_id);
    }
    if let Some(local_id) = local_id.filter(|value| !value.is_empty()) {
        aliases.push(local_id.to_string());
    }
    aliases.sort();
    aliases.dedup();
    aliases
}

fn first_non_empty_field(record: &HashMap<String, String>, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        record.get(*key).and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        })
    })
}

fn dedupe_latest_hosts(hosts: Vec<TermiusRawHost>) -> Vec<TermiusRawHost> {
    dedupe_latest(hosts, termius_host_key, |item| item.updated_at.as_deref())
}

fn dedupe_latest_ssh_configs(configs: Vec<TermiusRawSshConfig>) -> Vec<TermiusRawSshConfig> {
    dedupe_latest(configs, termius_ssh_config_key, |item| {
        item.updated_at.as_deref()
    })
}

fn dedupe_latest_identities(identities: Vec<TermiusRawIdentity>) -> Vec<TermiusRawIdentity> {
    dedupe_latest(identities, termius_identity_key, |item| {
        item.updated_at.as_deref()
    })
}

fn dedupe_latest_ssh_keys(keys: Vec<TermiusRawSshKey>) -> Vec<TermiusRawSshKey> {
    dedupe_latest(keys, termius_ssh_key_key, |item| item.updated_at.as_deref())
}

fn dedupe_latest_groups(groups: Vec<TermiusRawGroup>) -> Vec<TermiusRawGroup> {
    dedupe_latest(groups, termius_group_key, |item| item.updated_at.as_deref())
}

fn dedupe_latest<T, K, F, U>(items: Vec<T>, key_fn: F, updated_fn: U) -> Vec<T>
where
    K: Ord,
    F: Fn(&T) -> K,
    U: Fn(&T) -> Option<&str>,
{
    let mut map: BTreeMap<K, T> = BTreeMap::new();
    for item in items {
        let key = key_fn(&item);
        let replace = map.get(&key).is_none_or(|existing| {
            updated_fn(&item).unwrap_or_default() >= updated_fn(existing).unwrap_or_default()
        });
        if replace {
            map.insert(key, item);
        }
    }
    map.into_values().collect()
}

fn termius_host_key(host: &TermiusRawHost) -> String {
    host.local_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&host.id)
        .to_string()
}

fn termius_identity_key(identity: &TermiusRawIdentity) -> String {
    identity
        .local_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&identity.id)
        .to_string()
}

fn termius_ssh_config_key(config: &TermiusRawSshConfig) -> String {
    config.id.clone()
}

fn termius_ssh_key_key(key: &TermiusRawSshKey) -> String {
    key.local_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&key.id)
        .to_string()
}

fn termius_group_key(group: &TermiusRawGroup) -> String {
    group
        .local_id
        .as_deref()
        .filter(|value| !value.is_empty())
        .unwrap_or(&group.id)
        .to_string()
}

fn build_termius_group_paths(groups: &[TermiusRawGroup]) -> HashMap<String, Vec<String>> {
    let by_key: HashMap<String, &TermiusRawGroup> = groups
        .iter()
        .map(|group| (termius_group_key(group), group))
        .collect();
    let mut paths = HashMap::new();
    for group in groups {
        let key = termius_group_key(group);
        let mut visited = HashSet::new();
        if let Some(path) = build_termius_group_path(&key, &by_key, &mut visited) {
            paths.insert(key, path);
        }
    }
    paths
}

fn build_termius_group_path(
    key: &str,
    by_key: &HashMap<String, &TermiusRawGroup>,
    visited: &mut HashSet<String>,
) -> Option<Vec<String>> {
    if !visited.insert(key.to_string()) {
        return None;
    }
    let group = by_key.get(key)?;
    let label = normalize_optional_string(group.label.clone())?;
    let mut path =
        if let Some(parent) = group.parent_id.as_deref().filter(|value| !value.is_empty()) {
            build_termius_group_path(parent, by_key, visited).unwrap_or_default()
        } else {
            Vec::new()
        };
    path.push(label);
    Some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tagged(value: &str, out: &mut Vec<u8>) {
        out.push(b'"');
        let mut remaining = value.len();
        while remaining >= 0x80 {
            out.push((remaining as u8 & 0x7f) | 0x80);
            remaining >>= 7;
        }
        out.push(remaining as u8);
        out.extend_from_slice(value.as_bytes());
    }

    fn encrypted(value: &str, key: &[u8; 32]) -> String {
        let nonce = *b"abcdefghijklmnopqrstuvwx";
        let cipher = XSalsa20Poly1305::new(Key::from_slice(key));
        let mut output = vec![4, 0];
        output.extend_from_slice(&nonce);
        output.extend_from_slice(
            &cipher
                .encrypt(Nonce::from_slice(&nonce), value.as_bytes())
                .unwrap(),
        );
        STANDARD.encode(output)
    }

    #[test]
    fn key_formats_and_authenticated_value() {
        let key = *b"12345678901234567890123456789012";
        let utf16: Vec<u8> = std::str::from_utf8(&key)
            .unwrap()
            .encode_utf16()
            .flat_map(u16::to_le_bytes)
            .collect();
        assert_eq!(normalize_key(&utf16, 0), Some(key));
        assert_eq!(normalize_key(STANDARD.encode(key).as_bytes(), 0), Some(key));
        let value = encrypted("fixture-secret", &key);
        assert_eq!(
            decrypt_termius_secret(&value, &key).unwrap().as_str(),
            "fixture-secret"
        );
        assert!(decrypt_termius_secret(&value, b"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").is_err());
    }

    #[test]
    fn encrypted_host_record_reaches_preview_without_leaking_secret() {
        let key = *b"12345678901234567890123456789012";
        let password = encrypted("fixture-secret", &key);
        let mut raw = Vec::new();
        for (field, value) in [
            ("status", "SYNCHRONIZED"),
            ("ssh_config", "id"),
            ("id", "host-fixture"),
            ("address", "web.example.org"),
            ("label", "Web"),
            ("username", "deploy"),
            ("password", password.as_str()),
            ("port", "2222"),
            ("status", "SYNCHRONIZED"),
        ] {
            tagged(field, &mut raw);
            tagged(value, &mut raw);
        }
        let preview = parse_records(&raw, &key, Path::new("synthetic-leveldb")).unwrap();
        assert_eq!(preview.profiles.len(), 1);
        assert_eq!(preview.profiles[0].host, "web.example.org");
        assert_eq!(preview.profiles[0].port, 2222);
        assert_eq!(
            preview.profiles[0].password.as_deref(),
            Some("fixture-secret")
        );
        assert!(!serde_json::to_string(&preview.profiles[0])
            .unwrap()
            .contains("fixture-secret"));
    }

    #[test]
    fn leveldb_is_copied_read_only_and_temporary_copy_is_removed() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("file__0.indexeddb.leveldb");
        fs::create_dir_all(&source).unwrap();
        let mut db = rusty_leveldb::DB::open(&source, rusty_leveldb::Options::default()).unwrap();
        let mut bytes = Vec::new();
        tagged("ssh_config", &mut bytes);
        tagged("id", &mut bytes);
        tagged("host-fixture", &mut bytes);
        tagged("address", &mut bytes);
        tagged("web.example.org", &mut bytes);
        db.put(b"fixture", &bytes).unwrap();
        db.flush().unwrap();
        drop(db);
        let before = fs::read_dir(&source)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<HashMap<_, _>>();
        let extracted = records(&source).unwrap();
        assert!(extracted.windows(bytes.len()).any(|window| window == bytes));
        let after = fs::read_dir(&source)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), fs::read(entry.path()).unwrap())
            })
            .collect::<HashMap<_, _>>();
        assert_eq!(before, after);
    }
}
