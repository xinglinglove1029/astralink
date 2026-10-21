use super::*;
use std::collections::HashMap;
struct Block {
    patterns: Vec<String>,
    options: Vec<(String, String)>,
    conditional: bool,
}
fn tokens(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    for c in line.chars() {
        match c {
            '#' if quote.is_none() => break,
            '"' | '\'' if quote == Some(c) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(c),
            c if (c.is_whitespace() || c == '=') && quote.is_none() => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word)
    }
    words
}
fn matches(patterns: &[String], alias: &str) -> bool {
    let single = |pattern: &str| {
        let regex = format!(
            "^{}$",
            regex::escape(pattern)
                .replace("\\*", ".*")
                .replace("\\?", ".")
        );
        regex::RegexBuilder::new(&regex)
            .case_insensitive(true)
            .build()
            .is_ok_and(|r| r.is_match(alias))
    };
    !patterns
        .iter()
        .any(|p| p.strip_prefix('!').is_some_and(single))
        && patterns.iter().any(|p| !p.starts_with('!') && single(p))
}
pub(super) fn parse(
    input: &str,
    source: &Path,
    home: &Path,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    let mut blocks = vec![Block {
        patterns: vec!["*".into()],
        options: Vec::new(),
        conditional: false,
    }];
    let mut aliases = Vec::new();
    for line in input.lines() {
        let parts = tokens(line);
        if parts.len() < 2 {
            continue;
        }
        let key = parts[0].to_ascii_lowercase();
        if key == "host" {
            let patterns = parts[1..].to_vec();
            for p in &patterns {
                if !p.contains(['*', '?', '!']) && !aliases.contains(p) {
                    aliases.push(p.clone())
                }
            }
            blocks.push(Block {
                patterns,
                options: Vec::new(),
                conditional: false,
            });
        } else if key == "match" {
            issue(
                preview,
                "warning",
                "OpenSSH Match 条件未解析，相关连接需手动确认",
                None,
            );
            blocks.push(Block {
                patterns: vec!["*".into()],
                options: Vec::new(),
                conditional: true,
            });
        } else {
            if let Some(block) = blocks.last_mut() {
                block.options.push((key, parts[1..].join(" ")));
            }
        }
    }
    if aliases.len() > 512 {
        return Err("最多导入 512 个连接".into());
    }
    if aliases.is_empty() {
        issue(preview, "warning", "配置只有通配符，未生成具体连接", None)
    }
    let mut jumps = Vec::new();
    for alias in &aliases {
        let mut options = HashMap::new();
        let mut has_conditional = false;
        for block in &blocks {
            if block.conditional {
                if !block.options.is_empty() {
                    has_conditional = true
                }
                continue;
            }
            if matches(&block.patterns, alias) {
                for (key, value) in &block.options {
                    options.entry(key.as_str()).or_insert(value.as_str());
                }
            }
        }
        let mut p = new_profile(alias, "openssh");
        p.host = options
            .get("hostname")
            .copied()
            .unwrap_or(alias)
            .replace("%h", alias);
        p.username = options
            .get("user")
            .copied()
            .map(Into::into)
            .or_else(|| std::env::var("USERNAME").ok())
            .unwrap_or_default();
        if let Some(value) = options.get("port") {
            p.port = match value.parse::<u16>() {
                Ok(n) if n > 0 => n,
                _ => {
                    preview.stats.unsupported += 1;
                    issue(preview, "error", "OpenSSH 端口无效", Some(alias));
                    continue;
                }
            }
        }
        if let Some(key) = options.get("identityfile") {
            if *key != "none" {
                p.auth_type = AuthType::PrivateKey;
                let key = key
                    .replace("%d", &home.to_string_lossy())
                    .replace("%h", &p.host)
                    .replace("%r", &p.username);
                if key.contains('%') {
                    disable(preview, &mut p, "私钥路径包含未支持的变量")
                } else {
                    p.private_key_path = Some(path(&key, source, home))
                }
            }
        }
        if let Some(agent) = options.get("identityagent") {
            if *agent != "none" {
                p.auth_type = AuthType::Agent;
                p.agent = Some(path(agent, source, home));
            }
        }
        if options
            .get("preferredauthentications")
            .is_some_and(|s| s.contains("keyboard-interactive"))
        {
            p.try_keyboard = true
        }
        if let Some(jump) = options.get("proxyjump") {
            if *jump != "none" {
                jumps.push((p.id.clone(), jump.to_string()));
            }
        }
        for key in [
            "proxycommand",
            "include",
            "certificatefile",
            "pkcs11provider",
            "securitykeyprovider",
            "remotecommand",
        ] {
            if options.get(key).is_some_and(|v| *v != "none") {
                disable(preview, &mut p, "OpenSSH 包含未支持的代理、引用或认证指令")
            }
        }
        if has_conditional {
            disable(preview, &mut p, "OpenSSH Match 条件未解析，请确认连接设置")
        }
        if p.host.contains('%') {
            disable(preview, &mut p, "主机包含未支持的变量")
        }
        normalize(p, preview);
    }
    let ids: HashMap<_, _> = preview
        .profiles
        .iter()
        .map(|p| (p.name.clone(), p.id.clone()))
        .collect();
    for (id, jump) in jumps {
        let Some(index) = preview.profiles.iter().position(|p| p.id == id) else {
            continue;
        };
        if let Some(target) = ids.get(&jump) {
            preview.profiles[index].jump_host_id = Some(target.clone())
        } else {
            preview.profiles[index].enabled = false;
            let name = preview.profiles[index].name.clone();
            issue(
                preview,
                "error",
                "ProxyJump 仅支持已导入的单个主机别名",
                Some(&name),
            );
        }
    }
    // Includes can carry secrets or execute Match exec; never run commands during discovery/import.
    if input.lines().any(|line| {
        tokens(line)
            .first()
            .is_some_and(|s| s.eq_ignore_ascii_case("include"))
    }) {
        issue(
            preview,
            "warning",
            "OpenSSH Include 未展开，请导入引用文件并检查配置",
            None,
        )
    }
    Ok(())
}
