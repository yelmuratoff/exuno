//! `exuno simplify`: `cmd_simplify` of `lib/helpers/simplify.sh`, which
//! drops override fields equal to the base and byte-identical payload copies.

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::customize::relative;
use super::put;
use crate::config::tool::Tool;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::project::Project;
use crate::{Error, config::catalog, config::yaml_edit, config::yaml_subset};

pub const HELP: Help = Help {
    command: "simplify",
    tagline: "drop override fields that match base",
    synopsis: &["simplify [<tool>] [--apply] [-y]"],
    description: &[
        "Removes fields from user overrides when they match the base.\nDry-run by default: pass --apply to persist.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            ("--apply", "Write changes to disk (default: preview)"),
            ("-y, --yes", "Auto-delete empty override files (no prompt)"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &["simplify", "simplify cursor --apply", "simplify --apply -y"],
};

const KEYS: [&str; 31] = [
    "name",
    "enabled",
    "targets.agents.dest",
    "targets.agents.source",
    "targets.rules.dest",
    "targets.rules.source",
    "targets.rules.extension",
    "targets.rules.header",
    "targets.rules.scoped_header",
    "targets.rules.append_imports",
    "targets.rules.merge_to_file",
    "targets.rules.inline_into_agents",
    "targets.rules.prepend_agents",
    "targets.skills.dest",
    "targets.skills.source",
    "targets.skills.inline_into_agents",
    "targets.commands.dest",
    "targets.commands.format",
    "targets.commands.extension",
    "targets.commands.as_skills",
    "targets.commands.inline_into_agents",
    "targets.subagents.dest",
    "targets.subagents.format",
    "targets.subagents.extension",
    "targets.settings.source",
    "targets.settings.dest",
    "targets.mcp.source",
    "targets.mcp.dest",
    "targets.hooks.source",
    "targets.hooks.dest",
    "post_sync",
];

type Ask<'a> = &'a mut dyn FnMut(&str, &mut dyn Write) -> String;

fn yes(answer: &str) -> bool {
    matches!(answer, "y" | "Y" | "yes" | "Yes")
}

fn read_text(path: &Path) -> String {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}

/// `_simplify_file_has_content`.
fn has_content(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r');
    read_text(path).split('\n').any(|line| {
        let stripped = line.trim_start_matches(is_space);
        if line.is_empty() || stripped.starts_with('#') {
            return false;
        }
        let key_end = stripped
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .unwrap_or(stripped.len());
        let assignment = key_end > 0
            && stripped[key_end..]
                .strip_prefix(':')
                .is_some_and(|rest| rest.starts_with(is_space) && rest.chars().count() >= 2);
        let item = stripped
            .strip_prefix('-')
            .is_some_and(|rest| rest.starts_with(is_space));
        assignment || item
    })
}

pub fn simplify(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    style: &Style,
    interactive: bool,
    ask: Ask,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (apply, auto_yes, filter) = match simplify_flags(args) {
        Ok(flags) => flags,
        Err(None) => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Err(Some(message)) => {
            put(
                err,
                format!("{}: {message}\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(1);
        }
    };
    let project = discover()?;
    if apply && !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let mut pass = Pass {
        project: &project,
        apply,
        auto_yes,
        interactive,
        style,
        ask,
        out,
    };
    let mut matched = false;
    for slug in project.user_override_tools()? {
        if !filter.is_empty() && slug != filter {
            continue;
        }
        matched = true;
        pass.tool(&slug)?;
    }
    if pass.payloads(&filter)? {
        matched = true;
    }
    if !matched && !filter.is_empty() {
        let text = format!(
            "{}: No override found for '{filter}'.\n",
            style.red("Error")
        );
        put(err, text.as_bytes())?;
        return Ok(1);
    }
    let footer = if !matched {
        format!(
            "\n  {}\n\n",
            style.dim("No user overrides — nothing to simplify.")
        )
    } else if apply {
        format!(
            "\n{}\n  Run {} to verify outputs are unchanged.\n\n",
            style.green("Done."),
            style.cyan("exuno sync")
        )
    } else {
        format!("\n{}\n\n", style.dim("Dry run — pass --apply to persist."))
    };
    put(pass.out, footer.as_bytes())?;
    Ok(0)
}

/// `--apply`, `--yes`, and the tool filter; `Err(None)` asks for the help,
/// `Err(Some(message))` refuses the command line.
fn simplify_flags(args: &[String]) -> Result<(bool, bool, String), Option<String>> {
    let (mut apply, mut auto_yes, mut filter) = (false, false, String::new());
    for arg in args {
        match arg.as_str() {
            "--apply" => apply = true,
            "-y" | "--yes" => auto_yes = true,
            "-h" | "--help" => return Err(None),
            flag if flag.starts_with('-') => return Err(Some(format!("Unknown flag: {flag}"))),
            value if filter.is_empty() => filter = value.to_string(),
            _ => return Err(Some("Only one tool at a time.".into())),
        }
    }
    Ok((apply, auto_yes, filter))
}

/// One `simplify` run: the project, the flags, and the streams it reports to.
struct Pass<'a> {
    project: &'a Project,
    apply: bool,
    auto_yes: bool,
    interactive: bool,
    style: &'a Style,
    ask: Ask<'a>,
    out: &'a mut dyn Write,
}

/// A tool override's fields: equal to the shipped value, different from it,
/// and absent from it.
#[derive(Default)]
struct Fields {
    redundant: Vec<(&'static str, String)>,
    kept: Vec<(&'static str, String)>,
    user_only: Vec<(&'static str, String)>,
}

impl Fields {
    fn of(user_text: &str, base: Option<&str>) -> Self {
        let mut fields = Self::default();
        for key in KEYS {
            let user = yaml_subset::value(user_text, key);
            if user.is_empty() {
                continue;
            }
            let shipped = base.map(|t| yaml_subset::value(t, key)).unwrap_or_default();
            if !shipped.is_empty() && user == shipped {
                fields.redundant.push((key, user));
            } else if shipped.is_empty() {
                fields.user_only.push((key, user));
            } else {
                fields.kept.push((key, user));
            }
        }
        fields
    }

    fn report(&self, style: &Style) -> String {
        let mut text = format!("  {}\n", style.yellow("Redundant (match base):"));
        for (key, value) in &self.redundant {
            text.push_str(&format!("    {} {key:<42}  {value}\n", style.dim("-")));
        }
        text.push('\n');
        for (title, list) in [
            ("  Kept (diverge from base):", &self.kept),
            ("  Kept (no base value):", &self.user_only),
        ] {
            if list.is_empty() {
                continue;
            }
            text.push_str(&format!("{}\n", style.dim(title)));
            for (key, value) in list {
                text.push_str(&format!("    {} {key:<42}  {value}\n", style.dim("=")));
            }
            text.push('\n');
        }
        text
    }
}

/// The payload overrides a pass considers: byte-identical to base, diverging
/// or without base, and still in the flat legacy layout.
#[derive(Default)]
struct Payloads {
    redundant: Vec<PathBuf>,
    kept: Vec<PathBuf>,
    legacy: Vec<PathBuf>,
    matched: bool,
}

impl Payloads {
    fn of(project: &Project, filter: &str) -> Result<Self, Error> {
        let src = project.root.join(".ai").join("src");
        let mut payloads = Self::default();
        for (tool, dir) in sorted_entries(&project.user_tools_dir()) {
            if !dir.is_dir() || (!filter.is_empty() && tool != filter) {
                continue;
            }
            let loaded = Tool::load(project, &tool)?;
            for resource in ["hooks", "mcp", "settings"] {
                for (name, file) in sorted_entries(&dir) {
                    if !name.starts_with(&format!("{resource}.")) || !file.is_file() {
                        continue;
                    }
                    payloads.matched = true;
                    let identical = loaded.base_payload(resource).is_some_and(|base| {
                        std::fs::read(&file).is_ok_and(|bytes| bytes == base.contents())
                    });
                    if identical {
                        payloads.redundant.push(file);
                    } else {
                        payloads.kept.push(file);
                    }
                }
            }
        }
        for resource in ["hooks", "mcp", "settings"] {
            for (name, file) in sorted_entries(&src.join(resource)) {
                let tool = name
                    .rsplit_once('.')
                    .map_or(name.as_str(), |(stem, _)| stem);
                if !file.is_file() || (!filter.is_empty() && tool != filter) {
                    continue;
                }
                payloads.matched = true;
                payloads.legacy.push(file);
            }
        }
        Ok(payloads)
    }

    fn legacy_report(&self, project: &Project, style: &Style) -> String {
        let mut text = format!(
            "  {}\n",
            style.yellow(
                "Legacy layout — move into .ai/src/tools/<tool>/ (flat layout is deprecated):"
            )
        );
        for file in &self.legacy {
            text.push_str(&format!(
                "    {} {}\n",
                style.dim("·"),
                relative(project, file)
            ));
        }
        text.push_str(&format!(
            "\n{}\n{}\n\n",
            style.dim(&format!(
                "  Run {} to preview the migration,",
                style.cyan("exuno migrate --legacy")
            )),
            style.dim(&format!(
                "  then {} to move these files.",
                style.cyan("exuno migrate --apply")
            ))
        ));
        text
    }

    fn redundant_report(&self, project: &Project, style: &Style) -> String {
        let mut text = format!(
            "  {}\n",
            style.yellow("Byte-identical to base (safe to delete):")
        );
        for file in &self.redundant {
            text.push_str(&format!(
                "    {} {}\n",
                style.dim("-"),
                relative(project, file)
            ));
        }
        text.push('\n');
        if !self.kept.is_empty() {
            let kept = format!(
                "  Kept (diverge from base or no base): {} file(s)",
                self.kept.len()
            );
            text.push_str(&format!("{}\n", style.dim(&kept)));
        }
        text
    }
}

impl Pass<'_> {
    fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn confirmed(&mut self, question: &str) -> bool {
        let prompt = self.style.bold(question);
        yes(&(self.ask)(&prompt, self.out))
    }

    /// `_simplify_one_tool`.
    fn tool(&mut self, slug: &str) -> Result<(), Error> {
        let style = self.style;
        let user_file = self.project.user_tool_file(slug);
        if !user_file.is_file() {
            return Ok(());
        }
        let rel = relative(self.project, &user_file);
        let display = Tool::load(self.project, slug)?.display_name();
        self.say(&format!(
            "\n{}\n{}\n",
            style.bold(&format!("  {display}")),
            style.dim(&format!("  override: {rel}"))
        ))?;
        let fields = Fields::of(&read_text(&user_file), catalog::base_tool_yaml(slug));
        if fields.redundant.is_empty() {
            return self.say(&format!(
                "{}\n",
                style.dim("  No redundant fields — already minimal.")
            ));
        }
        let mut text = fields.report(style);
        if !self.apply {
            let hint = if fields.kept.len() + fields.user_only.len() == 0 {
                "  → would delete the override file (all fields match base).".to_string()
            } else {
                format!("  → would remove {} field(s).", fields.redundant.len())
            };
            text.push_str(&format!("{}\n\n", style.dim(&hint)));
            return self.say(&text);
        }
        self.say(&text)?;
        for (key, _) in &fields.redundant {
            yaml_edit::remove_key(&user_file, key)?;
        }
        self.say(&format!(
            "  {} {} field(s).\n",
            style.green("Removed"),
            fields.redundant.len()
        ))?;
        if !has_content(&user_file) {
            self.drop_empty_override(&user_file, &rel)?;
        }
        self.say("\n")
    }

    fn drop_empty_override(&mut self, user_file: &Path, rel: &str) -> Result<(), Error> {
        let style = self.style;
        let delete = self.auto_yes
            || (self.interactive && self.confirmed("Delete empty override file? [y/N]"));
        if !delete {
            let kept = style.dim("  Kept empty file — remove manually if desired.");
            return self.say(&format!("{kept}\n"));
        }
        std::fs::remove_file(user_file).map_err(|e| Error::io(user_file, e))?;
        self.say(&format!("  {} {rel}\n", style.green("Deleted")))
    }

    /// `_simplify_payload_overrides`: whether any payload was considered.
    fn payloads(&mut self, filter: &str) -> Result<bool, Error> {
        let style = self.style;
        let payloads = Payloads::of(self.project, filter)?;
        if payloads.redundant.is_empty() && payloads.kept.is_empty() && payloads.legacy.is_empty() {
            return Ok(payloads.matched);
        }
        let mut text = format!("\n{}\n\n", style.bold("  Payload overrides"));
        if !payloads.legacy.is_empty() {
            text.push_str(&payloads.legacy_report(self.project, style));
        }
        if payloads.redundant.is_empty() {
            if !payloads.kept.is_empty() {
                let note = format!(
                    "  No byte-identical payload overrides — {} real customization(s).",
                    payloads.kept.len()
                );
                text.push_str(&format!("{}\n", style.dim(&note)));
            }
            self.say(&text)?;
            return Ok(payloads.matched);
        }
        text.push_str(&payloads.redundant_report(self.project, style));
        if !self.apply {
            let note = format!(
                "  → would delete {} payload override(s).",
                payloads.redundant.len()
            );
            text.push_str(&format!("{}\n", style.dim(&note)));
            self.say(&text)?;
            return Ok(payloads.matched);
        }
        self.say(&text)?;
        self.delete_payloads(&payloads.redundant)?;
        Ok(payloads.matched)
    }

    fn delete_payloads(&mut self, redundant: &[PathBuf]) -> Result<(), Error> {
        let style = self.style;
        let (mut deleted, mut skipped) = (0usize, 0usize);
        for file in redundant {
            let rel = relative(self.project, file);
            let delete = self.auto_yes
                || (self.interactive && self.confirmed(&format!("Delete {rel}? [y/N]")));
            if !delete {
                let kept = if self.interactive {
                    format!("  Kept {rel}")
                } else {
                    format!("  Kept {rel} (not a terminal; pass -y to delete)")
                };
                self.say(&format!("{}\n", style.dim(&kept)))?;
                skipped += 1;
                continue;
            }
            std::fs::remove_file(file).map_err(|e| Error::io(file, e))?;
            if let Some(dir) = file.parent() {
                let _ = std::fs::remove_dir(dir);
            }
            self.say(&format!("  {} {rel}\n", style.green("Deleted")))?;
            deleted += 1;
        }
        self.say(&format!(
            "\n{}\n",
            style.dim(&format!("  Removed {deleted}, kept {skipped}."))
        ))
    }
}

fn sorted_entries(dir: &Path) -> Vec<(String, PathBuf)> {
    let mut entries: Vec<(String, PathBuf)> = std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| (e.file_name().disk_text(), e.path()))
                .filter(|(name, _)| !name.starts_with('.'))
                .collect()
        })
        .unwrap_or_default();
    entries.sort();
    entries
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.ai/src/tools")).unwrap();
        std::fs::write(
            format!("{root}/.ai/agent_sync.yaml"),
            "tools:\n  enabled:\n    - cursor\n",
        )
        .unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str], interactive: bool, answer: &str) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = simplify(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |prompt, out| {
                let _ = out.write_all(format!("  {prompt} ").as_bytes());
                answer.to_string()
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

    #[test]
    fn a_dry_run_lists_redundant_and_kept_fields_like_bash() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["--help"], false, ""),
            (
                0,
                "\n  exuno simplify — drop override fields that match base\n\n  USAGE\n    exuno simplify [<tool>] [--apply] [-y]\n\n  DESCRIPTION\n    Removes fields from user overrides when they match the base.\n    Dry-run by default: pass --apply to persist.\n\n  OPTIONS\n    --apply      Write changes to disk (default: preview)\n    -y, --yes    Auto-delete empty override files (no prompt)\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno simplify\n    exuno simplify cursor --apply\n    exuno simplify --apply -y\n\n".to_string(),
                String::new()
            )
        );
        assert_eq!(
            call(&root, &[], false, "").1,
            "\n  No user overrides — nothing to simplify.\n\n"
        );
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(
            &file,
            "name: \"Cursor\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".cursor/rules\"\n    extension: \".mdcustom\"\n  custom:\n    x: 1\n",
        )
        .unwrap();
        let (status, out, _) = call(&root, &[], false, "");
        assert_eq!(status, 0);
        assert_eq!(
            out,
            format!(
                "\n  Cursor\n  override: .ai/src/tools/cursor.yaml\n  Redundant (match base):\n    - {:<42}  Cursor\n    - {:<42}  .cursor/rules\n\n  Kept (diverge from base):\n    = {:<42}  true\n    = {:<42}  .mdcustom\n\n  → would remove 2 field(s).\n\n\nDry run — pass --apply to persist.\n\n",
                "name", "targets.rules.dest", "enabled", "targets.rules.extension"
            )
        );
        assert_eq!(
            call(&root, &["nope"], false, "").2,
            "Error: No override found for 'nope'.\n"
        );

        let (_, applied, _) = call(&root, &["--apply"], false, "");
        assert!(applied.contains("  Removed 2 field(s).\n\n\nDone.\n  Run exuno sync to verify outputs are unchanged.\n\n"));
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "enabled: true\n\ntargets:\n  rules:\n    extension: \".mdcustom\"\n  custom:\n    x: 1\n"
        );
    }

    #[test]
    fn an_emptied_override_is_kept_off_a_terminal_and_deleted_on_yes() {
        let (_dir, root) = project();
        let file = format!("{root}/.ai/src/tools/cursor.yaml");
        std::fs::write(&file, "name: \"Cursor\"\n").unwrap();
        let (_, kept, _) = call(&root, &["--apply"], false, "");
        assert!(kept.ends_with("  Removed 1 field(s).\n  Kept empty file — remove manually if desired.\n\n\nDone.\n  Run exuno sync to verify outputs are unchanged.\n\n"));
        assert!(std::path::Path::new(&file).is_file());

        std::fs::write(&file, "name: \"Cursor\"\n").unwrap();
        let (_, deleted, _) = call(&root, &["--apply"], true, "y");
        assert!(deleted.contains("  Removed 1 field(s).\n  Delete empty override file? [y/N]   Deleted .ai/src/tools/cursor.yaml\n"));
        assert!(!std::path::Path::new(&file).exists());

        std::fs::create_dir_all(format!("{root}/.ai/src/tools/cursor")).unwrap();
        std::fs::write(
            format!("{root}/.ai/src/tools/cursor/hooks.json"),
            crate::config::catalog::base_payload("hooks", "cursor")
                .unwrap()
                .contents(),
        )
        .unwrap();
        let (_, payload, _) = call(&root, &["--apply"], true, "n");
        assert!(payload.contains("  Delete .ai/src/tools/cursor/hooks.json? [y/N]   Kept .ai/src/tools/cursor/hooks.json\n\n  Removed 0, kept 1.\n"));
        let (_, off_terminal, _) = call(&root, &["--apply"], false, "");
        assert!(off_terminal.contains(
            "  Kept .ai/src/tools/cursor/hooks.json (not a terminal; pass -y to delete)\n\n  Removed 0, kept 1.\n"
        ), "{off_terminal}");
        assert!(std::path::Path::new(&format!("{root}/.ai/src/tools/cursor/hooks.json")).is_file());
        let (_, gone, _) = call(&root, &["--apply", "-y"], false, "");
        assert!(
            gone.contains("  Deleted .ai/src/tools/cursor/hooks.json\n\n  Removed 1, kept 0.\n")
        );
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/cursor")).exists());
    }
}
