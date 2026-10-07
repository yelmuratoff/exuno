//! Where a tool's settings, mcp, or hooks override lives, mirroring the lookups
//! in `lib/helpers/tool_resolver.sh` that `list` reports on.

use crate::paths::DiskText;
use std::path::{Path, PathBuf};

use include_dir::File;

use crate::engine::session::Session;
use crate::output::style::Style;
use crate::paths::{self, ENGINE_ROOT};
use crate::{Error, config::tool::Tool, project::Project};

/// `.ai/src/tools/<slug>/<resource>.*`, first by name: the layout since 0.11.
pub fn find_new_override(
    project: &Project,
    slug: &str,
    resource: &str,
) -> Result<Option<PathBuf>, Error> {
    let dir = project.user_tools_dir().join(slug);
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
            ) =>
        {
            return Ok(None);
        }
        Err(e) => return Err(Error::io(dir, e)),
    };
    let prefix = format!("{resource}.");
    let mut matches = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(&dir, e))?;
        let is_match = entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.starts_with(&prefix));
        if is_match && entry.path().is_file() {
            matches.push(entry.path());
        }
    }
    matches.sort();
    Ok(matches.into_iter().next())
}

/// `.ai/src/<resource>/<slug>.<ext>` with the shipped payload's extension: the
/// pre-0.11 flat layout. `None` when no shipped payload fixes an extension.
pub fn legacy_override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf> {
    let ext = tool
        .base_payload(resource)?
        .path()
        .extension()?
        .to_str()?
        .to_string();
    Some(
        project
            .root
            .join(".ai")
            .join("src")
            .join(resource)
            .join(format!("{}.{ext}", tool.slug)),
    )
}

/// `_payload_override_path`: the canonical write path under the tool override
/// directory, with the shipped payload's extension.
pub fn override_path(project: &Project, tool: &Tool, resource: &str) -> Option<PathBuf> {
    let ext = tool
        .base_payload(resource)?
        .path()
        .extension()?
        .to_str()?
        .to_string();
    Some(
        project
            .user_tools_dir()
            .join(&tool.slug)
            .join(format!("{resource}.{ext}")),
    )
}

/// A payload as Bash names it: a file on disk, or a shipped template under the
/// virtual engine root.
pub enum Source {
    Disk(PathBuf),
    Shipped(&'static File<'static>),
}

impl Source {
    pub fn shown(&self) -> String {
        match self {
            Source::Disk(path) => path.disk_text(),
            Source::Shipped(file) => {
                format!("{ENGINE_ROOT}/lib/templates/{}", file.path().disk_text())
            }
        }
    }

    pub fn bytes(&self) -> Result<Vec<u8>, Error> {
        match self {
            Source::Disk(path) => std::fs::read(path).map_err(|e| Error::io(path, e)),
            Source::Shipped(file) => Ok(file.contents().to_vec()),
        }
    }
}

/// `_find_base_payload`.
pub fn base_source(tool: &Tool, resource: &str) -> Option<Source> {
    tool.base_payload(resource).map(Source::Shipped)
}

/// `resolve_payload_source` for a CLI command: per-tool override, declared
/// `targets.<resource>.source`, legacy flat layout, shared `mcp.json`, base.
/// The second value is the path `_warn_legacy_payload_path` names.
pub fn effective_source(
    project: &Project,
    tool: &Tool,
    resource: &str,
) -> Result<(Option<Source>, Option<PathBuf>), Error> {
    if let Some(path) = find_new_override(project, &tool.slug, resource)? {
        return Ok((Some(Source::Disk(path)), None));
    }
    let declared = tool.value(&format!("targets.{resource}.source"));
    if !declared.is_empty() {
        let abs = if crate::paths::is_absolute(&declared) {
            PathBuf::from(&declared)
        } else {
            project.root.join(&declared)
        };
        if abs.is_file() {
            let legacy = ["hooks", "mcp", "settings"]
                .iter()
                .any(|kind| declared.starts_with(&format!(".ai/src/{kind}/")));
            let warn = legacy.then(|| abs.clone());
            return Ok((Some(Source::Disk(abs)), warn));
        }
    }
    if let Some(legacy) = legacy_override_path(project, tool, resource).filter(|p| p.is_file()) {
        return Ok((Some(Source::Disk(legacy.clone())), Some(legacy)));
    }
    if resource == "mcp" && project.shared_mcp_path().is_file() {
        return Ok((Some(Source::Disk(project.shared_mcp_path())), None));
    }
    Ok((base_source(tool, resource), None))
}

