//! The file tree a render reads and writes.
//!
//! `lib/check.sh` copied `.ai/` and the manifest's outputs into a temporary
//! root with `tar` and ran `sync.sh` there. The in-memory workspace is that copy
//! without the copy: paths below the project root and below the virtual engine
//! and overlay roots are served from an index of disk paths, embedded templates,
//! and bytes written by the render. A workspace on disk reads and writes the
//! project itself, as `sync.sh` does, and keeps only the virtual roots in memory.

use crate::paths::DiskText;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::paths::{self, ENGINE_ROOT};
use crate::{Error, config::catalog};

#[derive(Clone, Debug)]
pub enum Content {
    Disk(PathBuf),
    Embedded(&'static [u8]),
    Bytes(Vec<u8>),
}

#[derive(Clone, Debug)]
enum Entry {
    Dir,
    File(Content),
}

#[derive(Debug)]
pub struct Workspace {
    root: String,
    on_disk: bool,
    entries: BTreeMap<String, Entry>,
}

impl Workspace {
    /// An empty in-memory tree rooted at `root`, with the embedded engine mounted.
    pub fn new(root: &str) -> Self {
        let mut ws = Self {
            root: root.to_string(),
            on_disk: false,
            entries: BTreeMap::new(),
        };
        ws.mkdir_entries(root);
        for (rel, bytes) in catalog::engine_files() {
            ws.insert_file(&format!("{ENGINE_ROOT}/{rel}"), Content::Embedded(bytes));
        }
        ws
    }

    /// The project at `root` read and written on disk; the embedded engine and
    /// the overlay trees stay in memory.
    pub fn on_disk(root: &str) -> Self {
        let mut ws = Self::new(root);
        ws.entries.retain(|path, _| paths::is_virtual(path));
        ws.on_disk = true;
        ws
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    fn indexed(&self, path: &str) -> bool {
        paths::is_virtual(path) || (!self.on_disk && paths::is_within(path, &self.root))
    }

    fn writes_disk(&self, path: &str) -> bool {
        self.on_disk && !paths::is_virtual(path)
    }

    /// Adds the disk tree at `disk` under `at`, skipping every path for which
    /// `skip` returns true, given its `/`-separated path relative to `disk`.
    pub fn seed_from_disk(
        &mut self,
        at: &str,
        disk: &Path,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<(), Error> {
        let meta = std::fs::metadata(disk).map_err(|e| Error::io(disk, e))?;
        if meta.is_file() {
            self.insert_file(at, Content::Disk(disk.to_path_buf()));
            return Ok(());
        }
        self.mkdir_entries(at);
        self.seed_dir(at, disk, "", skip)
    }

    fn seed_dir(
        &mut self,
        at: &str,
        disk: &Path,
        rel: &str,
        skip: &dyn Fn(&str) -> bool,
    ) -> Result<(), Error> {
        let entries = std::fs::read_dir(disk).map_err(|e| Error::io(disk, e))?;
        for entry in entries {
            let entry = entry.map_err(|e| Error::io(disk, e))?;
            let name = entry.file_name().disk_text();
            let child_rel = if rel.is_empty() {
                name.clone()
            } else {
                format!("{rel}/{name}")
            };
            if skip(&child_rel) {
                continue;
            }
            let child_disk = entry.path();
            let child_at = format!("{at}/{name}");
            let Ok(meta) = std::fs::metadata(&child_disk) else {
                continue;
            };
            if meta.is_dir() {
                self.entries.insert(child_at.clone(), Entry::Dir);
                self.seed_dir(&child_at, &child_disk, &child_rel, skip)?;
            } else {
                self.entries
                    .insert(child_at, Entry::File(Content::Disk(child_disk)));
            }
        }
        Ok(())
    }

    /// Adds a file to the in-memory index, creating its parents.
    pub fn insert_file(&mut self, path: &str, content: Content) {
        self.mkdir_entries(&paths::parent(path));
        self.entries.insert(path.to_string(), Entry::File(content));
    }

    fn mkdir_entries(&mut self, path: &str) {
        let mut current = path.to_string();
        while !matches!(self.entries.get(&current), Some(Entry::Dir)) {
            self.entries.insert(current.clone(), Entry::Dir);
            let up = paths::parent(&current);
            if up == current {
                break;
            }
            current = up;
        }
    }

    /// `mkdir -p`.
    pub fn create_dir_all(&mut self, path: &str) -> Result<(), Error> {
        if self.writes_disk(path) {
            return std::fs::create_dir_all(path).map_err(|e| Error::io(path, e));
        }
        self.mkdir_entries(path);
        Ok(())
    }

    pub fn is_file(&self, path: &str) -> bool {
        if self.indexed(path) {
            matches!(self.entries.get(path), Some(Entry::File(_)))
        } else {
            Path::new(path).is_file()
        }
    }

    pub fn is_dir(&self, path: &str) -> bool {
        if self.indexed(path) {
            matches!(self.entries.get(path), Some(Entry::Dir))
        } else {
            Path::new(path).is_dir()
        }
    }

    pub fn exists(&self, path: &str) -> bool {
        self.is_file(path) || self.is_dir(path)
    }

    pub fn content(&self, path: &str) -> Option<&Content> {
        match self.entries.get(path) {
            Some(Entry::File(content)) => Some(content),
            _ => None,
        }
    }

    pub fn read(&self, path: &str) -> Result<Vec<u8>, Error> {
        if !self.indexed(path) {
            return std::fs::read(path).map_err(|e| Error::io(path, e));
        }
        match self.entries.get(path) {
            Some(Entry::File(Content::Disk(disk))) => {
                std::fs::read(disk).map_err(|e| Error::io(disk, e))
            }
            Some(Entry::File(Content::Embedded(bytes))) => Ok(bytes.to_vec()),
            Some(Entry::File(Content::Bytes(bytes))) => Ok(bytes.clone()),
            _ => Err(not_found(path)),
        }
    }

    /// Entry names directly inside `dir` in byte order, dotfiles included.
    pub fn list(&self, dir: &str) -> Vec<String> {
        if !self.indexed(dir) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return Vec::new();
            };
            let mut names: Vec<String> = entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().disk_text())
                .collect();
            names.sort();
            return names;
        }
        let prefix = if dir == "/" {
            "/".to_string()
        } else {
            format!("{dir}/")
        };
        self.entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .filter_map(|(key, _)| {
                let rest = &key[prefix.len()..];
                (!rest.is_empty() && !rest.contains('/')).then(|| rest.to_string())
            })
            .collect()
    }

