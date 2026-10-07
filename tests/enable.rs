//! `tests/enable.bats`: `exuno enable` / `disable`.

mod common;

use common::Project;
use predicates::prelude::*;

#[test]
fn enable_adds_tool_to_tools_enabled() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .arg("enable")
        .arg("claude")
        .assert()
        .success()
        .stdout(predicate::str::contains("Enabled 1 tool(s)"));
    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|line| line == "    - claude")
    );
}

#[test]
fn enable_multiple_tools_at_once() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude", "cursor"])
        .assert()
        .success();
    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|line| line == "    - claude"));
    assert!(config.lines().any(|line| line == "    - cursor"));
}

#[test]
fn enable_is_idempotent_no_duplicates() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    let count = project
        .read(".ai/agent_sync.yaml")
        .lines()
        .filter(|line| *line == "    - claude")
        .count();
    assert_eq!(count, 1);
}

#[test]
fn enable_unknown_tool_shows_warning() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "bogus_tool_xyz"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Unknown tool"));
}

#[test]
fn disable_removes_tool_from_tools_enabled() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude", "cursor"])
        .assert()
        .success();
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success();
    let config = project.read(".ai/agent_sync.yaml");
    assert!(!config.lines().any(|line| line == "    - claude"));
    assert!(config.lines().any(|line| line == "    - cursor"));
}

#[test]
fn disable_unknown_tool_is_a_no_op() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("No matching tools"));
}

#[test]
fn enable_with_no_args_fails_with_usage() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .arg("enable")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Error"));
}

#[test]
fn enable_scaffolds_per_tool_payload_dir_by_default_non_tty() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edit settings:"))
        .stdout(predicate::str::contains(
            ".ai/src/tools/claude/settings.json",
        ))
        // Shared MCP not configured yet -> hint points at add mcp, not a phantom path.
        .stdout(predicate::str::contains("exuno add mcp"));
    assert!(project.exists(".ai/src/tools/claude/settings.json"));
}

#[test]
fn enable_mcp_line_shows_shared_file_once_ai_src_mcp_json_exists() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edit mcp:"))
        .stdout(predicate::str::contains(".ai/src/mcp.json"))
        .stdout(predicate::str::contains("(shared)"));
}

#[test]
fn enable_no_scaffold_skips_per_tool_dir_but_still_prints_hints() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edit settings:").not())
        .stdout(predicate::str::contains("exuno customize claude settings"));
    assert!(!project.join(".ai/src/tools/claude").is_dir());
}

#[test]
fn enable_is_idempotent_for_scaffolded_files() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    project.write(".ai/src/tools/claude/settings.json", "{\"custom\": true}\n");
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/tools/claude/settings.json")
            .contains("\"custom\": true")
    );
}

#[test]
fn enable_scaffolds_hooks_when_tool_has_hooks_base() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "windsurf"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Edit hooks:"));
    assert!(project.exists(".ai/src/tools/windsurf/hooks.json"));
}

#[test]
fn enable_respects_legacy_flat_layout_overrides_no_shadow() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/settings/claude.json", "{\"legacy\": true}\n");
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    assert!(!project.join(".ai/src/tools/claude/settings.json").exists());
    assert!(
        project
            .read(".ai/src/settings/claude.json")
            .contains("\"legacy\": true")
    );
}

#[test]
fn disable_leaves_other_lists_that_name_the_tool_alone() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude", "cursor"])
        .assert()
        .success();
    project.append(
        ".ai/agent_sync.yaml",
        "profiles:\n  hub:\n    tools:\n      - claude\n",
    );
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success();
    let config = project.read(".ai/agent_sync.yaml");
    assert!(!config.lines().any(|line| line == "    - claude"));
    assert!(config.lines().any(|line| line == "      - claude"));
}

#[test]
fn disable_removes_a_tool_from_an_inline_tools_enabled_list() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/agent_sync.yaml",
        "tools:\n  enabled: [claude, cursor]\n",
    );
    project
        .exuno()
        .args(["disable", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Claude Code (claude)"));
    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|line| line == "  enabled: [cursor]"));
}

#[test]
fn enable_and_disable_write_the_config_agentsync_config_path_names() {
    let project = Project::seeded(&[]);
    let before = project.read(".ai/agent_sync.yaml");
    project.write("selected.yaml", "tools:\n  enabled: []\n");
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .success();
    assert!(project.read("selected.yaml").contains("cursor"));
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .args(["disable", "cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Disabled 1 tool(s)"));
    assert!(!project.read("selected.yaml").contains("cursor"));
    assert_eq!(project.read(".ai/agent_sync.yaml"), before);
}

#[test]
fn enable_writes_the_config_exuno_config_path_names_over_the_legacy_one() {
    let project = Project::seeded(&[]);
    project.write("new.yaml", "tools:\n  enabled: []\n");
    project.write("old.yaml", "tools:\n  enabled: []\n");
    project
        .exuno()
        .env("EXUNO_CONFIG_PATH", "new.yaml")
        .env("AGENTSYNC_CONFIG_PATH", "old.yaml")
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .success();
    assert!(project.read("new.yaml").contains("cursor"));
    assert_eq!(project.read("old.yaml"), "tools:\n  enabled: []\n");
}

#[test]
fn disable_without_a_config_creates_none() {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Agent\n");
    project.write(".ai/src/tools/kimi.yaml", "enabled: true\n");
    project
        .exuno()
        .args(["disable", "kimi"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Disabled 1 tool(s)"));
    assert_eq!(project.read(".ai/src/tools/kimi.yaml"), "enabled: false\n");
    assert!(!project.exists(".ai/agent_sync.yaml"));
}

#[test]
fn enable_edits_an_existing_exuno_yaml_and_creates_no_legacy_config() {
    let project = Project::seeded(&[]);
    std::fs::rename(
        project.join(".ai/agent_sync.yaml"),
        project.join(".ai/exuno.yaml"),
    )
    .unwrap();
    project
        .exuno()
        .args(["enable", "cursor", "--no-scaffold"])
        .assert()
        .success();
    assert!(project.read(".ai/exuno.yaml").contains("cursor"));
    assert!(!project.exists(".ai/agent_sync.yaml"));
}
