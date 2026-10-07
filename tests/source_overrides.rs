//! `tests/source_overrides.bats`: `source.*` layouts that live outside the
//! default `.ai/src` tree — relative and absolute `source.tools`, symlink
//! containment, and the `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` trust list.

mod common;

use common::Project;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn write_project_sources(project: &Project) {
    write(&project.join(".ai/src/AGENTS.md"), "# Project Agent\n");
    write(
        &project.join(".ai/src/rules/project.md"),
        "# Project Rule\n",
    );
    write(
        &project.join(".ai/src/skills/project-skill/SKILL.md"),
        "---\nname: project-skill\ndescription: project fixture skill\n---\n\n# Project Skill\n",
    );
}

/// Writes `.ai/agent_sync.yaml` enabling Claude, with `source.rules` set when given.
fn write_rules_config(project: &Project, rules_source: Option<&str>) {
    let mut content =
        String::from("format: 2\noutputs: committed\ntools:\n  enabled:\n    - claude\n");
    if let Some(src) = rules_source {
        content.push_str(&format!("source:\n  rules: \"{src}\"\n"));
    }
    write(&project.join(".ai/agent_sync.yaml"), &content);
}

fn make_outside_rules() -> tempfile::TempDir {
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("rules/outside.md"), "# Outside Rule\n");
    outside
}

/// Mirrors `write_external_fixture`. Returns the external config path.
fn write_external_fixture(
    project: &Project,
    tools_path: &str,
    skills_dest: Option<&str>,
) -> PathBuf {
    let skills_dest = skills_dest.unwrap_or(".external/skills");
    let tools_dir = if tools_path.starts_with('/') {
        PathBuf::from(tools_path)
    } else {
        project.join(tools_path)
    };

    write_project_sources(project);
    std::fs::create_dir_all(project.join("config")).unwrap();
    std::fs::create_dir_all(project.join("sources/rules")).unwrap();
    std::fs::create_dir_all(project.join("sources/skills/external-skill")).unwrap();
    std::fs::create_dir_all(tools_dir.join("claude")).unwrap();

    write(&project.join("sources/AGENTS.md"), "# External Agent\n");
    write(
        &project.join("sources/rules/external.md"),
        "# External Rule\n",
    );
    write(
        &project.join("sources/skills/external-skill/SKILL.md"),
        "---\nname: external-skill\ndescription: external fixture skill\n---\n\n# External Skill\n",
    );
    write(
        &tools_dir.join("claude/settings.json"),
        "{\"external\":true}\n",
    );
    write(
        &tools_dir.join("claude.yaml"),
        &format!(
            "name: \"External Claude\"\nenabled: false\ntargets:\n  agents:\n    enabled: false\n  rules:\n    enabled: false\n  skills:\n    dest: \"{skills_dest}\"\n  commands:\n    enabled: false\n  subagents:\n    enabled: false\n  settings:\n    dest: \".external/settings.json\"\n  mcp:\n    enabled: false\n  hooks:\n    enabled: false\n  guard:\n    enabled: false\n"
        ),
    );

    std::fs::create_dir_all(project.join(".ai")).unwrap();
    let config = project.join("config/agent_sync.yaml");
    write(
        &config,
        &format!(
            "format: 2\noutputs: committed\ntools:\n  enabled:\n    - claude\nsource:\n  agents: \"sources/AGENTS.md\"\n  rules: \"sources/rules\"\n  skills: \"sources/skills\"\n  tools: \"{tools_path}\"\n"
        ),
    );
    config
}

fn run_external_sync(project: &Project, config: &Path) -> assert_cmd::assert::Assert {
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", config)
        .arg("sync")
        .assert()
}

/// `_rm_rf_resilient`'s Rust equivalent is unnecessary: `tempfile::TempDir`
/// cleans up on drop.
fn absent_git_config() -> PathBuf {
    std::env::temp_dir().join("agentsync-tests-absent-gitconfig")
}

