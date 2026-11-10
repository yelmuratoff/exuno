//! `tests/sync.bats`: `exuno sync` across every supported tool, driven
//! from one fixture project (a path-scoped rule, an explicit-only command).

mod common;

use common::{Project, is_ignored};
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
    "plugin",
];

const PLUGIN_MANIFESTS: [&str; 3] = [
    "plugin/plugin.json",
    ".claude-plugin/marketplace.json",
    ".agents/plugins/marketplace.json",
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

fn hiding_project(hide: &str) -> Project {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["claude", "opencode"]);
    project.append(".ai/exuno.yaml", &format!("skills:\n  hide:\n{hide}"));
    project.write(
        ".ai/src/skills/acme-bloc/SKILL.md",
        "---\nname: acme-bloc\ndescription: Project BLoC conventions\n---\n",
    );
    project
}

fn json(project: &Project, rel: &str) -> serde_json::Value {
    serde_json::from_str(&project.read(rel)).unwrap()
}

#[test]
fn sync_hides_listed_skills_in_the_settings_of_tools_that_can() {
    let project = hiding_project("    - bloc\n    - deploy\n");
    project.exuno().arg("sync").assert().success();

    let claude = json(&project, ".claude/settings.json");
    assert_eq!(claude["skillOverrides"]["bloc"], "off");
    assert_eq!(claude["skillOverrides"]["deploy"], "off");
    assert_eq!(claude["includeCoAuthoredBy"], false);
    let opencode = json(&project, "opencode.json");
    assert_eq!(opencode["permission"]["skill"]["bloc"], "deny");
    assert!(project.exists(".claude/skills/acme-bloc/SKILL.md"));
    project.exuno().arg("check").assert().success();

    let config = project.read(".ai/exuno.yaml").replace("    - deploy\n", "");
    project.write(".ai/exuno.yaml", &config);
    project.exuno().arg("sync").assert().success();
    let claude = json(&project, ".claude/settings.json");
    assert_eq!(claude["skillOverrides"]["bloc"], "off");
    assert!(claude["skillOverrides"].get("deploy").is_none());
}

#[test]
fn sync_owns_only_the_hidden_entries_of_a_keyed_settings_file() {
    let project = hiding_project("    - bloc\n");
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  settings:\n    ownership: keys\n",
    );
    project.exuno().arg("sync").assert().success();
    let live = project.read(".claude/settings.json").replacen(
        "\"skillOverrides\": {",
        "\"skillOverrides\": {\n    \"mine\": \"name-only\",",
        1,
    );
    project.write(".claude/settings.json", &live);

    let config = project.read(".ai/exuno.yaml").replace("    - bloc\n", "");
    project.write(".ai/exuno.yaml", &config);
    project.exuno().arg("sync").assert().success();
    let claude = json(&project, ".claude/settings.json");
    assert_eq!(claude["skillOverrides"]["mine"], "name-only");
    assert!(claude["skillOverrides"].get("bloc").is_none());
}

#[test]
fn sync_refuses_to_hide_a_skill_the_project_syncs() {
    let project = hiding_project("    - acme-bloc\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "skills.hide names a skill this project syncs: skills/acme-bloc",
        ))
        .stderr(predicate::str::contains("rename the project skill"));
    assert!(!project.exists(".claude/settings.json"));
}

#[test]
fn sync_refuses_an_invalid_name_in_skills_hide() {
    let project = hiding_project("    - Bad.Name\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "skills.hide lists invalid skill name(s): Bad.Name",
        ));
}

#[test]
fn sync_reads_a_scalar_skills_hide_as_one_name() {
    let project = Project::seeded(&["--outputs", "local"]);
    project.enable_tools(&["claude"]);
    project.append(".ai/exuno.yaml", "skills:\n  hide: bloc\n");
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        json(&project, ".claude/settings.json")["skillOverrides"]["bloc"],
        "off"
    );
}

#[test]
fn sync_dry_run_with_skills_hide_names_the_settings_and_writes_nothing() {
    let project = hiding_project("    - bloc\n");
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "+ skills.hide → .claude/settings.json (dry-run)",
        ));
    assert!(!project.exists(".claude/settings.json"));
}

