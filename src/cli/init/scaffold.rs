//! The writes inside the init transaction: scaffold, adopt, CI gate, project config.

use std::path::Path;

use super::Run;
use super::report::summary;
use crate::cli::adopt::{self, Resolver};
use crate::cli::refresh::write_template;
use crate::config::template_manifest::TemplateManifest;
use crate::paths::DiskText;
use crate::project::Project;
use crate::transaction::interrupt::Interrupt;
use crate::{Error, config::catalog, config::format_rev, config::names, engine::staging};

/// `AGENTSYNC_REPO` of `lib/helpers/update.sh`, which the CI template's install
/// URL names.
const REPO: &str = "yelmuratoff/agent_sync";

pub(super) struct Scaffold<'a> {
    pub(super) target: &'a str,
    pub(super) ai_dir: &'a str,
    pub(super) content: &'a [String],
    pub(super) tools: &'a [String],
    pub(super) no_templates: bool,
    pub(super) outputs: &'a str,
    pub(super) adopt: bool,
    pub(super) existing: &'a [String],
    pub(super) ci_github: bool,
    pub(super) detect_source: &'a str,
    pub(super) run_sync: bool,
    pub(super) shown_backup: &'a str,
}

pub(super) enum Failure {
    Io(Error),
    Signal(i32),
}

impl From<Error> for Failure {
    fn from(e: Error) -> Self {
        Failure::Io(e)
    }
}

fn checkpoint(interrupt: &Interrupt) -> Result<(), Failure> {
    match interrupt.received() {
        Some(sig) => Err(Failure::Signal(sig)),
        None => Ok(()),
    }
}

/// The writes between `backup_create` and the summary, each one a point where
/// a failure or a signal restores the snapshot.
pub(super) fn scaffold(
    run: &mut Run,
    interrupt: &mut Interrupt,
    s: Scaffold,
) -> Result<(), Failure> {
    let style = run.style;
    let src = format!("{}/src", s.ai_dir);
    write_content(&s, &src)?;
    checkpoint(interrupt)?;
    let payload_lines = write_payloads(s.tools, &src)?;
    checkpoint(interrupt)?;
    write_project_config(&s, run.env.version)?;
    checkpoint(interrupt)?;

    if !s.existing.is_empty() && s.adopt {
        run.say("\n")?;
        adopt_existing(run, s.target, s.existing)?;
    }
    checkpoint(interrupt)?;

    let mut manifest = TemplateManifest::load(Path::new(s.target))?;
    let templates = catalog::template_files();
    manifest.heal_from_match(
        templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
        |rel| Path::new(&src).join(rel),
    );
    manifest.write(Path::new(s.target))?;
    checkpoint(interrupt)?;

    if s.ci_github {
        write_ci_workflow(run, s.target)?;
    }
    checkpoint(interrupt)?;

    run.say(&summary(style, &s, &payload_lines))?;
    run.say(&format!("Backup: {}\n\n", s.shown_backup))?;
    Ok(())
}

/// The content directories `--content` names, then their shipped templates,
/// or an empty `AGENTS.md` under `--no-templates`.
fn write_content(s: &Scaffold, src: &str) -> Result<(), Error> {
    let has = |section: &str| s.content.iter().any(|c| c == section);
    create_dir(src)?;
    for (section, dir) in [
        ("rules", "rules"),
        ("skills", "skills"),
        ("commands", "commands"),
        ("subagents", "agents"),
    ] {
        if has(section) {
            create_dir(&format!("{src}/{dir}"))?;
        }
    }
    if s.no_templates {
        if has("agents") {
            std::fs::write(format!("{src}/AGENTS.md"), b"")
                .map_err(|e| Error::io(format!("{src}/AGENTS.md"), e))?;
        }
        return Ok(());
    }
    for (rel, bytes) in catalog::template_files() {
        let (dir, _) = rel.rsplit_once('/').unwrap_or(("", rel.as_str()));
        let wanted = match dir {
            "" => has("agents"),
            "rules" => has("rules"),
            "commands" => has("commands"),
            "agents" => has("subagents"),
            _ => has("skills"),
        };
        if wanted {
            write_template(Path::new(&format!("{src}/{rel}")), bytes)?;
        }
    }
    Ok(())
}

/// Each enabled tool's shipped settings and hooks payloads; the paths written,
/// below `src`.
fn write_payloads(tools: &[String], src: &str) -> Result<Vec<String>, Error> {
    let mut written = Vec::new();
    for resource in ["settings", "hooks"] {
        for slug in tools {
            for file in catalog::base_payloads(resource, slug) {
                let name = file
                    .path()
                    .file_name()
                    .map(|n| n.disk_text())
                    .unwrap_or_default();
                let ext = name.rsplit_once('.').map(|(_, ext)| ext).unwrap_or(&name);
                let rel = format!("tools/{slug}/{resource}.{ext}");
                write_template(Path::new(&format!("{src}/{rel}")), file.contents())?;
                written.push(rel);
            }
        }
    }
    Ok(written)
}

