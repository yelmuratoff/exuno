use assert_cmd::Command;
use predicates::prelude::*;

fn engine_version() -> &'static str {
    include_str!("../VERSION").trim()
}

fn exuno() -> Command {
    Command::new(env!("CARGO_BIN_EXE_exuno"))
}

#[test]
fn version_prints_the_engine_version() {
    exuno()
        .arg("version")
        .assert()
        .success()
        .stdout(format!("exuno v{}\n", engine_version()));
}

#[test]
fn version_names_the_binary_it_was_run_as() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["agentsync", "exuno"] {
        let copy = dir
            .path()
            .join(format!("{name}{}", std::env::consts::EXE_SUFFIX));
        std::fs::copy(env!("CARGO_BIN_EXE_exuno"), &copy).unwrap();
        Command::new(&copy)
            .arg("version")
            .assert()
            .success()
            .stdout(format!("{name} v{}\n", engine_version()));
    }
}

#[test]
fn version_flags_match_the_bash_cli() {
    for flag in ["--version", "-v"] {
        exuno()
            .arg(flag)
            .assert()
            .success()
            .stdout(format!("exuno v{}\n", engine_version()));
    }
}

#[test]
fn list_works_without_a_project_config() {
    let dir = tempfile::tempdir().unwrap();
    exuno()
        .current_dir(dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  Exuno Tools\n"))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("  0 of 15 enabled\n"))
        .stdout(predicate::str::contains("Enable a tool:"));
}

#[test]
fn ls_is_an_alias_for_list() {
    let dir = tempfile::tempdir().unwrap();
    exuno()
        .current_dir(dir.path())
        .arg("ls")
        .assert()
        .success()
        .stdout(predicate::str::contains("  Exuno Tools\n"));
}

#[test]
fn list_and_version_ignore_extra_arguments_like_bash() {
    let dir = tempfile::tempdir().unwrap();
    exuno()
        .current_dir(dir.path())
        .args(["list", "--bogus"])
        .assert()
        .success()
        .stdout(predicate::str::contains("  Exuno Tools\n"));
    exuno()
        .args(["version", "extra"])
        .assert()
        .success()
        .stdout(format!("exuno v{}\n", engine_version()));
}

#[test]
fn list_counts_configured_tools_and_honours_the_repo_root_variable() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
    std::fs::write(
        dir.path().join(".ai/agent_sync.yaml"),
        "tools:\n  enabled:\n    - claude\n",
    )
    .unwrap();
    exuno()
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("  1 of 15 enabled\n"))
        .stdout(predicate::str::contains("Customize a tool:"))
        .stdout(predicate::str::contains("Enable a tool:").not());
}

#[cfg(unix)]
fn sync_project(tool_yaml: Option<&str>) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".ai/src/rules")).unwrap();
    std::fs::write(dir.path().join(".ai/src/AGENTS.md"), "# Agents\n").unwrap();
    std::fs::write(dir.path().join(".ai/src/rules/core.md"), "# Core\n").unwrap();
    std::fs::write(
        dir.path().join(".ai/agent_sync.yaml"),
        "outputs: committed\ntools:\n  enabled: [claude]\n",
    )
    .unwrap();
    if let Some(yaml) = tool_yaml {
        std::fs::create_dir_all(dir.path().join(".ai/src/tools")).unwrap();
        std::fs::write(dir.path().join(".ai/src/tools/claude.yaml"), yaml).unwrap();
    }
    dir
}

#[cfg(unix)]
fn sync_in(dir: &tempfile::TempDir) -> Command {
    let mut command = exuno();
    command
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .env_remove("AGENTSYNC_ALLOW_POST_SYNC")
        .env_remove("AGENTSYNC_SKIP_POST_SYNC")
        .arg("sync");
    command
}

