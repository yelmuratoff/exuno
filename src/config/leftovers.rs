//! What a project still names `agentsync`, and the rename to `exuno` for each.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::config::{names, yaml_edit};
use crate::paths::DiskText;

const CONFIG: &str = ".ai/exuno.yaml";
const LEGACY_CONFIGS: [&str; 2] = [".ai/agent_sync.yaml", "agent_sync.yaml"];
const LEGACY_KEY: &str = "agentsync_version";
const KEY: &str = "exuno_version";
const LEGACY_SKILL: &str = "agentsync";
const SKILL: &str = "exuno";
const METADATA_KEYS: [&str; 3] = ["use-when", "not-for", "requirements"];
const CI_WORKFLOW: &str = ".github/workflows/exuno-check.yml";
const LEGACY_CI_WORKFLOW: &str = ".github/workflows/agentsync-check.yml";
const HOOKS: [&str; 3] = ["pre-commit", "post-merge", "post-checkout"];

/// The kind of thing that still carries the old name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ConfigFile,
    VersionKey,
    SkillMetadata,
    SkillDir,
    CiWorkflow,
    HookBlock,
}

/// One thing to rename: `path` is what carries the old name; `blocked_by` is
/// the path its new name already belongs to, which leaves it to the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Leftover {
    pub kind: Kind,
    pub path: PathBuf,
    pub blocked_by: Option<PathBuf>,
}

impl Leftover {
    fn new(kind: Kind, path: PathBuf, blocked_by: Option<PathBuf>) -> Self {
        Self {
            kind,
            path,
            blocked_by,
        }
    }

    /// The rename as one line, paths relative to `root`.
    pub fn describe(&self, root: &Path) -> String {
        let shown = |path: &Path| {
            path.strip_prefix(root)
                .map(|rel| rel.disk_text())
                .unwrap_or_else(|_| path.disk_text())
        };
        let path = shown(&self.path);
        if let Some(blocked) = &self.blocked_by {
            return format!(
                "{path} — {} already exists; merge it by hand",
                shown(blocked)
            );
        }
        match self.kind {
            Kind::ConfigFile => format!("{path} → {CONFIG}"),
            Kind::VersionKey => format!("{LEGACY_KEY} → {KEY} in {path}"),
            Kind::SkillMetadata => {
                format!("metadata.{LEGACY_SKILL}-* → metadata.{SKILL}-* in {path}")
            }
            Kind::SkillDir => format!("{path}/ → {}/", shown(&renamed_dir(&self.path))),
            Kind::CiWorkflow => format!("{path} → {CI_WORKFLOW}"),
            Kind::HookBlock => {
                format!("{path} still runs the {LEGACY_SKILL} block — run exuno setup-hooks")
            }
        }
    }
}

/// Everything under `root` that still carries the old name, in a stable order.
pub fn scan(root: &Path) -> Vec<Leftover> {
    let mut found = Vec::new();
    let config = root.join(CONFIG);
    let mut moving = false;
    for rel in LEGACY_CONFIGS {
        let legacy = root.join(rel);
        if legacy.is_file() {
            let blocked = config.is_file().then(|| config.clone());
            moving = blocked.is_none();
            found.push(Leftover::new(Kind::ConfigFile, legacy, blocked));
            break;
        }
    }
    if let Some(resolved) = resolved_config(root)
        && read(&resolved).is_some_and(|text| pins_legacy_key(&text))
    {
        let edited = if moving { config } else { resolved };
        found.push(Leftover::new(Kind::VersionKey, edited, None));
    }
    let mut skill_files = Vec::new();
    let mut skill_dirs = Vec::new();
    walk_skills(
        &root.join(".ai/src/skills"),
        &mut skill_files,
        &mut skill_dirs,
    );
    for file in skill_files {
        if read(&file).is_some_and(|text| has_legacy_metadata(&text)) {
            found.push(Leftover::new(Kind::SkillMetadata, file, None));
        }
    }
    for dir in skill_dirs {
        let target = renamed_dir(&dir);
        let blocked = target.exists().then_some(target);
        found.push(Leftover::new(Kind::SkillDir, dir, blocked));
    }
    let legacy_ci = root.join(LEGACY_CI_WORKFLOW);
    if legacy_ci.is_file() {
        let ci = root.join(CI_WORKFLOW);
        let blocked = ci.is_file().then_some(ci);
        found.push(Leftover::new(Kind::CiWorkflow, legacy_ci, blocked));
    }
    let (legacy_start, _) = names::HOOK_BLOCKS[1];
    for hook in HOOKS {
        let path = root.join(".git/hooks").join(hook);
        if read(&path).is_some_and(|text| text.contains(legacy_start)) {
            found.push(Leftover::new(Kind::HookBlock, path, None));
        }
    }
    found
}

