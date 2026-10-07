//! `tests/opencode.bats`: `opencode.json` composition — settings plus the
//! canonical MCP source.

mod common;

use common::Project;
use predicates::prelude::*;

/// `seed_project --no-detect` + `enable_tools opencode`: every case starts
/// from an initialised project with OpenCode enabled.
fn seeded() -> Project {
    let project = Project::seeded(&["--no-detect"]);
    project.enable_tools(&["opencode"]);
    project
}

fn write_shared_mcp(project: &Project, json: &str) {
    project.write(".ai/src/mcp.json", &format!("{json}\n"));
}

#[test]
fn opencode_shared_local_mcp_is_composed_into_settings() {
    let project = seeded();
    project.write(
        ".ai/src/tools/opencode/settings.json",
        "{\"$schema\":\"https://opencode.ai/config.json\",\"theme\":\"system\"}\n",
    );
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"github\":{\"command\":\"npx\",\"args\":[\"-y\",\"@github/mcp\"],\"env\":{\"TOKEN\":\"${GITHUB_TOKEN}\"}}}}",
    );

    project.exuno().arg("sync").assert().success();

    assert_eq!(
        project.read("opencode.json"),
        "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"theme\": \"system\",\n  \"mcp\": {\n    \"github\": {\"type\": \"local\", \"command\": [\"npx\", \"-y\",\"@github/mcp\"], \"environment\": {\"TOKEN\":\"${GITHUB_TOKEN}\"}}\n  }\n}\n"
    );
}

#[test]
fn opencode_per_tool_mcp_overrides_the_shared_source() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"shared\":{\"command\":\"shared\"}}}",
    );
    project.write(
        ".ai/src/tools/opencode/mcp.json",
        "{\"mcpServers\":{\"private\":{\"command\":\"private\"}}}\n",
    );

    project.exuno().arg("sync").assert().success();

    let composed = project.read("opencode.json");
    assert!(composed.contains("\"private\""));
    assert!(!composed.contains("shared"));
}

#[test]
fn opencode_settings_owned_mcp_is_preserved_without_a_canonical_source() {
    let project = seeded();
    project.write(
        ".ai/src/tools/opencode/settings.json",
        "{\"mcp\":{\"native\":{\"type\":\"local\",\"command\":[\"native\"]}}}\n",
    );

    project.exuno().arg("sync").assert().success();

    assert_eq!(
        project.read(".ai/src/tools/opencode/settings.json"),
        project.read("opencode.json")
    );
}

#[test]
fn opencode_remote_mcp_preserves_supported_options() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"docs\":{\"type\":\"sse\",\"url\":\"https://example.test/mcp\",\"headers\":{\"Authorization\":\"Bearer {env:TOKEN}\"},\"enabled\":false,\"timeout\":9000,\"oauth\":false}}}",
    );

    project.exuno().arg("sync").assert().success();

    assert_eq!(
        project.read("opencode.json"),
        "{\n  \"$schema\": \"https://opencode.ai/config.json\",\n  \"mcp\": {\n    \"docs\": {\"type\": \"remote\", \"url\": \"https://example.test/mcp\", \"headers\": {\"Authorization\":\"Bearer {env:TOKEN}\"}, \"oauth\": false, \"enabled\": false, \"timeout\": 9000}\n  }\n}\n"
    );
}

#[test]
fn opencode_rejects_non_object_mcp_servers() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":[]}");

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("mcpServers must be an object"));
}

#[test]
fn opencode_rejects_a_non_object_server() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":{\"x\":[]}}");

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("server 'x' must be an object"));
}

#[test]
fn opencode_validates_server_field_types() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":{\"x\":{\"command\":7}}}");

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "server 'x' field 'command' must be a string",
        ));
}

#[test]
fn opencode_rejects_ambiguous_transport() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"x\":{\"command\":\"a\",\"url\":\"https://x\"}}}",
    );

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "server 'x' must define exactly one transport",
        ));
}

#[test]
fn opencode_rejects_unsupported_fields() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"x\":{\"command\":\"a\",\"bogus\":true}}}",
    );

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "server 'x' has unsupported field 'bogus'",
        ));
}

#[test]
fn opencode_validates_collection_and_option_types() {
    let cases: [(&str, &str); 6] = [
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"args\":[1]}}}",
            "field 'args' must contain only strings",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"env\":{\"A\":1}}}}",
            "field 'env' values must be strings",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"url\":\"https://x\",\"headers\":{\"A\":1}}}}",
            "field 'headers' values must be strings",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"enabled\":\"yes\"}}}",
            "field 'enabled' must be a boolean",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"timeout\":-1}}}",
            "field 'timeout' must be a non-negative number",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"url\":\"https://x\",\"oauth\":\"yes\"}}}",
            "field 'oauth' must be a boolean or object",
        ),
    ];
    let project = seeded();
    for (input, expected) in cases {
        write_shared_mcp(&project, input);
        project
            .exuno()
            .arg("sync")
            .assert()
            .failure()
            .stderr(predicate::str::contains(expected));
    }
}

