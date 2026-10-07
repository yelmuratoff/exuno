//! `tests/list.bats`: `exuno list` in an initialised project. The alias
//! case is `ls_is_an_alias_for_list` in `tests/cli.rs`.

mod common;

use common::Project;
use predicates::prelude::*;

fn list(project: &Project) -> assert_cmd::assert::Assert {
    project.exuno().arg("list").assert().success()
}

#[test]
fn list_shows_tools_header() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("Exuno Tools"));
}

#[test]
fn list_shows_base_tools_from_catalog() {
    list(&Project::seeded(&[]))
        .stdout(predicate::str::contains("Claude Code"))
        .stdout(predicate::str::contains("Cursor"))
        .stdout(predicate::str::contains("Kimi Code"))
        .stdout(predicate::str::contains("OpenCode"));
}

#[test]
fn list_shows_available_for_unenabled_tools() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("available"));
}

#[test]
fn list_reports_enabled_tool_count() {
    list(&Project::seeded(&[])).stdout(predicate::str::contains("enabled"));
}

#[test]
fn list_works_even_without_ai_directory_uses_base_catalog() {
    let project = Project::seeded(&[]);
    std::fs::remove_dir_all(project.join(".ai")).unwrap();
    list(&project).stdout(predicate::str::contains("Exuno Tools"));
}

#[test]
fn list_shows_enabled_marker_after_enable() {
    let project = Project::seeded(&[]);
    project
        .exuno()
        .args(["enable", "claude"])
        .assert()
        .success();
    list(&project).stdout(predicate::str::contains("enabled"));
}

#[test]
fn list_survives_a_tool_override_that_does_not_set_enabled() {
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/cursor.yaml", "name: \"My Cursor\"\n");
    list(&project)
        .stdout(predicate::str::contains("My Cursor"))
        .stdout(predicate::str::contains("1 tool override(s)"));
}

#[test]
fn list_help_is_answered_on_stdout_without_the_table() {
    Project::empty()
        .exuno()
        .args(["list", "-h"])
        .assert()
        .success()
        .stdout(predicate::str::starts_with(
            "\n  exuno list — show available tools and their status\n\n  USAGE\n    exuno list\n    exuno ls\n",
        ))
        .stdout(predicate::str::contains("\n  LEGEND\n"))
        .stdout(predicate::str::contains("Exuno Tools").not())
        .stderr("");
}
