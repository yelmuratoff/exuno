//! `tests/sync.bats`: `exuno sync` across every supported tool, driven
//! from one fixture project (a path-scoped rule, an explicit-only command).

mod common;

use common::Project;
use predicates::prelude::*;
use std::path::Path;

const ENABLED_TOOLS: &[&str] = &[
    "claude",
    "cursor",
    "copilot",
    "windsurf",
    "gemini",
    "codex",
    "amazonq",
    "zed",
    "junie",
    "antigravity",
    "kimi",
    "minimax",
    "opencode",
];

const SCOPED_FIXTURE_RULE: &str =
    "---\npaths:\n  - \"**/*.dart\"\n---\n\n# Scoped Fixture Rule\n\n- Body.\n";

fn explicit_only_command(disable_model_invocation: bool) -> String {
    if disable_model_invocation {
        "---\ndescription: Explicit-only fixture command\ndisable-model-invocation: true\n---\n\nBody.\n".to_string()
    } else {
        "---\ndescription: Explicit-only fixture command\n---\n\nBody.\n".to_string()
    }
}

/// `setup_file`: init, enable every tool the assertions cover, add the
/// path-scoped rule and explicit-only command fixtures, then sync once.
fn synced_project() -> Project {
    let project = Project::seeded(&["--outputs", "local"]);
    project.write(".gitignore", "node_modules/\n");
    project.enable_tools(ENABLED_TOOLS);
    project.write(".ai/src/rules/scoped-fixture.md", SCOPED_FIXTURE_RULE);
    project.write(
        ".ai/src/commands/explicit-only.md",
        &explicit_only_command(true),
    );
    project.exuno().arg("sync").assert().success();
    project
}

/// Sorted files directly under `dir` (relative to the project root) whose
/// extension matches — mirrors `ls dir/*.ext | head -1` for "first file"
/// assertions.
fn files_with_extension(project: &Project, dir: &str, ext: &str) -> Vec<std::path::PathBuf> {
    let mut files: Vec<_> = std::fs::read_dir(project.join(dir))
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().and_then(|e| e.to_str()) == Some(ext))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn first_file(project: &Project, dir: &str, ext: &str) -> std::path::PathBuf {
    files_with_extension(project, dir, ext)
        .into_iter()
        .next()
        .unwrap_or_else(|| panic!("no *.{ext} files in {dir}"))
}

fn contains(path: &Path, needle: &str) -> bool {
    std::fs::read_to_string(path)
        .map(|content| content.contains(needle))
        .unwrap_or(false)
}

// ── Claude Code ──────────────────────────────────────────────────────────

#[test]
fn sync_claude_claude_md_exists() {
    assert!(synced_project().exists("CLAUDE.md"));
}

#[test]
fn sync_claude_rules_exist() {
    let project = synced_project();
    assert!(project.join(".claude/rules").is_dir());
    assert!(!files_with_extension(&project, ".claude/rules", "md").is_empty());
}

#[test]
fn sync_claude_skills_exist() {
    assert!(synced_project().join(".claude/skills").is_dir());
}

#[test]
fn sync_claude_commands_exist() {
    assert!(synced_project().join(".claude/commands").is_dir());
}

#[test]
fn sync_claude_agents_exist() {
    assert!(synced_project().join(".claude/agents").is_dir());
}

#[test]
fn sync_claude_claude_md_contains_agents_md_content() {
    let project = synced_project();
    let agents_md = project.read(".ai/src/AGENTS.md");
    let first_heading = agents_md
        .lines()
        .find(|line| line.starts_with('#'))
        .unwrap();
    assert!(contains(&project.join("CLAUDE.md"), first_heading));
}

// ── Cursor ───────────────────────────────────────────────────────────────

#[test]
fn sync_cursor_agents_md_exists_at_root() {
    assert!(synced_project().exists("AGENTS.md"));
}

#[test]
fn sync_cursor_mdc_rules_exist() {
    let project = synced_project();
    assert!(!files_with_extension(&project, ".cursor/rules", "mdc").is_empty());
}

#[test]
fn sync_cursor_rules_have_globs_frontmatter() {
    let project = synced_project();
    let first_mdc = first_file(&project, ".cursor/rules", "mdc");
    assert!(contains(&first_mdc, "alwaysApply: true"));
}

// ── GitHub Copilot ───────────────────────────────────────────────────────

