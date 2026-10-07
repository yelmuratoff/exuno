//! `exuno check`: render what `sync --force` would write and compare every
//! managed output with the project, with the messages and exit codes of
//! `lib/check.sh`. Nothing is copied and nothing is written.

use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;

use super::stale_targets;
use crate::engine::render::{self, Env};
use crate::engine::session::Session;
use crate::engine::workspace::Workspace;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::paths::Paths;
use crate::project::Project;
use crate::transaction::manifest::Manifest;
use crate::{
    Error, config::project_config, config::version, config::yaml_subset, engine::overlay,
    engine_version, paths,
};

const MANIFEST_REL: &str = ".ai/.sync-manifest";

pub const HELP: Help = Help {
    command: "check",
    tagline: "verify outputs are in sync with source",
    synopsis: &["check"],
    description: &[
        "Renders what exuno sync --force would write into a temporary\nworkspace and compares every managed output with the project: files\nthat differ, outputs that are missing, and outputs no longer\ngenerated. Nothing on disk is written.",
        "The report goes to stdout so a hook or CI step can read it; the exit\nstatus carries the verdict, following grep's shape.",
        "Honours EXUNO_CONFIG_PATH for the project config and\nEXUNO_REPO_ROOT for the project root, as exuno sync does.",
    ],
    sections: &[
        Section {
            title: "OPTIONS",
            entries: &[("-h, --help", "Show this help")],
        },
        Section {
            title: "EXIT STATUS",
            entries: &[
                ("0", "Every managed output matches the source"),
                ("1", "Out of sync, or the check could not run"),
            ],
        },
        Section {
            title: "SEE ALSO",
            entries: &[("exuno sync", "Write the outputs the check compares against")],
        },
    ],
    examples: &["check"],
};

pub fn run(
    args: &[String],
    root: &str,
    env: &Env,
    style: &Style,
    out: &mut impl Write,
    err: &mut impl Write,
) -> Result<u8, Error> {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        out.write_all(HELP.render(style).as_bytes())
            .map_err(|e| Error::io("<stdout>", e))?;
        return Ok(0);
    }
    let report = check(root, env)?;
    out.write_all(report.stdout.as_bytes())
        .map_err(|e| Error::io("<stdout>", e))?;
    err.write_all(report.stderr.as_bytes())
        .map_err(|e| Error::io("<stderr>", e))?;
    Ok(report.status)
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub stdout: String,
    pub stderr: String,
    pub status: u8,
}

impl Report {
    fn out(&mut self, line: &str) {
        self.stdout.push_str(line);
        self.stdout.push('\n');
    }

    fn err(&mut self, line: &str) {
        self.stderr.push_str(line);
        self.stderr.push('\n');
    }
}

pub fn check(root: &str, env: &Env) -> Result<Report, Error> {
    let mut report = Report::default();
    let is_file = |path: &str| Path::new(path).is_file();
    let config_path = match project_config::select(root, env.config_path.as_deref(), &is_file) {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            report.err(&format!("✗ {}", project_config::missing_message(&path)));
            report.status = 1;
            return Ok(report);
        }
    };
    let config = config_path.as_deref().map(read_config).transpose()?;
    if let Some(message) = version_pin_mismatch(root, config_path.as_deref(), config.as_deref()) {
        for line in message {
            report.err(&line);
        }
        report.status = 1;
        return Ok(report);
    }
    report.out("Checking Exuno configuration synchronization...");

    let manifest = manifest_paths(root)?;
    let ws = match seed_workspace(root, &manifest, config_path.as_deref(), config.as_deref()) {
        Ok(ws) => ws,
        Err(detail) => {
            report.out("✗ Failed to prepare temporary workspace for check");
            report.err(&detail);
            report.status = 1;
            return Ok(report);
        }
    };

    let mut session = Session::new(ws, Paths::for_disk_root(root));
    session.force = true;
    session.set_owned_before(
        Manifest::load(root)?
            .map(|m| m.owned_records())
            .unwrap_or_default(),
    );
    merge_shared_parent(&mut session.ws, root, config.as_deref())?;
    if render::render(&mut session, env).is_err() {
        report.out("✗ Sync script failed during check");
        report.out("Sync output (last 40 lines):");
        for line in session.log.tail(40) {
            report.out(line);
        }
        report.status = 1;
        return Ok(report);
    }

    let project = Project::select(root, env.config_path.as_deref())?;
    for stale in stale_targets::left_by_disabled_targets(&project, &manifest)? {
        for line in stale_targets::lines(&stale) {
            report.out(&line);
        }
    }

    let mut compare: BTreeSet<String> = manifest.into_iter().collect();
    for rel in session.touched() {
        if session.ws.is_file(&format!("{root}/{rel}")) {
            compare.insert(rel.clone());
        }
    }

    let mut differences = Vec::new();
    for rel in compare {
        let expected = format!("{root}/{rel}");
        let actual = Path::new(root).join(&rel);
        match (session.ws.is_file(&expected), actual.is_file()) {
            (true, true) => {
                let rendered = session.ws.read(&expected)?;
                let on_disk = std::fs::read(&actual).map_err(|e| Error::io(&actual, e))?;
                if rendered != on_disk {
                    differences.push(format!("Files {rel} differ"));
                }
            }
            (true, false) => differences.push(format!("Missing: {rel}")),
            (false, true) => differences.push(format!("No longer generated: {rel}")),
            (false, false) => {}
        }
    }

    if differences.is_empty() {
        report.out("✓ Exuno configurations are safe and synced.");
        return Ok(report);
    }
    report.out("");
    report.out("!  Exuno configurations are out of sync with source.");
    report.out("Differences detected (showing up to 20):");
    for line in differences.iter().take(20) {
        report.out(line);
    }
    report.out("");
    report.out("Please run: exuno sync");
    report.status = 1;
    Ok(report)
}

