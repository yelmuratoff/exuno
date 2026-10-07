//! `exuno release`: `lib/helpers/release.sh`, which bumps `VERSION`,
//! `Cargo.toml`, and `Cargo.lock` together, commits, tags with the changelog
//! section of the new version, and pushes `main` and the tag unless
//! `--no-push`. Git runs as the executable, its own output passing through.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use super::put;
use crate::Error;
use crate::output::help::{Help, Section};
use crate::output::style::Style;

pub const HELP: Help = Help {
    command: "release",
    tagline: "bump the version, commit, tag, and push",
    synopsis: &["release [major|minor|patch] [--no-push]"],
    description: &[
        "Bumps VERSION, Cargo.toml, and Cargo.lock together, commits, tags with\nthe changelog section of the new version, and pushes main and the tag.\nRuns from the Exuno repository checkout, or from AGENTSYNC_HOME when\nthat holds one.",
    ],
    sections: &[
        Section {
            title: "ARGUMENTS",
            entries: &[
                ("major", "Bump the major version (x+1.0.0)"),
                ("minor", "Bump the minor version (x.y+1.0)"),
                ("patch", "Bump the patch version (x.y.z+1); the default"),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--no-push",
                    "Commit and tag locally without pushing to origin",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
    ],
    examples: &["release", "release minor", "release major --no-push"],
};

const CRATE_FILES: [&str; 2] = ["Cargo.toml", "Cargo.lock"];
const NAME_LINE: &str = "name = \"exuno\"";

/// What `release` takes from the process.
pub struct Env<'a> {
    /// The working directory, tried first as the checkout.
    pub cwd: String,
    /// `resolve_install_dir`: `AGENTSYNC_HOME` when it holds a `.git`.
    pub install_dir: Option<String>,
    /// The next line of stdin without its newline; `None` at end of input.
    pub read_line: &'a mut dyn FnMut() -> Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Bump {
    Major,
    Minor,
    Patch,
}

impl Bump {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "major" => Some(Self::Major),
            "minor" => Some(Self::Minor),
            "patch" => Some(Self::Patch),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Major => "major",
            Self::Minor => "minor",
            Self::Patch => "patch",
        }
    }

    fn apply(self, (major, minor, patch): (u64, u64, u64)) -> (u64, u64, u64) {
        match self {
            Self::Major => (major + 1, 0, 0),
            Self::Minor => (major, minor + 1, 0),
            Self::Patch => (major, minor, patch + 1),
        }
    }
}

/// `read -r current_version < VERSION`: the first line, IFS whitespace trimmed.
fn current_version(text: &str) -> &str {
    text.lines().next().unwrap_or("").trim_matches([' ', '\t'])
}

/// `IFS='.' read -r major minor patch`, the three parts read as decimal
/// integers (design spec, "Accepted deviations", Phase 5b).
fn parse_version(version: &str) -> Option<(u64, u64, u64)> {
    let mut parts = version.split('.');
    let mut next = || parts.next().and_then(|part| part.parse::<u64>().ok());
    let parsed = (next()?, next()?, next()?);
    parts.next().is_none().then_some(parsed)
}

/// The records awk reads: every line, a final one without a newline included.
fn records(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return Vec::new();
    }
    text.strip_suffix('\n')
        .unwrap_or(text)
        .split('\n')
        .collect()
}

/// `_release_crate_version`: the first `version = "…"` line after
/// `name = "exuno"`, which names the crate in Cargo.toml and its entry in
/// Cargo.lock alike.
pub fn crate_version(text: &str) -> Option<String> {
    let mut hit = false;
    for line in records(text) {
        if line == NAME_LINE {
            hit = true;
            continue;
        }
        if hit && line.starts_with("version = \"") {
            let value = &line["version = \"".len()..];
            return Some(value.strip_suffix('"').unwrap_or(value).to_string());
        }
    }
    None
}

