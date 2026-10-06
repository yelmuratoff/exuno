//! Inspection and explicit source creation from an MCP catalog.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::{mcp_merge, put};
use crate::Error;
use crate::config::{mcp_catalog, names, payload, tool::Tool, yaml_subset};
use crate::engine::staging;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::paths;
use crate::project::Project;
use crate::transaction::{backup, witness};

pub const HELP: Help = Help {
    command: "mcp",
    tagline: "inspect an offline MCP catalog",
    synopsis: &[
        "mcp list --library <directory>",
        "mcp show <id> --library <directory>",
        "mcp validate [id] --library <directory>",
        "mcp render <id>[@variant] --library <directory>",
        "mcp use <id>[@variant] --tool <slug> [--merge] [--replace <id>] [--apply] --library <directory>",
    ],
    description: &[
        "Reads bounded JSON manifests from an explicit library or the configured\nlibrary.mcp.path. It never starts servers or contacts endpoints.",
        "list prints ID and title. show prints the original manifest bytes.\nvalidate checks one entry or the complete library, including all variants.\nrender prints one selected connection as AgentSync MCP source JSON.",
        "use previews a per-tool source; --apply writes it. --merge extends an\nexisting per-tool JSON source; --replace <id> permits one explicit replacement.\nRun agentsync sync separately to update client files.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[
            (
                "--library <directory>",
                "Explicit catalog path, absolute or relative to the project root",
            ),
            (
                "--tool <slug>",
                "Enabled tool receiving a per-tool MCP source",
            ),
            ("--apply", "Create the source after previewing"),
            ("--merge", "Extend the existing per-tool MCP JSON source"),
            (
                "--replace <id>",
                "Replace this selected server ID during --merge",
            ),
            ("-h, --help", "Show this help"),
        ],
    }],
    examples: &[
        "mcp list --library catalog/mcp",
        "mcp show microsoft-learn --library catalog/mcp",
        "mcp validate --library catalog/mcp",
        "mcp render microsoft-learn@recommended --library catalog/mcp",
        "mcp use microsoft-learn --tool claude --library catalog/mcp",
    ],
};

#[derive(Clone, Copy)]
enum Action {
    Help,
    List,
    Show,
    Validate,
    Render,
    Use,
}

struct Args {
    action: Action,
    id: Option<String>,
    variant: Option<String>,
    tool: Option<String>,
    apply: bool,
    merge: bool,
    replace: Option<String>,
    library: Option<PathBuf>,
}

pub fn run(
    args: &[String],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let args = match parse(args) {
        Ok(args) => args,
        Err(message) => return refuse(style, &message, err),
    };
    if matches!(args.action, Action::Help) {
        put(out, HELP.render(style).as_bytes())?;
        return Ok(0);
    }
    let project = Project::discover()?;
    let library = match library_path(&project, args.library.clone()) {
        Ok(path) => path,
        Err(message) => return refuse(style, &message, err),
    };
    if matches!(args.action, Action::Render | Action::Use) {
        let rendered = match mcp_catalog::render(
            &library,
            args.id.as_deref().expect("render requires an id"),
            args.variant.as_deref().unwrap_or("default"),
        ) {
            Ok(rendered) => rendered,
            Err(message) => return refuse(style, &message, err),
        };
        if matches!(args.action, Action::Use) {
            return use_source(&project, &args, &rendered, style, out, err);
        }
        put(out, &rendered)?;
        return Ok(0);
    }
    let entries = match mcp_catalog::read(&library, args.id.as_deref()) {
        Ok(entries) => entries,
        Err(message) => return refuse(style, &message, err),
    };
    match args.action {
        Action::List => {
            let mut text = String::new();
            for entry in &entries {
                text.push_str(&entry.id);
                text.push('\t');
                text.push_str(&mcp_catalog::escaped_title(&entry.title));
                text.push('\n');
            }
            put(out, text.as_bytes())?;
        }
        Action::Show => put(out, &entries[0].raw)?,
        Action::Validate => put(out, b"MCP library is valid\n")?,
        Action::Help | Action::Render | Action::Use => unreachable!(),
    }
    Ok(0)
}

fn use_source(
    project: &Project,
    args: &Args,
    rendered: &[u8],
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let mut run = McpUse {
        project,
        args,
        style,
        out,
        err,
    };
    run.apply(rendered)
}

