use crate::models::SourceCandidate;
use std::{fs, path::Path};
/// Probe known locations only; never scan the user's disk or read credential vaults.
pub fn discover(
    home: &Path,
    roaming: Option<&Path>,
    documents: Option<&Path>,
) -> Vec<SourceCandidate> {
    let mut found = Vec::new();
    let documents = documents
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| home.join("Documents"));
    let roaming = roaming
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| home.join("AppData/Roaming"));
    let mut add = |kind: &str, label: &str, path: std::path::PathBuf| {
        let exists = path.exists();
        let profile_count = if exists && kind == "xshell" && path.is_dir() {
            let mut visited = 0;
            let mut count = 0;
            let mut directories = vec![(path.clone(), 0)];
            while let Some((directory, depth)) = directories.pop() {
                if depth > 8 || visited >= 1024 {
                    break;
                }
                if let Ok(entries) = fs::read_dir(directory) {
                    for entry in entries.flatten() {
                        visited += 1;
                        if visited > 1024 {
                            break;
                        }
                        if let Ok(ty) = entry.file_type() {
                            if ty.is_symlink() {
                                continue;
                            }
                            if ty.is_dir() {
                                directories.push((entry.path(), depth + 1));
                            } else if ty.is_file()
                                && entry
                                    .path()
                                    .extension()
                                    .is_some_and(|extension| extension.eq_ignore_ascii_case("xsh"))
                            {
                                count += 1;
                            }
                        }
                    }
                }
            }
            Some(count)
        } else {
            None
        };
        found.push(SourceCandidate {
            kind: kind.into(),
            label: label.into(),
            path: path.to_string_lossy().into(),
            exists,
            profile_count,
        });
    };
    add("tabby", "Tabby", roaming.join("tabby/config.yaml"));
    add("tabby", "Tabby", home.join(".config/tabby/config.yaml"));
    add(
        "tabby",
        "Tabby",
        home.join("Library/Application Support/tabby/config.yaml"),
    );
    for version in ["7", "8", "9"] {
        add(
            "xshell",
            "Xshell",
            documents.join(format!("NetSarang Computer/{version}/Xshell/Sessions")),
        );
    }
    add("xshell", "Xshell", documents.join("Xshell/Sessions"));
    add("finalshell", "FinalShell", home.join("finalshell/conn"));
    add("finalshell", "FinalShell", home.join(".finalshell/conn"));
    add("nyaterm", "NyaTerm", home.join(".nyaterm/nyaterm.redb"));
    add(
        "termius",
        "Termius",
        roaming.join("Termius/IndexedDB/file__0.indexeddb.leveldb"),
    );
    add(
        "termius",
        "Termius",
        home.join(".config/Termius/IndexedDB/file__0.indexeddb.leveldb"),
    );
    add(
        "termius",
        "Termius",
        home.join("Library/Application Support/Termius/IndexedDB/file__0.indexeddb.leveldb"),
    );
    add(
        "windterm",
        "WindTerm",
        home.join(".wind/profiles/default.v10/terminal/user.sessions"),
    );
    add(
        "windterm",
        "WindTerm",
        roaming.join("WindTerm/profiles/default.v10/terminal/user.sessions"),
    );
    add("netcatty", "Netcatty CSV", documents.join("netcatty.csv"));
    add(
        "netcatty",
        "Netcatty CSV",
        home.join("Downloads/netcatty.csv"),
    );
    add(
        "netcatty",
        "Netcatty（请选择导出的 CSV）",
        roaming.join("netcatty"),
    );
    add("openssh", "OpenSSH", home.join(".ssh/config"));
    add(
        "mcp-json",
        "SSH MCP JSON",
        home.join(".ssh/ssh-config.json"),
    );
    add(
        "mcp-json",
        "SSH MCP JSON",
        home.join(".codex/ssh-config.json"),
    );
    add(
        "mcp-json",
        "Codex SSH MCP JSON",
        home.join(".codex/ssh-servers.json"),
    );
    add(
        "mcp-json",
        "SSH MCP JSON",
        home.join(".config/ssh-mcp-server/config.json"),
    );
    found
}