#[test]
fn sync_copilot_instructions_exist() {
    assert!(synced_project().exists(".github/copilot-instructions.md"));
}

#[test]
fn sync_copilot_instructions_md_rules_exist() {
    let project = synced_project();
    assert!(!files_with_extension(&project, ".github/instructions", "md").is_empty());
}

#[test]
fn sync_copilot_rules_have_applyto_frontmatter() {
    let project = synced_project();
    let first = first_file(&project, ".github/instructions", "md");
    assert!(contains(&first, "applyTo:"));
}

// ── Windsurf ─────────────────────────────────────────────────────────────

#[test]
fn sync_windsurf_agents_md_exists_at_root() {
    assert!(synced_project().exists("AGENTS.md"));
}

#[test]
fn sync_windsurf_rules_have_trigger_frontmatter() {
    let project = synced_project();
    let first = first_file(&project, ".devin/rules", "md");
    assert!(contains(&first, "trigger: always_on"));
}

// ── Gemini ───────────────────────────────────────────────────────────────

#[test]
fn sync_gemini_gemini_md_exists_at_root() {
    assert!(synced_project().exists("GEMINI.md"));
}

#[test]
fn sync_gemini_inlines_rules_into_gemini_md() {
    let project = synced_project();
    assert!(contains(&project.join("GEMINI.md"), "Rules"));
}

// ── Codex ────────────────────────────────────────────────────────────────

#[test]
fn sync_codex_agents_md_at_root_exists() {
    assert!(synced_project().exists("AGENTS.md"));
}

#[test]
fn sync_codex_rule_references_point_at_the_project_rules_directory() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    let home = tempfile::tempdir().unwrap();
    project
        .exuno()
        .env("HOME", home.path())
        .args(["sync", "--only", "codex"])
        .assert()
        .success();
    assert!(
        project
            .read("AGENTS.md")
            .contains("Find all rules in `.ai/src/rules/`.")
    );
}

#[test]
fn sync_codex_rule_references_point_at_the_home_rules_directory_when_the_project_is_home() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project
        .exuno()
        .env("HOME", project.path())
        .args(["sync", "--only", "codex"])
        .assert()
        .success();
    assert!(
        project
            .read("AGENTS.md")
            .contains("Find all rules in `~/.ai/src/rules/`.")
    );
}

#[test]
fn sync_codex_skills_directory_exists() {
    assert!(synced_project().join(".agents/skills").is_dir());
}

#[test]
fn sync_codex_commands_rendered_as_generated_skills_command() {
    let project = synced_project();
    assert!(project.join(".agents/skills/command-fix-issue").is_dir());
    assert!(project.join(".agents/skills/command-review").is_dir());
    assert!(project.exists(".agents/skills/command-fix-issue/SKILL.md"));
}

#[test]
fn sync_codex_generated_skill_carries_command_prefixed_name_and_copies_description() {
    let project = synced_project();
    let skill = project.read(".agents/skills/command-fix-issue/SKILL.md");
    assert!(skill.contains("name: \"command-fix-issue\""));
    assert!(skill.contains("Investigate and fix a GitHub issue"));
}

#[test]
fn sync_codex_generated_skill_strips_arguments_and_bang_slash_command_sugar() {
    let project = synced_project();
    let skill = project.read(".agents/skills/command-fix-issue/SKILL.md");
    assert!(!skill.contains("$ARGUMENTS"));
    assert!(!skill.contains("!`"));
}

#[test]
fn sync_codex_generated_skills_do_not_collide_with_native_skills() {
    let project = synced_project();
    // Native skill 'review' coexists with generated 'command-review'.
    assert!(project.join(".agents/skills/review").is_dir());
    assert!(project.join(".agents/skills/command-review").is_dir());
}

#[test]
fn sync_codex_generated_skill_opts_out_of_implicit_invocation_when_the_command_disables_model_invocation()
 {
    let project = synced_project();
    let openai_yaml = project.read(".agents/skills/command-explicit-only/agents/openai.yaml");
    assert!(openai_yaml.contains("  allow_implicit_invocation: false"));
    assert!(!project.exists(".agents/skills/command-fix-issue/agents/openai.yaml"));
}

