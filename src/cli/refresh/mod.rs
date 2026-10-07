//! `exuno refresh`: `cmd_refresh` of `lib/helpers/refresh.sh`, which pulls
//! updated templates into `.ai/src/` through a three-way diff against the
//! template manifest, so untouched files update silently and only true
//! conflicts wait for an answer.

mod apply;
mod args;
mod classify;
mod report;
mod session;

use std::io::Write;
use std::path::Path;

use super::put;
use crate::config::template_manifest::TemplateManifest;
use crate::output::style::Style;
use crate::{Error, config::catalog};

pub use args::HELP;
use args::{parse_args, resolve_scope};
use classify::{Classifier, Locator, load_overrides};
use report::Report;
use session::Run;
pub(crate) use session::write_template;

/// Printed where Bash printed `$AGENTSYNC_HOME/lib/templates`; the binary
/// reads the embedded copy, so the line names the release it came from.
fn templates_display() -> String {
    format!("shipped with exuno v{}", crate::engine_version())
}

/// What `refresh` takes from the terminal.
pub struct Env<'a> {
    /// `is_tty`: stdin and stdout are both terminals.
    pub interactive: bool,
    /// `read -r reply </dev/tty`, empty when the terminal cannot be read.
    pub read_line: &'a mut dyn FnMut() -> String,
    /// `AGENTSYNC_CONFIG_PATH`, which picks the config `template_overrides`
    /// comes from.
    pub config_path: Option<&'a str>,
}

/// The source directory below `root`: `.ai/src`, else a flat `.ai`.
fn src_base(root: &str) -> Option<&'static str> {
    [".ai/src", ".ai"]
        .into_iter()
        .find(|dir| Path::new(root).join(dir).is_dir())
}

