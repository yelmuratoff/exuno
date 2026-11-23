//! `exuno memory`: read the notes coding agents keep for themselves, wherever
//! each tool's `memory:` block says they live. Read-only.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::Path;

use crate::Error;
use crate::config::catalog;
use crate::config::tool::Tool;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::paths::DiskText;
use crate::project::Project;

use super::put;
use super::skills::cell;

mod content;
mod similar;
mod store;

use store::{Note, Place, Spec, Store};

pub const HELP: Help = Help {
    command: "memory",
    tagline: "inspect the memory coding agents keep for themselves",
    synopsis: &[
        "memory list [--all] [--json]",
        "memory check [--all] [--stale-days <n>] [--json]",
        "memory review [--all] [--stale-days <n>]",
        "memory promote <note> --rule <name> [--dry-run]",
    ],
    description: &[
        "Reads the memory stores each tool's memory: block declares: the\nshipped tools and any .ai/src/tools/<tool>.yaml, profile variants in\ntheir config homes included. Without --all, only this project's store\nand the stores every project shares. list, check, and review change no\nfile.",
        "check reports an index over the tool's size limit, notes the index\ndoes not list, index links to missing notes, a type outside the\ntool's list, paths under your home that no longer exist, lines\nshaped like a credential, a note that copies another,\nand a note that copies a file in .ai/src/. With --all it also reports\na note name several projects share. --stale-days adds notes last\nmodified more than <n> days ago. It exits 1 when it finds anything.",
        "review prints the memory-review skill's reviewer brief followed by\nthe stores, the check findings, and the notes, for an agent to judge\nwhat the notes mean: contradictions, stale facts, retold duplicates,\nand lessons to promote.",
        "promote writes .ai/src/rules/<name>.md from a note's body and leaves\nthe note alone; exuno writes only inside the project. --dry-run\nprints the rule instead.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            ("--all", "Read every project's store, not only this one"),
            (
                "--stale-days <n>",
                "check: report notes older than <n> days",
            ),
            ("--json", "list, check: print one JSON value on stdout"),
            ("--rule <name>", "promote: the rule file to create"),
            ("--dry-run", "promote: print the rule, write nothing"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "memory list",
        "memory list --all --json",
        "memory check --all --stale-days 90",
        "memory review --all",
        "memory promote ~/.claude/projects/-repo/memory/release.md --rule release",
    ],
};

/// What `main` supplies from the process.
pub struct Env<'a> {
    pub home: Option<String>,
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// Days since 1970-01-01, UTC.
    pub today: i64,
}

enum Action {
    List,
    Check,
    Review,
    Promote { note: String, rule: String },
}

struct Args {
    action: Action,
    all: bool,
    json: bool,
    stale_days: Option<i64>,
    dry_run: bool,
}

/// The stores found, each with the index of its tool's spec and its notes.
struct Found {
    specs: Vec<Spec>,
    stores: Vec<(usize, Store, Vec<Note>)>,
}

pub fn run(
    args: &[String],
    root: &str,
    env: &Env,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            put(err, format!("Error: {message}\n").as_bytes())?;
            return Ok(1);
        }
    };
    let promoting = matches!(parsed.action, Action::Promote { .. });
    let found = find(root, env, parsed.all || promoting, err)?;
    let shown = |path: &Path| shown(path, env.home.as_deref());
    let context = store::Context {
        stale: parsed.stale_days.map(|days| (env.today, days)),
        root: Path::new(root),
        home: env.home.as_deref().map(Path::new),
    };
    match &parsed.action {
        Action::List => list(&found, parsed.json, &shown, out),
        Action::Check => check(&found, &context, parsed.json, &shown, out),
        Action::Review => review(&found, &context, &shown, out),
        Action::Promote { note, rule } => {
            let request = Promotion {
                note,
                rule,
                dry_run: parsed.dry_run,
            };
            promote(&found, &request, &context, style, out, err)
        }
    }
}

/// What `promote` was asked to do.
struct Promotion<'a> {
    note: &'a str,
    rule: &'a str,
    dry_run: bool,
}

