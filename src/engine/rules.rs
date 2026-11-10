//! Rule, command, and subagent directory operations of
//! `lib/helpers/rule_operations.sh` and the directory loops of
//! `lib/helpers/format_conversion.sh`.

use crate::engine::filters::Filter;
use crate::engine::session::Session;
use crate::{Error, engine::convert, engine::file_ops, engine::frontmatter, paths, text};

/// `add_header`: `printf '%b\n'` of the header, a blank line, the file.
pub fn add_header(file: &[u8], header: &str) -> Vec<u8> {
    let (mut out, stopped) = text::printf_b(header.as_bytes());
    if !stopped {
        out.push(b'\n');
    }
    out.push(b'\n');
    out.extend_from_slice(file);
    out
}

/// `merge_or_prepend_header`: a file without frontmatter gets the header; a
/// file with frontmatter gains only the header keys it lacks.
pub fn merge_or_prepend_header(file: &[u8], header: &str) -> Vec<u8> {
    let lines = text::lines(file);
    if lines.first().copied().unwrap_or(b"") != b"---" {
        return add_header(file, header);
    }
    let mut existing: Vec<&[u8]> = Vec::new();
    let mut in_fm = false;
    for line in &lines {
        if *line == b"---" {
            if in_fm {
                break;
            }
            in_fm = true;
            continue;
        }
        if in_fm && let Some(key) = frontmatter::key(line) {
            existing.push(key);
        }
    }
    let (expanded, _) = text::printf_b(header.as_bytes());
    let mut additions = Vec::new();
    for hline in text::lines(&expanded) {
        if hline == b"---" || hline.is_empty() {
            continue;
        }
        if let Some(key) = frontmatter::key(hline)
            && !existing.contains(&key)
        {
            additions.extend_from_slice(hline);
            additions.push(b'\n');
        }
    }
    if additions.is_empty() {
        return file.to_vec();
    }
    let mut out = Vec::with_capacity(file.len() + additions.len());
    let mut fences = 0;
    for line in lines {
        if line == b"---" {
            fences += 1;
            if fences == 2 {
                out.extend_from_slice(&additions);
            }
        }
        out.extend_from_slice(line);
        out.push(b'\n');
    }
    out
}

/// `_rule_paths_csv`: every list item in the leading frontmatter, joined with
/// commas, when that frontmatter has a bare `paths:` key.
pub fn rule_paths_csv(file: &[u8]) -> Vec<u8> {
    rule_paths(file).join(&b","[..])
}

/// The globs of the leading frontmatter's `paths:` key, unquoted: a flow list
/// `[a, b]`, a single scalar, or the `- item` lines directly below the key.
fn rule_paths(file: &[u8]) -> Vec<Vec<u8>> {
    let lines = text::lines(file);
    if lines.first().copied() != Some(b"---".as_slice()) {
        return Vec::new();
    }
    let block: Vec<&[u8]> = lines
        .iter()
        .skip(1)
        .take_while(|line| **line != b"---")
        .copied()
        .collect();
    let Some(key) = block
        .iter()
        .position(|line| text::after_key(line, "paths").is_some())
    else {
        return Vec::new();
    };
    let value = text::trim_end_space(text::after_key(block[key], "paths").unwrap_or_default());
    if let Some(flow) = value.strip_prefix(b"[").and_then(|v| v.strip_suffix(b"]")) {
        return flow
            .split(|b| *b == b',')
            .map(|item| unquote(text::trim_end_space(text::trim_start_space(item))))
            .filter(|item| !item.is_empty())
            .collect();
    }
    if !value.is_empty() {
        return vec![unquote(value)];
    }
    let mut items = Vec::new();
    for line in &block[key + 1..] {
        let rest = text::trim_start_space(line);
        let Some(after_dash) = rest
            .strip_prefix(b"-")
            .filter(|after| after.first().is_some_and(|b| text::is_space(*b)))
        else {
            break;
        };
        items.push(unquote(text::trim_start_space(after_dash)));
    }
    items
}

fn unquote(item: &[u8]) -> Vec<u8> {
    let mut item = item;
    if let Some(first) = item.first()
        && (*first == b'"' || *first == b'\'')
    {
        item = &item[1..];
    }
    if let Some(last) = item.last()
        && (*last == b'"' || *last == b'\'')
    {
        item = &item[..item.len() - 1];
    }
    item.to_vec()
}

