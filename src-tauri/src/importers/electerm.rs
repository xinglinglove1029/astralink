use super::*;
use std::collections::HashMap;

fn group_path(
    id: &str,
    groups: &HashMap<String, Value>,
    parents: &HashMap<String, String>,
) -> String {
    let mut path = Vec::new();
    let mut current = id.to_owned();
    let mut visited = HashSet::new();
    while visited.insert(current.clone()) && path.len() < 32 {
        let Some(group) = groups.get(&current) else {
            break;
        };
        if let Some(name) = text(group, &["title"]) {
            path.push(name.to_owned());
        }
        let Some(parent) = parents.get(&current) else {
            break;
        };
        current = parent.clone();
    }
    path.reverse();
    path.join("/")
}

pub(super) fn parse(input: &str, preview: &mut ImportPreview) -> Result<(), String> {
    let data: Value = serde_json::from_str(input).map_err(|_| "Electerm JSON 格式无效")?;
    let bookmarks = data
        .get("bookmarks")
        .and_then(Value::as_array)
        .ok_or("Electerm 配置缺少 bookmarks 数组")?;
    if bookmarks.len() > 512 {
        return Err("最多导入 512 个连接".into());
    }
    let mut groups = HashMap::new();
    let mut bookmark_groups = HashMap::new();
    let mut group_parents = HashMap::new();
    if let Some(list) = data.get("bookmarkGroups").and_then(Value::as_array) {
        for group in list {
            let Some(id) = text(group, &["id"]) else {
                continue;
            };
            let id = id.to_owned();
            if let Some(ids) = group.get("bookmarkIds").and_then(Value::as_array) {
                for child in ids.iter().filter_map(Value::as_str) {
                    bookmark_groups
                        .entry(child.to_owned())
                        .or_insert_with(|| id.clone());
                }
            }
            if let Some(ids) = group.get("bookmarkGroupIds").and_then(Value::as_array) {
                for child in ids.iter().filter_map(Value::as_str) {
                    group_parents
                        .entry(child.to_owned())
                        .or_insert_with(|| id.clone());
                }
            }
            groups.insert(id, group.clone());
        }
    }
    for bookmark in bookmarks {
        let name = text(bookmark, &["title"]).unwrap_or_default();
        if !text(bookmark, &["type"]).is_some_and(|kind| kind.eq_ignore_ascii_case("ssh"))
            || bookmark.get("enableSsh") == Some(&Value::Bool(false))
        {
            unsupported(preview, "仅导入 Electerm SSH 书签", Some(name));
            continue;
        }
        let mut p = new_profile(name, "electerm");
        p.host = text(bookmark, &["host"]).unwrap_or_default().into();
        p.username = text(bookmark, &["username"]).unwrap_or("root").into();
        match port(bookmark.get("port")) {
            Ok(value) => p.port = value,
            Err(_) => {
                unsupported(preview, "Electerm 端口无效", Some(&p.name));
                continue;
            }
        }
        if let Some(id) = text(bookmark, &["id"]).and_then(|id| bookmark_groups.get(id)) {
            p.group = group_path(id, &groups, &group_parents);
        }
        if text(bookmark, &["authType"]).is_some_and(|kind| kind.eq_ignore_ascii_case("privateKey"))
        {
            p.auth_type = AuthType::PrivateKey;
            disable(preview, &mut p, "Electerm 私钥不随书签导出，请选择本地私钥");
        } else {
            issue(
                preview,
                "warning",
                "Electerm 书签不包含可用密码，请补充凭据",
                Some(&p.name),
            );
        }
        normalize(p, preview);
    }
    Ok(())
}
