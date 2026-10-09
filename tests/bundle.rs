//! `tests/bundle.bats`: `exuno export` and `exuno import` — a bundle
//! round trip, a directory source, archives, and git remotes served from
//! local bare repositories.

mod common;

use std::path::Path;
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

/// Bare repositories under `remotes/<owner>/<repo>.git`, each built from a
/// working clone, and a git config that sends `https://github.com/` to them.
struct Remotes {
    dir: tempfile::TempDir,
}

impl Remotes {
    fn new() -> Self {
        Self {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    fn git(dir: &Path, args: &[&str]) {
        let status = StdCommand::new("git")
            .current_dir(dir)
            .args(["-c", "user.name=Test", "-c", "user.email=test@example.com"])
            .args(["-c", "commit.gpgsign=false", "-c", "tag.gpgsign=false"])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    fn bare(&self, name: &str) -> std::path::PathBuf {
        self.dir.path().join("remotes").join(format!("{name}.git"))
    }

    /// Commits `files` as the whole tree of `branch` in repository `name`.
    fn commit(&self, name: &str, branch: &str, files: &[(&str, &str)]) {
        let work = self.dir.path().join("work").join(name);
        if !work.exists() {
            std::fs::create_dir_all(&work).unwrap();
            Self::git(&work, &["init", "-q", "-b", branch]);
            let bare = self.bare(name);
            std::fs::create_dir_all(&bare).unwrap();
            Self::git(&bare, &["init", "-q", "--bare", "-b", branch]);
            Self::git(&work, &["remote", "add", "origin", &Self::slashed(&bare)]);
        }
        Self::git(&work, &["checkout", "-q", "--orphan", branch]);
        Self::git(&work, &["rm", "-rqf", "--ignore-unmatch", "."]);
        for (rel, text) in files {
            let path = work.join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }
        Self::git(&work, &["add", "-A"]);
        Self::git(&work, &["commit", "-q", "-m", branch]);
        Self::git(&work, &["push", "-q", "-f", "origin", branch]);
    }

    fn tag(&self, name: &str, tag: &str) {
        let work = self.dir.path().join("work").join(name);
        Self::git(&work, &["tag", tag]);
        Self::git(&work, &["push", "-q", "origin", tag]);
    }

    fn default_branch(&self, name: &str, branch: &str) {
        let target = format!("refs/heads/{branch}");
        Self::git(&self.bare(name), &["symbolic-ref", "HEAD", &target]);
    }

    /// `name`'s bare repository as a `file://` URL, never mistaken for a
    /// local directory to copy.
    fn url(&self, name: &str) -> String {
        let path = Self::slashed(&self.bare(name));
        if path.starts_with('/') {
            format!("file://{path}")
        } else {
            format!("file:///{path}")
        }
    }

    /// Adds a symbolic link `rel` → `target` to the branch last committed in
    /// repository `name`.
    #[cfg(unix)]
    fn commit_link(&self, name: &str, rel: &str, target: &Path) {
        let work = self.dir.path().join("work").join(name);
        std::os::unix::fs::symlink(target, work.join(rel)).unwrap();
        Self::git(&work, &["add", "-A"]);
        Self::git(&work, &["commit", "-q", "-m", "link"]);
        Self::git(&work, &["push", "-q", "-f", "origin", "HEAD"]);
    }

    /// A global git config whose `insteadOf` turns GitHub URLs into these
    /// repositories.
    fn github_config(&self) -> std::path::PathBuf {
        let config = self.dir.path().join("gitconfig");
        let remotes = Self::slashed(&self.dir.path().join("remotes"));
        std::fs::write(
            &config,
            format!("[url \"{remotes}/\"]\n\tinsteadOf = https://github.com/\n"),
        )
        .unwrap();
        config
    }
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
            "\n  exuno import — import config from a git remote, archive, or directory\n\n  USAGE\n    exuno import <source> [OPTIONS]\n\n  DESCRIPTION\n    An imported skill replaces the project's copy whole: files the new\n    version no longer has are removed. An import that changes files is\n    backed up first, so exuno rollback undoes it.\n\n  SOURCES\n    Git repository    https://github.com/user/repo[/tree/<ref>/<folder>],\n",
        ))
        .stdout(predicate::str::contains("\n  OPTIONS\n    --ref <name>       "))
        .stdout(predicate::str::contains("\n    -h, --help         Show this help\n"));
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

const MCP: &str = "{\n  \"mcpServers\": {\n    \"github\": {\"command\": \"npx\", \"args\": [\"-y\", \"@github/mcp-server\"]}\n  }\n}\n";

/// A seeded project that also carries shared MCP and a per-tool override,
/// the files a fixed target list used to leave out of the bundle.
fn project_with_every_source() -> Project {
    let project = Project::seeded(&[]);
    project.write(".ai/src/mcp.json", MCP);
    project.write(
        ".ai/src/tools/claude/settings.json",
        "{\"permissions\": {\"allow\": [\"Bash(ls)\"]}}\n",
    );
    project
}

/// Imports `bundle` into a fresh project beside `project` and returns it.
fn import_into_fresh(project: &Project, bundle: &str) -> std::path::PathBuf {
    let fresh = project.join("fresh");
    std::fs::create_dir_all(&fresh).unwrap();
    exuno_in(&fresh)
        .args(["import", &project.join(bundle).to_string_lossy(), "--force"])
        .assert()
        .success();
    fresh
}

#[test]
fn a_bundle_round_trip_keeps_every_source_file() {
    let project = project_with_every_source();
    project
        .exuno()
        .args(["export", "-o", "bundle.tar.gz"])
        .assert()
        .success();
    let fresh = import_into_fresh(&project, "bundle.tar.gz");
    assert_eq!(
        std::fs::read_to_string(fresh.join(".ai/src/mcp.json")).unwrap(),
        MCP
    );
    assert!(fresh.join(".ai/src/tools/claude/settings.json").is_file());
}

#[test]
fn export_writes_a_zip_when_the_output_says_so() {
    let project = project_with_every_source();
    project
        .exuno()
        .args(["export", "-o", "bundle.zip"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Exported!"));
    let bytes = std::fs::read(project.join("bundle.zip")).unwrap();
    let entries = exuno::zip::read(&bytes).unwrap();
    assert!(entries.iter().any(|entry| entry.path == ".ai/src/mcp.json"));
    assert!(entries.iter().any(|entry| entry.path == ".ai/exuno.yaml"));
    let fresh = import_into_fresh(&project, "bundle.zip");
    assert!(fresh.join(".ai/src/mcp.json").is_file());
}

/// Every file path an archive the project exported holds.
fn bundle_listing(project: &Project, name: &str) -> Vec<String> {
    let archive = project.join(name);
    if name.ends_with(".zip") {
        let bytes = std::fs::read(archive).unwrap();
        return exuno::zip::read(&bytes)
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect();
    }
    tar_list(&archive).lines().map(str::to_string).collect()
}

#[test]
fn export_leaves_hidden_files_out_of_the_bundle() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/review/.env", "TOKEN=secret\n");
    project.write(".ai/src/rules/.drafts/next.md", "# Draft\n");
    for name in ["bundle.zip", "bundle.tar.gz"] {
        project
            .exuno()
            .args(["export", "-o", name])
            .assert()
            .success();
        let listing = bundle_listing(&project, name);
        assert!(
            listing.iter().any(|path| path.ends_with("review/SKILL.md")),
            "{name}"
        );
        assert!(
            !listing
                .iter()
                .any(|path| path.contains(".env") || path.contains(".drafts")),
            "{name}: {listing:?}"
        );
    }
}

// Symbolic links need POSIX `symlink`; Windows grants it only with extra rights.
#[cfg(unix)]
#[test]
fn export_leaves_symlinks_out_of_the_bundle() {
    let project = Project::seeded(&[]);
    project.write("outside/secret.md", "# Secret\n");
    std::os::unix::fs::symlink(project.join("outside"), project.join(".ai/src/leak")).unwrap();
    std::os::unix::fs::symlink(
        project.join("outside/secret.md"),
        project.join(".ai/src/rules/link.md"),
    )
    .unwrap();
    for name in ["bundle.zip", "bundle.tar.gz"] {
        project
            .exuno()
            .args(["export", "-o", name])
            .assert()
            .success();
        let listing = bundle_listing(&project, name);
        assert!(
            !listing
                .iter()
                .any(|path| path.contains("leak") || path.contains("link.md")),
            "{name}: {listing:?}"
        );
    }
}

// `chmod` bits are POSIX, and root reads through an unreadable folder.
#[cfg(unix)]
#[test]
fn export_fails_rather_than_leave_out_an_unreadable_folder() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = Project::seeded(&[]);
    project.write(".ai/src/rules/private/locked.md", "# Locked\n");
    let locked = project.join(".ai/src/rules/private");
    common::chmod(&locked, 0o000);
    for name in ["bundle.zip", "bundle.tar.gz"] {
        project
            .exuno()
            .args(["export", "-o", name])
            .assert()
            .failure()
            .stderr(predicate::str::contains("rules/private"));
    }
    common::chmod(&locked, 0o755);
}

// `chmod` bits are POSIX, and root reads through an unreadable folder.
#[cfg(unix)]
#[test]
fn export_fails_rather_than_call_an_unreadable_source_base_empty() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = Project::seeded(&[]);
    let base = project.join(".ai/src");
    common::chmod(&base, 0o000);
    let assert = project
        .exuno()
        .args(["export", "-o", "bundle.zip"])
        .assert();
    common::chmod(&base, 0o755);
    assert.failure().stderr(predicate::str::contains(".ai/src"));
}

#[test]
fn import_lists_legacy_mcp_and_every_tool_config() {
    let project = Project::seeded(&[]);
    project.write(
        "other/.ai/src/mcp/claude.json",
        "{\"mcpServers\": {\"legacy\": {\"command\": \"run-me\"}}}\n",
    );
    project.write(
        "other/.ai/src/tools/opencode/hooks.ts",
        "export default {}\n",
    );
    project.write("other/.ai/src/tools/zed/settings.jsonc", "{ // hooks\n}\n");
    project
        .exuno()
        .args(["import", "other"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("MCP server legacy: run-me"))
        .stdout(predicate::str::contains("tools/opencode/hooks.ts"))
        .stdout(predicate::str::contains("tools/zed/settings.jsonc"));
}

#[test]
fn import_keeps_the_project_config_unless_asked_to_replace_it() {
    let project = Project::seeded(&[]);
    let local = project.read(".ai/exuno.yaml");
    project.write("other/.ai/src/rules/other.md", "# Other\n");
    project.write("other/.ai/exuno.yaml", "outputs: committed\n");
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "exuno.yaml kept (pass --config to replace)",
        ));
    assert_eq!(project.read(".ai/exuno.yaml"), local);
    project
        .exuno()
        .args(["import", "other", "--force", "--config"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/exuno.yaml"), "outputs: committed\n");
}

#[test]
fn import_only_mcp_takes_the_shared_mcp_file() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/mcp.json", MCP);
    project.write("other/.ai/src/rules/other.md", "# Other\n");
    project
        .exuno()
        .args(["import", "other", "--only", "mcp", "--force"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/mcp.json"), MCP);
    assert!(!project.join(".ai/src/rules/other.md").exists());
}

#[test]
fn import_lists_what_would_run_commands_and_refuses_without_force() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/mcp.json", MCP);
    project.write(
        "other/.ai/src/tools/claude/settings.json",
        "{\"hooks\": {\"Stop\": [{\"hooks\": [{\"type\": \"command\", \"command\": \"./notify.sh\"}]}]}}\n",
    );
    project.write("other/.ai/src/skills/jury/SKILL.md", JURY);
    project.write("other/.ai/src/skills/jury/scripts/audit.sh", "#!/bin/sh\n");
    project
        .exuno()
        .args(["import", "other"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Runs commands:"))
        .stdout(predicate::str::contains(
            "MCP server github: npx -y @github/mcp-server",
        ))
        .stdout(predicate::str::contains(
            "hooks in tools/claude/settings.json",
        ))
        .stdout(predicate::str::contains(
            "script skills/jury/scripts/audit.sh",
        ))
        .stderr(predicate::str::contains("--force"));
    assert!(!project.join(".ai/src/mcp.json").exists());
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/mcp.json"), MCP);
}

#[test]
fn import_dry_run_shows_what_would_run_commands() {
    let project = Project::seeded(&[]);
    project.write("other/.ai/src/mcp.json", MCP);
    project
        .exuno()
        .args(["import", "other", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Runs commands:"))
        .stdout(predicate::str::contains("Dry run"));
    assert!(!project.join(".ai/src/mcp.json").exists());
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
        .args(["import", "bundle.tgz", "--force"])
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
        .args([
            "import",
            "other",
            "--only",
            "rules",
            "--dry-run",
            "--config",
        ])
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
        .args(["import", "other", "--force", "--config"])
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
            "No .ai/src/ (or .ai/) directory or SKILL.md found in source.",
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

#[test]
fn import_fetches_the_default_branch_of_a_git_remote() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit(
        "acme/kit",
        "develop",
        &[(".ai/src/rules/kit.md", "# Kit\n")],
    );
    remotes.default_branch("acme/kit", "develop");
    project
        .exuno()
        .args(["import", &remotes.url("acme/kit"), "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Fetched."));
    assert_eq!(project.read(".ai/src/rules/kit.md"), "# Kit\n");
}

#[test]
fn import_follows_a_github_link_to_a_tag_and_folder() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit(
        "acme/mono",
        "main",
        &[(".ai/src/rules/root.md", "# Root\n")],
    );
    remotes.commit(
        "acme/mono",
        "release",
        &[("packages/app/.ai/src/rules/app.md", "# App\n")],
    );
    remotes.tag("acme/mono", "v1");
    project
        .exuno()
        .env("GIT_CONFIG_GLOBAL", remotes.github_config())
        .args([
            "import",
            "https://github.com/acme/mono/tree/v1/packages/app",
            "--force",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("at v1, packages/app"));
    assert_eq!(project.read(".ai/src/rules/app.md"), "# App\n");
    assert!(!project.join(".ai/src/rules/root.md").exists());
}

#[test]
fn import_follows_a_link_to_the_ai_folder_itself() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit(
        "acme/kit",
        "main",
        &[
            ("app/.ai/src/rules/app.md", "# App\n"),
            ("app/.ai/src/mcp.json", "{}\n"),
        ],
    );
    for link in ["tree/main/app/.ai", "tree/main/app/.ai/src"] {
        project
            .exuno()
            .env("GIT_CONFIG_GLOBAL", remotes.github_config())
            .args([
                "import",
                &format!("https://github.com/acme/kit/{link}"),
                "--force",
            ])
            .assert()
            .success();
        assert_eq!(project.read(".ai/src/rules/app.md"), "# App\n", "{link}");
        assert!(project.join(".ai/src/mcp.json").is_file(), "{link}");
        std::fs::remove_file(project.join(".ai/src/rules/app.md")).unwrap();
    }
}

#[test]
fn import_takes_owner_slash_repo_from_github() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit("acme/kit", "main", &[(".ai/src/rules/kit.md", "# Kit\n")]);
    project
        .exuno()
        .env("GIT_CONFIG_GLOBAL", remotes.github_config())
        .args(["import", "acme/kit", "--force"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/rules/kit.md"), "# Kit\n");
}

#[test]
fn import_takes_a_ref_and_folder_from_flags() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit("acme/kit", "main", &[("README.md", "# Kit\n")]);
    remotes.commit(
        "acme/kit",
        "feature/x",
        &[("tools/ai/.ai/src/rules/x.md", "# X\n")],
    );
    project
        .exuno()
        .args([
            "import",
            &remotes.url("acme/kit"),
            "--ref",
            "feature/x",
            "--path",
            "tools/ai",
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/rules/x.md"), "# X\n");
}

#[test]
fn import_reports_a_ref_the_remote_lacks() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit("acme/kit", "main", &[(".ai/src/rules/kit.md", "# Kit\n")]);
    project
        .exuno()
        .args(["import", &remotes.url("acme/kit"), "--branch", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("could not fetch"))
        .stderr(predicate::str::contains("at nope"))
        .stderr(predicate::str::contains("couldn't find remote ref nope"));
}

#[test]
fn a_flag_overrides_only_its_own_part_of_a_tree_link() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit("acme/mono", "main", &[("README.md", "# Mono\n")]);
    remotes.commit(
        "acme/mono",
        "release",
        &[
            ("packages/app/.ai/src/rules/app.md", "# App\n"),
            ("tools/ai/.ai/src/rules/tools.md", "# Tools\n"),
        ],
    );
    remotes.tag("acme/mono", "v1");
    project
        .exuno()
        .env("GIT_CONFIG_GLOBAL", remotes.github_config())
        .args([
            "import",
            "https://github.com/acme/mono/tree/v1/packages/app",
            "--path",
            "tools/ai",
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/rules/tools.md"), "# Tools\n");
    assert!(!project.join(".ai/src/rules/app.md").exists());
}

#[test]
fn import_follows_a_link_to_a_file_to_its_folder() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit(
        "acme/kit",
        "main",
        &[
            ("app/.ai/src/AGENTS.md", "# App\n"),
            ("app/.ai/src/rules/app.md", "# App rule\n"),
        ],
    );
    project
        .exuno()
        .env("GIT_CONFIG_GLOBAL", remotes.github_config())
        .args([
            "import",
            "https://github.com/acme/kit/blob/main/app/.ai/src/AGENTS.md?plain=1",
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/rules/app.md"), "# App rule\n");
}

// A filter driver runs through `sh` with POSIX `touch`.
#[cfg(unix)]
#[test]
fn import_runs_no_filter_driver_of_the_users_git_config() {
    let project = Project::seeded(&[]);
    let remotes = Remotes::new();
    remotes.commit(
        "acme/kit",
        "main",
        &[
            (".gitattributes", "* filter=probe\n"),
            (".ai/src/rules/kit.md", "# Kit\n"),
        ],
    );
    let marker = project.join("filter-ran");
    let config = project.join("gitconfig");
    std::fs::write(
        &config,
        format!(
            "[filter \"probe\"]\n\tsmudge = touch '{}' && cat\n",
            marker.display()
        ),
    )
    .unwrap();
    project
        .exuno()
        .env("GIT_CONFIG_GLOBAL", &config)
        .args(["import", &remotes.url("acme/kit"), "--force"])
        .assert()
        .success();
    assert!(!marker.exists(), "the remote's .gitattributes ran a filter");
    assert_eq!(project.read(".ai/src/rules/kit.md"), "# Kit\n");
}

// Symbolic links need POSIX `symlink`; Windows grants it only with extra rights.
#[cfg(unix)]
#[test]
fn import_never_follows_a_symlink_the_remote_ships() {
    let project = Project::seeded(&[]);
    project.write("private/.ai/src/rules/secret.md", "# Secret\n");
    let remotes = Remotes::new();
    remotes.commit("acme/kit", "main", &[("README.md", "# Kit\n")]);
    remotes.commit_link("acme/kit", ".ai", &project.join("private/.ai"));
    project
        .exuno()
        .args(["import", &remotes.url("acme/kit"), "--force"])
        .assert()
        .code(1);
    assert!(!project.join(".ai/src/rules/secret.md").exists());
}

const JURY: &str = "---\nname: jury\ndescription: Judge a submission.\n---\n# Jury\n";

fn zip_entry(path: &str, data: &str, executable: bool) -> exuno::zip::Entry {
    exuno::zip::Entry {
        path: path.to_string(),
        data: data.as_bytes().to_vec(),
        executable,
    }
}

fn write_zip(project: &Project, rel: &str, entries: &[exuno::zip::Entry]) {
    std::fs::write(project.join(rel), exuno::zip::write(entries).unwrap()).unwrap();
}

#[test]
fn import_installs_a_skill_package() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "jury.skill",
        &[
            zip_entry("jury/SKILL.md", JURY, false),
            zip_entry("jury/scripts/run.sh", "#!/bin/sh\n", true),
        ],
    );
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skills (2 new)"));
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let script = project.join(".ai/src/skills/jury/scripts/run.sh");
        let mode = std::fs::metadata(script).unwrap().permissions().mode();
        assert_eq!(mode & 0o111, 0o111);
    }
}

#[test]
fn import_names_a_root_skill_by_its_frontmatter() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "Download (1).zip",
        &[zip_entry("SKILL.md", JURY, false)],
    );
    project
        .exuno()
        .args(["import", "Download (1).zip"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
}

#[test]
fn import_names_a_root_skill_without_a_name_after_the_archive() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "plain-notes.ZIP",
        &[zip_entry("SKILL.md", "# Notes\n", false)],
    );
    project
        .exuno()
        .args(["import", "plain-notes.ZIP"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/skills/plain-notes/SKILL.md"),
        "# Notes\n"
    );
}

/// A project holding `jury` with a file the next version drops, and that
/// next version packaged as `jury.skill`.
fn project_with_an_outdated_jury() -> Project {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", "# Old\n");
    project.write(".ai/src/skills/jury/references/old.md", "# Dropped\n");
    write_zip(
        &project,
        "jury.skill",
        &[zip_entry("jury/SKILL.md", JURY, false)],
    );
    project
}

#[test]
fn import_removes_files_the_newer_skill_version_dropped() {
    let project = project_with_an_outdated_jury();
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "- skills/jury/references/old.md (removed)",
        ))
        .stdout(predicate::str::contains("1 updated, 1 removed"));
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
    assert!(
        !project
            .join(".ai/src/skills/jury/references/old.md")
            .exists()
    );
}

#[test]
fn import_dry_run_lists_removals_without_removing() {
    let project = project_with_an_outdated_jury();
    project
        .exuno()
        .args(["import", "jury.skill", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "- skills/jury/references/old.md (removed)",
        ))
        .stdout(predicate::str::contains("Dry run"));
    assert!(
        project
            .join(".ai/src/skills/jury/references/old.md")
            .is_file()
    );
    let snapshots = std::fs::read_dir(project.join(".ai/backups")).unwrap();
    assert!(
        snapshots
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains("-import-"))
    );
}

#[test]
fn rollback_undoes_an_import() {
    let project = project_with_an_outdated_jury();
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("undo with exuno rollback"));
    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), "# Old\n");
    assert_eq!(
        project.read(".ai/src/skills/jury/references/old.md"),
        "# Dropped\n"
    );
}

