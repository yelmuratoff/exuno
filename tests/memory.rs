mod common;

use common::Project;
use predicates::prelude::*;

const NOTE: &str = "---\nname: no-attribution\ndescription: Commits carry no AI trailer\nmetadata:\n  type: feedback\n  modified: 2026-11-20T10:00:00.000Z\n---\n\nBody.\n";

fn project() -> Project {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Agent\n");
    project.write(".ai/exuno.yaml", "base_skills: false\n");
    project
}

/// The directory name Claude Code gives the project's auto memory.
fn key(project: &Project) -> String {
    project
        .path()
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn claude_memory(key: &str) -> String {
    format!("home/.claude/projects/{key}/memory")
}

fn memory(project: &Project, args: &[&str]) -> assert_cmd::Command {
    let mut command = project.exuno();
    command
        .arg("memory")
        .args(args)
        .env("HOME", project.join("home"))
        .env("PWD", project.path())
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CODEX_HOME");
    command
}

#[test]
fn list_reads_the_current_projects_memory_only() {
    let project = project();
    let dir = claude_memory(&key(&project));
    project.write(
        &format!("{dir}/MEMORY.md"),
        "- [No attribution](feedback.md)\n",
    );
    project.write(&format!("{dir}/feedback.md"), NOTE);
    project.write("home/.claude/projects/-other/memory/other.md", NOTE);
    memory(&project, &["list"])
        .assert()
        .success()
        .stdout(format!(
            "tool\tproject\tname\ttype\tmodified\tdescription\tpath\nclaude\t{key}\tno-attribution\tfeedback\t2026-11-20T10:00:00.000Z\tCommits carry no AI trailer\t~/.claude/projects/{key}/memory/feedback.md\n",
            key = key(&project)
        ))
        .stderr("");
}

#[test]
fn list_all_reads_every_project_and_global_stores() {
    let project = project();
    project.write("home/.claude/projects/-other/memory/other.md", NOTE);
    project.write("home/.codex/memories_v2/memory_summary.md", "# Summary\n");
    memory(&project, &["list", "--all"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "claude\t-other\tno-attribution\tfeedback\t",
        ))
        .stdout(predicate::str::contains(
            "codex\t\t\t\t\t\t~/.codex/memories_v2/memory_summary.md\n",
        ));
}

#[test]
fn list_honours_the_config_dir_variables() {
    let project = project();
    project.write("elsewhere/projects/-other/memory/other.md", NOTE);
    memory(&project, &["list", "--all"])
        .env("CLAUDE_CONFIG_DIR", project.join("elsewhere"))
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "elsewhere/projects/-other/memory/other.md",
        ));
}

#[test]
fn list_reads_a_profile_variant_in_its_config_home() {
    let project = project();
    project.write(
        ".ai/src/tools/claude-hub.yaml",
        "base: claude\nprofile_home: \".claude-hub\"\n",
    );
    project.write(
        &format!(".claude-hub/projects/{}/memory/feedback.md", key(&project)),
        NOTE,
    );
    memory(&project, &["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("claude-hub\t"))
        .stdout(predicate::str::contains("no-attribution"));
}

#[test]
fn a_tool_declares_its_memory_in_yaml_without_code() {
    let project = project();
    project.write(
        ".ai/src/tools/gemini.yaml",
        "memory:\n  home: \"~/.gemini\"\n  dirs: [memories]\n",
    );
    project.write("home/.gemini/memories/prefs.md", NOTE);
    memory(&project, &["list"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "gemini\t\tno-attribution\tfeedback\t",
        ));
}

#[test]
fn list_json_prints_one_array() {
    let project = project();
    let dir = claude_memory(&key(&project));
    project.write(&format!("{dir}/feedback.md"), NOTE);
    let output = memory(&project, &["list", "--json"]).output().unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value[0]["tool"], "claude");
    assert_eq!(value[0]["name"], "no-attribution");
    assert_eq!(value[0]["type"], "feedback");
    assert_eq!(value.as_array().unwrap().len(), 1);
}

#[test]
fn check_passes_a_tidy_store() {
    let project = project();
    let dir = claude_memory(&key(&project));
    project.write(
        &format!("{dir}/MEMORY.md"),
        "- [No attribution](feedback.md)\n",
    );
    project.write(&format!("{dir}/feedback.md"), NOTE);
    memory(&project, &["check"])
        .assert()
        .success()
        .stdout("Checked 1 memory: 0 issues\n");
}

#[test]
fn check_reports_index_and_type_problems() {
    let project = project();
    let dir = claude_memory(&key(&project));
    let index = format!("- [Gone](gone.md)\n{}", "- line\n".repeat(200));
    project.write(&format!("{dir}/MEMORY.md"), &index);
    project.write(
        &format!("{dir}/untyped.md"),
        "---\nname: untyped\ndescription: d\n---\n",
    );
    project.write(
        &format!("{dir}/odd.md"),
        "---\nname: odd\ndescription: d\ntype: diary\n---\n",
    );
    memory(&project, &["check"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "MEMORY.md: 201 lines; only the first 200 lines or 25600 bytes load",
        ))
        .stdout(predicate::str::contains(
            "MEMORY.md: links to gone.md, which does not exist",
        ))
        .stdout(predicate::str::contains(
            "untyped.md: not listed in MEMORY.md",
        ))
        .stdout(predicate::str::contains("untyped.md: no type"))
        .stdout(predicate::str::contains(
            "odd.md: type 'diary' is not one of user, feedback, project, reference",
        ))
        .stdout(predicate::str::contains("Checked 2 memories: 6 issues"));
}

#[test]
fn check_all_names_a_note_several_projects_share() {
    let project = project();
    for slug in ["-one", "-two"] {
        let dir = format!("home/.claude/projects/{slug}/memory");
        project.write(
            &format!("{dir}/MEMORY.md"),
            "- [No attribution](feedback.md)\n",
        );
        project.write(&format!("{dir}/feedback.md"), NOTE);
    }
    memory(&project, &["check", "--all"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "-one/memory/feedback.md: name 'no-attribution' is also in -two; consider one shared rule",
        ));
}

#[test]
fn check_reports_stale_notes_only_when_asked() {
    let project = project();
    let dir = claude_memory(&key(&project));
    project.write(&format!("{dir}/MEMORY.md"), "- [Old](old.md)\n");
    project.write(
        &format!("{dir}/old.md"),
        "---\nname: old\ndescription: d\ntype: project\nmodified: 2001-01-01\n---\n",
    );
    memory(&project, &["check"]).assert().success();
    memory(&project, &["check", "--stale-days", "30"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains(
            "old.md: last modified 2001-01-01, more than 30 days ago",
        ));
}

#[test]
fn memory_refuses_an_unknown_action() {
    let project = project();
    memory(&project, &[])
        .assert()
        .code(1)
        .stderr("Error: expected memory list|check\n");
    memory(&project, &["list", "--stale-days", "3"])
        .assert()
        .code(1)
        .stderr("Error: unknown option: --stale-days\n");
}
