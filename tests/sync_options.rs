//! `tests/sync_options.bats`: sync filtering, dry-run, disable/enable,
//! idempotency, transactional backups, and the post-sync trust boundary.

mod common;

use common::Project;
use predicates::prelude::*;
use std::path::Path;

/// `seed_project`/`clone_seed`: a freshly initialised (committed-outputs)
/// project per test.
fn seeded() -> Project {
    Project::seeded(&[])
}

fn complete_backup_count(project: &Project) -> usize {
    std::fs::read_dir(project.join(".ai/backups"))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.path().join(".complete").is_file())
                .count()
        })
        .unwrap_or(0)
}

/// A recursive, sorted `(relative path, contents)` snapshot of `dir` — used
/// to compare a tree across two sync runs for idempotency.
fn snapshot_dir(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            if path.is_dir() {
                walk(base, &path, out);
            } else {
                let rel = path
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, std::fs::read(&path).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

// ── --only / --skip / --dry-run ─────────────────────────────────────────

#[test]
fn sync_only_filters_to_single_tool() {
    let project = seeded();
    project.enable_tools(&["claude", "cursor"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(project.exists("CLAUDE.md"));
    assert!(!project.exists("AGENTS.md"));
}

#[test]
fn sync_skip_excludes_a_tool() {
    let project = seeded();
    project.enable_tools(&["claude", "cursor"]);
    project
        .exuno()
        .args(["sync", "--skip", "claude"])
        .assert()
        .success();
    assert!(!project.exists("CLAUDE.md"));
    assert!(project.exists("AGENTS.md"));
}

#[test]
fn sync_only_and_skip_filter_kiro() {
    let project = seeded();
    project.enable_tools(&["claude", "kiro"]);
    project
        .exuno()
        .args(["sync", "--only", "kiro"])
        .assert()
        .success();
    assert!(project.exists(".kiro/steering/core.md"));
    assert!(!project.exists("CLAUDE.md"));

    let skipped = seeded();
    skipped.enable_tools(&["claude", "kiro"]);
    skipped
        .exuno()
        .args(["sync", "--skip", "kiro"])
        .assert()
        .success();
    assert!(skipped.exists("CLAUDE.md"));
    assert!(!skipped.exists(".kiro"));
}

#[test]
fn sync_only_and_skip_filter_minimax() {
    let project = seeded();
    project.enable_tools(&["claude", "minimax"]);
    project
        .exuno()
        .args(["sync", "--only", "minimax"])
        .assert()
        .success();
    assert!(project.exists("AGENTS.md"));
    assert!(!project.exists("CLAUDE.md"));

    let skipped = seeded();
    skipped.enable_tools(&["claude", "minimax"]);
    skipped
        .exuno()
        .args(["sync", "--skip", "minimax"])
        .assert()
        .success();
    assert!(skipped.exists("CLAUDE.md"));
    assert!(!skipped.exists("AGENTS.md"));
}

#[test]
fn sync_only_multiple_tools() {
    let project = seeded();
    project.enable_tools(&["claude", "cursor", "copilot"]);
    project
        .exuno()
        .args(["sync", "--only", "claude,cursor"])
        .assert()
        .success();
    assert!(project.exists("CLAUDE.md"));
    assert!(project.exists("AGENTS.md"));
    assert!(!project.exists(".github/copilot-instructions.md"));
}

#[test]
fn sync_dry_run_does_not_create_files() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    let backups_before = complete_backup_count(&project);
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("dry-run"));
    assert!(!project.exists("CLAUDE.md"));
    assert_eq!(complete_backup_count(&project), backups_before);
}

#[test]
fn sync_skips_disabled_tools() {
    // Claude is disabled by default after init — no need to flip.
    let project = seeded();
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn sync_cleans_up_when_tool_is_disabled() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(project.exists("CLAUDE.md"));
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn sync_copies_settings_json_for_claude() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/settings/claude.json",
        "{\"permissions\":{\"allow\":[\"Read\"]}}",
    );
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    let content = project.read(".claude/settings.json");
    assert!(content.contains("permissions"));
}

#[test]
fn sync_is_idempotent() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    let snapshot1 = snapshot_dir(&project.join(".claude"));
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    let snapshot2 = snapshot_dir(&project.join(".claude"));
    assert_eq!(snapshot1, snapshot2);
}

