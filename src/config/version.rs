//! `lib/helpers/version.sh`: the `version_pin` policy and its messages.

use crate::config::yaml_subset;

/// `version_pin.mode`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Warn,
    Strict,
}

/// `version_pin_mode`: `version_pin.mode`, else the scalar `version_pin`, with
/// every `"` removed; no value is `warn`. An unknown value is the `Err`.
pub fn mode(config: &str) -> Result<Mode, String> {
    let mut value = yaml_subset::value(config, "version_pin.mode").replace('"', "");
    if value.is_empty() {
        value = yaml_subset::value(config, "version_pin").replace('"', "");
    }
    match value.as_str() {
        "" | "warn" => Ok(Mode::Warn),
        "strict" => Ok(Mode::Strict),
        _ => Err(value),
    }
}

/// `version_pin_mismatch_error` for outputs that are `committed` or not.
pub fn mismatch_error(pinned: &str, engine: &str, committed: bool) -> String {
    if committed {
        format!(
            "This project pins exuno {pinned} but you are running {engine} — committed outputs must come from one version everywhere."
        )
    } else {
        format!(
            "This project pins exuno {pinned} but you are running {engine} — version_pin.mode 'strict' requires local outputs to use the pinned version."
        )
    }
}

/// `version_pin_mismatch_hint`; `command` styles each command to run the way
/// the caller's output layer styles one.
pub fn hint(pinned: &str, engine: &str, command: impl Fn(&str) -> String) -> [String; 2] {
    [
        format!(
            "  • Match the pin:  {}",
            command(&format!("exuno update {pinned}"))
        ),
        format!(
            "  • Or move it:     {}   (re-pins to {engine}; re-sync and commit the outputs)",
            command("exuno upgrade-config")
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_nested_mode_and_the_scalar_shorthand_both_read() {
        assert_eq!(mode("version_pin:\n  mode: strict\n"), Ok(Mode::Strict));
        assert_eq!(mode("version_pin: strict\n"), Ok(Mode::Strict));
        assert_eq!(
            mode("version_pin:\n  mode: \"strict\" # keep\n"),
            Ok(Mode::Strict)
        );
    }

    #[test]
    fn no_value_is_warn() {
        assert_eq!(mode("agentsync_version: \"1\"\n"), Ok(Mode::Warn));
        assert_eq!(mode("version_pin:\n  mode:\n"), Ok(Mode::Warn));
    }

    #[test]
    fn a_later_mapping_is_not_hidden_by_an_earlier_scalar() {
        assert_eq!(
            mode("version_pin: warn\nversion_pin:\n  mode: strict\n"),
            Ok(Mode::Strict)
        );
    }

    #[test]
    fn an_unknown_value_is_returned_as_written() {
        assert_eq!(
            mode("version_pin:\n  mode: STRICT\n"),
            Err("STRICT".to_string())
        );
    }

    #[test]
    fn the_messages_are_the_bash_sentences() {
        assert_eq!(
            mismatch_error("0.1.0", "0.36.0", false),
            "This project pins exuno 0.1.0 but you are running 0.36.0 — version_pin.mode 'strict' requires local outputs to use the pinned version."
        );
        assert_eq!(
            mismatch_error("0.1.0", "0.36.0", true),
            "This project pins exuno 0.1.0 but you are running 0.36.0 — committed outputs must come from one version everywhere."
        );
        assert_eq!(
            hint("0.1.0", "0.36.0", str::to_string),
            [
                "  • Match the pin:  exuno update 0.1.0".to_string(),
                "  • Or move it:     exuno upgrade-config   (re-pins to 0.36.0; re-sync and commit the outputs)".to_string(),
            ]
        );
    }

    #[test]
    fn the_hint_styles_only_the_commands() {
        assert_eq!(
            hint("0.1.0", "0.36.0", |cmd| format!("<{cmd}>")),
            [
                "  • Match the pin:  <exuno update 0.1.0>".to_string(),
                "  • Or move it:     <exuno upgrade-config>   (re-pins to 0.36.0; re-sync and commit the outputs)".to_string(),
            ]
        );
    }
}
