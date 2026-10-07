//! `tests/update_native.bats`: `exuno update` on a binary install — the
//! binary run directly, as the installer links it, against a curl stand-in on
//! `PATH` that serves a fixture release and a real tar archive.
//!
//! The curl stand-in is a shell script the binary cannot spawn on Windows, so
//! this whole file is `#[cfg(unix)]`.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;
use common::Project;
use predicates::prelude::*;

/// Copy the binary under test to `dst` and wait until it can be executed.
///
/// `cargo test` runs these tests in parallel, and Linux refuses to exec a file
/// that another thread still holds open for writing: a `fs::copy` handle here
/// is inherited by a process another test spawns, and the exec fails with
/// `ETXTBSY` until that child exits. Retrying the spawn is the fix.
fn install_binary(src: &Path, dst: &Path) {
    std::fs::copy(src, dst).unwrap();
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(0o755)).unwrap();
    for _ in 0..200 {
        match StdCommand::new(dst).arg("version").output() {
            Ok(_) => return,
            Err(e) if e.kind() == std::io::ErrorKind::ExecutableFileBusy => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            Err(e) => panic!("{}: {e}", dst.display()),
        }
    }
    panic!("{} stayed busy", dst.display());
}

fn engine_version() -> String {
    include_str!("../VERSION").trim().to_string()
}

/// A fixture install: a copy of the binary under test at `install/bin/agentsync`,
/// separate from the one `cargo test` built, so `update` can replace it.
struct Install {
    project: Project,
    native_bin: PathBuf,
    install_bin: PathBuf,
    fake_releases: PathBuf,
    rel_root: PathBuf,
}

impl Install {
    fn new() -> Self {
        let project = Project::seeded(&[]);
        let native_bin = PathBuf::from(env!("CARGO_BIN_EXE_exuno"));
        let install_dir = project.join("install/bin");
        std::fs::create_dir_all(&install_dir).unwrap();
        let install_bin = install_dir.join("agentsync");
        install_binary(&native_bin, &install_bin);
        std::fs::write(project.join("install/.update_cache"), "9.9.9\n").unwrap();

        let fake_releases = project.join("releases");
        std::fs::create_dir_all(fake_releases.join("tags")).unwrap();
        let stub_dir = project.join("stub");
        std::fs::create_dir_all(&stub_dir).unwrap();
        let script = r#"#!/usr/bin/env bash
out=""; url=""
while [ $# -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift 2 ;;
        -w|--max-time) shift 2 ;;
        -*) shift ;;
        *) url="$1"; shift ;;
    esac
done
case "$url" in
    https://api.github.com/repos/yelmuratoff/exuno/releases/latest)
        file="$FAKE_RELEASES/latest.json" ;;
    https://api.github.com/repos/yelmuratoff/exuno/git/ref/tags/*)
        file="$FAKE_RELEASES/tags/${url##*/}" ;;
    https://github.com/yelmuratoff/exuno/releases/download/*)
        rest="${url#*/releases/download/}"; tag="${rest%%/*}"; name="${rest#*/}"
        case "$name" in
            exuno-*.sha256) name="archive.tar.xz.sha256" ;;
            exuno-*) name="archive.tar.xz" ;;
        esac
        file="$FAKE_RELEASES/$tag/$name" ;;
    *) printf '000'; exit 6 ;;
