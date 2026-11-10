//! The `manifests` target: template files rendered with `${key}` values from
//! `targets.manifests.vars`, owned file by file below the target's dest.

mod common;

use common::Project;
use predicates::prelude::*;

const A_JSON: &str = "{\"name\": \"${name}\", \"description\": \"${description}\"}\n";
const B_JSON: &str = "{\"owner\": {\"name\": \"${owner}\"}}\n";

fn demo_yaml(dest: &str, extra: &str) -> String {
    format!(
        "name: \"Demo\"\nenabled: true\ntargets:\n  manifests:\n    dest: \"{dest}\"\n{extra}    vars:\n      name: \"my-skills\"\n      description: 'Says \"hi\" \\ there'\n      owner: \"exuno\"\n"
    )
}

/// A seeded project with the custom `demo` tool and its two templates.
fn demo(dest: &str, extra: &str) -> Project {
    let project = Project::seeded(&[]);
    project.write(".ai/src/tools/demo.yaml", &demo_yaml(dest, extra));
    project.write(".ai/src/tools/demo/manifests/a.json", A_JSON);
    project.write(".ai/src/tools/demo/manifests/nested/b.json", B_JSON);
    project
}

fn json(project: &Project, rel: &str) -> serde_json::Value {
    serde_json::from_str(&project.read(rel)).unwrap()
}

#[test]
fn sync_writes_filled_manifests() {
    let project = demo("out", "");
    project.exuno().arg("sync").assert().success();
    let a = json(&project, "out/a.json");
    assert_eq!(a["name"], "my-skills");
    assert_eq!(a["description"], "Says \"hi\" \\ there");
    assert_eq!(
        json(&project, "out/nested/b.json")["owner"]["name"],
        "exuno"
    );
    assert_eq!(
        project.read("out/a.json"),
        "{\"name\": \"my-skills\", \"description\": \"Says \\\"hi\\\" \\\\ there\"}\n"
    );
}

#[test]
fn sync_fails_and_restores_when_a_var_is_missing() {
    let project = demo("out", "");
    project.exuno().arg("sync").assert().success();
    let before = project.read("out/a.json");

    let yaml = demo_yaml("out", "    agents:\n      dest: \"DEMO.md\"\n")
        .replace("      owner: \"exuno\"\n", "")
        .replace("my-skills", "renamed");
    project.write(".ai/src/tools/demo.yaml", &yaml);
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Manifest template .ai/src/tools/demo/manifests/nested/b.json: no value for ${owner} (set targets.manifests.vars.owner)",
        ));
    assert_eq!(project.read("out/a.json"), before);
    assert!(!project.exists("DEMO.md"));
}

#[test]
fn manifests_exclude_skips_matching_templates() {
    let project = demo("out", "");
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("out/nested/b.json"));

    project.write(
        ".ai/src/tools/demo.yaml",
        &demo_yaml("out", "    exclude: \"nested/*\"\n"),
    );
    project.exuno().arg("sync").assert().success();
    assert!(project.exists("out/a.json"));
    assert!(!project.exists("out/nested/b.json"));
    project.exuno().arg("check").assert().success();
}

#[test]
fn a_root_dest_leaves_unrelated_root_files_alone() {
    let project = demo(".", "");
    project.enable_tools(&["claude"]);
    project.write("notes.txt", "mine\n");
    project.write("nested/user.json", "{}\n");
    project.exuno().arg("sync").assert().success();
    assert_eq!(json(&project, "a.json")["name"], "my-skills");
    assert_eq!(json(&project, "nested/b.json")["owner"]["name"], "exuno");
    assert_eq!(project.read("notes.txt"), "mine\n");
    assert_eq!(project.read("nested/user.json"), "{}\n");
    project.exuno().arg("check").assert().success();

    project.write(
        ".ai/src/tools/demo.yaml",
        &demo_yaml(".", "").replace("enabled: true", "enabled: false"),
    );
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists("a.json"));
    assert!(!project.exists("nested/b.json"));
    assert_eq!(project.read("notes.txt"), "mine\n");
    assert_eq!(project.read("nested/user.json"), "{}\n");
    assert!(project.exists("CLAUDE.md"));
    assert!(project.exists(".ai/exuno.yaml"));
    assert!(project.exists(".ai/src/tools/demo/manifests/a.json"));
}

#[test]
fn a_root_dest_lists_its_files_in_the_managed_gitignore_block() {
    let project = demo(".", "");
    let config = project.read(".ai/exuno.yaml");
    project.write(
        ".ai/exuno.yaml",
        &config.replace("outputs: committed", "outputs: local"),
    );
    project.exuno().arg("sync").assert().success();
    let gitignore = project.read(".gitignore");
    let block: Vec<&str> = gitignore
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .collect();
    assert!(block.contains(&"a.json"), "{gitignore}");
    assert!(block.contains(&"nested/b.json"), "{gitignore}");
    assert!(
        !block.iter().any(|line| matches!(*line, "." | "./" | "/")),
        "{gitignore}"
    );
}

#[test]
fn removing_a_template_removes_only_its_output() {
    let project = demo("out", "");
    project.exuno().arg("sync").assert().success();
    project.write("out/user.txt", "mine\n");
    std::fs::remove_file(project.join(".ai/src/tools/demo/manifests/nested/b.json")).unwrap();
    project.exuno().arg("sync").assert().success();
    assert!(!project.exists("out/nested/b.json"));
    assert!(project.exists("out/a.json"));
    assert_eq!(project.read("out/user.txt"), "mine\n");
}

#[test]
fn second_sync_is_a_no_op() {
    let project = demo(".", "");
    project.exuno().arg("sync").assert().success();
    let files = ["a.json", "nested/b.json", ".ai/.sync-manifest"];
    let before: Vec<String> = files.iter().map(|rel| project.read(rel)).collect();
    project.exuno().arg("sync").assert().success();
    let after: Vec<String> = files.iter().map(|rel| project.read(rel)).collect();
    assert_eq!(before, after);
    assert!(before[2].contains("a.json\t"), "{}", before[2]);
}

#[test]
fn check_fails_after_a_manual_edit_of_a_manifest() {
    let project = demo("out", "");
    project.exuno().arg("sync").assert().success();
    project.exuno().arg("check").assert().success();
    project.write("out/a.json", "{\"name\": \"edited\"}\n");
    project
        .exuno()
        .arg("check")
        .assert()
        .code(1)
        .stdout(predicate::str::contains("out/a.json"));
}
