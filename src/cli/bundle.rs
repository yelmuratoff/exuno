//! `exuno export` and `exuno import`: `cmd_export` of
//! `lib/helpers/export.sh` and `cmd_import` of `lib/helpers/import.sh`, which
//! bundle a project's sources into a `tar.gz` and bring a bundle, a directory,
//! or a git remote back in. Tar archives go through the `tar` executable, as
//! Bash ran it; ZIP archives and `.skill` packages through [`crate::zip`],
//! which `export --skill` also writes; remotes through [`crate::remote`].

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::update::tar_extract;
use super::{files_below, put};
use crate::Error;
use crate::config::{command_surfaces, names, skill_metadata, yaml_subset};
use crate::engine::{skill_tree, workspace::Workspace};
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::remote;
use crate::transaction::interrupt::{self, Interrupt};
use crate::transaction::{backup, witness};

/// `_BUNDLE_DIR_TARGETS`, in the order `init` creates them.
const DIR_TARGETS: [&str; 8] = [
    "rules", "skills", "commands", "agents", "settings", "mcp", "hooks", "tools",
];
const CONFIG: &str = names::CONFIG;
const CONFIG_LEGACY: &str = "agent_sync.yaml";
const ZIP_SUFFIXES: [&str; 2] = [".zip", ".skill"];
const TAR_SUFFIXES: [&str; 5] = [".tar", ".tar.gz", ".tgz", ".tar.xz", ".tar.bz2"];

fn present_config(dir: &Path) -> Option<&'static str> {
    names::CONFIG_CANDIDATES
        .into_iter()
        .find(|rel| dir.join(rel).is_file())
}

/// The `.ai/` config `dir` already has, else [`CONFIG`].
fn ai_config(dir: &Path) -> &'static str {
    names::CONFIG_CANDIDATES
        .into_iter()
        .filter(|rel| rel.starts_with(".ai/"))
        .find(|rel| dir.join(rel).is_file())
        .unwrap_or(CONFIG)
}

/// What `import` takes from the process.
pub struct Env<'a> {
    /// The logical working directory a relative source is shown against.
    pub cwd: String,
    /// `[[ -t 0 ]]`: the confirmation is asked only when stdin is a terminal.
    pub interactive: bool,
    /// `read -r answer` on stdin.
    pub read_line: &'a mut dyn FnMut() -> String,
}

/// `_resolve_source_paths`: the detected base and every source path, each
/// relative to the project root or as the config spelled it.
#[derive(Debug, Default, PartialEq)]
struct Sources {
    base: String,
    agents: String,
    dirs: Vec<(&'static str, String)>,
}

fn resolve_sources(root: &str) -> Sources {
    let config = format!(
        "{root}/{}",
        present_config(Path::new(root)).unwrap_or(CONFIG_LEGACY)
    );
    let base = if Path::new(root).join(".ai/src").is_dir() {
        ".ai/src"
    } else if Path::new(root).join(".ai").is_dir() {
        ".ai"
    } else {
        ""
    };
    let mut sources = Sources {
        base: base.to_string(),
        ..Sources::default()
    };
    if !base.is_empty() && Path::new(root).join(base).join("AGENTS.md").is_file() {
        sources.agents = format!("{base}/AGENTS.md");
    }
    sources.dirs = DIR_TARGETS
        .iter()
        .map(|name| {
            let path = if !base.is_empty() && Path::new(root).join(base).join(name).is_dir() {
                format!("{base}/{name}")
            } else {
                String::new()
            };
            (*name, path)
        })
        .collect();
    if let Ok(bytes) = std::fs::read(&config) {
        let text = String::from_utf8_lossy(&bytes);
        let override_of = |key: &str| yaml_subset::value(&text, key);
        let agents = override_of("source.agents");
        if !agents.is_empty() {
            sources.agents = agents;
        }
        for (name, key) in [
            ("rules", "source.rules"),
            ("skills", "source.skills"),
            ("commands", "source.commands"),
            ("agents", "source.subagents"),
            ("tools", "source.tools"),
        ] {
            let value = override_of(key);
            if !value.is_empty()
                && let Some(entry) = sources.dirs.iter_mut().find(|(n, _)| *n == name)
            {
                entry.1 = value;
            }
        }
    }
    sources
}

fn count_files(dir: &Path) -> usize {
    let mut files = Vec::new();
    files_below(dir, &mut files);
    files.len()
}

pub const EXPORT_HELP: Help = Help {
    command: "export",
    tagline: "bundle source files into a shareable archive",
    synopsis: &["export [OPTIONS]"],
    description: &[],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            (
                "-o, --output <path>",
                "Output file; .zip writes a ZIP (default: ./exuno-bundle.tar.gz)",
            ),
            (
                "--skill <name>",
                "Package one skill as ./<name>.skill (Claude skill upload)",
            ),
            ("--dry-run", "Preview what would be exported"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "export",
        "export -o my-config.tar.gz",
        "export --skill my-skill",
        "export --dry-run",
    ],
};

/// `stat -f%z` rendered as `cmd_export` prints it.
fn human_size(size: Option<u64>) -> String {
    match size {
        Some(size) if size >= 1_048_576 => format!("{} MB", size / 1_048_576),
        Some(size) if size >= 1024 => format!("{} KB", size / 1024),
        Some(size) => format!("{size} B"),
        None => "? B".to_string(),
    }
}

/// `export`'s options; `output` and `skill` are empty when not given.
struct ExportArgs {
    output: String,
    dry_run: bool,
    skill: String,
}

fn parse_export(args: &[String]) -> Result<ExportArgs, BundleStop> {
    let mut parsed = ExportArgs {
        output: String::new(),
        dry_run: false,
        skill: String::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" | "-o" => {
                let Some(value) = args.next() else {
                    return Err(refuse("--output requires a path".into(), false));
                };
                parsed.output = value.clone();
            }
            "--skill" => {
                let Some(value) = args.next() else {
                    return Err(refuse("--skill requires a name".into(), false));
                };
                parsed.skill = value.clone();
            }
            "--dry-run" => parsed.dry_run = true,
            "--help" | "-h" => return Err(BundleStop::Help),
            other => return Err(refuse(format!("Unknown option: {other}"), true)),
        }
    }
    Ok(parsed)
}

/// The order a bundle lists the entries `init` creates; any other entry
/// follows in byte order.
const ENTRY_ORDER: [&str; 10] = [
    "AGENTS.md",
    "rules",
    "skills",
    "commands",
    "agents",
    "settings",
    "mcp",
    "mcp.json",
    "hooks",
    "tools",
];

/// The entries of a source base a bundle carries: everything but hidden
/// engine state, the backup store, and the config, which travels apart.
fn source_entries(base: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(base) else {
        return Vec::new();
    };
    let state = ["backups", "exuno.yaml", CONFIG_LEGACY];
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| !kind.is_symlink()))
        .map(|entry| entry.file_name().disk_text())
        .filter(|name| !name.starts_with('.') && !state.contains(&name.as_str()))
        .collect();
    let rank = |name: &String| {
        ENTRY_ORDER
            .iter()
            .position(|known| known == name)
            .unwrap_or(ENTRY_ORDER.len())
    };
    names.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| a.cmp(b)));
    names
}

/// `path`'s label in a contents list, `None` for an empty directory.
fn entry_label(root: &str, rel: &str, name: &str) -> Option<String> {
    let path = Path::new(root).join(rel);
    if path.is_file() {
        return Some(name.to_string());
    }
    let count = count_files(&path);
    (count > 0).then(|| format!("{name}/ ({count} files)"))
}

/// Each path the bundle archives, relative to `root`, with the label the
/// contents list shows for it: every entry of the source base, the
/// `source.*` paths declared outside it, and the config.
fn export_items(root: &str, sources: &Sources, style: &Style) -> Vec<(String, String)> {
    let mut items = Vec::new();
    for name in source_entries(&Path::new(root).join(&sources.base)) {
        let rel = format!("{}/{name}", sources.base);
        if let Some(label) = entry_label(root, &rel, &name) {
            items.push((rel, label));
        }
    }
    let declared = std::iter::once(("AGENTS.md", &sources.agents))
        .chain(sources.dirs.iter().map(|(name, path)| (*name, path)));
    for (name, path) in declared {
        let outside_base = !path.is_empty()
            && !path.starts_with(&format!("{}/", sources.base))
            && !crate::paths::is_absolute(path)
            && !path.split('/').any(|segment| segment == "..");
        if !outside_base || !Path::new(root).join(path).exists() {
            continue;
        }
        if let Some(label) = entry_label(root, path, name) {
            items.push((path.clone(), label));
        }
    }
    match present_config(Path::new(root)) {
        Some(CONFIG_LEGACY) => {
            let label = format!("{CONFIG_LEGACY} {}", style.dim("(legacy)"));
            items.push((CONFIG_LEGACY.to_string(), label));
        }
        Some(rel) => items.push((rel.to_string(), crate::paths::leaf(rel))),
        None => {}
    }
    items
}

