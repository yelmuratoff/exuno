//! Top-level keys of a Markdown file's leading `---` frontmatter, read and
//! filtered line by line so every kept byte stays as the source wrote it.

/// The key a column-0 `key:` frontmatter line declares.
pub fn key(line: &[u8]) -> Option<&[u8]> {
    let first = *line.first()?;
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return None;
    }
    let end = line
        .iter()
        .position(|b| !(b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-'))?;
    (line[end] == b':').then_some(&line[..end])
}

/// `text` with only the `keep` keys left in its frontmatter, and the keys it
/// dropped in source order. Text without frontmatter comes back unchanged.
pub fn retain_keys(text: &str, keep: &[String]) -> (String, Vec<String>) {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let is_fence = |line: &str| content(line) == "---";
    if !lines.first().is_some_and(|line| is_fence(line)) {
        return (text.to_string(), Vec::new());
    }
    let Some(close) = lines.iter().skip(1).position(|line| is_fence(line)) else {
        return (text.to_string(), Vec::new());
    };
    let body = &lines[1..=close];

    let mut out = String::with_capacity(text.len());
    out.push_str(lines[0]);
    let mut dropped: Vec<String> = Vec::new();
    let mut kept = true;
    for (index, line) in body.iter().enumerate() {
        let bytes = content(line).as_bytes();
        if let Some(name) = key(bytes) {
            let name = String::from_utf8_lossy(name).into_owned();
            kept = keep.contains(&name);
            if !kept && !dropped.contains(&name) {
                dropped.push(name);
            }
        } else if bytes.is_empty() {
            let continues = body[index + 1..]
                .iter()
                .map(|next| content(next))
                .find(|next| !next.is_empty())
                .is_some_and(continuation);
            if !continues {
                kept = true;
            }
        } else if !continuation(content(line)) {
            kept = true;
        }
        if kept {
            out.push_str(line);
        }
    }
    for line in &lines[close + 1..] {
        out.push_str(line);
    }
    (out, dropped)
}

fn content(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

/// An indented line, or a `- item` at column 0: YAML lets a block sequence
/// sit at its key's own indentation.
fn continuation(line: &str) -> bool {
    line.starts_with([' ', '\t']) || line == "-" || line.starts_with("- ")
}

#[cfg(test)]
mod tests {
    use super::retain_keys;

    fn keep(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    #[test]
    fn keeps_listed_keys_byte_for_byte() {
        let text = "---\nname: a\ndescription: \"Quoted: yes\"  # note\nargument-hint: <file>\n---\nBody\n\n---\nnot: frontmatter\n";
        let (out, dropped) = retain_keys(text, &keep(&["name", "description"]));
        assert_eq!(
            out,
            "---\nname: a\ndescription: \"Quoted: yes\"  # note\n---\nBody\n\n---\nnot: frontmatter\n"
        );
        assert_eq!(dropped, ["argument-hint"]);
    }

    #[test]
    fn drops_a_block_scalar_with_its_continuation_lines() {
        let text =
            "---\nname: a\ndescription: >-\n  line one\n\n  line two\nlicense: MIT\n---\nBody\n";
        let (out, dropped) = retain_keys(text, &keep(&["name", "license"]));
        assert_eq!(out, "---\nname: a\nlicense: MIT\n---\nBody\n");
        assert_eq!(dropped, ["description"]);

        let (out, dropped) = retain_keys(text, &keep(&["description"]));
        assert_eq!(
            out,
            "---\ndescription: >-\n  line one\n\n  line two\n---\nBody\n"
        );
        assert_eq!(dropped, ["name", "license"]);
    }

    #[test]
    fn drops_a_nested_map() {
        let text = "---\nname: a\nhooks:\n  PreToolUse:\n    - matcher: x\nallowed-tools:\n- Bash\n- Read\nmetadata:\n  version: 1\n---\n";
        let (out, dropped) = retain_keys(text, &keep(&["name", "metadata"]));
        assert_eq!(out, "---\nname: a\nmetadata:\n  version: 1\n---\n");
        assert_eq!(dropped, ["hooks", "allowed-tools"]);
    }

    #[test]
    fn leaves_text_without_frontmatter_unchanged() {
        for text in [
            "# Title\nname: x\n",
            "---\nname: a\nmodel: x\n",
            "",
            "--- \nmodel: x\n---\n",
        ] {
            let (out, dropped) = retain_keys(text, &keep(&["name"]));
            assert_eq!(out, text);
            assert!(dropped.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let text = "---\r\nname: a\r\nmodel: x\r\ndescription: >\r\n  folded\r\n---\r\nBody\r\n";
        let (out, dropped) = retain_keys(text, &keep(&["name", "description"]));
        assert_eq!(
            out,
            "---\r\nname: a\r\ndescription: >\r\n  folded\r\n---\r\nBody\r\n"
        );
        assert_eq!(dropped, ["model"]);
    }

    #[test]
    fn returns_dropped_keys_in_source_order() {
        let text = "---\nz-key: 1\nname: a\nargument-hint: h\n# a comment\n\ndisable-model-invocation: true\n---\n";
        let (out, dropped) = retain_keys(text, &keep(&["name"]));
        assert_eq!(out, "---\nname: a\n# a comment\n\n---\n");
        assert_eq!(
            dropped,
            ["z-key", "argument-hint", "disable-model-invocation"]
        );
    }
}
