mod common;

use common::Project;
use predicates::prelude::*;

#[test]
fn bundled_pilot_catalog_validates_and_renders_each_connection() {
    let project = Project::empty();
    let library = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("catalog/mcp");
    project
        .exuno()
        .args(["mcp", "list", "--library"])
        .arg(&library)
        .assert()
        .success()
        .stdout("context7\tContext7\nmicrosoft-learn\tMicrosoft Learn\noctocode\tOctocode\n");
    project
        .exuno()
        .args(["mcp", "validate", "--library"])
        .arg(&library)
        .assert()
        .success();
    for (id, expected) in [
        (
            "context7",
            "{\"mcpServers\":{\"context7\":{\"type\":\"http\",\"url\":\"https://mcp.context7.com/mcp/oauth\"}}}\n",
        ),
        (
            "microsoft-learn",
            "{\"mcpServers\":{\"microsoft-learn\":{\"type\":\"http\",\"url\":\"https://learn.microsoft.com/api/mcp\"}}}\n",
        ),
        (
            "octocode",
            "{\"mcpServers\":{\"octocode\":{\"args\":[\"-y\",\"octocode-mcp@19.1.0\"],\"command\":\"npx\"}}}\n",
        ),
    ] {
        project
            .exuno()
            .args(["mcp", "render", &format!("{id}@recommended"), "--library"])
            .arg(&library)
            .assert()
            .success()
            .stdout(expected);
    }
    assert!(!project.exists(".ai/backups"));
    assert!(!project.exists(".mcp.json"));
}

fn manifest(id: &str, title: &str) -> String {
    format!(
        r#"{{"schema_version":1,"id":"{id}","title":"{title}","connection":{{"type":"stdio","command":"never-run","args":[]}},"requirements":{{"binaries":[],"inputs":[]}}}}"#
    )
}

#[test]
fn help_and_read_only_commands_need_no_project_setup() {
    let project = Project::empty();
    project
        .exuno()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp list --library"));

    project.write("catalog/zeta/manifest.json", &manifest("zeta", "Last"));
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project
        .exuno()
        .args(["mcp", "list", "--library", "catalog"])
        .assert()
        .success()
        .stdout("alpha\tFirst\nzeta\tLast\n");
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .success()
        .stdout("MCP library is valid\n");
    assert!(!project.exists("never-run"));
}