/// `_release_set_crate_version`: that line rewritten, every record printed
/// with a newline as awk prints it.
pub fn set_crate_version(text: &str, new_version: &str) -> String {
    let mut hit = false;
    let mut out = String::with_capacity(text.len());
    for line in records(text) {
        if line == NAME_LINE {
            hit = true;
        }
        if hit && line.starts_with("version = \"") {
            out.push_str(&format!("version = \"{new_version}\"\n"));
            hit = false;
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out
}

/// The `## <version>` section of `CHANGELOG.md`: the lines after the heading
/// up to the next `## `, trailing newlines dropped as `$(...)` drops them.
pub fn changelog_section(changelog: &str, version: &str) -> String {
    let heading = format!("## {version}");
    let mut found = false;
    let mut body = String::new();
    for line in records(changelog) {
        if line == heading {
            found = true;
            continue;
        }
        if !found {
            continue;
        }
        if line.starts_with("## ") {
            break;
        }
        body.push_str(line);
        body.push('\n');
    }
    body.trim_end_matches('\n').to_string()
}

/// `printf 'v%s\n\n%s'`.
fn tag_message(version: &str, body: &str) -> String {
    format!("v{version}\n\n{body}")
}

/// The status the shell reports for a child: its exit code, or 128 plus the
/// signal that ended it.
fn shell_status(status: ExitStatus) -> u8 {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return 128u8.wrapping_add(signal as u8);
        }
    }
    status.code().unwrap_or(1) as u8
}

/// `[[ -n "$(git status --porcelain 2>/dev/null)" ]]`.
fn tree_is_dirty(repo: &Path) -> Result<bool, Error> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(["status", "--porcelain"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| Error::io("git", e))?;
    Ok(output.stdout.iter().any(|b| *b != b'\n'))
}

/// `git <args>` in `repo` on the process's streams, as the shell ran it, once
/// everything printed so far has reached them; `message` goes to its stdin.
fn git(
    repo: &Path,
    args: &[&str],
    message: Option<&str>,
    out: &mut dyn Write,
) -> Result<ExitStatus, Error> {
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let mut command = Command::new("git");
    command.current_dir(repo).args(args);
    let Some(message) = message else {
        command.stdin(Stdio::null());
        return command.status().map_err(|e| Error::io("git", e));
    };
    command.stdin(Stdio::piped());
    let mut child = command.spawn().map_err(|e| Error::io("git", e))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(message.as_bytes())
            .map_err(|e| Error::io("git", e))?;
    }
    child.wait().map_err(|e| Error::io("git", e))
}

fn refuse(err: &mut dyn Write, style: &Style, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}

/// `cmd_release`.
/// The bump and `--no-push`; `Err(None)` asks for the help, `Err(Some(arg))`
/// names an argument that is no bump type.
fn release_flags(args: &[String]) -> Result<(Bump, bool), Option<String>> {
    let (mut bump, mut skip_push) = (Bump::Patch, false);
    for arg in args {
        match (arg.as_str(), Bump::parse(arg)) {
            ("--help" | "-h", _) => return Err(None),
            ("--no-push", _) => skip_push = true,
            (_, Some(named)) => bump = named,
            (other, None) => return Err(Some(other.to_string())),
        }
    }
    Ok((bump, skip_push))
}

/// The Exuno checkout to release: the working directory when it is one,
/// else the install directory.
fn release_repo(env: &Env) -> Option<PathBuf> {
    let cwd = Path::new(&env.cwd);
    if cwd.join("VERSION").is_file() && cwd.join("Cargo.toml").is_file() {
        return Some(cwd.to_path_buf());
    }
    env.install_dir
        .as_deref()
        .filter(|dir| Path::new(dir).join("VERSION").is_file())
        .map(PathBuf::from)
}

