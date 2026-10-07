mod common;

use common::Project;
use predicates::prelude::*;

const CONFIG: &str = ".codex/config.toml";
const SETTINGS: &str = ".ai/src/tools/codex/settings.toml";
const APP_STATE: &str = "\n[projects.\"/tmp/p\"]\ntrust_level = \"trusted\"\n\n[mcp_servers.repl]\ncommand = \"node_repl\"\nstartup_timeout_sec = 120\n";

fn codex_project(ownership: Option<&str>) -> Project {
    let project = Project::seeded(&[]);
    project.enable_tools(&["codex"]);
    if let Some(mode) = ownership {
        project.write(
            ".ai/src/tools/codex.yaml",
            &format!("targets:\n  settings:\n    ownership: {mode}\n"),
        );
    }
    project.write(SETTINGS, "model = \"gpt\"\n\n[tui]\ntheme = \"dark\"\n");
    project.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers":{"dart":{"command":"dart","args":["mcp-server"]}}}"#,
    );
    project
}

fn sync(project: &Project) -> assert_cmd::assert::Assert {
    project.exuno().args(["sync", "--only", "codex"]).assert()
}

#[test]
fn app_written_keys_survive_sync_and_check() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    assert_eq!(
        project.read(CONFIG),
        "model = \"gpt\"\n\n[tui]\ntheme = \"dark\"\n\n[mcp_servers.dart]\ncommand = \"dart\"\nargs = [\"mcp-server\"]\n"
    );
    project.append(CONFIG, APP_STATE);
    let with_app_state = project.read(CONFIG);

    project.exuno().arg("check").assert().success();
    sync(&project).success();
    assert_eq!(project.read(CONFIG), with_app_state);
    let manifest = project.read(".ai/.sync-manifest");
    let line = manifest
        .lines()
        .find(|line| line.starts_with(".codex/config.toml\t"))
        .unwrap();
    assert_eq!(line.split('\t').count(), 3);
}

#[test]
fn a_changed_owned_key_stops_sync_and_names_it() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    let edited = project.read(CONFIG).replace("\"gpt\"", "\"ui-pick\"");
    project.write(CONFIG, &edited);

    sync(&project)
        .failure()
        .stderr(predicate::str::contains(".codex/config.toml (model)"));
    assert_eq!(project.read(CONFIG), edited);
    project
        .exuno()
        .arg("check")
        .assert()
        .failure()
        .stdout(predicate::str::contains(".codex/config.toml"));

    project
        .exuno()
        .args(["sync", "--only", "codex", "--force"])
        .assert()
        .success();
    let forced = project.read(CONFIG);
    assert!(forced.contains("model = \"gpt\""));
    assert!(forced.contains("[projects.\"/tmp/p\"]"));
    assert!(forced.contains("[mcp_servers.repl]"));
}

#[test]
fn keys_dropped_from_the_sources_leave_the_live_file() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    project.write(SETTINGS, "model = \"gpt\"\n");
    project.write(".ai/src/mcp.json", r#"{"mcpServers":{}}"#);

    sync(&project).success();
    assert_eq!(
        project.read(CONFIG),
        "model = \"gpt\"\n\n[projects.\"/tmp/p\"]\ntrust_level = \"trusted\"\n\n[mcp_servers.repl]\ncommand = \"node_repl\"\nstartup_timeout_sec = 120\n"
    );
}

#[test]
fn a_first_keyed_sync_stops_on_a_value_it_never_owned() {
    let project = codex_project(Some("keys"));
    project.write(CONFIG, "model = \"ui-pick\"\n");
    sync(&project)
        .failure()
        .stderr(predicate::str::contains(
            "differs from .ai/src in 1 key sync has not owned before",
        ))
        .stderr(predicate::str::contains("      model\n"));
    assert_eq!(project.read(CONFIG), "model = \"ui-pick\"\n");
    project
        .exuno()
        .args(["sync", "--only", "codex", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("A real sync would stop"));

    project
        .exuno()
        .args(["sync", "--only", "codex", "--force"])
        .assert()
        .success();
    assert!(project.read(CONFIG).starts_with("model = \"gpt\"\n"));
}

#[test]
fn switching_to_keys_keeps_what_the_app_wrote_after_a_whole_file_sync() {
    let project = codex_project(None);
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    let with_app_state = project.read(CONFIG);

    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: keys\n",
    );
    sync(&project).success();
    assert_eq!(project.read(CONFIG), with_app_state);
}