#[test]
fn show_preserves_exact_source_bytes_and_validates_only_the_selected_entry() {
    let project = Project::empty();
    let original = format!("{}\n", manifest("alpha", "Grüße"));
    project.write("catalog/alpha/manifest.json", &original);
    project.write("catalog/broken/manifest.json", "not JSON");
    project
        .exuno()
        .args(["mcp", "show", "alpha", "--library", "catalog"])
        .assert()
        .success()
        .stdout(original);
    project
        .exuno()
        .args(["mcp", "list", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("");
}

#[test]
fn render_default_stdio_as_source_without_running_or_writing() {
    let project = Project::empty();
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project.write("catalog/broken/manifest.json", "not JSON");
    for selection in ["alpha", "alpha@default"] {
        project
            .exuno()
            .args(["mcp", "render", selection, "--library", "catalog"])
            .assert()
            .success()
            .stdout("{\"mcpServers\":{\"alpha\":{\"args\":[],\"command\":\"never-run\"}}}\n");
    }
    assert!(!project.exists("never-run"));
    assert!(!project.exists(".ai/src/mcp/claude.json"));
    assert!(!project.exists(".mcp.json"));
}

#[test]
fn render_resolves_http_alternatives_and_recommendation_explicitly() {
    let project = Project::empty();
    let v2 = r#"{"schema_version":2,"id":"docs","title":"Docs","connection":{"type":"http","url":"https://example.invalid/default"},"requirements":{"binaries":[],"inputs":[]},"alternatives":{"remote":{"connection":{"type":"http","url":"https://example.invalid/remote"},"requirements":{"binaries":[],"inputs":[]}}},"guidance":{"recommended":"remote","authority":"vendor","source":"https://example.invalid/docs","checked_at":"2024-02-29","reason":"Documented remote option"}}"#;
    project.write("catalog/docs/manifest.json", v2);
    project
        .exuno()
        .args(["mcp", "render", "docs", "--library", "catalog"])
        .assert()
        .success()
        .stdout("{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.invalid/default\"}}}\n");
    for selection in ["docs@remote", "docs@recommended"] {
        project
            .exuno()
            .args(["mcp", "render", selection, "--library", "catalog"])
            .assert()
            .success()
            .stdout("{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.invalid/remote\"}}}\n");
    }
    project.write(
        "catalog/docs/manifest.json",
        &v2.replace("\"recommended\":\"remote\"", "\"recommended\":\"default\""),
    );
    project
        .exuno()
        .args(["mcp", "render", "docs@recommended", "--library", "catalog"])
        .assert()
        .success()
        .stdout("{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.invalid/default\"}}}\n");
}

#[test]
fn render_rejects_bad_selections_and_invalid_manifests_without_stdout() {
    let project = Project::empty();
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    for selection in ["alpha@", "alpha@bad/name", "alpha@@remote", "../alpha"] {
        project
            .exuno()
            .args(["mcp", "render", selection, "--library", "catalog"])
            .assert()
            .failure()
            .stdout("");
    }
    project
        .exuno()
        .args(["mcp", "render", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("requires an id"));
    project
        .exuno()
        .args(["mcp", "render", "alpha@recommended", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("no attributed recommendation"));
    project
        .exuno()
        .args(["mcp", "render", "alpha@remote", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("Unknown MCP variant"));
    project.write("catalog/alpha/manifest.json", "not JSON");
    project
        .exuno()
        .args(["mcp", "render", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("");
}

#[test]
fn render_serializes_connection_strings_as_json_data() {
    let project = Project::empty();
    let escaped = manifest("alpha", "First").replace(
        "\"command\":\"never-run\",\"args\":[]",
        r#""command":"a\"b\\c","args":["line\nnext","\u0000"]"#,
    );
    project.write("catalog/alpha/manifest.json", &escaped);
    project
        .exuno()
        .args(["mcp", "render", "alpha", "--library", "catalog"])
        .assert()
        .success()
        .stdout("{\"mcpServers\":{\"alpha\":{\"args\":[\"line\\nnext\",\"\\u0000\"],\"command\":\"a\\\"b\\\\c\"}}}\n");
}

#[test]
fn use_previews_then_creates_a_per_tool_source_without_syncing() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project
        .exuno()
        .args(["mcp", "use", "alpha", "--tool", "claude", "--library", "catalog"])
        .assert()
        .success()
        .stdout("Would create .ai/src/tools/claude/mcp.json from alpha@default for claude:\n{\"mcpServers\":{\"alpha\":{\"args\":[],\"command\":\"never-run\"}}}\nRun with --apply to write it.\n");
    assert!(!project.exists(".ai/src/tools/claude/mcp.json"));
    assert!(!project.exists(".ai/backups"));
    project
        .exuno()
        .args(["mcp", "use", "alpha", "--tool", "claude", "--library", "catalog", "--apply"])
        .assert()
        .success()
        .stdout("Created .ai/src/tools/claude/mcp.json from alpha@default for claude\nReview the source, then run exuno sync to update client files.\n");
    assert_eq!(
        project.read(".ai/src/tools/claude/mcp.json"),
        "{\"mcpServers\":{\"alpha\":{\"args\":[],\"command\":\"never-run\"}}}\n"
    );
    assert!(!project.exists(".mcp.json"));
    project
        .exuno()
        .args(["rollback", "--list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mcp-use"));
    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".ai/src/tools/claude/mcp.json"));
}

#[test]
fn use_merge_preserves_other_servers_and_rolls_back() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let original = "{\"notes\":{\"private\":\"SECRET_SOURCE_VALUE\"},\"mcpServers\":{\"existing\":{\"command\":\"old\"}}}\n";
    project.write(".ai/src/tools/claude/mcp.json", original);
    let output = project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--merge",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(!String::from_utf8_lossy(&output.stdout).contains("SECRET_SOURCE_VALUE"));
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), original);
    assert!(!project.exists(".ai/backups"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--merge",
            "--apply",
        ])
        .assert()
        .success();
    let merged = project.read(".ai/src/tools/claude/mcp.json");
    let value: serde_json::Value = serde_json::from_str(&merged).unwrap();
    assert_eq!(value["notes"]["private"], "SECRET_SOURCE_VALUE");
    assert_eq!(value["mcpServers"]["existing"]["command"], "old");
    assert_eq!(value["mcpServers"]["alpha"]["command"], "never-run");
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--merge",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("no change needed"));
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), merged);
    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), original);
}

