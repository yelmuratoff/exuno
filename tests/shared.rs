//! `tests/shared.bats`: the `shared:` resources concept — materializing
//! parent `.ai/src/` files into child output via a transient overlay during
//! sync.

mod common;

use common::Project;
use predicates::prelude::*;
use std::path::{Path, PathBuf};

fn exuno_in(dir: &Path) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::new(env!("CARGO_BIN_EXE_exuno"));
    cmd.current_dir(dir);
    common::scrub(&mut cmd);
    cmd
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

fn append(path: &Path, content: &str) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(content.as_bytes()).unwrap();
}

fn add_shared_block(child: &Path, path: &str, inherit: &str) {
    append(
        &child.join(".ai/exuno.yaml"),
        &format!("\nshared:\n  path: \"{path}\"\n  inherit: {inherit}\n"),
    );
}

fn inherit_skills_too(child: &Path) {
    let config = child.join(".ai/exuno.yaml");
    let text = std::fs::read_to_string(&config).unwrap();
    std::fs::write(
        &config,
        text.replace("inherit: rules", "inherit: rules, skills"),
    )
    .unwrap();
}

/// A parent project with one custom rule, and a child below it that declares
/// `shared:` inheritance for rules. Mirrors `_shared_make_pair`.
fn make_pair(project: &Project) -> (PathBuf, PathBuf) {
    let parent_dir = project.join("parent");
    let child_dir = parent_dir.join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    exuno_in(&parent_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    write(
        &parent_dir.join(".ai/src/rules/parent-only.md"),
        "parent-rule\n",
    );

    std::fs::create_dir_all(&child_dir).unwrap();
    exuno_in(&child_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    exuno_in(&child_dir)
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success();
    // Remove default rules so the test is unambiguous about what's inherited.
    for entry in std::fs::read_dir(child_dir.join(".ai/src/rules")).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|e| e == "md") {
            std::fs::remove_file(entry.path()).unwrap();
        }
    }
    write(
        &child_dir.join(".ai/src/rules/child-only.md"),
        "child-rule\n",
    );
    add_shared_block(&child_dir, "../", "rules");

    (parent_dir, child_dir)
}

/// A rules-only `--no-templates` scaffold — no `commands/`, `agents/`, or
/// `AGENTS.md`. The shared overlay tmpdir then lacks those paths; sync must
/// not abort under strict error propagation when they are missing. Mirrors
/// `_shared_make_sparse_pair`.
fn make_sparse_pair(project: &Project) -> (PathBuf, PathBuf) {
    let parent_dir = project.join("parent_sparse");
    let child_dir = parent_dir.join("child");
    let init_flags = [
        "init",
        "--no-templates",
        "--no-detect",
        "--content",
        "rules",
        "--yes",
    ];

    std::fs::create_dir_all(&parent_dir).unwrap();
    exuno_in(&parent_dir).args(init_flags).assert().success();
    write(
        &parent_dir.join(".ai/src/rules/parent-only.md"),
        "parent-rule\n",
    );

    std::fs::create_dir_all(&child_dir).unwrap();
    exuno_in(&child_dir).args(init_flags).assert().success();
    exuno_in(&child_dir)
        .args(["enable", "claude", "--no-scaffold"])
        .assert()
        .success();
    write(
        &child_dir.join(".ai/src/rules/child-only.md"),
        "child-rule\n",
    );
    write(&child_dir.join(".ai/AGENTS.md"), "# Child\n");
    let config_path = child_dir.join(".ai/exuno.yaml");
    let config = std::fs::read_to_string(&config_path).unwrap();
    let config = config.replace("agents: \".ai/src/AGENTS.md\"", "agents: \".ai/AGENTS.md\"");
    std::fs::write(&config_path, config).unwrap();

    for dir in [&parent_dir, &child_dir] {
        assert!(dir.join(".ai/src/rules").is_dir());
        assert!(!dir.join(".ai/src/commands").exists());
        assert!(!dir.join(".ai/src/agents").exists());
        assert!(!dir.join(".ai/src/AGENTS.md").exists());
    }

    add_shared_block(&child_dir, "../", "rules");

    (parent_dir, child_dir)
}

