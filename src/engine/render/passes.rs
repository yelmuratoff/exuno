//! The per-tool passes: the personal and profile loops, `sync_tool`, its post-sync hook, and `cleanup_tool`.

use super::steps::{
    sync_commands_step, sync_payloads_step, sync_rules_step, sync_skills_step, sync_subagents_step,
};
use super::tools::{self, load_tool};
use super::{Run, Step, Stop, TARGET_KEYS, checkpoint, io};
use crate::config::tool::Tool;
use crate::engine::session::Session;
use crate::{config::payload, config::profiles, engine::file_ops, engine::overlay};
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Dests {
    pub(super) agents: String,
    pub(super) rules: String,
    pub(super) skills: String,
    pub(super) commands: String,
    pub(super) subagents: String,
    pub(super) settings: String,
    pub(super) mcp: String,
    pub(super) hooks: String,
    pub(super) guard: String,
}

/// `_run_personal_pass` and `_run_profile_passes`.
pub fn run_passes(s: &mut Session, run: &mut Run) -> Step {
    let config = run.config.as_deref().unwrap_or_default();
    for profile in &run.profiles {
        for slug in profiles::tools(config, profile) {
            if load_tool(s, &slug).flag("profile_supported") == Some(false) {
                s.log.error(&format!(
                    "{slug} reads project-root files and does not support config-home profiles"
                ));
                return Err(Stop(1));
            }
        }
    }
    check_shared_agents_dests(s, run)?;
    check_shared_mcp_dests(s, run)?;
    let mut slugs = run.tools.clone();
    order_shared_agents(s, &mut slugs);
    for slug in &slugs {
        if run.profile_tools.contains(slug) {
            continue;
        }
        run.total += 1;
        checkpoint(s)?;
        if run.enabled.contains(slug) {
            sync_tool(s, run, slug)?;
        } else {
            cleanup_tool(s, run, slug);
        }
        if run.printed {
            s.log.blank();
        }
    }

    let text = run.config.clone().unwrap_or_default();
    for profile in run.profiles.clone() {
        let tools: Vec<String> = profiles::tools(&text, &profile)
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        if tools.is_empty() {
            continue;
        }
        s.log.info(&format!("Profile: {profile}"));
        run.sources = run.base_sources.clone();
        let base_src = run.profile_parent_src.clone();
        overlay::setup_profile(s, &text, &profile, &base_src, &mut run.sources)
            .map_err(|e| io(s, e))?;
        for slug in tools {
            run.total += 1;
            checkpoint(s)?;
            sync_tool(s, run, &slug)?;
            if run.printed {
                s.log.blank();
            }
        }
        overlay::cleanup_profile(&mut s.ws).map_err(|e| io(s, e))?;
    }
    Ok(())
}

fn order_shared_agents(s: &mut Session, slugs: &mut [String]) {
    let mut positions: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (index, slug) in slugs.iter().enumerate() {
        let tool = load_tool(s, slug);
        let dest = resolve_one_dest(s, &tool, "agents", &tool.display_name());
        if !dest.is_empty() {
            positions.entry(dest).or_default().push(index);
        }
    }
    for indexes in positions.values().filter(|indexes| indexes.len() > 1) {
        let mut group: Vec<String> = indexes.iter().map(|index| slugs[*index].clone()).collect();
        group.sort_by_key(|slug| {
            load_tool(s, slug).value("targets.rules.inline_into_agents") == "true"
        });
        for (index, slug) in indexes.iter().zip(group) {
            slugs[*index] = slug;
        }
    }
}

fn check_shared_agents_dests(s: &mut Session, run: &Run) -> Step {
    let mut owners: BTreeMap<String, (String, String, Vec<u8>)> = BTreeMap::new();
    for slug in &run.tools {
        if !run.enabled.contains(slug)
            || run.profile_tools.contains(slug)
            || !run.selection.includes(slug)
        {
            continue;
        }
        let tool = load_tool(s, slug);
        let display = tool.display_name();
        let dest = resolve_one_dest(s, &tool, "agents", &display);
        if dest.is_empty() {
            continue;
        }
        let source = tool_source(s, &tool, "agents", &run.sources.agents, &display)?;
        if !s.ws.is_file(&source) {
            continue;
        }
        let bytes = s.ws.read(&source).map_err(|e| io(s, e))?;
        if let Some((other_slug, other_source, other_bytes)) = owners.get(&dest) {
            if other_bytes != &bytes {
                s.log.error(&format!(
                    "Agents destination {} is shared by {other_slug} ({}) and {slug} ({}), but their sources differ",
                    s.display(&dest),
                    s.display(other_source),
                    s.display(&source)
                ));
                return Err(Stop(1));
            }
        } else {
            owners.insert(dest, (slug.clone(), source, bytes));
        }
    }
    Ok(())
}

