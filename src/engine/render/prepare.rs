//! The prepare phase: config and sources, the guards that stop a run, the banner, and the overlays.

use crate::paths::DiskText;
use std::collections::BTreeSet;

use super::tools::{load_tool, user_tool_slugs};
use super::{Env, Run, Selection, Step, Stop, io};
use crate::engine::overlay::{self, Sources};
use crate::engine::session::Session;
use crate::{
    config::catalog, config::project_config, config::version, config::yaml_subset, engine_version,
    paths, transaction::backup,
};

/// `_load_run_config` and `_resolve_sources`.
pub fn prepare(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
    let mut run = load_run_config(s, env, selection)?;
    resolve_sources(s, env, &mut run)?;
    Ok(run)
}

/// `resolve_project_config_path` and `_load_run_config`.
fn load_run_config(s: &mut Session, env: &Env, selection: Selection) -> Result<Run, Stop> {
    let root = s.paths.root.clone();
    let chosen = project_config::select(&root, env.config_path.as_deref(), &|path: &str| {
        s.ws.is_file(path)
    });
    let config_path = match chosen {
        project_config::Selection::Found(path) => Some(path),
        project_config::Selection::None => None,
        project_config::Selection::Missing(path) => {
            s.log.error(&project_config::missing_message(&path));
            return Err(Stop(1));
        }
    };
    let config = match &config_path {
        Some(path) => {
            let bytes = s.ws.read(path).map_err(|e| io(s, e))?;
            Some(String::from_utf8_lossy(&bytes).into_owned())
        }
        None => None,
    };
    let mut retention = backup::Retention::Bounded;
    if let Some(bounds) = &env.backup {
        let selected = config_path.as_deref().zip(config.as_deref());
        match backup::configure(selected, bounds.limit.as_deref(), bounds.max_age.as_deref()) {
            Ok(policy) => retention = policy,
            Err(e) => {
                s.log.err(format!("Error: {e}"));
                return Err(Stop(1));
            }
        }
    }

    fn env_set(value: &Option<String>) -> Option<&str> {
        value.as_deref().filter(|v| !v.is_empty())
    }
    let allow_post_sync = match env_set(&env.allow_post_sync) {
        Some(value) => value == "true",
        None => yaml_subset::value(catalog::GLOBAL_CONFIG, "post_sync.allow") == "true",
    };
    let mut skip_post_sync = env_set(&env.skip_post_sync) == Some("true");
    let mut cleanup = "true".to_string();
    let mut update_gitignore = true;
    let mut outputs = "local";
    let mut version_pin = version::Mode::Warn;
    if let (Some(text), Some(path)) = (&config, &config_path) {
        version_pin = match version::mode(text) {
            Ok(mode) => mode,
            Err(value) => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown version_pin.mode '{value}' in {shown} — expected 'warn' or 'strict'"
                ));
                return Err(Stop(1));
            }
        };
        let configured = yaml_subset::value(text, "defaults.cleanup");
        if !configured.is_empty() {
            cleanup = configured;
        }
        if env_set(&env.skip_post_sync).is_none()
            && yaml_subset::value(text, "post_sync.skip") == "true"
        {
            skip_post_sync = true;
        }
        update_gitignore = yaml_subset::value(text, "gitignore.update") != "false";
        outputs = match project_config::outputs_mode(text) {
            Ok(mode) => mode,
            Err(other) => {
                let shown = path
                    .strip_prefix(&format!("{root}/"))
                    .unwrap_or(path)
                    .to_string();
                s.log.error(&format!(
                    "Unknown outputs mode '{other}' in {shown} — expected 'committed' or 'local'"
                ));
                return Err(Stop(1));
            }
        };
    }

    Ok(Run {
        config,
        config_path,
        cleanup,
        update_gitignore,
        outputs,
        version_pin,
        retention,
        skip_post_sync,
        allow_post_sync,
        sources: Sources::default(),
        base_sources: Sources::default(),
        profile_base_src: String::new(),
        profile_parent_src: String::new(),
        selection,
        profiles: Vec::new(),
        enabled: BTreeSet::new(),
        profile_tools: BTreeSet::new(),
        protected: Vec::new(),
        backup_targets: Vec::new(),
        keyed_dests: BTreeSet::new(),
        gitignore_generated: Vec::new(),
        gitignore_profile: Vec::new(),
        tools: Vec::new(),
        printed: false,
        synced: 0,
        skipped: 0,
        total: 0,
        skipped_names: Vec::new(),
    })
}

