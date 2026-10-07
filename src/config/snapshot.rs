//! `lib/helpers/snapshot.sh`: the catalog diff and the conflict queue
//! `update` writes, and the pending-resolutions readers `resolve` uses.

use std::path::Path;

use crate::Error;
use crate::config::yaml_subset;

const PENDING: &str = ".ai/.pending-resolutions.yaml";

/// `_snapshot_keys`, in order.
pub const KEYS: [&str; 26] = [
    "name",
    "enabled",
    "targets.agents.dest",
    "targets.rules.dest",
    "targets.rules.extension",
    "targets.rules.header",
    "targets.rules.scoped_header",
    "targets.rules.append_imports",
    "targets.rules.merge_to_file",
    "targets.rules.inline_into_agents",
    "targets.rules.prepend_agents",
    "targets.skills.dest",
    "targets.skills.inline_into_agents",
    "targets.commands.dest",
    "targets.commands.format",
    "targets.commands.as_skills",
    "targets.commands.inline_into_agents",
    "targets.subagents.dest",
    "targets.subagents.format",
    "targets.settings.source",
    "targets.settings.dest",
    "targets.mcp.source",
    "targets.mcp.dest",
    "targets.hooks.source",
    "targets.hooks.dest",
    "post_sync",
];

/// One `snapshot_diff` line: a key whose value differs between the catalogs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Change {
    pub tool: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

/// One `snapshot_find_conflicts` line: a change on a field the project overrides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Conflict {
    pub tool: String,
    pub field: String,
    pub before: String,
    pub after: String,
    pub yours: String,
}

/// `snapshot_diff` over two catalogs given as `(slug, yaml)`: the changed
/// keys, tools in byte order, keys in [`KEYS`] order. A tool in one catalog
/// only reads as empty on the other side.
pub fn diff(old: &[(String, String)], new: &[(String, String)]) -> Vec<Change> {
    let mut tools: Vec<&str> = old
        .iter()
        .chain(new)
        .map(|(slug, _)| slug.as_str())
        .collect();
    tools.sort_unstable();
    tools.dedup();
    let text_of = |catalog: &[(String, String)], slug: &str| -> Option<String> {
        catalog
            .iter()
            .find(|(name, _)| name == slug)
            .map(|(_, yaml)| yaml.clone())
    };
    let mut changes = Vec::new();
    for tool in tools {
        let old_text = text_of(old, tool);
        let new_text = text_of(new, tool);
        for key in KEYS {
            let before = old_text
                .as_deref()
                .map(|text| yaml_subset::value(text, key))
                .unwrap_or_default();
            let after = new_text
                .as_deref()
                .map(|text| yaml_subset::value(text, key))
                .unwrap_or_default();
            if before != after {
                changes.push(Change {
                    tool: tool.to_string(),
                    field: key.to_string(),
                    before,
                    after,
                });
            }
        }
    }
    changes
}

/// `snapshot_find_conflicts`: the changes whose field the project overrides
/// with a non-empty value in `.ai/src/tools/<tool>.yaml`.
pub fn find_conflicts(root: &Path, changes: &[Change]) -> Vec<Conflict> {
    let tools_dir = root.join(".ai/src/tools");
    if !tools_dir.is_dir() {
        return Vec::new();
    }
    changes
        .iter()
        .filter_map(|change| {
            let file = tools_dir.join(format!("{}.yaml", change.tool));
            let text = std::fs::read(file).ok()?;
            let yours = yaml_subset::value(&String::from_utf8_lossy(&text), &change.field);
            (!yours.is_empty()).then(|| Conflict {
                tool: change.tool.clone(),
                field: change.field.clone(),
                before: change.before.clone(),
                after: change.after.clone(),
                yours,
            })
        })
        .collect()
}

/// `_snapshot_yaml_quote`: a double-quoted scalar with backslashes, quotes,
/// tabs, and newlines escaped.
fn yaml_quote(s: &str) -> String {
    let escaped = s
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\t', "\\t")
        .replace('\n', "\\n");
    format!("\"{escaped}\"")
}

