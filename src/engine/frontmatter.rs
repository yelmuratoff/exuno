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

/// Frontmatter `retain_keys` cannot filter safely; the text is the reason the
/// user sees.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Unparsed {
    #[error("a byte-order mark precedes the opening ---")]
    ByteOrderMark,
    #[error("the opening --- has trailing whitespace")]
    FenceWhitespace,
    #[error("no closing --- line")]
    Unclosed,
    #[error("line {0} is not a `key:` line")]
    NotAKey(usize),
}

/// `text` with only the `keep` keys left in its frontmatter, and the keys it
/// dropped in source order. Text without frontmatter comes back unchanged.
pub fn retain_keys(text: &str, keep: &[String]) -> Result<(String, Vec<String>), Unparsed> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let is_fence = |line: &str| content(line) == "---";
    let Some(first) = lines.first().map(|line| content(line)) else {
        return Ok((text.to_string(), Vec::new()));
    };
    if first
        .strip_prefix('\u{feff}')
        .is_some_and(|rest| rest.trim_end() == "---")
    {
        return Err(Unparsed::ByteOrderMark);
    }
    if first != "---" {
        if first.trim_end() == "---" {
            return Err(Unparsed::FenceWhitespace);
        }
        return Ok((text.to_string(), Vec::new()));
    }
    let close = lines
        .iter()
        .skip(1)
        .position(|line| is_fence(line))
        .ok_or(Unparsed::Unclosed)?;
    let body = &lines[1..=close];

    let mut out = String::with_capacity(text.len());
    out.push_str(lines[0]);
    let mut dropped: Vec<String> = Vec::new();
    let mut kept = true;
    for (index, line) in body.iter().enumerate() {
        let line_text = content(line);
        if let Some(name) = key(line_text.as_bytes()) {
            let name = String::from_utf8_lossy(name).into_owned();
            kept = keep.contains(&name);
            if !kept && !dropped.contains(&name) {
                dropped.push(name);
            }
        } else if line_text.is_empty() {
            let continues = body[index + 1..]
                .iter()
                .map(|next| content(next))
                .find(|next| !next.is_empty() && !comment(next))
                .is_some_and(continuation);
            if !continues {
                kept = true;
            }
        } else if !continuation(line_text) && !comment(line_text) {
            return Err(Unparsed::NotAKey(index + 2));
        }
        if kept {
            out.push_str(line);
        }
    }
    for line in &lines[close + 1..] {
        out.push_str(line);
    }
    Ok((out, dropped))
}

fn content(line: &str) -> &str {
    let line = line.strip_suffix('\n').unwrap_or(line);
    line.strip_suffix('\r').unwrap_or(line)
}

fn comment(line: &str) -> bool {
    line.starts_with('#')
}

/// An indented line, or a `- item` at column 0: YAML lets a block sequence
/// sit at its key's own indentation.
fn continuation(line: &str) -> bool {
    line.starts_with([' ', '\t']) || line == "-" || line.starts_with("- ")
}

#[cfg(test)]
mod tests {
    use super::{Unparsed, retain_keys};

    fn keep(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| key.to_string()).collect()
    }

    fn filtered(text: &str, keys: &[&str]) -> (String, Vec<String>) {
        retain_keys(text, &keep(keys)).unwrap()
    }

    #[test]
    fn keeps_listed_keys_byte_for_byte() {
        let text = "---\nname: a\ndescription: \"Quoted: yes\"  # note\nargument-hint: <file>\n---\nBody\n\n---\nnot: frontmatter\n";
        let (out, dropped) = filtered(text, &["name", "description"]);
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
        let (out, dropped) = filtered(text, &["name", "license"]);
        assert_eq!(out, "---\nname: a\nlicense: MIT\n---\nBody\n");
        assert_eq!(dropped, ["description"]);

        let (out, dropped) = filtered(text, &["description"]);
        assert_eq!(
            out,
            "---\ndescription: >-\n  line one\n\n  line two\n---\nBody\n"
        );
        assert_eq!(dropped, ["name", "license"]);
    }

    #[test]
    fn drops_a_nested_map() {
        let text = "---\nname: a\nhooks:\n  PreToolUse:\n    - matcher: x\nallowed-tools:\n- Bash\n- Read\nmetadata:\n  version: 1\n---\n";
        let (out, dropped) = filtered(text, &["name", "metadata"]);
        assert_eq!(out, "---\nname: a\nmetadata:\n  version: 1\n---\n");
        assert_eq!(dropped, ["hooks", "allowed-tools"]);
    }

    #[test]
    fn leaves_text_without_frontmatter_unchanged() {
        for text in ["# Title\nname: x\n", "", "Body\n---\nname: x\n---\n"] {
            let (out, dropped) = filtered(text, &["name"]);
            assert_eq!(out, text);
            assert!(dropped.is_empty(), "{text:?}");
        }
    }

    #[test]
    fn preserves_crlf_line_endings() {
        let text = "---\r\nname: a\r\nmodel: x\r\ndescription: >\r\n  folded\r\n---\r\nBody\r\n";
        let (out, dropped) = filtered(text, &["name", "description"]);
        assert_eq!(
            out,
            "---\r\nname: a\r\ndescription: >\r\n  folded\r\n---\r\nBody\r\n"
        );
        assert_eq!(dropped, ["model"]);
    }

    #[test]
    fn returns_dropped_keys_in_source_order() {
        let text = "---\nz-key: 1\nname: a\nargument-hint: h\n# a comment\n\ndisable-model-invocation: true\n---\n";
        let (out, dropped) = filtered(text, &["name"]);
        assert_eq!(out, "---\nname: a\n\n---\n");
        assert_eq!(
            dropped,
            ["z-key", "argument-hint", "disable-model-invocation"]
        );
    }

    #[test]
    fn a_comment_goes_with_the_block_it_sits_in() {
        let text = "---\nname: a\nhooks:\n# pre\n  PreToolUse:\n    - matcher: x\n\n# also hooks\n  Stop: []\ndescription: d\n# about d\n---\n";
        let (out, dropped) = filtered(text, &["name", "description"]);
        assert_eq!(out, "---\nname: a\ndescription: d\n# about d\n---\n");
        assert_eq!(dropped, ["hooks"]);
    }

    #[test]
    fn reports_frontmatter_it_cannot_parse() {
        for (text, reason) in [
            ("\u{feff}---\nname: a\n---\n", Unparsed::ByteOrderMark),
            ("--- \nname: a\n---\n", Unparsed::FenceWhitespace),
            ("---\nname: a\nmodel: x\n", Unparsed::Unclosed),
            ("---\nname: a\n--- \n", Unparsed::Unclosed),
            ("---\nname: a\n\"model\": x\n---\n", Unparsed::NotAKey(3)),
            ("---\nmy key: x\n---\n", Unparsed::NotAKey(2)),
            ("---\nname: a\na.b: x\n---\n", Unparsed::NotAKey(3)),
        ] {
            assert_eq!(retain_keys(text, &keep(&["name"])), Err(reason), "{text:?}");
        }
        assert_eq!(
            Unparsed::NotAKey(3).to_string(),
            "line 3 is not a `key:` line"
        );
    }
}