/// `_resolve_sources`.
fn resolve_sources(s: &mut Session, env: &Env, run: &mut Run) -> Step {
    let config = run.config.as_deref();
    let sources = source_paths(s, config);
    register_explicit_sources(s, env, config)?;
    s.tools_dir = tools_dir(s, config);
    require_agents(s, &sources.agents)?;
    run.sources = sources;
    Ok(())
}

/// Each source from the global default, then the detected `.ai/src/` or
/// `.ai/` entry, then the project config's `source.<key>` or bare `<key>`.
fn source_paths(s: &Session, config: Option<&str>) -> Sources {
    let global = catalog::GLOBAL_CONFIG;
    let root = &s.paths.root;
    let detect = |is_file: bool, sub: &str| -> Option<String> {
        [format!(".ai/src/{sub}"), format!(".ai/{sub}")]
            .into_iter()
            .find(|rel| {
                let abs = format!("{root}/{rel}");
                if is_file {
                    s.ws.is_file(&abs)
                } else {
                    s.ws.is_dir(&abs)
                }
            })
    };
    let mut sources = Sources {
        agents: yaml_subset::value(global, "source.agents"),
        rules: yaml_subset::value(global, "source.rules"),
        skills: yaml_subset::value(global, "source.skills"),
        commands: String::new(),
        subagents: String::new(),
    };
    for (is_file, sub, slot) in [
        (true, "AGENTS.md", &mut sources.agents),
        (false, "rules", &mut sources.rules),
        (false, "skills", &mut sources.skills),
        (false, "commands", &mut sources.commands),
        (false, "agents", &mut sources.subagents),
    ] {
        if let Some(found) = detect(is_file, sub) {
            *slot = found;
        }
    }
    let Some(text) = config else {
        return sources;
    };
    for (key, slot) in [
        ("agents", &mut sources.agents),
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("subagents", &mut sources.subagents),
    ] {
        let nested = yaml_subset::value(text, &format!("source.{key}"));
        let chosen = if nested.is_empty() {
            yaml_subset::value(text, key)
        } else {
            nested
        };
        if !chosen.is_empty() {
            *slot = chosen;
        }
    }
    sources
}

/// Every `source.*` outside the project registered as an explicit root; one
/// that is refused or untrusted stops the run.
fn register_explicit_sources(s: &mut Session, env: &Env, config: Option<&str>) -> Step {
    s.paths
        .trust_external_roots(env.external_source_roots.as_deref());
    let mut explicit = Vec::new();
    for key in [
        "agents",
        "rules",
        "skills",
        "tools",
        "commands",
        "subagents",
    ] {
        let raw = config
            .map(|text| yaml_subset::value(text, &format!("source.{key}")))
            .unwrap_or_default();
        if raw.is_empty() {
            continue;
        }
        match s.paths.classify_explicit_source(&raw) {
            paths::ExplicitSource::Inside => {}
            paths::ExplicitSource::Outside(canonical) => explicit.push(canonical),
            paths::ExplicitSource::Refused(canonical) => {
                s.log.error(&format!(
                    "source.{key} must not be the filesystem root, the home directory, or the project root or its ancestor: {raw} -> {canonical}"
                ));
                return Err(Stop(1));
            }
            paths::ExplicitSource::Untrusted(canonical) => {
                s.log.error(&format!(
                    "source.{key} points outside the project at {canonical}, which AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list; add that directory (or a parent) to the variable to read from it"
                ));
                return Err(Stop(1));
            }
        }
    }
    s.paths.register_explicit_roots(explicit);
    Ok(())
}

fn tools_dir(s: &Session, config: Option<&str>) -> String {
    let configured = config
        .map(|text| yaml_subset::value(text, "source.tools"))
        .unwrap_or_default();
    if configured.is_empty() {
        format!("{}/.ai/src/tools", s.paths.root)
    } else {
        s.paths.absolute(&configured)
    }
}

