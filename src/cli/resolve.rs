//! `exuno resolve`: `cmd_resolve` of `lib/helpers/resolve_cmd.sh`, which
//! walks every overridden field on a terminal and lets the base value win.

use std::io::Write;

use super::customize::relative;
use super::diff::KEYS;
use super::put;
use crate::config::tool::Tool;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::project::Project;
use crate::{Error, config::catalog, config::snapshot, config::yaml_edit, config::yaml_subset};

pub const HELP: Help = Help {
    command: "resolve",
    tagline: "interactively reconcile overrides with base values",
    synopsis: &["resolve [<slug>]"],
    description: &[
        "Walks every field in .ai/src/tools/<slug>.yaml that differs from the\nshipped base and asks, one field at a time, whether to [k]eep the\noverride, [a]dopt the base value (the field is removed from the\noverride file), or [s]kip it for now. With a slug, only that tool is\nreviewed.",
        "A field the last exuno update flagged, because upstream changed\nits base value while the override was in place, is marked with ⚡. A\nfull walk clears the flags.",
        "Without a terminal the command is read-only and says so; use\nexuno diff for a plain listing. Run exuno sync afterwards to\napply what was adopted.",
    ],
    sections: &[
        Section {
            title: "ARGUMENTS",
            entries: &[("<slug>", "Review only this tool's override")],
        },
        Section {
            title: "OPTIONS",
            entries: &[("-h, --help", "Show this help")],
        },
        Section {
            title: "EXIT STATUS",
            entries: &[
                ("0", "The walk finished, or there was nothing to resolve"),
                ("1", "No override exists for <slug>"),
            ],
        },
        Section {
            title: "SEE ALSO",
            entries: &[
                ("exuno diff", "List every override against its base"),
                ("exuno update", "Where the ⚡ flags come from"),
            ],
        },
    ],
    examples: &["resolve", "resolve cursor"],
};

type Ask<'a> = &'a mut dyn FnMut(&str, &mut dyn Write) -> String;

