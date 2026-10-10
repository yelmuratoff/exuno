//! `tests/adopt.bats`: `exuno adopt` — promote a manual edit in a
//! generated file back into `.ai/src/`, refuse transformed targets, keep the
//! next sync drift-free.

mod common;

use common::Project;
use predicates::prelude::*;

/// `seed_project` + `enable_tools` + `sync`: every case starts from a synced
/// tree for the tools it needs.
fn synced_project(tools: &[&str]) -> Project {
    let project = Project::seeded(&[]);
    project.enable_tools(tools);
    project.exuno().arg("sync").assert().success();
    project
}

/// The first regular file directly under `dir` (one level), as project-relative
/// text — a stand-in for the bats `find ... | head -1` fixtures.
fn first_file_in(project: &Project, dir: &str) -> String {
    let entries = std::fs::read_dir(project.join(dir)).unwrap();
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    format!("{dir}/{}", names.first().expect("no file found"))
}

/// The first `SKILL.md` one directory below `dir`, as project-relative text.
fn first_skill_md(project: &Project, dir: &str) -> String {
    let entries = std::fs::read_dir(project.join(dir)).unwrap();
    let mut names: Vec<String> = entries
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    format!(
        "{dir}/{}/SKILL.md",
        names.first().expect("no skill directory found")
    )
}

// ── 1:1 adoptable targets ────────────────────────────────────────────────────

#[test]
fn adopt_rule_file_claude_no_header_round_trips_into_source() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Manual addition\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/rules/core.md"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("Manual addition")
    );
}

#[test]
fn adopt_subsequent_sync_is_drift_free_after_rule_adoption() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Adopted\n");
    project
        .exuno()
        .args(["adopt", "--yes", ".claude/rules/core.md"])
        .assert()
        .success();

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Manual edits detected").not())
        .stderr(predicate::str::contains("Manual edits detected").not());
}

#[test]
fn adopt_agents_md_round_trips_into_ai_src_agents_md() {
    let project = synced_project(&["claude"]);
    project.append("CLAUDE.md", "## Custom appendix\n");

    project
        .exuno()
        .args(["adopt", "--yes", "CLAUDE.md"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/AGENTS.md")
            .contains("Custom appendix")
    );
}

#[test]
fn adopt_refuses_minimax_agents_with_generated_rule_references() {
    let project = synced_project(&["minimax"]);
    let source = project.read(".ai/src/AGENTS.md");
    project.append("AGENTS.md", "\nManual edit\n");

    project
        .exuno()
        .args(["adopt", "--yes", "AGENTS.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("generated content"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), source);
}

#[test]
fn adopt_refuses_shared_agents_when_minimax_is_enabled_with_codex() {
    let project = synced_project(&["codex", "minimax"]);
    let source = project.read(".ai/src/AGENTS.md");
    project.append("AGENTS.md", "\nManual edit\n");

    project
        .exuno()
        .args(["adopt", "--yes", "AGENTS.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("minimax has generated content"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), source);
}

#[test]
fn adopt_allows_agents_owned_by_cursor_when_minimax_agents_are_disabled() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["cursor", "minimax"]);
    project.write(
        ".ai/src/tools/minimax.yaml",
        "targets:\n  agents:\n    enabled: false\n",
    );
    project.exuno().arg("sync").assert().success();
    project.append("AGENTS.md", "\nManual edit\n");

    project
        .exuno()
        .args(["adopt", "--yes", "AGENTS.md"])
        .assert()
        .success();
    assert!(project.read(".ai/src/AGENTS.md").contains("Manual edit"));
}

#[test]
fn adopt_settings_scaffolds_canonical_override_path() {
    let project = synced_project(&["claude"]);
    project.write(".claude/settings.json", "{\"manualEdit\": true}\n");
    assert!(!project.exists(".ai/src/tools/claude/settings.json"));

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/settings.json"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/tools/claude/settings.json"));
    assert!(
        project
            .read(".ai/src/tools/claude/settings.json")
            .contains("manualEdit")
    );
}

#[test]
fn adopt_refuses_composed_opencode_settings() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["opencode"]);
    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"x\":{\"command\":\"x\"}}}\n",
    );
    project.exuno().arg("sync").assert().success();
    project.append("opencode.json", "\n");

    project
        .exuno()
        .args(["adopt", "--yes", "opencode.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("multi-source"))
        .stderr(predicate::str::contains(".ai/src/mcp.json"))
        .stderr(predicate::str::contains("settings"));
}

#[test]
fn adopt_refuses_settings_that_carry_skills_hide() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.append(".ai/exuno.yaml", "skills:\n  hide: [bloc]\n");
    project.exuno().arg("sync").assert().success();
    project.append(".claude/settings.json", "\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/settings.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            ".claude/settings.json also carries skills.hide from the project config",
        ));
    assert!(!project.exists(".ai/src/tools/claude/settings.json"));
}

