//! `profiles:` in `exuno.yaml`, read as `lib/helpers/profiles.sh` reads it.

use crate::config::yaml_subset;

/// `_profiles_names`: the child keys of the root `profiles:` mapping.
pub fn names(config: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_section = false;
    let mut child_indent: Option<usize> = None;
    for line in config.lines() {
        if line.is_empty() {
            continue;
        }
        let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if stripped.starts_with('#') {
            continue;
        }
        let indent = line.len() - stripped.len();
        let key = stripped.split_once(':').and_then(|(key, _)| {
            (!key.is_empty()
                && key
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-'))
            .then_some(key)
        });
        if !in_section {
            if indent == 0 && key == Some("profiles") {
                in_section = true;
            }
            continue;
        }
        if indent == 0 {
            break;
        }
        let Some(key) = key else {
            continue;
        };
        let expected = *child_indent.get_or_insert(indent);
        if indent == expected {
            names.push(key.to_string());
        }
    }
    names
}

/// `profile_overlay_dir`: `profiles.<name>.overlay`, else `.ai/profiles/<name>`.
pub fn overlay_dir(config: &str, name: &str) -> String {
    let value = yaml_subset::value(config, &format!("profiles.{name}.overlay"));
    if value.is_empty() {
        format!(".ai/profiles/{name}")
    } else {
        value
    }
}

/// `profile_tools`.
pub fn tools(config: &str, name: &str) -> Vec<String> {
    yaml_subset::list(config, &format!("profiles.{name}.tools"))
}

/// `profile_is_active`.
pub fn is_active(config: &str, name: &str) -> bool {
    matches!(
        yaml_subset::value(config, &format!("profiles.{name}.active"))
            .to_ascii_lowercase()
            .as_str(),
        "true" | "yes" | "1" | "on"
    )
}

/// `list_profile_tools`: every profile's tools, sorted and deduplicated.
pub fn all_tools(config: &str) -> Vec<String> {
    let mut all: Vec<String> = names(config)
        .iter()
        .flat_map(|name| tools(config, name))
        .collect();
    all.sort();
    all.dedup();
    all
}

/// `profile_rewrite_dest`: drop the leading tool directory when there is one
/// and re-root under the config home.
pub fn rewrite_dest(base_dest: &str, home: &str) -> String {
    let rel = base_dest
        .split_once('/')
        .map_or(base_dest, |(_, rest)| rest);
    format!("{home}/{rel}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: &str = "tools:\n  enabled: [claude]\nprofiles:\n  # personal\n  hub:\n    overlay: \".ai/profiles/hub\"\n    active: true\n    tools: [claude-hub, codex-hub]\n  work:\n    active: no\n    tools:\n      - claude-work\n      - claude-hub\noutputs: local\n";

    #[test]
    fn profile_names_are_the_direct_children_of_profiles() {
        assert_eq!(names(CONFIG), ["hub", "work"]);
        assert!(names("tools:\n  enabled: []\n").is_empty());
    }

    #[test]
    fn profile_fields_read_with_defaults() {
        assert!(is_active(CONFIG, "hub"));
        assert!(!is_active(CONFIG, "work"));
        assert_eq!(overlay_dir(CONFIG, "work"), ".ai/profiles/work");
        assert_eq!(tools(CONFIG, "work"), ["claude-work", "claude-hub"]);
        assert_eq!(
            all_tools(CONFIG),
            ["claude-hub", "claude-work", "codex-hub"]
        );
    }

    #[test]
    fn a_dest_moves_under_the_config_home_without_its_tool_directory() {
        assert_eq!(rewrite_dest(".claude/rules", ".h"), ".h/rules");
        assert_eq!(rewrite_dest(".amazonq/rules/x.md", ".h"), ".h/rules/x.md");
        assert_eq!(rewrite_dest("CLAUDE.md", ".h"), ".h/CLAUDE.md");
        assert_eq!(rewrite_dest(".mcp.json", ".h"), ".h/.mcp.json");
    }
}
