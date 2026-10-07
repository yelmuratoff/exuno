//! `exuno dedupe`: `cmd_dedupe` of `lib/helpers/dedupe.sh`, which deletes
//! source files a parent `.ai/src/` already holds byte for byte.

use crate::paths::DiskText;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use super::put;
use crate::config::project_config::{self, Selection};
use crate::engine::{skill_tree, workspace::Workspace};
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::{
    Error, config::catalog, config::template_manifest, config::yaml_edit, engine::overlay, paths,
};

type Answer<'a> = &'a mut dyn FnMut() -> String;

struct Run<'a> {
    style: &'a Style,
    assume_yes: bool,
    templates: Vec<String>,
    explicit_config: Option<&'a str>,
    project_config: Option<String>,
    terminal: Option<Answer<'a>>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

/// Where `dedupe` runs: the working directory, the project root, and the
/// `AGENTSYNC_CONFIG_PATH` value each project resolves its config by.
pub struct Place<'a> {
    pub cwd: &'a str,
    pub root: &'a dyn Fn() -> Result<String, Error>,
    pub config: Option<&'a str>,
}

pub fn dedupe<'a>(
    args: &[String],
    place: &Place<'a>,
    style: &'a Style,
    terminal: Option<Answer<'a>>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
) -> Result<u8, Error> {
    let parsed = match parse_dedupe(args) {
        Ok(parsed) => parsed,
        Err(DedupeStop::Help) => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Err(DedupeStop::Refuse { message, with_help }) => {
            let help = if with_help {
                HELP.render(style)
            } else {
                String::new()
            };
            put(
                err,
                format!("{}: {message}\n{help}", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        }
    };
    if !parsed.assume_yes && terminal.is_none() {
        put(
            err,
            format!(
                "{}: dedupe needs an interactive TTY (or pass {}).\n",
                style.red("Error"),
                style.cyan("--yes")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let mut run = Run {
        style,
        assume_yes: parsed.assume_yes,
        templates: catalog::template_sources(),
        explicit_config: place.config,
        project_config: None,
        terminal,
        out,
        err,
    };
    put(
        run.out,
        format!("\n{}\n", style.bold("  Exuno Dedupe")).as_bytes(),
    )?;
    if parsed.workspace {
        return run_workspace(&mut run, place.cwd);
    }
    let repo_root = (place.root)()?;
    run_one(&mut run, &repo_root, &parsed.against, place.cwd)
}

struct DedupeArgs {
    against: String,
    workspace: bool,
    assume_yes: bool,
}

/// How a command line that dedupes nothing ends: help on stdout, or an error
/// line on stderr followed by the help when `with_help`.
enum DedupeStop {
    Help,
    Refuse { message: String, with_help: bool },
}

fn parse_dedupe(args: &[String]) -> Result<DedupeArgs, DedupeStop> {
    let refuse = |message: String, with_help: bool| DedupeStop::Refuse { message, with_help };
    let mut parsed = DedupeArgs {
        against: String::new(),
        workspace: false,
        assume_yes: false,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--against" => match rest.next() {
                Some(path) => parsed.against = path.clone(),
                None => return Err(refuse("--against requires a path".into(), false)),
            },
            "--workspace" => parsed.workspace = true,
            "--yes" | "-y" => parsed.assume_yes = true,
            "--help" | "-h" => return Err(DedupeStop::Help),
            flag if flag.starts_with("--against=") => {
                parsed.against = flag["--against=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                return Err(refuse(format!("Unknown option: {flag}"), true));
            }
            value => return Err(refuse(format!("Unexpected argument: {value}"), true)),
        }
    }
    if parsed.workspace && !parsed.against.is_empty() {
        return Err(refuse(
            "--workspace and --against are mutually exclusive.".into(),
            false,
        ));
    }
    Ok(parsed)
}

/// `--workspace`: every project below `cwd`, stopping at the first failure.
fn run_workspace(run: &mut Run, cwd: &str) -> Result<u8, Error> {
    let style = run.style;
    let ai_dirs = paths::find_workspace_ai_dirs(cwd);
    if ai_dirs.is_empty() {
        put(
            run.err,
            format!(
                "  {}: No .ai/ directories found below {cwd}\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    put(
        run.out,
        format!("  Found {} project(s) below {cwd}\n\n", ai_dirs.len()).as_bytes(),
    )?;
    for ai_dir in &ai_dirs {
        let project_root = paths::parent(ai_dir);
        let rel = if project_root == cwd {
            "."
        } else {
            project_root
                .strip_prefix(&format!("{cwd}/"))
                .unwrap_or(&project_root)
        };
        put(run.out, format!("  {} {rel}\n", style.cyan("→")).as_bytes())?;
        let status = run_one(run, &project_root, "", cwd)?;
        if status != 0 {
            return Ok(status);
        }
        put(run.out, b"\n")?;
    }
    Ok(0)
}

#[derive(Default)]
struct Tally {
    deleted: usize,
    kept: usize,
    skipped: usize,
    cancelled: bool,
}

/// `_dedupe_run_one`.
fn run_one(run: &mut Run, repo_root: &str, against: &str, cwd: &str) -> Result<u8, Error> {
    let style = run.style;
    let child_src = format!("{repo_root}/.ai/src");
    if !Path::new(&child_src).is_dir() {
        put(
            run.out,
            format!("  {}: no .ai/src/ in {repo_root}\n", style.yellow("skip")).as_bytes(),
        )?;
        return Ok(0);
    }
    let is_file = |path: &str| Path::new(path).is_file();
    run.project_config = match project_config::select(repo_root, run.explicit_config, &is_file) {
        Selection::Found(path) => Some(path),
        Selection::None => None,
        Selection::Missing(path) => {
            let message = project_config::missing_message(&path);
            put(
                run.err,
                format!("{}: {message}\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        }
    };
    let (parent_src, from_shared) = match parent_for(run, repo_root, against, cwd)? {
        Ok(Some(found)) => found,
        Ok(None) => {
            let text = format!(
                "  {} no parent .ai/src/ found for {repo_root}\n",
                style.dim("·")
            );
            put(run.out, text.as_bytes())?;
            return Ok(0);
        }
        Err(status) => return Ok(status),
    };
    let (identical, divergent) = collect(&child_src, &parent_src);
    if identical.is_empty() && divergent.is_empty() {
        let text = format!(
            "  {} nothing shared with parent {parent_src}\n",
            style.green("✓")
        );
        put(run.out, text.as_bytes())?;
        return Ok(0);
    }
    let origin_hint = if from_shared {
        format!(" {}", style.dim("(from shared.path)"))
    } else {
        String::new()
    };
    put(
        run.out,
        format!(
            "  {} {parent_src}{origin_hint}\n  {} {}  {} {}\n\n",
            style.dim("Parent:"),
            style.dim("Identical:"),
            identical.len(),
            style.dim("Divergent:"),
            divergent.len()
        )
        .as_bytes(),
    )?;
    let mut tally = Tally::default();
    resolve_identical(run, &identical, repo_root, &mut tally)?;
    resolve_divergent(run, &divergent, &mut tally)?;
    put(run.out, b"\n")?;
    if tally.cancelled {
        put(
            run.out,
            format!(
                "  {} Decisions already made are kept.\n",
                style.yellow("Cancelled.")
            )
            .as_bytes(),
        )?;
    }
    let Tally {
        deleted,
        kept,
        skipped,
        ..
    } = tally;
    put(
        run.out,
        format!(
            "  {} Deleted: {deleted} · Kept: {kept} · Skipped: {skipped}\n",
            style.green("Done.")
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// The parent `.ai/src/` to compare against, and whether `shared.path` named
/// it: `--against`, else `shared.path`, else the nearest parent.
fn parent_for(
    run: &mut Run,
    repo_root: &str,
    against: &str,
    cwd: &str,
) -> Result<Result<Option<(String, bool)>, u8>, Error> {
    let logical = |path: &str| {
        if crate::paths::is_absolute(path) {
            paths::normalize(path)
        } else {
            paths::normalize(&format!("{cwd}/{path}"))
        }
    };
    if against.is_empty() {
        let shared = run
            .project_config
            .as_ref()
            .and_then(|path| std::fs::read(path).ok())
            .and_then(|bytes| {
                overlay::shared_parent_src(&String::from_utf8_lossy(&bytes), repo_root)
            });
        return Ok(Ok(match shared {
            Some(parent) => Some((parent, true)),
            None => paths::find_parent_ai_src(repo_root).map(|parent| (parent, false)),
        }));
    }
    if Path::new(&format!("{against}/.ai/src")).is_dir() {
        return Ok(Ok(Some((logical(&format!("{against}/.ai/src")), false))));
    }
    if Path::new(against).is_dir() && paths::leaf(against) == "src" {
        return Ok(Ok(Some((logical(against), false))));
    }
    let problem = if Path::new(against).is_dir() {
        "has no .ai/src/"
    } else {
        "does not exist"
    };
    put(
        run.err,
        format!(
            "{}: --against path {problem}: {against}\n",
            run.style.red("Error")
        )
        .as_bytes(),
    )?;
    Ok(Err(1))
}

/// Deletes each identical copy by `--yes` or by answer, declining a template
/// the project config can record.
fn resolve_identical(
    run: &mut Run,
    identical: &Identical,
    repo_root: &str,
    tally: &mut Tally,
) -> Result<(), Error> {
    let style = run.style;
    let child_src = format!("{repo_root}/.ai/src");
    let config = run.project_config.clone();
    let decline = |rel: &str| match &config {
        Some(config) => {
            yaml_edit::list_append(Path::new(config), "template_overrides.declined", rel)
                .map(|()| true)
        }
        None => Ok(false),
    };
    for (rel, child_file) in identical {
        if tally.cancelled {
            break;
        }
        let is_template = run.templates.iter().any(|path| path == rel);
        if run.assume_yes {
            delete_and_prune(child_file, &child_src)?;
            if is_template {
                decline(rel)?;
            }
            let line = format!("  {} {rel} {}\n", style.green("−"), style.dim("(deleted)"));
            put(run.out, line.as_bytes())?;
            tally.deleted += 1;
            continue;
        }
        match prompt_identical(run, rel, child_file, is_template)? {
            Choice::Delete => {
                delete_and_prune(child_file, &child_src)?;
                let declined = is_template && decline(rel)?;
                let note = if declined {
                    "deleted + declined."
                } else {
                    "deleted."
                };
                put(run.out, format!("    {}\n", style.green(note)).as_bytes())?;
                tally.deleted += 1;
            }
            Choice::Keep => {
                put(run.out, format!("    {}\n", style.dim("kept.")).as_bytes())?;
                tally.kept += 1;
            }
            Choice::Quit => tally.cancelled = true,
        }
    }
    Ok(())
}

/// Skips each divergent copy, showing its diff on request; nothing is deleted.
fn resolve_divergent(run: &mut Run, divergent: &Divergent, tally: &mut Tally) -> Result<(), Error> {
    let style = run.style;
    for (rel, child_file, parent_file) in divergent {
        if tally.cancelled {
            break;
        }
        if run.assume_yes {
            let note = style.dim("(divergent — skipped under --yes; review interactively)");
            put(
                run.out,
                format!("  {} {rel} {note}\n", style.yellow("~")).as_bytes(),
            )?;
            tally.skipped += 1;
            continue;
        }
        if prompt_diverge(run, rel, child_file, parent_file)? {
            tally.cancelled = true;
        } else {
            put(
                run.out,
                format!("    {}\n", style.dim("skipped.")).as_bytes(),
            )?;
            tally.skipped += 1;
        }
    }
    Ok(())
}

type Identical = Vec<(String, String)>;
type Divergent = Vec<(String, String, String)>;

/// `_dedupe_collect`: the flat categories' `*.md` in byte order, then every
/// file below `skills` not named `.*`, in byte order, links not followed.
fn collect(child_src: &str, parent_src: &str) -> (Identical, Divergent) {
    let mut parent_files = Vec::new();
    for category in ["rules", "commands", "agents"] {
        let dir = format!("{parent_src}/{category}");
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().disk_text())
            .filter(|name| name.ends_with(".md") && !name.starts_with('.'))
            .collect();
        names.sort();
        parent_files.extend(
            names
                .into_iter()
                .filter(|name| Path::new(&format!("{dir}/{name}")).is_file())
                .map(|name| format!("{category}/{name}")),
        );
    }
    let mut skills = Vec::new();
    walk_files(&format!("{parent_src}/skills"), &mut skills);
    skills.sort();
    parent_files.extend(skills.into_iter().filter_map(|path| {
        path.strip_prefix(&format!("{parent_src}/"))
            .map(str::to_string)
    }));

    let child_skills = skill_tree::discover(
        &Workspace::on_disk(child_src),
        &format!("{child_src}/skills"),
    );
    let (mut identical, mut divergent) = (Vec::new(), Vec::new());
    for rel in parent_files {
        let child_file = match rel.strip_prefix("skills/") {
            Some(inside) => format!("{child_src}/skills/{}", child_skills.locate(inside)),
            None => format!("{child_src}/{rel}"),
        };
        let parent_file = format!("{parent_src}/{rel}");
        if !Path::new(&child_file).is_file() {
            continue;
        }
        let (Some(child_hash), Some(parent_hash)) = (
            template_manifest::hash(Path::new(&child_file)),
            template_manifest::hash(Path::new(&parent_file)),
        ) else {
            continue;
        };
        if child_hash == parent_hash {
            identical.push((rel, child_file));
        } else {
            divergent.push((rel, child_file, parent_file));
        }
    }
    (identical, divergent)
}

/// `find <dir> -type f ! -name '.*'`.
fn walk_files(dir: &str, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let name = entry.file_name().disk_text();
        let path = format!("{dir}/{name}");
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            walk_files(&path, found);
        } else if meta.is_file() && !name.starts_with('.') {
            found.push(path);
        }
    }
}

/// `_dedupe_delete_and_prune`: `rm -f`, then `rmdir` up to the top-level
/// directory under `src` that holds the file, such as `rules/` or `skills/`.
fn delete_and_prune(file: &str, src: &str) -> Result<(), Error> {
    match std::fs::remove_file(file) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(Error::io(file, e)),
        _ => {}
    }
    let stop_at = file
        .strip_prefix(&format!("{src}/"))
        .and_then(|rel| rel.split_once('/'))
        .map_or_else(|| src.to_string(), |(top, _)| format!("{src}/{top}"));
    let mut dir = paths::parent(file);
    // `dir != "/"` alone names a root Windows does not have; stop at whatever
    // the platform's root is, which is the path that is its own parent.
    while dir != stop_at {
        if std::fs::remove_dir(&dir).is_err() {
            break;
        }
        let up = paths::parent(&dir);
        if up == dir {
            break;
        }
        dir = up;
    }
    Ok(())
}

enum Choice {
    Delete,
    Keep,
    Quit,
}

/// `read -r reply </dev/tty`, lowercased, with the prompt's default.
fn reply(run: &mut Run, default: &str) -> String {
    let reply = run
        .terminal
        .as_mut()
        .map(|answer| answer())
        .unwrap_or_default()
        .trim_matches([' ', '\t'])
        .to_lowercase();
    if reply.is_empty() {
        default.to_string()
    } else {
        reply
    }
}

/// `_dedupe_prompt_identical`.
fn prompt_identical(
    run: &mut Run,
    rel: &str,
    child_file: &str,
    is_template: bool,
) -> Result<Choice, Error> {
    let style = run.style;
    let hint = if is_template {
        style.dim("(template-derived — declined entry will be added)")
    } else {
        String::new()
    };
    loop {
        put(
            run.err,
            format!(
                "\n  {} {}  {hint}\n    [{}]elete  [{}]eep  [v]iew  [q]uit  > ",
                style.yellow("= IDENTICAL:"),
                style.cyan(rel),
                style.green("d"),
                style.yellow("k")
            )
            .as_bytes(),
        )?;
        match reply(run, "k").as_str() {
            "d" | "delete" => return Ok(Choice::Delete),
            "k" | "keep" => return Ok(Choice::Keep),
            "v" | "view" => show_head(run, child_file)?,
            "q" | "quit" => return Ok(Choice::Quit),
            _ => put(
                run.err,
                format!("    {}\n", style.dim("(unknown choice — try d, k, v, q)")).as_bytes(),
            )?,
        }
    }
}

/// `_dedupe_prompt_diverge`: whether the answer was to quit.
fn prompt_diverge(
    run: &mut Run,
    rel: &str,
    child_file: &str,
    parent_file: &str,
) -> Result<bool, Error> {
    let style = run.style;
    loop {
        put(
            run.err,
            format!(
                "\n  {} {}\n    [{}]iew  [{}]kip  [q]uit  > ",
                style.yellow("~ DIVERGES:"),
                style.cyan(rel),
                style.yellow("v"),
                style.yellow("s")
            )
            .as_bytes(),
        )?;
        match reply(run, "s").as_str() {
            "v" | "view" => show_diff(run, child_file, parent_file)?,
            "s" | "skip" => return Ok(false),
            "q" | "quit" => return Ok(true),
            _ => put(
                run.err,
                format!("    {}\n", style.dim("(unknown choice — try v, s, q)")).as_bytes(),
            )?,
        }
    }
}

/// `_dedupe_show_head`: the first 20 lines, each indented.
fn show_head(run: &mut Run, file: &str) -> Result<(), Error> {
    let style = run.style;
    let mut text = format!(
        "\n  {}\n\n",
        style.dim("──── content (first 20 lines) ────")
    );
    let content = std::fs::read(file)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    for (index, line) in content.split_inclusive('\n').enumerate() {
        if index == 20 {
            text.push_str(&format!("  {}\n", style.dim("  …")));
            break;
        }
        text.push_str(&format!("  {}\n", line.strip_suffix('\n').unwrap_or(line)));
    }
    text.push('\n');
    put(run.err, text.as_bytes())
}

/// `_dedupe_show_diff`: `diff -u --label yours --label parent`, on stderr.
fn show_diff(run: &mut Run, child_file: &str, parent_file: &str) -> Result<(), Error> {
    let style = run.style;
    put(
        run.err,
        format!("\n  {}\n\n", style.dim("──── diff: yours → parent ────")).as_bytes(),
    )?;
    match Command::new("diff")
        .args([
            "-u",
            "--label",
            "yours",
            "--label",
            "parent",
            child_file,
            parent_file,
        ])
        .output()
    {
        Ok(output) => {
            put(run.err, &output.stdout)?;
            put(run.err, &output.stderr)?;
        }
        Err(_) => put(
            run.err,
            format!("  {}\n", style.red("(diff command not available)")).as_bytes(),
        )?,
    }
    put(run.err, b"\n")
}

pub const HELP: Help = Help {
    command: "dedupe",
    tagline: "remove source files that duplicate a parent .ai/src/",
    synopsis: &["dedupe [OPTIONS]"],
    description: &[
        "For each path that exists in both your .ai/src/ and the parent, the\nfile's hash decides what is offered (see BEHAVIOR).",
        "Identical-hash deletions also prune empty parent directories so empty\nskill folders don't linger after their SKILL.md is removed.",
    ],
    sections: &[
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--against <path>",
                    "Compare against an explicit .ai/src/ (or a project\nroot containing one). Default: nearest parent\n.ai/src/ walking up from cwd, bounded by the git\nrepository boundary.",
                ),
                (
                    "--workspace",
                    "Run dedupe in every .ai/ below cwd, bottom-up\nalphabetical; each child is deduped against its\nown nearest parent.",
                ),
                (
                    "-y, --yes",
                    "Non-interactive: delete every identical-hash file\n(and add to template_overrides.declined when the\nfile is template-derived). Divergent files are\nalways left alone — pass --yes does not auto-pick\na side. Required when stdin is not a TTY.",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
        Section {
            title: "BEHAVIOR",
            entries: &[
                (
                    "identical hash, template-derived",
                    "offer delete + declined entry",
                ),
                ("identical hash, manual file", "offer delete only"),
                ("different hash", "show diff, skip by default"),
            ],
        },
    ],
    examples: &[
        "dedupe",
        "dedupe --against ../",
        "dedupe --workspace",
        "dedupe --yes",
    ],
};

#[cfg(test)]
mod help_tests {
    use super::*;

    #[test]
    fn help_has_the_shared_shape() {
        assert_eq!(
            HELP.render(&Style::plain()),
            "\n  exuno dedupe — remove source files that duplicate a parent .ai/src/\n\n  USAGE\n    exuno dedupe [OPTIONS]\n\n  DESCRIPTION\n    For each path that exists in both your .ai/src/ and the parent, the\n    file's hash decides what is offered (see BEHAVIOR).\n\n    Identical-hash deletions also prune empty parent directories so empty\n    skill folders don't linger after their SKILL.md is removed.\n\n  OPTIONS\n    --against <path>   Compare against an explicit .ai/src/ (or a project\n                       root containing one). Default: nearest parent\n                       .ai/src/ walking up from cwd, bounded by the git\n                       repository boundary.\n    --workspace        Run dedupe in every .ai/ below cwd, bottom-up\n                       alphabetical; each child is deduped against its\n                       own nearest parent.\n    -y, --yes          Non-interactive: delete every identical-hash file\n                       (and add to template_overrides.declined when the\n                       file is template-derived). Divergent files are\n                       always left alone — pass --yes does not auto-pick\n                       a side. Required when stdin is not a TTY.\n    -h, --help         Show this help\n\n  BEHAVIOR\n    identical hash, template-derived   offer delete + declined entry\n    identical hash, manual file        offer delete only\n    different hash                     show diff, skip by default\n\n  EXAMPLES\n    exuno dedupe\n    exuno dedupe --against ../\n    exuno dedupe --workspace\n    exuno dedupe --yes\n\n"
        );
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    struct Fixture {
        _dir: tempfile::TempDir,
        parent: String,
        child: String,
    }

    fn fixture(files: &[(&str, &str, &str)]) -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        let parent = format!("{root}/p");
        let child = format!("{parent}/child");
        std::fs::create_dir_all(format!("{parent}/.git")).unwrap();
        std::fs::create_dir_all(format!("{child}/.ai/src")).unwrap();
        std::fs::write(
            format!("{child}/.ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        for (rel, in_parent, in_child) in files {
            for (base, text) in [(&parent, in_parent), (&child, in_child)] {
                let path = format!("{base}/.ai/src/{rel}");
                std::fs::create_dir_all(paths::parent(&path)).unwrap();
                std::fs::write(path, text).unwrap();
            }
        }
        Fixture {
            _dir: dir,
            parent,
            child,
        }
    }

    fn comments_template() -> String {
        let (_, bytes) = catalog::engine_files()
            .into_iter()
            .find(|(path, _)| path == "lib/templates/rules/comments.md")
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    fn call(fx: &Fixture, args: &[&str], answers: Option<&[&str]>) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let child = fx.child.clone();
        let root = move || Ok(child.clone());
        let mut replies = answers.unwrap_or_default().iter().map(|a| a.to_string());
        let mut answer = move || replies.next().unwrap_or_default();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let place = Place {
            cwd: &fx.child,
            root: &root,
            config: None,
        };
        let status = dedupe(
            &args,
            &place,
            &Style::plain(),
            answers.map(|_| &mut answer as &mut dyn FnMut() -> String),
            &mut out,
            &mut err,
        )
        .unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn yes_deletes_identical_files_declines_templates_and_prunes_like_bash() {
        let comments = comments_template();
        let fx = fixture(&[
            ("rules/comments.md", &comments, &comments),
            ("rules/shared.md", "shared rule\n", "shared rule\n"),
            ("rules/diverge.md", "parent\n", "child\n"),
            ("skills/foo/SKILL.md", "skill\n", "skill\n"),
            ("skills/foo/ref/a.md", "ref\n", "ref\n"),
            ("skills/foo/.x", ".x\n", ".x\n"),
        ]);
        let (status, out, err) = call(&fx, &["--yes"], None);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "\n  Exuno Dedupe\n  Parent: {}/.ai/src\n  Identical: 4  Divergent: 1\n\n  − rules/comments.md (deleted)\n  − rules/shared.md (deleted)\n  − skills/foo/SKILL.md (deleted)\n  − skills/foo/ref/a.md (deleted)\n  ~ rules/diverge.md (divergent — skipped under --yes; review interactively)\n\n  Done. Deleted: 4 · Kept: 0 · Skipped: 1\n",
                fx.parent
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{}/.ai/agent_sync.yaml", fx.child)).unwrap(),
            "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n"
        );
        let src = format!("{}/.ai/src", fx.child);
        assert!(Path::new(&format!("{src}/rules/diverge.md")).is_file());
        assert!(Path::new(&format!("{src}/skills/foo/.x")).is_file());
        assert!(!Path::new(&format!("{src}/skills/foo/ref")).exists());
        assert!(!Path::new(&format!("{src}/rules/shared.md")).exists());
    }

    #[test]
    fn pruning_stops_at_the_top_level_source_directory() {
        let fx = fixture(&[
            ("rules/only.md", "rule\n", "rule\n"),
            ("skills/flutter/bloc/SKILL.md", "skill\n", "skill\n"),
        ]);
        let (status, _, err) = call(&fx, &["--yes"], None);
        assert_eq!((status, err.as_str()), (0, ""));
        let src = format!("{}/.ai/src", fx.child);
        assert!(Path::new(&format!("{src}/rules")).is_dir());
        assert!(Path::new(&format!("{src}/skills")).is_dir());
        assert!(!Path::new(&format!("{src}/skills/flutter")).exists());
    }

    #[test]
    fn answers_on_the_terminal_view_delete_keep_skip_and_quit_like_bash() {
        let comments = comments_template();
        let fx = fixture(&[
            ("rules/comments.md", &comments, &comments),
            ("rules/keep.md", "one\ntwo\n", "one\ntwo\n"),
            ("rules/diverge.md", "parent\n", "child\n"),
            ("rules/zz.md", "x\n", "y\n"),
        ]);
        let answers = [" V ", "x", "D", "", "v", "s", "q"];
        let (status, out, err) = call(&fx, &[], Some(&answers));
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  Exuno Dedupe\n  Parent: {}/.ai/src\n  Identical: 2  Divergent: 2\n\n    deleted + declined.\n    kept.\n    skipped.\n\n  Cancelled. Decisions already made are kept.\n  Done. Deleted: 1 · Kept: 1 · Skipped: 1\n",
                fx.parent
            )
        );
        let identical = "\n  = IDENTICAL: rules/comments.md  (template-derived — declined entry will be added)\n    [d]elete  [k]eep  [v]iew  [q]uit  > ";
        let head: String = comments
            .split_inclusive('\n')
            .take(20)
            .map(|line| format!("  {line}"))
            .collect();
        assert_eq!(
            err,
            format!(
                "{identical}\n  ──── content (first 20 lines) ────\n\n{head}    …\n\n{identical}    (unknown choice — try d, k, v, q)\n{identical}\n  = IDENTICAL: rules/keep.md  \n    [d]elete  [k]eep  [v]iew  [q]uit  > \n  ~ DIVERGES: rules/diverge.md\n    [v]iew  [s]kip  [q]uit  > \n  ──── diff: yours → parent ────\n\n--- yours\n+++ parent\n@@ -1 +1 @@\n-child\n+parent\n\n\n  ~ DIVERGES: rules/diverge.md\n    [v]iew  [s]kip  [q]uit  > \n  ~ DIVERGES: rules/zz.md\n    [v]iew  [s]kip  [q]uit  > "
            )
        );
        assert!(!Path::new(&format!("{}/.ai/src/rules/comments.md", fx.child)).exists());
        assert!(Path::new(&format!("{}/.ai/src/rules/keep.md", fx.child)).is_file());
    }

    #[test]
    fn arguments_and_setup_errors_are_refused_with_the_bash_statuses() {
        let fx = fixture(&[]);
        let (status, out, err) = call(&fx, &["--bogus"], None);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "Error: Unknown option: --bogus\n{}",
                HELP.render(&Style::plain())
            )
        );
        assert_eq!(
            call(&fx, &["--against"], None),
            (
                1,
                String::new(),
                "Error: --against requires a path\n".to_string()
            )
        );
        assert_eq!(
            call(&fx, &["--workspace", "--against", "x"], None),
            (
                1,
                String::new(),
                "Error: --workspace and --against are mutually exclusive.\n".to_string()
            )
        );
        assert_eq!(
            call(&fx, &[], None),
            (
                1,
                String::new(),
                "Error: dedupe needs an interactive TTY (or pass --yes).\n".to_string()
            )
        );
        let missing = format!("{}/nope", fx.parent);
        assert_eq!(
            call(&fx, &["--against", &missing, "-y"], None),
            (
                1,
                "\n  Exuno Dedupe\n".to_string(),
                format!("Error: --against path does not exist: {missing}\n")
            )
        );
        assert_eq!(
            call(&fx, &["-y"], None).1,
            format!(
                "\n  Exuno Dedupe\n  · no parent .ai/src/ found for {}\n",
                fx.child
            )
        );
        std::fs::create_dir_all(format!("{}/.ai/src", fx.parent)).unwrap();
        assert_eq!(
            call(&fx, &["-y"], None).1,
            format!(
                "\n  Exuno Dedupe\n  ✓ nothing shared with parent {}/.ai/src\n",
                fx.parent
            )
        );
    }
}