#[test]
fn adopt_refuses_a_keyed_settings_key_skills_hide_writes() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.append(".ai/exuno.yaml", "skills:\n  hide: [bloc]\n");
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  settings:\n    ownership: keys\n",
    );
    project
        .exuno()
        .args(["customize", "claude", "settings"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    let live = project
        .read(".claude/settings.json")
        .replace("\"bloc\": \"off\"", "\"bloc\": \"on\"");
    project.write(".claude/settings.json", &live);

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/settings.json"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "skillOverrides.bloc comes from skills.hide in the project config",
        ));
}

#[test]
fn adopt_opencode_hooks_remain_one_to_one() {
    let project = synced_project(&["opencode"]);
    project.append(".opencode/plugins/agentsync.ts", "\n// user hook\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".opencode/plugins/agentsync.ts"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/tools/opencode/hooks.ts")
            .contains("user hook")
    );
}

#[test]
fn adopt_skill_file_round_trips() {
    let project = synced_project(&["claude"]);
    let skill_file = first_skill_md(&project, ".claude/skills");
    project.append(&skill_file, "## Skill addition\n");

    project
        .exuno()
        .args(["adopt", "--yes", &skill_file])
        .assert()
        .success();
}

#[test]
fn adopt_returns_an_edit_to_the_skill_category_it_came_from() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.write(
        ".ai/src/skills/flutter/bloc/SKILL.md",
        "---\nname: bloc\ndescription: Bloc\n---\n",
    );
    project.write(".ai/src/skills/flutter/bloc/references/r.md", "r\n");
    project.exuno().arg("sync").assert().success();
    project.append(".claude/skills/bloc/references/r.md", "edited\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/skills/bloc/references/r.md"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/skills/flutter/bloc/references/r.md"),
        "r\nedited\n"
    );
    assert!(!project.exists(".ai/src/skills/bloc"));
    project.exuno().arg("check").assert().success();
}

// ── Refusals ─────────────────────────────────────────────────────────────────

#[test]
fn adopt_refuses_cursor_rule_header_injection() {
    let project = synced_project(&["cursor"]);
    project.append(".cursor/rules/core.mdc", "extra\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".cursor/rules/core.mdc"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("frontmatter header"));
}

#[test]
fn adopt_refuses_a_merged_rules_file() {
    let project = synced_project(&["zed"]);
    project.append(".rules", "extra\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".rules"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "zed merges rules into a single file. Edit the source rules in",
        ));
}

#[test]
fn adopt_refuses_codex_toml_subagent() {
    let project = synced_project(&["codex"]);
    let toml_file = first_file_in(&project, ".codex/agents");
    project.append(&toml_file, "# edit\n");

    project
        .exuno()
        .args(["adopt", "--yes", &toml_file])
        .assert()
        .failure()
        .stderr(predicate::str::contains("toml"));
}

#[test]
fn adopt_refuses_converted_opencode_subagent() {
    let project = synced_project(&["opencode"]);
    let agent_file = first_file_in(&project, ".opencode/agents");
    project.append(&agent_file, "# edit\n");

    project
        .exuno()
        .args(["adopt", "--yes", &agent_file])
        .assert()
        .failure()
        .stderr(predicate::str::contains("opencode_md"));
}

