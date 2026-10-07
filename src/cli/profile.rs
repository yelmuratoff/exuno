//! `exuno profile`: `cmd_profile` of `lib/helpers/profile.sh`, which
//! scaffolds, lists, and removes config-home variants of tools.

use crate::paths::DiskText;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::{Path, PathBuf};

use super::put;
use crate::config::tool::Tool;
use crate::engine::render::TARGET_KEYS;
use crate::output::help::{Help, Section};
use crate::output::log::Log;
use crate::output::style::Style;
use crate::paths::Paths;
use crate::project::Project;
use crate::transaction::manifest::Manifest;
use crate::{Error, config::catalog, config::profiles, config::yaml_edit};

pub const HELP: Help = Help {
    command: "profile",
    tagline: "scaffold, list, and remove config-home variants of tools",
    synopsis: &["profile <subcommand>"],
    description: &[],
    sections: &[
        Section {
            title: "SUBCOMMANDS",
            entries: &[
                (
                    "add <name> [--tools a,b] [--adopt]",
                    "Scaffold variant tools + overlay + config entry",
                ),
                ("list", "Show profiles, their tools and config homes"),
                (
                    "remove <name> [--yes]",
                    "Delete config-home output, variants, and entry",
                ),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[("-h, --help", "Show this help")],
        },
        Section {
            title: "SEE ALSO",
            entries: &[
                ("exuno sync --profile <name>", "Sync a profile"),
                ("exuno sync", "Active profiles also sync on a plain run"),
            ],
        },
    ],
    examples: &[
        "profile add hub --tools claude,cursor",
        "profile list",
        "profile remove hub --yes",
    ],
};

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;

pub fn profile(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let rest = args.get(1..).unwrap_or_default();
    match args.first().map_or("list", String::as_str) {
        "add" => add(rest, discover, style, out, err),
        "list" | "ls" => list(discover, style, out, err),
        "remove" | "rm" => remove(rest, discover, style, interactive, confirm, out, err),
        "--help" | "-h" | "help" => {
            put(out, HELP.render(style).as_bytes())?;
            Ok(0)
        }
        other => {
            put(
                err,
                format!(
                    "{}: unknown subcommand: {other}\nTry: {}\n",
                    style.red("Error"),
                    style.cyan("exuno profile --help")
                )
                .as_bytes(),
            )?;
            Ok(2)
        }
    }
}

/// `_profile_prepare_context`: a missing explicit config is status 2.
fn context(
    discover: Discover,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Project, u8>, Error> {
    match discover() {
        Ok(project) => Ok(Ok(project)),
        Err(Error::ConfigPathNotFound(path)) => {
            put(
                err,
                format!(
                    "{}: AGENTSYNC_CONFIG_PATH is set but file not found: {}\n",
                    style.red("Error"),
                    path.disk_text()
                )
                .as_bytes(),
            )?;
            Ok(Err(2))
        }
        Err(other) => Err(other),
    }
}

fn config_text(project: &Project) -> Result<String, Error> {
    match &project.config_path {
        Some(path) => std::fs::read(path)
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .map_err(|e| Error::io(path, e)),
        None => Ok(String::new()),
    }
}

fn config_shown(project: &Project) -> String {
    project
        .config_path
        .as_ref()
        .map(|p| p.disk_text())
        .unwrap_or_default()
}

fn usage_error(style: &Style, err: &mut dyn Write, message: &str) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(2)
}

/// The profile name, `--tools`, and `--adopt`; `Err(message)` is a usage error.
fn add_args(args: &[String]) -> Result<(String, String, bool), String> {
    let (mut name, mut tools_csv, mut adopt) = (String::new(), String::new(), false);
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--tools" => {
                tools_csv = rest
                    .next()
                    .cloned()
                    .ok_or("--tools needs a comma-separated list of tools.")?;
            }
            "--adopt" => adopt = true,
            "--yes" | "-y" => {}
            flag if flag.starts_with('-') => return Err(format!("unknown flag: {flag}")),
            value if name.is_empty() => name = value.to_string(),
            _ => return Err("too many arguments.".into()),
        }
    }
    if name.is_empty() {
        return Err("exuno profile add <name> [--tools a,b] [--adopt]".into());
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err("profile name must be [a-zA-Z0-9_-].".into());
    }
    Ok((name, tools_csv, adopt))
}

