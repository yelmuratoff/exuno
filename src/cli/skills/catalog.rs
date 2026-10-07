//! `exuno skills catalog`: an explicit, read-only view of skill cards.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;

use crate::Error;
use crate::cli::put;
use crate::config::{skill_cards, skill_source};
use crate::engine::filters::Filter;
use crate::output::help::{Help, Section};
use crate::output::style::Style;

pub const HELP: Help = Help {
    command: "skills catalog",
    tagline: "inspect an explicit experimental skill-card catalog",
    synopsis: &[
        "skills catalog list --catalog <file> [--source <alias=local-repo>]... [--include <globs>] [--exclude <globs>]",
        "skills catalog show <id> --catalog <file> [--source <alias=local-repo>]...",
    ],
    description: &[
        "Reads one explicit TSV catalog and, optionally, declarations from pinned\nlocal Git objects. It does not discover skills, select work, install or\nprovision anything, fetch from a network, or execute catalog fields.",
        "Catalog annotations are curator notes. Requirements and capabilities are\nUNVERIFIED: this command does not establish runtime suitability or portability.",
    ],
    sections: &[
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--catalog <file>",
                    "Required regular TSV file (maximum 128 KiB and 256 cards)",
                ),
                (
                    "--source <alias=local-repo>",
                    "Optional local source mapping; may be repeated",
                ),
                (
                    "--include <globs>",
                    "List only card IDs matching these glob patterns",
                ),
                (
                    "--exclude <globs>",
                    "Exclude card IDs matching these glob patterns",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
        Section {
            title: "SOURCE STATUS",
            entries: &[
                (
                    "declaration",
                    "Catalog source, commit, and path are claimed provenance",
                ),
                (
                    "verification",
                    "Pinned metadata is available only when its explicit local Git object can be read",
                ),
            ],
        },
    ],
    examples: &[
        "skills catalog list --catalog docs/examples/skill-cards/pilot/catalog.tsv",
        "skills catalog list --catalog catalog.tsv --source anthropic-skills=/work/anthropic-skills --include 'pdf*'",
        "skills catalog show pdf --catalog catalog.tsv --source anthropic-skills=/work/anthropic-skills",
    ],
};

#[derive(Clone, Copy)]
enum Action {
    Help,
    List,
    Show,
}

struct Args {
    action: Action,
    id: Option<String>,
    catalog: PathBuf,
    sources: BTreeMap<String, PathBuf>,
    filter: Filter,
}

