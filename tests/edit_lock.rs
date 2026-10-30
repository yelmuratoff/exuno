//! The project lock the commands that edit the config or `.ai/src/` take.

mod common;

use common::Project;
use exuno::paths::DiskText;
use exuno::transaction::backup::{self, Retention};
use exuno::transaction::lock::Lock;
use predicates::prelude::*;

fn root(project: &Project) -> String {
    std::fs::canonicalize(project.path()).unwrap().disk_text()
}

fn sources(project: &Project) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut files: Vec<_> = walk(&project.join(".ai"))
        .into_iter()
        .filter(|path| !path.starts_with(project.join(".ai/backups")))
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect();
    files.sort();
    files
}

fn walk(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap().filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walk(&path));
        } else {
            found.push(path);
        }
    }
    found
}

#[test]
fn every_config_writer_refuses_while_another_run_holds_the_lock() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    let before = sources(&project);
    let (held, _) = Lock::acquire(&root(&project), "sync", Retention::Bounded).unwrap();
    let busy = format!(
        "Another exuno sync (pid {}) is changing this project",
        std::process::id()
    );
    let writers: [&[&str]; 13] = [
        &["enable", "cursor"],
        &["disable", "claude"],
        &["customize", "cursor"],
        &["add", "rule", "style"],
        &["add", "mcp", "docs", "--url", "https://example.com/mcp"],
        &["simplify", "--apply"],
        &["profile", "add", "work", "--tools", "claude"],
        &["profile", "remove", "hub", "--yes"],
        &["adopt", "CLAUDE.md", "--yes"],
        &["dedupe", "--yes"],
        &["migrate", "--legacy", "--apply", "--yes"],
        &["upgrade-config"],
        &["refresh", "--yes"],
    ];
    for args in writers {
        project
            .exuno()
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(busy.as_str()));
    }
    assert_eq!(sources(&project), before);
    drop(held);
}

#[test]
fn read_only_runs_take_no_lock() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/cursor.yaml", "name: Mine\n");
    let (held, _) = Lock::acquire(&root(&project), "sync", Retention::Bounded).unwrap();
    project.exuno().args(["simplify"]).assert().success();
    project.exuno().args(["resolve"]).assert().success();
    project.exuno().args(["profile", "list"]).assert().success();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success();
    project
        .exuno()
        .args(["refresh", "--dry-run"])
        .assert()
        .success();
    project
        .exuno()
        .args(["refresh", "--status"])
        .assert()
        .success();
    project
        .exuno()
        .args(["adopt", "CLAUDE.md", "--dry-run"])
        .assert()
        .stderr(predicate::str::contains("is changing this project").not());
    drop(held);
}

#[test]
fn add_takes_its_backup_policy_from_the_config_config_path_selects() {
    let project = Project::seeded(&[]);
    project.write("elsewhere.yaml", "backup:\n  retention: forever\n");
    for args in [
        &["add", "rule", "style"][..],
        &["add", "mcp", "docs", "--url", "https://example.com/mcp"],
    ] {
        project
            .exuno()
            .env("AGENTSYNC_CONFIG_PATH", project.join("elsewhere.yaml"))
            .args(args)
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "Invalid backup.retention 'forever'",
            ));
    }
}

/// Leaves `.ai/exuno.yaml` as `text` under a sync that died before restoring it.
fn interrupt_with_config(project: &Project, text: &str) {
    let root = root(project);
    let (lock, _) = Lock::acquire(&root, "sync", Retention::Bounded).unwrap();
    let snapshot = backup::create(
        &root,
        "sync",
        &[format!("{root}/.ai/exuno.yaml")],
        Retention::Bounded,
    )
    .unwrap();
    lock.begin(&snapshot).unwrap();
    project.write(".ai/exuno.yaml", text);
    drop(lock);
}

#[test]
fn a_tools_directory_an_interrupted_run_moved_outside_is_restored_before_the_refusal() {
    let project = Project::seeded(&[]);
    let config = project.read(".ai/exuno.yaml");
    let outside = tempfile::tempdir().unwrap();
    let moved = config.replace(
        "  tools: \".ai/src/tools\"",
        &format!("  tools: \"{}\"", outside.path().display()),
    );
    assert_ne!(moved, config);
    interrupt_with_config(&project, &moved);

    project
        .exuno()
        .args(["customize", "cursor"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "exuno customize changed nothing; run it again.",
        ))
        .stderr(predicate::str::contains("resolves outside the project").not());
    assert_eq!(project.read(".ai/exuno.yaml"), config);
}

#[test]
fn a_backup_policy_an_interrupted_run_broke_does_not_block_its_restore() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "-o", "bundle.tgz"])
        .assert()
        .success();
    let config = project.read(".ai/exuno.yaml");
    let broken = format!("{config}backup:\n  retention: forever\n");

    interrupt_with_config(&project, &broken);
    project
        .exuno()
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "exuno enable changed nothing; run it again.",
        ));
    assert_eq!(project.read(".ai/exuno.yaml"), config);

    interrupt_with_config(&project, &broken);
    project
        .exuno()
        .args(["import", "bundle.tgz"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Restored the state from before an interrupted exuno sync",
        ));
    assert_eq!(project.read(".ai/exuno.yaml"), config);
}

#[test]
fn enable_stops_after_restoring_an_interrupted_run_and_applies_when_run_again() {
    let project = Project::seeded(&[]);
    let config = project.read(".ai/exuno.yaml");
    interrupt_with_config(&project, "half-written\n");

    project
        .exuno()
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Restored the state from before an interrupted exuno sync",
        ))
        .stderr(predicate::str::contains(
            "exuno enable changed nothing; run it again.",
        ));
    assert_eq!(project.read(".ai/exuno.yaml"), config);
    assert!(!project.exists(".ai/backups/.pending"));

    project
        .exuno()
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line == "    - cursor")
    );
}