/// `_warn_legacy_payload_path`.
pub fn legacy_warning(project: &Project, path: &Path, style: &Style) -> String {
    let text = path.disk_text();
    let root = format!("{}/", project.root.disk_text());
    let rel = text.strip_prefix(&root).unwrap_or(&text);
    format!(
        "!  Legacy payload override layout detected: {rel}\n   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).\n   Migrate with: {}\n",
        style.cyan("exuno migrate --legacy")
    )
}

/// `resolve_payload_source`: per-tool override → declared `targets.<res>.source`
/// → legacy flat layout → shared `.ai/src/mcp.json` (mcp only) → shipped base.
pub fn resolve_source(s: &mut Session, tool: &Tool, resource: &str) -> Option<String> {
    let root = s.paths.root.clone();
    let override_dir = format!("{}/{}", s.tools_dir, tool.slug);
    if s.ws.is_dir(&override_dir) {
        let prefix = format!("{resource}.");
        let found =
            s.ws.glob(&override_dir)
                .into_iter()
                .map(|name| format!("{override_dir}/{name}"))
                .find(|path| paths::leaf(path).starts_with(&prefix) && s.ws.is_file(path));
        if found.is_some() {
            return found;
        }
    }

    let declared = tool.value(&format!("targets.{resource}.source"));
    if !declared.is_empty() {
        let declared_abs = if crate::paths::is_absolute(&declared) {
            declared.clone()
        } else {
            format!("{root}/{declared}")
        };
        if s.ws.is_file(&declared_abs) {
            if ["hooks", "mcp", "settings"]
                .iter()
                .any(|kind| declared.starts_with(&format!(".ai/src/{kind}/")))
            {
                s.warn_legacy_payload(&declared_abs);
            }
            return Some(declared_abs);
        }
    }

    let base = tool.base_payload(resource);
    if let Some(ext) = base
        .and_then(|file| file.path().extension())
        .and_then(|ext| ext.to_str())
    {
        let legacy = format!("{root}/.ai/src/{resource}/{}.{ext}", tool.slug);
        if s.ws.is_file(&legacy) {
            s.warn_legacy_payload(&legacy);
            return Some(legacy);
        }
    }

    if resource == "mcp" {
        let shared = format!("{root}/.ai/src/mcp.json");
        if s.ws.is_file(&shared) {
            return Some(shared);
        }
    }

    base.and_then(|file| file.path().file_name())
        .and_then(|name| name.to_str())
        .map(|name| format!("{ENGINE_ROOT}/lib/templates/{resource}/{name}"))
}

