use super::*;
use crate::credentials::SecretStore;
use std::{
    collections::HashMap,
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
fn core(dir: &std::path::Path, vault: Vault) -> AppCore {
    AppCore::open(
        dir.join("data"),
        Box::new(vault),
        crate::engine::candidate_paths(Some(dir), dir, None),
        dir.into(),
        None,
        None,
        dir.join("codex/config.toml"),
    )
    .unwrap()
}
#[test]
fn preview_hides_secrets_but_apply_writes_them_to_vault() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    std::fs::write(
        &path,
        r#"[{"name":"prod","host":"prod.test","username":"ops","password":"preview-test-secret"}]"#,
    )
    .unwrap();
    let vault = Vault::default();
    let core = core(dir.path(), vault.clone());
    let preview = core.preview("mcp-json", &path).unwrap();
    assert!(preview.profiles[0].password.is_none());
    assert!(preview.profiles[0].has_password);
    assert!(!serde_json::to_string(&preview)
        .unwrap()
        .contains("preview-test-secret"));
    let saved = core.apply(preview.profiles, ImportStrategy::Merge).unwrap();
    assert!(saved[0].password.is_none());
    assert_eq!(
        vault.0.lock().unwrap().values().next().unwrap(),
        "preview-test-secret"
    );
}
#[test]
fn saving_an_edited_profile_keeps_its_totp_secret_in_the_vault() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::default();
    let core = core(dir.path(), vault.clone());
    let id = uuid::Uuid::new_v4().to_string();
    let seed = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";
    let saved = core
        .apply(
            vec![ConnectionProfile {
                id: id.clone(),
                name: "jump".into(),
                host: "example.test".into(),
                username: "ops".into(),
                try_keyboard: true,
                totp_secret: Some(seed.into()),
                ..Default::default()
            }],
            ImportStrategy::Merge,
        )
        .unwrap();
    assert!(saved[0].has_totp_secret);
    assert_eq!(
        core.store()
            .unwrap()
            .read_secret(&id, "totpSecret")
            .unwrap()
            .as_deref(),
        Some(seed)
    );
    assert!(!String::from_utf8(
        crate::sqlite_store::committed_bytes(&dir.path().join("data")).unwrap()
    )
    .unwrap()
    .contains(seed));
    let edited = core.store().unwrap().profiles();
    assert!(edited[0].totp_secret.is_none());
    assert!(core.apply(edited, ImportStrategy::Merge).unwrap()[0].has_totp_secret);
}
#[test]
fn preview_credentials_are_not_reused_for_changed_host() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    std::fs::write(
        &path,
        r#"[{"name":"prod","host":"prod.test","username":"ops","password":"secret"}]"#,
    )
    .unwrap();
    let vault = Vault::default();
    let core = core(dir.path(), vault.clone());
    let mut preview = core.preview("mcp-json", &path).unwrap();
    preview.profiles[0].host = "different.test".into();
    assert!(core.apply(preview.profiles, ImportStrategy::Merge).is_err());
    assert!(vault.0.lock().unwrap().is_empty());
}
#[test]
fn expired_preview_rejects_claimed_credentials_instead_of_saving_missing_password() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("connections.json");
    std::fs::write(
        &path,
        r#"[{"name":"prod","host":"prod.test","username":"ops","password":"secret"}]"#,
    )
    .unwrap();
    let core = core(dir.path(), Vault::default());
    let preview = core.preview("mcp-json", &path).unwrap();
    core.pending.lock().unwrap().as_mut().unwrap().expires =
        std::time::Instant::now() - std::time::Duration::from_secs(1);
    assert!(core.apply(preview.profiles, ImportStrategy::Merge).is_err());
}
#[test]
#[ignore = "explicit local metadata QA via ASTRALINK_TEST_TABBY"]
fn local_tabby_whole_preview() {
    let path = std::path::PathBuf::from(
        std::env::var_os("ASTRALINK_TEST_TABBY").expect("ASTRALINK_TEST_TABBY"),
    );
    let dir = tempfile::tempdir().unwrap();
    let core = core(dir.path(), Vault::default());
    let preview = core.preview("tabby", &path).unwrap();
    assert_eq!(preview.profiles.len(), 21);
    assert!(preview
        .profiles
        .iter()
        .all(|p| p.password.is_none() && p.passphrase.is_none()));
    println!(
        "Tabby preview: profiles={}, issues={}, ready={}, needsCredential={}",
        preview.profiles.len(),
        preview.issues.len(),
        preview.stats.ready,
        preview.stats.needs_credential
    );
}

#[tokio::test]
async fn rejected_engine_update_rolls_back_metadata_and_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    let paths = crate::engine::resolve_paths(
        Some(&root),
        dir.path(),
        std::env::var_os("ASTRALINK_NODE_BINARY")
            .map(std::path::PathBuf::from)
            .as_deref(),
    )
    .unwrap();
    let vault = Vault::default();
    let core = AppCore::open(
        dir.path().join("data"),
        Box::new(vault.clone()),
        paths,
        dir.path().into(),
        None,
        None,
        dir.path().join("codex/config.toml"),
    )
    .unwrap();
    let id = uuid::Uuid::new_v4().to_string();
    let original = ConnectionProfile {
        id: id.clone(),
        name: "source".into(),
        host: "example.invalid".into(),
        port: 22,
        username: "ops".into(),
        password: Some("old-secret".into()),
        enabled: true,
        ..Default::default()
    };
    core.store()
        .unwrap()
        .apply(vec![original], ImportStrategy::Merge)
        .unwrap();
    let port = std::net::TcpListener::bind(("127.0.0.1", 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let token = core.store().unwrap().mcp_token().unwrap();
    core.engine.configure(port, token).await.unwrap();
    assert!(
        core.engine
            .start(core.store().unwrap().runtime_profiles().unwrap())
            .await
            .running
    );
    let key = dir.path().join("invalid-key.pem");
    std::fs::write(&key, "not a private key").unwrap();
    let update = ConnectionProfile {
        id: id.clone(),
        name: "source".into(),
        host: "example.invalid".into(),
        port: 22,
        username: "ops".into(),
        auth_type: AuthType::PrivateKey,
        private_key_path: Some(key.to_string_lossy().into()),
        enabled: true,
        ..Default::default()
    };
    assert!(core
        .commit_and_sync(|core| core.apply(vec![update], ImportStrategy::Merge))
        .await
        .is_err());
    let restored = core.store().unwrap().resolve(&id).unwrap();
    assert_eq!(restored.auth_type, AuthType::Password);
    assert_eq!(restored.password.as_deref(), Some("old-secret"));
    assert!(core.engine.status().await.running);
    core.engine.stop().await;
}
