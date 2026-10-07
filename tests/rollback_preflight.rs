//! `tests/rollback_preflight.bats`: foreign changes must block the whole
//! rollback, even with `--yes`. Every project in this file shares the same
//! setup: `claude` and `codex` initialised with every target but `skills`
//! disabled, so `.claude/skills`/`.agents/skills` is the one Exuno-managed
//! tree and `.codex/config.toml`/`.claude/settings.json` stay native files
//! that are still protected targets (their dest is collected regardless of
//! `enabled`).

mod common;

use std::path::Path;
#[cfg(unix)]
use std::path::PathBuf;

use common::Project;
use predicates::prelude::*;

/// `setup()`: `init --tools claude,codex`, then the shared config and the
/// per-tool overrides that disable every target but `skills`.
fn project() -> Project {
    let project = Project::empty();
    project
        .exuno()
        .args(["init", "--tools", "claude,codex", "--yes", "--no-sync"])
        .assert()
        .success();
    project.write(
        ".ai/agent_sync.yaml",
        "outputs: local\nbase_skills: false\ndefaults:\n  cleanup: false\ntools:\n  enabled: [claude, codex]\nbackup:\n  retention: preserve\n",
    );
    for tool in ["claude", "codex"] {
        project.write(
            &format!(".ai/src/tools/{tool}.yaml"),
            "targets:\n  agents:\n    enabled: false\n  rules:\n    enabled: false\n  commands:\n    enabled: false\n  subagents:\n    enabled: false\n  settings:\n    enabled: false\n  mcp:\n    enabled: false\n  hooks:\n    enabled: false\n  guard:\n    enabled: false\n",
        );
    }
    project
}

/// `sync_once`: a plain sync, returning the snapshot id it took.
fn sync_once(project: &Project) -> String {
    project.exuno().arg("sync").assert().success();
    project.read(".ai/backups/.latest").trim().to_string()
}

/// `assert_refused_unchanged`: the rollback attempt fails with a conflict, and
/// the whole project tree is byte-for-byte what it was right before the
/// attempt — the bats tar+diff evidence, replaced by hashing the live tree
/// before and after since there is no tar round trip to prove here.
fn assert_refused_unchanged(project: &Project, sync_id: &str) {
    let before = snapshot_tree(project.path());
    project
        .exuno()
        .args(["rollback", sync_id, "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains("Error: Rollback conflict: "));
    assert_eq!(snapshot_tree(project.path()), before);
}

type Tree = Vec<(String, Entry)>;

/// What a path is, for comparing two points in time: a symlink's target text,
/// a regular file's mode and content hash, a directory's mode, or a FIFO —
/// mirroring the bats `assert_tree_equal` oracle (`lstat` plus content hash,
/// never a followed symlink).
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Dir(u32),
    File(u32, String),
    Symlink(String),
    // Only a #[cfg(unix)] case creates one.
    #[cfg_attr(not(unix), allow(dead_code))]
    Fifo,
    Other,
}

fn snapshot_tree(root: &Path) -> Tree {
    let mut out = Vec::new();
    walk(root, root, &mut out);
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

fn walk(root: &Path, dir: &Path, out: &mut Tree) {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap())
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        let rel = path
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let described = describe(&path);
        let recurse = matches!(described, Entry::Dir(_));
        out.push((rel, described));
        if recurse {
            walk(root, &path, out);
        }
    }
}

fn describe(path: &Path) -> Entry {
    let meta = std::fs::symlink_metadata(path).unwrap();
    let ft = meta.file_type();
    if ft.is_symlink() {
        Entry::Symlink(
            std::fs::read_link(path)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/"),
        )
    } else if ft.is_dir() {
        Entry::Dir(mode_of(&meta))
    } else if ft.is_file() {
        Entry::File(
            mode_of(&meta),
            exuno::transaction::manifest::sha256_hex(&std::fs::read(path).unwrap()),
        )
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::FileTypeExt;
            if ft.is_fifo() {
                return Entry::Fifo;
            }
        }
        Entry::Other
    }
}

