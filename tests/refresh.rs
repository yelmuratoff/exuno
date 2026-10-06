//! `tests/refresh.bats`: `agentsync refresh` (three-way diff via
//! `.ai/.template-manifest`) on a project scaffolded with every content
//! category.

mod common;

use common::Project;
use predicates::prelude::*;

/// `seed_project --yes --no-detect --content rules,skills,commands,subagents`.
fn seeded() -> Project {
    Project::seeded(&[
        "--yes",
        "--no-detect",
        "--content",
        "rules,skills,commands,subagents",
    ])
}

/// `_drop_manifest`: removes one relative-path entry from the template
/// manifest, so refresh treats the file as never recorded.
fn drop_manifest_entry(project: &Project, rel: &str) {
    let content = project.read(".ai/.template-manifest");
    let prefix = format!("{rel}\t");
    let kept: String = content
        .lines()
        .filter(|line| !line.starts_with(&prefix))
        .map(|line| format!("{line}\n"))
        .collect();
    project.write(".ai/.template-manifest", &kept);
}

/// `_drop_manifest_prefix`: removes every manifest entry whose rel-path
/// begins with `prefix`, so a whole category reads as brand-new.
fn drop_manifest_prefix(project: &Project, prefix: &str) {
    let content = project.read(".ai/.template-manifest");
    let kept: String = content
        .lines()
        .filter(|line| !line.starts_with(prefix))
        .map(|line| format!("{line}\n"))
        .collect();
    project.write(".ai/.template-manifest", &kept);
}

// ── basics ───────────────────────────────────────────────────────────────────

#[test]
fn refresh_help_prints_usage() {
    seeded()
        .exuno()
        .args(["refresh", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  agentsync refresh — pull new template files into an existing .ai/src/\n\n  USAGE\n    agentsync refresh [OPTIONS]\n\n  DESCRIPTION\n",
        ))
        .stdout(predicate::str::contains("\n  OPTIONS\n    --only <csv>          "))
        .stdout(predicate::str::contains("--include-agents-md"))
        .stdout(predicate::str::contains("--include-deleted"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--review"))
        .stdout(predicate::str::contains("REMEMBERED SKIPS"))
        .stdout(predicate::str::contains("PERSISTENT OVERRIDES"));
}

#[test]
fn refresh_errors_when_no_ai_directory() {
    let project = seeded();
    std::fs::remove_dir_all(project.join(".ai")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No .ai/"));
}

#[test]
fn refresh_rejects_unknown_only_value() {
    seeded()
        .exuno()
        .args(["refresh", "--yes", "--only", "bogus"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown --only"));
}

#[test]
fn refresh_errors_when_no_source_categories_present_and_no_only() {
    let project = seeded();
    for dir in ["rules", "skills", "commands", "agents"] {
        let path = project.join(&format!(".ai/src/{dir}"));
        if path.exists() {
            std::fs::remove_dir_all(path).unwrap();
        }
    }
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("No source content categories"));
}

#[test]
fn refresh_non_tty_without_yes_errors_with_hint_when_changes_pending() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    project
        .exuno()
        .arg("refresh")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--yes"));
}

// ── manifest baseline ────────────────────────────────────────────────────────

#[test]
fn init_writes_template_manifest_with_file_hashes() {
    let project = seeded();
    assert!(project.exists(".ai/.template-manifest"));
    let manifest = project.read(".ai/.template-manifest");
    assert!(manifest.lines().any(|l| l.starts_with("rules/core.md\t")));
    assert!(
        manifest
            .lines()
            .any(|l| l.starts_with("skills/humanizer/SKILL.md\t"))
    );
    assert!(
        manifest
            .lines()
            .any(|line| { line.split('\t').nth(1).is_some_and(|hash| hash.len() == 64) })
    );
}

// ── three-way diff: AUTO_UPDATE ──────────────────────────────────────────────

#[test]
fn refresh_untouched_file_missing_manifest_matching_template_is_unchanged() {
    let project = seeded();
    drop_manifest_entry(&project, "rules/core.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Already up to date").or(predicate::str::contains("Done.")),
        );
}

#[test]
fn refresh_new_template_no_manifest_entry_file_absent_is_added_with_yes() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/rules/comments.md"));
    assert!(
        project
            .read(".ai/.template-manifest")
            .lines()
            .any(|l| l.starts_with("rules/comments.md\t"))
    );
}

#[test]
fn refresh_user_edited_no_change_is_silent() {
    let project = seeded();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    let before = project.read(".ai/src/rules/core.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict").not());
    assert_eq!(project.read(".ai/src/rules/core.md"), before);
}