/// `agent_sync.yaml`, unless the project already has one in `.ai/` or its root.
fn write_project_config(s: &Scaffold, version: &str) -> Result<(), Error> {
    let config_file = format!("{}/agent_sync.yaml", s.ai_dir);
    if names::CONFIG_CANDIDATES
        .iter()
        .any(|rel| Path::new(&format!("{}/{rel}", s.target)).is_file())
    {
        return Ok(());
    }
    std::fs::write(
        &config_file,
        project_config_text(version, s.tools, s.outputs),
    )
    .map_err(|e| Error::io(&config_file, e))
}

fn create_dir(path: &str) -> Result<(), Error> {
    std::fs::create_dir_all(path).map_err(|e| Error::io(path, e))
}

/// `_init_adopt_existing`.
fn adopt_existing(run: &mut Run, target: &str, existing: &[String]) -> Result<(), Error> {
    let style = run.style;
    let project = Project::at(target)?;
    let sources = adopt::discover_sources(&project)?;
    let mut resolver = Resolver::new(&project, sources, style)?;
    let mut claimed: Vec<String> = Vec::new();
    let mut skips: Vec<String> = Vec::new();
    let mut adopted = 0;
    for file in existing {
        match resolver.resolve(&format!("{target}/{file}"), run.err)? {
            Ok(found) => {
                if claimed.contains(&found.source_rel) {
                    skips.push(format!(
                        "{file} — another file already became {}",
                        found.source_rel
                    ));
                    continue;
                }
                adopt::copy_into_source(&found)?;
                claimed.push(found.source_rel.clone());
                run.say(&format!(
                    "   {} {} → {}\n",
                    style.green("Adopted"),
                    style.cyan(file),
                    style.dim(&found.source_rel)
                ))?;
                adopted += 1;
            }
            Err(reason) => skips.push(format!("{file} — {reason}")),
        }
    }
    for note in &skips {
        run.say(&format!("   {} {note}\n", style.yellow("Kept as-is")))?;
    }
    if !skips.is_empty() {
        run.say(&format!(
            "   {}\n",
            style.dim("Skipped files are regenerated from .ai/src/ — restore them with 'exuno rollback' if needed.")
        ))?;
    }
    if adopted > 0 || !skips.is_empty() {
        run.say("\n")?;
    }
    Ok(())
}

/// `_init_write_ci_workflow`.
fn write_ci_workflow(run: &mut Run, target: &str) -> Result<(), Error> {
    let style = run.style;
    let dest = format!("{target}/.github/workflows/agentsync-check.yml");
    if Path::new(&dest).is_file() {
        return run.say(&format!(
            "   {} {} {}\n",
            style.yellow("Kept"),
            style.cyan(".github/workflows/agentsync-check.yml"),
            style.dim("(already exists)")
        ));
    }
    create_dir(&format!("{target}/.github/workflows"))?;
    let text = catalog::CI_GITHUB_WORKFLOW
        .replace("__AGENTSYNC_VERSION__", run.env.version)
        .replace(
            "__AGENTSYNC_INSTALL_URL__",
            &format!("https://raw.githubusercontent.com/{REPO}/main/install.sh"),
        );
    staging::write_beside(Path::new(&dest), text.as_bytes())?;
    run.say(&format!(
        "   Created {} — CI gate (exuno check)\n",
        style.cyan(".github/workflows/agentsync-check.yml")
    ))
}

