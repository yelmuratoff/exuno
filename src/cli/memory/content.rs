//! What a note's text says: the paths it names and the credentials it holds.

use std::path::Path;

use crate::paths::DiskText;

/// Where a named path is looked up. `root` is set only for a store of the
/// project being checked; a relative path in another project's note means
/// nothing here.
pub struct Bases<'a> {
    pub root: Option<&'a Path>,
    pub home: Option<&'a Path>,
}

/// Paths in inline code that point at nothing. Only paths under the home
/// directory or, for the project's own store, the project root are checked:
/// a system path in a note often describes another machine. A path counts
/// only when its first segment exists, so a branch name (`origin/main`) or a
/// slash command (`/memory`) is not mistaken for one.
pub fn missing_paths(body: &str, bases: &Bases) -> Vec<String> {
    let home_text = bases.home.map(DiskText::disk_text);
    let mut missing: Vec<String> = Vec::new();
    for token in inline_code(body) {
        let Some(path) = candidate(token) else {
            continue;
        };
        let located = if let Some(rest) = path.strip_prefix("~/") {
            bases.home.map(|base| (base, rest))
        } else if path.starts_with('/') {
            bases
                .home
                .zip(home_text.as_deref())
                .and_then(|(base, home)| Some((base, path.strip_prefix(home)?.strip_prefix('/')?)))
        } else {
            bases.root.map(|base| (base, path))
        };
        let Some((base, rest)) = located else {
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

/// The path a code span names, its `:line[:col]` suffix dropped; `None` for a
/// URL, a placeholder, glob, or elided example (`...`, `…`, `..`), a command
/// line, or anything without a `/`.
fn candidate(token: &str) -> Option<&str> {
    let token = token.trim();
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
            Some((head, tail)) if !tail.is_empty() && tail.bytes().all(|b| b.is_ascii_digit()) => {
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
    fn a_path_is_missing_only_below_an_existing_first_segment() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/cli")).unwrap();
        std::fs::write(dir.path().join("src/cli/mod.rs"), "").unwrap();
        let bases = Bases {
            root: Some(dir.path()),
            home: Some(dir.path()),
        };
        let home = dir.path().disk_text();
        let body = format!(
            "`src/cli/mod.rs` `src/gone.rs` `origin/main` `/memory` `~/src/gone.rs` `src/gone.rs:3` `{home}/src/old.rs` `/etc/nowhere/x`"
        );
        assert_eq!(
            missing_paths(&body, &bases),
            [
                "src/gone.rs".to_string(),
                "~/src/gone.rs".to_string(),
                format!("{home}/src/old.rs")
            ]
        );
        let body = body.as_str();
        let no_root = Bases {
            root: None,
            home: None,
        };
        assert!(missing_paths(body, &no_root).is_empty());
    }

    #[test]
    fn a_credential_is_reported_by_line_only() {
        assert_eq!(
            credential_lines("ok\ntoken ghp_abcdefghijklmnopqrstuvwxyz0123456789\n"),
            [2]
        );
    }
}
