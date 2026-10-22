mod electerm;
mod finalshell;
mod json;
mod mobaxterm;
mod netcatty;
mod nya_backup;
mod nyaterm;
mod openssh;
mod securecrt;
mod tabby;
mod termius;
mod windterm;
mod xshell;
use crate::models::*;
use serde_json::Value;
use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};
const MAX_IMPORT_BYTES: u64 = 8 * 1024 * 1024;

pub fn parse(kind: &str, text: &str, path: &Path, home: &Path) -> Result<ImportPreview, String> {
    let mut preview = ImportPreview {
        source: kind.into(),
        source_path: path.to_string_lossy().into(),
        profiles: Vec::new(),
        issues: Vec::new(),
        stats: ImportStats::default(),
    };
    match kind {
        "mcp-json" => json::parse(text, path, home, &mut preview)?,
        "tabby" => tabby::parse(text, path, home, &mut preview)?,
        "netcatty" => netcatty::parse(text, path, home, &mut preview)?,
        "openssh" => openssh::parse(text, path, home, &mut preview)?,
        "xshell" => xshell::parse(text, path, home, &mut preview)?,
        "finalshell" => finalshell::parse(text, path, &mut preview)?,
        "nyaterm" => nyaterm::parse(text, path, home, &mut preview)?,
        "mobaxterm" => mobaxterm::parse(text, &mut preview)?,
        "electerm" => electerm::parse(text, &mut preview)?,
        "securecrt" => securecrt::parse(text, &mut preview)?,
        "windterm" => windterm::parse(text, path, home, None, &mut preview)?,
        _ => return Err("不支持的导入来源".into()),
    }
    finalize(&mut preview);
    Ok(preview)
}
pub fn preview_file(kind: &str, path: &Path, home: &Path) -> Result<ImportPreview, String> {
    preview_file_with_password(kind, path, home, None)
}

