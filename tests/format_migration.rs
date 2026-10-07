//! `tests/format_migration.bats`: a project that predates a migration is told
//! so, and `migrate --apply` retires the stale engine-owned skill copy and
//! records the revision.

mod common;

use common::Project;
use predicates::prelude::*;

/// A project as an older engine would have left it: the agentsync skill
/// copied into `.ai/src/`, its hash recorded in the template manifest, no
/// format key in the config.
fn seed_pre_format_project() -> Project {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    let config = project.read(".ai/agent_sync.yaml");
    let stripped: String = config
        .lines()
        .filter(|line| !line.starts_with("format:"))
        .map(|line| format!("{line}\n"))
        .collect();
    project.write(".ai/agent_sync.yaml", &stripped);

    let skill = std::fs::read_to_string(
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
            + "/lib/templates/base-src/skills/agentsync/SKILL.md",
    )
    .unwrap();
    std::fs::create_dir_all(project.join(".ai/src/skills/agentsync/references")).unwrap();
    project.write(".ai/src/skills/agentsync/SKILL.md", &skill);
    let hash = project.sha256(".ai/src/skills/agentsync/SKILL.md");
    project.append(
        ".ai/.template-manifest",
        &format!("skills/agentsync/SKILL.md\t{hash}\n"),
    );
    project
}

#[test]
fn format_init_records_the_engines_revision_on_a_fresh_project() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line == expected)
    );
}

#[test]
fn format_a_fresh_project_has_nothing_to_migrate() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
}

#[test]
fn format_doctor_reports_the_revision() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("Project format"));
}

#[test]
fn format_doctor_warns_when_the_project_is_behind() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("behind the engine"))
        .stdout(predicate::str::contains("exuno migrate"));
}

#[test]
fn format_migrate_previews_the_stale_skill_copy_without_touching_it() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would remove"))
        .stdout(predicate::str::contains("skills/agentsync"));
    assert!(project.exists(".ai/src/skills/agentsync/SKILL.md"));
    assert!(
        !project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line.starts_with("format:"))
    );
}

#[test]
fn format_migrate_apply_removes_the_unedited_copy_and_records_the_revision() {
    let project = seed_pre_format_project();
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    assert!(!project.join(".ai/src/skills/agentsync").exists());
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line == expected)
    );
    assert!(
        !project
            .read(".ai/.template-manifest")
            .contains("skills/agentsync")
    );
}

#[test]
fn format_after_migrating_the_engine_supplies_the_skill_again() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/skills/agentsync/SKILL.md"));
}

#[test]
fn format_an_edited_copy_is_kept_as_a_deliberate_override() {
    let project = seed_pre_format_project();
    project.append(".ai/src/skills/agentsync/SKILL.md", "\nMY OWN NOTE\n");
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("keep"));
    assert!(
        project
            .read(".ai/src/skills/agentsync/SKILL.md")
            .contains("MY OWN NOTE")
    );
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line == expected)
    );
}

#[test]
fn format_an_edited_copy_still_shadows_the_engine_version_after_sync() {
    let project = seed_pre_format_project();
    project.append(".ai/src/skills/agentsync/SKILL.md", "\nMY OWN NOTE\n");
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".claude/skills/agentsync/SKILL.md")
            .contains("MY OWN NOTE")
    );
}

#[test]
fn format_migrating_twice_is_a_no_op() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
}

#[test]
fn format_upgrade_config_does_not_silence_a_pending_migration() {
    let project = seed_pre_format_project();
    project.exuno().arg("upgrade-config").assert().success();
    assert!(
        !project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line.starts_with("format:"))
    );
}