/// `cmd_export`.
pub fn export(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let ExportArgs {
        mut output,
        dry_run,
        skill,
    } = match parse_export(args) {
        Ok(parsed) => parsed,
        Err(stop) => return stopped(stop, &EXPORT_HELP, style, out, err),
    };
    let sources = resolve_sources(root);
    if sources.base.is_empty() {
        put(
            err,
            format!(
                "{}: No .ai/ directory found in {root}\nRun {} first.\n",
                style.red("Error"),
                style.cyan("exuno init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if !skill.is_empty() {
        let request = SkillExport {
            name: &skill,
            output: &output,
            dry_run,
        };
        return export_skill(root, &sources, &request, style, out, err);
    }
    if output.is_empty() {
        output = format!("{root}/exuno-bundle.tar.gz");
    }
    put(
        out,
        format!("\n{}\n\n", style.bold("  Exuno Export")).as_bytes(),
    )?;

    let base = Path::new(root).join(&sources.base);
    std::fs::read_dir(&base).map_err(|e| Error::io(&base, e))?;
    let (items, labels): (Vec<String>, Vec<String>) =
        export_items(root, &sources, style).into_iter().unzip();
    if items.is_empty() {
        put(
            out,
            format!(
                "  {} — source directories are empty.\n\n",
                style.yellow("Nothing to export")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let mut text = format!("  {}\n", style.green("Contents:"));
    for label in &labels {
        text.push_str(&format!("    {} {label}\n", style.dim("•")));
    }
    text.push('\n');
    if dry_run {
        text.push_str(&format!(
            "  {} — no files written.\n  Would create: {}\n\n",
            style.yellow("Dry run"),
            style.cyan(&output)
        ));
        put(out, text.as_bytes())?;
        return Ok(0);
    }
    put(out, text.as_bytes())?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    if !write_bundle(root, &output, &items)? {
        put(
            err,
            format!("  {}: Failed to create archive.\n", style.red("Error")).as_bytes(),
        )?;
        return Ok(1);
    }
    put(out, exported_text(style, root, &output).as_bytes()).map(|()| 0)
}

/// Archives the files of `items` at `output`: a ZIP when its name says so,
/// else a `tar.gz` through `tar`, both from the same file list.
/// `false` when the archive could not be made.
fn write_bundle(root: &str, output: &str, items: &[String]) -> Result<bool, Error> {
    let files = bundle_files(root, items)?;
    if output.to_ascii_lowercase().ends_with(".zip") {
        return write_zip_bundle(root, output, &files);
    }
    let Ok(mut tar) = Command::new("tar")
        .args(["-czf", output, "-T", "-"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .spawn()
    else {
        return Ok(false);
    };
    let listed = tar
        .stdin
        .take()
        .is_some_and(|mut stdin| stdin.write_all(files.join("\n").as_bytes()).is_ok());
    Ok(tar.wait().is_ok_and(|status| status.success()) && listed)
}

/// Writes `files` (paths relative to `root`) as a ZIP at `output`, itself
/// relative to `root` unless absolute; `false` when the archive cannot hold
/// them.
fn write_zip_bundle(root: &str, output: &str, files: &[String]) -> Result<bool, Error> {
    let entries = files
        .iter()
        .map(|rel| file_entry(&Path::new(root).join(rel), rel.clone()))
        .collect::<Result<Vec<_>, Error>>()?;
    let Ok(archive) = crate::zip::write(&entries) else {
        return Ok(false);
    };
    let dest = if crate::paths::is_absolute(output) {
        PathBuf::from(output)
    } else {
        Path::new(root).join(output)
    };
    std::fs::write(&dest, archive).map_err(|e| Error::io(&dest, e))?;
    Ok(true)
}

struct SkillExport<'a> {
    name: &'a str,
    output: &'a str,
    dry_run: bool,
}

/// `export --skill <name>`: the project's skill as a `.skill` package, a ZIP
/// of `<name>/` as Claude's skill upload takes it. Refuses a skill whose
/// `SKILL.md` frontmatter the upload would reject.
fn export_skill(
    root: &str,
    sources: &Sources,
    request: &SkillExport,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let name = request.name;
    let mut refuse_with = |message: String| -> Result<u8, Error> {
        put(
            err,
            format!("{}: {message}\n", style.red("Error")).as_bytes(),
        )?;
        Ok(1)
    };
    let skills_rel = sources
        .dirs
        .iter()
        .find(|(target, _)| *target == "skills")
        .map_or("", |(_, path)| path.as_str());
    let skills_root = Path::new(root).join(skills_rel).disk_text();
    let found = (!skills_rel.is_empty())
        .then(|| skill_tree::discover(&Workspace::on_disk(root), &skills_root))
        .and_then(|tree| tree.find(name).cloned());
    let Some(skill) = found else {
        return refuse_with(format!("Skill not found: {name}"));
    };
    let dir = Path::new(&skills_root).join(&skill.rel);
    let manifest = dir.join("SKILL.md");
    let bytes = std::fs::read(&manifest).map_err(|e| Error::io(&manifest, e))?;
    if let Err(reason) = skill_metadata::read(&bytes, name) {
        return refuse_with(format!("{skills_rel}/{}/SKILL.md: {reason}", skill.rel));
    }
    let entries = entries_below(&dir, name)?;
    let output = if request.output.is_empty() {
        format!("{root}/{name}.skill")
    } else {
        request.output.to_string()
    };
    put(
        out,
        format!(
            "\n{}\n\n  {}\n    {} {name}/ ({} files)\n\n",
            style.bold("  Exuno Export"),
            style.green("Contents:"),
            style.dim("•"),
            entries.len()
        )
        .as_bytes(),
    )?;
    if request.dry_run {
        let text = format!(
            "  {} — no files written.\n  Would create: {}\n\n",
            style.yellow("Dry run"),
            style.cyan(&output)
        );
        return put(out, text.as_bytes()).map(|()| 0);
    }
    let archive = match crate::zip::write(&entries) {
        Ok(archive) => archive,
        Err(reason) => return refuse_with(format!("Failed to create archive: {reason}")),
    };
    let dest = if crate::paths::is_absolute(&output) {
        PathBuf::from(&output)
    } else {
        Path::new(root).join(&output)
    };
    std::fs::write(&dest, archive).map_err(|e| Error::io(&dest, e))?;
    put(out, exported_text(style, root, &output).as_bytes()).map(|()| 0)
}

/// The regular files below `dir`, each as `<prefix>/<path below dir>`: a
/// hidden entry or a symbolic link at any depth stays out of an archive,
/// and a folder that cannot be read fails it rather than leaving files out.
fn shared_files(dir: &Path, prefix: &str, found: &mut Vec<String>) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(dir, e))?;
        let name = entry.file_name().disk_text();
        if name.starts_with('.') {
            continue;
        }
        let kind = entry.file_type().map_err(|e| Error::io(entry.path(), e))?;
        let rel = format!("{prefix}/{name}");
        if kind.is_dir() {
            shared_files(&entry.path(), &rel, found)?;
        } else if kind.is_file() {
            found.push(rel);
        }
    }
    Ok(())
}

/// Every file `shared_files` keeps below `dir` as an archive entry under
/// `<prefix>/`, in byte order so the archive is reproducible.
fn entries_below(dir: &Path, prefix: &str) -> Result<Vec<crate::zip::Entry>, Error> {
    let mut files = Vec::new();
    shared_files(dir, prefix, &mut files)?;
    files.sort();
    files
        .iter()
        .map(|rel| {
            let inner = rel
                .strip_prefix(prefix)
                .unwrap_or(rel)
                .trim_start_matches('/');
            file_entry(&dir.join(inner), rel.clone())
        })
        .collect()
}

fn file_entry(file: &Path, path: String) -> Result<crate::zip::Entry, Error> {
    let meta = std::fs::metadata(file).map_err(|e| Error::io(file, e))?;
    Ok(crate::zip::Entry {
        path,
        data: std::fs::read(file).map_err(|e| Error::io(file, e))?,
        executable: is_executable(&meta),
    })
}

/// The files the bundle `items` (paths relative to `root`) carry, relative
/// to `root` and in byte order; an item that is a symbolic link carries
/// nothing.
fn bundle_files(root: &str, items: &[String]) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    for rel in items {
        let path = Path::new(root).join(rel);
        let meta = std::fs::symlink_metadata(&path).map_err(|e| Error::io(&path, e))?;
        if meta.is_dir() {
            shared_files(&path, rel, &mut files)?;
        } else if meta.is_file() {
            files.push(rel.clone());
        }
    }
    files.sort();
    Ok(files)
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &std::fs::Metadata) -> bool {
    false
}

fn exported_text(style: &Style, root: &str, output: &str) -> String {
    let archive = if crate::paths::is_absolute(output) {
        PathBuf::from(output)
    } else {
        Path::new(root).join(output)
    };
    let size = std::fs::metadata(&archive).ok().map(|m| m.len());
    let base_name = output.rsplit('/').next().unwrap_or(output);
    format!(
        "  {} → {} ({})\n\n  Share this file and import with:\n    {} {}\n\n",
        style.green("Exported!"),
        style.cyan(output),
        human_size(size),
        style.cyan("exuno import"),
        style.dim(base_name)
    )
}

pub const IMPORT_HELP: Help = Help {
    command: "import",
    tagline: "import config from a git remote, archive, or directory",
    synopsis: &["import <source> [OPTIONS]"],
    description: &[
        "An imported skill replaces the project's copy whole: files the new\nversion no longer has are removed. An import that changes files is\nbacked up first, so exuno rollback undoes it.",
    ],
    sections: &[
        Section {
            title: "SOURCES",
            entries: &[
                (
                    "Git repository",
                    "https://github.com/user/repo[/tree/<ref>/<folder>],\ngit@host:org/repo.git, or user/repo for GitHub",
                ),
                (
                    "Archive file",
                    "path/to/exuno-bundle.tar.gz (.tar, .tar.xz, .tar.bz2, .zip)",
                ),
                (
                    "Skill package",
                    "path/to/my-skill.skill, or a folder or archive of skills",
                ),
                ("Local directory", "path/to/project/"),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--ref <name>",
                    "Branch, tag, or commit to fetch (default: the remote's\ndefault branch); -b and --branch are aliases",
                ),
                (
                    "--path <folder>",
                    "Folder of the repository that holds its .ai/",
                ),
                (
                    "--only <targets>",
                    "Import only specific targets (comma-separated)\nTargets: rules,skills,commands,agents,settings,mcp,hooks,tools",
                ),
                (
                    "--config",
                    "Replace the project's exuno.yaml with the source's",
                ),
                (
                    "--force",
                    "Skip confirmations, config that runs commands included",
                ),
                ("--dry-run", "Preview changes without writing"),
                ("-h, --help", "Show this help"),
            ],
        },
    ],
    examples: &[
        "import https://github.com/user/repo",
        "import https://github.com/user/repo/tree/v2/packages/app",
        "import user/repo --ref develop",
        "import git@github.com:org/private.git",
        "import exuno-bundle.tar.gz",
        "import my-skill.skill",
        "import ../other-project/",
        "import https://github.com/user/repo --only rules,skills",
        "import bundle.tar.gz --dry-run",
    ],
};

/// A scratch directory under the system temp dir, removed on drop as the run
/// directory was.
pub(crate) struct Scratch(pub(crate) PathBuf);

impl Scratch {
    pub(crate) fn create(prefix: &str) -> Result<Self, Error> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("{prefix}.{}.{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| Error::io(&dir, e))?;
        Ok(Self(dir))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// `cp -R <src>/. <dest>/`: every entry below `src`, directories made as met.
fn copy_tree(src: &Path, dest: &Path) -> Result<(), Error> {
    std::fs::create_dir_all(dest).map_err(|e| Error::io(dest, e))?;
    let entries = std::fs::read_dir(src).map_err(|e| Error::io(src, e))?;
    for entry in entries.filter_map(|e| e.ok()) {
        let from = entry.path();
        let to = dest.join(entry.file_name());
        let meta = std::fs::symlink_metadata(&from).map_err(|e| Error::io(&from, e))?;
        if meta.is_dir() {
            copy_tree(&from, &to)?;
        } else if meta.is_file() {
            std::fs::copy(&from, &to).map_err(|e| Error::io(&from, e))?;
        }
    }
    Ok(())
}

/// The folder that holds `path` as its `.ai/`: a link straight to `.ai` or
/// `.ai/src` names the config, not a project to search.
fn ai_owner(path: &Path) -> PathBuf {
    let named = |path: &Path, name: &str| path.file_name().is_some_and(|leaf| leaf == name);
    let ai = if named(path, "src") && path.parent().is_some_and(|ai| named(ai, ".ai")) {
        path.parent()
    } else {
        Some(path)
    };
    match ai.filter(|ai| named(ai, ".ai")).and_then(Path::parent) {
        Some(owner) => owner.to_path_buf(),
        None => path.to_path_buf(),
    }
}

/// `_import_find_ai_src`: `.ai/src` over `.ai`, directly or one level down.
fn find_ai_src(search_root: &Path) -> Option<PathBuf> {
    let direct = |dir: &Path| -> Option<PathBuf> {
        let src = dir.join(".ai/src");
        if src.is_dir() {
            return Some(src);
        }
        let ai = dir.join(".ai");
        ai.is_dir().then_some(ai)
    };
    if let Some(found) = direct(search_root) {
        return Some(found);
    }
    let mut names: Vec<String> = std::fs::read_dir(search_root)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().disk_text())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names
        .into_iter()
        .find_map(|name| direct(&search_root.join(name)))
}

/// The skill directories of a tree without `.ai/`: the tree itself when
/// `SKILL.md` sits at its root, named `stem`, else every skill folder below
/// it, named by its folder.
fn skill_dirs(tree: &Path, stem: &str) -> Vec<(PathBuf, String)> {
    if tree.join("SKILL.md").is_file() {
        return vec![(tree.to_path_buf(), stem.to_string())];
    }
    let tree_text = tree.disk_text();
    skill_tree::discover(&Workspace::on_disk(&tree_text), &tree_text)
        .skills
        .into_iter()
        .map(|skill| (tree.join(&skill.rel), skill.name))
        .collect()
}

/// The skills of a source without `.ai/` laid out under
/// `<staged>/.ai/src/skills/` as an import of that tree. Each skill takes the
/// name its `SKILL.md` declares and lands where the project already keeps a
/// skill of that name. `Ok(None)` when the source holds no skill; the inner
/// `Err` names two skills of the source that share a name.
fn stage_skills(
    root: &str,
    fetched: &Path,
    staged: &Path,
    source: &str,
) -> Result<Result<Option<PathBuf>, String>, Error> {
    let found = skill_dirs(fetched, &source_stem(source));
    if found.is_empty() {
        return Ok(Ok(None));
    }
    let mut named: Vec<(PathBuf, String)> = Vec::new();
    for (dir, fallback) in found {
        let declared = std::fs::read(dir.join("SKILL.md"))
            .ok()
            .and_then(|bytes| skill_metadata::name(&bytes))
            .filter(|name| skill_metadata::valid_name(name));
        let name = declared.unwrap_or_else(|| skill_slug(&fallback));
        if let Some((first, _)) = named.iter().find(|(_, taken)| *taken == name) {
            let shown = |dir: &Path| dir.strip_prefix(fetched).unwrap_or(dir).disk_text();
            return Ok(Err(format!(
                "Two skills in the source are named '{name}': {} and {}",
                shown(first),
                shown(&dir)
            )));
        }
        named.push((dir, name));
    }
    let base = match resolve_sources(root).base.as_str() {
        "" => ".ai/src".to_string(),
        base => base.to_string(),
    };
    let existing =
        skill_tree::discover(&Workspace::on_disk(root), &format!("{root}/{base}/skills"));
    let skills_root = staged.join(".ai/src/skills");
    for (dir, name) in named {
        let rel = existing.find(&name).map_or(name, |skill| skill.rel.clone());
        copy_tree(&dir, &skills_root.join(rel))?;
    }
    Ok(Ok(Some(staged.join(".ai/src"))))
}

/// `text` as a valid skill name: ASCII letters and digits lowercased, every
/// other run of characters one hyphen, at most 64 bytes.
fn skill_slug(text: &str) -> String {
    let mut slug = String::new();
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
        } else if !slug.is_empty() && !slug.ends_with('-') {
            slug.push('-');
        }
    }
    slug.truncate(64);
    match slug.trim_end_matches('-') {
        "" => "imported-skill".to_string(),
        trimmed => trimmed.to_string(),
    }
}

/// The file name of `source` without an archive suffix: the name of a skill
/// whose `SKILL.md` declares none.
fn source_stem(source: &str) -> String {
    let leaf = Path::new(source)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let lower = leaf.to_ascii_lowercase();
    ZIP_SUFFIXES
        .iter()
        .chain(TAR_SUFFIXES.iter())
        .find(|suffix| lower.ends_with(*suffix))
        .map_or_else(
            || leaf.clone(),
            |suffix| leaf[..leaf.len() - suffix.len()].to_string(),
        )
}

fn same_bytes(a: &Path, b: &Path) -> bool {
    match (std::fs::read(a), std::fs::read(b)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// `cp <src> <dest>`: an existing destination keeps its mode, a new one takes
/// the source's.
fn copy_file(src: &Path, dest: &Path) -> Result<(), Error> {
    if dest.is_file() {
        let bytes = std::fs::read(src).map_err(|e| Error::io(src, e))?;
        std::fs::write(dest, bytes).map_err(|e| Error::io(dest, e))
    } else {
        std::fs::copy(src, dest)
            .map(|_| ())
            .map_err(|e| Error::io(src, e))
    }
}

enum Change {
    New(String),
    Update(String),
    Dir(String),
    Remove(String),
}

#[derive(Default)]
struct Counts {
    new: usize,
    updated: usize,
    removed: usize,
    skipped: usize,
}

/// What an import changes: one line per file or directory, the tallies, and
/// the exact files it writes and removes.
#[derive(Default)]
struct Diff {
    changes: Vec<Change>,
    counts: Counts,
    /// `(source, destination)` of every new or changed file.
    writes: Vec<(PathBuf, PathBuf)>,
    removals: Vec<PathBuf>,
}

impl Diff {
    /// `_import_diff_file`.
    fn file(&mut self, src: &Path, dest: &Path, label: &str) {
        if !dest.is_file() {
            self.changes.push(Change::New(label.to_string()));
            self.counts.new += 1;
        } else if same_bytes(src, dest) {
            self.counts.skipped += 1;
            return;
        } else {
            self.changes.push(Change::Update(label.to_string()));
            self.counts.updated += 1;
        }
        self.writes.push((src.to_path_buf(), dest.to_path_buf()));
    }

    /// `_import_diff_dir`.
    fn dir(&mut self, src: &Path, dest: &Path, label: &str) {
        let (mut new, mut updated, mut skipped) = (0usize, 0usize, 0usize);
        let mut files = Vec::new();
        files_below(src, &mut files);
        files.sort();
        for file in files {
            let rel = file.strip_prefix(src).unwrap_or(&file);
            let target = dest.join(rel);
            if !target.is_file() {
                new += 1;
            } else if same_bytes(&file, &target) {
                skipped += 1;
                continue;
            } else {
                updated += 1;
            }
            self.writes.push((file.clone(), target));
        }
        self.counts.skipped += skipped;
        if new + updated == 0 {
            return;
        }
        let mut detail = Vec::new();
        if new > 0 {
            detail.push(format!("{new} new"));
        }
        if updated > 0 {
            detail.push(format!("{updated} updated"));
        }
        if skipped > 0 {
            detail.push(format!("{skipped} unchanged"));
        }
        let line = format!("{label} ({})", detail.join(", "));
        self.changes.push(Change::Dir(line));
        self.counts.new += new;
        self.counts.updated += updated;
    }

    /// The installed files each skill below `src` replaces: a skill is
    /// replaced as a whole, and by name, so a copy the project keeps under
    /// another category goes entirely.
    fn dropped_skill_files(&mut self, src: &Path, dest: &Path, label: &str) {
        let src_text = src.disk_text();
        let dest_text = dest.disk_text();
        let incoming = skill_tree::discover(&Workspace::on_disk(&src_text), &src_text).skills;
        let installed = skill_tree::discover(&Workspace::on_disk(&dest_text), &dest_text).skills;
        for skill in &incoming {
            let kept = src.join(&skill.rel);
            self.drop_missing(&dest.join(&skill.rel), &skill.rel, Some(&kept), label);
            for moved in installed
                .iter()
                .filter(|copy| copy.name == skill.name && copy.rel != skill.rel)
            {
                self.drop_missing(&dest.join(&moved.rel), &moved.rel, None, label);
            }
        }
    }

    /// Every file below `installed` that `kept` lacks, or all of them.
    fn drop_missing(&mut self, installed: &Path, rel: &str, kept: Option<&Path>, label: &str) {
        let mut files = Vec::new();
        files_below(installed, &mut files);
        files.sort();
        for file in files {
            let inner = file.strip_prefix(installed).unwrap_or(&file);
            if kept.is_some_and(|kept| kept.join(inner).is_file()) {
                continue;
            }
            let shown = format!("{label}/{rel}/{}", inner.disk_text());
            self.changes.push(Change::Remove(shown));
            self.counts.removed += 1;
            self.removals.push(file);
        }
    }
}

/// `--only`: the entries the comma list names, in their order; an item names
/// an entry with or without its extension, so `AGENTS` is `AGENTS.md` and
/// `mcp` is both `mcp/` and `mcp.json`.
fn filter_targets(names: Vec<String>, only: &str) -> Vec<String> {
    let selected: Vec<&str> = only
        .split('\n')
        .next()
        .unwrap_or("")
        .split(',')
        .map(|item| {
            item.trim_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c'))
        })
        .collect();
    names
        .into_iter()
        .filter(|name| {
            let stem = name
                .rsplit_once('.')
                .map_or(name.as_str(), |(stem, _)| stem);
            selected.iter().any(|item| name == item || stem == *item)
        })
        .collect()
}

struct ImportArgs {
    source: String,
    dry_run: bool,
    force: bool,
    config: bool,
    only: String,
    /// `--ref`: the branch, tag, or commit to fetch from a remote.
    reference: String,
    /// `--path`: the folder of a remote that holds its `.ai/`.
    path: String,
}

/// How a command line that imports nothing ends: help on stdout, or an error
/// line on stderr followed by the help when `with_help`.
enum BundleStop {
    Help,
    Refuse { message: String, with_help: bool },
}

fn refuse(message: String, with_help: bool) -> BundleStop {
    BundleStop::Refuse { message, with_help }
}

/// Ends a command line that bundles nothing: `help` on stdout, or the error
/// on stderr followed by `help` when asked. The exit status.
fn stopped(
    stop: BundleStop,
    help: &Help,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match stop {
        BundleStop::Help => {
            put(out, help.render(style).as_bytes())?;
            Ok(0)
        }
        BundleStop::Refuse { message, with_help } => {
            put(
                err,
                format!("{}: {message}\n", style.red("Error")).as_bytes(),
            )?;
            if with_help {
                put(err, help.render(style).as_bytes())?;
            }
            Ok(1)
        }
    }
}

fn parse_import(args: &[String]) -> Result<ImportArgs, BundleStop> {
    let mut parsed = ImportArgs {
        source: String::new(),
        dry_run: false,
        force: false,
        config: false,
        only: String::new(),
        reference: String::new(),
        path: String::new(),
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--dry-run" => parsed.dry_run = true,
            "--force" => parsed.force = true,
            "--config" => parsed.config = true,
            flag @ ("--only" | "--ref" | "--branch" | "-b" | "--path") => {
                let name = match flag {
                    "--only" | "--path" => flag,
                    _ => "--ref",
                };
                let Some(value) = args.next() else {
                    return Err(refuse(format!("{name} requires a value"), false));
                };
                let field = match name {
                    "--only" => &mut parsed.only,
                    "--path" => &mut parsed.path,
                    _ => &mut parsed.reference,
                };
                *field = value.clone();
            }
            "--help" | "-h" => return Err(BundleStop::Help),
            flag if flag.starts_with('-') => {
                return Err(refuse(format!("Unknown option: {flag}"), true));
            }
            positional if parsed.source.is_empty() => parsed.source = positional.to_string(),
            positional => return Err(refuse(format!("Unexpected argument: {positional}"), false)),
        }
    }
    if parsed.source.is_empty() {
        return Err(refuse("No source specified.".into(), true));
    }
    Ok(parsed)
}

/// The streams and process inputs an import reports through while it fetches.
struct Importer<'a, 'b> {
    style: &'a Style,
    env: &'a mut Env<'b>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Importer<'_, '_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())?;
        self.out.flush().map_err(|e| Error::io("<stdout>", e))
    }

    /// Asks before an import without `--force` writes: always when it brings
    /// config that runs commands, which is refused outright without a
    /// terminal to ask on, and when it overwrites or removes files. The exit
    /// status once the import stops here.
    fn confirm(&mut self, plan: &ImportPlan) -> Result<Option<u8>, Error> {
        let (question, default_yes) = if !plan.runs.is_empty() {
            if !self.env.interactive {
                let text = format!(
                    "  {}: The import brings config that runs commands (listed above).\n  Review it with {}, then pass {} to import it.\n",
                    self.style.red("Error"),
                    self.style.cyan("--dry-run"),
                    self.style.cyan("--force")
                );
                put(self.err, text.as_bytes())?;
                return Ok(Some(1));
            }
            ("  Import config that runs these commands? [y/N] ", false)
        } else if plan.diff.counts.updated + plan.diff.counts.removed > 0 {
            if !self.env.interactive {
                return Ok(None);
            }
            ("  Proceed? [Y/n] ", true)
        } else {
            return Ok(None);
        };
        self.say(question)?;
        let answer = (self.env.read_line)();
        let proceed = if default_yes {
            !answer.starts_with(['N', 'n'])
        } else {
            answer.starts_with(['Y', 'y'])
        };
        if proceed {
            return Ok(None);
        }
        put(self.out, b"  Cancelled.\n\n")?;
        Ok(Some(0))
    }

    /// Prints the failure on stderr and answers the failed fetch.
    fn fail(&mut self, message: &str) -> Result<Option<String>, Error> {
        let text = format!("  {}: {message}\n", self.style.red("Error"));
        put(self.err, text.as_bytes())?;
        Ok(None)
    }

    /// Fetches `source` into `tmp`: the label the report names it by, or `None`
    /// once the failure is printed.
    fn fetch(&mut self, parsed: &ImportArgs, tmp: &Path) -> Result<Option<String>, Error> {
        let source = parsed.source.as_str();
        let path = Path::new(source);
        let lower = source.to_ascii_lowercase();
        if path.is_file() && ZIP_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix)) {
            return self.extract_zip(source, tmp);
        }
        if path.is_file() && TAR_SUFFIXES.iter().any(|suffix| lower.ends_with(suffix)) {
            return self.extract_archive(source, tmp);
        }
        if path.is_dir() {
            return self.copy_directory(source, tmp);
        }
        if let Some(remote) = remote::parse(source).filter(|_| !path.exists()) {
            return self.fetch_git(&remote, parsed, tmp);
        }
        self.fail(&format!(
            "Cannot recognize source: {source}\n  Expected: git URL, archive (.tar.gz, .zip, .skill), or directory path."
        ))
    }

    /// Fetches the remote's ref and folder — from `--ref` and `--path`, else
    /// from the link — and moves that folder to `tmp`.
    fn fetch_git(
        &mut self,
        remote: &remote::Remote,
        parsed: &ImportArgs,
        tmp: &Path,
    ) -> Result<Option<String>, Error> {
        let (reference, folder) = match &remote.tree {
            Some(tree) if parsed.reference.is_empty() && parsed.path.is_empty() => {
                match remote::refs(&remote.url) {
                    Ok(refs) => remote::split_tree(tree, &refs),
                    Err(message) => return self.fail(&message),
                }
            }
            _ => (
                parsed.reference.clone(),
                parsed.path.trim_matches('/').to_string(),
            ),
        };
        let place = match (reference.as_str(), folder.as_str()) {
            ("", "") => String::new(),
            (reference, "") => format!(" at {reference}"),
            ("", folder) => format!(" at the default branch, {folder}"),
            (reference, folder) => format!(" at {reference}, {folder}"),
        };
        let shown = format!("{}{place}", remote.url);
        self.say(&format!("  Fetching {}...\n", self.style.cyan(&shown)))?;
        let checkout = tmp.with_file_name("repo");
        std::fs::create_dir_all(&checkout).map_err(|e| Error::io(&checkout, e))?;
        let request = remote::Request {
            url: &remote.url,
            reference: &reference,
            folder: &folder,
        };
        let fetched = match remote::fetch(&request, &checkout) {
            Ok(fetched) => fetched,
            Err(message) => return self.fail(&message),
        };
        let root = ai_owner(&fetched);
        std::fs::remove_dir(tmp).map_err(|e| Error::io(tmp, e))?;
        std::fs::rename(&root, tmp).map_err(|e| Error::io(&root, e))?;
        self.say(&format!("  {}\n", self.style.green("Fetched.")))?;
        Ok(Some(format!("Git: {shown}")))
    }

    fn extract_zip(&mut self, source: &str, tmp: &Path) -> Result<Option<String>, Error> {
        let style = self.style;
        let base_name = source.rsplit('/').next().unwrap_or(source).to_string();
        self.say(&format!("  Extracting {}...\n", style.cyan(&base_name)))?;
        let size = std::fs::metadata(source)
            .map_err(|e| Error::io(source, e))?
            .len();
        let limit = crate::zip::MAX_EXPANDED;
        if u64::try_from(limit).is_ok_and(|limit| size > limit) {
            return self.fail(&format!(
                "Failed to extract archive: larger than {} MB",
                limit >> 20
            ));
        }
        let bytes = std::fs::read(source).map_err(|e| Error::io(source, e))?;
        let entries = match crate::zip::read(&bytes) {
            Ok(entries) => entries,
            Err(reason) => return self.fail(&format!("Failed to extract archive: {reason}")),
        };
        for entry in entries {
            // macOS Archive Utility adds __MACOSX/ resource forks and Finder's .DS_Store.
            if entry.path.starts_with("__MACOSX/") || crate::paths::leaf(&entry.path) == ".DS_Store"
            {
                continue;
            }
            let dest = tmp.join(&entry.path);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            std::fs::write(&dest, &entry.data).map_err(|e| Error::io(&dest, e))?;
            #[cfg(unix)]
            if entry.executable {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(&dest, std::fs::Permissions::from_mode(0o755))
                    .map_err(|e| Error::io(&dest, e))?;
            }
        }
        self.say(&format!("  {}\n", style.green("Extracted.")))?;
        Ok(Some(format!("Archive: {base_name}")))
    }

    fn extract_archive(&mut self, source: &str, tmp: &Path) -> Result<Option<String>, Error> {
        let style = self.style;
        let base_name = source.rsplit('/').next().unwrap_or(source).to_string();
        self.say(&format!("  Extracting {}...\n", style.cyan(&base_name)))?;
        if !tar_extract(Path::new(source), tmp) {
            return self.fail("Failed to extract archive.");
        }
        self.say(&format!("  {}\n", style.green("Extracted.")))?;
        Ok(Some(format!("Archive: {base_name}")))
    }

    fn copy_directory(&mut self, source: &str, tmp: &Path) -> Result<Option<String>, Error> {
        let Ok(canonical) = std::fs::canonicalize(source) else {
            return self.fail(&format!("Cannot access directory: {source}"));
        };
        let spelled = if crate::paths::is_absolute(source) {
            source.to_string()
        } else {
            format!("{}/{source}", self.env.cwd)
        };
        let spelled = crate::paths::normalize(&spelled);
        let shown = if std::fs::canonicalize(&spelled).ok().as_deref() == Some(canonical.as_path())
        {
            spelled
        } else {
            canonical.disk_text()
        };
        self.say(&format!("  Reading from {}...\n", self.style.cyan(&shown)))?;
        let src_dir = Path::new(&shown);
        if src_dir.join(".ai").is_dir() {
            copy_tree(&src_dir.join(".ai"), &tmp.join(".ai"))?;
        } else {
            for (dir, _) in skill_dirs(src_dir, "") {
                let rel = dir.strip_prefix(src_dir).unwrap_or(&dir);
                copy_tree(&dir, &tmp.join(rel))?;
            }
        }
        for (rel, _) in export_items(&shown, &resolve_sources(&shown), self.style) {
            let outside_ai = !rel.starts_with(".ai/")
                && !crate::paths::is_absolute(&rel)
                && !rel.split('/').any(|segment| segment == "..");
            let from = src_dir.join(&rel);
            if !outside_ai {
                continue;
            }
            if from.is_dir() {
                copy_tree(&from, &tmp.join(&rel))?;
            } else if from.is_file() {
                let to = tmp.join(&rel);
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
                }
                std::fs::copy(&from, &to).map_err(|e| Error::io(&from, e))?;
            }
        }
        if ai_config(tmp) == CONFIG
            && !tmp.join(CONFIG).is_file()
            && src_dir.join(CONFIG_LEGACY).is_file()
        {
            std::fs::create_dir_all(tmp.join(".ai")).map_err(|e| Error::io(tmp, e))?;
            std::fs::copy(src_dir.join(CONFIG_LEGACY), tmp.join(CONFIG))
                .map_err(|e| Error::io(src_dir.join(CONFIG_LEGACY), e))?;
        }
        Ok(Some(format!("Directory: {source}")))
    }
}