#[test]
fn sync_codex_drops_the_openai_yaml_opt_out_once_the_command_allows_model_invocation_again() {
    let project = synced_project();
    project.write(
        ".ai/src/commands/explicit-only.md",
        &explicit_only_command(false),
    );
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists(".agents/skills/command-explicit-only/agents/openai.yaml"));

    project.write(
        ".ai/src/commands/explicit-only.md",
        &explicit_only_command(true),
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".agents/skills/command-explicit-only/agents/openai.yaml"));
}

#[test]
fn sync_codex_repeat_sync_is_idempotent_no_command_sweep() {
    let project = synced_project();
    project.exuno().arg("sync").assert().success();
    assert!(project.join(".agents/skills/command-fix-issue").is_dir());
    assert!(project.join(".agents/skills/command-review").is_dir());
}

// ── Kimi Code ────────────────────────────────────────────────────────────

#[test]
fn sync_kimi_code_emits_native_skills_and_command_skills() {
    let project = synced_project();
    assert!(project.exists(".kimi-code/AGENTS.md"));
    assert!(contains(&project.join(".kimi-code/AGENTS.md"), "## Rules"));
    assert!(project.join(".kimi-code/skills").is_dir());
    assert!(project.exists(".kimi-code/skills/command-review/SKILL.md"));
}

#[test]
fn sync_kimi_code_emits_project_mcp_config() {
    let project = synced_project();
    assert!(project.exists(".kimi-code/mcp.json"));
    assert!(contains(
        &project.join(".kimi-code/mcp.json"),
        "\"mcpServers\""
    ));
}

// ── OpenCode ─────────────────────────────────────────────────────────────

#[test]
fn sync_opencode_emits_native_skills_and_commands() {
    let project = synced_project();
    assert!(project.join(".opencode/skills").is_dir());
    assert!(project.exists(".opencode/commands/review.md"));
}

#[test]
fn sync_opencode_converts_portable_subagents_safely() {
    let project = synced_project();
    let agent = project.read(".opencode/agents/code-reviewer.md");
    assert!(agent.contains("mode: subagent"));
    assert!(agent.contains("  \"*\": deny"));
    assert!(agent.contains("  \"read\": allow"));
}

#[test]
fn sync_opencode_emits_project_settings() {
    let project = synced_project();
    assert!(project.exists("opencode.json"));
    assert!(contains(
        &project.join("opencode.json"),
        "https://opencode.ai/config.json"
    ));
}

#[test]
fn sync_opencode_emits_the_managed_native_plugin() {
    let project = synced_project();
    assert!(project.exists(".opencode/plugins/agentsync.ts"));
    assert!(contains(
        &project.join(".opencode/plugins/agentsync.ts"),
        "AgentSyncHooks"
    ));
}

// ── Hooks (per-tool) ─────────────────────────────────────────────────────

#[test]
fn sync_cursor_hooks_json_exists() {
    assert!(synced_project().exists(".cursor/hooks.json"));
}

#[test]
fn sync_codex_hooks_json_exists() {
    assert!(synced_project().exists(".codex/hooks.json"));
}

#[test]
fn sync_copilot_hooks_json_exists() {
    assert!(synced_project().exists(".github/hooks/hooks.json"));
}

#[test]
fn sync_windsurf_hooks_json_exists() {
    assert!(synced_project().exists(".devin/hooks.json"));
}

// ── MCP / settings (per-tool) ────────────────────────────────────────────

#[test]
fn sync_claude_mcp_json_exists() {
    assert!(synced_project().exists(".mcp.json"));
}

#[test]
fn sync_minimax_uses_project_agents_and_shared_mcp() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["minimax"]);
    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.com/mcp\"}}}\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.read("AGENTS.md").contains("## Rules"));
    assert_eq!(project.read(".mcp.json"), project.read(".ai/src/mcp.json"));
    project.exuno().arg("sync").assert().success();
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_rejects_different_mcp_sources_at_one_destination() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude", "minimax"]);
    project.write(
        ".ai/src/tools/minimax/mcp.json",
        "{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.com/mcp\"}}}\n",
    );
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "MCP destination .mcp.json is shared by claude",
        ));
    assert!(!project.exists(".mcp.json"));
}