#[test]
fn sync_hides_skills_in_an_opencode_config_composed_with_mcp() {
    let project = hiding_project("    - bloc\n");
    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"x\":{\"command\":\"x\"}}}\n",
    );
    project.exuno().arg("sync").assert().success();
    let opencode = json(&project, "opencode.json");
    assert_eq!(opencode["permission"]["skill"]["bloc"], "deny");
    assert!(opencode["mcp"].get("x").is_some());
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_warns_when_skills_hide_has_no_settings_file_to_land_in() {
    let project = hiding_project("    - bloc\n");
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  settings:\n    enabled: false\n",
    );
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "skills.hide has no settings file to land in for Claude Code",
        ));
}

// ── Plugin ───────────────────────────────────────────────────────────────

#[test]
fn sync_plugin_writes_skills_commands_agents_and_three_manifests() {
    let project = synced_project();
    let skill = "---\nname: demo\nargument-hint: <file>\ndescription: Runs the demo\ndisable-model-invocation: true\n---\n\n# Demo\n\nBody.\n";
    project.write(".ai/src/skills/demo/SKILL.md", skill);
    let agent = "---\nname: reviewer\ndescription: Reviews a diff\nmodel: sonnet\ntools: [Read, Grep]\n---\n\nReview the diff.\n";
    project.write(".ai/src/agents/reviewer.md", agent);
    let output = project.exuno().arg("sync").assert().success();
    let stderr = String::from_utf8(output.get_output().stderr.clone()).unwrap();

    assert_eq!(project.read("plugin/agents/reviewer.md"), agent);
    assert_eq!(
        project.read("plugin/skills/demo/SKILL.md"),
        "---\nname: demo\ndescription: Runs the demo\n---\n\n# Demo\n\nBody.\n"
    );
    assert!(
        stderr.contains(
            "[WARNING] Agent plugin (claude.ai, ChatGPT, Codex): dropped frontmatter keys argument-hint, disable-model-invocation (allowlist targets.skills.frontmatter_keys) from 1 skill\n"
        ),
        "{stderr}"
    );
    assert!(
        stderr.contains(
            "[WARNING] Agent plugin (claude.ai, ChatGPT, Codex): dropped frontmatter key"
        ) && stderr.contains("(allowlist targets.commands.frontmatter_keys) from "),
        "{stderr}"
    );
    assert_eq!(
        stderr.matches("dropped frontmatter key").count(),
        2,
        "{stderr}"
    );
    assert_eq!(project.read(".claude/skills/demo/SKILL.md"), skill);
    assert!(project.exists("plugin/skills/exuno/SKILL.md"));
    assert_eq!(
        project.read("plugin/commands/explicit-only.md"),
        "---\ndescription: Explicit-only fixture command\n---\n\nBody.\n"
    );

    for rel in PLUGIN_MANIFESTS {
        let manifest = json(&project, rel);
        assert_eq!(manifest["name"], "my-skills", "{rel}");
        assert!(manifest.get("version").is_none(), "{rel}");
        assert!(project.read(rel).ends_with("}\n"), "{rel}");
    }
    let plugin = json(&project, "plugin/plugin.json");
    assert_eq!(
        plugin["$schema"],
        "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"
    );
    assert_eq!(
        plugin["description"],
        "Skills, commands and agents synced by Exuno"
    );
    let claude = json(&project, ".claude-plugin/marketplace.json");
    assert_eq!(claude["owner"]["name"], "exuno");
    assert_eq!(
        claude["description"],
        "Skills, commands and agents synced by Exuno"
    );
    assert_eq!(claude["plugins"][0]["name"], "my-skills");
    assert_eq!(claude["plugins"][0]["source"], "./plugin");
    let codex = json(&project, ".agents/plugins/marketplace.json");
    assert_eq!(codex["interface"]["displayName"], "my-skills");
    assert_eq!(codex["plugins"][0]["source"]["source"], "local");
    assert_eq!(codex["plugins"][0]["source"]["path"], "./plugin");
    assert_eq!(codex["plugins"][0]["policy"]["installation"], "AVAILABLE");
    assert_eq!(codex["plugins"][0]["category"], "Productivity");
}

