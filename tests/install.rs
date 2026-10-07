//! `tests/install.bats`: `install.sh` against a purpose-built origin
//! repository and a curl stand-in that serves fixture binary releases. Every
//! path the installer touches is redirected into the test project so the
//! developer's `~/.agentsync`, `PATH` symlink, and shell rc are never
//! modified.
//!
//! The origin is a fixture, not this repository, so the tests stay
//! independent of this repository's tag history. The binary release is a
//! fixture too: an archive laid out as cargo-dist lays it out, its binary a
//! script.
//!
//! The curl stand-in is a shell script Windows cannot run as curl, so this
//! whole file is `#[cfg(unix)]`.
#![cfg(unix)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command as StdCommand, Output};

use common::Project;

const FIXTURE_NEW: &str = "9.9.2";
const FIXTURE_OLD: &str = "9.9.1";
const FIXTURE_ABSENT: &str = "999.0.0";

fn absent_git_config() -> PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

fn git(dir: &Path, args: &[&str]) -> Output {
    StdCommand::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", absent_git_config())
        .env("GIT_CONFIG_SYSTEM", absent_git_config())
        .output()
        .unwrap()
}

fn git_ok(dir: &Path, args: &[&str]) {
    let output = git(dir, args);
    assert!(output.status.success(), "git {args:?} failed: {output:?}");
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn make_executable(path: &Path) {
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// `_build_origin_fixture`: a bare source checkout the installer clones,
/// tagged before and after the (fictional) first binary release.
fn build_origin_fixture(origin: &Path) {
    std::fs::create_dir_all(origin).unwrap();
    git_ok(origin, &["init", "--quiet"]);
    git_ok(origin, &["config", "user.email", "test@test.com"]);
    git_ok(origin, &["config", "user.name", "Test"]);

    write(
        &origin.join("bin/agentsync.sh"),
        "#!/usr/bin/env bash\necho \"agentsync v$(cat \"$(dirname \"$0\")/../VERSION\")\"\n",
    );
    make_executable(&origin.join("bin/agentsync.sh"));
    write(&origin.join("lib/helpers/.keep"), "");
    write(&origin.join("VERSION"), &format!("{FIXTURE_OLD}\n"));
    git_ok(origin, &["add", "-A"]);
    git_ok(
        origin,
        &[
            "commit",
            "--quiet",
            "-m",
            &format!("fixture engine {FIXTURE_OLD}"),
        ],
    );
    git_ok(origin, &["tag", FIXTURE_OLD]);
    git_ok(origin, &["branch", "-M", "main"]);

    write(&origin.join("VERSION"), &format!("{FIXTURE_NEW}\n"));
    git_ok(
        origin,
        &[
            "commit",
            "--quiet",
            "-am",
            &format!("fixture engine {FIXTURE_NEW}"),
        ],
    );
    git_ok(origin, &["tag", FIXTURE_NEW]);
}

/// A `curl` on `PATH` that serves `$FAKE_RELEASES` for the URLs the installer
/// builds and prints the HTTP status as `curl -w %{http_code}` does. Nothing
/// here reaches the network: a tag without a fixture release is a 404.
fn install_curl_stub(stub_dir: &Path) {
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
            exuno-*.sha256) name="exuno.tar.xz.sha256" ;;
            exuno-*) name="exuno.tar.xz" ;;
            agentsync-*.sha256) name="agentsync.tar.xz.sha256" ;;
            agentsync-*) name="agentsync.tar.xz" ;;
        esac
        file="$FAKE_RELEASES/$tag/$name" ;;
    *) printf '000'; exit 6 ;;
esac
if [ -f "$file" ]; then cp "$file" "$out"; printf '200'; else printf '404'; fi
"#;
    std::fs::create_dir_all(stub_dir).unwrap();
    let curl_path = stub_dir.join("curl");
    write(&curl_path, script);
    make_executable(&curl_path);
}

/// A fixture binary release for `tag` published as `name`: the archive holds
/// a `name` script that answers `version` with the tag, plus a
/// `CHANGELOG.md`, under one directory.
fn publish_release(fake_releases: &Path, rel_root: &Path, tag: &str, name: &str) {
    let staged = rel_root.join(format!("rel-{tag}-{name}"));
    let top = staged.join(format!("{name}-fixture"));
    std::fs::create_dir_all(&top).unwrap();
    let release_dir = fake_releases.join(tag);
    std::fs::create_dir_all(&release_dir).unwrap();

    write(
        &top.join(name),
        &format!(
            "#!/usr/bin/env bash\ncase \"${{1:-}}\" in\n    version) echo \"{name} v{tag}\" ;;\n    *) exit 1 ;;\nesac\n"
        ),
    );
    make_executable(&top.join(name));
    write(
        &top.join("CHANGELOG.md"),
        &format!("# Changelog\n\n## {tag}\n\n- Fixture.\n"),
    );

    let archive = release_dir.join(format!("{name}.tar.xz"));
    let status = StdCommand::new("tar")
        .arg("-cJf")
        .arg(&archive)
        .arg("-C")
        .arg(&staged)
        .arg(format!("{name}-fixture"))
        .status()
        .unwrap();
    assert!(status.success());
    let sum = exuno::transaction::manifest::sha256_hex(&std::fs::read(&archive).unwrap());
    write(
        &release_dir.join(format!("{name}.tar.xz.sha256")),
        &format!("{sum}  {name}.tar.xz\n"),
    );
}

