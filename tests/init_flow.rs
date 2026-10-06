//! `tests/init_flow.bats`: `agentsync init` as the one setup command — pick
//! the outputs mode, keep the project's existing tool config, write the CI
//! gate, and run the first sync.

mod common;

use common::Project;
use predicates::prelude::*;

fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

#[test]
fn init_runs_the_first_sync_so_outputs_exist_without_a_second_command() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    assert!(project.exists("CLAUDE.md"));
    assert!(project.exists(".claude/rules/core.md"));
    assert!(project.exists(".ai/.sync-manifest"));
}

#[test]
fn init_no_sync_leaves_the_outputs_unwritten() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    assert!(!project.exists("CLAUDE.md"));
    assert!(!project.exists(".ai/.sync-manifest"));
}

#[test]
fn init_no_enabled_tools_means_no_sync() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".ai/.sync-manifest"));
}

#[test]
fn init_an_existing_claude_md_is_adopted_so_the_first_sync_reproduces_it() {
    let project = Project::empty();
    project.write("CLAUDE.md", "# Hand-written team rules\n");
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopted"));
    assert!(
        project
            .read(".ai/src/AGENTS.md")
            .contains("Hand-written team rules")
    );
    assert!(
        project
            .read("CLAUDE.md")
            .contains("Hand-written team rules")
    );
}

#[test]
fn init_an_existing_rule_file_is_adopted_into_ai_src_rules() {
    let project = Project::empty();
    project.write(".claude/rules/legacy.md", "# Legacy rule\n");
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/rules/legacy.md")
            .contains("Legacy rule")
    );
    assert!(
        project
            .read(".claude/rules/legacy.md")
            .contains("Legacy rule")
    );
}

#[test]
fn init_existing_replace_regenerates_instead_of_keeping() {
    let project = Project::empty();
    project.write("CLAUDE.md", "# Hand-written team rules\n");
    project
        .exuno()
        .args([
            "init",
            "--tools",
            "claude",
            "--yes",
            "--existing",
            "replace",
        ])
        .assert()
        .success();
    assert!(
        !project
            .read(".ai/src/AGENTS.md")
            .contains("Hand-written team rules")
    );
    assert!(
        !project
            .read("CLAUDE.md")
            .contains("Hand-written team rules")
    );
}

#[test]
fn init_rejects_an_unknown_existing_action() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--existing", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("adopt"))
        .stderr(predicate::str::contains("replace"));
}

#[test]
fn init_two_destinations_mapping_to_one_source_keep_the_first_and_report_the_rest() {
    let project = Project::empty();
    project.write("CLAUDE.md", "# From CLAUDE\n");
    project.write("AGENTS.md", "# From AGENTS\n");
    project
        .exuno()
        .args(["init", "--tools", "claude,codex", "--yes", "--no-sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopted AGENTS.md"))
        .stdout(predicate::str::contains(
            "Kept as-is CLAUDE.md — another file already became .ai/src/AGENTS.md",
        ));
    assert_eq!(project.read(".ai/src/AGENTS.md"), "# From AGENTS\n");
}

#[test]
fn init_a_file_no_tool_produces_is_kept_with_the_resolvers_reason() {
    let project = Project::empty();
    project.write(".cursor/rules/core.mdc", "body\n");
    project
        .exuno()
        .args(["init", "--tools", "cursor", "--yes", "--no-sync"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Kept as-is .cursor/rules/core.mdc — cursor injects a frontmatter header on sync.",
        ));
    assert_eq!(project.read(".cursor/rules/core.mdc"), "body\n");
}

#[test]
fn init_ci_github_writes_the_check_workflow_with_the_pinned_version() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--ci", "github"])
        .assert()
        .success();
    let workflow = project.read(".github/workflows/agentsync-check.yml");
    assert!(workflow.contains("agentsync check"));
    assert!(workflow.contains(&format!("AGENTSYNC_VERSION={}", engine_version())));
    assert!(!workflow.contains("__AGENTSYNC_VERSION__"));
}

#[test]
fn init_the_ci_workflow_ships_the_autofix_job_disabled() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--ci", "github"])
        .assert()
        .success();
    let workflow = project.read(".github/workflows/agentsync-check.yml");
    assert!(workflow.contains("autofix:"));
    assert!(workflow.contains("if: false"));
}

#[test]
fn init_an_existing_ci_workflow_is_never_overwritten() {
    let project = Project::empty();
    project.write(".github/workflows/agentsync-check.yml", "name: mine\n");
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--ci", "github"])
        .assert()
        .success();
    assert_eq!(
        project.read(".github/workflows/agentsync-check.yml"),
        "name: mine\n"
    );
}

#[test]
fn init_no_ci_workflow_without_ci() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".github")).unwrap();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".github/workflows/agentsync-check.yml"));
}

#[test]
fn init_rejects_an_unsupported_ci_provider() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--ci", "gitlab"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("github"));
}

#[test]
fn init_committed_mode_ends_by_telling_the_user_to_commit() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .stdout(predicate::str::contains("Commit"));
}