/// `git diff -- <paths>` against the un-added working tree, the way the bats
/// suite asserts idempotency: an untracked path never shows in `git diff`
/// output regardless of its content, so this checks the same (weak) thing
/// the original case did.
fn git_diff_empty(project: &Project, paths: &[&str]) -> bool {
    let output = std::process::Command::new("git")
        .arg("diff")
        .arg("--")
        .args(paths)
        .current_dir(project.path())
        .env("GIT_CONFIG_GLOBAL", absent_git_config())
        .env("GIT_CONFIG_SYSTEM", absent_git_config())
        .output()
        .unwrap();
    output.stdout.is_empty()
}

#[cfg(unix)]
fn symlink(target: &Path, link: &Path) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

#[cfg(windows)]
fn symlink(target: &Path, link: &Path) {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link).unwrap();
    } else {
        std::os::windows::fs::symlink_file(target, link).unwrap();
    }
}

#[test]
fn source_tools_relative_override_drives_tool_yaml_and_payloads() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);

    run_external_sync(&project, &config).success();

    assert!(project.exists(".external/skills/external-skill/SKILL.md"));
    assert_eq!(
        project.read(".external/settings.json"),
        "{\"external\":true}\n"
    );
    assert!(!project.exists("CLAUDE.md"));
    assert!(!project.exists(".claude/rules"));
    assert!(!project.exists(".claude/skills"));
    assert!(!project.exists(".mcp.json"));
}

// An absolute `source.tools` outside the project is not applied on Windows
// (open in the phase 6 receipt) — the bats case skips there too.
#[cfg(unix)]
#[test]
fn source_tools_absolute_override_drives_the_same_layout() {
    let project = Project::empty();
    let external_tools_root = tempfile::tempdir().unwrap();
    let config =
        write_external_fixture(&project, external_tools_root.path().to_str().unwrap(), None);

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            external_tools_root.path(),
        )
        .arg("sync")
        .assert()
        .success();

    assert!(project.exists(".external/skills/external-skill/SKILL.md"));
    assert_eq!(
        project.read(".external/settings.json"),
        "{\"external\":true}\n"
    );
    assert!(!project.exists("CLAUDE.md"));
    assert!(!project.exists(".claude/settings.json"));
}

#[test]
fn external_config_and_sources_remain_valid_through_isolated_check() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    run_external_sync(&project, &config).success();

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .arg("check")
        .assert()
        .success()
        .stdout(predicate::str::contains("synced"));
}

#[test]
fn isolated_check_reads_the_version_pin_from_an_external_config() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&config)
        .unwrap();
    {
        use std::io::Write;
        file.write_all(b"agentsync_version: \"0.0.0\"\n").unwrap();
    }

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .arg("check")
        .assert()
        .code(1)
        .stderr(predicate::str::contains("pins exuno 0.0.0"))
        .stdout(predicate::str::contains("Sync script failed during check").not());
}

#[test]
fn external_source_sync_is_idempotent_and_detects_source_drift() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    run_external_sync(&project, &config).success();
    run_external_sync(&project, &config).success();
    assert!(git_diff_empty(
        &project,
        &[".ai/.sync-manifest", ".gitignore"]
    ));

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(project.join("sources/skills/external-skill/SKILL.md"))
        .unwrap();
    {
        use std::io::Write;
        file.write_all(b"# changed externally\n").unwrap();
    }
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("out of sync"));
}

#[test]
fn source_tools_changes_trigger_sync_if_stale() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    run_external_sync(&project, &config).success();

    write(
        &project.join("sources/tools/claude/settings.json"),
        "{\"external\":false}\n",
    );
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["sync", "--if-stale"])
        .assert()
        .success();
    assert_eq!(
        project.read(".external/settings.json"),
        "{\"external\":false}\n"
    );
}

#[test]
fn show_and_doctor_resolve_an_external_tool_catalog() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["show", "claude"])
        .assert()
        .success()
        .stdout(predicate::str::contains("External Claude"));

    // The fixture's settings payload does not register the guard, so doctor
    // warns (exit 1) about the unwired guard script.
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .arg("doctor")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("External Claude"));
}