struct Fixture {
    _project: Project,
    home: PathBuf,
    origin: PathBuf,
    fake_releases: PathBuf,
    rel_root: PathBuf,
    stub_dir: PathBuf,
    bin_dir: PathBuf,
    install_dir: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let project = Project::empty();
        let home = project.join("home");
        std::fs::create_dir_all(&home).unwrap();
        write(&home.join(".zshrc"), "");

        let origin = project.join("origin");
        build_origin_fixture(&origin);

        let fake_releases = project.join("releases");
        std::fs::create_dir_all(fake_releases.join("tags")).unwrap();
        let stub_dir = project.join("stub");
        install_curl_stub(&stub_dir);

        Self {
            bin_dir: project.join("bin"),
            install_dir: project.join("engine"),
            rel_root: project.join("rel-root"),
            fake_releases,
            origin,
            stub_dir,
            home,
            _project: project,
        }
    }

    fn path(&self) -> String {
        format!(
            "{}:{}:{}",
            self.bin_dir.display(),
            self.stub_dir.display(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn publish(&self, tag: &str) {
        publish_release(&self.fake_releases, &self.rel_root, tag, "exuno");
    }

    /// A release from before the rename, published only as `agentsync-<target>`.
    fn publish_legacy(&self, tag: &str) {
        publish_release(&self.fake_releases, &self.rel_root, tag, "agentsync");
    }

    fn write_latest_json(&self, tag: &str) {
        write(
            &self.fake_releases.join("latest.json"),
            &format!("{{\"tag_name\":\"{tag}\"}}\n"),
        );
    }

    /// `bash install.sh`, `EXUNO_VERSION` optionally pinned.
    fn run_install(&self, version: Option<&str>) -> Output {
        match version {
            Some(version) => self.run_install_with(&[("EXUNO_VERSION", version)]),
            None => self.run_install_with(&[]),
        }
    }

    /// `bash install.sh` with `vars` set on top of the fixture's paths.
    fn run_install_with(&self, vars: &[(&str, &str)]) -> Output {
        let mut command = StdCommand::new("bash");
        command
            .arg(concat!(env!("CARGO_MANIFEST_DIR"), "/install.sh"))
            .env("HOME", &self.home)
            .env("EXUNO_REPO_URL", &self.origin)
            .env("EXUNO_INSTALL_DIR", &self.install_dir)
            .env("EXUNO_BIN_DIR", &self.bin_dir)
            .env("PATH", self.path())
            .env("FAKE_RELEASES", &self.fake_releases)
            .env("GIT_CONFIG_GLOBAL", absent_git_config())
            .env("GIT_CONFIG_SYSTEM", absent_git_config());
        for suffix in ["VERSION", "REPO_URL", "INSTALL_DIR", "BIN_DIR"] {
            command.env_remove(format!("AGENTSYNC_{suffix}"));
        }
        command.env_remove("EXUNO_VERSION");
        for (key, value) in vars {
            command.env(key, value);
        }
        command.output().unwrap()
    }

    /// What the `name` link on the fixture's PATH answers to `version`.
    fn link_version(&self, name: &str) -> String {
        let output = StdCommand::new(self.bin_dir.join(name))
            .arg("version")
            .output()
            .unwrap();
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn bin_link_version(&self) -> String {
        self.link_version("exuno")
    }
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn the_fixture_origin_publishes_the_tags_these_tests_pin_to() {
    let fx = Fixture::new();
    let tags = git(&fx.origin, &["tag"]);
    assert!(tags.status.success());
    let listing = stdout(&tags);
    assert!(listing.contains(FIXTURE_OLD));
    assert!(listing.contains(FIXTURE_NEW));
    assert!(!listing.contains(FIXTURE_ABSENT));
    let head = git(
        &fx.origin,
        &["rev-parse", "--verify", "--quiet", "refs/heads/main"],
    );
    assert!(head.status.success());
}

#[test]
fn the_latest_binary_release_is_downloaded_verified_and_linked() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    fx.write_latest_json(FIXTURE_NEW);
    let output = fx.run_install(None);
    assert!(output.status.success(), "{}", stderr(&output));
    let out = stdout(&output);
    assert!(out.contains("Installed successfully!"));
    assert!(out.contains(&format!("v{FIXTURE_NEW}")));
    assert!(fx.install_dir.join("bin/exuno").is_file());
    assert!(fx.bin_dir.join("exuno").exists());
    assert_eq!(fx.bin_link_version(), format!("exuno v{FIXTURE_NEW}"));
    assert!(!fx.install_dir.join(".git").exists());
    let zshrc = std::fs::read_to_string(fx.home.join(".zshrc")).unwrap();
    assert!(!zshrc.contains("AGENTSYNC_HOME"));
}

#[test]
fn exuno_version_pins_a_binary_release() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fx.bin_link_version(), format!("exuno v{FIXTURE_NEW}"));
    assert!(!fx.install_dir.join(".git").exists());
}

#[test]
fn a_checksum_mismatch_aborts_before_anything_is_linked() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    write(
        &fx.fake_releases
            .join(format!("{FIXTURE_NEW}/exuno.tar.xz.sha256")),
        "0000  exuno.tar.xz\n",
    );
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("checksum mismatch"));
    assert!(!fx.install_dir.join("bin/exuno").exists());
    assert!(!fx.bin_dir.join("exuno").exists());
    assert!(!fx.bin_dir.join("agentsync").exists());
}

