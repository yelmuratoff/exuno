//! `exuno diff`: `cmd_diff`, `_diff_payload`, and `_diff_one_tool` of
//! `lib/helpers/customize.sh`. Payload hunks come from the system `diff -u`.

use crate::paths::DiskText;
use std::io::Write;
use std::process::{Command, Stdio};

use super::customize::{VALID_RESOURCES, unknown_resource};
use super::put;
use super::show::{base_tool_shown, read_text};
use crate::config::payload;
use crate::config::tool::Tool;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::project::Project;
use crate::{Error, config::catalog, config::yaml_subset, text};

pub const HELP: Help = Help {
    command: "diff",
    tagline: "show where an override diverges from base",
    synopsis: &["diff [<slug>] [<resource>]"],
    description: &[
        "Show fields where your user override diverges from the shipped base\ntemplate. With no <slug>, walks every customized tool.",
    ],
    sections: &[
        Section {
            title: "ARGUMENTS",
            entries: &[
                ("<slug>", "Tool to diff (default: every customized tool)"),
                (
                    "<resource>",
                    "Payload resource: tool, hooks, mcp, settings\n(default: tool, the YAML config)",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[("-h, --help", "Show this help")],
        },
    ],
    examples: &["diff", "diff cursor", "diff cursor hooks"],
};

pub(crate) const KEYS: [&str; 26] = [
    "name",
    "enabled",
    "targets.agents.dest",
    "targets.rules.dest",
    "targets.rules.extension",
    "targets.rules.header",
    "targets.rules.scoped_header",
    "targets.rules.append_imports",
    "targets.rules.merge_to_file",
    "targets.rules.inline_into_agents",
    "targets.rules.prepend_agents",
    "targets.skills.dest",
    "targets.skills.inline_into_agents",
    "targets.commands.dest",
    "targets.commands.format",
    "targets.commands.as_skills",
    "targets.commands.inline_into_agents",
    "targets.subagents.dest",
    "targets.subagents.format",
    "targets.settings.source",
    "targets.settings.dest",
    "targets.mcp.source",
    "targets.mcp.dest",
    "targets.hooks.source",
    "targets.hooks.dest",
    "post_sync",
];

pub fn diff(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut slug, mut resource) = (String::new(), String::new());
    for arg in args {
        match arg.as_str() {
            "--help" | "-h" => {
                put(out, HELP.render(style).as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(1);
            }
            value if slug.is_empty() => slug = value.to_string(),
            value if resource.is_empty() => resource = value.to_string(),
            _ => {
                put(
                    err,
                    format!("{}: Too many arguments.\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(1);
            }
        }
    }
    let resource = if resource.is_empty() {
        "tool".to_string()
    } else {
        resource
    };
    if !VALID_RESOURCES.contains(&resource.as_str()) {
        return unknown_resource(style, &resource, err);
    }
    let project = discover()?;
    if resource != "tool" {
        if slug.is_empty() {
            put(
                err,
                format!("{}: exuno diff <slug> <resource>\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        }
        return diff_payload(&project, &slug, &resource, style, out, err);
    }

    let overrides = project.user_override_tools()?;
    if overrides.is_empty() && slug.is_empty() {
        put(
            out,
            format!(
                "\n  {}\n\n",
                style.dim("No user overrides — all tools inherit fully from base.")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let mut any = false;
    for tool in overrides.iter().filter(|t| slug.is_empty() || **t == slug) {
        diff_one_tool(&project, tool, style, out)?;
        any = true;
    }
    if !any {
        put(
            err,
            format!("{}: No override found for '{slug}'.\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(1);
    }
    Ok(0)
}

/// `_diff_one_tool`.
fn diff_one_tool(
    project: &Project,
    slug: &str,
    style: &Style,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let base = catalog::base_tool_yaml(slug);
    let user_file = project.user_tool_file(slug);
    let user_text = read_text(&user_file)?;
    let base_line = if base.is_some() {
        format!("    base: {}", base_tool_shown(slug))
    } else {
        "    base: (none — custom tool)".to_string()
    };
    let mut text = format!(
        "\n{}\n{}\n{}\n\n",
        style.bold(&format!("  {slug}")),
        style.dim(&format!("    user: {}", user_file.disk_text())),
        style.dim(&base_line)
    );
    let values = |key: &str| {
        (
            user_text
                .as_deref()
                .map(|t| yaml_subset::value(t, key))
                .unwrap_or_default(),
            base.map(|t| yaml_subset::value(t, key)).unwrap_or_default(),
        )
    };
    let mut printed_override = false;
    for key in KEYS {
        let (user, shipped) = values(key);
        if user.is_empty() || user == shipped {
            continue;
        }
        if !printed_override {
            text.push_str(&format!(
                "    {}\n",
                style.yellow("Your overrides (win over base):")
            ));
            printed_override = true;
        }
        text.push_str(&format!("      {key}\n        you:  {user}\n"));
        if shipped.is_empty() {
            text.push_str(&format!("        base: {}\n", style.dim("(not in base)")));
        } else {
            text.push_str(&format!("        base: {shipped}\n"));
        }
    }
    if printed_override {
        text.push('\n');
    }
    let mut printed_inherit = false;
    for key in KEYS {
        let (user, shipped) = values(key);
        if !user.is_empty() || shipped.is_empty() {
            continue;
        }
        if !printed_inherit {
            text.push_str(&format!(
                "    {}\n",
                style.dim("Inherited from base (remove override to keep inheriting):")
            ));
            printed_inherit = true;
        }
        text.push_str(&format!("      {key:<42}  {shipped}\n"));
    }
    if printed_inherit {
        text.push('\n');
    }
    if !printed_override && !printed_inherit {
        text.push_str(&format!("{}\n", style.dim("    No diverging fields.")));
    }
    put(out, text.as_bytes())
}

/// `_diff_payload`.
fn diff_payload(
    project: &Project,
    slug: &str,
    resource: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let tool = Tool::load(project, slug)?;
    let base = payload::base_source(&tool, resource);
    let user_file = match payload::find_new_override(project, slug, resource)? {
        Some(path) => Some(path),
        None => payload::legacy_override_path(project, &tool, resource).filter(|p| p.is_file()),
    };
    let display = tool.display_name();
    let (base, user_file) = match (base, user_file) {
        (None, None) => {
            put(
                err,
                format!(
                    "{}: No {resource} source for '{slug}' (no base, no override).\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        (Some(base), None) => {
            put(
                out,
                format!(
                    "\n{}\n{}\n\n",
                    style.dim(&format!(
                        "  No override for {display} {resource} — inheriting fully from base."
                    )),
                    style.dim(&format!("  base: {}", base.shown()))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        (None, Some(user)) => {
            put(
                out,
                format!(
                    "\n{}\n{}\n\n",
                    style.yellow(&format!(
                        "  Custom {resource} override (no base to diff against):"
                    )),
                    style.dim(&format!("  override: {}", user.disk_text()))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        (Some(base), Some(user)) => (base, user),
    };
    put(
        out,
        format!(
            "\n{}\n{}\n{}\n\n",
            style.bold(&format!("  {display} — {resource} diff")),
            style.dim(&format!("    override: {}", user_file.disk_text())),
            style.dim(&format!("    base:     {}", base.shown()))
        )
        .as_bytes(),
    )?;
    let base_bytes = base.bytes()?;
    if std::fs::read(&user_file).is_ok_and(|user| user == base_bytes) {
        put(
            out,
            format!(
                "{}\n{}\n",
                style.dim("    Identical — override is a byte-for-byte copy of base."),
                style.dim(&format!(
                    "    Tip: {} can remove redundant overrides.",
                    style.cyan("exuno simplify")
                ))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    put(
        out,
        &text::sed_indent(&unified_diff(&base_bytes, &user_file)),
    )?;
    put(out, b"\n")?;
    Ok(0)
}

/// `diff -u --label base --label override <base> <override> 2>/dev/null`, with
/// the shipped template on stdin; empty when `diff` cannot run.
fn unified_diff(base: &[u8], override_file: &std::path::Path) -> Vec<u8> {
    let child = Command::new("diff")
        .args(["-u", "--label", "base", "--label", "override", "-"])
        .arg(override_file)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let Ok(mut child) = child else {
        return Vec::new();
    };
    let feed = child.stdin.take().map(|mut stdin| {
        let bytes = base.to_vec();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        })
    });
    let output = child
        .wait_with_output()
        .map(|o| o.stdout)
        .unwrap_or_default();
    if let Some(handle) = feed {
        let _ = handle.join();
    }
    output
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn call(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = diff(&args, &discover, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn an_unknown_resource_is_refused_before_the_project_is_looked_up() {
        let args = ["cursor".to_string(), "nope".to_string()];
        let discover = || Project::at("/nonexistent-agentsync-root");
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = diff(&args, &discover, &Style::plain(), &mut out, &mut err).unwrap();
        assert_eq!(status, 1);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            "Error: Unknown resource 'nope'.\nValid: tool hooks mcp settings\n"
        );
    }

    #[test]
    fn diff_reports_overrides_inherited_fields_and_identical_payloads() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools/cursor")).unwrap();
        assert_eq!(
            call(&root, &["-h"]),
            (
                0,
                "\n  exuno diff — show where an override diverges from base\n\n  USAGE\n    exuno diff [<slug>] [<resource>]\n\n  DESCRIPTION\n    Show fields where your user override diverges from the shipped base\n    template. With no <slug>, walks every customized tool.\n\n  ARGUMENTS\n    <slug>       Tool to diff (default: every customized tool)\n    <resource>   Payload resource: tool, hooks, mcp, settings\n                 (default: tool, the YAML config)\n\n  OPTIONS\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno diff\n    exuno diff cursor\n    exuno diff cursor hooks\n\n".to_string(),
                String::new()
            )
        );
        assert_eq!(
            call(&root, &[]).1,
            "\n  No user overrides — all tools inherit fully from base.\n\n"
        );
        assert_eq!(
            call(&root, &["cursro"]),
            (
                1,
                String::new(),
                "Error: No override found for 'cursro'.\n".to_string()
            )
        );
        std::fs::write(
            format!("{root}/.ai/src/tools/cursor.yaml"),
            "name: Cursor\ntargets:\n  rules:\n    dest: \".custom/rules\"\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &["cursor"]);
        assert_eq!(status, 0);
        assert!(out.starts_with(&format!(
            "\n  cursor\n    user: {root}/.ai/src/tools/cursor.yaml\n    base: /<agentsync>/lib/templates/tools/cursor.yaml\n\n    Your overrides (win over base):\n      targets.rules.dest\n        you:  .custom/rules\n        base: .cursor/rules\n\n    Inherited from base (remove override to keep inheriting):\n"
        )));
        assert_eq!(
            call(&root, &["claude"]),
            (
                1,
                String::new(),
                "Error: No override found for 'claude'.\n".to_string()
            )
        );

        std::fs::write(
            format!("{root}/.ai/src/tools/cursor/hooks.json"),
            crate::config::catalog::base_payload("hooks", "cursor")
                .unwrap()
                .contents(),
        )
        .unwrap();
        assert_eq!(
            call(&root, &["cursor", "hooks"]).1,
            format!(
                "\n  Cursor — hooks diff\n    override: {root}/.ai/src/tools/cursor/hooks.json\n    base:     /<agentsync>/lib/templates/hooks/cursor.json\n\n    Identical — override is a byte-for-byte copy of base.\n    Tip: exuno simplify can remove redundant overrides.\n"
            )
        );
        assert_eq!(
            call(&root, &["claude", "settings"]).1,
            "\n  No override for Claude Code settings — inheriting fully from base.\n  base: /<agentsync>/lib/templates/settings/claude.json\n\n"
        );
    }
}
