//! `tests/migrate.bats`: `exuno migrate` — the upgrade-prompt path and
//! `--legacy` flat-layout retirement. Deeper coverage of the legacy-move
//! planning, MCP consolidation, and engine-owned skill retirement logic
//! lives in the `#[cfg(all(test, unix))]` unit tests at the bottom of
//! `src/cli/migrate.rs`, which call `migrate()` directly; the cases here
//! exercise the compiled binary end to end (argument parsing, environment
//! reads, clipboard spawn).

mod common;

use common::Project;
use predicates::prelude::*;

/// `seed_project --yes --no-detect --content agents,rules`.
fn seeded() -> Project {
    Project::seeded(&["--yes", "--no-detect", "--content", "agents,rules"])
}

#[test]
fn migrate_outputs_a_grounded_upgrade_prompt() {
    let project = seeded();
    let mut config: String = project
        .read(".ai/agent_sync.yaml")
        .lines()
        .map(|line| {
            if line.starts_with("agentsync_version:") {
                "agentsync_version: \"0.7.0\"".to_string()
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    config.push('\n');
    project.write(".ai/agent_sync.yaml", &config);

    project
        .exuno()
        .env("AGENTSYNC_NO_CLIPBOARD", "1")
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains("Exuno migration context"))
        .stdout(predicate::str::contains(
            "Project-pinned Exuno version: 0.7.0",
        ))
        .stdout(predicate::str::contains("CHANGELOG.md"))
        .stdout(predicate::str::contains("latest stable AgentSync release"))
        .stdout(predicate::str::contains("agentsync doctor"))
        .stdout(predicate::str::contains("agentsync check"));
}

// The pbcopy stand-in is a shell script the binary cannot spawn on Windows.
#[cfg(unix)]
#[test]
fn migrate_copies_the_full_prompt_with_an_available_clipboard_tool() {
    let project = seeded();
    let mock_bin = project.join("mock-bin");
    let clipboard_capture = project.join("clipboard.txt");
    std::fs::create_dir_all(&mock_bin).unwrap();
    let script = mock_bin.join("pbcopy");
    std::fs::write(
        &script,
        "#!/usr/bin/env bash\ncat > \"$MIGRATE_CLIPBOARD_CAPTURE\"\n",
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{path}", mock_bin.display());

    project
        .exuno()
        .env("PATH", new_path)
        .env("AGENTSYNC_NO_CLIPBOARD", "0")
        .env("MIGRATE_CLIPBOARD_CAPTURE", &clipboard_capture)
        .arg("migrate")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Copied migration prompt to clipboard",
        ));

    let captured = std::fs::read_to_string(&clipboard_capture).unwrap();
    assert!(!captured.is_empty());
    assert!(captured.contains("Exuno migration context"));
    assert!(captured.contains("latest stable AgentSync release"));
}

#[test]
fn migrate_uses_an_explicit_fallback_when_the_project_version_is_absent() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/agent_sync.yaml")).unwrap();

    project
        .exuno()
        .env("AGENTSYNC_NO_CLIPBOARD", "1")
        .arg("migrate")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Project-pinned Exuno version: not detected",
        ));
}

#[test]
fn migrate_legacy_reports_nothing_when_layout_is_already_clean() {
    seeded()
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
}

#[test]
fn migrate_legacy_dry_run_shows_planned_moves_but_does_not_touch_files() {
    let project = seeded();
    project.write(".ai/src/hooks/cursor.json", "{\"m\":\"H\"}\n");
    project.write(".ai/src/settings/claude.json", "{\"m\":\"S\"}\n");

    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Planned moves"))
        .stdout(predicate::str::contains(".ai/src/hooks/cursor.json"))
        .stdout(predicate::str::contains(".ai/src/tools/cursor/hooks.json"))
        .stdout(predicate::str::contains("Dry-run"));

    assert!(project.exists(".ai/src/hooks/cursor.json"));
    assert!(project.exists(".ai/src/settings/claude.json"));
    assert!(!project.exists(".ai/src/tools/cursor/hooks.json"));
}