#[test]
fn sync_claude_minimax_and_opencode_keep_their_mcp_outputs() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude", "minimax", "opencode"]);
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".mcp.json"), project.read(".ai/src/mcp.json"));
    assert!(project.exists("opencode.json"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_minimax_and_windsurf_preserve_rule_references() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["minimax", "windsurf"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.read("AGENTS.md").contains("## Rules"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_rejects_different_agents_sources_at_one_destination() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["minimax", "windsurf"]);
    project.write(".ai/src/AGENTS.md", "Shared instructions\n");
    project.write(".ai/src/other-agents.md", "Different instructions\n");
    project.write(
        ".ai/src/tools/windsurf.yaml",
        "targets:\n  agents:\n    source: .ai/src/other-agents.md\n",
    );
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("their sources differ"));
    assert!(!project.exists("AGENTS.md"));
}

#[test]
fn sync_only_ignores_a_shared_agents_conflict_in_skipped_tools() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["minimax", "windsurf"]);
    project.write(".ai/src/AGENTS.md", "Shared instructions\n");
    project.write(".ai/src/other-agents.md", "Different instructions\n");
    project.write(
        ".ai/src/tools/windsurf.yaml",
        "targets:\n  agents:\n    source: .ai/src/other-agents.md\n",
    );

    project
        .exuno()
        .args(["sync", "--only", "minimax"])
        .assert()
        .success();
    assert!(project.read("AGENTS.md").contains("Shared instructions"));
}

#[test]
fn sync_cursor_mcp_json_exists() {
    assert!(synced_project().exists(".cursor/mcp.json"));
}

#[test]
fn sync_windsurf_mcp_config_json_exists() {
    assert!(synced_project().exists(".devin/mcp_config.json"));
}

#[test]
fn sync_amazon_q_mcp_json_exists() {
    assert!(synced_project().exists(".amazonq/mcp.json"));
}

#[test]
fn sync_amazon_q_agents_md_gets_commands_inline_section() {
    let project = synced_project();
    let content = project.read(".amazonq/rules/00-context.md");
    assert!(content.contains("## Commands"));
    assert!(content.contains("- `/fix-issue` —"));
}

#[test]
fn sync_zed_rules_gets_commands_inline_section_merge_to_file_fallback() {
    let project = synced_project();
    let content = project.read(".rules");
    assert!(content.contains("## Commands"));
    assert!(content.contains("- `/fix-issue` —"));
}

#[test]
fn sync_a_folded_command_description_reads_whole_in_the_index_and_the_generated_skill() {
    let project = synced_project();
    project.write(
        ".ai/src/commands/folded.md",
        "---\ndescription: >\n  Review the diff\n  before a merge.\n---\n\nGo.\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".rules")
            .contains("- `/folded` — Review the diff before a merge.\n")
    );
    assert!(
        project
            .read(".agents/skills/command-folded/SKILL.md")
            .contains("description: >-\n  Review the diff before a merge.\n")
    );
}

#[test]
fn sync_gemini_settings_json_exists() {
    assert!(synced_project().exists(".gemini/settings.json"));
}

#[test]
fn sync_zed_settings_json_exists() {
    assert!(synced_project().exists(".zed/settings.json"));
}

// ── Tool-specific assertions for less-common tools ────────────────────────

#[test]
fn sync_junie_agents_md_exists_and_rules_are_inlined() {
    let project = synced_project();
    assert!(project.exists(".junie/AGENTS.md"));
    // Rules were inlined — no unsupported .junie/rules/ subdirectory.
    assert!(!project.join(".junie/rules").exists());
}

#[test]
fn sync_inlined_rule_inventory_shows_heading_not_frontmatter_delimiter() {
    // A path-scoped rule opens with a `---` frontmatter block; the shared
    // inline-into-agents inventory must surface its heading, not the `---`
    // delimiter. Asserted on .junie/AGENTS.md (uniquely owned — the root
    // AGENTS.md is contended by several tools depending on sync order).
    let project = synced_project();
    let content = project.read(".junie/AGENTS.md");
    assert!(content.contains("- `scoped-fixture.md` — Scoped Fixture Rule"));
    assert!(!content.contains("`scoped-fixture.md` — ---"));
}

// ── Path-scoped rules: canonical `paths:` → each tool's native glob trigger ─

#[test]
fn sync_path_scoped_rule_keeps_native_paths_for_claude() {
    let project = synced_project();
    let content = project.read(".claude/rules/scoped-fixture.md");
    assert!(content.lines().any(|line| line.starts_with("paths:")));
    assert!(content.contains("\"**/*.dart\""));
}