#[test]
fn use_merge_requires_explicit_replacement_and_refuses_ambiguous_sources() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let original = "{\"mcpServers\":{\"alpha\":{\"command\":\"old\"}}}";
    project.write(".ai/src/tools/claude/mcp.json", original);
    let base = [
        "mcp",
        "use",
        "alpha",
        "--tool",
        "claude",
        "--library",
        "catalog",
        "--merge",
        "--apply",
    ];
    project.exuno().args(base).assert().failure().stdout("");
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), original);
    project
        .exuno()
        .args(base)
        .args(["--replace", "other"])
        .assert()
        .failure()
        .stdout("");
    project
        .exuno()
        .args(base)
        .args(["--replace", "alpha"])
        .assert()
        .success();
    assert!(
        project
            .read(".ai/src/tools/claude/mcp.json")
            .contains("never-run")
    );

    project.write(".ai/src/tools/claude/mcp.json.bak", "other");
    project.exuno().args(base).assert().failure().stdout("");
}

#[test]
fn use_merge_refuses_a_live_or_stale_lock_without_writing() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let original = "{\"mcpServers\":{}}";
    project.write(".ai/src/tools/claude/mcp.json", original);
    project.write(".ai/src/tools/claude/.agentsync-mcp-use.lock", "occupied");
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--merge",
            "--apply",
        ])
        .assert()
        .failure()
        .stdout("");
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), original);
    assert!(!project.exists(".ai/backups"));
}

#[test]
fn use_merge_refuses_invalid_sources_and_backup_failure() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let command = [
        "mcp",
        "use",
        "alpha",
        "--tool",
        "claude",
        "--library",
        "catalog",
        "--merge",
        "--apply",
    ];
    for invalid in [
        "{\"mcpServers\":{},\"mcpServers\":{}}",
        "{\"mcpServers\":[]}",
        "{\"mcpServers\":{\"other\":[]}}",
    ] {
        project.write(".ai/src/tools/claude/mcp.json", invalid);
        project.exuno().args(command).assert().failure().stdout("");
        assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), invalid);
    }
    let original = "{\"mcpServers\":{}}";
    project.write(".ai/src/tools/claude/mcp.json", original);
    project.write(".ai/backups", "private bytes");
    project.exuno().args(command).assert().failure().stdout("");
    assert_eq!(project.read(".ai/src/tools/claude/mcp.json"), original);
    assert_eq!(project.read(".ai/backups"), "private bytes");
    assert!(!project.exists(".ai/src/tools/claude/.agentsync-mcp-use.lock"));
}

#[test]
fn use_merge_does_not_migrate_a_shared_source() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let original = "{\"mcpServers\":{}}";
    project.write(".ai/src/mcp.json", original);
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--merge",
            "--apply",
        ])
        .assert()
        .failure()
        .stdout("");
    assert_eq!(project.read(".ai/src/mcp.json"), original);
    assert!(!project.exists(".ai/src/tools/claude/mcp.json"));
    assert!(!project.exists(".ai/backups"));
}

#[test]
fn use_respects_source_tools_and_opencode_composition() {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Agent\n");
    project.write(
        ".ai/exuno.yaml",
        "tools:\n  enabled: [opencode]\nsource:\n  tools: custom/tools\n",
    );
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "opencode",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("custom/tools/opencode/mcp.json"));
    assert!(project.exists("custom/tools/opencode/mcp.json"));
    assert!(!project.exists("opencode.json"));
    project.exuno().arg("sync").assert().success();
    assert!(project.read("opencode.json").contains("\"alpha\""));
}

#[test]
fn use_source_reaches_claude_on_a_separate_sync() {
    let project = Project::empty();
    project.write(".ai/src/AGENTS.md", "# Agent\n");
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".mcp.json"),
        project.read(".ai/src/tools/claude/mcp.json")
    );
}

