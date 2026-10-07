//! `exuno rollback`: `cmd_rollback` of `lib/helpers/backup.sh`. A safety
//! snapshot is taken first, and a restore that fails or is interrupted puts
//! it back.

use std::io::Write;

use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::transaction::interrupt::{self, Interrupt};
use crate::transaction::witness::{self, Preflight};
use crate::{Error, config::project_config, paths, transaction::backup};

pub const HELP: Help = Help {
    command: "rollback",
    tagline: "restore targets from a backup",
    synopsis: &["rollback [<backup-id>] [OPTIONS]"],
    description: &[
        "Restore Exuno-managed targets from a backup. Without an ID, restores\nthe latest complete snapshot. A safety snapshot is created before every\nrestore, so the rollback itself can be undone.",
        "Rollback refuses, naming the first changed path, when a target differs\nfrom the state recorded after the backup's operation finished.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            ("--list", "List complete backups"),
            (
                "--dry-run",
                "Show the restore plan and any conflict without changing files",
            ),
            (
                "--force",
                "Restore even when targets changed after the backup's operation",
            ),
            ("-y, --yes", "Skip the confirmation prompt"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "rollback --list",
        "rollback",
        "rollback 20260921T120000Z-sync-4242 --dry-run",
        "rollback --force --yes",
    ],
};

/// The environment `backup_configure` and `backup_prune` read.
#[derive(Default)]
pub struct Env {
    pub config_path: Option<String>,
    pub backup_limit: Option<String>,
    pub backup_max_age: Option<String>,
}

/// Runs `rollback` for the project at `supplied_root`; `confirm` answers the
/// restore question when `--yes` is absent. Returns the exit status.
pub fn run(
    supplied_root: &str,
    args: &[String],
    env: &Env,
    style: &Style,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> u8 {
    let parsed = match parse(args, style, out, err) {
        Ok(parsed) => parsed,
        Err(status) => return status,
    };
    let root = match backup::canonical_root(supplied_root) {
        Ok(root) => root,
        Err(e) => return report(err, e),
    };
    if parsed.list_only {
        return list_backups(&root, parsed.backup_id.is_some(), out, err);
    }
    let retention = match load_retention(&root, env, err) {
        Ok(retention) => retention,
        Err(status) => return status,
    };
    let snapshot = match select_snapshot(&root, parsed.backup_id.as_deref(), err) {
        Ok(snapshot) => snapshot,
        Err(status) => return status,
    };
    let id = paths::leaf(&snapshot).to_string();
    let targets = match backup::load_targets(&root, &snapshot) {
        Ok(targets) => targets,
        Err(e) => return report(err, e),
    };
    let is_latest = matches!(backup::latest(&root), Ok(Some(latest)) if paths::leaf(&latest) == id);
    let mut rollback = Rollback {
        root,
        env,
        retention,
        snapshot,
        id,
        is_latest,
        out,
        err,
    };
    rollback.run(&parsed, &targets, confirm)
}

struct Args {
    backup_id: Option<String>,
    list_only: bool,
    dry_run: bool,
    assume_yes: bool,
    force: bool,
}

/// The parsed flags, or the exit status after the help or a refusal.
fn parse(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<Args, u8> {
    let mut parsed = Args {
        backup_id: None,
        list_only: false,
        dry_run: false,
        assume_yes: false,
        force: false,
    };
    for arg in args {
        match arg.as_str() {
            "--list" => parsed.list_only = true,
            "--dry-run" => parsed.dry_run = true,
            "--force" => parsed.force = true,
            "--yes" | "-y" => parsed.assume_yes = true,
            "--help" | "-h" => {
                let _ = out.write_all(HELP.render(style).as_bytes());
                return Err(0);
            }
            option if option.starts_with('-') => {
                let _ = writeln!(err, "Error: Unknown rollback option: {option}");
                let _ = err.write_all(HELP.render(style).as_bytes());
                return Err(1);
            }
            id if parsed.backup_id.is_some() => {
                let _ = writeln!(err, "Error: Unexpected rollback argument: {id}");
                return Err(1);
            }
            id => parsed.backup_id = Some(id.to_string()),
        }
    }
    Ok(parsed)
}

/// Prints `e` as the backup helpers do and answers exit status 1.
fn report(err: &mut dyn Write, e: Error) -> u8 {
    let _ = match e {
        Error::Backup(message) => writeln!(err, "Error: {message}"),
        other => writeln!(err, "{other}"),
    };
    1
}

/// `rollback --list`.
fn list_backups(root: &str, with_id: bool, out: &mut dyn Write, err: &mut dyn Write) -> u8 {
    if with_id {
        let _ = writeln!(err, "Error: A backup ID cannot be combined with --list");
        return 1;
    }
    let rows = match backup::list(root) {
        Ok(rows) => rows,
        Err(e) => return report(err, e),
    };
    if rows.is_empty() {
        let _ = writeln!(out, "No Exuno backups found.");
        return 0;
    }
    let _ = writeln!(out, "Backup ID\tOperation\tCreated (UTC)");
    for (id, operation, created) in rows {
        let _ = writeln!(out, "{id}\t{operation}\t{created}");
    }
    0
}

/// The backup retention the project config sets.
fn load_retention(root: &str, env: &Env, err: &mut dyn Write) -> Result<backup::Retention, u8> {
    let is_file = |path: &str| std::path::Path::new(path).is_file();
    let config_path = match project_config::select(root, env.config_path.as_deref(), &is_file) {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            let _ = writeln!(err, "Error: {}", project_config::missing_message(&path));
            return Err(1);
        }
    };
    let config = match config_path.as_deref().map(std::fs::read) {
        Some(Ok(bytes)) => Some(String::from_utf8_lossy(&bytes).into_owned()),
        Some(Err(e)) => {
            let path = config_path.as_deref().unwrap_or_default();
            return Err(report(err, Error::io(path, e)));
        }
        None => None,
    };
    backup::configure(
        config_path.as_deref().zip(config.as_deref()),
        env.backup_limit.as_deref(),
        env.backup_max_age.as_deref(),
    )
    .map_err(|e| report(err, e))
}

/// The snapshot a backup ID names, or the latest complete one without an ID.
fn select_snapshot(root: &str, backup_id: Option<&str>, err: &mut dyn Write) -> Result<String, u8> {
    match backup_id {
        Some(id) if id != paths::leaf(id) => {
            let _ = writeln!(err, "Error: Invalid backup ID: {id}");
            Err(1)
        }
        Some(id) => backup::snapshot_path(root, id).map_err(|e| report(err, e)),
        None => match backup::latest(root) {
            Ok(Some(snapshot)) => Ok(snapshot),
            Ok(None) => {
                let _ = writeln!(err, "Error: No complete Exuno backup found");
                Err(1)
            }
            Err(e) => Err(report(err, e)),
        },
    }
}

/// One restore of a chosen snapshot, and the streams it reports through.
struct Rollback<'a> {
    root: String,
    env: &'a Env,
    retention: backup::Retention,
    snapshot: String,
    id: String,
    is_latest: bool,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Rollback<'_> {
    fn run(
        &mut self,
        parsed: &Args,
        targets: &[backup::Target],
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> u8 {
        let (sealed, conflict) = self.preflight(parsed);
        if let Some(path) = &conflict
            && !parsed.dry_run
        {
            report_conflict(self.err, &self.id, path, self.is_latest);
            return 1;
        }
        let _ = writeln!(self.out, "Rollback plan:");
        let _ = writeln!(self.out, "  Backup: {}", self.id);
        for target in targets {
            let action = if target.present { "restore" } else { "remove" };
            let _ = writeln!(self.out, "  {action:<7} {}", target.rel);
        }
        if parsed.dry_run {
            return self.dry_run_verdict(conflict.as_deref(), parsed.force);
        }
        if !parsed.assume_yes && !confirm(&format!("Restore backup {}?", self.id)) {
            let _ = writeln!(self.out, "Cancelled.");
            return 130;
        }
        let (safety, previous_latest) = match self.safety_backup(targets) {
            Ok(safety) => safety,
            Err(status) => return status,
        };
        if sealed && let Preflight::Conflict(path) = witness::preflight(&self.root, &self.snapshot)
        {
            let store = format!("{}/.ai/backups", self.root);
            if backup::discard_safety(&store, &safety, &previous_latest).is_err() {
                let _ = writeln!(
                    self.err,
                    "Warning: Could not remove the unused safety backup {}.",
                    paths::leaf(&safety)
                );
            }
            report_conflict(self.err, &self.id, &path, self.is_latest);
            return 1;
        }
        self.restore(&safety)
    }

    /// Whether the witness sealed the snapshot, and the first path changed
    /// since; skipped under `--force` unless it is a dry run.
    fn preflight(&mut self, parsed: &Args) -> (bool, Option<String>) {
        if parsed.force && !parsed.dry_run {
            return (false, None);
        }
        match witness::preflight(&self.root, &self.snapshot) {
            Preflight::Clean => (true, None),
            Preflight::Conflict(path) => (false, Some(path)),
            Preflight::Unsealed(detail) => {
                let _ = writeln!(
                    self.err,
                    "Warning: Backup {} {detail}; changes made after that operation cannot be detected.",
                    self.id
                );
                (false, None)
            }
        }
    }

    fn dry_run_verdict(&mut self, conflict: Option<&str>, force: bool) -> u8 {
        match conflict {
            Some(path) if force => {
                let _ = writeln!(
                    self.err,
                    "Warning: Rollback conflict: {path} changed after the operation recorded in backup {}; --force will overwrite it.",
                    self.id
                );
            }
            Some(path) => report_conflict(self.err, &self.id, path, self.is_latest),
            None => {}
        }
        let _ = writeln!(self.out, "Dry run — nothing was written.");
        u8::from(conflict.is_some() && !force)
    }

    /// The pre-rollback safety snapshot, and the `.latest` pointer it replaced.
    fn safety_backup(&mut self, targets: &[backup::Target]) -> Result<(String, String), u8> {
        let root = &self.root;
        let current: Vec<String> = targets
            .iter()
            .map(|target| format!("{root}/{}", target.rel))
            .collect();
        let pointer = std::path::Path::new(&format!("{root}/.ai/backups")).join(".latest");
        let previous_latest = if pointer.is_file() && !pointer.is_symlink() {
            std::fs::read(&pointer)
                .map(|bytes| {
                    String::from_utf8_lossy(&bytes)
                        .split('\n')
                        .next()
                        .unwrap_or("")
                        .to_string()
                })
                .unwrap_or_default()
        } else {
            String::new()
        };
        match backup::create(root, "rollback", &current, self.retention) {
            Ok(safety) => Ok((safety, previous_latest)),
            Err(e) => {
                report(self.err, e);
                let _ = writeln!(
                    self.err,
                    "Error: Could not create a pre-rollback safety backup; no files were changed"
                );
                Err(1)
            }
        }
    }

    /// Restores the snapshot; a failure or a signal puts `safety` back.
    fn restore(&mut self, safety: &str) -> u8 {
        let mut interrupt = Interrupt::arm();
        let restored = backup::restore(&self.root, &self.snapshot);
        let signal = interrupt.received();
        if restored.is_err() || signal.is_some() {
            let status = match (restored, signal) {
                (_, Some(sig)) => interrupt::status(sig),
                (Err(e), None) => report(self.err, e),
                (Ok(()), None) => 0,
            };
            self.recover(safety);
            if let Some(sig) = signal {
                interrupt.resend(sig);
            }
            return status;
        }
        drop(interrupt);
        let root = &self.root;
        if let Err(reason) = witness::seal(root, safety) {
            let _ = writeln!(
                self.err,
                "Warning: Could not record the post-rollback state ({reason}); rolling back backup {} cannot detect later changes.",
                paths::leaf(safety)
            );
        }
        if let Err(e) = backup::prune(
            root,
            self.env.backup_limit.as_deref(),
            self.env.backup_max_age.as_deref(),
            self.retention,
        ) {
            report(self.err, e);
            let _ = writeln!(self.err, "Warning: Could not prune old Exuno backups.");
        }
        let _ = writeln!(self.out, "Restored backup {}.", self.id);
        let _ = writeln!(self.out, "Undo backup: {}", paths::leaf(safety));
        0
    }

    /// Puts the pre-rollback state back from `safety` after a failed restore.
    fn recover(&mut self, safety: &str) {
        let root = &self.root;
        let shown = safety.strip_prefix(&format!("{root}/")).unwrap_or(safety);
        let _ = writeln!(
            self.err,
            "Warning: Rollback failed; restoring the state from before rollback..."
        );
        match backup::restore(root, safety) {
            Ok(()) => {
                if let Err(reason) = witness::seal(root, safety) {
                    let _ = writeln!(
                        self.err,
                        "Warning: Could not record the restored state ({reason}); rolling back backup {} cannot detect later changes.",
                        paths::leaf(safety)
                    );
                }
                let _ = writeln!(self.err, "Restored pre-rollback state from {shown}");
            }
            Err(e) => {
                report(self.err, e);
                let _ = writeln!(
                    self.err,
                    "Error: Recovery failed. Safety backup retained at {shown}"
                );
            }
        }
    }
}

/// `_rollback_report_conflict`.
fn report_conflict(err: &mut dyn Write, id: &str, path: &str, is_latest: bool) {
    let _ = writeln!(
        err,
        "Error: Rollback conflict: {path} changed after the operation recorded in backup {id}; no files were changed."
    );
    let _ = if is_latest {
        writeln!(
            err,
            "Re-run with --force to restore the backup anyway and discard that change."
        )
    } else {
        writeln!(
            err,
            "Newer Exuno operations may have changed this target. Roll back the newer backups first, or re-run with --force to restore anyway."
        )
    };
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    struct Output {
        status: u8,
        out: String,
        err: String,
    }

    fn rollback(root: &str, args: &[&str], answer: bool) -> Output {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = run(
            root,
            &args,
            &Env::default(),
            &Style::plain(),
            &mut |_| answer,
            &mut out,
            &mut err,
        );
        Output {
            status,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::write(dir.path().join("CLAUDE.md"), "before\n").unwrap();
        (dir, root)
    }

    #[test]
    fn a_rollback_restores_the_latest_backup_and_leaves_an_undo_backup() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md"), format!("{root}/.claude/rules")];
        let snapshot = backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "after\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".claude/rules")).unwrap();
        crate::transaction::witness::seal(&root, &snapshot).unwrap();

        let plan = rollback(&root, &["--dry-run"], false);
        assert_eq!(
            plan.out,
            format!(
                "Rollback plan:\n  Backup: {id}\n  restore CLAUDE.md\n  remove  .claude/rules\nDry run — nothing was written.\n"
            )
        );
        let cancelled = rollback(&root, &[], false);
        assert_eq!(cancelled.status, 130);
        assert!(cancelled.out.ends_with("Cancelled.\n"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "after\n"
        );

        let done = rollback(&root, &["--yes"], false);
        assert_eq!(done.status, 0);
        assert_eq!(done.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        assert!(!dir.path().join(".claude/rules").exists());
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(done.out.ends_with(&format!(
            "Restored backup {id}.\nUndo backup: {}\n",
            paths::leaf(&undo)
        )));
        assert_eq!(
            std::fs::read_to_string(format!("{undo}/metadata"))
                .unwrap()
                .lines()
                .nth(1),
            Some("operation=rollback")
        );

        let listed = rollback(&root, &["--list"], false);
        assert!(
            listed
                .out
                .starts_with("Backup ID\tOperation\tCreated (UTC)\n")
        );
        assert!(listed.out.contains(&format!("{id}\tsync\t")));
    }

    #[test]
    fn a_changed_target_refuses_and_force_restores_it() {
        let (dir, root) = project();
        let targets = [format!("{root}/CLAUDE.md")];
        let snapshot = backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        let id = paths::leaf(&snapshot);
        std::fs::write(dir.path().join("CLAUDE.md"), "synced\n").unwrap();
        crate::transaction::witness::seal(&root, &snapshot).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "edited\n").unwrap();

        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(refused.out, "");
        assert_eq!(
            refused.err,
            format!(
                "Error: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; no files were changed.\nRe-run with --force to restore the backup anyway and discard that change.\n"
            )
        );
        let dry = rollback(&root, &["--dry-run", "--force"], true);
        assert_eq!(dry.status, 0);
        assert_eq!(
            dry.err,
            format!(
                "Warning: Rollback conflict: CLAUDE.md changed after the operation recorded in backup {id}; --force will overwrite it.\n"
            )
        );

        let forced = rollback(&root, &["--force", "--yes"], true);
        assert_eq!(forced.status, 0);
        assert_eq!(forced.err, "");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
            "before\n"
        );
        let undo = backup::latest(&root).unwrap().unwrap();
        assert!(std::path::Path::new(&format!("{undo}/after.tsv")).is_file());

        std::fs::remove_file(format!("{undo}/after.tsv")).unwrap();
        let unsealed = rollback(&root, &["--yes"], true);
        assert_eq!(unsealed.status, 0);
        assert!(unsealed.err.starts_with(&format!(
            "Warning: Backup {} has no post-operation record; changes made after that operation cannot be detected.\n",
            paths::leaf(&undo)
        )));
    }

    #[test]
    fn arguments_and_ids_are_checked_before_anything_is_read() {
        let (_dir, root) = project();
        let unknown = rollback(&root, &["--nope"], true);
        assert_eq!(unknown.status, 1);
        assert!(unknown.err.starts_with(
            "Error: Unknown rollback option: --nope\n\n  exuno rollback — restore targets from a backup\n\n  USAGE\n"
        ));
        assert_eq!(
            rollback(&root, &["a", "b"], true).err,
            "Error: Unexpected rollback argument: b\n"
        );
        assert_eq!(
            rollback(&root, &["../x", "--yes"], true).err,
            "Error: Invalid backup ID: ../x\n"
        );
        assert_eq!(
            rollback(&root, &["x", "--list"], true).err,
            "Error: A backup ID cannot be combined with --list\n"
        );
        assert_eq!(
            rollback(&root, &["--list"], true).out,
            "No Exuno backups found.\n"
        );
        assert_eq!(
            rollback(&root, &["--yes"], true).err,
            "Error: No complete Exuno backup found\n"
        );
        let help = rollback(&root, &["--help", "--nope"], true);
        assert_eq!((help.status, help.err.as_str()), (0, ""));
        assert_eq!(
            help.out,
            "\n  exuno rollback — restore targets from a backup\n\n  USAGE\n    exuno rollback [<backup-id>] [OPTIONS]\n\n  DESCRIPTION\n    Restore Exuno-managed targets from a backup. Without an ID, restores\n    the latest complete snapshot. A safety snapshot is created before every\n    restore, so the rollback itself can be undone.\n\n    Rollback refuses, naming the first changed path, when a target differs\n    from the state recorded after the backup's operation finished.\n\n  OPTIONS\n    --list       List complete backups\n    --dry-run    Show the restore plan and any conflict without changing files\n    --force      Restore even when targets changed after the backup's operation\n    -y, --yes    Skip the confirmation prompt\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno rollback --list\n    exuno rollback\n    exuno rollback 20260921T120000Z-sync-4242 --dry-run\n    exuno rollback --force --yes\n\n"
        );
    }

    #[test]
    fn the_policy_is_checked_after_list_and_preserve_keeps_the_history() {
        let (dir, root) = project();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: typo\n",
        )
        .unwrap();
        let targets = [format!("{root}/CLAUDE.md")];
        backup::create(&root, "sync", &targets, backup::Retention::Bounded).unwrap();
        assert_eq!(rollback(&root, &["--list"], true).status, 0);
        let refused = rollback(&root, &["--yes"], true);
        assert_eq!(refused.status, 1);
        assert_eq!(
            refused.err,
            format!(
                "Error: Invalid backup.retention 'typo' in {root}/.ai/agent_sync.yaml; expected bounded or preserve\n"
            )
        );

        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "backup:\n  retention: preserve\n",
        )
        .unwrap();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            backup_limit: Some("1".into()),
            ..Env::default()
        };
        let args = ["--yes".to_string()];
        assert_eq!(
            run(
                &root,
                &args,
                &env,
                &Style::plain(),
                &mut |_| true,
                &mut out,
                &mut err
            ),
            0
        );
        assert_eq!(backup::list(&root).unwrap().len(), 2);

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let env = Env {
            config_path: Some("missing.yaml".into()),
            ..Env::default()
        };
        assert_eq!(
            run(
                &root,
                &args,
                &env,
                &Style::plain(),
                &mut |_| true,
                &mut out,
                &mut err
            ),
            1
        );
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("Error: EXUNO_CONFIG_PATH is set but file not found: {root}/missing.yaml\n")
        );
    }
}