#[test]
fn auto_owns_keys_when_the_project_root_is_home() {
    let project = codex_project(None);
    let home = project.path().to_str().unwrap().to_string();
    let run = |args: &[&str]| project.exuno().env("HOME", &home).args(args).assert();
    run(&["sync", "--only", "codex"]).success();
    project.append(CONFIG, APP_STATE);
    run(&["sync", "--only", "codex"]).success();
    assert!(project.read(CONFIG).contains("[projects.\"/tmp/p\"]"));
}

#[test]
fn auto_owns_the_whole_file_in_a_repository() {
    let project = codex_project(None);
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    sync(&project)
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
}

#[test]
fn a_disabled_settings_target_still_merges_mcp_keys_only() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: keys\n    enabled: false\n",
    );
    sync(&project).success();
    assert_eq!(
        project.read(CONFIG),
        "[mcp_servers.dart]\ncommand = \"dart\"\nargs = [\"mcp-server\"]\n\n[projects.\"/tmp/p\"]\ntrust_level = \"trusted\"\n\n[mcp_servers.repl]\ncommand = \"node_repl\"\nstartup_timeout_sec = 120\n"
    );
    project.append(CONFIG, "\n[notice]\nhide = true\n");
    sync(&project).success();
    assert!(project.read(CONFIG).contains("[notice]"));
}

#[test]
fn switching_from_keys_to_file_needs_force() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    let with_app_state = project.read(CONFIG);
    project.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    ownership: file\n",
    );
    project
        .exuno()
        .args(["sync", "--only", "codex", "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains("A real sync would stop"));
    sync(&project)
        .failure()
        .stderr(predicate::str::contains(
            ".codex/config.toml is owned by key; owning the whole file drops what OpenAI Codex wrote there",
        ))
        .stderr(predicate::str::contains("exuno sync --force"));
    assert_eq!(project.read(CONFIG), with_app_state);
    project
        .exuno()
        .args(["sync", "--only", "codex", "--force"])
        .assert()
        .success();
    assert!(!project.read(CONFIG).contains("[projects."));
}

#[test]
fn an_unknown_ownership_value_stops_sync() {
    let project = codex_project(Some("key"));
    sync(&project).failure().stderr(predicate::str::contains(
        "Unknown targets.settings.ownership for OpenAI Codex: key (expected auto, keys, or file)",
    ));
}

#[test]
fn disabling_codex_leaves_a_key_owned_config_in_place() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    let before = project.read(CONFIG);
    project
        .exuno()
        .args(["disable", "codex"])
        .assert()
        .success();
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "Kept .codex/config.toml (the app writes to it too)",
        ));
    assert_eq!(project.read(CONFIG), before);
}

#[test]
fn rollback_restores_the_whole_config() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.append(CONFIG, APP_STATE);
    let before = project.read(CONFIG);
    project.write(SETTINGS, "model = \"other\"\n");
    sync(&project).success();
    assert_ne!(project.read(CONFIG), before);

    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .success();
    assert_eq!(project.read(CONFIG), before);
}

#[test]
fn live_toml_that_does_not_parse_stops_sync_before_writing() {
    let project = codex_project(Some("keys"));
    sync(&project).success();
    project.write(CONFIG, "model = \n");
    project
        .exuno()
        .args(["sync", "--only", "codex", "--force"])
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "Cannot merge into .codex/config.toml",
        ));
    assert_eq!(project.read(CONFIG), "model = \n");
}

/// A project synced as a config home: every command runs with `HOME` at
/// the project root, as a global `~/.ai` does.
struct Home(Project);

impl Home {
    fn new(tools: &[&str]) -> Self {
        let home = Self(Project::seeded(&[]));
        home.0.enable_tools(tools);
        home
    }

    fn run(&self, args: &[&str]) -> assert_cmd::assert::Assert {
        let home = self.0.path().to_str().unwrap().to_string();
        self.0.exuno().env("HOME", home).args(args).assert()
    }
}

const CLAUDE_SETTINGS: &str = ".claude/settings.json";

