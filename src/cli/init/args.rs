//! The `init` flags, their help text, and the csv helpers that read them.

use super::Run;
use crate::Error;
use crate::output::help::{Help, Section};

pub(super) const CONTENT_DEFAULT: &str = "agents,rules,skills,commands,subagents";
pub(super) const CONTENT_VALID: [&str; 5] = ["agents", "rules", "skills", "commands", "subagents"];

pub const HELP: Help = Help {
    command: "init",
    tagline: "scaffold .ai/ in a project",
    synopsis: &["init [<dir>] [OPTIONS]"],
    description: &[
        "Minimal by default: only tools you opt in to get per-tool payload\nscaffolding (settings/mcp/hooks).",
        "Before writing, init snapshots its .ai/ paths and the selected tools'\nexisting destinations under .ai/backups/ so a partial setup can be\nrestored safely.",
        "In a terminal, init opens an interactive wizard that lets you pick tools\nand content sections. In non-TTY environments (CI, scripts), it runs\nsilently with auto-detected defaults. Pass --yes or any of --tools,\n--content, --no-detect, or --no-templates to skip the wizard.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            (
                "--tools <csv>",
                "Enable these tools (e.g. claude,cursor). Unions\nwith auto-detection unless --no-detect is passed",
            ),
            (
                "--content <csv>",
                "Which source sections to scaffold. Valid tokens:\nagents, rules, skills, commands, subagents\n(default: all of them)",
            ),
            (
                "--no-detect",
                "Skip filesystem marker auto-detection (tools only)",
            ),
            (
                "--outputs <mode>",
                "Where generated tool files live. committed\n(default) keeps them and .ai/.sync-manifest in git\nso teammates need only git pull; local gitignores\nboth and every clone runs exuno sync",
            ),
            (
                "--existing <action>",
                "What to do with tool config the project already\nhas: adopt (default) copies it into .ai/src/ so the\nfirst sync reproduces it; replace regenerates from\nthe shipped templates",
            ),
            (
                "--ci <provider>",
                "Write a CI gate that runs exuno check. Only\ngithub is supported; an existing workflow is kept",
            ),
            ("--no-sync", "Skip the first exuno sync at the end"),
            (
                "--no-templates",
                "Create selected content paths without copying\nshipped starter files. AGENTS.md is empty when\nagents is selected",
            ),
            ("-y, --yes", "Skip all prompts, accept defaults"),
            (
                "--dry-run",
                "Show what would be created without writing anything",
            ),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "init                              # interactive wizard (TTY)",
        "init --yes                        # auto-detect + defaults, no prompt",
        "init --tools claude               # Claude only, no detection union",
        "init --tools claude,cursor --content agents,rules",
        "init --no-detect                  # no tool auto-detection; pick tools later",
        "init --no-templates --no-detect   # empty .ai/src/ layout, no starters",
        "init --dry-run                    # preview without writing",
    ],
};

pub(super) struct Options {
    pub(super) target: Option<String>,
    pub(super) tools: Option<String>,
    pub(super) content: Option<String>,
    pub(super) no_detect: bool,
    pub(super) outputs: String,
    pub(super) existing: String,
    pub(super) ci: String,
    pub(super) run_sync: bool,
    pub(super) no_templates: bool,
    pub(super) assume_yes: bool,
    pub(super) dry_run: bool,
}

const VALUED_FLAGS: [&str; 5] = ["--tools", "--content", "--outputs", "--existing", "--ci"];

impl Options {
    fn set(&mut self, flag: &str, value: String) {
        match flag {
            "--tools" => self.tools = Some(value),
            "--content" => self.content = Some(value),
            "--outputs" => self.outputs = value,
            "--existing" => self.existing = value,
            _ => self.ci = value,
        }
    }
}

