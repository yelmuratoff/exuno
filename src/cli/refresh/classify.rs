//! The three-way classification of each template against the manifest, the
//! project file, and the `agent_sync.yaml` overrides.

use std::path::{Path, PathBuf};

use crate::config::project_config::{self, Selection};
use crate::config::template_manifest::{self, TemplateManifest};
use crate::config::yaml_subset;
use crate::engine::skill_tree::{self, Tree};
use crate::engine::workspace::Workspace;
use crate::transaction::manifest::sha256_hex;

/// One `<rel>|<template>|<hash>` entry of the `*_FILES` arrays, and where its
/// project copy lives.
pub(super) struct Candidate {
    pub(super) rel: String,
    pub(super) bytes: &'static [u8],
    pub(super) hash: String,
    pub(super) dest: PathBuf,
}

/// Where a template's project copy lives: at the template's own path, except
/// inside a skill the project keeps in a category, which the copy follows.
pub(super) struct Locator {
    base: PathBuf,
    skills: Tree,
}

impl Locator {
    pub(super) fn new(base: &str) -> Self {
        Self {
            base: PathBuf::from(base),
            skills: skill_tree::discover(&Workspace::on_disk(base), &format!("{base}/skills")),
        }
    }

    pub(super) fn path(&self, rel: &str) -> PathBuf {
        match rel.strip_prefix("skills/") {
            Some(inside) => self.base.join("skills").join(self.skills.locate(inside)),
            None => self.base.join(rel),
        }
    }
}

#[derive(Default)]
pub(super) struct Changes {
    pub(super) new: Vec<Candidate>,
    pub(super) conflicts: Vec<Candidate>,
    pub(super) auto: Vec<Candidate>,
    pub(super) deleted: Vec<Candidate>,
    pub(super) unchanged: usize,
    pub(super) silently_kept: usize,
}

impl Changes {
    /// Whether the run has anything to add, update, or restore.
    pub(super) fn offers_anything(&self, visible_deleted: bool) -> bool {
        !self.new.is_empty()
            || !self.conflicts.is_empty()
            || !self.auto.is_empty()
            || visible_deleted
    }
}

/// `template_overrides.declined` and `.pinned` from `agent_sync.yaml`.
#[derive(Default)]
pub(super) struct Overrides {
    pub(super) declined: Vec<String>,
    pub(super) pinned: Vec<String>,
}

pub(super) struct Classifier<'a> {
    pub(super) locator: &'a Locator,
    pub(super) manifest: &'a TemplateManifest,
    pub(super) overrides: &'a Overrides,
    pub(super) review: bool,
}

