//! `tests/baseline.bats`: first sync in a project that already has its own
//! tool config. The run says what it is replacing, keeps a restorable
//! snapshot, and `adopt` works before any manifest exists so the existing
//! content can be kept instead.

mod common;

use common::Project;
use predicates::prelude::*;

fn seeded() -> Project {
    Project::seeded(&["--tools", "claude", "--yes", "--no-sync"])
}

#[test]
fn baseline_a_pre_existing_claude_md_is_reported_before_being_replaced() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("already exist"))
        .stderr(predicate::str::contains("CLAUDE.md"));
}

#[test]
fn baseline_the_warning_names_adopt_and_rollback() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .stderr(predicate::str::contains("exuno adopt"))
        .stderr(predicate::str::contains("exuno rollback"));
}

#[test]
fn baseline_the_replaced_content_is_restorable_from_the_snapshot() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules\n");
    project.exuno().arg("sync").assert().success();
    assert!(!project.read("CLAUDE.md").contains("Hand-written rules"));

    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert!(project.read("CLAUDE.md").contains("Hand-written rules"));
}

#[test]
fn baseline_a_file_inside_a_generated_directory_is_reported_too() {
    let project = seeded();
    project.write(".claude/rules/legacy.md", "# Legacy rule\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(".claude/rules/"));
}

#[test]
fn baseline_a_path_several_tools_write_is_counted_once() {
    let project = seeded();
    project.enable_tools(&["cursor", "codex"]);
    project.write("AGENTS.md", "# Hand-written agents\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "regenerating 1 path(s) that already exist",
        ));
}

#[test]
fn baseline_an_empty_generated_directory_is_not_reported() {
    let project = seeded();
    std::fs::create_dir_all(project.join(".claude/rules")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("already exist").not());
}

#[test]
fn baseline_dry_run_reports_nothing_and_writes_nothing() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules");
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("already exist").not());
    assert_eq!(project.read("CLAUDE.md"), "# Hand-written rules");
}

#[test]
fn baseline_adopt_before_the_first_sync_keeps_the_content() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules\n");
    project
        .exuno()
        .args(["adopt", "--yes", "CLAUDE.md"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/AGENTS.md")
            .contains("Hand-written rules")
    );

    project.exuno().arg("sync").assert().success();
    assert!(project.read("CLAUDE.md").contains("Hand-written rules"));
}

#[test]
fn baseline_adopt_of_a_missing_destination_still_fails() {
    let project = seeded();
    project
        .exuno()
        .args(["adopt", "--yes", "CLAUDE.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

#[test]
fn baseline_adopt_all_still_needs_a_manifest() {
    let project = seeded();
    project.write("CLAUDE.md", "# Hand-written rules\n");
    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("manifest"));
}

#[test]
fn baseline_a_second_sync_reports_nothing() {
    let project = seeded();
    project.exuno().arg("sync").assert().success();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("already exist").not());
}