esac
if [ -f "$file" ]; then cp "$file" "$out"; printf '200'; else printf '404'; fi
"#;
        let curl_path = stub_dir.join("curl");
        std::fs::write(&curl_path, script).unwrap();
        std::fs::set_permissions(&curl_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        let rel_root = project.join("rel-root");
        std::fs::create_dir_all(&rel_root).unwrap();

        Self {
            project,
            native_bin,
            install_bin,
            fake_releases,
            rel_root,
        }
    }

    fn path(&self) -> String {
        format!(
            "{}:{}",
            self.project.join("stub").display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    /// The installed binary, run against the curl stand-in.
    fn exuno(&self) -> Command {
        let mut command = Command::new(&self.install_bin);
        command.current_dir(self.project.path());
        common::scrub(&mut command);
        command
            .env("PATH", self.path())
            .env("FAKE_RELEASES", &self.fake_releases);
        command
    }

    fn version(&self) -> String {
        let output = StdCommand::new(&self.install_bin)
            .arg("version")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn installed_unchanged(&self) -> bool {
        std::fs::read(&self.install_bin).unwrap() == std::fs::read(&self.native_bin).unwrap()
    }

    fn catalog_dump(&self) -> String {
        let output = StdCommand::new(&self.native_bin)
            .arg("__catalog")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).into_owned()
    }

    /// A fixture release: an archive laid out as cargo-dist lays it out, its
    /// binary a script answering `version` and `__catalog`, its catalog the
    /// running binary's unless a dump is given.
    fn publish_release(&self, tag: &str, dump: Option<&str>) {
        let top = self
            .rel_root
            .join(format!("rel-{tag}"))
            .join("exuno-fixture");
        std::fs::create_dir_all(&top).unwrap();
        let release_dir = self.fake_releases.join(tag);
        std::fs::create_dir_all(&release_dir).unwrap();

        let catalog = dump
            .map(str::to_string)
            .unwrap_or_else(|| self.catalog_dump());
        std::fs::write(top.join("catalog.dump"), &catalog).unwrap();

        let script = format!(
            "#!/usr/bin/env bash\ncase \"${{1:-}}\" in\n    version) echo \"exuno v{tag}\" ;;\n    __catalog) cat \"$(dirname \"$0\")/catalog.dump\" ;;\n    *) exit 1 ;;\nesac\n"
        );
        let bin_path = top.join("exuno");
        std::fs::write(&bin_path, script).unwrap();
        std::fs::set_permissions(&bin_path, std::fs::Permissions::from_mode(0o755)).unwrap();

        std::fs::write(
            top.join("CHANGELOG.md"),
            format!(
                "# Changelog\n\n## {tag}\n\n### Fixed\n\n- **Something** with `code`.\n\n## 0.1.0\n\n- Ancient.\n"
            ),
        )
        .unwrap();

        let archive = release_dir.join("archive.tar.xz");
        let status = StdCommand::new("tar")
            .arg("-cJf")
            .arg(&archive)
            .arg("-C")
            .arg(self.rel_root.join(format!("rel-{tag}")))
            .arg("exuno-fixture")
            .status()
            .unwrap();
        assert!(status.success());
        let sum = exuno::transaction::manifest::sha256_hex(&std::fs::read(&archive).unwrap());
        std::fs::write(
            release_dir.join("archive.tar.xz.sha256"),
            format!("{sum}  archive.tar.xz\n"),
        )
        .unwrap();
    }

    fn write_latest_json(&self, tag: &str) {
        std::fs::write(
            self.fake_releases.join("latest.json"),
            format!("{{\"tag_name\":\"{tag}\"}}\n"),
        )
        .unwrap();
    }
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

#[test]
fn help_prints_the_usage_without_touching_the_network() {
    let install = Install::new();
    install
        .exuno()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--strict"))
        .stdout(predicate::str::contains("the latest release"));
}

#[test]
fn an_unknown_flag_is_refused_with_status_2() {
    let install = Install::new();
    install
        .exuno()
        .args(["update", "--bogus"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("Unknown flag: --bogus"));
}

#[test]
fn the_latest_release_replaces_the_binary_and_prints_its_changelog() {
    let install = Install::new();
    install.publish_release("9.9.9", None);
    install.write_latest_json("9.9.9");
    let engine_version = engine_version();
    install
        .exuno()
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains("Updating..."))
        .stdout(predicate::str::contains(format!(
            "Updated! v{engine_version} → v9.9.9"
        )))
        .stdout(predicate::str::contains("What's new in v9.9.9"))
        .stdout(predicate::str::contains("• Something with code."))
        .stdout(predicate::str::contains("Upstream touched").not());
    assert_eq!(install.version(), "exuno v9.9.9");
    assert!(!install.project.join("install/.update_cache").exists());
    assert!(
        !install
            .project
            .join(".ai/.pending-resolutions.yaml")
            .exists()
    );
}

#[test]
fn the_running_version_is_already_up_to_date() {
    let install = Install::new();
    let engine_version = engine_version();
    install.write_latest_json(&engine_version);
    install
        .exuno()
        .arg("update")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "Already up to date! (v{engine_version})"
        )));
    assert!(install.installed_unchanged());
    assert!(install.project.join("install/.update_cache").exists());
}

