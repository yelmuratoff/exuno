//! `lib/helpers/file_ops.sh`: copies and sweeps that honour `--dry-run`, and
//! prune an extraneous entry only when `Session::may_prune` allows it.

use crate::engine::filters::Filter;
use crate::engine::session::Session;
use crate::engine::skill_tree::{self, Skill};
use crate::{Error, paths};

/// `copy_file`: a missing source is a warning, not a failure.
pub fn copy_file(s: &mut Session, src: &str, dest: &str) -> Result<(), Error> {
    copy_file_noted(s, src, dest, "")
}

/// `copy_file` whose log line ends with `(note)` when `note` is not empty,
/// for a source the reader cannot tell apart by path alone.
pub fn copy_file_noted(s: &mut Session, src: &str, dest: &str, note: &str) -> Result<(), Error> {
    if !s.ws.is_file(src) {
        s.log.warning(&format!("Source file not found: {src}"));
        return Ok(());
    }
    let src_disp = s.display(src);
    let dest_disp = s.display(dest);
    let note = if note.is_empty() {
        String::new()
    } else {
        format!(" ({note})")
    };
    if s.dry_run {
        s.log
            .step(&format!("{src_disp} → {dest_disp}{note} (dry-run)"));
        return Ok(());
    }
    s.ws.create_dir_all(&paths::parent(dest))?;
    if s.ws.is_dir(dest) {
        s.ws.copy(src, &format!("{dest}/{}", paths::leaf(src)))?;
    } else {
        refuse_copy_onto_itself(s, src, dest)?;
        s.ws.remove(dest)?;
        s.ws.copy(src, dest)?;
    }
    s.record_write(dest);
    s.log.step(&format!("{src_disp} → {dest_disp}{note}"));
    Ok(())
}

/// `sync_dir` for skills: copy every filtered skill flat to `dest/<name>` and
/// every root file as it is, then prune entries the filter owns that the
/// source no longer has and this run did not write.
pub fn sync_skills_dir(
    s: &mut Session,
    src: &str,
    dest: &str,
    filter: &Filter,
) -> Result<(), Error> {
    if !s.ws.is_dir(src) {
        s.log.warning(&format!("Source directory not found: {src}"));
        return Ok(());
    }
    let src_disp = s.display(src);
    let dest_disp = s.display(dest);
    if !s.dry_run {
        s.ws.create_dir_all(dest)?;
    }

    let tree = skill_tree::discover(&s.ws, src);
    let entries = tree
        .skills
        .iter()
        .cloned()
        .chain(tree.files.iter().map(|file| Skill::at(file)));
    let mut source_items: Vec<String> = Vec::new();
    for entry in entries {
        if !filter.accepts_skill(&entry) {
            continue;
        }
        source_items.push(entry.name.clone());
        if s.dry_run {
            continue;
        }
        let source = format!("{src}/{}", entry.rel);
        let target = format!("{dest}/{}", entry.name);
        replace_skill(s, &source, &target)?;
    }

    let mut cleaned = 0usize;
    for name in s.ws.glob(dest) {
        let entry = tree
            .find(&name)
            .cloned()
            .unwrap_or_else(|| Skill::at(&name));
        if source_items.contains(&name) || !filter.accepts_skill(&entry) {
            continue;
        }
        let item = format!("{dest}/{name}");
        if s.was_touched(&item) {
            continue;
        }
        if !s.may_prune(&item) {
            s.note_preserved(&format!("{dest_disp}/{name}"));
            continue;
        }
        if s.dry_run {
            s.log
                .step(&format!("Would remove: {dest_disp}/{name} (extraneous)"));
        } else if remove_obsolete(s, &item)? {
            s.log.step(&format!("Removed: {dest_disp}/{name}"));
        } else {
            s.log.step(&format!(
                "Left {dest_disp}/{name} in place: it holds files sync did not write"
            ));
            continue;
        }
        cleaned += 1;
    }

    let extra = if filter.include.is_empty() {
        String::new()
    } else {
        format!(", include='{}'", filter.include)
    };
    let suffix = if s.dry_run { " (dry-run)" } else { "" };
    let counts = counts(source_items.len(), cleaned);
    s.log.step(&format!(
        "{src_disp}/ → {dest_disp}/ {counts}{extra}{suffix}"
    ));
    Ok(())
}

