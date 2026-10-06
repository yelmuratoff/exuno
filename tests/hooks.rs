//! `tests/hooks.bats`: `agentsync setup-hooks` — which hooks each outputs
//! mode gets, that they land where git actually looks, and that the
//! committed-mode pre-commit gate blocks a commit whose generated files lag
//! the source.

mod common;

#[cfg(unix)]
use std::path::Path;
#[cfg(unix)]
use std::process::{Command as StdCommand, Output};

use common::Project;
use predicates::prelude::*;

/// Scaffold a project in `mode` without templates or a first sync — these
/// tests only need `agent_sync.yaml` and a git repo.
fn init_mode(project: &Project, mode: &str) {
    project
        .exuno()
        .args([
            "init",
            "--no-detect",
            "--no-templates",
            "--yes",
            "--outputs",
            mode,
            "--no-sync",
        ])
        .assert()
        .success();
}

#[cfg(unix)]
fn absent_git_config() -> std::path::PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

#[cfg(unix)]
fn git(
    project: &Project,
    args: &[&str],
    path_prefix: Option<&Path>,
    extra_env: &[(&str, &str)],
) -> Output {
    let absent = absent_git_config();
    let mut command = StdCommand::new("git");
    command
        .args(args)
        .current_dir(project.path())
        .env("GIT_CONFIG_GLOBAL", &absent)
        .env("GIT_CONFIG_SYSTEM", &absent);
    if let Some(prefix) = path_prefix {
        let existing = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{}:{existing}", prefix.display()));
    }
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[cfg(unix)]
/// Put an `agentsync` on PATH that runs the working copy, so an installed
/// hook can call it. Also keeps the developer's real install out of the test.
fn shim_agentsync_on_path(project: &Project) -> std::path::PathBuf {
    let bin_dir = project.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let shim = bin_dir.join("agentsync");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nexec \"{}\" \"$@\"\n",
            env!("CARGO_BIN_EXE_exuno")
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    bin_dir
}

// ── local mode: sync after pull/checkout ────────────────────────────────────

#[test]
fn setup_hooks_local_mode_creates_post_merge_and_post_checkout_hooks() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.exuno().arg("setup-hooks").assert().success();
    assert!(project.join(".git/hooks/post-merge").is_file());
    assert!(project.join(".git/hooks/post-checkout").is_file());
    assert!(
        project
            .read(".git/hooks/post-merge")
            .contains("AGENTSYNC AUTO SYNC")
    );
    assert!(
        project
            .read(".git/hooks/post-checkout")
            .contains("AGENTSYNC AUTO SYNC")
    );
}

#[test]
fn setup_hooks_local_mode_hook_invokes_the_installed_binary() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.exuno().arg("setup-hooks").assert().success();
    let hook = project.read(".git/hooks/post-merge");
    assert!(hook.contains("command -v agentsync"));
    assert!(hook.contains("agentsync sync"));
}

#[test]
fn setup_hooks_local_mode_hook_is_non_fatal_on_sync_failure() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.exuno().arg("setup-hooks").assert().success();
    assert!(
        project
            .read(".git/hooks/post-merge")
            .contains("agentsync sync ||")
    );
}

#[test]
fn setup_hooks_local_mode_is_idempotent() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.exuno().arg("setup-hooks").assert().success();
    project.exuno().arg("setup-hooks").assert().success();
    let count = project
        .read(".git/hooks/post-merge")
        .matches("AGENTSYNC AUTO SYNC START")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn setup_hooks_local_mode_preserves_existing_hook_content() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.write(
        ".git/hooks/post-merge",
        "#!/bin/sh\necho \"existing hook\"\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            project.join(".git/hooks/post-merge"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    project.exuno().arg("setup-hooks").assert().success();
    let hook = project.read(".git/hooks/post-merge");
    assert!(hook.contains("existing hook"));
    assert!(hook.contains("AGENTSYNC AUTO SYNC"));
}

