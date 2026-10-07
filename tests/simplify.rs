//! `tests/simplify.bats`: `exuno simplify` — dropping override fields
//! that match the base, deleting byte-identical payload overrides, and the
//! dry-run/--apply/-y interactions. Deeper coverage of the redundant-field
//! diffing and payload comparison logic lives in the
//! `#[cfg(all(test, unix))]` unit tests at the bottom of
//! `src/cli/simplify.rs`, which call `simplify()` directly; the cases here
//! exercise the compiled binary end to end.

mod common;

use common::Project;
use predicates::prelude::*;

fn customize_full(project: &Project, tool: &str) {
    project
        .exuno()
        .args(["customize", tool, "--full"])
        .assert()
        .success();
}

// ── No-override path ───────────────────────────────────────────────────────

#[test]
fn simplify_with_no_overrides_prints_friendly_message() {
    Project::seeded(&[])
        .exuno()
        .arg("simplify")
        .assert()
        .success()
        .stdout(predicate::str::contains("No user overrides"));
}

#[test]
fn simplify_rejects_unknown_tool_name() {
    let project = Project::seeded(&[]);
    customize_full(&project, "claude");

    project
        .exuno()
        .args(["simplify", "bogus_tool_xyz"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No override found"));
}

#[test]
fn simplify_rejects_unknown_flag() {
    Project::seeded(&[])
        .exuno()
        .args(["simplify", "--whatever"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown flag"));
}

// ── Dry-run preview ─────────────────────────────────────────────────────────

#[test]
fn simplify_is_dry_run_by_default_and_does_not_mutate_files() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    let before = std::fs::read(project.join(".ai/src/tools/cursor.yaml")).unwrap();

    project
        .exuno()
        .arg("simplify")
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));

    let after = std::fs::read(project.join(".ai/src/tools/cursor.yaml")).unwrap();
    assert_eq!(before, after);
}

#[test]
fn simplify_dry_run_flags_redundant_fields_and_file_would_delete() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");

    project
        .exuno()
        .arg("simplify")
        .assert()
        .success()
        .stdout(predicate::str::contains("Redundant"))
        .stdout(predicate::str::contains("targets.rules.dest"))
        .stdout(predicate::str::contains("would delete the override file"));
}

#[test]
fn simplify_dry_run_reports_remove_count_when_some_fields_stay() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    // Mutate one field so not every field is redundant. yaml_subset::value
    // returns the first match, so overwrite the file rather than appending.
    project.write(
        ".ai/src/tools/cursor.yaml",
        "name: \"Cursor\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".cursor/rules\"\n    extension: \".mdc\"\n",
    );

    project
        .exuno()
        .arg("simplify")
        .assert()
        .success()
        .stdout(predicate::str::contains("would remove"))
        .stdout(predicate::str::contains("would delete the override file").not());
}

// ── --apply removes redundant fields ────────────────────────────────────────

#[test]
fn simplify_apply_removes_redundant_fields_and_keeps_diverging_ones() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    // Override rules.extension so it diverges from base.
    project.write(
        ".ai/src/tools/cursor.yaml",
        "name: \"Cursor\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".cursor/rules\"\n    extension: \".mdcustom\"\n",
    );

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed"));

    let text = project.read(".ai/src/tools/cursor.yaml");
    assert!(project.exists(".ai/src/tools/cursor.yaml"));
    assert!(text.contains("extension: \".mdcustom\""));
    assert!(text.lines().any(|l| l == "enabled: true"));
    assert!(!text.lines().any(|l| l.starts_with("name:")));
    assert!(!text.contains("dest: \".cursor/rules\""));
}

#[test]
fn simplify_apply_y_deletes_override_when_all_fields_match_base() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    assert!(project.exists(".ai/src/tools/cursor.yaml"));

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"));

    assert!(!project.exists(".ai/src/tools/cursor.yaml"));
}

#[test]
fn simplify_apply_keeps_empty_file_when_no_y_and_no_tty() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");

    project
        .exuno()
        .args(["simplify", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Kept empty file"));

    assert!(project.exists(".ai/src/tools/cursor.yaml"));
    // Post-removal, file should have no real key: value content.
    let text = project.read(".ai/src/tools/cursor.yaml");
    assert!(text.lines().all(|line| {
        let trimmed = line.trim_start();
        trimmed.is_empty()
            || !trimmed
                .split(':')
                .nth(1)
                .is_some_and(|rest| rest.starts_with(' '))
    }));
}

// ── Idempotency ──────────────────────────────────────────────────────────────

#[test]
fn simplify_apply_is_idempotent() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    project.write(
        ".ai/src/tools/cursor.yaml",
        "name: \"Cursor\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".cursor/rules\"\n    extension: \".mdcustom\"\n",
    );

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success();
    let snapshot = std::fs::read(project.join(".ai/src/tools/cursor.yaml")).unwrap();

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No redundant fields"));

    let after = std::fs::read(project.join(".ai/src/tools/cursor.yaml")).unwrap();
    assert_eq!(snapshot, after);
}