// ── Transactional backups ───────────────────────────────────────────────

#[test]
fn sync_backs_up_the_pre_sync_target_state() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write("CLAUDE.md", "before-sync\n");

    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();

    let snapshot_id = project.read(".ai/backups/.latest");
    let snapshot_id = snapshot_id.trim();
    let snapshot_rel = format!(".ai/backups/{snapshot_id}");
    let metadata = project.read(&format!("{snapshot_rel}/metadata"));
    assert!(metadata.lines().any(|l| l == "operation=sync"));
    assert_eq!(
        project.read(&format!("{snapshot_rel}/files/CLAUDE.md")),
        "before-sync\n"
    );
    let targets = project.read(&format!("{snapshot_rel}/targets.tsv"));
    assert!(targets.lines().any(|l| l == "missing\t.claude/rules"));
}

// A `bash -lc` hook is a POSIX shell dependency; not exercised on Windows.
#[cfg(unix)]
#[test]
fn sync_restores_every_target_when_a_post_sync_hook_fails() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write("CLAUDE.md", "before-sync\n");
    project.write(".gitignore", "before-gitignore\n");
    project.write(".ai/src/tools/claude.yaml", "post_sync: \"false\"\n");

    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Restored pre-sync state"));

    assert_eq!(project.read("CLAUDE.md"), "before-sync\n");
    assert_eq!(project.read(".gitignore"), "before-gitignore\n");
    assert!(!project.exists(".claude/rules"));
    assert!(!project.exists(".claude/skills"));
    assert!(!project.exists(".ai/.sync-manifest"));
}

// A `bash -lc` hook is a POSIX shell dependency; not exercised on Windows.
#[cfg(unix)]
#[test]
fn sync_refuses_a_second_run_while_the_first_holds_the_project() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/tools/claude.yaml",
        &format!(
            "post_sync: \"[ -e nested.log ] || '{}' sync --only claude 2> nested.log; true\"\n",
            env!("CARGO_BIN_EXE_exuno")
        ),
    );

    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .args(["sync", "--only", "claude"])
        .assert()
        .success();

    let nested = project.read("nested.log");
    assert!(
        nested.contains("Another exuno sync (pid ")
            && nested.contains(") is changing this project; wait for it to finish"),
        "{nested}"
    );
    assert!(!project.exists(".ai/backups/.pending"));
}

// A `bash -lc` hook is a POSIX shell dependency, and SIGKILL is POSIX.
#[cfg(unix)]
#[test]
fn sync_restores_a_run_killed_mid_write_before_it_starts() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write("CLAUDE.md", "before-sync\n");
    project.write(
        ".ai/src/tools/claude.yaml",
        "post_sync: \"kill -9 $PPID\"\n",
    );
    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .args(["sync", "--only", "claude"])
        .assert()
        .interrupted();
    let killed = project.read(".ai/backups/.latest").trim().to_string();
    assert!(
        project
            .read(".ai/backups/.pending")
            .contains(&format!("backup={killed}\n"))
    );
    let half_written = project.read("CLAUDE.md");
    assert_ne!(half_written, "before-sync\n");

    project.write(".ai/src/tools/claude.yaml", "{}\n");
    let assert = project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Restored the state from before an interrupted exuno sync (backup {killed}); undo with exuno rollback "
        )));
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr).into_owned();
    let undo = stderr
        .split("undo with exuno rollback ")
        .nth(1)
        .and_then(|rest| rest.split_whitespace().next())
        .unwrap();
    assert_eq!(
        project.read(&format!(".ai/backups/{undo}/files/CLAUDE.md")),
        half_written
    );
    assert!(project.exists(&format!(".ai/backups/{killed}/after.tsv")));
    assert!(!project.exists(".ai/backups/.pending"));
}

#[test]
fn sync_preflight_failures_do_not_create_a_backup() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    project.append("CLAUDE.md", "\nmanual edit\n");
    let backups_before = complete_backup_count(&project);

    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
    assert_eq!(complete_backup_count(&project), backups_before);
}

