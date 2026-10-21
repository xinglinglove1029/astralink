use astralink_lib::{
    credentials::OsSecretStore,
    engine::{resolve_paths, EngineManager},
    storage::AppStore,
};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let root =
        PathBuf::from(std::env::var_os("ASTRALINK_TEST_INSTALLED_DIR").expect("installed dir"));
    let data = PathBuf::from(std::env::var_os("ASTRALINK_TEST_DATA_DIR").expect("data dir"));
    let paths = resolve_paths(None, &root, None).expect("bundled resources");
    let store = AppStore::open(data, Box::new(OsSecretStore)).expect("profile store");
    let profiles = store.runtime_profiles().expect("profile credentials");
    let token = store.mcp_token().expect("local token");
    println!(
        "profiles={}, token_format_valid={}",
        profiles.len(),
        token.len() > 20
            && token
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    );
    let manager =
        EngineManager::new(paths, store.settings().mcp_port, String::new()).expect("manager");
    manager
        .configure(store.settings().mcp_port, token)
        .await
        .expect("configure");
    let empty = manager.start(vec![]).await;
    println!("empty_start={}, error={:?}", empty.running, empty.error);
    if empty.running {
        manager.stop().await;
    }
    let full = manager.start(profiles).await;
    println!("full_start={}, error={:?}", full.running, full.error);
    if full.running {
        manager.stop().await;
    }
}
