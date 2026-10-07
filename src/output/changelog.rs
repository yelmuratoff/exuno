//! The changelog renderer of `lib/helpers/update.sh`: `_md_plain`,
//! `_print_wrapped` over `fold -s`, `_show_changelog_sections`, the range
//! `_show_changelog_range` selects, and the `sort -V` order they rely on.

use std::cmp::Ordering;

use crate::output::style::Style;

/// `_md_plain`: backticks removed, and outside a code span `**` dropped and
/// `[text](url)` reduced to what it says — a terminal has nothing to click.
/// Inside a code span every character stays, so a glob keeps its `**`.
pub fn md_plain(text: &str) -> String {
    plain_links(text)
        .split('`')
        .enumerate()
        .map(|(index, part)| {
            if index % 2 == 0 {
                part.replace("**", "")
            } else {
                part.to_string()
            }
        })
        .collect()
}

/// `[text](url)` becomes `text`, or `text (url)` when the url adds something.
fn plain_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](") else {
            break;
        };
        let after = open + close + 2;
        let Some(end) = rest[after..].find(')') else {
            break;
        };
        let label = &rest[open + 1..open + close];
        let url = &rest[after..after + end];
        out.push_str(&rest[..open]);
        out.push_str(label);
        if !url.is_empty() && url != label.replace('`', "") {
            out.push_str(&format!(" ({url})"));
        }
        rest = &rest[after + end + 1..];
    }
    out.push_str(rest);
    out
}

/// `sort -V` on two versions: digit runs compare as numbers, other runs as
/// bytes, and a version that is a prefix of the other sorts first.
pub fn version_cmp(a: &str, b: &str) -> Ordering {
    let mut left = runs(a).into_iter();
    let mut right = runs(b).into_iter();
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((true, x)), Some((true, y))) => {
                let x = x.trim_start_matches('0');
                let y = y.trim_start_matches('0');
                let order = x.len().cmp(&y.len()).then_with(|| x.cmp(y));
                if order != Ordering::Equal {
                    return order;
                }
            }
            (Some((_, x)), Some((_, y))) => {
                let order = x.cmp(y);
                if order != Ordering::Equal {
                    return order;
                }
            }
        }
    }
}

fn runs(version: &str) -> Vec<(bool, &str)> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = version.as_bytes();
    while start < bytes.len() {
        let digits = bytes[start].is_ascii_digit();
        let mut end = start;
        while end < bytes.len() && bytes[end].is_ascii_digit() == digits {
            end += 1;
        }
        out.push((digits, &version[start..end]));
        start = end;
    }
    out
}

/// `_changelog_width`: `tput cols` range-checked, 80 when unusable, 100 at most.
pub fn clamp_width(cols: Option<&str>) -> usize {
    let cols = cols.unwrap_or("");
    if cols.is_empty() || !cols.bytes().all(|b| b.is_ascii_digit()) {
        return 80;
    }
    match cols.parse::<usize>() {
        Ok(n) if n < 40 => 80,
        Ok(n) if n > 100 => 100,
        Ok(n) => n,
        Err(_) => 80,
    }
}

/// `fold -s -w <width>`: a line longer than `width` characters breaks after
/// its last space within the width, or at the width when it has none.
fn fold_spaces(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut column = 0usize;
    for c in text.chars() {
        if c == '\n' {
            lines.push(std::mem::take(&mut line));
            column = 0;
            continue;
        }
        if column >= width {
            match line.rfind(' ') {
                Some(at) => {
                    let rest = line.split_off(at + 1);
                    lines.push(std::mem::replace(&mut line, rest));
                    column = line.chars().count();
                }
                None => {
                    lines.push(std::mem::take(&mut line));
                    column = 0;
                }
            }
        }
        line.push(c);
        column += 1;
    }
    lines.push(line);
    lines
}

/// `_print_wrapped`: `text` folded to `width` less the continuation prefix
/// (24 columns at least), trailing whitespace dropped from every piece, the
/// first piece behind `first` and the others behind `cont`. `$(...)` drops
/// the trailing newlines `fold` prints, so an empty text is one empty line.
pub fn wrap(text: &str, first: &str, cont: &str, width: usize) -> String {
    let avail = width.saturating_sub(cont.chars().count()).max(24);
    let mut pieces = fold_spaces(&format!("{text}\n"), avail);
    while pieces.len() > 1 && pieces.last().is_some_and(String::is_empty) {
        pieces.pop();
    }
    let mut out = String::new();
    for (index, piece) in pieces.iter().enumerate() {
        let piece = piece.trim_end_matches(|c: char| c.is_ascii_whitespace());
        let prefix = if index == 0 { first } else { cont };
        out.push_str(prefix);
        out.push_str(piece);
        out.push('\n');
    }
    out
}

