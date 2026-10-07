//! `tests/doctor.bats`: `exuno doctor`, plus the two `list`/`init` cases
//! bats colocated in the same file (payload-override column, version pin).
//! Doctor writes everything to stdout, `main.rs`'s `cli::doctor::doctor` call.

mod common;

use std::path::Path;
use std::process::Command as StdCommand;

use assert_cmd::Command;
use common::Project;
use predicates::prelude::*;

fn doctor(project: &Project) -> assert_cmd::assert::Assert {
    project.exuno().arg("doctor").assert()
}

fn doctor_at(dir: &Path) -> assert_cmd::assert::Assert {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command.arg("doctor").assert()
}

fn init_at(dir: &Path, args: &[&str]) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_exuno"));
    command.current_dir(dir);
    common::scrub(&mut command);
    command.arg("init").args(args).assert().success();
}

/// Same identity `test_helper.bash` uses, pointed at a git config that does
/// not exist so the developer's global config cannot decide the test.
fn git_init_at(dir: &Path) {
    let absent = std::env::temp_dir().join("agentsync-tests-absent-gitconfig");
    let run = |args: &[&str]| {
        let status = StdCommand::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", &absent)
            .env("GIT_CONFIG_SYSTEM", &absent)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed in {dir:?}");
    };
    run(&["init", "--quiet"]);
    run(&["config", "user.email", "test@test.com"]);
    run(&["config", "user.name", "Test"]);
}

/// `pin_version`: rewrite the `exuno_version:` line in place.
fn pin_version(project: &Project, version: &str) {
    let config = project.read(".ai/exuno.yaml");
    let rewritten: String = config
        .lines()
        .map(|line| {
            if line.starts_with("exuno_version:") {
                format!("exuno_version: \"{version}\"")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    project.write(".ai/exuno.yaml", &rewritten);
}

#[test]
fn doctor_names_what_still_carries_the_agentsync_name() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Deploy\nmetadata:\n  agentsync-use-when: Shipping\n---\n",
    );
    project.write(
        ".git/hooks/post-merge",
        "#!/bin/sh\n# >>> AGENTSYNC AUTO SYNC START >>>\nagentsync sync\n# <<< AGENTSYNC AUTO SYNC END <<<\n",
    );
    doctor(&project)
        .stdout(predicate::str::contains(
            "! metadata.agentsync-* → metadata.exuno-* in .ai/src/skills/deploy/SKILL.md — run exuno migrate\n",
        ))
        .stdout(predicate::str::contains(
            "! .git/hooks/post-merge still runs the agentsync block — run exuno setup-hooks\n",
        ));
}

#[test]
fn doctor_fails_with_exit_2_when_ai_missing() {
    doctor(&Project::empty())
        .code(2)
        .stdout(predicate::str::contains(".ai/ directory missing"));
}

#[test]
fn doctor_passes_on_fresh_init() {
    doctor(&Project::seeded(&[]))
        .success()
        .stdout(predicate::str::contains("All checks passed"));
}

#[test]
fn doctor_advises_about_the_file_a_disabled_target_left_behind() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    project.write(
        ".ai/src/tools/claude.yaml",
        "targets:\n  agents:\n    enabled: false\n",
    );
    doctor(&project).success().stdout(predicate::str::contains(
        "CLAUDE.md — left from claude targets.agents, which is disabled",
    ));
}

#[test]
fn doctor_reports_enabled_tools() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("Claude Code"));
}

#[test]
fn doctor_shows_customization_marker() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["customize", "claude"])
        .assert()
        .success();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("customized"));
}

#[test]
fn doctor_flags_legacy_enabled_true() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/claude.yaml", "enabled: true\n");
    doctor(&project)
        .code(1)
        .stdout(predicate::str::contains("legacy"));
}

#[test]
fn doctor_reports_source_directories() {
    let project = Project::seeded(&[]);
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".ai/src/AGENTS.md"))
        .stdout(predicate::str::contains(".ai/src/rules"));
}

