//! `tests/init.bats`: `exuno init`.

mod common;

use std::path::Path;
#[cfg(unix)]
use std::process::Command as StdCommand;

use common::Project;
use predicates::prelude::*;

fn md_files(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().ends_with(".md") && e.path().is_file())
                .count()
        })
        .unwrap_or(0)
}

fn skill_files(dir: &Path, found: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            skill_files(&path, found);
        } else if path.file_name().is_some_and(|n| n == "SKILL.md") {
            *found += 1;
        }
    }
}

#[test]
fn init_creates_ai_src_content_directories() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();

    assert!(project.exists(".ai/src/AGENTS.md"));
    assert!(project.join(".ai/src/rules").is_dir());
    assert!(project.join(".ai/src/skills").is_dir());
    assert!(project.join(".ai/src/commands").is_dir());
    assert!(project.join(".ai/src/agents").is_dir());
    // tools/ is created on demand by `customize`.
    assert!(!project.exists(".ai/src/tools"));
    // Payload dirs (settings/mcp/hooks) are lazy — created only when a tool
    // is enabled (auto-detect or --tools). Empty project = no payload dirs.
    assert!(!project.exists(".ai/src/settings"));
    assert!(!project.exists(".ai/src/mcp"));
    assert!(!project.exists(".ai/src/hooks"));
}

#[test]
fn init_tools_claude_scaffolds_only_claude_payloads() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude"])
        .assert()
        .success();

    // Claude has settings + hooks (none) templates; MCP is excluded in 0.11+
    // because it resolves via shared .ai/src/mcp.json (or base).
    assert!(project.exists(".ai/src/tools/claude/settings.json"));
    assert!(!project.exists(".ai/src/settings"));
    assert!(!project.exists(".ai/src/mcp"));
    assert!(!project.exists(".ai/src/hooks"));
    // No cursor leakage.
    assert!(!project.exists(".ai/src/tools/cursor"));
}

#[test]
fn init_tools_claude_payloads_land_in_the_layout_sync_treats_as_canonical() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            ".ai/src/tools/claude/settings.json",
        ));

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("Legacy payload override layout").not());
}

#[test]
fn init_refuses_to_run_from_inside_ai() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();

    project
        .exuno()
        .current_dir(project.join(".ai"))
        .arg("init")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(".ai"));
    // Running init from inside .ai/ must not create a nested .ai/.ai/.
    assert!(!project.exists(".ai/.ai"));
}

#[test]
fn sync_refuses_to_run_from_inside_ai() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();

    project
        .exuno()
        .current_dir(project.join(".ai"))
        .arg("sync")
        .assert()
        .code(2)
        .stderr(predicate::str::contains(".ai"));
}

#[test]
fn init_content_agents_rules_skips_skills_commands_agents() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--content", "agents,rules"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/AGENTS.md"));
    assert!(project.join(".ai/src/rules").is_dir());
    assert!(!project.exists(".ai/src/skills"));
    assert!(!project.exists(".ai/src/commands"));
    assert!(!project.exists(".ai/src/agents"));
}

#[test]
fn init_no_detect_ignores_existing_tool_markers() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    project
        .exuno()
        .args(["init", "--no-detect"])
        .assert()
        .success();

    assert!(project.read(".ai/agent_sync.yaml").contains("enabled: []"));
    assert!(!project.exists(".ai/src/settings"));
}

#[test]
fn init_tools_union_with_auto_detect() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();
    project
        .exuno()
        .args(["init", "--tools", "claude"])
        .assert()
        .success();

    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|l| l == "    - claude"));
    assert!(config.lines().any(|l| l == "    - cursor"));
}

#[test]
fn init_rejects_unknown_content_token() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--content", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown --content section"));
}

#[test]
fn init_creates_starter_rules() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(md_files(&project.join(".ai/src/rules")) >= 1);
}

#[test]
fn init_creates_agent_sync_yaml_with_tools_enabled_list() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();

    assert!(project.exists(".ai/agent_sync.yaml"));
    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|l| l == "tools:"));
    assert!(config.contains("enabled:"));
}

#[test]
fn init_creates_skills() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    let mut count = 0;
    skill_files(&project.join(".ai/src/skills"), &mut count);
    assert!(count >= 1);
}

#[test]
fn init_creates_commands() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(md_files(&project.join(".ai/src/commands")) >= 1);
}

#[test]
fn init_creates_agents() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(md_files(&project.join(".ai/src/agents")) >= 1);
}

#[test]
fn init_skips_if_ai_src_already_exists() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    project
        .exuno()
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("already exists"));
}

#[test]
fn init_keeps_an_existing_exuno_yaml_and_writes_no_legacy_config() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: []\n");
    project.exuno().arg("init").assert().success();
    assert_eq!(project.read(".ai/exuno.yaml"), "tools:\n  enabled: []\n");
    assert!(!project.exists(".ai/agent_sync.yaml"));
}

#[test]
fn init_output_mentions_enable_command() {
    let project = Project::empty();
    project
        .exuno()
        .arg("init")
        .assert()
        .success()
        .stdout(predicate::str::contains("exuno enable"));
}