/// What an import does with the config the source carries.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ConfigAction {
    /// The source has none, or it matches the project's.
    Skip,
    New,
    Update,
    /// The project keeps its own; `--config` would replace it.
    Kept,
}

impl ConfigAction {
    fn writes(self) -> bool {
        matches!(self, Self::New | Self::Update)
    }
}

/// What importing `src_root` over the project writes.
struct ImportPlan {
    dest_base: PathBuf,
    dest_base_rel: String,
    diff: Diff,
    imported_config: Option<String>,
    config_dest: PathBuf,
    config_action: ConfigAction,
    /// What the new and changed files would have a tool run.
    runs: Vec<String>,
}

/// The imported project's copy of `target`: the path its `source.*` declares
/// when that is relative, stays inside the project, and exists; else the
/// standard place under `src_root`.
fn imported_source(src_root: &Path, project: &Path, declared: &Sources, target: &str) -> PathBuf {
    let rel = if target == "AGENTS.md" {
        declared.agents.as_str()
    } else {
        declared
            .dirs
            .iter()
            .find(|(name, _)| *name == target)
            .map_or("", |(_, path)| path.as_str())
    };
    let inside = !rel.is_empty()
        && !crate::paths::is_absolute(rel)
        && !rel.split('/').any(|segment| segment == "..");
    if inside && project.join(rel).exists() {
        return project.join(rel);
    }
    src_root.join(target)
}