#[test]
fn use_kimi_http_writes_native_source_and_round_trips_through_sync_and_adopt() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["kimi"]);
    project.write(
        "catalog/docs/manifest.json",
        r#"{"schema_version":1,"id":"docs","title":"Docs","connection":{"type":"http","url":"https://example.invalid/mcp"},"requirements":{"binaries":[],"inputs":[]}}"#,
    );
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "docs",
            "--tool",
            "kimi",
            "--library",
            "catalog",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "{\"mcpServers\":{\"docs\":{\"url\":\"https://example.invalid/mcp\"}}}",
        ));
    assert!(!project.exists(".ai/src/tools/kimi/mcp.json"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "docs",
            "--tool",
            "kimi",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .success();
    let native = "{\"mcpServers\":{\"docs\":{\"url\":\"https://example.invalid/mcp\"}}}\n";
    assert_eq!(project.read(".ai/src/tools/kimi/mcp.json"), native);
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".kimi-code/mcp.json"), native);
    project.exuno().arg("check").assert().success();
    project.exuno().arg("doctor").assert().success();
    project.append(".kimi-code/mcp.json", "\n");
    project
        .exuno()
        .args(["adopt", "--yes", ".kimi-code/mcp.json"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/tools/kimi/mcp.json"),
        format!("{native}\n")
    );
}

#[test]
fn use_kimi_stdio_keeps_native_command_and_args() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [kimi]\n");
    project.write("catalog/docs/manifest.json", &manifest("docs", "Docs"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "docs",
            "--tool",
            "kimi",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/tools/kimi/mcp.json"),
        "{\"mcpServers\":{\"docs\":{\"args\":[],\"command\":\"never-run\"}}}\n"
    );
}

#[test]
fn use_http_selection_reaches_claude_and_opencode_through_normal_sync() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude", "opencode"]);
    project.write(
        "catalog/docs/manifest.json",
        r#"{"schema_version":1,"id":"docs","title":"Docs","connection":{"type":"http","url":"https://example.invalid/mcp"},"requirements":{"binaries":[],"inputs":[]}}"#,
    );
    for slug in ["claude", "opencode"] {
        project
            .exuno()
            .args([
                "mcp",
                "use",
                "docs",
                "--tool",
                slug,
                "--library",
                "catalog",
                "--apply",
            ])
            .assert()
            .success();
    }
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".mcp.json"),
        "{\"mcpServers\":{\"docs\":{\"type\":\"http\",\"url\":\"https://example.invalid/mcp\"}}}\n"
    );
    let opencode = project.read("opencode.json");
    assert!(
        opencode
            .contains("\"docs\": {\"type\": \"remote\", \"url\": \"https://example.invalid/mcp\"}")
    );
    project.exuno().arg("check").assert().success();
    project.exuno().arg("doctor").assert().success();
    project.append(".mcp.json", "\n");
    project
        .exuno()
        .args(["adopt", "--yes", ".mcp.json"])
        .assert()
        .success();
    assert_eq!(
        project.read(".ai/src/tools/claude/mcp.json"),
        project.read(".mcp.json")
    );
}

#[test]
fn use_codex_composes_selected_http_source_with_settings() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project.write(".ai/src/tools/codex/settings.toml", "model = \"gpt-5\"\n");
    project.write(
        "catalog/docs/manifest.json",
        r#"{"schema_version":1,"id":"docs","title":"Docs","connection":{"type":"http","url":"https://example.invalid/mcp"},"requirements":{"binaries":[],"inputs":[]}}"#,
    );
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "docs",
            "--tool",
            "codex",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .success();
    assert!(!project.exists(".codex/config.toml"));
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".codex/config.toml"),
        "model = \"gpt-5\"\n\n[mcp_servers.docs]\nurl = \"https://example.invalid/mcp\"\n"
    );
    project.exuno().arg("check").assert().success();
    project.exuno().arg("doctor").assert().success();
    project.write("catalog/extra/manifest.json", &manifest("extra", "Extra"));
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "extra",
            "--tool",
            "codex",
            "--library",
            "catalog",
            "--merge",
            "--apply",
        ])
        .assert()
        .success();
    project.exuno().arg("sync").assert().success();
    assert!(
        project
            .read(".codex/config.toml")
            .contains("[mcp_servers.docs]")
    );
    assert!(
        project
            .read(".codex/config.toml")
            .contains("[mcp_servers.extra]")
    );
    project.exuno().arg("check").assert().success();
    project.append(".codex/config.toml", "\n");
    project
        .exuno()
        .args(["adopt", "--yes", ".codex/config.toml"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("multi-source"));
}