#[test]
fn sync_plugin_outputs_are_not_gitignored() {
    let project = synced_project();
    let mut plugin_outputs = vec![
        "plugin/skills/exuno/SKILL.md",
        "plugin/commands/explicit-only.md",
    ];
    plugin_outputs.extend(PLUGIN_MANIFESTS);
    for rel in plugin_outputs {
        assert!(project.exists(rel), "{rel}");
        assert!(!is_ignored(&project, rel), "{rel}");
    }
    for rel in [
        "CLAUDE.md",
        ".claude/skills/exuno/SKILL.md",
        ".agents/skills/exuno/SKILL.md",
    ] {
        assert!(is_ignored(&project, rel), "{rel}");
    }
}

const HAND_WRITTEN_SKILL: &str = "plugin/skills/mine/SKILL.md";

#[test]
fn a_disabled_plugin_leaves_hand_written_files_in_its_dests() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.write(HAND_WRITTEN_SKILL, "---\nname: mine\n---\n");
    project.write("plugin/agents/mine.md", "mine\n");
    project.write(".claude-plugin/marketplace.json", "{}\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Cleaned up Agent plugin").not());
    assert_eq!(project.read(HAND_WRITTEN_SKILL), "---\nname: mine\n---\n");
    assert_eq!(project.read("plugin/agents/mine.md"), "mine\n");
    assert_eq!(project.read(".claude-plugin/marketplace.json"), "{}\n");
}

#[test]
fn disabling_a_tool_leaves_another_tools_files_under_its_dest() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/agents/helper.md",
        "---\nname: helper\n---\nHelp.\n",
    );
    project.write(
        ".ai/src/tools/aaa.yaml",
        "name: \"A\"\nenabled: true\ntargets:\n  subagents:\n    dest: \".claude\"\n",
    );
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/helper.md"));
    let core_rule = project.read(".claude/rules/core.md");
    project.write(".claude/notes.md", "mine\n");

    project.write(
        ".ai/src/tools/aaa.yaml",
        "name: \"A\"\nenabled: false\ntargets:\n  subagents:\n    dest: \".claude\"\n",
    );
    project
        .exuno()
        .args(["sync", "--skip", "claude"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Cleaned up A (disabled)"));
    assert!(!project.exists(".claude/helper.md"));
    assert_eq!(project.read(".claude/notes.md"), "mine\n");
    assert_eq!(project.read(".claude/rules/core.md"), core_rule);
    assert!(project.exists(".claude/skills/exuno/SKILL.md"));
    assert!(project.exists(".claude/settings.json"));
    assert!(project.exists(".claude/hooks/agentsync-guard.sh"));
}

#[test]
fn disabling_a_tool_removes_only_its_recorded_files() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude", "cursor"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".cursor/rules/core.mdc"));
    project.write(".cursor/rules/mine.mdc", "mine\n");

    project
        .exuno()
        .args(["disable", "cursor"])
        .assert()
        .success();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[INFO] Cleaned up Cursor (disabled)",
        ))
        .stderr(predicate::str::contains(
            "[INFO] Left .cursor/rules in place: it holds files sync did not write\n",
        ));
    assert!(!project.exists(".cursor/rules/core.mdc"));
    assert!(!project.exists(".cursor/mcp.json"));
    assert_eq!(project.read(".cursor/rules/mine.mdc"), "mine\n");

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Left .cursor").not())
        .stderr(predicate::str::contains("Cleaned up Cursor").not());
    assert_eq!(project.read(".cursor/rules/mine.mdc"), "mine\n");
}

#[test]
fn disabling_the_plugin_removes_only_what_it_wrote() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude", "plugin"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("plugin/skills/exuno/SKILL.md"));
    project.write(HAND_WRITTEN_SKILL, "---\nname: mine\n---\n");

    project
        .exuno()
        .args(["disable", "plugin"])
        .assert()
        .success();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Cleaned up Agent plugin"));
    assert_eq!(project.read(HAND_WRITTEN_SKILL), "---\nname: mine\n---\n");
    for gone in [
        "plugin/skills/exuno",
        "plugin/commands",
        "plugin/agents",
        "plugin/plugin.json",
        ".claude-plugin",
        ".agents",
    ] {
        assert!(!project.exists(gone), "{gone}");
    }
    assert!(project.exists("CLAUDE.md"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn a_tool_based_on_plugin_renders_the_plugin_templates_with_its_own_vars() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/tools/team.yaml",
        "base: plugin\nenabled: true\ntargets:\n  manifests:\n    vars:\n      name: \"team-skills\"\n",
    );
    project.exuno().arg("sync").assert().success();
    for rel in PLUGIN_MANIFESTS {
        assert_eq!(json(&project, rel)["name"], "team-skills", "{rel}");
    }
    assert_eq!(
        json(&project, ".claude-plugin/marketplace.json")["owner"]["name"],
        "exuno"
    );
    project.exuno().arg("check").assert().success();
}

