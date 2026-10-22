use crate::{
    config_sync,
    credentials::SecretStore,
    discovery, engine, importers,
    models::*,
    storage::{self, AppStore},
};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, Mutex},
};
#[derive(Clone, Default)]
struct Vault(Arc<Mutex<HashMap<String, String>>>);
impl SecretStore for Vault {
    fn get(&self, k: &str) -> Result<Option<String>, String> {
        Ok(self.0.lock().unwrap().get(k).cloned())
    }
    fn set(&self, k: &str, v: &str) -> Result<(), String> {
        self.0.lock().unwrap().insert(k.into(), v.into());
        Ok(())
    }
    fn delete(&self, k: &str) -> Result<(), String> {
        self.0.lock().unwrap().remove(k);
        Ok(())
    }
}
fn profile(name: &str) -> ConnectionProfile {
    ConnectionProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        host: "example.test".into(),
        port: 22,
        username: "ops".into(),
        enabled: true,
        ..Default::default()
    }
}
fn parse(kind: &str, text: &str) -> ImportPreview {
    importers::parse(
        kind,
        text,
        Path::new("C:/imports/sample"),
        Path::new("C:/Users/test"),
    )
    .unwrap()
}
#[test]
fn json_object_maps_names_and_does_not_serialize_secrets() {
    let p = parse(
        "mcp-json",
        r#"{"prod":{"host":"prod.test","username":"ops","password":"sensitive-test","transportMode":"shell","tryKeyboard":true}}"#,
    );
    assert_eq!(p.profiles[0].name, "prod");
    assert_eq!(p.profiles[0].password.as_deref(), Some("sensitive-test"));
    assert_eq!(p.profiles[0].transport_mode, TransportMode::Shell);
    assert_eq!(p.stats.ready, 1);
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains("sensitive-test"));
    let encoded: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(encoded["profiles"][0].get("password").is_none());
    assert!(encoded["profiles"][0].get("passphrase").is_none());
}
#[test]
fn json_array_rejects_bad_ports_without_discarding_valid_rows() {
    let p = parse(
        "mcp-json",
        r#"[{"name":"ok","host":"good","username":"ops","port":22},{"name":"bad","host":"bad","username":"ops","port":70000}]"#,
    );
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.stats.unsupported, 1);
    assert!(p.issues.iter().any(|i| i.severity == "error"));
}
#[test]
fn json_inline_private_keys_are_never_mistaken_for_paths() {
    let p = parse(
        "mcp-json",
        r#"[{"name":"key","host":"good","username":"ops","privateKey":"-----BEGIN OPENSSH PRIVATE KEY-----secret"}]"#,
    );
    assert!(!serde_json::to_string(&p).unwrap().contains("PRIVATE KEY"));
    assert!(!p.profiles[0].enabled);
    assert!(!p.issues.is_empty());
}
#[test]
fn tabby_applies_ssh_defaults_and_warns_about_vault() {
    let p=parse("tabby","profileDefaults:\n  ssh:\n    options:\n      user: ops\n      port: 2222\nprofiles:\n  - type: ssh\n    name: prod\n    options:\n      host: prod.test\n      password: vault:entry\n  - type: local\n    name: local\n");
    assert_eq!(p.profiles[0].username, "ops");
    assert_eq!(p.profiles[0].port, 2222);
    assert!(p.profiles[0].password.is_none());
    assert_eq!(p.stats.needs_credential, 1);
    assert_eq!(p.stats.unsupported, 1);
    assert!(!p.issues.is_empty());
}
#[test]
fn tabby_jump_relations_use_normalized_ids() {
    let p=parse("tabby","profiles:\n  - type: ssh\n    id: ssh:jump\n    name: jump\n    options: {host: bastion, user: ops}\n  - type: ssh\n    name: target\n    options: {host: target, user: ops, jumpHost: 'ssh:jump'}\n");
    assert_eq!(
        p.profiles[1].jump_host_id.as_deref(),
        Some(p.profiles[0].id.as_str())
    );
}
#[test]
fn csv_handles_real_headers_quotes_multiline_and_marked_passphrase() {
    let p=parse("netcatty","\u{feff}Groups,Label,Tags,Notes,Hostname/IP,Protocol,Port,Username,Password,KeyPath,Passphrase,Proxy\r\nProd,\"db,primary\",db,\"first\nsecond\",[::1],ssh,2222,ops,\" a,b \",,,,\r\nProd,telnet,,,legacy,telnet,23,ops,,,,\r\n");
    assert_eq!(p.profiles[0].name, "db,primary");
    assert_eq!(p.profiles[0].host, "::1");
    assert_eq!(p.profiles[0].password.as_deref(), Some(" a,b "));
    assert_eq!(p.profiles[0].notes, "first\nsecond");
    assert_eq!(p.stats.unsupported, 1);
}
#[test]
fn csv_proxy_is_explicitly_unsupported_and_never_silently_direct() {
    let p = parse(
        "netcatty",
        "Label,Hostname/IP,Username,Proxy\nproxied,server,ops,socks5://127.0.0.1:1080\n",
    );
    assert!(!p.profiles[0].enabled);
    assert!(!p.issues.is_empty());
}
#[test]
fn openssh_first_obtained_values_and_wildcards_match_aliases() {
    let p=parse("openssh","Host prod\n  HostName prod.test\n  User deploy\nHost *\n  User root\n  Port 2222\nHost prod\n  Port 9999\nHost *.internal !banned.internal\n  IdentityFile ~/.ssh/id_test\n");
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].username, "deploy");
    assert_eq!(p.profiles[0].port, 2222);
    assert_eq!(p.profiles[0].host, "prod.test");
}
#[test]
fn openssh_proxycommand_is_not_executed_or_silently_ignored() {
    let p=parse("openssh","Host dangerous\nHostName example\nUser ops\nProxyCommand curl dangerous\nInclude missing.conf\n");
    assert!(!p.profiles[0].enabled);
    assert!(p.issues.len() >= 2);
}
#[test]
fn xshell_encrypted_auth_never_becomes_plaintext() {
    let p=parse("xshell","[CONNECTION]\nHost=prod.test\nPort=22\nProtocol=SSH\n[CONNECTION:AUTHENTICATION]\nUserName=ops\nPassword=encrypted-secret\n");
    assert!(p.profiles[0].password.is_none());
    assert_eq!(p.stats.needs_credential, 1);
    assert!(!p.issues.is_empty());
    assert!(!serde_json::to_string(&p)
        .unwrap()
        .contains("encrypted-secret"));
}
#[test]
fn persistence_separates_credentials_and_resolves_after_reload() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut s = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let mut p = profile("prod");
    p.password = Some("secret-test".into());
    p.passphrase = Some("passphrase-test".into());
    let id = p.id.clone();
    s.apply(vec![p], ImportStrategy::Merge).unwrap();
    let json =
        String::from_utf8(crate::sqlite_store::committed_bytes(dir.path()).unwrap()).unwrap();
    assert!(!json.contains("secret-test"));
    assert!(!json.contains("passphrase-test"));
    assert_eq!(vault.0.lock().unwrap().len(), 2);
    let loaded = AppStore::open(dir.path().into(), Box::new(vault)).unwrap();
    assert!(loaded.profiles()[0].password.is_none());
    assert!(loaded.profiles()[0].has_password);
    assert_eq!(
        loaded.resolve(&id).unwrap().password.as_deref(),
        Some("secret-test")
    );
}
#[test]
fn merge_preserves_existing_credentials_and_replace_cleans_deleted_entries() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut s = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let mut a = profile("a");
    a.password = Some("secret".into());
    s.apply(vec![a], ImportStrategy::Merge).unwrap();
    let mut a = s.profiles()[0].clone();
    a.notes = "changed".into();
    s.apply(vec![a, profile("b")], ImportStrategy::Merge)
        .unwrap();
    assert_eq!(s.profiles().len(), 2);
    assert_eq!(vault.0.lock().unwrap().len(), 1);
    s.apply(vec![], ImportStrategy::Replace).unwrap();
    assert!(s.profiles().is_empty());
    assert!(vault.0.lock().unwrap().is_empty());
}
#[test]
fn invalid_batch_preserves_disk_and_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut s = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    s.apply(vec![profile("ok")], ImportStrategy::Merge).unwrap();
    let before = crate::sqlite_store::committed_bytes(dir.path()).unwrap();
    let mut p = profile("bad");
    p.port = 0;
    p.password = Some("secret".into());
    assert!(s.apply(vec![p], ImportStrategy::Replace).is_err());
    assert_eq!(
        crate::sqlite_store::committed_bytes(dir.path()).unwrap(),
        before
    );
    assert!(vault.0.lock().unwrap().is_empty());
}
#[test]
fn duplicate_names_and_unresolved_jumps_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
    assert!(s
        .apply(
            vec![profile("same"), profile("same")],
            ImportStrategy::Merge
        )
        .is_err());
    let mut p = profile("jump");
    p.jump_host_id = Some(uuid::Uuid::new_v4().to_string());
    assert!(s.apply(vec![p], ImportStrategy::Merge).is_err());
}
#[test]
fn key_file_validation_preserves_good_config() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
    let mut p = profile("key");
    p.auth_type = AuthType::PrivateKey;
    p.private_key_path = Some(dir.path().join("missing").to_string_lossy().into());
    assert!(s.apply(vec![p], ImportStrategy::Merge).is_err());
}
#[test]
fn settings_persist_and_zero_port_is_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut s = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let settings = AppSettings {
        mcp_port: 18888,
        start_on_launch: true,
        minimize_to_tray: false,
    };
    s.update_settings(settings.clone()).unwrap();
    assert_eq!(
        AppStore::open(dir.path().into(), Box::new(vault))
            .unwrap()
            .settings(),
        settings
    );
    assert!(s
        .update_settings(AppSettings {
            mcp_port: 0,
            ..settings
        })
        .is_err());
    assert_eq!(s.settings().mcp_port, 18888);
}
#[test]
fn atomic_write_replaces_existing_file_and_keeps_old_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("state.json");
    storage::atomic_write(&path, b"old").unwrap();
    storage::atomic_write(&path, b"new").unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
    assert!(storage::atomic_write(dir.path(), b"bad").is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"new");
}
#[test]
fn codex_registration_preserves_unrelated_config_and_exact_backup() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    let text="# comment\nmodel = \"demo\"\n[mcp_servers.other]\ncommand = \"other.exe\"\n[mcp_servers.astralink]\ncommand = \"obsolete.exe\"\nargs = [\"legacy\"]\nstartup_timeout_sec = 20\n";
    std::fs::write(&path, text).unwrap();
    let result = config_sync::register(&path, 18766, "test-token").unwrap();
    assert_eq!(
        std::fs::read_to_string(result.backup_path.unwrap()).unwrap(),
        text
    );
    let output = std::fs::read_to_string(&path).unwrap();
    assert!(output.contains("# comment"));
    let doc = output.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        doc["mcp_servers"]["other"]["command"].as_str(),
        Some("other.exe")
    );
    assert_eq!(
        doc["mcp_servers"]["astralink"]["url"].as_str(),
        Some("http://127.0.0.1:18766/mcp")
    );
    assert!(doc["mcp_servers"]["astralink"].get("command").is_none());
    assert_eq!(
        doc["mcp_servers"]["astralink"]["startup_timeout_sec"].as_integer(),
        Some(20)
    );
    assert_eq!(
        doc["mcp_servers"]["astralink"]["http_headers"]["Authorization"].as_str(),
        Some("Bearer test-token")
    );
    assert!(
        !config_sync::register(&path, 18766, "test-token")
            .unwrap()
            .reload_required
    );
}
#[test]
fn malformed_codex_config_is_never_overwritten() {
    let dir = tempfile::tempdir().unwrap();
    let p = dir.path().join("config.toml");
    std::fs::write(&p, "[[broken").unwrap();
    assert!(config_sync::register(&p, 18766, "token").is_err());
    assert_eq!(std::fs::read_to_string(&p).unwrap(), "[[broken");
}
#[test]
fn discovery_uses_supplied_roots_and_does_not_read_vault_files() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".nyaterm")).unwrap();
    std::fs::write(dir.path().join(".nyaterm/nyaterm.redb"), b"fixture").unwrap();
    let roaming = dir.path().join("AppData/Roaming");
    std::fs::create_dir_all(roaming.join("tabby")).unwrap();
    std::fs::write(roaming.join("tabby/config.yaml"), "vault: encrypted").unwrap();
    let found = discovery::discover(dir.path(), Some(&roaming), None);
    let tabby = found
        .iter()
        .find(|s| s.kind == "tabby" && s.exists)
        .unwrap();
    assert!(tabby.path.ends_with("config.yaml"));
    assert!(found.iter().any(|s| s.kind == "netcatty"));
    assert!(found
        .iter()
        .any(|s| s.kind == "nyaterm" && s.exists && s.path.ends_with("nyaterm.redb")));
}
#[test]
fn release_runtime_resolves_exact_resource_map() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("runtime")).unwrap();
    std::fs::create_dir_all(dir.path().join("engine/src")).unwrap();
    let node = if cfg!(windows) { "node.exe" } else { "node" };
    std::fs::write(dir.path().join("runtime").join(node), b"fixture").unwrap();
    std::fs::write(dir.path().join("engine/src/main.mjs"), b"fixture").unwrap();
    let p = engine::resolve_paths(None, dir.path(), None).unwrap();
    assert_eq!(p.node, dir.path().join("runtime").join(node));
    assert_eq!(p.cwd, dir.path().join("engine"));
}
#[test]
fn dev_runtime_honors_node_override() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("engine/src")).unwrap();
    std::fs::write(dir.path().join("engine/src/main.mjs"), b"fixture").unwrap();
    let node = dir.path().join("node.exe");
    std::fs::write(&node, b"fixture").unwrap();
    let p = engine::resolve_paths(Some(dir.path()), Path::new("unused"), Some(&node)).unwrap();
    assert_eq!(p.node, node);
    assert_eq!(p.entry, dir.path().join("engine/src/main.mjs"));
}

