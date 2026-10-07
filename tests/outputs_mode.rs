//! `tests/outputs_mode.bats`: the `outputs:` mode in agent_sync.yaml —
//! committed (generated files and the manifest stay visible to git) vs local
//! (both are gitignored).

mod common;

use std::process::Command as StdCommand;

use common::Project;
use predicates::prelude::*;

/// `git check-ignore -q <rel>`: whether git ignores the path.
fn is_ignored(project: &Project, rel: &str) -> bool {
    StdCommand::new("git")
        .args(["check-ignore", "-q", rel])
        .current_dir(project.path())
        .env("GIT_CONFIG_GLOBAL", absent_git_config())
        .env("GIT_CONFIG_SYSTEM", absent_git_config())
        .status()
        .unwrap()
        .success()
}

fn absent_git_config() -> std::path::PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

/// Rewrite the `outputs:` line in `.ai/exuno.yaml`.
fn set_outputs_mode(project: &Project, mode: &str) {
    let config = project.read(".ai/exuno.yaml");
    let rewritten: String = config
        .lines()
        .map(|line| {
            if line.starts_with("outputs: ") {
                format!("outputs: {mode}")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    project.write(".ai/exuno.yaml", &format!("{rewritten}\n"));
}

fn drop_outputs_key(project: &Project) {
    let config = project.read(".ai/exuno.yaml");
    let rewritten: String = config
        .lines()
        .filter(|line| !line.starts_with("outputs:"))
        .collect::<Vec<_>>()
        .join("\n");
    project.write(".ai/exuno.yaml", &format!("{rewritten}\n"));
}

fn init_no_sync(project: &Project, extra: &[&str]) {
    let mut args = vec!["init", "--tools", "claude", "--yes", "--no-sync"];
    args.extend_from_slice(extra);
    project.exuno().args(args).assert().success();
}

#[test]
fn outputs_init_defaults_new_projects_to_committed() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    assert!(
        project
            .read(".ai/exuno.yaml")
            .contains("outputs: committed")
    );
}

#[test]
fn outputs_init_outputs_local_writes_local() {
    let project = Project::empty();
    init_no_sync(&project, &["--outputs", "local"]);
    assert!(project.read(".ai/exuno.yaml").contains("outputs: local"));
}

#[test]
fn outputs_init_rejects_an_unknown_mode() {
    let project = Project::empty();
    project
        .exuno()
        .args([
            "init",
            "--tools",
            "claude",
            "--yes",
            "--no-sync",
            "--outputs",
            "bogus",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("committed"))
        .stderr(predicate::str::contains("local"));
}

#[test]
fn outputs_committed_sync_leaves_generated_files_and_the_manifest_visible_to_git() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("CLAUDE.md"));
    assert!(!is_ignored(&project, "CLAUDE.md"));
    assert!(!is_ignored(&project, ".ai/.sync-manifest"));
    // Committed mode has nothing to ignore, so sync need not create .gitignore.
    if project.exists(".gitignore") {
        assert!(
            !project
                .read(".gitignore")
                .contains("AI SYNC GENERATED START")
        );
    }
}

#[test]
fn outputs_local_sync_gitignores_generated_files_and_the_manifest() {
    let project = Project::empty();
    init_no_sync(&project, &["--outputs", "local"]);
    project.exuno().arg("sync").assert().success();
    assert!(is_ignored(&project, "CLAUDE.md"));
    assert!(is_ignored(&project, ".ai/.sync-manifest"));
}

#[test]
fn outputs_switching_local_to_committed_empties_the_managed_block() {
    let project = Project::empty();
    init_no_sync(&project, &["--outputs", "local"]);
    project.exuno().arg("sync").assert().success();
    assert!(is_ignored(&project, "CLAUDE.md"));

    set_outputs_mode(&project, "committed");
    project.exuno().arg("sync").assert().success();
    assert!(!is_ignored(&project, "CLAUDE.md"));
    assert!(!is_ignored(&project, ".ai/.sync-manifest"));
    assert!(
        project
            .read(".gitignore")
            .contains("AI SYNC GENERATED START")
    );
}

#[test]
fn outputs_a_missing_key_keeps_the_pre_existing_local_behaviour() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    drop_outputs_key(&project);
    project.exuno().arg("sync").assert().success();
    assert!(is_ignored(&project, "CLAUDE.md"));
    assert!(is_ignored(&project, ".ai/.sync-manifest"));
}

#[test]
fn outputs_a_missing_key_with_gitignore_update_false_means_committed() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    drop_outputs_key(&project);
    let config = project.read(".ai/exuno.yaml");
    let rewritten = config.replace("  update: true", "  update: false");
    project.write(".ai/exuno.yaml", &rewritten);
    project.write(".gitignore", "keep-me\n");
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".gitignore"), "keep-me\n");
    assert!(!is_ignored(&project, "CLAUDE.md"));
}

#[test]
fn outputs_an_unknown_value_fails_sync_before_writing_anything() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    set_outputs_mode(&project, "bogus");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("outputs"));
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn outputs_committed_mode_still_gitignores_profile_config_homes() {
    let project = Project::empty();
    init_no_sync(&project, &[]);
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(project.read(".gitignore").contains("claude-hub"));
    assert!(!is_ignored(&project, "CLAUDE.md"));
}
