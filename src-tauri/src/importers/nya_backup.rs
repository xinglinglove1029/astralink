// NyaTerm portable snapshot compatibility, based on MIT source commit
// 6af18f59b865d5eae596876ffc159acd14826abb. See docs/licenses/nyaterm-MIT.txt.
use super::*;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use redb::{Database, ReadableDatabase, ReadableTable, TableDefinition};
use serde::ser::{Serialize, SerializeMap, Serializer};
use serde_json::value::RawValue;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
};
use zeroize::Zeroizing;

const MAX: u64 = 50 * 1024 * 1024;

fn decrypt(bytes: &[u8], key: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    if bytes.len() < 28 {
        return Err("NyaTerm 加密数据长度无效".into());
    }
    Aes256Gcm::new_from_slice(key)
        .map_err(|_| "NyaTerm 密钥格式无效")?
        .decrypt(Nonce::from_slice(&bytes[..12]), &bytes[12..])
        .map(Zeroizing::new)
        .map_err(|_| "NyaTerm 备份密码不正确或内容已损坏".into())
}

fn derive(prefix: &[u8], password: &str) -> Vec<u8> {
    let mut hash = Sha256::new();
    hash.update(prefix);
    hash.update(password.as_bytes());
    hash.finalize().to_vec()
}

fn compatible_decrypt(
    bytes: &[u8],
    password: &str,
    prefixes: [&[u8]; 2],
) -> Result<Zeroizing<Vec<u8>>, String> {
    decrypt(bytes, &derive(prefixes[0], password))
        .or_else(|_| decrypt(bytes, &derive(prefixes[1], password)))
}

struct RawEntities(Vec<(String, Box<RawValue>)>);
impl Serialize for RawEntities {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (key, value) in &self.0 {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }
}

fn validate_hash(meta: &Value, entities: &BTreeMap<String, String>) -> Result<(), String> {
    let mut names = vec![
        "settings",
        "sessions",
        "keys",
        "passwords",
        "credentials",
        "otp",
        "proxies",
    ];
    if entities.contains_key("proxy_groups") {
        names.push("proxy_groups");
    }
    names.push("tunnels");
    if entities.contains_key("tunnel_groups") {
        names.push("tunnel_groups");
    }
    names.extend([
        "quick_commands",
        "history",
        "master_key_token",
        "known_hosts",
    ]);
    if entities.contains_key("notes") {
        names.push("notes");
    }
    let values = names
        .into_iter()
        .map(|name| {
            let raw = entities.get(name).ok_or("NyaTerm 备份缺少实体")?;
            let value =
                serde_json::from_str::<Box<RawValue>>(raw).map_err(|_| "NyaTerm 备份实体无效")?;
            Ok((name.to_owned(), value))
        })
        .collect::<Result<Vec<_>, String>>()?;
    let payload = serde_json::to_vec(&RawEntities(values)).map_err(|_| "无法校验 NyaTerm 快照")?;
    if Some(format!("{:x}", Sha256::digest(payload)).as_str()) != text(meta, &["payload_hash"]) {
        return Err("NyaTerm 备份完整性校验失败".into());
    }
    Ok(())
}

fn table(
    read: &redb::ReadTransaction,
    name: &'static str,
) -> Result<BTreeMap<String, String>, String> {
    let table = read
        .open_table(TableDefinition::<&str, &str>::new(name))
        .map_err(|_| "NyaTerm 备份缺少数据表")?;
    let mut entries = BTreeMap::new();
    let mut size = 0;
    for entry in table.iter().map_err(|_| "无法读取 NyaTerm 备份表")? {
        let (key, value) = entry.map_err(|_| "NyaTerm 备份表损坏")?;
        size += value.value().len();
        if size > MAX as usize || entries.len() > 100_000 {
            return Err("NyaTerm 备份内容过大".into());
        }
        entries.insert(key.value().to_owned(), value.value().to_owned());
    }
    Ok(entries)
}

fn entity(entities: &BTreeMap<String, String>, name: &str) -> Result<Value, String> {
    serde_json::from_str(entities.get(name).ok_or("NyaTerm 备份缺少数据")?)
        .map_err(|_| "NyaTerm 备份格式无效".into())
}