impl Classifier<'_> {
    /// `_refresh_collect_changes`: `AGENTS.md` when asked, then the `*.md`
    /// files of `rules`, `commands`, and `agents`, then every file below `skills`.
    pub(super) fn collect(
        &self,
        templates: &[(String, &'static [u8])],
        categories: &[String],
        include_agents_md: bool,
    ) -> Changes {
        let mut changes = Changes::default();
        let in_scope = |category: &str| categories.iter().any(|c| c == category);
        let in_category = |rel: &str, category: &str| {
            if category == "skills" {
                rel.starts_with("skills/")
            } else {
                rel.rsplit_once('/').is_some_and(|(dir, _)| dir == category)
            }
        };
        if include_agents_md {
            for (rel, bytes) in templates.iter().filter(|(rel, _)| rel == "AGENTS.md") {
                self.classify(&mut changes, rel, bytes);
            }
        }
        for category in ["rules", "commands", "agents", "skills"] {
            if !in_scope(category) {
                continue;
            }
            for (rel, bytes) in templates
                .iter()
                .filter(|(rel, _)| in_category(rel, category))
            {
                self.classify(&mut changes, rel, bytes);
            }
        }
        changes
    }

    /// `_refresh_classify`.
    fn classify(&self, changes: &mut Changes, rel: &str, bytes: &'static [u8]) {
        if self.overrides.declined.iter().any(|item| item == rel) {
            return;
        }
        let t_new = sha256_hex(bytes);
        let t_old = self.manifest.lookup(rel);
        let dest = self.locator.path(rel);
        let candidate = || Candidate {
            rel: rel.to_string(),
            bytes,
            hash: t_new.clone(),
            dest: dest.clone(),
        };
        if !dest.is_file() {
            if t_old.is_some() {
                changes.deleted.push(candidate());
            } else {
                changes.new.push(candidate());
            }
            return;
        }
        let Some(u_cur) = template_manifest::hash(&dest) else {
            return;
        };
        if u_cur == t_new {
            changes.unchanged += 1;
            return;
        }
        if self.overrides.pinned.iter().any(|item| item == rel) {
            return;
        }
        let Some(t_old) = t_old else {
            changes.conflicts.push(candidate());
            return;
        };
        if u_cur == t_old {
            changes.auto.push(candidate());
            return;
        }
        if t_old == t_new {
            changes.silently_kept += 1;
            if self.review {
                changes.conflicts.push(candidate());
            }
            return;
        }
        changes.conflicts.push(candidate());
    }
}

/// `_refresh_load_overrides`: `template_overrides.declined` and `.pinned` from
/// the project config `explicit` or the default search selects; `Err` names a
/// missing explicit config.
pub(super) fn load_overrides(root: &str, explicit: Option<&str>) -> Result<Overrides, String> {
    let is_file = |path: &str| Path::new(path).is_file();
    let config = match project_config::select(root, explicit, &is_file) {
        Selection::Found(path) => Some(path),
        Selection::None => None,
        Selection::Missing(path) => return Err(project_config::missing_message(&path)),
    };
    let text = config
        .and_then(|path| std::fs::read(path).ok())
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default();
    let list = |key: &str| -> Vec<String> {
        yaml_subset::list(&text, key)
            .into_iter()
            .filter(|item| !item.is_empty())
            .collect()
    };
    Ok(Overrides {
        declined: list("template_overrides.declined"),
        pinned: list("template_overrides.pinned"),
    })
}

#[cfg(all(test, unix))]
mod tests {
    use std::path::Path;

    use crate::cli::refresh::tests::{
        NOT_A_TTY, append, call, drop_entry, header, seeded, set_entry,
    };
    use crate::config::catalog;
    use crate::config::template_manifest::{self, TemplateManifest};
    use crate::transaction::manifest::sha256_hex;

    #[test]
    fn an_auto_update_off_a_terminal_waits_for_yes() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        set_entry(
            &root,
            "rules/core.md",
            &template_manifest::hash(&base.join("rules/core.md")).unwrap(),
        );
        let refused = call(&root, &[], false, &[]);
        assert_eq!((refused.status, refused.err.as_str()), (1, NOT_A_TTY));
        assert!(
            std::fs::read_to_string(base.join("rules/core.md"))
                .unwrap()
                .ends_with("USER LOCAL EDIT\n")
        );
    }

    #[test]
    fn new_deleted_auto_update_and_conflict_files_classify_and_apply_like_bash() {
        let (_dir, root) = seeded();
        let base = Path::new(&root).join(".ai/src");
        std::fs::remove_file(base.join("rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        set_entry(
            &root,
            "rules/core.md",
            &template_manifest::hash(&base.join("rules/core.md")).unwrap(),
        );
        append(&root, "rules/git.md", "EDIT\n");
        drop_entry(&root, "rules/git.md");
        std::fs::remove_file(base.join("commands/review.md")).unwrap();

        let plan = "  Summary:\n    + 1 new template(s)\n    ↑ 1 auto-update(s) — you hadn't touched them locally\n    ~ 1 conflict(s) — your version differs from the template\n    ? 1 previously declined — pass --include-deleted to revisit\n    · 14 unchanged\n\n  New:\n    + rules/comments.md\n\n  Auto-update: (your version matches the previous template; safe to update)\n    ↑ rules/core.md\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  Previously declined:\n    ? commands/review.md\n\n";
        let head = header(&root, "rules,skills,commands,agents");

        let dry = call(&root, &["--dry-run", "--include-deleted"], false, &[]);
        assert_eq!(
            (dry.status, dry.out),
            (0, format!("{head}{plan}  Dry run — no files written.\n\n"))
        );
        assert!(!base.join("rules/comments.md").exists());

        let blocked = call(&root, &["--include-deleted"], false, &[]);
        assert_eq!(
            (blocked.status, blocked.out, blocked.err.as_str()),
            (1, format!("{head}{plan}"), NOT_A_TTY)
        );

        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Local       (.template-manifest — deleted from disk; --include-deleted to restore):\n    · commands/review.md\n\n"
        );

        let applied = call(
            &root,
            &["--yes", "--include-deleted", "--review"],
            false,
            &[],
        );
        assert_eq!(
            (applied.status, applied.out),
            (
                0,
                format!(
                    "{head}{plan}  ↑ rules/core.md  (auto-updated; you hadn't touched it)\n  ? commands/review.md (previously declined — skipped under --yes; run interactively)\n  + rules/comments.md\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 1 · Auto-updated: 1 · Updated: 0 · Skipped: 2 · Unchanged: 14\n\n  Next: exuno sync to distribute the updates to enabled tools.\n\n"
                )
            )
        );
        let template = |rel: &str| {
            catalog::template_files()
                .into_iter()
                .find(|(path, _)| path == rel)
                .map(|(_, bytes)| bytes.to_vec())
                .unwrap()
        };
        assert_eq!(
            std::fs::read(base.join("rules/comments.md")).unwrap(),
            template("rules/comments.md")
        );
        assert_eq!(
            std::fs::read(base.join("rules/core.md")).unwrap(),
            template("rules/core.md")
        );
        assert!(
            std::fs::read_to_string(base.join("rules/git.md"))
                .unwrap()
                .ends_with("EDIT\n")
        );
        assert!(!base.join("commands/review.md").exists());
        let manifest = TemplateManifest::load(Path::new(&root)).unwrap();
        assert_eq!(
            manifest.lookup("rules/comments.md"),
            Some(sha256_hex(&template("rules/comments.md")).as_str())
        );
        assert_eq!(
            manifest.lookup("rules/core.md"),
            Some(sha256_hex(&template("rules/core.md")).as_str())
        );
        assert_eq!(manifest.lookup("rules/git.md"), None);
        assert!(manifest.lookup("commands/review.md").is_some());

        let again = call(&root, &["--yes"], false, &[]);
        assert_eq!(
            again.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/git.md\n\n  ~ rules/git.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n"
            )
        );
    }

    #[test]
    fn silently_kept_edits_and_deleted_files_show_in_the_up_to_date_summary() {
        let (_dir, root) = seeded();
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        std::fs::remove_file(Path::new(&root).join(".ai/src/commands/review.md")).unwrap();
        let head = header(&root, "rules,skills,commands,agents");
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{head}  Already up to date! 16 file(s) match the current templates.\n  Locally declined (.template-manifest):    1 file(s); --include-deleted to revisit.\n  Pass --status for the full list.\n  1 file(s) differ from the shipped template (local edits or earlier skips); pass --review to revisit.\n\n"
            )
        );
        let review = call(&root, &["--review", "--dry-run"], false, &[]);
        assert_eq!(
            review.out,
            format!(
                "{head}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ rules/core.md\n\n  Dry run — no files written.\n\n"
            )
        );
        let review = call(&root, &["--review"], false, &[]);
        assert_eq!((review.status, review.err.as_str()), (1, NOT_A_TTY));
        assert!(
            std::fs::read_to_string(Path::new(&root).join(".ai/src/rules/core.md"))
                .unwrap()
                .ends_with("USER LOCAL EDIT\n")
        );

        append(&root, "AGENTS.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "AGENTS.md");
        let agents = call(&root, &["--yes", "--include-agents-md"], false, &[]);
        assert_eq!(
            agents.out,
            format!(
                "{}  Summary:\n    ~ 1 conflict(s) — your version differs from the template\n    · 1 silently kept (local edits or earlier skips) — pass --review to revisit\n    · 16 unchanged\n\n  Conflicts: (both your version and the template diverged from the recorded baseline)\n    ~ AGENTS.md\n\n  ~ AGENTS.md (conflict — skipped; run interactively to review)\n\n  Done. Added: 0 · Auto-updated: 0 · Updated: 0 · Skipped: 1 · Unchanged: 16\n\n",
                header(&root, "rules,skills,commands,agents,AGENTS.md")
            )
        );
    }

    #[test]
    fn declined_and_pinned_overrides_silence_templates_and_status_lists_them() {
        let (_dir, root) = seeded();
        let config = Path::new(&root).join(".ai/agent_sync.yaml");
        std::fs::write(
            &config,
            "tools:\n  enabled: []\n\ntemplate_overrides:\n  declined:\n    - rules/comments.md\n    - rules/git.md\n  pinned:\n    - rules/core.md\n",
        )
        .unwrap();
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/comments.md")).unwrap();
        drop_entry(&root, "rules/comments.md");
        append(&root, "rules/core.md", "USER LOCAL EDIT\n");
        drop_entry(&root, "rules/core.md");
        assert_eq!(
            call(&root, &["--status"], false, &[]).out,
            "\n  Declined templates\n  Persistent  (template_overrides.declined in agent_sync.yaml — never offered):\n    · rules/comments.md\n    · rules/git.md\n\n"
        );
        assert_eq!(
            call(&root, &["--yes"], false, &[]).out,
            format!(
                "{}  Already up to date! 15 file(s) match the current templates.\n  Persistently declined (agent_sync.yaml): 2 file(s).\n  Pass --status for the full list.\n\n",
                header(&root, "rules,skills,commands,agents")
            )
        );
        assert!(!Path::new(&root).join(".ai/src/rules/comments.md").exists());
        assert_eq!(
            TemplateManifest::load(Path::new(&root))
                .unwrap()
                .lookup("rules/core.md"),
            None
        );
        // A declined template that was recorded and then removed is not a local decline.
        std::fs::remove_file(Path::new(&root).join(".ai/src/rules/git.md")).unwrap();
        assert!(
            !call(&root, &["--yes"], false, &[])
                .out
                .contains("Locally declined")
        );
    }
}