// `chmod` bits are POSIX, and root writes through a read-only directory.
#[cfg(unix)]
#[test]
fn a_failed_import_restores_what_it_already_wrote() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", "# Old\n");
    project.write(".ai/src/skills/jury/old.md", "# Dropped\n");
    let locked = project.join(".ai/src/skills/jury/locked");
    std::fs::create_dir_all(&locked).unwrap();
    common::chmod(&locked, 0o555);
    write_zip(
        &project,
        "jury.skill",
        &[
            zip_entry("jury/SKILL.md", JURY, false),
            zip_entry("jury/locked/new.md", "# New\n", false),
        ],
    );
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Import failed; restored the project from backup",
        ));
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), "# Old\n");
    assert_eq!(project.read(".ai/src/skills/jury/old.md"), "# Dropped\n");
    assert!(!locked.join("new.md").exists());
    common::chmod(&locked, 0o755);
}

#[test]
fn import_moves_a_skill_the_bundle_keeps_in_another_category() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/judging/jury/SKILL.md", "# Old\n");
    project.write(".ai/src/skills/judging/jury/extra.md", "# Extra\n");
    project.write("other/.ai/src/skills/panel/jury/SKILL.md", JURY);
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "- skills/judging/jury/extra.md (removed)",
        ));
    assert_eq!(project.read(".ai/src/skills/panel/jury/SKILL.md"), JURY);
    assert!(!project.join(".ai/src/skills/judging").exists());
}

