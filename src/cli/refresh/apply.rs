//! Applying the classified changes: auto-updates silently, then restores, new
//! templates, and conflicts, each by prompt or by `--yes`.

use super::classify::{Candidate, Changes};
use super::session::Run;
use crate::Error;

#[derive(Default)]
pub(super) struct Tally {
    pub(super) added: usize,
    pub(super) updated: usize,
    pub(super) auto_applied: usize,
    pub(super) skipped: usize,
    pub(super) cancelled: bool,
}

impl Run<'_, '_> {
    /// Stops asking at the first `q`; what was already applied stays.
    pub(super) fn apply(
        &mut self,
        changes: &Changes,
        assume_yes: bool,
        visible_deleted: bool,
    ) -> Result<Tally, Error> {
        let mut tally = Tally::default();
        for entry in &changes.auto {
            self.copy(entry)?;
            let note = self.style.dim("(auto-updated; you hadn't touched it)");
            self.say(&format!(
                "  {} {}  {note}\n",
                self.style.cyan("↑"),
                entry.rel
            ))?;
            tally.auto_applied += 1;
        }
        if visible_deleted {
            self.restore_deleted(&changes.deleted, assume_yes, &mut tally)?;
        }
        if !tally.cancelled {
            self.add_new(&changes.new, assume_yes, &mut tally)?;
        }
        if !tally.cancelled {
            self.resolve_conflicts(&changes.conflicts, assume_yes, &mut tally)?;
        }
        Ok(tally)
    }

    fn restore_deleted(
        &mut self,
        entries: &[Candidate],
        assume_yes: bool,
        tally: &mut Tally,
    ) -> Result<(), Error> {
        let style = self.style;
        for entry in entries {
            if tally.cancelled {
                break;
            }
            if assume_yes {
                let note =
                    style.dim("(previously declined — skipped under --yes; run interactively)");
                self.say(&format!("  {} {} {note}\n", style.dim("?"), entry.rel))?;
                tally.skipped += 1;
                continue;
            }
            match self.prompt_deleted(entry)? {
                'a' => {
                    self.copy(entry)?;
                    self.say(&format!("    {}\n", style.green("restored.")))?;
                    tally.added += 1;
                }
                'q' => tally.cancelled = true,
                _ => {
                    self.say(&format!("    {}\n", style.dim("still declined.")))?;
                    tally.skipped += 1;
                }
            }
        }
        Ok(())
    }

    fn add_new(
        &mut self,
        entries: &[Candidate],
        assume_yes: bool,
        tally: &mut Tally,
    ) -> Result<(), Error> {
        let style = self.style;
        for entry in entries {
            if tally.cancelled {
                break;
            }
            if assume_yes {
                self.copy(entry)?;
                self.say(&format!("  {} {}\n", style.green("+"), entry.rel))?;
                tally.added += 1;
                continue;
            }
            match self.prompt_new(entry)? {
                'a' => {
                    self.copy(entry)?;
                    self.say(&format!("    {}\n", style.green("added.")))?;
                    tally.added += 1;
                }
                'q' => tally.cancelled = true,
                _ => {
                    // Skip-as-decline: recorded so the file never reappears as NEW.
                    self.manifest.record(&entry.rel, &entry.hash);
                    let note =
                        "declined (will not be offered again — use --include-deleted to revisit).";
                    self.say(&format!("    {}\n", style.dim(note)))?;
                    tally.skipped += 1;
                }
            }
        }
        Ok(())
    }

    fn resolve_conflicts(
        &mut self,
        entries: &[Candidate],
        assume_yes: bool,
        tally: &mut Tally,
    ) -> Result<(), Error> {
        let style = self.style;
        for entry in entries {
            if tally.cancelled {
                break;
            }
            if assume_yes {
                let note = style.dim("(conflict — skipped; run interactively to review)");
                self.say(&format!("  {} {} {note}\n", style.yellow("~"), entry.rel))?;
                tally.skipped += 1;
                continue;
            }
            match self.prompt_conflict(entry)? {
                'u' => {
                    self.copy(entry)?;
                    self.say(&format!("    {}\n", style.yellow("updated.")))?;
                    tally.updated += 1;
                }
                'q' => tally.cancelled = true,
                _ => {
                    // Recorded at the new template hash so the skip is remembered.
                    self.manifest.record(&entry.rel, &entry.hash);
                    let note = "skipped (remembered — exuno refresh --review to revisit).";
                    self.say(&format!("    {}\n", style.dim(note)))?;
                    tally.skipped += 1;
                }
            }
        }
        Ok(())
    }
}