// ── skills kept in a category ────────────────────────────────────────────────

fn move_skill_into_category(project: &Project, name: &str, category: &str) {
    std::fs::create_dir_all(project.join(&format!(".ai/src/skills/{category}"))).unwrap();
    std::fs::rename(
        project.join(&format!(".ai/src/skills/{name}")),
        project.join(&format!(".ai/src/skills/{category}/{name}")),
    )
    .unwrap();
}

#[test]
fn refresh_follows_a_template_skill_into_its_category() {
    let project = seeded();
    move_skill_into_category(&project, "comments", "meta");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"))
        .stdout(predicate::str::contains("Locally declined").not());
    assert!(!project.exists(".ai/src/skills/comments"));
}

#[test]
fn refresh_auto_updates_a_template_skill_kept_in_a_category() {
    let project = seeded();
    let shipped = project.read(".ai/src/skills/comments/SKILL.md");
    move_skill_into_category(&project, "comments", "meta");
    project.write(".ai/src/skills/meta/comments/SKILL.md", "OLD TEMPLATE\n");
    drop_manifest_entry(&project, "skills/comments/SKILL.md");
    let old = project.sha256(".ai/src/skills/meta/comments/SKILL.md");
    project.append(
        ".ai/.template-manifest",
        &format!("skills/comments/SKILL.md\t{old}\n"),
    );

    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/skills/meta/comments/SKILL.md"),
        shipped
    );
    assert!(!project.exists(".ai/src/skills/comments"));
}

// ── deleted (skip-as-decline + restore) ──────────────────────────────────────

#[test]
fn refresh_file_removed_locally_is_silent_without_include_deleted() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(
            predicate::str::contains("Already up to date").or(predicate::str::contains("Done.")),
        );
    assert!(!project.exists(".ai/src/rules/comments.md"));
}

#[test]
fn refresh_include_deleted_lists_previously_declined_files_in_dry_run() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    project
        .exuno()
        .args(["refresh", "--include-deleted", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rules/comments.md"))
        .stdout(predicate::str::contains("declined").or(predicate::str::contains("deleted")));
    assert!(!project.exists(".ai/src/rules/comments.md"));
}

#[test]
fn refresh_include_deleted_with_yes_still_skips_restoration_interactive_only() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes", "--include-deleted"])
        .assert()
        .success()
        .stdout(predicate::str::contains("declined").or(predicate::str::contains("skipped")));
    assert!(!project.exists(".ai/src/rules/comments.md"));
}

// ── overrides ────────────────────────────────────────────────────────────────

#[test]
fn refresh_declined_override_skips_template_entirely() {
    let project = seeded();
    project.append(
        ".ai/agent_sync.yaml",
        "\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n",
    );
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("+ rules/comments.md").not());
    assert!(!project.exists(".ai/src/rules/comments.md"));
}