#[test]
fn sync_path_scoped_rule_becomes_cursor_auto_attached_glob() {
    let project = synced_project();
    let content = project.read(".cursor/rules/scoped-fixture.mdc");
    assert!(content.contains("globs: '**/*.dart'"));
    assert!(content.contains("alwaysApply: false"));
}

#[test]
fn sync_path_scoped_rule_becomes_copilot_applyto_glob() {
    let project = synced_project();
    let content = project.read(".github/instructions/scoped-fixture.instructions.md");
    assert!(content.contains("applyTo: '**/*.dart'"));
}

#[test]
fn sync_path_scoped_rule_becomes_windsurf_glob_trigger() {
    let project = synced_project();
    let content = project.read(".devin/rules/scoped-fixture.md");
    assert!(content.contains("trigger: glob"));
    assert!(content.contains("globs: '**/*.dart'"));
}

#[test]
fn sync_path_scoped_rule_becomes_antigravity_glob_trigger() {
    let project = synced_project();
    let content = project.read(".agents/rules/scoped-fixture.md");
    assert!(content.contains("trigger: glob"));
    assert!(content.contains("globs: '**/*.dart'"));
}

#[test]
fn sync_rule_without_paths_stays_always_on_cursor() {
    // core.md has no `paths:` — must keep the always-on default, not a glob.
    let project = synced_project();
    let content = project.read(".cursor/rules/core.mdc");
    assert!(content.contains("alwaysApply: true"));
}

// ── Gitignore ──────────────────────────────────────────────────────────────

#[test]
fn sync_gitignore_has_sync_markers() {
    let project = synced_project();
    let content = project.read(".gitignore");
    assert!(content.contains("AI SYNC GENERATED START"));
    assert!(content.contains("AI SYNC GENERATED END"));
}

#[test]
fn sync_gitignore_preserves_existing_content() {
    let project = synced_project();
    assert!(project.read(".gitignore").contains("node_modules/"));
}

#[test]
fn sync_gitignore_has_exactly_one_marker_block() {
    let project = synced_project();
    let content = project.read(".gitignore");
    assert_eq!(content.matches("AI SYNC GENERATED START").count(), 1);
}

// ── Re-sync stability (finding 5: shared-dest / nested-agents churn) ────────

#[test]
fn sync_re_sync_emits_no_churn_for_shared_dest_command_or_nested_agents() {
    // A second sync must not warn "Kept" for Codex-generated
    // .agents/skills/command-* (which Antigravity's skills step sweeps because
    // both tools share .agents/skills), nor "Removed" the nested AGENTS file
    // .amazonq/rules/00-context.md (written by the agents step, then swept by
    // the rules step) only to re-copy it every run.
    let project = synced_project();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept .agents/skills/command-").not())
        .stdout(predicate::str::contains("Removed: .amazonq/rules/00-context.md").not());
}

// ── Kiro ─────────────────────────────────────────────────────────────────

#[test]
fn sync_kiro_writes_steering_skills_agents_and_mcp() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["kiro"]);
    project.write(".ai/src/rules/scoped-fixture.md", SCOPED_FIXTURE_RULE);
    project.write(
        ".ai/src/agents/reviewer.md",
        "---\nname: reviewer\ndescription: Reviews\nmodel: sonnet\ntools: [Read, Grep, Bash]\n---\nReview.\n",
    );
    project.exuno().arg("sync").assert().success();

    assert!(project.exists("AGENTS.md"));
    assert!(
        project
            .read(".kiro/steering/core.md")
            .starts_with("---\ninclusion: always\n---\n")
    );
    assert!(
        project
            .read(".kiro/steering/scoped-fixture.md")
            .starts_with("---\ninclusion: fileMatch\nfileMatchPattern: ['**/*.dart']\n---\n")
    );
    assert!(project.exists(".kiro/skills/exuno/SKILL.md"));
    assert!(project.exists(".kiro/skills/command-review/SKILL.md"));
    assert_eq!(
        project.read(".kiro/agents/reviewer.md"),
        "---\nname: \"reviewer\"\ndescription: \"Reviews\"\ntools: [read, shell]\n---\nReview.\n"
    );
    assert!(
        project
            .read(".kiro/settings/mcp.json")
            .contains("\"mcpServers\"")
    );
    project.exuno().arg("sync").assert().success();
    project.exuno().arg("check").assert().success();
}

