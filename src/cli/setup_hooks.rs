//! `exuno setup-hooks`: `lib/setup_hooks.sh`, which installs the git hooks
//! that suit the project's outputs mode, appending one marked block per hook
//! and leaving whatever the hook already ran.

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::put;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::{Error, config::names, config::project_config};

const BLOCK_START: &str = names::HOOK_BLOCKS[0].0;
const BLOCK_END: &str = names::HOOK_BLOCKS[0].1;

pub const HELP: Help = Help {
    command: "setup-hooks",
    tagline: "install the git hooks that suit the project's outputs mode",
    synopsis: &["setup-hooks [--pre-commit]"],
    description: &[
        "Installs the git hooks that suit this project's outputs mode. Each hook\ngets one marked block appended; whatever the hook already ran stays.\nRunning it again rewrites a block an older release installed.",
    ],
    sections: &[
        Section {
            title: "MODES",
            entries: &[
                (
                    "committed",
                    "pre-commit re-syncs and fails the commit when a generated file\nchanged, so outputs never lag source",
                ),
                (
                    "local",
                    "post-merge and post-checkout run exuno sync after\npull/checkout",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--pre-commit",
                    "In local mode, also install a pre-commit hook that runs\nexuno sync --if-stale",
                ),
                ("-h, --help", "Show this help"),
            ],
        },
        Section {
            title: "ENVIRONMENT",
            entries: &[("EXUNO_SKIP_HOOKS=1", "Make the installed hooks no-ops")],
        },
    ],
    examples: &["setup-hooks", "setup-hooks --pre-commit"],
};

const HOOKS_PATH_ELSEWHERE: &str =
    "This repository points core.hooksPath at another directory, so Exuno
will not write there:

  {hooks}

A hook manager (husky, lefthook, pre-commit) most likely owns it. Add this
to the hook it manages instead:

  command -v exuno >/dev/null 2>&1 && exuno sync --if-stale || true

";

/// `emit_sync_body`: the POSIX body a hook runs, `$(...)` having dropped the
/// trailing newline.
fn sync_body(sync_args: &str) -> String {
    format!(
        "[ -n \"${{EXUNO_SKIP_HOOKS:-}}${{AGENTSYNC_SKIP_HOOKS:-}}\" ] && exit 0
_exuno=exuno
command -v exuno >/dev/null 2>&1 || _exuno=agentsync
if command -v \"$_exuno\" >/dev/null 2>&1; then
    echo \"Exuno: syncing AI config...\"
    \"$_exuno\" {sync_args} || echo \"Exuno: sync skipped — run 'exuno sync' to see why.\" >&2
else
    echo \"Exuno: exuno is not on PATH; skipping. Install it or set EXUNO_SKIP_HOOKS=1.\" >&2
fi"
    )
}

/// `emit_precommit_gate_body`.
const GATE_BODY: &str = "[ -n \"${EXUNO_SKIP_HOOKS:-}${AGENTSYNC_SKIP_HOOKS:-}\" ] && exit 0
_exuno=exuno
command -v exuno >/dev/null 2>&1 || _exuno=agentsync
if command -v \"$_exuno\" >/dev/null 2>&1; then
    \"$_exuno\" sync --if-stale || echo \"Exuno: sync skipped — run 'exuno sync' to see why.\" >&2
