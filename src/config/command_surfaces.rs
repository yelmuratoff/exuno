//! What in a source tree a tool runs or obeys once synced: MCP servers, tool
//! config (hooks, permissions, plugins), and scripts a skill ships. `import`
//! lists these before it writes a `.ai/` from somewhere else.
//!
//! Where a file sits decides whether it is listed; its contents only refine
//! the label, so a format this module cannot read is listed, never skipped.

use std::path::Path;

use serde_json::Value;

const SCRIPT_EXTENSIONS: [&str; 14] = [
    "sh", "bash", "zsh", "fish", "py", "js", "mjs", "cjs", "ts", "rb", "pl", "ps1", "bat", "cmd",
];

/// The source folders whose every file a tool reads as its own config.
const TOOL_FOLDERS: [&str; 3] = ["tools", "hooks", "settings"];

/// What `file`, at `rel` below the source base, would have a tool run or
/// obey: one line per server, tool config file, or script, with control
/// characters escaped so a name cannot rewrite the list.
pub fn of(rel: &str, file: &Path) -> Vec<String> {
    let leaf = crate::paths::leaf(rel);
    let top = rel.split('/').next().unwrap_or("");
    let lines = if leaf == "mcp.json" || top == "mcp" {
        mcp_servers(rel, file)
    } else if TOOL_FOLDERS.contains(&top) {
        let hooks = top == "hooks" || leaf.starts_with("hooks.") || declares_hooks(file);
        let kind = if hooks { "hooks in" } else { "tool config" };
        vec![format!("{kind} {rel}")]
    } else if top == "skills" && is_script(file, &leaf) {
        vec![format!("script {rel}")]
    } else {
        Vec::new()
    };
    lines.iter().map(|line| printable(line)).collect()
}

fn printable(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() {
                c.escape_default().to_string()
            } else {
                c.to_string()
            }
        })
        .collect()
}

fn parse(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// One line per server — its command, else its URL — or the file whole when
/// its shape is not the `mcpServers` map; nothing for an empty file.
fn mcp_servers(rel: &str, file: &Path) -> Vec<String> {
    let whole = vec![format!("MCP config {rel}")];
    let Some(config) = parse(file) else {
        return whole;
    };
    let Some(servers) = config.get("mcpServers").and_then(Value::as_object) else {
        let empty = config.as_object().is_some_and(serde_json::Map::is_empty);
        return if empty { Vec::new() } else { whole };
    };
    servers
        .iter()
        .map(|(name, server)| {
            let text = |key: &str| server.get(key).and_then(Value::as_str);
            let args: Vec<&str> = server
                .get("args")
                .and_then(Value::as_array)
                .map(|args| args.iter().filter_map(Value::as_str).collect())
                .unwrap_or_default();
            match (text("command"), text("url")) {
                (Some(command), _) => {
                    let line = std::iter::once(command)
                        .chain(args)
                        .collect::<Vec<_>>()
                        .join(" ");
                    format!("MCP server {name}: {line}")
                }
                (None, Some(url)) => format!("MCP server {name}: {url}"),
                (None, None) => format!("MCP server {name}"),
            }
        })
        .collect()
}

fn declares_hooks(file: &Path) -> bool {
    let Some(config) = parse(file) else {
        return false;
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
            [
                "MCP server github: npx -y @github/mcp-server",
                "MCP server remote: https://example.com/mcp"
            ]
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
    fn a_remote_server_is_listed_by_its_url() {
        assert_eq!(
            surfaces(
                "mcp.json",
                r#"{"mcpServers": {"docs": {"type": "http", "url": "https://example.com/mcp"}}}"#
            ),
            ["MCP server docs: https://example.com/mcp"]
        );
        assert!(surfaces("mcp.json", r#"{"mcpServers": {}}"#).is_empty());
    }

    #[test]
    fn control_characters_cannot_rewrite_the_list() {
        let text = "{\"mcpServers\": {\"a\\u001b[2Jb\": {\"command\": \"x\\ny\"}}}";
        assert_eq!(
            surfaces("mcp.json", text),
            ["MCP server a\\u{1b}[2Jb: x\\ny"]
        );
    }

    #[test]
    fn every_file_a_tool_config_folder_holds_is_listed() {
        assert_eq!(
            surfaces("tools/opencode/hooks.ts", "export default {}\n"),
            ["hooks in tools/opencode/hooks.ts"]
        );
        assert_eq!(
            surfaces("tools/zed/settings.jsonc", "{ // comment\n}\n"),
            ["tool config tools/zed/settings.jsonc"]
        );
        assert_eq!(
            surfaces("tools/claude/settings.json", r#"{"permissions": {}}"#),
            ["tool config tools/claude/settings.json"]
        );
        assert_eq!(
            surfaces(
                "mcp/claude.json",
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
            ["tool config tools/claude/settings.json"]
        );
    }

    #[test]
    fn a_tool_config_is_named_hooks_once_it_declares_some() {
        let with = r#"{"hooks": {"Stop": [{"hooks": [{"type": "command", "command": "x"}]}]}}"#;
        assert_eq!(
            surfaces("tools/claude/settings.json", with),
            ["hooks in tools/claude/settings.json"]
        );
        assert_eq!(
            surfaces("tools/claude/settings.json", r#"{"hooks": {}}"#),
            ["tool config tools/claude/settings.json"]
        );
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
