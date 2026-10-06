//! `tests/shell_init.bats`: `agentsync shell-init` — the printed shell hook
//! snippet, plus its behaviour once actually eval'd into a shell.

mod common;

#[cfg(unix)]
use std::process::{Command as StdCommand, Output};

use common::Project;
use predicates::prelude::*;

fn shell_init(project: &Project, args: &[&str]) -> assert_cmd::assert::Assert {
    project.exuno().arg("shell-init").args(args).assert()
}

#[cfg(unix)]
/// Run `script` in `shell` (`bash` or `zsh`) inside `project`'s directory,
/// with `extra_env` added on top of the inherited environment.
fn spawn(project: &Project, shell: &str, script: &str, extra_env: &[(&str, &str)]) -> Output {
    let mut command = StdCommand::new(shell);
    command.arg("-c").arg(script).current_dir(project.path());
    for (key, value) in extra_env {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[cfg(unix)]
/// A `command -v agentsync` on PATH that prints `$AGENTSYNC_REPO_ROOT` to
/// `$AGENTSYNC_TEST_LOG` instead of syncing anything.
fn write_logging_stub(project: &Project, rel_dir: &str) {
    project.write(
        &format!("{rel_dir}/agentsync"),
        "#!/bin/sh\nprintf \"%s\\n\" \"$AGENTSYNC_REPO_ROOT\" >> \"$AGENTSYNC_TEST_LOG\"\n",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            project.join(&format!("{rel_dir}/agentsync")),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
}

#[cfg(unix)]
fn path_with(project: &Project, rel_dir: &str) -> String {
    format!(
        "{}:{}",
        project.join(rel_dir).display(),
        std::env::var("PATH").unwrap_or_default()
    )
}

#[cfg(unix)]
fn has_zsh() -> bool {
    StdCommand::new("which")
        .arg("zsh")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[test]
fn shell_init_zsh_prints_a_zsh_hook() {
    shell_init(&Project::empty(), &["zsh"])
        .success()
        .stdout(predicate::str::contains("agentsync shell hook (zsh)"))
        .stdout(predicate::str::contains(
            "add-zsh-hook chpwd _agentsync_autosync",
        ))
        .stdout(predicate::str::contains("agentsync sync --if-stale"));
}

#[test]
fn shell_init_bash_prints_a_bash_hook() {
    shell_init(&Project::empty(), &["bash"])
        .success()
        .stdout(predicate::str::contains("agentsync shell hook (bash)"))
        .stdout(predicate::str::contains("PROMPT_COMMAND"))
        .stdout(predicate::str::contains("agentsync sync --if-stale"));
}

#[test]
fn shell_init_checks_the_current_directory_for_ai_src() {
    shell_init(&Project::empty(), &["bash"])
        .success()
        .stdout(predicate::str::contains("\"$PWD/.ai/src\""));
}

#[cfg(unix)]
#[test]
fn shell_init_does_not_sync_a_parent_project_from_a_nested_directory() {
    let project = Project::empty();
    project.write("project/.ai/src/.keep", "");
    project.write("project/nested/.keep", "");
    write_logging_stub(&project, "stub");

    let script = format!(
        "eval \"$('{}' shell-init bash)\"\ncd \"$TEST_PROJECT_ROOT/nested\"\n_agentsync_autosync\n",
        env!("CARGO_BIN_EXE_exuno")
    );
    let output = spawn(
        &project,
        "bash",
        &script,
        &[
            ("PATH", &path_with(&project, "stub")),
            (
                "AGENTSYNC_TEST_LOG",
                &project.join("autosync.log").display().to_string(),
            ),
            ("AGENTSYNC_HOME", env!("CARGO_MANIFEST_DIR")),
            (
                "TEST_PROJECT_ROOT",
                &project.join("project").display().to_string(),
            ),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    assert!(!project.join("autosync.log").exists());
}

#[cfg(unix)]
#[test]
fn shell_init_syncs_when_the_current_directory_is_a_project_root() {
    let project = Project::empty();
    project.write("project/.ai/src/.keep", "");
    write_logging_stub(&project, "stub");

    let script = format!(
        "eval \"$('{}' shell-init bash)\"\ncd \"$TEST_PROJECT_ROOT\"\n_agentsync_autosync\n",
        env!("CARGO_BIN_EXE_exuno")
    );
    let output = spawn(
        &project,
        "bash",
        &script,
        &[
            ("PATH", &path_with(&project, "stub")),
            (
                "AGENTSYNC_TEST_LOG",
                &project.join("autosync.log").display().to_string(),
            ),
            ("AGENTSYNC_HOME", env!("CARGO_MANIFEST_DIR")),
            (
                "TEST_PROJECT_ROOT",
                &project.join("project").display().to_string(),
            ),
        ],
    );
    assert!(output.status.success(), "{output:?}");
    let log = project.read("autosync.log");
    let lines: Vec<&str> = log.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].ends_with("/project"));
}

#[test]
fn shell_init_snippet_honors_the_agentsync_no_auto_sync_kill_switch() {
    shell_init(&Project::empty(), &["zsh"])
        .success()
        .stdout(predicate::str::contains("AGENTSYNC_NO_AUTO_SYNC"));
}

#[test]
fn shell_init_hook_never_cds_zsh_chpwd_recursion_regression() {
    // A `cd` inside the chpwd hook would re-trigger it and recurse.
    let output = shell_init(&Project::empty(), &["zsh"])
        .success()
        .get_output()
        .stdout
        .clone();
    let text = String::from_utf8(output).unwrap();
    assert!(!text.contains("cd "));
    assert!(text.contains("AGENTSYNC_REPO_ROOT="));
}

#[test]
fn shell_init_hook_guards_against_re_entrancy() {
    shell_init(&Project::empty(), &["bash"])
        .success()
        .stdout(predicate::str::contains("_AGENTSYNC_BUSY"));
}

#[cfg(unix)]
#[test]
fn shell_init_zsh_hook_does_not_recurse_on_cd() {
    if !has_zsh() {
        eprintln!("skipping: zsh not installed");
        return;
    }
    let project = Project::empty();
    project.write("proj/.ai/src/.keep", "");
    project.write("stub/agentsync", "#!/bin/sh\nexit 0\n");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            project.join("stub/agentsync"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }

    let script = format!(
        "eval \"$('{}' shell-init zsh)\"\ncd '{}'\nprint OK\n",
        env!("CARGO_BIN_EXE_exuno"),
        project.join("proj").display()
    );
    let output = spawn(
        &project,
        "zsh",
        &script,
        &[("PATH", &path_with(&project, "stub"))],
    );
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("maximum nested"));
    assert!(stdout.contains("OK"));
}

#[test]
fn shell_init_auto_detects_zsh_from_shell() {
    Project::empty()
        .exuno()
        .arg("shell-init")
        .env("SHELL", "/usr/bin/zsh")
        .assert()
        .success()
        .stdout(predicate::str::contains("agentsync shell hook (zsh)"));
}

#[test]
fn shell_init_auto_detects_bash_from_shell() {
    Project::empty()
        .exuno()
        .arg("shell-init")
        .env("SHELL", "/bin/bash")
        .assert()
        .success()
        .stdout(predicate::str::contains("agentsync shell hook (bash)"));
}

#[test]
fn shell_init_errors_when_the_shell_is_unsupported() {
    shell_init(&Project::empty(), &["fish"]).code(2);
}

#[test]
fn shell_init_errors_when_the_shell_cannot_be_detected() {
    Project::empty()
        .exuno()
        .arg("shell-init")
        .env("SHELL", "")
        .assert()
        .code(2);
}

#[test]
fn shell_init_help_prints_usage() {
    shell_init(&Project::empty(), &["--help"])
        .success()
        .stdout(predicate::str::contains(
            "\n  USAGE\n    agentsync shell-init [zsh|bash]\n",
        ));
}

#[test]
fn shell_init_help_recommends_the_eval_form() {
    shell_init(&Project::empty(), &["--help"])
        .success()
        .stdout(predicate::str::contains(
            "eval \"$(agentsync shell-init zsh)\"",
        ));
}
