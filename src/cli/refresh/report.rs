//! The text `refresh` prints around its prompts: the header, the two summaries,
//! and the closing tally.

use super::apply::Tally;
use super::args::Options;
use super::classify::Changes;
use crate::output::style::Style;

pub(super) struct Report<'a> {
    pub(super) style: &'a Style,
    pub(super) options: &'a Options,
}

impl Report<'_> {
    pub(super) fn header(
        &self,
        project: &str,
        categories: &[String],
        has_manifest: bool,
    ) -> String {
        let style = self.style;
        let mut scope_label = categories.join(",");
        if self.options.include_agents_md {
            scope_label.push_str(",AGENTS.md");
        }
        let mut header = format!(
            "\n{}\n\n  {} {}\n  {}   {project}\n  {}     {scope_label}\n",
            style.bold("  Exuno Refresh"),
            style.dim("Templates:"),
            super::templates_display(),
            style.dim("Project:"),
            style.dim("Scope:")
        );
        if !has_manifest {
            header.push_str(&format!(
                "  {}  {}\n",
                style.dim("Manifest:"),
                style.yellow("none — falling back to two-way diff")
            ));
        }
        header.push('\n');
        header
    }

    pub(super) fn up_to_date(&self, changes: &Changes, declined: usize) -> String {
        let style = self.style;
        let mut text = format!(
            "  {} {} file(s) match the current templates.\n",
            style.green("Already up to date!"),
            changes.unchanged
        );
        if declined > 0 {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Persistently declined (agent_sync.yaml): {declined} file(s)."
                ))
            ));
        }
        if !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "Locally declined (.template-manifest):    {} file(s); --include-deleted to revisit.",
                    changes.deleted.len()
                ))
            ));
        }
        if declined > 0 || !changes.deleted.is_empty() {
            text.push_str(&format!(
                "  {}\n",
                style.dim("Pass --status for the full list.")
            ));
        }
        if changes.silently_kept > 0 && !self.options.review {
            text.push_str(&format!(
                "  {}\n",
                style.dim(&format!(
                    "{} file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.",
                    changes.silently_kept
                ))
            ));
        }
        text
    }

    pub(super) fn summary(&self, changes: &Changes, visible_deleted: bool) -> String {
        let style = self.style;
        let mut summary = format!("  {}\n", style.green("Summary:"));
        let mut line = |glyph: String, count: usize, what: &str| {
            if count > 0 {
                summary.push_str(&format!("    {glyph} {count} {what}\n"));
            }
        };
        line(style.green("+"), changes.new.len(), "new template(s)");
        line(
            style.cyan("↑"),
            changes.auto.len(),
            "auto-update(s) — you hadn't touched them locally",
        );
        line(
            style.yellow("~"),
            changes.conflicts.len(),
            "conflict(s) — your version differs from the template",
        );
        if visible_deleted {
            line(
                style.dim("?"),
                changes.deleted.len(),
                "previously declined — pass --include-deleted to revisit",
            );
        }
        if !self.options.review {
            line(
                style.dim("·"),
                changes.silently_kept,
                "silently kept (local edits or earlier skips) — pass --review to revisit",
            );
        }
        line(style.dim("·"), changes.unchanged, "unchanged");
        summary.push('\n');
        summary
    }

    pub(super) fn dry_run(&self) -> String {
        format!("  {} — no files written.\n\n", self.style.yellow("Dry run"))
    }

    pub(super) fn not_a_tty(&self) -> String {
        let style = self.style;
        format!(
            "  {}: Cannot run interactively (not a TTY).\n  Use {} to add new files and apply auto-updates\n  (conflicts are always skipped non-interactively).\n  Use {} to preview.\n",
            style.red("Error"),
            style.cyan("--yes"),
            style.cyan("--dry-run")
        )
    }

    pub(super) fn closing(&self, tally: &Tally, unchanged: usize) -> String {
        let style = self.style;
        let mut closing = String::from("\n");
        if tally.cancelled {
            closing.push_str(&format!(
                "  {} Files already applied are kept.\n",
                style.yellow("Cancelled.")
            ));
        }
        closing.push_str(&format!(
            "  {} Added: {} · Auto-updated: {} · Updated: {} · Skipped: {} · Unchanged: {unchanged}\n",
            style.green("Done."),
            tally.added,
            tally.auto_applied,
            tally.updated,
            tally.skipped,
        ));
        if tally.added + tally.auto_applied + tally.updated > 0 {
            closing.push_str(&format!(
                "\n  Next: {} to distribute the updates to enabled tools.\n",
                style.cyan("exuno sync")
            ));
        }
        closing.push('\n');
        closing
    }
}
