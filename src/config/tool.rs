//! Layered tool config: user override → shipped base → the `base:` tool's
//! project override → its shipped base, resolved per field as
//! `get_tool_value_r` in `lib/helpers/tool_resolver.sh` did for the shipped layers.

use include_dir::File;

use crate::engine::filters::Filter;
use crate::{Error, config::catalog, config::yaml_subset, project::Project};

/// Keys a variant never takes from its base tool's project override. A variant
/// owns its identity and its paths: inheriting a project-level `dest` would
/// make two tools write one file, and a `post_sync` would run twice.
const NOT_FROM_BASE_OVERRIDE: [&str; 8] = [
    "name",
    "base",
    "profile_home",
    "enabled",
    "post_sync",
    "targets.*.dest",
    "targets.*.source",
    "targets.*.user_dirs",
];

fn inherits_from_base_override(key_path: &str) -> bool {
    let segments: Vec<&str> = key_path.split('.').collect();
    !NOT_FROM_BASE_OVERRIDE.iter().any(|pattern| {
        let parts: Vec<&str> = pattern.split('.').collect();
        parts.len() == segments.len()
            && parts
                .iter()
                .zip(&segments)
                .all(|(part, segment)| *part == "*" || part == segment)
    })
}

pub struct Tool {
    pub slug: String,
    user_yaml: Option<String>,
    base_yaml: Option<&'static str>,
    base_override: Option<String>,
}

impl Tool {
    pub fn load(project: &Project, slug: &str) -> Result<Self, Error> {
        let read = |slug: &str| -> Result<Option<String>, Error> {
            let path = project.user_tool_file(slug);
            if !path.is_file() {
                return Ok(None);
            }
            std::fs::read_to_string(&path)
                .map(Some)
                .map_err(|e| Error::io(&path, e))
        };
        let tool = Self::from_parts(slug, read(slug)?, catalog::base_tool_yaml(slug));
        let base = tool.base_name();
        if base.is_empty() || base == slug {
            return Ok(tool);
        }
        Ok(tool.with_base_override(read(&base)?))
    }

    /// A tool whose override text was read by the caller, with its shipped base.
    pub fn new(slug: &str, user_yaml: Option<String>) -> Self {
        Self::from_parts(slug, user_yaml, catalog::base_tool_yaml(slug))
    }

    fn from_parts(slug: &str, user_yaml: Option<String>, base_yaml: Option<&'static str>) -> Self {
        Self {
            slug: slug.to_string(),
            user_yaml,
            base_yaml,
            base_override: None,
        }
    }

    /// This tool with the project's `.ai/src/tools/<base>.yaml` text as the
    /// layer between its own files and the `base:` tool's shipped one.
    pub fn with_base_override(mut self, text: Option<String>) -> Self {
        self.base_override = text;
        self
    }

    fn base_override_found(&self, key_path: &str) -> Option<String> {
        if !inherits_from_base_override(key_path) {
            return None;
        }
        yaml_subset::found(self.base_override.as_deref()?, key_path)
    }

    /// `base:` from the user file: the slug a profile variant inherits from.
    pub fn base_name(&self) -> String {
        self.user_yaml
            .as_deref()
            .map(|text| yaml_subset::value(text, "base"))
            .unwrap_or_default()
    }

    /// Effective scalar for a dotted key. A key the user file writes wins, even
    /// empty; the shipped file answers next; what both leave empty comes from
    /// the `base:` tool, never for `base` or `name`: its project override first
    /// (except the [`NOT_FROM_BASE_OVERRIDE`] keys), then its shipped file.
    pub fn value(&self, key_path: &str) -> String {
        if let Some(found) = self
            .user_yaml
            .as_deref()
            .and_then(|user| yaml_subset::found(user, key_path))
        {
            return found;
        }
        let shipped = self
            .base_yaml
            .map(|base| yaml_subset::value(base, key_path))
            .unwrap_or_default();
        if !shipped.is_empty() || key_path == "base" || key_path == "name" {
            return shipped;
        }
        if let Some(found) = self.base_override_found(key_path) {
            return found;
        }
        catalog::base_tool_yaml(&self.base_name())
            .map(|text| yaml_subset::value(text, key_path))
            .unwrap_or_default()
    }

    /// A scalar from `.ai/src/tools/<slug>.yaml` alone, as the legacy
    /// `enabled: true` lookup reads it.
    pub fn user_value(&self, key_path: &str) -> String {
        self.user_yaml
            .as_deref()
            .map(|text| yaml_subset::value(text, key_path))
            .unwrap_or_default()
    }