fn read_config(path: &str) -> Result<String, Error> {
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// `_check_version_pin`: an unknown mode, or a mismatch with committed outputs
/// or `version_pin.mode: strict`, fails before the banner.
fn version_pin_mismatch(
    root: &str,
    path: Option<&str>,
    config: Option<&str>,
) -> Option<Vec<String>> {
    let (Some(path), Some(config)) = (path, config) else {
        return None;
    };
    let mode = match version::mode(config) {
        Ok(mode) => mode,
        Err(value) => {
            let shown = path.strip_prefix(&format!("{root}/")).unwrap_or(path);
            return Some(vec![format!(
                "✗ Unknown version_pin.mode '{value}' in {shown} — expected 'warn' or 'strict'"
            )]);
        }
    };
    let committed = project_config::outputs_mode(config) == Ok("committed");
    if !committed && mode != version::Mode::Strict {
        return None;
    }
    let pinned = crate::config::names::pinned_version(config);
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return None;
    }
    let [first, second] = version::hint(&pinned, engine, str::to_string);
    Some(vec![
        format!("✗ {}", version::mismatch_error(&pinned, engine, committed)),
        first,
        second,
    ])
}

/// `manifest_paths`: the text before the first tab of every complete line.
fn manifest_paths(root: &str) -> Result<Vec<String>, Error> {
    let path = Path::new(root).join(MANIFEST_REL);
    if !path.is_file() {
        return Ok(Vec::new());
    }
    let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    lines.pop();
    Ok(lines
        .into_iter()
        .map(|line| line.trim_start_matches('\t'))
        .map(|line| line.split('\t').next().unwrap_or(""))
        .filter(|rel| !rel.is_empty())
        .map(str::to_string)
        .collect())
}