#[test]
fn init_next_steps_lists_mcp_and_customize_hints() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--no-detect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("exuno add mcp"))
        .stdout(predicate::str::contains("exuno customize"));
}

#[test]
fn init_to_custom_directory() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join("subdir")).unwrap();
    project.exuno().args(["init", "subdir"]).assert().success();
    assert!(project.exists("subdir/.ai/src/AGENTS.md"));
}

// Init's wizard runs only on a real TTY, so this needs a pty — `script(1)`
// stands in the way bats did (`command -v script` gates the case).
#[cfg(unix)]
#[test]
fn init_the_wizard_draws_its_tool_list_on_a_terminal() {
    if StdCommand::new("script").arg("--version").output().is_err()
        && StdCommand::new("which")
            .arg("script")
            .output()
            .is_ok_and(|o| !o.status.success())
    {
        eprintln!("script(1) not available; skipping");
        return;
    }
    let project = Project::empty();
    // Move down once in the tool list (an arrow must not read as Escape), Enter
    // through both lists, keep committed outputs, decline at Proceed.
    let keys = "\x1b[B\n\ny\nn\n";
    let bin = env!("CARGO_BIN_EXE_exuno");
    let gnu = StdCommand::new("script")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    let output = if gnu {
        StdCommand::new("bash")
            .arg("-c")
            .arg("printf '%s' \"$1\" | script -q -c \"$2 init\" /dev/null")
            .arg("_")
            .arg(keys)
            .arg(bin)
            .current_dir(project.path())
            .output()
            .unwrap()
    } else {
        StdCommand::new("bash")
            .arg("-c")
            .arg("printf '%s' \"$1\" | script -q /dev/null \"$2\" init")
            .arg("_")
            .arg(keys)
            .arg(bin)
            .current_dir(project.path())
            .output()
            .unwrap()
    };
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(text.contains("Tools to enable"));
    assert!(text.contains("(space: toggle"));
    assert!(text.contains("Content sections:"));
    assert!(text.contains("Cancelled."));
    assert!(!project.exists(".ai"));
}

#[test]
fn init_names_a_missing_target_directory_and_fails() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "missing-dir"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Directory not found: missing-dir"));
    assert!(!project.exists(".ai"));
    assert!(!project.exists("missing-dir"));
}

#[test]
fn init_does_not_copy_system_engine_into_project() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(!project.exists(".ai/system"));
}

#[test]
fn init_agents_md_is_valid_markdown() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    let text = project.read(".ai/src/AGENTS.md");
    assert!(text.starts_with('#'));
}

#[test]
fn init_with_empty_project_enables_no_tools_by_default() {
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(project.read(".ai/agent_sync.yaml").contains("enabled: []"));
}

#[test]
fn init_auto_detects_existing_tool_markers() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    std::fs::create_dir_all(project.join(".cursor")).unwrap();
    project.exuno().arg("init").assert().success();

    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|l| l == "    - claude"));
    assert!(config.lines().any(|l| l == "    - cursor"));
}

#[test]
fn init_writes_only_the_defaults_sync_reads() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--no-detect", "--no-sync"])
        .assert()
        .success();
    let config = project.read(".ai/agent_sync.yaml");
    assert!(
        config.contains("\ndefaults:\n  cleanup: true\n"),
        "{config}"
    );
}

#[test]
fn init_detects_cline_from_a_single_file_clinerules_and_keeps_it() {
    let project = Project::empty();
    project.write(".clinerules", "# hand-written Cline rules\n");
    project.exuno().arg("init").assert().success();

    assert!(
        project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|l| l == "    - cline")
    );
    assert!(project.exists(".cline/rules"));
    assert_eq!(project.read(".clinerules"), "# hand-written Cline rules\n");
}

#[test]
fn init_backs_up_existing_destinations_for_enabled_tools() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".claude")).unwrap();
    project.write("CLAUDE.md", "claude-before\n");
    project.write(".claude/settings.json", "settings-before\n");

    // --no-sync so .latest is init's own snapshot, not the first sync's.
    project
        .exuno()
        .args(["init", "--tools", "claude", "--no-sync"])
        .assert()
        .success();

    let snapshot_id = project.read(".ai/backups/.latest");
    let snapshot_id = snapshot_id.trim();
    let snapshot = format!(".ai/backups/{snapshot_id}");
    assert!(
        project
            .read(&format!("{snapshot}/metadata"))
            .lines()
            .any(|l| l == "operation=init")
    );
    assert_eq!(
        project.read(&format!("{snapshot}/files/CLAUDE.md")),
        "claude-before\n"
    );
    assert_eq!(
        project.read(&format!("{snapshot}/files/.claude/settings.json")),
        "settings-before\n"
    );
}