// ── Per-tool filter ──────────────────────────────────────────────────────────

#[test]
fn simplify_tool_only_touches_that_tool() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    customize_full(&project, "claude");
    assert!(project.exists(".ai/src/tools/cursor.yaml"));
    assert!(project.exists(".ai/src/tools/claude.yaml"));

    project
        .exuno()
        .args(["simplify", "cursor", "--apply", "-y"])
        .assert()
        .success();

    // cursor override deleted, claude untouched.
    assert!(!project.exists(".ai/src/tools/cursor.yaml"));
    assert!(project.exists(".ai/src/tools/claude.yaml"));
}

#[test]
fn simplify_tool_only_reports_that_tool_in_dry_run() {
    let project = Project::seeded(&[]);
    customize_full(&project, "cursor");
    customize_full(&project, "claude");

    project
        .exuno()
        .args(["simplify", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Cursor"))
        .stdout(predicate::str::contains("Claude Code").not());
}

// ── Phase 6/7: payload overrides (hooks / mcp / settings) ───────────────────
//
// These tests hand-place scaffolded-but-unchanged payload copies that mirror
// what `customize cursor <resource>` would produce.

fn scaffold_cursor_payloads(project: &Project) {
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_hooks = std::fs::read(repo_root.join("lib/templates/hooks/cursor.json")).unwrap();
    let base_mcp = std::fs::read(repo_root.join("lib/templates/mcp/cursor.json")).unwrap();
    std::fs::create_dir_all(project.join(".ai/src/tools/cursor")).unwrap();
    std::fs::write(project.join(".ai/src/tools/cursor/hooks.json"), &base_hooks).unwrap();
    std::fs::write(project.join(".ai/src/tools/cursor/mcp.json"), &base_mcp).unwrap();
}

#[test]
fn simplify_apply_removes_byte_identical_payload_overrides() {
    let project = Project::seeded(&[]);
    scaffold_cursor_payloads(&project);
    assert!(project.exists(".ai/src/tools/cursor/hooks.json"));
    assert!(project.exists(".ai/src/tools/cursor/mcp.json"));

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Payload overrides"));

    // Scaffolded-but-unchanged copies are gone; sync now uses base directly.
    assert!(!project.exists(".ai/src/tools/cursor/hooks.json"));
    assert!(!project.exists(".ai/src/tools/cursor/mcp.json"));
}

#[test]
fn simplify_keeps_payload_overrides_that_diverge_from_base() {
    let project = Project::seeded(&[]);
    scaffold_cursor_payloads(&project);
    project.write(
        ".ai/src/tools/cursor/hooks.json",
        "{\"marker\":\"USER_EDIT\"}\n",
    );

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success();

    // Diverged override preserved; unchanged MCP removed.
    assert!(project.exists(".ai/src/tools/cursor/hooks.json"));
    assert!(
        project
            .read(".ai/src/tools/cursor/hooks.json")
            .contains("USER_EDIT")
    );
    assert!(!project.exists(".ai/src/tools/cursor/mcp.json"));
}

#[test]
fn simplify_dry_run_on_payloads_reports_byte_identical_files() {
    let project = Project::seeded(&[]);
    scaffold_cursor_payloads(&project);

    project
        .exuno()
        .arg("simplify")
        .assert()
        .success()
        .stdout(predicate::str::contains("Byte-identical"))
        .stdout(predicate::str::contains("would delete"));

    // Dry-run must not mutate.
    assert!(project.exists(".ai/src/tools/cursor/hooks.json"));
    assert!(project.exists(".ai/src/tools/cursor/mcp.json"));
}

#[test]
fn simplify_reads_payload_overrides_from_the_configured_tools_directory() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/agent_sync.yaml",
        "tools:\n  enabled: [cursor]\nsource:\n  tools: custom/tools\n",
    );
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_hooks = std::fs::read(repo_root.join("lib/templates/hooks/cursor.json")).unwrap();
    std::fs::create_dir_all(project.join("custom/tools/cursor")).unwrap();
    std::fs::write(project.join("custom/tools/cursor/hooks.json"), &base_hooks).unwrap();

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Payload overrides"));

    assert!(!project.exists("custom/tools/cursor/hooks.json"));
}

#[test]
fn simplify_flags_legacy_flat_layout_payloads_without_deleting_them() {
    let project = Project::seeded(&[]);
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_hooks = std::fs::read(repo_root.join("lib/templates/hooks/cursor.json")).unwrap();
    std::fs::create_dir_all(project.join(".ai/src/hooks")).unwrap();
    std::fs::write(project.join(".ai/src/hooks/cursor.json"), &base_hooks).unwrap();

    project
        .exuno()
        .args(["simplify", "--apply", "-y"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy layout"));

    // Legacy files are NOT deleted — only flagged for migration.
    assert!(project.exists(".ai/src/hooks/cursor.json"));
}