#[test]
fn external_source_layout_preserves_foreign_skills_and_sibling_files() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    write(
        &project.join(".external/skills/foreign-skill/SKILL.md"),
        "foreign skill\n",
    );
    write(
        &project.join(".external/foreign-plugin.js"),
        "foreign plugin\n",
    );

    run_external_sync(&project, &config).success();

    assert_eq!(
        project.read(".external/skills/foreign-skill/SKILL.md"),
        "foreign skill\n"
    );
    assert_eq!(
        project.read(".external/foreign-plugin.js"),
        "foreign plugin\n"
    );
}

#[test]
fn external_source_layout_cannot_widen_output_targets_through_traversal() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", Some("escape-link/skills"));
    let outside = tempfile::tempdir().unwrap();
    symlink(outside.path(), &project.join("escape-link"));

    run_external_sync(&project, &config)
        .success()
        .stderr(predicate::str::contains("outside repository root"));

    assert!(!outside.path().join("skills").exists());
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn profile_variants_use_the_configured_external_source_tools_directory() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();

    assert!(project.exists("sources/tools/claude-hub.yaml"));
    let content = project.read("sources/tools/claude-hub.yaml");
    assert!(content.lines().any(|l| l == "base: claude"));
}

#[test]
fn source_containment_default_config_refuses_a_ai_src_symlink_that_escapes_the_project() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    std::fs::remove_dir_all(project.join(".ai/src/rules")).unwrap();
    symlink(
        &outside.path().join("rules"),
        &project.join(".ai/src/rules"),
    );
    write_rules_config(&project, None);

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Source symlink .ai/src/rules resolves outside the project",
        ));
    assert!(!project.exists(".claude/rules/outside.md"));
}

#[test]
fn source_containment_an_explicit_in_project_source_rules_symlink_escaping_the_project_is_refused()
{
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    std::fs::remove_dir_all(project.join(".ai/src/rules")).unwrap();
    symlink(
        &outside.path().join("rules"),
        &project.join(".ai/src/rules"),
    );
    write_rules_config(&project, Some(".ai/src/rules"));

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Source symlink .ai/src/rules resolves outside the project",
        ));
    assert!(!project.exists(".claude/rules/outside.md"));
}

#[test]
fn source_containment_explicit_absolute_source_rules_outside_the_project_syncs_and_checks() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    let outside_rules = outside.path().join("rules");
    write_rules_config(&project, Some(outside_rules.to_str().unwrap()));

    project
        .exuno()
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            common::canonical_engine_path(&outside_rules),
        )
        .arg("sync")
        .assert()
        .success();
    assert!(project.exists(".claude/rules/outside.md"));
    assert!(!project.exists(".claude/rules/project.md"));

    project
        .exuno()
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            common::canonical_engine_path(&outside_rules),
        )
        .arg("check")
        .assert()
        .success();
}

#[test]
fn source_containment_an_outside_source_rules_not_listed_in_agentsync_external_source_roots_is_refused()
 {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    let outside_rules = outside.path().join("rules");
    write_rules_config(&project, Some(outside_rules.to_str().unwrap()));
    let other_root = tempfile::tempdir().unwrap();

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "source.rules points outside the project at",
        ))
        .stderr(predicate::str::contains(
            "which AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list",
        ));
    assert!(!project.exists(".claude"));

    let roots = common::path_list(&[
        "relative/path",
        &common::canonical_engine_path(other_root.path()),
    ]);
    project
        .exuno()
        .env("AGENTSYNC_EXTERNAL_SOURCE_ROOTS", roots)
        .arg("sync")
        .assert()
        .code(1);
    assert!(!project.exists(".claude"));
}

