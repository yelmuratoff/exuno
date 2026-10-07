//! Line-oriented edits of `lib/helpers/yaml_edit.sh`: comments stay, every
//! rewritten line ends in a newline as `while read` prints it, and files are
//! replaced through a staging file beside them.

use std::path::Path;

use crate::{Error, config::yaml_subset, engine::staging};

fn is_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
}

/// `while IFS= read -r line || [[ -n "$line" ]]`.
fn lines(text: &str) -> Vec<&str> {
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    lines
}

fn split_indent(line: &str) -> (usize, &str) {
    let stripped = line.trim_start_matches(is_space);
    (line.len() - stripped.len(), stripped)
}

fn is_comment(line: &str) -> bool {
    line.trim_start_matches(is_space).starts_with('#')
}

/// `^([a-zA-Z0-9_-]+):`.
fn key_of(stripped: &str) -> Option<&str> {
    let end = stripped
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
        .unwrap_or(stripped.len());
    (end > 0 && stripped[end..].starts_with(':')).then(|| &stripped[..end])
}

/// `_yaml_find_key_line`.
pub fn find_key_line(text: &str, key_path: &str) -> Option<(usize, usize)> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut looking = 0;
    let mut in_section = false;
    let mut section_indent = 0;
    let mut child_indent: Option<usize> = None;
    for (index, line) in lines(text).into_iter().enumerate() {
        if line.is_empty() || is_comment(line) {
            continue;
        }
        let (indent, stripped) = split_indent(line);
        let Some(key) = key_of(stripped) else {
            continue;
        };
        if in_section && indent <= section_indent {
            return None;
        }
        let at_level = if in_section {
            indent == *child_indent.get_or_insert(indent)
        } else {
            indent == 0
        };
        if at_level && key == keys[looking] {
            if looking + 1 == keys.len() {
                return Some((index + 1, indent));
            }
            in_section = true;
            section_indent = indent;
            child_indent = None;
            looking += 1;
        }
    }
    None
}

/// `yaml_set_scalar` on text; `None` is a missing file.
pub fn set_scalar_text(text: Option<&str>, key: &str, value: &str) -> String {
    let replacement = format!("{key}: {value}\n");
    let Some(text) = text else {
        return replacement;
    };
    let mut out = String::new();
    let mut found = false;
    for line in lines(text) {
        let matches = line == format!("{key}:")
            || line
                .strip_prefix(&format!("{key}:"))
                .is_some_and(|rest| rest.starts_with(is_space));
        if !found && matches {
            out.push_str(&replacement);
            found = true;
        } else {
            out.push_str(line);
            out.push('\n');
        }
    }
    if !found {
        out.push_str(&replacement);
    }
    out
}

