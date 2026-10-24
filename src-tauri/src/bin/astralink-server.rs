use astralink_lib::{engine, web_server};
use std::{
    io::{self, Read},
    path::PathBuf,
};

#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
async fn execute() -> Result<(), String> {
    let directory = std::env::var_os("ASTRALINK_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./data"));
    match std::env::args().nth(1).as_deref() {
        Some("init-admin") => {
            let mut password = String::new();
            io::stdin()
                .read_to_string(&mut password)
                .map_err(|_| "无法读取管理员密码")?;
            web_server::initialize(&directory, password.trim_end_matches(['\r', '\n']))?;
            println!("AstraLink 管理员已初始化");
            Ok(())
        }
        Some("serve") | None => {
            let password = std::env::var("ASTRALINK_ADMIN_PASSWORD")
                .ok()
                .filter(|value| !value.is_empty());
            web_server::initialize_if_needed(&directory, password.as_deref())?;
            let root = std::env::var_os("ASTRALINK_RESOURCE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."));
            let paths = engine::resolve_paths(
                Some(&root),
                &root,
                std::env::var_os("ASTRALINK_NODE_BINARY")
                    .as_deref()
                    .map(std::path::Path::new),
            )?;
            let assets = std::env::var_os("ASTRALINK_WEB_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| root.join("dist"));
            let address =
                std::env::var("ASTRALINK_LISTEN").unwrap_or_else(|_| "127.0.0.1:1421".into());
            web_server::run(directory, assets, &address, paths).await
        }
        _ => Err("用法: astralink-server [init-admin|serve]".into()),
    }
}
