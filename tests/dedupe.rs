//! `tests/dedupe.bats`: `exuno dedupe` — deleting source files a parent
//! `.ai/src/` already holds byte for byte, across walk-up, `--against`,
//! `--workspace`, and `shared.path`. Deeper coverage of the interactive
//! delete/keep/view/quit prompt loop and the identical/divergent diffing
//! lives in the `#[cfg(all(test, unix))]` unit tests at the bottom of
//! `src/cli/dedupe.rs`, which call `dedupe()` directly; the cases here
//! exercise the compiled binary end to end against real nested projects.

mod common;

use common::Project;
use predicates::prelude::*;
use std::path::Path;

fn init_at(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    // A null stdin and stdout, or `init` reads the runner's console as a
    // terminal on Windows and waits for the wizard's answers forever.
    let status = std::process::Command::new(env!("CARGO_BIN_EXE_exuno"))
        .current_dir(dir)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .args(["init", "--no-detect"])
        .env(
            "GIT_CONFIG_GLOBAL",
            std::env::temp_dir().join("agentsync-tests-absent-gitconfig"),
        )
        .env(
            "GIT_CONFIG_SYSTEM",
            std::env::temp_dir().join("agentsync-tests-absent-gitconfig"),
        )
        .status()
        .unwrap();
    assert!(status.success(), "init failed in {}", dir.display());
}

fn git_init_at(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    for args in [
        vec!["init", "--quiet"],
        vec!["config", "user.email", "test@test.com"],
        vec!["config", "user.name", "Test"],
    ] {
        let status = std::process::Command::new("git")
            .current_dir(dir)
            .args(&args)
            .env(
                "GIT_CONFIG_GLOBAL",
                std::env::temp_dir().join("agentsync-tests-absent-gitconfig"),
            )
            .env(
                "GIT_CONFIG_SYSTEM",
                std::env::temp_dir().join("agentsync-tests-absent-gitconfig"),
            )
            .status()
            .unwrap();
        assert!(status.success());
    }
}

fn dedupe_in(dir: &Path, args: &[&str]) -> assert_cmd::assert::Assert {
    let mut command = assert_cmd::Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command.arg("dedupe").args(args).assert()
}

/// A parent project with one rule + one skill, and a child below it with the
/// SAME files (identical hash). Returns `(parent_dir, child_dir)`.
fn make_parent_child_identical(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let parent_dir = root.join("parent");
    let child_dir = parent_dir.join("child");
    init_at(&parent_dir);
    std::fs::write(parent_dir.join(".ai/src/rules/shared.md"), "shared rule\n").unwrap();
    std::fs::create_dir_all(parent_dir.join(".ai/src/skills/foo")).unwrap();
    std::fs::write(
        parent_dir.join(".ai/src/skills/foo/SKILL.md"),
        "shared skill\n",
    )
    .unwrap();

    init_at(&child_dir);
    std::fs::copy(
        parent_dir.join(".ai/src/rules/shared.md"),
        child_dir.join(".ai/src/rules/shared.md"),
    )
    .unwrap();
    std::fs::create_dir_all(child_dir.join(".ai/src/skills/foo")).unwrap();
    std::fs::copy(
        parent_dir.join(".ai/src/skills/foo/SKILL.md"),
        child_dir.join(".ai/src/skills/foo/SKILL.md"),
    )
    .unwrap();

    (parent_dir, child_dir)
}

#[test]
fn dedupe_yes_deletes_a_parent_skill_the_child_keeps_in_a_category() {
    let root = tempfile::tempdir().unwrap();
    let (_parent, child) = make_parent_child_identical(root.path());
    std::fs::create_dir_all(child.join(".ai/src/skills/meta")).unwrap();
    std::fs::rename(
        child.join(".ai/src/skills/foo"),
        child.join(".ai/src/skills/meta/foo"),
    )
    .unwrap();

    dedupe_in(&child, &["--yes"])
        .success()
        .stdout(predicate::str::contains("skills/foo/SKILL.md"));

    assert!(!child.join(".ai/src/skills/meta/foo/SKILL.md").exists());
    assert!(!child.join(".ai/src/skills/meta").exists());
}

