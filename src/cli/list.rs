//! `exuno list`: the tool catalog with per-project status, byte for byte
//! the table `lib/helpers/list.sh` prints.

use std::collections::BTreeSet;
use std::io::Write;

use crate::output::help::{Help, Section};
use crate::output::style::{Style, pad_right};
use crate::{Error, config::catalog, config::payload, config::tool::Tool, project::Project};

const RESOURCES: [(&str, &str); 3] = [("hooks", "H"), ("mcp", "M"), ("settings", "S")];

pub const HELP: Help = Help {
    command: "list",
    tagline: "show available tools and their status",
    synopsis: &["list", "ls"],
    description: &[
        "Prints one row per tool in the shipped catalog, plus any tool that\nexists only as a project override: its display name, the slug to use\nin commands, whether it is enabled here, and which payloads it\ncarries. Other arguments are ignored.",
    ],
    sections: &[
        Section {
            title: "LEGEND",
            entries: &[
                ("●", "Enabled in this project"),
                ("○", "Available, not enabled"),
                ("★", "Tool override in .ai/src/tools/<slug>.yaml"),
                (
                    "H M S",
                    "Hooks, MCP, and settings payloads: a bare letter is the\nshipped base, * a project override, ~ a legacy-layout\noverride, and · no payload of that kind",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[("-h, --help", "Show this help")],
        },
        Section {
            title: "SEE ALSO",
            entries: &[
                ("exuno enable <slug>", "Opt in to a tool"),
                (
                    "exuno customize <slug>",
                    "Create a per-field override for a tool",
                ),
            ],
        },
    ],
    examples: &["list", "ls"],
};

pub fn run(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    out: &mut impl Write,
) -> Result<u8, Error> {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        out.write_all(HELP.render(style).as_bytes())
            .map_err(|e| Error::io("<stdout>", e))?;
        return Ok(0);
    }
    let report = render(&discover()?, style)?;
    out.write_all(report.as_bytes())
        .map_err(|e| Error::io("<stdout>", e))?;
    Ok(0)
}

pub fn render(project: &Project, style: &Style) -> Result<String, Error> {
    let enabled = project.enabled_tools()?;
    let customized: BTreeSet<String> = project.user_override_tools()?.into_iter().collect();
    let all: BTreeSet<String> = catalog::base_tools()
        .into_iter()
        .chain(customized.iter().cloned())
        .collect();

    let mut text = String::from("\n");
    text.push_str(&style.bold("  Exuno Tools"));
    text.push('\n');
    text.push_str(&style.dim(
        "  ● enabled   ○ available   ★ tool override   H M S = hooks/mcp/settings (· = base only, * = override, ~ = legacy override)",
    ));
    text.push('\n');
    text.push_str(&style.dim("  Columns: name · slug (use in commands) · status · resources"));
    text.push_str("\n\n");

    let mut enabled_count = 0usize;
    let mut customized_count = 0usize;
    let mut payload_override_count = 0usize;
    for slug in &all {
        let tool = Tool::load(project, slug)?;
        let (marker, status) = if enabled.contains(slug) {
            enabled_count += 1;
            (style.green("●"), style.dim("enabled"))
        } else {
            (style.dim("○"), style.dim("available"))
        };
        let star = if customized.contains(slug) {
            customized_count += 1;
            style.yellow("★")
        } else {
            " ".to_string()
        };
        let (resources, has_override) = resources_column(project, &tool, style)?;
        if has_override {
            payload_override_count += 1;
        }
        text.push_str(&format!(
            "    {marker} {star}  {} {} {}  {resources}\n",
            pad_right(&tool.display_name(), 22),
            pad_right(&style.dim(slug), 13),
            pad_right(&status, 10),
        ));
    }

    text.push('\n');
    let mut summary = format!("{enabled_count} of {} enabled", all.len());
    if customized_count > 0 {
        summary.push_str(&format!(", {customized_count} tool override(s)"));
    }
    if payload_override_count > 0 {
        summary.push_str(&format!(", {payload_override_count} payload override(s)"));
    }
    text.push_str(&format!("  {summary}\n"));

    let shared_mcp = project.shared_mcp_path().is_file();
    if shared_mcp {
        let mut mcp_overrides = 0usize;
        for slug in &all {
            if payload::find_new_override(project, slug, "mcp")?.is_some() {
                mcp_overrides += 1;
            }
        }
        let mut hint = format!("  Shared MCP: {}", style.yellow(".ai/src/mcp.json"));
        if mcp_overrides > 0 {
            hint.push(' ');
            hint.push_str(&style.dim(&format!("(+ {mcp_overrides} per-tool override)")));
        }
        text.push_str(&hint);
        text.push('\n');
    }

    text.push('\n');
    if enabled_count == 0 {
        text.push_str(&format!(
            "  Enable a tool:     {}\n",
            style.cyan("exuno enable <slug>")
        ));
    }
    text.push_str(&format!(
        "  Customize a tool:  {}\n",
        style.cyan("exuno customize <slug> [<resource>]")
    ));
    if !shared_mcp {
        text.push_str(&format!(
            "  Add MCP server:    {}\n",
            style.cyan("exuno add mcp <server> --command …")
        ));
    }
    text.push_str(&format!(
        "  Sync outputs:      {}\n",
        style.cyan("exuno sync")
    ));
    text.push('\n');
    Ok(text)
}

/// One cell per payload resource (`H*`, `M~`, `S `, or `· `, each followed by
/// a space) and whether any override, new layout or legacy, was found.
fn resources_column(
    project: &Project,
    tool: &Tool,
    style: &Style,
) -> Result<(String, bool), Error> {
    let mut cells = String::new();
    let mut has_override = false;
    for (resource, letter) in RESOURCES {
        let cell = if payload::find_new_override(project, &tool.slug, resource)?.is_some() {
            has_override = true;
            style.yellow(&format!("{letter}*"))
        } else if payload::legacy_override_path(project, tool, resource)
            .is_some_and(|p| p.is_file())
        {
            has_override = true;
            style.yellow(&format!("{letter}~"))
        } else if tool.base_payload(resource).is_some() {
            style.dim(&format!("{letter} "))
        } else {
            style.dim("· ")
        };
        cells.push_str(&cell);
        cells.push(' ');
    }
    Ok((cells, has_override))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn help_is_answered_on_stdout_before_the_project_is_discovered() {
        let mut out = Vec::new();
        let status = run(
            &["--help".to_string()],
            &|| panic!("--help must not discover the project"),
            &Style::plain(),
            &mut out,
        )
        .unwrap();
        assert_eq!(status, 0);
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, HELP.render(&Style::plain()));
        assert!(out.starts_with(
            "\n  exuno list — show available tools and their status\n\n  USAGE\n    exuno list\n    exuno ls\n"
        ));
        assert!(out.contains("\n  LEGEND\n    ●       Enabled in this project\n"));
    }

    #[test]
    fn an_unknown_argument_is_ignored_like_bash() {
        let dir = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        let status = run(
            &["--bogus".to_string()],
            &|| Project::at(dir.path()),
            &Style::plain(),
            &mut out,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert!(
            String::from_utf8(out)
                .unwrap()
                .starts_with("\n  Exuno Tools\n")
        );
    }

    #[test]
    fn a_fresh_project_renders_the_bash_table_shape() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let text = render(&project, &Style::plain()).unwrap();
        assert!(text.starts_with("\n  Exuno Tools\n  ● enabled"));
        assert!(
            text.contains("    ○    Claude Code            claude        available   ·  M  S  \n")
        );
        assert!(
            text.contains("    ○    Zed                    zed           available   ·  ·  S  \n")
        );
        assert!(text.contains("\n  0 of 15 enabled\n\n  Enable a tool:     exuno enable <slug>\n"));
        assert!(text.contains("  Add MCP server:    exuno add mcp <server> --command …\n"));
        assert!(text.ends_with("  Sync outputs:      exuno sync\n\n"));
    }

    #[test]
    fn overrides_and_shared_mcp_change_markers_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        write(
            dir.path(),
            ".ai/src/tools/cursor.yaml",
            "name: \"My Cursor\"\n",
        );
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        write(dir.path(), ".ai/src/mcp/claude.json", "{}");
        write(dir.path(), ".ai/src/mcp.json", "{}");
        write(dir.path(), ".ai/src/tools/kimi/mcp.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        let text = render(&project, &Style::plain()).unwrap();
        assert!(
            text.contains("    ●    Claude Code            claude        enabled     ·  M~ S  \n")
        );
        assert!(
            text.contains("    ○ ★  My Cursor              cursor        available   H* M  ·  \n")
        );
        assert!(
            text.contains("    ○    Kimi Code              kimi          available   ·  M* ·  \n")
        );
        assert!(text.contains("\n  1 of 15 enabled, 1 tool override(s), 3 payload override(s)\n"));
        assert!(text.contains("  Shared MCP: .ai/src/mcp.json (+ 1 per-tool override)\n"));
        assert!(!text.contains("Enable a tool:"));
        assert!(!text.contains("Add MCP server:"));
    }
}
