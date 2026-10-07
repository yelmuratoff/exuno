//! Project-wide checks: drift, secret scan, skills, rules, orphan outputs, and parent duplicates.

use std::path::{Path, PathBuf};

use super::Doctor;
use super::json::json_valid;
use super::secrets::scan_secrets;
use crate::cli::stale_targets::{self, Stale};
use crate::cli::{files_below, sorted_entries};
use crate::engine::{skill_tree, workspace::Workspace};
use crate::paths::{self, DiskText};
use crate::transaction::manifest::{self, Manifest};
use crate::{
    Error, config::catalog, config::template_manifest, config::yaml_subset, engine::convert,
    engine::overlay,
};

/// `_DOCTOR_OUTPUT_DIR_MAP`.
const OUTPUT_DIRS: [(&str, &str); 14] = [
    (".claude", "claude"),
    (".cursor", "cursor"),
    (".codex", "codex"),
    (".kimi-code", "kimi"),
    (".opencode", "opencode"),
    (".windsurf", "windsurf"),
    (".devin", "windsurf"),
    (".gemini", "gemini"),
    (".junie", "junie"),
    (".cline", "cline"),
    (".amazonq", "amazonq"),
    (".kiro", "kiro"),
    (".zed", "zed"),
    (".agents", "codex"),
];