    /// `get_tool_bool`:`Some` for the true and false spellings, `None` otherwise.
    pub fn flag(&self, key_path: &str) -> Option<bool> {
        match self.value(key_path).to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" | "on" => Some(true),
            "false" | "no" | "0" | "off" => Some(false),
            _ => None,
        }
    }

    /// `targets.<target>.include` and `.exclude`, each layered as `filter` reads it.
    pub fn target_filter(&self, target: &str) -> Filter {
        Filter::new(
            &self.filter(&format!("targets.{target}.include")),
            &self.filter(&format!("targets.{target}.exclude")),
        )
    }

    /// `get_tool_filter`: an include/exclude list as one space-joined string,
    /// layered like `value` but per file, from a scalar, `[a, b]`, or a block list.
    pub fn filter(&self, key_path: &str) -> String {
        if let Some(user) = &self.user_yaml
            && yaml_subset::found(user, key_path).is_some()
        {
            return read_filter(user, key_path);
        }
        let shipped = self
            .base_yaml
            .map(|base| read_filter(base, key_path))
            .unwrap_or_default();
        if !shipped.is_empty() {
            return shipped;
        }
        if self.base_override_found(key_path).is_some()
            && let Some(text) = &self.base_override
        {
            return read_filter(text, key_path);
        }
        catalog::base_tool_yaml(&self.base_name())
            .map(|text| read_filter(text, key_path))
            .unwrap_or_default()
    }

    /// Whether the settings file is composed with the MCP source into one file.
    pub fn composed(&self) -> bool {
        matches!(
            self.value("targets.mcp.format").as_str(),
            "codex_toml" | "opencode_json"
        )
    }

    /// Whether sync owns only the declared keys of the TOML or JSON file the
    /// `settings` or `mcp` target writes: `targets.<resource>.ownership` `keys`,
    /// or `auto` (the default) in a config home, the project rooted at `$HOME`
    /// or a profile variant. A file composed from both takes the settings
    /// target's ownership.
    pub fn keyed(&self, resource: &str, root_is_home: bool) -> bool {
        let dest = self.value(&format!("targets.{resource}.dest"));
        if !(dest.ends_with(".toml") || dest.ends_with(".json")) {
            return false;
        }
        let owner = if self.composed() {
            "settings"
        } else {
            resource
        };
        match self.value(&format!("targets.{owner}.ownership")).as_str() {
            "keys" => true,
            "" | "auto" => root_is_home || !self.value("profile_home").is_empty(),
            _ => false,
        }
    }

    pub fn display_name(&self) -> String {
        let name = self.value("name");
        if name.is_empty() {
            self.slug.clone()
        } else {
            name
        }
    }

    /// Shipped payload template for this tool, or for its `base:` tool.
    pub fn base_payload(&self, resource: &str) -> Option<&'static File<'static>> {
        catalog::base_payload(resource, &self.slug).or_else(|| {
            let base_tool = self.base_name();
            if base_tool.is_empty() {
                None
            } else {
                catalog::base_payload(resource, &base_tool)
            }
        })
    }
}

