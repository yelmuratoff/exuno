//! `tests/drift.bats`: manifest creation, idempotent re-sync, refusal to
//! overwrite manual edits, `--force` override, dest deletion safety, tool
//! disable cleanup, doctor's drift section, and `--if-stale`.

mod common;

use common::Project;
use predicates::prelude::*;
use std::time::{Duration, SystemTime};

/// `seed_project` + `enable claude --no-scaffold` + `sync`: every case starts
/// from a synced tree.
fn synced_project() -> Project {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    project
}

/// Sets a file's mtime, the way `touch -t` pins it in the bats suite.
fn set_mtime(path: &std::path::Path, time: SystemTime) {
    let file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    let times = std::fs::FileTimes::new().set_modified(time);
    file.set_times(times).unwrap();
}

fn future_mtime() -> SystemTime {
    // 2030-12-31, well after anything the test writes "now".
    SystemTime::UNIX_EPOCH + Duration::from_secs(1_924_991_999)
}

fn past_mtime() -> SystemTime {
    // 2000-01-01, well before anything the test writes "now".
    SystemTime::UNIX_EPOCH + Duration::from_secs(946_684_800)
}

// ── Manifest creation ────────────────────────────────────────────────────

#[test]
fn manifest_is_created_on_first_sync() {
    assert!(synced_project().exists(".ai/.sync-manifest"));
}

#[test]
fn manifest_contains_tab_separated_path_hash_entries() {
    let project = synced_project();
    let manifest = project.read(".ai/.sync-manifest");
    let first_line = manifest.lines().next().unwrap();
    assert!(first_line.contains('\t'));
    let hash = first_line.rsplit('\t').next().unwrap();
    assert_eq!(hash.len(), 64);
}

#[test]
fn manifest_is_sorted_lc_all_c() {
    let project = synced_project();
    let manifest = project.read(".ai/.sync-manifest");
    let lines: Vec<&str> = manifest.lines().collect();
    let mut sorted = lines.clone();
    // `LC_ALL=C sort` orders by raw byte value, matching Rust's default
    // (unlocalized) `&str`/`&[u8]` ordering.
    sorted.sort();
    assert_eq!(lines, sorted);
}

#[test]
fn manifest_contains_expected_destinations() {
    let project = synced_project();
    let manifest = project.read(".ai/.sync-manifest");
    assert!(manifest.lines().any(|l| l.starts_with("CLAUDE.md\t")));
    assert!(manifest.lines().any(|l| l.starts_with(".claude/rules/")));
}

// ── Idempotency ──────────────────────────────────────────────────────────

#[test]
fn second_sync_produces_byte_identical_manifest() {
    let project = synced_project();
    let before = project.sha256(".ai/.sync-manifest");
    project.exuno().arg("sync").assert().success();
    let after = project.sha256(".ai/.sync-manifest");
    assert_eq!(before, after);
}

// ── Drift refusal ────────────────────────────────────────────────────────

#[test]
fn manual_edit_triggers_refusal() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Manual edits detected"))
        .stderr(predicate::str::contains(".claude/rules/core.md"));
}

#[test]
fn refused_sync_leaves_edited_file_untouched() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    let _ = project.exuno().arg("sync").assert();
    assert!(
        project
            .read(".claude/rules/core.md")
            .contains("MANUAL EDIT")
    );
}

#[test]
fn refused_sync_does_not_rewrite_manifest() {
    let project = synced_project();
    let before = project.sha256(".ai/.sync-manifest");
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    let _ = project.exuno().arg("sync").assert();
    let after = project.sha256(".ai/.sync-manifest");
    assert_eq!(before, after);
}

#[test]
fn force_overwrites_edited_file() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(
        !project
            .read(".claude/rules/core.md")
            .contains("MANUAL EDIT")
    );
}

#[test]
fn force_updates_manifest_to_new_dest_hashes() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    let before = project.sha256(".ai/.sync-manifest");
    project.exuno().args(["sync", "--force"]).assert().success();
    let after = project.sha256(".ai/.sync-manifest");
    // The manual edit never touched the manifest, and `--force` restores the
    // dest content to what source produced originally, so the manifest hash
    // matches its pre-edit baseline.
    assert_eq!(before, after);
}

// ── Safe scenarios ───────────────────────────────────────────────────────

#[test]
fn manually_deleted_dest_is_rewritten_without_error() {
    let project = synced_project();
    std::fs::remove_file(project.join(".claude/rules/core.md")).unwrap();
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/rules/core.md"));
}

#[test]
fn dry_run_does_not_check_drift_and_does_not_write_manifest() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    let before = project.sha256(".ai/.sync-manifest");
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success();
    let after = project.sha256(".ai/.sync-manifest");
    assert_eq!(before, after);
    assert!(
        project
            .read(".claude/rules/core.md")
            .contains("MANUAL EDIT")
    );
}

// ── Tool disable cleanup ─────────────────────────────────────────────────

#[test]
fn disabling_a_tool_drops_its_entries_from_the_manifest() {
    let project = synced_project();
    assert!(
        project
            .read(".ai/.sync-manifest")
            .lines()
            .any(|l| l.starts_with(".claude/"))
    );
    project
        .exuno()
        .arg("disable")
        .arg("claude")
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    // With claude the only enabled tool, an empty manifest is removed rather
    // than written empty (see `manifest::write`) — either state means no
    // `.claude/` entries remain.
    let no_claude_entries = !project.exists(".ai/.sync-manifest")
        || !project
            .read(".ai/.sync-manifest")
            .lines()
            .any(|l| l.starts_with(".claude/"));
    assert!(no_claude_entries);
}