#[test]
fn dedupe_requires_tty_without_yes() {
    let root = tempfile::tempdir().unwrap();
    let (_parent, child) = make_parent_child_identical(root.path());

    dedupe_in(&child, &[])
        .failure()
        .stderr(predicate::str::contains("interactive TTY"));
}

#[test]
fn dedupe_yes_deletes_identical_hash_files_and_prunes_empty_skill_dirs() {
    let root = tempfile::tempdir().unwrap();
    let (_parent, child) = make_parent_child_identical(root.path());

    dedupe_in(&child, &["--yes"])
        .success()
        .stdout(predicate::str::contains("rules/shared.md"))
        .stdout(predicate::str::contains("skills/foo/SKILL.md"));

    assert!(!child.join(".ai/src/rules/shared.md").exists());
    assert!(!child.join(".ai/src/skills/foo/SKILL.md").exists());
    // Skill dir pruned after losing its only file.
    assert!(!child.join(".ai/src/skills/foo").exists());
}

#[test]
fn dedupe_leaves_divergent_files_untouched_even_with_yes() {
    let root = tempfile::tempdir().unwrap();
    let parent_dir = root.path().join("parent");
    let child_dir = parent_dir.join("child");
    init_at(&parent_dir);
    std::fs::write(
        parent_dir.join(".ai/src/rules/shared.md"),
        "parent version\n",
    )
    .unwrap();
    init_at(&child_dir);
    std::fs::write(child_dir.join(".ai/src/rules/shared.md"), "child version\n").unwrap();

    dedupe_in(&child_dir, &["--yes"])
        .success()
        .stdout(predicate::str::contains("Divergent: 1"));

    assert!(child_dir.join(".ai/src/rules/shared.md").is_file());
    assert!(
        std::fs::read_to_string(child_dir.join(".ai/src/rules/shared.md"))
            .unwrap()
            .contains("child version")
    );
}

#[test]
fn dedupe_yes_adds_template_derived_dupe_to_declined() {
    let root = tempfile::tempdir().unwrap();
    let parent_dir = root.path().join("parent");
    let child_dir = parent_dir.join("child");
    init_at(&parent_dir);
    // Overwrite the shipped comments.md in parent with a stable test value.
    std::fs::write(
        parent_dir.join(".ai/src/rules/comments.md"),
        "comments rule\n",
    )
    .unwrap();

    init_at(&child_dir);
    std::fs::copy(
        parent_dir.join(".ai/src/rules/comments.md"),
        child_dir.join(".ai/src/rules/comments.md"),
    )
    .unwrap();

    dedupe_in(&child_dir, &["--yes"]).success();

    assert!(!child_dir.join(".ai/src/rules/comments.md").exists());
    let config = std::fs::read_to_string(child_dir.join(".ai/agent_sync.yaml")).unwrap();
    assert!(config.contains("  declined:"));
    assert!(config.contains("rules/comments.md"));
}

#[test]
fn dedupe_declines_into_the_config_agentsync_config_path_names() {
    let root = tempfile::tempdir().unwrap();
    let parent_dir = root.path().join("parent");
    let child_dir = parent_dir.join("child");
    init_at(&parent_dir);
    std::fs::write(
        parent_dir.join(".ai/src/rules/comments.md"),
        "comments rule\n",
    )
    .unwrap();
    init_at(&child_dir);
    std::fs::copy(
        parent_dir.join(".ai/src/rules/comments.md"),
        child_dir.join(".ai/src/rules/comments.md"),
    )
    .unwrap();
    let before = std::fs::read_to_string(child_dir.join(".ai/agent_sync.yaml")).unwrap();
    std::fs::write(child_dir.join("selected.yaml"), "tools:\n  enabled: []\n").unwrap();

    let mut command = assert_cmd::Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(&child_dir);
    common::scrub(&mut command);
    command
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .args(["dedupe", "--yes"])
        .assert()
        .success();

    let selected = std::fs::read_to_string(child_dir.join("selected.yaml")).unwrap();
    assert!(selected.contains("rules/comments.md"));
    assert_eq!(
        std::fs::read_to_string(child_dir.join(".ai/agent_sync.yaml")).unwrap(),
        before
    );

    let mut missing = assert_cmd::Command::new(env!("CARGO_BIN_EXE_exuno"));
    missing.current_dir(&child_dir);
    common::scrub(&mut missing);
    missing
        .env("AGENTSYNC_CONFIG_PATH", "missing.yaml")
        .args(["dedupe", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "AGENTSYNC_CONFIG_PATH is set but file not found",
        ));
}