// ── Engine safety ────────────────────────────────────────────────────────

#[cfg(unix)]
#[test]
fn sync_refuses_to_write_through_a_symlink_that_leaves_the_project() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("notes.txt");
    std::fs::write(&secret, "keep me\n").unwrap();
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    std::fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    std::os::unix::fs::symlink(&secret, project.join(".cursor/rules/core.mdc")).unwrap();
    project.enable_tools(&["cursor"]);
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Refusing to write .cursor/rules/core.mdc: .cursor/rules/core.mdc is a symlink to",
        ))
        .stderr(predicate::str::contains("outside the project"));
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "keep me\n");
    assert!(project.exists("CLAUDE.md"));
}

#[cfg(unix)]
#[test]
fn sync_follows_a_symlink_that_stays_inside_the_project() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let project = Project::seeded(&[]);
    std::fs::create_dir_all(project.join("store/claude")).unwrap();
    std::os::unix::fs::symlink("store/claude", project.join(".claude")).unwrap();
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("store/claude/rules/core.md"));
    assert!(project.exists("store/claude/skills/exuno/SKILL.md"));
    project.exuno().arg("sync").assert().success();
}

#[cfg(unix)]
#[test]
fn sync_refuses_to_write_through_a_symlink_into_ai_or_git() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/mine/SKILL.md", "---\nname: mine\n---\n");
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::os::unix::fs::symlink("../.ai/src/skills", project.join(".claude/skills")).unwrap();
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "more file(s): .claude/skills is a symlink to",
        ))
        .stderr(predicate::str::contains(
            "inside .ai/, where the sources live",
        ));
    assert!(!project.exists(".ai/src/skills/exuno"));
    assert!(project.exists(".ai/src/skills/mine/SKILL.md"));

    std::fs::remove_file(project.join(".claude/skills")).unwrap();
    std::fs::create_dir_all(project.join(".git/info")).unwrap();
    std::os::unix::fs::symlink("../.git/info", project.join(".claude/rules")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(".claude/rules is a symlink to"))
        .stderr(predicate::str::contains("inside .git/"));
    assert!(!project.exists(".git/info/core.md"));
}

#[cfg(unix)]
#[test]
fn cleanup_refuses_to_remove_through_a_symlink_that_leaves_the_project() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let outside = tempfile::tempdir().unwrap();
    let project = Project::seeded(&[]);
    project.write(".ai/src/skills/demo/SKILL.md", "---\nname: demo\n---\n");
    project.enable_tools(&["claude", "cursor"]);
    project.exuno().arg("sync").assert().success();
    let synced = project.read(".cursor/skills/demo/SKILL.md");
    std::fs::write(outside.path().join("SKILL.md"), &synced).unwrap();
    std::fs::remove_dir_all(project.join(".cursor/skills/demo")).unwrap();
    std::os::unix::fs::symlink(outside.path(), project.join(".cursor/skills/demo")).unwrap();

    project
        .exuno()
        .args(["disable", "cursor"])
        .assert()
        .success();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[WARNING] Refusing to remove .cursor/skills/demo/SKILL.md: .cursor/skills/demo is a symlink to",
        ));
    assert_eq!(
        std::fs::read_to_string(outside.path().join("SKILL.md")).unwrap(),
        synced
    );
}

/// A synced claude project, then a hand-written `.cursor/rules/core.mdc` that
/// enabling cursor would overwrite.
fn unrecorded_cursor_rule() -> Project {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".ai/.sync-manifest"));
    project.write(".cursor/rules/core.mdc", "mine\n");
    project.enable_tools(&["cursor"]);
    project
}

#[test]
fn sync_refuses_to_overwrite_an_unrecorded_file_that_differs() {
    let project = unrecorded_cursor_rule();
    let manifest = project.read(".ai/.sync-manifest");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "[ERROR] Sync would overwrite 1 file(s) it did not write:\n      .cursor/rules/core.mdc\n",
        ))
        .stderr(predicate::str::contains("--force"));
    assert_eq!(project.read(".cursor/rules/core.mdc"), "mine\n");
    assert!(!project.exists("AGENTS.md"));
    assert_eq!(project.read(".ai/.sync-manifest"), manifest);
}