#[test]
fn settings_deserialize_old_state_defaults_without_resetting_profiles() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("state.json"),r#"{"profiles":[],"settings":{"mcpPort":18766,"startOnLaunch":false,"minimizeToTray":true}}"#).unwrap();
    assert_eq!(
        AppStore::open(dir.path().into(), Box::new(Vault::default()))
            .unwrap()
            .settings()
            .mcp_port,
        18766
    );
}
#[test]
fn secret_store_failure_does_not_write_metadata() {
    struct Denied;
    impl SecretStore for Denied {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Ok(None)
        }
        fn set(&self, _: &str, _: &str) -> Result<(), String> {
            Err("denied".into())
        }
        fn delete(&self, _: &str) -> Result<(), String> {
            Ok(())
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let mut s = AppStore::open(dir.path().into(), Box::new(Denied)).unwrap();
    let mut p = profile("prod");
    p.password = Some("never-disk".into());
    assert!(s.apply(vec![p], ImportStrategy::Merge).is_err());
    assert!(!dir.path().join("state.json").exists());
    assert!(s.profiles().is_empty());
}
#[test]
fn cyclic_jump_relations_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
    let mut a = profile("a");
    let mut b = profile("b");
    a.jump_host_id = Some(b.id.clone());
    b.jump_host_id = Some(a.id.clone());
    assert!(s.apply(vec![a, b], ImportStrategy::Merge).is_err());
}
#[test]
fn openssh_proxyjump_resolves_aliases_and_blocks_missing_jump() {
    let p=parse("openssh","Host jump\nHostname bastion\nUser ops\nHost target\nHostname target\nUser deploy\nProxyJump jump\nHost unknown\nUser ops\nProxyJump absent\n");
    assert_eq!(
        p.profiles[1].jump_host_id.as_deref(),
        Some(p.profiles[0].id.as_str())
    );
    assert!(!p.profiles[2].enabled);
}
#[test]
fn empty_password_explicitly_clears_keyring_entry() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut s = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let mut p = profile("p");
    p.password = Some("old".into());
    s.apply(vec![p], ImportStrategy::Merge).unwrap();
    let mut p = s.profiles()[0].clone();
    p.password = Some(String::new());
    s.apply(vec![p], ImportStrategy::Merge).unwrap();
    assert!(!s.profiles()[0].has_password);
    assert!(vault.0.lock().unwrap().is_empty());
}
#[test]
fn csv_marked_passphrase_decodes_without_changing_password_spaces() {
    let p=parse("netcatty","Label,Hostname/IP,Username,KeyPath,Passphrase\nkey,host,ops,C:/keys/id,__netcatty_csv_passphrase_v1__:%2Bhello%20world\n");
    assert_eq!(p.profiles[0].passphrase.as_deref(), Some("+hello world"));
}

