//! `exuno add`: `cmd_add` and `cmd_add_mcp` of `lib/helpers/add.sh`,
//! which scaffold a rule, skill, command, or subagent from the shipped content
//! templates and splice one server into the shared `.ai/src/mcp.json`.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::put;
use crate::engine::{skill_tree, workspace::Workspace};
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::{Error, config::catalog, engine::staging};

pub const HELP: Help = Help {
    command: "add",
    tagline: "scaffold a rule, skill, command, subagent, or MCP server",
    synopsis: &[
        "add <kind> <name> [--category <path>] [--force]",
        "add mcp <server> (--url URL | --command CMD) [MCP OPTIONS] [--force]",
    ],
    description: &[
        "Scaffold a new entry under .ai/src/ from the shipped content templates,\nor add one server entry to the shared .ai/src/mcp.json.",
        "Edit the scaffold, then run exuno sync to propagate.",
    ],
    sections: &[
        Section {
            title: "KINDS",
            entries: &[
                ("rule", "Create .ai/src/rules/<name>.md"),
                (
                    "skill",
                    "Create .ai/src/skills/[<category>/]<name>/SKILL.md",
                ),
                ("command", "Create .ai/src/commands/<name>.md"),
                ("subagent", "Create .ai/src/agents/<name>.md"),
                ("mcp", "Add an MCP server entry to .ai/src/mcp.json"),
            ],
        },
        Section {
            title: "MCP OPTIONS",
            entries: &[
                ("--url URL", "HTTP server endpoint"),
                ("--command CMD", "Command that starts the server"),
                ("--args \"a b c\"", "Command arguments, split on whitespace"),
                ("--env K=V[,K=V...]", "Environment variables for the server"),
            ],
        },
        Section {
            title: "OPTIONS",
            entries: &[
                (
                    "--category <path>",
                    "Place a skill in a category, e.g. flutter/ui",
                ),
                ("-f, --force", "Overwrite an existing file or server entry"),
                ("-h, --help", "Show this help"),
            ],
        },
    ],
    examples: &[
        "add rule testing",
        "add skill deploy",
        "add skill slivers --category flutter/ui",
        "add mcp linear --url https://mcp.linear.app/sse",
        "add mcp github --command npx --args \"-y @github/mcp-server\"",
    ],
};

const EMPTY_MCP: &str = "{\n  \"mcpServers\": {}\n}\n";

/// `_add_print_usage`: the refusal line, then the help.
fn usage(message: &str, style: &Style, err: &mut dyn Write) -> Result<(), Error> {
    put(
        err,
        format!("{}: {message}\n{}", style.red("Error"), HELP.render(style)).as_bytes(),
    )
}

/// `_add_validate_name`: the refusal, or nothing.
fn validate_name(name: &str) -> Option<String> {
    if name.is_empty() {
        return Some("Name is empty.".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Some(format!("Name cannot contain path separators: {name}"));
    }
    if name.contains("..") {
        return Some(format!("Name cannot contain '..': {name}"));
    }
    if name.starts_with('.') || name.starts_with('-') {
        return Some(format!("Name cannot start with '.' or '-': {name}"));
    }
    if !name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Some(format!(
            "Name may only contain letters, digits, hyphens, and underscores: {name}"
        ));
    }
    None
}

/// Why `entry` cannot be scaffolded, checked in the order `cmd_add` reports.
fn entry_refusal(entry: &Entry, root: &str) -> Option<ParseStop> {
    let Entry { kind, name, .. } = entry;
    if !matches!(kind.as_str(), "rule" | "skill" | "command" | "subagent") {
        return Some(ParseStop::Error(format!(
            "Unknown kind '{kind}'.\nValid kinds: rule, skill, command, subagent"
        )));
    }
    if let Some(message) = validate_name(name) {
        return Some(ParseStop::Error(message));
    }
    if kind == "skill" && !crate::config::skill_metadata::valid_name(name) {
        return Some(ParseStop::Error(format!(
            "Skill name must be 1–64 lowercase letters, digits, or single hyphens and cannot end with a hyphen: {name}"
        )));
    }
    if kind != "skill" && entry.category.is_some() {
        return Some(ParseStop::Usage(format!(
            "--category applies to skills, not {kind}"
        )));
    }
    if kind != "skill" {
        return None;
    }
    skill_place_refusal(root, &entry.category_path(), name).map(ParseStop::Error)
}