// ── Moved destinations ───────────────────────────────────────────────────

#[test]
fn sync_removes_what_it_generated_at_a_moved_destination_and_keeps_the_rest() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["windsurf"]);
    project.write(
        ".ai/src/tools/windsurf.yaml",
        "targets:\n  rules:\n    dest: \".windsurf/rules\"\n  skills:\n    dest: \".windsurf/skills\"\n  hooks:\n    dest: \".windsurf/hooks.json\"\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".windsurf/rules/core.md"));
    assert!(project.exists(".windsurf/hooks.json"));
    project.write(".windsurf/rules/mine.md", "hand-written\n");

    std::fs::remove_file(project.join(".ai/src/tools/windsurf.yaml")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "earlier output(s) from .windsurf/rules/ (targets.rules moved)",
        ));
    assert!(project.exists(".devin/rules/core.md"));
    assert!(!project.exists(".windsurf/rules/core.md"));
    assert!(!project.exists(".windsurf/skills"));
    assert!(!project.exists(".windsurf/hooks.json"));
    assert_eq!(project.read(".windsurf/rules/mine.md"), "hand-written\n");
    project.exuno().arg("check").assert().success();
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".windsurf/rules/mine.md"), "hand-written\n");
}

#[test]
fn rollback_restores_what_a_moved_destination_removed() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["windsurf"]);
    project.write(
        ".ai/src/tools/windsurf.yaml",
        "targets:\n  rules:\n    dest: \".windsurf/rules\"\n",
    );
    project.exuno().arg("sync").assert().success();
    let generated = project.read(".windsurf/rules/core.md");
    std::fs::remove_file(project.join(".ai/src/tools/windsurf.yaml")).unwrap();
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists(".windsurf/rules/core.md"));

    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert_eq!(project.read(".windsurf/rules/core.md"), generated);
}

#[test]
fn sync_cline_writes_native_skills_and_moves_off_clinerules() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["cline"]);
    project.write(
        ".ai/src/tools/cline.yaml",
        "targets:\n  agents:\n    dest: \".clinerules/00-context.md\"\n  rules:\n    dest: \".clinerules\"\n  commands:\n    dest: \".clinerules/workflows\"\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".clinerules/00-context.md"));
    project.write(".clinerules/team.md", "hand-written\n");

    std::fs::remove_file(project.join(".ai/src/tools/cline.yaml")).unwrap();
    project.write(
        ".ai/src/skills/flutter/bloc/SKILL.md",
        "---\nname: bloc\ndescription: Bloc\n---\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("AGENTS.md"));
    assert!(project.exists(".cline/rules/core.md"));
    assert!(project.exists(".cline/skills/bloc/SKILL.md"));
    assert!(project.exists(".cline/workflows/review.md"));
    assert!(!project.exists(".clinerules/00-context.md"));
    assert!(!project.exists(".clinerules/core.md"));
    assert!(!project.exists(".clinerules/workflows"));
    assert_eq!(project.read(".clinerules/team.md"), "hand-written\n");
    assert!(!project.read("AGENTS.md").contains("## Skills"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_cline_leaves_a_single_file_clinerules_alone() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["cline"]);
    project.write(".clinerules", "# hand-written Cline rules\n");
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".cline/rules/core.md"));
    assert_eq!(project.read(".clinerules"), "# hand-written Cline rules\n");
}

#[test]
fn a_failed_sync_restores_what_it_removed_at_a_moved_destination() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["windsurf"]);
    project.write(
        ".ai/src/tools/windsurf.yaml",
        "targets:\n  rules:\n    dest: \".windsurf/rules\"\n",
    );
    project.exuno().arg("sync").assert().success();
    project.write(".ai/src/tools/windsurf.yaml", "post_sync: \"false\"\n");
    project
        .exuno()
        .env("AGENTSYNC_ALLOW_POST_SYNC", "true")
        .arg("sync")
        .assert()
        .failure();
    assert!(project.exists(".windsurf/rules/core.md"));
    assert!(!project.exists(".devin/rules/core.md"));
}

// ── Command filters ──────────────────────────────────────────────────────