#[cfg(unix)]
fn mode_of(meta: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn mode_of(_meta: &std::fs::Metadata) -> u32 {
    0
}

/// The entries at or below `prefix`, from a whole-tree snapshot.
fn subset(tree: &Tree, prefix: &str) -> Tree {
    tree.iter()
        .filter(|(p, _)| p == prefix || p.starts_with(&format!("{prefix}/")))
        .cloned()
        .collect()
}

/// `create_test_symlink`: a symlink to `target` at `link`, directory-aware on
/// Windows the way the bats helper's `MSYS=winsymlinks:nativestrict` is.
fn symlink(target: &Path, link: &Path) {
    #[cfg(unix)]
    std::os::unix::fs::symlink(target, link).unwrap();
    #[cfg(windows)]
    {
        if target.is_dir() {
            std::os::windows::fs::symlink_dir(target, link).unwrap();
        } else {
            std::os::windows::fs::symlink_file(target, link).unwrap();
        }
    }
}

#[test]
fn rollback_preflight_blocks_later_native_settings_even_when_settings_are_disabled() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".codex/config.toml", "foreign-codex\n");
    project.write(".claude/settings.json", "{\"foreign\":\"claude\"}\n");
    assert_refused_unchanged(&project, &sync_id);
    assert_eq!(project.read(".codex/config.toml"), "foreign-codex\n");
    assert_eq!(
        project.read(".claude/settings.json"),
        "{\"foreign\":\"claude\"}\n"
    );
}

#[test]
fn rollback_preflight_blocks_a_changed_existing_native_file() {
    let project = project();
    project.write(".codex/config.toml", "native-before\n");
    let sync_id = sync_once(&project);
    project.write(".codex/config.toml", "native-after\n");
    assert_refused_unchanged(&project, &sync_id);
}

#[test]
fn rollback_preflight_blocks_deletion_of_an_existing_target() {
    let project = project();
    project.write(".claude/settings.json", "native-before\n");
    let sync_id = sync_once(&project);
    std::fs::remove_file(project.join(".claude/settings.json")).unwrap();
    assert_refused_unchanged(&project, &sync_id);
}

#[test]
fn rollback_preflight_blocks_foreign_children_in_managed_directories_before_any_writes() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".claude/skills/foreign/NOTE.md", "unrelated\n");
    assert_refused_unchanged(&project, &sync_id);
}

#[test]
fn rollback_preflight_blocks_a_directory_replaced_by_a_file() {
    let project = project();
    let sync_id = sync_once(&project);
    let holder = project.path().join("skills-before-type-change");
    std::fs::rename(project.join(".claude/skills"), &holder).unwrap();
    project.write(".claude/skills", "foreign file\n");
    assert_refused_unchanged(&project, &sync_id);
}

#[test]
fn rollback_preflight_blocks_a_file_replaced_by_a_directory() {
    let project = project();
    project.write(".codex/config.toml", "native-before\n");
    let sync_id = sync_once(&project);
    std::fs::remove_file(project.join(".codex/config.toml")).unwrap();
    std::fs::create_dir_all(project.join(".codex/config.toml")).unwrap();
    project.write(".codex/config.toml/child", "foreign child\n");
    assert_refused_unchanged(&project, &sync_id);
}

