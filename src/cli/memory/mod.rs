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

mod store;

use store::{Note, Place, Spec, Store};

pub const HELP: Help = Help {
    command: "memory",
    tagline: "inspect the memory coding agents keep for themselves",
    synopsis: &[
        "memory list [--all] [--json]",
        "memory check [--all] [--stale-days <n>] [--json]",
    ],
    description: &[
        "Reads the memory stores each tool's memory: block declares: the\nshipped tools and any .ai/src/tools/<tool>.yaml, profile variants in\ntheir config homes included. Without --all, only this project's store\nand the stores every project shares. No file is changed.",
        "check reports an index over the tool's size limit, notes the index\ndoes not list, index links to missing notes, and a type outside the\ntool's list. With --all it also reports a note name several projects\nshare. --stale-days adds notes last modified more than <n> days ago.\nIt exits 1 when it finds anything.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            ("--all", "Read every project's store, not only this one"),
            (
                "--stale-days <n>",
                "check: report notes older than <n> days",
            ),
            ("--json", "Print one JSON value on stdout"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "memory list",
        "memory list --all --json",
        "memory check --all --stale-days 90",
    ],
};

/// What `main` supplies from the process.
pub struct Env<'a> {
    pub home: Option<String>,
    pub var: &'a dyn Fn(&str) -> Option<String>,
    /// Days since 1970-01-01, UTC.
    pub today: i64,
}

#[derive(PartialEq)]
enum Action {
    List,
    Check,
}

struct Args {
    action: Action,
    all: bool,
    json: bool,
    stale_days: Option<i64>,
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
    let found = find(root, env, parsed.all, err)?;
    let shown = |path: &Path| shown(path, env.home.as_deref());
    match parsed.action {
        Action::List => list(&found, parsed.json, &shown, out),
        Action::Check => {
            let stale = parsed.stale_days.map(|days| (env.today, days));
            check(&found, stale, parsed.json, &shown, out)
        }
    }
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
        let values: Vec<serde_json::Value> = rows
            .map(|(tool, store, note)| {
                serde_json::json!({
                    "tool": tool,
                    "project": store.project,
                    "name": note.name,
                    "type": note.kind,
                    "modified": note.modified,
                    "description": note.description,
                    "path": shown(&note.path),
                })
            })
            .collect();
        print_json(&serde_json::Value::Array(values), out)?;
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
    stale: Option<(i64, i64)>,
    json: bool,
    shown: &dyn Fn(&Path) -> String,
    out: &mut dyn Write,
) -> Result<u8, Error> {
    let mut issues = Vec::new();
    for (spec, store, notes) in &found.stores {
        issues.extend(store::findings(&found.specs[*spec], store, notes, stale));
    }
    issues.extend(shared_names(found));
    let checked: usize = found.stores.iter().map(|(_, _, notes)| notes.len()).sum();
    if json {
        let values: Vec<serde_json::Value> = issues
            .iter()
            .map(|(path, message)| serde_json::json!({"path": shown(path), "message": message}))
            .collect();
        print_json(
            &serde_json::json!({"checked": checked, "issues": values}),
            out,
        )?;
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
    let action = match args.first().map(String::as_str) {
        Some("list") => Action::List,
        Some("check") => Action::Check,
        _ => return Err("expected memory list|check".to_string()),
    };
    let mut parsed = Args {
        action,
        all: false,
        json: false,
        stale_days: None,
    };
    let mut options = args[1..].iter();
    while let Some(option) = options.next() {
        match option.as_str() {
            "--all" => parsed.all = true,
            "--json" => parsed.json = true,
            "--stale-days" if parsed.action == Action::Check => {
                let days = options.next().and_then(|value| value.parse::<i64>().ok());
                match days.filter(|days| *days >= 0) {
                    Some(days) => parsed.stale_days = Some(days),
                    None => return Err("--stale-days requires a number of days".to_string()),
                }
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    Ok(parsed)
}