#[test]
fn sync_succeeds_when_overlay_omits_commands_agents_and_agents_md() {
    let project = Project::empty();
    let (_parent, child) = make_sparse_pair(&project);

    exuno_in(&child)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Shared overlay active"));
    assert!(child.join(".claude/rules/parent-only.md").is_file());
    assert!(child.join(".claude/rules/child-only.md").is_file());
}

#[test]
fn parent_rules_materialise_into_child_output_dirs() {
    let project = Project::empty();
    let (_parent, child) = make_pair(&project);

    exuno_in(&child)
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains("Shared overlay active"));
    assert!(child.join(".claude/rules/parent-only.md").is_file());
    assert!(child.join(".claude/rules/child-only.md").is_file());
}

#[test]
fn child_wins_on_path_collision() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project);

    write(&parent.join(".ai/src/rules/clash.md"), "PARENT VERSION\n");
    write(&child.join(".ai/src/rules/clash.md"), "CHILD VERSION\n");

    exuno_in(&child).arg("sync").assert().success();
    assert_eq!(
        std::fs::read_to_string(child.join(".claude/rules/clash.md")).unwrap(),
        "CHILD VERSION\n"
    );
}

#[test]
fn a_categorized_child_skill_shadows_the_parent_skill_of_its_name() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project);
    inherit_skills_too(&child);
    write(
        &parent.join(".ai/src/skills/bloc/SKILL.md"),
        "---\nname: bloc\ndescription: Parent\n---\n",
    );
    write(&parent.join(".ai/src/skills/bloc/references/p.md"), "p\n");
    write(
        &child.join(".ai/src/skills/flutter/bloc/SKILL.md"),
        "---\nname: bloc\ndescription: Child\n---\n",
    );

    exuno_in(&child).arg("sync").assert().success();
    assert_eq!(
        std::fs::read_to_string(child.join(".claude/skills/bloc/SKILL.md")).unwrap(),
        "---\nname: bloc\ndescription: Child\n---\n"
    );
    assert!(!child.join(".claude/skills/bloc/references").exists());
    exuno_in(&child).arg("check").assert().success();
}

#[test]
fn a_categorized_child_extension_adds_to_the_parent_skill_of_its_name() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project);
    inherit_skills_too(&child);
    write(
        &parent.join(".ai/src/skills/bloc/SKILL.md"),
        "---\nname: bloc\ndescription: Parent\n---\n\nParent body.\n",
    );
    write(&parent.join(".ai/src/skills/bloc/references/p.md"), "p\n");
    write(
        &child.join(".ai/src/skills/flutter/bloc/SKILL.append.md"),
        "Child notes.\n",
    );
    write(
        &child.join(".ai/src/skills/flutter/bloc/references/c.md"),
        "c\n",
    );

    exuno_in(&child).arg("sync").assert().success();
    let out = child.join(".claude/skills/bloc");
    assert_eq!(
        std::fs::read_to_string(out.join("SKILL.md")).unwrap(),
        "---\nname: bloc\ndescription: Parent\n---\n\nParent body.\n\nChild notes.\n"
    );
    assert!(out.join("references/p.md").is_file());
    assert!(out.join("references/c.md").is_file());
    assert!(!out.join("SKILL.append.md").exists());
    exuno_in(&child).arg("check").assert().success();
}

#[test]
fn inherit_list_filters_which_categories_materialise() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project); // inherits: rules

    // Parent has a custom skill — but child only inherits rules, not skills.
    write(&parent.join(".ai/src/skills/parent-skill/SKILL.md"), "ps\n");

    exuno_in(&child).arg("sync").assert().success();
    // Inherited via rules — present.
    assert!(child.join(".claude/rules/parent-only.md").is_file());
    // NOT inherited (skills not in list) — absent.
    assert!(!child.join(".claude/skills/parent-skill").exists());
}

#[test]
fn child_skills_survive_alongside_the_engine_base_skills() {
    let project = Project::empty();
    let (_parent, child) = make_pair(&project); // inherits: rules
    write(
        &child.join(".ai/src/skills/child-skill/SKILL.md"),
        "---\nname: child-skill\ndescription: child fixture skill\n---\n",
    );

    exuno_in(&child).arg("sync").assert().success();
    assert!(child.join(".claude/skills/child-skill/SKILL.md").is_file());
    assert!(child.join(".claude/skills/exuno/SKILL.md").is_file());
}