#[test]
fn codex_ownership_conflict_refuses_sync_without_changing_output() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project.exuno().arg("sync").assert().success();
    project.write(
        ".ai/src/tools/codex/settings.toml",
        "[mcp_servers.existing]\ncommand = \"manual\"\n",
    );
    project.write(
        ".ai/src/tools/codex/mcp.json",
        "{\"mcpServers\":{\"docs\":{\"url\":\"https://example.invalid/mcp\"}}}\n",
    );
    let before = project.read(".codex/config.toml");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("MCP ownership"))
        .stderr(predicate::str::contains(
            "  • Or keep them in settings: set targets.mcp.enabled: false in .ai/src/tools/codex.yaml",
        ));
    assert_eq!(project.read(".codex/config.toml"), before);
    project
        .exuno()
        .arg("doctor")
        .assert()
        .failure()
        .stdout(predicate::str::contains("Codex MCP ownership conflict"));
}

#[test]
fn codex_settings_keep_their_servers_when_mcp_composition_is_disabled() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    let settings = "[mcp_servers.repl]\ncommand = \"manual\"\nstartup_timeout_sec = 120\n";
    project.write(".ai/src/tools/codex/settings.toml", settings);
    project.write(
        ".ai/src/mcp.json",
        "{\"mcpServers\":{\"docs\":{\"url\":\"https://example.invalid/mcp\"}}}\n",
    );
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  mcp:\n    enabled: false\n",
    );
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(".codex/config.toml"), settings);
    project
        .exuno()
        .arg("doctor")
        .assert()
        .stdout(predicate::str::contains("MCP ownership conflict").not());
}

#[test]
fn codex_composes_native_server_fields_from_a_per_tool_source() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    project.write(".ai/src/tools/codex/settings.toml", "model = \"gpt\"\n");
    project.write(
        ".ai/src/tools/codex/mcp.json",
        r#"{"mcpServers":{"repl":{"command":"node_repl","args":[],"cwd":".","enabled":false,"startup_timeout_sec":120,"env":{"MODE":"1"}}}}"#,
    );
    project.exuno().arg("sync").assert().success();
    assert_eq!(
        project.read(".codex/config.toml"),
        "model = \"gpt\"\n\n[mcp_servers.repl]\ncommand = \"node_repl\"\nargs = []\ncwd = \".\"\n\
         enabled = false\nstartup_timeout_sec = 120\n\n[mcp_servers.repl.env]\n\"MODE\" = \"1\"\n"
    );
    project.exuno().arg("check").assert().success();
}

#[test]
fn use_refuses_occupied_sources_without_revealing_or_changing_them() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    for occupied in [
        ".ai/src/mcp.json",
        ".ai/src/mcp/claude.json",
        ".ai/src/tools/claude/mcp.json",
        ".ai/src/tools/claude/mcp.json.bak",
    ] {
        project.write(occupied, "SECRET_SOURCE_VALUE");
        for apply in [false, true] {
            let mut command = project.exuno();
            command.args([
                "mcp",
                "use",
                "alpha",
                "--tool",
                "claude",
                "--library",
                "catalog",
            ]);
            if apply {
                command.arg("--apply");
            }
            let output = command.output().unwrap();
            assert!(!output.status.success(), "{occupied}");
            assert!(output.stdout.is_empty());
            assert!(
                !output
                    .stderr
                    .windows(19)
                    .any(|bytes| bytes == b"SECRET_SOURCE_VALUE")
            );
        }
        assert_eq!(project.read(occupied), "SECRET_SOURCE_VALUE");
        std::fs::remove_file(project.join(occupied)).unwrap();
    }
    assert!(!project.exists(".ai/backups"));
}

#[test]
fn use_refuses_disabled_tools_invalid_selections_and_unsafe_source_roots() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    for args in [
        vec!["alpha", "--tool", "cursor"],
        vec!["alpha", "--tool", "codex"],
        vec!["alpha@missing", "--tool", "claude"],
    ] {
        project
            .exuno()
            .arg("mcp")
            .arg("use")
            .args(args)
            .args(["--library", "catalog", "--apply"])
            .assert()
            .failure()
            .stdout("");
    }
    let external = tempfile::tempdir().unwrap();
    project.write(
        ".ai/exuno.yaml",
        &format!(
            "tools:\n  enabled: [claude]\nsource:\n  tools: {}\n",
            external.path().display()
        ),
    );
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("outside the project root"));
    assert!(!project.exists(".ai/backups"));
}