/// Each `source.<key>` with the target the import writes it to.
const SOURCE_KEYS: [(&str, &str); 6] = [
    ("agents", "AGENTS.md"),
    ("rules", "rules"),
    ("skills", "skills"),
    ("commands", "commands"),
    ("subagents", "agents"),
    ("tools", "tools"),
];

/// The imported config's text without the `source.*` paths that name another
/// place than `<dest_base_rel>/<target>`, where the import writes each target,
/// and without a `source:` those removals empty; `None` when nothing is left.
fn imported_config_text(project: &Path, dest_base_rel: &str) -> Option<String> {
    use crate::config::{yaml_edit::remove_key_text, yaml_subset};
    let path = project.join(present_config(project)?);
    let mut text = String::from_utf8_lossy(&std::fs::read(&path).ok()?).into_owned();
    for (key, target) in SOURCE_KEYS {
        let key_path = format!("source.{key}");
        let value = yaml_subset::value(&text, &key_path);
        if !value.is_empty() && value != format!("{dest_base_rel}/{target}") {
            text = remove_key_text(&text, &key_path).unwrap_or(text);
        }
    }
    let emptied = SOURCE_KEYS
        .iter()
        .all(|(key, _)| yaml_subset::found(&text, &format!("source.{key}")).is_none());
    if emptied && yaml_subset::found(&text, "source") == Some(String::new()) {
        text = remove_key_text(&text, "source").unwrap_or(text);
    }
    (!text.trim().is_empty()).then_some(text)
}

