pub mod client_setup;
pub mod cloud_backup;
pub mod commands;
pub mod config_sync;
#[cfg(test)]
mod core_tests;
pub mod credentials;
pub mod desktop;
pub mod discovery;
pub mod engine;
pub mod importers;
pub mod migration;
pub mod models;
pub mod runtime_logs;
pub mod sqlite_store;
pub mod storage;
pub mod tabby_credentials;
pub mod web_access;
pub mod web_server;

use commands::AppCore;
use std::path::PathBuf;
use tauri::Manager;

/// Shared builder is exposed so native integration can add tray/window lifecycle behavior.
pub fn builder() -> tauri::Builder<tauri::Wry> {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let home = std::env::var_os("USERPROFILE")
                .or_else(|| std::env::var_os("HOME"))
                .map(PathBuf::from)
                .ok_or_else(|| std::io::Error::other("无法定位用户目录"))?;
            let resource_dir = app.path().resource_dir()?;
            let dev_root = if cfg!(debug_assertions) {
                PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                    .parent()
                    .map(ToOwned::to_owned)
            } else {
                None
            };
            let node_override = std::env::var_os("ASTRALINK_NODE_BINARY").map(PathBuf::from);
            let paths = engine::candidate_paths(
                dev_root.as_deref(),
                &resource_dir,
                node_override.as_deref(),
            );
            let codex_home = std::env::var_os("CODEX_HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex"));
            let default_data_dir = app.path().app_data_dir()?;
            let data_dir = if cfg!(debug_assertions) {
                std::env::var_os("ASTRALINK_DATA_DIR")
                    .map(PathBuf::from)
                    .unwrap_or_else(|| default_data_dir.clone())
            } else {
                default_data_dir.clone()
            };
            let previous_dir = (data_dir == default_data_dir)
                .then(|| migration::previous_data_directory(&default_data_dir));
            migration::prepare_data_directory(&data_dir, previous_dir.as_deref())
                .map_err(std::io::Error::other)?;
            let core = AppCore::open(
                data_dir,
                Box::new(migration::secret_store()),
                paths,
                home,
                std::env::var_os("APPDATA").map(PathBuf::from),
                app.path().document_dir().ok(),
                codex_home.join("config.toml"),
            )
            .map_err(std::io::Error::other)?;
            core.engine.attach(app.handle().clone());
            app.manage(core);
            desktop::install(app.handle())?;
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let core = handle.state::<AppCore>();
                if core
                    .store()
                    .is_ok_and(|store| store.settings().start_on_launch)
                {
                    let _ = commands::start_engine(core).await;
                }
            });
            Ok(())
        })
        .on_window_event(desktop::window_event)
        .invoke_handler(tauri::generate_handler![
            commands::get_app_state,
            commands::get_storage_info,
            commands::get_runtime_logs,
            commands::export_runtime_logs,
            commands::discover_sources,
            commands::preview_import,
            commands::apply_import,
            commands::upsert_profile,
            commands::reveal_profile_secret,
            commands::delete_profile,
            commands::delete_profiles,
            commands::update_settings,
            commands::start_engine,
            commands::stop_engine,
            commands::get_engine_status,
            commands::test_connection,
            commands::submit_otp,
            commands::cancel_auth,
            commands::register_codex,
            commands::register_mcp_client,
            commands::get_mcp_token,
            commands::get_mcp_bridge_paths,
            commands::preview_webdav_backup,
            commands::backup_to_webdav,
            commands::restore_webdav_backup,
            commands::export_backup_file,
            commands::preview_backup_file,
            commands::restore_backup_file,
        ])
}
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    match builder().build(tauri::generate_context!()) {
        Ok(app) => app.run(desktop::run_event),
        Err(_) => eprintln!("AstraLink 启动失败，请检查应用数据和桌面运行环境"),
    }
}
