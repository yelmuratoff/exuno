use std::io::Write;

use crate::config::skill_metadata::{self, SkillMetadata};
use crate::engine::filters::Filter;
use crate::engine::render::{self, Env};
use crate::engine::session::Session;
use crate::engine::skill_tree::{self, Skill, Tree};
use crate::engine::workspace::Workspace;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::paths::Paths;
use crate::{Error, paths};

mod catalog;

pub const HELP: Help = Help {
    command: "skills",
    tagline: "inspect skills used by this project",
    synopsis: &[
        "skills list [--profile <name>] [--include <globs>] [--exclude <globs>]",
        "skills show <name> [--profile <name>]",
        "skills check [--profile <name>]",
        "skills catalog list --catalog <file> [--source <alias=local-repo>]...",
        "skills catalog show <id> --catalog <file> [--source <alias=local-repo>]...",
    ],
    description: &[
        "Reads the effective source.skills tree, including shared and bundled\nskills. --profile applies the same profile overlay as sync. No project\nfiles are changed.",
        "show displays the skill's declared metadata and optional annotations.\nThose annotations and requirements are not verified by Exuno.",
        "check verifies the required fields, the supported scalar forms, and\nthe category layout. For full Agent Skills validation, use skills-ref\nvalidate <skill-dir>. Of its findings, sync refuses only a name two\nskills share.",
        "catalog inspects explicitly declared external skills. Its curator\nnotes are unverified; pinned local Git metadata is read only when a source\nmapping is supplied. It never installs or runs a skill.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            (
                "--profile <name>",
                "Inspect this configured profile's skills",
            ),
            (
                "--include <globs>",
                "List only skills whose name or category path matches",
            ),
            (
                "--exclude <globs>",
                "Exclude skills whose name or category path matches",
            ),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "skills list",
        "skills show deploy",
        "skills check",
        "skills list --profile work --include 'review*'",
        "skills list --include 'flutter/*'",
        "skills catalog list --catalog cards.tsv",
    ],
};

enum Action {
    List,
    Check,
    Show(String),
}

struct Args {
    action: Action,
    profile: Option<String>,
    filter: Filter,
}

pub fn run(
    args: &[String],
    root: &str,
    env: &Env,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args.first().is_some_and(|arg| arg == "catalog") {
        return catalog::run(&args[1..], style, out, err);
    }
    if args
        .iter()
        .any(|arg| matches!(arg.as_str(), "-h" | "--help"))
    {
        write(out, &HELP.render(style))?;
        return Ok(0);
    }
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => {
            write(err, &format!("Error: {message}\n"))?;
            return Ok(1);
        }
    };
    let Some((session, source)) = load_source(root, env, parsed.profile.as_deref(), err)? else {
        return Ok(1);
    };
    let mut count = 0;
    let mut issues = 0;
    if matches!(parsed.action, Action::List) {
        write(out, "name\tdescription\tpath\tcategory\n")?;
    }
    let tree = skill_tree::discover(&session.ws, &source.effective);
    for skill in &tree.skills {
        let (name, rel) = (&skill.name, &skill.rel);
        if let Action::Show(target) = &parsed.action
            && name != target
        {
            continue;
        }
        if matches!(parsed.action, Action::List) && !parsed.filter.accepts_skill(skill) {
            continue;
        }
        count += 1;
        let shown = shown_file(&session, &source, rel);
        let bytes = session
            .ws
            .read(&format!("{}/{rel}/SKILL.md", source.effective))?;
        let metadata = skill_metadata::read(&bytes, name);
        match &parsed.action {
            Action::List => {
                let description = metadata
                    .as_ref()
                    .map(|metadata| metadata.description.as_str())
                    .unwrap_or_default();
                write(out, &list_row(skill, description, &shown))?;
            }
            Action::Check => match metadata {
                Ok(_) => {}
                Err(message) => {
                    issues += 1;
                    write(out, &format!("{}: {}\n", cell(&shown), cell(&message)))?;
                }
            },
            Action::Show(_) => match metadata {
                Ok(metadata) => write(out, &show_text(metadata, skill, &shown))?,
                Err(message) => {
                    write(
                        err,
                        &format!("Error: {}: {}\n", cell(&shown), cell(&message)),
                    )?;
                    return Ok(1);
                }
            },
        }
    }
    if matches!(parsed.action, Action::Check) {
        let findings = layout_findings(&session, &source, &tree);
        issues += findings.len();
        for finding in findings {
            write(out, &format!("{finding}\n"))?;
        }
        write(out, &format!("Checked {count} skills: {issues} issue(s)\n"))?;
    }
    if let Action::Show(name) = &parsed.action
        && count == 0
    {
        write(err, &format!("Error: unknown skill: {}\n", cell(name)))?;
        return Ok(1);
    }
    Ok(u8::from(issues > 0))
}

/// The effective skills source, with its warnings on `err`; `None` after
/// printing why it could not be resolved.
fn load_source(
    root: &str,
    env: &Env,
    profile: Option<&str>,
    err: &mut dyn Write,
) -> Result<Option<(Session, render::SkillSource)>, Error> {
    let mut session = Session::new(Workspace::on_disk(root), Paths::on_disk(root));
    let resolved = render::skill_source(&mut session, env, profile);
    for (_, line) in session.log.lines() {
        if resolved.is_err() || line.starts_with("[WARNING]") {
            write(err, &format!("{line}\n"))?;
        }
    }
    Ok(resolved.ok().map(|source| (session, source)))
}

