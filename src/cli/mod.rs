pub mod add;
pub mod adopt;
pub mod bundle;
pub mod check;
pub mod customize;
pub mod dedupe;
pub mod diff;
pub mod doctor;
pub mod enable;
pub mod generate;
pub mod init;
pub mod list;
pub mod mcp;
mod mcp_merge;
pub mod migrate;
pub mod notice;
pub mod profile;
pub mod refresh;
pub mod release;
pub mod resolve;
pub mod rollback;
pub mod setup_hooks;
pub mod shell_init;
pub mod show;
pub mod simplify;
pub mod skills;
mod stale_targets;
pub mod sync;
pub mod update;
pub mod upgrade_config;
pub mod usage;
pub mod workspace;

use crate::paths::DiskText;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::Error;
use crate::output::style::Style;
use crate::project::Project;

pub(crate) fn put(writer: &mut dyn Write, bytes: &[u8]) -> Result<(), Error> {
    writer
        .write_all(bytes)
        .map_err(|e| Error::io("<output>", e))
}

/// `find <dir> -type f`, recursively; symlinks are not followed.
pub(crate) fn files_below(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|entry| entry.ok()) {
        let path = entry.path();
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            files_below(&path, found);
        } else if meta.is_file() {
            found.push(path);
        }
    }
}

/// Non-hidden entries of a directory in byte order, as `printf '%s\0' dir/* | LC_ALL=C sort -z`.
pub(crate) fn sorted_entries(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut names: Vec<String> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().disk_text())
        .filter(|name| !name.starts_with('.'))
        .collect();
    names.sort();
    names.into_iter().map(|name| dir.join(name)).collect()
}

/// `tool_resolver_require_project_user_dir`: prints why and returns status 1.
pub(crate) fn refuse_outside_tools_dir(
    project: &Project,
    style: &Style,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    err.write_all(
        format!(
            "{}: source.tools resolves outside the project: {}\nExuno only reads that catalog; edit its tool overrides where they live.\n",
            style.red("Error"),
            project.user_tools_dir().disk_text()
        )
        .as_bytes(),
    )
    .map_err(|e| Error::io("<stderr>", e))?;
    Ok(1)
}

/// The command word `main` dispatches on. Each command parses its own options
/// as its Bash `cmd_*` did, so only the word is matched here: a parser owning
/// the options would consume a leading `--`. `usage::wants_usage` answers
/// `help` and the help flags before this runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Version,
    List,
    Skills,
    Mcp,
    Check,
    Sync,
    Rollback,
    Update,
    UpdateCache,
    Catalog,
    Dedupe,
    Migrate,
    Generate,
    ShellInit,
    SetupHooks,
    Release,
    Export,
    Import,
    Add,
    Doctor,
    Init,
    Refresh,
    UpgradeConfig,
    Enable,
    Disable,
    Customize,
    Show,
    Diff,
    Simplify,
    Resolve,
    Profile,
    Adopt,
}

impl Command {
    /// The command `word` names, with the aliases the Bash `case` accepted:
    /// `ls`, `gen`, and the version flags as commands.
    pub fn parse(word: &str) -> Option<Self> {
        Some(match word {
            "version" | "--version" | "-v" => Self::Version,
            "list" | "ls" => Self::List,
            "skills" => Self::Skills,
            "mcp" => Self::Mcp,
            "check" => Self::Check,
            "sync" => Self::Sync,
            "rollback" => Self::Rollback,
            "update" => Self::Update,
            "__update-cache" => Self::UpdateCache,
            update::CATALOG_COMMAND => Self::Catalog,
            "dedupe" => Self::Dedupe,
            "migrate" => Self::Migrate,
            "generate" | "gen" => Self::Generate,
            "shell-init" => Self::ShellInit,
            "setup-hooks" => Self::SetupHooks,
            "release" => Self::Release,
            "export" => Self::Export,
            "import" => Self::Import,
            "add" => Self::Add,
            "doctor" => Self::Doctor,
            "init" => Self::Init,
            "refresh" => Self::Refresh,
            "upgrade-config" => Self::UpgradeConfig,
            "enable" => Self::Enable,
            "disable" => Self::Disable,
            "customize" => Self::Customize,
            "show" => Self::Show,
            "diff" => Self::Diff,
            "simplify" => Self::Simplify,
            "resolve" => Self::Resolve,
            "profile" => Self::Profile,
            "adopt" => Self::Adopt,
            _ => return None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, files_below, sorted_entries};

    #[test]
    fn sorted_entries_skips_hidden_names_and_sorts_bytewise() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["b", "Z", "a", ".hidden"] {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
        std::fs::create_dir(dir.path().join("c")).unwrap();
        let names: Vec<String> = sorted_entries(dir.path())
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["Z", "a", "b", "c"]);
        assert!(sorted_entries(&dir.path().join("missing")).is_empty());
    }

    #[test]
    fn files_below_lists_nested_files_but_not_directories() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("x/y")).unwrap();
        std::fs::write(dir.path().join("top"), "").unwrap();
        std::fs::write(dir.path().join("x/y/deep"), "").unwrap();
        let mut found = Vec::new();
        files_below(dir.path(), &mut found);
        found.sort();
        assert_eq!(found, [dir.path().join("top"), dir.path().join("x/y/deep")]);
    }

    // Windows creates symlinks only with developer mode or administrator rights.
    #[cfg(unix)]
    #[test]
    fn files_below_does_not_follow_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("secret"), "").unwrap();
        std::os::unix::fs::symlink(outside.path(), dir.path().join("dir-link")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret"), dir.path().join("file-link"))
            .unwrap();
        let mut found = Vec::new();
        files_below(dir.path(), &mut found);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn aliases_name_the_same_command() {
        assert_eq!(Command::parse("ls"), Some(Command::List));
        assert_eq!(Command::parse("gen"), Some(Command::Generate));
        assert_eq!(Command::parse("--version"), Some(Command::Version));
        assert_eq!(Command::parse("-v"), Some(Command::Version));
    }

    #[test]
    fn hidden_commands_parse() {
        assert_eq!(Command::parse("__update-cache"), Some(Command::UpdateCache));
        assert_eq!(Command::parse("__catalog"), Some(Command::Catalog));
    }

    #[test]
    fn unknown_words_and_help_are_not_commands() {
        assert_eq!(Command::parse(""), None);
        assert_eq!(Command::parse("help"), None);
        assert_eq!(Command::parse("--help"), None);
        assert_eq!(Command::parse("synch"), None);
    }
}
