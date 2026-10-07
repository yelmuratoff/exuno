//! `lib/helpers/edit_paths.sh`: where a tool's payload overrides are edited.
//! `enable` prints the block; `doctor` gets its checklist when it is ported.

use crate::paths::DiskText;
use std::path::Path;

use crate::config::payload;
use crate::config::tool::Tool;
use crate::output::style::Style;
use crate::project::Project;

enum Row {
    Override(&'static str, String),
    Shared(String),
    CustomizeHint(&'static str, String),
    SharedHint,
}

fn shown(project: &Project, path: &Path) -> String {
    let text = path.disk_text();
    let root = format!("{}/", project.root.disk_text());
    text.strip_prefix(&root).unwrap_or(&text).to_string()
}

/// `tool_edit_paths_rows`.
fn rows(project: &Project, tool: &Tool) -> Vec<Row> {
    let mut rows = Vec::new();
    for resource in ["settings", "hooks"] {
        let Some(path) = payload::override_path(project, tool, resource) else {
            continue;
        };
        if path.is_file() {
            rows.push(Row::Override(resource, shown(project, &path)));
        } else {
            rows.push(Row::CustomizeHint(
                resource,
                format!("exuno customize {} {resource}", tool.slug),
            ));
        }
    }
    let Some(per_tool) = payload::override_path(project, tool, "mcp") else {
        return rows;
    };
    if per_tool.is_file() {
        rows.push(Row::Override("mcp", shown(project, &per_tool)));
    } else if project.shared_mcp_path().is_file() {
        rows.push(Row::Shared(shown(project, &project.shared_mcp_path())));
    } else {
        rows.push(Row::SharedHint);
    }
    rows
}

/// `print_tool_edit_paths_block`.
pub fn block(project: &Project, tool: &Tool, style: &Style) -> String {
    let rows = rows(project, tool);
    if rows.is_empty() {
        return String::new();
    }
    let mut text = format!("\n{}\n", style.bold(&format!("  {}", tool.display_name())));
    for row in rows {
        text.push_str(&match row {
            Row::Override(resource, path) => {
                format!("    Edit {:<9} {path}\n", format!("{resource}:"))
            }
            Row::Shared(path) => {
                format!("    Edit {:<9} {path}  {}\n", "mcp:", style.dim("(shared)"))
            }
            Row::CustomizeHint(resource, command) => {
                let label = match resource {
                    "settings" => "Settings:",
                    _ => "Hooks:",
                };
                format!("    {label:<14} {}\n", style.dim(&command))
            }
            Row::SharedHint => format!(
                "    {:<14} {}\n",
                "MCP:",
                style.dim("exuno add mcp <server>  (shared — not yet configured)")
            ),
        });
    }
    text
}

/// `print_tool_edit_paths_checklist`: `doctor`'s indented rows with a glyph per
/// state; empty when the tool has no base payloads.
pub fn checklist(project: &Project, tool: &Tool, style: &Style) -> String {
    let rows = rows(project, tool);
    if rows.is_empty() {
        return String::new();
    }
    let mut text = format!("      {}\n", style.bold(&tool.display_name()));
    for row in rows {
        let (glyph, resource, render) = match row {
            Row::Override(resource, path) => (style.green("✓"), resource, path),
            Row::Shared(path) => (
                style.green("✓"),
                "mcp",
                format!("{path} {}", style.dim("(shared)")),
            ),
            Row::CustomizeHint(resource, command) => {
                (style.dim("·"), resource, style.dim(&command))
            }
            Row::SharedHint => (
                style.dim("·"),
                "mcp",
                style.dim("exuno add mcp <server> (shared — not yet configured)"),
            ),
        };
        text.push_str(&format!("          {glyph}  {resource:<10} {render}\n"));
    }
    text
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_block_names_overrides_the_shared_mcp_and_hints_like_bash() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            block(&project, &claude, &Style::plain()),
            "\n  Claude Code\n    Settings:      exuno customize claude settings\n    MCP:           exuno add mcp <server>  (shared — not yet configured)\n"
        );

        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        assert_eq!(
            block(&project, &claude, &Style::plain()),
            "\n  Claude Code\n    Edit settings: .ai/src/tools/claude/settings.json\n    Edit mcp:      .ai/src/mcp.json  (shared)\n"
        );

        let windsurf = Tool::load(&project, "windsurf").unwrap();
        write(dir.path(), ".ai/src/tools/windsurf/mcp.json", "{}");
        assert_eq!(
            block(&project, &windsurf, &Style::plain()),
            "\n  Devin Desktop\n    Hooks:         exuno customize windsurf hooks\n    Edit mcp:      .ai/src/tools/windsurf/mcp.json\n"
        );
        let amp = Tool::load(&project, "no-such-tool").unwrap();
        assert_eq!(block(&project, &amp, &Style::plain()), "");
    }

    #[test]
    fn the_checklist_glyphs_each_row_like_doctor() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            checklist(&project, &claude, &Style::plain()),
            "      Claude Code\n          ·  settings   exuno customize claude settings\n          ·  mcp        exuno add mcp <server> (shared — not yet configured)\n"
        );
        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        assert_eq!(
            checklist(&project, &claude, &Style::plain()),
            "      Claude Code\n          ✓  settings   .ai/src/tools/claude/settings.json\n          ✓  mcp        .ai/src/mcp.json (shared)\n"
        );
        let cursor = Tool::load(&project, "cursor").unwrap();
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        assert_eq!(
            checklist(&project, &cursor, &Style::plain()),
            "      Cursor\n          ✓  hooks      .ai/src/tools/cursor/hooks.json\n          ✓  mcp        .ai/src/mcp.json (shared)\n"
        );
        assert_eq!(
            checklist(
                &project,
                &Tool::load(&project, "mytool").unwrap(),
                &Style::plain()
            ),
            ""
        );
    }
}
