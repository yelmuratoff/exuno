use crate::config::{names, yaml_subset};

#[derive(Debug)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub use_when: Option<String>,
    pub not_for: Option<String>,
    pub requirements: Option<String>,
}

/// The lines between a leading `---` and the closing one.
fn frontmatter(text: &str) -> Result<Vec<&str>, String> {
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err("missing YAML frontmatter".to_string());
    }
    let mut frontmatter = Vec::new();
    for line in lines {
        if line.trim() == "---" {
            return Ok(frontmatter);
        }
        frontmatter.push(line);
    }
    Err("unclosed YAML frontmatter".to_string())
}

/// The `description` a `SKILL.md` declares, block strings folded to one line;
/// `None` when it declares none or cannot be read. Checks nothing else.
pub fn description(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    field(&frontmatter(text).ok()?, "description")
        .ok()
        .flatten()
        .filter(|description| !description.is_empty())
}

/// The `name` a `SKILL.md` declares; `None` when it declares none or cannot
/// be read. Checks nothing else.
pub fn name(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    field(&frontmatter(text).ok()?, "name")
        .ok()
        .flatten()
        .filter(|name| !name.is_empty())
}

pub fn read(bytes: &[u8], directory: &str) -> Result<SkillMetadata, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "SKILL.md is not UTF-8".to_string())?;
    let frontmatter = frontmatter(text)?;
    for key in ["name", "description", "compatibility"] {
        if frontmatter
            .iter()
            .filter(|line| line.starts_with(&format!("{key}:")))
            .count()
            > 1
        {
            return Err(format!("duplicate {key}"));
        }
    }
    let name = field(&frontmatter, "name")?.filter(|value| !value.is_empty());
    let Some(name) = name else {
        return Err("missing name".to_string());
    };
    if name != directory {
        return Err(format!(
            "name '{name}' does not match directory '{directory}'"
        ));
    }
    if !valid_name(&name) {
        return Err("name must be 1–64 lowercase letters, digits, or single hyphens".to_string());
    }
    let description = field(&frontmatter, "description")?.unwrap_or_default();
    if description.trim().is_empty() || description.chars().count() > 1024 {
        return Err("description must be 1–1024 characters".to_string());
    }
    let compatibility = field(&frontmatter, "compatibility")?;
    if compatibility
        .as_ref()
        .is_some_and(|value| value.is_empty() || value.chars().count() > 500)
    {
        return Err("compatibility must be 1–500 characters when provided".to_string());
    }
    Ok(SkillMetadata {
        name,
        description,
        license: field(&frontmatter, "license")?.filter(|value| !value.is_empty()),
        compatibility,
        use_when: annotation(&frontmatter, "use-when")?.filter(|value| !value.is_empty()),
        not_for: annotation(&frontmatter, "not-for")?.filter(|value| !value.is_empty()),
        requirements: annotation(&frontmatter, "requirements")?.filter(|value| !value.is_empty()),
    })
}