    /// Names a Bash `"$dir"/*` glob yields: no dotfiles.
    pub fn glob(&self, dir: &str) -> Vec<String> {
        self.list(dir)
            .into_iter()
            .filter(|name| !name.starts_with('.'))
            .collect()
    }

    /// Regular files below `dir` at any depth, as `find "$dir" -type f` lists them.
    pub fn files_under(&self, dir: &str) -> Vec<String> {
        let mut found = Vec::new();
        for name in self.list(dir) {
            let child = format!("{dir}/{name}");
            if self.is_dir(&child) {
                found.extend(self.files_under(&child));
            } else if self.is_file(&child) {
                found.push(child);
            }
        }
        found
    }

    /// `>`: the parent directory must exist.
    pub fn write(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), Error> {
        if self.writes_disk(path) {
            return std::fs::write(path, bytes).map_err(|e| Error::io(path, e));
        }
        if !self.is_dir(&paths::parent(path)) || self.is_dir(path) {
            return Err(not_found(path));
        }
        self.entries
            .insert(path.to_string(), Entry::File(Content::Bytes(bytes)));
        Ok(())
    }

    pub fn replace_atomically(&mut self, path: &str, bytes: Vec<u8>) -> Result<(), Error> {
        if self.writes_disk(path) {
            return crate::engine::staging::write_beside(Path::new(path), &bytes);
        }
        self.write(path, bytes)
    }