#[test]
fn without_a_pin_an_unreachable_github_fails_clearly() {
    let fx = Fixture::new();
    let output = fx.run_install(None);
    assert_eq!(output.status.code(), Some(1));
    assert!(stderr(&output).contains("could not read the latest release"));
    assert!(!fx.bin_dir.join("exuno").exists());
}

#[test]
fn exuno_version_pins_a_tag_without_a_binary_from_source() {
    let fx = Fixture::new();
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(stdout(&output).contains("predates the binary releases"));
    assert_eq!(
        std::fs::read_to_string(fx.install_dir.join("VERSION")).unwrap(),
        format!("{FIXTURE_NEW}\n")
    );
    assert!(fx.bin_dir.join("exuno").exists());
    let zshrc = std::fs::read_to_string(fx.home.join(".zshrc")).unwrap();
    assert!(zshrc.contains("AGENTSYNC_HOME"));
}

#[test]
fn an_unknown_exuno_version_fails_clearly() {
    let fx = Fixture::new();
    let output = fx.run_install(Some(FIXTURE_ABSENT));
    assert!(!output.status.success());
    assert!(stderr(&output).contains(FIXTURE_ABSENT));
}

#[test]
fn re_running_with_a_different_pin_moves_an_existing_source_install() {
    let fx = Fixture::new();
    let first = fx.run_install(Some(FIXTURE_NEW));
    assert!(first.status.success(), "{}", stderr(&first));
    let second = fx.run_install(Some(FIXTURE_OLD));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(
        std::fs::read_to_string(fx.install_dir.join("VERSION")).unwrap(),
        format!("{FIXTURE_OLD}\n")
    );
}

#[test]
fn a_binary_release_replaces_an_existing_source_installs_link() {
    let fx = Fixture::new();
    let first = fx.run_install(Some(FIXTURE_OLD));
    assert!(first.status.success(), "{}", stderr(&first));
    fx.publish(FIXTURE_NEW);
    let second = fx.run_install(Some(FIXTURE_NEW));
    assert!(second.status.success(), "{}", stderr(&second));
    assert_eq!(fx.bin_link_version(), format!("exuno v{FIXTURE_NEW}"));
    assert!(fx.install_dir.join(".git").exists());
}

#[test]
fn both_links_reach_the_installed_binary_until_1_0() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fx.link_version("exuno"), format!("exuno v{FIXTURE_NEW}"));
    assert_eq!(
        fx.link_version("agentsync"),
        format!("exuno v{FIXTURE_NEW}")
    );
    assert!(stdout(&output).contains(&format!(
        "rm -f {} {}",
        fx.bin_dir.join("exuno").display(),
        fx.bin_dir.join("agentsync").display()
    )));
}

#[test]
fn the_legacy_agentsync_version_variable_still_pins() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    let output = fx.run_install_with(&[("AGENTSYNC_VERSION", FIXTURE_NEW)]);
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(fx.bin_link_version(), format!("exuno v{FIXTURE_NEW}"));
}

#[test]
fn a_pin_before_the_rename_installs_the_agentsync_archive_as_exuno() {
    let fx = Fixture::new();
    fx.publish_legacy(FIXTURE_NEW);
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert!(output.status.success(), "{}", stderr(&output));
    assert!(fx.install_dir.join("bin/exuno").is_file());
    assert!(!fx.install_dir.join(".git").exists());
    assert_eq!(
        fx.link_version("agentsync"),
        format!("agentsync v{FIXTURE_NEW}")
    );
}

#[test]
fn an_agentsync_install_dir_hands_over_its_update_cache_and_is_named() {
    let fx = Fixture::new();
    fx.publish(FIXTURE_NEW);
    let legacy = fx.home.join(".agentsync");
    write(&legacy.join(".update_cache"), "9.9.2\n");
    write(&legacy.join("bin/agentsync"), "old binary\n");
    let output = fx.run_install(Some(FIXTURE_NEW));
    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(
        std::fs::read_to_string(fx.install_dir.join(".update_cache")).unwrap(),
        "9.9.2\n"
    );
    assert!(legacy.join("bin/agentsync").is_file());
    assert!(stdout(&output).contains(&format!("rm -rf {}", legacy.display())));
}