#[test]
fn sync_command_filters_apply_to_native_and_toml_command_dirs() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["claude", "gemini"]);
    for tool in ["claude", "gemini"] {
        project.write(
            &format!(".ai/src/tools/{tool}.yaml"),
            "targets:\n  commands:\n    exclude:\n      - review.md\n",
        );
    }
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists(".claude/commands/review.md"));
    assert!(project.exists(".claude/commands/fix-issue.md"));
    assert!(!project.exists(".gemini/commands/review.toml"));
    assert!(project.exists(".gemini/commands/fix-issue.toml"));
    project.exuno().arg("check").assert().success();
}

// ── Skill categories ─────────────────────────────────────────────────────

fn skill(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} fixture skill\n---\n\nBody.\n")
}

fn categorized_project() -> Project {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["claude", "codex"]);
    project.write(".ai/src/skills/flutter/bloc/SKILL.md", &skill("bloc"));
    project.write(
        ".ai/src/skills/flutter/ui/slivers/SKILL.md",
        &skill("slivers"),
    );
    project.write(
        ".ai/src/skills/flutter/ui/slivers/references/grid.md",
        "Grid.\n",
    );
    project.write(
        ".ai/src/skills/cloudflare/wrangler/SKILL.md",
        &skill("wrangler"),
    );
    project
}

#[test]
fn sync_lands_categorized_skills_flat_by_name_in_every_skills_dir() {
    let project = categorized_project();
    project.exuno().arg("sync").assert().success();
    for dest in [".claude/skills", ".agents/skills"] {
        assert_eq!(
            project.read(&format!("{dest}/bloc/SKILL.md")),
            skill("bloc")
        );
        assert!(project.exists(&format!("{dest}/slivers/references/grid.md")));
        assert!(project.exists(&format!("{dest}/wrangler/SKILL.md")));
        assert!(!project.exists(&format!("{dest}/flutter")));
    }
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_groups_the_inlined_skill_index_by_category() {
    let project = categorized_project();
    project.enable_tools(&["amazonq"]);
    project.exuno().arg("sync").assert().success();
    let index = project.read(".amazonq/rules/00-context.md");
    assert!(index.contains(
        "\n### cloudflare\n\n- `wrangler` — The wrangler fixture skill\n\n### flutter\n\n- `bloc` — The bloc fixture skill\n\n### flutter/ui\n\n- `slivers` — The slivers fixture skill\n"
    ));
    assert!(index.find("- `exuno` — ").unwrap() < index.find("### ").unwrap());
}

#[test]
fn sync_filters_a_whole_category_by_its_path() {
    let project = categorized_project();
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  skills:\n    exclude:\n      - cloudflare/*\n",
    );
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists(".agents/skills/wrangler"));
    assert!(project.exists(".agents/skills/bloc/SKILL.md"));
    assert!(project.exists(".claude/skills/wrangler/SKILL.md"));
}

#[test]
fn sync_ignores_a_shared_name_every_skills_consumer_filters_out() {
    let project = categorized_project();
    project.enable_tools(&["minimax"]);
    project.write(".ai/src/skills/backend/bloc/SKILL.md", &skill("bloc"));
    for tool in ["claude", "codex"] {
        project.write(
            &format!(".ai/src/tools/{tool}.yaml"),
            "targets:\n  skills:\n    exclude:\n      - backend/*\n",
        );
    }
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".claude/skills/bloc/SKILL.md"), skill("bloc"));
}

#[test]
fn sync_refuses_two_skills_sharing_a_name_and_changes_nothing() {
    let project = categorized_project();
    project.exuno().arg("sync").assert().success();
    let before = project.sha256(".claude/skills/bloc/SKILL.md");
    project.write(
        ".ai/src/skills/flutter/bloc/SKILL.md",
        "---\nname: bloc\ndescription: Edited\n---\n",
    );
    project.write(".ai/src/skills/backend/bloc/SKILL.md", &skill("bloc"));
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "[ERROR] Skill name 'bloc' is claimed by skills/backend/bloc, skills/flutter/bloc; every tool installs skills flat by name\n  • Rename one skill of each pair: skill names are unique across categories\n",
        ));
    assert_eq!(project.sha256(".claude/skills/bloc/SKILL.md"), before);
}

#[test]
fn sync_reads_the_tools_a_legacy_ai_agent_sync_yaml_enables() {
    let project = Project::seeded(&[]);
    std::fs::remove_file(project.join(".ai/exuno.yaml")).unwrap();
    project.write(".ai/agent_sync.yaml", "tools:\n  enabled: [zed]\n");
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".rules"));
}