#[test]
fn xshell_folder_imports_nested_groups_and_never_exports_encrypted_password() {
    let dir = tempfile::tempdir().unwrap();
    let folder = dir.path().join("Sessions");
    std::fs::create_dir_all(folder.join("生产/数据库")).unwrap();
    std::fs::write(folder.join("生产/数据库/db.xsh"),"[CONNECTION]\nHost=db.test\nPort=22\n[CONNECTION:AUTHENTICATION]\nUserName=ops\nPassword=encrypted-never-plain\n").unwrap();
    std::fs::write(folder.join("ignored.txt"), "ignore").unwrap();
    let p = importers::preview_file("xshell", &folder, dir.path()).unwrap();
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].group, "生产/数据库");
    assert_eq!(p.profiles[0].name, "db");
    assert!(p.profiles[0].password.is_none());
    assert!(!serde_json::to_string(&p)
        .unwrap()
        .contains("encrypted-never-plain"));
}
#[test]
fn xshell_utf16_session_imports_unicode_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("数据库.xsh");
    let text="[CONNECTION]\nHost=db.test\nPort=2222\n[CONNECTION:AUTHENTICATION]\nUserName=运维\nPassword=encrypted\n";
    let bytes: Vec<u8> = [0xff, 0xfe]
        .into_iter()
        .chain(text.encode_utf16().flat_map(u16::to_le_bytes))
        .collect();
    std::fs::write(&path, bytes).unwrap();
    let p = importers::preview_file("xshell", &path, dir.path()).unwrap();
    assert_eq!(p.profiles[0].username, "运维");
    assert_eq!(p.profiles[0].port, 2222);
}
#[test]
fn xshell_zip_xts_imports_grouped_sessions_without_extracting_paths() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export.xts");
    let file = std::fs::File::create(&path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    zip.start_file("Prod/db.xsh", zip::write::SimpleFileOptions::default())
        .unwrap();
    use std::io::Write;
    zip.write_all(b"[CONNECTION]\nHost=db\nPort=22\n[CONNECTION:AUTHENTICATION]\nUserName=ops\nPassword=ciphertext\n").unwrap();
    zip.start_file("../../escape.xsh", zip::write::SimpleFileOptions::default())
        .unwrap();
    zip.write_all(b"not a session").unwrap();
    zip.finish().unwrap();
    let p = importers::preview_file("xshell", &path, dir.path()).unwrap();
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].group, "Prod");
    assert!(p.issues.iter().any(|i| i.message.contains("路径")));
    assert!(!dir.path().parent().unwrap().join("escape.xsh").exists());
}
#[test]
fn xshell_nonzip_xts_returns_explicit_container_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export.xts");
    std::fs::write(&path, b"encrypted nonzip container").unwrap();
    let error = importers::preview_file("xshell", &path, dir.path())
        .err()
        .unwrap();
    assert!(error.contains("容器"));
}

