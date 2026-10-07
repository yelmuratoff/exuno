//! `exuno adopt`: `cmd_adopt` of `lib/helpers/adopt.sh`, which promotes a
//! manual edit in a generated file back into its source.

use crate::paths::DiskText;
use std::collections::BTreeSet;
use std::io::Write;
use std::path::Path;
use std::process::Command;

use super::put;
use crate::config::payload::{self, Source};
use crate::config::tool::Tool;
use crate::engine::keyed::{self, Format, KeyPath, Owned};
use crate::engine::{skill_tree, workspace::Workspace};
use crate::output::help::{Help, Section};
use crate::output::log::Log;
use crate::output::style::Style;
use crate::paths::{self, Paths};
use crate::project::Project;
use crate::transaction::manifest::{self, Manifest};
use crate::{Error, config::catalog, config::template_manifest, config::yaml_subset};

type Discover<'a> = &'a dyn Fn() -> Result<Project, Error>;
type Confirm<'a> = &'a mut dyn FnMut(&str) -> bool;

pub const HELP: Help = Help {
    command: "adopt",
    tagline: "promote a manual edit back into .ai/src/",
    synopsis: &["adopt <dest-file> [OPTIONS]", "adopt --all [OPTIONS]"],
    description: &[
        "Promote a manual edit in a destination file back into .ai/src/ as the\nnew canonical content. Refuses transformed targets (merged rules,\ninlined skills, format-converted commands/subagents).",
        "With --all, adopt every drifted (manually-edited) tracked output at\nonce, skipping refused targets and same-source conflicts.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            ("-a, --all", "Adopt every drifted output (no <dest-file>)"),
            ("--dry-run", "Show the plan without writing"),
            ("-y, --yes", "Skip confirmation (required outside a TTY)"),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "adopt CLAUDE.md",
        "adopt .claude/rules/core.md --dry-run",
        "adopt --all --yes",
    ],
};

/// `SOURCE_*` as `_adopt_discover_sources` sets them.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub agents: String,
    pub rules: String,
    pub skills: String,
    pub commands: String,
    pub subagents: String,
}

/// `_adopt_discover_sources`: the `.ai/src/` layout, else the flat `.ai/`
/// one, then the project's `source.<key>` or root-level `<key>`.
pub fn discover_sources(project: &Project) -> Result<Sources, Error> {
    let root = &project.root;
    let pick = |name: &str, file: bool| {
        [format!(".ai/src/{name}"), format!(".ai/{name}")]
            .into_iter()
            .find(|rel| {
                let path = root.join(rel);
                if file { path.is_file() } else { path.is_dir() }
            })
            .unwrap_or_default()
    };
    let mut sources = Sources {
        agents: pick("AGENTS.md", true),
        rules: pick("rules", false),
        skills: pick("skills", false),
        commands: pick("commands", false),
        subagents: pick("agents", false),
    };
    let Some(config_path) = project.config_path.as_ref().filter(|p| p.is_file()) else {
        return Ok(sources);
    };
    let config = std::fs::read(config_path)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .map_err(|e| Error::io(config_path, e))?;
    for (key, slot) in [
        ("agents", &mut sources.agents),
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("subagents", &mut sources.subagents),
    ] {
        let mut value = yaml_subset::value(&config, &format!("source.{key}"));
        if value.is_empty() {
            value = yaml_subset::value(&config, key);
        }
        if !value.is_empty() {
            *slot = value;
        }
    }
    Ok(sources)
}

/// A destination mapped to the source it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    pub tool: String,
    pub resource: &'static str,
    pub dest_rel: String,
    pub dest_abs: String,
    pub source_abs: String,
    pub source_rel: String,
    /// The destination is a settings file sync owns by key, not whole.
    pub keyed: bool,
    /// Server entries in the file come from an MCP source in another format.
    pub units_from_mcp: bool,
}

/// What `_adopt_resolve_dest` needs besides the destination.
pub struct Resolver<'a> {
    pub project: &'a Project,
    pub paths: Paths,
    pub sources: Sources,
    tools: Vec<String>,
    blocked_agents: Vec<(String, String)>,
    style: &'a Style,
    warned_legacy: bool,
}

impl<'a> Resolver<'a> {
    pub fn new(project: &'a Project, sources: Sources, style: &'a Style) -> Result<Self, Error> {
        let root = project.root.disk_text();
        let mut tools = catalog::base_tools();
        tools.extend(project.user_override_tools()?);
        tools.sort();
        tools.dedup();
        Ok(Self {
            project,
            paths: Paths::on_disk(&root),
            sources,
            tools,
            blocked_agents: Vec::new(),
            style,
            warned_legacy: false,
        })
    }

    fn for_adopt(mut self) -> Result<Self, Error> {
        let enabled = self.project.enabled_tools()?;
        self.tools.sort_by_key(|slug| !enabled.contains(slug));
        for slug in &enabled {
            let tool = Tool::load(self.project, slug)?;
            if tool.flag("targets.agents.adoptable") == Some(false)
                && let Some(dest) = self.dest_for(&tool, "agents")
            {
                self.blocked_agents.push((dest, slug.clone()));
            }
        }
        Ok(self)
    }

    fn root(&self) -> String {
        self.project.root.disk_text()
    }

    fn strip_root(&self, abs: &str) -> String {
        abs.strip_prefix(&format!("{}/", self.root()))
            .unwrap_or(abs)
            .to_string()
    }

    /// `_adopt_resolve_dest`: the adoption, or the refusal reason.
    pub fn resolve(
        &mut self,
        raw: &str,
        err: &mut dyn Write,
    ) -> Result<Result<Adoption, String>, Error> {
        let abs = self.paths.absolute(raw);
        let Some(canonical) = self.paths.canonicalize_with_existing_ancestor(&abs) else {
            return Ok(Err(format!("Cannot resolve path: {raw}")));
        };
        if !paths::is_within(&canonical, &self.paths.root_canonical) {
            return Ok(Err(format!("Path is outside the project: {raw}")));
        }
        if !Path::new(&abs).is_file() {
            return Ok(Err(format!("Destination file not found: {raw}")));
        }
        let dest_rel = self.strip_root(&abs);
        if let Some((_, slug)) = self.blocked_agents.iter().find(|(dest, _)| dest == &abs) {
            return Ok(Err(format!(
                "{slug} has generated content in {dest_rel}. Edit the source AGENTS.md and rules instead."
            )));
        }
        for slug in self.tools.clone() {
            let tool = Tool::load(self.project, &slug)?;
            if let Some(found) = self.try_tool(&tool, &abs, &dest_rel, err)? {
                return Ok(found);
            }
        }
        Ok(Err(format!(
            "{dest_rel} is not a recognised Exuno output (no enabled tool produces it)."
        )))
    }

