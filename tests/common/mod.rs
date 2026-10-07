//! The fixtures `tests/test_helper.bash` gave the bats suite, for the Rust
//! integration tests: a throwaway git project, the binary with the developer's
//! environment scrubbed, and file helpers the assertions need.

#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::process::Command as StdCommand;

use assert_cmd::Command;

/// A temporary project: `mktemp -d` plus `git init` with a test identity.
/// Removed with the value.
pub struct Project {
    dir: tempfile::TempDir,
}

impl Project {
    /// `setup_test_project`: an empty repository.
    pub fn empty() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let project = Self { dir };
        project.git(&["init", "--quiet"]);
        project.git(&["config", "user.email", "test@test.com"]);
        project.git(&["config", "user.name", "Test"]);
        project
    }

    /// `seed_project`: an empty repository after `exuno init <args>`.
    pub fn seeded(init_args: &[&str]) -> Self {
        let project = Self::empty();
        project
            .exuno()
            .arg("init")
            .args(init_args)
            .assert()
            .success();
        project
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub fn join(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    /// The binary under test in this project, with the shell variables that
    /// would leak trust or the developer's install removed, and git told to
    /// read no global or system config.
    pub fn exuno(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
        command.current_dir(self.path());
        scrub(&mut command);
        command
    }

    pub fn git(&self, args: &[&str]) {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(self.path())
            .env("GIT_CONFIG_GLOBAL", absent_git_config())
            .env("GIT_CONFIG_SYSTEM", absent_git_config())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    pub fn write(&self, rel: &str, content: &str) {
        let path = self.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    pub fn append(&self, rel: &str, content: &str) {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(self.join(rel))
            .unwrap();
        file.write_all(content.as_bytes()).unwrap();
    }

    pub fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.join(rel)).unwrap()
    }

    pub fn exists(&self, rel: &str) -> bool {
        self.join(rel).exists()
    }

    /// `file_sha256`: the hex digest of one file.
    pub fn sha256(&self, rel: &str) -> String {
        exuno::transaction::manifest::sha256_hex(&std::fs::read(self.join(rel)).unwrap())
    }

    /// `enable_tools`: enable without scaffolding, so a test starts from the
    /// same tree whatever `enable` scaffolds later.
    pub fn enable_tools(&self, tools: &[&str]) {
        self.exuno()
            .arg("enable")
            .args(tools)
            .arg("--no-scaffold")
            .assert()
            .success();
    }
}

/// The variables `tests/test_helper.bash` unsets, and the config paths it
/// points at a file that does not exist.
pub fn scrub(command: &mut Command) {
    for suffix in [
        "ALLOW_POST_SYNC",
        "SKIP_POST_SYNC",
        "SKIP_HOOKS",
        "EXTERNAL_SOURCE_ROOTS",
        "HOME",
    ] {
        for prefix in ["EXUNO_", "AGENTSYNC_"] {
            command.env_remove(format!("{prefix}{suffix}"));
        }
    }
    command
        .env("GIT_CONFIG_GLOBAL", absent_git_config())
        .env("GIT_CONFIG_SYSTEM", absent_git_config());
}

fn absent_git_config() -> PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

/// A path spelled the way the engine spells it: `/`-separated even on
/// Windows, where `Path::display` would print backslashes. An expectation
/// about a path the engine prints, or a path written into a config the engine
/// reads, goes through this — `tests/test_helper.bash` used `cygpath -ml` for
/// the same reason.
pub fn engine_path(path: &Path) -> String {
    use exuno::paths::DiskText;
    path.disk_text()
}

/// A path the engine can compare against one it canonicalised itself: the
/// symlinks resolved and, on Windows, the 8.3 short name of the runner's
/// `TEMP` (`RUNNER~1`) spelled out in full. A trust root like
/// `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` goes through this, as the bats helper's
/// `cygpath -ml` did.
pub fn canonical_engine_path(path: &Path) -> String {
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    engine_path(&canonical)
}

/// A list for a path-list variable, joined the way the engine splits it:
/// `;` on Windows, where a `C:` would otherwise split at its colon.
pub fn path_list(parts: &[&str]) -> String {
    parts.join(if cfg!(windows) { ";" } else { ":" })
}

/// Whether the platform honours `chmod 000` for this user: not root, not
/// Windows, where Git Bash ignores permission bits.
pub fn unreadable_dirs_are_possible() -> bool {
    if !cfg!(unix) {
        return false;
    }
    let output = StdCommand::new("id").arg("-u").output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim() != "0"
}

#[cfg(unix)]
pub fn chmod(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}