pub fn run(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let parsed = match parse(args) {
        Ok(parsed) => parsed,
        Err(message) => return refuse(style, &message, err),
    };
    if matches!(parsed.action, Action::Help) {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let cards = match skill_cards::read(&parsed.catalog) {
        Ok(cards) => cards,
        Err(message) => return refuse(style, &message, err),
    };
    match parsed.action {
        Action::List => {
            let mut rendered = String::from("id\tsource\tsource_status\tmapping\n");
            for card in cards.iter().filter(|card| parsed.filter.accepts(&card.id)) {
                let info = source_info(card, &parsed.sources);
                rendered.push_str(&format!(
                    "{}\t{}\t{}\t{}\n",
                    card.id, card.source, info.status, card.mapping
                ));
            }
            put(out, rendered.as_bytes())?;
            Ok(0)
        }
        Action::Show => {
            let id = parsed.id.as_deref().expect("show requires an id");
            let Some(card) = cards.iter().find(|card| card.id == id) else {
                return refuse(style, &format!("Unknown skill id: {id}"), err);
            };
            let info = source_info(card, &parsed.sources);
            let rendered = format!(
                "ID: {}\nSource declaration: {}@{}:{}\nSource status: {}\nFrontmatter: {}\nName: {}\nDescription: {}\nCatalog annotations: curator notes; UNVERIFIED; not source-verified\nOASF: {}; paths: {}\nMapping: {}\nUse when: {}\nNot for: {}\nRequirements (curator notes; UNVERIFIED; not checked): {}\n",
                card.id,
                card.source,
                card.commit,
                card.path,
                info.status,
                info.frontmatter,
                info.name,
                info.description,
                card.oasf_version,
                card.oasf_terms,
                card.mapping,
                card.use_when,
                card.not_for,
                card.requirements,
            );
            put(out, rendered.as_bytes())?;
            Ok(0)
        }
        Action::Help => unreachable!(),
    }
}

fn source_info(
    card: &skill_cards::Card,
    sources: &BTreeMap<String, PathBuf>,
) -> skill_source::SourceInfo {
    skill_source::read(
        sources.get(&card.source).map(PathBuf::as_path),
        &card.commit,
        &card.path,
        &card.id,
    )
}

fn parse(args: &[String]) -> Result<Args, String> {
    let action = match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => return Ok(help_args()),
        Some("list") => Action::List,
        Some("show") => Action::Show,
        _ => {
            return Err("Usage: exuno skills catalog <list|show> --catalog <file>".to_string());
        }
    };

    let mut catalog = None;
    let mut id = None;
    let mut sources = BTreeMap::new();
    let mut filter = Filter::default();
    let mut index = 1;
    while let Some(argument) = args.get(index) {
        match argument.as_str() {
            "--help" | "-h" => return Ok(help_args()),
            "--catalog" => {
                let value = required(args, index, "--catalog requires one non-empty path")?;
                if catalog.replace(PathBuf::from(value)).is_some() {
                    return Err("--catalog may be supplied only once".to_string());
                }
                index += 2;
            }
            "--source" => {
                let value = required(args, index, "--source requires ALIAS=LOCAL_REPO")?;
                let (alias, repo) = value
                    .split_once('=')
                    .filter(|(_, repo)| !repo.is_empty())
                    .ok_or_else(|| "--source requires ALIAS=LOCAL_REPO".to_string())?;
                if !slug(alias) {
                    return Err(format!("Unsafe source alias: {alias}"));
                }
                if sources
                    .insert(alias.to_string(), PathBuf::from(repo))
                    .is_some()
                {
                    return Err(format!("Duplicate source alias: {alias}"));
                }
                index += 2;
            }
            "--include" => {
                if !matches!(action, Action::List) {
                    return Err("--include is a list option".to_string());
                }
                filter.include_also(required(args, index, "--include requires globs")?);
                index += 2;
            }
            "--exclude" => {
                if !matches!(action, Action::List) {
                    return Err("--exclude is a list option".to_string());
                }
                filter.exclude_also(required(args, index, "--exclude requires globs")?);
                index += 2;
            }
            flag if flag.starts_with('-') => return Err(format!("Unknown skills option: {flag}")),
            value if matches!(action, Action::Show) && id.is_none() => {
                if !slug(value) {
                    return Err(format!("Unsafe skill id: {value}"));
                }
                id = Some(value.to_string());
                index += 1;
            }
            value => return Err(format!("Unexpected skills argument: {value}")),
        }
    }
    let catalog = catalog.ok_or_else(|| "--catalog is required".to_string())?;
    if matches!(action, Action::Show) && id.is_none() {
        return Err("skills catalog show requires an id".to_string());
    }
    Ok(Args {
        action,
        id,
        catalog,
        sources,
        filter,
    })
}

fn help_args() -> Args {
    Args {
        action: Action::Help,
        id: None,
        catalog: PathBuf::new(),
        sources: BTreeMap::new(),
        filter: Filter::default(),
    }
}

fn required<'a>(args: &'a [String], index: usize, message: &str) -> Result<&'a str, String> {
    args.get(index + 1)
        .map(String::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| message.to_string())
}

fn slug(value: &str) -> bool {
    !value.is_empty()
        && value.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn refuse(style: &Style, message: &str, err: &mut dyn Write) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cli::notice;

    #[test]
    fn help_needs_no_catalog_or_source() {
        let mut out = Vec::new();
        let mut err = Vec::new();
        assert_eq!(
            run(&["help".into()], &Style::plain(), &mut out, &mut err).unwrap(),
            0
        );
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("exuno skills catalog list")
        );
        assert!(err.is_empty());
    }

    #[test]
    fn skills_is_outside_the_update_notice_allowlist() {
        assert!(!notice::wants_notice("skills"));
    }

    #[test]
    fn repeated_filters_share_the_engine_glob_semantics() {
        let parsed = parse(&[
            "list".into(),
            "--catalog".into(),
            "cards.tsv".into(),
            "--include".into(),
            "pdf*".into(),
            "--include".into(),
            "tdd".into(),
            "--exclude".into(),
            "pdf-old".into(),
        ])
        .unwrap();
        assert!(parsed.filter.accepts("pdf"));
        assert!(parsed.filter.accepts("tdd"));
        assert!(!parsed.filter.accepts("pdf-old"));
    }
}