    fn dest_for(&self, tool: &Tool, key: &str) -> Option<String> {
        if tool.flag(&format!("targets.{key}.enabled")) == Some(false) {
            return None;
        }
        let raw = tool.value(&format!("targets.{key}.dest"));
        if raw.is_empty() {
            return None;
        }
        self.paths.resolve_dest(
            &raw,
            &format!("targets.{key}.dest for {}", tool.slug),
            &mut Log::default(),
        )
    }

    fn adoption(&self, tool: &Tool, resource: &'static str, dest: (&str, &str)) -> Adoption {
        Adoption {
            tool: tool.slug.clone(),
            resource,
            dest_abs: dest.0.to_string(),
            dest_rel: dest.1.to_string(),
            source_abs: String::new(),
            source_rel: String::new(),
            keyed: false,
            units_from_mcp: false,
        }
    }

    /// `_adopt_try_tool`: `None` when the tool produces no such output.
    fn try_tool(
        &mut self,
        tool: &Tool,
        abs: &str,
        dest_rel: &str,
        err: &mut dyn Write,
    ) -> Result<Option<Result<Adoption, String>>, Error> {
        let dest = (abs, dest_rel);
        if self.dest_for(tool, "agents").as_deref() == Some(abs) {
            if tool.flag("targets.agents.adoptable") == Some(false) {
                return Ok(Some(Err(format!(
                    "{} has generated content in {}. Edit the source AGENTS.md and rules instead.",
                    tool.slug, dest_rel
                ))));
            }
            return Ok(Some(
                self.agents_source(tool, self.adoption(tool, "agents", dest)),
            ));
        }
        if self.dest_for(tool, "settings").as_deref() == Some(abs) {
            if tool.keyed("settings", self.paths.root_is_home()) {
                let units_from_mcp = tool.composed()
                    && tool.flag("targets.mcp.enabled") != Some(false)
                    && self.payload_source(tool, "mcp", err)?.is_some();
                let found = self.payload_target(tool, self.adoption(tool, "settings", dest))?;
                return Ok(Some(found.map(|found| Adoption {
                    keyed: true,
                    units_from_mcp,
                    ..found
                })));
            }
            if tool.value("targets.mcp.format") == "codex_toml"
                && let Some(mcp) = self.payload_source(tool, "mcp", err)?
            {
                let settings = self
                    .payload_source(tool, "settings", err)?
                    .map(|source| self.strip_root(&source.shown()))
                    .unwrap_or_default();
                return Ok(Some(Err(format!(
                    "Codex config.toml is a multi-source output. Edit {settings} and {} separately.",
                    self.strip_root(&mcp.shown())
                ))));
            }
            if tool.value("targets.mcp.format") == "opencode_json"
                && let Some(mcp) = self.payload_source(tool, "mcp", err)?
            {
                let settings = self
                    .payload_source(tool, "settings", err)?
                    .map(|source| self.strip_root(&source.shown()))
                    .unwrap_or_default();
                return Ok(Some(Err(format!(
                    "OpenCode opencode.json is a multi-source output. Edit {settings} and {} separately.",
                    self.strip_root(&mcp.shown())
                ))));
            }
            return Ok(Some(
                self.payload_target(tool, self.adoption(tool, "settings", dest))?,
            ));
        }
        for resource in ["mcp", "hooks"] {
            if self.dest_for(tool, resource).as_deref() == Some(abs) {
                let found = self.payload_target(tool, self.adoption(tool, resource, dest))?;
                let keyed = resource == "mcp" && tool.keyed("mcp", self.paths.root_is_home());
                return Ok(Some(found.map(|found| Adoption { keyed, ..found })));
            }
        }
        if tool.value("targets.rules.merge_to_file") == "true"
            && self.dest_for(tool, "rules").as_deref() == Some(abs)
        {
            return Ok(Some(self.dir_source(
                tool,
                self.adoption(tool, "rules", dest),
                abs,
            )));
        }
        let mut best: Option<(&'static str, String)> = None;
        for key in ["rules", "skills", "commands", "subagents"] {
            let Some(dir) = self.dest_for(tool, key) else {
                continue;
            };
            let inside = abs.starts_with(&format!("{dir}/"));
            if inside && dir.len() > best.as_ref().map_or(0, |(_, d)| d.len()) {
                best = Some((key, dir));
            }
        }
        Ok(best.map(|(key, dir)| self.dir_source(tool, self.adoption(tool, key, dest), &dir)))
    }

    /// `resolve_payload_source`, printing its legacy-layout warning once.
    fn payload_source(
        &mut self,
        tool: &Tool,
        resource: &str,
        err: &mut dyn Write,
    ) -> Result<Option<Source>, Error> {
        let (source, legacy) = payload::effective_source(self.project, tool, resource)?;
        if let Some(path) = legacy
            && !self.warned_legacy
        {
            self.warned_legacy = true;
            put(
                err,
                payload::legacy_warning(self.project, &path, self.style).as_bytes(),
            )?;
        }
        Ok(source)
    }

    /// `_adopt_resolve_agents_source`.
    fn agents_source(&self, tool: &Tool, mut found: Adoption) -> Result<Adoption, String> {
        let over = tool.value("targets.agents.source");
        let raw = if over.is_empty() {
            self.sources.agents.clone()
        } else {
            over
        };
        if raw.is_empty() {
            return Err(format!(
                "No agents source resolved for {} — set source.agents in exuno.yaml or place AGENTS.md in .ai/src/.",
                found.tool
            ));
        }
        if crate::paths::is_absolute(&raw) {
            found.source_rel = self.strip_root(&raw);
            found.source_abs = raw;
        } else {
            found.source_abs = format!("{}/{raw}", self.root());
            found.source_rel = raw;
        }
        Ok(found)
    }

    /// `_adopt_resolve_payload_target`.
    fn payload_target(
        &self,
        tool: &Tool,
        mut found: Adoption,
    ) -> Result<Result<Adoption, String>, Error> {
        let resource = found.resource;
        let existing = match payload::find_new_override(self.project, &tool.slug, resource)? {
            Some(path) => Some(path),
            None => payload::legacy_override_path(self.project, tool, resource)
                .filter(|path| path.is_file()),
        };
        let root = self.root();
        let chosen = if let Some(path) = existing {
            Some(path.disk_text())
        } else {
            let declared = if self.project.user_tool_file(&tool.slug).is_file() {
                tool.user_value(&format!("targets.{resource}.source"))
            } else {
                String::new()
            };
            let declared_abs = if declared.is_empty() || crate::paths::is_absolute(&declared) {
                declared
            } else {
                format!("{root}/{declared}")
            };
            if declared_abs.starts_with(&format!("{root}/")) {
                Some(declared_abs)
            } else {
                payload::override_path(self.project, tool, resource).map(|path| path.disk_text())
            }
        };
        let Some(abs) = chosen else {
            return Ok(Err(format!(
                "No base template for {} {resource} — cannot pick a canonical override path.",
                tool.slug
            )));
        };
        found.source_rel = self.strip_root(&abs);
        found.source_abs = abs;
        Ok(Ok(found))
    }

    /// `_adopt_resolve_dir_source`.
    fn dir_source(
        &self,
        tool: &Tool,
        mut found: Adoption,
        dest_dir: &str,
    ) -> Result<Adoption, String> {
        let key = found.resource;
        let slug = &tool.slug;
        let value = |path: &str| tool.value(path);
        let refusal = match key {
            "rules" if value("targets.rules.merge_to_file") == "true" => Some(format!(
                "{slug} merges rules into a single file. Edit the source rules in {}/ instead.",
                self.sources.rules
            )),
            "rules" if value("targets.rules.inline_into_agents") == "true" => Some(format!(
                "{slug} inlines rules into AGENTS.md. Edit the source rules in {}/ instead.",
                self.sources.rules
            )),
            "rules"
                if !value("targets.rules.header").is_empty()
                    || !value("targets.rules.scoped_header").is_empty() =>
            {
                Some(format!(
                    "{slug} injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit {}/ instead.",
                    self.sources.rules
                ))
            }
            "skills" if value("targets.skills.inline_into_agents") == "true" => Some(format!(
                "{slug} inlines a skill index into AGENTS.md. Edit {}/ instead.",
                self.sources.skills
            )),
            "commands" if value("targets.commands.format") == "toml" => Some(format!(
                "{slug} serializes commands as TOML. Conversion is not reversible — edit {}/ instead.",
                self.sources.commands
            )),
            "subagents" => {
                let format = value("targets.subagents.format");
                matches!(format.as_str(), "toml" | "amazonq_json" | "opencode_md" | "kiro_md").then(|| {
                    format!(
                        "{slug} serializes subagents as {format}. Conversion is not reversible — edit {}/ instead.",
                        self.sources.subagents
                    )
                })
            }
            _ => None,
        };
        if let Some(reason) = refusal {
            return Err(reason);
        }

        let over = value(&format!("targets.{key}.source"));
        let fallback = match key {
            "rules" => &self.sources.rules,
            "skills" => &self.sources.skills,
            "commands" => &self.sources.commands,
            _ => &self.sources.subagents,
        };
        let raw = if over.is_empty() {
            fallback.clone()
        } else {
            over
        };
        let src_root = if raw.is_empty() {
            None
        } else {
            self.paths.resolve_source(
                &raw,
                &format!("targets.{key}.source for {slug}"),
                &mut Log::default(),
            )
        };
        let Some(src_root) = src_root else {
            return Err(format!("No source directory resolved for {slug} {key}."));
        };

        let mut rel_inside = found
            .dest_abs
            .strip_prefix(&format!("{dest_dir}/"))
            .unwrap_or(&found.dest_abs)
            .to_string();
        if key == "skills" {
            rel_inside =
                skill_tree::discover(&Workspace::on_disk(&src_root), &src_root).locate(&rel_inside);
        } else {
            let ext = value(&format!("targets.{key}.extension"));
            if !ext.is_empty()
                && let Some(stem) = rel_inside.strip_suffix(&ext)
            {
                rel_inside = format!("{stem}.md");
            }
        }
        found.source_abs = format!("{src_root}/{rel_inside}");
        found.source_rel = self.strip_root(&found.source_abs);
        Ok(found)
    }
}

pub fn adopt(
    args: &[String],
    discover: Discover,
    style: &Style,
    interactive: bool,
    confirm: Confirm,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let parsed = match parse_args(args) {
        Ok(parsed) => parsed,
        Err(ParseStop::Help) => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Err(ParseStop::Refuse(message)) => {
            put(
                err,
                format!("{}: {message}\n", style.red("Error")).as_bytes(),
            )?;
            return Ok(2);
        }
        Err(ParseStop::MissingDest) => {
            let help = HELP.render(style);
            let text = format!("{}: missing <dest-file>\n{help}", style.red("Error"));
            put(err, text.as_bytes())?;
            return Ok(2);
        }
    };

    let project = match discover() {
        Ok(project) => project,
        Err(Error::ConfigPathNotFound(path)) => {
            put(
                err,
                format!(
                    "{}: EXUNO_CONFIG_PATH is set but file not found: {}\n",
                    style.red("Error"),
                    path.disk_text()
                )
                .as_bytes(),
            )?;
            return Ok(2);
        }
        Err(other) => return Err(other),
    };
    if !project.tools_dir_in_project() {
        return super::refuse_outside_tools_dir(&project, style, err);
    }
    let sources = discover_sources(&project)?;
    let root = project.root.disk_text();
    let loaded = Manifest::load(&root)?;
    if loaded.is_none() && parsed.all {
        put(
            err,
            format!(
                "{}: no .ai/.sync-manifest yet — run {} first, or adopt one file at a time.\n",
                style.red("Error"),
                style.cyan("exuno sync")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    let mut resolver = Resolver::new(&project, sources, style)?.for_adopt()?;
    let mut run = Run {
        style,
        root: &root,
        interactive,
        dry_run: parsed.dry_run,
        assume_yes: parsed.assume_yes,
        confirm,
        out,
        err,
    };
    match loaded {
        Some(manifest) if parsed.all => adopt_all(&mut run, &mut resolver, &manifest),
        manifest => adopt_one(&mut run, &mut resolver, manifest.as_ref(), &parsed.dest),
    }
}

struct Args {
    dry_run: bool,
    assume_yes: bool,
    all: bool,
    dest: String,
}

/// How a command line that names nothing to adopt ends: help on stdout, an
/// error line, or the missing destination followed by the usage.
enum ParseStop {
    Help,
    Refuse(String),
    MissingDest,
}

fn parse_args(args: &[String]) -> Result<Args, ParseStop> {
    let mut parsed = Args {
        dry_run: false,
        assume_yes: false,
        all: false,
        dest: String::new(),
    };
    for arg in args {
        match arg.as_str() {
            "--dry-run" => parsed.dry_run = true,
            "--yes" | "-y" => parsed.assume_yes = true,
            "--all" | "-a" => parsed.all = true,
            "--help" | "-h" => return Err(ParseStop::Help),
            flag if flag.starts_with('-') => {
                return Err(ParseStop::Refuse(format!("unknown flag: {flag}")));
            }
            _ if !parsed.dest.is_empty() => {
                return Err(ParseStop::Refuse(
                    "adopt accepts a single destination file".into(),
                ));
            }
            value => parsed.dest = value.to_string(),
        }
    }
    if parsed.all && !parsed.dest.is_empty() {
        return Err(ParseStop::Refuse("adopt --all takes no <dest-file>".into()));
    }
    if !parsed.all && parsed.dest.is_empty() {
        return Err(ParseStop::MissingDest);
    }
    Ok(parsed)
}

struct Run<'a> {
    style: &'a Style,
    root: &'a str,
    interactive: bool,
    dry_run: bool,
    assume_yes: bool,
    confirm: Confirm<'a>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl Run<'_> {
    fn cannot(&mut self, reason: &str) -> Result<u8, Error> {
        let text = format!("{}: {reason}\n", self.style.red("Cannot adopt"));
        put(self.err, text.as_bytes())?;
        Ok(1)
    }

    fn nothing_written(&mut self) -> Result<u8, Error> {
        let text = format!("{}\n", self.style.dim("Dry-run — nothing written."));
        put(self.out, text.as_bytes())?;
        Ok(0)
    }
}

fn hash(path: &str) -> Option<String> {
    template_manifest::hash(Path::new(path))
}

/// `cp <dest> <source>` after `ensure_dir`: an existing source keeps its mode,
/// a new one takes the destination's mode under the umask.
pub(crate) fn copy_into_source(found: &Adoption) -> Result<(), Error> {
    let bytes = std::fs::read(&found.dest_abs).map_err(|e| Error::io(&found.dest_abs, e))?;
    write_into_source(found, &bytes)
}

/// What adopting a key-owned file writes: its source with the owned
/// keys the live file changed, those keys, and the owned-key record after.
pub(crate) struct KeyedAdoption {
    source_text: String,
    keys: Vec<KeyPath>,
    owned: Owned,
}

/// Why a whole-file adoption of `found` would copy another program's keys into
/// the source: the manifest recorded the file as owned by key.
fn whole_file_refusal(found: &Adoption, manifest: Option<&Manifest>) -> Option<String> {
    manifest?.entry(&found.dest_rel)?.owned.as_ref()?;
    Some(format!(
        "{} is recorded as owned by key, and a whole-file copy would pull the app's keys into the source; set targets.{}.ownership: keys, or run exuno sync --force to own the whole file",
        found.dest_rel, found.resource
    ))
}

/// The adoption of the owned keys `found` changed since the manifest recorded
/// them; `Err` explains why none can be adopted.
fn keyed_adoption(
    found: &Adoption,
    manifest: Option<&Manifest>,
) -> Result<Result<KeyedAdoption, String>, Error> {
    let entry = manifest.and_then(|m| m.entry(&found.dest_rel));
    let Some(recorded) = entry.and_then(|entry| entry.owned.as_ref()) else {
        return Ok(Err(format!(
            "{} is owned by key and has no owned-key record yet; run exuno sync first",
            found.dest_rel
        )));
    };
    let Some(format) = Format::of(&found.dest_rel) else {
        return Ok(Err(format!(
            "{} is not a TOML or JSON file",
            found.dest_rel
        )));
    };
    let live =
        std::fs::read_to_string(&found.dest_abs).map_err(|e| Error::io(&found.dest_abs, e))?;
    let now = match keyed::owned_in(format, &live, recorded) {
        Ok(now) => now,
        Err(reason) => return Ok(Err(format!("{} does not parse: {reason}", found.dest_rel))),
    };
    let keys = recorded.changed(&now);
    if keys.is_empty() {
        return Ok(Ok(KeyedAdoption {
            source_text: String::new(),
            keys,
            owned: now.present(),
        }));
    }
    if (found.units_from_mcp || found.resource == "mcp")
        && let Some(key) = keys.iter().find(|key| keyed::is_unit(key))
    {
        return Ok(Err(format!(
            "{} comes from the MCP source; edit that source instead",
            keyed::display(key)
        )));
    }
    let source = Path::new(&found.source_abs);
    if !source.is_file() {
        return Ok(Err(format!(
            "{} does not exist, and a file of only the changed keys would drop the rest; run exuno customize {} {} first",
            found.source_rel, found.tool, found.resource
        )));
    }
    if Format::of(&found.source_rel) != Some(format) {
        return Ok(Err(format!(
            "{} and {} are different formats; edit the source instead",
            found.dest_rel, found.source_rel
        )));
    }
    let source_text = std::fs::read_to_string(source).map_err(|e| Error::io(source, e))?;
    let source_text = match keyed::adopt(format, &live, &source_text, &keys) {
        Ok(text) => text,
        Err(reason) => {
            return Ok(Err(format!(
                "{} does not parse: {reason}",
                found.source_rel
            )));
        }
    };
    Ok(Ok(KeyedAdoption {
        source_text,
        keys,
        owned: now.present(),
    }))
}

fn write_into_source(found: &Adoption, bytes: &[u8]) -> Result<(), Error> {
    let source = Path::new(&found.source_abs);
    if let Some(parent) = source.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        let mode = std::fs::metadata(&found.dest_abs)
            .map_err(|e| Error::io(&found.dest_abs, e))?
            .permissions()
            .mode();
        options.mode(mode & 0o7777);
    }
    options
        .open(source)
        .and_then(|mut file| file.write_all(bytes))
        .map_err(|e| Error::io(source, e))
}

/// Whether to go on: refuses off a terminal without `--yes`, then asks.
fn confirmed(run: &mut Run, question: &str) -> Result<Option<u8>, Error> {
    let style = run.style;
    if run.assume_yes {
        return Ok(None);
    }
    if !run.interactive {
        put(
            run.err,
            format!(
                "{}: refusing to adopt non-interactively without --yes.\n",
                style.red("Error")
            )
            .as_bytes(),
        )?;
        return Ok(Some(1));
    }
    if !(run.confirm)(question) {
        put(run.out, format!("{}\n", style.dim("Cancelled.")).as_bytes())?;
        return Ok(Some(0));
    }
    Ok(None)
}

fn verify_hint(style: &Style) -> String {
    format!(
        "{} {} {}\n",
        style.dim("Run"),
        style.cyan("exuno sync"),
        style.dim("to verify everything is consistent.")
    )
}

/// `diff -u --label <source> --label <dest> <source> <dest> | head -n 40`.
fn plan_diff(found: &Adoption) -> String {
    let Ok(output) = Command::new("diff")
        .args([
            "-u",
            "--label",
            &found.source_rel,
            "--label",
            &found.dest_rel,
            &found.source_abs,
            &found.dest_abs,
        ])
        .output()
    else {
        return String::new();
    };
    let text = String::from_utf8_lossy(&output.stdout);
    let head: String = text.split_inclusive('\n').take(40).collect();
    head.trim_end_matches('\n').to_string()
}

fn adopt_one(
    run: &mut Run,
    resolver: &mut Resolver,
    manifest: Option<&Manifest>,
    dest: &str,
) -> Result<u8, Error> {
    let style = run.style;
    let found = match resolver.resolve(dest, run.err)? {
        Ok(found) => found,
        Err(reason) => return run.cannot(&reason),
    };
    if let Some(manifest) = manifest
        && !manifest.paths().contains(&found.dest_rel)
    {
        return run.cannot(&format!(
            "{} is not tracked in the manifest.\n  Exuno only adopts files it produced. Run sync first to register the file.",
            found.dest_rel
        ));
    }
    if found.keyed {
        return adopt_keys(run, &found, manifest);
    }
    if let Some(reason) = whole_file_refusal(&found, manifest) {
        return run.cannot(&reason);
    }
    let Some(current) = hash(&found.dest_abs) else {
        put(
            run.err,
            format!("{}: cannot hash {}\n", style.red("Error"), found.dest_rel).as_bytes(),
        )?;
        return Ok(1);
    };
    let source_exists = Path::new(&found.source_abs).is_file();
    if source_exists && hash(&found.source_abs).as_deref() == Some(current.as_str()) {
        put(
            run.out,
            format!(
                "{}\n",
                style.dim(&format!(
                    "Nothing to adopt: {} already matches the source.",
                    found.dest_rel
                ))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }

    put(run.out, one_plan(style, &found, source_exists).as_bytes())?;
    if run.dry_run {
        return run.nothing_written();
    }
    if let Some(status) = confirmed(run, "Apply this adoption?")? {
        return Ok(status);
    }
    copy_into_source(&found)?;
    let mut done = format!("\n{} Wrote {}\n", style.green("✓"), found.source_rel);
    if manifest.is_some() {
        manifest::update_entry(run.root, &found.dest_rel, &current, None)?;
        done.push_str(&format!(
            "{} Updated .ai/.sync-manifest\n",
            style.green("✓")
        ));
    }
    done.push('\n');
    done.push_str(&verify_hint(style));
    put(run.out, done.as_bytes())?;
    Ok(0)
}

fn one_plan(style: &Style, found: &Adoption, source_exists: bool) -> String {
    let mut plan = format!(
        "\n{}\n    {}     {}\n    {} {}\n    {}     {} {}\n    {}       {} {}\n\n",
        style.bold("  Adopt plan"),
        style.dim("tool:"),
        style.cyan(&found.tool),
        style.dim("resource:"),
        found.resource,
        style.dim("from:"),
        style.yellow(&found.dest_rel),
        style.dim("(destination — your edit)"),
        style.dim("to:"),
        style.green(&found.source_rel),
        style.dim("(source)")
    );
    if !source_exists {
        plan.push_str(&format!(
            "    {}\n\n",
            style.dim("(creating new source file)")
        ));
        return plan;
    }
    let diff = plan_diff(found);
    if !diff.is_empty() {
        for line in diff.split('\n') {
            plan.push_str(&format!("    {line}\n"));
        }
        plan.push('\n');
    }
    plan
}

/// `adopt_one` for a key-owned settings or MCP file: only the owned keys the live
/// file changed move into its source.
fn adopt_keys(run: &mut Run, found: &Adoption, manifest: Option<&Manifest>) -> Result<u8, Error> {
    let style = run.style;
    let adoption = match keyed_adoption(found, manifest)? {
        Ok(adoption) => adoption,
        Err(reason) => return run.cannot(&reason),
    };
    if adoption.keys.is_empty() {
        put(
            run.out,
            format!(
                "{}\n",
                style.dim(&format!(
                    "Nothing to adopt: the keys sync owns in {} already match the source.",
                    found.dest_rel
                ))
            )
            .as_bytes(),
        )?;
        return Ok(0);
    }
    let keys: Vec<String> = adoption.keys.iter().map(keyed::display).collect();
    let plan = format!(
        "\n{}\n    {}     {}\n    {} {}\n    {}     {} {}\n    {}       {} {}\n    {}     {}\n\n",
        style.bold("  Adopt plan"),
        style.dim("tool:"),
        style.cyan(&found.tool),
        style.dim("resource:"),
        found.resource,
        style.dim("from:"),
        style.yellow(&found.dest_rel),
        style.dim("(owned keys only)"),
        style.dim("to:"),
        style.green(&found.source_rel),
        style.dim("(source)"),
        style.dim("keys:"),
        keys.join(", ")
    );
    put(run.out, plan.as_bytes())?;
    if run.dry_run {
        return run.nothing_written();
    }
    if let Some(status) = confirmed(run, "Apply this adoption?")? {
        return Ok(status);
    }
    apply_keyed(run.root, found, &adoption)?;
    let done = format!(
        "\n{} Wrote {}\n{} Updated .ai/.sync-manifest\n\n{}",
        style.green("✓"),
        found.source_rel,
        style.green("✓"),
        verify_hint(style)
    );
    put(run.out, done.as_bytes())?;
    Ok(0)
}

fn apply_keyed(root: &str, found: &Adoption, adoption: &KeyedAdoption) -> Result<(), Error> {
    write_into_source(found, adoption.source_text.as_bytes())?;
    manifest::update_entry(
        root,
        &found.dest_rel,
        &adoption.owned.digest(),
        Some(&adoption.owned),
    )
}

/// What `adopt --all` found: the outputs it promotes, each with the hash the
/// manifest records for it, the key adoptions among them, and why the rest
/// are skipped.
struct AllPlan {
    ready: Vec<(Adoption, String)>,
    keyed: Vec<(String, KeyedAdoption)>,
    skipped: Vec<(String, String)>,
}

fn adopt_all(run: &mut Run, resolver: &mut Resolver, manifest: &Manifest) -> Result<u8, Error> {
    let style = run.style;
    let plan = plan_all(run, resolver, manifest)?;
    if plan.ready.is_empty() && plan.skipped.is_empty() {
        let text = style.dim("Nothing to adopt: every tracked output matches its source.");
        put(run.out, format!("{text}\n").as_bytes())?;
        return Ok(0);
    }
    put(run.out, all_plan_text(style, &plan).as_bytes())?;
    if plan.ready.is_empty() {
        let text =
            style.dim("No adoptable edits — the drifted files above need manual source edits.");
        put(run.out, format!("{text}\n").as_bytes())?;
        return Ok(0);
    }
    if run.dry_run {
        return run.nothing_written();
    }
    let question = format!("Apply these {} adoption(s)?", plan.ready.len());
    if let Some(status) = confirmed(run, &question)? {
        return Ok(status);
    }
    apply_all(run, &plan)?;
    Ok(0)
}

fn plan_all(run: &mut Run, resolver: &mut Resolver, manifest: &Manifest) -> Result<AllPlan, Error> {
    let mut planned: Vec<(Adoption, String)> = Vec::new();
    let mut keyed: Vec<(String, KeyedAdoption)> = Vec::new();
    let mut skipped: Vec<(String, String)> = Vec::new();
    for rel in manifest.drift(run.root) {
        match resolver.resolve(&format!("{}/{rel}", run.root), run.err)? {
            Err(reason) => skipped.push((rel, reason)),
            Ok(found) if !found.keyed && whole_file_refusal(&found, Some(manifest)).is_some() => {
                skipped.push((
                    rel,
                    whole_file_refusal(&found, Some(manifest)).unwrap_or_default(),
                ));
            }
            Ok(found) if found.keyed => match keyed_adoption(&found, Some(manifest))? {
                Err(reason) => skipped.push((rel, reason)),
                Ok(adoption) => {
                    let digest = adoption.owned.digest();
                    keyed.push((rel, adoption));
                    planned.push((found, digest));
                }
            },
            Ok(found) => match hash(&found.dest_abs) {
                Some(current) => planned.push((found, current)),
                None => skipped.push((rel, "cannot hash destination".to_string())),
            },
        }
    }
    let unshared: Vec<bool> = planned
        .iter()
        .enumerate()
        .map(|(i, (found, current))| {
            !planned.iter().enumerate().any(|(j, (other, other_hash))| {
                i != j && found.source_abs == other.source_abs && current != other_hash
            })
        })
        .collect();
    let mut ready = Vec::new();
    for ((found, current), fine) in planned.into_iter().zip(unshared) {
        if fine {
            ready.push((found, current));
        } else {
            let reason = format!(
                "multiple edited outputs map to {} — adopt one explicitly",
                found.source_rel
            );
            skipped.push((found.dest_rel, reason));
        }
    }
    Ok(AllPlan {
        ready,
        keyed,
        skipped,
    })
}

fn all_plan_text(style: &Style, plan: &AllPlan) -> String {
    let mut text = format!("\n{}\n", style.bold("  Adopt plan (--all)"));
    if !plan.ready.is_empty() {
        text.push_str(&format!(
            "    {} file(s) will be promoted to source:\n\n",
            plan.ready.len()
        ));
        for (found, _) in &plan.ready {
            text.push_str(&format!(
                "    {}  {} {} {}\n",
                style.cyan(&found.tool),
                style.yellow(&found.dest_rel),
                style.dim("→"),
                style.green(&found.source_rel)
            ));
        }
        text.push('\n');
    }
    if !plan.skipped.is_empty() {
        text.push_str(&format!(
            "    {}\n",
            style.dim(&format!(
                "{} skipped (edit .ai/src/ directly):",
                plan.skipped.len()
            ))
        ));
        for (rel, reason) in &plan.skipped {
            text.push_str(&format!(
                "    {} {} {}\n",
                style.yellow(rel),
                style.dim("—"),
                style.dim(reason)
            ));
        }
        text.push('\n');
    }
    text
}

fn apply_all(run: &mut Run, plan: &AllPlan) -> Result<(), Error> {
    let style = run.style;
    put(run.out, b"\n")?;
    let mut adopted = BTreeSet::new();
    for (found, current) in &plan.ready {
        match plan.keyed.iter().find(|(rel, _)| *rel == found.dest_rel) {
            Some((_, adoption)) => apply_keyed(run.root, found, adoption)?,
            None => {
                copy_into_source(found)?;
                manifest::update_entry(run.root, &found.dest_rel, current, None)?;
            }
        }
        if !adopted.insert(found.source_rel.as_str()) {
            continue;
        }
        let line = format!(
            "{} {} {}\n",
            style.green("✓"),
            style.dim("adopted"),
            found.source_rel
        );
        put(run.out, line.as_bytes())?;
    }
    put(
        run.out,
        format!(
            "\n{} Adopted {} file(s) into .ai/src/ and refreshed .ai/.sync-manifest\n\n{}",
            style.green("✓"),
            adopted.len(),
            verify_hint(style)
        )
        .as_bytes(),
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn project(files: &[(&str, &str)]) -> (tempfile::TempDir, String) {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        for (rel, text) in files {
            let path = format!("{root}/{rel}");
            std::fs::create_dir_all(paths::parent(&path)).unwrap();
            std::fs::write(path, text).unwrap();
        }
        (dir, root)
    }

    fn manifest_of(root: &str, rels: &[(&str, &str)]) {
        let text: String = rels
            .iter()
            .map(|(rel, content)| format!("{rel}\t{}\n", manifest::sha256_hex(content.as_bytes())))
            .collect();
        std::fs::write(format!("{root}/{}", manifest::REL), text).unwrap();
    }

    fn call(root: &str, args: &[&str], interactive: bool, accept: bool) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let discover = || Project::at(root);
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = adopt(
            &args,
            &discover,
            &Style::plain(),
            interactive,
            &mut |_| accept,
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

    const SOURCES: [(&str, &str); 4] = [
        (".ai/agent_sync.yaml", "tools:\n  enabled:\n    - claude\n"),
        (".ai/src/AGENTS.md", "# Agents\n"),
        (".ai/src/rules/core.md", "# Core\n\nBody.\n"),
        (".ai/src/commands/go.md", "---\ndescription: Go\n---\nGo.\n"),
    ];

    #[test]
    fn a_destination_maps_to_its_source_or_to_the_bash_refusal() {
        let mut files = SOURCES.to_vec();
        files.extend([
            ("CLAUDE.md", "# Agents\n"),
            (".cline/workflows/go.md", "Go.\n"),
            (".github/prompts/go.prompt.md", "Go.\n"),
            (".cursor/rules/core.mdc", "---\n---\n# Core\n"),
            (".claude/settings.json", "{}\n"),
            ("README.md", "readme\n"),
        ]);
        let (_dir, root) = project(&files);
        let project = Project::at(&root).unwrap();
        let style = Style::plain();
        let mut resolver =
            Resolver::new(&project, discover_sources(&project).unwrap(), &style).unwrap();
        let mut err = Vec::new();
        let mut resolve = |raw: &str| {
            resolver
                .resolve(raw, &mut err)
                .unwrap()
                .map(|found| (found.tool, found.resource, found.source_rel))
        };
        assert_eq!(
            resolve("CLAUDE.md"),
            Ok((
                "claude".to_string(),
                "agents",
                ".ai/src/AGENTS.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".cline/workflows/go.md"),
            Ok((
                "cline".to_string(),
                "commands",
                ".ai/src/commands/go.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".github/prompts/go.prompt.md"),
            Ok((
                "copilot".to_string(),
                "commands",
                ".ai/src/commands/go.md".to_string()
            ))
        );
        assert_eq!(
            resolve(".claude/settings.json"),
            Ok((
                "claude".to_string(),
                "settings",
                ".ai/src/tools/claude/settings.json".to_string()
            ))
        );
        assert_eq!(
            resolve(".cursor/rules/core.mdc"),
            Err("cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.".to_string())
        );
        assert_eq!(
            resolve("../outside.md"),
            Err("Path is outside the project: ../outside.md".to_string())
        );
        assert_eq!(
            resolve(".claude/rules/nope.md"),
            Err("Destination file not found: .claude/rules/nope.md".to_string())
        );
        assert_eq!(
            resolve("README.md"),
            Err(
                "README.md is not a recognised Exuno output (no enabled tool produces it)."
                    .to_string()
            )
        );
    }

    #[test]
    fn the_project_source_keys_move_the_detected_layout() {
        let (_dir, root) = project(&[
            (
                ".ai/agent_sync.yaml",
                "tools:\n  enabled: []\nsource:\n  rules: \"docs/rules\"\ncommands: \"docs/commands\"\n",
            ),
            (".ai/src/AGENTS.md", "# A\n"),
            (".ai/src/rules/core.md", "# Core\n"),
            (".ai/skills/s/SKILL.md", "s\n"),
        ]);
        let project = Project::at(&root).unwrap();
        assert_eq!(
            discover_sources(&project).unwrap(),
            Sources {
                agents: ".ai/src/AGENTS.md".to_string(),
                rules: "docs/rules".to_string(),
                skills: ".ai/skills".to_string(),
                commands: "docs/commands".to_string(),
                subagents: String::new(),
            }
        );
    }

    #[test]
    fn one_file_is_planned_confirmed_written_and_recorded_like_bash() {
        let mut files = SOURCES.to_vec();
        files.push(("CLAUDE.md", "# Agents\n\nEdited.\n"));
        let (_dir, root) = project(&files);
        manifest_of(&root, &[("CLAUDE.md", "# Agents\n")]);
        let plan = "\n  Adopt plan\n    tool:     claude\n    resource: agents\n    from:     CLAUDE.md (destination — your edit)\n    to:       .ai/src/AGENTS.md (source)\n\n    --- .ai/src/AGENTS.md\n    +++ CLAUDE.md\n    @@ -1 +1,3 @@\n     # Agents\n    +\n    +Edited.\n\n";

        assert_eq!(
            call(&root, &["CLAUDE.md"], false, true),
            (
                1,
                plan.to_string(),
                "Error: refusing to adopt non-interactively without --yes.\n".to_string()
            )
        );
        assert_eq!(
            call(&root, &["--dry-run", "CLAUDE.md"], false, true).1,
            format!("{plan}Dry-run — nothing written.\n")
        );
        assert_eq!(
            call(&root, &["CLAUDE.md"], true, false),
            (0, format!("{plan}Cancelled.\n"), String::new())
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/AGENTS.md")).unwrap(),
            "# Agents\n"
        );

        assert_eq!(
            call(&root, &["--yes", "CLAUDE.md"], false, false),
            (
                0,
                format!(
                    "{plan}\n✓ Wrote .ai/src/AGENTS.md\n✓ Updated .ai/.sync-manifest\n\nRun exuno sync to verify everything is consistent.\n"
                ),
                String::new()
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/AGENTS.md")).unwrap(),
            "# Agents\n\nEdited.\n"
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/{}", manifest::REL)).unwrap(),
            format!(
                "CLAUDE.md\t{}\n",
                manifest::sha256_hex(b"# Agents\n\nEdited.\n")
            )
        );
        assert_eq!(
            call(&root, &["CLAUDE.md"], false, false).1,
            "Nothing to adopt: CLAUDE.md already matches the source.\n"
        );
    }

    #[test]
    fn all_skips_refusals_and_same_source_conflicts_like_bash() {
        let mut files = SOURCES.to_vec();
        files.extend([
            (".claude/rules/core.md", "# Core\n\nClaude edit.\n"),
            (".amazonq/rules/core.md", "# Core\n\nAmazon edit.\n"),
            (".cursor/rules/core.mdc", "edited\n"),
            (".claude/skills/foo/SKILL.md", "Skill two.\n"),
            (".ai/src/skills/foo/SKILL.md", "Skill.\n"),
        ]);
        let (_dir, root) = project(&files);
        manifest_of(
            &root,
            &[
                (".amazonq/rules/core.md", "# Core\n\nBody.\n"),
                (".claude/rules/core.md", "# Core\n\nBody.\n"),
                (".claude/skills/foo/SKILL.md", "Skill.\n"),
                (".cursor/rules/core.mdc", "generated\n"),
            ],
        );
        let plan = "\n  Adopt plan (--all)\n    1 file(s) will be promoted to source:\n\n    claude  .claude/skills/foo/SKILL.md → .ai/src/skills/foo/SKILL.md\n\n    3 skipped (edit .ai/src/ directly):\n    .cursor/rules/core.mdc — cursor injects a frontmatter header on sync. Adopting would propagate it to other tools' rule files. Edit .ai/src/rules/ instead.\n    .amazonq/rules/core.md — multiple edited outputs map to .ai/src/rules/core.md — adopt one explicitly\n    .claude/rules/core.md — multiple edited outputs map to .ai/src/rules/core.md — adopt one explicitly\n\n";
        assert_eq!(
            call(&root, &["--all", "--dry-run"], false, false),
            (
                0,
                format!("{plan}Dry-run — nothing written.\n"),
                String::new()
            )
        );
        assert_eq!(
            call(&root, &["-a", "-y"], false, false).1,
            format!(
                "{plan}\n✓ adopted .ai/src/skills/foo/SKILL.md\n\n✓ Adopted 1 file(s) into .ai/src/ and refreshed .ai/.sync-manifest\n\nRun exuno sync to verify everything is consistent.\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(format!("{root}/.ai/src/skills/foo/SKILL.md")).unwrap(),
            "Skill two.\n"
        );
    }

    #[test]
    fn help_renders_in_the_shared_shape() {
        let (_dir, root) = project(&SOURCES);
        assert_eq!(
            call(&root, &["--help"], false, false),
            (
                0,
                "\n  exuno adopt — promote a manual edit back into .ai/src/\n\n  USAGE\n    exuno adopt <dest-file> [OPTIONS]\n    exuno adopt --all [OPTIONS]\n\n  DESCRIPTION\n    Promote a manual edit in a destination file back into .ai/src/ as the\n    new canonical content. Refuses transformed targets (merged rules,\n    inlined skills, format-converted commands/subagents).\n\n    With --all, adopt every drifted (manually-edited) tracked output at\n    once, skipping refused targets and same-source conflicts.\n\n  OPTIONS\n    -a, --all    Adopt every drifted output (no <dest-file>)\n    --dry-run    Show the plan without writing\n    -y, --yes    Skip confirmation (required outside a TTY)\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno adopt CLAUDE.md\n    exuno adopt .claude/rules/core.md --dry-run\n    exuno adopt --all --yes\n\n".to_string(),
                String::new()
            )
        );
    }

    #[test]
    fn arguments_are_refused_with_the_bash_statuses() {
        let (_dir, root) = project(&SOURCES);
        let refused = |args: &[&str]| {
            let (status, _, err) = call(&root, args, false, false);
            (status, err)
        };
        assert_eq!(
            refused(&[]),
            (
                2,
                format!(
                    "Error: missing <dest-file>\n{}",
                    HELP.render(&Style::plain())
                )
            )
        );
        assert_eq!(
            refused(&["--bogus"]),
            (2, "Error: unknown flag: --bogus\n".to_string())
        );
        assert_eq!(
            refused(&["a", "b"]),
            (
                2,
                "Error: adopt accepts a single destination file\n".to_string()
            )
        );
        assert_eq!(
            refused(&["--all", "CLAUDE.md"]),
            (2, "Error: adopt --all takes no <dest-file>\n".to_string())
        );
        assert_eq!(
            refused(&["--all"]),
            (
                1,
                "Error: no .ai/.sync-manifest yet — run exuno sync first, or adopt one file at a time.\n"
                    .to_string()
            )
        );
        assert_eq!(
            refused(&["CLAUDE.md"]),
            (
                1,
                "Cannot adopt: Destination file not found: CLAUDE.md\n".to_string()
            )
        );
    }
}
