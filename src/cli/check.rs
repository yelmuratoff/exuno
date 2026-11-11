//! `exuno check`: render what `sync --force` would write and compare every
//! managed output with the project, with the messages and exit codes of
//! `lib/check.sh`. Nothing is copied and nothing is written.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use super::stale_targets;
use crate::engine::render::{self, Env, Run, Selection};
use crate::engine::session::Session;
use crate::engine::workspace::Workspace;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::paths::{DiskText, Paths};
use crate::project::Project;
use crate::transaction::manifest::Manifest;
use crate::{
    Error, config::catalog, config::profiles, config::project_config, config::tool::Tool,
    config::version, config::yaml_subset, engine::overlay, engine_version, paths,
};

const MANIFEST_REL: &str = ".ai/.sync-manifest";

pub const HELP: Help = Help {
    command: "check",
    tagline: "verify outputs are in sync with source",
    synopsis: &["check"],
    description: &[
        "Renders what exuno sync --force would write into a temporary\nworkspace and compares every managed output with the project: files\nthat differ, outputs that are missing, outputs no longer generated,\nand outputs edited since the last sync that still match it, which\nexuno sync refuses to overwrite. Nothing on disk is written.",
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
    let previous = Manifest::load(root)?;
    let rendered = render_in_memory(
        root,
        &manifest,
        (config_path.as_deref(), config.as_deref()),
        env,
        Selection::default(),
        Purpose::Check,
    )?;
    let (session, run) = match rendered {
        InMemory::Rendered(session, run) => (session, run),
        InMemory::SeedFailed(detail) => {
            report.out("✗ Failed to prepare temporary workspace for check");
            report.err(&detail);
            report.status = 1;
            return Ok(report);
        }
        InMemory::RenderFailed(session, _) => {
            report.out("✗ Sync script failed during check");
            report.out("Sync output (last 40 lines):");
            for line in session.log.tail(40) {
                report.out(line);
            }
            report.status = 1;
            return Ok(report);
        }
    };
    let stops = hazards(&session, &run, root, previous.as_ref()).lines("A real sync would stop: ");
    for line in &stops {
        report.out(line);
    }
    let manual_edits: BTreeSet<String> = previous
        .map(|m| m.manual_edits(root, &run.keyed_dests))
        .unwrap_or_default()
        .into_iter()
        .map(|edit| edit.rel)
        .collect();

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
    let mut edited_to_source = false;
    for rel in compare {
        let expected = format!("{root}/{rel}");
        let actual = Path::new(root).join(&rel);
        match (session.ws.is_file(&expected), actual.is_file()) {
            (true, true) => {
                let rendered = session.ws.read(&expected)?;
                let on_disk = std::fs::read(&actual).map_err(|e| Error::io(&actual, e))?;
                if rendered != on_disk {
                    differences.push(format!("Files {rel} differ"));
                } else if manual_edits.contains(&rel) && session.touched().contains(&rel) {
                    differences.push(format!("Edited since last sync: {rel} (matches source)"));
                    edited_to_source = true;
                } else if manual_edits.contains(&rel) {
                    differences.push(format!("Edited since last sync: {rel}"));
                }
            }
            (true, false) => differences.push(format!("Missing: {rel}")),
            (false, true) => differences.push(format!("No longer generated: {rel}")),
            (false, false) => {}
        }
    }

    if differences.is_empty() && stops.is_empty() {
        report.out("✓ Exuno configurations are safe and synced.");
        return Ok(report);
    }
    if differences.is_empty() {
        report.status = 1;
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
    if edited_to_source {
        report.out(
            "  • Edited files that match the source stop exuno sync; exuno sync --force records them as they are, and also rewrites any other edited output",
        );
    }
    report.status = 1;
    Ok(report)
}

/// What a sync would write, rendered in memory the way `check` sees it.
pub(crate) enum InMemory {
    Rendered(Box<Session>, Box<Run>),
    SeedFailed(String),
    RenderFailed(Box<Session>, u8),
}

/// Who renders in memory: `check` runs every refusal itself; a sync's
/// preflight has already passed them on the disk.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Purpose {
    Check,
    Preflight,
}

/// Renders the sync of `root` for `selection` into a workspace seeded from
/// `.ai/`, the recorded outputs, and the other in-project inputs, as
/// `sync --force` would write it, with no post-sync hook and no disk write.
pub(crate) fn render_in_memory(
    root: &str,
    manifest: &[String],
    config: (Option<&str>, Option<&str>),
    env: &Env,
    selection: Selection,
    purpose: Purpose,
) -> Result<InMemory, Error> {
    let (config_path, config) = config;
    let ws = match seed_workspace(root, manifest, config_path, config) {
        Ok(ws) => ws,
        Err(detail) => return Ok(InMemory::SeedFailed(detail)),
    };
    let previous = Manifest::load(root)?;
    let mut session = Session::new(ws, Paths::for_disk_root(root));
    session.force = true;
    session.activate_manifest(previous.as_ref().map(Manifest::paths).unwrap_or_default());
    session.set_owned_before(
        previous
            .as_ref()
            .map(Manifest::owned_records)
            .unwrap_or_default(),
    );
    merge_shared_parent(&mut session.ws, root, config)?;
    let rendered = match purpose {
        Purpose::Check => render::render_selected(&mut session, env, selection),
        Purpose::Preflight => render::render_for_preflight(&mut session, env, selection),
    };
    Ok(match rendered {
        Ok(run) => InMemory::Rendered(Box::new(session), Box::new(run)),
        Err(stop) => InMemory::RenderFailed(Box::new(session), stop.0),
    })
}

/// What stops a real sync before it writes anything.
#[derive(Clone, Default)]
pub(crate) struct Hazards {
    /// One refusal per link sync may not write or remove through.
    pub refusals: Vec<String>,
    /// Existing files the previous manifest does not record that the render changes.
    pub unowned: Vec<String>,
}

impl Hazards {
    pub fn is_empty(&self) -> bool {
        self.refusals.is_empty() && self.unowned.is_empty()
    }

    /// The report lines, each headline after `lead`.
    pub fn lines(&self, lead: &str) -> Vec<String> {
        let mut lines: Vec<String> = self
            .refusals
            .iter()
            .map(|refusal| format!("{lead}{refusal}"))
            .collect();
        if !self.unowned.is_empty() {
            lines.push(format!("{lead}{}", unowned_headline(self.unowned.len())));
            lines.extend(self.unowned.iter().map(|rel| format!("      {rel}")));
        }
        lines
    }
}

pub(crate) fn unowned_headline(count: usize) -> String {
    format!("Sync would overwrite {count} file(s) it did not write:")
}

/// Compares the in-memory render of `root` with the disk before anything is
/// written. Settings and MCP files owned by key merge by design, a first sync
/// (no previous manifest) owns whatever it writes, and a path under a dest the
/// disk resolves outside the project is one a real sync skips, not writes.
pub(crate) fn hazards(
    rendered: &Session,
    run: &Run,
    root: &str,
    previous: Option<&Manifest>,
) -> Hazards {
    let recorded = previous.map(Manifest::paths);
    let disk = Paths::on_disk(root);
    let skipped_by_sync = |abs: &str| {
        run.backup_targets
            .iter()
            .filter(|dest| paths::is_within(abs, dest))
            .max_by_key(|dest| dest.len())
            .is_some_and(|dest| {
                disk.resolve_dest(dest, "", &mut crate::output::log::Log::default())
                    .is_none()
            })
    };
    let mut refused: BTreeMap<(&'static str, String), Vec<String>> = BTreeMap::new();
    let mut found = Hazards::default();
    if run.update_gitignore
        && let Some(reason) = rendered
            .paths
            .unsafe_output(&format!("{root}/.gitignore"), true)
    {
        refused
            .entry(("write", reason))
            .or_default()
            .push(".gitignore".to_string());
    }
    for rel in recorded.iter().flatten() {
        let abs = format!("{root}/{rel}");
        if rendered.ws.is_file(&abs) || !Path::new(&abs).is_file() || skipped_by_sync(&abs) {
            continue;
        }
        if let Some(reason) = rendered.paths.unsafe_output(&abs, false) {
            refused
                .entry(("remove", reason))
                .or_default()
                .push(rel.clone());
        }
    }
    for rel in rendered.touched() {
        let abs = format!("{root}/{rel}");
        if skipped_by_sync(&abs) {
            continue;
        }
        if let Some(reason) = rendered.paths.unsafe_output(&abs, true) {
            refused
                .entry(("write", reason))
                .or_default()
                .push(rel.clone());
            continue;
        }
        if let Some(blocker) = blocker(root, &abs) {
            let blocker_rel = blocker[root.len() + 1..].to_string();
            if !written_by_sync(&blocker, root, recorded.as_ref())
                && !found.unowned.contains(&blocker_rel)
            {
                found.unowned.push(blocker_rel);
            }
            continue;
        }
        let Some(recorded) = &recorded else {
            continue;
        };
        if recorded.contains(rel) || run.keyed_dests.contains(rel) {
            continue;
        }
        let on_disk = match std::fs::read(&abs) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) => {
                found
                    .refusals
                    .push(format!("Could not read {rel} to compare it: {e}"));
                continue;
            }
        };
        if rendered.ws.read(&abs).ok() != Some(on_disk) {
            found.unowned.push(rel.clone());
        }
    }
    for ((action, reason), rels) in refused {
        let path = match rels.len() {
            1 => rels[0].clone(),
            count => format!("{} and {} more file(s)", rels[0], count - 1),
        };
        let refusal = Error::UnsafeOutput {
            action,
            path,
            reason,
        };
        found.refusals.push(refusal.to_string());
    }
    found
}

