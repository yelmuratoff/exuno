//! State one render shares across its steps: the workspace, path rules, the
//! log, the run's `--dry-run` and `--force`, and the manifest's record of what
//! this run wrote (`manifest.sh`).

use std::collections::{BTreeMap, BTreeSet};

use crate::engine::keyed::Owned;
use crate::engine::workspace::Workspace;
use crate::output::log::Log;
use crate::paths::Paths;
use crate::transaction::interrupt::Interrupt;

pub struct Session {
    pub ws: Workspace,
    pub paths: Paths,
    /// `TOOL_RESOLVER_USER_DIR`: `.ai/src/tools` until `source.tools` moves it.
    pub tools_dir: String,
    pub log: Log,
    pub dry_run: bool,
    pub force: bool,
    pub interrupt: Option<Interrupt>,
    manifest: Option<BTreeSet<String>>,
    preserved: usize,
    touched: BTreeSet<String>,
    owned_before: BTreeMap<String, Owned>,
    owned_after: BTreeMap<String, Owned>,
    legacy_payload_warned: bool,
}

impl Session {
    pub fn new(ws: Workspace, paths: Paths) -> Self {
        Self {
            ws,
            tools_dir: format!("{}/.ai/src/tools", paths.root),
            paths,
            log: Log::default(),
            dry_run: false,
            force: false,
            interrupt: None,
            manifest: None,
            preserved: 0,
            touched: BTreeSet::new(),
            owned_before: BTreeMap::new(),
            owned_after: BTreeMap::new(),
            legacy_payload_warned: false,
        }
    }

    pub fn display(&self, path: &str) -> String {
        self.paths.display(path)
    }

    /// The exit status of a trapped signal that arrived, once one has.
    pub fn interrupted(&self) -> Option<u8> {
        self.interrupt
            .as_ref()?
            .received()
            .map(crate::transaction::interrupt::status)
    }

    /// `SYNC_MANIFEST_ACTIVE="true"` with `MANIFEST_KEYS` loaded: from here on
    /// a sweep keeps what the previous sync did not generate.
    pub fn activate_manifest(&mut self, paths: BTreeSet<String>) {
        self.manifest = Some(paths);
    }

    /// `sync_may_prune`: outside a manifest-aware run, or under `--force`,
    /// every extraneous entry may go; otherwise a file the manifest records, or
    /// a directory it records a file below.
    pub fn may_prune(&self, abs: &str) -> bool {
        let Some(manifest) = &self.manifest else {
            return true;
        };
        if self.force {
            return true;
        }
        self.paths.to_repo_relative(abs).is_none_or(|rel| {
            let below = format!("{rel}/");
            manifest.contains(&rel)
                || (self.ws.is_dir(abs)
                    && manifest
                        .range(below.clone()..)
                        .next()
                        .is_some_and(|key| key.starts_with(&below)))
        })
    }

    /// Whether the previous run's manifest records the file `abs`; false
    /// without a manifest, so nothing unrecorded ever counts as generated.
    pub fn recorded(&self, abs: &str) -> bool {
        let Some(manifest) = &self.manifest else {
            return false;
        };
        self.paths
            .to_repo_relative(abs)
            .is_some_and(|rel| manifest.contains(&rel))
    }

    /// `sync_note_preserved`.
    pub fn note_preserved(&mut self, shown: &str) {
        if self.dry_run {
            self.log.warning(&format!(
                "Would keep {shown} (not from .ai/src/; --force to prune)"
            ));
        } else {
            self.log.warning(&format!(
                "Kept {shown} (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)"
            ));
        }
        self.preserved += 1;
    }

    pub fn preserved(&self) -> usize {
        self.preserved
    }

    /// The owned-key records the previous sync left, by repo-relative path.
    pub fn set_owned_before(&mut self, records: BTreeMap<String, Owned>) {
        self.owned_before = records;
    }

    pub fn owned_before(&self, abs: &str) -> Option<&Owned> {
        self.owned_before.get(&self.paths.to_repo_relative(abs)?)
    }

    /// The keys this run owns in `abs`, for the manifest's third column.
    pub fn record_owned(&mut self, abs: &str, owned: Owned) {
        if let Some(rel) = self.paths.to_repo_relative(abs) {
            self.owned_after.insert(rel, owned);
        }
    }