/// Where `mcp use` writes: the canonical project root, the per-tool source
/// relative to it, the checked destination, and the path the backup records.
struct SourceTarget {
    root: String,
    rel: String,
    dest: PathBuf,
    resolved: String,
}

/// One `mcp use` run: the project, the parsed arguments, and the streams.
struct McpUse<'a> {
    project: &'a Project,
    args: &'a Args,
    style: &'a Style,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl McpUse<'_> {
    fn refuse(&mut self, message: &str) -> Result<u8, Error> {
        refuse(self.style, message, self.err)
    }

    fn slug(&self) -> &str {
        self.args.tool.as_deref().expect("use requires a tool")
    }

    fn id(&self) -> &str {
        self.args.id.as_deref().expect("use requires an id")
    }

    fn selection(&self) -> String {
        let variant = self.args.variant.as_deref().unwrap_or("default");
        format!("{}@{variant}", self.id())
    }

    fn apply(&mut self, rendered: &[u8]) -> Result<u8, Error> {
        let slug = self.slug().to_string();
        if !self.project.tools_dir_in_project() {
            return self.refuse("source.tools resolves outside the project root");
        }
        if !self.project.enabled_tools()?.contains(&slug) {
            return self.refuse("MCP target tool is not enabled in this project");
        }
        let tool = Tool::load(self.project, &slug)?;
        if tool.value("targets.mcp.dest").is_empty() {
            return self.refuse("MCP target tool has no MCP destination");
        }
        let format = tool.value("targets.mcp.format");
        if !matches!(
            format.as_str(),
            "" | "opencode_json" | "kimi_json" | "codex_toml"
        ) {
            return self.refuse("MCP target tool uses an unsupported MCP format");
        }
        let rendered = if format == "kimi_json" {
            mcp_catalog::render_kimi_source(rendered, self.id())
        } else {
            rendered.to_vec()
        };
        let target = match self.target(&slug)? {
            Ok(target) => target,
            Err(message) => return self.refuse(message),
        };
        if self.args.merge {
            self.merge(&tool, &target, &rendered)
        } else {
            self.create(&tool, &target, &rendered)
        }
    }

    /// The per-tool `mcp.json`, resolved inside the project root.
    fn target(&self, slug: &str) -> Result<Result<SourceTarget, &'static str>, Error> {
        let root = backup::canonical_root(&paths::from_disk(&self.project.root))?;
        let intended = self.project.user_tools_dir().join(slug).join("mcp.json");
        let disk_paths = paths::Paths::on_disk(&paths::from_disk(&self.project.root));
        let Some(resolved) =
            disk_paths.canonicalize_with_existing_ancestor(&paths::from_disk(&intended))
        else {
            return Ok(Err("Cannot resolve per-tool MCP source path"));
        };
        let Some(rel) = resolved.strip_prefix(&format!("{root}/")) else {
            return Ok(Err("Per-tool MCP source resolves outside the project root"));
        };
        let rel = rel.to_string();
        let dest = PathBuf::from(backup::safe_target_path(&root, &rel, false)?);
        Ok(Ok(SourceTarget {
            root,
            rel,
            dest,
            resolved,
        }))
    }

    /// Writes a new per-tool source, refusing any MCP source already there.
    fn create(&mut self, tool: &Tool, target: &SourceTarget, rendered: &[u8]) -> Result<u8, Error> {
        let slug = self.slug().to_string();
        let override_dir = self.project.user_tools_dir().join(&slug);
        if let Some(message) = occupied(&override_dir)? {
            return self.refuse(message);
        }
        if matches!(
            payload::effective_source(self.project, tool, "mcp")?.0,
            Some(payload::Source::Disk(_))
        ) {
            return self.refuse("An MCP source already exists for this tool");
        }
        if std::fs::symlink_metadata(&target.dest).is_ok() {
            return self.refuse("Per-tool MCP source is already occupied");
        }
        let (rel, selection) = (&target.rel, self.selection());
        if !self.args.apply {
            let heading = format!("Would create {rel} from {selection} for {slug}:\n");
            put(self.out, heading.as_bytes())?;
            put(self.out, rendered)?;
            put(self.out, b"Run with --apply to write it.\n")?;
            return Ok(0);
        }
        let dest = &target.dest;
        let written = self.write_backed_up(target, target.resolved.clone(), "create", || {
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            staging::write_new_beside(dest, rendered)
        })?;
        if let Some(status) = written {
            return Ok(status);
        }
        put(self.out, format!("Created {rel} from {selection} for {slug}\nReview the source, then run agentsync sync to update client files.\n").as_bytes())?;
        Ok(0)
    }

    /// Merges into the per-tool `mcp.json` that already owns MCP.
    fn merge(&mut self, tool: &Tool, target: &SourceTarget, rendered: &[u8]) -> Result<u8, Error> {
        let (slug, id) = (self.slug().to_string(), self.id().to_string());
        let override_dir = self.project.user_tools_dir().join(&slug);
        let intended = override_dir.join("mcp.json");
        if let Some(message) = self.merge_refusal(tool, &intended, &override_dir)? {
            return self.refuse(message);
        }
        let _lock = if self.args.apply {
            Some(mcp_merge::Lock::acquire(&override_dir)?)
        } else {
            None
        };
        let previous = std::fs::read(&intended).map_err(|e| Error::io(&intended, e))?;
        let rel = &target.rel;
        let merged = match mcp_merge::compose(&previous, rendered, &id, self.args.replace.is_some())
        {
            Ok(Some(merged)) => merged,
            Ok(None) => {
                let text = format!("{rel} already contains {id}; no change needed.\n");
                put(self.out, text.as_bytes())?;
                return Ok(0);
            }
            Err(reason) => return self.refuse(reason),
        };
        let selection = self.selection();
        if !self.args.apply {
            let heading = format!("Would merge {selection} into {rel} for {slug}:\n");
            put(self.out, heading.as_bytes())?;
            put(self.out, rendered)?;
            put(self.out, b"Run with --apply to write it.\n")?;
            return Ok(0);
        }
        let dest = &target.dest;
        let written = self.write_backed_up(target, paths::from_disk(dest), "merge", || {
            let unchanged = std::fs::symlink_metadata(&intended)
                .map_err(|e| Error::io(&intended, e))?
                .file_type()
                .is_file()
                && std::fs::read(&intended).map_err(|e| Error::io(&intended, e))? == previous;
            if !unchanged {
                return Err(Error::io(
                    &intended,
                    std::io::Error::other("MCP source changed while preparing merge"),
                ));
            }
            staging::write_beside(dest, &merged)
        })?;
        if let Some(status) = written {
            return Ok(status);
        }
        put(self.out, format!("Merged {selection} into {rel} for {slug}\nReview the source, then run agentsync sync to update client files.\n").as_bytes())?;
        Ok(0)
    }

    /// Why `--merge` cannot merge into `intended`: it is not a regular file
    /// within the byte limit, a sibling `mcp.*` makes it ambiguous, or it
    /// does not own the tool's MCP.
    fn merge_refusal(
        &self,
        tool: &Tool,
        intended: &Path,
        override_dir: &Path,
    ) -> Result<Option<&'static str>, Error> {
        let meta = match std::fs::symlink_metadata(intended) {
            Ok(meta) if meta.file_type().is_file() => meta,
            _ => return Ok(Some("--merge requires a regular per-tool mcp.json source")),
        };
        if meta.len() > mcp_catalog::MAX_MANIFEST_BYTES {
            return Ok(Some("Existing MCP source exceeds the merge byte limit"));
        }
        for entry in std::fs::read_dir(override_dir).map_err(|e| Error::io(override_dir, e))? {
            let entry = entry.map_err(|e| Error::io(override_dir, e))?;
            if entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("mcp.") && name != "mcp.json")
            {
                return Ok(Some("Per-tool MCP source is ambiguous"));
            }
        }
        match payload::effective_source(self.project, tool, "mcp")?.0 {
            Some(payload::Source::Disk(path)) if path == intended => Ok(None),
            _ => Ok(Some("--merge requires the per-tool source to own MCP")),
        }
    }

    /// Runs `write` inside an `mcp-use` backup of `backup_target`; a failed
    /// write discards that backup. `Some(status)` once the failure is reported.
    fn write_backed_up(
        &mut self,
        target: &SourceTarget,
        backup_target: String,
        verb: &str,
        write: impl FnOnce() -> Result<(), Error>,
    ) -> Result<Option<u8>, Error> {
        let root = target.root.as_str();
        let config = self
            .project
            .config_path
            .as_ref()
            .map(|path| std::fs::read_to_string(path).map(|text| (path.clone(), text)))
            .transpose()
            .map_err(|e| {
                Error::io(
                    self.project
                        .config_path
                        .as_ref()
                        .expect("config path exists"),
                    e,
                )
            })?;
        let lookup = |name: &str| std::env::var(name).ok();
        let limit = names::env("BACKUP_LIMIT", &lookup);
        let age = names::env("BACKUP_MAX_AGE_DAYS", &lookup);
        let retention = backup::configure(
            config
                .as_ref()
                .map(|(path, text)| (path.to_str().unwrap_or("<config>"), text.as_str())),
            limit.as_deref(),
            age.as_deref(),
        )?;
        let previous_latest =
            backup::latest(root)?.map_or_else(String::new, |path| paths::leaf(&path));
        let snapshot = backup::create(root, "mcp-use", &[backup_target], retention)?;
        if let Err(error) = write() {
            let store = format!("{root}/.ai/backups");
            let message = match backup::discard_safety(&store, &snapshot, &previous_latest) {
                Err(cleanup) => format!(
                    "Could not {verb} MCP source: {error}; backup cleanup failed: {cleanup}"
                ),
                Ok(()) => format!("Could not {verb} MCP source: {error}"),
            };
            return self.refuse(&message).map(Some);
        }
        if let Err(reason) = witness::seal(root, &snapshot) {
            let text = format!("Warning: Could not record MCP source backup state: {reason}\n");
            put(self.err, text.as_bytes())?;
        }
        if let Err(error) = backup::prune(root, limit.as_deref(), age.as_deref(), retention) {
            put(
                self.err,
                format!("Warning: Could not prune backups: {error}\n").as_bytes(),
            )?;
        }
        Ok(None)
    }
}

