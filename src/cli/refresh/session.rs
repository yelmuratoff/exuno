//! The interactive run: reports, prompts, the diff view, and copying a
//! template into `.ai/src/`.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::Env;
use super::classify::{Candidate, Changes, Locator};
use crate::Error;
use crate::cli::put;
use crate::config::template_manifest::TemplateManifest;
use crate::output::style::Style;

pub(super) struct Run<'a, 'b> {
    pub(super) style: &'a Style,
    pub(super) env: &'a mut Env<'b>,
    pub(super) locator: Locator,
    pub(super) manifest: TemplateManifest,
    pub(super) out: &'a mut dyn Write,
    pub(super) err: &'a mut dyn Write,
}

impl Run<'_, '_> {
    pub(super) fn say(&mut self, text: &str) -> Result<(), Error> {
        put(self.out, text.as_bytes())
    }

    fn tell(&mut self, text: &str) -> Result<(), Error> {
        put(self.err, text.as_bytes())
    }

    /// `_refresh_heal_unchanged`.
    pub(super) fn heal(&mut self, templates: &[(String, &'static [u8])]) {
        self.manifest.heal_from_match(
            templates.iter().map(|(rel, bytes)| (rel.as_str(), *bytes)),
            |rel| self.locator.path(rel),
        );
    }

    /// `_refresh_copy` followed by `template_manifest_record`.
    pub(super) fn copy(&mut self, entry: &Candidate) -> Result<(), Error> {
        write_template(&entry.dest, entry.bytes)?;
        self.manifest.record(&entry.rel, &entry.hash);
        Ok(())
    }

    /// `_refresh_list_proposed`.
    pub(super) fn list_proposed(
        &mut self,
        changes: &Changes,
        visible_deleted: bool,
    ) -> Result<(), Error> {
        let style = self.style;
        let mut text = String::new();
        let mut section = |title: String, marker: String, entries: &[Candidate]| {
            if entries.is_empty() {
                return;
            }
            text.push_str(&format!("  {title}\n"));
            for entry in entries {
                text.push_str(&format!("    {marker} {}\n", entry.rel));
            }
            text.push('\n');
        };
        section(style.green("New:"), style.green("+"), &changes.new);
        section(
            format!(
                "{} {}",
                style.cyan("Auto-update:"),
                style.dim("(your version matches the previous template; safe to update)")
            ),
            style.cyan("↑"),
            &changes.auto,
        );
        section(
            format!(
                "{} {}",
                style.yellow("Conflicts:"),
                style.dim(
                    "(both your version and the template diverged from the recorded baseline)"
                )
            ),
            style.yellow("~"),
            &changes.conflicts,
        );
        if visible_deleted {
            section(
                style.dim("Previously declined:"),
                style.dim("?"),
                &changes.deleted,
            );
        }
        self.say(&text)
    }

    /// `_refresh_print_status`.
    pub(super) fn print_status(
        &mut self,
        declined: &[String],
        deleted: &[Candidate],
    ) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n{}\n", style.bold("  Declined templates"));
        if declined.is_empty() && deleted.is_empty() {
            text.push_str(&format!("  {}\n\n", style.dim("Nothing declined.")));
            return self.say(&text);
        }
        if !declined.is_empty() {
            text.push_str(&format!(
                "  {}  {}\n",
                style.yellow("Persistent"),
                style.dim("(template_overrides.declined in exuno.yaml — never offered):")
            ));
            for item in declined {
                text.push_str(&format!("    {} {item}\n", style.dim("·")));
            }
            text.push('\n');
        }
        if !deleted.is_empty() {
            text.push_str(&format!(
                "  {}       {}\n",
                style.yellow("Local"),
                style
                    .dim("(.template-manifest — deleted from disk; --include-deleted to restore):")
            ));
            for entry in deleted {
                text.push_str(&format!("    {} {}\n", style.dim("·"), entry.rel));
            }
            text.push('\n');
        }
        self.say(&text)
    }

    /// `read -r reply </dev/tty`, lowercased, `s` when empty.
    fn answer(&mut self) -> String {
        let reply = (self.env.read_line)();
        let reply = reply.trim_matches([' ', '\t']).to_lowercase();
        if reply.is_empty() {
            "s".to_string()
        } else {
            reply
        }
    }

    /// `_refresh_prompt_new`: `a`, `s`, or `q`.
    pub(super) fn prompt_new(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!("\n  {} {}\n", style.green("+ NEW:"), style.cyan(&entry.rel));
        self.prompt_add(entry, &banner)
    }

    /// `_refresh_prompt_deleted`: `a`, `s`, or `q`.
    pub(super) fn prompt_deleted(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        let banner = format!(
            "\n  {} {}  {}\n",
            style.dim("? RESTORE:"),
            style.cyan(&entry.rel),
            style.dim("(previously declined)")
        );
        self.prompt_add(entry, &banner)
    }

    fn prompt_add(&mut self, entry: &Candidate, banner: &str) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "{banner}    [{}]dd  [{}]kip  [v]iew  [q]uit  > ",
                style.green("a"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "a" | "add" => return Ok('a'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => self.show_new(entry.bytes)?,
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try a, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_prompt_conflict`: `u`, `s`, or `q`.
    pub(super) fn prompt_conflict(&mut self, entry: &Candidate) -> Result<char, Error> {
        let style = self.style;
        loop {
            self.tell(&format!(
                "\n  {} {}\n    [{}]pdate  [{}]kip  [v]iew  [q]uit  > ",
                style.yellow("~ CONFLICT:"),
                style.cyan(&entry.rel),
                style.yellow("u"),
                style.yellow("s")
            ))?;
            match self.answer().as_str() {
                "u" | "update" => return Ok('u'),
                "s" | "skip" => return Ok('s'),
                "v" | "view" => self.show_diff(&entry.dest, entry.bytes)?,
                "q" | "quit" => return Ok('q'),
                _ => self.tell(&format!(
                    "    {}\n",
                    style.dim("(unknown choice — try u, s, v, q)")
                ))?,
            }
        }
    }

    /// `_refresh_show_new`: the template, each line indented, on stderr.
    fn show_new(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── new file content ────"));
        let content = String::from_utf8_lossy(bytes);
        let mut lines: Vec<&str> = content.split('\n').collect();
        if content.ends_with('\n') || content.is_empty() {
            lines.pop();
        }
        for line in lines {
            text.push_str(&format!("  {line}\n"));
        }
        text.push('\n');
        self.tell(&text)
    }

    /// `_refresh_show_diff`: `diff -u --label yours --label template`, on stderr.
    fn show_diff(&mut self, dest: &Path, template: &[u8]) -> Result<(), Error> {
        let style = self.style;
        let mut text = format!("\n  {}\n\n", style.dim("──── diff: yours → template ────"));
        match unified_diff(dest, template) {
            Some(hunks) => text.push_str(&String::from_utf8_lossy(&hunks)),
            None => text.push_str(&format!(
                "  {}\n",
                style.red("(diff command not available)")
            )),
        }
        text.push('\n');
        self.tell(&text)
    }
}

/// `diff -u --label yours --label template <yours> <template>`, the embedded
/// template written to a temporary file for the call; `None` when `diff`
/// cannot start.
fn unified_diff(yours: &Path, template: &[u8]) -> Option<Vec<u8>> {
    let staged = stage_template(template)?;
    let output = Command::new("diff")
        .args(["-u", "--label", "yours", "--label", "template"])
        .arg(yours)
        .arg(&staged)
        .stdin(Stdio::null())
        .output();
    let _ = std::fs::remove_file(&staged);
    let output = output.ok()?;
    let mut text = output.stdout;
    text.extend_from_slice(&output.stderr);
    Some(text)
}

fn stage_template(bytes: &[u8]) -> Option<PathBuf> {
    let dir = std::env::temp_dir();
    let pid = std::process::id();
    for attempt in 0u32..100 {
        let path = dir.join(format!("agentsync-refresh-{pid}-{attempt}"));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        match options.open(&path) {
            Ok(mut file) => {
                if file.write_all(bytes).is_err() {
                    let _ = std::fs::remove_file(&path);
                    return None;
                }
                return Some(path);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return None,
        }
    }
    None
}

/// `mkdir -p` and `cp <template> <dest>`. An existing file keeps its mode; a new
/// one is created executable when the template starts with `#!`, the mode the
/// shipped scripts carry in the checkout `cp` copied from.
pub(crate) fn write_template(dest: &Path, bytes: &[u8]) -> Result<(), Error> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    if bytes.starts_with(b"#!") {
        std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o755);
    }
    options
        .open(dest)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| Error::io(dest, e))
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use crate::cli::refresh::tests::{append, call, drop_entry, manifest_text, seeded};
    use crate::config::catalog;
    use crate::config::template_manifest::{REL, TemplateManifest};

    #[test]
    fn prompts_restore_add_update_skip_view_and_quit_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");

        let run = call(
            &root,
            &["--include-deleted"],
            true,
            &["v", " A ", "x", "", "view", "Update"],
        );
        assert_eq!(run.status, 0);
        let review = catalog::template_files()
            .into_iter()
            .find(|(path, _)| path == "commands/review.md")
            .map(|(_, bytes)| String::from_utf8(bytes.to_vec()).unwrap())
            .unwrap();
        let shown: String = review.lines().map(|line| format!("  {line}\n")).collect();
        let restore = "\n  ? RESTORE: commands/review.md  (previously declined)\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let new = "\n  + NEW: rules/comments.md\n    [a]dd  [s]kip  [v]iew  [q]uit  > ";
        let conflict = "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > ";
        let expected_err = format!(
            "{restore}\n  ──── new file content ────\n\n{shown}\n{restore}{new}    (unknown choice — try a, s, v, q)\n{new}{conflict}\n  ──── diff: yours → template ────\n\n--- yours\n+++ template\n@@ "
        );
        assert!(
            run.err.starts_with(&expected_err),
            "stderr was:\n{}",
            run.err
        );
        assert!(run.err.contains("\n-EDIT\n"));
        assert!(run.err.ends_with(&format!("\n{conflict}")));
        assert!(run.out.ends_with(
            "    restored.\n    declined (will not be offered again — use --include-deleted to revisit).\n    updated.\n\n  Done. Added: 1 · Auto-updated: 0 · Updated: 1 · Skipped: 1 · Unchanged: 15\n\n  Next: exuno sync to distribute the updates to enabled tools.\n\n"
        ));
        assert!(base.join("commands/review.md").is_file());
        assert!(!base.join("rules/comments.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(
            std::fs::read_to_string(base.join("rules/git.md")).unwrap(),
            String::from_utf8(
                catalog::template_files()
                    .into_iter()
                    .find(|(path, _)| path == "rules/git.md")
                    .unwrap()
                    .1
                    .to_vec()
            )
            .unwrap()
        );

        let (_dir, root) = seeded();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        let quit = call(&root, &[], true, &["skip", "q"]);
        assert_eq!(quit.status, 0);
        assert!(quit.out.ends_with(
            "    declined (will not be offered again — use --include-deleted to revisit).\n\n  Cancelled. Files already applied are kept.\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
        ));
        assert!(
            quit.err.ends_with(
                "\n  ~ CONFLICT: rules/git.md\n    [u]pdate  [s]kip  [v]iew  [q]uit  > "
            )
        );
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert!(manifest.lookup("rules/comments.md").is_some());
        assert_eq!(manifest.lookup("rules/git.md"), None);
    }

    #[test]
    fn a_re_added_script_is_executable_and_a_missing_manifest_heals() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_dir_all(base.join("skills/humanizer/scripts")).unwrap();
        std::fs::remove_dir_all(base.join("skills/humanizer/references")).unwrap();
        std::fs::remove_file(Path::new(&root).join(REL)).unwrap();
        let run = call(&root, &["--yes"], false, &[]);
        assert!(run.out.contains(
            "  New:\n    + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n    + skills/humanizer/scripts/strip-ai-chars.sh\n\n  + skills/humanizer/references/wikipedia_signs_of_ai_writing.md\n  + skills/humanizer/scripts/strip-ai-chars.sh\n\n  Done. Added: 2 · Auto-updated: 0 · Updated: 0 · Skipped: 0 · Unchanged: 16\n"
        ));
        let mode = |rel: &str| {
            std::fs::metadata(base.join(rel))
                .unwrap()
                .permissions()
                .mode()
                & 0o777
        };
        assert_eq!(mode("skills/humanizer/scripts/strip-ai-chars.sh"), 0o755);
        assert_eq!(
            mode("skills/humanizer/references/wikipedia_signs_of_ai_writing.md"),
            0o644
        );
        assert_eq!(manifest_text(&root).lines().count(), 19);
        assert_eq!(
            std::fs::metadata(Path::new(&root).join(REL))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}