#[test]
fn force_overwrites_an_unrecorded_file() {
    let project = unrecorded_cursor_rule();
    project
        .exuno()
        .args(["sync", "--force"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[WARNING] Overwriting 1 file(s) sync did not write (--force):\n      .cursor/rules/core.mdc\n",
        ));
    assert!(project.read(".cursor/rules/core.mdc").starts_with("---\n"));
    project.exuno().arg("sync").assert().success();
}

#[test]
fn a_refused_overwrite_runs_no_post_sync_hook() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/tools/claude.yaml",
        "post_sync: \"echo ran >> hook.log\"\n",
    );
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .arg("sync")
        .env("EXUNO_ALLOW_POST_SYNC", "true")
        .assert()
        .success();
    assert_eq!(project.read("hook.log"), "ran\n");
    project.write(".cursor/rules/core.mdc", "mine\n");
    project.enable_tools(&["cursor"]);
    project
        .exuno()
        .arg("sync")
        .env("EXUNO_ALLOW_POST_SYNC", "true")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Sync would overwrite 1 file(s)"))
        .stderr(predicate::str::contains("Syncing Claude Code").not());
    assert_eq!(project.read("hook.log"), "ran\n");
}

#[test]
fn a_refused_overwrite_writes_nothing_without_a_backup() {
    let project = unrecorded_cursor_rule();
    project
        .exuno()
        .arg("sync")
        .env("EXUNO_INTERNAL_SKIP_BACKUP", "true")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Sync would overwrite 1 file(s)"));
    assert_eq!(project.read(".cursor/rules/core.mdc"), "mine\n");
    assert!(!project.exists("AGENTS.md"));
}

#[test]
fn a_dry_run_reports_an_overwrite_a_real_sync_would_refuse() {
    let project = unrecorded_cursor_rule();
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[WARNING] A real sync would stop: Sync would overwrite 1 file(s) it did not write:\n      .cursor/rules/core.mdc\n",
        ));
    assert_eq!(project.read(".cursor/rules/core.mdc"), "mine\n");
}

#[test]
fn an_unrecorded_file_that_matches_the_final_render_is_taken_over() {
    let reference = Project::seeded(&[]);
    reference.enable_tools(&["claude", "codex"]);
    reference.exuno().arg("sync").assert().success();
    let agents = reference.read("AGENTS.md");
    assert!(agents.contains("## Rules"), "codex appends to AGENTS.md");

    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    project.write("AGENTS.md", &agents);
    project.enable_tools(&["codex"]);
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read("AGENTS.md"), agents);
}

#[cfg(unix)]
#[test]
fn a_dry_run_reports_a_write_a_real_sync_would_refuse() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let outside = tempfile::tempdir().unwrap();
    let secret = outside.path().join("notes.txt");
    std::fs::write(&secret, "keep me\n").unwrap();
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    std::fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    std::os::unix::fs::symlink(&secret, project.join(".cursor/rules/core.mdc")).unwrap();
    project.enable_tools(&["cursor"]);
    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "[WARNING] A real sync would stop: Refusing to write .cursor/rules/core.mdc: .cursor/rules/core.mdc is a symlink to",
        ));
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "A real sync would stop: Refusing to write .cursor/rules/core.mdc",
        ));
    assert_eq!(std::fs::read_to_string(&secret).unwrap(), "keep me\n");
}

#[test]
fn a_first_sync_without_a_manifest_still_writes_over_existing_files() {
    let project = Project::seeded(&[]);
    assert!(!project.exists(".ai/.sync-manifest"));
    project.write("CLAUDE.md", "mine\n");
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read("CLAUDE.md"), project.read(".ai/src/AGENTS.md"));
}

