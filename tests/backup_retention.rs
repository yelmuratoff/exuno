//! `tests/backup_retention.bats`: recovery policy through real commands,
//! including failure/rollback paths. Each case proves the project tree is
//! either untouched (a failure before mutation) or that the recovery store
//! survives a low-retention run, by hashing every file instead of the bats
//! tar+diff round trip.

mod common;

use std::path::Path;
use std::time::{Duration, UNIX_EPOCH};

use common::Project;
use predicates::prelude::*;

/// `init_project`: `init --tools claude --yes --no-sync`, then a hand-written
/// `CLAUDE.md` sync would otherwise regenerate.
fn init_project(project: &Project) {
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    project.write("CLAUDE.md", "before-sync\n");
}

/// `set_retention`: appends `backup.retention: <value>` to the project config.
fn set_retention(project: &Project, value: &str) {
    project.append(
        ".ai/agent_sync.yaml",
        &format!("\nbackup:\n  retention: {value}\n"),
    );
}

/// `seed_recovery`: two complete snapshots (one dated 2020, one 2099) and
/// every staging kind `sweep_stale_staging` and `prune` must tell apart —
/// stale sync staging, a fresh one, stale pointer/gitignore staging, and an
/// unrelated directory the store never manages.
fn seed_recovery(project: &Project) {
    for id in ["20200101T000000Z-sync-old", "20990101T000000Z-sync-count"] {
        let created = id.split('-').next().unwrap();
        project.write(
            &format!(".ai/backups/{id}/metadata"),
            &format!("schema=1\noperation=sync\ncreated_at={created}\n"),
        );
        project.write(
            &format!(".ai/backups/{id}/targets.tsv"),
            "present\tCLAUDE.md\n",
        );
        project.write(
            &format!(".ai/backups/{id}/files/CLAUDE.md"),
            "recovered-output\n",
        );
        project.write(&format!(".ai/backups/{id}/.complete"), "");
    }
    std::fs::create_dir_all(project.join(".ai/backups/.tmp.sync.old/files")).unwrap();
    std::fs::create_dir_all(project.join(".ai/backups/.tmp.sync.fresh/files")).unwrap();
    std::fs::create_dir_all(project.join(".ai/backups/unrelated")).unwrap();
    project.write(
        ".ai/backups/.tmp.sync.old/files/sentinel",
        "partial recovery\n",
    );
    project.write(".ai/backups/.latest.tmp.old", "old pointer\n");
    project.write(".ai/backups/.gitignore.tmp.old", "old ignore\n");
    project.write(".ai/backups/unrelated/sentinel", "foreign content\n");

    // `touch -t 202001010000`: old enough for the 24-hour stale-staging sweep;
    // `.tmp.sync.fresh` and `unrelated` keep their just-created mtime.
    backdate(&project.join(".ai/backups/.latest.tmp.old"));
    backdate(&project.join(".ai/backups/.gitignore.tmp.old"));
    #[cfg(unix)]
    backdate(&project.join(".ai/backups/.tmp.sync.old"));
}

/// `touch -t 202001010000`. Windows refuses `set_modified` on a handle that is
/// not open for writing, and the standard library cannot open a directory for
/// writing there at all — so the two cases that need a stale staging
/// *directory* are `#[cfg(unix)]`.
fn backdate(path: &Path) {
    let old = UNIX_EPOCH + Duration::from_secs(1_577_836_800);
    #[cfg(unix)]
    let file = std::fs::File::open(path).unwrap();
    #[cfg(not(unix))]
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.set_modified(old).unwrap();
}

/// Every regular file's relative path and content hash, standing in for the
/// bats tar+diff evidence: two snapshots are equal exactly when no tracked
/// file's content changed.
fn snapshot_tree(root: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort();
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        if entry.file_type().unwrap().is_dir() {
            walk(root, &path, out);
        } else {
            let bytes = std::fs::read(&path).unwrap();
            out.push((rel, exuno::transaction::manifest::sha256_hex(&bytes)));
        }
    }
}