// The tar checkpoint cannot recreate Windows symlinks (the bats skip reason);
// `create_test_symlink` also needs privileges Windows CI may not grant here.
#[cfg(unix)]
#[test]
fn rollback_preflight_blocks_symlink_replacement_without_touching_either_link_target() {
    let project = project();
    project.write("private-knowledge/data", "original\n");
    project.write("other-knowledge/data", "other\n");
    std::fs::create_dir_all(project.join(".claude/skills/manual")).unwrap();
    symlink(
        &project.join("private-knowledge"),
        &project.join(".claude/skills/manual/knowledge"),
    );
    let sync_id = sync_once(&project);
    std::fs::remove_file(project.join(".claude/skills/manual/knowledge")).unwrap();
    symlink(
        &project.join("other-knowledge"),
        &project.join(".claude/skills/manual/knowledge"),
    );
    assert_refused_unchanged(&project, &sync_id);
    assert_eq!(project.read("private-knowledge/data"), "original\n");
    assert_eq!(project.read("other-knowledge/data"), "other\n");
}

// The tar checkpoint cannot recreate Windows symlinks.
#[cfg(unix)]
#[test]
fn rollback_preflight_blocks_a_dangling_link_added_at_an_absent_target() {
    let project = project();
    let sync_id = sync_once(&project);
    std::fs::create_dir_all(project.join(".codex")).unwrap();
    symlink(
        &project.join("does-not-exist"),
        &project.join(".codex/config.toml"),
    );
    assert_refused_unchanged(&project, &sync_id);
    assert!(project.join(".codex/config.toml").is_symlink());
}

// The tar checkpoint cannot recreate Windows symlinks.
#[cfg(unix)]
#[test]
fn rollback_preflight_ignores_mutable_knowledge_contents_and_normal_undo_is_still_safe() {
    let project = project();
    project.write("private-knowledge/data", "before\n");
    std::fs::create_dir_all(project.join(".claude/skills/manual")).unwrap();
    symlink(
        &project.join("private-knowledge"),
        &project.join(".claude/skills/manual/knowledge"),
    );
    let sync_id = sync_once(&project);
    let after_sync = subset(&snapshot_tree(project.path()), ".claude/skills");
    project.write("private-knowledge/data", "legitimate new knowledge\n");
    project.write("private-knowledge/new", "new entry\n");

    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success();
    assert_eq!(
        project.read("private-knowledge/data"),
        "legitimate new knowledge\n"
    );
    assert_eq!(project.read("private-knowledge/new"), "new entry\n");
    assert!(project.join(".claude/skills/manual/knowledge").is_symlink());

    let undo = project.read(".ai/backups/.latest").trim().to_string();
    project
        .exuno()
        .args(["rollback", &undo, "--yes"])
        .assert()
        .success();
    assert_eq!(
        project.read("private-knowledge/data"),
        "legitimate new knowledge\n"
    );
    assert_eq!(project.read("private-knowledge/new"), "new entry\n");
    assert!(project.join(".claude/skills/manual/knowledge").is_symlink());
    assert_eq!(
        subset(&snapshot_tree(project.path()), ".claude/skills"),
        after_sync
    );
    assert!(project.join(&format!(".ai/backups/{sync_id}")).is_dir());
    assert!(project.join(&format!(".ai/backups/{undo}")).is_dir());
}

#[test]
fn rollback_preflight_compares_the_actual_post_sync_state_not_the_pre_sync_backup() {
    let project = project();
    project.write(".claude/skills/manual/SKILL.md", "old user skill\n");
    let before_sync = subset(&snapshot_tree(project.path()), ".claude/skills");
    let sync_id = sync_once(&project);
    let after_sync = subset(&snapshot_tree(project.path()), ".claude/skills");

    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success();
    assert_eq!(
        subset(&snapshot_tree(project.path()), ".claude/skills"),
        before_sync
    );

    let undo = project.read(".ai/backups/.latest").trim().to_string();
    project
        .exuno()
        .args(["rollback", &undo, "--yes"])
        .assert()
        .success();
    assert_eq!(
        subset(&snapshot_tree(project.path()), ".claude/skills"),
        after_sync
    );
}