/// `_read_filter_file`.
fn read_filter(text: &str, key_path: &str) -> String {
    let scalar = yaml_subset::value(text, key_path);
    if !scalar.is_empty() && !(scalar.starts_with('[') && scalar.ends_with(']')) {
        return scalar;
    }
    yaml_subset::list(text, key_path).join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_and_mcp_files_are_keyed_in_a_config_home_or_when_asked() {
        let codex = Tool::new("codex", None);
        assert!(codex.keyed("settings", true));
        assert!(codex.keyed("mcp", true));
        assert!(!codex.keyed("settings", false));
        let variant = Tool::new(
            "claude-hub",
            Some("base: claude\nprofile_home: \"/home/me\"\n".into()),
        );
        assert!(variant.keyed("settings", false));
        let keys = "targets:\n  settings:\n    ownership: keys\n";
        assert!(Tool::new("claude", Some(keys.into())).keyed("settings", false));
        assert!(!Tool::new("claude", Some(keys.into())).keyed("mcp", false));
        let file = "targets:\n  settings:\n    ownership: file\n";
        assert!(!Tool::new("codex", Some(file.into())).keyed("mcp", true));
        assert!(Tool::new("cursor", None).keyed("mcp", true));
        assert!(!Tool::new("cursor", None).keyed("rules", true));
    }

    #[test]
    fn every_shipped_settings_and_mcp_file_but_zed_settings_is_keyed_in_a_home() {
        for slug in crate::config::catalog::base_tools() {
            let tool = Tool::new(&slug, None);
            for resource in ["settings", "mcp"] {
                if tool.value(&format!("targets.{resource}.dest")).is_empty() {
                    continue;
                }
                let expected = !(slug == "zed" && resource == "settings");
                assert_eq!(tool.keyed(resource, true), expected, "{slug} {resource}");
                assert!(!tool.keyed(resource, false), "{slug} {resource}");
            }
        }
    }

    fn claude_with(user: &str) -> Tool {
        Tool::from_parts(
            "claude",
            Some(user.to_string()),
            catalog::base_tool_yaml("claude"),
        )
    }

    #[test]
    fn a_non_empty_user_value_wins() {
        assert_eq!(claude_with("name: \"Mine\"\n").display_name(), "Mine");
    }

    #[test]
    fn an_explicit_empty_user_value_wins_over_the_base() {
        let tool = claude_with("targets:\n  rules:\n    dest: \"\"\n");
        assert_eq!(tool.value("targets.rules.dest"), "");
        assert_eq!(tool.value("targets.skills.dest"), ".claude/skills");
    }

    #[test]
    fn a_declared_base_fills_what_the_shipped_file_leaves_empty() {
        let tool = claude_with("base: cursor\n");
        assert_eq!(tool.value("targets.rules.extension"), ".mdc");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
        assert_eq!(tool.display_name(), "Claude Code");
    }

    #[test]
    fn an_explicit_empty_user_filter_wins_over_the_base() {
        let tool = Tool::from_parts(
            "x",
            Some("targets:\n  skills:\n    exclude: []\n".to_string()),
            Some("targets:\n  skills:\n    exclude: [a]\n"),
        );
        assert_eq!(tool.filter("targets.skills.exclude"), "");
        assert_eq!(
            Tool::from_parts("x", None, Some("targets:\n  skills:\n    exclude: [a]\n"))
                .filter("targets.skills.exclude"),
            "a"
        );
    }

    #[test]
    fn a_variant_inherits_from_its_base_tool_but_keeps_its_own_identity() {
        let tool = Tool::from_parts(
            "claude-hub",
            Some("base: claude\nprofile_home: \".claude-hub\"\n".to_string()),
            None,
        );
        assert_eq!(tool.base_name(), "claude");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
        assert_eq!(tool.display_name(), "claude-hub");
        let payload = tool
            .base_payload("settings")
            .expect("inherits claude's settings");
        assert!(payload.path().ends_with("claude.json"));
    }

    #[test]
    fn a_variant_takes_its_base_override_but_never_its_paths_or_identity() {
        let tool = Tool::from_parts(
            "claude-hub",
            Some("base: claude\nprofile_home: \".claude-hub\"\n".to_string()),
            None,
        )
        .with_base_override(Some(
            "name: \"Mine\"\nenabled: true\npost_sync: \"x\"\ntargets:\n  rules:\n    extension: \".x.md\"\n    dest: \".claude/custom\"\n    exclude: [secret.md]\n  agents:\n    source: \"o.md\"\n  skills:\n    user_dirs: \".claude/skills\"\n  manifests:\n    vars:\n      name: \"n\"\n".to_string(),
        ));
        assert_eq!(tool.value("targets.rules.extension"), ".x.md");
        assert_eq!(tool.filter("targets.rules.exclude"), "secret.md");
        assert_eq!(tool.value("targets.manifests.vars.name"), "n");
        assert_eq!(tool.value("targets.rules.dest"), ".claude/rules");
        assert_eq!(tool.value("targets.agents.source"), "");
        assert_eq!(tool.value("targets.skills.user_dirs"), ".claude/skills");
        assert_eq!(tool.value("post_sync"), "");
        assert_eq!(tool.value("enabled"), "false");
        assert_eq!(tool.display_name(), "claude-hub");
        assert_eq!(tool.value("profile_home"), ".claude-hub");
    }

    #[test]
    fn flags_accept_the_bash_spellings_case_insensitively() {
        let tool =
            claude_with("targets:\n  rules:\n    enabled: OFF\n  skills:\n    enabled: maybe\n");
        assert_eq!(tool.flag("targets.rules.enabled"), Some(false));
        assert_eq!(tool.flag("targets.skills.enabled"), None);
    }

    #[test]
    fn filters_join_scalar_inline_and_block_forms() {
        let block = claude_with("targets:\n  skills:\n    exclude:\n      - a\n      - \"b*\"\n");
        assert_eq!(block.filter("targets.skills.exclude"), "a b*");
        let inline = claude_with("targets:\n  skills:\n    exclude: [a, b]\n");
        assert_eq!(inline.filter("targets.skills.exclude"), "a b");
        let scalar = claude_with("targets:\n  skills:\n    exclude: \"a b\"\n");
        assert_eq!(scalar.filter("targets.skills.exclude"), "a b");
        assert_eq!(claude_with("").filter("targets.rules.include"), "");
    }

    #[test]
    fn an_unknown_tool_reads_as_empty_and_shows_its_slug() {
        let tool = Tool::from_parts("nope", None, None);
        assert_eq!(tool.value("targets.rules.dest"), "");
        assert_eq!(tool.display_name(), "nope");
        assert!(tool.base_payload("mcp").is_none());
    }
}