#[test]
fn missing_parent_path_warns_and_skips_overlay() {
    let project = Project::seeded(&["--no-detect", "--yes"]);
    project.enable_tools(&["claude"]);
    project.append(
        ".ai/exuno.yaml",
        "\nshared:\n  path: \"../does-not-exist\"\n  inherit: rules\n",
    );

    project.exuno().arg("sync").assert().success().stderr(
        predicate::str::contains("shared.path does not exist")
            .or(predicate::str::contains("overlay skipped")),
    );
    // Child's own rules still sync.
    assert!(project.join(".claude/rules").is_dir());
}

#[test]
fn cleans_up_tmpdir_after_sync_no_leaked_dirs() {
    let project = Project::empty();
    let (_parent, child) = make_pair(&project);
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();

    exuno_in(&child)
        .env("TMPDIR", &sandbox)
        .arg("sync")
        .assert()
        .success();
    // Nothing at all: the run directory, the overlay inside it, and every
    // other scratch file the run created.
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn dry_run_does_not_produce_output_but_still_tears_down_tmpdir() {
    let project = Project::empty();
    let (_parent, child) = make_pair(&project);
    let sandbox = project.join("tmpdir_sandbox");
    std::fs::create_dir_all(&sandbox).unwrap();

    exuno_in(&child)
        .env("TMPDIR", &sandbox)
        .args(["sync", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Shared overlay active"));
    assert!(!child.join(".claude/rules/parent-only.md").exists());
    assert_eq!(std::fs::read_dir(&sandbox).unwrap().count(), 0);
}

#[test]
fn doctor_finds_a_parent_skill_the_child_copied_into_a_category() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project);
    let skill = "---\nname: bloc\ndescription: Bloc\n---\n";
    write(&parent.join(".ai/src/skills/bloc/SKILL.md"), skill);
    write(&child.join(".ai/src/skills/flutter/bloc/SKILL.md"), skill);

    exuno_in(&child)
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains(
            "skills/flutter/bloc/SKILL.md — duplicate of parent's",
        ));
}

#[test]
fn doctor_adds_inherited_via_shared_hint_on_duplicates_in_inherited_categories() {
    let project = Project::empty();
    let (parent, child) = make_pair(&project);

    // Force a duplicate in child for an inherited category.
    std::fs::copy(
        parent.join(".ai/src/rules/parent-only.md"),
        child.join(".ai/src/rules/parent-only.md"),
    )
    .unwrap();

    exuno_in(&child)
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("rules/parent-only.md — duplicate"))
        .stdout(predicate::str::contains("inherited via shared:"));
}

#[test]
fn doctor_governance_category_divergent_file_is_upgraded_to_advisory() {
    let project = Project::empty();
    let parent_dir = project.join("parent");
    let child_dir = parent_dir.join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    exuno_in(&parent_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    write(
        &parent_dir.join(".ai/src/rules/governance-rule.md"),
        "---\nname: governance-rule\ndescription: a rule\ncategory: governance\n---\nparent body\n",
    );

    std::fs::create_dir_all(&child_dir).unwrap();
    exuno_in(&child_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    write(
        &child_dir.join(".ai/src/rules/governance-rule.md"),
        "---\nname: governance-rule\ndescription: a rule\ncategory: governance\n---\nCHILD overrides body\n",
    );

    exuno_in(&child_dir)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "governance file diverges from parent",
        ))
        .stdout(predicate::str::contains("likely a mistake"));
}

#[test]
fn doctor_non_governance_divergent_file_stays_info_tier() {
    let project = Project::empty();
    let parent_dir = project.join("parent");
    let child_dir = parent_dir.join("child");
    std::fs::create_dir_all(&parent_dir).unwrap();
    exuno_in(&parent_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    write(
        &parent_dir.join(".ai/src/rules/plain.md"),
        "no frontmatter, parent\n",
    );

    std::fs::create_dir_all(&child_dir).unwrap();
    exuno_in(&child_dir)
        .args(["init", "--no-detect", "--yes"])
        .assert()
        .success();
    write(
        &child_dir.join(".ai/src/rules/plain.md"),
        "no frontmatter, child\n",
    );

    exuno_in(&child_dir)
        .arg("doctor")
        .assert()
        .success()
        .stdout(predicate::str::contains("diverges from parent"))
        .stdout(predicate::str::contains("governance file diverges").not());
}
