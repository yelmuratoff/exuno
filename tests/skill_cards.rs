mod common;

use std::path::Path;
use std::process::Command;

use common::Project;

const HEADER: &str =
    "id\tsource\tcommit\tpath\toasf_version\toasf_terms\tmapping\tuse_when\tnot_for\trequirements";

fn revision(project: &Project) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn status(project: &Project) -> String {
    let output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(project.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn row(id: &str, commit: &str, path: &str) -> String {
    format!(
        "{id}\tsample\t{commit}\t{path}\tv1.1.0\tengineering/skills\tclear\tInspect a pinned skill.\tInstall or execute anything.\tLocal Git object only."
    )
}

fn write_catalog(project: &Project, name: &str, rows: &[String]) {
    project.write(name, &format!("{HEADER}\n{}\n", rows.join("\n")));
}

fn source_with_skill(name: &str, description: &str) -> Project {
    let source = Project::empty();
    source.write(
        &format!("{name}/SKILL.md"),
        &format!("---\nname: {name}\ndescription: {description}\n---\n"),
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "pinned skill"]);
    source
}

fn source_arg(source: &Project) -> String {
    format!("sample={}", source.path().display())
}

#[test]
fn help_is_available_before_project_discovery_or_any_write() {
    let project = Project::empty();
    project.write("not-a-temp-directory", "not a directory\n");
    let output = project
        .exuno()
        .env("TMPDIR", project.join("not-a-temp-directory"))
        .args(["skills", "catalog", "help"])
        .output()
        .unwrap();

    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("agentsync skills catalog list")
    );
    assert!(!project.exists(".ai"));
    assert!(!project.exists(".update_cache"));
}

#[test]
fn list_keeps_cards_visible_when_their_source_is_not_mapped() {
    let project = Project::empty();
    let commit = "a".repeat(40);
    write_catalog(
        &project,
        "catalog.tsv",
        &[row("skill-one", &commit, "skill-one")],
    );

    let output = project
        .exuno()
        .args(["skills", "catalog", "list", "--catalog", "catalog.tsv"])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.starts_with("id\tsource\tsource_status\tmapping\n"));
    assert!(stdout.contains("skill-one\tsample\tunavailable ("));
    assert!(!project.exists(".ai"));
}

#[test]
fn show_reads_the_pinned_blob_not_a_later_or_dirty_worktree() {
    let project = Project::empty();
    let source = source_with_skill("skill-one", "Pinned description");
    let pinned = revision(&source);
    write_catalog(
        &project,
        "catalog.tsv",
        &[row("skill-one", &pinned, "skill-one")],
    );

    source.write(
        "skill-one/SKILL.md",
        "---\nname: skill-one\ndescription: Later committed description\n---\n",
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "later source"]);
    source.write(
        "skill-one/SKILL.md",
        "---\nname: skill-one\ndescription: Dirty working tree description\n---\n",
    );
    let before = status(&source);

    let output = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "show",
            "skill-one",
            "--catalog",
            "catalog.tsv",
            "--source",
            &source_arg(&source),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Source status: available ("));
    assert!(stdout.contains("Description: Pinned description"));
    assert!(!stdout.contains("Later committed description"));
    assert!(!stdout.contains("Dirty working tree description"));
    assert!(stdout.contains("Catalog annotations: curator notes; UNVERIFIED"));
    assert_eq!(status(&source), before);
}

#[test]
fn list_filters_ids_with_the_engine_globs() {
    let project = Project::empty();
    let commit = "a".repeat(40);
    write_catalog(
        &project,
        "catalog.tsv",
        &[row("pdf", &commit, "pdf"), row("tdd", &commit, "tdd")],
    );

    let output = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "list",
            "--catalog",
            "catalog.tsv",
            "--include",
            "p* tdd",
            "--exclude",
            "tdd",
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("pdf\tsample\t"));
    assert!(!stdout.contains("tdd\tsample\t"));
}

