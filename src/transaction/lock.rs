//! The project lock a mutating run takes before its backup, and the
//! `.ai/backups/.pending` record that names the snapshot while the run changes
//! files. The OS releases the lock when the process dies, so a record that no
//! lock guards is a run that never finished: the next run restores it first.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;

use crate::Error;
use crate::paths;
use crate::transaction::{backup, witness};

/// The held project lock; released when dropped or when the process ends.
pub struct Lock {
    file: File,
    store: String,
    operation: String,
}

impl Drop for Lock {
    // flock belongs to the open file description, which a child forked
    // elsewhere in the process shares until it execs; closing our descriptor
    // alone would leave the project busy until then.
    fn drop(&mut self) {
        let _ = self.file.unlock();
    }
}

/// An interrupted run that `Lock::acquire` restored before handing over.
#[derive(Debug)]
pub struct Recovered {
    pub operation: String,
    pub backup: String,
    pub undo: String,
}

impl Recovered {
    pub fn message(&self) -> String {
        format!(
            "Restored the state from before an interrupted exuno {} (backup {}); undo with exuno rollback {}",
            self.operation, self.backup, self.undo
        )
    }
}

#[derive(Default)]
struct Pending {
    operation: String,
    pid: String,
    backup: Option<String>,
}

fn read_pending(store: &str) -> Option<Pending> {
    let path = Path::new(store).join(".pending");
    if path.is_symlink() || !path.is_file() {
        return None;
    }
    let bytes = std::fs::read(&path).ok()?;
    let mut pending = Pending::default();
    for line in String::from_utf8_lossy(&bytes).split('\n') {
        match line.split_once('=') {
            Some(("operation", value)) => pending.operation = printable(value),
            Some(("pid", value)) => pending.pid = printable(value),
            Some(("backup", value)) if !value.is_empty() => {
                pending.backup = Some(printable(value));
            }
            _ => {}
        }
    }
    Some(pending)
}

/// The record comes from the project directory and its fields end up on the
/// terminal, so control characters are shown escaped rather than emitted.
fn printable(value: &str) -> String {
    let mut shown = String::new();
    for c in value.chars() {
        if c.is_control() {
            shown.extend(c.escape_default());
        } else {
            shown.push(c);
        }
    }
    shown
}

fn busy(store: &str) -> Error {
    let holder = match read_pending(store) {
        Some(pending) if !pending.operation.is_empty() && !pending.pid.is_empty() => {
            format!("exuno {} (pid {})", pending.operation, pending.pid)
        }
        _ => "exuno run".to_string(),
    };
    Error::Backup(format!(
        "Another {holder} is changing this project; wait for it to finish, then run the command again"
    ))
}

impl Lock {
    /// Takes the project lock for `operation`, or refuses while another run
    /// holds it. A run that died mid-write is restored from its snapshot
    /// first, after a `recover` snapshot of what it left.
    pub fn acquire(
        root: &str,
        operation: &str,
        retention: backup::Retention,
    ) -> Result<(Self, Option<Recovered>), Error> {
        let store = backup::open_store(root)?;
        let path = format!("{store}/.lock");
        if Path::new(&path).is_symlink() {
            return Err(Error::Backup(
                "Exuno lock cannot be a symlink: .ai/backups/.lock".to_string(),
            ));
        }
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|e| Error::io(&path, e))?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => return Err(busy(&store)),
            Err(TryLockError::Error(e)) => return Err(Error::io(&path, e)),
        }
        let lock = Self {
            file,
            store,
            operation: operation.to_string(),
        };
        let recovered = lock.recover(root, retention)?;
        lock.record(None)?;
        Ok((lock, recovered))
    }

    /// Names `snapshot` as the one to restore if the run dies before `finish`.
    pub fn begin(&self, snapshot: &str) -> Result<(), Error> {
        self.record(Some(&paths::leaf(snapshot)))
    }

    /// Clears the record once the run has sealed, restored, or discarded its
    /// snapshot; the lock stays held until dropped.
    pub fn finish(&self) -> Result<(), Error> {
        let path = format!("{}/.pending", self.store);
        match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(Error::io(&path, e)),
            _ => Ok(()),
        }
    }

    fn record(&self, backup: Option<&str>) -> Result<(), Error> {
        let mut text = format!("operation={}\npid={}\n", self.operation, std::process::id());
        if let Some(id) = backup {
            text.push_str(&format!("backup={id}\n"));
        }
        backup::write_store_file(&self.store, ".pending", text.as_bytes())
    }

    fn recover(
        &self,
        root: &str,
        retention: backup::Retention,
    ) -> Result<Option<Recovered>, Error> {
        let Some(pending) = read_pending(&self.store) else {
            return Ok(None);
        };
        let Some(id) = pending.backup else {
            self.finish()?;
            return Ok(None);
        };
        let operation = pending.operation;
        let snapshot = backup::snapshot_path(root, &id).map_err(|_| {
            Error::Backup(format!(
                "An interrupted exuno {operation} left its backup {id} missing; check the project, then delete .ai/backups/.pending"
            ))
        })?;
        let unrecoverable = |e: Error| {
            Error::Backup(format!(
                "Could not restore the state from before an interrupted exuno {operation} (backup {id}): {e}"
            ))
        };
        let targets: Vec<String> = backup::load_targets(root, &snapshot)
            .map_err(unrecoverable)?
            .iter()
            .map(|target| format!("{root}/{}", target.rel))
            .collect();
        let undo = backup::create(root, "recover", &targets, retention).map_err(unrecoverable)?;
        backup::write_store_file(&self.store, ".latest", format!("{id}\n").as_bytes())?;
        backup::restore(root, &snapshot).map_err(unrecoverable)?;
        let _ = witness::seal(root, &snapshot);
        let _ = witness::seal(root, &undo);
        Ok(Some(Recovered {
            operation,
            backup: id,
            undo: paths::leaf(&undo),
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    #[test]
    fn dropping_the_lock_releases_it_while_a_forked_copy_of_the_descriptor_lives() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        let (lock, _) = Lock::acquire(&root, "rollback", backup::Retention::Bounded).unwrap();
        let inherited = lock.file.try_clone().unwrap();
        lock.finish().unwrap();
        drop(lock);

        assert!(Lock::acquire(&root, "rollback", backup::Retention::Bounded).is_ok());
        drop(inherited);
    }

    #[test]
    fn a_pending_record_reaches_the_terminal_with_its_control_characters_escaped() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        let (held, _) = Lock::acquire(&root, "sync", backup::Retention::Bounded).unwrap();
        std::fs::write(
            format!("{}/.pending", held.store),
            "operation=sync\x1b]0;owned\x07\npid=1\x1b[2J\nbackup=x\x1b[1m\n",
        )
        .unwrap();

        let busy = Lock::acquire(&root, "sync", backup::Retention::Bounded)
            .err()
            .unwrap()
            .to_string();
        assert!(!busy.contains('\x1b') && !busy.contains('\x07'), "{busy:?}");
        assert!(busy.contains(r"exuno sync\u{1b}]0;owned\u{7} (pid 1\u{1b}[2J)"));
        drop(held);

        let missing = Lock::acquire(&root, "sync", backup::Retention::Bounded)
            .err()
            .unwrap()
            .to_string();
        assert!(!missing.contains('\x1b'), "{missing:?}");
        assert!(missing.contains(r"backup x\u{1b}[1m missing"));
    }
}