#[test]
fn init_restores_pre_init_state_after_a_partial_scaffold_failure() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".ai/agent_sync.yaml")).unwrap();
    project.write(".ai/agent_sync.yaml/sentinel", "keep\n");

    project
        .exuno()
        .args(["init", "--no-detect"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Restored pre-init state"));

    assert!(!project.exists(".ai/src"));
    assert!(project.exists(".ai/agent_sync.yaml/sentinel"));
    assert!(!project.exists(".ai/.template-manifest"));
}

#[test]
fn init_records_no_template_hash_for_an_adopted_agents_md() {
    let project = Project::empty();
    project.write("CLAUDE.md", "# Hand-written\n");
    project
        .exuno()
        .args(["init", "--tools", "claude", "--no-sync"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/AGENTS.md"), "# Hand-written\n");
    let manifest = project.read(".ai/.template-manifest");
    assert!(
        !manifest.lines().any(|line| line.starts_with("AGENTS.md\t")),
        "{manifest}"
    );
    assert!(
        manifest.lines().any(|line| line.starts_with("rules/")),
        "{manifest}"
    );
}

#[test]
fn init_auto_detects_kimi_code_and_opencode_markers() {
    let project = Project::empty();
    std::fs::create_dir_all(project.join(".kimi-code")).unwrap();
    std::fs::create_dir_all(project.join(".opencode")).unwrap();
    project.exuno().arg("init").assert().success();

    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.lines().any(|l| l == "    - kimi"));
    assert!(config.lines().any(|l| l == "    - opencode"));
}

#[test]
fn init_does_not_treat_generic_agents_md_as_a_codex_marker() {
    let project = Project::empty();
    project.write("AGENTS.md", "# Shared agent instructions\n");
    project.exuno().arg("init").assert().success();

    assert!(
        !project
            .read(".ai/agent_sync.yaml")
            .lines()
            .any(|l| l == "    - codex")
    );
}

// ── Phase 4: interactive init, --yes, --dry-run ─────────────────────────────

#[test]
fn init_dry_run_shows_plan_without_writing() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--dry-run", "--tools", "claude,cursor"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"))
        .stdout(predicate::str::contains("Plan:"))
        .stdout(predicate::str::contains("claude"))
        .stdout(predicate::str::contains("cursor"));

    assert!(!project.exists(".ai"));
    assert!(!project.exists(".ai/agent_sync.yaml"));
}

#[test]
fn init_dry_run_content_filters_in_plan() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--dry-run", "--content", "agents,rules"])
        .assert()
        .success()
        .stdout(predicate::str::contains("agents, rules"));
    assert!(!project.exists(".ai"));
}

#[test]
fn init_yes_in_non_tty_behaves_like_defaults() {
    // --yes is a no-op here (no prompts run in non-TTY), but must not error.
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--yes", "--no-detect"])
        .assert()
        .success();
    assert!(project.exists(".ai/agent_sync.yaml"));
    assert!(project.exists(".ai/src/AGENTS.md"));
}

#[test]
fn init_non_tty_without_flags_runs_silently_with_defaults() {
    // No flags, no TTY → fall through to defaults; must NOT hang on prompts.
    let project = Project::empty();
    project.exuno().arg("init").assert().success();
    assert!(project.exists(".ai/agent_sync.yaml"));
}

#[test]
fn init_help_documents_yes_and_dry_run() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--yes"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--no-templates"));
}

#[test]
fn init_no_templates_creates_empty_layout_without_starter_files() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--no-templates", "--no-detect"])
        .assert()
        .success();

    assert!(project.exists(".ai/src/AGENTS.md"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), "");
    assert!(project.join(".ai/src/rules").is_dir());
    assert!(project.join(".ai/src/skills").is_dir());
    assert!(project.join(".ai/src/commands").is_dir());
    assert!(project.join(".ai/src/agents").is_dir());

    assert_eq!(md_files(&project.join(".ai/src/rules")), 0);
    assert_eq!(md_files(&project.join(".ai/src/commands")), 0);
    assert_eq!(md_files(&project.join(".ai/src/agents")), 0);
    let mut skills = 0;
    skill_files(&project.join(".ai/src/skills"), &mut skills);
    assert_eq!(skills, 0);
}

#[test]
fn init_no_templates_content_agents_rules_narrows_empty_dirs() {
    let project = Project::empty();
    project
        .exuno()
        .args([
            "init",
            "--no-templates",
            "--no-detect",
            "--content",
            "agents,rules",
        ])
        .assert()
        .success();

    assert!(project.exists(".ai/src/AGENTS.md"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), "");
    assert!(project.join(".ai/src/rules").is_dir());
    assert!(!project.exists(".ai/src/skills"));
    assert!(!project.exists(".ai/src/commands"));
    assert!(!project.exists(".ai/src/agents"));
    assert_eq!(md_files(&project.join(".ai/src/rules")), 0);
}

#[test]
fn init_dry_run_no_templates_notes_no_starter_templates_in_plan() {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--dry-run", "--no-templates", "--no-detect"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no starter templates"));
    assert!(!project.exists(".ai"));
}

#[test]
fn init_leaves_no_temp_artifacts_behind() {
    // cmd_init replaces the router's exit handler with its own; a disarm on the
    // success path used to leave the run directory with no owner.
    let project = Project::empty();
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    project
        .exuno()
        .env("TMPDIR", &sandbox)
        .args(["init", "--no-detect"])
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}