#[test]
fn claude_settings_in_a_home_keep_what_claude_code_writes() {
    let home = Home::new(&["claude"]);
    home.0.write(
        ".ai/src/tools/claude/settings.json",
        "{\n  \"theme\": \"dark\",\n  \"enabledPlugins\": {\"a@m\": true}\n}\n",
    );
    home.run(&["sync", "--only", "claude"]).success();
    assert_eq!(
        home.0.read(CLAUDE_SETTINGS),
        "{\n  \"theme\": \"dark\",\n  \"enabledPlugins\": {\"a@m\": true}\n}\n"
    );
    home.0.write(
        CLAUDE_SETTINGS,
        "{\n  \"enabledPlugins\": {\"a@m\": true, \"b@m\": true},\n  \"feedbackSurveyState\": {\"last\": 1},\n  \"theme\": \"dark\"\n}\n",
    );
    let with_app_state = home.0.read(CLAUDE_SETTINGS);

    home.run(&["check"]).success();
    home.run(&["sync", "--only", "claude"]).success();
    assert_eq!(home.0.read(CLAUDE_SETTINGS), with_app_state);

    home.0.write(
        CLAUDE_SETTINGS,
        &with_app_state.replace("\"dark\"", "\"light\""),
    );
    home.run(&["sync", "--only", "claude"])
        .failure()
        .stderr(predicate::str::contains(".claude/settings.json (theme)"));
    home.run(&["adopt", "--yes", CLAUDE_SETTINGS]).success();
    assert_eq!(
        home.0.read(".ai/src/tools/claude/settings.json"),
        "{\n  \"enabledPlugins\": {\n    \"a@m\": true\n  },\n  \"theme\": \"light\"\n}\n"
    );
    home.run(&["sync", "--only", "claude"]).success();
    assert!(home.0.read(CLAUDE_SETTINGS).contains("feedbackSurveyState"));
}

#[test]
fn a_server_added_in_the_editor_survives_a_home_sync() {
    let home = Home::new(&["cursor"]);
    home.0.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers": {"dart": {"command": "dart"}}}"#,
    );
    home.run(&["sync", "--only", "cursor"]).success();
    home.0.write(
        ".cursor/mcp.json",
        r#"{"mcpServers": {"dart": {"command": "dart"}, "ui-added": {"url": "https://x.test"}}}"#,
    );
    home.run(&["sync", "--only", "cursor"]).success();
    assert!(home.0.read(".cursor/mcp.json").contains("ui-added"));

    home.0.write(".ai/src/mcp.json", r#"{"mcpServers": {}}"#);
    home.run(&["sync", "--only", "cursor"]).success();
    assert_eq!(
        home.0.read(".cursor/mcp.json"),
        "{\n  \"mcpServers\": {\n    \"ui-added\": {\n      \"url\": \"https://x.test\"\n    }\n  }\n}\n"
    );
}

#[test]
fn a_composed_opencode_config_keeps_keys_opencode_writes() {
    let home = Home::new(&["opencode"]);
    home.0.write(
        ".ai/src/tools/opencode/settings.json",
        r#"{"$schema": "https://opencode.ai/config.json", "theme": "dark"}"#,
    );
    home.0.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers": {"dart": {"command": "dart"}}}"#,
    );
    home.run(&["sync", "--only", "opencode"]).success();
    let composed = home.0.read("opencode.json");
    assert!(composed.contains("\"dart\""), "{composed}");
    let with_app_key = composed.replacen('{', "{\n  \"autoupdate\": false,", 1);
    home.0.write("opencode.json", &with_app_key);
    home.run(&["sync", "--only", "opencode"]).success();
    assert_eq!(home.0.read("opencode.json"), with_app_key);
}

#[test]
fn a_key_owned_codex_config_with_no_source_is_not_created() {
    let home = Home::new(&["codex"]);
    home.0.write(
        ".ai/src/tools/codex.yaml",
        "targets:\n  settings:\n    enabled: false\n",
    );
    home.run(&["sync", "--only", "codex"]).success();
    assert!(!home.0.exists(".codex/config.toml"));
}