fn fail(err: &mut dyn Write, style: &Style, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

pub fn refresh(
    args: &[String],
    root: &str,
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let options = match parse_args(args, style, out, err)? {
        Ok(options) => options,
        Err(status) => return Ok(status),
    };

    let Some(src_base) = src_base(root) else {
        let message = format!(
            "No .ai/ directory found in {root}\nRun {} first.",
            style.cyan("exuno init")
        );
        return fail(err, style, &message);
    };
    let user_base_shown = format!("{root}/{src_base}");
    let locator = Locator::new(&user_base_shown);

    let categories = match resolve_scope(&options.only, &user_base_shown, style, err)? {
        Ok(categories) => categories,
        Err(status) => return Ok(status),
    };

    let manifest = TemplateManifest::load(Path::new(root))?;
    let has_manifest = !manifest.is_empty();
    let overrides = match load_overrides(root, env.config_path) {
        Ok(overrides) => overrides,
        Err(message) => return fail(err, style, &message),
    };
    let templates = catalog::template_files();
    let changes = Classifier {
        locator: &locator,
        manifest: &manifest,
        overrides: &overrides,
        review: options.review,
    }
    .collect(&templates, &categories, options.include_agents_md);
    let report = Report {
        style,
        options: &options,
    };

    let mut run = Run {
        style,
        env,
        locator,
        manifest,
        out,
        err,
    };

    if options.status_only {
        run.print_status(&overrides.declined, &changes.deleted)?;
        return Ok(0);
    }
    run.say(&report.header(&user_base_shown, &categories, has_manifest))?;

    let visible_deleted = options.include_deleted && !changes.deleted.is_empty();
    if !changes.offers_anything(visible_deleted) {
        run.say(&report.up_to_date(&changes, overrides.declined.len()))?;
        run.heal(&templates);
        if !options.dry_run {
            run.manifest.write(Path::new(root))?;
        }
        run.say("\n")?;
        return Ok(0);
    }

    run.say(&report.summary(&changes, visible_deleted))?;
    run.list_proposed(&changes, visible_deleted)?;

    if options.dry_run {
        run.say(&report.dry_run())?;
        return Ok(0);
    }
    if !run.env.interactive && !options.assume_yes {
        put(run.err, report.not_a_tty().as_bytes())?;
        return Ok(1);
    }

    let tally = run.apply(&changes, options.assume_yes, visible_deleted)?;
    run.heal(&templates);
    run.manifest.write(Path::new(root))?;
    run.say(&report.closing(&tally, changes.unchanged))?;
    Ok(0)
}

#[cfg(all(test, unix))]
mod tests {
    use std::collections::VecDeque;

    use super::*;
    use crate::config::template_manifest::REL;
    use crate::paths::DiskText;
    use crate::transaction::manifest::sha256_hex;

    /// A project `init` scaffolded: every template under `.ai/src/` and a
    /// manifest recording each hash.
    pub(super) fn seeded() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        let base = Path::new(&root).join(".ai/src");
        let mut manifest = TemplateManifest::default();
        for (rel, bytes) in catalog::template_files() {
            let path = base.join(&rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, bytes).unwrap();
            manifest.record(&rel, &sha256_hex(bytes));
        }
        std::fs::write(
            Path::new(&root).join(".ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        manifest.write(Path::new(&root)).unwrap();
        (dir, root)
    }

    pub(super) struct Outcome {
        pub(super) status: u8,
        pub(super) out: String,
        pub(super) err: String,
    }

    pub(super) fn call(root: &str, args: &[&str], interactive: bool, replies: &[&str]) -> Outcome {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut queue: VecDeque<String> = replies.iter().map(|r| r.to_string()).collect();
        let mut read_line = || queue.pop_front().unwrap_or_default();
        let mut env = Env {
            interactive,
            read_line: &mut read_line,
            config_path: None,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = refresh(&args, root, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        Outcome {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    pub(super) fn manifest_text(root: &str) -> String {
        std::fs::read_to_string(Path::new(root).join(REL)).unwrap_or_default()
    }

    pub(super) fn drop_entry(root: &str, rel: &str) {
        let kept: String = manifest_text(root)
            .lines()
            .filter(|line| !line.starts_with(&format!("{rel}\t")))
            .map(|line| format!("{line}\n"))
            .collect();
        std::fs::write(Path::new(root).join(REL), kept).unwrap();
    }

    pub(super) fn set_entry(root: &str, rel: &str, hash: &str) {
        drop_entry(root, rel);
        let mut lines: Vec<String> = manifest_text(root).lines().map(str::to_string).collect();
        lines.push(format!("{rel}\t{hash}"));
        lines.sort();
        std::fs::write(
            Path::new(&root).join(REL),
            lines.iter().map(|l| format!("{l}\n")).collect::<String>(),
        )
        .unwrap();
    }

    pub(super) fn append(root: &str, rel: &str, text: &str) {
        let path = Path::new(root).join(".ai/src").join(rel);
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(text.as_bytes()).unwrap();
    }

    pub(super) fn header(root: &str, scope: &str) -> String {
        format!(
            "\n  Exuno Refresh\n\n  Templates: {}\n  Project:   {root}/.ai/src\n  Scope:     {scope}\n\n",
            templates_display()
        )
    }

    pub(super) const NOT_A_TTY: &str = "  Error: Cannot run interactively (not a TTY).\n  Use --yes to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use --dry-run to preview.\n";

    #[test]
    fn an_untouched_project_is_up_to_date_and_status_reports_nothing_declined() {
        let (_dir, root) = seeded();
        let before = manifest_text(&root);
        let run = call(&root, &["--yes"], false, &[]);
        assert_eq!(run.status, 0);
        assert_eq!(
            run.out,
            format!(
                "{}  Already up to date! 18 file(s) match the current templates.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert_eq!(run.err, "");
        assert_eq!(manifest_text(&root), before);
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Nothing declined.\n\n"
        );

        std::fs::rename(
            Path::new(&root).join(".ai/src"),
            Path::new(&root).join("legacy"),
        )
        .unwrap();
        for entry in std::fs::read_dir(Path::new(&root).join("legacy")).unwrap() {
            let entry = entry.unwrap();
            std::fs::rename(
                entry.path(),
                Path::new(&root).join(".ai").join(entry.file_name()),
            )
            .unwrap();
        }
        let legacy = call(&root, &["--yes"], false, &[]);
        assert!(legacy.out.contains(&format!("  Project:   {root}/.ai\n")));
        assert!(legacy.out.contains("Already up to date! 18 file(s)"));

        std::fs::remove_dir_all(Path::new(&root).join(".ai")).unwrap();
        let gone = call(&root, &["--status"], false, &[]);
        assert_eq!(
            (gone.status, gone.out.as_str(), gone.err),
            (
                1,
                "",
                format!("Error: No .ai/ directory found in {root}\nRun exuno init first.\n")
            )
        );
    }
}