/// Why `category` cannot hold a skill, or nothing; an empty one is the root.
fn category_refusal(category: &str) -> Option<String> {
    if category.is_empty() {
        return None;
    }
    let segments: Vec<&str> = category.split('/').collect();
    if let Some(bad) = segments
        .iter()
        .find(|segment| !crate::config::skill_metadata::valid_name(segment))
    {
        return Some(format!(
            "Category segments must be lowercase letters, digits, or single hyphens: '{bad}' in {category}"
        ));
    }
    (segments.len() > skill_tree::MAX_CATEGORY_DEPTH).then(|| {
        format!(
            "Category is deeper than {} levels: {category}",
            skill_tree::MAX_CATEGORY_DEPTH
        )
    })
}

/// Why a skill named `name` cannot land in `category`, or nothing: a bad
/// category path, or the name already taken at another path.
fn skill_place_refusal(root: &str, category: &str, name: &str) -> Option<String> {
    if let Some(refusal) = category_refusal(category) {
        return Some(refusal);
    }
    let wanted = if category.is_empty() {
        name.to_string()
    } else {
        format!("{category}/{name}")
    };
    let skills = format!("{root}/.ai/src/skills");
    let tree = skill_tree::discover(&Workspace::on_disk(root), &skills);
    let taken = tree.find(name).filter(|skill| skill.rel != wanted)?;
    Some(format!(
        "Skill '{name}' already exists at .ai/src/skills/{}/\n\nSkill names are unique across categories — every tool installs skills flat by name.",
        taken.rel
    ))
}

/// `_add_resolve_dest`, below `.ai/src/`.
fn dest_rel(kind: &str, category: &str, name: &str) -> String {
    match kind {
        "rule" => format!(".ai/src/rules/{name}.md"),
        "skill" if category.is_empty() => format!(".ai/src/skills/{name}/SKILL.md"),
        "skill" => format!(".ai/src/skills/{category}/{name}/SKILL.md"),
        "command" => format!(".ai/src/commands/{name}.md"),
        _ => format!(".ai/src/agents/{name}.md"),
    }
}