#[test]
fn refresh_pinned_override_silences_conflict_on_user_edited_file_when_template_moves() {
    let project = seeded();
    project.append(
        ".ai/agent_sync.yaml",
        "\ntemplate_overrides:\n  pinned:\n    - rules/core.md\n",
    );
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    drop_manifest_entry(&project, "rules/core.md");
    let before = project.read(".ai/src/rules/core.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("conflict").not());
    assert_eq!(project.read(".ai/src/rules/core.md"), before);
}

// ── flags / scope ────────────────────────────────────────────────────────────

#[test]
fn refresh_dry_run_does_not_write() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    project
        .exuno()
        .args(["refresh", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Dry run"));
    assert!(!project.exists(".ai/src/rules/comments.md"));
}

#[test]
fn refresh_only_filters_by_category() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    std::fs::remove_dir_all(project.join(".ai/src/skills/comments")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    drop_manifest_entry(&project, "skills/comments/SKILL.md");
    project
        .exuno()
        .args(["refresh", "--yes", "--only", "rules"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/rules/comments.md"));
    assert!(!project.join(".ai/src/skills/comments").exists());
}

#[test]
fn refresh_only_subagents_alias_maps_to_agents_dir() {
    let project = seeded();
    std::fs::remove_dir_all(project.join(".ai/src/agents")).unwrap();
    drop_manifest_prefix(&project, "agents/");
    project
        .exuno()
        .args(["refresh", "--yes", "--only", "subagents"])
        .assert()
        .success();
    assert!(project.join(".ai/src/agents").is_dir());
}

#[test]
fn refresh_only_value_form_equals_separator() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    std::fs::remove_dir_all(project.join(".ai/src/skills/comments")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    drop_manifest_entry(&project, "skills/comments/SKILL.md");
    project
        .exuno()
        .args(["refresh", "--yes", "--only=rules"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/rules/comments.md"));
    assert!(!project.join(".ai/src/skills/comments").exists());
}

#[test]
fn refresh_idempotent_second_run_reports_up_to_date() {
    let project = seeded();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"));
}

#[test]
fn refresh_agents_md_excluded_by_default() {
    let project = seeded();
    // `--content` in `seeded()` omits "agents", so init never scaffolds
    // AGENTS.md; `>>` in the bats original created it fresh with just this line.
    project.write(".ai/src/AGENTS.md", "USER LOCAL EDIT\n");
    let before = project.read(".ai/src/AGENTS.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/AGENTS.md"), before);
}

#[test]
fn refresh_include_agents_md_surfaces_agents_md_but_conflict_still_skipped_under_yes() {
    let project = seeded();
    // See the note in `refresh_agents_md_excluded_by_default`.
    project.write(".ai/src/AGENTS.md", "USER LOCAL EDIT\n");
    drop_manifest_entry(&project, "AGENTS.md");
    let before = project.read(".ai/src/AGENTS.md");
    project
        .exuno()
        .args(["refresh", "--yes", "--include-agents-md"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTS.md"));
    assert_eq!(project.read(".ai/src/AGENTS.md"), before);
}

#[test]
fn refresh_leaves_users_custom_files_alone_not_in_templates() {
    let project = seeded();
    project.write(
        ".ai/src/rules/my-custom.md",
        "# My Custom Rule\nCustom content.\n",
    );
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/rules/my-custom.md"));
    assert!(
        project
            .read(".ai/src/rules/my-custom.md")
            .contains("My Custom Rule")
    );
}

#[test]
fn refresh_scope_auto_detection_skips_categories_absent_from_ai_src() {
    let project = seeded();
    std::fs::remove_dir_all(project.join(".ai/src/commands")).unwrap();
    std::fs::remove_dir_all(project.join(".ai/src/agents")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(!project.join(".ai/src/commands").exists());
    assert!(!project.join(".ai/src/agents").exists());
    assert!(project.exists(".ai/src/rules/core.md"));
    assert!(project.join(".ai/src/skills").is_dir());
}

#[test]
fn refresh_explicit_only_opts_into_a_category_not_yet_in_tree() {
    let project = seeded();
    std::fs::remove_dir_all(project.join(".ai/src/commands")).unwrap();
    drop_manifest_prefix(&project, "commands/");
    project
        .exuno()
        .args(["refresh", "--yes", "--only", "commands"])
        .assert()
        .success();
    assert!(project.join(".ai/src/commands").is_dir());
    let count = std::fs::read_dir(project.join(".ai/src/commands"))
        .unwrap()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "md"))
        .count();
    assert!(count >= 1);
}

#[test]
fn refresh_nested_skill_references_files_added_when_missing_new() {
    let project = seeded();
    std::fs::remove_dir_all(project.join(".ai/src/skills/humanizer/references")).unwrap();
    drop_manifest_prefix(&project, "skills/humanizer/references/");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(project.exists(".ai/src/skills/humanizer/references/wikipedia_signs_of_ai_writing.md"));
}

#[test]
fn init_copies_nested_skill_subdirectories_references_scripts() {
    let project = seeded();
    assert!(project.exists(".ai/src/skills/humanizer/references/wikipedia_signs_of_ai_writing.md"));
    assert!(project.exists(".ai/src/skills/humanizer/scripts/strip-ai-chars.sh"));
}

// ── manifest semantics under --yes ───────────────────────────────────────────

#[test]
fn refresh_yes_records_new_manifest_entries_when_adding_new_files() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    drop_manifest_entry(&project, "rules/comments.md");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/.template-manifest")
            .lines()
            .any(|l| l.starts_with("rules/comments.md\t"))
    );
}

#[test]
fn refresh_backward_compat_works_on_project_with_no_manifest_at_all() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/.template-manifest")).unwrap();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("USER LOCAL EDIT")
    );
}

#[test]
fn refresh_heals_manifest_from_current_matches_when_no_manifest_existed() {
    let project = seeded();
    std::fs::remove_file(project.join(".ai/.template-manifest")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    assert!(project.exists(".ai/.template-manifest"));
    assert!(
        project
            .read(".ai/.template-manifest")
            .lines()
            .any(|l| l.starts_with("rules/core.md\t"))
    );
}

// ── remembered skips / --review ──────────────────────────────────────────────
// A "previously skipped conflict" is recorded by writing the current template
// hash into the manifest for the user's diverged file. The manifest entry that
// init creates already records the current template hash — so editing a user
// file is enough to reproduce the post-skip state.

#[test]
fn refresh_silently_kept_divergence_does_not_surface_without_review() {
    let project = seeded();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"))
        .stdout(predicate::str::contains("~ rules/core.md").not());
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("USER LOCAL EDIT")
    );
}

#[test]
fn refresh_review_surfaces_silently_kept_divergence_in_dry_run() {
    let project = seeded();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--review", "--dry-run"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rules/core.md"))
        .stdout(predicate::str::contains("onflict"))
        .stdout(predicate::str::contains("Dry run"));
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("USER LOCAL EDIT")
    );
}