/// Each crate manifest with its text; `Err` names the first without an
/// exuno version.
fn crate_files(repo: &Path) -> Result<Vec<(&'static str, String)>, &'static str> {
    CRATE_FILES
        .iter()
        .map(|name| {
            std::fs::read_to_string(repo.join(name))
                .ok()
                .filter(|text| crate_version(text).is_some())
                .map(|text| (*name, text))
                .ok_or(*name)
        })
        .collect()
}

/// Commits the version bump and tags it with its CHANGELOG section: a failed
/// git step's exit status, or `None`.
fn commit_and_tag(
    repo: &Path,
    new_version: &str,
    style: &Style,
    out: &mut dyn Write,
) -> Result<Option<u8>, Error> {
    let subject = format!("release: v{new_version}");
    for step in [
        &["add", "VERSION", "Cargo.toml", "Cargo.lock"][..],
        &["commit", "-m", &subject, "--quiet"][..],
    ] {
        let status = git(repo, step, None, out)?;
        if !status.success() {
            return Ok(Some(shell_status(status)));
        }
    }
    put(
        out,
        format!("  Created commit: {}\n", style.dim(&subject)).as_bytes(),
    )?;
    let changelog_file = repo.join("CHANGELOG.md");
    let changelog =
        std::fs::read_to_string(&changelog_file).map_err(|e| Error::io(&changelog_file, e))?;
    let message = tag_message(new_version, &changelog_section(&changelog, new_version));
    let status = git(
        repo,
        &["tag", "-a", new_version, "-F", "-"],
        Some(&message),
        out,
    )?;
    if !status.success() {
        return Ok(Some(shell_status(status)));
    }
    put(
        out,
        format!("  Created tag: {}\n", style.cyan(new_version)).as_bytes(),
    )?;
    Ok(None)
}

pub fn release(
    args: &[String],
    style: &Style,
    env: &mut Env,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (bump, skip_push) = match release_flags(args) {
        Ok(flags) => flags,
        Err(None) => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Err(Some(other)) => {
            let usage = HELP.synopsis_line();
            let message = format!("Unknown bump type: {other}\nUsage: {usage}");
            return refuse(err, style, &message);
        }
    };
    let Some(repo) = release_repo(env) else {
        return refuse(err, style, "Must be run from the Exuno repository.");
    };
    if tree_is_dirty(&repo)? {
        return refuse(
            err,
            style,
            "Working tree is not clean. Commit or stash changes first.",
        );
    }
    let version_file = repo.join("VERSION");
    let version_text =
        std::fs::read_to_string(&version_file).map_err(|e| Error::io(&version_file, e))?;
    let current = current_version(&version_text);
    let Some(parts) = parse_version(current) else {
        return refuse(err, style, &format!("Cannot parse VERSION: {current}"));
    };
    let crate_texts = match crate_files(&repo) {
        Ok(texts) => texts,
        Err(name) => {
            let message = format!("Cannot find the exuno crate version in {name}");
            return refuse(err, style, &message);
        }
    };
    let (major, minor, patch) = bump.apply(parts);
    let new_version = format!("{major}.{minor}.{patch}");
    put(
        out,
        format!(
            "\n{}\n\n  {} → {} ({})\n\n  {} Continue? [Y/n]: ",
            style.bold("  Exuno Release"),
            style.dim(current),
            style.green(&new_version),
            bump.name(),
            style.green("▸")
        )
        .as_bytes(),
    )?;
    out.flush().map_err(|e| Error::io("<stdout>", e))?;
    let Some(answer) = (env.read_line)() else {
        put(err, b"\n")?;
        return refuse(
            err,
            style,
            "input ended before an answer; nothing was released.",
        );
    };
    if answer.trim_matches([' ', '\t']).starts_with(['n', 'N']) {
        put(out, b"  Cancelled.\n")?;
        return Ok(0);
    }
    let mut files = vec![("VERSION", format!("{new_version}\n"))];
    for (name, text) in &crate_texts {
        files.push((name, set_crate_version(text, &new_version)));
    }
    for (name, text) in files {
        let path = repo.join(name);
        std::fs::write(&path, text).map_err(|e| Error::io(&path, e))?;
        put(
            out,
            format!("  Updated {} → {new_version}\n", style.cyan(name)).as_bytes(),
        )?;
    }
    if let Some(status) = commit_and_tag(&repo, &new_version, style, out)? {
        return Ok(status);
    }
    let released = style.green(&format!("Released v{new_version}!"));
    if skip_push {
        put(
            out,
            format!(
                "\n  {released} (local only, --no-push)\n\n  Push manually:\n    git push origin main && git push origin {new_version}\n\n"
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    push(&repo, &new_version, &released, out)
}

/// Pushes `main` and the tag, then announces the release.
fn push(repo: &Path, new_version: &str, released: &str, out: &mut dyn Write) -> Result<u8, Error> {
    put(out, b"\n  Pushing to origin...\n")?;
    for refname in ["main", new_version] {
        let status = git(repo, &["push", "--quiet", "origin", refname], None, out)?;
        if !status.success() {
            return Ok(shell_status(status));
        }
    }
    put(
        out,
        format!(
            "\n  {released}\n\n  GitHub Release will be created automatically by CI.\n  Users will see the update notification on next run.\n\n"
        )
        .as_bytes(),
    )?;
    Ok(0)
}

#[cfg(test)]
const TOML: &str = "[package]\nname = \"exuno\"\n# The crate version follows VERSION.\nversion = \"1.0.0\"\nedition = \"2024\"\n\n[dependencies]\nclap = { version = \"4.6\", features = [\"derive\"] }\n";
#[cfg(test)]
const LOCK: &str = "# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"exuno\"\nversion = \"1.0.0\"\ndependencies = [\n \"clap\",\n]\n\n[[package]]\nname = \"clap\"\nversion = \"4.6.0\"\n";
#[cfg(test)]
const CHANGELOG: &str = "# Changelog\n\n## 1.0.1\n\nA patch.\n\n- one fix\n\n## 1.0.0\n\nFirst.\n";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_and_bumps_read_like_cmd_release() {
        assert_eq!(current_version("1.0.0\n"), "1.0.0");
        assert_eq!(current_version("  1.2.3 \nsecond\n"), "1.2.3");
        assert_eq!(current_version(""), "");
        assert_eq!(parse_version("1.2.3"), Some((1, 2, 3)));
        for bad in ["1.2", "1.a.0", "1.2.3.4", "", "1..3", "1.2.3."] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
        assert_eq!(Bump::parse("major"), Some(Bump::Major));
        assert_eq!(Bump::parse("Patch"), None);
        assert_eq!(Bump::Major.apply((1, 2, 3)), (2, 0, 0));
        assert_eq!(Bump::Minor.apply((1, 2, 3)), (1, 3, 0));
        assert_eq!(Bump::Patch.apply((1, 2, 3)), (1, 2, 4));
    }

    #[test]
    fn the_crate_version_after_the_name_is_read_and_rewritten_like_the_awk() {
        assert_eq!(crate_version(TOML).as_deref(), Some("1.0.0"));
        assert_eq!(crate_version(LOCK).as_deref(), Some("1.0.0"));
        assert_eq!(crate_version("version = 4\n"), None);
        assert_eq!(
            crate_version("version = \"1.0.0\"\nname = \"exuno\"\n"),
            None
        );
        assert_eq!(
            set_crate_version(TOML, "1.1.0"),
            TOML.replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        );
        let lock = set_crate_version(LOCK, "1.1.0");
        assert_eq!(
            lock,
            LOCK.replace("version = \"1.0.0\"", "version = \"1.1.0\"")
        );
        assert!(lock.contains("\nversion = 4\n") && lock.ends_with("version = \"4.6.0\"\n"));
        assert_eq!(set_crate_version("version = 4\n", "1.1.0"), "version = 4\n");
        assert_eq!(
            set_crate_version("name = \"exuno\"\nversion = \"1.0.0\"", "2.0.0"),
            "name = \"exuno\"\nversion = \"2.0.0\"\n"
        );
        assert_eq!(set_crate_version("", "2.0.0"), "");
    }

    #[test]
    fn the_changelog_section_is_cut_like_the_awk() {
        assert_eq!(
            changelog_section(CHANGELOG, "1.0.1"),
            "\nA patch.\n\n- one fix"
        );
        assert_eq!(changelog_section(CHANGELOG, "1.0.0"), "\nFirst.");
        assert_eq!(changelog_section(CHANGELOG, "1.1.0"), "");
        assert_eq!(
            changelog_section("## 2.0.0\nx\n## 2.0.0\ny\n## 1.0.0\n", "2.0.0"),
            "x\ny"
        );
        assert_eq!(
            tag_message("1.0.1", "\nA patch.\n\n- one fix"),
            "v1.0.1\n\n\nA patch.\n\n- one fix"
        );
        assert_eq!(tag_message("1.1.0", ""), "v1.1.0\n\n");
    }
}

#[cfg(all(test, unix))]
mod checkout_tests {
    use super::*;
    use crate::paths::DiskText;

    fn sh(dir: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap()
    }

    /// A checkout `release` accepts, committed on `main`, with the developer's
    /// signing and hooks settings overridden locally.
    fn checkout(version: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("VERSION"), format!("{version}\n")).unwrap();
        std::fs::write(root.join("Cargo.toml"), TOML.replace("1.0.0", version)).unwrap();
        std::fs::write(root.join("Cargo.lock"), LOCK.replace("1.0.0", version)).unwrap();
        std::fs::write(root.join("CHANGELOG.md"), CHANGELOG).unwrap();
        sh(&root, &["init", "-q"]);
        sh(&root, &["symbolic-ref", "HEAD", "refs/heads/main"]);
        for (key, value) in [
            ("user.email", "test@test.com"),
            ("user.name", "Test"),
            ("commit.gpgsign", "false"),
            ("tag.gpgsign", "false"),
            ("core.hooksPath", ".git/hooks"),
        ] {
            sh(&root, &["config", key, value]);
        }
        sh(&root, &["add", "-A"]);
        sh(&root, &["commit", "-q", "-m", "seed"]);
        (dir, root)
    }

    fn run(
        args: &[&str],
        cwd: &Path,
        install_dir: Option<&Path>,
        answer: Option<&str>,
    ) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let mut answer = answer.map(str::to_string);
        let mut read_line = || answer.take();
        let mut env = Env {
            cwd: cwd.disk_text(),
            install_dir: install_dir.map(|dir| dir.disk_text()),
            read_line: &mut read_line,
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = release(&args, &Style::plain(), &mut env, &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_release_bumps_the_three_files_commits_and_tags_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  Exuno Release\n\n  1.0.0 → 1.0.1 (patch)\n\n  ▸ Continue? [Y/n]:   Updated VERSION → 1.0.1\n  Updated Cargo.toml → 1.0.1\n  Updated Cargo.lock → 1.0.1\n  Created commit: release: v1.0.1\n  Created tag: 1.0.1\n\n  Released v1.0.1! (local only, --no-push)\n\n  Push manually:\n    git push origin main && git push origin 1.0.1\n\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "1.0.1\n"
        );
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.toml")).unwrap(),
            TOML.replace("1.0.0", "1.0.1")
        );
        assert_eq!(
            std::fs::read_to_string(root.join("Cargo.lock")).unwrap(),
            LOCK.replace("1.0.0", "1.0.1")
        );
        assert_eq!(
            sh(&root, &["log", "--format=%s", "-2"]),
            "release: v1.0.1\nseed\n"
        );
        assert_eq!(
            sh(&root, &["show", "--format=", "--name-only", "HEAD"]),
            "Cargo.lock\nCargo.toml\nVERSION\n"
        );
        assert_eq!(
            sh(&root, &["tag", "-l", "--format=%(contents)", "1.0.1"]),
            "v1.0.1\n\nA patch.\n\n- one fix\n\n"
        );
        assert_eq!(sh(&root, &["status", "--porcelain"]), "");
        let (status, out, _) = run(&["minor", "--no-push"], &root, None, Some(""));
        assert_eq!(status, 0);
        assert!(out.contains("\n  1.0.1 → 1.1.0 (minor)\n"));
        assert_eq!(
            sh(&root, &["tag", "-l", "--format=%(contents)", "1.1.0"]),
            "v1.1.0\n\n"
        );
        let (status, out, _) = run(&["major", "--no-push"], &root, None, Some("yes"));
        assert_eq!(status, 0);
        assert!(out.contains("\n  1.1.0 → 2.0.0 (major)\n"));
    }

    #[test]
    fn the_prompt_cancels_on_n_and_ends_at_end_of_input_like_bash_does() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["--no-push"], &root, None, Some(" nope"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.ends_with("\n  ▸ Continue? [Y/n]:   Cancelled.\n"));
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, None);
        assert_eq!(
            (status, err.as_str()),
            (
                1,
                "\nError: input ended before an answer; nothing was released.\n"
            )
        );
        assert!(out.ends_with("\n  ▸ Continue? [Y/n]: "));
        assert_eq!(
            std::fs::read_to_string(root.join("VERSION")).unwrap(),
            "1.0.0\n"
        );
        assert_eq!(sh(&root, &["log", "--format=%s"]), "seed\n");
    }

    #[test]
    fn refusals_leave_the_checkout_untouched_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let (status, out, err) = run(&["banana"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Unknown bump type: banana\nUsage: exuno release [major|minor|patch] [--no-push]\n"
        );
        let (_, _, err) = run(&["patch", "extra"], &root, None, Some("y"));
        assert_eq!(
            err,
            "Error: Unknown bump type: extra\nUsage: exuno release [major|minor|patch] [--no-push]\n"
        );
        let (status, out, err) = run(&["--help"], &root, None, Some("y"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  exuno release — bump the version, commit, tag, and push\n\n  USAGE\n    exuno release [major|minor|patch] [--no-push]\n\n  DESCRIPTION\n    Bumps VERSION, Cargo.toml, and Cargo.lock together, commits, tags with\n    the changelog section of the new version, and pushes main and the tag.\n    Runs from the Exuno repository checkout, or from AGENTSYNC_HOME when\n    that holds one.\n\n  ARGUMENTS\n    major   Bump the major version (x+1.0.0)\n    minor   Bump the minor version (x.y+1.0)\n    patch   Bump the patch version (x.y.z+1); the default\n\n  OPTIONS\n    --no-push    Commit and tag locally without pushing to origin\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno release\n    exuno release minor\n    exuno release major --no-push\n\n"
        );
        std::fs::write(root.join("dirty.txt"), "x\n").unwrap();
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Working tree is not clean. Commit or stash changes first.\n"
        );
        std::fs::remove_file(root.join("dirty.txt")).unwrap();
        std::fs::write(root.join("VERSION"), "1.2\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "two parts"]);
        let (status, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Cannot parse VERSION: 1.2\n")
        );
        std::fs::write(root.join("VERSION"), "1.a.0\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "alpha"]);
        let (status, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Cannot parse VERSION: 1.a.0\n")
        );
        std::fs::write(root.join("VERSION"), "1.0.0\n").unwrap();
        std::fs::write(root.join("Cargo.lock"), "version = 4\n").unwrap();
        sh(&root, &["commit", "-q", "-am", "lock"]);
        let (status, out, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: Cannot find the exuno crate version in Cargo.lock\n"
        );
        // Cargo.toml is what makes a directory a checkout now (Phase 6).
        std::fs::remove_file(root.join("Cargo.toml")).unwrap();
        sh(&root, &["commit", "-q", "-am", "toml"]);
        let (_, _, err) = run(&["patch", "--no-push"], &root, None, Some("y"));
        assert_eq!(err, "Error: Must be run from the Exuno repository.\n");
        assert_eq!(sh(&root, &["status", "--porcelain"]), "");
        assert_eq!(sh(&root, &["tag", "-l"]), "");
    }

    #[test]
    fn the_checkout_comes_from_agentsync_home_when_cwd_is_none_like_resolve_install_dir() {
        let (_dir, root) = checkout("1.0.0");
        let elsewhere = tempfile::tempdir().unwrap();
        let (status, _, err) = run(&["patch", "--no-push"], elsewhere.path(), None, Some("y"));
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Must be run from the Exuno repository.\n")
        );
        let (status, out, err) = run(
            &["patch", "--no-push"],
            elsewhere.path(),
            Some(&root),
            Some("y"),
        );
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.contains("  Created tag: 1.0.1\n"));
        assert_eq!(
            sh(&root, &["log", "--format=%s", "-1"]),
            "release: v1.0.1\n"
        );
        std::fs::remove_file(root.join("VERSION")).unwrap();
        sh(&root, &["commit", "-q", "-am", "no version"]);
        let (status, _, err) = run(
            &["patch", "--no-push"],
            elsewhere.path(),
            Some(&root),
            Some("y"),
        );
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: Must be run from the Exuno repository.\n")
        );
    }

    #[test]
    fn the_push_reaches_origin_like_release_sh() {
        let (_dir, root) = checkout("1.0.0");
        let remote = tempfile::tempdir().unwrap();
        sh(remote.path(), &["init", "-q", "--bare"]);
        sh(
            &root,
            &["remote", "add", "origin", &remote.path().disk_text()],
        );
        let (status, out, err) = run(&["patch"], &root, None, Some("y"));
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.ends_with(
            "  Created tag: 1.0.1\n\n  Pushing to origin...\n\n  Released v1.0.1!\n\n  GitHub Release will be created automatically by CI.\n  Users will see the update notification on next run.\n\n"
        ));
        assert_eq!(
            sh(remote.path(), &["log", "--format=%s", "-1", "main"]),
            "release: v1.0.1\n"
        );
        assert_eq!(sh(remote.path(), &["tag", "-l"]), "1.0.1\n");
    }
}
