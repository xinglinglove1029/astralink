use super::*;
use std::collections::HashMap;

fn read_json(path: &Path) -> Result<Value, String> {
    let text = read_text(path, false)?;
    serde_json::from_str(&text).map_err(|_| "FinalShell JSON 格式无效".into())
}

pub(super) fn parse(input: &str, _path: &Path, preview: &mut ImportPreview) -> Result<(), String> {
    let value: Value = serde_json::from_str(input).map_err(|_| "FinalShell JSON 格式无效")?;
    connection(&value, &HashMap::new(), preview);
    Ok(())
}

fn folder_path(id: &str, folders: &HashMap<String, Value>) -> String {
    let mut chain = Vec::new();
    let mut current = id.to_owned();
    let mut seen = HashSet::new();
    while !current.is_empty() && current != "root" && current != "0" && seen.insert(current.clone())
    {
        let Some(folder) = folders.get(&current) else {
            break;
        };
        if let Some(name) = text(folder, &["name"]) {
            chain.push(name.to_owned());
        }
        current = text(folder, &["parent_id"]).unwrap_or_default().to_owned();
    }
    chain.reverse();
    chain.join("/")
}

fn connection(value: &Value, folders: &HashMap<String, Value>, preview: &mut ImportPreview) {
    if value
        .get("delete_time")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        != 0
    {
        return;
    }
    if value.get("conection_type").and_then(Value::as_i64) != Some(100) {
        unsupported(
            preview,
            "仅导入 FinalShell SSH 连接",
            text(value, &["name"]),
        );
        return;
    }
    let mut profile = new_profile(text(value, &["name"]).unwrap_or_default(), "finalshell");
    profile.host = text(value, &["host"]).unwrap_or_default().into();
    profile.username = text(value, &["user_name"]).unwrap_or("root").into();
    profile.notes = text(value, &["description"]).unwrap_or_default().into();
    profile.group = folder_path(text(value, &["parent_id"]).unwrap_or_default(), folders);
    match port(value.get("port")) {
        Ok(number) => profile.port = number,
        Err(_) => {
            unsupported(preview, "FinalShell 端口无效", Some(&profile.name));
            return;
        }
    }
    issue(
        preview,
        "warning",
        "FinalShell 保存的密码不会解密，请补充凭据",
        Some(&profile.name),
    );
    normalize(profile, preview);
}

pub(super) fn directory(root: &Path) -> Result<ImportPreview, String> {
    let mut files = Vec::new();
    collect(root, 0, &mut files)?;
    files.sort();
    let mut folders = HashMap::new();
    let mut connections = Vec::new();
    for file in files {
        let name = file
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        let value = read_json(&file)?;
        if name == "folder.json" {
            if value
                .get("delete_time")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                == 0
            {
                if let Some(id) = text(&value, &["id"]) {
                    folders.insert(id.to_owned(), value);
                }
            }
        } else {
            connections.push(value);
        }
    }
    if connections.is_empty() {
        return Err("目录中未找到 FinalShell 连接文件".into());
    }
    let mut preview = ImportPreview {
        source: "finalshell".into(),
        source_path: root.to_string_lossy().into(),
        profiles: Vec::new(),
        issues: Vec::new(),
        stats: ImportStats::default(),
    };
    for value in &connections {
        connection(value, &folders, &mut preview);
    }
    finalize(&mut preview);
    Ok(preview)
}

fn collect(dir: &Path, depth: usize, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if depth > 8 {
        return Err("FinalShell 目录层级过多".into());
    }
    for item in fs::read_dir(dir).map_err(|_| "无法读取 FinalShell 目录")? {
        let item = item.map_err(|_| "无法读取 FinalShell 目录")?;
        let ty = item
            .file_type()
            .map_err(|_| "无法读取 FinalShell 文件类型")?;
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            collect(&item.path(), depth + 1, files)?;
        } else if ty.is_file() {
            let name = item.file_name();
            let name = name.to_string_lossy();
            if name == "folder.json" || name.ends_with("_connect_config.json") {
                files.push(item.path());
                if files.len() > 512 {
                    return Err("FinalShell 配置文件超过 512 个".into());
                }
            }
        }
    }
    Ok(())
}