#[test]
fn doctor_fails_with_exit_2_when_the_default_agents_md_source_is_missing() {
    let project = Project::seeded(&[]);
    std::fs::remove_file(project.join(".ai/src/AGENTS.md")).unwrap();
    std::fs::remove_dir_all(project.join(".ai/src/rules")).unwrap();
    doctor(&project).code(2).stdout(
        predicate::str::contains(".ai/src/AGENTS.md missing (required)")
            .and(predicate::str::contains(
                ".ai/src/rules not present (optional)",
            ))
            .and(predicate::str::contains(
                "No AGENTS.md in .ai/src/ or .ai/ — sync will fail",
            )),
    );
}

#[test]
fn doctor_checks_explicit_external_sources_at_their_configured_location() {
    let project = Project::seeded(&[]);
    let outside = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(outside.path().join("rules")).unwrap();
    let outside_str = common::engine_path(outside.path());
    // Overwrite the config wholesale, as the bats fixture does with `>`.
    project.write(
        ".ai/exuno.yaml",
        &format!(
            "format: 3\ntools:\n  enabled: []\nsource:\n  agents: \"{outside_str}/AGENTS.md\"\n  rules: \"{outside_str}/rules\"\n"
        ),
    );

    project
        .exuno()
        .env("AGENTSYNC_EXTERNAL_SOURCE_ROOTS", &outside_str)
        .arg("doctor")
        .assert()
        .code(2)
        .stdout(predicate::str::contains(format!(
            "{outside_str}/AGENTS.md missing (required)"
        )));

    std::fs::write(outside.path().join("AGENTS.md"), "# External\n").unwrap();

    project
        .exuno()
        .env("AGENTSYNC_EXTERNAL_SOURCE_ROOTS", &outside_str)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "✓ {outside_str}/AGENTS.md"
        )))
        .stdout(predicate::str::contains(format!("✓ {outside_str}/rules")));
}

#[test]
fn doctor_catches_planted_github_pat_in_mcp_override() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/tools/claude/mcp.json",
        "{\"mcpServers\":{\"gh\":{\"env\":{\"TOKEN\":\"ghp_abcdefghijklmnopqrstuvwxyz012345678901\"}}}}\n",
    );
    doctor(&project)
        .code(2)
        .stdout(predicate::str::contains("possible secret"));
}

#[test]
fn doctor_catches_aws_access_key() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/tools/claude/settings.json",
        "{\"aws\":{\"key\":\"AKIAIOSFODNN7EXAMPLE\"}}\n",
    );
    doctor(&project)
        .code(2)
        .stdout(predicate::str::contains("possible secret"));
}

#[test]
fn doctor_allows_var_placeholders_in_mcp_overrides() {
    let project = Project::seeded(&[]);
    project.write(
        ".ai/src/tools/claude/mcp.json",
        "{\"mcpServers\":{\"gh\":{\"env\":{\"TOKEN\":\"${GITHUB_TOKEN}\"}}}}\n",
    );
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("possible secret").not());
}

#[test]
fn doctor_flags_invalid_json_override() {
    // The Rust engine validates JSON itself (no python3/node dependency), so
    // unlike the bats case this never skips.
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/cursor/mcp.json", "{\"broken\":\n");
    doctor(&project)
        .code(2)
        .stdout(predicate::str::contains("invalid JSON"));
}

#[test]
fn doctor_warns_about_legacy_flat_layout_payload_overrides() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/mcp/cursor.json", "{\"mcpServers\":{}}\n");
    doctor(&project)
        .code(1)
        .stdout(predicate::str::contains("Legacy payload layout"));
}

#[test]
fn list_shows_payload_override_column_when_hooks_override_exists() {
    let project = Project::seeded(&["--tools", "cursor"]);
    project
        .exuno()
        .args(["customize", "cursor", "hooks", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .arg("list")
        .assert()
        .success()
        .stdout(predicate::str::contains("H*"))
        .stdout(predicate::str::contains("payload override"));
}

#[test]
fn init_pins_exuno_version_in_agent_sync_yaml() {
    let project = Project::seeded(&["--no-detect"]);
    assert!(
        project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line.starts_with("exuno_version:"))
    );
}

#[test]
fn doctor_warns_when_pinned_version_differs_from_cli() {
    let project = Project::seeded(&["--no-detect"]);
    pin_version(&project, "0.0.1");
    doctor(&project)
        .code(1)
        .stdout(predicate::str::contains("pinned"))
        .stdout(predicate::str::contains("upgrade-config"));
}

#[test]
fn upgrade_config_bumps_pinned_version_to_current_cli() {
    let project = Project::seeded(&["--no-detect"]);
    pin_version(&project, "0.0.1");
    project.exuno().arg("upgrade-config").assert().success();
    assert!(
        !project
            .read(".ai/exuno.yaml")
            .contains("exuno_version: \"0.0.1\"")
    );
    doctor(&project).stdout(predicate::str::contains("differs from pinned").not());
}

#[test]
fn doctor_shows_edit_paths_section_for_enabled_tools() {
    let project = Project::seeded(&["--no-detect"]);
    // Plain `enable`, not the no-scaffold helper: this asserts the scaffolded
    // settings.json path, which `--no-scaffold` would never create.
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("Edit paths"))
        .stdout(predicate::str::contains(
            ".ai/src/tools/claude/settings.json",
        ))
        .stdout(predicate::str::contains("exuno add mcp"));
}