/// The choices an import plan follows: `--only` and `--config`.
#[derive(Default)]
struct PlanOptions<'a> {
    only: &'a str,
    replace_config: bool,
}

fn plan_import(root: &str, src_root: PathBuf, options: &PlanOptions) -> ImportPlan {
    let PlanOptions {
        only,
        replace_config,
    } = *options;
    let src_project_root = if src_root.ends_with("src") {
        src_root.parent().and_then(Path::parent)
    } else {
        src_root.parent()
    }
    .map_or_else(|| src_root.clone(), Path::to_path_buf);
    let declared = resolve_sources(&src_project_root.disk_text());

    let local = resolve_sources(root);
    let dest_base_rel = if local.base.is_empty() {
        ".ai/src".to_string()
    } else {
        local.base.clone()
    };
    let imported_config = imported_config_text(&src_project_root, &dest_base_rel);
    let dest_base = Path::new(root).join(&dest_base_rel);

    let mut names = source_entries(&src_root);
    for target in std::iter::once("AGENTS.md").chain(DIR_TARGETS) {
        let declared_elsewhere = imported_source(&src_root, &src_project_root, &declared, target)
            != src_root.join(target);
        if declared_elsewhere && !names.iter().any(|name| name == target) {
            names.push(target.to_string());
        }
    }
    if !only.is_empty() {
        names = filter_targets(names, only);
    }
    let mut diff = Diff::default();
    for name in &names {
        let src_path = imported_source(&src_root, &src_project_root, &declared, name);
        let dest_path = dest_base.join(name);
        if src_path.is_file() {
            diff.file(&src_path, &dest_path, name);
        } else if src_path.is_dir() {
            diff.dir(&src_path, &dest_path, name);
            if name == "skills" {
                diff.dropped_skill_files(&src_path, &dest_path, name);
            }
        }
    }
    let config_dest = Path::new(root).join(ai_config(Path::new(root)));
    let config_action = match &imported_config {
        None => ConfigAction::Skip,
        Some(_) if !config_dest.is_file() => ConfigAction::New,
        Some(text) if std::fs::read(&config_dest).ok().as_deref() == Some(text.as_bytes()) => {
            ConfigAction::Skip
        }
        Some(_) if !replace_config => ConfigAction::Kept,
        Some(_) => ConfigAction::Update,
    };
    match config_action {
        ConfigAction::New => diff.counts.new += 1,
        ConfigAction::Update => diff.counts.updated += 1,
        ConfigAction::Skip | ConfigAction::Kept => {}
    }
    let runs = diff
        .writes
        .iter()
        .flat_map(|(src, dest)| {
            let rel = dest.strip_prefix(&dest_base).unwrap_or(dest).disk_text();
            command_surfaces::of(&rel, src)
        })
        .collect();
    ImportPlan {
        dest_base,
        dest_base_rel,
        diff,
        imported_config,
        config_dest,
        config_action,
        runs,
    }
}