#[test]
fn malformed_catalogs_and_unsupported_options_write_no_partial_stdout() {
    let project = Project::empty();
    let commit = "a".repeat(40);
    write_catalog(
        &project,
        "bad.tsv",
        &[
            row("skill-one", &commit, "skill-one"),
            "not-a-valid-row".into(),
        ],
    );

    for args in [
        vec!["skills", "catalog", "list", "--catalog", "bad.tsv"],
        vec![
            "skills",
            "catalog",
            "list",
            "--catalog",
            "bad.tsv",
            "--set",
            "anything",
        ],
        vec!["skills", "catalog", "set", "--catalog", "bad.tsv"],
    ] {
        let output = project.exuno().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }
}

#[test]
fn unavailable_pins_and_frontmatter_are_reported_without_execution() {
    let project = Project::empty();
    let source = source_with_skill("plain-multiline", "One line");
    let pinned = revision(&source);
    source.write(
        "plain-multiline/SKILL.md",
        "---\nname: plain-multiline\ndescription: first line\n  second line\n---\n",
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "unsupported frontmatter"]);
    let unsupported = revision(&source);
    let marker = project.join("MUST-NOT-EXIST");
    let mut malicious = row("plain-multiline", &unsupported, "plain-multiline");
    malicious.push_str(&format!(" $(touch {})", marker.display()));
    write_catalog(
        &project,
        "catalog.tsv",
        &[
            malicious,
            row("missing-object", &"b".repeat(40), "plain-multiline"),
            row("old-pin", &pinned, "missing-file"),
        ],
    );

    let output = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "show",
            "plain-multiline",
            "--catalog",
            "catalog.tsv",
            "--source",
            &source_arg(&source),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Frontmatter: unsupported ("));
    assert!(stdout.contains("Name: unknown"));
    assert!(!Path::new(&marker).exists());

    let listed = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "list",
            "--catalog",
            "catalog.tsv",
            "--source",
            &source_arg(&source),
        ])
        .output()
        .unwrap();
    assert!(listed.status.success());
    let stdout = String::from_utf8(listed.stdout).unwrap();
    assert!(stdout.contains("missing-object\tsample\tunavailable ("));
    assert!(stdout.contains("old-pin\tsample\tunavailable ("));
    assert!(!marker.exists());
}

#[test]
fn unsupported_frontmatter_never_fabricates_a_pinned_name_or_description() {
    let project = Project::empty();
    let source = Project::empty();
    source.write(
        "missing-name/SKILL.md",
        "---\ndescription: no declared name\n---\n",
    );
    source.write(
        "plain-multiline/SKILL.md",
        "---\nname: plain-multiline\ndescription: first\n  second\n---\n",
    );
    source.write(
        "indicator-yaml/SKILL.md",
        "---\nname: indicator-yaml\ndescription: !untrusted value\n---\n",
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "unsupported metadata"]);
    let commit = revision(&source);
    write_catalog(
        &project,
        "catalog.tsv",
        &[
            row("missing-name", &commit, "missing-name"),
            row("plain-multiline", &commit, "plain-multiline"),
            row("indicator-yaml", &commit, "indicator-yaml"),
        ],
    );

    for id in ["missing-name", "plain-multiline", "indicator-yaml"] {
        let output = project
            .exuno()
            .args([
                "skills",
                "catalog",
                "show",
                id,
                "--catalog",
                "catalog.tsv",
                "--source",
                &source_arg(&source),
            ])
            .output()
            .unwrap();
        assert!(output.status.success());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(stdout.contains("Frontmatter: unsupported ("));
        assert!(stdout.contains("Name: unknown"));
        assert!(stdout.contains("Description: unknown"));
    }
}

