//! `exuno migrate`: `cmd_migrate` of `lib/helpers/migrate.sh`, which prints
//! an upgrade prompt or, with `--legacy`, retires pre-0.11 layouts.

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{files_below, put, sorted_entries};
use crate::config::leftovers::{self, Kind, Leftover};
use crate::config::template_manifest::{self, TemplateManifest};
use crate::engine::{skill_tree, workspace::Workspace};
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::project::Project;
use crate::{Error, config::catalog, config::format_rev, config::names, config::yaml_edit};

pub const HELP: Help = Help {
    command: "migrate",
    tagline: "upgrade a project to the current format",
    synopsis: &["migrate", "migrate --legacy [--apply] [--yes]"],
    description: &[
        "Prints an AI prompt for safely upgrading an existing Exuno project to\nthe latest documented format and copies it to the system clipboard.",
        "With --legacy, moves legacy flat-layout overrides to the canonical\nper-tool layout. When every legacy MCP file is byte-identical, migrate\noffers to consolidate them into the shared .ai/src/mcp.json. Dry-run by\ndefault: re-run with --apply to move files.",
    ],
    sections: &[
        Section {
            title: "LEGACY MOVES",
            entries: &[
                (
                    ".ai/src/hooks/<tool>.<ext>",
                    "→ .ai/src/tools/<tool>/hooks.<ext>",
                ),
                (
                    ".ai/src/mcp/<tool>.<ext>",
                    "→ .ai/src/tools/<tool>/mcp.<ext>",
                ),
                (
                    ".ai/src/settings/<tool>.<ext>",
                    "→ .ai/src/tools/<tool>/settings.<ext>",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--legacy",
                    "Preview old flat-layout file moves without changing files",
                ),
                (
                    "--apply",
                    "Apply those moves (backwards-compatible historical behavior)",
                ),
                (
                    "-y, --yes",
                    "Accept safe legacy consolidation without prompting",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
    ],
    examples: &[
        "migrate",
        "migrate --legacy",
        "migrate --legacy --apply --yes",
    ],
};

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;

/// What `migrate` takes from the process and the terminal.
pub struct Env<'a> {
    pub version: &'a str,
    /// `${AGENTSYNC_REPO_ROOT:-$(pwd)}` as the prompt reads it, unchecked.
    pub prompt_root: String,
    pub no_clipboard: bool,
    pub stdout_tty: bool,
    pub interactive: bool,
    pub confirm: &'a mut dyn FnMut(&str, bool) -> bool,
    /// The first clipboard tool's exit status, `None` when none is installed.
    pub copy: &'a mut dyn FnMut(&str) -> Option<i32>,
}

pub fn migrate(
    args: &[String],
    discover: Discover,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match args.first().map(String::as_str) {
        Some("--legacy") => legacy(&args[1..], discover, style, env, out, err),
        Some("--apply" | "--yes" | "-y") => legacy(args, discover, style, env, out, err),
        _ => prompt(args, style, env, out, err),
    }
}

/// `_cmd_migrate_prompt`.
fn prompt(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match args.first().map(String::as_str) {
        Some("--help" | "-h") => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Some(flag) => {
            put(
                err,
                format!(
                    "{}: Unknown flag: {flag}\nUsage: exuno migrate [--legacy [--apply] [--yes]]\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        None => {}
    }

    let text = format!(
        "## Exuno migration context\n\n- Exuno CLI that generated this prompt: {}\n- Project-pinned Exuno version: {}\n\n---\n\n{}",
        env.version,
        project_version(&env.prompt_root),
        catalog::MIGRATE_PROMPT.trim_end_matches('\n')
    );
    let status = if env.no_clipboard {
        Some(3)
    } else {
        match (env.copy)(&text) {
            None => Some(2),
            Some(0) => Some(0),
            Some(_) => None,
        }
    };

    if env.stdout_tty {
        put(
            err,
            format!(
                "\n  {}\n\n",
                style.dim("─── migration prompt below ────────────────────────────────")
            )
            .as_bytes(),
        )?;
    }
    put(out, format!("{text}\n").as_bytes())?;
    if env.stdout_tty {
        put(
            err,
            format!(
                "\n  {}\n\n",
                style.dim("─── end of migration prompt ──────────────────────────────")
            )
            .as_bytes(),
        )?;
    }
    let notice = match status {
        Some(0) => format!(
            "  {}\n",
            style.green("Copied migration prompt to clipboard.")
        ),
        Some(2) => format!(
            "  {} Prompt was printed to stdout.\n",
            style.yellow("Clipboard tool not found.")
        ),
        Some(_) => String::new(),
        None => format!(
            "  {} Prompt was printed to stdout.\n",
            style.yellow("Could not copy to clipboard.")
        ),
    };
    put(err, notice.as_bytes())?;
    Ok(0)
}

/// `_migrate_project_version`.
fn project_version(root: &str) -> String {
    let pinned = names::CONFIG_CANDIDATES
        .iter()
        .map(|rel| format!("{root}/{rel}"))
        .find(|path| Path::new(path).is_file())
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| names::pinned_version(&String::from_utf8_lossy(&bytes)))
        .unwrap_or_default();
    if pinned.is_empty() {
        "not detected".to_string()
    } else {
        pinned
    }
}

/// `_migrate_copy_prompt`'s tool search and pipe: `None` when no clipboard
/// tool is on `PATH`, else its exit status.
pub fn copy_to_clipboard(text: &str, path_var: Option<&str>) -> Option<i32> {
    let tools: [(&str, &[&str]); 5] = [
        ("pbcopy", &[]),
        ("wl-copy", &[]),
        ("xclip", &["-selection", "clipboard"]),
        ("xsel", &["--clipboard", "--input"]),
        ("clip.exe", &[]),
    ];
    let (program, args) = tools
        .iter()
        .find_map(|(name, args)| on_path(name, path_var).map(|path| (path, *args)))?;
    let Ok(mut child) = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return Some(126);
    };
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(text.as_bytes());
    }
    Some(child.wait().ok().and_then(|s| s.code()).unwrap_or(1))
}

/// `command -v <name>` over `PATH`.
fn on_path(name: &str, path_var: Option<&str>) -> Option<PathBuf> {
    path_var?
        .split(':')
        .filter(|dir| !dir.is_empty())
        .find_map(|dir| {
            let candidate = Path::new(dir).join(name);
            is_executable(&candidate).then_some(candidate)
        })
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

/// One `resource|tool|path|ext` line of `_migrate_scan_legacy`.
struct Legacy {
    resource: &'static str,
    tool: String,
    src: PathBuf,
    ext: String,
}

struct Run<'a, 'b> {
    project: &'a Project,
    root: String,
    style: &'a Style,
    env: &'a mut Env<'b>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Run<'_, '_> {
    fn rel(&self, path: &Path) -> String {
        let text = path.disk_text();
        text.strip_prefix(&format!("{}/", self.root))
            .unwrap_or(&text)
            .to_string()
    }

    fn dest(&self, entry: &Legacy) -> PathBuf {
        self.project
            .user_tools_dir()
            .join(&entry.tool)
            .join(format!("{}.{}", entry.resource, entry.ext))
    }

    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }
}

/// `_migrate_scan_legacy`.
fn scan_legacy(root: &Path) -> Vec<Legacy> {
    let mut found = Vec::new();
    for resource in ["hooks", "mcp", "settings"] {
        for file in sorted_entries(&root.join(".ai/src").join(resource)) {
            if !file.is_file() {
                continue;
            }
            let base = file.file_name().map(|n| n.disk_text()).unwrap_or_default();
            let Some(dot) = base.rfind('.') else {
                continue;
            };
            let (tool, ext) = (base[..dot].to_string(), base[dot + 1..].to_string());
            if tool.is_empty() || ext.is_empty() {
                continue;
            }
            found.push(Legacy {
                resource,
                tool,
                src: file,
                ext,
            });
        }
    }
    found
}

/// `_migrate_mcp_consolidation_candidate`.
fn consolidation_candidate(root: &Path) -> Option<PathBuf> {
    let dir = root.join(".ai/src/mcp");
    if !dir.is_dir() {
        return None;
    }
    let mut files = Vec::new();
    for file in sorted_entries(&dir) {
        if !file.is_file() {
            continue;
        }
        if file.extension().is_none_or(|ext| ext != "json") {
            return None;
        }
        files.push(file);
    }
    let first = files.first()?.clone();
    if root.join(".ai/src/mcp.json").is_file() {
        return None;
    }
    let bytes = std::fs::read(&first).ok()?;
    files[1..]
        .iter()
        .all(|other| std::fs::read(other).is_ok_and(|b| b == bytes))
        .then_some(first)
}

/// An engine-owned skill the project copies, found by name in any category.
struct BaseSkillCopy {
    name: String,
    rel: String,
    edited: bool,
}

impl BaseSkillCopy {
    /// Template manifest keys stay flat (`skills/<name>/…`) wherever the copy lives.
    fn manifest_keys(&self, skills: &Path) -> Vec<(String, PathBuf)> {
        let copy = skills.join(&self.rel);
        let mut files = Vec::new();
        files_below(&copy, &mut files);
        files
            .into_iter()
            .map(|file| {
                let inside = file
                    .strip_prefix(&copy)
                    .map(|p| p.disk_text())
                    .unwrap_or_default();
                (format!("skills/{}/{inside}", self.name), file)
            })
            .collect()
    }
}

/// `_migrate_scan_base_skills`: each engine-owned skill the project copies, and
/// whether every file still matches its recorded template hash.
fn scan_base_skills(root: &Path) -> Result<Vec<BaseSkillCopy>, Error> {
    let manifest = TemplateManifest::load(root)?;
    let skills = root.join(".ai/src/skills");
    let skills_text = skills.disk_text();
    let tree = skill_tree::discover(&Workspace::on_disk(&skills_text), &skills_text);
    let mut copies = Vec::new();
    let legacy = names::LEGACY_ENGINE_SKILLS
        .iter()
        .map(|name| name.to_string());
    for name in catalog::base_src_skills().into_iter().chain(legacy) {
        let Some(skill) = tree.find(&name) else {
            continue;
        };
        let mut copy = BaseSkillCopy {
            name,
            rel: skill.rel.clone(),
            edited: false,
        };
        copy.edited = copy.manifest_keys(&skills).iter().any(|(key, file)| {
            let current = template_manifest::hash(file).unwrap_or_default();
            manifest
                .lookup(key)
                .is_none_or(|recorded| recorded != current)
        });
        copies.push(copy);
    }
    Ok(copies)
}

/// `cp <src> <dst>` onto a missing destination: the source's mode under the umask.
fn copy_new(src: &Path, dst: &Path) -> Result<(), Error> {
    let bytes = std::fs::read(src).map_err(|e| Error::io(src, e))?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(src)
            .map_err(|e| Error::io(src, e))?
            .permissions()
            .mode();
        options.mode(mode & 0o7777);
    }
    options
        .open(dst)
        .and_then(|mut file| file.write_all(&bytes))
        .map_err(|e| Error::io(dst, e))
}

/// `--apply` and `--yes`; `Err(None)` asks for the help, `Err(Some(flag))`
/// names a flag `--legacy` does not take.
fn legacy_flags(args: &[String]) -> Result<(bool, bool), Option<String>> {
    let (mut apply, mut yes) = (false, false);
    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "--yes" | "-y" => yes = true,
            "--help" | "-h" => return Err(None),
            flag => return Err(Some(flag.to_string())),
        }
    }
    Ok((apply, yes))
}