#[test]
fn use_keeps_the_source_absent_when_backup_creation_fails() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project.write(".ai/backups", "private bytes");
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .failure()
        .stdout("");
    assert_eq!(project.read(".ai/backups"), "private bytes");
    assert!(!project.exists(".ai/src/tools/claude/mcp.json"));
}

// Windows does not consistently enforce chmod bits; root can bypass them.
#[cfg(unix)]
#[test]
fn use_discards_a_snapshot_when_the_new_source_cannot_be_written() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    let parent = project.join(".ai/src/tools/claude");
    std::fs::create_dir_all(&parent).unwrap();
    common::chmod(&parent, 0o500);
    let output = project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--apply",
        ])
        .output()
        .unwrap();
    common::chmod(&parent, 0o700);
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!project.exists(".ai/src/tools/claude/mcp.json"));
    project
        .exuno()
        .args(["rollback", "--list"])
        .assert()
        .success()
        .stdout("No Exuno backups found.\n");
}

// Windows symlink creation can require Developer Mode or elevated privileges.
#[cfg(unix)]
#[test]
fn use_refuses_a_per_tool_directory_link_outside_the_project() {
    use std::os::unix::fs::symlink;

    let project = Project::empty();
    let external = tempfile::tempdir().unwrap();
    project.write(".ai/exuno.yaml", "tools:\n  enabled: [claude]\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    std::fs::create_dir_all(project.join(".ai/src/tools")).unwrap();
    symlink(external.path(), project.join(".ai/src/tools/claude")).unwrap();
    project
        .exuno()
        .args([
            "mcp",
            "use",
            "alpha",
            "--tool",
            "claude",
            "--library",
            "catalog",
            "--apply",
        ])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("outside the project root"));
    assert!(!external.path().join("mcp.json").exists());
}

