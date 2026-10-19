//! `.ai/.sync-manifest` as `lib/helpers/manifest.sh` reads and writes it: one
//! `<rel>\t<sha256>` line per output, `LC_ALL=C sort -u`, no header. A file
//! sync owns only some keys of adds a third column, the owned-key record, and
//! its hash covers those keys alone.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::engine::keyed::{self, Format, KeyPath, Owned};
use crate::output::log::Log;
use crate::{Error, engine::staging};

pub const REL: &str = ".ai/.sync-manifest";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub rel: String,
    pub hash: String,
    pub owned: Option<Owned>,
}

impl Entry {
    fn from_line(rel: String, rest: String) -> Self {
        if let Some((hash, column)) = rest.split_once('\t')
            && let Some(owned) = Owned::decode(column)
        {
            return Self {
                rel,
                hash: hash.to_string(),
                owned: Some(owned),
            };
        }
        Self {
            rel,
            hash: rest,
            owned: None,
        }
    }

    fn line(&self) -> String {
        match &self.owned {
            Some(owned) => format!("{}\t{}\t{}", self.rel, self.hash, owned.encode()),
            None => format!("{}\t{}", self.rel, self.hash),
        }
    }

    /// The hash the file has now, measured the way this entry was recorded;
    /// `None` when the file is gone. An owned-key file that no longer parses
    /// hashes as empty, which never matches.
    pub fn current_hash(&self, root: &str) -> Option<String> {
        let path = Path::new(root).join(&self.rel);
        match &self.owned {
            None => hash_file(&path),
            Some(owned) => {
                if !path.is_file() {
                    return None;
                }
                Some(
                    self.owned_now(root, owned)
                        .map(|now| now.digest())
                        .unwrap_or_default(),
                )
            }
        }
    }

    fn owned_now(&self, root: &str, owned: &Owned) -> Result<Owned, String> {
        let format = Format::of(&self.rel).ok_or("not a TOML or JSON file")?;
        let text = std::fs::read_to_string(Path::new(root).join(&self.rel)).unwrap_or_default();
        keyed::owned_in(format, &text, owned)
    }

    /// The owned keys whose value changed since the record; empty for a
    /// whole-file entry or a file that no longer parses.
    pub fn changed_keys(&self, root: &str) -> Vec<KeyPath> {
        let Some(owned) = &self.owned else {
            return Vec::new();
        };
        self.owned_now(root, owned)
            .map(|now| owned.changed(&now))
            .unwrap_or_default()
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    entries: Vec<Entry>,
}

impl Manifest {
    /// `manifest_load`: `None` when no manifest exists, which makes the run a
    /// baseline initialisation.
    pub fn load(root: &str) -> Result<Option<Self>, Error> {
        let path = Path::new(root).join(REL);
        if !path.is_file() {
            return Ok(None);
        }
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        Ok(Some(Self::parse(&bytes)))
    }

    /// The manifest's `hashed_lines`.
    pub fn parse(bytes: &[u8]) -> Self {
        Self {
            entries: hashed_lines(bytes)
                .into_iter()
                .map(|(rel, rest)| Entry::from_line(rel, rest))
                .collect(),
        }
    }

    pub fn paths(&self) -> BTreeSet<String> {
        self.entries.iter().map(|entry| entry.rel.clone()).collect()
    }

    /// `MANIFEST_KEYS` and `MANIFEST_VALUES`, in file order.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, rel: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.rel == rel)
    }

    /// The owned-key record of every entry that has one.
    pub fn owned_records(&self) -> BTreeMap<String, Owned> {
        self.entries
            .iter()
            .filter_map(|entry| Some((entry.rel.clone(), entry.owned.clone()?)))
            .collect()
    }

    /// `manifest_check_drift`: entries whose file exists with another hash, in
    /// manifest order. A missing file is not drift.
    pub fn drift(&self, root: &str) -> Vec<String> {
        self.entries
            .iter()
            .filter(|entry| {
                entry
                    .current_hash(root)
                    .is_some_and(|current| current != entry.hash)
            })
            .map(|entry| entry.rel.clone())
            .collect()
    }

    /// The drifted outputs `sync` refuses to overwrite without `--force`. A
    /// key-owned dest without an owned-key record yet is left to its merge
    /// step, which compares the declared keys instead; a record no enabled
    /// tool owns by key any more is left to the tool, or to the step that
    /// refuses to own it whole.
    pub fn manual_edits(&self, root: &str, keyed_dests: &BTreeSet<String>) -> Vec<ManualEdit> {
        self.drift(root)
            .into_iter()
            .filter_map(|rel| {
                let entry = self.entry(&rel)?;
                if entry.owned.is_some() != keyed_dests.contains(&rel) {
                    return None;
                }
                let keys = entry.changed_keys(root);
                Some(ManualEdit { rel, keys })
            })
            .collect()
    }
}

/// An output edited since the last sync, with the owned keys that changed
/// when sync owns only some of its keys.
#[derive(Debug, PartialEq, Eq)]
pub struct ManualEdit {
    pub rel: String,
    pub keys: Vec<KeyPath>,
}