#[test]
fn adopt_refuses_unknown_destination() {
    let project = synced_project(&["claude"]);
    project.write("README.md", "hello\n");

    project
        .exuno()
        .args(["adopt", "--yes", "README.md"])
        .assert()
        .failure();
}

#[test]
fn adopt_refuses_path_outside_repo() {
    let project = synced_project(&["claude"]);

    project
        .exuno()
        .args(["adopt", "--yes", "/etc/hosts"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("outside the project"));
}

#[test]
fn adopt_works_before_the_first_sync_when_nothing_is_tracked_yet() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    // No sync yet, no manifest — a project's own CLAUDE.md from before Exuno.
    project.write("CLAUDE.md", "# Pre-existing\n");

    project
        .exuno()
        .args(["adopt", "--yes", "CLAUDE.md"])
        .assert()
        .success();
    assert!(project.read(".ai/src/AGENTS.md").contains("Pre-existing"));
    assert!(!project.exists(".ai/.sync-manifest"));
}

#[test]
fn adopt_refuses_untracked_file_not_in_manifest() {
    let project = synced_project(&["claude"]);
    // A file inside a dest dir that Exuno didn't produce.
    project.write(".claude/rules/extraneous.md", "rogue\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/rules/extraneous.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not tracked"));
}

// ── Dry-run safety ───────────────────────────────────────────────────────────

#[test]
fn adopt_dry_run_does_not_write_source() {
    let project = synced_project(&["claude"]);
    let before = project.sha256(".ai/src/rules/core.md");
    project.append(".claude/rules/core.md", "## Edit\n");

    project
        .exuno()
        .args(["adopt", "--dry-run", ".claude/rules/core.md"])
        .assert()
        .success();
    assert_eq!(project.sha256(".ai/src/rules/core.md"), before);
}

#[test]
fn adopt_dry_run_does_not_update_manifest() {
    let project = synced_project(&["claude"]);
    let before = project.sha256(".ai/.sync-manifest");
    project.append(".claude/rules/core.md", "## Edit\n");

    project
        .exuno()
        .args(["adopt", "--dry-run", ".claude/rules/core.md"])
        .assert()
        .success();
    assert_eq!(project.sha256(".ai/.sync-manifest"), before);
}

#[test]
fn adopt_refuses_non_interactive_without_yes() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Edit\n");

    project
        .exuno()
        .args(["adopt", ".claude/rules/core.md"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-interactively"));
}

// ── No-op already-in-sync ────────────────────────────────────────────────────

#[test]
fn adopt_no_op_when_dest_already_matches_source() {
    let project = synced_project(&["claude"]);

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/rules/core.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("already matches"));
}

// ── Batch mode (--all) ───────────────────────────────────────────────────────

#[test]
fn adopt_all_promotes_every_drifted_1_to_1_output() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Rule edit\n");
    project.append("CLAUDE.md", "## Agents edit\n");

    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success();
    assert!(project.read(".ai/src/rules/core.md").contains("Rule edit"));
    assert!(project.read(".ai/src/AGENTS.md").contains("Agents edit"));
}

#[test]
fn adopt_all_names_a_source_two_outputs_share_once() {
    let project = synced_project(&["claude", "cursor"]);
    project.append(".claude/skills/exuno/SKILL.md", "## Same edit\n");
    project.append(".cursor/skills/exuno/SKILL.md", "## Same edit\n");

    let output = project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Adopted 1 file(s)"))
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    assert_eq!(stdout.matches("✓ adopted").count(), 1, "{stdout}");
    assert!(
        project
            .read(".ai/src/skills/exuno/SKILL.md")
            .contains("Same edit")
    );
}

#[test]
fn adopt_all_no_op_when_nothing_drifted() {
    let project = synced_project(&["claude"]);

    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to adopt"));
}