#[test]
fn transfer_roots_roundtrip_through_json_and_storage() {
    let dir = tempfile::tempdir().unwrap();
    let text=serde_json::json!([{ "name":"deploy","host":"host","username":"ops","allowedLocalPaths":[dir.path().to_string_lossy()],"allowedRemotePaths":["/srv/app","C:/deploy"]}]).to_string();
    let p = parse("mcp-json", &text);
    assert_eq!(
        p.profiles[0].allowed_remote_paths.as_ref().unwrap(),
        &vec!["/srv/app".to_string(), "C:/deploy".to_string()]
    );
    let mut store = AppStore::open(dir.path().join("data"), Box::new(Vault::default())).unwrap();
    let saved = store.apply(p.profiles, ImportStrategy::Merge).unwrap();
    let json = serde_json::to_value(&saved[0]).unwrap();
    assert_eq!(
        json["allowedLocalPaths"][0],
        dir.path().to_string_lossy().as_ref()
    );
    assert_eq!(
        store
            .resolve(&saved[0].id)
            .unwrap()
            .allowed_remote_paths
            .as_ref()
            .unwrap()
            .len(),
        2
    );
}
#[test]
fn transfer_roots_reject_relative_missing_and_parent_traversal() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AppStore::open(dir.path().join("data"), Box::new(Vault::default())).unwrap();
    for roots in [
        vec!["relative".into()],
        vec![dir.path().join("missing").to_string_lossy().into()],
    ] {
        let mut p = profile("p");
        p.allowed_local_paths = Some(roots);
        assert!(store.apply(vec![p], ImportStrategy::Merge).is_err());
    }
    for roots in [vec!["relative".into()], vec!["/srv/../etc".into()]] {
        let mut p = profile("p");
        p.allowed_remote_paths = Some(roots);
        assert!(store.apply(vec![p], ImportStrategy::Merge).is_err());
    }
}