#[test]
fn setup_hooks_local_mode_rewrites_an_outdated_block_in_place() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.write(
        ".git/hooks/post-checkout",
        "#!/bin/sh\necho before\n\n# >>> AGENTSYNC AUTO SYNC START >>>\nif [ -f \"lib/system/sync.sh\" ]; then\n    bash lib/system/sync.sh\nfi\n# <<< AGENTSYNC AUTO SYNC END <<<\necho after\n",
    );
    project
        .exuno()
        .arg("setup-hooks")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Updated AgentSync hook in post-checkout.\n",
        ));
    let hook = project.read(".git/hooks/post-checkout");
    assert!(!hook.contains("lib/system/sync.sh"), "{hook}");
    assert!(hook.starts_with("#!/bin/sh\necho before\n\n# >>> AGENTSYNC"));
    assert!(hook.ends_with("# <<< AGENTSYNC AUTO SYNC END <<<\necho after\n"));
    assert_eq!(hook.matches("AGENTSYNC AUTO SYNC START").count(), 1);
    assert!(hook.contains("agentsync sync ||"));
    project
        .exuno()
        .arg("setup-hooks")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "AgentSync hook already present in post-checkout.\n",
        ));
    assert_eq!(project.read(".git/hooks/post-checkout"), hook);
}

#[test]
fn setup_hooks_rewrites_an_exuno_block_in_place() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.write(
        ".git/hooks/post-checkout",
        "#!/bin/sh\necho before\n\n# >>> EXUNO AUTO SYNC START >>>\nold\n# <<< EXUNO AUTO SYNC END <<<\necho after\n",
    );
    project
        .exuno()
        .arg("setup-hooks")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Updated AgentSync hook in post-checkout.\n",
        ));
    let hook = project.read(".git/hooks/post-checkout");
    assert!(!hook.contains("\nold\n"), "{hook}");
    assert!(!hook.contains("EXUNO AUTO SYNC"), "{hook}");
    assert_eq!(hook.matches("AUTO SYNC START").count(), 1, "{hook}");
    assert!(hook.starts_with("#!/bin/sh\necho before\n\n# >>> AGENTSYNC"));
    assert!(hook.ends_with("# <<< AGENTSYNC AUTO SYNC END <<<\necho after\n"));
}

#[test]
fn setup_hooks_local_mode_installs_no_pre_commit_hook_by_default() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.exuno().arg("setup-hooks").assert().success();
    assert!(!project.join(".git/hooks/pre-commit").is_file());
}

#[test]
fn setup_hooks_local_mode_pre_commit_uses_if_stale() {
    let project = Project::empty();
    init_mode(&project, "local");
    project
        .exuno()
        .args(["setup-hooks", "--pre-commit"])
        .assert()
        .success();
    assert!(project.join(".git/hooks/pre-commit").is_file());
    assert!(
        project
            .read(".git/hooks/pre-commit")
            .contains("agentsync sync --if-stale")
    );
}

// ── committed mode: keep outputs in the same commit ─────────────────────────

#[test]
fn setup_hooks_committed_mode_installs_only_a_pre_commit_gate() {
    let project = Project::empty();
    init_mode(&project, "committed");
    project.exuno().arg("setup-hooks").assert().success();
    assert!(project.join(".git/hooks/pre-commit").is_file());
    assert!(!project.join(".git/hooks/post-merge").exists());
    assert!(!project.join(".git/hooks/post-checkout").exists());
}

#[test]
fn setup_hooks_committed_mode_gate_reads_the_manifest() {
    let project = Project::empty();
    init_mode(&project, "committed");
    project.exuno().arg("setup-hooks").assert().success();
    let gate = project.read(".git/hooks/pre-commit");
    assert!(gate.contains(".ai/.sync-manifest"));
    assert!(gate.contains("git add -A"));
}

#[test]
fn setup_hooks_every_hook_honours_agentsync_skip_hooks() {
    let project = Project::empty();
    init_mode(&project, "committed");
    project.exuno().arg("setup-hooks").assert().success();
    assert!(
        project
            .read(".git/hooks/pre-commit")
            .contains("AGENTSYNC_SKIP_HOOKS")
    );
}