/// `manifest_update_entry`: the manifest rewritten with `<rel>\t<hash>` in place
/// of any line for `rel`, every other line kept as `read` split it, `sort -u`.
/// An owned-key record rides along as the third column.
pub fn update_entry(root: &str, rel: &str, hash: &str, owned: Option<&Owned>) -> Result<(), Error> {
    if rel.is_empty() || hash.is_empty() {
        return Ok(());
    }
    let path = Path::new(root).join(REL);
    let ai = Path::new(root).join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    let mut lines = BTreeSet::new();
    if path.is_file() {
        let bytes = std::fs::read(&path).map_err(|e| Error::io(&path, e))?;
        for line in String::from_utf8_lossy(&bytes).split('\n') {
            let line = line.trim_matches('\t');
            let (existing, existing_hash) = match line.find('\t') {
                Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
                None => (line, ""),
            };
            if existing.is_empty() || existing == rel {
                continue;
            }
            lines.insert(format!("{existing}\t{existing_hash}"));
        }
    }
    let entry = Entry {
        rel: rel.to_string(),
        hash: hash.to_string(),
        owned: owned.cloned(),
    };
    lines.insert(entry.line());
    let mut text = lines.into_iter().collect::<Vec<_>>().join("\n");
    text.push('\n');
    staging::write_beside(&path, text.as_bytes())
}