/// The entry on disk that stands where the render puts the file `abs`: a
/// folder at `abs`, or a file where the render needs one of its folders.
fn blocker(root: &str, abs: &str) -> Option<String> {
    if std::fs::symlink_metadata(abs).is_ok_and(|meta| meta.is_dir()) {
        return Some(abs.to_string());
    }
    let mut dir = paths::parent(abs);
    while dir != root && paths::is_within(&dir, root) {
        match std::fs::symlink_metadata(&dir) {
            Ok(meta) if meta.is_file() => return Some(dir),
            Ok(_) => return None,
            Err(_) => dir = paths::parent(&dir),
        }
    }
    None
}

/// Whether the previous manifest records `path`, or every file below it.
fn written_by_sync(path: &str, root: &str, recorded: Option<&BTreeSet<String>>) -> bool {
    let Some(recorded) = recorded else {
        return false;
    };
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return true;
    };
    if !meta.is_dir() {
        return recorded.contains(&path[root.len() + 1..]);
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return false;
    };
    entries.into_iter().all(|entry| {
        entry.is_ok_and(|entry| {
            written_by_sync(
                &format!("{path}/{}", entry.file_name().to_string_lossy()),
                root,
                Some(recorded),
            )
        })
    })
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
pub(crate) fn manifest_paths(root: &str) -> Result<Vec<String>, Error> {
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
        .filter(|rel| !rel.is_empty() && !rel.starts_with('#'))
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
    for abs in inputs_outside_ai(root, config_text) {
        if !ws.exists(&abs) && Path::new(&abs).exists() {
            ws.seed_from_disk(&abs, Path::new(&abs), &is_git)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(ws)
}

/// The other inputs a sync reads from the project outside `.ai/`: profile
/// overlays and every tool's `targets.<key>.source`.
fn inputs_outside_ai(root: &str, config_text: Option<&str>) -> Vec<String> {
    let absolute = |raw: &str| {
        if crate::paths::is_absolute(raw) {
            paths::normalize(raw)
        } else {
            paths::normalize(&format!("{root}/{raw}"))
        }
    };
    let mut raw_paths: Vec<String> = Vec::new();
    if let Some(text) = config_text {
        raw_paths.extend(
            profiles::names(text)
                .iter()
                .map(|name| profiles::overlay_dir(text, name)),
        );
    }
    let tools_dir = config_text
        .map(|text| yaml_subset::value(text, "source.tools"))
        .filter(|raw| !raw.is_empty())
        .unwrap_or_else(|| ".ai/src/tools".to_string());
    let tools_dir = absolute(&tools_dir);
    let mut slugs: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    if let Ok(entries) = std::fs::read_dir(&tools_dir) {
        slugs.extend(entries.filter_map(|entry| {
            let name = entry.ok()?.file_name().disk_text();
            Some(name.strip_suffix(".yaml")?.to_string())
        }));
    }
    for slug in slugs {
        let user = std::fs::read_to_string(format!("{tools_dir}/{slug}.yaml")).ok();
        let tool = Tool::new(&slug, user);
        for key in render::TARGET_KEYS {
            let raw = tool.value(&format!("targets.{key}.source"));
            if !raw.is_empty() {
                raw_paths.push(raw);
            }
        }
    }
    let ai = format!("{root}/.ai/");
    raw_paths
        .iter()
        .map(|raw| absolute(raw))
        .filter(|abs| abs.starts_with(&format!("{root}/")) && !abs.starts_with(&ai) && !is_git(abs))
        .collect::<BTreeSet<String>>()
        .into_iter()
        .collect()
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
            let hash = crate::transaction::manifest::sha256_hex(&bytes);
            write(Path::new(&root), rel, &String::from_utf8(bytes).unwrap());
            manifest.push_str(&format!("{rel}\t{hash}\n"));
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
