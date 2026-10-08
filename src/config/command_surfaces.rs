//! What in a source tree has a tool run a command once synced: MCP servers
//! started from a command, hooks, and scripts a skill ships. `import` lists
//! these before it writes a `.ai/` from somewhere else.

use std::path::Path;

use serde_json::Value;

const SCRIPT_EXTENSIONS: [&str; 14] = [
    "sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "ps1", "bat", "cmd",
];

/// What `file`, at `rel` below the source base, would have a tool run: one
/// line per server, hook file, or script. A config that cannot be parsed is
/// reported whole rather than passed as harmless.
pub fn of(rel: &str, file: &Path) -> Vec<String> {
    let leaf = crate::paths::leaf(rel);
    if leaf == "mcp.json" {
        return mcp_servers(rel, file);
    }
    let tool_config = rel.starts_with("tools/") || rel.starts_with("hooks/");
    let json = leaf.ends_with(".json");
    if tool_config && json && (leaf == "hooks.json" || declares_hooks(file)) {
        return vec![format!("hooks in {rel}")];
    }
    if rel.starts_with("skills/") && is_script(file, &leaf) {
        return vec![format!("script {rel}")];
    }
    Vec::new()
}

fn parse(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

fn mcp_servers(rel: &str, file: &Path) -> Vec<String> {
    let Some(config) = parse(file) else {
        return vec![format!("MCP config {rel}")];
    };
    let Some(servers) = config.get("mcpServers").and_then(Value::as_object) else {
        return Vec::new();
    };
    servers
        .iter()
        .filter_map(|(name, server)| {
            let command = server.get("command")?.as_str()?;
            let args: Vec<&str> = server
                .get("args")
                .and_then(Value::as_array)
                .map(|args| args.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            let line = std::iter::once(command)
                .chain(args)
                .collect::<Vec<_>>()
                .join(" ");
            Some(format!("MCP server {name}: {line}"))
        })
        .collect()
}

fn declares_hooks(file: &Path) -> bool {
    let Some(config) = parse(file) else {
        return true;
    };
    match config.get("hooks") {
        None | Some(Value::Null) => false,
        Some(Value::Object(hooks)) => !hooks.is_empty(),
        Some(Value::Array(hooks)) => !hooks.is_empty(),
        Some(_) => true,
    }
}

fn is_script(file: &Path, leaf: &str) -> bool {
    let by_extension = leaf
        .rsplit_once('.')
        .is_some_and(|(_, ext)| SCRIPT_EXTENSIONS.contains(&ext.to_ascii_lowercase().as_str()));
    by_extension || is_executable(file)
}

#[cfg(unix)]
fn is_executable(file: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(file).is_ok_and(|meta| meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(_: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn surfaces(rel: &str, text: &str) -> Vec<String> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, text).unwrap();
        of(rel, &file)
    }

    #[test]
    fn each_command_server_of_an_mcp_file_is_listed() {
        let text = r#"{"mcpServers": {
            "github": {"command": "npx", "args": ["-y", "@github/mcp-server"]},
            "remote": {"type": "http", "url": "https://example.com/mcp"}
        }}"#;
        assert_eq!(
            surfaces("mcp.json", text),
            ["MCP server github: npx -y @github/mcp-server"]
        );
        assert_eq!(
            surfaces(
                "tools/cursor/mcp.json",
                r#"{"mcpServers": {"x": {"command": "x"}}}"#
            ),
            ["MCP server x: x"]
        );
    }

    #[test]
    fn an_unreadable_config_is_reported_whole() {
        assert_eq!(surfaces("mcp.json", "{ not json"), ["MCP config mcp.json"]);
        assert_eq!(
            surfaces("tools/claude/settings.json", "{ not json"),
            ["hooks in tools/claude/settings.json"]
        );
    }

    #[test]
    fn hooks_count_only_when_a_tool_config_declares_some() {
        let with = r#"{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "x"}]}]}}"#;
        assert_eq!(
            surfaces("tools/claude/settings.json", with),
            ["hooks in tools/claude/settings.json"]
        );
        assert!(surfaces("tools/claude/settings.json", r#"{"hooks": {}}"#).is_empty());
        assert!(surfaces("tools/claude/settings.json", r#"{"permissions": {}}"#).is_empty());
        assert_eq!(
            surfaces("tools/codex/hooks.json", "{}"),
            ["hooks in tools/codex/hooks.json"]
        );
        assert!(surfaces("rules/settings.json", with).is_empty());
    }

    #[test]
    fn a_skill_script_is_listed_by_its_extension() {
        assert_eq!(
            surfaces("skills/jury/scripts/audit.SH", "#!/bin/sh\n"),
            ["script skills/jury/scripts/audit.SH"]
        );
        assert!(surfaces("skills/jury/SKILL.md", "# Jury\n").is_empty());
        assert!(surfaces("rules/run.sh", "#!/bin/sh\n").is_empty());
    }

    // An executable bit is a POSIX permission.
    #[cfg(unix)]
    #[test]
    fn an_executable_skill_file_is_listed_without_an_extension() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("run");
        std::fs::write(&file, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            of("skills/jury/bin/run", &file),
            ["script skills/jury/bin/run"]
        );
    }
}