/// The project config's format revision; r1 without a config.
fn project_format(project: &Project) -> Result<u32, Error> {
    let Some(path) = &project.config_path else {
        return Ok(1);
    };
    let bytes = std::fs::read(path).map_err(|e| Error::io(path, e))?;
    Ok(format_rev::project(&String::from_utf8_lossy(&bytes)))
}

/// `_cmd_migrate_legacy`.
fn legacy(
    args: &[String],
    discover: Discover,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (apply, yes) = match legacy_flags(args) {
        Ok(flags) => flags,
        Err(None) => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Err(Some(flag)) => {
            put(
                err,
                format!(
                    "{}: Unknown flag: {flag}\nUsage: exuno migrate --legacy [--apply] [--yes]\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
    };
    let project = discover()?;
    let root_path = project.root.clone();
    let mut run = Run {
        project: &project,
        root: root_path.disk_text(),
        style,
        env,
        out,
        err,
    };

    let legacy = scan_legacy(&root_path);
    let agent_dir = root_path.join(".agent");
    let has_agent_dir = agent_dir.is_dir();
    let skills = scan_base_skills(&root_path)?;
    let renames = pending_renames(&root_path, &skills);
    let engine_rev = format_rev::engine();
    let current_rev = project_format(&project)?;

    run.say(&format!(
        "\n{}\n{}\n\n",
        style.bold("  Exuno Migrate"),
        style.dim(&format!("  {}", run.root))
    ))?;
    if legacy.is_empty()
        && !has_agent_dir
        && skills.is_empty()
        && renames.is_empty()
        && current_rev >= engine_rev
    {
        run.nothing_to_migrate(current_rev)?;
        return Ok(0);
    }
    if apply && !legacy.is_empty() && !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, run.err);
    }
    if !skills.is_empty() {
        run.say(&format!("  {}:\n", style.bold("Engine-owned skills")))?;
        retire_base_skills(&mut run, apply, &skills)?;
        run.say("\n")?;
    }
    if current_rev < engine_rev {
        run.bump_format(current_rev, engine_rev, apply)?;
    }
    if !renames.is_empty() {
        rename_leftovers(&mut run, apply, &renames)?;
    }
    if legacy.is_empty() && !has_agent_dir {
        let hint = run.closing_hint(apply);
        run.say(&hint)?;
        return Ok(0);
    }
    if has_agent_dir {
        run.retire_agent_dir(&agent_dir, apply, yes)?;
        if legacy.is_empty() {
            return Ok(0);
        }
    }

    let moves = Moves::of(&legacy, &root_path);
    let plan = run.planned_moves(&moves);
    run.say(&plan)?;
    if !apply {
        run.rerun_hint(" to move files.")?;
        return Ok(0);
    }
    let tally = run.apply_moves(&moves, yes)?;
    for dir in ["hooks", "mcp", "settings"] {
        let _ = std::fs::remove_dir(root_path.join(".ai/src").join(dir));
    }
    run.say(&tally.summary(style))?;
    Ok(0)
}

/// The legacy entries to move: the MCP ones, which may consolidate into one
/// shared file, apart from the rest.
struct Moves<'a> {
    mcp: Vec<&'a Legacy>,
    other: Vec<&'a Legacy>,
    candidate: Option<PathBuf>,
}