#[test]
fn tools_sharing_an_mcp_file_must_own_it_the_same_way() {
    let home = Home::new(&["claude", "minimax"]);
    home.0.write(".ai/src/mcp.json", r#"{"mcpServers": {}}"#);
    home.0.write(
        ".ai/src/tools/minimax.yaml",
        "targets:\n  mcp:\n    ownership: file\n",
    );
    home.run(&["sync"])
        .failure()
        .stderr(predicate::str::contains(
            "one owns it by key and the other whole; give both the same targets.mcp.ownership",
        ));
}

#[test]
fn a_disabled_tool_keeps_its_file_without_blocking_later_syncs() {
    let home = Home::new(&["claude", "cursor"]);
    home.0.write(
        ".ai/src/tools/claude/settings.json",
        "{\"theme\": \"dark\"}\n",
    );
    home.run(&["sync"]).success();
    home.run(&["disable", "claude"]).success();
    home.0.write(CLAUDE_SETTINGS, "{\"theme\": \"light\"}\n");
    home.run(&["sync"]).success();
    home.run(&["sync"]).success();
    assert_eq!(home.0.read(CLAUDE_SETTINGS), "{\"theme\": \"light\"}\n");
}

#[test]
fn adopt_sends_a_changed_mcp_server_back_to_its_source() {
    let home = Home::new(&["cursor"]);
    home.0.write(
        ".ai/src/mcp.json",
        r#"{"mcpServers": {"dart": {"command": "dart"}}}"#,
    );
    home.run(&["sync", "--only", "cursor"]).success();
    home.0.write(
        ".cursor/mcp.json",
        r#"{"mcpServers": {"dart": {"command": "dart2"}}}"#,
    );
    home.run(&["adopt", "--yes", ".cursor/mcp.json"])
        .failure()
        .stderr(predicate::str::contains(
            "mcpServers.dart comes from the MCP source",
        ));
}

#[test]
fn gemini_settings_in_a_home_keep_what_gemini_writes() {
    let home = Home::new(&["gemini"]);
    home.0.write(
        ".ai/src/tools/gemini/settings.json",
        "{\"theme\": \"Dracula\"}\n",
    );
    home.run(&["sync", "--only", "gemini"]).success();
    home.0.write(
        ".gemini/settings.json",
        "{\"theme\": \"Dracula\", \"security\": {\"auth\": {\"selectedType\": \"oauth-personal\"}}}\n",
    );
    let with_app_key = home.0.read(".gemini/settings.json");
    home.run(&["sync", "--only", "gemini"]).success();
    assert_eq!(home.0.read(".gemini/settings.json"), with_app_key);
}

#[test]
fn a_profile_variant_in_a_repository_owns_its_config_home_by_key() {
    let project = Project::seeded(&[]);
    project.enable_tools(&["claude"]);
    project
        .exuno()
        .args(["profile", "add", "hub", "--tools", "claude"])
        .assert()
        .success();
    project.write(
        ".ai/src/tools/claude/settings.json",
        "{\"theme\": \"dark\"}\n",
    );
    project.exuno().arg("sync").assert().success();
    let hub = ".claude-hub/settings.json";
    let with_app_key =
        project
            .read(hub)
            .replacen('{', "{\n  \"feedbackSurveyState\": {\"last\": 1},", 1);
    project.write(hub, &with_app_key);
    project.exuno().arg("sync").assert().success();
    assert_eq!(project.read(hub), with_app_key);
    project.write(".claude/settings.json", "{\"theme\": \"light\"}\n");
    project
        .exuno()
        .arg("sync")
        .assert()
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
}

#[test]
fn an_unknown_mcp_ownership_value_stops_sync() {
    let home = Home::new(&["cursor"]);
    home.0.write(
        ".ai/src/tools/cursor.yaml",
        "targets:\n  mcp:\n    ownership: key\n",
    );
    home.run(&["sync", "--only", "cursor"])
        .failure()
        .stderr(predicate::str::contains(
            "Unknown targets.mcp.ownership for Cursor: key (expected auto, keys, or file)",
        ));
}

#[test]
fn servers_left_in_key_owned_codex_settings_point_at_the_app_owned_way_out() {
    let project = codex_project(Some("keys"));
    project.write(SETTINGS, "[mcp_servers.repl]\ncommand = \"repl\"\n");
    sync(&project).failure().stderr(predicate::str::contains(
        "Servers the Codex app manages need no source",
    ));
}

#[test]
fn zed_settings_stay_owned_whole_in_a_home() {
    let home = Home::new(&["zed"]);
    home.run(&["sync", "--only", "zed"]).success();
    let manifest = home.0.read(".ai/.sync-manifest");
    let line = manifest
        .lines()
        .find(|line| line.starts_with(".zed/settings.json\t"))
        .unwrap();
    assert_eq!(line.split('\t').count(), 2, "{line}");
    home.0.append(".zed/settings.json", "// app note\n");
    home.run(&["sync", "--only", "zed"])
        .failure()
        .stderr(predicate::str::contains("Manual edits detected"));
}