#[test]
fn sync_if_stale_no_op_does_not_create_a_backup() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    let backups_before = complete_backup_count(&project);

    project
        .exuno()
        .args(["sync", "--only", "claude", "--if-stale"])
        .assert()
        .success()
        .stdout("");
    assert_eq!(complete_backup_count(&project), backups_before);
}

// ── post_sync trust boundary ─────────────────────────────────────────────

#[test]
fn sync_in_repo_post_sync_allow_does_not_enable_the_hook() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/tools/claude.yaml",
        "post_sync: \"touch post_sync_ran\"\n",
    );
    // An in-repo allow must be ignored — cloning a repo can't run its hook.
    project.append(".ai/exuno.yaml", "\npost_sync:\n  allow: true\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Skipping post-sync hook"));
    assert!(!project.exists("post_sync_ran"));
}

// The hook runs through a POSIX shell (`bash -lc`).
#[cfg(unix)]
#[test]
fn sync_out_of_repo_env_allow_runs_the_post_sync_hook() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/tools/claude.yaml",
        "post_sync: \"touch post_sync_ran\"\n",
    );
    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .arg("sync")
        .assert()
        .success();
    assert!(project.exists("post_sync_ran"));
}

// ── A misconfigured per-tool source must not abort the whole run ─────────

#[test]
fn sync_bad_per_tool_source_is_skipped_run_completes_and_writes_manifest() {
    let project = seeded();
    project.enable_tools(&["claude", "cursor"]);
    project.write(
        ".ai/src/tools/cursor.yaml",
        "targets:\n  rules:\n    source: \".ai/src/DOES_NOT_EXIST\"\n",
    );
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Rules source not found"));
    // Both tools still produced output, and the manifest was written.
    assert!(project.exists("CLAUDE.md"));
    assert!(project.exists("AGENTS.md"));
    assert!(project.exists(".ai/.sync-manifest"));
    // A second sync sees no false drift.
    project.exuno().arg("check").assert().success();
}

// ── Per-target enabled: false opts a tool out of one whole category ──────

#[test]
fn sync_targets_category_enabled_false_skips_that_category_keeps_the_rest() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  rules:\n    enabled: false\n",
    );
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    // Non-opted-out categories still sync…
    assert!(project.exists("CLAUDE.md"));
    // …but the opted-out category is skipped entirely.
    assert!(!project.exists(".claude/rules"));
}

#[test]
fn sync_a_category_with_no_enabled_flag_stays_on_default() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--only", "claude"])
        .assert()
        .success();
    assert!(project.join(".claude/rules").is_dir());
}

// ── --quiet and --json: the two ways a script reads a sync ──────────────

#[test]
fn sync_quiet_leaves_only_the_closing_line_on_stderr_and_nothing_on_stdout() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--quiet"])
        .assert()
        .success()
        .stdout("")
        .stderr(predicate::str::starts_with("[DONE] Synced 1/"));
}

#[test]
fn sync_json_prints_one_summary_object_on_stdout() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    let output = project
        .exuno()
        .args(["sync", "--json"])
        .assert()
        .success()
        .stderr(predicate::str::contains("[DONE] Synced 1/"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert_eq!(stdout.lines().count(), 1, "one line: {stdout}");
    assert!(stdout.starts_with("{\"dry_run\":false,\"synced\":1,\"total\":"));
    assert!(stdout.contains("\"skipped\":[\"Amazon Q Developer\","));
    assert!(stdout.contains("\"written\":[\".claude/"));
    assert!(stdout.contains("\"CLAUDE.md\""));
    assert!(stdout.contains("\"preserved\":0,\"backup\":\".ai/backups/"));
    assert!(stdout.ends_with("\"}\n"));
}

#[test]
fn sync_json_on_a_dry_run_reports_no_writes_and_no_backup() {
    let project = seeded();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["sync", "--dry-run", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\"written\":[],\"preserved\":0,\"backup\":null}\n",
        ))
        .stdout(predicate::str::starts_with("{\"dry_run\":true,"));
}