#[test]
fn rollback_preflight_restores_an_unsealed_historical_snapshot_with_a_warning() {
    let project = project();
    project.write(".codex/config.toml", "before\n");
    let sync_id = sync_once(&project);
    // A snapshot from before the seal existed: the same layout without after.tsv.
    std::fs::remove_file(project.join(&format!(".ai/backups/{sync_id}/after.tsv"))).unwrap();
    project.write(".codex/config.toml", "after\n");
    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Warning: Backup {sync_id} has no post-operation record; changes made after that operation cannot be detected."
        )));
    assert_eq!(project.read(".codex/config.toml"), "before\n");
}

#[test]
fn rollback_preflight_dry_run_shows_the_plan_and_the_conflict_and_exits_1() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".codex/config.toml", "foreign\n");
    let before_refusal = snapshot_tree(project.path());

    project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run"])
        .assert()
        .code(1)
        .stdout(predicate::str::contains("Rollback plan:"))
        .stderr(predicate::str::contains(format!(
            "Error: Rollback conflict: .codex/config.toml changed after the operation recorded in backup {sync_id}; no files were changed."
        )))
        .stdout(predicate::str::contains("Dry run — nothing was written."));

    project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run", "--force"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Warning: Rollback conflict: .codex/config.toml changed after the operation recorded in backup {sync_id}; --force will overwrite it."
        )));
    assert_eq!(snapshot_tree(project.path()), before_refusal);
}

#[test]
fn rollback_conflict_names_the_first_differing_path_inside_a_directory() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".claude/skills/.DS_Store", "finder\n");
    project
        .exuno()
        .args(["rollback", "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Error: Rollback conflict: .claude/skills/.DS_Store changed after the operation recorded in backup {sync_id}; no files were changed."
        )))
        .stderr(predicate::str::contains(
            "Re-run with --force to restore the backup anyway and discard that change.",
        ));
    assert_eq!(project.read(".ai/backups/.latest").trim(), sync_id);
}

#[test]
fn rollback_force_restores_over_a_conflict_and_keeps_it_undoable() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".claude/skills/foreign/NOTE.md", "unrelated\n");
    project
        .exuno()
        .args(["rollback", &sync_id, "--yes", "--force"])
        .assert()
        .success();
    assert!(!project.exists(".claude/skills/foreign"));
    let undo = project.read(".ai/backups/.latest").trim().to_string();
    assert_ne!(undo, sync_id);
    project
        .exuno()
        .args(["rollback", &undo, "--yes"])
        .assert()
        .success();
    assert_eq!(
        project.read(".claude/skills/foreign/NOTE.md"),
        "unrelated\n"
    );
}

#[test]
fn rolling_back_an_older_backup_after_a_newer_sync_points_at_the_newer_backups() {
    let project = project();
    let first = sync_once(&project);
    project.write(
        ".ai/src/skills/added/SKILL.md",
        "---\nname: added\ndescription: Added after the first sync.\n---\n\nBody.\n",
    );
    project.exuno().arg("sync").assert().success();
    let second = project.read(".ai/backups/.latest").trim().to_string();
    assert!(project.exists(".claude/skills/added/SKILL.md"));

    project
        .exuno()
        .args(["rollback", &first, "--yes"])
        .assert()
        .code(1)
        .stderr(predicate::str::contains(format!(
            "Error: Rollback conflict: .claude/skills/added changed after the operation recorded in backup {first}; no files were changed."
        )))
        .stderr(predicate::str::contains(
            "Newer Exuno operations may have changed this target. Roll back the newer backups first, or re-run with --force to restore anyway.",
        ));
    project
        .exuno()
        .args(["rollback", &second, "--yes"])
        .assert()
        .success();
    project
        .exuno()
        .args(["rollback", &first, "--yes"])
        .assert()
        .success();
    assert!(!project.exists(".claude/skills"));
}

// Windows does not model POSIX chmod modes.
#[cfg(unix)]
#[test]
fn rollback_preflight_detects_mode_changes_on_regular_files() {
    project_mode_change_case();
}