// ── Doctor drift section ─────────────────────────────────────────────────

#[test]
fn doctor_reports_clean_manifest_when_nothing_changed() {
    let project = synced_project();
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("match the manifest"));
}

#[test]
fn doctor_reports_edited_file_as_drift() {
    let project = synced_project();
    project.append(".claude/rules/core.md", "MANUAL EDIT\n");
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains(".claude/rules/core.md"))
        .stdout(predicate::str::contains("edited since last sync"));
}

#[test]
fn doctor_reports_deleted_file_as_missing() {
    let project = synced_project();
    std::fs::remove_file(project.join(".claude/rules/core.md")).unwrap();
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains(".claude/rules/core.md"))
        .stdout(predicate::str::contains("missing"));
}

// ── User-added files in generated dirs ───────────────────────────────────

#[test]
fn sync_preserves_a_user_added_rule_in_a_generated_dir() {
    let project = synced_project();
    project.write(".claude/rules/my-own.md", "my own rule\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept"))
        .stderr(predicate::str::contains("my-own.md"));
    assert!(project.exists(".claude/rules/my-own.md"));
}

#[test]
fn preserved_user_added_file_is_not_recorded_in_the_manifest() {
    let project = synced_project();
    project.write(".claude/rules/my-own.md", "my own rule\n");
    project.exuno().arg("sync").assert().success();
    assert!(!project.read(".ai/.sync-manifest").contains("my-own.md"));
}

#[test]
fn sync_force_prunes_a_user_added_rule_in_a_generated_dir() {
    let project = synced_project();
    project.write(".claude/rules/my-own.md", "my own rule\n");
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(!project.exists(".claude/rules/my-own.md"));
}

#[test]
fn dry_run_previews_keeping_a_user_added_file_without_deleting_it() {
    let project = synced_project();
    project.write(".claude/rules/my-own.md", "my own rule\n");
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Would keep"));
    assert!(project.exists(".claude/rules/my-own.md"));
}

#[test]
fn obsolete_sync_generated_rule_is_still_pruned_when_removed_from_source() {
    let project = synced_project();
    project.write(".ai/src/rules/temp-rule.md", "# Temp\n");
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/rules/temp-rule.md"));
    std::fs::remove_file(project.join(".ai/src/rules/temp-rule.md")).unwrap();
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists(".claude/rules/temp-rule.md"));
}

#[test]
fn obsolete_sync_generated_skill_directory_is_pruned_when_removed_from_source() {
    let project = synced_project();
    project.write(
        ".ai/src/skills/temp-skill/SKILL.md",
        "---\nname: temp-skill\n---\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/skills/temp-skill/SKILL.md"));
    std::fs::remove_dir_all(project.join(".ai/src/skills/temp-skill")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept .claude/skills/temp-skill").not());
    assert!(!project.exists(".claude/skills/temp-skill"));
}

#[test]
fn sync_preserves_a_user_added_skill_directory_in_a_generated_dir() {
    let project = synced_project();
    project.write(".claude/skills/my-own/SKILL.md", "mine\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept .claude/skills/my-own"));
    assert!(project.exists(".claude/skills/my-own/SKILL.md"));
}

// ── --if-stale probe ─────────────────────────────────────────────────────
// The manifest mtime is pinned directly (rather than via a shell `touch -t`)
// so these stay deterministic regardless of clone/filesystem mtime quirks.

#[test]
fn if_stale_is_a_no_op_when_source_is_older_than_the_manifest() {
    let project = synced_project();
    // Manifest in the future — nothing under .ai/src/ is newer — fresh.
    set_mtime(&project.join(".ai/.sync-manifest"), future_mtime());
    project
        .exuno()
        .args(["sync", "--if-stale"])
        .assert()
        .success()
        .stdout(predicate::str::contains("[DONE] Synced ").not());
}

#[test]
fn if_stale_leaves_the_manifest_untouched_when_fresh() {
    let project = synced_project();
    set_mtime(&project.join(".ai/.sync-manifest"), future_mtime());
    let before = project.sha256(".ai/.sync-manifest");
    project
        .exuno()
        .args(["sync", "--if-stale"])
        .assert()
        .success();
    let after = project.sha256(".ai/.sync-manifest");
    assert_eq!(before, after);
}

#[test]
fn if_stale_runs_a_full_sync_when_source_is_newer_than_the_manifest() {
    let project = synced_project();
    // Manifest in the past — every source file is newer — stale.
    set_mtime(&project.join(".ai/.sync-manifest"), past_mtime());
    project
        .exuno()
        .args(["sync", "--if-stale"])
        .assert()
        .success()
        .stderr(predicate::str::contains("[DONE] Synced "));
}

#[test]
fn if_stale_treats_a_missing_manifest_as_stale_and_re_syncs() {
    let project = synced_project();
    std::fs::remove_file(project.join(".ai/.sync-manifest")).unwrap();
    project
        .exuno()
        .args(["sync", "--if-stale"])
        .assert()
        .success()
        .stderr(predicate::str::contains("[DONE] Synced "));
    assert!(project.exists(".ai/.sync-manifest"));
}

// ── First-run baseline ───────────────────────────────────────────────────

#[test]
fn first_sync_baseline_message_printed() {
    let project = synced_project();
    std::fs::remove_file(project.join(".ai/.sync-manifest")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Initialized .ai/.sync-manifest"));
    assert!(project.exists(".ai/.sync-manifest"));
}
