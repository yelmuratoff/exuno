//! `tests/base_skills.bats`: engine-owned skills. Content documenting
//! Exuno itself is resolved from the install dir at sync time, so
//! upgrading the engine updates it in every project. A project copy still
//! wins, and `base_skills: false` opts out.

mod common;

use common::Project;
use predicates::prelude::*;

fn synced_project() -> Project {
    Project::seeded(&["--tools", "claude", "--yes"])
}

fn set_config(project: &Project, line: &str) {
    project.append(".ai/exuno.yaml", &format!("{line}\n"));
}

#[test]
fn base_skills_the_exuno_skill_reaches_outputs_without_living_in_ai_src() {
    let project = synced_project();
    assert!(!project.join(".ai/src/skills/exuno").exists());
    assert!(project.exists(".claude/skills/exuno/SKILL.md"));
    assert!(
        project
            .read(".claude/skills/exuno/SKILL.md")
            .contains("Exuno")
    );
}

#[test]
fn base_skills_init_no_longer_scaffolds_it_as_project_content() {
    let project = synced_project();
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(!project.join(".ai/src/skills/exuno").exists());
}

#[test]
fn base_skills_nested_reference_files_come_along() {
    let project = synced_project();
    assert!(project.exists(".claude/skills/exuno/references/writing-skills.md"));
    assert!(project.exists(".claude/skills/exuno/references/maintenance.md"));
}

#[test]
fn base_skills_it_is_a_tracked_output_like_any_other() {
    let project = synced_project();
    assert!(
        project
            .read(".ai/.sync-manifest")
            .lines()
            .any(|line| line.starts_with(".claude/skills/exuno/SKILL.md\t"))
    );
}

#[test]
fn base_skills_the_projects_own_copy_wins() {
    let project = synced_project();
    project.write(
        ".ai/src/skills/exuno/SKILL.md",
        "---\nname: exuno\ndescription: Project version\n---\n\nPROJECT OVERRIDE\n",
    );
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(
        project
            .read(".claude/skills/exuno/SKILL.md")
            .contains("PROJECT OVERRIDE")
    );
}

#[test]
fn base_skills_the_projects_categorized_copy_wins_without_a_collision() {
    let project = synced_project();
    project.write(
        ".ai/src/skills/meta/exuno/SKILL.md",
        "---\nname: exuno\ndescription: Project version\n---\n\nPROJECT OVERRIDE\n",
    );
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(
        project
            .read(".claude/skills/exuno/SKILL.md")
            .contains("PROJECT OVERRIDE")
    );
    assert!(!project.exists(".claude/skills/exuno/references"));
}

#[test]
fn base_skills_a_categorized_extension_adds_to_the_engine_skill_and_keeps_updating() {
    let project = synced_project();
    project.write(
        ".ai/src/skills/meta/exuno/SKILL.append.md",
        "## Team notes\n\nRead `references/team.md` before a release.\n",
    );
    project.write(".ai/src/skills/meta/exuno/references/team.md", "TEAM\n");
    project.exuno().args(["sync", "--force"]).assert().success();

    let skill = project.read(".claude/skills/exuno/SKILL.md");
    assert!(skill.starts_with("---\nname: exuno\n"));
    assert!(skill.ends_with("\n\n## Team notes\n\nRead `references/team.md` before a release.\n"));
    assert_eq!(
        project.read(".claude/skills/exuno/references/team.md"),
        "TEAM\n"
    );
    assert!(project.exists(".claude/skills/exuno/references/maintenance.md"));
    assert!(!project.exists(".claude/skills/exuno/SKILL.append.md"));
    project.exuno().arg("check").assert().success();
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("missing SKILL.md").not());
}

#[test]
fn base_skills_base_skills_false_leaves_it_out_entirely() {
    let project = synced_project();
    set_config(&project, "base_skills: false");
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(!project.join(".claude/skills/exuno").exists());
}

#[test]
fn base_skills_the_projects_other_skills_are_untouched() {
    let project = synced_project();
    assert!(project.exists(".ai/src/skills/commit"));
    assert!(project.exists(".claude/skills/commit/SKILL.md"));
}

#[test]
fn base_skills_shared_inheritance_preserves_child_and_parent_skills_together() {
    let project = synced_project();
    project.write(".ai/src/skills/child-only/SKILL.md", "child skill\n");
    project.write(
        "shared/.ai/src/skills/parent-only/SKILL.md",
        "parent skill\n",
    );
    set_config(&project, "shared:\n  path: shared\n  inherit: skills");

    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".claude/skills/child-only/SKILL.md"),
        "child skill\n"
    );
    assert_eq!(
        project.read(".claude/skills/parent-only/SKILL.md"),
        "parent skill\n"
    );
    assert!(project.exists(".claude/skills/exuno/SKILL.md"));
}

#[test]
fn base_skills_a_second_sync_is_byte_identical_no_drift() {
    let project = synced_project();
    let before = project.sha256(".claude/skills/exuno/SKILL.md");
    project.exuno().arg("sync").assert().success();
    let after = project.sha256(".claude/skills/exuno/SKILL.md");
    assert_eq!(before, after);
}

#[test]
fn base_skills_check_stays_green_with_the_layer_active() {
    let project = synced_project();
    project.exuno().arg("check").assert().success();
}

#[test]
fn base_skills_removing_the_engine_skill_from_a_project_prunes_the_output() {
    let project = synced_project();
    assert!(project.exists(".claude/skills/exuno/SKILL.md"));
    set_config(&project, "base_skills: false");
    project.exuno().args(["sync", "--force"]).assert().success();
    assert!(!project.exists(".claude/skills/exuno/SKILL.md"));
}

#[test]
fn base_skills_the_overlay_leaves_no_temp_directory_behind() {
    let project = synced_project();
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();
    project
        .exuno()
        .env("TMPDIR", &sandbox)
        .args(["sync", "--force"])
        .assert()
        .success();
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}