fi
if [ -f \".ai/.sync-manifest\" ]; then
    # Flag a generated file only when the commit would leave it behind: the
    # worktree differs from the index (second status column) or it is untracked.
    # Already-staged output is exactly what belongs in the commit. The manifest
    # is awk's first input, so the hook needs no temp file.
    _as_dirty=$(git status --porcelain --untracked-files=all \\
        | awk -F'\\t' '
            NR == FNR { keep[$1] = 1; next }
            {
                code = substr($0, 1, 2)
                path = substr($0, 4)
                if ((code == \"??\" || substr(code, 2, 1) != \" \") && (path in keep))
                    print path
            }' \".ai/.sync-manifest\" - || true)
    if [ -n \"$_as_dirty\" ]; then
        echo \"Exuno: generated files are out of date in this commit:\" >&2
        printf '%s\\n' \"$_as_dirty\" | sed 's/^/      /' >&2
        echo \"\" >&2
        echo \"  They are regenerated from .ai/src/ and belong in the same commit.\" >&2
        echo \"  Stage them (git add -A) and commit again.\" >&2
        echo \"\" >&2
        echo \"  To commit without them: EXUNO_SKIP_HOOKS=1 git commit ...\" >&2
        exit 1
    fi
fi";

fn git(root: &str, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string()
    })
}

/// `_physical_path`: the parent resolved, the leaf as given.
fn physical(path: &str) -> String {
    let leaf = crate::paths::leaf(path);
    match std::fs::canonicalize(crate::paths::parent(path)) {
        Ok(parent) => format!("{}/{leaf}", parent.disk_text()),
        Err(_) => path.to_string(),
    }
}

fn find(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    haystack
        .get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| from + i)
}

/// `OUTPUTS_MODE` from the first config present: `outputs`, else `committed`
/// when `gitignore.update` is `false`, else `local`.
fn outputs_mode(root: &str) -> &'static str {
    for rel in names::CONFIG_CANDIDATES {
        let path = Path::new(root).join(rel);
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        return project_config::outputs_mode(&String::from_utf8_lossy(&bytes)).unwrap_or("local");
    }
    "local"
}

/// `install_hook`.
fn install_hook(
    hooks_dir: &Path,
    name: &str,
    body: &str,
    out: &mut dyn Write,
) -> Result<(), Error> {
    let hook = hooks_dir.join(name);
    if !hook.is_file() {
        std::fs::write(&hook, "#!/bin/sh\n\n").map_err(|e| Error::io(&hook, e))?;
    }
    let existing = std::fs::read(&hook).map_err(|e| Error::io(&hook, e))?;
    let block = format!("{BLOCK_START}\n{body}\n{BLOCK_END}");
    let present = names::HOOK_BLOCKS
        .iter()
        .find_map(|(start, end)| find(&existing, start.as_bytes(), 0).map(|at| (at, *end)));
    match present {
        None => {
            let mut appended = existing;
            appended.extend_from_slice(format!("\n{block}\n").as_bytes());
            std::fs::write(&hook, appended).map_err(|e| Error::io(&hook, e))?;
        }
        Some((start, end_marker)) => {
            let end = find(&existing, end_marker.as_bytes(), start).map(|i| i + end_marker.len());
            match end {
                Some(end) if existing[start..end] != *block.as_bytes() => {
                    let mut rewritten = existing[..start].to_vec();
                    rewritten.extend_from_slice(block.as_bytes());
                    rewritten.extend_from_slice(&existing[end..]);
                    std::fs::write(&hook, rewritten).map_err(|e| Error::io(&hook, e))?;
                    put(out, format!("Updated Exuno hook in {name}.\n").as_bytes())?;
                }
                _ => put(
                    out,
                    format!("Exuno hook already present in {name}.\n").as_bytes(),
                )?,
            }
        }
    }
    executable(&hook)?;
    put(out, format!("Configured {name} hook.\n").as_bytes())
}

/// `chmod +x`.
#[cfg(unix)]
fn executable(path: &Path) -> Result<(), Error> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::metadata(path).map_err(|e| Error::io(path, e))?;
    let mut perms = meta.permissions();
    perms.set_mode(perms.mode() | 0o111);
    std::fs::set_permissions(path, perms).map_err(|e| Error::io(path, e))
}

#[cfg(not(unix))]
fn executable(_path: &Path) -> Result<(), Error> {
    Ok(())
}

