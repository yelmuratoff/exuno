//! `tests/customize.bats`: `exuno customize` / `show` / `diff`.

mod common;

use common::Project;
use predicates::prelude::*;

#[test]
fn customize_creates_empty_override_stub() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "claude"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/tools/claude.yaml"));
}

#[test]
fn customize_stub_mentions_show_base() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "claude"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/tools/claude.yaml")
            .contains("exuno show claude --base")
    );
}

#[test]
fn customize_full_copies_entire_base() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "cursor", "--full"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/tools/cursor.yaml"));
    // Full copy should have name field from base.
    assert!(
        project
            .read(".ai/src/tools/cursor.yaml")
            .lines()
            .any(|line| line.starts_with("name:"))
    );
}

#[test]
fn customize_is_idempotent_warns_if_exists() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["customize", "claude"])
        .assert()
        .stdout(predicate::str::contains("Override already exists"));
}

#[test]
fn customize_unknown_tool_with_full_fails() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "bogus_tool_xyz", "--full"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No base template"));
}

#[test]
fn show_displays_effective_config() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["show", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("base"));
}

#[test]
fn show_base_prints_the_base_yaml() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["show", "claude", "--base"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Base template"));
}

#[test]
fn show_marks_user_overrides_with_star() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "claude", "--full"])
        .assert()
        .success();
    project
        .exuno()
        .args(["show", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user"));
}

#[test]
fn diff_with_no_overrides_shows_message() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .arg("diff")
        .assert()
        .success()
        .stdout(predicate::str::contains("No user overrides"));
}

#[test]
fn diff_lists_tools_with_overrides() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "claude", "--full"])
        .assert()
        .success();
    project
        .exuno()
        .arg("diff")
        .assert()
        .success()
        .stdout(predicate::str::contains("claude"));
}

// Phase 7: per-tool payload layout.

#[test]
fn customize_cursor_hooks_writes_to_ai_src_tools_cursor_hooks_json() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "cursor", "hooks", "--yes"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/tools/cursor/hooks.json"));
    // Legacy flat path must NOT be created.
    assert!(!project.join(".ai/src/hooks/cursor.json").exists());
}

#[test]
fn customize_migrates_an_existing_legacy_flat_file_to_per_tool_dir() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/mcp/cursor.json", "{\"marker\":\"USER\"}\n");
    project
        .exuno()
        .args(["customize", "cursor", "mcp"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Migrated legacy override"));
    // Legacy file moved into per-tool dir.
    assert!(!project.join(".ai/src/mcp/cursor.json").exists());
    assert!(project.exists(".ai/src/tools/cursor/mcp.json"));
    assert!(
        project
            .read(".ai/src/tools/cursor/mcp.json")
            .contains("USER")
    );
}

#[test]
fn show_cursor_hooks_reflects_per_tool_dir_override_with_star() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["customize", "cursor", "hooks", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["show", "cursor", "hooks"])
        .assert()
        .success()
        .stdout(predicate::str::contains("user override"))
        .stdout(predicate::str::contains("tools/cursor/hooks.json"));
}