#[test]
fn version_pins_to_that_release() {
    let install = Install::new();
    install.publish_release("9.9.9", None);
    install
        .exuno()
        .args(["update", "9.9.9"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Pinning to v9.9.9..."));
    assert_eq!(install.version(), "exuno v9.9.9");
}

#[test]
fn a_tag_that_is_not_a_release_is_refused() {
    let install = Install::new();
    install
        .exuno()
        .args(["update", "999.0.0"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "No Exuno release is tagged 999.0.0.",
        ));
    assert!(install.installed_unchanged());
}

#[test]
fn a_tag_older_than_the_binary_releases_points_at_the_installer() {
    let install = Install::new();
    write(
        &install.fake_releases.join("tags/0.1.0"),
        "{\"ref\":\"refs/tags/0.1.0\"}\n",
    );
    install
        .exuno()
        .args(["update", "0.1.0"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("0.1.0 predates the binary releases"))
        .stderr(predicate::str::contains(
            "AGENTSYNC_VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash",
        ));
    assert!(install.installed_unchanged());
}

#[test]
fn a_checksum_mismatch_keeps_the_old_binary() {
    let install = Install::new();
    install.publish_release("9.9.9", None);
    write(
        &install.fake_releases.join("9.9.9/archive.tar.xz.sha256"),
        "0000  archive.tar.xz\n",
    );
    install
        .exuno()
        .args(["update", "9.9.9"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("checksum mismatch"));
    assert!(install.installed_unchanged());
}

#[test]
fn an_unreachable_github_is_reported() {
    let install = Install::new();
    let empty_path = install.project.join("nobin");
    std::fs::create_dir_all(&empty_path).unwrap();
    install
        .exuno()
        .env("PATH", &empty_path)
        .args(["update", "9.9.9"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Failed to fetch updates from GitHub.",
        ));
    assert!(install.installed_unchanged());
}

#[test]
fn a_changed_overridden_field_is_reported_queued_and_fails_strict() {
    let install = Install::new();
    let catalog = install.catalog_dump();
    assert!(catalog.lines().any(|l| l.starts_with("claude ")));
    // The dump frames each YAML by byte length; a value of the same length
    // keeps the frame intact.
    let changed = catalog.replacen("dest: \".claude/rules\"", "dest: \".claude/rulez\"", 1);
    assert_ne!(catalog, changed);
    install.publish_release("9.9.9", Some(&changed));
    write(
        &install.project.join(".ai/src/tools/claude.yaml"),
        "targets:\n  rules:\n    dest: \".claude/my-rules\"\n",
    );

    let engine_version = engine_version();
    install
        .exuno()
        .args(["update", "9.9.9"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Upstream touched fields you have overridden:",
        ))
        .stdout(predicate::str::contains("◆ targets.rules.dest"))
        .stdout(predicate::str::contains(
            "base: .claude/rules → .claude/rulez",
        ))
        .stdout(predicate::str::contains("your override: .claude/my-rules"))
        .stdout(predicate::str::contains(
            "Queued in .ai/.pending-resolutions.yaml",
        ));
    let queue = install.project.read(".ai/.pending-resolutions.yaml");
    assert!(queue.contains(&format!("from_version: \"{engine_version}\"")));
    assert!(queue.contains("to_version: \"9.9.9\""));
    assert!(queue.contains("your_override: \".claude/my-rules\""));

    install_binary(&install.native_bin, &install.install_bin);
    install
        .exuno()
        .args(["update", "9.9.9", "--strict"])
        .assert()
        .code(1);
    assert_eq!(install.version(), "exuno v9.9.9");
}

#[test]
fn catalog_frames_every_shipped_tool_by_byte_length() {
    let install = Install::new();
    let assert = install.exuno().arg("__catalog").assert().success();
    let output = assert.get_output();
    let stdout = String::from_utf8_lossy(&output.stdout);
    let framed = stdout
        .lines()
        .filter(|l| {
            l.split_once(' ')
                .is_some_and(|(slug, len)| !slug.is_empty() && len.parse::<u32>().is_ok())
        })
        .count();
    assert!(
        framed >= 13,
        "expected at least 13 framed entries, got {framed}"
    );
    assert!(stdout.lines().any(|l| l.starts_with("claude ")));
}
