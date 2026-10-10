//! A forked child holds every descriptor of its parent until it execs, so a
//! test elsewhere in the same binary spawning a process can keep a dropped
//! `flock` held for a moment. This file spawns nothing and is its own process.

use exuno::paths::{self, DiskText};
use exuno::transaction::backup::{self, Retention};
use exuno::transaction::lock::{Lock, Recovered};
use std::path::Path;

struct Project {
    _dir: tempfile::TempDir,
    root: String,
}

impl Project {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        Self { _dir: dir, root }
    }

    fn write(&self, rel: &str, text: &str) {
        let path = Path::new(&self.root).join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(Path::new(&self.root).join(rel)).unwrap()
    }

    fn exists(&self, rel: &str) -> bool {
        Path::new(&self.root).join(rel).exists()
    }

    fn acquire(&self, operation: &str) -> Result<(Lock, Option<Recovered>), exuno::Error> {
        Lock::acquire(&self.root, operation, Retention::Bounded)
    }

    fn snapshot(&self, rel: &str) -> String {
        backup::create(
            &self.root,
            "sync",
            &[format!("{}/{rel}", self.root)],
            Retention::Bounded,
        )
        .unwrap()
    }
}

#[test]
fn a_second_run_is_refused_while_the_first_holds_the_lock() {
    let p = Project::new();
    let (first, _) = p.acquire("sync").unwrap();
    assert_eq!(
        p.acquire("import").err().unwrap().to_string(),
        format!(
            "Another exuno sync (pid {}) is changing this project; wait for it to finish, then run the command again",
            std::process::id()
        )
    );
    first.finish().unwrap();
    drop(first);
    assert!(p.acquire("import").unwrap().1.is_none());
}

#[test]
fn a_run_that_died_mid_write_is_restored_by_the_next() {
    let p = Project::new();
    p.write("AGENTS.md", "before\n");
    let (lock, _) = p.acquire("sync").unwrap();
    let snapshot = p.snapshot("AGENTS.md");
    lock.begin(&snapshot).unwrap();
    p.write("AGENTS.md", "half-written\n");
    drop(lock);

    let (next, recovered) = p.acquire("sync").unwrap();
    let recovered = recovered.unwrap();
    assert_eq!(recovered.operation, "sync");
    assert_eq!(recovered.backup, paths::leaf(&snapshot));
    assert!(recovered.undo.contains("-recover-"));
    assert_eq!(p.read("AGENTS.md"), "before\n");
    assert_eq!(
        p.read(&format!(".ai/backups/{}/files/AGENTS.md", recovered.undo)),
        "half-written\n"
    );
    assert_eq!(
        p.read(".ai/backups/.latest"),
        format!("{}\n", recovered.backup)
    );
    assert_eq!(
        p.read(".ai/backups/.pending"),
        format!("operation=sync\npid={}\n", std::process::id())
    );
    next.finish().unwrap();
    assert!(!p.exists(".ai/backups/.pending"));
}

#[test]
fn a_finished_run_or_one_that_died_before_its_backup_leaves_nothing_to_restore() {
    let p = Project::new();
    p.write("AGENTS.md", "before\n");
    let (lock, _) = p.acquire("sync").unwrap();
    lock.begin(&p.snapshot("AGENTS.md")).unwrap();
    p.write("AGENTS.md", "after\n");
    lock.finish().unwrap();
    drop(lock);
    assert!(p.acquire("sync").unwrap().1.is_none());
    assert_eq!(p.read("AGENTS.md"), "after\n");

    p.write(".ai/backups/.pending", "operation=sync\npid=1\n");
    assert!(p.acquire("sync").unwrap().1.is_none());
    assert_eq!(p.read("AGENTS.md"), "after\n");
}

#[test]
fn a_record_naming_a_missing_backup_stops_the_run() {
    let p = Project::new();
    p.write(
        ".ai/backups/.pending",
        "operation=sync\npid=1\nbackup=20200101T000000Z-sync-1\n",
    );
    assert_eq!(
        p.acquire("sync").err().unwrap().to_string(),
        "An interrupted exuno sync left its backup 20200101T000000Z-sync-1 missing; check the project, then delete .ai/backups/.pending"
    );
    assert!(p.exists(".ai/backups/.pending"));
}
