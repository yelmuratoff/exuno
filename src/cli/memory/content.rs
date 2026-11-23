//! What a note's text says: the paths it names and the credentials it holds.

use std::path::Path;

use crate::paths::DiskText;

/// Paths in inline code under the home directory that point at nothing,
/// written `~/…` or in full. A system path in a note often describes another
/// machine, and a relative one names no root it is relative to, so neither is
/// checked. A path counts only when its first segment exists, so a slash
/// command (`/memory`) or an example home is not mistaken for one.
pub fn missing_paths(body: &str, home: Option<&Path>) -> Vec<String> {
    let Some(base) = home else {
        return Vec::new();
    };
    let home_text = base.disk_text();
    let home_text = home_text.trim_end_matches('/');
    let mut missing: Vec<String> = Vec::new();
    for token in inline_code(body) {
        let Some(path) = candidate(token) else {
            continue;
        };
        let rest = path
            .strip_prefix("~/")
            .or_else(|| path.strip_prefix(home_text)?.strip_prefix('/'));
        let Some(rest) = rest else {
            continue;
        };
        let mut segments = rest.split('/').filter(|segment| !segment.is_empty());
        let Some(first) = segments.next() else {
            continue;
        };
        if segments.next().is_none() || !base.join(first).exists() {
            continue;
        }
        if !base.join(rest).exists() && !missing.iter().any(|seen| seen == path) {
            missing.push(path.to_string());
        }
    }
    missing
}

/// Inline code spans outside fenced blocks: example output in a fence names
/// paths that never had to exist.
fn inline_code(body: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut fenced = false;
    for line in body.lines() {
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        spans.extend(line.split('`').skip(1).step_by(2));
    }
    spans
}

/// The path a code span names, its `#anchor` and `:line[:col]` or
/// `:first-last` suffix dropped; `None` for a URL, a placeholder, glob, or
/// elided example (`...`, `…`, `..`), a command line, or anything without a
/// `/`.
fn candidate(token: &str) -> Option<&str> {
    let token = token.trim();
    let token = token.split_once('#').map_or(token, |(path, _)| path);
    let token = strip_position(token);
    let path_like = token.contains('/')
        && !token.contains("://")
        && !token.contains("..")
        && !token.contains('…')
        && !token.chars().any(char::is_whitespace)
        && !token.contains([
            '*', '?', '{', '}', '<', '>', '$', '[', ']', '(', ')', '|', '=', '"', '\'',
        ])
        && !token.starts_with('-');
    path_like.then_some(token.trim_end_matches('/'))
}

fn strip_position(token: &str) -> &str {
    let mut rest = token;
    for _ in 0..2 {
        match rest.rsplit_once(':') {
            Some((head, tail))
                if tail.starts_with(|c: char| c.is_ascii_digit())
                    && tail.bytes().all(|b| b.is_ascii_digit() || b == b'-') =>
            {
                rest = head;
            }
            _ => break,
        }
    }
    rest
}

/// The line numbers holding something shaped like a credential, by the
/// patterns `doctor` scans overrides with; the credential itself is dropped.
pub fn credential_lines(text: &str) -> Vec<usize> {
    crate::cli::doctor::secrets::scan_secrets(text.as_bytes())
        .iter()
        .filter_map(|hit| hit.split_once(':')?.0.parse().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidates_are_slash_paths_without_placeholders() {
        assert_eq!(candidate("src/main.rs:42:7"), Some("src/main.rs"));
        assert_eq!(candidate("~/.claude/rules/"), Some("~/.claude/rules"));
        assert_eq!(candidate("~/docs/a.md#setup"), Some("~/docs/a.md"));
        assert_eq!(candidate("~/src/x.rs:10-20"), Some("~/src/x.rs"));
        for not_a_path in [
            "https://e.com/a/b",
            "cargo test --test memory",
            "<config dir>/projects",
            "$HOME/x/y",
            "tests/*.rs",
            "plain",
            "--profile/x",
            "a=b/c",
            "/var/folders/…",
            "~/.ai/.ai/src/...",
            "/../service",
        ] {
            assert_eq!(candidate(not_a_path), None, "{not_a_path}");
        }
    }

    #[test]
    fn fenced_blocks_hold_no_spans() {
        let body = "see `a/b`\n```\n`c/d`\n```\nand `e/f` or `g`\n";
        assert_eq!(inline_code(body), ["a/b", "e/f", "g"]);
    }

    #[test]
    fn only_home_paths_below_an_existing_first_segment_count() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/cli")).unwrap();
        std::fs::write(dir.path().join("src/cli/mod.rs"), "").unwrap();
        let home = dir.path().disk_text();
        let body = format!(
            "`~/src/cli/mod.rs` `src/gone.rs` `origin/main` `/memory` `~/src/gone.rs:3` `{home}/src/old.rs` `/etc/nowhere/x` `~/nowhere/x`"
        );
        assert_eq!(
            missing_paths(&body, Some(dir.path())),
            ["~/src/gone.rs".to_string(), format!("{home}/src/old.rs")]
        );
        assert!(missing_paths(&body, None).is_empty());
    }

    #[test]
    fn a_credential_is_reported_by_line_only() {
        assert_eq!(
            credential_lines("ok\ntoken ghp_abcdefghijklmnopqrstuvwxyz0123456789\n"),
            [2]
        );
    }
}