#[cfg(unix)]
#[test]
fn sync_options_are_checked_before_anything_runs() {
    let dir = sync_project(None);
    sync_in(&dir)
        .arg("--bogus")
        .assert()
        .code(1)
        .stdout("")
        .stderr(predicate::str::starts_with(
            "[ERROR] Unknown option: --bogus\n\n  exuno sync — sync .ai/src/ to every enabled tool\n\n  USAGE\n    exuno sync [OPTIONS]\n",
        ));
    sync_in(&dir)
        .args(["--", "--dry-run"])
        .assert()
        .code(1)
        .stderr(predicate::str::starts_with("[ERROR] Unknown option: --\n"));
    sync_in(&dir)
        .arg("--help")
        .assert()
        .success()
        .stderr("")
        .stdout(predicate::str::contains(
            "\n  OPTIONS\n    --only <tools>     Sync only these tools (comma-separated)\n",
        ));
    assert!(!dir.path().join("CLAUDE.md").exists());
}

#[cfg(unix)]
#[test]
fn sync_writes_outputs_then_refuses_to_overwrite_a_manual_edit_unless_forced() {
    let dir = sync_project(None);
    sync_in(&dir)
        .assert()
        .success()
        .stderr(predicate::str::contains("[INFO] Syncing Claude Code\n"))
        .stderr(predicate::str::contains(
            "[DONE] Synced 1/15 tools (14 skipped)\n",
        ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "# Agents\n"
    );
    assert!(dir.path().join(".ai/.sync-manifest").is_file());
    assert!(dir.path().join(".ai/backups/.latest").is_file());

    std::fs::write(dir.path().join("CLAUDE.md"), "edited\n").unwrap();
    sync_in(&dir).assert().code(1).stderr(predicate::str::starts_with(
        "[ERROR] Manual edits detected in 1 destination file(s) since last sync:\n      CLAUDE.md\n",
    ));
    sync_in(&dir)
        .arg("--force")
        .assert()
        .success()
        .stderr(predicate::str::contains("      CLAUDE.md\n"));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "# Agents\n"
    );
}

#[cfg(unix)]
#[test]
fn a_terminated_sync_restores_the_pre_sync_state_and_dies_of_the_signal() {
    use std::io::{BufRead, BufReader};
    use std::os::unix::process::ExitStatusExt;

    let dir = sync_project(Some("post_sync: \"sleep 1\"\n"));
    std::fs::write(dir.path().join("CLAUDE.md"), "before-sync\n").unwrap();
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_exuno"))
        .env("AGENTSYNC_REPO_ROOT", dir.path())
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .env_remove("AGENTSYNC_SKIP_POST_SYNC")
        .arg("sync")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut lines = BufReader::new(child.stderr.take().unwrap()).lines();
    for line in lines.by_ref() {
        if line.unwrap().starts_with("[INFO] Running post-sync hook: ") {
            break;
        }
    }
    std::process::Command::new("kill")
        .args(["-TERM", &child.id().to_string()])
        .status()
        .unwrap();
    let rest: Vec<String> = lines.map(Result::unwrap).collect();
    let stdout: Vec<String> = BufReader::new(child.stdout.take().unwrap())
        .lines()
        .map(Result::unwrap)
        .collect();
    assert_eq!(child.wait().unwrap().signal(), Some(15));
    assert!(stdout.is_empty(), "stdout carried {stdout:?}");
    assert_eq!(
        rest[0],
        "[WARNING] Sync failed; restoring pre-sync state..."
    );
    assert!(rest[1].starts_with("[INFO] Restored pre-sync state from "));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "before-sync\n"
    );
    assert!(!dir.path().join(".claude/rules").exists());
}

#[cfg(unix)]
#[test]
fn a_failing_post_sync_hook_restores_the_pre_sync_state() {
    let dir = sync_project(Some("post_sync: \"false\"\n"));
    std::fs::write(dir.path().join("CLAUDE.md"), "before-sync\n").unwrap();
    sync_in(&dir)
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("[INFO] Restored pre-sync state from "))
        .stderr(predicate::str::contains(
            "[ERROR] Sync failed because post-sync hook failed for Claude Code\n[WARNING] Sync failed; restoring pre-sync state...\n",
        ));
    assert_eq!(
        std::fs::read_to_string(dir.path().join("CLAUDE.md")).unwrap(),
        "before-sync\n"
    );
    assert!(!dir.path().join(".claude/rules").exists());
    assert!(!dir.path().join(".ai/.sync-manifest").exists());
}