#[test]
fn malformed_extra_source_field_does_not_appear_as_parsed_frontmatter() {
    let project = Project::empty();
    let source = Project::empty();
    source.write(
        "skill-one/SKILL.md",
        "---\nname: skill-one\ndescription: Valid description\nmetadata: [unterminated\n---\n",
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "malformed extra field"]);
    let commit = revision(&source);
    write_catalog(
        &project,
        "catalog.tsv",
        &[row("skill-one", &commit, "skill-one")],
    );

    let output = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "show",
            "skill-one",
            "--catalog",
            "catalog.tsv",
            "--source",
            &source_arg(&source),
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Frontmatter: unsupported ("));
    assert!(stdout.contains("Name: unknown\n"));
    assert!(stdout.contains("Description: unknown\n"));
}

#[test]
fn optional_frontmatter_blocks_preserve_pinned_source_metadata() {
    let project = Project::empty();
    let source = Project::empty();
    source.write(
        "block-scalars/SKILL.md",
        "---\nname: block-scalars\ndescription: Pinned description\ncompatibility: >-\n  Requires a local Git\n  repository.\nlicense: MIT\nx-curator-note: |-\n  This is an arbitrary\n  extension field.\nallowed-tools: Bash(git:*) Read\nmetadata:\n  source: |-\n    https://example.invalid/source\n    pinned only\n  version: '1.0'\n  when_to_use: >-\n    Inspect a pinned\n    skill card.\n  maintainer: example-org\n---\n",
    );
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "block scalar metadata"]);
    let commit = revision(&source);
    write_catalog(
        &project,
        "catalog.tsv",
        &[row("block-scalars", &commit, "block-scalars")],
    );

    let output = project
        .exuno()
        .args([
            "skills",
            "catalog",
            "show",
            "block-scalars",
            "--catalog",
            "catalog.tsv",
            "--source",
            &source_arg(&source),
        ])
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Frontmatter: parsed"));
    assert!(stdout.contains("Name: block-scalars"));
    assert!(stdout.contains("Description: Pinned description"));
}

#[test]
fn unsupported_optional_block_forms_leave_pinned_metadata_unknown() {
    let project = Project::empty();
    let source = Project::empty();
    let cases = [
        ("tag", "compatibility: !untrusted value\nlicense: MIT"),
        ("list", "compatibility: [git, curl]\nlicense: MIT"),
        ("flow", "compatibility: [unterminated\nlicense: MIT"),
        (
            "root-after-block",
            "compatibility: >-\n  Requires git.\nx-note: [unterminated",
        ),
        (
            "metadata-after-block",
            "metadata:\n  source: |-\n    A source note.\n  owner: [unterminated",
        ),
        (
            "indent",
            "compatibility: >-\n   Three spaces are not a block line.\nlicense: MIT",
        ),
        (
            "deeper",
            "metadata:\n  source: |-\n      Too deeply indented.\n  owner: example-org",
        ),
        (
            "blank",
            "metadata:\n  when_to_use: >-\n    First line.\n\n    Last line.\n  owner: example-org",
        ),
        (
            "allowed-tools-list",
            "allowed-tools:\n  - Bash(git:*)\nlicense: MIT",
        ),
    ];

    for (id, extra) in cases {
        source.write(
            &format!("{id}/SKILL.md"),
            &format!("---\nname: {id}\ndescription: Pinned description\n{extra}\n---\n"),
        );
    }
    source.git(&["add", "."]);
    source.git(&["commit", "--quiet", "-m", "unsupported block metadata"]);
    let commit = revision(&source);
    let rows: Vec<_> = cases.iter().map(|(id, _)| row(id, &commit, id)).collect();
    write_catalog(&project, "catalog.tsv", &rows);

    for (id, _) in cases {
        let output = project
            .exuno()
            .args([
                "skills",
                "catalog",
                "show",
                id,
                "--catalog",
                "catalog.tsv",
                "--source",
                &source_arg(&source),
            ])
            .output()
            .unwrap();

        assert!(output.status.success(), "{id}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(
            stdout.contains("Frontmatter: unsupported ("),
            "{id}: {stdout}"
        );
        assert!(stdout.contains("Name: unknown\n"), "{id}: {stdout}");
        assert!(stdout.contains("Description: unknown\n"), "{id}: {stdout}");
    }
}