impl Doctor<'_> {
    /// `_doctor_check_drift`.
    pub(super) fn check_drift(&mut self) -> Result<(), Error> {
        let style = self.style;
        let manifest_path = Path::new(&self.root).join(manifest::REL);
        if !manifest_path.is_file() {
            return self.info(&format!(
                "No .sync-manifest yet — run {} to create it",
                style.cyan("exuno sync")
            ));
        }
        let Some(manifest) = Manifest::load(&self.root)? else {
            return Ok(());
        };
        if manifest.entries().is_empty() {
            return self.info(".sync-manifest is empty");
        }
        let (mut edited, mut missing, mut clean) = (0, 0, 0);
        for entry in manifest.entries() {
            let rel = &entry.rel;
            let dest = Path::new(&self.root).join(rel);
            if !dest.is_file() {
                self.warn(&format!("{rel} — missing (deleted manually)"))?;
                missing += 1;
                continue;
            }
            let Some(current) = entry.current_hash(&self.root) else {
                self.warn(&format!("{rel} — could not hash"))?;
                continue;
            };
            if current != entry.hash {
                self.warn(&format!("{rel} — edited since last sync"))?;
                edited += 1;
            } else {
                clean += 1;
            }
        }
        if edited == 0 && missing == 0 {
            self.ok(&format!("All {clean} tracked file(s) match the manifest"))
        } else {
            self.say(&format!(
                "\n    {} {} {} {} {}\n",
                style.dim("Re-run"),
                style.cyan("exuno sync"),
                style.dim("to overwrite, or move edits into"),
                style.cyan(".ai/src/"),
                style.dim("first.")
            ))
        }
    }

    /// `_doctor_scan_one_file`; returns (secret hit, invalid JSON).
    fn scan_one_file(&mut self, file: &Path) -> Result<(bool, bool), Error> {
        let style = self.style;
        let shown = self.rel(&file.disk_text());
        let bytes = std::fs::read(file).map_err(|e| Error::io(file, e))?;
        if file.extension().is_some_and(|ext| ext == "json") && !json_valid(&bytes) {
            self.fail(&format!("{shown}: invalid JSON syntax"))?;
            return Ok((false, true));
        }
        let hits = scan_secrets(&bytes);
        if hits.is_empty() {
            return Ok((false, false));
        }
        self.fail(&format!("{shown}: possible secret"))?;
        for hit in hits {
            self.say(&format!("        {}\n", style.dim(&hit)))?;
        }
        Ok((true, false))
    }

    /// `_doctor_scan_overrides`.
    pub(super) fn scan_overrides(&mut self) -> Result<(), Error> {
        let style = self.style;
        let (mut hits, mut invalid, mut legacy) = (0, 0, 0);
        let tools_root = self.project.user_tools_dir();
        if tools_root.is_dir() {
            for tool_dir in sorted_entries(&tools_root)
                .into_iter()
                .filter(|p| p.is_dir())
            {
                for resource in ["mcp", "settings", "hooks"] {
                    for file in sorted_entries(&tool_dir).into_iter().filter(|p| {
                        p.is_file()
                            && p.file_name()
                                .is_some_and(|n| n.disk_text().starts_with(&format!("{resource}.")))
                    }) {
                        let (hit, bad) = self.scan_one_file(&file)?;
                        hits += usize::from(hit);
                        invalid += usize::from(bad);
                    }
                }
            }
        }
        for resource in ["mcp", "settings", "hooks"] {
            let dir = Path::new(&self.root).join(".ai/src").join(resource);
            if !dir.is_dir() {
                continue;
            }
            for file in sorted_entries(&dir).into_iter().filter(|p| p.is_file()) {
                legacy += 1;
                let (hit, bad) = self.scan_one_file(&file)?;
                hits += usize::from(hit);
                invalid += usize::from(bad);
            }
        }
        if legacy > 0 {
            self.warn(&format!(
                "Legacy payload layout ({legacy} file(s) under .ai/src/{{hooks,mcp,settings}}/). Run {} to move them to .ai/src/tools/<tool>/<resource>.<ext>.",
                style.cyan("exuno migrate --apply")
            ))?;
        }
        if hits == 0 && invalid == 0 && legacy == 0 {
            self.info("No overrides to scan, or all clean.")?;
        } else if hits > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{}: use ${{ENV_VAR}} placeholders; never commit raw secrets.",
                style.yellow("Reminder")
            ))?;
        }
        Ok(())
    }

    /// `_doctor_check_empty_skills`.
    pub(super) fn check_empty_skills(&mut self) -> Result<(), Error> {
        let style = self.style;
        let skills = Path::new(&self.root).join(".ai/src/skills");
        if !skills.is_dir() {
            return self.info("No .ai/src/skills/ — nothing to scan.");
        }
        let tree = skill_tree::discover(
            &Workspace::on_disk(&self.root),
            &format!("{}/.ai/src/skills", self.root),
        );
        let collisions = skill_tree::collisions(&tree.skills);
        for (name, rels) in &collisions {
            let claims: Vec<_> = rels.iter().map(|rel| format!("skills/{rel}/")).collect();
            self.warn(&format!(
                "skill name '{name}' is claimed by {} — exuno sync refuses it; rename one",
                claims.join(", ")
            ))?;
        }
        for rel in &tree.too_deep {
            self.advise(&format!(
                "skills/{rel}/ — deeper than {} categories {}",
                skill_tree::MAX_CATEGORY_DEPTH,
                style.dim("(not synced — move it up)")
            ))?;
        }
        let base_skills = self
            .config
            .as_deref()
            .is_none_or(|config| yaml_subset::value(config, "base_skills") != "false");
        let inherited = if base_skills {
            catalog::base_src_skills()
        } else {
            Vec::new()
        };
        let empty = tree.empty_categories_besides(&inherited);
        for rel in &empty {
            self.advise(&format!(
                "skills/{rel}/ — missing SKILL.md {}",
                style.dim("(empty skill — populate or remove)")
            ))?;
        }
        for skill in inherited.iter().filter_map(|name| tree.find(name)) {
            self.advise(&format!(
                "skills/{}/ — replaces the bundled skill, so engine updates stop here {}",
                skill.rel,
                style.dim("(to extend it instead, keep only your additions and drop SKILL.md)")
            ))?;
        }
        let nonstandard = tree.nonstandard_categories();
        for rel in &nonstandard {
            self.advise(&format!(
                "skills/{rel}/ — category name is not lowercase-kebab {}",
                style.dim("(exuno add --category refuses it — rename)")
            ))?;
        }
        if collisions.is_empty()
            && tree.too_deep.is_empty()
            && empty.is_empty()
            && nonstandard.is_empty()
        {
            self.ok("All skill directories contain SKILL.md")
        } else {
            Ok(())
        }
    }

    /// `_doctor_check_always_on_rules`.
    pub(super) fn check_always_on_rules(&mut self) -> Result<(), Error> {
        let style = self.style;
        let rules = Path::new(&self.root).join(".ai/src/rules");
        if !rules.is_dir() {
            return self.info("No .ai/src/rules/ — nothing to scan.");
        }
        let (mut count, mut bytes) = (0usize, 0usize);
        for file in sorted_entries(&rules)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        {
            let content = std::fs::read(&file).map_err(|e| Error::io(&file, e))?;
            if is_path_scoped(&content) {
                continue;
            }
            count += 1;
            bytes += content.len();
        }
        if count == 0 {
            self.ok("No always-on rules (every rule is paths:-scoped)")
        } else if bytes >= 20000 {
            self.advise(&format!(
                "{count} always-on rule(s) load on every task (~{} KB, ~{} tokens). Add {} frontmatter to domain rules so they load only when matching files are touched — a large always-on set dilutes attention.",
                bytes / 1024,
                bytes / 4,
                style.cyan("paths:")
            ))
        } else {
            self.ok(&format!(
                "Always-on rule context is lean ({count} file(s), ~{} KB)",
                bytes / 1024
            ))
        }
    }

    /// `_doctor_check_orphan_outputs`.
    pub(super) fn check_orphan_outputs(&mut self) -> Result<(), Error> {
        let style = self.style;
        let enabled = self.project.enabled_tools()?;
        let mut found = 0;
        if Path::new(&self.root).join(".agent").is_dir() {
            self.advise(&format!(
                ".agent/ — legacy pre-v0.6 layout (run {} to preview cleanup)",
                style.cyan("exuno migrate --legacy")
            ))?;
            found += 1;
        }
        for (dir, tool) in OUTPUT_DIRS {
            if !Path::new(&self.root).join(dir).is_dir() {
                continue;
            }
            if dir == ".agents" && (enabled.contains("codex") || enabled.contains("antigravity")) {
                continue;
            }
            if !enabled.contains(tool) {
                self.advise(&format!(
                    "{dir}/ — orphan (tool '{tool}' not enabled; output left from prior run)"
                ))?;
                found += 1;
            }
        }
        let recorded: Vec<String> = Manifest::load(&self.root)?
            .map(|manifest| manifest.paths().into_iter().collect())
            .unwrap_or_default();
        for stale in stale_targets::left_by_disabled_targets(self.project, &recorded)? {
            let Stale { rel, slug, key } = stale;
            self.advise(&format!(
                "{rel} — left from {slug} targets.{key}, which is disabled (delete it, or set targets.{key}.enabled back to true)"
            ))?;
            found += 1;
        }
        if found == 0 {
            self.ok("No orphan tool-output directories")?;
        }
        Ok(())
    }

    /// `_doctor_check_cross_project`.
    pub(super) fn check_cross_project(&mut self) -> Result<(), Error> {
        let style = self.style;
        let child_src = format!("{}/.ai/src", self.root);
        if !Path::new(&child_src).is_dir() {
            return self.info("No .ai/src/ in this project — skipping cross-project scan.");
        }
        let mut from_shared = false;
        let parent_src = match self
            .config
            .as_deref()
            .and_then(|config| overlay::shared_parent_src(config, &self.root))
        {
            Some(parent) => {
                from_shared = true;
                Some(parent)
            }
            None => paths::find_parent_ai_src(&self.root),
        };
        let Some(parent_src) = parent_src else {
            return self.info("No parent .ai/src/ found within git boundary.");
        };
        let origin_hint = if from_shared {
            format!(" {}", style.dim("(from shared.path)"))
        } else {
            String::new()
        };
        self.info(&format!(
            "Parent source: {}{origin_hint}",
            style.dim(&parent_src)
        ))?;
        self.say("\n")?;

        let scan = CrossScan {
            inherited: self
                .config
                .as_deref()
                .map(|config| {
                    overlay::inherit_categories(&yaml_subset::value(config, "shared.inherit"))
                })
                .unwrap_or_default(),
            parent_root: paths::parent(&parent_src),
        };
        let child_skills = skill_tree::discover(
            &Workspace::on_disk(&self.root),
            &format!("{child_src}/skills"),
        );
        let (mut dupes, mut divergent) = (0, 0);
        for (rel, parent) in parent_files(&parent_src) {
            let rel = match rel.strip_prefix("skills/") {
                Some(inside) => format!("skills/{}", child_skills.locate(inside)),
                None => rel,
            };
            let child = Path::new(&child_src).join(&rel);
            let shared = SharedFile { rel, child, parent };
            match self.report_shared(&scan, &shared)? {
                Some(true) => dupes += 1,
                Some(false) => divergent += 1,
                None => {}
            }
        }
        if dupes == 0 && divergent == 0 {
            self.ok("No source files shared with parent.")
        } else if dupes > 0 {
            self.say("\n")?;
            self.info(&format!(
                "{} {} {}",
                style.dim("Run"),
                style.cyan("exuno dedupe"),
                style.dim("to remove duplicates interactively.")
            ))
        } else {
            Ok(())
        }
    }

    /// Reports a parent file the child also has: `Some(true)` for an identical
    /// copy, `Some(false)` for a divergent one, `None` when either is unreadable
    /// or the child has none.
    fn report_shared(
        &mut self,
        scan: &CrossScan,
        shared: &SharedFile,
    ) -> Result<Option<bool>, Error> {
        let style = self.style;
        let SharedFile { rel, child, parent } = shared;
        if !child.is_file() {
            return Ok(None);
        }
        let (Some(child_hash), Some(parent_hash)) = (
            template_manifest::hash(child),
            template_manifest::hash(parent),
        ) else {
            return Ok(None);
        };
        if child_hash == parent_hash {
            let category = rel.split('/').next().unwrap_or("");
            let hint = if scan.inherited.contains(&category) {
                format!(" {}", style.dim("(inherited via shared: — safe to delete)"))
            } else {
                String::new()
            };
            let parent_text = parent.disk_text();
            let shown = parent_text
                .strip_prefix(&format!("{}/", scan.parent_root))
                .unwrap_or(&parent_text);
            self.advise(&format!(
                "{rel} — duplicate of parent's {}{hint}",
                style.dim(shown)
            ))?;
            return Ok(Some(true));
        }
        let content = std::fs::read(parent).map_err(|e| Error::io(parent, e))?;
        if convert::read_field(&content, "category") == b"governance" {
            self.advise(&format!(
                "{rel} — {} {}",
                style.yellow("governance file diverges from parent"),
                style.dim("(category: governance — likely a mistake, not an override)")
            ))?;
        } else {
            self.info(&format!(
                "{rel} — diverges from parent {}",
                style.dim("(review intent)")
            ))?;
        }
        Ok(Some(false))
    }
}

