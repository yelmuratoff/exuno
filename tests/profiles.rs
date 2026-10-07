//! `tests/profiles.bats`: config-home profiles — variant tools + per-profile
//! overlay.

mod common;

use common::Project;
use predicates::prelude::*;

/// `init`, `enable claude`: every case starts from this seeded tree.
fn seeded() -> Project {
    let project = Project::seeded(&["--no-detect", "--yes"]);
    project
        .exuno()
        .args(["enable", "claude"])
        .arg("--no-scaffold")
        .assert()
        .success();
    project
}

fn add_hub(project: &Project) {
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
}

#[test]
fn profile_add_scaffolds_a_thin_variant_tool_with_config_home_dests() {
    let project = seeded();
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    let text = project.read(".ai/src/tools/claude-hub.yaml");
    assert!(text.contains("base: claude"));
    assert!(text.contains("profile_home: \".claude-hub\""));
    assert!(text.contains("dest: \".claude-hub/rules\""));
    assert!(text.contains("dest: \".claude-hub/CLAUDE.md\""));
    assert!(text.contains("dest: \".claude-hub/.mcp.json\""));
}

#[test]
fn profile_add_refuses_minimax_project_files_before_writing() {
    let project = seeded();
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "minimax"])
        .assert()
        .code(2)
        .stderr(predicate::str::contains(
            "does not support config-home profiles",
        ));
    assert!(!project.exists(".ai/profiles/hub"));
    assert!(!project.exists(".ai/src/tools/minimax-hub.yaml"));
}

#[test]
fn sync_refuses_a_hand_written_minimax_profile() {
    let project = seeded();
    project.write(".ai/src/tools/minimax-hub.yaml", "base: minimax\n");
    project.append(
        ".ai/agent_sync.yaml",
        "\nprofiles:\n  hub:\n    overlay: \".ai/profiles/hub\"\n    active: true\n    tools: [minimax-hub]\n",
    );
    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "does not support config-home profiles",
        ));
    assert!(!project.exists(".mcp.json"));
}

#[test]
fn profile_add_nested_base_dest_keeps_internal_structure_not_basename() {
    let project = seeded();
    project
        .exuno()
        .args(["enable", "amazonq", "--no-scaffold"])
        .assert()
        .success();
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "amazonq"])
        .assert()
        .success();
    // amazonq's agents dest is .amazonq/rules/00-context.md — the rules/
    // segment must survive the rewrite into the config home.
    let text = project.read(".ai/src/tools/amazonq-hub.yaml");
    assert!(text.contains("dest: \".amazonq-hub/rules/00-context.md\""));
}

#[test]
fn profile_add_registers_a_profiles_block_in_agent_sync_yaml() {
    let project = seeded();
    add_hub(&project);
    let config = project.read(".ai/agent_sync.yaml");
    assert!(config.contains("\nprofiles:"));
    assert!(config.contains("claude-hub"));
}

#[test]
fn profile_add_second_profile_inserts_under_the_existing_profiles_block() {
    let project = seeded();
    add_hub(&project);
    project
        .exuno()
        .args(["profile", "add", "klara", "--tools", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["profile", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hub"))
        .stdout(predicate::str::contains("klara"));
    let config = project.read(".ai/agent_sync.yaml");
    assert_eq!(config.matches("\nprofiles:").count(), 1);
}

#[test]
fn sync_profile_produces_a_self_contained_config_home_directory() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude-hub/CLAUDE.md"));
    assert!(project.join(".claude-hub/rules").is_dir());
    assert!(project.exists(".claude-hub/.mcp.json"));
}

#[test]
fn sync_profile_output_does_not_touch_personal_tool_output() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("CLAUDE.md"));
    assert!(project.join(".claude/rules").is_dir());
}

#[test]
fn sync_profile_only_rule_lands_in_the_profile_output_not_personal() {
    let project = seeded();
    add_hub(&project);
    project.write(".ai/profiles/hub/src/rules/work-only.md", "work-only\n");
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude-hub/rules/work-only.md"));
    assert!(!project.exists(".claude/rules/work-only.md"));
}