/// Puts the skill at `source` at `target`. An existing folder is updated in
/// place: the recorded files the source no longer has go, and files nothing
/// recorded (caches, `.DS_Store`, a virtualenv) stay. Each copied file
/// replaces the file at its path; a file where the source has a folder, or a
/// folder where it has a file, goes only when sync wrote it or under
/// `--force`. Only the files copied from `source` are recorded.
fn replace_skill(s: &mut Session, source: &str, target: &str) -> Result<(), Error> {
    if !s.ws.is_dir(source) {
        return replace_file(s, source, target);
    }
    if s.ws.exists(target) && !s.ws.is_dir(target) {
        clear_the_way(s, target)?;
    }
    if s.ws.is_dir(target) {
        for file in recorded_under(s, target) {
            let rel = &file[target.len() + 1..];
            if s.ws.is_file(&format!("{source}/{rel}")) || s.was_touched(&file) {
                continue;
            }
            s.ws.remove(&file)?;
            remove_empty_parents(s, &file, target)?;
        }
    }
    s.ws.create_dir_all(target)?;
    for file in s.ws.files_under(source) {
        let dest = format!("{target}{}", &file[source.len()..]);
        let mut dir = paths::parent(&dest);
        while dir != target && paths::is_within(&dir, target) {
            if s.ws.exists(&dir) && !s.ws.is_dir(&dir) {
                clear_the_way(s, &dir)?;
                break;
            }
            dir = paths::parent(&dir);
        }
        s.ws.create_dir_all(&paths::parent(&dest))?;
        replace_file(s, &file, &dest)?;
    }
    Ok(())
}

fn replace_file(s: &mut Session, source: &str, dest: &str) -> Result<(), Error> {
    if s.ws.is_dir(dest) {
        clear_the_way(s, dest)?;
    } else {
        refuse_copy_onto_itself(s, source, dest)?;
        s.ws.remove(dest)?;
    }
    s.ws.copy(source, dest)?;
    s.record_write(dest);
    Ok(())
}

/// A dest that a link makes the source file itself would be deleted by the
/// removal before its copy.
fn refuse_copy_onto_itself(s: &Session, src: &str, dest: &str) -> Result<(), Error> {
    if !s.ws.same_disk_file(src, dest) {
        return Ok(());
    }
    Err(Error::UnsafeOutput {
        action: "write",
        path: s.display(dest),
        reason: format!("it is the source file {} itself", s.display(src)),
    })
}

/// Removes `path`, which stands where the source has the other kind of
/// entry, when sync wrote everything in it or under `--force`.
fn clear_the_way(s: &mut Session, path: &str) -> Result<(), Error> {
    let (files, kind) = if s.ws.is_dir(path) {
        (s.ws.files_under(path), "a folder")
    } else {
        (vec![path.to_string()], "a file")
    };
    if s.force || files.iter().all(|f| s.recorded(f) || s.was_touched(f)) {
        return s.ws.remove(path);
    }
    Err(Error::UnsafeOutput {
        action: "replace",
        path: s.display(path),
        reason: format!("{kind} sync did not write is in the way"),
    })
}

/// Removes an obsolete entry sync may prune: a file whole, a folder whole
/// under `--force`, otherwise only its recorded files and the folders that
/// leaves empty. True when the entry is gone.
pub fn remove_obsolete(s: &mut Session, item: &str) -> Result<bool, Error> {
    if s.ws.is_dir(item) && !s.force {
        remove_recorded(s, item)?;
    } else {
        s.ws.remove(item)?;
    }
    Ok(!s.ws.exists(item))
}

/// Removes `path`, or every file below it, that the previous manifest records
/// and this run did not write, then the directories that leaves empty; files
/// nothing recorded stay. Returns how many files went, or would under
/// `--dry-run`.
pub fn remove_recorded(s: &mut Session, path: &str) -> Result<usize, Error> {
    remove_recorded_outside(s, path, &[])
}