fn annotation(frontmatter: &[&str], name: &str) -> Result<Option<String>, String> {
    for prefix in names::SKILL_METADATA_PREFIXES {
        if let Some(value) = field(frontmatter, &format!("{prefix}{name}"))? {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
        && !name.contains("--")
}

fn field(frontmatter: &[&str], key: &str) -> Result<Option<String>, String> {
    let Some((index, field_indent, source)) = field_line(frontmatter, key) else {
        return Ok(None);
    };
    let raw = scalar(source);
    let source = source.trim_start();
    let numeric = raw.chars().next().is_some_and(|character| {
        character.is_ascii_digit() || matches!(character, '+' | '-' | '.')
    }) && raw.parse::<f64>().is_ok();
    if !source.starts_with(['"', '\''])
        && (source.starts_with(['[', '{', '*', '&', '!'])
            || matches!(
                raw.as_str(),
                "~" | "null"
                    | "Null"
                    | "NULL"
                    | "true"
                    | "True"
                    | "TRUE"
                    | "false"
                    | "False"
                    | "FALSE"
            )
            || numeric)
    {
        return Err(format!("{key} must be a string"));
    }
    if source.starts_with(['>', '|'])
        && matches!(raw.as_str(), ">" | ">-" | ">+" | "|" | "|-" | "|+")
    {
        let value = frontmatter[index + 1..]
            .iter()
            .take_while(|line| {
                line.is_empty() || line.len() - line.trim_start().len() > field_indent
            })
            .map(|line| line.trim())
            .collect::<Vec<_>>();
        return Ok(Some(value.join(" ").trim().to_string()));
    }
    if source.starts_with(['>', '|']) && raw.starts_with(['>', '|']) {
        return Err(format!("unsupported {key} block style"));
    }
    if let Some(quote) = source
        .chars()
        .next()
        .filter(|quote| matches!(quote, '"' | '\''))
    {
        let closed = source[1..].rmatch_indices(quote).any(|(index, _)| {
            let tail = source[index + 2..].trim_start();
            tail.is_empty() || tail.starts_with('#')
        });
        if !closed {
            return Err(format!("unclosed {key} quote"));
        }
    }
    Ok(Some(raw))
}

fn scalar(source: &str) -> String {
    let source = source.trim();
    if source.starts_with(['"', '\'']) {
        return yaml_subset::normalize_scalar(source);
    }
    let comment = source.char_indices().find_map(|(index, character)| {
        (character == '#' && (index == 0 || source[..index].ends_with(char::is_whitespace)))
            .then_some(index)
    });
    source[..comment.unwrap_or(source.len())]
        .trim_end()
        .to_string()
}

fn field_line<'a>(frontmatter: &[&'a str], key: &str) -> Option<(usize, usize, &'a str)> {
    let (parent, leaf) = key
        .split_once('.')
        .map_or((None, key), |(parent, leaf)| (Some(parent), leaf));
    let mut in_parent = parent.is_none();
    for (index, line) in frontmatter.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let Some((candidate, source)) = line.trim_start().split_once(':') else {
            continue;
        };
        if indent == 0 {
            if parent == Some(candidate) {
                in_parent = true;
                continue;
            }
            if parent.is_some() && in_parent {
                break;
            }
        }
        if in_parent && (parent.is_some() || indent == 0) && candidate == leaf {
            return Some((index, indent, source));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_standard_fields_and_annotations() {
        let skill = b"---\nname: review\ndescription: >-\n  Review a diff\n  before merging\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  agentsync-use-when: Changes need review\n  agentsync-not-for: Writing code\n  agentsync-requirements: A selected diff\n---\n# Review\n";
        let card = read(skill, "review").unwrap();
        assert_eq!(card.name, "review");
        assert_eq!(card.description, "Review a diff before merging");
        assert_eq!(card.license.as_deref(), Some("MIT"));
        assert_eq!(card.compatibility.as_deref(), Some("Requires git"));
        assert_eq!(card.use_when.as_deref(), Some("Changes need review"));
        assert_eq!(card.not_for.as_deref(), Some("Writing code"));
        assert_eq!(card.requirements.as_deref(), Some("A selected diff"));
    }

    #[test]
    fn name_reads_the_declared_name_alone() {
        assert_eq!(
            name(b"---\nname: \"jury\"\n---\n# Jury\n").as_deref(),
            Some("jury")
        );
        assert_eq!(name(b"---\ndescription: x\n---\n"), None);
        assert_eq!(name(b"# No frontmatter\n"), None);
    }

    #[test]
    fn exuno_annotations_win_over_the_legacy_ones() {
        let skill = b"---\nname: review\ndescription: Review a diff\nmetadata:\n  agentsync-use-when: Old\n  exuno-use-when: New\n  agentsync-not-for: Writing code\n---\n";
        let card = read(skill, "review").unwrap();
        assert_eq!(card.use_when.as_deref(), Some("New"));
        assert_eq!(card.not_for.as_deref(), Some("Writing code"));
        assert_eq!(card.requirements, None);
    }

    #[test]
    fn rejects_consecutive_hyphens_and_oversize_compatibility() {
        assert!(
            read(
                b"---\nname: bad--name\ndescription: Use it\n---\n",
                "bad--name"
            )
            .unwrap_err()
            .contains("single hyphens")
        );
        let skill = format!(
            "---\nname: review\ndescription: Review\ncompatibility: {}\n---\n",
            "x".repeat(501)
        );
        assert!(
            read(skill.as_bytes(), "review")
                .unwrap_err()
                .contains("compatibility must")
        );
    }

    #[test]
    fn rejects_duplicate_fields_and_unclosed_quotes() {
        let duplicate = b"---\nname: review\nname: review\ndescription: Review\n---\n";
        assert_eq!(read(duplicate, "review").unwrap_err(), "duplicate name");
        let unclosed = b"---\nname: review\ndescription: \"Review\n---\n";
        assert_eq!(
            read(unclosed, "review").unwrap_err(),
            "unclosed description quote"
        );
    }

    #[test]
    fn block_description_uses_the_root_field() {
        let skill =
            b"---\nname: review\nmetadata:\n  description: ignored\ndescription: >-\n  Review this diff\n---\n";
        assert_eq!(
            read(skill, "review").unwrap().description,
            "Review this diff"
        );
    }

    #[test]
    fn quoted_block_marker_is_a_scalar() {
        let skill = b"---\nname: review\ndescription: \">\"\n---\n";
        assert_eq!(read(skill, "review").unwrap().description, ">");
        let skill = b"---\nname: review\ndescription: \"Review\" # context\n---\n";
        assert_eq!(read(skill, "review").unwrap().description, "Review");
    }

    #[test]
    fn plain_scalar_keeps_hash_without_comment_spacing() {
        let skill = b"---\nname: review\ndescription: Use C# tools # note\n---\n";
        assert_eq!(read(skill, "review").unwrap().description, "Use C# tools");
    }

    #[test]
    fn rejects_non_string_required_fields() {
        for description in ["[one, two]", "{what: review}", "true", "42", "null"] {
            let skill = format!("---\nname: review\ndescription: {description}\n---\n");
            assert_eq!(
                read(skill.as_bytes(), "review").unwrap_err(),
                "description must be a string"
            );
        }
        let skill = b"---\nname: review\ndescription: inf\n---\n";
        assert_eq!(read(skill, "review").unwrap().description, "inf");
    }
}