impl<'a> Moves<'a> {
    fn of(legacy: &'a [Legacy], root: &Path) -> Self {
        let (mcp, other) = legacy.iter().partition(|entry| entry.resource == "mcp");
        Self {
            mcp,
            other,
            candidate: consolidation_candidate(root),
        }
    }
}

#[derive(Default)]
struct MoveTally {
    applied: usize,
    skipped: usize,
    consolidated: bool,
}

impl MoveTally {
    fn count(&mut self, moved: bool) {
        if moved {
            self.applied += 1;
        } else {
            self.skipped += 1;
        }
    }

    fn summary(&self, style: &Style) -> String {
        let mut summary = format!(
            "\n{}\n{}\n",
            style.green("  Migration complete."),
            style.dim(&format!("    moved:        {}", self.applied))
        );
        if self.skipped > 0 {
            let skipped = format!(
                "    skipped:      {} (target already existed)",
                self.skipped
            );
            summary.push_str(&format!("{}\n", style.yellow(&skipped)));
        }
        if self.consolidated {
            summary.push_str(&format!(
                "{}\n",
                style.dim("    consolidated: .ai/src/mcp.json")
            ));
        }
        summary.push_str(&format!(
            "\n{} {}{}\n\n",
            style.dim("  Run"),
            style.cyan("exuno sync"),
            style.dim(" to confirm outputs are unchanged.")
        ));
        summary
    }
}