#[cfg(unix)]
fn project_mode_change_case() {
    let project = project();
    project.write(".codex/config.toml", "before\n");
    common::chmod(&project.join(".codex/config.toml"), 0o644);
    let sync_id = sync_once(&project);
    common::chmod(&project.join(".codex/config.toml"), 0o755);
    assert_refused_unchanged(&project, &sync_id);
}

// The tar checkpoint cannot recreate Windows symlinks.
#[cfg(unix)]
#[test]
fn rollback_preflight_refuses_changed_ancestor_links_even_when_contents_match() {
    let project = project();
    let sync_id = sync_once(&project);
    // Children only — `moved_tree` below walks the relocated directory itself,
    // so it never yields a self-entry for `.claude` to compare against.
    let after_sync_claude: Tree = snapshot_tree(project.path())
        .into_iter()
        .filter(|(p, _)| p.starts_with(".claude/"))
        .collect();
    // Moved outside the project root, like the bats `$PROOF_DIR`: the target's
    // bytes are unchanged, but reaching them now escapes the canonical root,
    // which is refused independently of the byte-for-byte content match.
    let outside = tempfile::tempdir().unwrap();
    let moved = outside.path().join("claude-original");
    std::fs::rename(project.join(".claude"), &moved).unwrap();
    symlink(&moved, &project.join(".claude"));
    let before_refusal = snapshot_tree(project.path());

    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .failure();
    assert_eq!(snapshot_tree(project.path()), before_refusal);
    // The moved directory (now reached only through the ancestor symlink)
    // still holds exactly the post-sync content.
    let moved_tree: Tree = {
        let mut out = Vec::new();
        walk(&moved, &moved, &mut out);
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out.into_iter()
            .map(|(rel, entry)| (format!(".claude/{rel}"), entry))
            .collect()
    };
    assert_eq!(moved_tree, after_sync_claude);
}

#[test]
fn rollback_preflight_treats_a_malformed_post_operation_record_as_unsealed() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".claude/skills/foreign/NOTE.md", "broken\n");
    let record = format!(".ai/backups/{sync_id}/after.tsv");
    project.write(&record, "broken\n");
    project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Warning: Backup {sync_id} has a malformed post-operation record; changes made after that operation cannot be detected."
        )));

    project.write(
        &record,
        &format!(
            "post-state-v2\t{}\nfile\tnot-a-hash\t.claude/skills\n",
            "0".repeat(64)
        ),
    );
    project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run"])
        .assert()
        .success()
        .stderr(predicate::str::contains(
            "has a post-operation record that does not match its target list",
        ));

    project.write(&record, "");
    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Warning: Backup {sync_id} has a malformed post-operation record"
        )));
    assert!(!project.exists(".claude/skills/foreign"));
}

#[test]
fn rollback_preflight_treats_a_record_with_a_malformed_body_as_unsealed() {
    let project = project();
    let sync_id = sync_once(&project);
    project.write(".claude/skills/NOTE.md", "foreign\n");
    let record = format!(".ai/backups/{sync_id}/after.tsv");
    let header = project.read(&record).lines().next().unwrap().to_string();
    project.write(
        &record,
        &format!("{header}\nfile\tnot-a-hash\t.claude/skills\n"),
    );
    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success()
        .stderr(predicate::str::contains(format!(
            "Warning: Backup {sync_id} has a malformed post-operation record"
        )));
    assert!(!project.exists(".claude/skills/NOTE.md"));
}

// Windows does not support control characters in filenames (the bats skip
// checked `$OSTYPE` for `msys*`).
#[cfg(unix)]
#[test]
fn rollback_preflight_supports_unusual_child_names_without_ignoring_their_changes() {
    let project = project();
    let name = "tab\tand\nnewline";
    project.write(&format!(".claude/skills/manual/{name}"), "before\n");
    let sync_id = sync_once(&project);
    project.write(&format!(".claude/skills/manual/{name}"), "after\n");
    assert_refused_unchanged(&project, &sync_id);
}