/// `{{TITLE}}`: hyphens become spaces and each part starts upper-case, as the
/// awk program splits it, so `my_tool-x2--y` reads `My_tool X2  Y`.
fn title(name: &str) -> String {
    name.split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// The `sed` pass: placeholders everywhere, the sentinel `name:` lines whole.
fn render(template: &str, name: &str) -> String {
    let text = template
        .replace("{{TITLE}}", &title(name))
        .replace("{{NAME}}", name);
    text.split('\n')
        .map(|line| {
            if line == "name: \"content\"" || line == "name: \"template-agent\"" {
                format!("name: \"{name}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

struct Entry {
    kind: String,
    name: String,
    category: Option<String>,
    force: bool,
}

impl Entry {
    /// The `--category` path without a trailing `/`, empty for the root.
    fn category_path(&self) -> String {
        self.category
            .as_deref()
            .map_or_else(String::new, |c| c.trim_end_matches('/').to_string())
    }
}

/// How a command line that names no entry ends: help on stdout, a refusal
/// followed by the usage, or a bare error line.
enum ParseStop {
    Help,
    Usage(String),
    Error(String),
}

fn parse_entry(args: &[String]) -> Result<Entry, ParseStop> {
    let mut entry = Entry {
        kind: String::new(),
        name: String::new(),
        category: None,
        force: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--force" | "-f" => entry.force = true,
            "--category" => match args.next().filter(|value| !value.starts_with('-')) {
                Some(value) => entry.category = Some(value.clone()),
                None => return Err(ParseStop::Usage("--category requires a value".into())),
            },
            "--help" | "-h" => return Err(ParseStop::Help),
            flag if flag.starts_with('-') => {
                return Err(ParseStop::Error(format!("Unknown flag: {flag}")));
            }
            positional if entry.kind.is_empty() => entry.kind = positional.to_string(),
            positional if entry.name.is_empty() => entry.name = positional.to_string(),
            positional => {
                return Err(ParseStop::Usage(format!(
                    "Unexpected argument: {positional}"
                )));
            }
        }
    }
    if entry.kind.is_empty() {
        return Err(ParseStop::Usage("missing <kind> and <name>".into()));
    }
    if entry.name.is_empty() {
        return Err(ParseStop::Usage(format!(
            "missing <name> for {}",
            entry.kind
        )));
    }
    Ok(entry)
}

/// `cmd_add`: the report on `out`, refusals on `err`, the status as the result.
pub fn add(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    if args.first().map(String::as_str) == Some("mcp") {
        return add_mcp(&args[1..], root, style, out, err);
    }
    let entry = match parse_entry(args) {
        Ok(entry) => entry,
        Err(stop) => return report_stop(stop, style, out, err),
    };
    if let Some(stop) = entry_refusal(&entry, root) {
        return report_stop(stop, style, out, err);
    }
    let category = entry.category_path();
    let Entry {
        kind, name, force, ..
    } = entry;
    let Some(template) = catalog::content_template(&kind) else {
        put(
            err,
            format!(
                "{}: Template missing: {}/lib/templates/content/{kind}.md\n",
                style.red("Error"),
                crate::paths::ENGINE_ROOT
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let rel = dest_rel(&kind, &category, &name);
    let dest = PathBuf::from(format!("{root}/{rel}"));
    if dest.exists() && !force {
        put(
            err,
            format!(
                "{}: Already exists: {}\n\nPass {} to overwrite, or pick a different name.\n",
                style.red("Error"),
                dest.display(),
                style.cyan("--force")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
    }
    std::fs::write(&dest, render(template, &name)).map_err(|e| Error::io(&dest, e))?;
    put(
        out,
        format!(
            "\n{} {rel}\n\nEdit the file, then run {} to propagate.\n\n",
            style.green(&format!("Created {kind}:")),
            style.cyan("exuno sync")
        )
        .as_bytes(),
    )
    .map(|()| 0)
}

/// `_add_json_escape`.
fn json_escape(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for c in raw.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            other => out.push(other),
        }
    }
    out
}

fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r' | '\x0b' | '\x0c')
}

/// `_add_mcp_build_entry`: the compact-to-be JSON object, or the `--env`
/// refusal.
fn build_entry(
    url: &str,
    command: &str,
    args: &str,
    env: &str,
    style: &Style,
) -> Result<String, String> {
    let mut fields = if !url.is_empty() {
        format!("\"type\": \"http\", \"url\": \"{}\"", json_escape(url))
    } else {
        let mut fields = format!("\"command\": \"{}\"", json_escape(command));
        if !args.is_empty() {
            let list = args
                .split(is_blank)
                .filter(|token| !token.is_empty())
                .map(|token| format!("\"{}\"", json_escape(token)))
                .collect::<Vec<_>>()
                .join(", ");
            fields.push_str(&format!(", \"args\": [{list}]"));
        }
        fields
    };
    if !env.is_empty() {
        let mut members = Vec::new();
        for pair in env.split([',', '\n']) {
            let pair = pair.trim_matches(is_blank);
            if pair.is_empty() {
                continue;
            }
            let Some((key, value)) = pair.split_once('=') else {
                return Err(format!(
                    "{}: --env entry '{pair}' must be KEY=VALUE.\n",
                    style.red("Error")
                ));
            };
            members.push(format!(
                "\"{}\": \"{}\"",
                json_escape(key.trim_matches(is_blank)),
                json_escape(value)
            ));
        }
        if !members.is_empty() {
            fields.push_str(&format!(", \"env\": {{{}}}", members.join(", ")));
        }
    }
    Ok(format!("{{{fields}}}"))
}

/// Why `_add_mcp_merge` refused: the server exists (awk exit 3), or the
/// `mcpServers` member is not an object (awk exit 2).
enum Refusal {
    Exists,
    Malformed,
}

fn is_ws(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\r')
}

/// awk `skip_string`: `at` is the opening quote; the index past the closing
/// quote, or the end.
fn skip_string(s: &[u8], at: usize) -> usize {
    let mut i = at + 1;
    let mut escaped = false;
    while i < s.len() {
        let c = s[i];
        if escaped {
            escaped = false;
        } else if c == b'\\' {
            escaped = true;
        } else if c == b'"' {
            return i + 1;
        }
        i += 1;
    }
    i
}

/// awk `skip_value`: the index past a string, a balanced object or array,
/// or a scalar, blanks before it skipped.
fn skip_value(s: &[u8], at: usize) -> usize {
    let mut i = at;
    while i < s.len() && is_ws(s[i]) {
        i += 1;
    }
    match s.get(i) {
        Some(b'"') => skip_string(s, i),
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            let mut in_string = false;
            let mut escaped = false;
            while i < s.len() {
                let c = s[i];
                if in_string {
                    if escaped {
                        escaped = false;
                    } else if c == b'\\' {
                        escaped = true;
                    } else if c == b'"' {
                        in_string = false;
                    }
                } else if c == b'"' {
                    in_string = true;
                } else if c == b'{' || c == b'[' {
                    depth += 1;
                } else if c == b'}' || c == b']' {
                    depth -= 1;
                    if depth == 0 {
                        return i + 1;
                    }
                }
                i += 1;
            }
            i
        }
        _ => {
            while i < s.len() {
                let c = s[i];
                if c == b',' || c == b'}' || c == b']' || is_ws(c) {
                    break;
                }
                i += 1;
            }
            i
        }
    }
}

/// awk `compact`: blanks outside strings dropped.
fn compact(value: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(value.len());
    let mut in_string = false;
    let mut escaped = false;
    for &c in value {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == b'\\' {
                escaped = true;
            } else if c == b'"' {
                in_string = false;
            }
        } else if c == b'"' {
            in_string = true;
            out.push(c);
        } else if !is_ws(c) {
            out.push(c);
        }
    }
    out
}

fn slice(s: &[u8], from: usize, to: usize) -> &[u8] {
    if from < to && to <= s.len() {
        &s[from..to]
    } else {
        &[]
    }
}

/// The members of the object whose `{` is at `open`, in file order: each key
/// as written between its quotes, with its value.
fn members(s: &[u8], open: usize) -> Vec<(&[u8], &[u8])> {
    let end = skip_value(s, open);
    let skip_ws = |mut at: usize| {
        while at < s.len() && is_ws(s[at]) {
            at += 1;
        }
        at
    };
    let mut found = Vec::new();
    let mut p = open + 1;
    while p + 1 < end {
        let c = s[p];
        if is_ws(c) || c == b',' {
            p += 1;
            continue;
        }
        if c != b'"' {
            break;
        }
        let key_end = skip_string(s, p);
        let mut q = skip_ws(key_end);
        if s.get(q) == Some(&b':') {
            q += 1;
        }
        let q = skip_ws(q);
        let value_end = skip_value(s, q);
        found.push((
            slice(s, p + 1, key_end.saturating_sub(1)),
            slice(s, q, value_end),
        ));
        p = value_end;
    }
    found
}

/// `_add_mcp_merge`: the file re-emitted canonically with `entry` spliced in
/// under `server` in its top-level `mcpServers`, every other top-level member
/// kept in order. An empty file starts a fresh object; a file that is not a
/// JSON object, or whose `mcpServers` is not one, is refused.
fn merge(content: &[u8], server: &str, entry: &str, force: bool) -> Result<Vec<u8>, Refusal> {
    let top = match content.iter().position(|b| !is_ws(*b)) {
        None => Vec::new(),
        Some(root) => {
            if !serde_json::from_slice::<serde_json::Value>(content).is_ok_and(|v| v.is_object()) {
                return Err(Refusal::Malformed);
            }
            members(content, root)
        }
    };
    let mut servers: Vec<(&[u8], Vec<u8>)> =
        match top.iter().find(|(name, _)| *name == b"mcpServers") {
            None => Vec::new(),
            Some((_, value)) if value.first() == Some(&b'{') => members(value, 0)
                .into_iter()
                .map(|(name, value)| (name, value.to_vec()))
                .collect(),
            Some(_) => return Err(Refusal::Malformed),
        };
    match servers
        .iter()
        .position(|(name, _)| *name == server.as_bytes())
    {
        Some(_) if !force => return Err(Refusal::Exists),
        Some(found) => servers[found].1 = entry.as_bytes().to_vec(),
        None => servers.push((server.as_bytes(), entry.as_bytes().to_vec())),
    }
    let mut block = b"  \"mcpServers\": {".to_vec();
    for (index, (name, value)) in servers.iter().enumerate() {
        if index > 0 {
            block.push(b',');
        }
        block.extend_from_slice(b"\n    \"");
        block.extend_from_slice(name);
        block.extend_from_slice(b"\": ");
        block.extend_from_slice(&compact(value));
    }
    block.extend_from_slice(b"\n  }");
    let mut parts: Vec<Vec<u8>> = Vec::new();
    for (name, value) in &top {
        if *name == b"mcpServers" {
            if !block.is_empty() {
                parts.push(std::mem::take(&mut block));
            }
            continue;
        }
        let mut part = b"  \"".to_vec();
        part.extend_from_slice(name);
        part.extend_from_slice(b"\": ");
        part.extend_from_slice(&compact(value));
        parts.push(part);
    }
    if !block.is_empty() {
        parts.push(block);
    }
    let mut out = b"{\n".to_vec();
    out.extend_from_slice(&parts.join(&b",\n"[..]));
    out.extend_from_slice(b"\n}\n");
    Ok(out)
}

struct McpArgs {
    server: String,
    url: String,
    command: String,
    args: String,
    env: String,
    force: bool,
}

fn parse_mcp(args: &[String]) -> Result<McpArgs, ParseStop> {
    let mut parsed = McpArgs {
        server: String::new(),
        url: String::new(),
        command: String::new(),
        args: String::new(),
        env: String::new(),
        force: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            flag @ ("--url" | "--command" | "--args" | "--env") => {
                let Some(value) = args.next() else {
                    return Err(ParseStop::Usage(format!("{flag} requires a value.")));
                };
                let slot = match flag {
                    "--url" => &mut parsed.url,
                    "--command" => &mut parsed.command,
                    "--args" => &mut parsed.args,
                    _ => &mut parsed.env,
                };
                *slot = value.clone();
            }
            "--force" | "-f" => parsed.force = true,
            "-h" | "--help" => return Err(ParseStop::Help),
            flag if flag.starts_with('-') => {
                return Err(ParseStop::Usage(format!("Unknown flag: {flag}")));
            }
            positional if parsed.server.is_empty() => parsed.server = positional.to_string(),
            positional => {
                return Err(ParseStop::Usage(format!(
                    "Unexpected argument: {positional}"
                )));
            }
        }
    }
    if parsed.server.is_empty() {
        return Err(ParseStop::Usage("missing <server> for mcp".into()));
    }
    Ok(parsed)
}

/// Why `--url` and `--command` together name no single transport.
fn transport_refusal(url: &str, command: &str) -> Option<ParseStop> {
    match (url.is_empty(), command.is_empty()) {
        (true, true) => Some(ParseStop::Usage(
            "one of --url or --command is required.".into(),
        )),
        (false, false) => Some(ParseStop::Error(
            "--url and --command are mutually exclusive.".into(),
        )),
        _ => None,
    }
}

fn report_stop(
    stop: ParseStop,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match stop {
        ParseStop::Help => {
            put(out, HELP.render(style).as_bytes())?;
            Ok(0)
        }
        ParseStop::Usage(message) => {
            usage(&message, style, err)?;
            Ok(1)
        }
        ParseStop::Error(message) => {
            put(
                err,
                format!("{}: {message}\n", style.red("Error")).as_bytes(),
            )?;
            Ok(1)
        }
    }
}

/// `cmd_add_mcp`.
fn add_mcp(
    args: &[String],
    root: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    let McpArgs {
        server,
        url,
        command,
        args: args_str,
        env: env_str,
        force,
    } = match parse_mcp(args) {
        Ok(parsed) => parsed,
        Err(stop) => return report_stop(stop, style, out, err),
    };
    if let Some(message) = validate_name(&server) {
        return report_stop(ParseStop::Error(message), style, out, err);
    }
    if let Some(stop) = transport_refusal(&url, &command) {
        return report_stop(stop, style, out, err);
    }
    let entry = match build_entry(&url, &command, &args_str, &env_str, style) {
        Ok(entry) => entry,
        Err(refusal) => {
            put(err, refusal.as_bytes())?;
            return Ok(1);
        }
    };
    let mcp_file = PathBuf::from(format!("{root}/.ai/src/mcp.json"));
    let mut created = false;
    if !mcp_file.is_file() {
        let parent = Path::new(root).join(".ai/src");
        std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
        std::fs::write(&mcp_file, EMPTY_MCP).map_err(|e| Error::io(&mcp_file, e))?;
        created = true;
    }
    let content = std::fs::read(&mcp_file).map_err(|e| Error::io(&mcp_file, e))?;
    match merge(&content, &server, &entry, force) {
        Ok(bytes) => staging::write_beside(&mcp_file, &bytes)?,
        Err(Refusal::Exists) => {
            put(
                err,
                format!(
                    "{}: server '{server}' already exists. Pass --force to overwrite.\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
        Err(Refusal::Malformed) => {
            put(
                err,
                format!(
                    "{}: .ai/src/mcp.json is not a JSON object with an object under mcpServers.\nFix it by hand, then run the command again.\n",
                    style.red("Error")
                )
                .as_bytes(),
            )?;
            return Ok(1);
        }
    }
    let headline = if created {
        style.green("Created shared MCP source:")
    } else {
        style.green("Updated shared MCP source:")
    };
    put(
        out,
        format!(
            "\n{headline} .ai/src/mcp.json\n{} {server}\n\nThis MCP source applies to every enabled tool on next {}.\nAdd a per-tool override with {} if needed.\n\n",
            style.green("Added server:"),
            style.cyan("exuno sync"),
            style.cyan("exuno customize <tool> mcp")
        )
        .as_bytes(),
    )
    .map(|()| 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::paths::DiskText;

    #[test]
    fn titles_split_on_hyphens_like_the_awk_program() {
        assert_eq!(title("testing"), "Testing");
        assert_eq!(title("my-skill"), "My Skill");
        assert_eq!(title("my_tool-x2--y"), "My_tool X2  Y");
    }

    #[test]
    fn templates_are_filled_like_sed() {
        let skill = render(catalog::content_template("skill").unwrap(), "my-skill");
        assert!(skill.contains("\nname: \"my-skill\"\n"));
        assert!(skill.contains("\n# My Skill\n"));
        let agent = render(catalog::content_template("subagent").unwrap(), "reviewer");
        assert!(agent.starts_with("---\nname: \"reviewer\"\ndescription: >-\n"));
        let rule = render(catalog::content_template("rule").unwrap(), "testing");
        assert!(rule.starts_with("# Testing\n\n- One imperative"));
        assert!(rule.ends_with("native glob trigger.\n"));
    }

    #[test]
    fn names_are_refused_like_add_validate_name() {
        let refusal = |name: &str| {
            let (mut out, mut err) = (Vec::new(), Vec::new());
            if let Some(message) = validate_name(name) {
                report_stop(
                    ParseStop::Error(message),
                    &Style::plain(),
                    &mut out,
                    &mut err,
                )
                .unwrap();
            }
            String::from_utf8(err).unwrap()
        };
        assert_eq!(refusal(""), "Error: Name is empty.\n");
        assert_eq!(
            refusal("sub/dir"),
            "Error: Name cannot contain path separators: sub/dir\n"
        );
        assert_eq!(
            refusal("a\\b"),
            "Error: Name cannot contain path separators: a\\b\n"
        );
        assert_eq!(
            refusal("..evil"),
            "Error: Name cannot contain '..': ..evil\n"
        );
        assert_eq!(
            refusal(".hidden"),
            "Error: Name cannot start with '.' or '-': .hidden\n"
        );
        assert_eq!(
            refusal("my rule"),
            "Error: Name may only contain letters, digits, hyphens, and underscores: my rule\n"
        );
        assert_eq!(
            refusal("règle"),
            "Error: Name may only contain letters, digits, hyphens, and underscores: règle\n"
        );
        assert_eq!(refusal("my_rule-42"), "");
    }

    #[test]
    fn entries_are_built_like_add_mcp_build_entry() {
        let style = Style::plain();
        let entry = |url: &str, command: &str, args: &str, env: &str| {
            build_entry(url, command, args, env, &style)
        };
        assert_eq!(
            entry("https://mcp.linear.app/sse", "", "", "").unwrap(),
            "{\"type\": \"http\", \"url\": \"https://mcp.linear.app/sse\"}"
        );
        assert_eq!(
            entry(
                "",
                "fs-server",
                "--root /tmp   --debug",
                " TOKEN = abc ,DEBUG=1,, "
            )
            .unwrap(),
            "{\"command\": \"fs-server\", \"args\": [\"--root\", \"/tmp\", \"--debug\"], \"env\": {\"TOKEN\": \" abc\", \"DEBUG\": \"1\"}}"
        );
        assert_eq!(
            entry("", "say \"hi\" path\\to", "x\"y z\\w", "MSG=a\"b").unwrap(),
            "{\"command\": \"say \\\"hi\\\" path\\\\to\", \"args\": [\"x\\\"y\", \"z\\\\w\"], \"env\": {\"MSG\": \"a\\\"b\"}}"
        );
        assert_eq!(entry("", "c", "", ",").unwrap(), "{\"command\": \"c\"}");
        assert_eq!(
            entry("", "a\tb\nc\rd", "", "K=v\tw").unwrap(),
            "{\"command\": \"a\\tb\\nc\\rd\", \"env\": {\"K\": \"v\\tw\"}}"
        );
        assert_eq!(
            entry("https://x", "", "", "A= b =c").unwrap(),
            "{\"type\": \"http\", \"url\": \"https://x\", \"env\": {\"A\": \" b =c\"}}"
        );
        assert_eq!(
            entry("", "c", "", "NOEQ").unwrap_err(),
            "Error: --env entry 'NOEQ' must be KEY=VALUE.\n"
        );
        assert_eq!(
            entry("", "c", "-y\n@pkg", "A=1\nB=2").unwrap(),
            "{\"command\": \"c\", \"args\": [\"-y\", \"@pkg\"], \"env\": {\"A\": \"1\", \"B\": \"2\"}}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn an_invalid_mcp_file_is_refused_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join(".ai/src")).unwrap();
        std::fs::write(dir.path().join(".ai/src/mcp.json"), "garbage\n").unwrap();
        let (status, _, err) = run(&root, &["mcp", "n", "--url", "https://n"]);
        assert_eq!(
            (status, err.as_str()),
            (
                1,
                "Error: .ai/src/mcp.json is not a JSON object with an object under mcpServers.\nFix it by hand, then run the command again.\n"
            )
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join(".ai/src/mcp.json")).unwrap(),
            "garbage\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_refused_env_leaves_no_mcp_file_behind() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        let (status, _, err) = run(&root, &["mcp", "bad", "--command", "c", "--env", "NOEQ"]);
        assert_eq!(
            (status, err.as_str()),
            (1, "Error: --env entry 'NOEQ' must be KEY=VALUE.\n")
        );
        assert!(!dir.path().join(".ai/src/mcp.json").exists());
    }

    fn merged(content: &str, server: &str, entry: &str, force: bool) -> String {
        String::from_utf8(
            merge(content.as_bytes(), server, entry, force)
                .ok()
                .unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn servers_are_spliced_like_the_awk_merge() {
        let github = "{\"command\": \"npx @github/mcp-server\"}";
        let one = merged(EMPTY_MCP, "github", github, false);
        assert_eq!(
            one,
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"}\n  }\n}\n"
        );
        let two = merged(
            &one,
            "linear",
            "{\"type\": \"http\", \"url\": \"https://l\"}",
            false,
        );
        assert_eq!(
            two,
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"},\n    \"linear\": {\"type\":\"http\",\"url\":\"https://l\"}\n  }\n}\n"
        );
        assert!(matches!(
            merge(two.as_bytes(), "github", "{\"command\": \"other\"}", false),
            Err(Refusal::Exists)
        ));
        assert_eq!(
            merged(&two, "github", "{\"command\": \"other\"}", true),
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"other\"},\n    \"linear\": {\"type\":\"http\",\"url\":\"https://l\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\":{\"a\":{\"k\":\"v\"}}}",
                "b",
                "{\"type\": \"http\", \"url\": \"https://b\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"a\": {\"k\":\"v\"},\n    \"b\": {\"type\":\"http\",\"url\":\"https://b\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\": {\"esc\\\"aped\": {}}}\n",
                "n",
                "{\"type\": \"http\", \"url\": \"https://n\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"esc\\\"aped\": {},\n    \"n\": {\"type\":\"http\",\"url\":\"https://n\"}\n  }\n}\n"
        );
        assert_eq!(
            merged(
                "{\"mcpServers\": {\"a\": { \"env\": { \"X\": \"}\\\"{\" }, \"args\": [ \"1\", [ \"2\" ] ] }, \"b\": \"str\", \"c\": 12}, \"trailing\": true}\n",
                "d",
                "{\"type\": \"http\", \"url\": \"https://d\"}",
                false
            ),
            "{\n  \"mcpServers\": {\n    \"a\": {\"env\":{\"X\":\"}\\\"{\"},\"args\":[\"1\",[\"2\"]]},\n    \"b\": \"str\",\n    \"c\": 12,\n    \"d\": {\"type\":\"http\",\"url\":\"https://d\"}\n  },\n  \"trailing\": true\n}\n"
        );
    }

    #[test]
    fn other_members_stay_and_invalid_files_are_refused() {
        let n = "{\"type\": \"http\", \"url\": \"https://n\"}";
        let n_line = "    \"n\": {\"type\":\"http\",\"url\":\"https://n\"}\n";
        assert_eq!(
            merged("", "n", n, false),
            format!("{{\n  \"mcpServers\": {{\n{n_line}  }}\n}}\n")
        );
        assert_eq!(
            merged("{\"servers\": {}}\n", "n", n, false),
            format!("{{\n  \"servers\": {{}},\n  \"mcpServers\": {{\n{n_line}  }}\n}}\n")
        );
        assert_eq!(
            merged(
                "{\n  \"other\": {\"mcpServers\": 1},\n  \"mcpServers\": {}\n}\n",
                "n",
                n,
                false
            ),
            format!(
                "{{\n  \"other\": {{\"mcpServers\":1}},\n  \"mcpServers\": {{\n{n_line}  }}\n}}\n"
            )
        );
        for invalid in [
            "garbage\n",
            "[1]\n",
            "{\"mcpServers\": {\"a\": {}, 1: {}, \"z\": {}}}\n",
            "{\"mcpServers\": []}\n",
        ] {
            assert!(
                matches!(
                    merge(invalid.as_bytes(), "n", n, false),
                    Err(Refusal::Malformed)
                ),
                "{invalid}"
            );
        }
    }

    #[cfg(unix)]
    fn run(root: &str, args: &[&str]) -> (u8, String, String) {
        let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let status = add(&args, root, &Style::plain(), &mut out, &mut err).unwrap();
        (
            status,
            String::from_utf8(out).unwrap(),
            String::from_utf8(err).unwrap(),
        )
    }

    #[cfg(unix)]
    #[test]
    fn a_rule_is_scaffolded_once_like_cmd_add() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        let (status, out, err) = run(&root, &["rule", "testing"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\nCreated rule: .ai/src/rules/testing.md\n\nEdit the file, then run exuno sync to propagate.\n\n"
        );
        let written = std::fs::read_to_string(dir.path().join(".ai/src/rules/testing.md")).unwrap();
        assert_eq!(
            written,
            catalog::content_template("rule")
                .unwrap()
                .replace("{{TITLE}}", "Testing")
        );
        let (status, out, err) = run(&root, &["rule", "testing"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "Error: Already exists: {root}/.ai/src/rules/testing.md\n\nPass --force to overwrite, or pick a different name.\n"
            )
        );
        let (status, _, err) = run(&root, &["rule", "testing", "--force", "extra"]);
        assert_eq!(status, 1);
        assert!(err.starts_with(
            "Error: Unexpected argument: extra\n\n  exuno add — scaffold a rule, skill, command, subagent, or MCP server\n\n  USAGE\n"
        ));
        let (status, _, err) = run(&root, &["rule"]);
        assert_eq!(status, 1);
        assert!(err.starts_with("Error: missing <name> for rule\n\n  exuno add — "));
        let (status, out, _) = run(&root, &["-h"]);
        assert_eq!((status, out), (0, HELP.render(&Style::plain())));
    }

    #[test]
    fn help_renders_both_forms_with_their_kinds_and_mcp_options() {
        assert_eq!(
            HELP.render(&Style::plain()),
            "\n  exuno add — scaffold a rule, skill, command, subagent, or MCP server\n\n  USAGE\n    exuno add <kind> <name> [--category <path>] [--force]\n    exuno add mcp <server> (--url URL | --command CMD) [MCP OPTIONS] [--force]\n\n  DESCRIPTION\n    Scaffold a new entry under .ai/src/ from the shipped content templates,\n    or add one server entry to the shared .ai/src/mcp.json.\n\n    Edit the scaffold, then run exuno sync to propagate.\n\n  KINDS\n    rule       Create .ai/src/rules/<name>.md\n    skill      Create .ai/src/skills/[<category>/]<name>/SKILL.md\n    command    Create .ai/src/commands/<name>.md\n    subagent   Create .ai/src/agents/<name>.md\n    mcp        Add an MCP server entry to .ai/src/mcp.json\n\n  MCP OPTIONS\n    --url URL            HTTP server endpoint\n    --command CMD        Command that starts the server\n    --args \"a b c\"       Command arguments, split on whitespace\n    --env K=V[,K=V...]   Environment variables for the server\n\n  OPTIONS\n    --category <path>   Place a skill in a category, e.g. flutter/ui\n    -f, --force         Overwrite an existing file or server entry\n    -h, --help          Show this help\n\n  EXAMPLES\n    exuno add rule testing\n    exuno add skill deploy\n    exuno add skill slivers --category flutter/ui\n    exuno add mcp linear --url https://mcp.linear.app/sse\n    exuno add mcp github --command npx --args \"-y @github/mcp-server\"\n\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_server_is_added_once_like_cmd_add_mcp() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        let (status, out, err) = run(
            &root,
            &["mcp", "github", "--command", "npx @github/mcp-server"],
        );
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(
            out,
            "\nCreated shared MCP source: .ai/src/mcp.json\nAdded server: github\n\nThis MCP source applies to every enabled tool on next exuno sync.\nAdd a per-tool override with exuno customize <tool> mcp if needed.\n\n"
        );
        let file = dir.path().join(".ai/src/mcp.json");
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "{\n  \"mcpServers\": {\n    \"github\": {\"command\":\"npx @github/mcp-server\"}\n  }\n}\n"
        );
        let (status, out, err) = run(&root, &["mcp", "github", "--command", "other"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            "Error: server 'github' already exists. Pass --force to overwrite.\n"
        );
        let (status, out, err) = run(&root, &["mcp", "gh", "--url"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(
            err,
            format!(
                "Error: --url requires a value.\n{}",
                HELP.render(&Style::plain())
            )
        );
        let (status, _, err) = run(&root, &["mcp"]);
        assert_eq!(status, 1);
        assert!(err.starts_with("Error: missing <server> for mcp\n\n  exuno add — "));
        let (status, out, err) = run(&root, &["mcp", "bad", "--command", "c", "--env", "NOEQ"]);
        assert_eq!((status, out.as_str()), (1, ""));
        assert_eq!(err, "Error: --env entry 'NOEQ' must be KEY=VALUE.\n");
        let (status, out, err) = run(&root, &["mcp", "-h"]);
        assert_eq!((status, err.as_str()), (0, ""));
        assert_eq!(out, HELP.render(&Style::plain()));
    }
}