pub fn preview_file_with_password(
    kind: &str,
    path: &Path,
    home: &Path,
    password: Option<&str>,
) -> Result<ImportPreview, String> {
    if kind == "nya-backup" {
        return nya_backup::file(path, home, password);
    }
    if kind == "termius" {
        return termius::database(path);
    }
    if kind == "windterm" {
        let mut preview = ImportPreview {
            source: kind.into(),
            source_path: path.to_string_lossy().into(),
            profiles: Vec::new(),
            issues: Vec::new(),
            stats: ImportStats::default(),
        };
        windterm::parse(&read_text(path, false)?, path, home, password, &mut preview)?;
        finalize(&mut preview);
        return Ok(preview);
    }
    if kind == "nyaterm"
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("redb"))
    {
        return nyaterm::database(path);
    }
    if kind == "finalshell" && path.is_dir() {
        return finalshell::directory(path);
    }
    if kind == "xshell"
        && path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xts"))
    {
        return xshell::archive(path, home);
    }
    if kind == "xshell" && path.is_dir() {
        let mut files = Vec::new();
        collect_xshell(path, &mut files, 0)?;
        files.sort();
        let mut result = ImportPreview {
            source: kind.into(),
            source_path: path.to_string_lossy().into(),
            profiles: Vec::new(),
            issues: Vec::new(),
            stats: ImportStats::default(),
        };
        for file in files {
            match read_text(&file, true).and_then(|text| parse(kind, &text, &file, home)) {
                Ok(mut part) => {
                    for p in &mut part.profiles {
                        p.group = file
                            .parent()
                            .and_then(|p| p.strip_prefix(path).ok())
                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                            .unwrap_or_default();
                    }
                    let omitted = part.stats.total.saturating_sub(part.profiles.len());
                    result.profiles.extend(part.profiles);
                    result.issues.extend(part.issues);
                    result.stats.unsupported += omitted;
                }
                Err(_) => {
                    result.stats.unsupported += 1;
                    issue(&mut result, "error", "无法读取 Xshell 会话", None);
                }
            }
        }
        finalize(&mut result);
        return Ok(result);
    }
    parse(
        kind,
        &read_text(path, matches!(kind, "xshell" | "mobaxterm" | "securecrt"))?,
        path,
        home,
    )
}
fn collect_xshell(path: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<(), String> {
    if depth > 8 {
        return Err("Xshell 会话目录层级过多".into());
    }
    for entry in fs::read_dir(path).map_err(|_| "无法读取会话目录")? {
        let entry = entry.map_err(|_| "无法读取会话目录")?;
        let ty = entry.file_type().map_err(|_| "无法读取会话类型")?;
        if ty.is_symlink() {
            continue;
        }
        if ty.is_dir() {
            collect_xshell(&entry.path(), files, depth + 1)?
        } else if entry
            .path()
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xsh"))
        {
            files.push(entry.path());
            if files.len() > 512 {
                return Err("会话文件超过 512 个".into());
            }
        }
    }
    Ok(())
}
fn read_text(path: &Path, legacy: bool) -> Result<String, String> {
    let metadata = fs::metadata(path).map_err(|_| "导入文件不存在或无法读取")?;
    if !metadata.is_file() || metadata.len() > MAX_IMPORT_BYTES {
        return Err("请选择不超过 8 MB 的配置文件".into());
    }
    let bytes = fs::read(path).map_err(|_| "无法读取导入文件")?;
    decode_bytes(&bytes, legacy)
}
fn decode_bytes(bytes: &[u8], legacy: bool) -> Result<String, String> {
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        if bytes.len() % 2 != 0 {
            return Err("UTF-16 文件长度无效".into());
        }
        let le = bytes[0] == 0xff;
        let words: Vec<_> = bytes[2..]
            .chunks_exact(2)
            .map(|p| {
                if le {
                    u16::from_le_bytes([p[0], p[1]])
                } else {
                    u16::from_be_bytes([p[0], p[1]])
                }
            })
            .collect();
        return String::from_utf16(&words).map_err(|_| "UTF-16 编码无效".into());
    }
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(bytes);
    match std::str::from_utf8(bytes) {
        Ok(s) => Ok(s.into()),
        Err(_) if legacy => {
            let (text, _, errors) = encoding_rs::GBK.decode(bytes);
            if errors {
                Err("Xshell 文件编码无法识别".into())
            } else {
                Ok(text.into())
            }
        }
        Err(_) => Err("文件需要 UTF-8 或 UTF-16 编码".into()),
    }
}
fn issue(preview: &mut ImportPreview, severity: &str, message: &str, name: Option<&str>) {
    preview.issues.push(ImportIssue {
        severity: severity.into(),
        message: message.into(),
        profile_name: name.map(Into::into),
    });
}
fn unsupported(preview: &mut ImportPreview, message: &str, name: Option<&str>) {
    preview.stats.unsupported += 1;
    issue(preview, "warning", message, name)
}
fn text<'a>(value: &'a Value, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| value.get(*key).and_then(Value::as_str))
        .filter(|s| !s.is_empty())
}
fn optional(value: &Value, keys: &[&str]) -> Option<String> {
    text(value, keys).map(Into::into)
}
fn port(value: Option<&Value>) -> Result<u16, String> {
    match value {
        None | Some(Value::Null) => Ok(22),
        Some(Value::Number(n)) => n
            .as_u64()
            .and_then(|n| u16::try_from(n).ok())
            .filter(|n| *n > 0)
            .ok_or_else(|| "端口无效".into()),
        Some(Value::String(s)) => s
            .parse::<u16>()
            .ok()
            .filter(|n| *n > 0)
            .ok_or_else(|| "端口无效".into()),
        _ => Err("端口无效".into()),
    }
}
fn path(value: &str, source: &Path, home: &Path) -> String {
    let expanded = if value == "~" {
        home.to_path_buf()
    } else if let Some(s) = value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        home.join(s)
    } else {
        PathBuf::from(value)
    };
    if expanded.is_absolute() {
        expanded.to_string_lossy().into()
    } else {
        source
            .parent()
            .unwrap_or(home)
            .join(expanded)
            .to_string_lossy()
            .into()
    }
}
fn new_profile(name: &str, source: &str) -> ConnectionProfile {
    ConnectionProfile {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        port: 22,
        source: source.into(),
        enabled: true,
        ..Default::default()
    }
}
fn normalize(mut p: ConnectionProfile, preview: &mut ImportPreview) {
    p.name = p.name.trim().into();
    p.host = p
        .host
        .trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .into();
    p.username = p.username.trim().into();
    p.has_password = p.password.as_ref().is_some_and(|s| !s.is_empty());
    p.has_passphrase = p.passphrase.as_ref().is_some_and(|s| !s.is_empty());
    if p.name.is_empty() {
        p.name = if p.host.is_empty() {
            "未命名连接".into()
        } else {
            p.host.clone()
        }
    }
    if p.host.is_empty() || p.username.is_empty() {
        p.enabled = false;
        issue(preview, "error", "主机或用户名缺失", Some(&p.name));
    }
    preview.profiles.push(p);
}
fn disable(preview: &mut ImportPreview, p: &mut ConnectionProfile, message: &str) {
    p.enabled = false;
    issue(preview, "warning", message, Some(&p.name));
}
fn finalize(preview: &mut ImportPreview) {
    let mut seen = HashSet::new();
    for p in &mut preview.profiles {
        if !seen.insert(p.name.clone()) {
            p.enabled = false;
            preview.issues.push(ImportIssue {
                severity: "error".into(),
                message: "连接名称重复，请修改名称后导入".into(),
                profile_name: Some(p.name.clone()),
            })
        }
    }
    preview.stats.ready = 0;
    preview.stats.needs_credential = 0;
    for p in &preview.profiles {
        if !p.enabled {
            continue;
        }
        let ready = match p.auth_type {
            AuthType::Password => p.has_password,
            AuthType::PrivateKey => p
                .private_key_path
                .as_ref()
                .is_some_and(|p| Path::new(p).is_file()),
            AuthType::Agent => p.agent.is_some() || std::env::var_os("SSH_AUTH_SOCK").is_some(),
        };
        if ready {
            preview.stats.ready += 1
        } else {
            preview.stats.needs_credential += 1
        }
    }
    let omitted = preview.stats.unsupported;
    preview.stats.total = preview.profiles.len() + omitted;
    preview.stats.unsupported = omitted + preview.profiles.iter().filter(|p| !p.enabled).count();
}
