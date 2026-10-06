//! `tests/resource_resolver.bats`: `resolve_payload_source` — hooks/mcp/
//! settings resolution order:
//!   1. Project override at .ai/src/<resource>/<tool>.<ext>
//!   2. Fallback to <install-dir>/lib/templates/<resource>/<tool>.<ext>
//!   3. Nothing found → sync skips silently.

mod common;

use common::Project;
use predicates::prelude::*;

fn init(project: &Project) {
    project
        .exuno()
        .args(["init", "--no-detect"])
        .assert()
        .success();
}

#[test]
fn sync_falls_back_to_base_template_when_override_is_absent() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    // No .ai/src/hooks/cursor.json override exists.
    assert!(!project.exists(".ai/src/hooks/cursor.json"));

    project.exuno().arg("sync").assert().success();

    // Base template was used — destination file appears.
    assert!(project.exists(".cursor/hooks.json"));
    assert!(project.exists(".cursor/mcp.json"));
}

#[test]
fn project_override_wins_over_base_template() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(
        ".ai/src/hooks/cursor.json",
        "{\"marker\":\"USER_OVERRIDE\"}\n",
    );

    project.exuno().arg("sync").assert().success();

    assert!(project.read(".cursor/hooks.json").contains("USER_OVERRIDE"));
}

#[test]
fn no_override_and_no_base_template_sync_skips_silently() {
    let project = Project::empty();
    init(&project);
    // Claude has no hooks template — neither override nor base for hooks.
    project.enable_tools(&["claude"]);

    project.exuno().arg("sync").assert().success();

    // Claude's settings and .mcp.json come from base — should exist.
    assert!(project.exists(".claude/settings.json"));
    assert!(project.exists(".mcp.json"));
    // But no hooks dest is declared for Claude; nothing to create.
    assert!(!project.exists(".claude/hooks.json"));
}

#[test]
fn base_fallback_works_for_settings_across_template_types_json_toml() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["gemini", "codex"]);

    project.exuno().arg("sync").assert().success();

    assert!(project.exists(".gemini/settings.json")); // json base
    assert!(project.exists(".codex/config.toml")); // toml base
}

#[test]
fn removing_an_override_after_sync_restores_base_on_next_sync() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(
        ".ai/src/hooks/cursor.json",
        "{\"marker\":\"USER_OVERRIDE\"}\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.read(".cursor/hooks.json").contains("USER_OVERRIDE"));

    std::fs::remove_file(project.join(".ai/src/hooks/cursor.json")).unwrap();
    project.exuno().arg("sync").assert().success();

    // Base content replaced the override.
    assert!(project.exists(".cursor/hooks.json"));
    assert!(!project.read(".cursor/hooks.json").contains("USER_OVERRIDE"));
}

// ── Phase 7: per-tool override directory ────────────────────────────────────

#[test]
fn new_per_tool_dir_override_is_used_by_sync() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(
        ".ai/src/tools/cursor/hooks.json",
        "{\"marker\":\"PER_TOOL_DIR\"}\n",
    );

    project.exuno().arg("sync").assert().success();

    assert!(project.read(".cursor/hooks.json").contains("PER_TOOL_DIR"));
}

#[test]
fn new_layout_wins_over_legacy_flat_layout_when_both_exist() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(".ai/src/hooks/cursor.json", "{\"marker\":\"LEGACY\"}\n");
    project.write(
        ".ai/src/tools/cursor/hooks.json",
        "{\"marker\":\"CANONICAL\"}\n",
    );

    project.exuno().arg("sync").assert().success();

    let hooks = project.read(".cursor/hooks.json");
    assert!(hooks.contains("CANONICAL"));
    assert!(!hooks.contains("LEGACY"));
}

#[test]
fn legacy_flat_override_emits_deprecation_warning() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(".ai/src/hooks/cursor.json", "{\"marker\":\"LEGACY\"}\n");

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Legacy payload override layout"));

    // But the override still takes effect.
    assert!(project.read(".cursor/hooks.json").contains("LEGACY"));
}

// ── Phase 8: shared MCP (.ai/src/mcp.json) ──────────────────────────────────

#[test]
fn shared_mcp_json_propagates_to_every_enabled_tool() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude", "cursor"]);

    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"shared\":{\"command\":\"shared-mcp\"}}}\n",
    );

    project.exuno().arg("sync").assert().success();

    // Claude's dest is repo-root .mcp.json; Cursor's is .cursor/mcp.json.
    assert!(project.read(".mcp.json").contains("shared-mcp"));
    assert!(project.read(".cursor/mcp.json").contains("shared-mcp"));
}

