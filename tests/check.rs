//! `tests/check.bats`: `exuno check` on a project synced for Claude.

mod common;

use common::Project;
use predicates::prelude::*;

/// `init`, `enable claude`, `sync`: every case starts from a synced tree.
fn synced_project() -> Project {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    project
}

fn check(project: &Project) -> assert_cmd::assert::Assert {
    project.exuno().arg("check").assert()
}

#[test]
fn check_passes_after_sync() {
    check(&synced_project())
        .success()
        .stdout(predicate::str::contains("synced"));
}

#[test]
fn check_warns_about_the_file_a_disabled_target_left_behind() {
    let project = synced_project();
    assert!(project.exists("CLAUDE.md"));
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  agents:\n    enabled: false\n",
    );
    project.exuno().arg("sync").assert().success();
    check(&project)
        .success()
        .stdout(predicate::str::contains(
            "! CLAUDE.md is left from claude targets.agents, which is disabled; sync no longer updates it",
        ))
        .stdout(predicate::str::contains(
            "  • Delete it, or set targets.agents.enabled back to true",
        ))
        .stdout(predicate::str::contains("AGENTS.md is left").not());
}

#[test]
fn check_detects_minimax_mcp_drift() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["minimax"]);
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    project.exuno().arg("sync").assert().success();
    check(&project).success();
    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"changed\":{\"command\":\"echo\"}}}\n",
    );
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
}

#[test]
fn check_detects_kiro_steering_drift() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["kiro"]);
    project.exuno().arg("sync").assert().success();
    check(&project).success();
    project.append(".kiro/steering/core.md", "Edited by hand.\n");
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
}

#[test]
fn check_fails_when_generated_file_is_modified() {
    let project = synced_project();
    project.append("CLAUDE.md", "modified\n");
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
}

#[test]
fn check_fails_when_generated_file_is_missing() {
    let project = synced_project();
    std::fs::remove_file(project.join("CLAUDE.md")).unwrap();
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("Missing: CLAUDE.md"));
}

#[test]
fn check_detects_source_rule_changes() {
    let project = synced_project();
    project.append(".ai/src/rules/core.md", "# New rule\n");
    check(&project).code(1);
}

#[test]
fn check_follows_relative_shared_sources_and_detects_parent_changes_without_writing_outputs() {
    let project = synced_project();
    project.write(
        "shared parent/.ai/src/rules/parent-only.md",
        "parent rule\n",
    );
    project.append(
        ".ai/agent_sync.yaml",
        "\nshared:\n  path: \"shared parent\"\n  inherit: rules\n",
    );
    project.exuno().arg("sync").assert().success();
    let config_before = project.sha256(".ai/agent_sync.yaml");
    let manifest_before = project.sha256(".ai/.sync-manifest");
    let manifest = project.read(".ai/.sync-manifest");
    let generated = manifest
        .lines()
        .filter_map(|line| line.split('\t').next())
        .find(|path| path.ends_with("parent-only.md"))
        .unwrap()
        .to_string();
    let generated_before = project.sha256(&generated);

    check(&project).success();
    assert_eq!(project.sha256(".ai/agent_sync.yaml"), config_before);

    project.write(
        "shared parent/.ai/src/rules/parent-only.md",
        "changed parent rule\n",
    );
    check(&project)
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
    assert_eq!(project.sha256(&generated), generated_before);
    assert_eq!(project.sha256(".ai/.sync-manifest"), manifest_before);
    assert_eq!(
        project.read("shared parent/.ai/src/rules/parent-only.md"),
        "changed parent rule\n"
    );
}

// A global install makes the project root $HOME, so the root holds
// directories check has no business reading: OS-protected ones and multi-GB
// tool caches. `chmod 000` cannot deny root, and Windows ignores the bits, so
// the two cases pass vacuously there.

#[cfg(unix)]
#[test]
fn check_ignores_an_unreadable_directory_in_the_project_root() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = synced_project();
    std::fs::create_dir_all(project.join("protected/inner")).unwrap();
    common::chmod(&project.join("protected"), 0o000);
    let assert = check(&project);
    common::chmod(&project.join("protected"), 0o755);
    assert.success().stdout(predicate::str::contains("synced"));
}

#[cfg(unix)]
#[test]
fn check_ignores_an_unreadable_ai_backups_directory() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = synced_project();
    std::fs::create_dir_all(project.join(".ai/backups/snapshot")).unwrap();
    common::chmod(&project.join(".ai/backups"), 0o000);
    let assert = check(&project);
    common::chmod(&project.join(".ai/backups"), 0o755);
    assert.success().stdout(predicate::str::contains("synced"));
}

#[test]
fn check_ignores_unrelated_root_files_rather_than_reporting_them_as_drift() {
    let project = synced_project();
    project.write("unrelated/notes.txt", "not agentsync's business\n");
    project.write("stray.txt", "stray\n");
    check(&project)
        .success()
        .stdout(predicate::str::contains("stray.txt").not())
        .stdout(predicate::str::contains("unrelated").not());
}

#[test]
fn check_leaves_no_temp_artifacts_behind() {
    let project = synced_project();
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    project
        .exuno()
        .env("TMPDIR", &sandbox)
        .arg("check")
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn check_agrees_with_sync_when_shared_inherit_names_a_category_sync_skips() {
    let project = synced_project();
    project.write("parent/.ai/src/rules/parent-only.md", "parent rule\n");
    project.write(
        "parent/.ai/src/tools/claude.yaml",
        "targets:\n  agents:\n    dest: \"OTHER.md\"\n",
    );
    project.append(
        ".ai/agent_sync.yaml",
        "\nshared:\n  path: \"parent\"\n  inherit: rules, tools\n",
    );
    project.exuno().arg("sync").assert().success();
    check(&project)
        .success()
        .stdout(predicate::str::contains("synced"));
}

#[test]
fn check_help_is_answered_on_stdout_without_rendering() {
    Project::empty()
        .exuno()
        .args(["check", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno check — verify outputs are in sync with source\n\n  USAGE\n    exuno check\n",
        ))
        .stdout(predicate::str::contains("\n  EXIT STATUS\n"))
        .stderr("");
}