/// `IFS=$'\t' read -r rel hash` per line: tabs around the line are dropped, the
/// hash is the rest after the first run of tabs, and comments and entries
/// without a hash are skipped.
pub(crate) fn hashed_lines(bytes: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(bytes);
    let mut entries = Vec::new();
    for line in text.split('\n') {
        let line = line.trim_matches('\t');
        let (rel, hash) = match line.find('\t') {
            Some(tab) => (&line[..tab], line[tab..].trim_start_matches('\t')),
            None => (line, ""),
        };
        if rel.is_empty() || rel.starts_with('#') || hash.is_empty() {
            continue;
        }
        entries.push((rel.to_string(), hash.to_string()));
    }
    entries
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn hash_file(path: &Path) -> Option<String> {
    if !path.is_file() {
        return None;
    }
    std::fs::read(path).ok().map(|bytes| sha256_hex(&bytes))
}

/// `manifest_write`: previous entries whose file still exists and this run did
/// not touch, plus fresh hashes of every touched file that exists, measured
/// over the owned keys alone where `owned` records them. An empty result
/// removes the manifest.
pub fn write(
    root: &str,
    previous: Option<&Manifest>,
    touched: &BTreeSet<String>,
    owned: &BTreeMap<String, Owned>,
    log: &mut Log,
) -> Result<(), Error> {
    let exists = |rel: &str| Path::new(root).join(rel).is_file();
    let mut lines: BTreeSet<String> = BTreeSet::new();
    for entry in previous.map(|m| m.entries.as_slice()).unwrap_or_default() {
        if exists(&entry.rel) && !touched.contains(&entry.rel) {
            lines.insert(entry.line());
        }
    }
    for rel in touched {
        let mut entry = Entry {
            rel: rel.clone(),
            hash: String::new(),
            owned: owned.get(rel).cloned(),
        };
        if let Some(hash) = entry.current_hash(root) {
            entry.hash = hash;
            lines.insert(entry.line());
        }
    }

    let path = Path::new(root).join(REL);
    if lines.is_empty() {
        if path.is_file() {
            std::fs::remove_file(&path).map_err(|e| Error::io(&path, e))?;
            log.info("Removed .ai/.sync-manifest (no tracked outputs)");
        }
        return Ok(());
    }
    let ai = Path::new(root).join(".ai");
    std::fs::create_dir_all(&ai).map_err(|e| Error::io(&ai, e))?;
    let mut text = lines
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    staging::write_beside(&path, text.as_bytes())?;
    if previous.is_none() {
        log.info(&format!(
            "Initialized .ai/.sync-manifest with {} entries — commit it to track drift in CI",
            lines.len()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::DiskText;

    #[test]
    fn digests_match_sha256sum() {
        assert_eq!(
            sha256_hex(b"hello\n"),
            "5891b5b522d5df086d0ff0b110fbd9d21bb4fc7163af34d08286a2e846f6be03"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn lines_are_read_the_way_bash_read_splits_them_on_tabs() {
        let manifest =
            Manifest::parse(b"a.md\th1\n\tb.md\th2\t\n#c\th\nd.md\n\ne.md\th\te\nlast\th9");
        let pairs: Vec<(&str, &str, bool)> = manifest
            .entries
            .iter()
            .map(|e| (e.rel.as_str(), e.hash.as_str(), e.owned.is_some()))
            .collect();
        assert_eq!(
            pairs,
            [
                ("a.md", "h1", false),
                ("b.md", "h2", false),
                ("e.md", "h\te", false),
                ("last", "h9", false),
            ]
        );
    }

    fn owned_record(settings: &str) -> Owned {
        keyed::merge(Format::Toml, "", settings, None)
            .unwrap()
            .owned
    }

    #[test]
    fn an_owned_key_record_rides_as_a_third_column() {
        let owned = owned_record("model = \"a\"\n");
        let line = format!("cfg.toml\tdigest\t{}\n", owned.encode());
        let manifest = Manifest::parse(line.as_bytes());
        assert_eq!(manifest.entries[0].hash, "digest");
        assert_eq!(manifest.entries[0].owned.as_ref(), Some(&owned));
        assert_eq!(manifest.entries[0].line() + "\n", line);
    }

    #[test]
    fn owned_key_drift_ignores_other_keys_and_names_a_changed_one() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::write(dir.path().join("cfg.toml"), "model = \"a\"\n").unwrap();
        let owned = owned_record("model = \"a\"\n");
        let touched = BTreeSet::from(["cfg.toml".to_string()]);
        let records = BTreeMap::from([("cfg.toml".to_string(), owned)]);
        write(&root, None, &touched, &records, &mut Log::capturing(false)).unwrap();
        let load = || Manifest::load(&root).unwrap().unwrap();

        std::fs::write(
            dir.path().join("cfg.toml"),
            "model = 'a'\n[projects.p]\ntrust = 1\n",
        )
        .unwrap();
        assert!(load().drift(&root).is_empty());

        std::fs::write(dir.path().join("cfg.toml"), "model = \"b\"\n").unwrap();
        assert_eq!(load().drift(&root), ["cfg.toml"]);
        assert_eq!(
            load().entry("cfg.toml").unwrap().changed_keys(&root),
            [vec!["model".to_string()]]
        );

        std::fs::write(dir.path().join("cfg.toml"), "model = \n").unwrap();
        assert_eq!(load().drift(&root), ["cfg.toml"]);
    }

    #[cfg(unix)]
    #[test]
    fn one_entry_is_replaced_and_every_other_line_is_kept_as_bash_reads_it() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join(".ai")).unwrap();
        std::fs::write(
            dir.path().join(REL),
            "z.md\tzz\n# note\t\nb.md\told\n\ta.md\t\taa\t\nnohash\nb.md\tdup\n",
        )
        .unwrap();
        update_entry(&root, "b.md", "new", None).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            "# note\t\na.md\taa\nb.md\tnew\nnohash\t\nz.md\tzz\n"
        );
        update_entry(&root, "", "x", None).unwrap();
        update_entry(&root, "c.md", "", None).unwrap();
        assert!(
            std::fs::read_to_string(dir.path().join(REL))
                .unwrap()
                .ends_with("z.md\tzz\n")
        );
    }

    #[cfg(unix)]
    #[test]
    fn drift_is_a_changed_file_in_manifest_order_and_a_missing_file_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::write(dir.path().join("b.md"), "edited\n").unwrap();
        std::fs::write(dir.path().join("a.md"), "hello\n").unwrap();
        std::fs::write(dir.path().join("c.md"), "edited\n").unwrap();
        let text = format!(
            "c.md\t{0}\nb.md\t{0}\na.md\t{0}\ngone.md\tx\n",
            sha256_hex(b"hello\n")
        );
        assert_eq!(
            Manifest::parse(text.as_bytes()).drift(&root),
            ["c.md", "b.md"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn writing_keeps_untouched_entries_hashes_touched_files_and_sorts_bytewise() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join(".claude")).unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "hello\n").unwrap();
        std::fs::write(dir.path().join(".claude/x.md"), "").unwrap();
        std::fs::write(dir.path().join("skipped.md"), "stale\n").unwrap();
        let previous = Manifest::parse(b"skipped.md\told\ngone.md\told\nCLAUDE.md\told\n");
        let touched = BTreeSet::from(["CLAUDE.md".to_string(), ".claude/x.md".to_string()]);

        let mut log = Log::default();
        write(&root, Some(&previous), &touched, &BTreeMap::new(), &mut log).unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join(REL)).unwrap(),
            format!(
                ".claude/x.md\t{}\nCLAUDE.md\t{}\nskipped.md\told\n",
                sha256_hex(b""),
                sha256_hex(b"hello\n")
            )
        );
        assert!(log.lines().is_empty());

        write(&root, None, &touched, &BTreeMap::new(), &mut log).unwrap();
        assert_eq!(
            log.tail(1),
            [
                "[INFO] Initialized .ai/.sync-manifest with 2 entries — commit it to track drift in CI"
            ]
        );

        std::fs::remove_file(dir.path().join("CLAUDE.md")).unwrap();
        std::fs::remove_file(dir.path().join(".claude/x.md")).unwrap();
        write(
            &root,
            Some(&previous),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut log,
        )
        .unwrap();
        std::fs::remove_file(dir.path().join("skipped.md")).unwrap();
        write(
            &root,
            Some(&previous),
            &BTreeSet::new(),
            &BTreeMap::new(),
            &mut log,
        )
        .unwrap();
        assert!(!dir.path().join(REL).exists());
        assert_eq!(
            log.tail(1),
            ["[INFO] Removed .ai/.sync-manifest (no tracked outputs)"]
        );
    }
}