#[test]
fn rollback_preflight_supports_sealed_init_snapshots_and_their_undo() {
    let fresh = Project::empty();
    fresh
        .exuno()
        .args(["init", "--tools", "claude", "--yes", "--no-sync"])
        .assert()
        .success();
    let init_id = fresh.read(".ai/backups/.latest").trim().to_string();
    let before_init_rollback = subset(&snapshot_tree(fresh.path()), ".ai/src");

    fresh
        .exuno()
        .args(["rollback", &init_id, "--yes"])
        .assert()
        .success();
    assert!(!fresh.exists(".ai/src"));

    let undo_id = fresh.read(".ai/backups/.latest").trim().to_string();
    fresh
        .exuno()
        .args(["rollback", &undo_id, "--yes"])
        .assert()
        .success();
    assert_eq!(
        subset(&snapshot_tree(fresh.path()), ".ai/src"),
        before_init_rollback
    );
}

// chmod 000 does not restrict access on Windows; bats skipped this case at
// runtime there too (`[ -r "$victim" ]`).
#[cfg(unix)]
#[test]
fn an_unreadable_file_is_the_reported_conflict_not_the_files_hashed_after_it() {
    if !common::unreadable_dirs_are_possible() {
        return;
    }
    let project = project();
    let sync_id = sync_once(&project);
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(&project.join(".claude/skills"), &mut files);
    files.sort();
    let victim = files
        .into_iter()
        .next()
        .expect("sync should have left at least one file under .claude/skills");

    common::chmod(&victim, 0o000);
    if std::fs::read(&victim).is_ok() {
        common::chmod(&victim, 0o644);
        return;
    }
    let output = project
        .exuno()
        .args(["rollback", &sync_id, "--dry-run"])
        .output()
        .unwrap();
    common::chmod(&victim, 0o644);

    assert_eq!(output.status.code(), Some(1));
    let victim_rel = victim
        .strip_prefix(project.path())
        .unwrap()
        .to_string_lossy()
        .replace('\\', "/");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(&format!(
        "Error: Rollback conflict: {victim_rel} changed after"
    )));
}

#[cfg(unix)]
fn collect_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            collect_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

#[test]
fn sync_through_a_claude_symlink_inside_the_project_succeeds_and_seals() {
    let project = project();
    std::fs::create_dir_all(project.join("tooling/claude")).unwrap();
    symlink(&project.join("tooling/claude"), &project.join(".claude"));
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("restoring pre-sync state").not())
        .stderr(predicate::str::contains("restoring pre-sync state").not());
    let sync_id = project.read(".ai/backups/.latest").trim().to_string();
    let record = project.read(&format!(".ai/backups/{sync_id}/after.tsv"));
    assert!(record.contains("dir\t-\t.claude/skills"));
    assert!(project.join(".claude").is_symlink());
    project
        .exuno()
        .args(["rollback", &sync_id, "--yes"])
        .assert()
        .success();
}

// Windows has no FIFOs.
#[cfg(unix)]
#[test]
fn sync_with_a_fifo_under_a_target_succeeds_and_records_it_by_type() {
    let project = project();
    std::fs::create_dir_all(project.join(".claude/skills")).unwrap();
    let pipe = project.join(".claude/skills/pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&pipe)
        .status()
        .is_ok_and(|status| status.success());
    if !made {
        // The filesystem does not support FIFOs.
        return;
    }
    project
        .exuno()
        .arg("sync")
        .assert()
        .success()
        .stdout(predicate::str::contains("restoring pre-sync state").not())
        .stderr(predicate::str::contains("restoring pre-sync state").not());
    let sync_id = project.read(".ai/backups/.latest").trim().to_string();
    let record = project.read(&format!(".ai/backups/{sync_id}/after.tsv"));
    assert!(record.contains("other\t-\t.claude/skills/pipe"));
}