/// The text `snapshot_write_pending_resolutions` writes.
pub fn pending_resolutions(today: &str, from: &str, to: &str, conflicts: &[Conflict]) -> String {
    let mut out = format!(
        "# Exuno — pending upstream resolutions from `exuno update`.\n# Run `exuno resolve` to walk these fields interactively.\n# Remove this file once you've reviewed every entry.\n\nschema: 1\ngenerated_on: \"{today}\"\nfrom_version: \"{from}\"\nto_version: \"{to}\"\nconflicts:\n"
    );
    for conflict in conflicts {
        out.push_str(&format!(
            "  - tool: \"{}\"\n    field: \"{}\"\n    base_before: {}\n    base_after: {}\n    your_override: {}\n",
            conflict.tool,
            conflict.field,
            yaml_quote(&conflict.before),
            yaml_quote(&conflict.after),
            yaml_quote(&conflict.yours)
        ));
    }
    if conflicts.is_empty() {
        out.push_str("  []\n");
    }
    out
}

/// `snapshot_write_pending_resolutions`: the queue written beside its
/// destination, nothing when `.ai/` is missing.
pub fn write_pending_resolutions(
    root: &Path,
    today: &str,
    from: &str,
    to: &str,
    conflicts: &[Conflict],
) -> Result<(), Error> {
    if !root.join(".ai").is_dir() {
        return Ok(());
    }
    crate::engine::staging::write_beside(
        &root.join(PENDING),
        pending_resolutions(today, from, to, conflicts).as_bytes(),
    )
}

/// `date -u +%Y-%m-%d` for a time in seconds since the Unix epoch.
pub fn utc_date(secs: u64) -> String {
    let days = (secs / 86_400) as i64 + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

fn after_label(stripped: &str, label: &str) -> String {
    let value = stripped[label.len()..].trim_start_matches(|c: char| c.is_ascii_whitespace());
    let value = value.strip_suffix('"').unwrap_or(value);
    value.strip_prefix('"').unwrap_or(value).to_string()
}

/// `snapshot_read_pending_pairs`: `(tool, field)` per complete conflict.
pub fn read_pending_pairs(root: &Path) -> Vec<(String, String)> {
    let Ok(bytes) = std::fs::read(root.join(PENDING)) else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&bytes);
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let mut pairs = Vec::new();
    let (mut in_conflicts, mut tool, mut field) = (false, String::new(), String::new());
    for line in lines {
        let stripped = line.trim_start_matches(|c: char| c.is_ascii_whitespace());
        if stripped.starts_with("conflicts:") {
            in_conflicts = true;
            continue;
        }
        if !in_conflicts {
            continue;
        }
        if !stripped.is_empty() && line == stripped && !stripped.starts_with('-') {
            break;
        }
        if stripped.starts_with("- tool:") {
            if !tool.is_empty() && !field.is_empty() {
                pairs.push((tool.clone(), field.clone()));
            }
            tool = after_label(stripped, "- tool:");
            field.clear();
        } else if stripped.starts_with("field:") {
            field = after_label(stripped, "field:");
        }
    }
    if !tool.is_empty() && !field.is_empty() {
        pairs.push((tool, field));
    }
    pairs
}