pub fn resolve(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    interactive: bool,
    ask: Ask,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if matches!(args.first().map(String::as_str), Some("--help" | "-h")) {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let filter = args.first().cloned().unwrap_or_default();
    let project = discover()?;
    let pending = snapshot::read_pending_pairs(&project.root);
    let overrides = project.user_override_tools()?;
    if overrides.is_empty() {
        put(
            out,
            format!(
                "\n  {}\n\n",
                style.dim("No user overrides — nothing to resolve.")
            )
            .as_bytes(),
        )?;
        // Read-only without a terminal, as the branch below promises.
        if interactive && !pending.is_empty() {
            snapshot::clear_pending(&project.root);
        }
        return Ok(0);
    }
    if !interactive {
        put(
            out,
            format!(
                "\n{}\n  Run from an interactive shell to review overrides one by one.\n  Use {} for a full list.\n\n",
                style.bold("  Resolve (read-only — not a TTY)"),
                style.cyan("exuno diff")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    if !pending.is_empty() {
        put(
            out,
            format!(
                "\n  {} {}\n  {}\n  {} {} {}\n",
                style.yellow(&format!(
                    "⚡ {} field(s) flagged by the last",
                    pending.len()
                )),
                style.cyan("exuno update"),
                style.dim("Upstream changed base values while you had overrides. Flagged entries"),
                style.dim("are marked with"),
                style.yellow("⚡"),
                style.dim("below.")
            )
            .as_bytes(),
        )?;
    }
    let mut matched = false;
    for slug in overrides
        .iter()
        .filter(|t| filter.is_empty() || **t == filter)
    {
        resolve_tool(&project, slug, &pending, style, ask, out)?;
        matched = true;
    }
    if !matched {
        put(
            err,
            format!(
                "{}: No override found for '{filter}'.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if !pending.is_empty() && filter.is_empty() {
        snapshot::clear_pending(&project.root);
    }
    put(
        out,
        format!(
            "\n{}\n  Run {} to apply any changes.\n\n",
            style.green("Done."),
            style.cyan("exuno sync")
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// `_resolve_one_tool`.
fn resolve_tool(
    project: &Project,
    slug: &str,
    pending: &[(String, String)],
    style: &Style,
    ask: Ask,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let base = catalog::base_tool_yaml(slug);
    let user_file = project.user_tool_file(slug);
    let base_line = if base.is_some() {
        format!("  base:     lib/templates/tools/{slug}.yaml")
    } else {
        "  base:     (custom tool — no base)".to_string()
    };
    put(
        out,
        format!(
            "\n{}\n{}\n{}\n\n",
            style.bold(&format!("  {}", Tool::load(project, slug)?.display_name())),
            style.dim(&format!("  override: {}", relative(project, &user_file))),
            style.dim(&base_line)
        )
        .as_bytes(),
    )?;
    let mut any = false;
    for key in KEYS {
        let user_text = std::fs::read(&user_file)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let user = yaml_subset::value(&user_text, key);
        let shipped = base.map(|t| yaml_subset::value(t, key)).unwrap_or_default();
        if user.is_empty() || user == shipped {
            continue;
        }
        any = true;
        let flagged = pending
            .iter()
            .any(|(tool, field)| tool == slug && field == key);
        let marker = style.yellow(if flagged { "⚡" } else { "◆" });
        let shipped_shown = if shipped.is_empty() {
            style.dim("(not set)")
        } else {
            shipped
        };
        put(
            out,
            format!(
                "    {marker} {key}\n        {} {user}\n        {} {shipped_shown}\n",
                style.dim("user:"),
                style.dim("base:")
            )
            .as_bytes(),
        )?;
        let answer = ask(&style.bold("[k]eep / [a]dopt base / [s]kip"), out);
        let outcome = match answer.as_str() {
            "a" | "A" | "adopt" => {
                yaml_edit::remove_key(&user_file, key)?;
                style.green("→ adopted base value")
            }
            "s" | "S" | "skip" | "" => style.dim("→ skipped"),
            _ => style.dim("→ kept user value"),
        };
        put(out, format!("        {outcome}\n\n").as_bytes())?;
    }
    if !any {
        put(
            out,
            format!("{}\n", style.dim("    No diverging fields.")).as_bytes(),
        )?;
    }
    Ok(())
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    fn call(
        root: &str,
        args: &[&str],
        interactive: bool,
        answers: &[&str],
    ) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let mut answers = answers.iter();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = resolve(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |prompt, out| {
                let _ = out.write_all(format!("        {prompt} ").as_bytes());
                answers.next().copied().unwrap_or("").to_string()
            },
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

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools")).unwrap();
        (dir, root)
    }

    #[test]
    fn help_is_answered_on_stdout_before_the_project_is_discovered() {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = resolve(
            &["-h".to_string()],
            &|| panic!("--help must not discover the project"),
            &Style::plain(),
            false,
            &mut |_, _| String::new(),
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(err, b"");
        assert_eq!(
            String::from_utf8(out).unwrap(),
            HELP.render(&Style::plain())
        );
        assert!(HELP.render(&Style::plain()).starts_with(
            "\n  exuno resolve — interactively reconcile overrides with base values\n\n  USAGE\n    exuno resolve [<slug>]\n"
        ));
    }

    #[test]
    fn resolve_without_a_terminal_reports_and_leaves_the_queue_alone() {
        let (_dir, root) = project();
        let pending = format!("{root}/.ai/.pending-resolutions.yaml");
        std::fs::write(
            &pending,
            "conflicts:\n  - tool: \"cursor\"\n    field: \"name\"\n",
        )
        .unwrap();
        assert_eq!(
            call(&root, &[], false, &[]),
            (
                0,
                "\n  No user overrides — nothing to resolve.\n\n".to_string(),
                String::new()
            )
        );
        // Was known quirk 24: the queue was cleared before the terminal
        // check one branch below promised the run would change nothing.
        assert!(std::path::Path::new(&pending).exists());

        std::fs::write(format!("{root}/.ai/src/tools/cursor.yaml"), "name: Mine\n").unwrap();
        assert_eq!(
            call(&root, &["nope"], false, &[]).1,
            "\n  Resolve (read-only — not a TTY)\n  Run from an interactive shell to review overrides one by one.\n  Use exuno diff for a full list.\n\n"
        );
    }

    #[test]
    fn an_interactive_walk_adopts_keeps_and_marks_flagged_fields() {
        let (_dir, root) = project();
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(
            &file,
            "name: Mine\ntargets:\n  rules:\n    dest: \".mine\"\n",
        )
        .unwrap();
        std::fs::write(
            format!("{root}/.ai/.pending-resolutions.yaml"),
            "conflicts:\n  - tool: \"cursor\"\n    field: \"targets.rules.dest\"\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &[], true, &["k", "a"]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "\n  ⚡ 1 field(s) flagged by the last exuno update\n  Upstream changed base values while you had overrides. Flagged entries\n  are marked with ⚡ below.\n\n  Mine\n  override: .ai/src/tools/cursor.yaml\n  base:     lib/templates/tools/cursor.yaml\n\n    ◆ name\n        user: Mine\n        base: Cursor\n        [k]eep / [a]dopt base / [s]kip         → kept user value\n\n    ⚡ targets.rules.dest\n        user: .mine\n        base: .cursor/rules\n        [k]eep / [a]dopt base / [s]kip         → adopted base value\n\n\nDone.\n  Run exuno sync to apply any changes.\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "name: Mine\ntargets:\n  rules:\n"
        );
        assert!(!std::path::Path::new(&format!("{root}/.ai/.pending-resolutions.yaml")).exists());
        assert_eq!(
            call(&root, &["nope"], true, &[]).2,
            "Error: No override found for 'nope'.\n"
        );
    }
}