/// `promote`: a rule in `.ai/src/rules/` from a note's body. The note stays
/// as it is; exuno writes only inside the project.
fn promote(
    found: &Found,
    request: &Promotion,
    context: &store::Context,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let refuse = |err: &mut dyn Write, message: String| -> Result<u8, Error> {
        put(
            err,
            format!("{}: {message}\n", style.red("Error")).as_bytes(),
        )?;
        Ok(1)
    };
    if let Some(message) = super::add::validate_name(request.rule) {
        return refuse(err, message);
    }
    let given = match request.note.strip_prefix("~/") {
        Some(rest) => match context.home {
            Some(home) => home.join(rest),
            None => return refuse(err, format!("no home directory to expand {}", request.note)),
        },
        None => context.root.join(request.note),
    };
    let wanted = store::canonical(&given);
    let Some(note) = found
        .stores
        .iter()
        .flat_map(|(_, _, notes)| notes)
        .find(|note| store::canonical(&note.path) == wanted)
    else {
        return refuse(
            err,
            format!(
                "{} is not a note in any memory store exuno reads",
                request.note
            ),
        );
    };
    let rel = format!(".ai/src/rules/{}.md", request.rule);
    let dest = context.root.join(&rel);
    if dest.exists() {
        return refuse(
            err,
            format!("Already exists: {rel}; pick a different name or edit that rule"),
        );
    }
    let content = format!(
        "# {}\n\n{}\n",
        super::add::title(request.rule),
        note.body().trim()
    );
    if request.dry_run {
        let text = format!("Would create rule: {rel}\n\n{content}");
        put(out, text.as_bytes())?;
        return Ok(0);
    }
    let project = Project::configured(context.root)?;
    let (_editing, _) = match super::lock_for_edit(&project, "memory promote", style, err)? {
        Ok(locked) => locked,
        Err(status) => return Ok(status),
    };
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&dest, content).map_err(|e| Error::io(&dest, e))?;
    let text = format!(
        "\n{} {rel}\n\nEdit the rule, then run {} to propagate.\nThe note at {} is unchanged: replace it with one line that points at the rule,\nand keep its line in the store's index in step.\n\n",
        style.green("Created rule:"),
        style.cyan("exuno sync"),
        shown(&note.path, context.home.and_then(Path::to_str)),
    );
    put(out, text.as_bytes())?;
    Ok(0)
}

/// How much memory this project has and how many `check` findings it holds.
pub struct Summary {
    pub notes: usize,
    pub issues: usize,
}

/// `check` for the project at `root` alone, counted, for `doctor`.
pub fn summary(root: &str, env: &Env) -> Result<Summary, Error> {
    let found = find(root, env, false, &mut std::io::sink())?;
    let context = store::Context {
        stale: None,
        root: Path::new(root),
        home: env.home.as_deref().map(Path::new),
    };
    let shown = |path: &Path| shown(path, env.home.as_deref());
    Ok(Summary {
        notes: found.stores.iter().map(|(_, _, notes)| notes.len()).sum(),
        issues: issues(&found, &context, &shown).len(),
    })
}

fn find(root: &str, env: &Env, all: bool, err: &mut dyn Write) -> Result<Found, Error> {
    let project = Project::configured(root)?;
    let place = Place {
        root: Path::new(root),
        home: env.home.as_deref(),
        var: env.var,
    };
    let mut specs = Vec::new();
    for slug in tool_slugs(&project)? {
        specs.extend(Spec::read(&Tool::load(&project, &slug)?, &place));
    }
    let profile_homes: Vec<_> = specs.iter().filter_map(Spec::profile_home).collect();
    for spec in &mut specs {
        spec.leave_profile_homes(&profile_homes);
    }
    let mut stores = Vec::new();
    let mut read_by: BTreeMap<std::path::PathBuf, &str> = BTreeMap::new();
    for (index, spec) in specs.iter().enumerate() {
        let (found, warnings) = store::stores(spec, place.root, all);
        for warning in warnings {
            put(err, format!("Warning: {warning}\n").as_bytes())?;
        }
        for store in found {
            if let Some(other) = read_by.get(&store::canonical(&store.dir)) {
                let warning = format!(
                    "Warning: {}: {} is already read for {other}\n",
                    spec.tool,
                    shown(&store.dir, env.home.as_deref())
                );
                put(err, warning.as_bytes())?;
                continue;
            }
            read_by.insert(store::canonical(&store.dir), &spec.tool);
            let notes = store::notes(&store, &spec.index);
            stores.push((index, store, notes));
        }
    }
    Ok(Found { specs, stores })
}

/// Every shipped tool and every `.ai/src/tools/<slug>.yaml`, in byte order.
fn tool_slugs(project: &Project) -> Result<Vec<String>, Error> {
    let mut slugs: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    slugs.extend(project.user_override_tools()?);
    Ok(slugs.into_iter().collect())
}

fn list(
    found: &Found,
    json: bool,
    shown: &dyn Fn(&Path) -> String,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let rows = found.stores.iter().flat_map(|(spec, store, notes)| {
        notes
            .iter()
            .map(move |note| (&found.specs[*spec].tool, store, note))
    });
    if json {
        print_json(&notes_json(found, shown), out)?;
        return Ok(0);
    }
    let mut text = String::from("tool\tproject\tname\ttype\tmodified\tdescription\tpath\n");
    for (tool, store, note) in rows {
        let cells = [
            tool.as_str(),
            &store.project,
            &note.name,
            &note.kind,
            &note.modified,
            &note.description,
            &shown(&note.path),
        ];
        let cells: Vec<String> = cells.iter().map(|value| cell(value)).collect();
        text.push_str(&cells.join("\t"));
        text.push('\n');
    }
    put(out, text.as_bytes())?;
    Ok(0)
}