/// Where a skill's `SKILL.md` comes from, as `list` and `show` print it.
fn shown_file(session: &Session, source: &render::SkillSource, rel: &str) -> String {
    let origin = source
        .origins
        .iter()
        .map(|dir| format!("{dir}/{rel}/SKILL.md"))
        .find(|file| session.ws.is_file(file))
        .unwrap_or_else(|| format!("{}/{rel}/SKILL.md", source.effective));
    if origin.starts_with(paths::ENGINE_ROOT) {
        format!("bundled:skills/{rel}/SKILL.md")
    } else {
        session.display(&origin)
    }
}

fn list_row(skill: &Skill, description: &str, shown: &str) -> String {
    format!(
        "{}\t{}\t{}\t{}\n",
        cell(&skill.name),
        cell(description),
        cell(shown),
        cell(skill.category())
    )
}

fn show_text(metadata: SkillMetadata, skill: &Skill, shown: &str) -> String {
    let mut text = format!(
        "Name: {}\nDescription: {}\n",
        cell(&metadata.name),
        cell(&metadata.description)
    );
    if let Some(value) = metadata.compatibility {
        text.push_str(&format!("Compatibility (declared): {}\n", cell(&value)));
    }
    if let Some(value) = metadata.license {
        text.push_str(&format!("License: {}\n", cell(&value)));
    }
    for (label, value) in [
        ("Use when", metadata.use_when),
        ("Not for", metadata.not_for),
        ("Requirements", metadata.requirements),
    ] {
        if let Some(value) = value {
            text.push_str(&format!(
                "{label} (annotation, unverified): {}\n",
                cell(&value)
            ));
        }
    }
    if !skill.category().is_empty() {
        text.push_str(&format!("Category: {}\n", cell(skill.category())));
    }
    text.push_str(&format!("Path: {}\n", cell(shown)));
    text
}

/// `check`'s findings about the tree itself: shared names, empty and too-deep categories.
fn layout_findings(session: &Session, source: &render::SkillSource, tree: &Tree) -> Vec<String> {
    let shown_dir = |rel: &str| {
        let dir = source
            .origins
            .iter()
            .map(|origin| format!("{origin}/{rel}"))
            .find(|dir| session.ws.is_dir(dir))
            .unwrap_or_else(|| format!("{}/{rel}", source.effective));
        cell(&session.display(&dir))
    };
    let mut findings = Vec::new();
    for (name, rels) in skill_tree::collisions(&tree.skills) {
        let claims: Vec<_> = rels.iter().map(|rel| shown_dir(rel)).collect();
        findings.push(format!(
            "{}: name claimed by {} — tools install skills flat by name",
            cell(name),
            claims.join(", ")
        ));
    }
    for rel in &tree.empty_categories {
        findings.push(format!(
            "{}/: no SKILL.md here or in any subdirectory",
            shown_dir(rel)
        ));
    }
    for rel in tree.nonstandard_categories() {
        findings.push(format!(
            "{}/: category name is not lowercase letters, digits, and single hyphens",
            shown_dir(&rel)
        ));
    }
    for rel in &tree.too_deep {
        findings.push(format!(
            "{}/: deeper than {} categories — not synced",
            shown_dir(rel),
            skill_tree::MAX_CATEGORY_DEPTH
        ));
    }
    findings
}

fn parse(args: &[String]) -> Result<Args, String> {
    let action = match args.first().map(String::as_str) {
        Some("show") => {
            let Some(name) = args.get(1).filter(|name| !name.starts_with('-')) else {
                return Err("skills show requires a name".to_string());
            };
            Action::Show(name.clone())
        }
        Some("list") => Action::List,
        Some("check") => Action::Check,
        _ => return Err("expected skills list|show|check".to_string()),
    };
    let mut parsed = Args {
        action,
        profile: None,
        filter: Filter::default(),
    };
    let start = if matches!(parsed.action, Action::Show(_)) {
        2
    } else {
        1
    };
    let mut options = args[start..].iter();
    while let Some(option) = options.next() {
        let Some(value) = options
            .next()
            .filter(|value| !value.is_empty() && !value.starts_with('-'))
        else {
            return Err(format!("{option} requires a value"));
        };
        match option.as_str() {
            "--profile" if parsed.profile.is_none() => parsed.profile = Some(value.clone()),
            "--include" if matches!(parsed.action, Action::List) => {
                parsed.filter.include_also(value)
            }
            "--exclude" if matches!(parsed.action, Action::List) => {
                parsed.filter.exclude_also(value)
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    Ok(parsed)
}

fn cell(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_control()
                || matches!(c, '\u{061c}' | '\u{200b}'..='\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2060}'..='\u{206f}' | '\u{feff}')
            {
                ' '
            } else {
                c
            }
        })
        .collect()
}

fn write(out: &mut dyn Write, text: &str) -> Result<(), Error> {
    out.write_all(text.as_bytes())
        .map_err(|e| Error::io("<output>", e))
}