/// `assert_recovery_preserved`: every seeded snapshot and staging kind has the
/// same paths and content before and after.
fn assert_recovery_preserved(before: &[(String, String)], after: &[(String, String)]) {
    for entry in [
        "20200101T000000Z-sync-old",
        "20990101T000000Z-sync-count",
        ".tmp.sync.old",
        ".tmp.sync.fresh",
        ".latest.tmp.old",
        ".gitignore.tmp.old",
        "unrelated",
    ] {
        let prefix = format!(".ai/backups/{entry}");
        let subset = |snapshot: &[(String, String)]| -> Vec<(String, String)> {
            snapshot
                .iter()
                .filter(|(p, _)| *p == prefix || p.starts_with(&format!("{prefix}/")))
                .cloned()
                .collect()
        };
        assert_eq!(
            subset(before),
            subset(after),
            "recovery entry changed: {entry}"
        );
    }
}

// The swept `.tmp.sync.old` must be older than the sweep window, and only
// Unix can backdate a directory here.
#[cfg(unix)]
#[test]
fn retention_default_sync_still_sweeps_staging_when_both_snapshot_limits_are_zero() {
    let project = Project::empty();
    init_project(&project);
    seed_recovery(&project);
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "0")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "0")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(!project.exists(".ai/backups/.tmp.sync.old"));
    assert!(!project.exists(".ai/backups/.latest.tmp.old"));
    assert!(!project.exists(".ai/backups/.gitignore.tmp.old"));
    assert!(project.join(".ai/backups/.tmp.sync.fresh").is_dir());
    assert!(project.exists(".ai/backups/unrelated/sentinel"));
    assert!(
        project
            .join(".ai/backups/20200101T000000Z-sync-old")
            .is_dir()
    );
}

// The swept `.tmp.sync.old` must be older than the sweep window, and only
// Unix can backdate a directory here.
#[cfg(unix)]
#[test]
fn retention_bounded_sync_applies_age_and_count_limits_and_sweeps_old_staging() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "bounded");
    seed_recovery(&project);
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "30")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(
        !project
            .join(".ai/backups/20200101T000000Z-sync-old")
            .is_dir()
    );
    assert!(
        !project
            .join(".ai/backups/20990101T000000Z-sync-count")
            .is_dir()
    );
    assert!(!project.exists(".ai/backups/.tmp.sync.old"));
    assert!(!project.exists(".ai/backups/.latest.tmp.old"));
    assert!(!project.exists(".ai/backups/.gitignore.tmp.old"));
    assert!(project.join(".ai/backups/.tmp.sync.fresh").is_dir());
    assert!(project.exists(".ai/backups/unrelated/sentinel"));
}

#[test]
fn retention_preserve_sync_keeps_snapshots_and_every_staging_kind_despite_low_limits() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "preserve");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "1")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert_recovery_preserved(&before, &snapshot_tree(project.path()));
    assert_ne!(project.read("CLAUDE.md"), "before-sync\n");
    let snapshot = project.read(".ai/backups/.latest").trim().to_string();
    assert_eq!(
        project.read(&format!(".ai/backups/{snapshot}/files/CLAUDE.md")),
        "before-sync\n"
    );

    // AgentSync's own restore, in addition to the hash-based evidence above.
    project
        .exuno()
        .args(["rollback", &snapshot, "--yes"])
        .assert()
        .success();
    assert_eq!(project.read("CLAUDE.md"), "before-sync\n");
}

#[test]
fn retention_preserve_sync_with_zero_limits_also_keeps_abandoned_staging() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "preserve");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "0")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "0")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert_recovery_preserved(&before, &snapshot_tree(project.path()));
}

#[test]
fn retention_invalid_or_empty_values_reject_sync_before_any_project_mutation() {
    let project = Project::empty();
    init_project(&project);
    seed_recovery(&project);
    let original_config = project.read(".ai/agent_sync.yaml");
    for value in ["typo", "false", "0", "null", "[]", "{}", "\"\"", ""] {
        project.write(".ai/agent_sync.yaml", &original_config);
        set_retention(&project, value);
        let before = snapshot_tree(project.path());
        project
            .exuno()
            .args(["sync", "--only", "claude"])
            .assert()
            .failure()
            .stderr(predicate::str::contains("backup.retention"));
        assert_eq!(
            snapshot_tree(project.path()),
            before,
            "tree changed for retention value {value:?}"
        );
    }
}

