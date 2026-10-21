use super::*;
fn header(s: &str) -> String {
    s.trim_start_matches('\u{feff}')
        .trim()
        .to_lowercase()
        .replace([' ', '_', '-'], "")
}
fn unguard(value: &str) -> &str {
    if value.starts_with('\'') && value.chars().nth(1).is_some_and(|c| "=+-@\t\r".contains(c)) {
        &value[1..]
    } else {
        value
    }
}
fn marked(value: &str, marker: &str) -> Result<String, String> {
    let Some(encoded) = value.strip_prefix(marker) else {
        return Ok(value.into());
    };
    let bytes = encoded.as_bytes();
    let mut result = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            if i + 2 >= bytes.len() {
                return Err("CSV 编码字段无效".into());
            }
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).map_err(|_| "CSV 编码字段无效")?;
            result.push(u8::from_str_radix(hex, 16).map_err(|_| "CSV 编码字段无效")?);
            i += 3;
        } else {
            result.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(result).map_err(|_| "CSV 编码字段无效".into())
}
pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let mut reader = csv::ReaderBuilder::new()
        .flexible(true)
        .from_reader(input.trim_start_matches('\u{feff}').as_bytes());
    let headers = reader
        .headers()
        .map_err(|_| "CSV 表头无效")?
        .iter()
        .map(header)
        .collect::<Vec<_>>();
    if !headers
        .iter()
        .any(|h| ["hostname/ip", "hostname", "host", "ip"].contains(&h.as_str()))
    {
        return Err("CSV 缺少主机列".into());
    }
    for (index, row) in reader.records().enumerate() {
        if index >= 512 {
            return Err("最多导入 512 个连接".into());
        }
        let row = match row {
            Ok(r) => r,
            Err(_) => {
                preview.stats.unsupported += 1;
                issue(preview, "error", "CSV 行格式无效", None);
                continue;
            }
        };
        let get = |keys: &[&str]| -> &str {
            keys.iter()
                .find_map(|key| {
                    headers
                        .iter()
                        .position(|h| h == key)
                        .and_then(|i| row.get(i))
                })
                .unwrap_or("")
        };
        let name = unguard(get(&["label", "name"]));
        let protocol = get(&["protocol"]).trim();
        if !protocol.is_empty() && !protocol.eq_ignore_ascii_case("ssh") {
            unsupported(preview, "仅支持 SSH，其他协议未导入", Some(name));
            continue;
        }
        let mut p = new_profile(name, "netcatty");
        p.host = unguard(get(&["hostname/ip", "hostname", "host", "ip"])).into();
        p.username = unguard(get(&["username", "user"])).into();
        p.group = unguard(get(&["groups", "group"])).into();
        p.notes = unguard(get(&["notes"])).into();
        let raw_port = get(&["port"]);
        if !raw_port.is_empty() {
            p.port = match raw_port.trim().parse::<u16>() {
                Ok(n) if n > 0 => n,
                _ => {
                    preview.stats.unsupported += 1;
                    issue(preview, "error", "CSV 端口无效", Some(name));
                    continue;
                }
            }
        }
        // Password fields are not trimmed or spreadsheet-unescaped: whitespace is credential data.
        let password = get(&["password"]);
        if !password.is_empty() {
            p.password = Some(password.into())
        }
        let key = get(&["keypath", "privatekeypath", "identityfile"]);
        if !key.is_empty() {
            match marked(key, "__netcatty_csv_keypath_v1__:") {
                Ok(key) => {
                    p.private_key_path = Some(path(&key, source, home));
                    p.auth_type = AuthType::PrivateKey
                }
                Err(_) => disable(preview, &mut p, "CSV 私钥路径编码无效"),
            }
        }
        let passphrase = get(&["passphrase"]);
        if !passphrase.is_empty() {
            match marked(passphrase, "__netcatty_csv_passphrase_v1__:") {
                Ok(s) => p.passphrase = Some(s),
                Err(_) => disable(preview, &mut p, "CSV 私钥口令编码无效"),
            }
        }
        if !get(&["proxy"]).trim().is_empty() {
            disable(preview, &mut p, "Netcatty 代理暂不支持，请手动配置跳板")
        }
        normalize(p, preview);
    }
    Ok(())
}