#[test]
fn migrate_apply_moves_hooks_and_settings_to_per_tool_dirs() {
    let project = seeded();
    project.write(".ai/src/hooks/cursor.json", "{\"m\":\"H\"}\n");
    project.write(".ai/src/settings/claude.json", "{\"m\":\"S\"}\n");

    project
        .exuno()
        .args(["migrate", "--apply"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/tools/cursor/hooks.json"));
    assert!(project.exists(".ai/src/tools/claude/settings.json"));
    assert!(
        project
            .read(".ai/src/tools/cursor/hooks.json")
            .contains("\"m\":\"H\"")
    );
    assert!(
        project
            .read(".ai/src/tools/claude/settings.json")
            .contains("\"m\":\"S\"")
    );

    assert!(!project.exists(".ai/src/hooks/cursor.json"));
    assert!(!project.exists(".ai/src/settings/claude.json"));
    assert!(!project.exists(".ai/src/hooks"));
    assert!(!project.exists(".ai/src/settings"));
}

#[test]
fn migrate_apply_consolidates_identical_mcp_files_into_shared_mcp_json() {
    let project = seeded();
    project.write(
        ".ai/src/mcp/claude.json",
        "{\"mcpServers\":{\"shared\":{\"command\":\"x\"}}}\n",
    );
    project.write(
        ".ai/src/mcp/cursor.json",
        "{\"mcpServers\":{\"shared\":{\"command\":\"x\"}}}\n",
    );

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/mcp.json"));
    assert!(project.read(".ai/src/mcp.json").contains("\"shared\""));
    assert!(!project.exists(".ai/src/mcp/claude.json"));
    assert!(!project.exists(".ai/src/mcp/cursor.json"));
    assert!(!project.exists(".ai/src/mcp"));
}

#[test]
fn migrate_apply_migrates_mcp_per_tool_when_files_differ() {
    let project = seeded();
    project.write(".ai/src/mcp/claude.json", "{\"m\":\"A\"}\n");
    project.write(".ai/src/mcp/cursor.json", "{\"m\":\"B\"}\n");

    project
        .exuno()
        .args(["migrate", "--apply"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/tools/claude/mcp.json"));
    assert!(project.exists(".ai/src/tools/cursor/mcp.json"));
    assert!(
        project
            .read(".ai/src/tools/claude/mcp.json")
            .contains("\"m\":\"A\"")
    );
    assert!(
        project
            .read(".ai/src/tools/cursor/mcp.json")
            .contains("\"m\":\"B\"")
    );
    assert!(!project.exists(".ai/src/mcp.json"));
}

#[test]
fn migrate_apply_skips_collisions_without_overwriting_target() {
    let project = seeded();
    project.write(".ai/src/hooks/cursor.json", "{\"m\":\"LEGACY\"}\n");
    project.write(".ai/src/tools/cursor/hooks.json", "{\"m\":\"EXISTING\"}\n");

    project
        .exuno()
        .args(["migrate", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped"));

    assert!(
        project
            .read(".ai/src/tools/cursor/hooks.json")
            .contains("\"m\":\"EXISTING\"")
    );
    assert!(project.exists(".ai/src/hooks/cursor.json"));
}

#[test]
fn doctor_hint_points_at_migrate_apply_when_legacy_files_exist() {
    let project = seeded();
    project.write(".ai/src/hooks/cursor.json", "{}\n");

    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("exuno migrate --apply"));
}

#[test]
fn migrate_rejects_unknown_flag() {
    seeded()
        .exuno()
        .args(["migrate", "--bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown flag"));
}

#[test]
fn migrate_help_prints_usage() {
    seeded()
        .exuno()
        .args(["migrate", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("latest documented format"))
        .stdout(predicate::str::contains("--legacy"))
        .stdout(predicate::str::contains("--apply"));
}

#[test]
fn migrate_legacy_help_documents_the_legacy_route() {
    seeded()
        .exuno()
        .args(["migrate", "--legacy", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "\n  USAGE\n    exuno migrate\n    exuno migrate --legacy [--apply] [--yes]\n",
        ))
        .stdout(predicate::str::contains("moves legacy flat-layout"));
}

// ── Legacy pre-v0.6 .agent/ (singular) directory removal ──────────────────

#[test]
fn migrate_legacy_dry_run_lists_legacy_agent_dir_contents() {
    let project = seeded();
    project.write(".agent/rules/.keep", "");
    project.write(".agent/skills/.keep", "");
    project.write(".agent/AGENTS.md", "legacy AGENTS\n");

    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy pre-v0.6"))
        .stdout(predicate::str::contains(".agent/"))
        .stdout(predicate::str::contains("AGENTS.md"));

    assert!(project.join(".agent").is_dir());
}

#[test]
fn migrate_apply_yes_removes_legacy_agent_dir() {
    let project = seeded();
    project.write(".agent/rules/.keep", "");
    project.write(".agent/AGENTS.md", "legacy\n");

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed .agent/"));

    assert!(!project.join(".agent").exists());
}

#[test]
fn migrate_apply_without_yes_and_without_tty_leaves_agent_dir_in_place() {
    let project = seeded();
    project.write(".agent/AGENTS.md", "legacy\n");

    // assert_cmd spawns without a controlling TTY, matching bats' non-interactive `run`.
    project
        .exuno()
        .args(["migrate", "--apply"])
        .assert()
        .success()
        .stdout(predicate::str::contains("non-interactive"));

    assert!(project.join(".agent").is_dir());
}

#[test]
fn migrate_flags_agent_dir_even_when_antigravity_is_enabled() {
    let project = seeded();
    project.enable_tools(&["antigravity"]);
    project.write(".agent/rules/.keep", "");
    project.write(".agent/AGENTS.md", "stale\n");

    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy pre-v0.6"));

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("removed .agent/"));

    assert!(!project.join(".agent").exists());
}

#[test]
fn migrate_detects_agent_dir_even_alongside_flat_layout_overrides() {
    let project = seeded();
    project.write(".agent/AGENTS.md", "legacy\n");
    project.write(".ai/src/hooks/cursor.json", "{}\n");

    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy pre-v0.6"))
        .stdout(predicate::str::contains("Planned moves"));
}

#[test]
fn migrate_apply_moves_overrides_into_the_source_tools_directory() {
    let project = seeded();
    project.write(".ai/src/hooks/cursor.json", "{}\n");
    std::fs::create_dir_all(project.join("catalog")).unwrap();
    // Overwrite wholesale, as the bats fixture does, so the sole `source:`
    // block is the one under test rather than the one `init` already wrote.
    project.write(
        ".ai/agent_sync.yaml",
        "format: 2\ntools:\n  enabled: []\nsource:\n  tools: \"catalog\"\n",
    );

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();

    assert!(project.exists("catalog/cursor/hooks.json"));
    assert!(!project.exists(".ai/src/tools/cursor/hooks.json"));
}

// `migrate --apply refuses a source.tools outside the project before
// changing anything` is covered by the unit test
// `a_json_mcp_set_with_another_config_moves_per_tool_and_an_outside_catalog_is_refused`
// in `src/cli/migrate.rs`, which exercises the outside-project refusal
// directly against `migrate()`.

#[test]
fn migrate_apply_keeps_a_non_json_mcp_override_next_to_identical_json_ones() {
    let project = seeded();
    project.write(".ai/src/mcp/claude.json", "{\"mcpServers\": {}}\n");
    project.write(".ai/src/mcp/cursor.json", "{\"mcpServers\": {}}\n");
    project.write(".ai/src/mcp/codex.toml", "[mcp_servers]\n");

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();

    assert!(
        project
            .read(".ai/src/tools/codex/mcp.toml")
            .contains("mcp_servers")
    );
    assert!(!project.exists(".ai/src/mcp.json"));
}

// `en_US.UTF-8` locale-dependent byte-order case: `migrate --legacy lists
// legacy files in byte order whatever the locale`. The engine sorts with a
// plain Rust `Vec<String>::sort` (byte order, locale-independent) rather
// than shelling out to a locale-sensitive `sort`, so there is no locale
// dependent behavior left to gate — `migrate_legacy_dry_run_shows_planned_moves_but_does_not_touch_files`
// and `migrate_apply_moves_hooks_and_settings_to_per_tool_dirs` above already
// exercise the same sorted listing.