fn require_agents(s: &mut Session, agents: &str) -> Step {
    let agents_abs = s
        .paths
        .clone()
        .resolve_source(agents, "source.agents", &mut s.log)
        .ok_or(Stop(1))?;
    if s.ws.is_file(&agents_abs) {
        return Ok(());
    }
    s.log
        .error(&format!("Source agents file not found: {agents_abs}"));
    s.log
        .error("Run 'exuno init' or set source.agents in agent_sync.yaml");
    Err(Stop(1))
}

/// `_refuse_configless_cleanup_or_exit`: without a project config, a write run
/// whose tools are all disabled would only remove every tool's outputs.
pub fn refuse_configless_cleanup(s: &mut Session, run: &Run) -> Step {
    if run.config.is_some() || s.dry_run {
        return Ok(());
    }
    let legacy_enabled = user_tool_slugs(s)
        .iter()
        .any(|slug| load_tool(s, slug).user_value("enabled") == "true");
    if legacy_enabled {
        return Ok(());
    }
    s.log.error(
        "No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'exuno enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH.",
    );
    Err(Stop(1))
}

/// `_refuse_escaping_source_links_or_exit`: `.ai/` without its backups, then
/// the resolved sources, then the tool overrides.
pub fn refuse_escaping_source_links(s: &mut Session, run: &Run) -> Step {
    let ai = format!("{}/.ai", s.paths.root);
    let mut names: Vec<String> = std::fs::read_dir(&ai)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().disk_text())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    let (dotted, plain): (Vec<String>, Vec<String>) =
        names.into_iter().partition(|name| name.starts_with('.'));
    let mut roots: Vec<String> = plain
        .into_iter()
        .chain(dotted.into_iter().filter(|name| !name.starts_with("..")))
        .filter(|name| name != "backups")
        .map(|name| format!("{ai}/{name}"))
        .collect();
    let sources = &run.sources;
    for raw in [
        &sources.agents,
        &sources.rules,
        &sources.skills,
        &sources.commands,
        &sources.subagents,
    ] {
        if !raw.is_empty() {
            roots.push(s.paths.absolute(raw));
        }
    }
    roots.push(s.tools_dir.clone());
    if let Err(message) = s.paths.escaping_source_link(&roots) {
        s.log.error(&message);
        return Err(Stop(1));
    }
    Ok(())
}

/// `_check_version_pin_or_exit`.
pub fn check_version_pin(s: &mut Session, run: &Run) -> Step {
    let Some(config) = &run.config else {
        return Ok(());
    };
    let pinned = crate::config::names::pinned_version(config);
    let engine = engine_version();
    if pinned.is_empty() || pinned == engine {
        return Ok(());
    }
    let hint = version::hint(&pinned, engine, |cmd| s.log.command(cmd));
    let committed = run.outputs == "committed";
    if committed || run.version_pin == version::Mode::Strict {
        s.log
            .error(&version::mismatch_error(&pinned, engine, committed));
        for line in hint {
            s.log.err(line);
        }
        return Err(Stop(1));
    }
    s.log.warning(&format!(
        "This project pins exuno {pinned} but you are running {engine}."
    ));
    for line in hint {
        s.log.err(line);
    }
    Ok(())
}

/// `_print_banner`: only a dry run announces itself; a real run's first line
/// is its first tool.
pub fn banner(s: &mut Session) {
    if s.dry_run {
        s.log.info("Dry run: nothing will be written");
        s.log.blank();
    }
}

