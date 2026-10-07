//! `tests/format_migration.bats`: a project that predates a migration is told
//! so, and `migrate --apply` retires the stale engine-owned skill copy and
//! records the revision.

mod common;

use common::Project;
use predicates::prelude::*;

/// A project as an older engine would have left it: the agentsync skill
/// copied into `.ai/src/`, its hash recorded in the template manifest, no
/// format key in the config.
fn seed_pre_format_project() -> Project {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    let config = project.read(".ai/exuno.yaml");
    let stripped: String = config
        .lines()
        .filter(|line| !line.starts_with("format:"))
        .map(|line| format!("{line}\n"))
        .collect();
    project.write(".ai/exuno.yaml", &stripped);

    let skill = std::fs::read_to_string(
        std::env::var("CARGO_MANIFEST_DIR").unwrap()
            + "/lib/templates/base-src/skills/exuno/SKILL.md",
    )
    .unwrap()
    .replacen("name: exuno", "name: agentsync", 1);
    std::fs::create_dir_all(project.join(".ai/src/skills/agentsync/references")).unwrap();
    project.write(".ai/src/skills/agentsync/SKILL.md", &skill);
    let hash = project.sha256(".ai/src/skills/agentsync/SKILL.md");
    project.append(
        ".ai/.template-manifest",
        &format!("skills/agentsync/SKILL.md\t{hash}\n"),
    );
    project
}

#[test]
fn format_init_records_the_engines_revision_on_a_fresh_project() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line == expected)
    );
}

#[test]
fn format_a_fresh_project_has_nothing_to_migrate() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
}

#[test]
fn format_doctor_reports_the_revision() {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("Project format"));
}

#[test]
fn format_doctor_warns_when_the_project_is_behind() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("behind the engine"))
        .stdout(predicate::str::contains("exuno migrate"));
}

#[test]
fn format_migrate_previews_the_stale_skill_copy_without_touching_it() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("would remove"))
        .stdout(predicate::str::contains("skills/agentsync"));
    assert!(project.exists(".ai/src/skills/agentsync/SKILL.md"));
    assert!(
        !project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line.starts_with("format:"))
    );
}

#[test]
fn format_migrate_apply_removes_the_unedited_copy_and_records_the_revision() {
    let project = seed_pre_format_project();
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    assert!(!project.join(".ai/src/skills/agentsync").exists());
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line == expected)
    );
    assert!(
        !project
            .read(".ai/.template-manifest")
            .contains("skills/agentsync")
    );
}

#[test]
fn format_after_migrating_the_engine_supplies_the_skill_again() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(project.exists(".claude/skills/exuno/SKILL.md"));
}

#[test]
fn format_an_edited_copy_is_kept_as_a_deliberate_override() {
    let project = seed_pre_format_project();
    project.append(".ai/src/skills/agentsync/SKILL.md", "\nMY OWN NOTE\n");
    let format_rev =
        std::fs::read_to_string(std::env::var("CARGO_MANIFEST_DIR").unwrap() + "/FORMAT")
            .unwrap()
            .trim()
            .to_string();

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains("keep"));
    assert!(
        project
            .read(".ai/src/skills/exuno/SKILL.md")
            .contains("MY OWN NOTE")
    );
    let expected = format!("format: {format_rev}");
    assert!(
        project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line == expected)
    );
}

#[test]
fn format_an_edited_copy_moves_to_exuno_and_still_shadows_the_engine_version() {
    let project = seed_pre_format_project();
    project.append(".ai/src/skills/agentsync/SKILL.md", "\nMY OWN NOTE\n");
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".ai/src/skills/agentsync"));
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".claude/skills/exuno/SKILL.md")
            .contains("MY OWN NOTE")
    );
    assert!(!project.exists(".claude/skills/agentsync"));
}

/// A project an `agentsync` 0.44 install left at format r2: the legacy config
/// file and pin key, an edited engine-skill copy, legacy skill metadata, and
/// the legacy CI gate.
fn seed_r2_agentsync_project() -> Project {
    let project = Project::seeded(&["--tools", "claude", "--yes", "--no-sync"]);
    let config: String = project
        .read(".ai/exuno.yaml")
        .lines()
        .map(|line| {
            if line.starts_with("format:") {
                "format: 2\n".to_string()
            } else {
                format!("{}\n", line.replace("exuno_version:", "agentsync_version:"))
            }
        })
        .collect();
    std::fs::remove_file(project.join(".ai/exuno.yaml")).unwrap();
    project.write(".ai/agent_sync.yaml", &config);
    project.write(
        ".ai/src/skills/agentsync/SKILL.md",
        "---\nname: agentsync\ndescription: Notes on the engine\n---\n\nMY OWN NOTE\n",
    );
    project.write(
        ".ai/src/skills/deploy/SKILL.md",
        "---\nname: deploy\ndescription: Deploy the app\nmetadata:\n  agentsync-use-when: Shipping\n---\n",
    );
    project.write(
        ".github/workflows/agentsync-check.yml",
        "name: AgentSync\njobs:\n  check:\n    steps:\n      - run: agentsync check\n",
    );
    project
}