/// Renames one leftover; a blocked one and a git hook block are left alone.
pub fn apply(root: &Path, leftover: &Leftover) -> Result<(), Error> {
    if leftover.blocked_by.is_some() {
        return Ok(());
    }
    match leftover.kind {
        Kind::ConfigFile => {
            let config = root.join(CONFIG);
            create_parent(&config)?;
            rename(&leftover.path, &config)
        }
        Kind::VersionKey => {
            let Some(config) = resolved_config(root) else {
                return Ok(());
            };
            let text = read_or_fail(&config)?;
            write(&config, &without_legacy_key(&text))
        }
        Kind::SkillMetadata => {
            let text = read_or_fail(&leftover.path)?;
            write(&leftover.path, &with_new_metadata(&text))
        }
        Kind::SkillDir => {
            let target = renamed_dir(&leftover.path);
            rename(&leftover.path, &target)?;
            let skill_md = target.join("SKILL.md");
            match read(&skill_md) {
                Some(text) => write(&skill_md, &with_new_skill_name(&text)),
                None => Ok(()),
            }
        }
        Kind::CiWorkflow => {
            let text = read_or_fail(&leftover.path)?;
            let ci = root.join(CI_WORKFLOW);
            write(&ci, &with_new_ci_names(&text))?;
            std::fs::remove_file(&leftover.path).map_err(|e| Error::io(&leftover.path, e))
        }
        Kind::HookBlock => Ok(()),
    }
}

fn resolved_config(root: &Path) -> Option<PathBuf> {
    names::CONFIG_CANDIDATES
        .iter()
        .map(|rel| root.join(rel))
        .find(|path| path.is_file())
}

fn renamed_dir(dir: &Path) -> PathBuf {
    dir.with_file_name(SKILL)
}

fn pins_legacy_key(text: &str) -> bool {
    text.lines()
        .any(|line| line.starts_with(&format!("{LEGACY_KEY}:")))
}

fn without_legacy_key(text: &str) -> String {
    let has_new = text
        .lines()
        .any(|line| line.starts_with(&format!("{KEY}:")));
    if has_new {
        return yaml_edit::remove_key_text(text, LEGACY_KEY).unwrap_or_else(|| text.to_string());
    }
    text.split_inclusive('\n')
        .map(|line| match line.strip_prefix(&format!("{LEGACY_KEY}:")) {
            Some(rest) => format!("{KEY}:{rest}"),
            None => line.to_string(),
        })
        .collect()
}

fn legacy_metadata_key(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let indent = &line[..line.len() - trimmed.len()];
    let rest = trimmed.strip_prefix(&format!("{LEGACY_SKILL}-"))?;
    METADATA_KEYS
        .iter()
        .any(|key| rest.starts_with(&format!("{key}:")))
        .then_some((indent, rest))
}

fn has_legacy_metadata(text: &str) -> bool {
    text.lines().any(|line| legacy_metadata_key(line).is_some())
}

fn with_new_metadata(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| match legacy_metadata_key(line) {
            Some((indent, rest)) => format!("{indent}{SKILL}-{rest}"),
            None => line.to_string(),
        })
        .collect()
}

