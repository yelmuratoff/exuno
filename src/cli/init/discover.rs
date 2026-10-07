//! What the project already has: tool markers, existing destinations, and the paths init backs up.

use std::collections::BTreeSet;
use std::path::Path;

use super::Run;
use crate::Error;
use crate::cli::files_below;
use crate::config::tool::Tool;
use crate::engine::render::TARGET_KEYS;
use crate::output::log::Log;
use crate::paths::{DiskText, Paths};
use crate::project::Project;

/// `_init_detect_enabled_tools`: a tool is detected when any marker exists.
const DETECTORS: [(&str, &[&str]); 14] = [
    ("claude", &[".claude", "CLAUDE.md"]),
    ("cursor", &[".cursor", ".cursorrules"]),
    (
        "copilot",
        &[
            ".github/copilot-instructions.md",
            ".github/instructions",
            ".github/prompts",
        ],
    ),
    ("gemini", &[".gemini", "GEMINI.md"]),
    ("codex", &[".codex"]),
    ("kimi", &[".kimi-code"]),
    (
        "opencode",
        &[".opencode", "opencode.json", "opencode.jsonc"],
    ),
    ("windsurf", &[".devin", ".windsurf", ".windsurfrules"]),
    ("junie", &[".junie"]),
    ("cline", &[".cline", ".clinerules"]),
    ("amazonq", &[".amazonq"]),
    ("kiro", &[".kiro"]),
    ("zed", &[".zed", ".rules"]),
    ("antigravity", &[".agents/rules", ".agents/workflows"]),
];

/// `_init_detect_enabled_tools`.
pub(super) fn detect_tools(root: &str) -> Vec<String> {
    DETECTORS
        .iter()
        .filter(|(_, markers)| {
            markers
                .iter()
                .any(|marker| Path::new(root).join(marker).exists())
        })
        .map(|(tool, _)| tool.to_string())
        .collect()
}

/// The destinations a tool's enabled targets name, resolved inside the project.
fn tool_dests(
    project: &Project,
    paths: &Paths,
    slug: &str,
    log: &mut Log,
) -> Result<Vec<String>, Error> {
    let tool = Tool::load(project, slug)?;
    let mut dests = Vec::new();
    for key in TARGET_KEYS {
        if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
            continue;
        }
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        if let Some(abs) = paths.resolve_dest(&raw, &format!("targets.{key}.dest for {slug}"), log)
        {
            dests.push(abs);
        }
    }
    Ok(dests)
}

/// `_init_existing_dest_files`: repo-relative files under the selected tools'
/// destinations, in byte order, once each.
pub(super) fn existing_dest_files(
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let paths = Paths::on_disk(target);
    let prefix = format!("{target}/");
    let mut found = BTreeSet::new();
    for slug in tools {
        for abs in tool_dests(project, &paths, slug, &mut Log::default())? {
            let path = Path::new(&abs);
            if path.is_file() {
                found.insert(abs.strip_prefix(&prefix).unwrap_or(&abs).to_string());
            } else if path.is_dir() {
                let mut files = Vec::new();
                files_below(path, &mut files);
                for file in files {
                    let file = file.disk_text();
                    found.insert(file.strip_prefix(&prefix).unwrap_or(&file).to_string());
                }
            }
        }
    }
    Ok(found.into_iter().collect())
}

/// `_init_collect_backup_targets`; a destination that cannot be resolved is
/// reported the way `resolve_dest_path` logs it and skipped.
pub(super) fn backup_targets(
    run: &mut Run,
    project: &Project,
    target: &str,
    tools: &[String],
) -> Result<Vec<String>, Error> {
    let mut targets = vec![
        format!("{target}/.ai/src"),
        format!("{target}/.ai/agent_sync.yaml"),
        format!("{target}/.ai/.template-manifest"),
    ];
    let paths = Paths::on_disk(target);
    let mut log = Log::default();
    for slug in tools {
        targets.extend(tool_dests(project, &paths, slug, &mut log)?);
    }
    for (_, line) in log.lines() {
        run.tell(&format!("{line}\n"))?;
    }
    Ok(targets)
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{call, project, quiet};
    use std::path::Path;

    #[test]
    fn markers_flags_and_content_shape_the_plan_like_bash() {
        let (_dir, root) = project(&[
            (".claude/x", ""),
            (".cursor/x", ""),
            (".github/instructions/x", ""),
            (".gemini/x", ""),
            (".codex/x", ""),
            (".kimi-code/x", ""),
            (".opencode/x", ""),
            (".windsurf/x", ""),
            (".junie/x", ""),
            (".clinerules", ""),
            (".amazonq/x", ""),
            (".zed/x", ""),
            (".agents/rules/x", ""),
        ]);
        let all = call(&root, &["--dry-run"], quiet());
        assert_eq!(
            all.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules, skills, commands, subagents\n  Tools:    claude, cursor, copilot, gemini, codex, kimi, opencode, windsurf, junie, cline, amazonq, zed, antigravity (detect)\n  settings: claude.json, gemini.json, codex.toml, opencode.json, zed.json\n  hooks:    cursor.json, copilot.json, codex.json, opencode.ts, windsurf.json\n\nDry run — nothing was written.\n"
            )
        );
        assert!(!Path::new(&root).join(".ai").exists());

        let (_dir, root) = project(&[
            ("AGENTS.md", "# generic\n"),
            ("GEMINI.md", ""),
            (".rules", ""),
            ("opencode.json", ""),
        ]);
        let files = call(&root, &["--dry-run"], quiet());
        assert!(files.out.contains("  Tools:    gemini, opencode, zed (detect)\n  settings: gemini.json, opencode.json, zed.json\n  hooks:    opencode.ts\n"));

        let (_dir, root) = project(&[(".cursor/x", "")]);
        let mixed = call(
            &root,
            &[
                "--dry-run",
                "--tools",
                "claude, cursor,claude",
                "--content",
                " rules , skills ",
            ],
            quiet(),
        );
        assert!(mixed.out.contains("  Content:  rules, skills\n  Tools:    claude, cursor (mixed)\n  settings: claude.json\n  hooks:    cursor.json\n"));
        let no_templates = call(
            &root,
            &[
                "--dry-run",
                "--no-templates",
                "--no-detect",
                "--content",
                "agents,rules",
            ],
            quiet(),
        );
        assert_eq!(
            no_templates.out,
            format!(
                "Plan:\n  Target:   {root}/.ai/\n  Content:  agents, rules (no starter templates)\n  Tools:    (none — opt in later via 'exuno enable')\n\nDry run — nothing was written.\n"
            )
        );
        let empty = call(
            &root,
            &["--dry-run", "--no-detect", "--content", ","],
            quiet(),
        );
        assert_eq!(
            (empty.status, empty.err.as_str()),
            (1, "Error: --content requires a value\n")
        );
        let kimi = call(
            &root,
            &["--dry-run", "--no-detect", "--tools", "kimi"],
            quiet(),
        );
        assert!(kimi.out.contains("  Tools:    kimi (flag)\n  No payloads to scaffold — tools will use base templates at sync time.\n"));
    }
}