    pub fn owned_after(&self) -> &BTreeMap<String, Owned> {
        &self.owned_after
    }

    /// `manifest_record_write`: paths outside the root are ignored silently.
    pub fn record_write(&mut self, abs: &str) {
        if let Some(rel) = self.paths.to_repo_relative(abs) {
            self.touched.insert(rel);
        }
    }

    /// `manifest_record_tree`.
    pub fn record_tree(&mut self, dir: &str) {
        for file in self.ws.files_under(dir) {
            self.record_write(&file);
        }
    }

    /// `manifest_was_touched`.
    pub fn was_touched(&self, abs: &str) -> bool {
        self.paths
            .to_repo_relative(abs)
            .is_some_and(|rel| self.touched.contains(&rel))
    }

    pub fn touched(&self) -> &BTreeSet<String> {
        &self.touched
    }

    /// `_warn_legacy_payload_path`: once per run, on stderr.
    pub fn warn_legacy_payload(&mut self, abs: &str) {
        if self.legacy_payload_warned {
            return;
        }
        self.legacy_payload_warned = true;
        let root_prefix = format!("{}/", self.paths.root);
        let rel = abs.strip_prefix(&root_prefix).unwrap_or(abs).to_string();
        self.log
            .err(format!("!  Legacy payload override layout detected: {rel}"));
        self.log
            .err("   Move to .ai/src/tools/<tool>/<resource>.<ext> (canonical since 0.11).".into());
        let migrate = self.log.command("exuno migrate --legacy");
        self.log.err(format!("   Migrate with: {migrate}"));
    }
}

#[cfg(test)]
pub(crate) fn test_session() -> Session {
    Session::new(Workspace::new("/proj"), Paths::new("/proj", "/proj", None))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_are_recorded_relative_to_the_root_and_outside_paths_are_ignored() {
        let mut s = test_session();
        s.record_write("/proj/CLAUDE.md");
        s.record_write("/elsewhere/x");
        assert!(s.was_touched("/proj/CLAUDE.md"));
        assert_eq!(s.touched().len(), 1);
    }

    #[test]
    fn the_legacy_warning_prints_once() {
        let mut s = test_session();
        s.warn_legacy_payload("/proj/.ai/src/mcp/claude.json");
        s.warn_legacy_payload("/proj/.ai/src/mcp/cursor.json");
        assert_eq!(s.log.lines().len(), 3);
        assert_eq!(
            s.log.tail(3)[0],
            "!  Legacy payload override layout detected: .ai/src/mcp/claude.json"
        );
    }

    #[test]
    fn only_manifest_paths_may_be_pruned_once_the_manifest_is_active_unless_forced() {
        let mut s = test_session();
        s.ws.create_dir_all("/proj/.claude/skills/old").unwrap();
        s.ws.create_dir_all("/proj/.claude/skills/mine").unwrap();
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
        s.activate_manifest(BTreeSet::from([
            ".claude/rules/old.md".to_string(),
            ".claude/skills/old/SKILL.md".to_string(),
        ]));
        assert!(s.may_prune("/proj/.claude/rules/old.md"));
        assert!(!s.may_prune("/proj/.claude/rules/mine.md"));
        assert!(s.may_prune("/proj/.claude/skills/old"));
        assert!(!s.may_prune("/proj/.claude/skills/mine"));
        assert!(!s.may_prune("/proj/.claude/skills/ol"));
        assert!(s.may_prune("/elsewhere/mine.md"));
        s.force = true;
        assert!(s.may_prune("/proj/.claude/rules/mine.md"));
    }

    #[test]
    fn a_preserved_entry_is_counted_and_worded_for_the_run() {
        let mut s = test_session();
        s.note_preserved(".claude/rules/mine.md");
        s.dry_run = true;
        s.note_preserved(".claude/rules/other.md");
        assert_eq!(s.preserved(), 2);
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] Kept .claude/rules/mine.md (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "[WARNING] Would keep .claude/rules/other.md (not from .ai/src/; --force to prune)"
            ]
        );
    }
}