pub(super) fn parse_args(args: &[String], run: &mut Run) -> Result<Result<Options, u8>, Error> {
    let style = run.style;
    let mut options = Options {
        target: None,
        tools: None,
        content: None,
        no_detect: false,
        outputs: "committed".to_string(),
        existing: "adopt".to_string(),
        ci: String::new(),
        run_sync: true,
        no_templates: false,
        assume_yes: false,
        dry_run: false,
    };
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        if let Some((flag, value)) = arg
            .split_once('=')
            .filter(|(flag, _)| VALUED_FLAGS.contains(flag))
        {
            options.set(flag, value.to_string());
            continue;
        }
        match arg.as_str() {
            flag if VALUED_FLAGS.contains(&flag) => {
                let Some(value) = rest.next() else {
                    run.tell(&format!(
                        "{}: {flag} requires a value\n",
                        style.red("Error")
                    ))?;
                    return Ok(Err(1));
                };
                options.set(flag, value.clone());
            }
            "--no-detect" => options.no_detect = true,
            "--no-sync" => options.run_sync = false,
            "--no-templates" => options.no_templates = true,
            "--yes" | "-y" => options.assume_yes = true,
            "--dry-run" => options.dry_run = true,
            "--help" | "-h" => {
                run.say(&HELP.render(style))?;
                return Ok(Err(0));
            }
            flag if flag.starts_with('-') => {
                run.tell(&format!(
                    "{}: Unknown flag: {flag}\nRun {} for usage.\n",
                    style.red("Error"),
                    style.cyan("exuno init --help")
                ))?;
                return Ok(Err(1));
            }
            value => {
                if options.target.is_some() {
                    run.tell(&format!(
                        "{}: Unexpected argument: {value}\n",
                        style.red("Error")
                    ))?;
                    return Ok(Err(1));
                }
                options.target = Some(value.to_string());
            }
        }
    }
    for (flag, value) in [("--tools", &options.tools), ("--content", &options.content)] {
        if value
            .as_deref()
            .is_some_and(|csv| normalize_csv(csv).is_empty())
        {
            run.tell(&format!(
                "{}: {flag} requires a value\n",
                style.red("Error")
            ))?;
            return Ok(Err(1));
        }
    }
    Ok(Ok(options))
}

/// `_init_normalize_csv`: each token trimmed, empties skipped, first
/// occurrence kept.
pub(super) fn normalize_csv(csv: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in csv.split(',') {
        let token = token.trim().to_string();
        if token.is_empty() || out.contains(&token) {
            continue;
        }
        out.push(token);
    }
    out
}

/// `_init_merge_lists`.
pub(super) fn merge_lists(a: &[String], b: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in a.iter().chain(b) {
        if !token.is_empty() && !out.contains(token) {
            out.push(token.clone());
        }
    }
    out
}

#[cfg(all(test, unix))]
mod tests {
    use super::super::tests::{call, project, quiet};
    use std::path::Path;

