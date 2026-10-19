//! The run of `lib/sync.sh`: config and sources, the `shared:` and engine skill
//! overlays, every enabled tool and selected profile rendered into the
//! workspace, disabled tools cleaned. `check` renders forced and in memory;
//! `cli::sync` runs the stages on disk with its transaction between them.

mod env;
mod passes;
mod prepare;
mod steps;
mod tools;

pub use env::{BackupBounds, Env, Run, Selection};
pub use passes::run_passes;
pub use prepare::{
    banner, check_version_pin, prepare, refuse_configless_cleanup, refuse_escaping_source_links,
    setup_overlays,
};
pub use tools::{build_catalog, collect_legacy_targets};

use crate::Error;
use crate::engine::session::Session;
use crate::{config::profiles, engine::overlay};

pub const TARGET_KEYS: [&str; 9] = [
    "agents",
    "rules",
    "skills",
    "commands",
    "subagents",
    "settings",
    "mcp",
    "hooks",
    "guard",
];

/// A render stopped the way `sync.sh` exits: the status after its log lines.
#[derive(Debug, PartialEq, Eq)]
pub struct Stop(pub u8);

pub type Step = Result<(), Stop>;

pub struct SkillSource {
    pub effective: String,
    pub origins: Vec<String>,
}

pub fn skill_source(
    s: &mut Session,
    env: &Env,
    profile: Option<&str>,
) -> Result<SkillSource, Stop> {
    let mut run = prepare(s, env, Selection::default())?;
    refuse_escaping_source_links(s, &run)?;
    let configured = s
        .paths
        .clone()
        .resolve_source(&run.sources.skills, "source.skills", &mut s.log)
        .ok_or(Stop(1))?;
    setup_overlays(s, &mut run, true)?;

    let mut origins = Vec::new();
    if let Some(name) = profile {
        let config = run.config.as_deref().unwrap_or_default();
        if !profiles::names(config)
            .iter()
            .any(|candidate| candidate == name)
        {
            s.log.error(&format!("Unknown profile: {name}"));
            return Err(Stop(1));
        }
        let overlay_dir = profiles::overlay_dir(config, name);
        let profile_root = s.paths.absolute(&overlay_dir);
        let prior_skills_source = run.sources.skills.clone();
        overlay::setup_profile(s, config, name, &run.profile_parent_src, &mut run.sources)
            .map_err(|e| io(s, e))?;
        let profile_skills_overlay = run.sources.skills != prior_skills_source;
        if profile_skills_overlay {
            origins.push(format!("{profile_root}/src/skills"));
            if run.profile_base_src == format!("{}/.ai/src", s.paths.root) {
                origins.push(format!("{}/.ai/src/skills", s.paths.root));
            }
        }
    }
    origins.push(configured);
    if let Some(config) = run.config.as_deref() {
        if overlay::inherit_categories(&crate::config::yaml_subset::value(config, "shared.inherit"))
            .contains(&"skills")
            && let Some(parent) = overlay::shared_parent_src(config, &s.paths.root)
        {
            origins.push(format!("{parent}/skills"));
        }
        if crate::config::yaml_subset::value(config, "base_skills") != "false" {
            origins.push(format!(
                "{}/lib/templates/base-src/skills",
                crate::paths::ENGINE_ROOT
            ));
        }
    } else {
        origins.push(format!(
            "{}/lib/templates/base-src/skills",
            crate::paths::ENGINE_ROOT
        ));
    }
    let effective = s
        .paths
        .clone()
        .resolve_source(&run.sources.skills, "source.skills", &mut s.log)
        .ok_or(Stop(1))?;
    Ok(SkillSource { effective, origins })
}

fn io(s: &mut Session, error: Error) -> Stop {
    s.log.err(error.to_string());
    Stop(1)
}

/// What `check` needs: `sync.sh --force` without its transaction and without
/// `shared:`, which `lib/check.sh` merged into the workspace beforehand.
pub fn render(s: &mut Session, env: &Env) -> Result<Run, Stop> {
    let mut run = prepare(s, env, Selection::default())?;
    refuse_configless_cleanup(s, &run)?;
    refuse_escaping_source_links(s, &run)?;
    check_version_pin(s, &run)?;
    banner(s);
    setup_overlays(s, &mut run, false)?;
    build_catalog(s, &mut run);
    run_passes(s, &mut run)?;
    Ok(run)
}

/// Where a trapped signal ends the run: Bash's trap fires once the command in
/// progress returns.
pub fn checkpoint(s: &Session) -> Step {
    match s.interrupted() {
        Some(status) => Err(Stop(status)),
        None => Ok(()),
    }
}

#[cfg(test)]
mod test_support {
    use crate::engine::session::{Session, test_session};
    use crate::engine::workspace::Content;

    pub(super) fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    pub(super) fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    pub(super) fn project() -> Session {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agents\n");
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: Review\n---\nBody\n",
        );
        s
    }
}