/// `_init_create_project_config`'s text.
fn project_config_text(version: &str, tools: &[String], outputs: &str) -> String {
    let enabled = if tools.is_empty() {
        "  enabled: []\n".to_string()
    } else {
        let mut text = "  enabled:\n".to_string();
        for tool in tools {
            text.push_str(&format!("    - {tool}\n"));
        }
        text
    };
    format!(
        "# Exuno — Project Configuration
# All keys are optional — remove any that you leave at the default.

agentsync_version: \"{version}\"
format: {}

# Tools: which ones to sync for this project.
# Each name must match a base tool (see `exuno list`) or a custom override
# file under .ai/src/tools/<name>.yaml.
tools:
{enabled}
# Source paths (override if you use a custom layout).
source:
  agents: \".ai/src/AGENTS.md\"
  rules: \".ai/src/rules\"
  skills: \".ai/src/skills\"
  commands: \".ai/src/commands\"
  subagents: \".ai/src/agents\"
  tools: \".ai/src/tools\"

# cleanup: remove a disabled tool's generated files on the next sync.
defaults:
  cleanup: true

# Post-sync hooks run arbitrary shell — enabling them requires the out-of-repo
# signal AGENTSYNC_ALLOW_POST_SYNC=true, never this in-repo file.
# `skip: true` here always disables them.
post_sync:
  skip: false

# Where generated tool files live.
#   committed — outputs and .ai/.sync-manifest are committed; teammates get
#               them from `git pull` and CI runs `exuno check`.
#   local     — outputs and the manifest are gitignored; every clone runs
#               `exuno sync` (see `exuno setup-hooks`).
outputs: {outputs}

# .gitignore management (false leaves the managed block untouched).
gitignore:
  update: true
",
        format_rev::engine()
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{backups, call, project, quiet};
    use std::path::Path;

    #[test]
    fn existing_outputs_are_adopted_first_wins_or_replaced_like_bash() {
        let (_dir, root) = project(&[
            ("CLAUDE.md", "# Hand-written team rules\n"),
            (".claude/rules/legacy.md", "# Legacy rule\n"),
            (".claude/settings.json", "{\"settings\": true}\n"),
        ]);
        let run = call(&root, &["--tools", "claude", "--yes", "--no-sync"], quiet());
        assert_eq!(run.status, 0);
        assert!(run.out.contains(&format!(
            "Initializing Exuno in {root}\n\n\n   Adopted .claude/rules/legacy.md → .ai/src/rules/legacy.md\n   Adopted .claude/settings.json → .ai/src/tools/claude/settings.json\n   Adopted CLAUDE.md → .ai/src/AGENTS.md\n\n\n   Created .ai/agent_sync.yaml"
        )));
        assert!(
            run.out
                .contains("   Created .ai/src/rules/          — 4 rule(s)\n")
        );
        assert!(
            run.out
                .contains("   Enabled 1 tool(s): claude (auto-detect + --tools)\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/tools/claude/settings.json"))
                .unwrap(),
            "{\"settings\": true}\n"
        );
        let snapshot = backups(&root).pop().unwrap();
        assert!(snapshot.join("files/CLAUDE.md").is_file());
        assert!(snapshot.join("files/.claude/rules/legacy.md").is_file());

        let (_dir, root) = project(&[
            ("CLAUDE.md", "# From CLAUDE\n"),
            ("AGENTS.md", "# From AGENTS\n"),
        ]);
        let two = call(
            &root,
            &["--tools", "claude,codex", "--yes", "--no-sync"],
            quiet(),
        );
        assert!(two.out.contains("\n   Adopted AGENTS.md → .ai/src/AGENTS.md\n   Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md\n   Skipped files are regenerated from .ai/src/ — restore them with 'exuno rollback' if needed.\n\n"));
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# From AGENTS\n"
        );

        let (_dir, root) = project(&[
            (".cursor/rules/core.mdc", "body\n"),
            (".cursor/mcp.json", "{}\n"),
        ]);
        let refused = call(&root, &["--tools", "cursor", "--yes", "--no-sync"], quiet());
        assert!(refused.out.contains("\n   Adopted .cursor/mcp.json → .ai/src/tools/cursor/mcp.json\n   Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.\n   Skipped files"));

        let (_dir, root) = project(&[("CLAUDE.md", "# Hand-written team rules\n")]);
        let replaced = call(
            &root,
            &[
                "--tools",
                "claude",
                "--yes",
                "--existing",
                "replace",
                "--no-sync",
            ],
            quiet(),
        );
        assert!(!replaced.out.contains("Adopted"));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md"))
                .unwrap()
                .starts_with("# ")
        );
        assert_ne!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap(),
            "# Hand-written team rules\n"
        );
    }

    #[test]
    fn the_ci_gate_is_written_once_with_the_pinned_version() {
        let (_dir, root) = project(&[]);
        let run = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(run.out.contains(&format!("Initializing Exuno in {root}\n\n   Created .github/workflows/agentsync-check.yml — CI gate (exuno check)\n\n   Created .ai/agent_sync.yaml")));
        let workflow = Path::new(&root).join(".github/workflows/agentsync-check.yml");
        let text = std::fs::read_to_string(&workflow).unwrap();
        assert!(text.contains("AGENTSYNC_VERSION=9.9.9 bash"));
        assert!(
            text.contains(
                "https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh"
            )
        );
        assert!(!text.contains("__AGENTSYNC_"));
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&workflow).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }

        let (_dir, root) = project(&[(".github/workflows/agentsync-check.yml", "name: mine\n")]);
        let kept = call(
            &root,
            &["--tools", "claude", "--yes", "--ci", "github", "--no-sync"],
            quiet(),
        );
        assert!(
            kept.out
                .contains("\n   Kept .github/workflows/agentsync-check.yml (already exists)\n\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".github/workflows/agentsync-check.yml"))
                .unwrap(),
            "name: mine\n"
        );
        let none = call(
            &root,
            &["--dry-run", "--tools", "claude", "--ci", "github"],
            quiet(),
        );
        assert!(!none.out.contains("workflows"));
    }
}