#[cfg(windows)]
#[test]
fn windows_keyring_roundtrip_excludes_disk_secrets() {
    struct Cleanup {
        key: String,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            use crate::credentials::OsSecretStore;
            let _ = OsSecretStore.delete(&self.key);
        }
    }
    use crate::credentials::{credential_key, OsSecretStore};
    let dir = tempfile::tempdir().unwrap();
    let mut p = profile("keyring-test");
    let _cleanup = Cleanup {
        key: credential_key(&p.id, "password"),
    };
    p.password = Some("synthetic-keyring-test".into());
    let id = p.id.clone();
    let mut store = AppStore::open(dir.path().into(), Box::new(OsSecretStore)).unwrap();
    store.apply(vec![p], ImportStrategy::Merge).unwrap();
    let loaded = AppStore::open(dir.path().into(), Box::new(OsSecretStore)).unwrap();
    assert_eq!(
        loaded.resolve(&id).unwrap().password.as_deref(),
        Some("synthetic-keyring-test")
    );
    assert!(
        !String::from_utf8(crate::sqlite_store::committed_bytes(dir.path()).unwrap())
            .unwrap()
            .contains("synthetic-keyring-test")
    );
    store.delete(&id).unwrap();
    assert!(OsSecretStore
        .get(&credential_key(&id, "password"))
        .unwrap()
        .is_none());
}

#[test]
fn config_sync_handles_inline_tables_and_refuses_scalar_servers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path,"mcp_servers = { other = { command = 'other' }, astralink = { command = 'old', http_headers = { 'X-Test' = 'kept' } } }\n").unwrap();
    config_sync::register(&path, 18766, "synthetic-token").unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    let doc = text.parse::<toml_edit::DocumentMut>().unwrap();
    assert_eq!(
        doc["mcp_servers"]["other"]["command"].as_str(),
        Some("other")
    );
    assert_eq!(
        doc["mcp_servers"]["astralink"]["http_headers"]["X-Test"].as_str(),
        Some("kept")
    );
    std::fs::write(&path, "mcp_servers = 1\n").unwrap();
    assert!(config_sync::register(&path, 18766, "token").is_err());
    assert_eq!(std::fs::read_to_string(path).unwrap(), "mcp_servers = 1\n");
}
#[test]
fn failed_metadata_replacement_rolls_back_updated_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let mut p = profile("p");
    p.password = Some("old".into());
    store.apply(vec![p], ImportStrategy::Merge).unwrap();
    let mut p = store.profiles()[0].clone();
    p.password = Some("new".into());
    let db = rusqlite::Connection::open(dir.path().join("astralink.db")).unwrap();
    db.execute_batch("CREATE TRIGGER deny_update BEFORE INSERT ON servers BEGIN SELECT RAISE(ABORT, 'fixture'); END;").unwrap();
    assert!(store.apply(vec![p], ImportStrategy::Merge).is_err());
    assert_eq!(vault.0.lock().unwrap().values().next().unwrap(), "old");
    assert_eq!(store.profiles().len(), 1);
}
#[test]
fn xshell_gbk_text_preserves_chinese_name() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.xsh");
    let (text, _, errors) = encoding_rs::GBK
        .encode("[CONNECTION]\nHost=db.test\n[CONNECTION:AUTHENTICATION]\nUserName=运维\n");
    assert!(!errors);
    std::fs::write(&path, text.as_ref()).unwrap();
    let p = importers::preview_file("xshell", &path, dir.path()).unwrap();
    assert_eq!(p.profiles[0].username, "运维");
}

#[test]
fn unsupported_preview_stats_count_disabled_rows_once() {
    let p=parse("netcatty","Label,Hostname/IP,Username,Protocol,Proxy\nproxy,host,ops,ssh,socks5://127.0.0.1:1080\ntelnet,legacy,ops,telnet,\n");
    assert_eq!(p.stats.total, 2);
    assert_eq!(p.stats.unsupported, 2);
    assert_eq!(p.stats.ready + p.stats.needs_credential, 0);
}

#[test]
fn discovers_existing_codex_ssh_servers_json() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    std::fs::write(dir.path().join(".codex/ssh-servers.json"), "[]").unwrap();
    let sources = crate::discovery::discover(dir.path(), None, None);
    assert!(sources
        .iter()
        .any(|s| s.kind == "mcp-json" && s.exists && s.path.ends_with("ssh-servers.json")));
}

#[test]
fn discovered_xshell_count_includes_nested_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let sessions = dir.path().join("NetSarang Computer/8/Xshell/Sessions");
    std::fs::create_dir_all(sessions.join("生产/数据库")).unwrap();
    std::fs::write(
        sessions.join("生产/数据库/db.xsh"),
        "[CONNECTION]\nHost=db\n",
    )
    .unwrap();
    let sources = crate::discovery::discover(dir.path(), None, Some(dir.path()));
    assert_eq!(
        sources
            .iter()
            .find(|s| s.kind == "xshell" && s.exists)
            .unwrap()
            .profile_count,
        Some(1)
    );
}

#[test]
fn registering_an_existing_disabled_astralink_entry_enables_it() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("config.toml");
    std::fs::write(
        &file,
        "[mcp_servers.astralink]\nenabled = false\nurl = 'http://127.0.0.1:1234/mcp'\n",
    )
    .unwrap();
    crate::config_sync::register(&file, 18766, "test-token").unwrap();
    let output = std::fs::read_to_string(&file).unwrap();
    let value: toml_edit::DocumentMut = output.parse().unwrap();
    assert_eq!(
        value["mcp_servers"]["astralink"]["enabled"].as_bool(),
        Some(true)
    );
}