    #[test]
    fn help_renders_in_the_shared_shape() {
        let (_dir, root) = project(&[]);
        let help = call(&root, &["--help"], quiet());
        assert_eq!(help.status, 0);
        assert_eq!(help.err, "");
        assert_eq!(
            help.out,
            "\n  exuno init — scaffold .ai/ in a project\n\n  USAGE\n    exuno init [<dir>] [OPTIONS]\n\n  DESCRIPTION\n    Minimal by default: only tools you opt in to get per-tool payload\n    scaffolding (settings/mcp/hooks).\n\n    Before writing, init snapshots its .ai/ paths and the selected tools'\n    existing destinations under .ai/backups/ so a partial setup can be\n    restored safely.\n\n    In a terminal, init opens an interactive wizard that lets you pick tools\n    and content sections. In non-TTY environments (CI, scripts), it runs\n    silently with auto-detected defaults. Pass --yes or any of --tools,\n    --content, --no-detect, or --no-templates to skip the wizard.\n\n  OPTIONS\n    --tools <csv>         Enable these tools (e.g. claude,cursor). Unions\n                          with auto-detection unless --no-detect is passed\n    --content <csv>       Which source sections to scaffold. Valid tokens:\n                          agents, rules, skills, commands, subagents\n                          (default: all of them)\n    --no-detect           Skip filesystem marker auto-detection (tools only)\n    --outputs <mode>      Where generated tool files live. committed\n                          (default) keeps them and .ai/.sync-manifest in git\n                          so teammates need only git pull; local gitignores\n                          both and every clone runs exuno sync\n    --existing <action>   What to do with tool config the project already\n                          has: adopt (default) copies it into .ai/src/ so the\n                          first sync reproduces it; replace regenerates from\n                          the shipped templates\n    --ci <provider>       Write a CI gate that runs exuno check. Only\n                          github is supported; an existing workflow is kept\n    --no-sync             Skip the first exuno sync at the end\n    --no-templates        Create selected content paths without copying\n                          shipped starter files. AGENTS.md is empty when\n                          agents is selected\n    -y, --yes             Skip all prompts, accept defaults\n    --dry-run             Show what would be created without writing anything\n    -h, --help            Show this help\n\n  EXAMPLES\n    exuno init                              # interactive wizard (TTY)\n    exuno init --yes                        # auto-detect + defaults, no prompt\n    exuno init --tools claude               # Claude only, no detection union\n    exuno init --tools claude,cursor --content agents,rules\n    exuno init --no-detect                  # no tool auto-detection; pick tools later\n    exuno init --no-templates --no-detect   # empty .ai/src/ layout, no starters\n    exuno init --dry-run                    # preview without writing\n\n"
        );
    }

    #[test]
    fn arguments_and_validation_are_refused_like_bash() {
        let (_dir, root) = project(&[]);
        let cases: [(&[&str], u8, &str); 13] = [
            (&["--tools="], 1, "Error: --tools requires a value\n"),
            (
                &["--content", " "],
                1,
                "Error: --content requires a value\n",
            ),
            (
                &["--tools", "claude,cla ude"],
                1,
                "Error: Invalid tool name in --tools: cla ude\n",
            ),
            (
                &["--tools", "claude", "--content", " agents , rul es"],
                1,
                "Error: Unknown --content section: rul es\nValid sections: agents rules skills commands subagents\n",
            ),
            (
                &["--bogus"],
                1,
                "Error: Unknown flag: --bogus\nRun exuno init --help for usage.\n",
            ),
            (&["a", "b"], 1, "Error: Unexpected argument: b\n"),
            (&["--tools"], 1, "Error: --tools requires a value\n"),
            (
                &["--outputs", "bogus"],
                1,
                "Error: --outputs must be 'committed' or 'local' (got 'bogus')\n",
            ),
            (
                &["--existing", "bogus"],
                1,
                "Error: --existing must be 'adopt' or 'replace' (got 'bogus')\n",
            ),
            (
                &["--ci", "gitlab"],
                1,
                "Error: --ci only supports 'github' (got 'gitlab')\n",
            ),
            (
                &["missing-dir"],
                1,
                "Error: Directory not found: missing-dir\n",
            ),
            (
                &["--content", "bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
            (
                &["--tools", "claude", "--content", "agents,bogus"],
                1,
                "Error: Unknown --content section: bogus\nValid sections: agents rules skills commands subagents\n",
            ),
        ];
        for (args, status, err) in cases {
            let run = call(&root, args, quiet());
            assert_eq!(
                (run.status, run.out.as_str(), run.err.as_str()),
                (status, "", err),
                "{args:?}"
            );
        }
        assert!(!Path::new(&root).join(".ai").exists());

        std::fs::create_dir_all(Path::new(&root).join(".ai")).unwrap();
        let inside = call(&format!("{root}/.ai"), &[], quiet());
        assert_eq!(
            (inside.status, inside.err),
            (
                2,
                format!(
                    "Error: Cannot init inside the .ai/ directory: {root}/.ai\nRun exuno init from the project root (the parent of .ai/):\n  cd \"{root}\" && exuno init\n"
                )
            )
        );
    }
}
