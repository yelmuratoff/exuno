//! What a project still names `agentsync`, or pins to a release that reads
//! only those names, and the rename to `exuno` for each.

use std::path::{Path, PathBuf};

use crate::Error;
use crate::config::{names, yaml_edit};
use crate::engine::skill_tree::MAX_CATEGORY_DEPTH;
use crate::paths::DiskText;

use names::{
    CI_WORKFLOW, CONFIG, LEGACY_CI_WORKFLOW, LEGACY_CONFIGS, LEGACY_NAME as LEGACY_SKILL,
    LEGACY_VERSION_KEY as LEGACY_KEY, NAME as SKILL, VERSION_KEY as KEY,
};

const METADATA_KEYS: [&str; 3] = ["use-when", "not-for", "requirements"];
const HOOKS: [&str; 3] = ["pre-commit", "post-merge", "post-checkout"];
const FIRST_RELEASE: (u64, u64, u64) = (0, 45, 0);
const CI_PIN: &str = "EXUNO_VERSION=";
const CI_INSTALL_STEP: &str = "Install Exuno ";

/// The kind of thing that still carries the old name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    ConfigFile,
    VersionKey,
    OldPin,
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

    /// Whether `apply` fixes it: a blocked rename and a git hook block are the
    /// user's to fix.
    pub fn is_automatic(&self) -> bool {
        self.blocked_by.is_none() && self.kind != Kind::HookBlock
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
            Kind::OldPin => format!(
                "pin → {} in {path} — releases before 0.45.0 cannot read these names",
                crate::engine_version()
            ),
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
        && let Some(text) = read(&resolved)
        && pins_legacy_key(&text)
    {
        let edited = if moving { config } else { resolved };
        found.push(Leftover::new(Kind::VersionKey, edited.clone(), None));
        if predates_rename(&names::pinned_version(&text)) {
            found.push(Leftover::new(Kind::OldPin, edited, None));
        }
    }
    let mut skill_files = Vec::new();
    let mut skill_dirs = Vec::new();
    walk_skills(
        &root.join(".ai/src/skills"),
        0,
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
    let (legacy_start, _) = names::LEGACY_HOOK_BLOCK;
    for hook in HOOKS {
        let path = root.join(".git/hooks").join(hook);
        if read(&path).is_some_and(|text| text.contains(legacy_start)) {
            found.push(Leftover::new(Kind::HookBlock, path, None));
        }
    }
    found
}

/// Renames one leftover; one that is not automatic is left alone.
pub fn apply(root: &Path, leftover: &Leftover) -> Result<(), Error> {
    if !leftover.is_automatic() {
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
        Kind::OldPin => {
            let Some(config) = resolved_config(root) else {
                return Ok(());
            };
            let text = read_or_fail(&config)?;
            write(&config, &with_current_pin(&text))
        }
        Kind::SkillMetadata => {
            let text = read_or_fail(&leftover.path)?;
            write(&leftover.path, &with_new_metadata(&text))
        }
        Kind::SkillDir => {
            let skill_md = leftover.path.join("SKILL.md");
            if let Some(text) = read(&skill_md) {
                write(&skill_md, &with_new_skill_name(&text))?;
            }
            rename(&leftover.path, &renamed_dir(&leftover.path))
        }
        Kind::CiWorkflow => {
            let ci = root.join(CI_WORKFLOW);
            rename(&leftover.path, &ci)?;
            let text = read_or_fail(&ci)?;
            write(&ci, &with_current_ci_pin(&with_new_ci_names(&text)))
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

fn predates_rename(pin: &str) -> bool {
    let pin = pin.trim_matches(['"', '\'']).trim_start_matches('v');
    let mut parts = pin.split('.').map(|part| part.parse::<u64>().ok());
    match (parts.next(), parts.next(), parts.next()) {
        (Some(Some(major)), Some(Some(minor)), Some(Some(patch))) => {
            (major, minor, patch) < FIRST_RELEASE
        }
        _ => false,
    }
}

fn with_current_pin(text: &str) -> String {
    let engine = crate::engine_version();
    text.split_inclusive('\n')
        .map(|line| {
            match names::VERSION_KEYS
                .iter()
                .find(|key| line.starts_with(&format!("{key}:")))
            {
                Some(key) => format!("{key}: \"{engine}\"{}", line_end(line)),
                None => line.to_string(),
            }
        })
        .collect()
}

fn with_current_ci_pin(text: &str) -> String {
    let engine = crate::engine_version();
    let mut old: Vec<&str> = text
        .match_indices(CI_PIN)
        .filter_map(|(at, _)| text[at + CI_PIN.len()..].split_whitespace().next())
        .filter(|pin| predates_rename(pin))
        .collect();
    old.dedup();
    old.iter().fold(text.to_string(), |text, pin| {
        text.replace(&format!("{CI_PIN}{pin}"), &format!("{CI_PIN}{engine}"))
            .replace(
                &format!("{CI_INSTALL_STEP}{pin}"),
                &format!("{CI_INSTALL_STEP}{engine}"),
            )
    })
}

fn line_end(line: &str) -> &str {
    &line[line.trim_end().len()..]
}

fn with_new_skill_name(text: &str) -> String {
    let mut fences = 0;
    text.split_inclusive('\n')
        .map(|line| {
            if line.trim_end() == "---" && fences < 2 {
                fences += 1;
                return line.to_string();
            }
            let value = line
                .trim_end()
                .strip_prefix("name:")
                .map(|value| value.trim().trim_matches(['"', '\'']));
            if fences == 1 && value == Some(LEGACY_SKILL) {
                format!("name: {SKILL}{}", line_end(line))
            } else {
                line.to_string()
            }
        })
        .collect()
}

fn with_new_ci_names(text: &str) -> String {
    [
        ("agentsync check", "exuno check"),
        ("agentsync sync", "exuno sync"),
        ("AGENTSYNC_VERSION=", CI_PIN),
        ("yelmuratoff/agent_sync/", "yelmuratoff/exuno/"),
        ("AgentSync", "Exuno"),
    ]
    .iter()
    .fold(text.to_string(), |text, (from, to)| {
        replace_word(&text, from, to)
    })
}

fn replace_word(text: &str, from: &str, to: &str) -> String {
    let is_word = |c: char| c.is_alphanumeric() || c == '_' || c == '-';
    let starts_word = from.chars().next().is_some_and(is_word);
    let ends_word = from.chars().next_back().is_some_and(is_word);
    let mut out = String::with_capacity(text.len());
    let mut last = 0;
    for (at, _) in text.match_indices(from) {
        let end = at + from.len();
        let joined_before = starts_word && text[..at].chars().next_back().is_some_and(is_word);
        let joined_after = ends_word && text[end..].chars().next().is_some_and(is_word);
        if !joined_before && !joined_after {
            out.push_str(&text[last..at]);
            out.push_str(to);
            last = end;
        }
    }
    out.push_str(&text[last..]);
    out
}

/// Walks categories only, like `skill_tree::discover`, so no skill's own directory is renamed.
fn walk_skills(dir: &Path, depth: usize, files: &mut Vec<PathBuf>, dirs: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let is_dir = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !is_dir || name.starts_with('.') {
            continue;
        }
        let skill_md = path.join("SKILL.md");
        let is_skill = skill_md.is_file();
        if is_skill {
            files.push(skill_md);
        }
        let is_legacy = name == LEGACY_SKILL;
        if !is_skill && depth < MAX_CATEGORY_DEPTH {
            let nested_dirs = if is_legacy {
                &mut Vec::new()
            } else {
                &mut *dirs
            };
            walk_skills(&path, depth + 1, files, nested_dirs);
        }
        if is_legacy {
            dirs.push(path);
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
                Kind::OldPin,
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
        let engine = crate::engine_version();
        assert_eq!(
            read(root, ".ai/exuno.yaml"),
            format!("exuno_version: \"{engine}\"\nformat: 2\n")
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
            format!(
                "name: Exuno\n      - name: Install Exuno {engine}\n        run: curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | EXUNO_VERSION={engine} bash\n        run: exuno check\n"
            )
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
    fn a_skill_is_not_searched_for_legacy_directories() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".ai/src/skills/review/SKILL.md",
            "---\nname: review\n---\n",
        );
        write(root, ".ai/src/skills/review/refs/agentsync/x.md", "x\n");
        write(
            root,
            ".ai/src/skills/agentsync/SKILL.md",
            "---\nname: agentsync\n---\n",
        );
        write(root, ".ai/src/skills/agentsync/agentsync/y.md", "y\n");
        write(root, ".ai/src/skills/meta/agentsync/references/z.md", "z\n");

        let found = scan(root);
        let dirs: Vec<&Path> = found
            .iter()
            .map(|leftover| leftover.path.as_path())
            .collect();
        assert_eq!(
            dirs,
            [
                root.join(".ai/src/skills/agentsync").as_path(),
                root.join(".ai/src/skills/meta/agentsync").as_path(),
            ]
        );
        apply_all(root);
        assert!(
            root.join(".ai/src/skills/review/refs/agentsync/x.md")
                .is_file()
        );
        assert!(root.join(".ai/src/skills/exuno/agentsync/y.md").is_file());
        assert!(
            root.join(".ai/src/skills/meta/exuno/references/z.md")
                .is_file()
        );
    }

    #[test]
    fn skills_inside_a_legacy_directory_are_renamed_in_one_run() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".ai/src/skills/agentsync/deploy/SKILL.md",
            "---\nname: deploy\nmetadata:\n  agentsync-use-when: Shipping\n---\n",
        );
        write(
            root,
            ".ai/src/skills/agentsync/deploy/agentsync/x.md",
            "x\n",
        );
        assert_eq!(kinds(&scan(root)), [Kind::SkillMetadata, Kind::SkillDir]);
        apply_all(root);
        assert!(
            read(root, ".ai/src/skills/exuno/deploy/SKILL.md")
                .contains("\n  exuno-use-when: Shipping\n")
        );
        assert!(
            root.join(".ai/src/skills/exuno/deploy/agentsync/x.md")
                .is_file()
        );
        assert!(scan(root).is_empty());
    }

    #[test]
    fn a_pin_older_than_the_rename_moves_to_the_running_engine() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".ai/agent_sync.yaml",
            "agentsync_version: \"0.44.2\"\n",
        );
        write(
            root,
            ".github/workflows/agentsync-check.yml",
            "      - name: Install AgentSync 0.44.2\n        run: curl -fsSL x | AGENTSYNC_VERSION=0.44.2 bash\n",
        );
        assert!(kinds(&scan(root)).contains(&Kind::OldPin));

        apply_all(root);
        let engine = crate::engine_version();
        assert_eq!(
            read(root, ".ai/exuno.yaml"),
            format!("exuno_version: \"{engine}\"\n")
        );
        assert_eq!(
            read(root, ".github/workflows/exuno-check.yml"),
            format!(
                "      - name: Install Exuno {engine}\n        run: curl -fsSL x | EXUNO_VERSION={engine} bash\n"
            )
        );
        assert!(scan(root).is_empty());
    }

    #[test]
    fn a_pin_from_the_rename_on_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "agentsync_version: \"0.45.0\"\n",
        );
        assert!(!kinds(&scan(dir.path())).contains(&Kind::OldPin));
        apply_all(dir.path());
        assert_eq!(
            read(dir.path(), ".ai/exuno.yaml"),
            "exuno_version: \"0.45.0\"\n"
        );
    }

    #[test]
    fn the_ci_rewrite_leaves_other_words_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".github/workflows/agentsync-check.yml",
            "name: AgentSync\n        run: agentsync check\n        run: myagentsync check && notify AgentSyncBot acme/AgentSync-dashboard\n",
        );
        apply_all(root);
        assert_eq!(
            read(root, ".github/workflows/exuno-check.yml"),
            "name: Exuno\n        run: exuno check\n        run: myagentsync check && notify AgentSyncBot acme/AgentSync-dashboard\n"
        );
    }

    #[test]
    fn only_the_frontmatter_name_is_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            ".ai/src/skills/agentsync/SKILL.md",
            "---\nname: \"agentsync\"\n---\nExample:\n\nname: agentsync\n",
        );
        apply_all(root);
        assert_eq!(
            read(root, ".ai/src/skills/exuno/SKILL.md"),
            "---\nname: exuno\n---\nExample:\n\nname: agentsync\n"
        );
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
