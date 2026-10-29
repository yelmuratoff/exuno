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
    let writers: [&[&str]; 8] = [
        &["enable", "cursor"],
        &["disable", "claude"],
        &["customize", "cursor"],
        &["add", "rule", "style"],
        &["add", "mcp", "docs", "--url", "https://example.com/mcp"],
        &["simplify", "--apply"],
        &["profile", "add", "work", "--tools", "claude"],
        &["profile", "remove", "hub", "--yes"],
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
    let (held, _) = Lock::acquire(&root(&project), "sync", Retention::Bounded).unwrap();
    project.exuno().args(["simplify"]).assert().success();
    project.exuno().args(["resolve"]).assert().success();
    project.exuno().args(["profile", "list"]).assert().success();
    drop(held);
}

#[test]
fn enable_stops_after_restoring_an_interrupted_run_and_applies_when_run_again() {
    let project = Project::seeded(&[]);
    let root = root(&project);
    let config = project.read(".ai/exuno.yaml");
    let (lock, _) = Lock::acquire(&root, "sync", Retention::Bounded).unwrap();
    let snapshot = backup::create(
        &root,
        "sync",
        &[format!("{root}/.ai/exuno.yaml")],
        Retention::Bounded,
    )
    .unwrap();
    lock.begin(&snapshot).unwrap();
    project.write(".ai/exuno.yaml", "half-written\n");
    drop(lock);

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