#[test]
fn dedupe_yes_does_not_add_non_template_duplicate_to_declined() {
    let root = tempfile::tempdir().unwrap();
    let (_parent, child) = make_parent_child_identical(root.path());

    dedupe_in(&child, &["--yes"]).success();

    // rules/shared.md is NOT a shipped template — must not be added to declined.
    let config = std::fs::read_to_string(child.join(".ai/agent_sync.yaml")).unwrap();
    assert!(!config.contains("rules/shared.md"));
}

#[test]
fn dedupe_against_path_compares_against_an_explicit_tree() {
    let root = tempfile::tempdir().unwrap();
    let parent_dir = root.path().join("parent");
    let child_dir = root.path().join("unrelated");
    init_at(&parent_dir);
    std::fs::write(parent_dir.join(".ai/src/rules/shared.md"), "shared\n").unwrap();
    init_at(&child_dir);
    std::fs::copy(
        parent_dir.join(".ai/src/rules/shared.md"),
        child_dir.join(".ai/src/rules/shared.md"),
    )
    .unwrap();

    // Without --against, child has no parent walk-up target (sibling, not nested).
    dedupe_in(&child_dir, &["--yes"]).success();
    assert!(child_dir.join(".ai/src/rules/shared.md").is_file());

    // With --against, the dupe is found and removed.
    dedupe_in(
        &child_dir,
        &["--against", parent_dir.to_str().unwrap(), "--yes"],
    )
    .success();
    assert!(!child_dir.join(".ai/src/rules/shared.md").exists());
}

#[test]
fn dedupe_workspace_iterates_bottom_up_alphabetical() {
    // Three children under one parent. Each child has an identical dupe.
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().join("root");
    init_at(&root);
    std::fs::write(root.join(".ai/src/rules/shared.md"), "shared\n").unwrap();

    for name in ["alpha", "bravo", "charlie"] {
        let child = root.join(name);
        init_at(&child);
        std::fs::copy(
            root.join(".ai/src/rules/shared.md"),
            child.join(".ai/src/rules/shared.md"),
        )
        .unwrap();
    }

    let assert = dedupe_in(&root, &["--workspace", "--yes"]).success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(root.join(".ai/src/rules/shared.md").is_file());
    assert!(!root.join("alpha/.ai/src/rules/shared.md").exists());
    assert!(!root.join("bravo/.ai/src/rules/shared.md").exists());
    assert!(!root.join("charlie/.ai/src/rules/shared.md").exists());

    // Ordering: alpha first, then bravo, then charlie, then root (bottom-up alpha).
    let alpha_pos = stdout.find("→ alpha").expect("alpha listed");
    let bravo_pos = stdout.find("→ bravo").expect("bravo listed");
    let charlie_pos = stdout.find("→ charlie").expect("charlie listed");
    let root_pos = stdout.find("→ .").expect("root listed");
    assert!(alpha_pos < bravo_pos);
    assert!(bravo_pos < charlie_pos);
    assert!(charlie_pos < root_pos);
}