/// `shared_setup_overlay` when `shared` is set, `base_src_setup_overlay`, and
/// `_snapshot_base_sources`.
pub fn setup_overlays(s: &mut Session, run: &mut Run, shared: bool) -> Step {
    let config = run.config.clone();
    let mut child_src = format!("{}/.ai/src", s.paths.root);
    if shared
        && let Some(text) = config.as_deref()
        && let Some(dir) = overlay::setup_shared(s, text, &mut run.sources).map_err(|e| io(s, e))?
    {
        child_src = format!("{dir}/src");
    }
    let engine_skills = overlay::setup_base_src(s, config.as_deref(), &child_src, &mut run.sources)
        .map_err(|e| io(s, e))?;
    run.base_sources = run.sources.clone();
    run.profile_parent_src =
        engine_skills.map_or_else(|| child_src.clone(), |dir| format!("{dir}/src"));
    run.profile_base_src = child_src;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::engine::render::test_support::{file, project, text_of};
    use crate::engine::render::{BackupBounds, Env, Stop, render};
    use crate::engine::session::test_session;
    use crate::engine_version;

    #[test]
    fn a_project_without_agents_md_stops_with_status_one() {
        let mut s = test_session();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(2),
            [
                "[ERROR] Source agents file not found: /proj/.ai/src/AGENTS.md",
                "[ERROR] Run 'exuno init' or set source.agents in agent_sync.yaml"
            ]
        );
    }

    #[test]
    fn an_unknown_outputs_mode_stops_before_the_banner() {
        let mut s = project();
        file(&mut s, "/proj/.ai/agent_sync.yaml", "outputs: shared\n");
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(1),
            [
                "[ERROR] Unknown outputs mode 'shared' in .ai/agent_sync.yaml — expected 'committed' or 'local'"
            ]
        );
    }

    #[test]
    fn an_invalid_backup_policy_stops_before_the_pin_mode_when_the_run_backs_up() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "backup:\n  retention: typo\nversion_pin:\n  mode: refuse\n",
        );
        let env = Env {
            backup: Some(BackupBounds::default()),
            ..Env::default()
        };
        assert_eq!(render(&mut s, &env), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "Error: Invalid backup.retention 'typo' in /proj/.ai/agent_sync.yaml; expected bounded or preserve"
            ]
        );

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nbackup:\n  retention: typo\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
    }

    #[test]
    fn source_tools_moves_the_tool_overrides() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "source:\n  tools: \"catalog\"\n",
        );
        file(&mut s, "/proj/catalog/claude.yaml", "enabled: true\n");
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
        assert_eq!(s.tools_dir, "/proj/catalog");
        assert!(s.ws.is_file("/proj/CLAUDE.md"));
    }

    #[cfg(unix)]
    #[test]
    fn a_source_at_the_filesystem_root_stops() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\nsource:\n  rules: \"/\"\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "[ERROR] source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: / -> /"
            ]
        );
    }

    fn with_config(path: &str) -> Env {
        Env {
            config_path: Some(path.to_string()),
            ..Env::default()
        }
    }

    #[test]
    fn a_missing_explicit_config_stops_without_falling_back() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "tools:\n  enabled: [claude]\n",
        );
        assert_eq!(render(&mut s, &with_config("missing.yaml")), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            ["[ERROR] AGENTSYNC_CONFIG_PATH is set but file not found: /proj/missing.yaml"]
        );
    }

    #[test]
    fn without_a_config_a_run_with_no_enabled_tool_is_refused() {
        let mut s = project();
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "[ERROR] No project configuration found and no tool is enabled; refusing a sync that would remove every tool's outputs. Run 'exuno enable <tool>' to create .ai/agent_sync.yaml, or set AGENTSYNC_CONFIG_PATH."
            ]
        );
    }

    #[test]
    fn without_a_config_a_tool_enabled_in_its_own_yaml_still_renders() {
        let mut s = project();
        file(&mut s, "/proj/.ai/src/tools/claude.yaml", "enabled: true\n");
        assert_eq!(render(&mut s, &Env::default()), Ok(()));
        assert_eq!(text_of(&s, "/proj/CLAUDE.md"), "# Agents\n");
    }

    #[test]
    fn a_strict_pin_stops_local_outputs_and_an_unknown_mode_stops_first() {
        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "outputs: local\nagentsync_version: \"0.0.1\"\nversion_pin:\n  mode: strict\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        let engine = engine_version();
        assert_eq!(
            s.log.tail(3),
            [
                format!("[ERROR] This project pins exuno 0.0.1 but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version.").as_str(),
                "  • Match the pin:  exuno update 0.0.1",
                format!("  • Or move it:     exuno upgrade-config   (re-pins to {engine}; re-sync and commit the outputs)").as_str(),
            ]
        );

        let mut s = project();
        file(
            &mut s,
            "/proj/.ai/agent_sync.yaml",
            "version_pin:\n  mode: refuse\noutputs: shared\n",
        );
        assert_eq!(render(&mut s, &Env::default()), Err(Stop(1)));
        assert_eq!(
            s.log.tail(5),
            [
                "[ERROR] Unknown version_pin.mode 'refuse' in .ai/agent_sync.yaml — expected 'warn' or 'strict'"
            ]
        );
    }
}