// The following three exercise the installed hook through a real `git
// commit`, which looks up `agentsync` on PATH — a POSIX shim script this test
// puts there. There is no such shim mechanism worth trusting on Windows CI.
#[cfg(unix)]
#[test]
fn setup_hooks_committed_gate_blocks_a_commit_whose_outputs_lag_the_source() {
    let project = Project::empty();
    let shim_dir = shim_agentsync_on_path(&project);
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    project.exuno().arg("setup-hooks").assert().success();
    assert!(git(&project, &["add", "-A"], None, &[]).status.success());
    assert!(
        git(
            &project,
            &["commit", "--quiet", "-m", "add agentsync"],
            Some(&shim_dir),
            &[],
        )
        .status
        .success()
    );

    project.append(".ai/src/rules/core.md", "\n- A rule only in the source.\n");
    assert!(
        git(&project, &["add", ".ai/src/rules/core.md"], None, &[])
            .status
            .success()
    );

    let output = git(
        &project,
        &["commit", "-m", "rules: edit source only"],
        Some(&shim_dir),
        &[],
    );
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("out of date"));
    assert!(combined.contains(".claude/rules/core.md"));
}

#[cfg(unix)]
#[test]
fn setup_hooks_committed_gate_passes_once_the_outputs_are_staged() {
    let project = Project::empty();
    let shim_dir = shim_agentsync_on_path(&project);
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    project.exuno().arg("setup-hooks").assert().success();
    assert!(git(&project, &["add", "-A"], None, &[]).status.success());
    assert!(
        git(
            &project,
            &["commit", "--quiet", "-m", "add agentsync"],
            Some(&shim_dir),
            &[],
        )
        .status
        .success()
    );

    project.append(".ai/src/rules/core.md", "\n- A rule only in the source.\n");
    project.exuno().arg("sync").assert().success();
    assert!(git(&project, &["add", "-A"], None, &[]).status.success());

    let output = git(
        &project,
        &["commit", "-m", "rules: edit source and outputs"],
        Some(&shim_dir),
        &[],
    );
    assert!(output.status.success());
    let stat = git(
        &project,
        &["show", "--stat", "--format=", "HEAD"],
        None,
        &[],
    );
    assert!(String::from_utf8_lossy(&stat.stdout).contains(".claude/rules/core.md"));
}

#[cfg(unix)]
#[test]
fn setup_hooks_agentsync_skip_hooks_1_lets_the_commit_through() {
    let project = Project::empty();
    let shim_dir = shim_agentsync_on_path(&project);
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes"])
        .assert()
        .success();
    project.exuno().arg("setup-hooks").assert().success();
    assert!(git(&project, &["add", "-A"], None, &[]).status.success());
    assert!(
        git(
            &project,
            &["commit", "--quiet", "-m", "add agentsync"],
            Some(&shim_dir),
            &[],
        )
        .status
        .success()
    );

    project.append(".ai/src/rules/core.md", "\n- A rule only in the source.\n");
    assert!(
        git(&project, &["add", ".ai/src/rules/core.md"], None, &[])
            .status
            .success()
    );

    let output = git(
        &project,
        &["commit", "-m", "rules: source only, on purpose"],
        Some(&shim_dir),
        &[("AGENTSYNC_SKIP_HOOKS", "1")],
    );
    assert!(output.status.success());
}

// ── core.hooksPath and argument handling ────────────────────────────────────

#[test]
fn setup_hooks_refuses_to_write_when_core_hooks_path_points_elsewhere() {
    let project = Project::empty();
    init_mode(&project, "local");
    project.write(".githooks/.keep", "");
    project.git(&["config", "core.hooksPath", ".githooks"]);
    let assert = project
        .exuno()
        .arg("setup-hooks")
        .assert()
        .success()
        .stdout(predicates::str::contains("core.hooksPath"))
        .stdout(predicates::str::contains("sync --if-stale"));
    drop(assert);
    assert!(!project.join(".githooks/post-merge").exists());
    assert!(!project.join(".git/hooks/post-merge").exists());
}

#[test]
fn setup_hooks_rejects_unknown_options() {
    let project = Project::empty();
    init_mode(&project, "local");
    project
        .exuno()
        .args(["setup-hooks", "--bogus"])
        .assert()
        .code(2);
}

#[test]
fn setup_hooks_fails_outside_a_git_repository() {
    let project = Project::empty();
    init_mode(&project, "local");
    std::fs::remove_dir_all(project.join(".git")).unwrap();
    let output = project.exuno().arg("setup-hooks").output().unwrap();
    assert!(!output.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(combined.contains("git repository"));
}