#[test]
fn configured_catalog_stays_inside_project_root() {
    let project = Project::empty();
    project.write(".ai/exuno.yaml", "library:\n  mcp:\n    path: catalog\n");
    project.write("catalog/alpha/manifest.json", &manifest("alpha", "First"));
    project
        .exuno()
        .args(["mcp", "list"])
        .assert()
        .success()
        .stdout("alpha\tFirst\n");

    let external = tempfile::tempdir().unwrap();
    project.write(
        ".ai/exuno.yaml",
        &format!(
            "library:\n  mcp:\n    path: {}\n",
            external.path().display()
        ),
    );
    project
        .exuno()
        .args(["mcp", "list"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("outside the project root"));
}

#[test]
fn duplicate_keys_and_oversized_or_deep_manifests_fail_without_stdout() {
    let project = Project::empty();
    let path = "catalog/alpha/manifest.json";
    let duplicate = manifest("alpha", "First").replace(
        "\"connection\":",
        "\"extensions\":{\"example.dev\":{\"key\":1,\"\\u006bey\":2}},\"connection\":",
    );
    project.write(path, &duplicate);
    project
        .exuno()
        .args(["mcp", "show", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains("duplicate JSON key: key"));

    let oversized = " ".repeat(131_073);
    project.write(path, &oversized);
    project
        .exuno()
        .args(["mcp", "validate", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("byte limit"));

    let deep = manifest("alpha", "First").replace(
        "\"connection\":",
        &format!(
            "\"extensions\":{{\"example.dev\":{}0{}}},\"connection\":",
            "[".repeat(17),
            "]".repeat(17)
        ),
    );
    project.write(path, &deep);
    project
        .exuno()
        .args(["mcp", "validate", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("depth limit"));
}

#[test]
fn v2_validates_every_variant_and_attributed_guidance() {
    let project = Project::empty();
    let v2 = r#"{"schema_version":2,"id":"docs","title":"Docs","connection":{"type":"http","url":"https://example.invalid/mcp"},"requirements":{"binaries":[],"inputs":[]},"alternatives":{"local":{"connection":{"type":"stdio","command":"never-run","args":[]},"requirements":{"binaries":[],"inputs":[]}}},"guidance":{"recommended":"local","authority":"vendor","source":"https://example.invalid/docs","checked_at":"2024-02-29","reason":"Documented local option"}}"#;
    project.write("catalog/docs/manifest.json", v2);
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .success();

    project.write(
        "catalog/docs/manifest.json",
        &v2.replace("2024-02-29", "2026-02-29"),
    );
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("guidance.checked_at"));

    project.write(
        "catalog/docs/manifest.json",
        &v2.replace(",\"args\":[]", ""),
    );
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("args"));

    project.write(
        "catalog/docs/manifest.json",
        &v2.replace("https://example.invalid/docs", "https://:"),
    );
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("guidance.source"));
}

#[test]
fn schema_errors_reject_unknown_fields_inputs_and_mismatched_ids() {
    let project = Project::empty();
    let original = manifest("alpha", "First");
    for invalid in [
        original.replace("\"id\":\"alpha\"", "\"id\":\"other\""),
        original.replace("\"inputs\":[]", "\"inputs\":[\"TOKEN\"]"),
        original.replace("\"schema_version\":1", "\"schema_version\":99"),
        original.replace("\"title\":\"First\"", "\"title\":\"First\",\"bogus\":true"),
        original.replace(
            "\"schema_version\":1",
            "\"schema_version\":1,\"alternatives\":{}",
        ),
    ] {
        project.write("catalog/alpha/manifest.json", &invalid);
        project
            .exuno()
            .args(["mcp", "validate", "alpha", "--library", "catalog"])
            .assert()
            .failure()
            .stdout("");
    }
}

#[test]
fn escaped_nul_is_data_but_raw_nul_is_invalid_json() {
    let project = Project::empty();
    let original = manifest("alpha", "First").replace("First", r"First\u0000End");
    project.write("catalog/alpha/manifest.json", &original);
    project
        .exuno()
        .args(["mcp", "show", "alpha", "--library", "catalog"])
        .assert()
        .success()
        .stdout(original);

    let raw = manifest("alpha", "First").replace("First", "First\0End");
    project.write("catalog/alpha/manifest.json", &raw);
    project
        .exuno()
        .args(["mcp", "validate", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("");
}

#[test]
fn list_escapes_control_characters_in_titles() {
    let project = Project::empty();
    let title = format!(
        r"Привет\n\t\u0085\u202e{}{}{}",
        '\u{00ad}', '\u{3164}', '\u{e0100}'
    );
    project.write("catalog/alpha/manifest.json", &manifest("alpha", &title));
    project
        .exuno()
        .args(["mcp", "list", "--library", "catalog"])
        .assert()
        .success()
        .stdout("alpha\tПривет\\n\\t\\u0085\\u202e\\u00ad\\u3164\\u{e0100}\n");
}

#[test]
fn diagnostics_escape_untrusted_json_keys() {
    let project = Project::empty();
    let path = "catalog/alpha/manifest.json";
    let original = manifest("alpha", "First");
    let key = r"\u001b[31m";
    project.write(
        path,
        &original.replace(
            "\"connection\":",
            &format!("\"{key}\":1,\"{key}\":2,\"connection\":"),
        ),
    );
    project
        .exuno()
        .args(["mcp", "validate", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stdout("")
        .stderr(predicate::str::contains(r"duplicate JSON key: \u001b[31m"));

    project.write(
        path,
        &original.replace("\"connection\":", &format!("\"{key}\":1,\"connection\":")),
    );
    let output = project
        .exuno()
        .args(["mcp", "validate", "alpha", "--library", "catalog"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!output.stderr.contains(&0x1b));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains(r"unknown manifest field: \u001b[31m")
    );
}

#[test]
fn catalog_entry_limit_and_unsafe_ids_are_rejected() {
    let project = Project::empty();
    for index in 0..257 {
        std::fs::create_dir_all(project.join(&format!("catalog/item{index}"))).unwrap();
    }
    project
        .exuno()
        .args(["mcp", "validate", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("entry limit"));
    project
        .exuno()
        .args(["mcp", "show", "../item0", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unsafe MCP library id"));
}

// Windows symlink creation can require Developer Mode or elevated privileges.
#[cfg(unix)]
#[test]
fn symlinks_cannot_escape_the_selected_catalog() {
    use std::os::unix::fs::symlink;

    let project = Project::empty();
    project.write("outside.json", &manifest("alpha", "Outside"));
    std::fs::create_dir_all(project.join("catalog/alpha")).unwrap();
    symlink(
        project.join("outside.json"),
        project.join("catalog/alpha/manifest.json"),
    )
    .unwrap();
    project
        .exuno()
        .args(["mcp", "show", "alpha", "--library", "catalog"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not a regular file"));
}
