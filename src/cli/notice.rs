//! `check_for_updates` of `lib/helpers/update.sh`: the project-format notice
//! and the update banner printed on a terminal before the commands
//! `wants_notice` lists, and the background refresh of the banner's cache.

use std::path::Path;
use std::process::{Command, Stdio};

use crate::config::{format_rev, names};
use crate::output::style::Style;

/// The GitHub repository releases come from.
pub const REPO: &str = "yelmuratoff/exuno";

/// The cache file below the install root, one tag per line.
pub const CACHE_FILE: &str = ".update_cache";

const NOTICE_COMMANDS: [&str; 26] = [
    "sync",
    "init",
    "rollback",
    "check",
    "list",
    "ls",
    "setup-hooks",
    "export",
    "import",
    "refresh",
    "enable",
    "disable",
    "add",
    "adopt",
    "customize",
    "simplify",
    "migrate",
    "show",
    "diff",
    "resolve",
    "doctor",
    "dedupe",
    "profile",
    "help",
    "--help",
    "-h",
];

/// The commands `main` runs `check_for_updates` for; an empty word is `help`.
pub fn wants_notice(command: &str) -> bool {
    let command = if command.is_empty() { "help" } else { command };
    NOTICE_COMMANDS.contains(&command)
}

/// `_check_project_format`: the notice when the project is behind the engine's
/// format revision, nothing otherwise. Commands are named as `program`, the
/// name the binary was run as (`names::invoked_as`).
pub fn format_notice(project_dir: &Path, program: &str, style: &Style) -> String {
    let Some(config) = format_rev::config_path(project_dir) else {
        return String::new();
    };
    let text = std::fs::read(&config)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let engine = format_rev::engine();
    let current = format_rev::project(&text);
    if current >= engine {
        return String::new();
    }
    let mut out = format!(
        "\n  {} {}\n",
        style.yellow("This project's agent config is a migration behind"),
        style.dim(&format!("(format r{current} → r{engine})"))
    );
    for note in format_rev::pending_notes(current, engine) {
        out.push_str(&format!("    {}\n", style.dim(&note)));
    }
    out.push_str(&format!(
        "  Preview it with {}, apply with {}\n",
        style.cyan(&format!("{program} migrate")),
        style.cyan(&format!("{program} migrate --apply"))
    ));
    if program == names::LEGACY_NAME {
        out.push_str(&format!(
            "  The command is {} now; rerun the installer to add it ({program} keeps working until 1.0):\n    {}\n",
            names::NAME,
            style.cyan(&installer_command())
        ));
    }
    out.push('\n');
    out
}

fn installer_command() -> String {
    if cfg!(windows) {
        format!("irm https://github.com/{REPO}/releases/latest/download/exuno-installer.ps1 | iex")
    } else {
        format!("curl -fsSL https://raw.githubusercontent.com/{REPO}/main/install.sh | bash")
    }
}

/// `read -r latest_tag < "$cache_file"`: the first line, IFS blanks trimmed.
fn cached_tag(cache: &str) -> &str {
    cache
        .split('\n')
        .next()
        .unwrap_or("")
        .trim_matches([' ', '\t'])
}

/// The banner when the cache names a version newer than `version`, naming
/// the update command as `program`.
pub fn update_banner(cache: &str, version: &str, program: &str, style: &Style) -> String {
    let latest = cached_tag(cache);
    if latest.is_empty()
        || latest == version
        || crate::output::changelog::version_cmp(version, latest) != std::cmp::Ordering::Less
    {
        return String::new();
    }
    format!(
        "\n  ╭──────────────────────────────────────────────────────╮\n  │  {}: {} → {}              \n  │  Run: {}                                \n  ╰──────────────────────────────────────────────────────╯\n\n",
        style.yellow("Update available"),
        style.dim(&format!("v{version}")),
        style.green(&format!("v{latest}")),
        style.cyan(&format!("{program} update"))
    )
}

/// The GitHub API answer the background fetch reads.
pub fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

/// The `tag_name` of a release JSON, a leading `v` dropped as the Bash `sed`
/// dropped it.
pub fn parse_tag_name(json: &str) -> Option<String> {
    let after = &json[json.find("\"tag_name\"")? + "\"tag_name\"".len()..];
    let after = after.trim_start_matches([' ', '\t', '\n', '\r']);
    let after = after.strip_prefix(':')?;
    let after = after.trim_start_matches([' ', '\t', '\n', '\r']);
    let after = after.strip_prefix('"')?;
    let value = &after[..after.find('"')?];
    let value = value.strip_prefix('v').unwrap_or(value);
    (!value.is_empty()).then(|| value.to_string())
}