/// `lib/setup_hooks.sh`: `root` is `AGENTSYNC_REPO_ROOT` or the working
/// directory, as `cmd_engine` exported it.
pub fn setup_hooks(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args.iter().any(|arg| arg == "--help" || arg == "-h") {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let mut pre_commit = false;
    for arg in args {
        match arg.as_str() {
            "--pre-commit" => pre_commit = true,
            other => {
                put(
                    err,
                    format!(
                        "Error: Unknown option: {other}\nUsage: {}\n",
                        HELP.synopsis_line()
                    )
                    .as_bytes(),
                )?;
                return Ok(2);
            }
        }
    }
    if !Path::new(root).is_dir() {
        put(
            err,
            format!("Error: Repository root not found: {root}\n").as_bytes(),
        )?;
        return Ok(1);
    }
    let root = crate::paths::normalize(root);
    if git(&root, &["rev-parse", "--git-dir"]).is_none() {
        put(
            err,
            format!("Error: Not a git repository: {root}\n").as_bytes(),
        )?;
        return Ok(1);
    }
    let mut hooks_dir = git(&root, &["rev-parse", "--git-path", "hooks"]).unwrap_or_default();
    if !crate::paths::is_absolute(&hooks_dir) {
        hooks_dir = format!("{root}/{hooks_dir}");
    }
    let git_dir = git(&root, &["rev-parse", "--absolute-git-dir"]).unwrap_or_default();
    if physical(&hooks_dir) != physical(&format!("{git_dir}/hooks")) {
        put(
            out,
            HOOKS_PATH_ELSEWHERE
                .replace("{hooks}", &hooks_dir)
                .as_bytes(),
        )?;
        return Ok(0);
    }
    let hooks_dir = PathBuf::from(hooks_dir);
    if outputs_mode(&root) == "committed" {
        install_hook(&hooks_dir, "pre-commit", GATE_BODY, out)?;
        put(out, b"Git hooks configured for committed outputs.\n")?;
    } else {
        install_hook(&hooks_dir, "post-merge", &sync_body("sync"), out)?;
        install_hook(&hooks_dir, "post-checkout", &sync_body("sync"), out)?;
        if pre_commit {
            install_hook(&hooks_dir, "pre-commit", &sync_body("sync --if-stale"), out)?;
        }
        put(out, b"Git hooks configured for local outputs.\n")?;
    }
    Ok(0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn run(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = setup_hooks(&args, root, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    /// A repository whose hooks path is pinned to `.git/hooks`, so the
    /// developer's global `core.hooksPath` never reaches the test.
    fn repo(config: &str) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        assert!(
            Command::new("git")
                .args(["-C", &root, "init", "-q"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .args(["-C", &root, "config", "core.hooksPath", ".git/hooks"])
                .status()
                .unwrap()
                .success()
        );
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), config).unwrap();
        (dir, root)
    }

    fn mode(path: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    #[test]
    fn local_outputs_get_the_sync_hooks_like_setup_hooks_sh() {
        let (dir, root) = repo("outputs: local\n");
        let (status, out, err) = run(&root, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "Configured post-merge hook.\nConfigured post-checkout hook.\nGit hooks configured for local outputs.\n"
        );
        let hook = dir.path().join(".git/hooks/post-merge");
        assert_eq!(mode(&hook), 0o755);
        assert_eq!(
            std::fs::read_to_string(&hook).unwrap(),
            format!(
                "#!/bin/sh\n\n\n{BLOCK_START}\n{}\n{BLOCK_END}\n",
                sync_body("sync")
            )
        );
        assert!(!dir.path().join(".git/hooks/pre-commit").exists());
        let (status, out, _) = run(&root, &["--pre-commit"]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "Exuno hook already present in post-merge.\nConfigured post-merge hook.\nExuno hook already present in post-checkout.\nConfigured post-checkout hook.\nConfigured pre-commit hook.\nGit hooks configured for local outputs.\n"
        );
        let pre_commit = std::fs::read_to_string(dir.path().join(".git/hooks/pre-commit")).unwrap();
        assert!(pre_commit.contains("\n    \"$_exuno\" sync --if-stale || echo"));
        std::fs::write(&hook, "#!/bin/sh\necho \"existing hook\"\n").unwrap();
        run(&root, &[]);
        assert!(
            std::fs::read_to_string(&hook)
                .unwrap()
                .starts_with("#!/bin/sh\necho \"existing hook\"\n\n# >>> EXUNO")
        );
    }

    #[test]
    fn a_block_without_its_end_marker_is_left_alone() {
        let (dir, root) = repo("outputs: local\n");
        let hook = dir.path().join(".git/hooks/post-merge");
        let unterminated = format!("#!/bin/sh\n{BLOCK_START}\nbash lib/sync.sh\n");
        std::fs::write(&hook, &unterminated).unwrap();
        let (status, out, _) = run(&root, &[]);
        assert_eq!(status, 0);
        assert!(out.starts_with("Exuno hook already present in post-merge.\n"));
        assert_eq!(std::fs::read_to_string(&hook).unwrap(), unterminated);
    }

    #[test]
    fn committed_outputs_get_the_gate_like_setup_hooks_sh() {
        let (dir, root) = repo("gitignore:\n  update: false\n");
        let (status, out, _) = run(&root, &[]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "Configured pre-commit hook.\nGit hooks configured for committed outputs.\n"
        );
        let gate = std::fs::read_to_string(dir.path().join(".git/hooks/pre-commit")).unwrap();
        assert_eq!(
            gate,
            format!("#!/bin/sh\n\n\n{BLOCK_START}\n{GATE_BODY}\n{BLOCK_END}\n")
        );
        assert!(!dir.path().join(".git/hooks/post-merge").exists());
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "outputs: \"committed\"\n",
        )
        .unwrap();
        assert_eq!(outputs_mode(&root), "committed");
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "outputs: other\n").unwrap();
        assert_eq!(outputs_mode(&root), "local");
        std::fs::write(dir.path().join(".ai/exuno.yaml"), "outputs: committed\n").unwrap();
        assert_eq!(outputs_mode(&root), "committed");
    }

    #[test]
    fn another_hooks_path_a_missing_repo_and_bad_options_are_refused_like_setup_hooks_sh() {
        let (dir, root) = repo("outputs: local\n");
        std::fs::create_dir_all(dir.path().join(".githooks")).unwrap();
        assert!(
            Command::new("git")
                .args(["-C", &root, "config", "core.hooksPath", ".githooks"])
                .status()
                .unwrap()
                .success()
        );
        let (status, out, err) = run(&root, &[]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            format!(
                "This repository points core.hooksPath at another directory, so Exuno\nwill not write there:\n\n  {root}/.githooks\n\nA hook manager (husky, lefthook, pre-commit) most likely owns it. Add this\nto the hook it manages instead:\n\n  command -v exuno >/dev/null 2>&1 && exuno sync --if-stale || true\n\n"
            )
        );
        assert!(!dir.path().join(".githooks/post-merge").exists());
        let (status, _, err) = run(&root, &["--bogus"]);
        assert_eq!(status, 2);
        assert_eq!(
            err,
            "Error: Unknown option: --bogus\nUsage: exuno setup-hooks [--pre-commit]\n"
        );
        let (status, out, err) = run(&root, &["--bogus", "--help"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(out.contains("exuno setup-hooks — install the git hooks"));
        let (status, out, _) = run(&root, &["--help"]);
        assert_eq!(status, 0);
        assert_eq!(
            out,
            "\n  exuno setup-hooks — install the git hooks that suit the project's outputs mode\n\n  USAGE\n    exuno setup-hooks [--pre-commit]\n\n  DESCRIPTION\n    Installs the git hooks that suit this project's outputs mode. Each hook\n    gets one marked block appended; whatever the hook already ran stays.\n    Running it again rewrites a block an older release installed.\n\n  MODES\n    committed   pre-commit re-syncs and fails the commit when a generated file\n                changed, so outputs never lag source\n    local       post-merge and post-checkout run exuno sync after\n                pull/checkout\n\n  OPTIONS\n    --pre-commit   In local mode, also install a pre-commit hook that runs\n                   exuno sync --if-stale\n    -h, --help     Show this help\n\n  ENVIRONMENT\n    EXUNO_SKIP_HOOKS=1   Make the installed hooks no-ops\n\n  EXAMPLES\n    exuno setup-hooks\n    exuno setup-hooks --pre-commit\n\n"
        );
        let missing = format!("{root}/nowhere");
        let (status, _, err) = run(&missing, &[]);
        assert_eq!(status, 1);
        assert_eq!(
            err,
            format!("Error: Repository root not found: {missing}\n")
        );
        let plain = tempfile::tempdir().unwrap();
        let plain_root = std::fs::canonicalize(plain.path()).unwrap().disk_text();
        let (status, _, err) = run(&plain_root, &[]);
        assert_eq!(status, 1);
        assert_eq!(err, format!("Error: Not a git repository: {plain_root}\n"));
    }
}