#[test]
fn import_turns_a_skill_file_into_a_folder() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", "# Old\n");
    project.write(".ai/src/skills/jury/references", "a file\n");
    write_zip(
        &project,
        "jury.skill",
        &[
            zip_entry("jury/SKILL.md", JURY, false),
            zip_entry("jury/references/x.md", "# X\n", false),
        ],
    );
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/skills/jury/references/x.md"), "# X\n");
}

#[test]
fn import_turns_a_skill_folder_into_a_file() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", "# Old\n");
    project.write(".ai/src/skills/jury/references/a.md", "# A\n");
    write_zip(
        &project,
        "jury.skill",
        &[
            zip_entry("jury/SKILL.md", JURY, false),
            zip_entry("jury/references", "now a file\n", false),
        ],
    );
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/skills/jury/references"),
        "now a file\n"
    );
}

#[test]
fn import_keeps_local_files_outside_the_imported_skills() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/rules/local.md", "# Local\n");
    project.write(".ai/src/skills/mine/SKILL.md", "# Mine\n");
    project.write("other/.ai/src/rules/other.md", "# Other\n");
    project.write("other/.ai/src/skills/jury/SKILL.md", JURY);
    project
        .exuno()
        .args(["import", "other", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed").not());
    assert!(project.join(".ai/src/rules/local.md").is_file());
    assert!(project.join(".ai/src/skills/mine/SKILL.md").is_file());
    assert!(project.join(".ai/src/skills/jury/SKILL.md").is_file());
}

#[test]
fn import_updates_a_skill_where_the_project_keeps_it() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/judging/jury/SKILL.md", "# Old\n");
    write_zip(
        &project,
        "jury.skill",
        &[zip_entry("jury/SKILL.md", JURY, false)],
    );
    project
        .exuno()
        .args(["import", "jury.skill", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skills (1 updated)"));
    assert_eq!(project.read(".ai/src/skills/judging/jury/SKILL.md"), JURY);
    assert!(!project.join(".ai/src/skills/jury").exists());
}

#[test]
fn import_finds_skills_below_a_wrapping_folder() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "bundle.zip",
        &[
            zip_entry("my-skills/jury/SKILL.md", JURY, false),
            zip_entry("my-skills/jury/.DS_Store", "finder", false),
            zip_entry("__MACOSX/my-skills/jury/._SKILL.md", "fork", false),
        ],
    );
    project
        .exuno()
        .args(["import", "bundle.zip"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skills (1 new)"));
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
    assert!(!project.join(".ai/src/skills/jury/.DS_Store").exists());
}

#[test]
fn import_reads_a_zip_bundle_of_ai_sources() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "config.zip",
        &[zip_entry(".ai/src/rules/zipped.md", "# Zipped\n", false)],
    );
    project
        .exuno()
        .args(["import", "config.zip"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/rules/zipped.md"), "# Zipped\n");
}

#[test]
fn import_reads_an_uncompressed_tar() {
    let project = Project::seeded(&[]);
    project.write("staging/jury/SKILL.md", JURY);
    let status = StdCommand::new("tar")
        .arg("-cf")
        .arg(project.join("jury.tar"))
        .arg("-C")
        .arg(project.join("staging"))
        .arg("jury")
        .status()
        .unwrap();
    assert!(status.success());
    project
        .exuno()
        .args(["import", "jury.tar"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
}

#[test]
fn import_refuses_an_archive_entry_that_escapes() {
    let project = Project::seeded(&[]);
    std::fs::create_dir_all(project.join("inbox")).unwrap();
    write_zip(
        &project,
        "inbox/evil.skill",
        &[
            zip_entry("jury/SKILL.md", JURY, false),
            zip_entry("../escaped.md", "x", false),
        ],
    );
    exuno_in(&project.join("inbox"))
        .args(["import", "evil.skill"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Failed to extract archive: ../escaped.md: path escapes the archive root",
        ));
    assert!(!project.join("escaped.md").exists());
    assert!(!project.join(".ai/src/skills/jury").exists());
}

#[test]
fn import_installs_a_skill_folder() {
    let project = Project::seeded(&[]);
    project.write("incoming/jury/SKILL.md", JURY);
    project.write("incoming/jury/scripts/run.sh", "#!/bin/sh\n");
    project
        .exuno()
        .args(["import", "incoming/jury", "--force"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skills (2 new)"));
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
    assert!(project.join(".ai/src/skills/jury/scripts/run.sh").is_file());
}

#[test]
fn import_installs_the_skill_folders_below_a_directory() {
    let project = Project::seeded(&[]);
    project.write("shelf/judging/jury/SKILL.md", JURY);
    project.write("shelf/notes/SKILL.md", "# Notes\n");
    project.write("shelf/README.md", "not a skill\n");
    project.exuno().args(["import", "shelf"]).assert().success();
    assert_eq!(project.read(".ai/src/skills/jury/SKILL.md"), JURY);
    assert_eq!(project.read(".ai/src/skills/notes/SKILL.md"), "# Notes\n");
    assert!(!project.join(".ai/src/skills/README.md").exists());
}

#[test]
fn import_refuses_two_skills_with_the_same_name() {
    let project = Project::seeded(&[]);
    write_zip(
        &project,
        "twins.zip",
        &[
            zip_entry("a/SKILL.md", JURY, false),
            zip_entry("b/SKILL.md", JURY, false),
        ],
    );
    project
        .exuno()
        .args(["import", "twins.zip"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Two skills in the source are named 'jury': a and b",
        ));
    assert!(!project.join(".ai/src/skills/jury").exists());
}

#[test]
fn import_refuses_an_archive_past_the_size_limit_before_reading_it() {
    let project = Project::seeded(&[]);
    let file = std::fs::File::create(project.join("huge.skill")).unwrap();
    file.set_len((256 << 20) + 1).unwrap();
    project
        .exuno()
        .args(["import", "huge.skill"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Failed to extract archive: larger than 256 MB",
        ));
}

#[test]
fn import_reports_a_corrupt_zip() {
    let project = Project::seeded(&[]);
    project.write("broken.skill", "not a zip\n");
    project
        .exuno()
        .args(["import", "broken.skill"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Failed to extract archive: not a ZIP archive",
        ));
}

#[test]
fn export_skill_packages_one_skill_as_a_skill_archive() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/judging/jury/SKILL.md", JURY);
    project.write(".ai/src/skills/judging/jury/references/rubric.md", "# R\n");
    project
        .exuno()
        .args(["export", "--skill", "jury"])
        .assert()
        .success()
        .stdout(predicate::str::contains("• jury/ (2 files)"))
        .stdout(predicate::str::contains("Exported! → "));
    let entries = exuno::zip::read(&std::fs::read(project.join("jury.skill")).unwrap()).unwrap();
    let paths: Vec<&str> = entries.iter().map(|entry| entry.path.as_str()).collect();
    assert_eq!(paths, ["jury/SKILL.md", "jury/references/rubric.md"]);
    assert_eq!(entries[0].data, JURY.as_bytes());
}

#[test]
fn export_skill_dry_run_writes_nothing() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", JURY);
    project
        .exuno()
        .args(["export", "--skill", "jury", "--dry-run", "-o", "out.skill"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would create: out.skill"));
    assert!(!project.join("out.skill").exists());
}

#[test]
fn an_exported_skill_imports_into_another_project() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", JURY);
    project.write(".ai/src/skills/jury/scripts/run.sh", "#!/bin/sh\n");
    project
        .exuno()
        .args(["export", "--skill", "jury"])
        .assert()
        .success();
    let other = Project::seeded(&[]);
    other
        .exuno()
        .args([
            "import",
            &project.join("jury.skill").to_string_lossy(),
            "--force",
        ])
        .assert()
        .success();
    assert_eq!(other.read(".ai/src/skills/jury/SKILL.md"), JURY);
    assert_eq!(
        other.read(".ai/src/skills/jury/scripts/run.sh"),
        "#!/bin/sh\n"
    );
}

#[test]
fn export_skill_refuses_an_unknown_skill() {
    Project::seeded(&[])
        .exuno()
        .args(["export", "--skill", "nope"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Skill not found: nope"));
}

#[test]
fn export_skill_refuses_frontmatter_the_upload_would_reject() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/jury/SKILL.md", "---\nname: jury\n---\n");
    project
        .exuno()
        .args(["export", "--skill", "jury"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            ".ai/src/skills/jury/SKILL.md: description must be 1–1024 characters",
        ));
    assert!(!project.join("jury.skill").exists());
}