#[test]
fn refresh_review_with_yes_surfaces_but_preserves_users_edit() {
    let project = seeded();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--review", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rules/core.md"));
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("USER LOCAL EDIT")
    );
}

#[test]
fn refresh_up_to_date_summary_hints_at_review_when_files_differ_silently() {
    let project = seeded();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"))
        .stdout(predicate::str::contains("--review"));
}

#[test]
fn refresh_up_to_date_summary_omits_the_review_hint_when_nothing_differs() {
    let project = seeded();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already up to date"))
        .stdout(predicate::str::contains("pass --review").not());
}

#[test]
fn refresh_pinned_override_still_wins_over_review() {
    let project = seeded();
    project.append(
        ".ai/agent_sync.yaml",
        "\ntemplate_overrides:\n  pinned:\n    - rules/core.md\n",
    );
    project.append(".ai/src/rules/core.md", "USER LOCAL EDIT\n");
    project
        .exuno()
        .args(["refresh", "--review", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("~ rules/core.md").not());
    assert!(
        project
            .read(".ai/src/rules/core.md")
            .contains("USER LOCAL EDIT")
    );
}

// ── --status and declined-list visibility ─────────────────────────────────────

#[test]
fn refresh_status_prints_persistent_declined_list() {
    let project = seeded();
    project.append(
        ".ai/agent_sync.yaml",
        "\ntemplate_overrides:\n  declined:\n    - rules/core.md\n    - rules/git.md\n",
    );
    project
        .exuno()
        .args(["refresh", "--status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Declined templates"))
        .stdout(predicate::str::contains("Persistent"))
        .stdout(predicate::str::contains("rules/core.md"))
        .stdout(predicate::str::contains("rules/git.md"));
}

#[test]
fn refresh_reads_template_overrides_from_the_config_agentsync_config_path_names() {
    let project = seeded();
    project.write(
        "selected.yaml",
        "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/git.md\n",
    );
    project
        .exuno()
        .env("AGENTSYNC_CONFIG_PATH", "selected.yaml")
        .args(["refresh", "--status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("rules/git.md"));
}

#[test]
fn refresh_status_prints_nothing_declined_when_list_is_empty() {
    seeded()
        .exuno()
        .args(["refresh", "--status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Declined templates"))
        .stdout(predicate::str::contains("Nothing declined"));
}

#[test]
fn refresh_up_to_date_output_splits_persistent_vs_local_declined_counts() {
    let project = seeded();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success();
    project.append(
        ".ai/agent_sync.yaml",
        "\ntemplate_overrides:\n  declined:\n    - rules/core.md\n",
    );
    std::fs::remove_file(project.join(".ai/src/rules/comments.md")).unwrap();
    project
        .exuno()
        .args(["refresh", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Persistently declined"))
        .stdout(predicate::str::contains("Locally declined"))
        .stdout(predicate::str::contains("--status for the full list"));
}
