//! `tests/generate.bats`: `exuno generate`.

mod common;

use common::Project;
use predicates::prelude::*;

#[test]
fn generate_with_argument_outputs_prompt_with_context() {
    let project = Project::empty();
    project
        .exuno()
        .args(["generate", "Flutter app with BLoC"])
        .assert()
        .success()
        .stdout(predicate::str::contains("My Project"))
        .stdout(predicate::str::contains("Flutter app with BLoC"))
        .stdout(predicate::str::contains("Exuno"));
}

#[test]
fn generate_with_argument_includes_instructions() {
    let project = Project::empty();
    project
        .exuno()
        .args(["generate", "React project"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTS.md"))
        .stdout(predicate::str::contains("rules"))
        .stdout(predicate::str::contains("skills"));
}

#[test]
fn generate_piped_outputs_raw_prompt() {
    let project = Project::empty();
    project
        .exuno()
        .arg("generate")
        .write_stdin("")
        .assert()
        .success()
        .stdout(predicate::str::contains("Exuno"))
        .stdout(predicate::str::contains(".ai/src/"));
}

#[test]
fn generate_context_appears_before_instructions() {
    let project = Project::empty();
    let output = project
        .exuno()
        .args(["generate", "My custom project"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8(output).unwrap();
    let context_line = stdout
        .lines()
        .position(|line| line.contains("My custom project"))
        .unwrap();
    let instructions_line = stdout
        .lines()
        .position(|line| line.contains("Exuno"))
        .unwrap();
    assert!(context_line < instructions_line);
}

#[test]
fn generate_multi_word_context_is_preserved() {
    let project = Project::empty();
    project
        .exuno()
        .args(["generate", "React + Next.js + Prisma ORM with PostgreSQL"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "React + Next.js + Prisma ORM with PostgreSQL",
        ));
}

#[test]
fn generate_prompt_mentions_all_source_types() {
    let project = Project::empty();
    project
        .exuno()
        .args(["generate", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("AGENTS.md"))
        .stdout(predicate::str::contains("rules/"))
        .stdout(predicate::str::contains("skills/"))
        .stdout(predicate::str::contains("commands/"))
        .stdout(predicate::str::contains("agents/"))
        .stdout(predicate::str::contains("settings.json"));
}

#[test]
fn generate_prompt_uses_yaml_safe_frontmatter_examples() {
    let project = Project::empty();
    project
        .exuno()
        .args(["generate", "test"])
        .assert()
        .success()
        .stdout(predicate::str::contains("name: \"skill-name\""))
        .stdout(predicate::str::contains("name: \"agent-name\""))
        .stdout(predicate::str::contains("description: >-"))
        .stdout(predicate::str::contains(
            "YAML frontmatter must be parseable",
        ));
}