#[test]
fn retention_malformed_backup_section_rejects_sync_before_changes() {
    let project = Project::empty();
    init_project(&project);
    project.append(".ai/agent_sync.yaml", "\nbackup: preserve\n");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("backup.retention"));
    assert_eq!(snapshot_tree(project.path()), before);
}

#[test]
fn retention_invalid_init_config_fails_before_scaffolding_or_recovery_changes() {
    let project = Project::empty();
    project.write(".ai/agent_sync.yaml", "backup:\n  retention: typo\n");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("backup.retention"));
    assert_eq!(snapshot_tree(project.path()), before);
}

#[test]
fn retention_preserve_init_keeps_existing_snapshots_and_staging() {
    let project = Project::empty();
    project.write(".ai/agent_sync.yaml", "backup:\n  retention: preserve\n");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "1")
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    assert_recovery_preserved(&before, &snapshot_tree(project.path()));
    assert!(project.exists(".ai/src/AGENTS.md"));
}

#[test]
fn retention_preserve_failed_sync_restores_outputs_without_pruning_recovery() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "preserve");
    project.write(".ai/src/tools/claude.yaml", "post_sync: \"false\"\n");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "1")
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Restored pre-sync state"));
    assert_eq!(project.read("CLAUDE.md"), "before-sync\n");
    assert!(!project.exists(".ai/.sync-manifest"));
    assert_recovery_preserved(&before, &snapshot_tree(project.path()));
}

#[test]
fn retention_invalid_rollback_config_fails_before_safety_snapshot_or_restore() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "typo");
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .args(["rollback", "20200101T000000Z-sync-old", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("backup.retention"));
    assert_eq!(snapshot_tree(project.path()), before);
}

#[test]
fn retention_uses_the_explicit_config_instead_of_a_conflicting_local_policy() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "bounded");
    seed_recovery(&project);
    project.write(
        "selected.yaml",
        "tools:\n  enabled: [claude]\nbackup:\n  retention: \"preserve\" # keep recovery\n",
    );
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert_recovery_preserved(&before, &snapshot_tree(project.path()));
}

#[test]
fn retention_reads_exuno_backup_limit_before_the_legacy_name() {
    let project = Project::empty();
    init_project(&project);
    project
        .exuno()
        .env("EXUNO_BACKUP_LIMIT", "typo")
        .env("AGENTSYNC_BACKUP_LIMIT", "1")
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Backup limit must be"));
}

#[test]
fn retention_invalid_numeric_bounds_reject_sync_before_changes() {
    let project = Project::empty();
    init_project(&project);
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "typo")
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Backup limit must be"));
    assert_eq!(snapshot_tree(project.path()), before);
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_MAX_AGE_DAYS", "-1")
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Backup max age must be"));
    assert_eq!(snapshot_tree(project.path()), before);
}

#[test]
fn retention_invalid_explicit_config_rejects_init_and_rollback_without_fallback() {
    let project = Project::empty();
    init_project(&project);
    seed_recovery(&project);
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "missing.yaml")
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AGENTSYNC_CONFIG_PATH is set but file not found",
        ));
    assert_eq!(snapshot_tree(project.path()), before);
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "missing.yaml")
        .args(["rollback", "20200101T000000Z-sync-old", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AGENTSYNC_CONFIG_PATH is set but file not found",
        ));
    assert_eq!(snapshot_tree(project.path()), before);
}

#[test]
fn retention_invalid_policy_still_lets_rollback_list_read_the_store() {
    let project = Project::empty();
    init_project(&project);
    set_retention(&project, "typo");
    seed_recovery(&project);
    project
        .exuno()
        .args(["rollback", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("20200101T000000Z-sync-old"));
}

#[test]
fn retention_invalid_bounds_do_not_fail_check_which_takes_no_backup() {
    let project = Project::empty();
    init_project(&project);
    project.exuno().arg("sync").assert().success();
    project
        .exuno()
        .env("AGENTSYNC_BACKUP_LIMIT", "typo")
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("safe and synced"));
}
