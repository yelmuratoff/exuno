//! `tests/config_safety.bats`: configuration selection must fail closed
//! before a write sync can reach cleanup or output mutation.

mod common;

use common::Project;
use predicates::prelude::*;

#[test]
fn an_invalid_explicit_config_path_fails_without_falling_back_or_mutating_outputs() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    project.write(".claude/skills/config-safety-sentinel.md", "");
    let missing_config = project.join("missing-agent-sync.yaml");

    project
        .exuno()
        .env(
            "AGENTSYNC_CONFIG_PATH",
            common::engine_path(&missing_config),
        )
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AGENTSYNC_CONFIG_PATH is set but file not found",
        ))
        .stderr(predicate::str::contains("falling back").not());
    assert!(project.exists(".claude/skills/config-safety-sentinel.md"));
}

#[test]
fn a_missing_config_refuses_write_sync_before_cleanup_defaults_can_run() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    std::fs::remove_file(project.join(".ai/agent_sync.yaml")).unwrap();
    project.write(".claude/skills/config-safety-sentinel.md", "");

    project
        .exuno()
        .env_remove("AGENTSYNC_CONFIG_PATH")
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "No project configuration found and no tool is enabled",
        ))
        .stderr(predicate::str::contains("exuno enable <tool>"));
    assert!(project.exists(".claude/skills/config-safety-sentinel.md"));
}

#[test]
fn a_project_without_a_config_still_syncs_tools_enabled_in_their_own_yaml() {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Project\n");
    project.write(".ai/src/tools/claude.yaml", "enabled: true\n");

    project
        .exuno()
        .env_remove("AGENTSYNC_CONFIG_PATH")
        .arg("sync")
        .assert()
        .success();
    assert!(project.exists("CLAUDE.md"));
}

#[test]
fn check_hands_a_relative_explicit_config_outside_ai_to_its_isolated_sync() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    std::fs::create_dir_all(project.join("config")).unwrap();
    std::fs::rename(
        project.join(".ai/agent_sync.yaml"),
        project.join("config/agentsync.yaml"),
    )
    .unwrap();
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "config/agentsync.yaml")
        .arg("sync")
        .assert()
        .success();

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "config/agentsync.yaml")
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("safe and synced"));
}

#[test]
fn a_missing_config_remains_usable_for_a_dry_run() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    std::fs::remove_file(project.join(".ai/agent_sync.yaml")).unwrap();
    project.write(".claude/skills/config-safety-sentinel.md", "");

    project
        .exuno()
        .env_remove("AGENTSYNC_CONFIG_PATH")
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    assert!(project.exists(".claude/skills/config-safety-sentinel.md"));
}

#[test]
fn check_rejects_an_invalid_explicit_config_path_instead_of_using_the_local_config() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    let missing_config = project.join("missing-agent-sync.yaml");

    project
        .exuno()
        .env(
            "AGENTSYNC_CONFIG_PATH",
            common::engine_path(&missing_config),
        )
        .arg("check")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AGENTSYNC_CONFIG_PATH is set but file not found",
        ));
}

#[test]
fn read_only_commands_reject_an_invalid_explicit_config_path_instead_of_using_the_local_config() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    let missing_config = project.join("missing-agent-sync.yaml");
    let expected = format!(
        "AGENTSYNC_CONFIG_PATH is set but file not found: {}",
        common::engine_path(&missing_config)
    );

    project
        .exuno()
        .env(
            "AGENTSYNC_CONFIG_PATH",
            common::engine_path(&missing_config),
        )
        .arg("list")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(expected.clone()));

    project
        .exuno()
        .env(
            "AGENTSYNC_CONFIG_PATH",
            common::engine_path(&missing_config),
        )
        .args(["show", "claude"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(expected.clone()));

    project
        .exuno()
        .env(
            "AGENTSYNC_CONFIG_PATH",
            common::engine_path(&missing_config),
        )
        .arg("doctor")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(expected));
}