    /// `>>`: creates the file when missing, the parent directory must exist.
    pub fn append(&mut self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        if self.writes_disk(path) {
            return std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .and_then(|mut file| file.write_all(bytes))
                .map_err(|e| Error::io(path, e));
        }
        let mut current = if self.is_file(path) {
            self.read(path)?
        } else {
            Vec::new()
        };
        current.extend_from_slice(bytes);
        self.write(path, current)
    }

    /// `rm -rf`.
    pub fn remove(&mut self, path: &str) -> Result<(), Error> {
        if self.writes_disk(path) {
            let removed = match std::fs::symlink_metadata(path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(e) => Err(e),
                Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
                Ok(_) => std::fs::remove_file(path),
            };
            return removed.map_err(|e| Error::io(path, e));
        }
        let prefix = format!("{path}/");
        let doomed: Vec<String> = self
            .entries
            .range(prefix.clone()..)
            .take_while(|(key, _)| key.starts_with(&prefix))
            .map(|(key, _)| key.clone())
            .collect();
        for key in doomed {
            self.entries.remove(&key);
        }
        self.entries.remove(path);
        Ok(())
    }

    /// `chmod +x` as the default umask applies it; the in-memory tree has no modes.
    #[cfg(unix)]
    pub fn make_executable(&mut self, path: &str) -> Result<(), Error> {
        if self.writes_disk(path) {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(path)
                .map_err(|e| Error::io(path, e))?
                .permissions();
            permissions.set_mode(permissions.mode() | 0o111);
            return std::fs::set_permissions(path, permissions).map_err(|e| Error::io(path, e));
        }
        Ok(())
    }

    #[cfg(not(unix))]
    pub fn make_executable(&mut self, _path: &str) -> Result<(), Error> {
        Ok(())
    }

    /// `cp -r src dst` onto a missing `dst`: a file, or a whole tree with its
    /// empty directories.
    pub fn copy(&mut self, src: &str, dst: &str) -> Result<(), Error> {
        if self.is_file(src) {
            let content = self.content_of(src)?;
            if self.writes_disk(dst) {
                return match content {
                    Content::Disk(disk) => std::fs::copy(&disk, dst).map(|_| ()),
                    Content::Embedded(bytes) => std::fs::write(dst, bytes),
                    Content::Bytes(bytes) => std::fs::write(dst, bytes),
                }
                .map_err(|e| Error::io(dst, e));
            }
            if !self.is_dir(&paths::parent(dst)) {
                return Err(not_found(dst));
            }
            self.entries.insert(dst.to_string(), Entry::File(content));
            return Ok(());
        }
        if !self.is_dir(src) {
            return Err(not_found(src));
        }
        self.create_dir_all(dst)?;
        for name in self.list(src) {
            self.copy(&format!("{src}/{name}"), &format!("{dst}/{name}"))?;
        }
        Ok(())
    }

    fn content_of(&self, path: &str) -> Result<Content, Error> {
        if !self.indexed(path) {
            return Ok(Content::Disk(PathBuf::from(path)));
        }
        self.content(path).cloned().ok_or_else(|| not_found(path))
    }
}