#[test]
fn source_containment_a_trusted_parent_directory_admits_every_source_below_it() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    let outside_rules = outside.path().join("rules");
    write_rules_config(&project, Some(outside_rules.to_str().unwrap()));

    let roots = common::path_list(&[
        "/nonexistent-agentsync-root",
        &common::canonical_engine_path(outside.path()),
    ]);
    project
        .exuno()
        .env("AGENTSYNC_EXTERNAL_SOURCE_ROOTS", roots)
        .arg("sync")
        .assert()
        .success();
    assert!(project.exists(".claude/rules/outside.md"));
}

#[test]
fn doctor_fails_an_outside_source_that_agentsync_external_source_roots_does_not_list() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    let outside_rules = outside.path().join("rules");
    write_rules_config(&project, Some(outside_rules.to_str().unwrap()));

    project
        .exuno()
        .arg("doctor")
        .assert()
        .code(2)
        .stdout(predicate::str::contains(
            "source.rules points outside the project and AGENTSYNC_EXTERNAL_SOURCE_ROOTS does not list it",
        ));
}

#[test]
fn source_containment_explicit_dot_dot_source_rules_resolves_from_the_project_root() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    let sibling_name = outside.path().file_name().unwrap().to_str().unwrap();
    write_rules_config(&project, Some(&format!("../{sibling_name}/rules")));

    project
        .exuno()
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            common::canonical_engine_path(outside.path()),
        )
        .arg("sync")
        .assert()
        .success();
    assert!(project.exists(".claude/rules/outside.md"));

    project
        .exuno()
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            common::canonical_engine_path(outside.path()),
        )
        .arg("check")
        .assert()
        .success();
}

#[test]
fn source_containment_explicit_source_root_at_slash_is_refused() {
    let project = Project::empty();
    write_project_sources(&project);
    write_rules_config(&project, Some("/"));

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        // The canonical spelling of / is C:/ on Windows; the refusal is the same.
        .stderr(predicate::str::contains(
            "source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: / ->",
        ));
    assert!(!project.exists(".claude"));
}

#[test]
fn source_containment_explicit_source_root_at_home_is_refused() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = tempfile::tempdir().unwrap();
    write(&outside.path().join("home.md"), "# Home Rule\n");
    write_rules_config(&project, Some(outside.path().to_str().unwrap()));

    project
        .exuno()
        .env("HOME", outside.path())
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "source.rules must not be the filesystem root, the home directory, or the project root or its ancestor",
        ));
    assert!(!project.exists(".claude/rules/home.md"));
}

#[test]
fn source_containment_explicit_source_root_at_a_project_ancestor_is_refused() {
    let project = Project::empty();
    write_project_sources(&project);
    write_rules_config(&project, Some(".."));

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "source.rules must not be the filesystem root, the home directory, or the project root or its ancestor: ..",
        ));
    assert!(!project.exists(".claude"));
}

#[test]
fn tool_resolver_ignores_an_auto_detected_flat_ai_tools_catalog_like_show_does() {
    let project = Project::empty();
    write_project_sources(&project);
    write_rules_config(&project, None);
    write(
        &project.join(".ai/tools/claude.yaml"),
        "name: \"Flat Claude\"\n",
    );

    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("[INFO] Syncing Claude Code"))
        .stderr(predicate::str::contains("Flat Claude").not());

    project
        .exuno()
        .args(["show", "claude"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Flat Claude").not());
}

#[test]
fn customize_refuses_to_write_into_an_external_source_tools_directory() {
    let project = Project::empty();
    let external_tools_root = tempfile::tempdir().unwrap();
    let config =
        write_external_fixture(&project, external_tools_root.path().to_str().unwrap(), None);

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["customize", "codex"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "source.tools resolves outside the project: {}",
            common::engine_path(external_tools_root.path())
        )));
    assert!(!external_tools_root.path().join("codex.yaml").exists());
}