/// Why a new per-tool MCP source cannot go into `override_dir`.
fn occupied(override_dir: &Path) -> Result<Option<&'static str>, Error> {
    let entries = match std::fs::read_dir(override_dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) if error.kind() == std::io::ErrorKind::NotADirectory => {
            return Ok(Some("Per-tool MCP source parent is not a directory"));
        }
        Err(error) => return Err(Error::io(override_dir, error)),
    };
    for entry in entries {
        let entry = entry.map_err(|e| Error::io(override_dir, e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            return Ok(Some("Per-tool source directory has a non-UTF-8 entry"));
        };
        if name.starts_with("mcp.") {
            return Ok(Some("Per-tool MCP source is already occupied or ambiguous"));
        }
    }
    Ok(None)
}

fn library_path(project: &Project, explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(path) = explicit {
        let path = paths::from_msys(
            &path.to_string_lossy(),
            std::env::var("MSYSTEM").ok().as_deref(),
        );
        let path = PathBuf::from(path);
        return Ok(if path.is_absolute() {
            path
        } else {
            project.root.join(path)
        });
    }
    let Some(config) = &project.config_path else {
        return Err("No MCP library selected; pass --library".to_string());
    };
    let text = std::fs::read_to_string(config).map_err(|e| {
        format!(
            "Cannot read project config {}: {e}",
            mcp_catalog::escaped_title(&config.to_string_lossy())
        )
    })?;
    let path = yaml_subset::value(&text, "library.mcp.path");
    if path.is_empty() {
        return Err("No MCP library selected; pass --library".to_string());
    }
    let path = paths::from_msys(&path, std::env::var("MSYSTEM").ok().as_deref());
    let path = Path::new(&path);
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        project.root.join(path)
    };
    let root = std::fs::canonicalize(&project.root)
        .map_err(|e| format!("Cannot resolve project root: {e}"))?;
    let resolved = std::fs::canonicalize(&path).map_err(|e| {
        format!(
            "Cannot resolve configured MCP library {}: {e}",
            mcp_catalog::escaped_title(&path.to_string_lossy())
        )
    })?;
    if !resolved.starts_with(&root) {
        return Err("Configured MCP library is outside the project root".to_string());
    }
    Ok(resolved)
}