fn check_shared_mcp_dests(s: &mut Session, run: &Run) -> Step {
    let mut owners: BTreeMap<String, (String, String, Vec<u8>, bool)> = BTreeMap::new();
    for slug in &run.tools {
        if !run.enabled.contains(slug) || run.profile_tools.contains(slug) {
            continue;
        }
        let tool = load_tool(s, slug);
        let dest = resolve_one_dest(s, &tool, "mcp", &tool.display_name());
        if dest.is_empty() {
            continue;
        }
        let Some(source) = payload::resolve_source(s, &tool, "mcp") else {
            continue;
        };
        if !s.ws.is_file(&source) {
            continue;
        }
        let bytes = s.ws.read(&source).map_err(|e| io(s, e))?;
        let keyed = tool.keyed("mcp", s.paths.root_is_home());
        if let Some((other_slug, other_source, other_bytes, other_keyed)) = owners.get(&dest) {
            if *other_keyed != keyed {
                s.log.error(&format!(
                    "MCP destination {} is shared by {other_slug} and {slug}, but one owns it by key and the other whole; give both the same targets.mcp.ownership",
                    s.display(&dest)
                ));
                return Err(Stop(1));
            }
            if other_bytes != &bytes {
                s.log.error(&format!(
                    "MCP destination {} is shared by {other_slug} ({}) and {slug} ({}), but their sources differ",
                    s.display(&dest),
                    s.display(other_source),
                    s.display(&source)
                ));
                return Err(Stop(1));
            }
        } else {
            owners.insert(dest, (slug.clone(), source, bytes, keyed));
        }
    }
    Ok(())
}

/// `_resolve_one_dest`.
fn resolve_one_dest(s: &mut Session, tool: &Tool, key: &str, display: &str) -> String {
    if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
        return String::new();
    }
    let raw = tool.value(&format!("targets.{key}.dest"));
    if raw.is_empty() {
        return String::new();
    }
    let label = format!("targets.{key}.dest for {display}");
    s.paths
        .clone()
        .resolve_dest(&raw, &label, &mut s.log)
        .unwrap_or_default()
}

fn resolve_dests(s: &mut Session, tool: &Tool, display: &str) -> Dests {
    Dests {
        agents: resolve_one_dest(s, tool, "agents", display),
        rules: resolve_one_dest(s, tool, "rules", display),
        skills: resolve_one_dest(s, tool, "skills", display),
        commands: resolve_one_dest(s, tool, "commands", display),
        subagents: resolve_one_dest(s, tool, "subagents", display),
        settings: resolve_one_dest(s, tool, "settings", display),
        mcp: resolve_one_dest(s, tool, "mcp", display),
        hooks: resolve_one_dest(s, tool, "hooks", display),
        guard: resolve_one_dest(s, tool, "guard", display),
    }
}

/// `resolve_source_path` as the steps call it: an unsafe root ends the run.
pub(super) fn source_path(s: &mut Session, raw: &str, label: &str) -> Result<String, Stop> {
    s.paths
        .clone()
        .resolve_source(raw, label, &mut s.log)
        .ok_or(Stop(1))
}

/// `_resolve_tool_src`.
pub(super) fn tool_source(
    s: &mut Session,
    tool: &Tool,
    key: &str,
    default: &str,
    display: &str,
) -> Result<String, Stop> {
    let configured = tool.value(&format!("targets.{key}.source"));
    let raw = if configured.is_empty() {
        default
    } else {
        &configured
    };
    source_path(s, raw, &format!("targets.{key}.source for {display}"))
}

/// `sync_tool`.
fn sync_tool(s: &mut Session, run: &mut Run, slug: &str) -> Step {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    if !run.selection.includes(slug) {
        run.skipped_names.push(display);
        run.skipped += 1;
        run.printed = false;
        return Ok(());
    }
    run.printed = true;
    let dests = resolve_dests(s, &tool, &display);
    s.log.info(&format!("Syncing {display}"));

    if !dests.agents.is_empty() {
        let src = tool_source(s, &tool, "agents", &run.sources.agents, &display)?;
        file_ops::copy_file(s, &src, &dests.agents).map_err(|e| io(s, e))?;
    }
    checkpoint(s)?;
    sync_rules_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_skills_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_commands_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_subagents_step(s, run, &tool, &dests, &display)?;
    checkpoint(s)?;
    sync_payloads_step(s, &tool, &dests)?;
    checkpoint(s)?;
    if !run.profile_tools.contains(slug) {
        remove_legacy_outputs(s, &tool)?;
        checkpoint(s)?;
    }

    let post_sync = tool.value("post_sync");
    if !s.dry_run && !run_post_sync_hook(s, run, &display, &post_sync)? {
        s.log.error(&format!(
            "Sync failed because post-sync hook failed for {display}"
        ));
        return Err(Stop(1));
    }
    run.synced += 1;
    Ok(())
}

/// `run_post_sync_hook`: false when the hook ran and failed.
fn run_post_sync_hook(
    s: &mut Session,
    run: &Run,
    display: &str,
    command: &str,
) -> Result<bool, Stop> {
    if command.is_empty() {
        return Ok(true);
    }
    if run.skip_post_sync {
        s.log.info(&format!(
            "Skipping post-sync hook for {display} (EXUNO_SKIP_POST_SYNC=true)"
        ));
        return Ok(true);
    }
    if !run.allow_post_sync {
        s.log.warning(&format!(
            "Skipping post-sync hook for {display} (set EXUNO_ALLOW_POST_SYNC=true to enable)"
        ));
        return Ok(true);
    }
    s.log.info(&format!("Running post-sync hook: {command}"));
    let succeeded = std::process::Command::new("bash")
        .arg("-lc")
        .arg(command)
        .current_dir(&s.paths.root)
        .status()
        .is_ok_and(|status| status.success());
    checkpoint(s)?;
    if !succeeded {
        s.log.warning("Post-sync hook failed");
    }
    Ok(succeeded)
}

