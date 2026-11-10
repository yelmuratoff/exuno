//! The tool catalog: every tool the run knows, the enabled and profile sets, and the protected dests.

use std::collections::BTreeSet;

use super::{Run, TARGET_KEYS};
use crate::config::tool::Tool;
use crate::engine::file_ops;
use crate::engine::session::Session;
use crate::{config::catalog, config::profiles, config::yaml_subset};

/// `_build_tool_catalog`, the `warm_*_cache` sets, and `_collect_protected_dests`.
pub fn build_catalog(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    run.profiles = match &run.selection.profile {
        Some(name) => vec![name.clone()],
        None => profiles::names(&text)
            .into_iter()
            .filter(|name| profiles::is_active(&text, name))
            .collect(),
    };
    load_tools(s, run);
    collect_protected_dests(s, run);
}

/// `list_all_tools`, plus the enabled and profile-tool sets `warm_*_cache` build.
fn load_tools(s: &mut Session, run: &mut Run) {
    let mut all: BTreeSet<String> = catalog::base_tools().into_iter().collect();
    if let Some(text) = &run.config {
        run.enabled.extend(yaml_subset::list(text, "tools.enabled"));
        run.profile_tools.extend(profiles::all_tools(text));
    }
    for slug in user_tool_slugs(s) {
        if load_tool(s, &slug).user_value("enabled") == "true" {
            run.enabled.insert(slug.clone());
        }
        all.insert(slug);
    }
    run.tools = all.into_iter().collect();
}

/// The `<tools dir>/<slug>.yaml` overrides, `_`-prefixed templates skipped.
pub(super) fn user_tool_slugs(s: &Session) -> Vec<String> {
    let tools_dir = &s.tools_dir;
    s.ws.glob(tools_dir)
        .into_iter()
        .filter_map(|name| {
            let stem = name.strip_suffix(".yaml")?;
            let listed = !stem.starts_with('_') && s.ws.is_file(&format!("{tools_dir}/{name}"));
            listed.then(|| stem.to_string())
        })
        .collect()
}

/// The layered tool with its `<tools dir>/<slug>.yaml` read from the workspace.
pub(super) fn load_tool(s: &Session, slug: &str) -> Tool {
    let path = format!("{}/{slug}.yaml", s.tools_dir);
    let user_yaml =
        s.ws.read(&path)
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
    Tool::new(slug, user_yaml)
}

/// `_collect_protected_dests`: cleanup never removes what an enabled tool or
/// any profile tool claims; the transaction snapshots every dest the run may
/// change; `.gitignore` gets the dests of enabled and profile tools.
fn collect_protected_dests(s: &mut Session, run: &mut Run) {
    let text = run.config.clone().unwrap_or_default();
    let selected_profile_tools: BTreeSet<String> = run
        .profiles
        .iter()
        .flat_map(|name| profiles::tools(&text, name))
        .collect();

    let slugs = run.tools.clone();
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        let tool = load_tool(s, slug);
        if run.enabled.contains(slug) {
            let dests = collect_tool_dests(s, run, &tool, false);
            if run.selection.includes(slug) {
                run.backup_targets.extend(dests);
            }
        } else if run.cleanup == "true" {
            for key in TARGET_KEYS {
                let raw = tool.value(&format!("targets.{key}.dest"));
                if raw.is_empty() {
                    continue;
                }
                let label = format!("targets.{key}.dest for {slug}");
                if let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) {
                    run.backup_targets.push(abs);
                }
            }
        }
    }
    for slug in run.profile_tools.clone() {
        let tool = load_tool(s, &slug);
        let dests = collect_tool_dests(s, run, &tool, true);
        if selected_profile_tools.contains(&slug) && run.selection.includes(&slug) {
            run.backup_targets.extend(dests);
        }
    }
}

/// The files the previous manifest records at each selected tool's
/// `legacy_dest`, which the transaction snapshots before the pass removes
/// them. Needs the manifest active; a path nothing recorded stays untouched.
pub fn collect_legacy_targets(s: &mut Session, run: &mut Run) {
    for slug in run.tools.clone() {
        if run.profile_tools.contains(&slug)
            || !run.enabled.contains(&slug)
            || !run.selection.includes(&slug)
        {
            continue;
        }
        let tool = load_tool(s, &slug);
        for (_, abs) in legacy_dests(s, &tool) {
            run.backup_targets.extend(file_ops::recorded_under(s, &abs));
        }
    }
}

/// Each target's resolved `legacy_dest`: where an earlier release of the tool
/// config wrote that target, keyed by the target.
pub(super) fn legacy_dests(s: &mut Session, tool: &Tool) -> Vec<(&'static str, String)> {
    let mut found = Vec::new();
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.legacy_dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.legacy_dest for {}", tool.slug);
        if let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) {
            found.push((key, abs));
        }
    }
    found
}

pub(super) fn keyed(s: &Session, tool: &Tool, resource: &str) -> bool {
    tool.keyed(resource, s.paths.root_is_home())
}

/// `_collect_tool_dests`: the tool's resolved dests, also recorded for cleanup
/// protection and the `.gitignore` payload.
fn collect_tool_dests(s: &mut Session, run: &mut Run, tool: &Tool, profile: bool) -> Vec<String> {
    let mut collected = Vec::new();
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {}", tool.slug);
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        run.protected.push(abs.clone());
        collected.push(abs.clone());
        let Some(mut rel) = s.paths.to_repo_relative(&abs) else {
            s.log
                .error(&format!("Path is outside repository root: {abs}"));
            continue;
        };
        if matches!(key, "settings" | "mcp")
            && tool.flag(&format!("targets.{key}.enabled")) != Some(false)
            && keyed(s, tool, key)
        {
            run.keyed_dests.insert(rel.clone());
        }
        if tool.flag(&format!("targets.{key}.gitignore")) == Some(false) {
            continue;
        }
        if matches!(key, "rules" | "skills" | "commands" | "subagents") {
            rel.push('/');
        }
        if profile && tool.flag(&format!("targets.{key}.profile_scoped")) != Some(false) {
            run.gitignore_profile.push(rel);
        } else {
            run.gitignore_generated.push(rel);
        }
    }
    collected
}
