//! `tests/version_pin.bats`: `agentsync_version` pin vs the running engine —
//! fatal when outputs are committed (every machine must generate identical
//! files), a warning otherwise.
//!
//! `render::check_version_pin` (sync) logs the mismatch/strict error through
//! `Log::error`/`Log::err` (stderr) and the warn message through
//! `Log::warning`/`Log::out` (stdout); `check`'s `version_pin_mismatch`
//! always writes to `Report::stderr`. See `src/output/log.rs` and
//! `src/cli/check.rs`.

mod common;

use common::Project;
use predicates::prelude::*;

/// `pin_version`: rewrite the `agentsync_version:` line in place.
fn pin_version(project: &Project, version: &str) {
    let config = project.read(".ai/agent_sync.yaml");
    let rewritten: String = config
        .lines()
        .map(|line| {
            if line.starts_with("agentsync_version:") {
                format!("agentsync_version: \"{version}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    project.write(".ai/agent_sync.yaml", &rewritten);
}

/// `set_version_pin_mode`: insert a `version_pin:\n  mode: <mode>` block
/// right after the `agentsync_version:` line.
fn set_version_pin_mode(project: &Project, mode: &str) {
    let config = project.read(".ai/agent_sync.yaml");
    let mut rewritten = String::new();
    for line in config.lines() {
        rewritten.push_str(line);
        rewritten.push('\n');
        if line.starts_with("agentsync_version:") {
            rewritten.push_str("version_pin:\n");
            rewritten.push_str(&format!("  mode: {mode}\n"));
        }
    }
    project.write(".ai/agent_sync.yaml", &rewritten);
}

fn init_committed(project: &Project) {
    project
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
}

fn init_local(project: &Project) {
    project
        .exuno()
        .args([
            "init",
            "--tools",
            "claude",
            "--yes",
            "--no-sync",
            "--outputs",
            "local",
        ])
        .assert()
        .success();
}

#[test]
fn version_pin_committed_mode_refuses_to_sync_with_a_different_engine() {
    let project = Project::empty();
    init_committed(&project);
    pin_version(&project, "0.1.0");
    project.exuno().arg("sync").assert().code(1).stderr(
        predicate::str::contains("pins exuno 0.1.0")
            .and(predicate::str::contains("exuno update 0.1.0"))
            .and(predicate::str::contains("exuno upgrade-config")),
    );
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_reads_an_exuno_version_key() {
    let project = Project::empty();
    init_committed(&project);
    let config = project
        .read(".ai/agent_sync.yaml")
        .lines()
        .map(|line| {
            if line.starts_with("agentsync_version:") {
                "exuno_version: \"0.1.0\"".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    project.write(".ai/agent_sync.yaml", &config);
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("pins exuno 0.1.0"));
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("pins exuno 0.1.0"));
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("differs from pinned v0.1.0"));
}

#[test]
fn version_pin_committed_mode_check_fails_with_the_same_explanation() {
    let project = Project::empty();
    init_committed(&project);
    project.exuno().arg("sync").assert().success();
    pin_version(&project, "0.1.0");
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("pins exuno 0.1.0"));
}

#[test]
fn version_pin_local_mode_without_version_pin_only_warns_and_still_syncs() {
    let project = Project::empty();
    init_local(&project);
    pin_version(&project, "0.1.0");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("pins exuno 0.1.0"));
    assert!(project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_local_mode_set_to_warn_only_warns_and_still_syncs() {
    let project = Project::empty();
    init_local(&project);
    pin_version(&project, "0.1.0");
    set_version_pin_mode(&project, "warn");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("pins exuno 0.1.0"));
    assert!(project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_the_scalar_shorthand_makes_local_mode_strict() {
    let project = Project::empty();
    init_local(&project);
    pin_version(&project, "0.1.0");
    project.append(".ai/agent_sync.yaml", "version_pin: strict\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("version_pin.mode 'strict'"));
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_check_treats_gitignore_update_false_as_committed_like_sync() {
    let project = Project::empty();
    init_committed(&project);
    let config = project.read(".ai/agent_sync.yaml");
    let rewritten: String = config
        .lines()
        .filter(|line| !line.starts_with("outputs:"))
        .map(|line| {
            if line == "  update: true" {
                "  update: false".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    project.write(".ai/agent_sync.yaml", &rewritten);
    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .contains("  update: false\n")
    );
    project.exuno().arg("sync").assert().success();
    pin_version(&project, "0.1.0");
    set_version_pin_mode(&project, "strict");
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("committed outputs must come"));
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("committed outputs must come"));
}

#[test]
fn version_pin_local_mode_can_be_made_strict_for_sync() {
    let project = Project::empty();
    init_local(&project);
    pin_version(&project, "0.1.0");
    set_version_pin_mode(&project, "strict");
    project.exuno().arg("sync").assert().code(1).stderr(
        predicate::str::contains("pins exuno 0.1.0")
            .and(predicate::str::contains("version_pin.mode 'strict'"))
            .and(predicate::str::contains("committed outputs must come").not()),
    );
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_local_strict_mode_also_fails_check() {
    let project = Project::empty();
    init_local(&project);
    project.exuno().arg("sync").assert().success();
    pin_version(&project, "0.1.0");
    set_version_pin_mode(&project, "strict");
    project.exuno().arg("check").assert().code(1).stderr(
        predicate::str::contains("pins exuno 0.1.0")
            .and(predicate::str::contains("version_pin.mode 'strict'"))
            .and(predicate::str::contains("committed outputs must come").not()),
    );
}

#[test]
fn version_pin_unknown_mode_fails_before_writing() {
    let project = Project::empty();
    init_local(&project);
    set_version_pin_mode(&project, "refuse");
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "[ERROR] Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml",
        ));
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn version_pin_check_rejects_an_unknown_mode() {
    let project = Project::empty();
    init_local(&project);
    set_version_pin_mode(&project, "refuse");
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml",
        ));
}

#[test]
fn version_pin_a_matching_pin_is_silent() {
    let project = Project::empty();
    init_committed(&project);
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("pins agentsync").not());
}

#[test]
fn version_pin_no_pin_means_no_check() {
    let project = Project::empty();
    init_committed(&project);
    let config = project.read(".ai/agent_sync.yaml");
    let rewritten: String = config
        .lines()
        .filter(|line| !line.starts_with("agentsync_version:"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    project.write(".ai/agent_sync.yaml", &rewritten);
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("pins agentsync").not());
}

#[test]
fn version_pin_upgrade_config_re_pins_to_the_running_engine_and_unblocks_sync() {
    let project = Project::empty();
    init_committed(&project);
    pin_version(&project, "0.1.0");
    project.exuno().arg("upgrade-config").assert().success();
    project.exuno().arg("sync").assert().success();
}

#[test]
fn upgrade_config_pins_the_config_agentsync_config_path_names() {
    let project = Project::seeded(&[]);
    let before = project.read(".ai/agent_sync.yaml");
    project.write("selected.yaml", "tools:\n  enabled: []\n");
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .arg("upgrade-config")
        .assert()
        .success();
    assert!(
        project
            .read("selected.yaml")
            .starts_with("agentsync_version: ")
    );
    assert_eq!(project.read(".ai/agent_sync.yaml"), before);
}
