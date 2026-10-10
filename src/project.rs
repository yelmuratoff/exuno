//! The project being operated on: its root and `exuno.yaml`.

use crate::paths::DiskText;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::config::project_config::{self, Selection};
use crate::{Error, config::names, config::yaml_subset, paths};

#[derive(Debug)]
pub struct Project {
    pub root: PathBuf,
    pub config_path: Option<PathBuf>,
    tools_dir: PathBuf,
}

impl Project {
    /// `_list_prepare_context`: `AGENTSYNC_REPO_ROOT` when set, else the
    /// working directory, with `AGENTSYNC_CONFIG_PATH` authoritative.
    pub fn discover() -> Result<Self, Error> {
        let msystem = std::env::var("MSYSTEM").ok();
        let lookup = |name: &str| std::env::var(name).ok();
        let env_root = names::env("REPO_ROOT", &lookup)
            .filter(|root| !root.is_empty())
            .map(|root| paths::from_msys(&root, msystem.as_deref()));
        let cwd = std::env::current_dir().map_err(|e| Error::io(".", e))?;
        let pwd = std::env::var("PWD").ok();
        let root = paths::logical_root(env_root.as_deref(), &cwd, pwd.as_deref());
        let explicit = names::env("CONFIG_PATH", &lookup)
            .map(|path| paths::from_msys(&path, msystem.as_deref()));
        Self::select(root, explicit.as_deref())
    }

    /// Config is the first of [`names::CONFIG_CANDIDATES`] present.
    pub fn at(root: impl Into<PathBuf>) -> Result<Self, Error> {
        Self::select(root, None)
    }

    /// `tool_resolver_select_project_config`: an explicit path is authoritative.
    pub fn select(root: impl Into<PathBuf>, explicit: Option<&str>) -> Result<Self, Error> {
        let root = root.into();
        if !root.is_dir() {
            return Err(Error::ProjectRootNotFound(root));
        }
        let shown = root.disk_text();
        let is_file = |path: &str| Path::new(path).is_file();
        let config_path = match project_config::select(&shown, explicit, &is_file) {
            Selection::Found(path) => Some(PathBuf::from(path)),
            Selection::None => None,
            Selection::Missing(path) => return Err(Error::ConfigPathNotFound(PathBuf::from(path))),
        };
        let configured = match &config_path {
            Some(path) => {
                let text = std::fs::read(path).map_err(|e| Error::io(path, e))?;
                yaml_subset::value(&String::from_utf8_lossy(&text), "source.tools")
            }
            None => String::new(),
        };
        let tools_dir = if configured.is_empty() {
            root.join(".ai").join("src").join("tools")
        } else if crate::paths::is_absolute(&configured) {
            PathBuf::from(configured)
        } else {
            root.join(configured)
        };
        Ok(Self {
            root,
            config_path,
            tools_dir,
        })
    }

    /// `TOOL_RESOLVER_USER_DIR`: `source.tools` when the config sets it.
    pub fn user_tools_dir(&self) -> PathBuf {
        self.tools_dir.clone()
    }

    /// `tool_resolver_user_dir_in_project`.
    pub fn tools_dir_in_project(&self) -> bool {
        let paths = paths::Paths::on_disk(&self.root.disk_text());
        let abs = paths::normalize(&self.tools_dir.disk_text());
        paths
            .canonicalize_with_existing_ancestor(&abs)
            .is_some_and(|canonical| paths::is_within(&canonical, &paths.root_canonical))
    }

    pub fn user_tool_file(&self, slug: &str) -> PathBuf {
        self.user_tools_dir().join(format!("{slug}.yaml"))
    }

    pub fn shared_mcp_path(&self) -> PathBuf {
        self.root.join(".ai").join("src").join("mcp.json")
    }

    /// Slugs with a `.ai/src/tools/<slug>.yaml`, in byte order, `_`-prefixed skipped.
    pub fn user_override_tools(&self) -> Result<Vec<String>, Error> {
        let dir = self.user_tools_dir();
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(Error::io(dir, e)),
        };
        let mut slugs = Vec::new();
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&dir, e))?;
            let name = entry.file_name();
            let Some(stem) = name.to_str().and_then(|n| n.strip_suffix(".yaml")) else {
                continue;
            };
            if stem.starts_with('_') || !entry.path().is_file() {
                continue;
            }
            slugs.push(stem.to_string());
        }
        slugs.sort();
        slugs.dedup();
        Ok(slugs)
    }

    fn config_text(&self) -> Result<Option<String>, Error> {
        match &self.config_path {
            None => Ok(None),
            Some(path) => std::fs::read_to_string(path)
                .map(Some)
                .map_err(|e| Error::io(path, e)),
        }
    }

    /// `tools.enabled` from the project config.
    pub fn configured_enabled_tools(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .config_text()?
            .map(|text| yaml_subset::list(&text, "tools.enabled"))
            .unwrap_or_default())
    }

    /// Override files that still carry the pre-`tools.enabled` `enabled: true`.
    pub fn legacy_enabled_tools(&self) -> Result<Vec<String>, Error> {
        let mut enabled = Vec::new();
        for slug in self.user_override_tools()? {
            let path = self.user_tool_file(&slug);
            let text = std::fs::read_to_string(&path).map_err(|e| Error::io(&path, e))?;
            if yaml_subset::value(&text, "enabled") == "true" {
                enabled.push(slug);
            }
        }
        Ok(enabled)
    }

    /// `skills.hide` from the project config.
    pub fn hidden_skills(&self) -> Result<Vec<String>, Error> {
        Ok(self
            .config_text()?
            .map(|text| hidden_skills(&text))
            .unwrap_or_default())
    }

    /// Union of the configured and legacy enabled sets.
    pub fn enabled_tools(&self) -> Result<BTreeSet<String>, Error> {
        let mut set: BTreeSet<String> = self.configured_enabled_tools()?.into_iter().collect();
        set.extend(self.legacy_enabled_tools()?);
        Ok(set)
    }
}