/// `text` with the missing tail of `key_path` and a one-item list added right
/// under the deepest ancestor that exists as a section; `None` when none does.
fn insert_under_ancestor(text: &str, key_path: &str, value: &str) -> Option<String> {
    let segments: Vec<&str> = key_path.split('.').collect();
    let all = lines(text);
    let (depth, lineno, indent) = (1..segments.len()).rev().find_map(|depth| {
        let (lineno, indent) = find_key_line(text, &segments[..depth].join("."))?;
        let (_, rest) = split_indent(all[lineno - 1]).1.split_once(':')?;
        let rest = rest.trim_matches(is_space);
        (rest.is_empty() || rest.starts_with('#')).then_some((depth, lineno, indent))
    })?;
    let mut out = String::new();
    for line in &all[..lineno] {
        out.push_str(line);
        out.push('\n');
    }
    for (step, segment) in segments[depth..].iter().enumerate() {
        out.push_str(&format!(
            "{}{segment}:\n",
            " ".repeat(indent + 2 * (step + 1))
        ));
    }
    let item_indent = indent + 2 * (segments.len() - depth + 1);
    out.push_str(&format!("{}- {value}\n", " ".repeat(item_indent)));
    for line in &all[lineno..] {
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// `yaml_list_append` on text.
pub fn list_append_text(text: Option<&str>, key_path: &str, value: &str) -> Option<String> {
    if yaml_subset::list(text.unwrap_or(""), key_path)
        .iter()
        .any(|item| item == value)
    {
        return None;
    }
    let found = text.and_then(|text| find_key_line(text, key_path));
    if found.is_none()
        && let Some(inserted) = text.and_then(|text| insert_under_ancestor(text, key_path, value))
    {
        return Some(inserted);
    }
    let (Some(text), Some((key_lineno, key_indent))) = (text, found) else {
        let mut out = String::new();
        let existing = text.unwrap_or("").trim_end_matches('\n');
        if !existing.is_empty() {
            out.push_str(existing);
            out.push_str("\n\n");
        }
        let segments: Vec<&str> = key_path.split('.').collect();
        for (depth, segment) in segments.iter().enumerate() {
            out.push_str(&format!("{}{segment}:\n", " ".repeat(2 * depth)));
        }
        out.push_str(&format!("{}- {value}\n", " ".repeat(2 * segments.len())));
        return Some(out);
    };

    let all = lines(text);
    let item_indent = key_indent + 2;
    let leaf = key_path.rsplit('.').next().unwrap_or(key_path);
    let has_inline = all[key_lineno - 1]
        .trim_start_matches(is_space)
        .strip_prefix(&format!("{leaf}:"))
        .is_some_and(|rest| rest.starts_with(is_space) && rest.chars().count() >= 2);
    let mut last_item = key_lineno;
    for (index, line) in all.iter().enumerate().skip(key_lineno) {
        if line.is_empty() || is_comment(line) {
            continue;
        }
        let (indent, stripped) = split_indent(line);
        if indent < item_indent {
            break;
        }
        if stripped.starts_with('-') {
            last_item = index + 1;
        }
    }
    let mut out = String::new();
    for (index, line) in all.iter().enumerate() {
        let lineno = index + 1;
        if lineno == key_lineno && has_inline {
            out.push_str(&format!("{}{leaf}:\n", " ".repeat(key_indent)));
        } else {
            out.push_str(line);
            out.push('\n');
        }
        if lineno == last_item {
            out.push_str(&format!("{}- {value}\n", " ".repeat(item_indent)));
        }
    }
    Some(out)
}

/// `^([[:space:]]*<leaf>:[[:space:]]*)\[(.*)\][[:space:]]*$`: the prefix up to
/// the bracket and the list body.
fn inline_list<'a>(line: &'a str, leaf: &str) -> Option<(&'a str, &'a str)> {
    let (_, stripped) = split_indent(line);
    let rest = stripped
        .strip_prefix(leaf)?
        .strip_prefix(':')?
        .trim_start_matches(is_space);
    let body = rest
        .strip_prefix('[')?
        .trim_end_matches(is_space)
        .strip_suffix(']')?;
    Some((&line[..line.len() - rest.len()], body))
}

/// `_yaml_inline_list_without`.
fn inline_without(body: &str, value: &str) -> String {
    body.split(',')
        .filter(|item| {
            let normalized = yaml_subset::normalize_scalar(item);
            !normalized.is_empty() && normalized != value
        })
        .map(|item| item.trim_matches(is_space))
        .collect::<Vec<_>>()
        .join(", ")
}

/// `yaml_list_remove` on text.
pub fn list_remove_text(text: &str, key_path: &str, value: &str) -> Option<String> {
    let (key_lineno, key_indent) = find_key_line(text, key_path)?;
    let item_indent = key_indent + 2;
    let leaf = key_path.rsplit('.').next().unwrap_or(key_path);
    let mut out = String::new();
    let mut in_list = true;
    for (index, line) in lines(text).into_iter().enumerate() {
        let lineno = index + 1;
        if lineno < key_lineno || !in_list {
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if lineno == key_lineno {
            match inline_list(line, leaf) {
                Some((prefix, body)) => {
                    out.push_str(&format!("{prefix}[{}]\n", inline_without(body, value)));
                    in_list = false;
                }
                None => {
                    out.push_str(line);
                    out.push('\n');
                }
            }
            continue;
        }
        let (indent, stripped) = split_indent(line);
        if !stripped.is_empty() && !stripped.starts_with('#') && indent < item_indent {
            in_list = false;
            out.push_str(line);
            out.push('\n');
            continue;
        }
        if let Some(item) = stripped.strip_prefix('-')
            && yaml_subset::normalize_scalar(item.trim_start_matches(is_space)) == value
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// `yaml_remove_key` on text: the key line, then every blank line and every
/// line indented deeper than the key until the first one that is not.
pub fn remove_key_text(text: &str, key_path: &str) -> Option<String> {
    let (key_lineno, key_indent) = find_key_line(text, key_path)?;
    let mut out = String::new();
    let mut skipping = false;
    let mut blanks = 0;
    let mut sibling_before = false;
    for (index, line) in lines(text).into_iter().enumerate() {
        if index + 1 == key_lineno {
            skipping = true;
            continue;
        }
        let (indent, stripped) = split_indent(line);
        if skipping {
            if stripped.is_empty() {
                blanks += 1;
                continue;
            }
            if indent > key_indent {
                blanks = 0;
                continue;
            }
            skipping = false;
            if sibling_before {
                out.push_str(&"\n".repeat(blanks));
            }
        } else if index + 1 < key_lineno {
            sibling_before = !stripped.is_empty() && indent >= key_indent;
        }
        out.push_str(line);
        out.push('\n');
    }
    Some(out)
}

/// `yaml_remove_key`.
pub fn remove_key(file: &Path, key_path: &str) -> Result<(), Error> {
    let Some(text) = read_existing(file)? else {
        return Ok(());
    };
    match remove_key_text(&text, key_path) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}

fn read_existing(file: &Path) -> Result<Option<String>, Error> {
    if !file.is_file() {
        return Ok(None);
    }
    let bytes = std::fs::read(file).map_err(|e| Error::io(file, e))?;
    Ok(Some(String::from_utf8_lossy(&bytes).into_owned()))
}

fn ensure_parent(file: &Path) -> Result<(), Error> {
    match file.parent() {
        Some(dir) if !dir.as_os_str().is_empty() && !dir.is_dir() => {
            std::fs::create_dir_all(dir).map_err(|e| Error::io(dir, e))
        }
        _ => Ok(()),
    }
}

/// `yaml_set_scalar`.
pub fn set_scalar(file: &Path, key: &str, value: &str) -> Result<(), Error> {
    ensure_parent(file)?;
    let text = read_existing(file)?;
    staging::write_beside(
        file,
        set_scalar_text(text.as_deref(), key, value).as_bytes(),
    )
}

/// `yaml_list_append`.
pub fn list_append(file: &Path, key_path: &str, value: &str) -> Result<(), Error> {
    ensure_parent(file)?;
    let text = read_existing(file)?;
    match list_append_text(text.as_deref(), key_path, value) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}

/// `yaml_list_remove`.
pub fn list_remove(file: &Path, key_path: &str, value: &str) -> Result<(), Error> {
    let Some(text) = read_existing(file)? else {
        return Ok(());
    };
    match list_remove_text(&text, key_path, value) {
        Some(updated) => staging::write_beside(file, updated.as_bytes()),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn append_follows_the_last_item_or_builds_the_path_like_yaml_list_append() {
        let cases: [(&str, Option<&str>); 8] = [
            (
                "# Exuno — Project Configuration\ntools:\n  enabled: []\n",
                Some("# Exuno — Project Configuration\ntools:\n  enabled:\n    - claude\n"),
            ),
            (
                "# c\ntools:\n  enabled:\n    - zed\n    # note\n    - cursor\n\nsource:\n  rules: x\n",
                Some(
                    "# c\ntools:\n  enabled:\n    - zed\n    # note\n    - cursor\n    - claude\n\nsource:\n  rules: x\n",
                ),
            ),
            ("tools:\n  enabled: [claude, cursor]", None),
            (
                "format: 2\ntools:\n  other: x\n\n\n",
                Some("format: 2\ntools:\n  enabled:\n    - claude\n  other: x\n\n\n"),
            ),
            ("", Some("tools:\n  enabled:\n    - claude\n")),
            (
                "tools:\n  enabled:\n    - zed",
                Some("tools:\n  enabled:\n    - zed\n    - claude\n"),
            ),
            (
                "tools:\n  foo:\n    enabled: x\n",
                Some("tools:\n  enabled:\n    - claude\n  foo:\n    enabled: x\n"),
            ),
            (
                "tools: x\n",
                Some("tools: x\n\ntools:\n  enabled:\n    - claude\n"),
            ),
        ];
        for (text, expected) in cases {
            assert_eq!(
                list_append_text(Some(text), "tools.enabled", "claude").as_deref(),
                expected,
                "{text:?}"
            );
        }
        assert_eq!(
            list_append_text(None, "tools.enabled", "claude").as_deref(),
            Some("tools:\n  enabled:\n    - claude\n")
        );
    }

    #[test]
    fn remove_stays_in_its_list_and_rewrites_an_inline_one() {
        let cases: [(&str, &str, &str); 6] = [
            (
                "tools:\n  enabled:\n    - claude\n    - \"cursor\" # q\nprofiles:\n  hub:\n    tools:\n      - claude\n",
                "claude",
                "tools:\n  enabled:\n    - \"cursor\" # q\nprofiles:\n  hub:\n    tools:\n      - claude\n",
            ),
            (
                "tools:\n  enabled: [claude]\n",
                "claude",
                "tools:\n  enabled: []\n",
            ),
            (
                "tools:\n  enabled: [claude, \"cursor\" , zed]",
                "cursor",
                "tools:\n  enabled: [claude, zed]\n",
            ),
            (
                "tools:\n  enabled:\n    - \"cursor\" # q\n  \n",
                "cursor",
                "tools:\n  enabled:\n  \n",
            ),
            (
                "tools:\n  enabled:\n# c\n    - claude\n  other: [claude]\n",
                "claude",
                "tools:\n  enabled:\n# c\n  other: [claude]\n",
            ),
            (
                "tools:\n  enabled: [*.md, claude]\n",
                "claude",
                "tools:\n  enabled: [*.md]\n",
            ),
        ];
        for (text, value, expected) in cases {
            assert_eq!(
                list_remove_text(text, "tools.enabled", value).as_deref(),
                Some(expected),
                "{text:?}"
            );
        }
        assert_eq!(
            list_remove_text("format: 2\n", "tools.enabled", "claude"),
            None
        );
    }

    #[test]
    fn set_scalar_replaces_the_first_root_key_or_appends_it() {
        assert_eq!(
            set_scalar_text(
                Some("name: X\nenabled: true\nenabled: true\n"),
                "enabled",
                "false"
            ),
            "name: X\nenabled: false\nenabled: true\n"
        );
        assert_eq!(
            set_scalar_text(Some("name: X"), "enabled", "false"),
            "name: X\nenabled: false\n"
        );
        assert_eq!(
            set_scalar_text(Some("enabled:\n  x: 1\n"), "enabled", "false"),
            "enabled: false\n  x: 1\n"
        );
        assert_eq!(
            set_scalar_text(None, "enabled", "false"),
            "enabled: false\n"
        );
    }

    #[test]
    fn a_key_is_found_only_inside_its_parent_block() {
        let text = "# c\ntools:\n  enabled:\n    - a\nsource:\n  enabled: x\n";
        assert_eq!(find_key_line(text, "tools.enabled"), Some((3, 2)));
        assert_eq!(find_key_line(text, "source.enabled"), Some((6, 2)));
        assert_eq!(find_key_line("tools:\nenabled: x\n", "tools.enabled"), None);
        assert_eq!(find_key_line("  tools:\n", "tools"), None);
    }

    #[cfg(unix)]
    #[test]
    fn file_edits_write_beside_and_skip_a_listed_value() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(".ai/agent_sync.yaml");
        list_append(&file, "tools.enabled", "claude").unwrap();
        list_append(&file, "tools.enabled", "claude").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "tools:\n  enabled:\n    - claude\n"
        );
        list_remove(&file, "tools.enabled", "claude").unwrap();
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            "tools:\n  enabled:\n"
        );
        list_remove(&dir.path().join("missing.yaml"), "tools.enabled", "claude").unwrap();
        assert!(!dir.path().join("missing.yaml").exists());
    }

    #[test]
    fn remove_key_drops_the_block_and_keeps_one_separator_after_a_sibling() {
        let cases: [(&str, &str, &str); 6] = [
            (
                "a: 1\nprofiles:\n  x: 1\n\nb: 2\n",
                "profiles",
                "a: 1\n\nb: 2\n",
            ),
            (
                "a: 1\n\nprofiles:\n  x: 1\n\nb: 2\n",
                "profiles",
                "a: 1\n\nb: 2\n",
            ),
            (
                "name: \"X\"\nenabled: true\n\ntargets:\n  rules:\n    dest: \".r\"\n    # note\n\n    extension: \".md\"\n  skills:\n    dest: \".s\"\n",
                "targets.rules.dest",
                "name: \"X\"\nenabled: true\n\ntargets:\n  rules:\n    # note\n\n    extension: \".md\"\n  skills:\n    dest: \".s\"\n",
            ),
            (
                "name: \"X\"\ntargets:\n  rules:\n    dest: \".r\"\n\n  # c\n  skills:\n    dest: \".s\"\n",
                "targets.rules",
                "name: \"X\"\ntargets:\n  # c\n  skills:\n    dest: \".s\"\n",
            ),
            ("name: \"X\"\n\nenabled: true", "name", "enabled: true\n"),
            (
                "post_sync:\n  - a\n  - b\n# tail\nx: 1",
                "post_sync",
                "# tail\nx: 1\n",
            ),
        ];
        for (text, key, expected) in cases {
            assert_eq!(
                remove_key_text(text, key).as_deref(),
                Some(expected),
                "{key}"
            );
        }
        assert_eq!(remove_key_text("a: 1\n", "missing.key"), None);
    }
}