fn not_found(path: &str) -> Error {
    Error::io(
        path,
        std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ws() -> Workspace {
        let mut ws = Workspace::new("/proj");
        ws.insert_file(
            "/proj/.ai/src/rules/core.md",
            Content::Bytes(b"# Core\n".to_vec()),
        );
        ws.insert_file("/proj/.ai/src/rules/.hidden.md", Content::Bytes(Vec::new()));
        ws.create_dir_all("/proj/.ai/src/skills/empty").unwrap();
        ws
    }

    #[test]
    fn inserting_a_file_creates_its_parents() {
        let ws = ws();
        assert!(ws.is_dir("/proj/.ai/src"));
        assert!(ws.is_file("/proj/.ai/src/rules/core.md"));
        assert!(!ws.is_file("/proj/.ai/src/rules"));
    }

    #[test]
    fn listing_is_immediate_children_in_byte_order_and_glob_drops_dotfiles() {
        let ws = ws();
        assert_eq!(ws.list("/proj/.ai/src/rules"), [".hidden.md", "core.md"]);
        assert_eq!(ws.glob("/proj/.ai/src/rules"), ["core.md"]);
        assert_eq!(ws.list("/proj/.ai/src"), ["rules", "skills"]);
    }

    #[test]
    fn write_needs_a_parent_and_append_creates_the_file() {
        let mut ws = ws();
        assert!(ws.write("/proj/missing/x.md", b"x".to_vec()).is_err());
        ws.create_dir_all("/proj/out").unwrap();
        ws.append("/proj/out/a.md", b"one\n").unwrap();
        ws.append("/proj/out/a.md", b"two\n").unwrap();
        assert_eq!(ws.read("/proj/out/a.md").unwrap(), b"one\ntwo\n");
    }

    #[test]
    fn copy_brings_empty_directories_and_remove_takes_the_subtree() {
        let mut ws = ws();
        ws.copy("/proj/.ai/src", "/proj/copy").unwrap();
        assert!(ws.is_dir("/proj/copy/skills/empty"));
        assert!(ws.is_file("/proj/copy/rules/core.md"));
        assert_eq!(ws.files_under("/proj/copy").len(), 2);
        ws.remove("/proj/copy/rules").unwrap();
        assert!(!ws.exists("/proj/copy/rules/core.md"));
        assert!(ws.is_dir("/proj/copy/skills"));
    }

    #[test]
    fn the_engine_templates_are_mounted_under_the_virtual_root() {
        let ws = Workspace::new("/proj");
        assert!(ws.is_file("/<agentsync>/lib/templates/settings/claude.json"));
        assert!(ws.is_file("/<agentsync>/lib/config.yaml"));
        assert!(ws.is_dir("/<agentsync>/lib/templates/base-src/skills/exuno"));
    }

    #[cfg(unix)]
    #[test]
    fn seeding_from_disk_honours_the_skip_filter() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("src/rules")).unwrap();
        std::fs::create_dir_all(dir.path().join("backups/x")).unwrap();
        std::fs::write(dir.path().join("src/rules/core.md"), "c").unwrap();
        let mut ws = Workspace::new("/proj");
        ws.seed_from_disk("/proj/.ai", dir.path(), &|rel| rel == "backups")
            .unwrap();
        assert_eq!(ws.read("/proj/.ai/src/rules/core.md").unwrap(), b"c");
        assert!(!ws.exists("/proj/.ai/backups"));
    }

    #[cfg(unix)]
    #[test]
    fn a_workspace_on_disk_writes_the_project_and_keeps_the_engine_in_memory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join(".ai/src/skills/a/scripts")).unwrap();
        let script = dir.path().join(".ai/src/skills/a/scripts/run.sh");
        std::fs::write(&script, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();

        let mut ws = Workspace::on_disk(&root);
        assert!(ws.is_file("/<agentsync>/lib/config.yaml"));
        assert!(!ws.exists(&format!("{root}/CLAUDE.md")));

        ws.copy(
            "/<agentsync>/lib/templates/settings/claude.json",
            &format!("{root}/settings.json"),
        )
        .unwrap();
        assert!(dir.path().join("settings.json").is_file());

        ws.copy(
            &format!("{root}/.ai/src/skills/a"),
            &format!("{root}/.claude/skills/a"),
        )
        .unwrap();
        let copied = dir.path().join(".claude/skills/a/scripts/run.sh");
        assert_eq!(
            std::fs::metadata(&copied).unwrap().permissions().mode() & 0o777,
            0o755
        );

        ws.append(&format!("{root}/.claude/skills/a/x.md"), b"1\n")
            .unwrap();
        ws.append(&format!("{root}/.claude/skills/a/x.md"), b"2\n")
            .unwrap();
        assert_eq!(
            ws.read(&format!("{root}/.claude/skills/a/x.md")).unwrap(),
            b"1\n2\n"
        );

        ws.remove(&format!("{root}/.claude")).unwrap();
        ws.remove(&format!("{root}/.claude")).unwrap();
        assert!(!dir.path().join(".claude").exists());
        assert!(ws.write(&format!("{root}/missing/x"), Vec::new()).is_err());
    }
}
