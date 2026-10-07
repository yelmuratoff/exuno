//! Command-line options, the `--only` scope, and the usage text.

use std::io::Write;
use std::path::Path;

use crate::Error;
use crate::cli::put;
use crate::output::help::{Help, Section};
use crate::output::style::Style;

const CATEGORIES_VALID: [&str; 5] = ["rules", "skills", "commands", "agents", "subagents"];
const CATEGORIES_DEFAULT: [&str; 4] = ["rules", "skills", "commands", "agents"];

pub(super) struct Options {
    pub(super) dry_run: bool,
    pub(super) assume_yes: bool,
    pub(super) include_agents_md: bool,
    pub(super) include_deleted: bool,
    pub(super) review: bool,
    pub(super) status_only: bool,
    pub(super) only: String,
}

pub(super) fn parse_args(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Result<Options, u8>, Error> {
    let mut options = Options {
        dry_run: false,
        assume_yes: false,
        include_agents_md: false,
        include_deleted: false,
        review: false,
        status_only: false,
        only: String::new(),
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--dry-run" => options.dry_run = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--include-agents-md" => options.include_agents_md = true,
            "--include-deleted" => options.include_deleted = true,
            "--review" => options.review = true,
            "--status" => options.status_only = true,
            "--only" => match rest.next() {
                Some(value) => options.only = value.clone(),
                None => {
                    put(
                        err,
                        format!("{}: --only requires a value\n", style.red("Error")).as_bytes(),
                    )?;
                    return Ok(Err(1));
                }
            },
            "--help" | "-h" => {
                put(out, HELP.render(style).as_bytes())?;
                return Ok(Err(0));
            }
            flag if flag.starts_with("--only=") => {
                options.only = flag["--only=".len()..].to_string();
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!(
                        "{}: Unknown option: {flag}\n{}",
                        style.red("Error"),
                        HELP.render(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(Err(1));
            }
            value => {
                put(
                    err,
                    format!(
                        "{}: Unexpected argument: {value}\n{}",
                        style.red("Error"),
                        HELP.render(style)
                    )
                    .as_bytes(),
                )?;
                return Ok(Err(1));
            }
        }
    }
    Ok(Ok(options))
}

/// `_refresh_resolve_scope`: the categories present under `user_base`, or the
/// validated, deduplicated `--only` list in the order given.
pub(super) fn resolve_scope(
    only: &str,
    user_base: &str,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Vec<String>, u8>, Error> {
    if only.is_empty() {
        let found: Vec<String> = CATEGORIES_DEFAULT
            .iter()
            .filter(|category| Path::new(user_base).join(category).is_dir())
            .map(|category| category.to_string())
            .collect();
        if found.is_empty() {
            put(
                err,
                format!(
                    "{}: No source content categories present in {user_base}.\nPass {} to opt into specific ones,\nor run {} to scaffold them.\n",
                    style.red("Error"),
                    style.cyan("--only rules,skills,commands,agents"),
                    style.cyan("exuno init")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        return Ok(Ok(found));
    }
    let mut categories: Vec<String> = Vec::new();
    for token in only.split(',') {
        let token = token.trim_matches(|c: char| c.is_ascii_whitespace() || c == '\x0b');
        if token.is_empty() {
            continue;
        }
        // `subagents` is the init/--content token; the directory is `agents`.
        let token = if token == "subagents" {
            "agents"
        } else {
            token
        };
        if !CATEGORIES_VALID.contains(&token) {
            put(
                err,
                format!(
                    "{}: Unknown --only value: {token}\nValid: rules, skills, commands, agents (or subagents)\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        if !categories.iter().any(|known| known == token) {
            categories.push(token.to_string());
        }
    }
    if categories.is_empty() {
        put(
            err,
            format!(
                "{}: --only must include at least one category\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Err(1));
    }
    Ok(Ok(categories))
}

pub const HELP: Help = Help {
    command: "refresh",
    tagline: "pull new template files into an existing .ai/src/",
    synopsis: &["refresh [OPTIONS]"],
    description: &[
        "Compares each shipped template (rules, skills, commands, agents) against\nyour local .ai/src/ using a three-way diff (template-old vs template-new\nvs your current file) when a template manifest is present. Files you\nhaven't touched auto-update silently; only true conflicts require review.\nFiles in .ai/src/ that aren't part of the templates (your custom content)\nare left alone.",
    ],
    sections: &[
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--only <csv>",
                    "Categories to consider: rules, skills, commands, agents\nDefault: only categories that already have a subdir\nin your .ai/src/. Pass --only to opt into a category\nyou don't have yet.",
                ),
                (
                    "--include-agents-md",
                    "Also offer updates to AGENTS.md (off by default —\nalmost always heavily customized).",
                ),
                (
                    "--include-deleted",
                    "Re-offer files you previously declined and removed\nfrom disk so they can be restored.",
                ),
                (
                    "--review",
                    "Resurface every local divergence from the shipped\ntemplates, including conflicts you previously\n[s]kipped. Use this to revisit earlier decisions\nor audit local edits.",
                ),
                (
                    "--status",
                    "Print declined breakdown (persistent + local) and\nexit. No mutation, no prompts.",
                ),
                ("--dry-run", "Print the plan without writing anything."),
                (
                    "-y, --yes",
                    "Apply auto-updates and add new files; skip conflicts\n(no prompts). Required in non-interactive contexts.",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
        Section {
            title: "REMEMBERED SKIPS",
            entries: &[(
                "",
                "Picking [s]kip on a conflict records the current template hash in\n.ai/.template-manifest. The divergence stays silent on future\nrefreshes until a newer template ships (at which point it\nresurfaces automatically so you can review the new change). Pass\n--review at any time to revisit your skips explicitly.",
            )],
        },
        Section {
            title: "PERSISTENT OVERRIDES",
            entries: &[(
                "",
                "Edit .ai/exuno.yaml to silence specific templates forever (this\nis stronger than [s]kip — even new template versions stay hidden):\ntemplate_overrides:\n  declined:        # always-skip; never offered\n    - rules/some-rule.md\n  pinned:          # ignore template updates; keep your version\n    - rules/my-version.md",
            )],
        },
    ],
    examples: &[
        "refresh",
        "refresh --only rules,skills",
        "refresh --dry-run",
        "refresh --yes               # CI-friendly: auto-update + add new",
        "refresh --include-deleted   # revisit previously declined files",
        "refresh --review            # revisit conflicts you skipped",
    ],
};

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use crate::cli::refresh::tests::{call, drop_entry, header, manifest_text, seeded};
    use crate::config::template_manifest::REL;

    #[test]
    fn arguments_are_refused_and_help_prints_like_bash() {
        let (_dir, root) = seeded();
        let help = call(&root, &["--help", "--bogus"], false, &[]);
        assert_eq!(help.status, 0);
        assert_eq!(
            help.out,
            "\n  exuno refresh — pull new template files into an existing .ai/src/\n\n  USAGE\n    exuno refresh [OPTIONS]\n\n  DESCRIPTION\n    Compares each shipped template (rules, skills, commands, agents) against\n    your local .ai/src/ using a three-way diff (template-old vs template-new\n    vs your current file) when a template manifest is present. Files you\n    haven't touched auto-update silently; only true conflicts require review.\n    Files in .ai/src/ that aren't part of the templates (your custom content)\n    are left alone.\n\n  OPTIONS\n    --only <csv>          Categories to consider: rules, skills, commands, agents\n                          Default: only categories that already have a subdir\n                          in your .ai/src/. Pass --only to opt into a category\n                          you don't have yet.\n    --include-agents-md   Also offer updates to AGENTS.md (off by default —\n                          almost always heavily customized).\n    --include-deleted     Re-offer files you previously declined and removed\n                          from disk so they can be restored.\n    --review              Resurface every local divergence from the shipped\n                          templates, including conflicts you previously\n                          [s]kipped. Use this to revisit earlier decisions\n                          or audit local edits.\n    --status              Print declined breakdown (persistent + local) and\n                          exit. No mutation, no prompts.\n    --dry-run             Print the plan without writing anything.\n    -y, --yes             Apply auto-updates and add new files; skip conflicts\n                          (no prompts). Required in non-interactive contexts.\n    -h, --help            Show this help\n\n  REMEMBERED SKIPS\n    Picking [s]kip on a conflict records the current template hash in\n    .ai/.template-manifest. The divergence stays silent on future\n    refreshes until a newer template ships (at which point it\n    resurfaces automatically so you can review the new change). Pass\n    --review at any time to revisit your skips explicitly.\n\n  PERSISTENT OVERRIDES\n    Edit .ai/exuno.yaml to silence specific templates forever (this\n    is stronger than [s]kip — even new template versions stay hidden):\n    template_overrides:\n      declined:        # always-skip; never offered\n        - rules/some-rule.md\n      pinned:          # ignore template updates; keep your version\n        - rules/my-version.md\n\n  EXAMPLES\n    exuno refresh\n    exuno refresh --only rules,skills\n    exuno refresh --dry-run\n    exuno refresh --yes               # CI-friendly: auto-update + add new\n    exuno refresh --include-deleted   # revisit previously declined files\n    exuno refresh --review            # revisit conflicts you skipped\n\n"
        );

        let bogus = call(&root, &["--bogus", "--help"], false, &[]);
        assert_eq!((bogus.status, bogus.out.as_str()), (1, ""));
        assert_eq!(
            bogus.err,
            format!("Error: Unknown option: --bogus\n{}", help.out)
        );
        let extra = call(&root, &["extra"], false, &[]);
        assert_eq!(
            extra.err,
            format!("Error: Unexpected argument: extra\n{}", help.out)
        );
        let missing = call(&root, &["--yes", "--only"], false, &[]);
        assert_eq!(
            (missing.status, missing.err.as_str()),
            (1, "Error: --only requires a value\n")
        );
        assert_eq!(
            call(&root, &["--yes", "--only", "bogus"], false, &[]).err,
            "Error: Unknown --only value: bogus\nValid: rules, skills, commands, agents (or subagents)\n"
        );
        assert_eq!(
            call(&root, &["--yes", "--only", ","], false, &[]).err,
            "Error: --only must include at least one category\n"
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
    }

    #[test]
    fn scope_follows_only_and_present_directories_and_heals_every_category() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        std::fs::remove_dir_all(base.join("skills/comments")).unwrap();
        std::fs::remove_dir_all(base.join("agents")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();

        let rules = call(&root, &["--yes", "--only", "rules"], false, &[]);
        assert_eq!(
            rules.out,
            format!(
                "{}  Manifest:  none — falling back to two-way diff\n\n  Summary:\n    + 1 new template(s)\n    · 2 unchanged\n\n  New:\n    + rules/comments.md\n\n  + rules/comments.md\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 2\n\n  Next: exuno sync to distribute the updates to enabled tools.\n\n",
                header(&root, "rules").strip_suffix('\n').unwrap()
            )
        );
        let healed = manifest_text(&root);
        assert_eq!(healed.lines().count(), 17);
        assert!(healed.contains("AGENTS.md\t"));
        assert!(healed.contains("skills/humanizer/SKILL.md\t"));
        assert!(!healed.contains("skills/comments/SKILL.md\t"));
        assert!(!healed.contains("agents/code-reviewer.md\t"));

        let spaced = call(
            &root,
            &["--yes", "--only= skills , subagents ,skills"],
            false,
            &[],
        );
        assert_eq!(
            spaced.out,
            format!(
                "{}  Summary:\n    + 2 new template(s)\n    · 11 unchanged\n\n  New:\n    + agents/code-reviewer.md\n    + skills/comments/SKILL.md\n\n  + agents/code-reviewer.md\n  + skills/comments/SKILL.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 11\n\n  Next: exuno sync to distribute the updates to enabled tools.\n\n",
                header(&root, "skills,agents")
            )
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);

        std::fs::remove_dir_all(base.join("commands")).unwrap();
        let absent = call(&root, &["--yes"], false, &[]);
        assert!(absent.out.contains("  Scope:     rules,skills,agents\n"));
        assert!(absent.out.contains("Already up to date! 16 file(s)"));
        assert!(!base.join("commands").exists());

        for category in ["rules", "skills", "agents"] {
            std::fs::remove_dir_all(base.join(category)).unwrap();
        }
        let none = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            (none.status, none.err),
            (
                1,
                format!(
                    "Error: No source content categories present in {root}/.ai/src.\nPass --only rules,skills,commands,agents to opt into specific ones,\nor run exuno init to scaffold them.\n"
                )
            )
        );
        let recorded = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert_eq!(
            recorded.out,
            format!(
                "{}  Already up to date! 0 file(s) match the current templates.\n  Locally declined (.template-manifest):    2 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n\n",
                header(&root, "commands")
            )
        );
        drop_entry(&root, "commands/fix-issue.md");
        drop_entry(&root, "commands/review.md");
        let opted = call(&root, &["--yes", "--only", "commands"], false, &[]);
        assert!(opted.out.contains(
            "  Summary:\n    + 2 new template(s)\n\n  New:\n    + commands/fix-issue.md\n    + commands/review.md\n\n  + commands/fix-issue.md\n  + commands/review.md\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 0\n"
        ));
        assert!(base.join("commands/review.md").is_file());
    }
}