impl Args {
    fn for_action(action: Action) -> Self {
        Self {
            action,
            id: None,
            variant: None,
            tool: None,
            apply: false,
            merge: false,
            replace: None,
            library: None,
        }
    }

    /// Why the parsed arguments name no complete command.
    fn incomplete(&self) -> Option<&'static str> {
        let action = self.action;
        if matches!(action, Action::Show | Action::Render | Action::Use) && self.id.is_none() {
            return Some(match action {
                Action::Show => "mcp show requires an id",
                Action::Use => "mcp use requires an id",
                _ => "mcp render requires an id",
            });
        }
        if matches!(action, Action::Use) && self.tool.is_none() {
            return Some("mcp use requires --tool <slug>");
        }
        let replaces_other = self
            .replace
            .as_ref()
            .is_some_and(|replaced| !self.merge || self.id.as_deref() != Some(replaced));
        replaces_other.then_some("--replace must name the selected ID and requires --merge")
    }
}

fn parse(args: &[String]) -> Result<Args, String> {
    let action = match args.first().map(String::as_str) {
        None | Some("help" | "-h" | "--help") => Action::Help,
        Some("list") => Action::List,
        Some("show") => Action::Show,
        Some("validate") => Action::Validate,
        Some("render") => Action::Render,
        Some("use") => Action::Use,
        _ => {
            return Err(
                "Usage: agentsync mcp <list|show|validate|render|use> [options]".to_string(),
            );
        }
    };
    let using = matches!(action, Action::Use);
    let mut parsed = Args::for_action(action);
    let mut rest = args[1..].iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "-h" | "--help" => return Ok(Args::for_action(Action::Help)),
            "--library" => {
                let value = rest
                    .next()
                    .filter(|value| !value.is_empty() && !value.starts_with('-'))
                    .ok_or_else(|| "--library requires a non-empty path".to_string())?;
                if parsed.library.replace(PathBuf::from(value)).is_some() {
                    return Err("--library may be supplied only once".to_string());
                }
            }
            "--tool" if using => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--tool requires a slug".to_string())?;
                if !mcp_catalog::valid_id(value) || parsed.tool.replace(value.clone()).is_some() {
                    return Err("--tool requires one valid tool slug".to_string());
                }
            }
            "--apply" if using && !parsed.apply => parsed.apply = true,
            "--merge" if using && !parsed.merge => parsed.merge = true,
            "--replace" if using => {
                let value = rest
                    .next()
                    .ok_or_else(|| "--replace requires a server ID".to_string())?;
                if !mcp_catalog::valid_id(value) || parsed.replace.replace(value.clone()).is_some()
                {
                    return Err("--replace requires one valid server ID".to_string());
                }
            }
            flag if flag.starts_with('-') => {
                return Err(format!(
                    "Unknown MCP option: {}",
                    mcp_catalog::escaped_title(flag)
                ));
            }
            value if !matches!(action, Action::List) && parsed.id.is_none() => {
                let (id, variant) = parse_selection(value, action)?;
                parsed.id = Some(id);
                if variant.is_some() {
                    parsed.variant = variant;
                }
            }
            value => {
                return Err(format!(
                    "Unexpected MCP argument: {}",
                    mcp_catalog::escaped_title(value)
                ));
            }
        }
    }
    match parsed.incomplete() {
        Some(message) => Err(message.to_string()),
        None => Ok(parsed),
    }
}

/// A library entry id, and for `render` and `use` its `@variant`, each checked
/// as a safe id.
fn parse_selection(value: &str, action: Action) -> Result<(String, Option<String>), String> {
    let (id, variant) = if matches!(action, Action::Render | Action::Use) {
        value
            .split_once('@')
            .map_or((value, None), |(id, variant)| (id, Some(variant)))
    } else {
        (value, None)
    };
    if !mcp_catalog::valid_id(id) {
        return Err(format!(
            "Unsafe MCP library id: {}",
            mcp_catalog::escaped_title(id)
        ));
    }
    if let Some(variant) = variant
        && !mcp_catalog::valid_id(variant)
    {
        return Err(format!(
            "Unsafe MCP variant id: {}",
            mcp_catalog::escaped_title(variant)
        ));
    }
    Ok((id.to_string(), variant.map(str::to_string)))
}

fn refuse(style: &Style, message: &str, err: &mut dyn Write) -> Result<u8, Error> {
    put(
        err,
        format!("{}: {message}\n", style.red("Error")).as_bytes(),
    )?;
    Ok(1)
}