#[test]
fn format_r3_previews_every_agentsync_leftover_without_touching_it() {
    let project = seed_r2_agentsync_project();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Renamed to Exuno"))
        .stdout(predicate::str::contains(
            "would rename  .ai/agent_sync.yaml → .ai/exuno.yaml\n",
        ))
        .stdout(predicate::str::contains(
            "would rename  agentsync_version → exuno_version in .ai/exuno.yaml\n",
        ))
        .stdout(predicate::str::contains(
            "would rename  metadata.agentsync-* → metadata.exuno-* in .ai/src/skills/deploy/SKILL.md\n",
        ))
        .stdout(predicate::str::contains(
            "would rename  .ai/src/skills/agentsync/ → .ai/src/skills/exuno/\n",
        ))
        .stdout(predicate::str::contains(
            "would rename  .github/workflows/agentsync-check.yml → .github/workflows/exuno-check.yml\n",
        ))
        .stdout(predicate::str::contains("r2 → r3"))
        .stdout(predicate::str::contains(
            "  Releases before 0.45.0 cannot read .ai/exuno.yaml: update every machine and CI to 0.45.0 before you commit this.\n",
        ));
    assert!(project.exists(".ai/agent_sync.yaml"));
    assert!(project.exists(".ai/src/skills/agentsync/SKILL.md"));
    assert!(project.exists(".github/workflows/agentsync-check.yml"));
}

#[cfg(unix)]
#[test]
fn migrate_run_as_agentsync_suggests_agentsync_commands() {
    let project = seed_r2_agentsync_project();
    let bin = project.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::os::unix::fs::symlink(env!("CARGO_BIN_EXE_exuno"), bin.join("agentsync")).unwrap();
    assert_cmd::Command::new(bin.join("agentsync"))
        .current_dir(project.path())
        .env_remove("EXUNO_REPO_ROOT")
        .env_remove("AGENTSYNC_REPO_ROOT")
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Dry-run — re-run with agentsync migrate --apply to apply.",
        ));
}

#[cfg(unix)]
#[test]
fn format_r3_apply_warns_about_the_moved_config_even_when_a_later_rename_fails() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = seed_r2_agentsync_project();
    let deploy = project.join(".ai/src/skills/deploy");
    common::chmod(&deploy, 0o555);
    let assert = project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .failure();
    common::chmod(&deploy, 0o755);
    assert.stdout(predicate::str::contains(
        "renamed       .ai/agent_sync.yaml → .ai/exuno.yaml\n  Releases before 0.45.0 cannot read .ai/exuno.yaml",
    ));
    assert!(project.exists(".ai/exuno.yaml"));
}

#[test]
fn format_r3_apply_repins_a_project_pinned_before_the_rename() {
    let project = seed_r2_agentsync_project();
    let config = project.read(".ai/agent_sync.yaml");
    let pin_line = config
        .lines()
        .find(|line| line.starts_with("agentsync_version:"))
        .unwrap();
    project.write(
        ".ai/agent_sync.yaml",
        &config.replace(pin_line, "agentsync_version: \"0.44.2\""),
    );
    project.write(
        ".github/workflows/agentsync-check.yml",
        "      - name: Install AgentSync 0.44.2\n        run: curl -fsSL x | AGENTSYNC_VERSION=0.44.2 bash\n      - run: agentsync check\n",
    );
    let engine = env!("CARGO_PKG_VERSION");

    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(format!(
            "renamed       pin → {engine} in .ai/exuno.yaml — releases before 0.45.0 cannot read these names\n"
        )));
    assert!(
        project
            .read(".ai/exuno.yaml")
            .contains(&format!("\nexuno_version: \"{engine}\"\n"))
    );
    assert_eq!(
        project.read(".github/workflows/exuno-check.yml"),
        format!(
            "      - name: Install Exuno {engine}\n        run: curl -fsSL x | EXUNO_VERSION={engine} bash\n      - run: exuno check\n"
        )
    );
}

#[test]
fn format_r3_apply_renames_every_leftover_and_sync_ships_one_engine_skill() {
    let project = seed_r2_agentsync_project();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "renamed       .ai/agent_sync.yaml → .ai/exuno.yaml\n",
        ));
    assert!(!project.exists(".ai/agent_sync.yaml"));
    let config = project.read(".ai/exuno.yaml");
    assert!(config.lines().any(|line| line == "format: 3"), "{config}");
    assert!(
        config
            .lines()
            .any(|line| line.starts_with("exuno_version: "))
    );
    assert!(!config.contains("agentsync_version"));
    assert!(
        project
            .read(".ai/src/skills/exuno/SKILL.md")
            .starts_with("---\nname: exuno\n")
    );
    assert!(
        project
            .read(".ai/src/skills/deploy/SKILL.md")
            .contains("\n  exuno-use-when: Shipping\n")
    );
    assert!(
        project
            .read(".github/workflows/exuno-check.yml")
            .contains("run: exuno check")
    );
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".claude/skills/exuno/SKILL.md")
            .contains("MY OWN NOTE")
    );
    assert!(!project.exists(".claude/skills/agentsync"));
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Renamed to Exuno").not())
        .stdout(predicate::str::contains("Project format").not());
}

#[test]
fn format_migrating_twice_is_a_no_op() {
    let project = seed_pre_format_project();
    project
        .exuno()
        .args(["migrate", "--apply", "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["migrate", "--legacy"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nothing to migrate"));
}

#[test]
fn format_upgrade_config_does_not_silence_a_pending_migration() {
    let project = seed_pre_format_project();
    project.exuno().arg("upgrade-config").assert().success();
    assert!(
        !project
            .read(".ai/exuno.yaml")
            .lines()
            .any(|line| line.starts_with("format:"))
    );
}