/// What the cross-project scan knows about the parent.
struct CrossScan {
    inherited: Vec<&'static str>,
    parent_root: String,
}

/// A parent source file and where the child would keep its copy.
struct SharedFile {
    rel: String,
    child: PathBuf,
    parent: PathBuf,
}

/// The parent's `rules`, `commands`, and `agents` Markdown files and every
/// file below its `skills`, each with its path below the parent's `src`.
fn parent_files(parent_src: &str) -> Vec<(String, PathBuf)> {
    let mut pairs = Vec::new();
    for category in ["rules", "commands", "agents"] {
        let dir = Path::new(parent_src).join(category);
        if !dir.is_dir() {
            continue;
        }
        for file in sorted_entries(&dir)
            .into_iter()
            .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "md"))
        {
            let name = file.file_name().unwrap_or_default().disk_text();
            pairs.push((format!("{category}/{name}"), file));
        }
    }
    let skills = Path::new(parent_src).join("skills");
    if skills.is_dir() {
        let mut files = Vec::new();
        files_below(&skills, &mut files);
        files.retain(|p| {
            !p.file_name()
                .is_some_and(|n| n.disk_text().starts_with('.'))
        });
        files.sort();
        for file in files {
            let rel = file
                .strip_prefix(parent_src)
                .map(|p| p.disk_text())
                .unwrap_or_default();
            pairs.push((rel, file));
        }
    }
    pairs
}