#[test]
fn profile_remove_refuses_to_delete_from_an_external_source_tools_directory() {
    let project = Project::empty();
    let config = write_external_fixture(&project, "sources/tools", None);
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    write(
        &project.join("sources/tools/claude-hub/settings.json"),
        "{}\n",
    );
    let external_tools_root = tempfile::tempdir().unwrap();
    std::fs::rename(
        project.join("sources/tools"),
        external_tools_root.path().join("tools"),
    )
    .unwrap();
    let external_tools_dir = external_tools_root.path().join("tools");
    let config_content = std::fs::read_to_string(&config).unwrap();
    let config_content: String = config_content
        .lines()
        .map(|line| {
            if line.trim_start().starts_with("tools:") {
                format!("  tools: \"{}\"", common::engine_path(&external_tools_dir))
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    std::fs::write(&config, config_content).unwrap();

    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", &config)
        .args(["profile", "remove", "hub", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "source.tools resolves outside the project: {}",
            common::engine_path(&external_tools_dir)
        )));
    assert!(external_tools_dir.join("claude-hub.yaml").exists());
    assert!(external_tools_dir.join("claude-hub/settings.json").exists());
    assert!(
        std::fs::read_to_string(&config)
            .unwrap()
            .lines()
            .any(|l| l == "  hub:")
    );
}

#[test]
fn source_symlinks_a_rule_file_linking_outside_the_project_is_refused_before_any_write() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    symlink(
        &outside.path().join("rules/outside.md"),
        &project.join(".ai/src/rules/leak.md"),
    );
    write_rules_config(&project, None);

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Source symlink .ai/src/rules/leak.md resolves outside the project",
        ));
    assert!(!project.exists(".claude"));
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn source_symlinks_an_agents_md_linking_outside_the_project_is_refused() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    std::fs::rename(
        project.join(".ai/src/AGENTS.md"),
        project.join("AGENTS.local.md"),
    )
    .unwrap();
    symlink(
        &outside.path().join("rules/outside.md"),
        &project.join(".ai/src/AGENTS.md"),
    );
    write_rules_config(&project, None);

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "Source symlink .ai/src/AGENTS.md resolves outside the project",
        ));
    assert!(!project.exists("CLAUDE.md"));
}

#[test]
fn source_symlinks_a_link_to_a_safe_directory_is_followed_and_its_own_links_are_checked() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    write(
        &project.join("vendor/skills/vendored/SKILL.md"),
        "---\nname: vendored\ndescription: vendored skill\n---\n",
    );
    symlink(
        &outside.path().join("rules/outside.md"),
        &project.join("vendor/skills/vendored/leak.md"),
    );
    symlink(
        &project.join("vendor/skills/vendored"),
        &project.join(".ai/src/skills/vendored"),
    );
    write_rules_config(&project, None);

    project
        .exuno()
        .arg("sync")
        .assert()
        .code(1)
        .stderr(predicate::str::contains(
            "vendor/skills/vendored/leak.md resolves outside the project",
        ));
    assert!(!project.exists(".claude"));
}

#[test]
fn source_symlinks_links_that_stay_inside_the_project_keep_syncing() {
    let project = Project::empty();
    write_project_sources(&project);
    write(&project.join("docs/shared.md"), "# Shared Rule\n");
    symlink(
        &project.join("docs/shared.md"),
        &project.join(".ai/src/rules/shared.md"),
    );
    write_rules_config(&project, None);

    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".claude/rules/shared.md"), "# Shared Rule\n");
}

#[test]
fn source_symlinks_a_trusted_outside_target_is_read() {
    let project = Project::empty();
    write_project_sources(&project);
    let outside = make_outside_rules();
    symlink(
        &outside.path().join("rules/outside.md"),
        &project.join(".ai/src/rules/outside.md"),
    );
    write_rules_config(&project, None);

    project
        .exuno()
        .env(
            "AGENTSYNC_EXTERNAL_SOURCE_ROOTS",
            common::canonical_engine_path(outside.path()),
        )
        .arg("sync")
        .assert()
        .success();
    assert_eq!(project.read(".claude/rules/outside.md"), "# Outside Rule\n");
}
