use super::*;
use std::collections::HashMap;
pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let mut section = String::new();
    let mut fields = HashMap::new();
    for line in input.lines() {
        let line = line.trim().trim_start_matches('\u{feff}');
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].to_ascii_lowercase();
        } else if let Some((key, value)) = line.split_once('=') {
            fields.insert(
                format!("{section}/{}", key.trim().to_ascii_lowercase()),
                value.trim().to_string(),
            );
        }
    }
    if fields.is_empty() {
        return Err("Xshell 会话格式无效；加密容器暂不支持".into());
    }
    let get = |keys: &[&str]| -> &str {
        keys.iter()
            .find_map(|key| fields.get(*key).map(String::as_str))
            .unwrap_or("")
    };
    let name = source
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("Xshell 连接");
    let protocol = get(&["connection/protocol"]);
    if !protocol.is_empty()
        && !protocol.eq_ignore_ascii_case("ssh")
        && !protocol.eq_ignore_ascii_case("ssh2")
    {
        unsupported(preview, "Xshell 非 SSH 会话未导入", Some(name));
        return Ok(());
    }
    let mut p = new_profile(name, "xshell");
    p.host = get(&["connection/host"]).into();
    p.username = get(&["connection:authentication/username", "connection/username"]).into();
    let raw_port = get(&["connection/port"]);
    if !raw_port.is_empty() {
        p.port = match raw_port.parse::<u16>() {
            Ok(n) if n > 0 => n,
            _ => {
                preview.stats.unsupported += 1;
                issue(preview, "error", "Xshell 端口无效", Some(name));
                return Ok(());
            }
        }
    }
    if !get(&[
        "connection:authentication/password",
        "connection:authentication/passphrase",
    ])
    .is_empty()
    {
        issue(
            preview,
            "warning",
            "Xshell 加密密码和私钥口令无法导入，请手动补充",
            Some(name),
        );
    }
    let key = get(&[
        "connection:authentication/userkey",
        "connection:authentication/privatekey",
    ]);
    if !key.is_empty() {
        p.auth_type = AuthType::PrivateKey;
        p.private_key_path = Some(path(key, source, home));
        issue(
            preview,
            "warning",
            "Xshell 私钥需要本地文件，不解密客户端凭据库",
            Some(name),
        );
    }
    let proxy = get(&["connection:proxy/proxytype", "connection/proxy"]);
    if !proxy.is_empty() && proxy != "0" && proxy != "None" {
        disable(preview, &mut p, "Xshell 代理配置暂不支持")
    }
    issue(
        preview,
        "warning",
        "Xshell 仅导入主机、端口、用户名和私钥路径；脚本及高级选项未迁移",
        Some(name),
    );
    normalize(p, preview);
    Ok(())
}

pub(super) fn archive(source: &Path, home: &Path) -> Result<ImportPreview, String> {
    use std::io::Read;
    let metadata = fs::metadata(source).map_err(|_| "无法读取 Xshell 归档")?;
    if metadata.len() > 32 * 1024 * 1024 {
        return Err("Xshell 归档超过 32 MB".into());
    }
    let file = fs::File::open(source).map_err(|_| "无法读取 Xshell 归档")?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|_| "不支持的 Xshell 容器：仅支持 ZIP 格式 .xts，专有或加密容器无法导入")?;
    if archive.len() > 2048 {
        return Err("Xshell 归档条目过多".into());
    }
    let mut preview = ImportPreview {
        source: "xshell".into(),
        source_path: source.to_string_lossy().into(),
        profiles: Vec::new(),
        issues: Vec::new(),
        stats: ImportStats::default(),
    };
    let mut expanded = 0u64;
    for i in 0..archive.len() {
        let mut entry = match archive.by_index(i) {
            Ok(e) => e,
            Err(_) => {
                unsupported(&mut preview, "归档条目加密或格式不支持", None);
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let Some(enclosed) = entry.enclosed_name() else {
            issue(&mut preview, "warning", "归档条目路径无效，已跳过", None);
            continue;
        };
        if entry
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            issue(&mut preview, "warning", "归档符号链接已跳过", None);
            continue;
        }
        if !enclosed
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("xsh"))
        {
            continue;
        }
        if preview.profiles.len() >= 512 {
            return Err("归档会话超过 512 个".into());
        }
        expanded += entry.size();
        if entry.size() > MAX_IMPORT_BYTES || expanded > 32 * 1024 * 1024 {
            return Err("归档解压内容超过大小限制".into());
        }
        let mut bytes = Vec::new();
        entry
            .by_ref()
            .take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "无法读取归档会话")?;
        if bytes.len() as u64 > MAX_IMPORT_BYTES {
            return Err("归档会话超过大小限制".into());
        }
        let text = decode_bytes(&bytes, true)?;
        // No archive entry is ever extracted. Resolve relative private keys against the archive's directory.
        let synthetic = source.parent().unwrap_or(home).join(&enclosed);
        let mut part = super::parse("xshell", &text, &synthetic, home)?;
        for p in &mut part.profiles {
            p.group = enclosed
                .parent()
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
        }
        let omitted = part.stats.total.saturating_sub(part.profiles.len());
        preview.profiles.extend(part.profiles);
        preview.issues.extend(part.issues);
        preview.stats.unsupported += omitted;
    }
    if preview.profiles.is_empty() && preview.stats.unsupported == 0 {
        issue(&mut preview, "warning", "归档没有可导入的 .xsh 会话", None)
    }
    finalize(&mut preview);
    Ok(preview)
}