/// `skills.hide` from the project config: names of personal skills the
/// project keeps tools from loading, as a list or one space-separated scalar.
pub fn hidden_skills(config: &str) -> Vec<String> {
    let scalar = yaml_subset::value(config, "skills.hide");
    if scalar.is_empty() || scalar.starts_with('[') {
        return yaml_subset::list(config, "skills.hide");
    }
    scalar.split_whitespace().map(str::to_string).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn hidden_skills_read_a_block_list_an_inline_list_or_a_scalar() {
        assert_eq!(
            hidden_skills("skills:\n  hide:\n    - bloc\n    - deploy\n"),
            ["bloc", "deploy"]
        );
        assert_eq!(
            hidden_skills("skills:\n  hide: [bloc, deploy]\n"),
            ["bloc", "deploy"]
        );
        assert_eq!(hidden_skills("skills:\n  hide: bloc\n"), ["bloc"]);
        assert!(hidden_skills("skills:\n  hide: []\n").is_empty());
        assert!(hidden_skills("tools:\n  enabled: [claude]\n").is_empty());
    }

    #[test]
    fn the_dot_ai_config_wins_over_a_root_level_one() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "agent_sync.yaml", "tools:\n  enabled: [zed]\n");
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(
            project.config_path,
            Some(dir.path().join(".ai/agent_sync.yaml"))
        );
        assert_eq!(project.configured_enabled_tools().unwrap(), ["claude"]);
    }

    #[test]
    fn a_project_without_config_or_overrides_is_empty_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.config_path, None);
        assert!(project.user_override_tools().unwrap().is_empty());
        assert!(project.enabled_tools().unwrap().is_empty());
    }

    #[test]
    fn a_missing_root_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let err = Project::at(dir.path().join("nope")).unwrap_err();
        assert!(matches!(err, Error::ProjectRootNotFound(_)));
    }

    #[test]
    fn override_tools_skip_the_template_and_non_yaml_files() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/zed.yaml", "name: Z\n");
        write(dir.path(), ".ai/src/tools/claude.yaml", "name: C\n");
        write(dir.path(), ".ai/src/tools/_TEMPLATE.yaml", "name: T\n");
        write(dir.path(), ".ai/src/tools/notes.md", "");
        write(dir.path(), ".ai/src/tools/claude/settings.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.user_override_tools().unwrap(), ["claude", "zed"]);
    }

    #[test]
    fn enabled_tools_union_the_config_list_and_legacy_flags() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled:\n    - claude\n    - zed\n",
        );
        write(dir.path(), ".ai/src/tools/zed.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/cursor.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/kimi.yaml", "enabled: false\n");
        let project = Project::at(dir.path()).unwrap();
        let set = project.enabled_tools().unwrap();
        let enabled: Vec<&str> = set.iter().map(String::as_str).collect();
        assert_eq!(enabled, ["claude", "cursor", "zed"]);
    }

    #[test]
    fn source_tools_moves_the_override_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "source:\n  tools: \"catalog\"\n",
        );
        write(dir.path(), "catalog/zed.yaml", "enabled: true\n");
        write(dir.path(), ".ai/src/tools/kimi.yaml", "enabled: true\n");
        let project = Project::at(dir.path()).unwrap();
        assert_eq!(project.user_tools_dir(), dir.path().join("catalog"));
        assert_eq!(project.legacy_enabled_tools().unwrap(), ["zed"]);
    }

    #[test]
    fn an_explicit_config_is_authoritative_and_a_missing_one_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        write(
            dir.path(),
            ".ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        write(dir.path(), "config/a.yaml", "tools:\n  enabled: [zed]\n");

        let project = Project::select(dir.path(), Some("config/a.yaml")).unwrap();
        assert_eq!(project.configured_enabled_tools().unwrap(), ["zed"]);

        let err = Project::select(dir.path(), Some("missing.yaml")).unwrap_err();
        assert_eq!(
            err.to_string(),
            format!(
                // The engine spells a disk path its own way, `/`-separated
                // even on Windows, so the expectation goes through `disk_text`.
                "EXUNO_CONFIG_PATH is set but file not found: {}/missing.yaml",
                dir.path().disk_text()
            )
        );
    }
}