/// `N new, M updated`, with `K removed` once anything is.
fn tally(counts: &Counts) -> String {
    let mut text = format!("{} new, {} updated", counts.new, counts.updated);
    if counts.removed > 0 {
        text.push_str(&format!(", {} removed", counts.removed));
    }
    text
}

fn plan_text(style: &Style, plan: &ImportPlan, dry_run: bool) -> String {
    let mut text = format!("  {}\n", style.green("Changes:"));
    for change in &plan.diff.changes {
        match change {
            Change::New(name) => text.push_str(&format!(
                "    {} {name} {}\n",
                style.green("+"),
                style.dim("(new)")
            )),
            Change::Update(name) => text.push_str(&format!(
                "    {} {name} {}\n",
                style.yellow("~"),
                style.dim("(update)")
            )),
            Change::Dir(name) => text.push_str(&format!("    {} {name}\n", style.cyan("↳"))),
            Change::Remove(name) => text.push_str(&format!(
                "    {} {name} {}\n",
                style.red("-"),
                style.dim("(removed)")
            )),
        }
    }
    let config_name = plan
        .config_dest
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    match plan.config_action {
        ConfigAction::New => text.push_str(&format!(
            "    {} {config_name} {}\n",
            style.green("+"),
            style.dim("(new)")
        )),
        ConfigAction::Update => text.push_str(&format!(
            "    {} {config_name} {}\n",
            style.yellow("~"),
            style.dim("(update)")
        )),
        ConfigAction::Kept => text.push_str(&format!(
            "    {} {config_name} kept {}\n",
            style.dim("="),
            style.dim("(pass --config to replace)")
        )),
        ConfigAction::Skip => {}
    }
    if !plan.runs.is_empty() {
        text.push_str(&format!("\n  {}\n", style.yellow("Runs commands:")));
        for line in &plan.runs {
            text.push_str(&format!("    {} {line}\n", style.yellow("!")));
        }
    }
    let counts = &plan.diff.counts;
    text.push_str(&format!(
        "\n  {} {}, {} unchanged\n\n",
        style.dim("Summary:"),
        tally(counts),
        counts.skipped
    ));
    if dry_run {
        text.push_str(&format!(
            "  {} — no files written.\n\n",
            style.yellow("Dry run")
        ));
    }
    text
}

/// Applies `plan`, removals first so a path that turns from a file into a
/// folder, or back, is free when its write comes. `Some(signal)` once
/// `interrupted` reports one between two steps, the rest left undone.
fn apply_import(
    plan: &ImportPlan,
    interrupted: &dyn Fn() -> Option<i32>,
) -> Result<Option<i32>, Error> {
    let skills = plan.dest_base.join("skills");
    for file in &plan.diff.removals {
        if let Some(signal) = interrupted() {
            return Ok(Some(signal));
        }
        std::fs::remove_file(file).map_err(|e| Error::io(file, e))?;
        let mut dir = file.parent();
        while let Some(parent) = dir.filter(|parent| *parent != skills.as_path()) {
            if std::fs::remove_dir(parent).is_err() {
                break;
            }
            dir = parent.parent();
        }
    }
    for (src, dest) in &plan.diff.writes {
        if let Some(signal) = interrupted() {
            return Ok(Some(signal));
        }
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
        }
        copy_file(src, dest)?;
    }
    if let Some(signal) = interrupted() {
        return Ok(Some(signal));
    }
    let Some(imported) = plan
        .imported_config
        .as_ref()
        .filter(|_| plan.config_action.writes())
    else {
        return Ok(None);
    };
    if let Some(parent) = plan.config_dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&plan.config_dest, imported).map_err(|e| Error::io(&plan.config_dest, e))?;
    Ok(None)
}

/// Every project-relative path `plan` writes or removes; a write below a
/// removed file is covered by that file's entry.
fn backup_targets(root: &str, plan: &ImportPlan) -> Vec<String> {
    let removals = &plan.diff.removals;
    let mut changed: Vec<&Path> = removals
        .iter()
        .map(PathBuf::as_path)
        .chain(
            plan.diff
                .writes
                .iter()
                .map(|(_, dest)| dest.as_path())
                .filter(|dest| {
                    !removals
                        .iter()
                        .any(|removed| dest.starts_with(removed) && dest != removed)
                }),
        )
        .collect();
    if plan.config_action.writes() {
        changed.push(&plan.config_dest);
    }
    changed
        .into_iter()
        .map(|path| path.strip_prefix(root).unwrap_or(path).disk_text())
        .collect()
}

/// How a backed-up import ended.
enum Applied {
    /// Sealed under this snapshot id.
    Done(String),
    /// Stopped by this signal and restored; the caller re-raises it.
    Interrupted(i32),
}

