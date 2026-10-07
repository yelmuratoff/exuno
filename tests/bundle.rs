//! `tests/bundle.bats`: `exuno export` and `exuno import` — a bundle
//! round trip, a directory source, and a GitHub archive served by a curl
//! stand-in on `PATH`.

mod common;

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use common::Project;
use predicates::prelude::*;

fn exuno_in(dir: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command
}

fn tar_list(archive: &Path) -> String {
    let output = StdCommand::new("tar")
        .arg("-tzf")
        .arg(archive)
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// A `curl` on `PATH` that serves `$FAKE_GITHUB_DIR/<owner>_<repo>-<branch>.tar.gz`
/// for the archive URL import builds, and fails like `curl -f` otherwise.
#[cfg(unix)]
fn github_stub(project: &Project) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;

    let stub_dir = project.join("stub");
    let github_dir = project.join("github");
    std::fs::create_dir_all(&stub_dir).unwrap();
    std::fs::create_dir_all(&github_dir).unwrap();
    let script = r#"#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        --max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
name="${url##*/archive/refs/heads/}"
repo="${url#https://github.com/}"; repo="${repo%%/archive/*}"
file="$FAKE_GITHUB_DIR/${repo//\//_}-$name"
[ -f "$file" ] || exit 22
cp "$file" "$out"
"#;
    let curl_path = stub_dir.join("curl");
    std::fs::write(&curl_path, script).unwrap();
    std::fs::set_permissions(&curl_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    (stub_dir, github_dir)
}

/// An archive as GitHub serves one: the repository under `<repo>-<branch>/`.
#[cfg(unix)]
fn github_archive(github_dir: &Path, owner_repo: &str, branch: &str) {
    let repo = owner_repo.split('/').nth(1).unwrap();
    let top = format!("{repo}-{branch}");
    let scratch = tempfile::tempdir().unwrap();
    let gh_root = scratch.path().join(&top);
    std::fs::create_dir_all(gh_root.join(".ai/src/rules")).unwrap();
    std::fs::write(
        gh_root.join(".ai/src/AGENTS.md"),
        format!("# From {branch}\n"),
    )
    .unwrap();
    std::fs::write(gh_root.join(".ai/src/rules/gh.md"), "# GH rule\n").unwrap();
    let archive_name = format!("{}-{branch}.tar.gz", owner_repo.replace('/', "_"));
    let status = StdCommand::new("tar")
        .arg("-czf")
        .arg(github_dir.join(&archive_name))
        .arg("-C")
        .arg(scratch.path())
        .arg(&top)
        .status()
        .unwrap();
    assert!(status.success());
}

#[cfg(unix)]
fn with_curl_stub(command: &mut Command, stub_dir: &Path, github_dir: &Path) {
    let path = format!(
        "{}:{}",
        stub_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    command.env("PATH", path).env("FAKE_GITHUB_DIR", github_dir);
}

#[test]
fn export_help_prints_usage() {
    Project::seeded(&[])
        .exuno()
        .args(["export", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno export — bundle source files into a shareable archive\n\n  USAGE\n    exuno export [OPTIONS]\n\n  OPTIONS\n    -o, --output <path>   ",
        ))
        .stdout(predicate::str::contains("\n    -h, --help            Show this help\n"));
}

#[test]
fn import_help_prints_usage() {
    Project::seeded(&[])
        .exuno()
        .args(["import", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno import — import config from GitHub, archive, or directory\n\n  USAGE\n    exuno import <source> [OPTIONS]\n\n  SOURCES\n    GitHub URL        https://github.com/user/repo\n",
        ))
        .stdout(predicate::str::contains("\n  OPTIONS\n    -b, --branch <name>   "))
        .stdout(predicate::str::contains("\n    -h, --help            Show this help\n"));
}

#[test]
fn export_writes_the_bundle_and_lists_its_contents() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .arg("export")
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTS.md"))
        .stdout(predicate::str::contains("rules/ ("))
        .stdout(predicate::str::contains("Exported!"));
    let archive = project.join("exuno-bundle.tar.gz");
    assert!(archive.is_file());
    let listing = tar_list(&archive);
    assert!(listing.lines().any(|l| l == ".ai/src/AGENTS.md"));
    assert!(listing.lines().any(|l| l == ".ai/exuno.yaml"));
}

#[test]
fn export_dry_run_writes_nothing() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
    assert!(!project.join("exuno-bundle.tar.gz").exists());
}