#[test]
fn adopt_all_adopts_adoptable_output_but_skips_refused_target() {
    let project = synced_project(&["claude", "cursor"]);
    project.append(".claude/rules/core.md", "## Adoptable\n");
    project.append(".cursor/rules/core.mdc", "extra\n");

    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skipped"))
        .stdout(predicate::str::contains(".cursor/rules/core.mdc"));
    assert!(project.read(".ai/src/rules/core.md").contains("Adoptable"));
}

#[test]
fn adopt_all_skips_same_source_conflicts_without_writing() {
    let project = synced_project(&["claude", "gemini"]);
    let before = project.sha256(".ai/src/AGENTS.md");
    project.append("CLAUDE.md", "## Claude only\n");
    project.append("GEMINI.md", "## Gemini only\n");

    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("multiple edited outputs map to"));
    assert_eq!(project.sha256(".ai/src/AGENTS.md"), before);
}

#[test]
fn adopt_all_dry_run_writes_nothing() {
    let project = synced_project(&["claude"]);
    let before = project.sha256(".ai/src/rules/core.md");
    project.append(".claude/rules/core.md", "## Edit\n");

    project
        .exuno()
        .args(["adopt", "--all", "--dry-run"])
        .assert()
        .success();
    assert_eq!(project.sha256(".ai/src/rules/core.md"), before);
}

#[test]
fn adopt_all_rejects_a_dest_file_argument() {
    let project = synced_project(&["claude"]);

    project
        .exuno()
        .args(["adopt", "--all", "CLAUDE.md"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains("takes no"));
}

#[test]
fn adopt_all_refuses_non_interactive_without_yes() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Edit\n");

    project
        .exuno()
        .args(["adopt", "--all"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("non-interactively"));
}

#[test]
fn adopt_all_subsequent_sync_is_drift_free() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Adopted\n");
    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success();

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Manual edits detected").not())
        .stderr(predicate::str::contains("Manual edits detected").not());
}

#[test]
fn adopt_writes_an_edited_rule_into_the_source_rules_directory_sync_reads() {
    let project = Project::seeded(&[]);
    project.write("docs/rules/team.md", "# Team\n");
    project.write(
        ".ai/exuno.yaml",
        "tools:\n  enabled:\n    - claude\nsource:\n  rules: \"docs/rules\"\n",
    );
    project.exuno().arg("sync").assert().success();
    project.append(".claude/rules/team.md", "## Edited\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".claude/rules/team.md"])
        .assert()
        .success();
    assert!(project.read("docs/rules/team.md").contains("Edited"));
    assert!(!project.exists(".ai/src/rules/team.md"));
}

#[test]
fn adopt_a_cline_workflow_goes_to_commands() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["cline"]);
    project.write(".ai/src/commands/go.md", "---\ndescription: Go\n---\nGo.\n");
    project.exuno().arg("sync").assert().success();
    project.append(".cline/workflows/go.md", "Edited.\n");

    project
        .exuno()
        .args(["adopt", "--yes", ".cline/workflows/go.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("resource: commands"));
    assert!(project.read(".ai/src/commands/go.md").contains("Edited."));
    assert!(!project.exists(".ai/src/rules/workflows"));
}

#[test]
fn adopt_the_plans_diff_names_source_and_destination_by_project_path() {
    let project = synced_project(&["claude"]);
    project.append(".claude/rules/core.md", "## Manual addition\n");

    project
        .exuno()
        .args(["adopt", "--dry-run", ".claude/rules/core.md"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "    --- .ai/src/rules/core.md\n    +++ .claude/rules/core.md\n",
        ));
}

const CODEX_CONFIG: &str = ".codex/config.toml";
const CODEX_SETTINGS: &str = ".ai/src/tools/codex/settings.toml";

/// A Codex project whose `config.toml` sync owns by key, synced once, with
/// state the Codex app wrote appended since.
fn keyed_codex_project() -> Project {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: keys\n",
    );
    project.write(
        CODEX_SETTINGS,
        "# mine\nmodel = \"gpt\"\neffort = \"low\"\n",
    );
    project.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers":{"dart":{"command":"dart"}}}"#,
    );
    project.exuno().arg("sync").assert().success();
    project.append(CODEX_CONFIG, "\n[projects.p]\ntrust_level = \"trusted\"\n");
    project
}