/// `_strip_frontmatter`: the leading `---` block and the blank lines after it.
pub fn strip_frontmatter(file: &[u8]) -> Vec<u8> {
    let lines = text::lines(file);
    if lines.first().copied() != Some(b"---".as_slice()) {
        return file.to_vec();
    }
    let close = lines.iter().skip(1).position(|l| *l == b"---");
    let Some(close) = close.map(|i| i + 1) else {
        return Vec::new();
    };
    let rest = &lines[close + 1..];
    let first_content = rest
        .iter()
        .position(|l| !l.is_empty())
        .unwrap_or(rest.len());
    let kept = &rest[first_content..];
    let mut out = Vec::new();
    for (i, line) in kept.iter().enumerate() {
        out.extend_from_slice(line);
        if i + 1 < kept.len() || file.ends_with(b"\n") {
            out.push(b'\n');
        }
    }
    out
}

/// `apply_rule_header`: a `paths:`-scoped rule takes the scoped header with
/// `{globs}` filled as a comma-joined string and `{globs_list}` as a YAML
/// flow list of single-quoted globs; otherwise the always-on header is merged in.
pub fn apply_rule_header(file: &[u8], header: &str, scoped_header: &str) -> Vec<u8> {
    let paths = rule_paths(file);
    if !paths.is_empty() && !scoped_header.is_empty() {
        let csv = String::from_utf8_lossy(&paths.join(&b","[..])).into_owned();
        let quoted: Vec<String> = paths
            .iter()
            .map(|glob| format!("'{}'", String::from_utf8_lossy(glob).replace('\'', "''")))
            .collect();
        let list = format!("[{}]", quoted.join(", "));
        let scoped = scoped_header
            .replace("{globs_list}", &list)
            .replace("{globs}", &csv);
        return add_header(&strip_frontmatter(file), &scoped);
    }
    if !header.is_empty() {
        return merge_or_prepend_header(file, header);
    }
    file.to_vec()
}

fn md_files(s: &Session, dir: &str) -> Vec<String> {
    s.ws.glob(dir)
        .into_iter()
        .filter(|name| name.ends_with(".md") && s.ws.is_file(&format!("{dir}/{name}")))
        .collect()
}

fn read(s: &Session, path: &str) -> Result<Vec<u8>, Error> {
    s.ws.read(path)
}

/// `append_imports`.
pub fn append_imports(s: &mut Session, agents_file: &str, rules_dir: &str) -> Result<(), Error> {
    if !s.ws.is_file(agents_file) {
        s.log
            .warning(&format!("Agents file not found: {agents_file}"));
        return Ok(());
    }
    if !s.ws.is_dir(rules_dir) {
        s.log.warning(&format!(
            "Rules directory not found for imports: {rules_dir}"
        ));
        return Ok(());
    }
    let mut block = b"\n<!-- Auto-generated imports -->\n".to_vec();
    for name in md_files(s, rules_dir) {
        block.extend_from_slice(format!("@rules/{name}\n").as_bytes());
    }
    s.ws.append(agents_file, &block)?;
    s.record_write(agents_file);
    Ok(())
}

/// `merge_rules_to_file`.
pub fn merge_rules_to_file(
    s: &mut Session,
    src_dir: &str,
    dest_file: &str,
    filter: &Filter,
    agents_file: Option<&str>,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        s.log.warning(&format!("Rules source not found: {src_dir}"));
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_file);
    let files: Vec<String> = md_files(s, src_dir)
        .into_iter()
        .filter(|name| filter.accepts(name))
        .collect();

    if s.dry_run {
        let extra = if agents_file.is_some() {
            " +agents"
        } else {
            ""
        };
        s.log.step(&format!(
            "{src_disp}/ → {dest_disp} ({}{extra}) (dry-run)",
            merged(files.len())
        ));
        return Ok(());
    }
    s.ws.create_dir_all(&paths::parent(dest_file))?;
    s.ws.remove(dest_file)?;
    if let Some(agents) = agents_file.filter(|a| s.ws.is_file(a)) {
        let mut preamble = read(s, agents)?;
        preamble.extend_from_slice(b"\n---\n\n");
        s.ws.append(dest_file, &preamble)?;
    }
    for (i, name) in files.iter().enumerate() {
        let mut chunk = if i == 0 {
            Vec::new()
        } else {
            b"\n---\n\n".to_vec()
        };
        chunk.extend(read(s, &format!("{src_dir}/{name}"))?);
        s.ws.append(dest_file, &chunk)?;
    }
    s.record_write(dest_file);
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp} ({})",
        merged(files.len())
    ));
    Ok(())
}

