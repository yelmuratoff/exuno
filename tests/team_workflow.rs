//! `tests/team_workflow.bats`: two-clone team workflow over a bare origin, in
//! both outputs modes.
//!
//! local: every clone regenerates outputs, so the manifest (what *this*
//! clone generated) must be gitignored with them — a committed manifest
//! beside ignored outputs makes every pull look like a manual edit.
//! committed: outputs and the manifest travel through git; a teammate who
//! never runs agentsync still gets current rules from git pull.

mod common;

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use predicates::prelude::*;

fn absent_git_config() -> PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

fn git_env(cmd: &mut StdCommand) {
    let absent = absent_git_config();
    cmd.env("GIT_CONFIG_GLOBAL", &absent)
        .env("GIT_CONFIG_SYSTEM", &absent)
        .env("GIT_AUTHOR_NAME", "Test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "Test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com");
}

/// `_git_step`: run one git step and assert it succeeded, printing what git
/// wrote so a failure on CI stays diagnosable.
fn git(dir: &Path, args: &[&str]) {
    let mut cmd = StdCommand::new("git");
    cmd.args(args).current_dir(dir);
    git_env(&mut cmd);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed (status {:?}) in {dir:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
}

/// A git query whose exit status is the answer (`check-ignore`,
/// `ls-files --error-unmatch`), not a step that must succeed.
fn git_succeeds(dir: &Path, args: &[&str]) -> bool {
    let mut cmd = StdCommand::new("git");
    cmd.args(args).current_dir(dir);
    git_env(&mut cmd);
    cmd.status().unwrap().success()
}

fn git_output(dir: &Path, args: &[&str]) -> String {
    let mut cmd = StdCommand::new("git");
    cmd.args(args).current_dir(dir);
    git_env(&mut cmd);
    let output = cmd.output().unwrap();
    assert!(
        output.status.success(),
        "git {args:?} failed (status {:?}) in {dir:?}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn exuno_at(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

fn append(path: &Path, content: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

/// Clone a: init in the given mode, sync, commit, push. Clone b: fresh clone.
/// `git clone` of the bare origin, retried once on a clean destination.
///
/// A local clone copies or hardlinks the object files, and on a CI filesystem
/// that occasionally fails partway — `failed to copy file to
/// '…/.git/objects/…': No such file or directory` on a macOS runner (CI run
/// 35495160944), where the same test passes on every other run and 25 times in
/// a row locally. The clone is the fixture here, not the behaviour under test.
fn git_clone(team_dir: &Path, origin: &Path, dest: &Path) {
    for attempt in 0..2 {
        let _ = std::fs::remove_dir_all(dest);
        let mut cmd = StdCommand::new("git");
        cmd.args(["clone", "--quiet"])
            .arg(origin)
            .arg(dest)
            .current_dir(team_dir);
        git_env(&mut cmd);
        let output = cmd.output().unwrap();
        if output.status.success() {
            return;
        }
        assert!(
            attempt == 0,
            "git clone into {dest:?} failed twice\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

fn seed_team(team_dir: &Path, mode: &str) -> (PathBuf, PathBuf) {
    let origin = team_dir.join("origin.git");
    git(
        team_dir,
        &["init", "--quiet", "--bare", origin.to_str().unwrap()],
    );

    let a = team_dir.join("a");
    git_clone(team_dir, &origin, &a);
    exuno_at(&a)
        .args(["init", "--tools", "claude", "--yes", "--outputs", mode])
        .assert()
        .success();
    exuno_at(&a).arg("sync").assert().success();
    git(&a, &["add", "-A"]);
    git(&a, &["commit", "--quiet", "-m", "init agentsync"]);
    git(&a, &["push", "--quiet", "-u", "origin", "HEAD"]);

    let b = team_dir.join("b");
    git_clone(team_dir, &origin, &b);
    (a, b)
}

/// Clone a edits a rule, syncs, and pushes.
fn push_rule_edit(a: &Path) {
    append(
        &a.join(".ai/src/rules/core.md"),
        "\n- Team rule added by a.\n",
    );
    exuno_at(a).arg("sync").assert().success();
    git(a, &["add", "-A"]);
    git(a, &["commit", "--quiet", "-m", "rules: add team rule"]);
    git(a, &["push", "--quiet"]);
}

/// Kept alive for the whole test so its `Drop` removes it at the end,
/// mirroring bats' `teardown_test_project` over `mktemp -d`.
fn team_dir() -> tempfile::TempDir {
    tempfile::tempdir().unwrap()
}

// ── local mode ──────────────────────────────────────────────────────────────

#[test]
fn team_local_sync_manifest_is_gitignored_alongside_the_outputs_it_describes() {
    let team = team_dir();
    let team_path = team.path();
    let (a, _b) = seed_team(team_path, "local");
    assert!(git_succeeds(
        &a,
        &["check-ignore", "-q", ".ai/.sync-manifest"]
    ));
    assert!(!git_succeeds(
        &a,
        &["ls-files", "--error-unmatch", ".ai/.sync-manifest"]
    ));
}

#[test]
fn team_local_a_rule_edit_commits_only_the_source_not_the_manifest() {
    let team = team_dir();
    let team_path = team.path();
    let (a, _b) = seed_team(team_path, "local");
    push_rule_edit(&a);
    let show = git_output(&a, &["show", "--stat", "--format=", "HEAD"]);
    assert!(show.contains("rules/core.md"));
    assert!(!show.contains("sync-manifest"));
}

#[test]
fn team_local_teammates_rule_edit_syncs_after_git_pull_without_a_drift_refusal() {
    let team = team_dir();
    let team_path = team.path();
    let (a, b) = seed_team(team_path, "local");
    exuno_at(&b).arg("sync").assert().success();
    push_rule_edit(&a);
    git(&b, &["pull", "--quiet"]);
    exuno_at(&b).arg("sync").assert().success();
    assert!(read(&b.join(".claude/rules/core.md")).contains("Team rule added by a"));
}

#[test]
fn team_local_manual_edit_of_a_generated_file_is_still_refused_after_a_pull() {
    let team = team_dir();
    let team_path = team.path();
    let (a, b) = seed_team(team_path, "local");
    exuno_at(&b).arg("sync").assert().success();
    push_rule_edit(&a);
    git(&b, &["pull", "--quiet"]);
    exuno_at(&b).arg("sync").assert().success();
    append(&b.join(".claude/rules/core.md"), "\n# hand edit\n");
    exuno_at(&b)
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
}

// ── committed mode ──────────────────────────────────────────────────────────

#[test]
fn team_committed_a_fresh_clone_has_generated_outputs_without_running_agentsync() {
    let team = team_dir();
    let team_path = team.path();
    let (_a, b) = seed_team(team_path, "committed");
    assert!(b.join("CLAUDE.md").is_file());
    assert!(b.join(".claude/rules/core.md").is_file());
    assert!(b.join(".ai/.sync-manifest").is_file());
}

#[test]
fn team_committed_a_rule_edit_commits_the_source_its_outputs_and_the_manifest() {
    let team = team_dir();
    let team_path = team.path();
    let (a, _b) = seed_team(team_path, "committed");
    push_rule_edit(&a);
    let show = git_output(&a, &["show", "--stat", "--format=", "HEAD"]);
    assert!(show.contains(".ai/src/rules/core.md"));
    assert!(show.contains(".claude/rules/core.md"));
    assert!(show.contains("sync-manifest"));
}

#[test]
fn team_committed_git_pull_alone_delivers_a_teammates_rule_edit() {
    let team = team_dir();
    let team_path = team.path();
    let (a, b) = seed_team(team_path, "committed");
    push_rule_edit(&a);
    git(&b, &["pull", "--quiet"]);
    assert!(read(&b.join(".claude/rules/core.md")).contains("Team rule added by a"));
    exuno_at(&b).arg("check").assert().success();
}

#[test]
fn team_committed_sync_after_a_pull_is_a_clean_no_op() {
    let team = team_dir();
    let team_path = team.path();
    let (a, b) = seed_team(team_path, "committed");
    push_rule_edit(&a);
    git(&b, &["pull", "--quiet"]);
    exuno_at(&b).arg("sync").assert().success();
    assert!(git_output(&b, &["status", "--porcelain"]).trim().is_empty());
}