#[test]
fn doctor_edit_paths_shows_customize_hint_when_no_override_exists() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["cursor"]);
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("Edit paths"))
        .stdout(predicate::str::contains("exuno customize cursor hooks"));
}

#[test]
fn doctor_edit_paths_points_at_shared_mcp_json_when_configured() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["claude"]);
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".ai/src/mcp.json"))
        .stdout(predicate::str::contains("(shared)"));
}

#[test]
fn doctor_skips_edit_paths_section_when_no_tools_enabled() {
    let project = Project::seeded(&["--no-detect"]);
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("Edit paths").not());
}

#[test]
fn doctor_advises_on_empty_skill_directory_no_skill_md() {
    let project = Project::seeded(&["--no-detect"]);
    std::fs::create_dir_all(project.join(".ai/src/skills/empty-one")).unwrap();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("empty-one/ — missing SKILL.md"))
        .stdout(predicate::str::contains("advisory"));
}

#[test]
fn doctor_advises_when_a_project_copy_replaces_the_bundled_skill() {
    let project = Project::seeded(&["--no-detect"]);
    let replaced = "skills/meta/exuno/ — replaces the bundled skill";
    project.write(".ai/src/skills/meta/exuno/SKILL.append.md", "Notes.\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(replaced).not());

    project.write(
        ".ai/src/skills/meta/exuno/SKILL.md",
        "---\nname: exuno\ndescription: Mine\n---\n",
    );
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(replaced));

    project.append(".ai/exuno.yaml", "base_skills: false\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(replaced).not());
}

#[test]
fn doctor_accepts_categories_and_warns_on_a_name_two_skills_share() {
    let project = Project::seeded(&["--no-detect"]);
    let skill = "---\nname: auth\ndescription: Auth\n---\n";
    project.write(".ai/src/skills/backend/auth/SKILL.md", skill);
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("backend/ — missing SKILL.md").not())
        .stdout(predicate::str::contains(
            "All skill directories contain SKILL.md",
        ));

    project.write(".ai/src/skills/flutter/auth/SKILL.md", skill);
    doctor(&project).stdout(predicate::str::contains(
        "skill name 'auth' is claimed by skills/backend/auth/, skills/flutter/auth/ — exuno sync refuses it; rename one",
    ));
}

#[test]
fn doctor_advises_on_a_category_name_add_would_refuse() {
    let project = Project::seeded(&["--no-detect"]);
    project.write(
        ".ai/src/skills/My_Cat/tool/SKILL.md",
        "---\nname: tool\ndescription: Tool\n---\n",
    );
    doctor(&project).success().stdout(predicate::str::contains(
        "skills/My_Cat/ — category name is not lowercase-kebab",
    ));
}

#[test]
fn doctor_advises_on_legacy_agent_directory() {
    let project = Project::seeded(&["--no-detect"]);
    project.write(".agent/AGENTS.md", "legacy\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".agent/ — legacy pre-v0.6 layout"));
}

#[test]
fn doctor_flags_agent_even_when_antigravity_is_enabled() {
    // Antigravity moved its output to `.agents/` (plural); `.agent/` (singular)
    // is purely the pre-v0.6 layout regardless of which tools are enabled.
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["antigravity"]);
    project.write(".agent/AGENTS.md", "stale\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".agent/ — legacy pre-v0.6 layout"));
}

#[test]
fn doctor_does_not_flag_agents_when_antigravity_is_enabled() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["antigravity"]);
    std::fs::create_dir_all(project.join(".agents/rules")).unwrap();
    std::fs::create_dir_all(project.join(".agents/skills")).unwrap();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".agents/ — orphan").not());
}