fn merged(count: usize) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} file{plural} merged")
}

pub struct RuleOptions<'a> {
    pub extension: &'a str,
    pub header: &'a str,
    pub scoped_header: &'a str,
    pub filter: &'a Filter,
}

/// `sync_rules`: copy with the extension and header applied, then prune
/// managed files the source no longer has.
pub fn sync_rules(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    opts: &RuleOptions<'_>,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        s.log.warning(&format!("Rules source not found: {src_dir}"));
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_dir);
    if !s.dry_run {
        s.ws.create_dir_all(dest_dir)?;
    }

    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !opts.filter.accepts(&name) {
            continue;
        }
        let dest_name = if opts.extension.is_empty() {
            name.clone()
        } else {
            format!(
                "{}{}",
                name.strip_suffix(".md").unwrap_or(&name),
                opts.extension
            )
        };
        valid.push(dest_name.clone());
        if s.dry_run {
            continue;
        }
        let dest_path = format!("{dest_dir}/{dest_name}");
        let bytes = read(s, &format!("{src_dir}/{name}"))?;
        let rendered = apply_rule_header(&bytes, opts.header, opts.scoped_header);
        s.ws.write(&dest_path, rendered)?;
        s.record_write(&dest_path);
    }

    let managed_suffix = if opts.extension.is_empty() {
        ".md"
    } else {
        opts.extension
    };
    let mut cleaned = 0usize;
    for name in s.ws.glob(dest_dir) {
        let path = format!("{dest_dir}/{name}");
        if !s.ws.is_file(&path) || !name.ends_with(managed_suffix) || valid.contains(&name) {
            continue;
        }
        if s.was_touched(&path) {
            continue;
        }
        if !s.may_prune(&path) {
            s.note_preserved(&format!("{dest_disp}/{name}"));
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove: {dest_disp}/{name} (obsolete)"));
        } else {
            s.ws.remove(&path)?;
            s.log.step(&format!("Removed: {dest_disp}/{name}"));
        }
        cleaned += 1;
    }

    let extra = if opts.filter.include.is_empty() {
        String::new()
    } else {
        format!(", include='{}'", opts.filter.include)
    };
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    let counts = file_ops::counts(valid.len(), cleaned);
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ {counts}{extra}{suffix}"
    ));
    Ok(())
}

/// `inline_commands_to_file`.
pub fn inline_commands_to_file(
    s: &mut Session,
    src_dir: &str,
    target_file: &str,
    filter: &Filter,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) || target_file.is_empty() {
        return Ok(());
    }
    let mut entries = Vec::new();
    for name in md_files(s, src_dir) {
        if !filter.accepts(&name) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name);
        let desc = convert::read_description(&read(s, &format!("{src_dir}/{name}"))?);
        entries.extend_from_slice(format!("- `/{stem}`").as_bytes());
        if !desc.is_empty() {
            entries.extend_from_slice(" — ".as_bytes());
            entries.extend(desc);
        }
        entries.push(b'\n');
    }
    if entries.is_empty() {
        return Ok(());
    }
    let mut block = "\n## Commands\n\nThe following commands provide quick workflows. Find them in `.ai/src/commands/`:\n\n"
        .as_bytes()
        .to_vec();
    block.extend(entries);
    s.ws.append(target_file, &block)?;
    s.record_write(target_file);
    s.log.step(&format!(
        "Appended command index to {}",
        paths::leaf(target_file)
    ));
    Ok(())
}

