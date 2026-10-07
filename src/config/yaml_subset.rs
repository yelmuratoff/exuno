//! Reader for the YAML subset Exuno configs are written in.
//!
//! This mirrors `lib/helpers/yaml.sh` rather than parsing YAML: every shipped
//! and user config was written against that reader's rules (first duplicate
//! key wins, `#` ends an unquoted value, `\n` stays literal inside quotes),
//! and the migration promises byte-identical outputs.

fn strip_indent(line: &str) -> (usize, &str) {
    let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
    (line.len() - stripped.len(), stripped)
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '-'
}

/// `(key, rest)` when the stripped line starts with a bare key and a colon.
fn split_key(stripped: &str) -> Option<(&str, &str)> {
    let (key, rest) = stripped.split_once(':')?;
    if key.is_empty() || !key.chars().all(is_key_char) {
        return None;
    }
    Some((
        key,
        rest.trim_start_matches(|c: char| c.is_ascii_whitespace()),
    ))
}

/// Only a fully empty line or a comment line is skipped; a whitespace-only
/// line still counts for indentation, as in Bash.
fn is_blank_or_comment(line: &str) -> bool {
    line.is_empty() || strip_indent(line).1.starts_with('#')
}

/// Trim, unwrap one layer of matching quotes (verbatim inside, no escape
/// processing), otherwise cut at the first `#`.
pub fn normalize_scalar(raw: &str) -> String {
    let value = raw.trim_matches(|c: char| c.is_ascii_whitespace());
    for quote in ['"', '\''] {
        if let Some(inner) = unwrap_quoted(value, quote) {
            return inner.to_string();
        }
    }
    let unquoted = value.split('#').next().unwrap_or("");
    unquoted
        .trim_end_matches(|c: char| c.is_ascii_whitespace())
        .to_string()
}

/// `"inner"` optionally followed by whitespace and a `# comment`. The closing
/// quote is the last one that leaves only such a tail, which is what Bash's
/// greedy `^"(.*)"[[:space:]]*(#.*)?$` picks.
fn unwrap_quoted(value: &str, quote: char) -> Option<&str> {
    let body = value.strip_prefix(quote)?;
    body.rmatch_indices(quote).find_map(|(idx, _)| {
        let tail =
            body[idx + quote.len_utf8()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
        (tail.is_empty() || tail.starts_with('#')).then_some(&body[..idx])
    })
}

/// The scalar at a dotted key path (`targets.rules.dest`), or `""` when the
/// key is missing or empty. Nesting is decided by indentation and the first
/// occurrence of a key wins.
pub fn value(text: &str, key_path: &str) -> String {
    found(text, key_path).unwrap_or_default()
}

/// [`value`], telling a key present with an empty value (`Some("")`) from a
/// missing one (`None`), as `YAML_VALUE_FOUND` does.
pub fn found(text: &str, key_path: &str) -> Option<String> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut level = 0usize;
    let mut section_indent = 0usize;
    let mut child_indent: Option<usize> = None;
    let mut in_section = false;

    for line in text.lines() {
        if is_blank_or_comment(line) {
            continue;
        }
        let (indent, stripped) = strip_indent(line);
        let Some((key, rest)) = split_key(stripped) else {
            continue;
        };
        if !in_section {
            if indent != 0 || key != keys[0] {
                continue;
            }
        } else {
            if indent <= section_indent {
                return None;
            }
            if indent != *child_indent.get_or_insert(indent) || key != keys[level] {
                continue;
            }
        }
        if level + 1 == keys.len() {
            return Some(normalize_scalar(rest));
        }
        if !normalize_scalar(rest).is_empty() {
            continue;
        }
        in_section = true;
        section_indent = indent;
        child_indent = None;
        level += 1;
    }
    None
}

/// Items of the list at a dotted key path: a single-line `[a, b]` or a block
/// of `- item` lines. Empty when the key is missing.
pub fn list(text: &str, key_path: &str) -> Vec<String> {
    let inline = value(text, key_path);
    if let Some(items) = inline.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return items
            .split(',')
            .map(normalize_scalar)
            .filter(|item| !item.is_empty())
            .collect();
    }
    block_list(text, key_path)
}