/// `describe_payload_source`.
pub fn describe_source(
    tools_dir: &str,
    root: &str,
    path: &str,
    slug: &str,
    resource: &str,
) -> &'static str {
    if path.is_empty() {
        return "";
    }
    if path.starts_with(&format!("{tools_dir}/{slug}/")) {
        return "override";
    }
    if resource == "mcp" && path == format!("{root}/.ai/src/mcp.json") {
        return "shared";
    }
    if path.starts_with(&format!("{root}/.ai/src/{resource}/")) {
        return "legacy";
    }
    if path.starts_with(&format!("{ENGINE_ROOT}/lib/templates/{resource}/")) {
        return "base";
    }
    "declared"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session::test_session;
    use crate::engine::workspace::Content;

    #[test]
    fn payload_resolution_follows_the_bash_order() {
        let mut s = test_session();
        let claude = Tool::new("claude", None);
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/<agentsync>/lib/templates/mcp/claude.json")
        );
        s.ws.insert_file("/proj/.ai/src/mcp.json", Content::Bytes(b"{}".to_vec()));
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp.json")
        );
        s.ws.insert_file(
            "/proj/.ai/src/mcp/claude.json",
            Content::Bytes(b"{}".to_vec()),
        );
        assert_eq!(
            resolve_source(&mut s, &claude, "mcp").as_deref(),
            Some("/proj/.ai/src/mcp/claude.json")
        );
        assert_eq!(s.log.lines().len(), 3);
        s.ws.insert_file(
            "/proj/.ai/src/tools/claude/mcp.json",
            Content::Bytes(b"{}".to_vec()),
        );
        let found = resolve_source(&mut s, &claude, "mcp").unwrap();
        assert_eq!(found, "/proj/.ai/src/tools/claude/mcp.json");
        assert_eq!(
            describe_source("/proj/.ai/src/tools", "/proj", &found, "claude", "mcp"),
            "override"
        );
        assert_eq!(
            describe_source(
                "/proj/.ai/src/tools",
                "/proj",
                "/<agentsync>/lib/templates/mcp/claude.json",
                "claude",
                "mcp"
            ),
            "base"
        );
        assert_eq!(resolve_source(&mut s, &claude, "hooks"), None);
    }

    fn write(root: &std::path::Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    #[test]
    fn the_first_file_named_after_the_resource_wins() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), ".ai/src/tools/cursor/hooks.json.bak", "{}");
        write(dir.path(), ".ai/src/tools/cursor/hooks.json", "{}");
        write(dir.path(), ".ai/src/tools/cursor/mcp.json", "{}");
        let project = Project::at(dir.path()).unwrap();
        let found = find_new_override(&project, "cursor", "hooks").unwrap();
        assert_eq!(
            found,
            Some(dir.path().join(".ai/src/tools/cursor/hooks.json"))
        );
        assert_eq!(
            find_new_override(&project, "cursor", "settings").unwrap(),
            None
        );
        assert_eq!(find_new_override(&project, "zed", "hooks").unwrap(), None);
    }

    #[test]
    fn the_legacy_path_takes_the_shipped_payload_extension() {
        let dir = tempfile::tempdir().unwrap();
        let project = Project::at(dir.path()).unwrap();
        let claude = Tool::load(&project, "claude").unwrap();
        assert_eq!(
            legacy_override_path(&project, &claude, "settings"),
            Some(dir.path().join(".ai/src/settings/claude.json"))
        );
        assert_eq!(legacy_override_path(&project, &claude, "hooks"), None);
        let codex = Tool::load(&project, "codex").unwrap();
        assert_eq!(
            legacy_override_path(&project, &codex, "settings"),
            Some(dir.path().join(".ai/src/settings/codex.toml"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_effective_source_walks_override_declared_legacy_shared_then_base() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap();
        let project = Project::at(&root).unwrap();
        let cursor = Tool::load(&project, "cursor").unwrap();
        let shown = |project: &Project, tool: &Tool, resource: &str| {
            let (source, warn) = effective_source(project, tool, resource).unwrap();
            (source.map(|s| s.shown()), warn)
        };
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (
                Some("/<agentsync>/lib/templates/hooks/cursor.json".to_string()),
                None
            )
        );
        write(&root, ".ai/src/hooks/cursor.json", "{}\n");
        let legacy = root.join(".ai/src/hooks/cursor.json");
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (Some(legacy.disk_text()), Some(legacy.clone()))
        );
        assert_eq!(
            legacy_warning(&project, &legacy, &Style::plain()),
            "!  Legacy payload override layout detected: .ai/src/hooks/cursor.json\n   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).\n   Migrate with: exuno migrate --legacy\n"
        );
        write(&root, ".ai/src/tools/cursor/hooks.json", "{}\n");
        assert_eq!(
            shown(&project, &cursor, "hooks"),
            (
                Some(root.join(".ai/src/tools/cursor/hooks.json").disk_text()),
                None
            )
        );
        let claude = Tool::load(&project, "claude").unwrap();
        write(&root, ".ai/src/mcp.json", "{}\n");
        assert_eq!(
            shown(&project, &claude, "mcp"),
            (Some(root.join(".ai/src/mcp.json").disk_text()), None)
        );
        assert_eq!(
            base_source(&claude, "settings").unwrap().shown(),
            "/<agentsync>/lib/templates/settings/claude.json"
        );
    }
}