#[test]
fn batch_delete_checks_all_ids_before_changing_state() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let mut store = AppStore::open(dir.path().into(), Box::new(vault.clone())).unwrap();
    let mut host = profile("one");
    host.password = Some("fixture-secret".into());
    store
        .apply(vec![host.clone()], ImportStrategy::Merge)
        .unwrap();
    assert!(store.delete_many(&[]).is_err());
    assert!(store
        .delete_many(&[host.id.clone(), "missing".into()])
        .is_err());
    assert_eq!(store.profiles().len(), 1);
    assert_eq!(
        store.resolve(&host.id).unwrap().password.as_deref(),
        Some("fixture-secret")
    );
    store
        .delete_many(&[host.id.clone(), host.id.clone()])
        .unwrap();
    assert!(store.profiles().is_empty());
    assert!(vault
        .0
        .lock()
        .unwrap()
        .get(&crate::credentials::credential_key(&host.id, "password"))
        .is_none());
}
#[test]
fn batch_delete_allows_targets_and_jump_host_together_but_rejects_retained_dependents() {
    let dir = tempfile::tempdir().unwrap();
    let mut store = AppStore::open(dir.path().into(), Box::new(Vault::default())).unwrap();
    let jump = profile("jump");
    let mut target = profile("target");
    target.jump_host_id = Some(jump.id.clone());
    store
        .apply(vec![jump.clone(), target.clone()], ImportStrategy::Merge)
        .unwrap();
    assert!(store.delete_many(&[jump.id.clone()]).is_err());
    assert_eq!(store.profiles().len(), 2);
    store.delete_many(&[jump.id, target.id]).unwrap();
    assert!(store.profiles().is_empty());
}

#[test]
fn client_registration_merges_only_astralink_and_keeps_backup() {
    let dir = tempfile::tempdir().unwrap();
    for client in [
        "claude-code",
        "cursor",
        "vscode",
        "windsurf",
        "antigravity",
        "opencode",
        "openclaw",
        "hermes",
    ] {
        let path = dir.path().join(format!("{client}.json"));
        let original = r#"{"theme":"dark","mcpServers":{"other":{"command":"other"}},"servers":{"other":{"url":"other"}},"mcp":{"servers":{"other":{"url":"other"}},"other":{"type":"local"}},"mcp_servers":{"other":{"url":"other"}}}"#;
        std::fs::write(&path, original).unwrap();
        let result =
            crate::client_setup::register(client, &path, 23456, "test-only-token").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let data: serde_json::Value = if client == "hermes" {
            serde_yaml_ng::from_str(&content).unwrap()
        } else {
            serde_json::from_str(&content).unwrap()
        };
        assert_eq!(data["theme"], "dark");
        assert_eq!(data["mcpServers"]["other"]["command"], "other");
        assert!(content.contains("127.0.0.1:23456/mcp"));
        assert!(content.contains("Bearer test-only-token"));
        assert_eq!(
            std::fs::read_to_string(result.backup_path.unwrap()).unwrap(),
            original
        );
        let unchanged =
            crate::client_setup::register(client, &path, 23456, "test-only-token").unwrap();
        assert!(!unchanged.reload_required);
    }
}
#[test]
fn claude_desktop_registration_uses_local_stdio_bridge_and_preserves_other_servers() {
    let dir = tempfile::tempdir().unwrap();
    let node = dir.path().join("node.exe");
    let bridge = dir.path().join("stdio-bridge.mjs");
    std::fs::write(&node, "node").unwrap();
    std::fs::write(&bridge, "bridge").unwrap();
    let path = dir.path().join("claude_desktop_config.json");
    let original = r#"{"mcpServers":{"other":{"command":"helper"}}}"#;
    std::fs::write(&path, original).unwrap();
    let result = crate::client_setup::register_with_bridge(
        "claude-desktop",
        &path,
        18766,
        "fixture-token",
        Some((&node, &bridge)),
    )
    .unwrap();
    assert_eq!(
        std::fs::read_to_string(result.backup_path.unwrap()).unwrap(),
        original
    );
    let data: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(data["mcpServers"]["other"]["command"], "helper");
    assert_eq!(
        data["mcpServers"]["astralink"]["command"],
        node.to_string_lossy().as_ref()
    );
    assert_eq!(
        data["mcpServers"]["astralink"]["args"][0],
        bridge.to_string_lossy().as_ref()
    );
    assert_eq!(
        data["mcpServers"]["astralink"]["env"]["ASTRALINK_MCP_TOKEN"],
        "fixture-token"
    );
    assert!(
        !crate::client_setup::register_with_bridge(
            "claude-desktop",
            &path,
            18766,
            "fixture-token",
            Some((&node, &bridge))
        )
        .unwrap()
        .reload_required
    );
    assert!(crate::client_setup::register_with_bridge(
        "claude-desktop",
        &path,
        18766,
        "fixture-token",
        None
    )
    .is_err());
    let workbuddy = dir.path().join("workbuddy.json");
    crate::client_setup::register_with_bridge(
        "workbuddy",
        &workbuddy,
        18766,
        "fixture-token",
        Some((&node, &bridge)),
    )
    .unwrap();
    let config: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&workbuddy).unwrap()).unwrap();
    assert_eq!(
        config["mcpServers"]["astralink"]["command"],
        node.to_string_lossy().as_ref()
    );
}
#[test]
fn client_registration_keeps_invalid_config_and_rejects_unsupported_client() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mcp.json");
    for invalid in [
        "broken",
        r#"{"mcpServers":[]}"#,
        r#"{"mcpServers":{"astralink":42}}"#,
    ] {
        std::fs::write(&path, invalid).unwrap();
        assert!(crate::client_setup::register("cursor", &path, 18766, "token").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), invalid);
    }
    assert!(crate::client_setup::register("chatgpt", &path, 18766, "token").is_err());
    assert!(crate::client_setup::register("cursor", &path, 18766, "").is_err());
    assert!(crate::client_setup::register("cursor", &path, 0, "token").is_err());
}
#[test]
fn hermes_registration_keeps_yaml_settings_and_other_servers() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.yaml");
    std::fs::write(
        &path,
        "model: custom
mcp_servers:
  other:
    command: helper
",
    )
    .unwrap();
    crate::client_setup::register("hermes", &path, 18766, "fixture-token").unwrap();
    let data: serde_json::Value =
        serde_yaml_ng::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(data["model"], "custom");
    assert_eq!(data["mcp_servers"]["other"]["command"], "helper");
    assert_eq!(
        data["mcp_servers"]["astralink"]["url"],
        "http://127.0.0.1:18766/mcp"
    );
}