/// What `lib/check.sh` copied with `tar`: `.ai/` without backups, a root
/// `agent_sync.yaml`, and every manifest output that exists; plus the
/// configured sources inside the project but outside `.ai/`, which its
/// isolated sync read from the project through `AGENTSYNC_INTERNAL_SOURCE_BASE_ROOT`.
fn seed_workspace(
    root: &str,
    manifest: &[String],
    selected: Option<&str>,
    config_text: Option<&str>,
) -> Result<Workspace, String> {
    let mut ws = Workspace::new(root);
    let ai = Path::new(root).join(".ai");
    if !ai.exists() {
        return Err("Incomplete copy — missing: .ai".to_string());
    }
    let skip_in_ai = |rel: &str| rel == "backups" || rel.starts_with("backups/") || is_git(rel);
    ws.seed_from_disk(&format!("{root}/.ai"), &ai, &skip_in_ai)
        .map_err(|e| e.to_string())?;
    let config = Path::new(root).join("agent_sync.yaml");
    if config.is_file() {
        ws.seed_from_disk(&format!("{root}/agent_sync.yaml"), &config, &is_git)
            .map_err(|e| e.to_string())?;
    }
    for rel in manifest {
        if rel.starts_with(".ai/") || is_git(rel) {
            continue;
        }
        let disk = Path::new(root).join(rel);
        if disk.exists() {
            ws.seed_from_disk(&format!("{root}/{rel}"), &disk, &is_git)
                .map_err(|e| e.to_string())?;
        }
    }
    if let Some(path) = selected
        && !path.starts_with(&format!("{root}/.ai/"))
        && path != format!("{root}/agent_sync.yaml")
    {
        ws.seed_from_disk(path, Path::new(path), &is_git)
            .map_err(|e| e.to_string())?;
    }
    for key in [
        "agents",
        "rules",
        "skills",
        "commands",
        "subagents",
        "tools",
    ] {
        let Some(text) = config_text else {
            break;
        };
        let nested = yaml_subset::value(text, &format!("source.{key}"));
        let raw = if nested.is_empty() && key != "tools" {
            yaml_subset::value(text, key)
        } else {
            nested
        };
        if raw.is_empty() {
            continue;
        }
        let abs = if crate::paths::is_absolute(&raw) {
            paths::normalize(&raw)
        } else {
            paths::normalize(&format!("{root}/{raw}"))
        };
        let below_root = abs.starts_with(&format!("{root}/"));
        if !below_root || abs.starts_with(&format!("{root}/.ai/")) || is_git(&abs) {
            continue;
        }
        if Path::new(&abs).exists() {
            ws.seed_from_disk(&abs, Path::new(&abs), &is_git)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(ws)
}

fn is_git(rel: &str) -> bool {
    rel.split('/').any(|segment| segment == ".git")
}

/// Merge the `shared:` parent into the workspace, as the isolated sync of
/// `lib/check.sh` inherits it.
fn merge_shared_parent(ws: &mut Workspace, root: &str, config: Option<&str>) -> Result<(), Error> {
    let Some(config) = config else {
        return Ok(());
    };
    if let Some(parent) = overlay::shared_parent_src(config, root) {
        let inherit = yaml_subset::value(config, "shared.inherit");
        overlay::merge_shared_parent(ws, &parent, &overlay::inherit_categories(&inherit))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        write(&root, ".ai/src/AGENTS.md", "# Agents\n");
        write(
            &root,
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let root = root.disk_text();
        (dir, root)
    }

    #[test]
    fn help_is_answered_on_stdout_without_a_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(
            &["--help".to_string()],
            &root,
            &Env::default(),
            &Style::plain(),
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(err, b"");
        let out = String::from_utf8(out).unwrap();
        assert_eq!(out, HELP.render(&Style::plain()));
        assert!(out.starts_with(
            "\n  exuno check — verify outputs are in sync with source\n\n  USAGE\n    exuno check\n"
        ));
    }

    #[test]
    fn a_project_never_synced_reports_every_output_missing() {
        let (_dir, root) = project();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking Exuno configuration synchronization...\n\n!  Exuno configurations are out of sync with source.\nDifferences detected (showing up to 20):\n"));
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
        assert!(report.stdout.ends_with("\nPlease run: exuno sync\n"));
    }

    #[test]
    fn sources_inside_the_project_but_outside_ai_are_read_from_the_project() {
        let (_dir, root) = project();
        write(Path::new(&root), "sources/AGENTS.md", "# External\n");
        write(Path::new(&root), "sources/tools/claude.yaml", "name: C\n");
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nsource:\n  agents: \"sources/AGENTS.md\"\n  tools: \"sources/tools\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert!(
            !report.stdout.contains("Sync script failed"),
            "{}",
            report.stdout
        );
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
    }

    #[test]
    fn manifest_outputs_that_match_the_render_are_in_sync() {
        let (_dir, root) = project();
        let mut session = Session::new(
            seed_workspace(
                &root,
                &[],
                Some(&format!("{root}/.ai/agent_sync.yaml")),
                None,
            )
            .unwrap(),
            Paths::for_disk_root(&root),
        );
        render::render(&mut session, &Env::default()).unwrap();
        let mut manifest = String::new();
        for rel in session.touched() {
            let bytes = session.ws.read(&format!("{root}/{rel}")).unwrap();
            write(Path::new(&root), rel, &String::from_utf8(bytes).unwrap());
            manifest.push_str(&format!("{rel}\thash\n"));
        }
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let clean = check(&root, &Env::default()).unwrap();
        assert_eq!(
            clean.stdout,
            "Checking Exuno configuration synchronization...\n✓ Exuno configurations are safe and synced.\n"
        );
        assert_eq!(clean.status, 0);

        write(Path::new(&root), "CLAUDE.md", "edited\n");
        write(Path::new(&root), ".cursor/rules/core.mdc", "stale\n");
        manifest.push_str(".cursor/rules/core.mdc\thash\n");
        write(Path::new(&root), MANIFEST_REL, &manifest);
        let dirty = check(&root, &Env::default()).unwrap();
        assert!(dirty.stdout.contains("Files CLAUDE.md differ\n"));
        assert!(
            dirty
                .stdout
                .contains("No longer generated: .cursor/rules/core.mdc\n")
        );
    }

    #[test]
    fn a_committed_pin_mismatch_fails_before_the_banner() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "outputs: committed\nagentsync_version: \"0.0.1\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert_eq!(report.stdout, "");
        assert!(
            report
                .stderr
                .starts_with("✗ This project pins exuno 0.0.1 but you are running ")
        );
    }

    #[test]
    fn a_failed_render_prints_the_log_tail() {
        let (_dir, root) = project();
        std::fs::remove_file(Path::new(&root).join(".ai/src/AGENTS.md")).unwrap();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.starts_with("Checking Exuno configuration synchronization...\n✗ Sync script failed during check\nSync output (last 40 lines):\n[ERROR] Source agents file not found: "));
    }

    #[test]
    fn a_missing_ai_directory_fails_the_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(
            report,
            Report {
                stdout: "Checking Exuno configuration synchronization...\n✗ Failed to prepare temporary workspace for check\n".into(),
                stderr: "Incomplete copy — missing: .ai\n".into(),
                status: 1,
            }
        );
    }

    #[test]
    fn manifest_lines_keep_the_text_before_the_first_tab_of_complete_lines() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            MANIFEST_REL,
            "a.md\th\n\tb.md\th\nc.md\nlast\th",
        );
        assert_eq!(manifest_paths(&root).unwrap(), ["a.md", "b.md", "c.md"]);
    }

    fn with_config(path: &str) -> Env {
        Env {
            config_path: Some(path.to_string()),
            ..Env::default()
        }
    }

    #[test]
    fn a_missing_explicit_config_fails_before_the_banner() {
        let (_dir, root) = project();
        let report = check(&root, &with_config("missing.yaml")).unwrap();
        assert_eq!(
            report,
            Report {
                stdout: String::new(),
                stderr: format!(
                    "✗ EXUNO_CONFIG_PATH is set but file not found: {root}/missing.yaml\n"
                ),
                status: 1,
            }
        );
    }

    #[test]
    fn a_relative_explicit_config_outside_dot_ai_drives_the_render() {
        let (_dir, root) = project();
        std::fs::remove_file(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        write(
            Path::new(&root),
            "config/agentsync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let report = check(&root, &with_config("config/agentsync.yaml")).unwrap();
        assert_eq!(report.status, 1);
        assert!(report.stdout.contains("Missing: CLAUDE.md\n"));
    }

    #[test]
    fn a_strict_pin_and_an_unknown_mode_fail_local_outputs_before_the_banner() {
        let (_dir, root) = project();
        let engine = engine_version();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "outputs: local\nagentsync_version: \"0.0.1\"\nversion_pin:\n  mode: strict\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!((report.status, report.stdout.as_str()), (1, ""));
        assert_eq!(
            report.stderr,
            format!(
                "✗ This project pins exuno 0.0.1 but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version.\n  • Match the pin:  exuno update 0.0.1\n  • Or move it:     exuno upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)\n"
            )
        );

        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "version_pin:\n  mode: refuse\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(
            report.stderr,
            "✗ Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml — expected 'warn' or 'strict'\n"
        );
        assert_eq!(report.status, 1);
    }

    #[test]
    fn gitignore_update_false_without_outputs_counts_as_committed() {
        let (_dir, root) = project();
        write(
            Path::new(&root),
            ".ai/agent_sync.yaml",
            "gitignore:\n  update: false\nagentsync_version: \"0.0.1\"\n",
        );
        let report = check(&root, &Env::default()).unwrap();
        assert_eq!(report.status, 1);
        assert!(
            report
                .stderr
                .starts_with("✗ This project pins exuno 0.0.1 but you are running ")
        );
        assert!(
            report
                .stderr
                .contains("— committed outputs must come from one version everywhere.\n")
        );
    }
}