/// Applies `plan` inside an `import` backup of every path it touches, as
/// `sync` and `mcp` back up theirs: a failed write, or a signal `interrupted`
/// reports, restores the project from it.
fn apply_backed_up(
    root: &str,
    plan: &ImportPlan,
    err: &mut dyn Write,
    interrupted: &dyn Fn() -> Option<i32>,
) -> Result<Applied, Error> {
    let config = present_config(Path::new(root)).and_then(|rel| {
        Some((
            rel,
            std::fs::read_to_string(Path::new(root).join(rel)).ok()?,
        ))
    });
    let lookup = |name: &str| std::env::var(name).ok();
    let limit = names::env("BACKUP_LIMIT", &lookup);
    let age = names::env("BACKUP_MAX_AGE_DAYS", &lookup);
    let retention = backup::configure(
        config.as_ref().map(|(rel, text)| (*rel, text.as_str())),
        limit.as_deref(),
        age.as_deref(),
    )?;
    let snapshot = backup::create(root, "import", &backup_targets(root, plan), retention)?;
    let id = crate::paths::leaf(&snapshot);
    let stopped = match apply_import(plan, interrupted) {
        Ok(None) => None,
        Ok(Some(signal)) => Some(Ok(signal)),
        Err(error) => Some(Err(error)),
    };
    if let Some(stopped) = stopped {
        let cause = if stopped.is_ok() {
            "interrupted"
        } else {
            "failed"
        };
        let note = match backup::restore(root, &snapshot) {
            Ok(()) => {
                let _ = witness::seal(root, &snapshot);
                format!("  Import {cause}; restored the project from backup {id}.\n")
            }
            Err(restore) => format!(
                "  Import {cause}; automatic restore failed ({restore}). Backup retained at .ai/backups/{id}\n"
            ),
        };
        put(err, note.as_bytes())?;
        return stopped.map(Applied::Interrupted);
    }
    if let Err(reason) = witness::seal(root, &snapshot) {
        let text = format!("  Warning: Could not record the import backup state: {reason}\n");
        put(err, text.as_bytes())?;
    }
    if let Err(error) = backup::prune(root, limit.as_deref(), age.as_deref(), retention) {
        put(
            err,
            format!("  Warning: Could not prune backups: {error}\n").as_bytes(),
        )?;
    }
    Ok(Applied::Done(id))
}

