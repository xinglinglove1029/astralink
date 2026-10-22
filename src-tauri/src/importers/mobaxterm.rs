use super::*;

pub(super) fn parse(input: &str, preview: &mut ImportPreview) -> Result<(), String> {
    let mut section = String::new();
    let mut group = String::new();
    for line in input.lines().map(str::trim) {
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line
            .strip_prefix('[')
            .and_then(|name| name.strip_suffix(']'))
        {
            section = name.to_owned();
            group.clear();
            continue;
        }
        if !section.starts_with("Bookmarks") {
            continue;
        }
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name == "SubRep" {
            group = value
                .split('\\')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join("/");
            continue;
        }
        if name == "ImgNum" {
            continue;
        }
        let Some((_, after_hash)) = value.split_once('#') else {
            continue;
        };
        let Some((marker, values)) = after_hash.split_once('%') else {
            continue;
        };
        if !marker.starts_with("109") {
            unsupported(preview, "仅导入 MobaXterm SSH 书签", Some(name));
            continue;
        }
        let fields: Vec<_> = values.split('%').collect();
        if fields.len() < 3 {
            unsupported(preview, "MobaXterm 书签字段不完整", Some(name));
            continue;
        }
        let mut p = new_profile(name.trim(), "mobaxterm");
        p.host = fields[0].trim().into();
        p.port = fields[1]
            .parse::<u16>()
            .ok()
            .filter(|port| *port > 0)
            .unwrap_or(22);
        p.username = if fields[2].trim().is_empty() {
            "root"
        } else {
            fields[2].trim()
        }
        .into();
        p.group = group.clone();
        issue(
            preview,
            "warning",
            "MobaXterm 书签不包含可用密码，请补充凭据",
            Some(&p.name),
        );
        normalize(p, preview);
    }
    Ok(())
}
