//! `lib/helpers/project_config.sh`: which `agent_sync.yaml` a project uses.

use crate::config::{names, yaml_subset};

/// What `project_config_path_r` answered.
#[derive(Debug, PartialEq, Eq)]
pub enum Selection {
    /// The config file to read.
    Found(String),
    /// No explicit path, and neither `.ai/agent_sync.yaml` nor `agent_sync.yaml`.
    None,
    /// `AGENTSYNC_CONFIG_PATH` names this path, which is not a regular file.
    Missing(String),
}

/// `project_config_path_r`: an explicit path, relative to `root` unless
/// absolute, is authoritative and never falls back; otherwise
/// `.ai/agent_sync.yaml`, then `agent_sync.yaml`. `is_file` answers `[[ -f ]]`.
pub fn select(root: &str, explicit: Option<&str>, is_file: &dyn Fn(&str) -> bool) -> Selection {
    if let Some(raw) = explicit.filter(|raw| !raw.is_empty()) {
        let path = if crate::paths::is_absolute(raw) {
            raw.to_string()
        } else {
            format!("{root}/{raw}")
        };
        return if is_file(&path) {
            Selection::Found(path)
        } else {
            Selection::Missing(path)
        };
    }
    names::CONFIG_CANDIDATES
        .iter()
        .map(|rel| format!("{root}/{rel}"))
        .find(|path| is_file(path))
        .map_or(Selection::None, Selection::Found)
}

/// The sentence every command prints for [`Selection::Missing`].
pub fn missing_message(path: &str) -> String {
    format!("AGENTSYNC_CONFIG_PATH is set but file not found: {path}")
}

/// Where a config keeps its generated files: `outputs:` when it says
/// `committed` or `local`, else `committed` when `gitignore.update` is `false`,
/// else `local`. `Err` holds an `outputs:` value that is neither.
pub fn outputs_mode(config: &str) -> Result<&'static str, String> {
    match yaml_subset::value(config, "outputs")
        .replace('"', "")
        .as_str()
    {
        "committed" => Ok("committed"),
        "local" => Ok("local"),
        "" if yaml_subset::value(config, "gitignore.update") == "false" => Ok("committed"),
        "" => Ok("local"),
        other => Err(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outputs_mode_falls_back_to_the_gitignore_setting() {
        assert_eq!(outputs_mode("outputs: \"committed\"\n"), Ok("committed"));
        assert_eq!(
            outputs_mode("outputs: local\ngitignore:\n  update: false\n"),
            Ok("local")
        );
        assert_eq!(
            outputs_mode("gitignore:\n  update: false\n"),
            Ok("committed")
        );
        assert_eq!(outputs_mode("tools: []\n"), Ok("local"));
        assert_eq!(outputs_mode("outputs: shared\n"), Err("shared".to_string()));
    }

    fn probe(files: &'static [&'static str]) -> impl Fn(&str) -> bool {
        move |path: &str| files.contains(&path)
    }

    #[test]
    fn without_an_explicit_path_the_dot_ai_config_wins_then_the_root_one() {
        assert_eq!(select("/q", None, &probe(&[])), Selection::None);
        assert_eq!(
            select("/q", None, &probe(&["/q/agent_sync.yaml"])),
            Selection::Found("/q/agent_sync.yaml".into())
        );
        assert_eq!(
            select(
                "/q",
                None,
                &probe(&["/q/agent_sync.yaml", "/q/.ai/agent_sync.yaml"])
            ),
            Selection::Found("/q/.ai/agent_sync.yaml".into())
        );
    }

    #[test]
    fn the_exuno_config_wins_over_both_legacy_names() {
        let files = probe(&[
            "/q/agent_sync.yaml",
            "/q/.ai/agent_sync.yaml",
            "/q/.ai/exuno.yaml",
        ]);
        assert_eq!(
            select("/q", None, &files),
            Selection::Found("/q/.ai/exuno.yaml".into())
        );
    }

    #[test]
    fn an_explicit_path_is_relative_to_the_root_unless_absolute() {
        let files = probe(&["/q/config/a.yaml", "/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some("config/a.yaml"), &files),
            Selection::Found("/q/config/a.yaml".into())
        );
        assert_eq!(
            select("/q", Some("/q/config/a.yaml"), &files),
            Selection::Found("/q/config/a.yaml".into())
        );
    }

    #[test]
    fn a_missing_explicit_path_never_falls_back() {
        let files = probe(&["/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some("config/none.yaml"), &files),
            Selection::Missing("/q/config/none.yaml".into())
        );
        assert_eq!(
            select("/q", Some("dir.yaml"), &files),
            Selection::Missing("/q/dir.yaml".into())
        );
    }

    #[test]
    fn an_empty_explicit_path_is_unset() {
        let files = probe(&["/q/.ai/agent_sync.yaml"]);
        assert_eq!(
            select("/q", Some(""), &files),
            Selection::Found("/q/.ai/agent_sync.yaml".into())
        );
    }

    #[test]
    fn the_missing_message_is_the_bash_sentence() {
        assert_eq!(
            missing_message("/q/missing.yaml"),
            "AGENTSYNC_CONFIG_PATH is set but file not found: /q/missing.yaml"
        );
    }
}