#[test]
fn doctor_advises_on_orphan_tool_output_dir_for_disabled_tool() {
    let project = Project::seeded(&["--no-detect"]);
    std::fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".cursor/ — orphan"));
}

#[test]
fn doctor_does_not_flag_cursor_when_cursor_is_enabled() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["cursor"]);
    std::fs::create_dir_all(project.join(".cursor/rules")).unwrap();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".cursor/ — orphan").not());
}

#[test]
fn doctor_recognizes_kimi_code_and_opencode_output_ownership() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["kimi", "opencode"]);
    std::fs::create_dir_all(project.join(".kimi-code/skills")).unwrap();
    std::fs::create_dir_all(project.join(".opencode/skills")).unwrap();
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(".kimi-code/ — orphan").not())
        .stdout(predicate::str::contains(".opencode/ — orphan").not());
}

#[test]
fn doctor_accepts_opencode_settings_mcp_when_the_mcp_target_is_disabled() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["opencode"]);
    project.write(".ai/src/tools/opencode/settings.json", "{\"mcp\":{}}\n");
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    project.write(
        ".ai/src/tools/opencode.yaml",
        "targets:\n  mcp:\n    enabled: false\n",
    );
    doctor(&project).stdout(predicate::str::contains("MCP ownership conflict").not());
}

#[test]
fn doctor_fails_when_opencode_settings_and_canonical_mcp_both_own_mcp() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["opencode"]);
    project.write(".ai/src/tools/opencode/settings.json", "{\"mcp\":{}}\n");
    project.write(".ai/src/mcp.json", "{\"mcpServers\":{}}\n");
    doctor(&project)
        .code(2)
        .stdout(predicate::str::contains("OpenCode MCP ownership conflict"))
        .stdout(predicate::str::contains(
            ".ai/src/tools/opencode/settings.json",
        ))
        .stdout(predicate::str::contains(".ai/src/mcp.json"));
}

#[test]
fn doctor_advises_that_kimi_hooks_are_global_only() {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["kimi"]);
    project.write(".ai/src/tools/kimi/hooks.toml", "[hooks]\n");
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("Kimi hooks are global-only"))
        .stdout(predicate::str::contains("KIMI_CODE_HOME/config.toml"))
        .stdout(predicate::str::contains("leaves it untouched"));
}

#[test]
fn doctor_detects_identical_hash_duplicate_against_parent_ai_src() {
    // Parent has rules/shared.md; child below it has an identical file.
    // Child's doctor should flag it as a duplicate and point at `dedupe`.
    let project = Project::empty();
    let parent_dir = project.join("parent");
    let child_dir = parent_dir.join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    init_at(&parent_dir, &["--no-detect"]);
    std::fs::write(
        parent_dir.join(".ai/src/rules/shared.md"),
        "shared content\n",
    )
    .unwrap();
    std::fs::create_dir_all(&child_dir).unwrap();
    init_at(&child_dir, &["--no-detect"]);
    std::fs::copy(
        parent_dir.join(".ai/src/rules/shared.md"),
        child_dir.join(".ai/src/rules/shared.md"),
    )
    .unwrap();

    doctor_at(&child_dir)
        .success()
        .stdout(predicate::str::contains(
            "rules/shared.md — duplicate of parent",
        ))
        .stdout(predicate::str::contains("exuno dedupe"));
}

#[test]
fn doctor_flags_divergent_shared_file_as_info_not_advisory() {
    let project = Project::empty();
    let parent_dir = project.join("parent");
    let child_dir = parent_dir.join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    init_at(&parent_dir, &["--no-detect"]);
    std::fs::write(
        parent_dir.join(".ai/src/rules/shared.md"),
        "parent version\n",
    )
    .unwrap();
    std::fs::create_dir_all(&child_dir).unwrap();
    init_at(&child_dir, &["--no-detect"]);
    std::fs::write(child_dir.join(".ai/src/rules/shared.md"), "child version\n").unwrap();

    doctor_at(&child_dir)
        .success()
        .stdout(predicate::str::contains(
            "rules/shared.md — diverges from parent",
        ));
}