#[test]
fn opencode_enforces_transport_specific_fields_and_types() {
    let cases: [(&str, &str); 5] = [
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"type\":\"sse\"}}}",
            "must be stdio for a local server",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"url\":\"https://x\",\"type\":\"stdio\"}}}",
            "is not a supported remote transport",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"headers\":{}}}}",
            "contains remote-only fields",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"url\":\"https://x\",\"args\":[]}}}",
            "contains local-only fields",
        ),
        (
            "{\"mcpServers\":{\"x\":{\"enabled\":true}}}",
            "must define exactly one transport",
        ),
    ];
    let project = seeded();
    for (input, expected) in cases {
        write_shared_mcp(&project, input);
        project
            .exuno()
            .arg("sync")
            .assert()
            .failure()
            .stderr(predicate::str::contains(expected));
    }
}

#[test]
fn opencode_preserves_valid_json_escapes() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcp\\u0053ervers\":{\"escaped\\u002dname\":{\"comm\\u0061nd\":\"tool\\\\bin\",\"args\":[\"line\\nvalue\"],\"env\":{\"QUOTE\":\"a\\\"b\"}}}}",
    );

    project.exuno().arg("sync").assert().success();

    let composed = project.read("opencode.json");
    assert!(composed.contains("\"command\": [\"tool\\\\bin\", \"line\\nvalue\"]"));
    assert!(composed.contains("\"QUOTE\":\"a\\\"b\""));
}

#[test]
fn opencode_rejects_unsupported_canonical_top_level_fields() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":{},\"version\":1}");

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "unsupported top-level field 'version'",
        ));
}

#[test]
fn opencode_rejects_duplicate_keys() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"x\":{\"command\":\"a\",\"command\":\"b\"}}}",
    );

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("duplicate server field 'command'"));
}

#[test]
fn opencode_rejects_settings_and_canonical_mcp_ownership_conflict() {
    let project = seeded();
    project.write(
        ".ai/src/tools/opencode/settings.json",
        "{\"mcp\":{\"native\":{\"type\":\"local\",\"command\":[\"native\"]}}}\n",
    );
    write_shared_mcp(&project, "{\"mcpServers\":{\"x\":{\"command\":\"x\"}}}");

    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "both define OpenCode MCP ownership",
        ))
        .stderr(predicate::str::contains(
            ".ai/src/tools/opencode/settings.json",
        ))
        .stderr(predicate::str::contains(".ai/src/mcp.json"));
}

#[test]
fn opencode_malformed_mcp_leaves_destination_and_manifest_unchanged() {
    let project = seeded();
    project.exuno().arg("sync").assert().success();
    project.write("opencode.json", "ORIGINAL\n");
    let manifest_before = project.sha256(".ai/.sync-manifest");
    write_shared_mcp(&project, "{\"mcpServers\":");

    project.exuno().args(["sync", "--force"]).assert().failure();

    assert_eq!(project.read("opencode.json"), "ORIGINAL\n");
    assert_eq!(project.sha256(".ai/.sync-manifest"), manifest_before);
}

#[test]
fn opencode_dry_run_validates_malformed_mcp() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":");

    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .failure();

    assert!(!project.exists("opencode.json"));
}

#[test]
fn opencode_dry_run_composes_without_writing() {
    let project = seeded();
    write_shared_mcp(&project, "{\"mcpServers\":{\"x\":{\"command\":\"x\"}}}");

    project
        .exuno()
        .args(["sync", "--dry-run"])
        .assert()
        .success();

    assert!(!project.exists("opencode.json"));
}

#[test]
fn opencode_repeated_composition_is_byte_identical() {
    let project = seeded();
    write_shared_mcp(
        &project,
        "{\"mcpServers\":{\"x\":{\"command\":\"x\",\"enabled\":true,\"timeout\":12}}}",
    );
    project.exuno().arg("sync").assert().success();
    let config_before = project.sha256("opencode.json");
    let manifest_before = project.sha256(".ai/.sync-manifest");

    project.exuno().arg("sync").assert().success();

    assert_eq!(project.sha256("opencode.json"), config_before);
    assert_eq!(project.sha256(".ai/.sync-manifest"), manifest_before);
}