/// [`remove_recorded`], keeping every file at or below one of `kept`.
pub fn remove_recorded_outside(
    s: &mut Session,
    path: &str,
    kept: &[String],
) -> Result<usize, Error> {
    let mut removed = 0;
    for file in recorded_under(s, path) {
        if s.was_touched(&file) || kept.iter().any(|keep| paths::is_within(&file, keep)) {
            continue;
        }
        if !s.dry_run {
            s.ws.remove(&file)?;
        }
        removed += 1;
    }
    if removed > 0 && !s.dry_run && s.ws.is_dir(path) {
        remove_empty_dirs(s, path)?;
    }
    Ok(removed)
}

/// The files at or below `path` the manifest records.
pub fn recorded_under(s: &Session, path: &str) -> Vec<String> {
    let files = if s.ws.is_dir(path) {
        s.ws.files_under(path)
    } else if s.ws.is_file(path) {
        vec![path.to_string()]
    } else {
        Vec::new()
    };
    files.into_iter().filter(|file| s.recorded(file)).collect()
}

/// Removes each directory above `path` that is empty, deepest first, up to
/// but not including `stop`.
pub fn remove_empty_parents(s: &mut Session, path: &str, stop: &str) -> Result<(), Error> {
    let mut dir = paths::parent(path);
    while dir != stop
        && paths::is_within(&dir, stop)
        && s.ws.is_dir(&dir)
        && s.ws.list(&dir).is_empty()
    {
        s.ws.remove(&dir)?;
        dir = paths::parent(&dir);
    }
    Ok(())
}

/// Whether `dir` ended up removed, being empty once its empty children went.
fn remove_empty_dirs(s: &mut Session, dir: &str) -> Result<bool, Error> {
    let mut empty = true;
    for name in s.ws.list(dir) {
        let child = format!("{dir}/{name}");
        if !s.ws.is_dir(&child) || !remove_empty_dirs(s, &child)? {
            empty = false;
        }
    }
    if empty {
        s.ws.remove(dir)?;
    }
    Ok(empty)
}