#[test]
fn sync_base_rules_fill_into_profile_output_overlay_fill() {
    let project = seeded();
    add_hub(&project);
    project.write(".ai/src/rules/base-rule.md", "base-rule body\n");
    project.write(".ai/profiles/hub/src/rules/work-only.md", "work-only\n");
    project.exuno().arg("sync").assert().success();
    // Both the profile extra and the inherited base rule are present.
    assert!(project.exists(".claude-hub/rules/work-only.md"));
    assert!(project.exists(".claude-hub/rules/base-rule.md"));
}

#[test]
fn sync_a_profile_with_its_own_source_keeps_the_engine_skill_and_its_extension() {
    let project = seeded();
    add_hub(&project);
    project.write(
        ".ai/profiles/hub/src/skills/work/SKILL.md",
        "---\nname: work\ndescription: Work\n---\n",
    );
    project.write(
        ".ai/src/skills/meta/agentsync/SKILL.append.md",
        "Local notes.\n",
    );
    project.exuno().arg("sync").assert().success();

    for home in [".claude", ".claude-hub"] {
        let skill = project.read(&format!("{home}/skills/agentsync/SKILL.md"));
        assert!(skill.starts_with("---\nname: agentsync\n"), "{home}");
        assert!(skill.ends_with("\n\nLocal notes.\n"), "{home}");
        assert!(project.exists(&format!(
            "{home}/skills/agentsync/references/maintenance.md"
        )));
    }
    assert!(project.exists(".claude-hub/skills/work/SKILL.md"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_profile_filters_skills_by_category_and_shadows_by_name() {
    let project = seeded();
    add_hub(&project);
    let skill = |name: &str, body: &str| format!("---\nname: {name}\ndescription: {body}\n---\n");
    project.write(
        ".ai/src/skills/flutter/bloc/SKILL.md",
        &skill("bloc", "Base"),
    );
    project.write(
        ".ai/src/skills/backend/auth/SKILL.md",
        &skill("auth", "Base"),
    );
    project.write(
        ".ai/profiles/hub/src/skills/work/auth/SKILL.md",
        &skill("auth", "Profile"),
    );
    let variant = project.read(".ai/src/tools/claude-hub.yaml");
    let dest_line = variant
        .lines()
        .find(|line| line.trim() == "dest: \".claude-hub/skills\"")
        .expect("variant skills dest");
    let indent = &dest_line[..dest_line.len() - dest_line.trim_start().len()];
    project.write(
        ".ai/src/tools/claude-hub.yaml",
        &variant.replace(
            dest_line,
            &format!("{dest_line}\n{indent}include: \"backend/* work/*\""),
        ),
    );

    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".claude-hub/skills/auth/SKILL.md"),
        skill("auth", "Profile")
    );
    assert!(!project.exists(".claude-hub/skills/bloc"));
    assert_eq!(
        project.read(".claude/skills/auth/SKILL.md"),
        skill("auth", "Base")
    );
    assert!(project.exists(".claude/skills/bloc/SKILL.md"));
}

#[test]
fn sync_profile_wins_on_path_collision_with_base() {
    let project = seeded();
    add_hub(&project);
    project.write(".ai/src/rules/clash.md", "BASE VERSION\n");
    project.write(".ai/profiles/hub/src/rules/clash.md", "PROFILE VERSION\n");
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".claude-hub/rules/clash.md")
            .contains("PROFILE VERSION")
    );
}

#[test]
fn sync_profile_flag_syncs_only_the_named_profile() {
    let project = seeded();
    add_hub(&project);
    project
        .exuno()
        .args(["profile", "add", "klara", "--tools", "claude"])
        .assert()
        .success();
    project
        .exuno()
        .args(["sync", "--profile", "hub"])
        .assert()
        .success();
    assert!(project.join(".claude-hub").is_dir());
    assert!(!project.join(".claude-klara").is_dir());
}