impl Run<'_, '_> {
    fn nothing_to_migrate(&mut self, current_rev: u32) -> Result<(), Error> {
        let style = self.style;
        self.say(&format!(
            "{}\n{}\n\n",
            style.green("  Nothing to migrate."),
            style.dim(&format!(
                "  Canonical layout, no engine-owned skill copies, format r{current_rev} is current."
            ))
        ))
    }

    fn bump_format(&mut self, current: u32, engine: u32, apply: bool) -> Result<(), Error> {
        let style = self.style;
        self.say(&format!(
            "  {} {}:\n",
            style.bold("Project format"),
            style.dim(&format!("r{current} → r{engine}"))
        ))?;
        if let Some(path) = &self.project.config_path {
            let shown = self.rel(path);
            let place = style.dim(&format!("in {shown}"));
            if apply {
                yaml_edit::set_scalar(path, "format", &engine.to_string())?;
                self.say(&format!(
                    "{}           format: {engine} {place}\n",
                    style.green("  set")
                ))?;
            } else {
                self.say(&format!(
                    "{}     format: {engine} {place}\n",
                    style.cyan("  would set")
                ))?;
            }
        }
        self.say("\n")
    }

    /// The dry-run line that names `migrate --apply` and what it would do.
    fn rerun_hint(&mut self, what: &str) -> Result<(), Error> {
        let style = self.style;
        self.say(&format!(
            "{} {}{}\n\n",
            style.dim("  Dry-run. Re-run with"),
            style.cyan("exuno migrate --apply"),
            style.dim(what)
        ))
    }

    fn closing_hint(&self, apply: bool) -> String {
        let style = self.style;
        if apply {
            return format!("{}\n\n", style.green("  Migration complete."));
        }
        format!(
            "{} {}{}\n\n",
            style.dim("  Dry-run — re-run with"),
            style.cyan("exuno migrate --apply"),
            style.dim(" to apply.")
        )
    }

    /// Lists the pre-v0.6 `.agent/` and, under `--apply`, removes it once
    /// `--yes` or the terminal agrees.
    fn retire_agent_dir(&mut self, agent_dir: &Path, apply: bool, yes: bool) -> Result<(), Error> {
        let style = self.style;
        let mut listing = format!(
            "  {}:\n    {} — orphan directory from before tool-specific outputs.\n",
            style.bold("Legacy pre-v0.6 layout"),
            style.yellow(".agent/")
        );
        for item in sorted_entries(agent_dir) {
            if item.exists() {
                let name = item.file_name().unwrap_or_default().disk_text();
                listing.push_str(&format!("      · {name}\n"));
            }
        }
        listing.push('\n');
        self.say(&listing)?;
        if !apply {
            return self.rerun_hint(" to remove .agent/.");
        }
        let remove = if yes {
            true
        } else if self.env.interactive {
            (self.env.confirm)("Remove .agent/ (review the listing above first)?", false)
        } else {
            let note = "(non-interactive; .agent/ left in place — re-run with --yes to remove)";
            self.say(&format!("  {}\n\n", style.dim(note)))?;
            false
        };
        if remove && agent_dir.is_dir() {
            std::fs::remove_dir_all(agent_dir).map_err(|e| Error::io(agent_dir, e))?;
            self.say(&format!(
                "{}\n\n",
                style.green("  removed .agent/ (pre-v0.6 layout)")
            ))?;
        }
        Ok(())
    }

    fn planned_moves(&self, moves: &Moves) -> String {
        let style = self.style;
        let move_line = |entry: &Legacy| {
            format!(
                "  {}  →  {}\n",
                self.rel(&entry.src),
                self.rel(&self.dest(entry))
            )
        };
        let mut plan = format!("  {}:\n", style.bold("Planned moves"));
        for entry in &moves.other {
            plan.push_str(&move_line(entry));
        }
        match &moves.candidate {
            Some(first) => plan.push_str(&format!(
                "\n  {}:\n    All {} .ai/src/mcp/*.json are byte-identical — can consolidate into .ai/src/mcp.json.\n    {} {}\n",
                style.bold("MCP consolidation"),
                moves.mcp.len(),
                style.dim("Source file:"),
                self.rel(first)
            )),
            None => {
                for entry in &moves.mcp {
                    plan.push_str(&move_line(entry));
                }
            }
        }
        plan.push('\n');
        plan
    }

    /// Consolidates the MCP files when `--yes` or the terminal agrees, else
    /// moves them one by one, then moves the rest.
    fn apply_moves(&mut self, moves: &Moves, yes: bool) -> Result<MoveTally, Error> {
        let mut tally = MoveTally::default();
        let consolidate = match &moves.candidate {
            None => false,
            Some(_) if yes || !self.env.interactive => true,
            Some(_) => {
                let question = format!(
                    "Consolidate {} identical MCP files into .ai/src/mcp.json?",
                    moves.mcp.len()
                );
                (self.env.confirm)(&question, true)
            }
        };
        if let (true, Some(first)) = (consolidate, &moves.candidate) {
            self.consolidate_mcp(first, &moves.mcp)?;
            tally.applied += moves.mcp.len();
            tally.consolidated = true;
        } else {
            for entry in &moves.mcp {
                tally.count(move_one(self, entry)?);
            }
        }
        for entry in &moves.other {
            tally.count(move_one(self, entry)?);
        }
        Ok(tally)
    }