/// `snapshot_clear_pending`.
pub fn clear_pending(root: &Path) {
    let _ = std::fs::remove_file(root.join(PENDING));
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn base_tool(name: &str, rules_dest: &str) -> (String, String) {
        (
            name.to_string(),
            format!(
                "name: {name}\nenabled: true\ntargets:\n  rules:\n    dest: \"{rules_dest}\"\n    extension: .md\n"
            ),
        )
    }

    #[test]
    fn the_diff_lists_changed_keys_and_added_tools_like_snapshot_diff() {
        let old = [base_tool("claude", ".claude/rules")];
        assert!(diff(&old, &old).is_empty());
        let new = [base_tool("claude", ".claude/rules-v2")];
        assert_eq!(
            diff(&old, &new),
            [Change {
                tool: "claude".to_string(),
                field: "targets.rules.dest".to_string(),
                before: ".claude/rules".to_string(),
                after: ".claude/rules-v2".to_string(),
            }]
        );
        let added = [
            base_tool("claude", ".claude/rules"),
            base_tool("cursor", ".cursor/rules"),
        ];
        let changes = diff(&old, &added);
        assert_eq!(
            changes
                .iter()
                .map(|c| (
                    c.tool.as_str(),
                    c.field.as_str(),
                    c.before.as_str(),
                    c.after.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("cursor", "name", "", "cursor"),
                ("cursor", "enabled", "", "true"),
                ("cursor", "targets.rules.dest", "", ".cursor/rules"),
                ("cursor", "targets.rules.extension", "", ".md"),
            ]
        );
    }

    #[test]
    fn conflicts_need_a_non_empty_override_on_the_changed_field() {
        let dir = tempfile::tempdir().unwrap();
        let changes = diff(
            &[base_tool("claude", ".claude/rules")],
            &[base_tool("claude", ".claude/rules-v2")],
        );
        assert!(find_conflicts(dir.path(), &changes).is_empty());
        std::fs::create_dir_all(dir.path().join(".ai/src/tools")).unwrap();
        std::fs::write(
            dir.path().join(".ai/src/tools/claude.yaml"),
            "targets:\n  rules:\n    extension: .mdc\n",
        )
        .unwrap();
        assert!(find_conflicts(dir.path(), &changes).is_empty());
        std::fs::write(
            dir.path().join(".ai/src/tools/claude.yaml"),
            "targets:\n  rules:\n    dest: \".claude/my-rules\"\n",
        )
        .unwrap();
        assert_eq!(
            find_conflicts(dir.path(), &changes),
            [Conflict {
                tool: "claude".to_string(),
                field: "targets.rules.dest".to_string(),
                before: ".claude/rules".to_string(),
                after: ".claude/rules-v2".to_string(),
                yours: ".claude/my-rules".to_string(),
            }]
        );
    }

    #[test]
    fn the_queue_is_the_yaml_snapshot_write_pending_resolutions_writes() {
        let conflicts = [
            Conflict {
                tool: "claude".to_string(),
                field: "targets.rules.dest".to_string(),
                before: ".claude/rules".to_string(),
                after: ".claude/rules-v2".to_string(),
                yours: ".claude/my-rules".to_string(),
            },
            Conflict {
                tool: "claude".to_string(),
                field: "targets.rules.header".to_string(),
                before: "one".to_string(),
                after: "quoted \"hi\"".to_string(),
                yours: "back\\slash\tand\nmore".to_string(),
            },
        ];
        assert_eq!(
            pending_resolutions("2026-09-18", "0.7.0", "0.8.0", &conflicts),
            "# Exuno — pending upstream resolutions from `exuno update`.\n# Run `exuno resolve` to walk these fields interactively.\n# Remove this file once you've reviewed every entry.\n\nschema: 1\ngenerated_on: \"2026-09-18\"\nfrom_version: \"0.7.0\"\nto_version: \"0.8.0\"\nconflicts:\n  - tool: \"claude\"\n    field: \"targets.rules.dest\"\n    base_before: \".claude/rules\"\n    base_after: \".claude/rules-v2\"\n    your_override: \".claude/my-rules\"\n  - tool: \"claude\"\n    field: \"targets.rules.header\"\n    base_before: \"one\"\n    base_after: \"quoted \\\"hi\\\"\"\n    your_override: \"back\\\\slash\\tand\\nmore\"\n"
        );
        assert!(pending_resolutions("2026-09-18", "a", "b", &[]).ends_with("conflicts:\n  []\n"));
        let dir = tempfile::tempdir().unwrap();
        write_pending_resolutions(dir.path(), "2026-09-18", "a", "b", &conflicts).unwrap();
        assert!(!dir.path().join(PENDING).exists());
        std::fs::create_dir(dir.path().join(".ai")).unwrap();
        write_pending_resolutions(dir.path(), "2026-09-18", "a", "b", &conflicts).unwrap();
        assert_eq!(
            read_pending_pairs(dir.path()),
            [
                ("claude".to_string(), "targets.rules.dest".to_string()),
                ("claude".to_string(), "targets.rules.header".to_string()),
            ]
        );
    }

    #[test]
    fn dates_read_as_date_u_prints_them() {
        assert_eq!(utc_date(0), "1970-01-01");
        assert_eq!(utc_date(1_758_153_600), "2025-09-18");
        assert_eq!(utc_date(951_782_400), "2000-02-29");
        assert_eq!(utc_date(4_107_542_399), "2100-02-28");
    }

    #[test]
    fn pending_pairs_are_read_from_the_conflicts_list_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        assert!(read_pending_pairs(dir.path()).is_empty());
        std::fs::write(
            dir.path().join(".ai/.pending-resolutions.yaml"),
            "# c\nschema: 1\nconflicts:\n  - tool: \"cursor\"\n    field: \"targets.rules.dest\"\n    base_before: \"a\"\n  - tool: claude\n    field: name\n\nafter: x\n  - tool: \"zed\"\n    field: \"name\"\n",
        )
        .unwrap();
        assert_eq!(
            read_pending_pairs(dir.path()),
            [
                ("cursor".to_string(), "targets.rules.dest".to_string()),
                ("claude".to_string(), "name".to_string()),
            ]
        );
        clear_pending(dir.path());
        assert!(!dir.path().join(".ai/.pending-resolutions.yaml").exists());
        clear_pending(dir.path());
    }
}