#[test]
fn client_default_paths_keep_existing_jsonc_instead_of_shadowing_it() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join(".config/opencode/opencode.jsonc");
    std::fs::create_dir_all(config.parent().unwrap()).unwrap();
    std::fs::write(&config, "{ // existing config\n}").unwrap();
    assert_eq!(
        crate::client_setup::default_path("opencode", dir.path(), None).unwrap(),
        config
    );
    assert_eq!(
        crate::client_setup::default_path("cursor", dir.path(), None).unwrap(),
        dir.path().join(".cursor/mcp.json")
    );
    assert_eq!(
        crate::client_setup::default_path("workbuddy", dir.path(), None).unwrap(),
        dir.path().join(".workbuddy/mcp.json")
    );
    if cfg!(windows) {
        assert!(crate::client_setup::default_path("vscode", dir.path(), None).is_err());
    } else {
        assert!(crate::client_setup::default_path("vscode", dir.path(), None).is_ok());
    }
    assert!(crate::client_setup::default_path("windsurf", dir.path(), None).is_err());
}
#[test]
fn finalshell_directory_imports_ssh_groups_without_decrypting_passwords() {
    let dir = tempfile::tempdir().unwrap();
    let conn = dir.path().join("conn");
    std::fs::create_dir_all(&conn).unwrap();
    std::fs::write(
        conn.join("folder.json"),
        r#"{"id":"f1","name":"生产","parent_id":"root"}"#,
    )
    .unwrap();
    std::fs::write(conn.join("one_connect_config.json"), r#"{"name":"web","host":"example.org","port":2222,"user_name":"ops","parent_id":"f1","conection_type":100,"password":"encrypted-fixture"}"#).unwrap();
    std::fs::write(
        conn.join("other_connect_config.json"),
        r#"{"name":"telnet","host":"example.org","conection_type":101}"#,
    )
    .unwrap();
    let preview = crate::importers::preview_file("finalshell", &conn, dir.path()).unwrap();
    assert_eq!(preview.profiles.len(), 1);
    assert_eq!(preview.profiles[0].group, "生产");
    assert_eq!(preview.profiles[0].port, 2222);
    assert!(preview.profiles[0].password.is_none());
    assert_eq!(preview.stats.needs_credential, 1);
    assert_eq!(preview.stats.unsupported, 1);
}
#[test]
fn nyaterm_json_imports_password_sessions_and_marks_inline_keys_for_reselection() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("sessions.json");
    let input = r#"{"version":1,"passwords":[{"ref":"p1","name":"ops","password":"fixture-secret"}],"ssh_keys":[{"ref":"k1","name":"ops-key","private_key":"-----BEGIN OPENSSH PRIVATE KEY-----fixture"}],"sessions":[{"name":"web","type":"ssh","group_path":["Production","Web"],"host":"example.org","username":"ops","auth":{"mode":"password","password_ref":"p1"}},{"name":"db","type":"ssh","host":"db.example.org","username":"root","auth":{"mode":"key","key_ref":"k1"}},{"name":"local","type":"local_terminal"}]}"#;
    std::fs::write(&file, input).unwrap();
    let preview = crate::importers::preview_file("nyaterm", &file, dir.path()).unwrap();
    assert_eq!(preview.profiles.len(), 2);
    assert_eq!(preview.profiles[0].group, "Production/Web");
    assert_eq!(
        preview.profiles[0].password.as_deref(),
        Some("fixture-secret")
    );
    assert!(!preview.profiles[1].enabled);
    assert!(preview
        .issues
        .iter()
        .any(|issue| issue.message.contains("重新选择本地私钥")));
    assert_eq!(preview.stats.unsupported, 2);
}
#[test]
fn nyaterm_redb_imports_ssh_metadata_without_reading_credentials_or_changing_source() {
    use redb::TableDefinition;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("nyaterm.redb");
    let db = redb::Database::create(&path).unwrap();
    let tx = db.begin_write().unwrap();
    {
        let mut meta = tx
            .open_table(TableDefinition::<&str, &str>::new("meta"))
            .unwrap();
        meta.insert("schema_version", "3").unwrap();
        let mut groups = tx
            .open_table(TableDefinition::<&str, &[u8]>::new("groups"))
            .unwrap();
        groups
            .insert("groups/root", r#"{"id":"root","name":"生产"}"#.as_bytes())
            .unwrap();
        groups
            .insert(
                "groups/web",
                r#"{"id":"web","name":"Web","parent_id":"root"}"#.as_bytes(),
            )
            .unwrap();
        let mut connections = tx
            .open_table(TableDefinition::<&str, &[u8]>::new("connections"))
            .unwrap();
        connections.insert("connections/one", r#"{"id":"one","name":"web-01","type":"ssh","host":"web.example.org","port":2222,"username":"deploy","group_id":"web","auth":{"mode":"password","has_password":true}}"#.as_bytes()).unwrap();
        connections
            .insert(
                "connections/two",
                r#"{"id":"two","name":"local","type":"local_terminal"}"#.as_bytes(),
            )
            .unwrap();
        let mut credentials = tx
            .open_table(TableDefinition::<&str, &[u8]>::new("credentials"))
            .unwrap();
        credentials
            .insert(
                "credentials/connection-password/one",
                b"encrypted-fixture".as_slice(),
            )
            .unwrap();
    }
    tx.commit().unwrap();
    drop(db);
    let original = std::fs::read(&path).unwrap();
    let preview = crate::importers::preview_file("nyaterm", &path, dir.path()).unwrap();
    assert_eq!(preview.profiles.len(), 1);
    assert_eq!(preview.profiles[0].group, "生产/Web");
    assert_eq!(preview.profiles[0].port, 2222);
    assert!(preview.profiles[0].password.is_none());
    assert_eq!(preview.stats.needs_credential, 1);
    assert_eq!(preview.stats.unsupported, 1);
    assert_eq!(std::fs::read(&path).unwrap(), original);
}
#[test]
fn mobaxterm_bookmarks_keep_nested_group_and_skip_other_protocols() {
    let input = "[Bookmarks]\nSubRep=Production\\Web\nweb=#109#0%web.example.org%2222%deploy%\nother=#111#0%telnet.example.org%23%ops%\n";
    let p = crate::importers::parse(
        "mobaxterm",
        input,
        Path::new("hosts.mxtsessions"),
        Path::new("."),
    )
    .unwrap();
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].group, "Production/Web");
    assert_eq!(p.profiles[0].host, "web.example.org");
    assert_eq!(p.profiles[0].port, 2222);
    assert_eq!(p.profiles[0].username, "deploy");
    assert_eq!(p.stats.unsupported, 1);
}