/// The lines `while read` yields: every line ended by a newline.
fn complete_lines(text: &str) -> impl Iterator<Item = &str> {
    text.split_inclusive('\n')
        .filter_map(|line| line.strip_suffix('\n'))
}

/// `_show_changelog_sections`: each version's section, a `## <version>`
/// heading matched by prefix as Bash matches it, `### ` as a bold heading,
/// `- ` as a wrapped bullet, other text wrapped as a paragraph.
pub fn sections(changelog: &str, versions: &[String], width: usize, style: &Style) -> String {
    let mut out = String::new();
    for version in versions {
        if version.is_empty() {
            continue;
        }
        out.push_str(&format!(
            "\n  {}\n\n",
            style.cyan(&format!("What's new in v{version}:"))
        ));
        let heading = format!("## {version}");
        let mut in_section = false;
        let mut started = false;
        for line in complete_lines(changelog) {
            // The whole version, not a prefix of it: `## 9.9.90` is not the
            // section of `9.9.9`. A suffix that cannot continue a version is
            // part of the heading, as in `## 0.35.2 (hotfix)`.
            if line
                .strip_prefix(&heading)
                .is_some_and(|rest| !rest.starts_with(|c: char| c.is_ascii_digit() || c == '.'))
            {
                in_section = true;
                continue;
            }
            if !in_section {
                continue;
            }
            if line.starts_with("## ") {
                break;
            }
            if line.is_empty() && !started {
                continue;
            }
            started = true;
            if let Some(rest) = line.strip_prefix("### ") {
                out.push_str(&format!("\n  {}\n", style.bold(&md_plain(rest))));
            } else if let Some(rest) = line.strip_prefix("- ") {
                out.push_str(&wrap(
                    &md_plain(rest),
                    &format!("    {} ", style.dim("•")),
                    "      ",
                    width,
                ));
            } else if let Some(rest) = line.strip_prefix("  - ") {
                out.push_str(&wrap(&md_plain(rest), "      - ", "        ", width));
            } else if !line.is_empty() {
                out.push_str(&wrap(&md_plain(line), "    ", "    ", width));
            }
        }
    }
    out
}