/// `sync_commands_as_skills`.
pub fn sync_commands_as_skills(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    filter: &Filter,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        return Ok(());
    }
    let src_disp = s.display(src_dir);
    let dest_disp = s.display(dest_dir);
    if !s.dry_run {
        s.ws.create_dir_all(dest_dir)?;
    }

    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !filter.accepts(&name) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let skill_dir = format!("{dest_dir}/command-{stem}");
        valid.push(format!("command-{stem}"));
        if s.dry_run {
            continue;
        }
        let source = read(s, &format!("{src_dir}/{name}"))?;

        s.ws.create_dir_all(&skill_dir)?;
        let skill_file = format!("{skill_dir}/SKILL.md");
        s.ws.write(&skill_file, convert::command_to_skill(&stem, &source))?;
        s.record_write(&skill_file);

        let policy = format!("{skill_dir}/agents/openai.yaml");
        if convert::read_field(&source, "disable-model-invocation") == b"true" {
            s.ws.create_dir_all(&format!("{skill_dir}/agents"))?;
            s.ws.write(
                &policy,
                b"policy:\n  allow_implicit_invocation: false\n".to_vec(),
            )?;
            s.record_write(&policy);
        } else if s.ws.is_file(&policy) && s.may_prune(&policy) {
            s.ws.remove(&policy)?;
            let agents_dir = format!("{skill_dir}/agents");
            if s.ws.list(&agents_dir).is_empty() {
                s.ws.remove(&agents_dir)?;
            }
        }
    }

    for name in s.ws.glob(dest_dir) {
        if !name.starts_with("command-") || !s.ws.is_dir(&format!("{dest_dir}/{name}")) {
            continue;
        }
        let path = format!("{dest_dir}/{name}");
        if valid.contains(&name) || s.was_touched(&path) {
            continue;
        }
        if !s.may_prune(&path) {
            s.note_preserved(&format!("{dest_disp}/{name}"));
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove obsolete generated skill: {name}"));
        } else {
            s.ws.remove(&format!("{dest_dir}/{name}"))?;
            s.log
                .step(&format!("Removed obsolete generated skill: {name}"));
        }
    }
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    s.log.step(&format!(
        "{src_disp}/*.md → {dest_disp}/command-*/SKILL.md ({} generated){suffix}",
        valid.len()
    ));
    Ok(())
}

#[derive(Clone, Copy)]
pub enum Conversion {
    CommandToml,
    AgentToml,
    AgentAmazonqJson,
    AgentOpencodeMd,
    AgentKiroMd,
}

impl Conversion {
    fn extension(self) -> &'static str {
        match self {
            Self::CommandToml | Self::AgentToml => ".toml",
            Self::AgentAmazonqJson => ".json",
            Self::AgentOpencodeMd | Self::AgentKiroMd => ".md",
        }
    }

    fn dry_run_label(self) -> &'static str {
        match self {
            Self::CommandToml => "md→toml",
            Self::AgentToml => "agent md→toml",
            Self::AgentAmazonqJson => "agent md→json",
            Self::AgentOpencodeMd => "agent md→opencode md",
            Self::AgentKiroMd => "agent md→kiro md",
        }
    }

    fn label(self, count: usize) -> String {
        let (noun, format) = match self {
            Self::CommandToml => ("command", "md→toml"),
            Self::AgentToml => ("agent", "md→toml"),
            Self::AgentAmazonqJson => ("agent", "md→amazonq json"),
            Self::AgentOpencodeMd => ("agent", "md→opencode md"),
            Self::AgentKiroMd => ("agent", "md→kiro md"),
        };
        let plural = if count == 1 { "" } else { "s" };
        format!("{count} {noun}{plural}, {format}")
    }

    fn render(self, stem: &str, source: &[u8]) -> Vec<u8> {
        match self {
            Self::CommandToml => convert::command_to_toml(source),
            Self::AgentToml => convert::agent_to_toml(stem, source),
            Self::AgentAmazonqJson => convert::agent_to_amazonq_json(stem, source),
            Self::AgentOpencodeMd => convert::agent_to_opencode_md(stem, source),
            Self::AgentKiroMd => convert::agent_to_kiro_md(stem, source),
        }
    }
}