/// `sed -n '2,/^---$/p' | grep -q '^paths:[[:space:]]*$'` after a first
/// line of `---`. sed tests the closing address from line 3 on, so a `---`
/// on line 2 does not end the range.
fn is_path_scoped(content: &[u8]) -> bool {
    let mut lines = content.split(|b| *b == b'\n');
    if lines.next() != Some(b"---") {
        return false;
    }
    for (index, line) in lines.enumerate() {
        if line.strip_prefix(b"paths:").is_some_and(|rest| {
            rest.iter()
                .all(|b| matches!(b, b' ' | b'\t' | b'\r' | 0x0b | 0x0c))
        }) {
            return true;
        }
        if index > 0 && line == b"---" {
            return false;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_paths_key_is_found_like_the_sed_range_does() {
        assert!(is_path_scoped(
            b"---\npaths:\n  - \"**/*.ts\"\n---\n# Scoped\n"
        ));
        assert!(is_path_scoped(b"---\n---\npaths:\n"));
        assert!(is_path_scoped(b"---\npaths:  \n---\n"));
        assert!(!is_path_scoped(b"# Rule\n"));
        assert!(!is_path_scoped(b"---\ndesc: x\n---\npaths:\n"));
        assert!(!is_path_scoped(b"---\npaths: foo\n---\n"));
        assert!(!is_path_scoped(b"---"));
        assert!(!is_path_scoped(b"paths:\n"));
    }
}