    fn consolidate_mcp(&mut self, first: &Path, mcp: &[&Legacy]) -> Result<(), Error> {
        copy_new(first, &self.project.root.join(".ai/src/mcp.json"))?;
        for entry in mcp {
            match std::fs::remove_file(&entry.src) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                    return Err(Error::io(&entry.src, e));
                }
                _ => {}
            }
            let text = format!(
                "{} {} → .ai/src/mcp.json\n",
                self.style.green("  consolidated"),
                self.rel(&entry.src)
            );
            self.say(&text)?;
        }
        Ok(())
    }
}

/// `_migrate_move_one`: whether the file moved; an existing target is skipped.
fn move_one(run: &mut Run, entry: &Legacy) -> Result<bool, Error> {
    let dest = run.dest(entry);
    if dest.is_file() {
        let text = format!(
            "{} {}\n",
            run.style.yellow("  skipped (target already exists)"),
            run.rel(&dest)
        );
        run.say(&text)?;
        return Ok(false);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::rename(&entry.src, &dest).map_err(|e| Error::io(&entry.src, e))?;
    let text = format!(
        "{} {} → {}\n",
        run.style.green("  moved"),
        run.rel(&entry.src),
        run.rel(&dest)
    );
    run.say(&text)?;
    Ok(true)
}

/// `_migrate_retire_base_skills`.
fn retire_base_skills(run: &mut Run, apply: bool, copies: &[BaseSkillCopy]) -> Result<(), Error> {
    let style = run.style;
    let root = PathBuf::from(&run.root);
    let skills = root.join(".ai/src/skills");
    let mut manifest = TemplateManifest::load(&root)?;
    let mut removed = 0;
    for copy in copies {
        let rel = &copy.rel;
        if copy.edited {
            run.say(&format!(
                "{}          .ai/src/skills/{rel}/ {}\n",
                style.yellow("  keep"),
                style.dim("(edited — stays your override; delete it to follow the engine)")
            ))?;
            continue;
        }
        if !apply {
            run.say(&format!(
                "{}  .ai/src/skills/{rel}/ {}\n",
                style.cyan("  would remove"),
                style.dim("(unedited — the engine supplies it)")
            ))?;
            continue;
        }
        for (key, _) in copy.manifest_keys(&skills) {
            manifest.remove(&key);
        }
        let dir = skills.join(rel);
        std::fs::remove_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        remove_empty_categories(&dir, &skills)?;
        run.say(&format!(
            "{}       .ai/src/skills/{rel}/ {}\n",
            style.green("  removed"),
            style.dim("(the engine supplies it now)")
        ))?;
        removed += 1;
    }
    if apply && removed > 0 {
        manifest.write(&root)?;
    }
    Ok(())
}

/// The leftovers r3 renames, without a skill copy the r2 step retires anyway.
fn pending_renames(root: &Path, skills: &[BaseSkillCopy]) -> Vec<Leftover> {
    let retired: Vec<PathBuf> = skills
        .iter()
        .filter(|copy| !copy.edited)
        .map(|copy| root.join(".ai/src/skills").join(&copy.rel))
        .collect();
    leftovers::scan(root)
        .into_iter()
        .filter(|leftover| !(leftover.kind == Kind::SkillDir && retired.contains(&leftover.path)))
        .collect()
}

fn rename_leftovers(run: &mut Run, apply: bool, renames: &[Leftover]) -> Result<(), Error> {
    let style = run.style;
    let root = PathBuf::from(&run.root);
    run.say(&format!("  {}:\n", style.bold("Renamed to Exuno")))?;
    for leftover in renames {
        let line = leftover.describe(&root);
        if leftover.blocked_by.is_some() || leftover.kind == Kind::HookBlock {
            run.say(&format!("{}          {line}\n", style.yellow("  keep")))?;
        } else if apply {
            leftovers::apply(&root, leftover)?;
            run.say(&format!("{}       {line}\n", style.green("  renamed")))?;
        } else {
            run.say(&format!("{}  {line}\n", style.cyan("  would rename")))?;
        }
    }
    run.say("\n")
}

/// Category directories above a removed skill that it left empty, up to `skills`.
fn remove_empty_categories(removed: &Path, skills: &Path) -> Result<(), Error> {
    for dir in removed.ancestors().skip(1).take_while(|dir| *dir != skills) {
        let mut entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
        if entries.next().is_some() {
            break;
        }
        std::fs::remove_dir(dir).map_err(|e| Error::io(dir, e))?;
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        for (rel, text) in files {
            let path = Path::new(&root).join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    struct Outcome {
        status: u8,
        out: String,
        err: String,
        asked: Vec<String>,
    }

    fn call(
        root: &str,
        args: &[&str],
        interactive: bool,
        answer: bool,
        copied: Option<i32>,
    ) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let mut asked = Vec::new();
        let mut confirm = |question: &str, _default: bool| {
            asked.push(question.to_string());
            answer
        };
        let mut copy = |_: &str| copied;
        let mut env = Env {
            version: "9.9.9",
            prompt_root: root.to_string(),
            no_clipboard: false,
            stdout_tty: false,
            interactive,
            confirm: &mut confirm,
            copy: &mut copy,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = migrate(
            &args,
            &discover,
            &Style::plain(),
            &mut env,
            &mut out,
            &mut err,
        )
        .unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
            asked,
        }
    }

    fn tree(root: &str) -> Vec<String> {
        let mut files = Vec::new();
        files_below(Path::new(root), &mut files);
        let mut rels: Vec<String> = files
            .iter()
            .map(|f| f.strip_prefix(root).unwrap().disk_text())
            .collect();
        rels.sort();
        rels
    }

    #[test]
    fn the_prompt_reads_an_exuno_version_pin() {
        let (_dir, root) = project(&[(".ai/exuno.yaml", "exuno_version: \"0.8.0\"\n")]);
        let copied = call(&root, &[], false, false, Some(0));
        assert!(
            copied
                .out
                .contains("- Project-pinned Exuno version: 0.8.0\n")
        );
    }

    #[test]
    fn the_prompt_names_both_versions_and_reports_the_clipboard_like_bash() {
        let (_dir, root) = project(&[(".ai/agent_sync.yaml", "agentsync_version: \"0.7.0\"\n")]);
        let copied = call(&root, &[], false, false, Some(0));
        assert_eq!(copied.status, 0);
        assert!(copied.out.starts_with(
            "## Exuno migration context\n\n- Exuno CLI that generated this prompt: 9.9.9\n- Project-pinned Exuno version: 0.7.0\n\n---\n\nI need you to safely migrate"
        ));
        assert!(copied.out.ends_with(&format!(
            "{}\n",
            catalog::MIGRATE_PROMPT.trim_end_matches('\n')
        )));
        assert_eq!(copied.err, "  Copied migration prompt to clipboard.\n");
        assert_eq!(
            call(&root, &[], false, false, None).err,
            "  Clipboard tool not found. Prompt was printed to stdout.\n"
        );
        assert_eq!(
            call(&root, &[], false, false, Some(7)).err,
            "  Could not copy to clipboard. Prompt was printed to stdout.\n"
        );
        std::fs::remove_file(Path::new(&root).join(".ai/agent_sync.yaml")).unwrap();
        assert!(
            call(&root, &[], false, false, Some(0))
                .out
                .contains("- Project-pinned Exuno version: not detected\n")
        );
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project(&[(".ai/exuno.yaml", "format: 3\n")]);
        let bogus = call(&root, &["--bogus", "extra"], false, false, None);
        assert_eq!(
            (bogus.status, bogus.out.as_str(), bogus.err.as_str()),
            (
                1,
                "",
                "Error: Unknown flag: --bogus\nUsage: exuno migrate [--legacy [--apply] [--yes]]\n"
            )
        );
        let legacy = call(&root, &["--legacy", "--bogus"], false, false, None);
        assert_eq!(
            (legacy.status, legacy.err.as_str()),
            (
                1,
                "Error: Unknown flag: --bogus\nUsage: exuno migrate --legacy [--apply] [--yes]\n"
            )
        );
        let help = call(&root, &["-h"], false, false, None);
        assert_eq!((help.status, help.err.as_str()), (0, ""));
        assert_eq!(
            help.out,
            "\n  exuno migrate — upgrade a project to the current format\n\n  USAGE\n    exuno migrate\n    exuno migrate --legacy [--apply] [--yes]\n\n  DESCRIPTION\n    Prints an AI prompt for safely upgrading an existing Exuno project to\n    the latest documented format and copies it to the system clipboard.\n\n    With --legacy, moves legacy flat-layout overrides to the canonical\n    per-tool layout. When every legacy MCP file is byte-identical, migrate\n    offers to consolidate them into the shared .ai/src/mcp.json. Dry-run by\n    default: re-run with --apply to move files.\n\n  LEGACY MOVES\n    .ai/src/hooks/<tool>.<ext>      → .ai/src/tools/<tool>/hooks.<ext>\n    .ai/src/mcp/<tool>.<ext>        → .ai/src/tools/<tool>/mcp.<ext>\n    .ai/src/settings/<tool>.<ext>   → .ai/src/tools/<tool>/settings.<ext>\n\n  OPTIONS\n    --legacy     Preview old flat-layout file moves without changing files\n    --apply      Apply those moves (backwards-compatible historical behavior)\n    -y, --yes    Accept safe legacy consolidation without prompting\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno migrate\n    exuno migrate --legacy\n    exuno migrate --legacy --apply --yes\n\n"
        );
        assert_eq!(
            call(&root, &["--legacy", "--help"], false, false, None).out,
            help.out
        );
        assert_eq!(
            call(&root, &["--apply"], false, false, None).out,
            format!(
                "\n  Exuno Migrate\n  {root}\n\n  Nothing to migrate.\n  Canonical layout, no engine-owned skill copies, format r3 is current.\n\n"
            )
        );
    }

    const LEGACY: [(&str, &str); 9] = [
        (
            ".ai/exuno.yaml",
            "format: 3\ntools:\n  enabled:\n    - claude\n",
        ),
        (".ai/src/hooks/cursor.json", "{\"hooks\": {}}\n"),
        (".ai/src/tools/cursor/settings.json", "{\"taken\": true}\n"),
        (".ai/src/settings/cursor.json", "{\"s\": 1}\n"),
        (".ai/src/settings/claude.json", "{\"s\": 2}\n"),
        (".ai/src/settings/README", "noext\n"),
        (".ai/src/mcp/claude.json", "{\"mcpServers\": {}}\n"),
        (".ai/src/mcp/cursor.json", "{\"mcpServers\": {}}\n"),
        (".agent/AGENTS.md", "# old\n"),
    ];

    #[test]
    fn legacy_files_are_planned_moved_consolidated_and_skipped_like_bash() {
        let (_dir, root) = project(&LEGACY);
        let header = format!(
            "\n  Exuno Migrate\n  {root}\n\n  Legacy pre-v0.6 layout:\n    .agent/ — orphan directory from before tool-specific outputs.\n      · AGENTS.md\n\n"
        );
        let plan = "  Planned moves:\n  .ai/src/hooks/cursor.json  →  .ai/src/tools/cursor/hooks.json\n  .ai/src/settings/claude.json  →  .ai/src/tools/claude/settings.json\n  .ai/src/settings/cursor.json  →  .ai/src/tools/cursor/settings.json\n\n  MCP consolidation:\n    All 2 .ai/src/mcp/*.json are byte-identical — can consolidate into .ai/src/mcp.json.\n    Source file: .ai/src/mcp/claude.json\n\n";

        let dry = call(&root, &["--legacy"], false, false, None);
        assert_eq!(
            dry.out,
            format!(
                "{header}  Dry-run. Re-run with exuno migrate --apply to remove .agent/.\n\n{plan}  Dry-run. Re-run with exuno migrate --apply to move files.\n\n"
            )
        );
        assert_eq!(tree(&root).len(), LEGACY.len());

        let applied = call(&root, &["--apply", "--yes"], false, false, None);
        assert_eq!(
            applied.out,
            format!(
                "{header}  removed .agent/ (pre-v0.6 layout)\n\n{plan}  consolidated .ai/src/mcp/claude.json → .ai/src/mcp.json\n  consolidated .ai/src/mcp/cursor.json → .ai/src/mcp.json\n  moved .ai/src/hooks/cursor.json → .ai/src/tools/cursor/hooks.json\n  moved .ai/src/settings/claude.json → .ai/src/tools/claude/settings.json\n  skipped (target already exists) .ai/src/tools/cursor/settings.json\n\n  Migration complete.\n    moved:        4\n    skipped:      1 (target already existed)\n    consolidated: .ai/src/mcp.json\n\n  Run exuno sync to confirm outputs are unchanged.\n\n"
            )
        );
        assert_eq!(
            tree(&root),
            [
                ".ai/exuno.yaml",
                ".ai/src/mcp.json",
                ".ai/src/settings/README",
                ".ai/src/settings/cursor.json",
                ".ai/src/tools/claude/settings.json",
                ".ai/src/tools/cursor/hooks.json",
                ".ai/src/tools/cursor/settings.json",
            ]
        );
    }

    #[test]
    fn prompts_decide_the_agent_dir_and_the_consolidation_off_the_flags() {
        let (_dir, root) = project(&LEGACY);
        let quiet = call(&root, &["--apply"], false, true, None);
        assert!(quiet.asked.is_empty());
        assert!(quiet.out.contains(
            "  (non-interactive; .agent/ left in place — re-run with --yes to remove)\n\n"
        ));
        assert!(Path::new(&root).join(".agent/AGENTS.md").is_file());
        assert!(Path::new(&root).join(".ai/src/mcp.json").is_file());

        let (_dir, root) = project(&LEGACY);
        let declined = call(&root, &["--apply"], true, false, None);
        assert_eq!(
            declined.asked,
            [
                "Remove .agent/ (review the listing above first)?",
                "Consolidate 2 identical MCP files into .ai/src/mcp.json?"
            ]
        );
        assert!(Path::new(&root).join(".agent/AGENTS.md").is_file());
        assert!(
            declined
                .out
                .contains("  moved .ai/src/mcp/claude.json → .ai/src/tools/claude/mcp.json\n")
        );
        assert!(!Path::new(&root).join(".ai/src/mcp.json").exists());
    }

    #[test]
    fn a_json_mcp_set_with_another_config_moves_per_tool_and_an_outside_catalog_is_refused() {
        let (_dir, root) = project(&[
            (".ai/agent_sync.yaml", "format: 2\n"),
            (".ai/src/mcp/claude.json", "{}\n"),
            (".ai/src/mcp/cursor.json", "{}\n"),
            (".ai/src/mcp/codex.toml", "[x]\n"),
        ]);
        let run = call(&root, &["--apply", "--yes"], false, false, None);
        assert!(
            run.out
                .contains("  moved .ai/src/mcp/codex.toml → .ai/src/tools/codex/mcp.toml\n")
        );
        assert!(
            Path::new(&root)
                .join(".ai/src/tools/codex/mcp.toml")
                .is_file()
        );

        let (_dir, root) = project(&[
            (
                ".ai/agent_sync.yaml",
                "tools:\n  enabled: []\nsource:\n  tools: \"../elsewhere\"\n",
            ),
            (".ai/src/hooks/claude.json", "{}\n"),
        ]);
        let refused = call(&root, &["--legacy", "--apply"], false, false, None);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.out, format!("\n  Exuno Migrate\n  {root}\n\n"));
        assert_eq!(
            refused.err,
            format!(
                "Error: source.tools resolves outside the project: {root}/../elsewhere\nExuno only reads that catalog; edit its tool overrides where they live.\n"
            )
        );
        assert!(Path::new(&root).join(".ai/src/hooks/claude.json").is_file());
    }

    #[test]
    fn legacy_agentsync_skill_copies_are_retired_unless_edited_and_the_format_is_recorded() {
        let skill = |rel: &str| {
            catalog::engine_files()
                .into_iter()
                .find(|(path, _)| path == &format!("lib/templates/base-src/skills/exuno/{rel}"))
                .map(|(_, bytes)| String::from_utf8(bytes.to_vec()).unwrap())
                .unwrap()
        };
        let files = [
            "SKILL.md",
            "references/maintenance.md",
            "references/writing-skills.md",
        ];
        let copies: Vec<(String, String)> = files
            .iter()
            .map(|rel| (format!(".ai/src/skills/agentsync/{rel}"), skill(rel)))
            .collect();
        let manifest: String = files
            .iter()
            .map(|rel| {
                format!(
                    "skills/agentsync/{rel}\t{}\n",
                    crate::transaction::manifest::sha256_hex(skill(rel).as_bytes())
                )
            })
            .chain(["rules/core.md\tabc\n".to_string()])
            .collect();
        let mut fixture: Vec<(&str, &str)> = copies
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        fixture.push((".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n"));
        fixture.push((".ai/.template-manifest", &manifest));

        let (_dir, root) = project(&fixture);
        let dry = call(&root, &["--legacy"], false, false, None);
        assert_eq!(
            dry.out,
            format!(
                "\n  Exuno Migrate\n  {root}\n\n  Engine-owned skills:\n  would remove  .ai/src/skills/agentsync/ (unedited — the engine supplies it)\n\n  Project format r1 → r3:\n  would set     format: 3 in .ai/agent_sync.yaml\n\n  Renamed to Exuno:\n  would rename  .ai/agent_sync.yaml → .ai/exuno.yaml\n\n  Dry-run — re-run with exuno migrate --apply to apply.\n\n"
            )
        );
        let applied = call(&root, &["--apply"], false, false, None);
        assert!(
            applied.out.contains(
                "  removed       .ai/src/skills/agentsync/ (the engine supplies it now)\n"
            )
        );
        assert!(applied.out.ends_with(
            "  set           format: 3 in .ai/agent_sync.yaml\n\n  Renamed to Exuno:\n  renamed       .ai/agent_sync.yaml → .ai/exuno.yaml\n\n  Migration complete.\n\n"
        ));
        assert_eq!(tree(&root), [".ai/.template-manifest", ".ai/exuno.yaml"]);
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/.template-manifest")).unwrap(),
            "rules/core.md\tabc\n"
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/exuno.yaml")).unwrap(),
            "tools:\n  enabled:\n    - claude\nformat: 3\n"
        );

        let (_dir, root) = project(&fixture);
        std::fs::write(
            Path::new(&root).join(".ai/src/skills/agentsync/SKILL.md"),
            "edited\n",
        )
        .unwrap();
        let kept = call(&root, &["--apply"], false, false, None);
        assert!(kept.out.contains("  keep          .ai/src/skills/agentsync/ (edited — stays your override; delete it to follow the engine)\n"));
        assert!(
            kept.out
                .contains("  renamed       .ai/src/skills/agentsync/ → .ai/src/skills/exuno/\n")
        );
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/skills/exuno/SKILL.md"))
                .unwrap(),
            "edited\n"
        );
    }

    #[test]
    fn an_engine_owned_skill_copy_moved_into_a_category_is_still_retired_or_kept() {
        let files: Vec<(String, String)> = catalog::engine_files()
            .into_iter()
            .filter_map(|(path, bytes)| {
                let rel = path.strip_prefix("lib/templates/base-src/skills/exuno/")?;
                Some((rel.to_string(), String::from_utf8(bytes.to_vec()).unwrap()))
            })
            .collect();
        let copies: Vec<(String, &str)> = files
            .iter()
            .map(|(rel, text)| {
                (
                    format!(".ai/src/skills/meta/sub/exuno/{rel}"),
                    text.as_str(),
                )
            })
            .collect();
        let manifest: String = files
            .iter()
            .map(|(rel, text)| {
                format!(
                    "skills/exuno/{rel}\t{}\n",
                    crate::transaction::manifest::sha256_hex(text.as_bytes())
                )
            })
            .chain(["rules/core.md\tabc\n".to_string()])
            .collect();
        let mut fixture: Vec<(&str, &str)> = copies.iter().map(|(p, t)| (p.as_str(), *t)).collect();
        fixture.push((
            ".ai/src/skills/meta/other/SKILL.md",
            "---\nname: other\n---\n",
        ));
        fixture.push((".ai/exuno.yaml", "format: 3\n"));
        fixture.push((".ai/.template-manifest", &manifest));

        let (_dir, root) = project(&fixture);
        let dry = call(&root, &["--legacy"], false, false, None);
        assert!(dry.out.contains(
            "  would remove  .ai/src/skills/meta/sub/exuno/ (unedited — the engine supplies it)\n"
        ));
        let applied = call(&root, &["--apply"], false, false, None);
        assert!(applied.out.contains(
            "  removed       .ai/src/skills/meta/sub/exuno/ (the engine supplies it now)\n"
        ));
        assert_eq!(
            tree(&root),
            [
                ".ai/.template-manifest",
                ".ai/exuno.yaml",
                ".ai/src/skills/meta/other/SKILL.md"
            ]
        );
        assert!(!Path::new(&root).join(".ai/src/skills/meta/sub").exists());
        assert_eq!(
            std::fs::read_to_string(Path::new(&root).join(".ai/.template-manifest")).unwrap(),
            "rules/core.md\tabc\n"
        );

        let (_dir, root) = project(&fixture);
        std::fs::write(
            Path::new(&root).join(".ai/src/skills/meta/sub/exuno/SKILL.md"),
            "edited\n",
        )
        .unwrap();
        let kept = call(&root, &["--apply"], false, false, None);
        assert!(kept.out.contains("  keep          .ai/src/skills/meta/sub/exuno/ (edited — stays your override; delete it to follow the engine)\n"));
    }
}