/// `sync_commands_as_toml`, `sync_agents_as_toml`, `sync_agents_as_amazonq_json`,
/// and `sync_agents_as_opencode_md`, with `_sweep_generated` after them.
pub fn sync_converted(
    s: &mut Session,
    src_dir: &str,
    dest_dir: &str,
    conversion: Conversion,
    filter: &Filter,
) -> Result<(), Error> {
    if !s.ws.is_dir(src_dir) {
        return Ok(());
    }
    let ext = conversion.extension();
    let mut valid: Vec<String> = Vec::new();
    for name in md_files(s, src_dir) {
        if !filter.accepts(&name) {
            continue;
        }
        let stem = name.strip_suffix(".md").unwrap_or(&name).to_string();
        let dest_name = format!("{stem}{ext}");
        let dest_file = format!("{dest_dir}/{dest_name}");
        valid.push(dest_name);
        let src_file = format!("{src_dir}/{name}");
        if s.dry_run {
            let (src_disp, dest_disp) = (s.display(&src_file), s.display(&dest_file));
            s.log.step(&format!(
                "{src_disp} → {dest_disp} ({}) (dry-run)",
                conversion.dry_run_label()
            ));
            continue;
        }
        let source = read(s, &src_file)?;
        s.ws.create_dir_all(dest_dir)?;
        s.ws.write(&dest_file, conversion.render(&stem, &source))?;
        s.record_write(&dest_file);
    }

    if s.ws.is_dir(dest_dir) {
        let dest_disp = s.display(dest_dir);
        for name in s.ws.glob(dest_dir) {
            let path = format!("{dest_dir}/{name}");
            if !name.ends_with(ext) || !s.ws.is_file(&path) || valid.contains(&name) {
                continue;
            }
            if s.was_touched(&path) {
                continue;
            }
            if !s.may_prune(&path) {
                s.note_preserved(&format!("{dest_disp}/{name}"));
                continue;
            }
            if s.dry_run {
                s.log
                    .step(&format!("Would remove: {dest_disp}/{name} (obsolete)"));
            } else {
                s.ws.remove(&path)?;
                s.log.step(&format!("Removed: {dest_disp}/{name}"));
            }
        }
    }

    if !valid.is_empty() {
        let src_disp = s.display(src_dir);
        let dest_disp = s.display(dest_dir);
        s.log.step(&format!(
            "{src_disp}/ → {dest_disp}/ ({})",
            conversion.label(valid.len())
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session::test_session;
    use crate::engine::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    fn text_of(s: &Session, path: &str) -> String {
        String::from_utf8(s.ws.read(path).unwrap()).unwrap()
    }

    const CURSOR_HEADER: &str = "---\\nglobs: '**/*'\\nalwaysApply: true\\n---";
    const CURSOR_SCOPED: &str = "---\\nglobs: '{globs}'\\nalwaysApply: false\\n---";

    #[test]
    fn a_plain_rule_gets_the_expanded_header_and_a_blank_line() {
        assert_eq!(
            String::from_utf8(apply_rule_header(b"# Core\n", CURSOR_HEADER, CURSOR_SCOPED))
                .unwrap(),
            "---\nglobs: '**/*'\nalwaysApply: true\n---\n\n# Core\n"
        );
    }

    #[test]
    fn a_rule_with_frontmatter_keeps_its_keys_and_gains_missing_ones() {
        let merged = merge_or_prepend_header(b"---\nglobs: src/**\n---\n# R", CURSOR_HEADER);
        assert_eq!(
            String::from_utf8(merged).unwrap(),
            "---\nglobs: src/**\nalwaysApply: true\n---\n# R\n"
        );
    }

    // Design spec, "Known quirks", item 10: reproduced on purpose until cutover.
    #[test]
    fn a_paths_scoped_rule_takes_only_the_items_under_paths() {
        let rule = b"---\npaths:\n  - \"a/*\"\n  - b\ntags:\n  - 'c\n---\n\n\n# T\nbody";
        assert_eq!(rule_paths_csv(rule), b"a/*,b");
        assert_eq!(
            String::from_utf8(apply_rule_header(rule, CURSOR_HEADER, CURSOR_SCOPED)).unwrap(),
            "---\nglobs: 'a/*,b'\nalwaysApply: false\n---\n\n# T\nbody"
        );
        assert_eq!(rule_paths_csv(b"---\ntags:\n  - x\n---\n"), b"");
        assert_eq!(
            rule_paths_csv(b"---\ntags:\n  - x\npaths:\n- y\n---\n"),
            b"y"
        );
        assert_eq!(
            rule_paths_csv(b"---\npaths: [\"src/**\", 'docs/*.md']\n---\n"),
            b"src/**,docs/*.md"
        );
        assert_eq!(rule_paths_csv(b"---\npaths: src/**\n---\n"), b"src/**");
    }

    #[test]
    fn a_scoped_header_can_take_the_globs_as_a_quoted_flow_list() {
        let rule = b"---\npaths:\n  - \"src/{a,b}/*.ts\"\n  - it's/*\n---\n# T\n";
        let scoped = "---\\ninclusion: fileMatch\\nfileMatchPattern: {globs_list}\\n---";
        assert_eq!(
            String::from_utf8(apply_rule_header(rule, "", scoped)).unwrap(),
            "---\ninclusion: fileMatch\nfileMatchPattern: ['src/{a,b}/*.ts', 'it''s/*']\n---\n\n# T\n"
        );
    }

    #[test]
    fn claude_rules_without_headers_are_copied_verbatim() {
        let rule = b"---\npaths:\n  - x\n---\nbody\n";
        assert_eq!(apply_rule_header(rule, "", ""), rule);
    }

    #[test]
    fn sync_rules_renames_and_prunes_obsolete_managed_files() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(&mut s, "/proj/.cursor/rules/old.mdc", "x");
        file(&mut s, "/proj/.cursor/rules/notes.txt", "keep");
        s.activate_manifest([".cursor/rules/old.mdc"].map(String::from).into());
        let opts = RuleOptions {
            extension: ".mdc",
            header: CURSOR_HEADER,
            scoped_header: CURSOR_SCOPED,
            filter: &Filter::default(),
        };
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        assert!(text_of(&s, "/proj/.cursor/rules/core.mdc").starts_with("---\nglobs: '**/*'"));
        assert!(!s.ws.exists("/proj/.cursor/rules/old.mdc"));
        assert!(s.ws.exists("/proj/.cursor/rules/notes.txt"));
        assert_eq!(
            s.log.tail(1),
            ["   .ai/src/rules/ → .cursor/rules/ (1 updated, 1 removed)"]
        );
    }

    #[test]
    fn merged_rules_are_separated_and_prefixed_by_agents() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agents\n");
        file(&mut s, "/proj/.ai/src/rules/a.md", "A\n");
        file(&mut s, "/proj/.ai/src/rules/b.md", "B\n");
        merge_rules_to_file(
            &mut s,
            "/proj/.ai/src/rules",
            "/proj/.rules",
            &Filter::default(),
            Some("/proj/.ai/src/AGENTS.md"),
        )
        .unwrap();
        assert_eq!(
            text_of(&s, "/proj/.rules"),
            "# Agents\n\n---\n\nA\n\n---\n\nB\n"
        );
    }

    #[test]
    fn imports_list_every_markdown_rule_in_the_dest() {
        let mut s = test_session();
        file(&mut s, "/proj/CLAUDE.md", "# A\n");
        file(&mut s, "/proj/.claude/rules/core.md", "");
        file(&mut s, "/proj/.claude/rules/git.md", "");
        append_imports(&mut s, "/proj/CLAUDE.md", "/proj/.claude/rules").unwrap();
        assert_eq!(
            text_of(&s, "/proj/CLAUDE.md"),
            "# A\n\n<!-- Auto-generated imports -->\n@rules/core.md\n@rules/git.md\n"
        );
    }

    #[test]
    fn the_command_index_uses_descriptions_when_present() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: Review\n---\n",
        );
        file(&mut s, "/proj/.ai/src/commands/ship.md", "Ship\n");
        file(&mut s, "/proj/AGENTS.md", "# A\n");
        inline_commands_to_file(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/AGENTS.md",
            &Filter::default(),
        )
        .unwrap();
        assert_eq!(
            text_of(&s, "/proj/AGENTS.md"),
            "# A\n\n## Commands\n\nThe following commands provide quick workflows. Find them in `.ai/src/commands/`:\n\n- `/review` — Review\n- `/ship`\n"
        );
    }

    #[test]
    fn command_skills_carry_the_codex_opt_out_and_drop_stale_dirs() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/commands/only.md",
            "---\ndescription: D\ndisable-model-invocation: true\n---\nBody\n",
        );
        file(&mut s, "/proj/.agents/skills/command-gone/SKILL.md", "x");
        file(&mut s, "/proj/.agents/skills/command-kept/SKILL.md", "x");
        file(&mut s, "/proj/.agents/skills/mine/SKILL.md", "x");
        s.activate_manifest(
            [".agents/skills/command-gone/SKILL.md"]
                .map(String::from)
                .into(),
        );
        sync_commands_as_skills(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.agents/skills",
            &Filter::default(),
        )
        .unwrap();
        assert_eq!(
            text_of(&s, "/proj/.agents/skills/command-only/agents/openai.yaml"),
            "policy:\n  allow_implicit_invocation: false\n"
        );
        assert!(!s.ws.exists("/proj/.agents/skills/command-gone"));
        assert!(s.ws.exists("/proj/.agents/skills/command-kept"));
        assert!(s.ws.exists("/proj/.agents/skills/mine"));
    }

    fn session_with_kept_and_obsolete_outputs() -> Session {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/rules/core.md", "# Core\n");
        file(
            &mut s,
            "/proj/.ai/src/agents/rev.md",
            "---\nname: rev\n---\nBody\n",
        );
        file(
            &mut s,
            "/proj/.ai/src/commands/review.md",
            "---\ndescription: R\n---\nBody\n",
        );
        file(&mut s, "/proj/.cursor/rules/old.mdc", "old");
        file(&mut s, "/proj/.cursor/rules/mine.mdc", "mine");
        file(&mut s, "/proj/.codex/agents/old.toml", "old");
        file(&mut s, "/proj/.codex/agents/mine.toml", "mine");
        file(&mut s, "/proj/.agents/skills/command-gone/SKILL.md", "x");
        s.activate_manifest(
            [
                ".cursor/rules/old.mdc",
                ".codex/agents/old.toml",
                ".agents/skills/command-gone/SKILL.md",
            ]
            .map(String::from)
            .into(),
        );
        s
    }

    #[test]
    fn dry_runs_and_kept_files_log_what_the_bash_helpers_log() {
        let mut s = session_with_kept_and_obsolete_outputs();
        let opts = RuleOptions {
            extension: ".mdc",
            header: "",
            scoped_header: "",
            filter: &Filter::default(),
        };

        s.dry_run = true;
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
            &Filter::default(),
        )
        .unwrap();
        sync_commands_as_skills(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.agents/skills",
            &Filter::default(),
        )
        .unwrap();
        merge_rules_to_file(
            &mut s,
            "/proj/.ai/src/rules",
            "/proj/.rules",
            &Filter::default(),
            Some("/proj/.ai/src/rules/core.md"),
        )
        .unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/commands",
            "/proj/.gemini/commands",
            Conversion::CommandToml,
            &Filter::default(),
        )
        .unwrap();
        assert!(s.ws.exists("/proj/.cursor/rules/old.mdc"));
        assert!(!s.ws.exists("/proj/.rules"));

        s.dry_run = false;
        sync_rules(&mut s, "/proj/.ai/src/rules", "/proj/.cursor/rules", &opts).unwrap();
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
            &Filter::default(),
        )
        .unwrap();
        assert_eq!(s.ws.list("/proj/.cursor/rules"), ["core.mdc", "mine.mdc"]);
        assert_eq!(s.ws.list("/proj/.codex/agents"), ["mine.toml", "rev.toml"]);
        assert_eq!(s.preserved(), 4);

        let lines: Vec<&str> = s.log.lines().iter().map(|(_, l)| l.as_str()).collect();
        assert_eq!(
            lines,
            [
                "[WARNING] Would keep .cursor/rules/mine.mdc (not from .ai/src/; --force to prune)",
                "   Would remove: .cursor/rules/old.mdc (obsolete)",
                "   .ai/src/rules/ → .cursor/rules/ (1 updated, 1 removed) (dry-run)",
                "   .ai/src/agents/rev.md → .codex/agents/rev.toml (agent md→toml) (dry-run)",
                "[WARNING] Would keep .codex/agents/mine.toml (not from .ai/src/; --force to prune)",
                "   Would remove: .codex/agents/old.toml (obsolete)",
                "   .ai/src/agents/ → .codex/agents/ (1 agent, md→toml)",
                "   Would remove obsolete generated skill: command-gone",
                "   .ai/src/commands/*.md → .agents/skills/command-*/SKILL.md (1 generated) (dry-run)",
                "   .ai/src/rules/ → .rules (1 file merged +agents) (dry-run)",
                "   .ai/src/commands/review.md → .gemini/commands/review.toml (md→toml) (dry-run)",
                "   .ai/src/commands/ → .gemini/commands/ (1 command, md→toml)",
                "[WARNING] Kept .cursor/rules/mine.mdc (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   Removed: .cursor/rules/old.mdc",
                "   .ai/src/rules/ → .cursor/rules/ (1 updated, 1 removed)",
                "[WARNING] Kept .codex/agents/mine.toml (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   Removed: .codex/agents/old.toml",
                "   .ai/src/agents/ → .codex/agents/ (1 agent, md→toml)",
            ]
        );
    }

    #[test]
    fn converted_directories_sweep_their_own_extension_only() {
        let mut s = test_session();
        file(
            &mut s,
            "/proj/.ai/src/agents/rev.md",
            "---\nname: rev\n---\nBody\n",
        );
        file(&mut s, "/proj/.codex/agents/old.toml", "x");
        file(&mut s, "/proj/.codex/agents/keep.json", "x");
        s.activate_manifest([".codex/agents/old.toml"].map(String::from).into());
        sync_converted(
            &mut s,
            "/proj/.ai/src/agents",
            "/proj/.codex/agents",
            Conversion::AgentToml,
            &Filter::default(),
        )
        .unwrap();
        assert!(s.ws.is_file("/proj/.codex/agents/rev.toml"));
        assert!(!s.ws.exists("/proj/.codex/agents/old.toml"));
        assert!(s.ws.exists("/proj/.codex/agents/keep.json"));
        assert_eq!(
            s.log.tail(1),
            ["   .ai/src/agents/ → .codex/agents/ (1 agent, md→toml)"]
        );
    }

    #[test]
    fn merge_or_prepend_header_keeps_the_source_value_and_a_complete_header() {
        let wins = merge_or_prepend_header(b"---\nglobs: \"**/*.ts\"\n---\nbody\n", CURSOR_HEADER);
        assert_eq!(
            String::from_utf8(wins).unwrap(),
            "---\nglobs: \"**/*.ts\"\nalwaysApply: true\n---\nbody\n"
        );
        let complete = b"---\nglobs: \"**/*.ts\"\nalwaysApply: false\n---\nbody\n";
        assert_eq!(merge_or_prepend_header(complete, CURSOR_HEADER), complete);
    }

    #[test]
    fn an_empty_rules_dir_merges_nothing_but_still_prepends_agents() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "# Agent\n");
        file(&mut s, "/proj/.ai/src/rules/.keep", "");
        merge_rules_to_file(
            &mut s,
            "/proj/.ai/src/rules",
            "/proj/merged.md",
            &Filter::default(),
            Some("/proj/.ai/src/AGENTS.md"),
        )
        .unwrap();
        assert_eq!(text_of(&s, "/proj/merged.md"), "# Agent\n\n---\n\n");
        assert_eq!(
            s.log.tail(1),
            ["   .ai/src/rules/ → merged.md (0 files merged)"]
        );
    }

    #[test]
    fn missing_rule_sources_warn_and_return() {
        let mut s = test_session();
        let opts = RuleOptions {
            extension: "",
            header: "",
            scoped_header: "",
            filter: &Filter::default(),
        };
        sync_rules(&mut s, "/proj/nope", "/proj/out", &opts).unwrap();
        merge_rules_to_file(
            &mut s,
            "/proj/nope",
            "/proj/out.md",
            &Filter::default(),
            None,
        )
        .unwrap();
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] Rules source not found: /proj/nope",
                "[WARNING] Rules source not found: /proj/nope"
            ]
        );
        assert!(!s.ws.exists("/proj/out"));
        assert!(!s.ws.exists("/proj/out.md"));
    }
}