fn with_new_skill_name(text: &str) -> String {
    text.split_inclusive('\n')
        .map(|line| {
            if line.trim_end() == format!("name: {LEGACY_SKILL}") {
                line.replacen(LEGACY_SKILL, SKILL, 1)
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn with_new_ci_names(text: &str) -> String {
    text.replace("agentsync check", "exuno check")
        .replace("agentsync sync", "exuno sync")
        .replace("AGENTSYNC_VERSION=", "EXUNO_VERSION=")
        .replace("yelmuratoff/agent_sync/", "yelmuratoff/exuno/")
        .replace("AgentSync", "Exuno")
}

/// Every `SKILL.md` below `dir`, and every directory named for the legacy
/// engine skill, both sorted; symlinks are not followed.
fn walk_skills(dir: &Path, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Ok(meta) = std::fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.is_dir() {
            if path.file_name().is_some_and(|name| name == LEGACY_SKILL) {
                dirs.push(path.clone());
            }
            walk_skills(&path, files, dirs);
        } else if meta.is_file() && path.file_name().is_some_and(|name| name == "SKILL.md") {
            files.push(path);
        }
    }
}

fn read(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn read_or_fail(path: &Path) -> Result<String, Error> {
    std::fs::read(path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| Error::io(path, e))
}

fn write(path: &Path, text: &str) -> Result<(), Error> {
    crate::engine::staging::write_beside(path, text.as_bytes())
}

fn rename(from: &Path, to: &Path) -> Result<(), Error> {
    std::fs::rename(from, to).map_err(|e| Error::io(from, e))
}

fn create_parent(path: &Path) -> Result<(), Error> {
    match path.parent() {
        Some(parent) => std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(root: &Path, rel: &str) -> String {
        std::fs::read_to_string(root.join(rel)).unwrap()
    }

    fn kinds(found: &[Leftover]) -> Vec<Kind> {
        found.iter().map(|leftover| leftover.kind).collect()
    }

    fn apply_all(root: &Path) {
        for leftover in scan(root) {
            apply(root, &leftover).unwrap();
        }
    }

    const SKILL: &str = "---\nname: agentsync\ndescription: Mine\nmetadata:\n  agentsync-use-when: Always\n  agentsync-not-for: Never\n---\n# Mine\n";

    #[test]
    fn a_fully_renamed_project_has_no_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/exuno.yaml", "exuno_version: \"1\"\n");
        write(
            dir.path(),
            ".ai/src/skills/exuno/SKILL.md",
            "---\nname: exuno\n---\n",
        );
        assert!(scan(dir.path()).is_empty());
    }

    #[test]
    fn every_old_name_is_found_and_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".ai/agent_sync.yaml",
            "agentsync_version: \"0.44.2\"\nformat: 2\n",
        );
        write(root, ".ai/src/skills/meta/agentsync/SKILL.md", SKILL);
        write(
            root,
            ".ai/src/skills/review/SKILL.md",
            "---\nname: review\nmetadata:\n  agentsync-requirements: A diff\n---\n",
        );
        write(
            root,
            ".github/workflows/agentsync-check.yml",
            "name: AgentSync\n      - name: Install AgentSync 0.44.2\n        run: curl -fsSL https://raw.githubusercontent.com/yelmuratoff/agent_sync/main/install.sh | AGENTSYNC_VERSION=0.44.2 bash\n        run: agentsync check\n",
        );
        write(
            root,
            ".git/hooks/post-merge",
            "#!/bin/sh\n# >>> AGENTSYNC AUTO SYNC START >>>\nagentsync sync\n# <<< AGENTSYNC AUTO SYNC END <<<\n",
        );

        let found = scan(root);
        assert_eq!(
            kinds(&found),
            [
                Kind::ConfigFile,
                Kind::VersionKey,
                Kind::SkillMetadata,
                Kind::SkillMetadata,
                Kind::SkillDir,
                Kind::CiWorkflow,
                Kind::HookBlock,
            ]
        );
        assert!(found.iter().all(|leftover| leftover.blocked_by.is_none()));
        assert_eq!(found[1].path, root.join(".ai/exuno.yaml"));

        apply_all(root);
        assert_eq!(
            read(root, ".ai/exuno.yaml"),
            "exuno_version: \"0.44.2\"\nformat: 2\n"
        );
        assert!(!root.join(".ai/agent_sync.yaml").exists());
        assert_eq!(
            read(root, ".ai/src/skills/meta/exuno/SKILL.md"),
            "---\nname: exuno\ndescription: Mine\nmetadata:\n  exuno-use-when: Always\n  exuno-not-for: Never\n---\n# Mine\n"
        );
        assert!(!root.join(".ai/src/skills/meta/agentsync").exists());
        assert!(
            read(root, ".ai/src/skills/review/SKILL.md")
                .contains("\n  exuno-requirements: A diff\n")
        );
        assert_eq!(
            read(root, ".github/workflows/exuno-check.yml"),
            "name: Exuno\n      - name: Install Exuno 0.44.2\n        run: curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | EXUNO_VERSION=0.44.2 bash\n        run: exuno check\n"
        );
        assert!(read(root, ".git/hooks/post-merge").contains("AGENTSYNC AUTO SYNC"));
        assert_eq!(kinds(&scan(root)), [Kind::HookBlock]);
    }

    #[test]
    fn a_root_config_moves_into_ai() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "agent_sync.yaml", "tools:\n  enabled: []\n");
        apply_all(dir.path());
        assert_eq!(
            read(dir.path(), ".ai/exuno.yaml"),
            "tools:\n  enabled: []\n"
        );
        assert!(!dir.path().join("agent_sync.yaml").exists());
    }

    #[test]
    fn a_new_name_already_taken_blocks_the_rename() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, ".ai/exuno.yaml", "tools:\n  enabled: []\n");
        write(root, ".ai/agent_sync.yaml", "tools:\n  enabled: [claude]\n");
        write(
            root,
            ".ai/src/skills/agentsync/SKILL.md",
            "---\nname: agentsync\n---\n",
        );
        write(
            root,
            ".ai/src/skills/exuno/SKILL.md",
            "---\nname: exuno\n---\n",
        );
        let found = scan(root);
        assert_eq!(kinds(&found), [Kind::ConfigFile, Kind::SkillDir]);
        assert_eq!(found[0].blocked_by, Some(root.join(".ai/exuno.yaml")));
        assert_eq!(found[1].blocked_by, Some(root.join(".ai/src/skills/exuno")));
        apply_all(root);
        assert_eq!(
            read(root, ".ai/agent_sync.yaml"),
            "tools:\n  enabled: [claude]\n"
        );
        assert!(root.join(".ai/src/skills/agentsync/SKILL.md").is_file());
    }

    #[test]
    fn both_version_keys_keep_the_new_one() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/exuno.yaml",
            "agentsync_version: \"0.1\"\nexuno_version: \"0.2\"\n",
        );
        apply_all(dir.path());
        assert_eq!(
            read(dir.path(), ".ai/exuno.yaml"),
            "exuno_version: \"0.2\"\n"
        );
    }
}
