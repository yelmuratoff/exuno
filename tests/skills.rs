mod common;

use common::Project;
use predicates::prelude::*;

fn project() -> Project {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Agent\n");
    project.write(".ai/exuno.yaml", "base_skills: false\n");
    project
}

#[test]
fn list_reads_project_skills_without_a_catalog() {
    let project = project();
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Deploy the app safely\n---\n\n# Deploy\n",
    );
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout("name\tdescription\tpath\tcategory\ndeploy\tDeploy the app safely\t.ai/src/skills/deploy/SKILL.md\t\n")
        .stderr("");
}

#[test]
fn list_neutralizes_invisible_formatting_in_skill_metadata() {
    let project = project();
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: safe\u{202e}spoof\n---\n",
    );
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout("name\tdescription\tpath\tcategory\ndeploy\tsafe spoof\t.ai/src/skills/deploy/SKILL.md\t\n");
}

#[test]
fn list_reads_bundled_skill_and_folded_description() {
    let project = project();
    project.write(".ai/exuno.yaml", "base_skills: true\n");
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: >-\n  Deploy the app\n  safely\n---\n",
    );
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("deploy\tDeploy the app safely\t"))
        .stdout(predicate::str::contains("exuno\t"))
        .stdout(predicate::str::contains("bundled:skills/exuno/SKILL.md"));
}

#[test]
fn check_reports_invalid_frontmatter_without_changing_sync() {
    let project = project();
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: wrong\ndescription: Deploy\n---\n",
    );
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            "name 'wrong' does not match directory 'deploy'",
        ));
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deploy\t\t.ai/src/skills/deploy/SKILL.md",
        ));
    project.enable_tools(&["claude"]);
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/skills/deploy/SKILL.md"));
}

#[test]
fn list_uses_configured_source_path() {
    let project = project();
    project.write(
        ".ai/exuno.yaml",
        "base_skills: false\nsource:\n  skills: custom/skills\n",
    );
    project.write(
        "custom/skills/review/SKILL.md",
        "---\nname: review\ndescription: Review changes\n---\n",
    );
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "review\tReview changes\tcustom/skills/review/SKILL.md",
        ));
}

#[test]
fn list_includes_shared_skills_and_respects_child_precedence() {
    let project = project();
    project.write(
        ".ai/exuno.yaml",
        "base_skills: false\nshared:\n  path: parent\n  inherit: skills\n",
    );
    project.write(
        "parent/.ai/src/skills/shared/SKILL.md",
        "---\nname: shared\ndescription: From parent\n---\n",
    );
    project.write(
        "parent/.ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Old deployment\n---\n",
    );
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Child deployment\n---\n",
    );
    project
        .exuno()
        .args(["skills", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deploy\tChild deployment\t.ai/src/skills/deploy/SKILL.md",
        ))
        .stdout(predicate::str::contains(
            "shared\tFrom parent\tparent/.ai/src/skills/shared/SKILL.md",
        ));
}

