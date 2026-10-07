//! `exuno upgrade-config`: `cmd_upgrade_config` of `lib/helpers/init.sh`,
//! which pins `agentsync_version` to the running engine.

use crate::paths::DiskText;
use crate::project::Project;
use std::io::Write;

use super::put;
use crate::output::help::{Help, Section};
use crate::output::style::Style;
use crate::{Error, config::names, engine::staging};

const KEY: &str = "agentsync_version";

pub const HELP: Help = Help {
    command: "upgrade-config",
    tagline: "re-pin agentsync_version to the running engine",
    synopsis: &["upgrade-config"],
    description: &[
        "Re-pins agentsync_version in agent_sync.yaml to the running engine, after\nan exuno update. Re-sync and commit the outputs afterwards.",
    ],
    sections: &[Section {
        title: "OPTIONS",
        entries: &[("-h, --help", "Show this help")],
    }],
    examples: &["upgrade-config"],
};

/// The `awk` insertion or the `sed` rewrite, as `cmd_upgrade_config` picks it.
pub fn upgrade_text(text: &str, version: &str) -> (String, bool) {
    let pins = |key: &str, line: &str| line.starts_with(&format!("{key}:"));
    let present = names::VERSION_KEYS
        .into_iter()
        .find(|key| text.split('\n').any(|line| pins(key, line)));
    if let Some(key) = present {
        let pin = format!("{key}: \"{version}\"");
        let rewritten: Vec<String> = text
            .split('\n')
            .map(|line| {
                if pins(key, line) {
                    pin.clone()
                } else {
                    line.to_string()
                }
            })
            .collect();
        return (rewritten.join("\n"), false);
    }
    let pin = format!("{KEY}: \"{version}\"");
    let mut lines: Vec<&str> = text.split('\n').collect();
    if lines.last() == Some(&"") {
        lines.pop();
    }
    let is_space = |c: char| matches!(c, ' ' | '\t' | '\x0b' | '\x0c' | '\r');
    let mut out = String::new();
    let mut inserted = false;
    for line in lines {
        let stripped = line.trim_start_matches(is_space);
        if !inserted && !(stripped.is_empty() || stripped.starts_with('#')) {
            out.push_str(&format!("{pin}\n\n"));
            inserted = true;
        }
        out.push_str(line);
        out.push('\n');
    }
    if !inserted {
        out.push_str(&format!("{pin}\n"));
    }
    (out, true)
}

pub fn run(
    args: &[String],
    discover: &dyn Fn() -> Result<Project, Error>,
    version: &str,
    style: &Style,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> Result<u8, Error> {
    match args.first().map(String::as_str) {
        None => {}
        Some("--help" | "-h") => {
            put(out, HELP.render(style).as_bytes())?;
            return Ok(0);
        }
        Some(other) => {
            put(
                err,
                format!(
                    "{}: Unknown argument: {other}\n{}",
                    style.red("Error"),
                    HELP.render(style)
                )
                .as_bytes(),
            )?;
            return Ok(2);
        }
    }
    let project = discover()?;
    let Some(config) = project.config_path else {
        put(
            err,
            format!(
                "{}: No agent_sync.yaml found in {}\nRun {} first.\n",
                style.red("Error"),
                project.root.disk_text(),
                style.cyan("exuno init")
            )
            .as_bytes(),
        )?;
        return Ok(1);
    };
    let bytes = std::fs::read(&config).map_err(|e| Error::io(&config, e))?;
    let (text, added) = upgrade_text(&String::from_utf8_lossy(&bytes), version);
    staging::write_beside(&config, text.as_bytes())?;
    let shown = style.dim(&config.disk_text());
    let line = if added {
        format!(
            "{}: agentsync_version: {version} → {shown}\n",
            style.green("Added")
        )
    } else {
        format!(
            "{}: agentsync_version → {version} {shown}\n",
            style.green("Updated")
        )
    };
    put(out, line.as_bytes())?;
    Ok(0)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_and_an_unknown_argument_leave_the_config_untouched() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join(".ai").join("agent_sync.yaml");
        std::fs::create_dir_all(config.parent().unwrap()).unwrap();
        std::fs::write(&config, "agentsync_version: \"0.1.0\"\n").unwrap();
        let style = Style::plain();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let discover = || Project::at(dir.path());
        let args = ["--help".to_string()];
        let status = run(&args, &discover, "9.9.9", &style, &mut out, &mut err).unwrap();
        assert_eq!(status, 0);
        let help = "\n  exuno upgrade-config — re-pin agentsync_version to the running engine\n\n  USAGE\n    exuno upgrade-config\n\n  DESCRIPTION\n    Re-pins agentsync_version in agent_sync.yaml to the running engine, after\n    an exuno update. Re-sync and commit the outputs afterwards.\n\n  OPTIONS\n    -h, --help   Show this help\n\n  EXAMPLES\n    exuno upgrade-config\n\n";
        assert_eq!(String::from_utf8(out).unwrap(), help);
        let args = ["--bogus".to_string()];
        let status = run(&args, &discover, "9.9.9", &style, &mut Vec::new(), &mut err).unwrap();
        assert_eq!(status, 2);
        assert_eq!(
            String::from_utf8(err).unwrap(),
            format!("Error: Unknown argument: --bogus\n{help}")
        );
        assert_eq!(
            std::fs::read_to_string(&config).unwrap(),
            "agentsync_version: \"0.1.0\"\n"
        );
    }

    #[test]
    fn an_exuno_pin_is_rewritten_in_place_and_the_legacy_key_left_alone() {
        assert_eq!(
            upgrade_text("exuno_version: \"0.1\"\n", "9.9.9"),
            ("exuno_version: \"9.9.9\"\n".to_string(), false)
        );
        assert_eq!(
            upgrade_text(
                "agentsync_version: \"0.1\"\nexuno_version: \"0.2\"\n",
                "9.9.9"
            ),
            (
                "agentsync_version: \"0.1\"\nexuno_version: \"9.9.9\"\n".to_string(),
                false
            )
        );
    }

    #[test]
    fn the_pin_is_inserted_after_leading_comments_or_every_line_is_rewritten() {
        let cases: [(&str, &str, bool); 5] = [
            (
                "# Exuno — Project Configuration\ntools:\n  enabled:\n    - claude\n\n",
                "# Exuno — Project Configuration\nagentsync_version: \"9.9.9\"\n\ntools:\n  enabled:\n    - claude\n\n",
                true,
            ),
            (
                "# head\n\n# more\nformat: 2\nagentsync_version: \"0.1\"\nagentsync_version: \"0.2\"\n",
                "# head\n\n# more\nformat: 2\nagentsync_version: \"9.9.9\"\nagentsync_version: \"9.9.9\"\n",
                false,
            ),
            (
                "# only comments\n\n",
                "# only comments\n\nagentsync_version: \"9.9.9\"\n",
                true,
            ),
            ("", "agentsync_version: \"9.9.9\"\n", true),
            (
                "agentsync_version: 1",
                "agentsync_version: \"9.9.9\"",
                false,
            ),
        ];
        for (text, expected, added) in cases {
            assert_eq!(
                upgrade_text(text, "9.9.9"),
                (expected.to_string(), added),
                "{text:?}"
            );
        }
    }
}