/// `_show_changelog_range`'s selection: the first word of every `## ` heading
/// with `old < version <= new` in `sort -V` order, ascending.
pub fn versions_in_range(changelog: &str, old: &str, new: &str) -> Vec<String> {
    let mut found: Vec<String> = complete_lines(changelog)
        .filter_map(|line| line.strip_prefix("## "))
        .map(|rest| rest.split(' ').next().unwrap_or("").to_string())
        .filter(|v| {
            version_cmp(old, v) == Ordering::Less && version_cmp(v, new) != Ordering::Greater
        })
        .collect();
    found.sort_by(|a, b| version_cmp(a, b));
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = "# Changelog\n\n## 9.9.9\n\nA summary line that is deliberately long enough to need wrapping when it is printed into a terminal of ordinary width, several times over, so the wrap is unmistakable.\n\n### Internal\n\n- **A bold lead-in.** Body text mentioning `code_span` and a second `span`, padded out so this bullet is certainly longer than any sensible terminal width and must wrap onto continuation lines.\n- A short bullet.\n\n## 9.9.8\n\n- Older entry that must not appear.\n";

    #[test]
    fn markdown_markers_are_stripped_like_md_plain() {
        assert_eq!(md_plain("**Bold.** rest"), "Bold. rest");
        assert_eq!(md_plain("run `exuno sync` now"), "run exuno sync now");
        // A terminal has nothing to click: the label carries the meaning, and
        // the target is added only when it says something the label does not.
        assert_eq!(
            md_plain("see [`docs/perf/x.md`](docs/perf/x.md) for the method"),
            "see docs/perf/x.md for the method"
        );
        assert_eq!(
            md_plain("see [the baseline](docs/perf/y.md)"),
            "see the baseline (docs/perf/y.md)"
        );
        assert_eq!(
            md_plain("an [unclosed link and a [pair]"),
            "an [unclosed link and a [pair]"
        );
        assert_eq!(
            md_plain("plain text — with an em dash"),
            "plain text — with an em dash"
        );
        assert_eq!(
            md_plain("**Scoped.** `paths: [\"src/**\"]` and `src/**` too"),
            "Scoped. paths: [\"src/**\"] and src/** too"
        );
    }

    #[test]
    fn a_nested_bullet_keeps_its_indent_on_every_line() {
        let changelog = "## 1.0.0\n\n- Parent.\n  - A nested item long enough that it has to wrap onto a second line here.\n";
        let out = sections(changelog, &["1.0.0".to_string()], 50, &Style::plain());
        assert!(out.contains("    • Parent.\n      - A nested item long enough that it has to\n        wrap onto a second line here.\n"), "{out}");
        assert!(out.lines().all(|line| line.chars().count() <= 50), "{out}");
    }

    #[test]
    fn versions_order_like_sort_v() {
        assert_eq!(version_cmp("0.9.0", "0.10.0"), Ordering::Less);
        assert_eq!(version_cmp("0.35.2", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.36.0", "0.36.0"), Ordering::Equal);
        assert_eq!(version_cmp("1.0.0", "0.99.99"), Ordering::Greater);
        assert_eq!(version_cmp("0.36", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.0.0-dev", "0.36.0"), Ordering::Less);
        assert_eq!(version_cmp("0.36.0-rc1", "0.36.0"), Ordering::Greater);
    }

    #[test]
    fn the_width_is_clamped_like_changelog_width() {
        assert_eq!(clamp_width(None), 80);
        assert_eq!(clamp_width(Some("")), 80);
        assert_eq!(clamp_width(Some("x")), 80);
        assert_eq!(clamp_width(Some("39")), 80);
        assert_eq!(clamp_width(Some("40")), 40);
        assert_eq!(clamp_width(Some("72")), 72);
        assert_eq!(clamp_width(Some("300")), 100);
    }

    #[test]
    fn wrapping_keeps_every_line_within_the_width_and_indents_continuations() {
        let text = "word ".repeat(80);
        let out = wrap(&text, "    • ", "      ", 72);
        assert!(out.lines().all(|line| line.chars().count() <= 72));
        let out = wrap(&"word ".repeat(40), "    * ", "      ", 60);
        let mut lines = out.lines();
        assert!(lines.next().unwrap().starts_with("    * word"));
        assert!(lines.next().unwrap().starts_with("      word"));
        assert_eq!(wrap("brief", "    • ", "      ", 80), "    • brief\n");
        let out = wrap("alpha beta gamma delta", "  ", "  ", 1);
        assert!(out.contains("alpha"));
        assert_eq!(wrap("", "    ", "    ", 80), "    \n");
    }

    #[test]
    fn a_word_longer_than_the_width_breaks_at_the_width_like_fold() {
        let text = "a".repeat(30);
        assert_eq!(fold_spaces(&text, 24), ["a".repeat(24), "a".repeat(6)]);
        assert_eq!(fold_spaces("ab cd ef", 5), ["ab ", "cd ef"]);
        assert_eq!(fold_spaces("ab cd\n", 5), ["ab cd", ""]);
    }

    #[test]
    fn only_the_requested_section_is_rendered_without_markers() {
        let out = sections(FIXTURE, &["9.9.9".to_string()], 80, &Style::plain());
        assert!(out.starts_with("\n  What's new in v9.9.9:\n\n"));
        assert!(out.contains("\n  Internal\n"));
        assert!(out.contains("    • A short bullet.\n"));
        assert!(out.contains("A bold lead-in. Body text mentioning code_span"));
        assert!(!out.contains("must not appear"));
        assert!(!out.contains("**"));
        assert!(!out.contains('`'));
        assert!(out.lines().all(|line| line.chars().count() <= 80));
        let coloured = sections(FIXTURE, &["9.9.9".to_string()], 80, &Style::colored());
        assert!(coloured.contains("\x1b[36mWhat's new in v9.9.9:\x1b[0m"));
        assert!(coloured.contains("    \x1b[2m•\x1b[0m A short bullet.\n"));
    }

    #[test]
    fn a_heading_matches_the_whole_version_not_a_prefix_of_it() {
        // Was known quirk 55: `## 9.9.90` opened the section of `9.9.9`, which
        // then ran to the end of the file.
        let out = sections(
            "## 9.9.90\n\n- Ninety.\n\n## 9.9.9\n\n- Nine.\n",
            &["9.9.9".to_string()],
            80,
            &Style::plain(),
        );
        assert_eq!(out, "\n  What's new in v9.9.9:\n\n    • Nine.\n");
        // A suffix that cannot continue a version belongs to the heading:
        // `versions_in_range` reads `0.35.2` off exactly this shape.
        assert_eq!(
            sections(
                "## 0.35.2 (hotfix)\n\n- Patched.\n",
                &["0.35.2".to_string()],
                80,
                &Style::plain(),
            ),
            "\n  What's new in v0.35.2:\n\n    • Patched.\n"
        );
    }

    #[test]
    fn the_range_excludes_the_old_version_and_includes_the_new_one() {
        let changelog = "## 0.36.0\n\n## 0.35.2 (hotfix)\n\n## 0.35.10\n\n## 0.35.1\n\n## 0.34.0\n";
        assert_eq!(
            versions_in_range(changelog, "0.35.1", "0.36.0"),
            ["0.35.2", "0.35.10", "0.36.0"]
        );
        assert!(versions_in_range(changelog, "0.36.0", "0.36.0").is_empty());
        assert!(versions_in_range(changelog, "0.36.0", "0.35.1").is_empty());
        assert_eq!(
            versions_in_range("## 0.36.0", "0.35.0", "0.36.0"),
            Vec::<String>::new()
        );
    }
}
