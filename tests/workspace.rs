//! `tests/workspace.bats`: `agentsync sync --workspace` and related fan-out
//! behavior. `cli::workspace::run` writes everything — the found-count, each
//! per-project `→ rel`, the completion line, and the "No .ai/ directories
//! found" hint — to stderr, like the sync log it wraps; stdout stays empty
//! for `--json` — see `src/cli/workspace.rs`.

mod common;

use std::path::{Path, PathBuf};

use assert_cmd::Command;
use common::Project;
use predicates::prelude::*;

fn exuno_at(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command
}

fn init_at(dir: &Path, args: &[&str]) {
    exuno_at(dir).arg("init").args(args).assert().success();
}

/// `_workspace_init_pair`: `root + root/leaf`, each initialized with only
/// `claude` enabled so sync output is predictable and fast.
fn workspace_init_pair(project: &Project) -> (PathBuf, PathBuf) {
    let root = project.join("root");
    let leaf = root.join("leaf");
    std::fs::create_dir_all(&root).unwrap();
    init_at(&root, &["--no-detect"]);
    exuno_at(&root)
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success();
    std::fs::create_dir_all(&leaf).unwrap();
    init_at(&leaf, &["--no-detect"]);
    exuno_at(&leaf)
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success();
    (root, leaf)
}

#[test]
fn sync_workspace_fails_when_no_ai_found_below_cwd() {
    let project = Project::empty();
    exuno_at(project.path())
        .args(["sync", "--workspace"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .ai/ directories found"));
}

#[test]
fn sync_workspace_dry_run_touches_every_project_in_bottom_up_alpha_order() {
    let project = Project::empty();
    let (root, _leaf) = workspace_init_pair(&project);

    let output = exuno_at(&root)
        .args(["sync", "--workspace", "--dry-run"])
        .assert()
        .success()
        .stdout("")
        .stderr(
            predicate::str::contains("Found 2 project(s)")
                .and(predicate::str::contains("→ leaf"))
                .and(predicate::str::contains("→ ."))
                .and(predicate::str::contains("Workspace sync complete")),
        )
        .get_output()
        .stderr
        .clone();
    let stderr = String::from_utf8(output).unwrap();
    let leaf_pos = stderr.find("→ leaf").unwrap();
    let root_pos = stderr.find("→ .").unwrap();
    assert!(leaf_pos < root_pos, "leaf must sync before root");
}

#[test]
fn sync_workspace_exits_with_the_highest_project_status() {
    let project = Project::empty();
    let (root, leaf) = workspace_init_pair(&project);
    exuno_at(&leaf)
        .args(["enable", "opencode", "--no-scaffold"])
        .assert()
        .success();
    std::fs::write(
        leaf.join(".ai/src/mcp.json"),
        "{\"mcpServers\":{\"x\":{}}}\n",
    )
    .unwrap();
    exuno_at(&leaf).arg("sync").assert().code(26);
    std::fs::remove_file(root.join(".ai/src/AGENTS.md")).unwrap();

    exuno_at(&root)
        .args(["sync", "--workspace"])
        .assert()
        .code(26)
        .stderr(predicate::str::contains("max exit code: 26"));
}

#[test]
fn sync_workspace_writes_outputs_in_every_project() {
    let project = Project::empty();
    let (root, leaf) = workspace_init_pair(&project);

    exuno_at(&root)
        .args(["sync", "--workspace"])
        .assert()
        .success();
    assert!(root.join("CLAUDE.md").is_file());
    assert!(leaf.join("CLAUDE.md").is_file());
}

#[test]
fn sync_workspace_forwards_extra_args_e_g_only_to_each_project() {
    let project = Project::empty();
    let (root, leaf) = workspace_init_pair(&project);
    let _ = std::fs::remove_file(root.join("CLAUDE.md"));
    let _ = std::fs::remove_file(leaf.join("CLAUDE.md"));

    // --only=cursor (not enabled) → claude output should NOT be touched.
    exuno_at(&root)
        .args(["sync", "--workspace", "--only", "cursor"])
        .assert()
        .success();
    assert!(!root.join("CLAUDE.md").exists());
    assert!(!leaf.join("CLAUDE.md").exists());
}

#[test]
fn sync_workspace_continues_past_per_project_failures() {
    let project = Project::empty();
    let (root, leaf) = workspace_init_pair(&project);
    std::fs::remove_file(leaf.join(".ai/src/AGENTS.md")).unwrap();

    // The exit status is data here: one project of the workspace fails.
    exuno_at(&root)
        .args(["sync", "--workspace"])
        .output()
        .unwrap();
    // Root must still produce CLAUDE.md; leaf's failure must not abort the loop.
    assert!(root.join("CLAUDE.md").is_file());
}

#[test]
fn sync_workspace_skips_ai_inside_vendored_and_vcs_directories() {
    let project = Project::empty();
    let (root, _leaf) = workspace_init_pair(&project);

    // A dependency shipping its own .ai/: syncing here writes config a package
    // manager will discard, and it is not the user's project.
    std::fs::create_dir_all(root.join("node_modules/some-pkg/.ai/src")).unwrap();
    std::fs::create_dir_all(root.join(".git/odd/.ai/src")).unwrap();

    exuno_at(&root)
        .args(["sync", "--workspace", "--dry-run"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains(".git/odd")
                .not()
                .and(predicate::str::contains("node_modules").not()),
        );
}

#[test]
fn sync_workspace_does_not_descend_into_a_projects_own_ai() {
    let project = Project::empty();
    let (root, _leaf) = workspace_init_pair(&project);

    // A backup snapshot mirrors .ai/src, which would otherwise look like a
    // second project nested inside the first.
    std::fs::create_dir_all(root.join(".ai/backups/20200101T000000Z-init-1/files/.ai/src"))
        .unwrap();

    exuno_at(&root)
        .args(["sync", "--workspace", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("backups").not());
}