#[test]
fn per_tool_mcp_override_wins_over_shared_source() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude", "cursor"]);

    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"shared\":{\"command\":\"shared-mcp\"}}}\n",
    );
    project.write(
        ".ai/src/tools/cursor/mcp.json",
        "{\"mcpServers\":{\"cursor_only\":{\"command\":\"cursor-mcp\"}}}\n",
    );

    project.exuno().arg("sync").assert().success();

    // Cursor takes the override; claude keeps the shared source.
    let cursor_mcp = project.read(".cursor/mcp.json");
    assert!(cursor_mcp.contains("cursor_only"));
    assert!(!cursor_mcp.contains("shared-mcp"));
    assert!(project.read(".mcp.json").contains("shared-mcp"));
}

#[test]
fn legacy_flat_mcp_override_wins_over_shared_source() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude", "cursor"]);

    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"shared\":{\"command\":\"shared-mcp\"}}}\n",
    );
    project.write(
        ".ai/src/mcp/cursor.json",
        "{\"mcpServers\":{\"legacy\":{\"command\":\"legacy-mcp\"}}}\n",
    );

    project.exuno().arg("sync").assert().success();

    assert!(project.read(".cursor/mcp.json").contains("legacy-mcp"));
    assert!(project.read(".mcp.json").contains("shared-mcp"));
}

#[test]
fn sync_emits_source_label_line_for_mcp() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            ".ai/src/mcp.json → .cursor/mcp.json (shared)",
        ));
}

#[test]
fn sync_mcp_label_is_base_when_nothing_overrides() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["cursor"]);

    assert!(!project.exists(".ai/src/mcp.json"));

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "templates/mcp/cursor.json → .cursor/mcp.json (base)",
        ));
}

#[test]
fn opencode_hooks_fall_back_to_the_native_base_plugin() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["opencode"]);

    project.exuno().arg("sync").assert().success();

    assert!(project.exists(".opencode/plugins/agentsync.ts"));
    assert!(
        project
            .read(".opencode/plugins/agentsync.ts")
            .contains("AgentSyncHooks")
    );
}

#[test]
fn opencode_hook_override_wins_and_preserves_sibling_plugins() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["opencode"]);
    project.write(
        ".ai/src/tools/opencode/hooks.ts",
        "export const AgentSyncHooks = async () => ({ marker: \"override\" })\n",
    );
    project.write(
        ".opencode/plugins/user.ts",
        "export const UserPlugin = async () => ({})\n",
    );

    project.exuno().arg("sync").assert().success();

    assert!(
        project
            .read(".opencode/plugins/agentsync.ts")
            .contains("marker: \"override\"")
    );
    assert!(project.exists(".opencode/plugins/user.ts"));
}

#[test]
fn customize_creates_the_canonical_opencode_hook_override() {
    let project = Project::empty();
    init(&project);

    project
        .exuno()
        .args(["customize", "opencode", "hooks", "--yes"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/tools/opencode/hooks.ts"));
    assert!(
        project
            .read(".ai/src/tools/opencode/hooks.ts")
            .contains("AgentSyncHooks")
    );
}

#[test]
fn skills_exclude_accepts_a_block_style_list() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude"]);

    project.write(".ai/src/skills/keepme/SKILL.md", "k\n");
    project.write(".ai/src/skills/dropme/SKILL.md", "d\n");

    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  skills:\n    exclude:\n      - dropme\n",
    );

    project.exuno().arg("sync").assert().success();
    assert!(project.join(".claude/skills/keepme").is_dir());
    assert!(!project.join(".claude/skills/dropme").is_dir());
}

#[test]
fn skills_exclude_accepts_an_inline_list() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude"]);

    project.write(".ai/src/skills/keepme/SKILL.md", "k\n");
    project.write(".ai/src/skills/dropone/SKILL.md", "1\n");
    project.write(".ai/src/skills/droptwo/SKILL.md", "2\n");

    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  skills:\n    exclude: [dropone, droptwo]\n",
    );

    project.exuno().arg("sync").assert().success();
    assert!(project.join(".claude/skills/keepme").is_dir());
    assert!(!project.join(".claude/skills/dropone").is_dir());
    assert!(!project.join(".claude/skills/droptwo").is_dir());
}

#[test]
fn skills_exclude_still_accepts_a_plain_scalar_string() {
    let project = Project::empty();
    init(&project);
    project.enable_tools(&["claude"]);

    project.write(".ai/src/skills/keepme/SKILL.md", "k\n");
    project.write(".ai/src/skills/dropme/SKILL.md", "d\n");

    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  skills:\n    exclude: \"dropme\"\n",
    );

    project.exuno().arg("sync").assert().success();
    assert!(project.join(".claude/skills/keepme").is_dir());
    assert!(!project.join(".claude/skills/dropme").is_dir());
}