fn secret(token: &str, key: &[u8]) -> Result<String, String> {
    let bytes = STANDARD.decode(token).map_err(|_| "NyaTerm 凭据编码无效")?;
    String::from_utf8(decrypt(&bytes, key)?.to_vec()).map_err(|_| "NyaTerm 凭据内容无效".into())
}

fn inline_password(token: &str, key: &[u8]) -> Result<String, String> {
    let looks_encrypted = token.len() >= 38
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'+' | b'/' | b'='));
    if looks_encrypted {
        secret(token, key)
    } else {
        Ok(token.to_owned())
    }
}

pub(super) fn file(
    path: &Path,
    home: &Path,
    password: Option<&str>,
) -> Result<ImportPreview, String> {
    let password = password
        .filter(|p| !p.is_empty() && p.len() <= 1024)
        .ok_or("请输入 NyaTerm 备份密码")?;
    if fs::metadata(path)
        .map_err(|_| "无法读取 NyaTerm 备份")?
        .len()
        > MAX
    {
        return Err("NyaTerm 备份超过 50 MB".into());
    }
    let encrypted = fs::read(path).map_err(|_| "无法读取 NyaTerm 备份")?;
    let outer = compatible_decrypt(
        &encrypted,
        password,
        [
            b"nyaterm-cloud-snapshot-v1:",
            b"dragonfly-cloud-snapshot-v1:",
        ],
    )?;
    let payload = if outer.starts_with(b"PK\x03\x04") {
        let mut zip = zip::ZipArchive::new(Cursor::new(outer.as_slice()))
            .map_err(|_| "NyaTerm 备份 ZIP 无效")?;
        let mut entry = zip
            .by_name("snapshot.redb")
            .map_err(|_| "NyaTerm 备份缺少快照")?;
        if entry.size() > MAX {
            return Err("NyaTerm 备份展开后超过 50 MB".into());
        }
        let mut bytes = Zeroizing::new(Vec::new());
        entry
            .by_ref()
            .take(MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "无法解压 NyaTerm 备份")?;
        if bytes.len() as u64 > MAX {
            return Err("NyaTerm 备份展开后过大".into());
        }
        bytes
    } else {
        outer
    };
    let temp = tempfile::Builder::new()
        .prefix("astralink-nya-")
        .tempdir()
        .map_err(|_| "无法创建备份读取副本")?;
    let copy = temp.path().join("snapshot.redb");
    fs::write(&copy, payload.as_slice()).map_err(|_| "无法读取 NyaTerm 快照")?;
    let db = Database::open(&copy).map_err(|_| "NyaTerm 快照数据库无效")?;
    let read = db.begin_read().map_err(|_| "无法读取 NyaTerm 快照数据库")?;
    let meta_table = read
        .open_table(TableDefinition::<&str, &str>::new("snapshot_meta"))
        .map_err(|_| "NyaTerm 备份缺少元数据")?;
    let meta_raw = meta_table
        .get("meta")
        .map_err(|_| "NyaTerm 备份元数据无效")?
        .ok_or("NyaTerm 备份缺少元数据")?;
    let meta: Value =
        serde_json::from_str(meta_raw.value()).map_err(|_| "NyaTerm 备份元数据无效")?;
    if text(&meta, &["snapshot_kind"]) != Some("backup") {
        return Err("所选文件不是 NyaTerm 配置备份".into());
    }
    let entities = match meta.get("schema_version").and_then(Value::as_u64) {
        Some(3) => {
            let entities = table(&read, "entity_docs")?;
            validate_hash(&meta, &entities)?;
            entities
        }
        Some(2) => {
            let json_docs = table(&read, "json_docs")?;
            let text_docs = table(&read, "text_docs")?;
            #[derive(serde::Serialize)]
            struct HashInput<'a> {
                json_docs: &'a BTreeMap<String, String>,
                text_docs: &'a BTreeMap<String, String>,
            }
            let hash = serde_json::to_vec(&HashInput {
                json_docs: &json_docs,
                text_docs: &text_docs,
            })
            .map_err(|_| "无法校验 NyaTerm 备份")?;
            if Some(format!("{:x}", Sha256::digest(hash)).as_str())
                != text(&meta, &["payload_hash"])
            {
                return Err("NyaTerm 备份完整性校验失败".into());
            }
            let mut entities = json_docs;
            entities.insert(
                "master_key_token".into(),
                serde_json::to_string(text_docs.get("master.key").ok_or("NyaTerm 备份密钥缺失")?)
                    .map_err(|_| "备份密钥无效")?,
            );
            entities
        }
        _ => return Err("NyaTerm 备份版本不受支持".into()),
    };
    let wrapped = entity(&entities, "master_key_token")?;
    let wrapped = STANDARD
        .decode(wrapped.as_str().ok_or("NyaTerm 备份密钥无效")?)
        .map_err(|_| "NyaTerm 备份密钥无效")?;
    let key = compatible_decrypt(
        &wrapped,
        password,
        [b"nyaterm-key-wrap-v1:", b"dragonfly-key-wrap-v1:"],
    )?;
    let sessions = entity(&entities, "sessions")?;
    let connections = sessions
        .get("connections")
        .and_then(Value::as_array)
        .ok_or("NyaTerm 备份没有连接列表")?;
    if connections.len() > 512 {
        return Err("NyaTerm 备份连接超过 512 个".into());
    }
    let groups: BTreeMap<String, Value> = sessions
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|g| text(g, &["id"]).map(|id| (id.to_owned(), g.clone())))
        .collect();
    let mut passwords = BTreeMap::new();
    for (kind, list) in [("passwords", "passwords"), ("credentials", "credentials")] {
        if let Ok(data) = entity(&entities, kind) {
            for item in data
                .get(list)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                if let (Some(id), Some(token)) = (text(item, &["id"]), text(item, &["password"])) {
                    passwords.insert(id.to_owned(), secret(token, &key)?);
                }
            }
        }
    }
    let mut normalized = Vec::new();
    for connection in connections {
        let mut session = connection.clone();
        let mut path = Vec::new();
        let mut current = text(connection, &["group_id"])
            .unwrap_or_default()
            .to_owned();
        let mut seen = HashSet::new();
        while seen.insert(current.clone()) && path.len() < 32 {
            let Some(group) = groups.get(&current) else {
                break;
            };
            path.push(text(group, &["name"]).unwrap_or_default().to_owned());
            current = text(group, &["parent_id"]).unwrap_or_default().to_owned();
        }
        path.reverse();
        session["group_path"] = serde_json::json!(path);
        let auth = connection.get("auth").unwrap_or(&Value::Null);
        let pass = if let Some(token) = text(auth, &["password"]) {
            Some(inline_password(token, &key)?)
        } else {
            text(auth, &["account_id", "password_id"]).and_then(|id| passwords.get(id).cloned())
        };
        if let Some(pass) = pass {
            session["auth"] = serde_json::json!({"mode":"password", "password":pass});
        } else if text(auth, &["mode"]) == Some("key") {
            session["auth"] = serde_json::json!({"mode":"key", "key_ref":"backup-key"});
        } else if auth.is_null() {
            session["auth"] = serde_json::json!({"mode":"password"});
        }
        normalized.push(session);
    }
    let mut preview = parse(
        "nyaterm",
        &serde_json::json!({"version":1,"sessions":normalized}).to_string(),
        path,
        home,
    )?;
    preview.source = "nya-backup".into();
    for p in &mut preview.profiles {
        p.source = "nya-backup".into();
    }
    issue(
        &mut preview,
        "warning",
        "仅提取 SSH 连接；不恢复 NyaTerm 设置、私钥文件及历史",
        None,
    );
    Ok(preview)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn seal(plaintext: &[u8], key: &[u8]) -> Vec<u8> {
        let nonce = *b"0123456789ab";
        let mut bytes = nonce.to_vec();
        bytes.extend_from_slice(
            &Aes256Gcm::new_from_slice(key)
                .unwrap()
                .encrypt(Nonce::from_slice(&nonce), plaintext)
                .unwrap(),
        );
        bytes
    }

    fn backup(path: &Path, password: &str, valid_hash: bool) {
        let portable_key = [7u8; 32];
        let fixture_password = STANDARD.encode(seal(b"fixture-secret", &portable_key));
        let wrapped = STANDARD.encode(seal(
            &portable_key,
            &derive(b"nyaterm-key-wrap-v1:", password),
        ));
        let mut entities = BTreeMap::<String, String>::new();
        for (name, value) in [
            ("settings", serde_json::json!({})),
            (
                "sessions",
                serde_json::json!({"groups":[{"id":"g1","name":"Production"}], "connections":[
                {"id":"c1","name":"Web","type":"ssh","host":"web.example.org","port":2222,"username":"deploy","group_id":"g1","auth":{"mode":"password","account_id":"p1"}},
                {"id":"c2","name":"Local","type":"local_terminal"}]}),
            ),
            ("keys", serde_json::json!({"keys":[]})),
            (
                "passwords",
                serde_json::json!({"passwords":[{"id":"p1","password":fixture_password}]}),
            ),
            ("credentials", serde_json::json!({"credentials":[]})),
            ("otp", serde_json::json!({"entries":[]})),
            ("proxies", serde_json::json!([])),
            ("proxy_groups", serde_json::json!([])),
            ("tunnels", serde_json::json!([])),
            ("tunnel_groups", serde_json::json!([])),
            ("quick_commands", serde_json::json!({})),
            ("history", serde_json::json!([])),
            ("master_key_token", serde_json::json!(wrapped)),
            ("known_hosts", serde_json::json!("")),
            ("notes", serde_json::json!({})),
        ] {
            entities.insert(name.to_owned(), serde_json::to_string(&value).unwrap());
        }
        let names = [
            "settings",
            "sessions",
            "keys",
            "passwords",
            "credentials",
            "otp",
            "proxies",
            "proxy_groups",
            "tunnels",
            "tunnel_groups",
            "quick_commands",
            "history",
            "master_key_token",
            "known_hosts",
            "notes",
        ];
        let hash_input = RawEntities(
            names
                .iter()
                .map(|name| {
                    (
                        name.to_string(),
                        serde_json::from_str::<Box<RawValue>>(&entities[*name]).unwrap(),
                    )
                })
                .collect(),
        );
        let hash = if valid_hash {
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&hash_input).unwrap())
            )
        } else {
            "bad-hash".into()
        };
        let inner = path.with_extension("redb");
        {
            let db = Database::create(&inner).unwrap();
            let tx = db.begin_write().unwrap();
            {
                let mut meta = tx
                    .open_table(TableDefinition::<&str, &str>::new("snapshot_meta"))
                    .unwrap();
                let record = serde_json::json!({"schema_version":3,"snapshot_kind":"backup","payload_hash":hash});
                meta.insert("meta", serde_json::to_string(&record).unwrap().as_str())
                    .unwrap();
                let mut table = tx
                    .open_table(TableDefinition::<&str, &str>::new("entity_docs"))
                    .unwrap();
                for (key, value) in &entities {
                    table.insert(key.as_str(), value.as_str()).unwrap();
                }
            }
            tx.commit().unwrap();
        }
        let archive = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let mut archive = archive;
        archive
            .start_file("snapshot.redb", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(&fs::read(&inner).unwrap()).unwrap();
        let zipped = archive.finish().unwrap().into_inner();
        fs::write(
            path,
            seal(&zipped, &derive(b"nyaterm-cloud-snapshot-v1:", password)),
        )
        .unwrap();
    }

    #[test]
    fn authenticated_backup_extracts_ssh_and_password_only() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fixture.nya");
        backup(&path, "fixture-passphrase", true);
        let preview = file(&path, dir.path(), Some("fixture-passphrase")).unwrap();
        assert_eq!(preview.profiles.len(), 1);
        assert_eq!(preview.profiles[0].group, "Production");
        assert_eq!(preview.profiles[0].port, 2222);
        assert_eq!(
            preview.profiles[0].password.as_deref(),
            Some("fixture-secret")
        );
        assert_eq!(preview.stats.unsupported, 1);
        assert!(file(&path, dir.path(), Some("wrong-passphrase")).is_err());
        backup(&path, "fixture-passphrase", false);
        assert!(file(&path, dir.path(), Some("fixture-passphrase"))
            .err()
            .is_some_and(|error| error.contains("完整性")));
    }
}
