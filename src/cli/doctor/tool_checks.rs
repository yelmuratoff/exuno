//! Checks `doctor` runs for each enabled tool: commands config, payload ownership, guard wiring.

use std::path::Path;

use super::Doctor;
use crate::cli::sorted_entries;
use crate::config::payload;
use crate::config::tool::Tool;
use crate::paths::{self, DiskText};
use crate::{
    Error,
    engine::{codex_toml, opencode_json},
};

impl Doctor<'_> {
    /// `_doctor_check_commands_config`.
    pub(super) fn check_commands_config(&mut self, tool: &Tool) -> Result<(), Error> {
        let slug = tool.slug.clone();
        let dest = tool.value("targets.commands.dest");
        let as_skills = tool.value("targets.commands.as_skills") == "true";
        let inline = tool.value("targets.commands.inline_into_agents") == "true";
        if !dest.is_empty() && as_skills {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .as_skills both set — dest wins; remove one"
            ))?;
        }
        if !dest.is_empty() && inline {
            self.warn(&format!(
                "{slug}: targets.commands.dest and .inline_into_agents both set — dest wins; remove one"
            ))?;
        }
        if as_skills && inline {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills and .inline_into_agents both true — as_skills wins; remove one"
            ))?;
        }
        if as_skills && tool.value("targets.skills.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.as_skills requires targets.skills.dest — option will no-op"
            ))?;
        }
        if inline && tool.value("targets.agents.dest").is_empty() {
            self.warn(&format!(
                "{slug}: targets.commands.inline_into_agents requires targets.agents.dest — option will no-op"
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_payload_ownership`.
    pub(super) fn check_payload_ownership(&mut self, tool: &Tool) -> Result<(), Error> {
        let mcp_synced = tool.flag("targets.mcp.enabled") != Some(false)
            && !tool.value("targets.mcp.dest").is_empty();
        if mcp_synced && tool.value("targets.mcp.format") == "codex_toml" {
            let settings = self.resolve(tool, "settings")?;
            let mcp = self.resolve(tool, "mcp")?;
            if let (Some(settings), Some(mcp)) = (settings, mcp) {
                let text = String::from_utf8_lossy(&settings.bytes()?).into_owned();
                if codex_toml::settings_claim_mcp(&text) {
                    self.fail(&format!(
                        "Codex MCP ownership conflict: {} and {} both define or may encode mcp_servers. Move the server map into one source, or set targets.mcp.enabled: false in .ai/src/tools/codex.yaml to keep it in settings.",
                        self.source_shown(&settings),
                        self.source_shown(&mcp)
                    ))?;
                }
            }
        }
        match tool.slug.as_str() {
            "opencode" if mcp_synced => {
                let settings = self.resolve(tool, "settings")?;
                let mcp = self.resolve(tool, "mcp")?;
                if let (Some(settings), Some(mcp)) = (settings, mcp) {
                    let text = String::from_utf8_lossy(&settings.bytes()?).into_owned();
                    if opencode_json::settings_has_mcp(&text) == Ok(true) {
                        self.fail(&format!(
                            "OpenCode MCP ownership conflict: {} and {} both define mcp. Move the canonical server map into one source.",
                            self.source_shown(&settings),
                            self.source_shown(&mcp)
                        ))?;
                    }
                }
            }
            "kimi" => {
                let mut hook = payload::find_new_override(self.project, "kimi", "hooks")?;
                if hook.is_none() {
                    hook = sorted_entries(&Path::new(&self.root).join(".ai/src/hooks"))
                        .into_iter()
                        .find(|path| {
                            path.is_file()
                                && path
                                    .file_name()
                                    .is_some_and(|n| n.disk_text().starts_with("kimi."))
                        });
                }
                if let Some(path) = hook {
                    let shown = self.rel(&path.disk_text());
                    self.advise(&format!(
                        "Kimi hooks are global-only in $KIMI_CODE_HOME/config.toml; Exuno leaves it untouched. Remove {shown} from project sources."
                    ))?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// `_doctor_check_guard_wired`.
    pub(super) fn check_guard_wired(&mut self, tool: &Tool) -> Result<(), Error> {
        if self.resolve(tool, "guard")?.is_none() {
            return Ok(());
        }
        let guard_dest = tool.value("targets.guard.dest");
        let settings_dest = tool.value("targets.settings.dest");
        if guard_dest.is_empty() || settings_dest.is_empty() {
            return Ok(());
        }
        let Some(settings) = self.resolve(tool, "settings")? else {
            return Ok(());
        };
        let guard_name = paths::leaf(&guard_dest);
        let bytes = settings.bytes()?;
        let referenced = bytes
            .windows(guard_name.len())
            .any(|window| window == guard_name.as_bytes());
        if !referenced {
            self.warn(&format!(
                "{}: {guard_dest} is generated but {} never references it — the guard against edits to generated files is inert. Add the hooks block from the shipped base, or delete the override to inherit it.",
                tool.display_name(),
                self.source_shown(&settings)
            ))?;
        }
        Ok(())
    }
}
