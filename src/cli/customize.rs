//! `exuno customize`: `cmd_customize` of `lib/helpers/customize.sh`, which
//! scaffolds a tool override or copies a shipped payload into the override directory.

use crate::paths::DiskText;
use std::io::Write;
use std::path::Path;

use super::put;
use crate::config::payload;
use crate::config::tool::Tool;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::project::Project;
use crate::{Error, config::catalog, text};

pub const VALID_RESOURCES: [&str; 4] = ["tool", "hooks", "mcp", "settings"];

pub const HELP: Help = Help {
    command: "customize",
    tagline: "scaffold a per-tool override",
    synopsis: &["customize <slug> [<resource>] [--full] [--yes]"],
    description: &[
        "Scaffold a per-tool override at .ai/src/tools/<slug>.yaml so you can\nchange fields without forking the whole base template. Empty by default:\nwrite only the keys you want to win over base; everything else inherits.",
    ],
    sections: &[
        Section {
            title: "ARGUMENTS",
            entries: &[
                ("<slug>", "Tool to override"),
                (
                    "<resource>",
                    "Payload override scaffold: tool, hooks, mcp, settings\n(default: tool, the YAML override itself)",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--full",
                    "Copy the entire base template into the override. Use when you\nwant to see every available field at once; trim what you don't\nneed with exuno simplify",
                ),
                (
                    "-y, --yes",
                    "Overwrite an existing override without prompting",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
        Section {
            title: "SEE ALSO",
            entries: &[
                ("exuno show <slug>", "Effective config for a tool"),
                ("exuno diff <slug>", "User vs base diff"),
            ],
        },
    ],
    examples: &[
        "customize cursor",
        "customize cursor --full",
        "customize claude hooks --yes",
    ],
};

/// `_validate_resource`, printed; the caller returns the status.
pub(crate) fn unknown_resource(
    style: &Style,
    resource: &str,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    put(
        err,
        format!(
            "{}: Unknown resource '{resource}'.\nValid: {}\n",
            style.red("Error"),
            VALID_RESOURCES.join(" ")
        )
        .as_bytes(),
    )?;
    Ok(1)
}

pub(crate) fn relative(project: &Project, path: &Path) -> String {
    let text = path.disk_text();
    let root = format!("{}/", project.root.disk_text());
    text.strip_prefix(&root).unwrap_or(&text).to_string()
}

pub fn customize(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    stdin_tty: bool,
    ask: &mut dyn FnMut(&str) -> String,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut full, mut yes) = (false, false);
    let (mut slug, mut resource) = (String::new(), String::new());
    for arg in args {
        match arg.as_str() {
            "--full" => full = true,
            "--yes" | "-y" => yes = true,
            "--help" | "-h" => {
                put(out, HELP.render(style).as_bytes())?;
                return Ok(0);
            }
            flag if flag.starts_with('-') => {
                put(
                    err,
                    format!("{}: Unknown flag: {flag}\n", style.red("Error")).as_bytes(),
                )?;
                return Ok(1);
            }
            value if slug.is_empty() => slug = value.to_string(),
            value if resource.is_empty() => resource = value.to_string(),
            _ => {
                put(
                    err,
                    format!(
                        "{}: Too many arguments.\nUsage: {}\n",
                        style.red("Error"),
                        HELP.synopsis_line()
                    )
                    .as_bytes(),
                )?;
                return Ok(1);
            }
        }
    }
    if slug.is_empty() {
        put(
            err,
            format!(
                "{}: {}\n  <resource>: {} (default: tool)\n",
                style.red("Error"),
                HELP.synopsis_line(),
                VALID_RESOURCES.join(" ")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let resource = if resource.is_empty() {
        "tool".to_string()
    } else {
        resource
    };
    if !VALID_RESOURCES.contains(&resource.as_str()) {
        return unknown_resource(style, &resource, err);
    }
    let project = discover()?;
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let mut request = Customize {
        project: &project,
        slug: &slug,
        style,
        yes,
        stdin_tty,
        ask,
    };
    if resource == "tool" {
        request.tool(full, out, err)
    } else {
        request.payload(&resource, out, err)
    }
}

/// One `customize` of a tool's override or payload; `yes`, `stdin_tty`, and
/// `ask` confirm a hooks override.
struct Customize<'a> {
    project: &'a Project,
    slug: &'a str,
    style: &'a Style,
    yes: bool,
    stdin_tty: bool,
    ask: &'a mut dyn FnMut(&str) -> String,
}

impl Customize<'_> {
    /// `_customize_tool`.
    fn tool(&self, full: bool, out: &mut dyn Write, err: &mut dyn Write) -> Result<u8, Error> {
        let (project, slug, style) = (self.project, self.slug, self.style);
        let base = catalog::base_tool_yaml(slug);
        let user_file = project.user_tool_file(slug);
        let shown = user_file.disk_text();
        if base.is_none() && full {
            put(
                err,
                format!(
                    "{}: No base template for '{slug}' — cannot use --full.\nCreate {shown} manually for a custom tool.\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        if user_file.is_file() {
            put(
                out,
                format!(
                    "{}: {shown}\n\nEdit it directly, or remove it to start over.\nSee effective config: {}\n",
                    style.yellow("Override already exists"),
                    style.cyan(&format!("exuno show {slug}"))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        let display = Tool::load(project, slug)?.display_name();
        if let Some(dir) = user_file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        if let (true, Some(base)) = (full, base) {
            std::fs::write(&user_file, base).map_err(|e| Error::io(&user_file, e))?;
            put(
                out,
                format!(
                    "\n{} {shown}\n\nThis is a full copy of the base template. Every field you keep\nwins over future base updates. Remove fields you don't need to\ncustomize — those will inherit from base automatically.\n\n",
                    style.green("Created full override:")
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        let mut stub = format!("# {display} — custom override for Exuno.\n");
        if base.is_some() {
            stub.push_str(&format!(
                "#\n# Only fields you write here are \"owned\" by you.\n# Everything else inherits from the base template and receives updates.\n#\n# See base fields:          exuno show {slug} --base\n# See effective config:     exuno show {slug}\n# See your vs base diff:    exuno diff {slug}\n"
            ));
        } else {
            stub.push_str("#\n# This is a custom tool — no base template exists.\n# Define the full config here, then add to tools.enabled in agent_sync.yaml.\n");
        }
        stub.push('\n');
        std::fs::write(&user_file, stub).map_err(|e| Error::io(&user_file, e))?;
        put(
            out,
            format!(
                "\n{} {shown}\n\nAdd only fields you want to change. Everything else inherits from base.\nSee overridable fields: {}\n\n",
                style.green("Created empty override:"),
                style.cyan(&format!("exuno show {slug} --base"))
            )
            .as_bytes(),
        )?;
        Ok(0)
    }

    /// `_customize_payload`.
    fn payload(
        &mut self,
        resource: &str,
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<u8, Error> {
        let (project, slug, style) = (self.project, self.slug, self.style);
        let tool = Tool::load(project, slug)?;
        let (Some(base), Some(user_file)) = (
            payload::base_source(&tool, resource),
            payload::override_path(project, &tool, resource),
        ) else {
            put(
                err,
                format!(
                    "{}: No base {resource} template for '{slug}'.\n\nEither '{slug}' is unknown, or this tool doesn't ship a {resource} template.\nRun {} to see available tools.\n",
                    style.red("Error"),
                    style.cyan("exuno list")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        };
        let shown = user_file.disk_text();
        self.migrate_legacy(&tool, resource, &user_file, out)?;
        if user_file.is_file() {
            put(
                out,
                format!(
                    "{}: {shown}\n\nEdit it directly, or remove it to start over.\nSee effective source:  {}\nSee your vs base diff: {}\n",
                    style.yellow("Override already exists"),
                    style.cyan(&format!("exuno show {slug} {resource}")),
                    style.cyan(&format!("exuno diff {slug} {resource}"))
                )
                .as_bytes(),
            )?;
            return Ok(0);
        }
        let base_bytes = base.bytes()?;
        if resource == "hooks" {
            if let Err(status) = self.confirm_hooks(&tool, &base, &base_bytes, out, err)? {
                return Ok(status);
            }
        }
        if let Some(dir) = user_file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        std::fs::write(&user_file, &base_bytes).map_err(|e| Error::io(&user_file, e))?;
        put(
            out,
            format!(
                "\n{} {shown}\n\n{}\n\nEdit this file to customize. Remove it to fall back to the base template.\nSee diff vs base: {}\n\n",
                style.green(&format!("Created {resource} override:")),
                style.dim(&format!("  source (base): {}", base.shown())),
                style.cyan(&format!("exuno diff {slug} {resource}"))
            )
            .as_bytes(),
        )?;
        Ok(0)
    }

    /// A legacy-layout override moved to `user_file` when nothing is there yet.
    fn migrate_legacy(
        &self,
        tool: &Tool,
        resource: &str,
        user_file: &Path,
        out: &mut dyn Write,
    ) -> Result<(), Error> {
        let Some(legacy) = payload::legacy_override_path(self.project, tool, resource) else {
            return Ok(());
        };
        if !legacy.is_file() || user_file.is_file() {
            return Ok(());
        }
        if let Some(dir) = user_file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
        }
        std::fs::rename(&legacy, user_file).map_err(|e| Error::io(&legacy, e))?;
        put(
            out,
            format!(
                "{} {} → {}\n",
                self.style.yellow("Migrated legacy override:"),
                relative(self.project, &legacy),
                relative(self.project, user_file)
            )
            .as_bytes(),
        )
    }

    /// The base hooks shown, then `--yes` or a `y` at the prompt; the status
    /// to return when the override is refused or cancelled.
    fn confirm_hooks(
        &mut self,
        tool: &Tool,
        base: &payload::Source,
        base_bytes: &[u8],
        out: &mut dyn Write,
        err: &mut dyn Write,
    ) -> Result<Result<(), u8>, Error> {
        let style = self.style;
        put(
            out,
            format!(
                "\n{}\nHooks can run shell commands after sync. Review the base template\nbelow before copying it — anything you put here will run locally.\n\n{}\n\n",
                style.yellow(&format!("!  You are about to override hooks for {}.", tool.display_name())),
                style.dim(&format!("  Base: {}", base.shown()))
            )
            .as_bytes(),
        )?;
        put(out, &text::sed_indent(base_bytes))?;
        put(out, b"\n")?;
        if self.yes {
            return Ok(Ok(()));
        }
        if !self.stdin_tty {
            put(
                err,
                format!(
                    "{}: Refusing to scaffold hook override in non-interactive mode.\nRe-run with {} to confirm.\n",
                    style.red("Error"),
                    style.cyan("--yes")
                )
                .as_bytes(),
            )?;
            return Ok(Err(1));
        }
        let reply = (self.ask)(&style.bold("Create this override? [y/N] "));
        if matches!(reply.as_str(), "y" | "Y" | "yes" | "YES") {
            return Ok(Ok(()));
        }
        put(out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
        Ok(Err(0))
    }
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    pub(crate) struct Run {
        pub status: u8,
        pub out: String,
        pub err: String,
    }

    pub(crate) fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.ai")).unwrap();
        std::fs::write(
            format!("{root}/.ai/agent_sync.yaml"),
            "tools:\n  enabled:\n    - cursor\n",
        )
        .unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str]) -> Run {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = customize(
            &args,
            &discover,
            &Style::plain(),
            false,
            &mut |_| String::new(),
            &mut out,
            &mut err,
        )
        .unwrap();
        Run {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    #[test]
    fn customize_writes_a_stub_and_refuses_hooks_without_yes() {
        let (_dir, root) = project();
        let created = call(&root, &["claude"]);
        assert_eq!(created.status, 0);
        assert_eq!(
            created.out,
            format!(
                "\nCreated empty override: {root}/.ai/src/tools/claude.yaml\n\nAdd only fields you want to change. Everything else inherits from base.\nSee overridable fields: exuno show claude --base\n\n"
            )
        );
        assert!(
            std::fs::read_to_string(format!("{root}/.ai/src/tools/claude.yaml"))
                .unwrap()
                .starts_with("# Claude Code — custom override for Exuno.\n#\n# Only fields")
        );
        assert_eq!(
            call(&root, &["claude"]).out,
            format!(
                "Override already exists: {root}/.ai/src/tools/claude.yaml\n\nEdit it directly, or remove it to start over.\nSee effective config: exuno show claude\n"
            )
        );

        let hooks = call(&root, &["cursor", "hooks"]);
        assert_eq!(hooks.status, 1);
        assert_eq!(
            hooks.out,
            "\n!  You are about to override hooks for Cursor.\nHooks can run shell commands after sync. Review the base template\nbelow before copying it — anything you put here will run locally.\n\n  Base: /<agentsync>/lib/templates/hooks/cursor.json\n\n    {\n      \"version\": 1,\n      \"hooks\": {}\n    }\n\n"
        );
        assert_eq!(
            hooks.err,
            "Error: Refusing to scaffold hook override in non-interactive mode.\nRe-run with --yes to confirm.\n"
        );
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/cursor/hooks.json")).exists());

        let usage = call(&root, &[]);
        assert_eq!(usage.status, 1);
        assert_eq!(
            usage.err,
            "Error: exuno customize <slug> [<resource>] [--full] [--yes]\n  <resource>: tool hooks mcp settings (default: tool)\n"
        );
        let help = call(&root, &["--help"]);
        assert_eq!((help.status, help.err.as_str()), (0, ""));
        assert_eq!(
            help.out,
            "\n  exuno customize — scaffold a per-tool override\n\n  USAGE\n    exuno customize <slug> [<resource>] [--full] [--yes]\n\n  DESCRIPTION\n    Scaffold a per-tool override at .ai/src/tools/<slug>.yaml so you can\n    change fields without forking the whole base template. Empty by default:\n    write only the keys you want to win over base; everything else inherits.\n\n  ARGUMENTS\n    <slug>       Tool to override\n    <resource>   Payload override scaffold: tool, hooks, mcp, settings\n                 (default: tool, the YAML override itself)\n\n  OPTIONS\n    --full       Copy the entire base template into the override. Use when you\n                 want to see every available field at once; trim what you don't\n                 need with exuno simplify\n    -y, --yes    Overwrite an existing override without prompting\n    -h, --help   Show this help\n\n  SEE ALSO\n    exuno show <slug>   Effective config for a tool\n    exuno diff <slug>   User vs base diff\n\n  EXAMPLES\n    exuno customize cursor\n    exuno customize cursor --full\n    exuno customize claude hooks --yes\n\n"
        );
    }

    #[test]
    fn a_hooks_override_on_a_terminal_follows_the_answer() {
        let (_dir, root) = project();
        let hooks = format!("{root}/.ai/src/tools/cursor/hooks.json");
        let answer = |reply: &str| {
            let args = vec!["cursor".to_string(), "hooks".to_string()];
            let discover = || Project::at(&root);
            let (mut out, mut err) = (Vec::new(), Vec::new());
            let status = customize(
                &args,
                &discover,
                &Style::plain(),
                true,
                &mut |_| reply.to_string(),
                &mut out,
                &mut err,
            )
            .unwrap();
            (status, String::from_utf8(out).unwrap(), err)
        };
        let (status, out, err) = answer("n");
        assert_eq!((status, err.is_empty()), (0, true));
        assert!(out.ends_with("}\n\nCancelled.\n"), "{out}");
        assert!(!Path::new(&hooks).exists());
        let (status, out, _) = answer("y");
        assert_eq!(status, 0);
        assert!(out.contains("Created hooks override:"), "{out}");
        assert!(Path::new(&hooks).is_file());
    }

    #[test]
    fn a_legacy_payload_moves_into_the_tool_directory() {
        let (_dir, root) = project();
        std::fs::create_dir_all(format!("{root}/.ai/src/mcp")).unwrap();
        std::fs::write(
            format!("{root}/.ai/src/mcp/claude.json"),
            "{\"marker\":\"USER\"}\n",
        )
        .unwrap();
        let moved = call(&root, &["claude", "mcp"]);
        assert_eq!(moved.status, 0);
        assert_eq!(
            moved.out,
            format!(
                "Migrated legacy override: .ai/src/mcp/claude.json → .ai/src/tools/claude/mcp.json\nOverride already exists: {root}/.ai/src/tools/claude/mcp.json\n\nEdit it directly, or remove it to start over.\nSee effective source:  exuno show claude mcp\nSee your vs base diff: exuno diff claude mcp\n"
            )
        );
        assert_eq!(
            call(&root, &["claude", "nope"]).err,
            "Error: Unknown resource 'nope'.\nValid: tool hooks mcp settings\n"
        );
    }
}