/// The tally a directory sync ends with: `(N updated)`, and `, M removed`
/// only once a prune happened.
pub fn counts(updated: usize, removed: usize) -> String {
    if removed == 0 {
        format!("({updated} updated)")
    } else {
        format!("({updated} updated, {removed} removed)")
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use crate::engine::session::test_session;
    use crate::engine::workspace::Content;

    fn file(s: &mut Session, path: &str, text: &str) {
        s.ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    #[test]
    fn copy_file_replaces_the_dest_and_records_it() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/AGENTS.md", "new");
        file(&mut s, "/proj/CLAUDE.md", "old");
        copy_file(&mut s, "/proj/.ai/src/AGENTS.md", "/proj/CLAUDE.md").unwrap();
        assert_eq!(s.ws.read("/proj/CLAUDE.md").unwrap(), b"new");
        assert!(s.was_touched("/proj/CLAUDE.md"));
        assert_eq!(s.log.tail(1), ["   .ai/src/AGENTS.md → CLAUDE.md"]);
    }

    #[test]
    fn copy_file_creates_parent_directories() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/mcp.json", "{}");
        copy_file(
            &mut s,
            "/proj/.ai/src/mcp.json",
            "/proj/.cursor/deep/mcp.json",
        )
        .unwrap();
        assert!(s.ws.is_dir("/proj/.cursor/deep"));
        assert_eq!(s.ws.read("/proj/.cursor/deep/mcp.json").unwrap(), b"{}");
    }

    #[test]
    fn sync_skills_dir_warns_on_a_missing_source() {
        let mut s = test_session();
        sync_skills_dir(&mut s, "/proj/nope", "/proj/out", &Filter::default()).unwrap();
        assert_eq!(
            s.log.tail(1),
            ["[WARNING] Source directory not found: /proj/nope"]
        );
        assert!(!s.ws.exists("/proj/out"));
    }

    #[test]
    fn copy_file_warns_on_a_missing_source() {
        let mut s = test_session();
        copy_file(&mut s, "/proj/nope.json", "/proj/.mcp.json").unwrap();
        assert_eq!(
            s.log.tail(1),
            ["[WARNING] Source file not found: /proj/nope.json"]
        );
        assert!(!s.ws.exists("/proj/.mcp.json"));
    }

    #[test]
    fn sync_skills_dir_copies_trees_and_prunes_only_what_the_filter_owns() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.ai/src/skills/a/references/r.md", "r");
        file(&mut s, "/proj/.ai/src/skills/.hidden/SKILL.md", "h");
        file(&mut s, "/proj/.claude/skills/stale/SKILL.md", "s");
        file(&mut s, "/proj/.claude/skills/command-review/SKILL.md", "c");
        s.activate_manifest(BTreeSet::from([
            ".claude/skills/stale/SKILL.md".to_string(),
            ".claude/skills/command-review/SKILL.md".to_string(),
        ]));
        sync_skills_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            &Filter::new("", "command-*"),
        )
        .unwrap();
        assert!(s.ws.is_file("/proj/.claude/skills/a/references/r.md"));
        assert!(!s.ws.exists("/proj/.claude/skills/.hidden"));
        assert!(!s.ws.exists("/proj/.claude/skills/stale"));
        assert!(s.ws.exists("/proj/.claude/skills/command-review"));
        assert!(s.was_touched("/proj/.claude/skills/a/references/r.md"));
        assert_eq!(
            s.log.tail(2),
            [
                "   Removed: .claude/skills/stale",
                "   .ai/src/skills/ → .claude/skills/ (1 updated, 1 removed)"
            ]
        );
    }

    #[test]
    fn sync_skills_dir_lands_categorized_skills_flat_by_name() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/flutter/bloc/SKILL.md", "b");
        file(
            &mut s,
            "/proj/.ai/src/skills/flutter/ui/slivers/SKILL.md",
            "s",
        );
        file(
            &mut s,
            "/proj/.ai/src/skills/flutter/ui/slivers/references/r.md",
            "r",
        );
        file(&mut s, "/proj/.ai/src/skills/flutter/notes.md", "n");
        file(
            &mut s,
            "/proj/.ai/src/skills/cloudflare/wrangler/SKILL.md",
            "w",
        );
        file(&mut s, "/proj/.claude/skills/wrangler/SKILL.md", "old");
        s.activate_manifest(BTreeSet::from([
            ".claude/skills/wrangler/SKILL.md".to_string()
        ]));
        sync_skills_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            &Filter::new("", "cloudflare/*"),
        )
        .unwrap();
        assert_eq!(
            s.ws.read("/proj/.claude/skills/bloc/SKILL.md").unwrap(),
            b"b"
        );
        assert!(s.ws.is_file("/proj/.claude/skills/slivers/references/r.md"));
        assert!(!s.ws.exists("/proj/.claude/skills/flutter"));
        assert!(!s.ws.exists("/proj/.claude/skills/cloudflare"));
        assert_eq!(
            s.ws.read("/proj/.claude/skills/wrangler/SKILL.md").unwrap(),
            b"old"
        );
        assert_eq!(
            s.log.tail(1),
            ["   .ai/src/skills/ → .claude/skills/ (2 updated)"]
        );
    }

    #[test]
    fn remove_recorded_takes_only_what_the_manifest_recorded() {
        let mut s = test_session();
        file(&mut s, "/proj/.windsurf/rules/core.md", "c");
        file(&mut s, "/proj/.windsurf/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.windsurf/skills/a/references/r.md", "r");
        file(&mut s, "/proj/.windsurf/rules/mine.md", "m");
        s.activate_manifest(BTreeSet::from([
            ".windsurf/rules/core.md".to_string(),
            ".windsurf/skills/a/SKILL.md".to_string(),
            ".windsurf/skills/a/references/r.md".to_string(),
        ]));
        assert_eq!(remove_recorded(&mut s, "/proj/.windsurf").unwrap(), 3);
        assert!(!s.ws.exists("/proj/.windsurf/skills"));
        assert!(!s.ws.exists("/proj/.windsurf/rules/core.md"));
        assert_eq!(s.ws.read("/proj/.windsurf/rules/mine.md").unwrap(), b"m");
        assert_eq!(remove_recorded(&mut s, "/proj/.nope").unwrap(), 0);
    }

    #[test]
    fn a_skill_replaces_an_entry_of_the_other_kind_only_when_sync_wrote_it() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.ai/src/skills/b/SKILL.md", "b");
        file(&mut s, "/proj/.claude/skills/a", "mine");
        file(&mut s, "/proj/.claude/skills/b", "old");
        s.activate_manifest(BTreeSet::from([".claude/skills/b".to_string()]));
        let skills = "/proj/.ai/src/skills";
        let dest = "/proj/.claude/skills";
        assert_eq!(
            replace_skill(&mut s, &format!("{skills}/a"), &format!("{dest}/a"))
                .unwrap_err()
                .to_string(),
            "Refusing to replace .claude/skills/a: a file sync did not write is in the way"
        );
        assert_eq!(s.ws.read("/proj/.claude/skills/a").unwrap(), b"mine");
        replace_skill(&mut s, &format!("{skills}/b"), &format!("{dest}/b")).unwrap();
        assert_eq!(s.ws.read("/proj/.claude/skills/b/SKILL.md").unwrap(), b"b");
        s.force = true;
        replace_skill(&mut s, &format!("{skills}/a"), &format!("{dest}/a")).unwrap();
        assert_eq!(s.ws.read("/proj/.claude/skills/a/SKILL.md").unwrap(), b"a");
    }

    #[test]
    fn remove_recorded_keeps_everything_without_a_manifest() {
        let mut s = test_session();
        file(&mut s, "/proj/.windsurf/rules/core.md", "c");
        assert_eq!(remove_recorded(&mut s, "/proj/.windsurf").unwrap(), 0);
        assert!(s.ws.exists("/proj/.windsurf/rules/core.md"));
    }

    #[test]
    fn a_dry_run_reports_every_change_and_makes_none() {
        let mut s = test_session();
        s.dry_run = true;
        file(&mut s, "/proj/.ai/src/AGENTS.md", "new");
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.claude/skills/stale/SKILL.md", "s");
        s.activate_manifest(BTreeSet::from(
            [".claude/skills/stale/SKILL.md".to_string()],
        ));
        copy_file(&mut s, "/proj/.ai/src/AGENTS.md", "/proj/CLAUDE.md").unwrap();
        sync_skills_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            &Filter::default(),
        )
        .unwrap();
        assert!(!s.ws.exists("/proj/CLAUDE.md"));
        assert!(s.ws.exists("/proj/.claude/skills/stale"));
        assert!(!s.ws.exists("/proj/.claude/skills/a"));
        assert!(s.touched().is_empty());
        assert_eq!(
            s.log.tail(3),
            [
                "   .ai/src/AGENTS.md → CLAUDE.md (dry-run)",
                "   Would remove: .claude/skills/stale (extraneous)",
                "   .ai/src/skills/ → .claude/skills/ (1 updated, 1 removed) (dry-run)",
            ]
        );
    }

    #[test]
    fn sync_skills_dir_keeps_an_entry_the_manifest_never_recorded() {
        let mut s = test_session();
        file(&mut s, "/proj/.ai/src/skills/a/SKILL.md", "a");
        file(&mut s, "/proj/.claude/skills/mine/SKILL.md", "m");
        file(&mut s, "/proj/.claude/skills/old/SKILL.md", "o");
        s.activate_manifest(BTreeSet::from([".claude/skills/old/SKILL.md".to_string()]));
        sync_skills_dir(
            &mut s,
            "/proj/.ai/src/skills",
            "/proj/.claude/skills",
            &Filter::default(),
        )
        .unwrap();
        assert!(s.ws.exists("/proj/.claude/skills/mine"));
        assert!(!s.ws.exists("/proj/.claude/skills/old"));
        assert_eq!(s.preserved(), 1);
        assert_eq!(
            s.log.tail(3),
            [
                "[WARNING] Kept .claude/skills/mine (not from .ai/src/; move it into .ai/src/, or re-run with --force to prune)",
                "   Removed: .claude/skills/old",
                "   .ai/src/skills/ → .claude/skills/ (1 updated, 1 removed)"
            ]
        );
    }
}