#[test]
fn a_first_sync_prunes_nothing_unrecorded() {
    let project = Project::seeded(&[]);
    assert!(!project.exists(".ai/.sync-manifest"));
    project.write(
        ".ai/src/agents/helper.md",
        "---\nname: helper\n---\nHelp.\n",
    );
    project.write(".claude/rules/mine.md", "# Mine\n");
    project.write(".claude/skills/mine/SKILL.md", "---\nname: mine\n---\n");
    project.write(".claude/agents/mine.md", "mine\n");
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept .claude/rules/mine.md"))
        .stderr(predicate::str::contains("Kept .claude/skills/mine"))
        .stderr(predicate::str::contains("Kept .claude/agents/mine.md"));
    assert_eq!(project.read(".claude/rules/mine.md"), "# Mine\n");
    assert!(project.exists(".claude/skills/mine/SKILL.md"));
    assert_eq!(project.read(".claude/agents/mine.md"), "mine\n");
    assert!(project.exists(".claude/rules/core.md"));
    assert!(project.exists(".claude/agents/helper.md"));
}

#[test]
fn a_synced_skill_folder_keeps_unrecorded_files_and_drops_what_the_source_lost() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.write(".ai/src/skills/demo/SKILL.md", "---\nname: demo\n---\n");
    project.write(".ai/src/skills/demo/references/old.md", "old\n");
    project.exuno().arg("sync").assert().success();
    project.write(".claude/skills/demo/__pycache__/x.pyc", "cache");
    project.write(".claude/skills/demo/.DS_Store", "finder");

    std::fs::remove_file(project.join(".ai/src/skills/demo/references/old.md")).unwrap();
    project.write(
        ".ai/src/skills/demo/SKILL.md",
        "---\nname: demo\n---\nNew.\n",
    );
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".claude/skills/demo/__pycache__/x.pyc"),
        "cache"
    );
    assert_eq!(project.read(".claude/skills/demo/.DS_Store"), "finder");
    assert!(!project.exists(".claude/skills/demo/references/old.md"));
    assert_eq!(
        project.read(".claude/skills/demo/SKILL.md"),
        "---\nname: demo\n---\nNew.\n"
    );
    let manifest = project.read(".ai/.sync-manifest");
    assert!(!manifest.contains("__pycache__"), "{manifest}");
    assert!(!manifest.contains(".DS_Store"), "{manifest}");

    project.exuno().arg("sync").assert().success();
    project.exuno().arg("check").assert().success();
}

#[test]
fn an_unrecorded_command_skill_folder_is_kept_and_a_recorded_obsolete_one_goes() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/commands/old.md",
        "---\ndescription: Old\n---\nOld.\n",
    );
    project.enable_tools(&["codex"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".agents/skills/command-old/SKILL.md"));
    project.write(".agents/skills/command-mine/SKILL.md", "mine\n");

    std::fs::remove_file(project.join(".ai/src/commands/old.md")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Kept .agents/skills/command-mine"));
    assert!(!project.exists(".agents/skills/command-old"));
    assert_eq!(
        project.read(".agents/skills/command-mine/SKILL.md"),
        "mine\n"
    );
}

/// A seeded project with local outputs, so sync manages `.gitignore`, and no
/// `.gitignore` yet.
#[cfg(unix)]
fn local_outputs_project() -> Project {
    let project = Project::seeded(&["--outputs", "local"]);
    let _ = std::fs::remove_file(project.join(".gitignore"));
    project.enable_tools(&["claude"]);
    project
}

#[cfg(unix)]
#[test]
fn sync_refuses_a_gitignore_link_that_leaves_the_project() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("ignore.txt");
    std::fs::write(&target, "outside\n").unwrap();
    let project = local_outputs_project();
    std::os::unix::fs::symlink(&target, project.join(".gitignore")).unwrap();
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Refusing to write .gitignore: .gitignore is a symlink to",
        ));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "outside\n");
}

#[cfg(unix)]
#[test]
fn sync_writes_through_a_gitignore_link_inside_the_project() {
    // Creating a symlink on Windows needs Developer Mode or admin rights.
    let project = local_outputs_project();
    project.write("config/gitignore", "node_modules/\n");
    std::os::unix::fs::symlink("config/gitignore", project.join(".gitignore")).unwrap();
    project.exuno().arg("sync").assert().success();
    project.write(".ai/src/rules/extra.md", "# Extra\n");
    project.exuno().arg("sync").assert().success();
    let meta = std::fs::symlink_metadata(project.join(".gitignore")).unwrap();
    assert!(meta.file_type().is_symlink());
    let text = project.read("config/gitignore");
    assert!(text.starts_with("node_modules/\n"), "{text}");
    assert!(text.contains("AI SYNC GENERATED START"), "{text}");
}