#[test]
fn sync_a_plain_run_syncs_every_active_profile() {
    let project = seeded();
    add_hub(&project);
    project
        .exuno()
        .args(["profile", "add", "klara", "--tools", "claude"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(project.join(".claude-hub").is_dir());
    assert!(project.join(".claude-klara").is_dir());
}

#[test]
fn sync_variant_inherits_behaviour_flags_from_base_base_fallback() {
    // Codex inlines rules into AGENTS.md and ships no rules dir — the variant
    // must inherit that, proving unset fields resolve through base:.
    let project = seeded();
    project
        .exuno()
        .args(["enable", "codex", "--no-scaffold"])
        .assert()
        .success();
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "codex"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".codex-hub/AGENTS.md"));
    assert!(!project.join(".codex-hub/rules").is_dir());
}

#[test]
fn sync_profile_dests_are_gitignored() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    assert!(project.read(".gitignore").contains("claude-hub"));
}

#[test]
fn sync_editing_a_profile_output_is_detected_as_drift() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    project.append(".claude-hub/CLAUDE.md", "manual edit\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
}

#[test]
fn sync_idempotent_across_runs_with_a_profile() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    project.exuno().arg("sync").assert().success();
    project.exuno().arg("check").assert().success();
}

#[test]
fn sync_profile_and_shared_overlays_compose() {
    // Parent project with a custom rule; this project inherits it via
    // shared: AND layers a profile on top.
    let project = seeded();
    let parent = project.join("parent");
    std::fs::create_dir_all(&parent).unwrap();
    let mut init_in_parent = assert_cmd::Command::new(env!("CARGO_BIN_EXE_exuno"));
    init_in_parent.current_dir(&parent);
    common::scrub(&mut init_in_parent);
    init_in_parent
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    project.write("parent/.ai/src/rules/parent-only.md", "from-parent\n");

    project.append(
        ".ai/agent_sync.yaml",
        "\nshared:\n  path: \"./parent\"\n  inherit: rules\n",
    );
    add_hub(&project);
    project.write(
        ".ai/profiles/hub/src/rules/profile-only.md",
        "from-profile\n",
    );
    project.exuno().arg("sync").assert().success();
    // Parent (shared) + profile extra both materialise into the profile
    // output.
    assert!(project.exists(".claude-hub/rules/parent-only.md"));
    assert!(project.exists(".claude-hub/rules/profile-only.md"));
}

#[test]
fn sync_tears_down_profile_overlay_tmpdir_no_leaked_dirs() {
    let project = seeded();
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    add_hub(&project);
    project.write(".ai/profiles/hub/src/rules/x.md", "x\n");
    project
        .exuno()
        .env("TMPDIR", &sandbox)
        .arg("sync")
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn profile_remove_deletes_config_home_output_variant_file_and_config_entry() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    assert!(project.join(".claude-hub").is_dir());
    project
        .exuno()
        .args(["profile", "remove", "hub", "--yes"])
        .assert()
        .success();
    assert!(!project.join(".claude-hub").is_dir());
    assert!(!project.exists(".ai/src/tools/claude-hub.yaml"));
    // Last profile gone — the empty profiles: header is cleaned up too.
    assert!(!project.read(".ai/agent_sync.yaml").contains("\nprofiles:"));
}

#[test]
fn profile_remove_keeps_files_agentsync_did_not_generate_in_the_config_home() {
    let project = seeded();
    add_hub(&project);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude-hub/CLAUDE.md"));
    project.write(".claude-hub/.credentials.json", "{\"token\":\"t\"}\n");
    project
        .exuno()
        .args(["profile", "remove", "hub", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "kept .claude-hub/ (it holds files Exuno did not generate)",
        ));
    assert!(!project.exists(".claude-hub/CLAUDE.md"));
    assert_eq!(
        project.read(".claude-hub/.credentials.json"),
        "{\"token\":\"t\"}\n"
    );
}

#[test]
fn sync_profile_flag_missing_value_is_a_usage_error() {
    let project = seeded();
    project.exuno().args(["sync", "--profile"]).assert().code(1);
}

#[test]
fn profile_add_scaffolds_a_readme_not_empty_overlay_dirs() {
    let project = seeded();
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    assert!(project.exists(".ai/profiles/hub/README.md"));
    assert!(!project.join(".ai/profiles/hub/src/rules").is_dir());
    assert!(!project.join(".ai/profiles/hub/src/skills").is_dir());
}