fn check(
    found: &Found,
    context: &store::Context,
    json: bool,
    shown: &dyn Fn(&Path) -> String,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let issues = issues(found, context, shown);
    let checked: usize = found.stores.iter().map(|(_, _, notes)| notes.len()).sum();
    if json {
        print_json(&check_json(checked, &issues, shown), out)?;
    } else {
        let mut text = String::new();
        for (path, message) in &issues {
            text.push_str(&format!("{}: {}\n", cell(&shown(path)), cell(message)));
        }
        text.push_str(&format!(
            "Checked {}: {}\n",
            count(checked, "memory", "memories"),
            count(issues.len(), "issue", "issues")
        ));
        put(out, text.as_bytes())?;
    }
    Ok(u8::from(!issues.is_empty()))
}

/// `list --json`: one object per note.
fn notes_json(found: &Found, shown: &dyn Fn(&Path) -> String) -> serde_json::Value {
    let values = found.stores.iter().flat_map(|(spec, store, notes)| {
        notes.iter().map(move |note| {
            serde_json::json!({
                "tool": found.specs[*spec].tool,
                "project": store.project,
                "name": note.name,
                "type": note.kind,
                "modified": note.modified,
                "description": note.description,
                "path": shown(&note.path),
            })
        })
    });
    serde_json::Value::Array(values.collect())
}

/// `check --json`: the count of notes read and every finding.
fn check_json(
    checked: usize,
    issues: &[(std::path::PathBuf, String)],
    shown: &dyn Fn(&Path) -> String,
) -> serde_json::Value {
    let values: Vec<serde_json::Value> = issues
        .iter()
        .map(|(path, message)| serde_json::json!({"path": shown(path), "message": message}))
        .collect();
    serde_json::json!({"checked": checked, "issues": values})
}

/// `review`: the reviewer brief the `memory-review` skill uses, then what it
/// reads, so any agent can run the review from one paste.
fn review(
    found: &Found,
    context: &store::Context,
    shown: &dyn Fn(&Path) -> String,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let issues = issues(found, context, shown);
    let checked: usize = found.stores.iter().map(|(_, _, notes)| notes.len()).sum();
    let mut stores: Vec<String> = found
        .stores
        .iter()
        .map(|(spec, store, _)| format!("- {}: {}", found.specs[*spec].tool, shown(&store.dir)))
        .collect();
    if stores.is_empty() {
        stores.push("- none found".to_string());
    }
    let src = context.root.join(".ai").join("src");
    let rules = if src.is_dir() {
        shown(&src)
    } else {
        "none (no .ai/src/ in this project)".to_string()
    };
    let pretty = |value: &serde_json::Value| {
        serde_json::to_string_pretty(value).expect("a JSON value serializes")
    };
    let text = format!(
        "{}\n\n# This run\n\n## Stores\n\n{}\n\n## Project rules\n\n{rules}\n\n## Findings from exuno memory check\n\n```json\n{}\n```\n\n## Notes\n\n```json\n{}\n```\n",
        catalog::MEMORY_REVIEW_PROMPT.trim_end_matches('\n'),
        stores.join("\n"),
        pretty(&check_json(checked, &issues, shown)),
        pretty(&notes_json(found, shown)),
    );
    put(out, text.as_bytes())?;
    Ok(0)
}

/// Every `check` finding, as `(path, message)`.
fn issues(
    found: &Found,
    context: &store::Context,
    shown: &dyn Fn(&Path) -> String,
) -> Vec<(std::path::PathBuf, String)> {
    let mut issues = Vec::new();
    for (spec, store, notes) in &found.stores {
        issues.extend(store::findings(&found.specs[*spec], store, notes, context));
    }
    issues.extend(shared_names(found));
    issues.extend(overlaps(found, context.root, shown));
    issues
}

/// A note whose name notes in other projects carry too: one rule could hold it.
fn shared_names(found: &Found) -> Vec<(std::path::PathBuf, String)> {
    let mut projects: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
    for (_, store, notes) in &found.stores {
        for note in notes.iter().filter(|note| !note.name.is_empty()) {
            if !store.project.is_empty() {
                projects
                    .entry(&note.name)
                    .or_default()
                    .insert(&store.project);
            }
        }
    }
    let mut issues = Vec::new();
    for (_, store, notes) in &found.stores {
        for note in notes {
            let Some(shared) = projects.get(note.name.as_str()) else {
                continue;
            };
            if shared.len() < 2 || store.project.is_empty() {
                continue;
            }
            let others: Vec<&str> = shared
                .iter()
                .copied()
                .filter(|project| *project != store.project)
                .collect();
            issues.push((
                note.path.clone(),
                format!(
                    "name '{}' is also in {}; consider one shared rule",
                    note.name,
                    others.join(", ")
                ),
            ));
        }
    }
    issues
}