/// `_bg_fetch_latest_version`, run by `__update-cache`: `curl -sfL --max-time 5`
/// on the latest release, the tag written to the cache; every failure is silent.
pub fn refresh_cache(cache_file: &Path) {
    let Ok(output) = Command::new("curl")
        .args(["-sfL", "--max-time", "5", &latest_release_url()])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    if !output.status.success() {
        return;
    }
    if let Some(tag) = parse_tag_name(&String::from_utf8_lossy(&output.stdout)) {
        let _ = std::fs::write(cache_file, format!("{tag}\n"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_runs_for_the_dispatcher_list_and_an_empty_word() {
        for command in ["sync", "help", "--help", "-h", "", "ls", "profile"] {
            assert!(wants_notice(command), "{command:?}");
        }
        for command in [
            "update",
            "release",
            "version",
            "generate",
            "shell-init",
            "upgrade-config",
            "nope",
        ] {
            assert!(!wants_notice(command), "{command:?}");
        }
    }

    #[test]
    fn the_format_notice_lists_the_pending_migrations() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(format_notice(dir.path(), "exuno", &Style::plain()), "");
        std::fs::create_dir(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(".ai/agent_sync.yaml"),
            "tools:\n  enabled: []\n",
        )
        .unwrap();
        assert_eq!(
            format_notice(dir.path(), "exuno", &Style::plain()),
            "\n  This project's agent config is a migration behind (format r1 → r3)\n    r2  The exuno skill is engine-owned now. A copy under .ai/src/skills/exuno/ (or the older skills/agentsync/) keeps engine upgrades from your agents.\n    r3  AgentSync is Exuno now: .ai/exuno.yaml, exuno_version, metadata.exuno-*, skills/exuno/, exuno-check.yml.\n  Preview it with exuno migrate, apply with exuno migrate --apply\n\n"
        );
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "format: 3\n").unwrap();
        assert_eq!(format_notice(dir.path(), "exuno", &Style::plain()), "");
    }

    #[test]
    fn the_format_notice_names_the_command_an_agentsync_binary_was_run_as() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".ai")).unwrap();
        std::fs::write(dir.path().join(".ai/agent_sync.yaml"), "format: 2\n").unwrap();
        let notice = format_notice(dir.path(), "agentsync", &Style::plain());
        assert!(
            notice.ends_with(&format!(
                "  Preview it with agentsync migrate, apply with agentsync migrate --apply\n  The command is exuno now; rerun the installer to add it (agentsync keeps working until 1.0):\n    {}\n\n",
                installer_command()
            )),
            "{notice}"
        );
    }

    #[test]
    fn the_installer_line_runs_in_the_platform_shell() {
        let expected = if cfg!(windows) {
            "irm https://github.com/yelmuratoff/exuno/releases/latest/download/exuno-installer.ps1 | iex"
        } else {
            "curl -fsSL https://raw.githubusercontent.com/yelmuratoff/exuno/main/install.sh | bash"
        };
        assert_eq!(installer_command(), expected);
    }

    #[test]
    fn the_banner_names_the_command_it_was_run_as() {
        assert!(
            update_banner("0.46.0\n", "0.45.0", "agentsync", &Style::plain())
                .contains("Run: agentsync update")
        );
    }

    #[test]
    fn the_banner_shows_only_a_newer_cached_tag() {
        let style = Style::plain();
        assert_eq!(update_banner("", "0.36.0", "exuno", &style), "");
        assert_eq!(update_banner("0.36.0\n", "0.36.0", "exuno", &style), "");
        assert_eq!(update_banner("0.35.2\n", "0.36.0", "exuno", &style), "");
        assert_eq!(
            update_banner("  0.37.0\nignored\n", "0.36.0", "exuno", &style),
            "\n  ╭──────────────────────────────────────────────────────╮\n  │  Update available: v0.36.0 → v0.37.0              \n  │  Run: exuno update                                \n  ╰──────────────────────────────────────────────────────╯\n\n"
        );
        assert!(
            update_banner("0.37.0\n", "0.36.0", "exuno", &Style::colored()).contains(
                "\x1b[33mUpdate available\x1b[0m: \x1b[2mv0.36.0\x1b[0m → \x1b[32mv0.37.0\x1b[0m"
            )
        );
    }

    #[test]
    fn the_tag_name_is_read_from_the_release_json() {
        assert_eq!(
            parse_tag_name(
                "{\"url\":\"x\",\"name\":\"Release 0.37.0\",\"tag_name\":\"0.37.0\",\"assets\":[{\"name\":\"a\"}]}"
            ),
            Some("0.37.0".to_string())
        );
        assert_eq!(
            parse_tag_name("{\n  \"tag_name\" : \"v1.2.3\"\n}"),
            Some("1.2.3".to_string())
        );
        assert_eq!(parse_tag_name("{\"message\":\"Not Found\"}"), None);
        assert_eq!(parse_tag_name("{\"tag_name\":\"\"}"), None);
        assert_eq!(
            latest_release_url(),
            "https://api.github.com/repos/yelmuratoff/exuno/releases/latest"
        );
    }
}