fn block_list(text: &str, key_path: &str) -> Vec<String> {
    let keys: Vec<&str> = key_path.split('.').collect();
    let mut items = Vec::new();
    let mut level = 0usize;
    let mut section_indent = 0usize;
    let mut child_indent: Option<usize> = None;
    let mut in_section = false;
    let mut collecting = false;
    let mut key_indent = 0usize;
    let mut list_indent: Option<usize> = None;

    for line in text.lines() {
        if is_blank_or_comment(line) {
            continue;
        }
        let (indent, stripped) = strip_indent(line);

        if collecting {
            let dash = stripped.strip_prefix('-');
            if list_indent.is_none() && (dash.is_none() || indent < key_indent) {
                return items;
            }
            if let Some(item) = dash {
                let expected = *list_indent.get_or_insert(indent);
                if indent == expected {
                    let item = normalize_scalar(item);
                    if !item.is_empty() {
                        items.push(item);
                    }
                    continue;
                }
            }
            if list_indent.is_some_and(|expected| indent < expected) {
                return items;
            }
            continue;
        }

        let Some((key, rest)) = split_key(stripped) else {
            continue;
        };
        if !in_section {
            if indent != 0 || key != keys[0] {
                continue;
            }
        } else {
            if indent <= section_indent {
                return items;
            }
            if indent != *child_indent.get_or_insert(indent) || key != keys[level] {
                continue;
            }
        }
        if level + 1 == keys.len() {
            collecting = true;
            key_indent = indent;
            continue;
        }
        if !normalize_scalar(rest).is_empty() {
            continue;
        }
        in_section = true;
        section_indent = indent;
        child_indent = None;
        level += 1;
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# A config in the shapes the Bash reader accepts.
name: "Claude Code"
enabled: false # switched off
count: 3
empty:
targets:
  rules:
    dest: ".claude/rules"
    header: "---\nglobs: '**/*'\n---"
  skills:
    dest: '.claude/skills' # single quotes
tools:
  enabled:
    - claude
    - "cursor"
    - codex   # trailing comment
inline: [a, b , "c"]
dup: first
dup: second
url: http://example.com/x#frag
"#;

    #[test]
    fn reads_a_quoted_root_scalar() {
        assert_eq!(value(SAMPLE, "name"), "Claude Code");
    }

    #[test]
    fn drops_an_inline_comment_from_an_unquoted_scalar() {
        assert_eq!(value(SAMPLE, "enabled"), "false");
        assert_eq!(value(SAMPLE, "count"), "3");
    }

    #[test]
    fn keeps_escapes_literal_inside_quotes() {
        assert_eq!(
            value(SAMPLE, "targets.rules.header"),
            r"---\nglobs: '**/*'\n---"
        );
    }

    #[test]
    fn walks_nested_keys_by_indent() {
        assert_eq!(value(SAMPLE, "targets.rules.dest"), ".claude/rules");
        assert_eq!(value(SAMPLE, "targets.skills.dest"), ".claude/skills");
    }

    #[test]
    fn a_missing_nested_key_does_not_leak_into_the_next_section() {
        assert_eq!(value(SAMPLE, "targets.rules.missing"), "");
    }

    #[test]
    fn missing_empty_and_section_keys_all_read_as_empty() {
        assert_eq!(value(SAMPLE, "missing"), "");
        assert_eq!(value(SAMPLE, "empty"), "");
        assert_eq!(value(SAMPLE, "targets"), "");
    }

    #[test]
    fn the_first_duplicate_key_wins() {
        assert_eq!(value(SAMPLE, "dup"), "first");
    }

    #[test]
    fn an_unquoted_value_ends_at_the_first_hash() {
        assert_eq!(value(SAMPLE, "url"), "http://example.com/x");
    }

    #[test]
    fn a_quoted_value_keeps_its_hash_and_drops_a_trailing_comment() {
        assert_eq!(value("k: \"a # b\" # c\n", "k"), "a # b");
    }

    #[test]
    fn a_key_without_a_space_after_the_colon_still_parses() {
        assert_eq!(value("k:v\n", "k"), "v");
    }

    #[test]
    fn reads_a_block_list() {
        assert_eq!(list(SAMPLE, "tools.enabled"), ["claude", "cursor", "codex"]);
    }

    #[test]
    fn reads_an_inline_list() {
        assert_eq!(list(SAMPLE, "inline"), ["a", "b", "c"]);
    }

    #[test]
    fn a_missing_key_yields_no_list() {
        assert!(list(SAMPLE, "missing").is_empty());
        assert!(list("k: v\n", "k").is_empty());
    }

    #[test]
    fn a_block_list_ends_at_a_shallower_line() {
        let text = "tools:\n  enabled:\n    - claude\n  other: x\n    - not-an-item\n";
        assert_eq!(list(text, "tools.enabled"), ["claude"]);
    }

    #[test]
    fn an_empty_block_key_ends_at_the_next_key() {
        let text = "tools:\n  enabled:\nother:\n  - stolen\n";
        assert!(list(text, "tools.enabled").is_empty());
        let text = "tools:\n  enabled:\n  other: x\n  list:\n    - stolen\n";
        assert!(list(text, "tools.enabled").is_empty());
        let compact = "tools:\n  enabled:\n  - claude\n";
        assert_eq!(list(compact, "tools.enabled"), ["claude"]);
    }

    #[test]
    fn a_nested_key_matches_only_a_direct_child() {
        let text = "tools:\n  foo:\n    enabled: x\n    list:\n      - a\n";
        assert_eq!(found(text, "tools.enabled"), None);
        assert!(list(text, "tools.list").is_empty());
        let text = "tools:\n  foo:\n    enabled: x\n  enabled: y\n";
        assert_eq!(value(text, "tools.enabled"), "y");
    }

    #[test]
    fn a_key_holding_a_scalar_is_not_the_section_of_a_later_mapping() {
        let text = "pin: warn\npin:\n  mode: strict\n";
        assert_eq!(value(text, "pin.mode"), "strict");
        assert_eq!(value(text, "pin"), "warn");
        let text = "tools: x\ntools:\n  enabled:\n    - a\n";
        assert_eq!(list(text, "tools.enabled"), ["a"]);
    }

    #[test]
    fn found_tells_an_empty_value_from_a_missing_key() {
        let text = "backup:\n  retention:\n  other: \"\"\n  note: # nothing\nkeep: x\n";
        assert_eq!(found(text, "backup.retention"), Some(String::new()));
        assert_eq!(found(text, "backup.other"), Some(String::new()));
        assert_eq!(found(text, "backup.note"), Some(String::new()));
        assert_eq!(found(text, "backup.missing"), None);
        assert_eq!(found(text, "keep"), Some("x".to_string()));
        assert_eq!(found(text, "backup"), Some(String::new()));
        assert_eq!(found("backup: preserve\n", "backup.retention"), None);
    }
}