/// Shingle overlap at which two notes read as one. Shingles catch copied
/// text, not a lesson retold in other words; that is the review skill's job.
const DUPLICATE: f64 = 0.5;
/// Share of a note's shingles a source file must hold to have said it already.
const COVERED: f64 = 0.5;

/// Notes that repeat another note, or repeat what `.ai/src/` already says.
fn overlaps(
    found: &Found,
    root: &Path,
    shown: &dyn Fn(&Path) -> String,
) -> Vec<(std::path::PathBuf, String)> {
    let notes: Vec<&Note> = found
        .stores
        .iter()
        .flat_map(|(_, _, notes)| notes)
        .collect();
    let texts: Vec<_> = notes
        .iter()
        .map(|note| similar::shingles(note.body()))
        .collect();
    let mut issues = Vec::new();
    for (earlier, later, score) in similar::similar_pairs(&texts, DUPLICATE) {
        issues.push((
            notes[later].path.clone(),
            format!(
                "reads like {} ({}% shared)",
                shown(&notes[earlier].path),
                (score * 100.0).round()
            ),
        ));
    }
    let sources = rule_sources(root);
    let source_texts: Vec<_> = sources
        .iter()
        .map(|(_, text)| similar::shingles(text))
        .collect();
    for (note, text) in notes.iter().zip(&texts) {
        if let Some((index, score)) = similar::best_container(text, &source_texts, COVERED) {
            issues.push((
                note.path.clone(),
                format!(
                    "already said in {} ({}%)",
                    sources[index].0,
                    (score * 100.0).round()
                ),
            ));
        }
    }
    issues
}

/// The Markdown under `.ai/src/` of the project, as `(shown path, text)`.
fn rule_sources(root: &Path) -> Vec<(String, String)> {
    let src = root.join(".ai").join("src");
    let mut files = Vec::new();
    super::files_below(&src, &mut files);
    files.sort();
    files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter_map(|path| {
            let text = std::fs::read_to_string(&path).ok()?;
            let shown = path.strip_prefix(root).ok()?.disk_text();
            Some((shown, text))
        })
        .collect()
}

fn count(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}

fn print_json(value: &serde_json::Value, out: &mut dyn Write) -> Result<(), Error> {
    let text = serde_json::to_string_pretty(value).expect("a JSON value serializes");
    put(out, format!("{text}\n").as_bytes())
}

/// A path under `$HOME` spelled from `~`.
fn shown(path: &Path, home: Option<&str>) -> String {
    match home.and_then(|home| path.strip_prefix(home).ok()) {
        Some(rest) => format!("~/{}", rest.disk_text()),
        None => path.disk_text(),
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let (action, rest) = match args.first().map(String::as_str) {
        Some("list") => (Action::List, &args[1..]),
        Some("check") => (Action::Check, &args[1..]),
        Some("review") => (Action::Review, &args[1..]),
        Some("promote") => match args.get(1).filter(|note| !note.starts_with('-')) {
            Some(note) => (
                Action::Promote {
                    note: note.clone(),
                    rule: String::new(),
                },
                &args[2..],
            ),
            None => return Err("memory promote requires a note path".to_string()),
        },
        _ => return Err("expected memory list|check|review|promote".to_string()),
    };
    let mut parsed = Args {
        action,
        all: false,
        json: false,
        stale_days: None,
        dry_run: false,
    };
    let promoting = matches!(parsed.action, Action::Promote { .. });
    let mut options = rest.iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--all" if !promoting => parsed.all = true,
            "--json" if matches!(parsed.action, Action::List | Action::Check) => parsed.json = true,
            "--stale-days" if matches!(parsed.action, Action::Check | Action::Review) => {
                let days = options.next().and_then(|value| value.parse::<i64>().ok());
                match days.filter(|days| *days >= 0) {
                    Some(days) => parsed.stale_days = Some(days),
                    None => return Err("--stale-days requires a number of days".to_string()),
                }
            }
            "--rule" if promoting => {
                let Some(name) = options.next().filter(|name| !name.starts_with('-')) else {
                    return Err("--rule requires a name".to_string());
                };
                if let Action::Promote { rule, .. } = &mut parsed.action {
                    *rule = name.clone();
                }
            }
            "--dry-run" if promoting => parsed.dry_run = true,
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    if let Action::Promote { rule, .. } = &parsed.action
        && rule.is_empty()
    {
        return Err("memory promote requires --rule <name>".to_string());
    }
    Ok(parsed)
}