/// Removes what an earlier run generated at a target's `legacy_dest`, so a
/// tool that reads both its old and new paths does not load it twice.
fn remove_legacy_outputs(s: &mut Session, tool: &Tool) -> Step {
    for (key, abs) in tools::legacy_dests(s, tool) {
        let removed = file_ops::remove_recorded(s, &abs).map_err(|e| io(s, e))?;
        if removed == 0 {
            continue;
        }
        let verb = if s.dry_run { "Would remove" } else { "Removed" };
        s.log.step(&format!(
            "{verb} {removed} earlier output(s) from {}/ (targets.{key} moved)",
            s.display(&abs)
        ));
    }
    Ok(())
}

/// `cleanup_tool`: remove a disabled tool's unprotected outputs.
fn cleanup_tool(s: &mut Session, run: &mut Run, slug: &str) {
    let tool = load_tool(s, slug);
    let display = tool.display_name();
    run.skipped_names.push(display.clone());
    run.skipped += 1;
    run.printed = false;
    if run.cleanup != "true" {
        return;
    }
    let mut cleaned = false;
    for key in TARGET_KEYS {
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            continue;
        }
        let label = format!("targets.{key}.dest for {display}");
        let Some(abs) = s.paths.clone().resolve_dest(&raw, &label, &mut s.log) else {
            continue;
        };
        if matches!(key, "settings" | "mcp") && tool.keyed(key, s.paths.root_is_home()) {
            if s.ws.is_file(&abs) && !run.protected.contains(&abs) {
                run.protected.push(abs.clone());
                if s.owned_before(&abs).is_some() {
                    s.log.step(&format!(
                        "Kept {} (the app writes to it too)",
                        s.display(&abs)
                    ));
                }
            }
            continue;
        }
        if !run.protected.contains(&abs) && file_ops::cleanup_path(s, &abs) {
            cleaned = true;
        }
    }
    if cleaned {
        s.log.info(&format!("Cleaned up {display} (disabled)"));
        run.printed = true;
    }
}

#[cfg(test)]
mod tests {
    use crate::engine::render::test_support::{file, project, text_of};
    use crate::engine::render::{Env, render};

    #[test]
    fn claude_renders_agents_rules_commands_payloads_and_the_engine_skill() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        render(&mut s, &Env::default()).unwrap();
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
        assert_eq!(text_of(&s, "/proj/.claude/rules/core.md"), "# Core\n");
        assert!(s.ws.is_file("/proj/.claude/commands/review.md"));
        assert!(s.ws.is_file("/proj/.claude/skills/exuno/SKILL.md"));
        assert!(s.ws.is_file("/proj/.claude/settings.json"));
        assert!(s.ws.is_file("/proj/.mcp.json"));
        assert!(s.ws.is_file("/proj/.claude/hooks/agentsync-guard.sh"));
        let touched: Vec<&str> = s.touched().iter().map(String::as_str).collect();
        assert!(touched.contains(&".claude/skills/exuno/references/maintenance.md"));
        assert!(!touched.contains(&"AGENTS.md"));
    }

    #[test]
    fn a_disabled_tool_is_cleaned_unless_an_enabled_tool_claims_the_dest() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [cursor]\n",
        );
        file(&mut s, "/proj/.codex/agents/x.toml", "x");
        render(&mut s, &Env::default()).unwrap();
        assert!(!s.ws.exists("/proj/.codex/agents"));
        assert!(s.ws.is_file("/proj/AGENTS.md"));
        assert!(
            s.log
                .lines()
                .iter()
                .any(|(_, l)| l == "[INFO] Cleaned up OpenAI Codex (disabled)")
        );
    }

    #[test]
    fn an_active_profile_renders_its_variant_under_its_overlay() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nprofiles:\n  hub:\n    active: true\n    tools: [claude-hub]\n",
        );
        file(
            &mut s,
            "/proj/.ai/src/tools/claude-hub.yaml",
            "base: claude\ntargets:\n  agents:\n    dest: \".claude-hub/CLAUDE.md\"\n  rules:\n    dest: \".claude-hub/rules\"\n",
        );
        file(&mut s, "/proj/.ai/profiles/hub/src/rules/hub.md", "# Hub\n");
        render(&mut s, &Env::default()).unwrap();
        assert!(s.ws.is_file("/proj/.claude-hub/rules/hub.md"));
        assert!(s.ws.is_file("/proj/.claude-hub/rules/core.md"));
        assert!(!s.ws.exists("/proj/.claude/rules/hub.md"));
        assert!(!s.ws.exists("/<agentsync-overlay>/profile"));
    }
}