// The colour decision follows the stream the log is written to, so this needs
// stdout on a real pty while stderr is a file — `script(1)` provides the pty,
// as it does for the init wizard case.
#[cfg(unix)]
#[test]
fn sync_writes_a_plain_log_to_a_redirected_stderr_even_when_stdout_is_a_terminal() {
    use std::process::Command as StdCommand;

    let gnu = StdCommand::new("script")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if !gnu
        && StdCommand::new("which")
            .arg("script")
            .output()
            .is_ok_and(|o| !o.status.success())
    {
        eprintln!("script(1) not available; skipping");
        return;
    }
    let dir = sync_project(None);
    let log = dir.path().join("sync.log");
    let inner = format!(
        "AGENTSYNC_REPO_ROOT={} AGENTSYNC_NO_UPDATE_CHECK=1 NO_COLOR= {} sync --dry-run 2>{} </dev/null",
        dir.path().display(),
        env!("CARGO_BIN_EXE_exuno"),
        log.display()
    );
    let status = if gnu {
        StdCommand::new("script")
            .args(["-q", "-c", &inner, "/dev/null"])
            .stdout(std::process::Stdio::null())
            .status()
    } else {
        StdCommand::new("script")
            .args(["-q", "/dev/null", "sh", "-c", &inner])
            .stdout(std::process::Stdio::null())
            .status()
    }
    .unwrap();
    assert!(status.success());
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("[INFO] Syncing Claude Code\n"), "{text}");
    assert!(
        !text.contains('\x1b'),
        "escape codes reached the file: {text:?}"
    );
}

mod common;

// `tests/cli.bats`: help, version, unknown commands. The version cases are
// asserted above (`version_prints_the_engine_version`,
// `version_flags_match_the_bash_cli`).

#[test]
fn help_shows_usage() {
    common::Project::empty()
        .exuno()
        .arg("help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Exuno"))
        .stdout(predicate::str::contains("COMMANDS"))
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("sync"))
        .stdout(predicate::str::contains("rollback"));
}

#[test]
fn help_flag_shows_usage() {
    common::Project::empty()
        .exuno()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("COMMANDS"));
}

#[test]
fn unknown_command_fails_with_error() {
    common::Project::empty()
        .exuno()
        .arg("nonexistent")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Unknown command"));
}

#[test]
fn no_arguments_shows_help() {
    common::Project::empty()
        .exuno()
        .assert()
        .success()
        .stdout(predicate::str::contains("COMMANDS"));
}

#[test]
fn every_command_with_its_own_usage_answers_help_without_running() {
    let project = common::Project::seeded(&[]);
    let stale = project
        .read(".ai/agent_sync.yaml")
        .replace(engine_version(), "0.0.1");
    project.write(".ai/agent_sync.yaml", &stale);
    for command in [
        "init",
        "sync",
        "rollback",
        "check",
        "list",
        "enable",
        "disable",
        "add",
        "customize",
        "simplify",
        "migrate",
        "show",
        "diff",
        "resolve",
        "doctor",
        "dedupe",
        "adopt",
        "profile",
        "generate",
        "setup-hooks",
        "shell-init",
        "export",
        "import",
        "refresh",
        "update",
        "upgrade-config",
        "release",
    ] {
        project
            .exuno()
            .args([command, "--help"])
            .assert()
            .success()
            .stdout(predicate::str::starts_with(format!(
                "\n  exuno {command} — "
            )))
            .stderr("");
    }
    assert_eq!(
        project.read(".ai/agent_sync.yaml"),
        stale,
        "upgrade-config --help must not touch the pin"
    );
}

#[test]
fn rollback_help_documents_safe_restore_options() {
    common::Project::empty()
        .exuno()
        .args(["rollback", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--list"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--yes"));
}
