//! The spellings the engine answers to while `agentsync` becomes `exuno`:
//! each list puts the new name first and keeps the old one readable until 1.0.

use crate::config::yaml_subset;

const ENV_PREFIXES: [&str; 2] = ["EXUNO_", "AGENTSYNC_"];

/// Project config files, in the order a project resolves them.
pub const CONFIG_CANDIDATES: [&str; 3] =
    [".ai/exuno.yaml", ".ai/agent_sync.yaml", "agent_sync.yaml"];

/// Keys that pin the engine version in a project config.
pub const VERSION_KEYS: [&str; 2] = ["exuno_version", "agentsync_version"];

/// Start and end markers of the auto-sync block in a git hook.
pub const HOOK_BLOCKS: [(&str, &str); 2] = [
    (
        "# >>> EXUNO AUTO SYNC START >>>",
        "# <<< EXUNO AUTO SYNC END <<<",
    ),
    (
        "# >>> AGENTSYNC AUTO SYNC START >>>",
        "# <<< AGENTSYNC AUTO SYNC END <<<",
    ),
];

/// Frontmatter key prefixes for a skill's card metadata.
pub const SKILL_METADATA_PREFIXES: [&str; 2] = ["metadata.exuno-", "metadata.agentsync-"];

/// `EXUNO_<suffix>`, else `AGENTSYNC_<suffix>`: the first one set wins, even
/// when empty.
pub fn env(suffix: &str, lookup: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    ENV_PREFIXES
        .iter()
        .find_map(|prefix| lookup(&format!("{prefix}{suffix}")))
}

/// The pinned engine version without its quotes, from the first
/// [`VERSION_KEYS`] entry present; empty when the config pins none.
pub fn pinned_version(config: &str) -> String {
    VERSION_KEYS
        .iter()
        .find_map(|key| yaml_subset::found(config, key))
        .unwrap_or_default()
        .replace('"', "")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(vars: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            vars.iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| value.to_string())
        }
    }

    #[test]
    fn env_prefers_new_spelling() {
        let vars = [("EXUNO_X", "new"), ("AGENTSYNC_X", "old")];
        assert_eq!(env("X", &lookup(&vars)).as_deref(), Some("new"));
    }

    #[test]
    fn env_falls_back_to_legacy() {
        let vars = [("AGENTSYNC_X", "old")];
        assert_eq!(env("X", &lookup(&vars)).as_deref(), Some("old"));
    }

    #[test]
    fn env_first_set_wins_even_empty() {
        let vars = [("EXUNO_X", ""), ("AGENTSYNC_X", "1")];
        assert_eq!(env("X", &lookup(&vars)).as_deref(), Some(""));
    }

    #[test]
    fn env_is_none_when_neither_is_set() {
        assert_eq!(env("X", &lookup(&[])), None);
    }

    #[test]
    fn pinned_version_prefers_exuno_key() {
        let config = "agentsync_version: \"0.1\"\nexuno_version: \"0.2\"\n";
        assert_eq!(pinned_version(config), "0.2");
    }

    #[test]
    fn pinned_version_reads_legacy_key() {
        assert_eq!(pinned_version("agentsync_version: \"0.1\"\n"), "0.1");
    }

    #[test]
    fn pinned_version_is_empty_without_a_pin() {
        assert_eq!(pinned_version("outputs: local\n"), "");
    }
}