#[test]
fn export_sizes_a_relative_archive_from_the_project_root() {
    let project = Project::seeded(&[]);
    std::fs::create_dir_all(project.join("sub")).unwrap();
    exuno_in(&project.join("sub"))
        .env("AGENTSYNC_REPO_ROOT", project.path())
        .args(["export", "-o", "rel.tgz"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rel.tgz ("))
        .stdout(predicate::str::contains("(? B)").not());
    assert!(project.join("rel.tgz").is_file());
}

#[test]
fn export_fails_without_ai() {
    let project = Project::seeded(&[]);
    std::fs::remove_dir_all(project.join(".ai")).unwrap();
    project
        .exuno()
        .arg("export")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("No .ai/ directory found"));
}

#[test]
fn import_copies_a_bundle_into_a_fresh_project() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "-o", "bundle.tgz"])
        .assert()
        .success();
    std::fs::create_dir_all(project.join("fresh")).unwrap();
    std::fs::rename(project.join("bundle.tgz"), project.join("fresh/bundle.tgz")).unwrap();
    exuno_in(&project.join("fresh"))
        .args(["import", "bundle.tgz"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Imported!"));
    assert!(project.join("fresh/.ai/src/AGENTS.md").is_file());
    assert!(project.join("fresh/.ai/exuno.yaml").is_file());
    assert_eq!(
        project.read(".ai/src/rules/core.md"),
        project.read("fresh/.ai/src/rules/core.md")
    );
}

#[test]
fn import_reports_an_up_to_date_project() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "-o", "bundle.tgz"])
        .assert()
        .success();
    project
        .exuno()
        .args(["import", "bundle.tgz"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date!"));
}

#[test]
fn import_dry_run_previews_without_writing() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "-o", "bundle.tgz"])
        .assert()
        .success();
    std::fs::create_dir_all(project.join("fresh")).unwrap();
    std::fs::rename(project.join("bundle.tgz"), project.join("fresh/bundle.tgz")).unwrap();
    exuno_in(&project.join("fresh"))
        .args(["import", "bundle.tgz", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
    assert!(!project.join("fresh/.ai").exists());
}

#[test]
fn import_only_limits_the_targets() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/rules/other.md", "# Other rule\n");
    project.write("other/.ai/src/skills/new/SKILL.md", "# New skill\n");
    project
        .exuno()
        .args(["import", "other", "--only", "rules"])
        .assert()
        .success();
    assert!(project.join(".ai/src/rules/other.md").is_file());
    assert!(!project.join(".ai/src/skills/new").exists());
}

#[test]
fn import_only_that_matches_nothing_reports_an_up_to_date_project() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["export", "-o", "bundle.tgz"])
        .assert()
        .success();
    project
        .exuno()
        .args(["import", "bundle.tgz", "--only", "bogus"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date!"));
}

#[test]
fn import_only_previews_a_config_only_change() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/skills/new/SKILL.md", "# New skill\n");
    project.write("other/.ai/exuno.yaml", "outputs: committed\n");
    project
        .exuno()
        .args(["import", "other", "--only", "rules", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("~ exuno.yaml (update)"))
        .stdout(predicate::str::contains(
            "Summary: 0 new, 1 updated, 0 unchanged",
        ));
}

#[test]
fn export_carries_a_legacy_ai_agent_sync_yaml() {
    let project = Project::seeded(&[]);
    std::fs::rename(
        project.join(".ai/exuno.yaml"),
        project.join(".ai/agent_sync.yaml"),
    )
    .unwrap();
    project
        .exuno()
        .arg("export")
        .assert()
        .success()
        .stdout(predicate::str::contains("agent_sync.yaml"));
    let listing = tar_list(&project.join("exuno-bundle.tar.gz"));
    assert!(listing.lines().any(|l| l == ".ai/agent_sync.yaml"));
}

#[test]
fn import_writes_into_a_legacy_agent_sync_yaml_the_project_already_has() {
    let project = Project::seeded(&[]);
    std::fs::rename(
        project.join(".ai/exuno.yaml"),
        project.join(".ai/agent_sync.yaml"),
    )
    .unwrap();
    project.write("other/.ai/src/rules/core.md", "# Replaced core\n");
    project.write("other/.ai/exuno.yaml", "outputs: committed\n");
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/agent_sync.yaml"), "outputs: committed\n");
    assert!(!project.exists(".ai/exuno.yaml"));
}

#[test]
fn import_from_a_directory_updates_changed_files() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/rules/core.md", "# Replaced core\n");
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1 updated"));
    assert_eq!(project.read(".ai/src/rules/core.md"), "# Replaced core\n");
}

#[test]
fn import_refuses_a_source_without_ai() {
    let project = Project::seeded(&[]);
    project.write("plain/docs/readme.md", "x\n");
    project
        .exuno()
        .args(["import", "plain"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "No .ai/src/ (or .ai/) directory found in source.",
        ));
}

#[test]
fn import_rejects_an_unrecognized_source() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["import", "nothing.txt"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Cannot recognize source"));
}

// The curl stand-in is a shell script the binary cannot spawn on Windows.
#[cfg(unix)]
#[test]
fn import_downloads_a_github_archive_through_curl() {
    let project = Project::seeded(&[]);
    let (stub_dir, github_dir) = github_stub(&project);
    github_archive(&github_dir, "user/repo", "main");
    let mut command = project.exuno();
    with_curl_stub(&mut command, &stub_dir, &github_dir);
    command
        .args(["import", "https://github.com/user/repo", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Downloading user/repo (branch: main)",
        ))
        .stdout(predicate::str::contains("Downloaded."));
    assert!(project.join(".ai/src/rules/gh.md").is_file());
}

#[cfg(unix)]
#[test]
fn import_falls_back_to_master_when_main_is_missing() {
    let project = Project::seeded(&[]);
    let (stub_dir, github_dir) = github_stub(&project);
    github_archive(&github_dir, "user/repo2", "master");
    let mut command = project.exuno();
    with_curl_stub(&mut command, &stub_dir, &github_dir);
    command
        .args(["import", "https://github.com/user/repo2", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trying 'master'"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), "# From master\n");
}

#[cfg(unix)]
#[test]
fn import_reports_a_branch_that_cannot_be_downloaded() {
    let project = Project::seeded(&[]);
    let (stub_dir, github_dir) = github_stub(&project);
    let mut command = project.exuno();
    with_curl_stub(&mut command, &stub_dir, &github_dir);
    command
        .args(["import", "https://github.com/user/repo", "--branch", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Failed to download branch 'nope'.",
        ));
}
