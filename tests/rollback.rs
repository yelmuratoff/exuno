//! `tests/rollback.bats`: restoring init/sync target backups.

mod common;

use common::Project;
use predicates::prelude::*;

fn project_with_claude() -> Project {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["claude"]);
    project
}

fn complete_backup_count(project: &Project) -> usize {
    std::fs::read_dir(project.join(".ai/backups"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().join(".complete").is_file())
        .count()
}

#[test]
fn rollback_restores_the_latest_pre_sync_state_and_creates_an_undo_backup() {
    let project = project_with_claude();
    project.write("CLAUDE.md", "before-sync\n");
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(project.read("CLAUDE.md").contains("# Project Agent"));

    let sync_id = project.read(".ai/backups/.latest");
    let sync_id = sync_id.trim();
    assert!(
        project
            .read(&format!(".ai/backups/{sync_id}/metadata"))
            .lines()
            .any(|line| line == "operation=sync")
    );

    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Restored backup {sync_id}"
        )));
    assert_eq!(project.read("CLAUDE.md"), "before-sync\n");
    assert!(!project.join(".claude/rules").exists());

    let undo_id = project.read(".ai/backups/.latest");
    let undo_id = undo_id.trim().to_string();
    assert_ne!(undo_id, sync_id);
    assert!(
        project
            .read(&format!(".ai/backups/{undo_id}/metadata"))
            .lines()
            .any(|line| line == "operation=rollback")
    );

    project
        .exuno()
        .args(["rollback", &undo_id, "--yes"])
        .assert()
        .success();
    assert!(project.read("CLAUDE.md").contains("# Project Agent"));
}

#[test]
fn rollback_dry_run_previews_an_explicit_backup_without_changing_state() {
    let project = project_with_claude();
    project.write("CLAUDE.md", "before-sync\n");
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    let sync_id = project.read(".ai/backups/.latest");
    let sync_id = sync_id.trim().to_string();
    let backups_before = complete_backup_count(&project);

    project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Rollback plan"))
        .stdout(predicate::str::contains("CLAUDE.md"));
    assert!(project.read("CLAUDE.md").contains("# Project Agent"));
    assert_eq!(complete_backup_count(&project), backups_before);
}

#[test]
fn rollback_list_shows_complete_init_and_sync_backups() {
    let project = project_with_claude();
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();

    project
        .exuno()
        .args(["rollback", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\tinit\t"))
        .stdout(predicate::str::contains("\tsync\t"));
}

#[test]
fn rollback_reads_the_project_named_by_agentsync_repo_root() {
    let project = project_with_claude();
    std::fs::create_dir_all(project.join("sub")).unwrap();

    project
        .exuno()
        .current_dir(project.join("sub"))
        .env("AGENTSYNC_REPO_ROOT", project.path())
        .args(["rollback", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\tinit\t"));
}

#[test]
fn rollback_requires_confirmation_outside_a_tty_unless_yes_is_passed() {
    let project = project_with_claude();
    project.write("CLAUDE.md", "before-sync\n");
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();

    project
        .exuno()
        .arg("rollback")
        .assert()
        .code(130)
        .stdout(predicate::str::contains("Cancelled"));
    assert!(project.read("CLAUDE.md").contains("# Project Agent"));
}

#[test]
fn rollback_rejects_backup_ids_containing_path_traversal() {
    let project = project_with_claude();
    let latest = project.read(".ai/backups/.latest");
    let latest = latest.trim();
    project
        .exuno()
        .args(["rollback", &format!("../{latest}"), "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid backup ID"));
}

#[test]
fn rollback_leaves_no_temp_artifacts_behind() {
    let project = project_with_claude();
    project.write("CLAUDE.md", "before-sync\n");
    project.exuno().arg("sync").assert().success();

    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();

    project
        .exuno()
        .env("TMPDIR", &sandbox)
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn rollback_removes_the_tool_folder_the_sync_created() {
    let project = project_with_claude();
    assert!(!project.exists(".claude"));
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(project.exists(".claude/rules"));

    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".claude"));
}