#[test]
fn dedupe_honors_shared_path_across_git_boundary() {
    // Single-project mode: child has its own .git so walk-up alone would stop
    // at its own root. Declaring shared.path makes the parent explicit and
    // dedupe must use it without --against.
    let root = tempfile::tempdir().unwrap();
    let outer = root.path().join("outer");
    let inner = outer.join("inner");
    git_init_at(&outer);
    init_at(&outer);
    std::fs::write(outer.join(".ai/src/rules/shared.md"), "shared\n").unwrap();

    git_init_at(&inner);
    init_at(&inner);
    std::fs::copy(
        outer.join(".ai/src/rules/shared.md"),
        inner.join(".ai/src/rules/shared.md"),
    )
    .unwrap();
    use std::io::Write;
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(inner.join(".ai/agent_sync.yaml"))
        .unwrap();
    write!(config, "\nshared:\n  path: \"../\"\n  inherit: rules\n").unwrap();
    drop(config);

    dedupe_in(&inner, &["--yes"])
        .success()
        .stdout(predicate::str::contains("(from shared.path)"));

    assert!(!inner.join(".ai/src/rules/shared.md").exists());
}

#[test]
fn dedupe_workspace_honors_per_project_shared_path_across_git_boundaries() {
    // Workspace fan-out: two sub-projects both declare shared.path. One shares
    // .git with parent (walk-up would have worked anyway), the other has its
    // own .git (walk-up would miss it). Both must dedupe.
    let root_dir = tempfile::tempdir().unwrap();
    let root = root_dir.path().join("root");
    init_at(&root);
    std::fs::write(root.join(".ai/src/rules/shared.md"), "shared\n").unwrap();
    use std::io::Write;

    // samerepo: no own .git, walk-up would find parent.
    let samerepo = root.join("samerepo");
    init_at(&samerepo);
    std::fs::copy(
        root.join(".ai/src/rules/shared.md"),
        samerepo.join(".ai/src/rules/shared.md"),
    )
    .unwrap();
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(samerepo.join(".ai/agent_sync.yaml"))
        .unwrap();
    write!(config, "\nshared:\n  path: \"../\"\n  inherit: rules\n").unwrap();
    drop(config);

    // ownrepo: has its own .git — without shared.path, walk-up stops here.
    let ownrepo = root.join("ownrepo");
    git_init_at(&ownrepo);
    init_at(&ownrepo);
    std::fs::copy(
        root.join(".ai/src/rules/shared.md"),
        ownrepo.join(".ai/src/rules/shared.md"),
    )
    .unwrap();
    let mut config = std::fs::OpenOptions::new()
        .append(true)
        .open(ownrepo.join(".ai/agent_sync.yaml"))
        .unwrap();
    write!(config, "\nshared:\n  path: \"../\"\n  inherit: rules\n").unwrap();
    drop(config);

    dedupe_in(&root, &["--workspace", "--yes"]).success();

    // Both children must have their dupe removed.
    assert!(!samerepo.join(".ai/src/rules/shared.md").exists());
    assert!(!ownrepo.join(".ai/src/rules/shared.md").exists());
    // Parent's file is untouched.
    assert!(root.join(".ai/src/rules/shared.md").is_file());
}

#[test]
fn dedupe_help_prints_usage() {
    Project::seeded(&[])
        .exuno()
        .args(["dedupe", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno dedupe — remove source files that duplicate a parent .ai/src/\n\n  USAGE\n    exuno dedupe [OPTIONS]\n",
        ))
        .stdout(predicate::str::contains("\n  OPTIONS\n    --against <path>   "))
        .stdout(predicate::str::contains("\n    --workspace        "))
        .stdout(predicate::str::contains("\n  BEHAVIOR\n"))
        .stdout(predicate::str::contains("\n    -h, --help         Show this help\n"));
}

#[test]
fn dedupe_rejects_workspace_and_against_combination() {
    Project::seeded(&[])
        .exuno()
        .args(["dedupe", "--workspace", "--against", "/tmp", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("mutually exclusive"));
}

// `dedupe lists duplicates in byte order whatever the locale` is a bare
// `Vec<String>::sort` (byte order, locale-independent) in `collect()` in
// `src/cli/dedupe.rs` rather than a shelled-out locale-sensitive `sort`, so
// there is no locale-dependent behavior left to gate. The ordering itself is
// exercised by `dedupe_workspace_iterates_bottom_up_alphabetical` above.