/// `cmd_import`.
pub fn import(
    args: &[String],
    root: &str,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let parsed = match parse_import(args) {
        Ok(parsed) => parsed,
        Err(stop) => return stopped(stop, &IMPORT_HELP, style, out, err),
    };
    put(
        out,
        format!("\n{}\n\n", style.bold("  Exuno Import")).as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let scratch = Scratch::create("agentsync-import")?;
    let tmp = scratch.0.as_path();
    let mut importer = Importer {
        style,
        env,
        out,
        err,
    };
    let fetched = tmp.join("source");
    std::fs::create_dir_all(&fetched).map_err(|e| Error::io(&fetched, e))?;
    let Some(label) = importer.fetch(&parsed, &fetched)? else {
        return Ok(1);
    };
    put(
        importer.out,
        format!("  {} {label}\n\n", style.dim("Source:")).as_bytes(),
    )?;
    let src_root = match find_ai_src(&fetched) {
        Some(found) => Some(found),
        None => match stage_skills(root, &fetched, &tmp.join("staged"), &parsed.source)? {
            Ok(staged) => staged,
            Err(message) => {
                importer.fail(&message)?;
                return Ok(1);
            }
        },
    };
    let Some(src_root) = src_root else {
        put(
            importer.err,
            format!(
                "  {}: No .ai/src/ (or .ai/) directory or SKILL.md found in source.\n  The source must contain a structure created by {}, or skills.\n",
                style.red("Error"),
                style.cyan("exuno init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let options = PlanOptions {
        only: &parsed.only,
        replace_config: parsed.config,
    };
    let plan = plan_import(root, src_root, &options);
    if plan.diff.changes.is_empty() && !plan.config_action.writes() {
        let text = format!(
            "  {} Nothing to import.\n\n",
            style.green("Already up to date!")
        );
        put(importer.out, text.as_bytes())?;
        return Ok(0);
    }
    put(
        importer.out,
        plan_text(style, &plan, parsed.dry_run).as_bytes(),
    )?;
    if parsed.dry_run {
        return Ok(0);
    }
    if !parsed.force
        && let Some(status) = importer.confirm(&plan)?
    {
        return Ok(status);
    }
    let mut interrupt = Interrupt::arm();
    let backup_id = match apply_backed_up(root, &plan, importer.err, &|| interrupt.received())? {
        Applied::Done(id) => id,
        Applied::Interrupted(signal) => {
            interrupt.resend(signal);
            return Ok(interrupt::status(signal));
        }
    };
    drop(interrupt);
    put(
        importer.out,
        imported_text(style, &plan, &backup_id).as_bytes(),
    )
    .map(|()| 0)
}

fn imported_text(style: &Style, plan: &ImportPlan, backup_id: &str) -> String {
    format!(
        "  {} {} files.\n  {} .ai/backups/{backup_id} — undo with {}\n\n  Next steps:\n    1. Review imported files in {}\n    2. Run {} to distribute to all tools\n\n",
        style.green("Imported!"),
        tally(&plan.diff.counts),
        style.dim("Backup:"),
        style.cyan("exuno rollback"),
        style.cyan(&plan.dest_base_rel),
        style.cyan("exuno sync")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_help_has_the_shared_shape() {
        assert_eq!(
            EXPORT_HELP.render(&Style::plain()),
            "\n  exuno export — bundle source files into a shareable archive\n\n  USAGE\n    exuno export [OPTIONS]\n\n  OPTIONS\n    -o, --output <path>   Output file; .zip writes a ZIP (default: ./exuno-bundle.tar.gz)\n    --skill <name>        Package one skill as ./<name>.skill (Claude skill upload)\n    --dry-run             Preview what would be exported\n    -h, --help            Show this help\n\n  EXAMPLES\n    exuno export\n    exuno export -o my-config.tar.gz\n    exuno export --skill my-skill\n    exuno export --dry-run\n\n"
        );
    }

    #[test]
    fn import_help_has_the_shared_shape() {
        assert_eq!(
            IMPORT_HELP.render(&Style::plain()),
            "\n  exuno import — import config from a git remote, archive, or directory\n\n  USAGE\n    exuno import <source> [OPTIONS]\n\n  DESCRIPTION\n    An imported skill replaces the project's copy whole: files the new\n    version no longer has are removed. An import that changes files is\n    backed up first, so exuno rollback undoes it.\n\n  SOURCES\n    Git repository    https://github.com/user/repo[/tree/<ref>/<folder>],\n                      git@host:org/repo.git, or user/repo for GitHub\n    Archive file      path/to/exuno-bundle.tar.gz (.tar, .tar.xz, .tar.bz2, .zip)\n    Skill package     path/to/my-skill.skill, or a folder or archive of skills\n    Local directory   path/to/project/\n\n  OPTIONS\n    --ref <name>       Branch, tag, or commit to fetch (default: the remote's\n                       default branch); -b and --branch are aliases\n    --path <folder>    Folder of the repository that holds its .ai/\n    --only <targets>   Import only specific targets (comma-separated)\n                       Targets: rules,skills,commands,agents,settings,mcp,hooks,tools\n    --config           Replace the project's exuno.yaml with the source's\n    --force            Skip confirmations, config that runs commands included\n    --dry-run          Preview changes without writing\n    -h, --help         Show this help\n\n  EXAMPLES\n    exuno import https://github.com/user/repo\n    exuno import https://github.com/user/repo/tree/v2/packages/app\n    exuno import user/repo --ref develop\n    exuno import git@github.com:org/private.git\n    exuno import exuno-bundle.tar.gz\n    exuno import my-skill.skill\n    exuno import ../other-project/\n    exuno import https://github.com/user/repo --only rules,skills\n    exuno import bundle.tar.gz --dry-run\n\n"
        );
    }

    #[test]
    fn source_stem_drops_the_directory_and_the_archive_suffix() {
        assert_eq!(source_stem("in/My Notes.SKILL"), "My Notes");
        assert_eq!(source_stem("a/b/pack.tar.gz"), "pack");
        assert_eq!(source_stem("https://github.com/u/repo"), "repo");
    }

    #[test]
    fn skill_slug_yields_a_valid_skill_name() {
        for (text, slug) in [
            ("Download (1)", "download-1"),
            ("C:\\Users\\me\\x", "c-users-me-x"),
            ("--Ünïcode--", "n-code"),
            ("…", "imported-skill"),
        ] {
            assert_eq!(skill_slug(text), slug);
            assert!(skill_metadata::valid_name(&skill_slug(text)), "{text}");
        }
        assert_eq!(skill_slug(&"a".repeat(80)).len(), 64);
    }

    #[test]
    fn only_filters_the_targets_in_target_order() {
        let targets =
            || -> Vec<String> { ENTRY_ORDER.iter().map(|name| name.to_string()).collect() };
        assert_eq!(
            filter_targets(targets(), " AGENTS , bogus "),
            vec!["AGENTS.md"]
        );
        assert_eq!(
            filter_targets(targets(), "skills,rules"),
            vec!["rules", "skills"]
        );
        assert_eq!(
            filter_targets(targets(), "AGENTS.md,tools"),
            vec!["AGENTS.md", "tools"]
        );
        assert_eq!(filter_targets(targets(), "mcp"), vec!["mcp", "mcp.json"]);
        assert!(filter_targets(targets(), "nothing").is_empty());
    }

    #[test]
    fn sizes_read_like_the_exported_line() {
        assert_eq!(human_size(Some(543)), "543 B");
        assert_eq!(human_size(Some(46_000)), "44 KB");
        assert_eq!(human_size(Some(2_000_000)), "1 MB");
        assert_eq!(human_size(None), "? B");
    }

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn a_signal_between_two_steps_restores_the_project() {
        let project = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(project.path()).unwrap().disk_text();
        write(project.path(), ".ai/src/skills/jury/SKILL.md", "# Old\n");
        write(project.path(), ".ai/src/skills/jury/old.md", "# Dropped\n");
        let source = tempfile::tempdir().unwrap();
        write(source.path(), ".ai/src/skills/jury/SKILL.md", "# New\n");
        let plan = plan_import(
            &root,
            source.path().join(".ai/src"),
            &PlanOptions::default(),
        );
        assert_eq!(plan.diff.removals.len(), 1);
        let steps = std::cell::Cell::new(0);
        let after_one_step = || {
            steps.set(steps.get() + 1);
            (steps.get() > 1).then_some(2)
        };
        let mut err = Vec::new();
        let applied = apply_backed_up(&root, &plan, &mut err, &after_one_step).unwrap();
        assert!(matches!(applied, Applied::Interrupted(2)));
        assert!(
            String::from_utf8(err)
                .unwrap()
                .contains("Import interrupted; restored")
        );
        let read = |rel: &str| std::fs::read_to_string(project.path().join(rel)).unwrap();
        assert_eq!(read(".ai/src/skills/jury/SKILL.md"), "# Old\n");
        assert_eq!(read(".ai/src/skills/jury/old.md"), "# Dropped\n");
    }

    /// The four-file project of `tiny_probe.sh`.
    #[cfg(unix)]
    fn tiny_project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        write(dir.path(), ".ai/src/AGENTS.md", "# Agents\n");
        write(dir.path(), ".ai/src/rules/a.md", "# A rule\n");
        write(dir.path(), ".ai/src/skills/x/SKILL.md", "# Skill\n");
        write(dir.path(), "custom/cmds/c.md", "# Command\n");
        write(
            dir.path(),
            ".ai/exuno.yaml",
            "source:\n  commands: custom/cmds\n",
        );
        (dir, root)
    }

    #[cfg(unix)]
    #[test]
    fn sources_are_resolved_like_resolve_source_paths() {
        let (dir, root) = tiny_project();
        let sources = resolve_sources(&root);
        assert_eq!(sources.base, ".ai/src");
        assert_eq!(sources.agents, ".ai/src/AGENTS.md");
        assert_eq!(
            sources.dirs,
            vec![
                ("rules", ".ai/src/rules".to_string()),
                ("skills", ".ai/src/skills".to_string()),
                ("commands", "custom/cmds".to_string()),
                ("agents", String::new()),
                ("settings", String::new()),
                ("mcp", String::new()),
                ("hooks", String::new()),
                ("tools", String::new()),
            ]
        );
        let legacy = tempfile::tempdir().unwrap();
        write(legacy.path(), ".ai/AGENTS.md", "# Old\n");
        write(legacy.path(), ".ai/rules/r.md", "# R\n");
        let sources = resolve_sources(&legacy.path().disk_text());
        assert_eq!(sources.base, ".ai");
        assert_eq!(sources.agents, ".ai/AGENTS.md");
        assert_eq!(sources.dirs[0], ("rules", ".ai/rules".to_string()));
        assert_eq!(
            resolve_sources(&dir.path().join("custom").disk_text()).base,
            ""
        );
    }

    #[cfg(unix)]
    fn run_export(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = export(&args, root, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    fn run_import(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut read_line = || String::new();
        let mut env = Env {
            cwd: root.to_string(),
            interactive: false,
            read_line: &mut read_line,
        };
        let status = import(&args, root, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_dry_run_export_lists_the_sources_like_cmd_export() {
        let (_dir, root) = tiny_project();
        let (status, out, err) = run_export(&root, &["--dry-run"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "\n  Exuno Export\n\n  Contents:\n    • AGENTS.md\n    • rules/ (1 files)\n    • skills/ (1 files)\n    • commands/ (1 files)\n    • exuno.yaml\n\n  Dry run — no files written.\n  Would create: {root}/exuno-bundle.tar.gz\n\n"
            )
        );
        let (status, out, err) = run_export(&root, &["--bogus"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "Error: Unknown option: --bogus\n{}",
                EXPORT_HELP.render(&Style::plain())
            )
        );
        let (status, _, err) = run_export(&root, &["-o"]);
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: --output requires a path\n")
        );
        let empty = tempfile::tempdir().unwrap();
        let empty_root = empty.path().disk_text();
        let (status, _, err) = run_export(&empty_root, &[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            format!("Error: No .ai/ directory found in {empty_root}\nRun exuno init first.\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_imported_config_drops_its_source_paths() {
        let (_source, source_root) = tiny_project();
        write(
            Path::new(&source_root),
            ".ai/exuno.yaml",
            "tools:\n  enabled: [claude]\n\nsource:\n  commands: custom/cmds\n",
        );
        let target = tempfile::tempdir().unwrap();
        let target_root = std::fs::canonicalize(target.path()).unwrap().disk_text();
        let (status, _, err) = run_import(&target_root, &[&source_root]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            std::fs::read_to_string(target.path().join(".ai/exuno.yaml")).unwrap(),
            "tools:\n  enabled: [claude]\n\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_import_copies_then_reports_up_to_date_like_cmd_import() {
        let (_source, source_root) = tiny_project();
        let target = tempfile::tempdir().unwrap();
        let target_root = std::fs::canonicalize(target.path()).unwrap().disk_text();
        let (status, out, err) = run_import(&target_root, &[&source_root]);
        assert_eq!((status, err.as_str()), (0, ""));
        let backup_id = crate::paths::leaf(&backup::latest(&target_root).unwrap().unwrap());
        assert_eq!(
            out,
            format!(
                "\n  Exuno Import\n\n  Reading from {source_root}...\n  Source: Directory: {source_root}\n\n  Changes:\n    + AGENTS.md (new)\n    ↳ rules (1 new)\n    ↳ skills (1 new)\n    ↳ commands (1 new)\n\n  Summary: 4 new, 0 updated, 0 unchanged\n\n  Imported! 4 new, 0 updated files.\n  Backup: .ai/backups/{backup_id} — undo with exuno rollback\n\n  Next steps:\n    1. Review imported files in .ai/src\n    2. Run exuno sync to distribute to all tools\n\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join(".ai/src/skills/x/SKILL.md")).unwrap(),
            "# Skill\n"
        );
        assert_eq!(
            std::fs::read_to_string(target.path().join(".ai/src/commands/c.md")).unwrap(),
            "# Command\n"
        );
        assert!(!target.path().join(".ai/exuno.yaml").exists());
        let (status, out, _) = run_import(&target_root, &[&source_root]);
        assert_eq!(status, 0);
        assert!(out.ends_with("  Source: Directory: {source_root}\n\n  Already up to date! Nothing to import.\n\n".replace("{source_root}", &source_root).as_str()));
        write(
            Path::new(&source_root),
            ".ai/src/rules/a.md",
            "# A rule, edited\n",
        );
        write(Path::new(&source_root), ".ai/src/rules/b.md", "# B\n");
        let (status, out, _) = run_import(
            &target_root,
            &[&source_root, "--only", "rules", "--dry-run"],
        );
        assert_eq!(status, 0);
        assert!(out.ends_with(
            "  Changes:\n    ↳ rules (1 new, 1 updated)\n\n  Summary: 1 new, 1 updated, 0 unchanged\n\n  Dry run — no files written.\n\n"
        ));
        let (status, out, err) = run_import(&target_root, &["nothing.txt"]);
        assert_eq!((status, out.as_str()), (1, "\n  Exuno Import\n\n"));
        assert_eq!(
            err,
            "  Error: Cannot recognize source: nothing.txt\n  Expected: git URL, archive (.tar.gz, .zip, .skill), or directory path.\n"
        );
    }
}