#[test]
fn electerm_bookmarks_resolve_group_and_skip_non_ssh() {
    let input = r#"{"bookmarkGroups":[{"id":"parent","title":"Production","bookmarkGroupIds":["child"]},{"id":"child","title":"Web","bookmarkIds":["web"]}],"bookmarks":[{"id":"web","title":"web-01","type":"ssh","enableSsh":true,"host":"web.example.org","username":"deploy","port":2200,"authType":"password"},{"title":"db","type":"rdp","host":"db.example.org"}]}"#;
    let p = crate::importers::parse(
        "electerm",
        input,
        Path::new("bookmarks.json"),
        Path::new("."),
    )
    .unwrap();
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].group, "Production/Web");
    assert_eq!(p.profiles[0].port, 2200);
    assert_eq!(p.stats.unsupported, 1);
}

#[test]
fn securecrt_xml_reads_nested_ssh2_only() {
    let input = r#"<VanDyke><key name="Sessions"><key name="Production"><key name="web"><string name="Protocol Name">SSH2</string><string name="Hostname">web.example.org</string><dword name="[SSH2] Port">2200</dword><string name="Username">deploy</string></key><key name="telnet"><string name="Protocol Name">Telnet</string><string name="Hostname">other</string></key></key></key></VanDyke>"#;
    let p = crate::importers::parse(
        "securecrt",
        input,
        Path::new("sessions.xml"),
        Path::new("."),
    )
    .unwrap();
    assert_eq!(p.profiles.len(), 1);
    assert_eq!(p.profiles[0].group, "Production");
    assert_eq!(p.profiles[0].port, 2200);
    assert_eq!(p.stats.unsupported, 1);
}

#[test]
fn windterm_sessions_use_plain_auto_login_and_require_master_password_when_configured() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("terminal")).unwrap();
    let sessions = dir.path().join("terminal/user.sessions");
    std::fs::write(&sessions, r#"[{"session.protocol":"SSH","session.target":"deploy@web.example.org","session.label":"web","session.group":"Production>Web","session.autoLogin":"{\"PasswordEnabled\":true,\"Password\":\"test-fixture\"}"}]"#).unwrap();
    let p = crate::importers::preview_file("windterm", &sessions, dir.path()).unwrap();
    assert_eq!(p.profiles[0].username, "deploy");
    assert_eq!(p.profiles[0].group, "Production/Web");
    assert_eq!(p.profiles[0].password.as_deref(), Some("test-fixture"));
    std::fs::write(
        dir.path().join("user.config"),
        r#"{"application.fingerprint":"fixture","application.masterPassword":true}"#,
    )
    .unwrap();
    assert!(
        crate::importers::preview_file("windterm", &sessions, dir.path())
            .err()
            .is_some_and(|error| error.contains("主密码"))
    );
    assert!(crate::importers::preview_file_with_password(
        "windterm",
        &sessions,
        dir.path(),
        Some("fixture-master")
    )
    .is_ok());
}