#[test]
fn adopt_moves_changed_owned_keys_into_codex_settings() {
    let project = keyed_codex_project();
    let edited = project
        .read(CODEX_CONFIG)
        .replace("model = \"gpt\"", "model = \"ui-pick\"")
        .replace("effort = \"low\"\n", "");
    project.write(CODEX_CONFIG, &edited);

    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .success()
        .stdout(predicate::str::contains("effort, model"));
    assert_eq!(
        project.read(CODEX_SETTINGS),
        "# mine\nmodel = \"ui-pick\"\n"
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.read(CODEX_CONFIG).contains("[projects.p]"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn adopt_ignores_keys_the_codex_app_owns() {
    let project = keyed_codex_project();
    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to adopt"));
    assert!(!project.read(CODEX_SETTINGS).contains("projects"));
}

#[test]
fn adopt_refuses_an_owned_mcp_server() {
    let project = keyed_codex_project();
    let edited = project
        .read(CODEX_CONFIG)
        .replace("command = \"dart\"", "command = \"dart2\"");
    project.write(CODEX_CONFIG, &edited);

    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "mcp_servers.dart comes from the MCP source",
        ));
    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "mcp_servers.dart comes from the MCP source",
        ));
}

#[test]
fn adopt_all_moves_changed_owned_keys_into_codex_settings() {
    let project = keyed_codex_project();
    let edited = project
        .read(CODEX_CONFIG)
        .replace("model = \"gpt\"", "model = \"ui-pick\"");
    project.write(CODEX_CONFIG, &edited);

    project
        .exuno()
        .args(["adopt", "--all", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "adopted .ai/src/tools/codex/settings.toml",
        ));
    assert_eq!(
        project.read(CODEX_SETTINGS),
        "# mine\nmodel = \"ui-pick\"\neffort = \"low\"\n"
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.read(CODEX_CONFIG).contains("[projects.p]"));
}

#[test]
fn adopt_takes_mcp_server_fields_the_settings_own() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: keys\n  mcp:\n    enabled: false\n",
    );
    project.write(CODEX_SETTINGS, "[mcp_servers.repl]\ncommand = \"repl\"\n");
    project.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers":{"shared":{"command":"x"}}}"#,
    );
    project.exuno().arg("sync").assert().success();
    let edited = project.read(CODEX_CONFIG).replace("\"repl\"", "\"repl2\"");
    project.write(CODEX_CONFIG, &edited);

    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .success();
    assert_eq!(
        project.read(CODEX_SETTINGS),
        "[mcp_servers.repl]\ncommand = \"repl2\"\n"
    );
}

#[test]
fn adopt_refuses_a_whole_file_copy_of_a_key_owned_config() {
    let project = keyed_codex_project();
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: file\n",
    );
    std::fs::remove_file(project.join(".ai/src/mcp.json")).unwrap();
    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            ".codex/config.toml is recorded as owned by key",
        ));
    assert!(!project.read(CODEX_SETTINGS).contains("projects"));
}

#[test]
fn adopt_refuses_to_create_a_settings_source_from_owned_keys() {
    let project = keyed_codex_project();
    std::fs::remove_file(project.join(CODEX_SETTINGS)).unwrap();
    let edited = project
        .read(CODEX_CONFIG)
        .replace("model = \"gpt\"", "model = \"ui-pick\"");
    project.write(CODEX_CONFIG, &edited);

    project
        .exuno()
        .args(["adopt", "--yes", CODEX_CONFIG])
        .assert()
        .failure()
        .stderr(predicate::str::contains("exuno customize codex settings"));
    assert!(!project.exists(CODEX_SETTINGS));
}
