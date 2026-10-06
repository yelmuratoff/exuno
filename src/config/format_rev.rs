//! `lib/helpers/format.sh`: the project format revision, a counter bumped only
//! when a project needs a migration step.

use std::path::{Path, PathBuf};

use crate::config::{names, yaml_subset};

const ENGINE_FORMAT_FILE: &str = include_str!("../../FORMAT");

fn revision(text: &str) -> u32 {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return 1;
    }
    text.parse().unwrap_or(1)
}

/// `engine_format`: the first line of `FORMAT`, 1 when it is not a number.
pub fn engine() -> u32 {
    revision(ENGINE_FORMAT_FILE.split('\n').next().unwrap_or_default())
}

/// `project_format`: `format:` without its quotes, 1 when absent or not a number.
pub fn project(config: &str) -> u32 {
    revision(&yaml_subset::value(config, "format").replace('"', ""))
}

/// `format_pending_notes`: one line per migration between the two revisions.
pub fn pending_notes(from: u32, to: u32) -> Vec<String> {
    (from.saturating_add(1)..=to)
        .map(|step| match step {
            2 => "r2  The agentsync skill is engine-owned now. A copy under .ai/src/skills/agentsync/ shadows it, so engine upgrades never reach your agents.".to_string(),
            _ => format!("r{step}  See CHANGELOG.md for what changed."),
        })
        .collect()
}

/// `format_config_path`: `.ai/agent_sync.yaml`, else `agent_sync.yaml`, when a file.
pub fn config_path(project_dir: &Path) -> Option<PathBuf> {
    names::CONFIG_CANDIDATES
        .iter()
        .map(|rel| project_dir.join(rel))
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_notes_name_each_migration_step() {
        assert_eq!(
            pending_notes(1, 3),
            [
                "r2  The agentsync skill is engine-owned now. A copy under .ai/src/skills/agentsync/ shadows it, so engine upgrades never reach your agents.",
                "r3  See CHANGELOG.md for what changed.",
            ]
        );
        assert!(pending_notes(2, 2).is_empty());
        assert!(pending_notes(3, 2).is_empty());
    }

    #[test]
    fn the_config_path_prefers_the_ai_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(config_path(dir.path()), None);
        std::fs::write(dir.path().join("agent_sync.yaml"), "").unwrap();
        assert_eq!(
            config_path(dir.path()),
            Some(dir.path().join("agent_sync.yaml"))
        );
        std::fs::create_dir(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "").unwrap();
        assert_eq!(
            config_path(dir.path()),
            Some(dir.path().join(".ai/agent_sync.yaml"))
        );
        std::fs::write(dir.path().join(".ai/exuno.yaml"), "").unwrap();
        assert_eq!(
            config_path(dir.path()),
            Some(dir.path().join(".ai/exuno.yaml"))
        );
    }

    #[test]
    fn revisions_read_as_format_sh_reads_them() {
        assert_eq!(engine(), 2);
        assert_eq!(project("format: 2\n"), 2);
        assert_eq!(project("format: \"3\"\n"), 3);
        assert_eq!(project("tools:\n  enabled: []\n"), 1);
        assert_eq!(project("format: two\n"), 1);
        assert_eq!(project("format: -2\n"), 1);
    }
}