#[test]
fn doctor_honors_shared_path_across_git_boundary_asymmetric_repro() {
    // Outer + inner with separate .git: walk-up alone would stop at inner's
    // boundary and miss the parent. Declaring shared.path in inner makes the
    // parent explicit, so doctor must use it regardless of the git boundary.
    let project = Project::empty();
    let outer = project.join("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&outer).unwrap();
    git_init_at(&outer);
    init_at(&outer, &["--no-detect"]);
    std::fs::write(outer.join(".ai/src/rules/shared.md"), "shared content\n").unwrap();

    std::fs::create_dir_all(&inner).unwrap();
    git_init_at(&inner);
    init_at(&inner, &["--no-detect"]);
    std::fs::copy(
        outer.join(".ai/src/rules/shared.md"),
        inner.join(".ai/src/rules/shared.md"),
    )
    .unwrap();
    let mut config = std::fs::read_to_string(inner.join(".ai/exuno.yaml")).unwrap();
    config.push_str("\nshared:\n  path: \"../\"\n  inherit: rules\n");
    std::fs::write(inner.join(".ai/exuno.yaml"), config).unwrap();

    doctor_at(&inner)
        .success()
        .stdout(predicate::str::contains(
            "rules/shared.md — duplicate of parent",
        ))
        .stdout(predicate::str::contains("(from shared.path)"))
        .stdout(predicate::str::contains("No parent .ai/src/ found").not());
}

#[test]
fn doctor_walk_up_stops_at_git_boundary() {
    // Outer project with .ai/src/, but the inner has its own .git — the
    // walk-up must NOT traverse out of the inner's git repo.
    let project = Project::empty();
    let outer = project.join("outer");
    let inner = outer.join("inner");
    std::fs::create_dir_all(&outer).unwrap();
    git_init_at(&outer);
    init_at(&outer, &["--no-detect"]);
    std::fs::write(outer.join(".ai/src/rules/shared.md"), "would be dupe\n").unwrap();

    std::fs::create_dir_all(&inner).unwrap();
    git_init_at(&inner);
    init_at(&inner, &["--no-detect"]);
    std::fs::copy(
        outer.join(".ai/src/rules/shared.md"),
        inner.join(".ai/src/rules/shared.md"),
    )
    .unwrap();

    doctor_at(&inner)
        .success()
        .stdout(predicate::str::contains("duplicate of parent").not())
        .stdout(predicate::str::contains("No parent .ai/src/ found"));
}

#[test]
fn doctor_advises_path_scoping_when_always_on_rules_exceed_the_budget() {
    let project = Project::seeded(&[]);
    for entry in std::fs::read_dir(project.join(".ai/src/rules")).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let big = "x".repeat(6000);
    for i in 1..=4 {
        project.write(
            &format!(".ai/src/rules/bloat-{i}.md"),
            &format!("# Rule {i}\n\n- {big}\n"),
        );
    }
    doctor(&project)
        .success()
        .stdout(predicate::str::contains(
            "always-on rule(s) load on every task",
        ))
        .stdout(predicate::str::contains("paths:"));
}

#[test]
fn doctor_does_not_count_paths_scoped_rules_toward_always_on_bloat() {
    let project = Project::seeded(&[]);
    for entry in std::fs::read_dir(project.join(".ai/src/rules")).unwrap() {
        std::fs::remove_file(entry.unwrap().path()).unwrap();
    }
    let big = "x".repeat(30000);
    project.write(
        ".ai/src/rules/scoped-big.md",
        &format!("---\npaths:\n  - \"**/*.ts\"\n---\n\n# Scoped\n\n- {big}\n"),
    );
    doctor(&project)
        .success()
        .stdout(predicate::str::contains("always-on rule(s) load on every task").not());
}

#[test]
fn doctor_the_summary_follows_a_blank_line_with_no_rule() {
    doctor(&Project::seeded(&[]))
        .success()
        .stdout(predicate::str::ends_with("\n\n  All checks passed.\n\n"))
        .stdout(predicate::str::contains("─").not());
}

#[test]
fn doctor_help_is_answered_on_stdout_without_a_project() {
    Project::empty()
        .exuno()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno doctor — validate setup and surface warnings\n\n  USAGE\n    exuno doctor\n",
        ))
        .stdout(predicate::str::contains("\n  EXIT STATUS\n"))
        .stderr("");
}