/// The tools a profile varies: `--tools`, else the enabled ones; `Err(status)`
/// once an empty list or a tool without config-home support is reported.
fn base_tools(
    project: &Project,
    tools_csv: &str,
    style: &Style,
    err: &mut dyn Write,
) -> Result<Result<Vec<String>, u8>, Error> {
    let base_tools: Vec<String> = if tools_csv.is_empty() {
        project.configured_enabled_tools()?
    } else {
        tools_csv
            .split(',')
            .map(str::trim)
            .filter(|t| !t.is_empty())
            .map(str::to_string)
            .collect()
    };
    if let Some(unknown) = base_tools.iter().find(|slug| {
        catalog::base_tool_yaml(slug).is_none() && !project.user_tool_file(slug).is_file()
    }) {
        let message = format!(
            "unknown tool: {unknown}.\nRun {} to see available tools.",
            style.cyan("exuno list")
        );
        return usage_error(style, err, &message).map(Err);
    }
    if base_tools.is_empty() {
        put(
            err,
            format!(
                "{}: no base tools — enable tools first or pass --tools.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Err(1));
    }
    for base in &base_tools {
        if Tool::load(project, base)?.flag("profile_supported") == Some(false) {
            let message = format!(
                "{base} reads project-root files and does not support config-home profiles."
            );
            return usage_error(style, err, &message).map(Err);
        }
    }
    Ok(Ok(base_tools))
}

/// Creates the profile's overlay `src/` and its README: the overlay `src/`.
fn profile_overlay(project: &Project, overlay_rel: &str, name: &str) -> Result<PathBuf, Error> {
    let overlay_root = project.root.join(overlay_rel);
    let overlay_src = overlay_root.join("src");
    std::fs::create_dir_all(&overlay_src).map_err(|e| Error::io(&overlay_src, e))?;
    let readme = overlay_root.join("README.md");
    if !readme.is_file() {
        std::fs::write(&readme, readme_text(name)).map_err(|e| Error::io(&readme, e))?;
    }
    Ok(overlay_src)
}

/// `_profile_add`.
fn add(
    args: &[String],
    discover: Discover,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (name, tools_csv, adopt) = match add_args(args) {
        Ok(parsed) => parsed,
        Err(message) => return usage_error(style, err, &message),
    };
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let Some(config) = project.config_path.clone() else {
        put(
            err,
            format!(
                "{}: no exuno.yaml — run {} first.\n",
                style.red("Error"),
                style.cyan("exuno init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let text = config_text(&project)?;
    if yaml_edit::find_key_line(&text, &format!("profiles.{name}")).is_some() {
        put(
            err,
            format!(
                "{} in {}\n",
                style.yellow(&format!("Profile '{name}' already exists")),
                config.disk_text()
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let base_tools = match base_tools(&project, &tools_csv, style, err)? {
        Ok(base_tools) => base_tools,
        Err(status) => return Ok(status),
    };
    let overlay_rel = format!(".ai/profiles/{name}");
    let overlay_src = profile_overlay(&project, &overlay_rel, &name)?;
    put(
        out,
        format!(
            "\n{}\n",
            style.bold(&format!("  Creating profile '{name}'"))
        )
        .as_bytes(),
    )?;

    let mut variants = Vec::new();
    for base in &base_tools {
        let variant = format!("{base}-{name}");
        let home = format!(".{base}-{name}");
        write_variant(&project, base, &variant, &home, &name)?;
        put(
            out,
            format!(
                "    {} {base} → {}  ({home}/)\n",
                style.green("✓"),
                style.cyan(&variant)
            )
            .as_bytes(),
        )?;
        if adopt {
            adopt_home(&project, &variant, &home, &overlay_src)?;
        }
        variants.push(variant);
    }
    register(&config, &name, &overlay_rel, &variants)?;
    put(
        out,
        format!(
            "\n{}\n    overlay:  {overlay_rel}/src/   {}\n    sync:     {}\n\n",
            style.green(&format!("  Profile '{name}' registered.")),
            style.dim("(drop profile-only rules/skills here)"),
            style.cyan(&format!("exuno sync --profile {name}"))
        )
        .as_bytes(),
    )?;
    Ok(0)
}

/// `_profile_write_overlay_readme`.
fn readme_text(name: &str) -> String {
    format!(
        "# Profile: {name} — overlay source\n\nDrop profile-only content under `src/`; it layers over the base `.ai/src/` at\nsync time (the profile wins on path conflicts):\n\n  src/AGENTS.md   src/rules/*.md   src/skills/<name>/   src/commands/*.md   src/agents/*.md\n\nAnything not present here is inherited from the base — an empty overlay mirrors\nthe base. Profile-specific MCP/settings/hooks go in `.ai/src/tools/<tool>-{name}/`.\n"
    )
}

/// `_profile_write_variant` with `_profile_variant_targets`.
fn write_variant(
    project: &Project,
    base: &str,
    variant: &str,
    home: &str,
    name: &str,
) -> Result<(), Error> {
    let tool = Tool::load(project, base)?;
    let label = format!("{} ({name})", tool.display_name());
    let mut text = format!(
        "# {label} — Exuno profile variant (profile: {name}).\n# Generated by `exuno profile add`. Inherits unset fields from base `{base}`.\nbase: {base}\nname: \"{label}\"\nprofile_home: \"{home}\"\n"
    );
    let mut emitted = false;
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        if !emitted {
            text.push_str("targets:\n");
            emitted = true;
        }
        let dest = if tool.flag(&format!("targets.{key}.profile_scoped")) == Some(false) {
            raw
        } else {
            profiles::rewrite_dest(&raw, home)
        };
        text.push_str(&format!("  {key}:\n    dest: \"{dest}\"\n"));
    }
    let file = project.user_tool_file(variant);
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))?;
    }
    std::fs::write(&file, text).map_err(|e| Error::io(&file, e))
}

/// `_profile_register`: a new child right after `profiles:`, or a new block.
fn register(
    config: &Path,
    name: &str,
    overlay_rel: &str,
    variants: &[String],
) -> Result<(), Error> {
    let child = format!(
        "  {name}:\n    overlay: \"{overlay_rel}\"\n    active: true\n    tools: [{}]\n",
        variants.join(", ")
    );
    let bytes = std::fs::read(config).map_err(|e| Error::io(config, e))?;
    let text = String::from_utf8_lossy(&bytes).into_owned();
    let Some((lineno, _)) = yaml_edit::find_key_line(&text, "profiles") else {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(config)
            .map_err(|e| Error::io(config, e))?;
        return file
            .write_all(format!("\nprofiles:\n{child}").as_bytes())
            .map_err(|e| Error::io(config, e));
    };
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut updated = String::new();
    for (index, line) in lines.into_iter().enumerate() {
        updated.push_str(line);
        updated.push('\n');
        if index + 1 == lineno {
            updated.push_str(&child);
        }
    }
    crate::engine::staging::write_beside(config, updated.as_bytes())
}

/// `cp -RL <src>/. <dst>/`: links followed, unreadable entries skipped.
fn copy_following(src: &Path, dst: &Path) {
    let Ok(entries) = std::fs::read_dir(src) else {
        return;
    };
    let _ = std::fs::create_dir_all(dst);
    for entry in entries.filter_map(|e| e.ok()) {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        match std::fs::metadata(&from) {
            Ok(meta) if meta.is_dir() => copy_following(&from, &to),
            Ok(_) => {
                let _ = std::fs::copy(&from, &to);
            }
            Err(_) => {}
        }
    }
}

/// `_profile_adopt_home`.
fn adopt_home(
    project: &Project,
    variant: &str,
    home: &str,
    overlay_src: &Path,
) -> Result<(), Error> {
    let home_abs = project.root.join(home);
    if !home_abs.is_dir() {
        return Ok(());
    }
    for item in ["rules", "skills", "commands", "agents"] {
        let from = home_abs.join(item);
        if from.is_dir() {
            copy_following(&from, &overlay_src.join(item));
        }
    }
    let mut markdown: Vec<_> = std::fs::read_dir(&home_abs)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| {
                    let name = p.file_name().map(|n| n.disk_text()).unwrap_or_default();
                    !name.starts_with('.') && name.ends_with(".md") && p.is_file()
                })
                .collect()
        })
        .unwrap_or_default();
    markdown.sort();
    if let Some(first) = markdown.first() {
        let target = overlay_src.join("AGENTS.md");
        std::fs::copy(first, &target).map_err(|e| Error::io(&target, e))?;
    }
    let payload_dir = project.user_tools_dir().join(variant);
    for (from, to) in [
        (".mcp.json", "mcp.json"),
        ("settings.json", "settings.json"),
        ("hooks.json", "hooks.json"),
    ] {
        let source = home_abs.join(from);
        if source.is_file() {
            std::fs::create_dir_all(&payload_dir).map_err(|e| Error::io(&payload_dir, e))?;
            let target = payload_dir.join(to);
            std::fs::copy(&source, &target).map_err(|e| Error::io(&target, e))?;
        }
    }
    Ok(())
}

/// Removes the files below `home` the manifest records, then the directories
/// that leaves empty; `true` once `home` itself is gone.
fn remove_generated(
    paths: &Paths,
    home: &Path,
    recorded: &BTreeSet<String>,
) -> Result<bool, Error> {
    let mut files = Vec::new();
    super::files_below(home, &mut files);
    for file in files {
        let generated = paths
            .to_repo_relative(&file.disk_text())
            .is_some_and(|rel| recorded.contains(&rel));
        if generated {
            std::fs::remove_file(&file).map_err(|e| Error::io(&file, e))?;
        }
    }
    remove_empty_dirs(home)?;
    Ok(!home.exists())
}

fn remove_empty_dirs(dir: &Path) -> Result<(), Error> {
    let entries = std::fs::read_dir(dir).map_err(|e| Error::io(dir, e))?;
    for entry in entries {
        let path = entry.map_err(|e| Error::io(dir, e))?.path();
        if std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir()) {
            remove_empty_dirs(&path)?;
        }
    }
    let empty = std::fs::read_dir(dir)
        .map_err(|e| Error::io(dir, e))?
        .next()
        .is_none();
    if empty {
        std::fs::remove_dir(dir).map_err(|e| Error::io(dir, e))?;
    }
    Ok(())
}

/// `_profile_list`.
fn list(
    discover: Discover,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    let text = config_text(&project)?;
    let names = profiles::names(&text);
    if names.is_empty() {
        put(
            out,
            format!(
                "\n{}\n",
                style.dim("  No profiles. Create one with: exuno profile add <name>")
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let mut body = format!("\n{}\n", style.bold("  Profiles"));
    for name in &names {
        let label = if profiles::is_active(&text, name) {
            style.green("active")
        } else {
            style.dim("inactive")
        };
        body.push_str(&format!(
            "    {}  [{label}]\n      overlay: {}/src/\n",
            style.cyan(name),
            profiles::overlay_dir(&text, name)
        ));
        for tool in profiles::tools(&text, name) {
            let home = Tool::load(&project, &tool)?.value("profile_home");
            body.push_str(&format!("      tool:    {tool}  →  {home}/\n"));
        }
    }
    body.push('\n');
    put(out, body.as_bytes())?;
    Ok(0)
}

/// `_profile_remove`.
fn remove(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: &mut dyn FnMut(&str) -> bool,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let (mut name, mut assume_yes) = (String::new(), false);
    for arg in args {
        match arg.as_str() {
            "--yes" | "-y" => assume_yes = true,
            flag if flag.starts_with('-') => {
                return usage_error(style, err, &format!("unknown flag: {flag}"));
            }
            value if name.is_empty() => name = value.to_string(),
            _ => return usage_error(style, err, "too many arguments."),
        }
    }
    if name.is_empty() {
        return usage_error(style, err, "exuno profile remove <name>");
    }
    let project = match context(discover, style, err)? {
        Ok(project) => project,
        Err(status) => return Ok(status),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let text = config_text(&project)?;
    if yaml_edit::find_key_line(&text, &format!("profiles.{name}")).is_none() {
        put(
            err,
            format!(
                "{}: no profile '{name}' in {}\n",
                style.red("Error"),
                config_shown(&project)
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let variants = profiles::tools(&text, &name);
    put(
        out,
        format!(
            "\n{}\n    Deletes config-home output, variant tool files, and the profiles entry.\n    Overlay sources under .ai/profiles/{name}/ are kept.\n\n",
            style.yellow(&format!("  Removing profile '{name}'"))
        )
        .as_bytes(),
    )?;
    if !assume_yes && interactive && !confirm("Proceed?") {
        put(out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
        return Ok(0);
    }
    let paths = Paths::on_disk(&project.root.disk_text());
    let recorded = Manifest::load(&project.root.disk_text())?
        .map(|manifest| manifest.paths())
        .unwrap_or_default();
    for variant in &variants {
        let home = Tool::load(&project, variant)?.value("profile_home");
        if !home.is_empty() {
            let mut quiet = Log::default();
            let resolved =
                paths.resolve_dest(&home, &format!("profile_home for {variant}"), &mut quiet);
            if let Some(abs) = resolved.filter(|abs| Path::new(abs).is_dir()) {
                let line = if remove_generated(&paths, Path::new(&abs), &recorded)? {
                    format!("    {} removed {home}/\n", style.green("✓"))
                } else {
                    let why = "(it holds files Exuno did not generate)";
                    format!("    {} kept {home}/ {why}\n", style.dim("·"))
                };
                put(out, line.as_bytes())?;
            }
        }
        let file = project.user_tool_file(variant);
        if file.is_file() {
            std::fs::remove_file(&file).map_err(|e| Error::io(&file, e))?;
        }
        let _ = std::fs::remove_dir_all(project.user_tools_dir().join(variant));
    }
    if let Some(config) = &project.config_path {
        yaml_edit::remove_key(config, &format!("profiles.{name}"))?;
        if profiles::names(&config_text(&project)?).is_empty() {
            yaml_edit::remove_key(config, "profiles")?;
        }
    }
    put(
        out,
        format!(
            "\n{}\n",
            style.green(&format!("  Profile '{name}' removed."))
        )
        .as_bytes(),
    )?;
    Ok(0)
}
#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.ai")).unwrap();
        std::fs::write(
            format!("{root}/.ai/agent_sync.yaml"),
            "tools:\n  enabled:\n    - claude\n",
        )
        .unwrap();
        (dir, root)
    }

    fn call(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = profile(
            &args,
            &discover,
            &Style::plain(),
            false,
            &mut |_| true,
            &mut out,
            &mut err,
        )
        .unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[test]
    fn a_profile_is_added_listed_and_removed_like_bash() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["add", "hub"]),
            (
                0,
                "\n  Creating profile 'hub'\n    ✓ claude → claude-hub  (.claude-hub/)\n\n  Profile 'hub' registered.\n    overlay:  .ai/profiles/hub/src/   (drop profile-only rules/skills here)\n    sync:     exuno sync --profile hub\n\n".to_string(),
                String::new()
            )
        );
        let config = format!("{root}/.ai/agent_sync.yaml");
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "tools:\n  enabled:\n    - claude\n\nprofiles:\n  hub:\n    overlay: \".ai/profiles/hub\"\n    active: true\n    tools: [claude-hub]\n"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/tools/claude-hub.yaml")).unwrap(),
            "# Claude Code (hub) — Exuno profile variant (profile: hub).\n# Generated by `exuno profile add`. Inherits unset fields from base `claude`.\nbase: claude\nname: \"Claude Code (hub)\"\nprofile_home: \".claude-hub\"\ntargets:\n  agents:\n    dest: \".claude-hub/CLAUDE.md\"\n  rules:\n    dest: \".claude-hub/rules\"\n  skills:\n    dest: \".claude-hub/skills\"\n  commands:\n    dest: \".claude-hub/commands\"\n  subagents:\n    dest: \".claude-hub/agents\"\n  settings:\n    dest: \".claude-hub/settings.json\"\n  mcp:\n    dest: \".claude-hub/.mcp.json\"\n  guard:\n    dest: \".claude/hooks/agentsync-guard.sh\"\n"
        );
        assert!(std::path::Path::new(&format!("{root}/.ai/profiles/hub/README.md")).is_file());
        assert_eq!(
            call(&root, &["list"]).1,
            "\n  Profiles\n    hub  [active]\n      overlay: .ai/profiles/hub/src/\n      tool:    claude-hub  →  .claude-hub/\n\n"
        );

        std::fs::create_dir_all(format!("{root}/.claude-hub/rules")).unwrap();
        assert_eq!(
            call(&root, &["remove", "hub", "-y"]).1,
            "\n  Removing profile 'hub'\n    Deletes config-home output, variant tool files, and the profiles entry.\n    Overlay sources under .ai/profiles/hub/ are kept.\n\n    ✓ removed .claude-hub/\n\n  Profile 'hub' removed.\n"
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "tools:\n  enabled:\n    - claude\n\n"
        );
        assert!(!std::path::Path::new(&format!("{root}/.claude-hub")).exists());
        assert!(!std::path::Path::new(&format!("{root}/.ai/src/tools/claude-hub.yaml")).exists());
    }

    #[test]
    fn a_declined_remove_prompt_cancels_like_bash() {
        let (_dir, root) = project();
        call(&root, &["add", "hub"]);
        let config = format!("{root}/.ai/agent_sync.yaml");
        let before = std::fs::read_to_string(&config).unwrap();
        let discover = || Project::at(&root);
        let mut questions = Vec::new();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = profile(
            &["remove".to_string(), "hub".to_string()],
            &discover,
            &Style::plain(),
            true,
            &mut |question| {
                questions.push(question.to_string());
                false
            },
            &mut out,
            &mut err,
        )
        .unwrap();
        assert_eq!(status, 0);
        assert_eq!(questions, ["Proceed?"]);
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "\n  Removing profile 'hub'\n    Deletes config-home output, variant tool files, and the profiles entry.\n    Overlay sources under .ai/profiles/hub/ are kept.\n\nCancelled.\n"
        );
        assert_eq!(std::fs::read_to_string(&config).unwrap(), before);
        assert!(std::path::Path::new(&format!("{root}/.ai/src/tools/claude-hub.yaml")).is_file());
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["bogus"]),
            (
                2,
                String::new(),
                "Error: unknown subcommand: bogus\nTry: exuno profile --help\n".to_string()
            )
        );
        assert_eq!(
            call(&root, &["add", "bad name"]).2,
            "Error: profile name must be [a-zA-Z0-9_-].\n"
        );
        assert_eq!(
            call(&root, &["add", "hub", "--tools"]),
            (
                2,
                String::new(),
                "Error: --tools needs a comma-separated list of tools.\n".to_string()
            )
        );
        assert_eq!(
            call(&root, &["remove", "nope"]),
            (
                1,
                String::new(),
                format!("Error: no profile 'nope' in {root}/.ai/agent_sync.yaml\n")
            )
        );
        assert_eq!(
            call(&root, &[]).1,
            "\n  No profiles. Create one with: exuno profile add <name>\n"
        );
    }

    #[test]
    fn tools_are_trimmed_and_an_unknown_one_is_refused() {
        let (_dir, root) = project();
        assert_eq!(
            call(&root, &["add", "hub", "--tools", "claude, nope"]),
            (
                2,
                String::new(),
                "Error: unknown tool: nope.\nRun exuno list to see available tools.\n".to_string()
            )
        );
        assert!(!Path::new(&format!("{root}/.ai/src/tools")).exists());
        let (status, _, err) = call(&root, &["add", "hub", "--tools", " claude , cursor"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert!(Path::new(&format!("{root}/.ai/src/tools/claude-hub.yaml")).is_file());
        assert!(
            std::fs::read_to_string(format!("{root}/.ai/agent_sync.yaml"))
                .unwrap()
                .contains("    tools: [claude-hub, cursor-hub]\n")
        );
    }

    #[test]
    fn help_renders_the_shared_shape() {
        let (_dir, root) = project();
        let (status, out, err) = call(&root, &["--help"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\n  exuno profile — scaffold, list, and remove config-home variants of tools\n\n  USAGE\n    exuno profile <subcommand>\n\n  SUBCOMMANDS\n    add <name> [--tools a,b] [--adopt]   Scaffold variant tools + overlay + config entry\n    list                                 Show profiles, their tools and config homes\n    remove <name> [--yes]                Delete config-home output, variants, and entry\n\n  OPTIONS\n    -h, --help   Show this help\n\n  SEE ALSO\n    exuno sync --profile <name>   Sync a profile\n    exuno sync                    Active profiles also sync on a plain run\n\n  EXAMPLES\n    exuno profile add hub --tools claude,cursor\n    exuno profile list\n    exuno profile remove hub --yes\n\n"
        );
        assert_eq!(call(&root, &["help"]).1, out);
    }
}