#[test]
fn profile_list_uses_profile_overlay() {
    let project = project();
    project.write(
        ".ai/exuno.yaml",
        "base_skills: false\nprofiles:\n  work:\n    tools: [claude-work]\n",
    );
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Personal deployment\n---\n",
    );
    project.write(
        ".ai/profiles/work/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Work deployment\n---\n",
    );
    project
        .exuno()
        .args(["skills", "list", "--profile", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "deploy\tWork deployment\t.ai/profiles/work/src/skills/deploy/SKILL.md",
        ));
    project
        .exuno()
        .args(["skills", "show", "deploy", "--profile", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Description: Work deployment\n"))
        .stdout(predicate::str::contains(
            "Path: .ai/profiles/work/src/skills/deploy/SKILL.md\n",
        ));
}

#[test]
fn unknown_profile_is_an_error() {
    project()
        .exuno()
        .args(["skills", "list", "--profile", "missing"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown profile: missing"));
}

#[test]
fn show_displays_declared_fields_and_unverified_annotations() {
    let project = project();
    project.write(
        ".ai/src/skills/review/SKILL.md",
        "---\nname: review\ndescription: Review a selected diff\nlicense: MIT\ncompatibility: Requires git\nmetadata:\n  agentsync-use-when: Before merging\n  agentsync-not-for: Writing the change\n  agentsync-requirements: A selected diff\n---\n# Review\n",
    );
    project
        .exuno()
        .args(["skills", "show", "review"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Description: Review a selected diff\n",
        ))
        .stdout(predicate::str::contains(
            "Compatibility (declared): Requires git\n",
        ))
        .stdout(predicate::str::contains("License: MIT\n"))
        .stdout(predicate::str::contains(
            "Use when (annotation, unverified): Before merging\n",
        ))
        .stdout(predicate::str::contains(
            "Not for (annotation, unverified): Writing the change\n",
        ))
        .stdout(predicate::str::contains(
            "Requirements (annotation, unverified): A selected diff\n",
        ))
        .stdout(predicate::str::contains(
            "Path: .ai/src/skills/review/SKILL.md\n",
        ))
        .stderr("");
}

#[test]
fn list_filters_names_without_affecting_check() {
    let project = project();
    for name in ["review", "release", "deploy"] {
        project.write(
            &format!(".ai/src/skills/{name}/SKILL.md"),
            &format!("---\nname: {name}\ndescription: Use {name}\n---\n"),
        );
    }
    project
        .exuno()
        .args(["skills", "list", "--include", "re*", "--exclude", "release"])
        .assert()
        .success()
        .stdout(predicate::str::contains("review\tUse review\t"))
        .stdout(predicate::str::contains("release\t").not())
        .stdout(predicate::str::contains("deploy\t").not());
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .success()
        .stdout("Checked 3 skills: 0 issue(s)\n");
    project
        .exuno()
        .args([
            "skills",
            "list",
            "--include",
            "review",
            "--include",
            "deploy",
            "--exclude",
            "release",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("review\tUse review\t"))
        .stdout(predicate::str::contains("deploy\tUse deploy\t"))
        .stdout(predicate::str::contains("release\t").not());
}

#[test]
fn profile_reports_the_source_used_by_its_overlay() {
    let project = project();
    project.write(
        ".ai/exuno.yaml",
        "base_skills: false\nsource:\n  skills: custom/skills\nprofiles:\n  work:\n    tools: [claude-work]\n",
    );
    project.write(
        "custom/skills/review/SKILL.md",
        "---\nname: review\ndescription: Custom source\n---\n",
    );
    project.write(
        ".ai/src/skills/review/SKILL.md",
        "---\nname: review\ndescription: Profile base source\n---\n",
    );
    project.write(
        ".ai/profiles/work/src/skills/other/SKILL.md",
        "---\nname: other\ndescription: Profile skill\n---\n",
    );
    project
        .exuno()
        .args(["skills", "show", "review", "--profile", "work"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Description: Profile base source\n",
        ))
        .stdout(predicate::str::contains(
            "Path: .ai/src/skills/review/SKILL.md\n",
        ));
}

#[test]
fn show_reports_unknown_and_invalid_skills() {
    let project = project();
    project
        .exuno()
        .args(["skills", "show", "absent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unknown skill: absent"));
    project.write(
        ".ai/src/skills/review/SKILL.md",
        "---\nname: review\ndescription:\n---\n",
    );
    project
        .exuno()
        .args(["skills", "show", "review"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "description must be 1–1024 characters",
        ));
}

#[test]
fn check_reports_a_directory_with_no_skill_at_any_depth() {
    let project = project();
    project.write(".ai/src/skills/empty/reference.md", "# Reference\n");
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .failure()
        .stdout(".ai/src/skills/empty/: no SKILL.md here or in any subdirectory\nChecked 0 skills: 1 issue(s)\n");
}

fn skill(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill\n---\n")
}

fn categorized_project() -> Project {
    let project = project();
    project.write(".ai/src/skills/commit/SKILL.md", &skill("commit"));
    project.write(".ai/src/skills/flutter/bloc/SKILL.md", &skill("bloc"));
    project.write(
        ".ai/src/skills/flutter/ui/slivers/SKILL.md",
        &skill("slivers"),
    );
    project.write(".ai/src/skills/backend/auth/SKILL.md", &skill("auth"));
    project
}

#[test]
fn list_shows_each_category_and_filters_by_it() {
    let project = categorized_project();
    project
        .exuno()
        .args(["skills", "list", "--include", "flutter/* commit"])
        .assert()
        .success()
        .stdout(
            "name\tdescription\tpath\tcategory\n\
             commit\tThe commit skill\t.ai/src/skills/commit/SKILL.md\t\n\
             bloc\tThe bloc skill\t.ai/src/skills/flutter/bloc/SKILL.md\tflutter\n\
             slivers\tThe slivers skill\t.ai/src/skills/flutter/ui/slivers/SKILL.md\tflutter/ui\n",
        );
}

#[test]
fn show_names_the_category_of_a_categorized_skill() {
    let project = categorized_project();
    project
        .exuno()
        .args(["skills", "show", "slivers"])
        .assert()
        .success()
        .stdout(
            "Name: slivers\nDescription: The slivers skill\nCategory: flutter/ui\nPath: .ai/src/skills/flutter/ui/slivers/SKILL.md\n",
        );
}

#[test]
fn check_reports_name_collisions_across_categories() {
    let project = categorized_project();
    project.write(".ai/src/skills/flutter/auth/SKILL.md", &skill("auth"));
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .failure()
        .stdout(
            "auth: name claimed by .ai/src/skills/backend/auth, .ai/src/skills/flutter/auth — tools install skills flat by name\nChecked 5 skills: 1 issue(s)\n",
        );
}

#[test]
fn check_reports_a_category_name_add_would_refuse() {
    let project = categorized_project();
    project.write(
        ".ai/src/skills/Mobile/navigation/SKILL.md",
        &skill("navigation"),
    );
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .failure()
        .stdout(
            ".ai/src/skills/Mobile/: category name is not lowercase letters, digits, and single hyphens\nChecked 5 skills: 1 issue(s)\n",
        );
}

#[test]
fn check_reports_directories_below_the_category_limit() {
    let project = categorized_project();
    project.write(".ai/src/skills/a/b/c/d/e/f/SKILL.md", &skill("f"));
    project
        .exuno()
        .args(["skills", "check"])
        .assert()
        .failure()
        .stdout(predicate::str::contains(
            ".ai/src/skills/a/b/c/d/e/: deeper than 4 categories — not synced\n",
        ));
}

#[test]
fn help_and_invalid_arguments_do_not_read_the_project() {
    Project::empty()
        .exuno()
        .args(["skills", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("skills list [--profile <name>]"));
    Project::empty()
        .exuno()
        .args(["skills", "install"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("expected skills list|show|check"));
}
