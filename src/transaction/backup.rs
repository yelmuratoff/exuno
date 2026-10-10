//! Transactional backups of `lib/helpers/backup.sh`, in its on-disk layout:
//! `.ai/backups/<UTC stamp>-<operation>-<pid>[-n]/` holding `metadata`,
//! `targets.tsv`, a `files/` mirror, and `.complete`; the store keeps `.latest`
//! and a `.gitignore` of `*`. A Bash `rollback` reads what this writes.

use crate::paths::DiskText;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::{Error, config::yaml_subset, paths};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub present: bool,
    pub rel: String,
}

fn refuse(message: impl Into<String>) -> Error {
    Error::Backup(message.into())
}

/// `backup.retention`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Retention {
    #[default]
    Bounded,
    Preserve,
}

/// `backup_configure` once the project config is selected: the retention
/// policy in `config` (`(path, text)`, when there is one), then the bounds.
pub fn configure(
    config: Option<(&str, &str)>,
    limit: Option<&str>,
    max_age: Option<&str>,
) -> Result<Retention, Error> {
    let mut retention = Retention::Bounded;
    if let Some((path, text)) = config {
        if !yaml_subset::value(text, "backup").is_empty() {
            return Err(refuse(format!(
                "backup must be a mapping with backup.retention: bounded or preserve in {path}"
            )));
        }
        if let Some(value) = yaml_subset::found(text, "backup.retention") {
            retention = match value.as_str() {
                "bounded" => Retention::Bounded,
                "preserve" => Retention::Preserve,
                other => {
                    let shown = if other.is_empty() { "<empty>" } else { other };
                    return Err(refuse(format!(
                        "Invalid backup.retention '{shown}' in {path}; expected bounded or preserve"
                    )));
                }
            };
        }
    }
    validate_limits(limit, max_age)?;
    Ok(retention)
}

/// `_backup_validate_limits` with `:-10` and `:-30` applied; the parsed bounds.
fn validate_limits(limit: Option<&str>, max_age: Option<&str>) -> Result<(u64, u64), Error> {
    let limit = limit.filter(|v| !v.is_empty()).unwrap_or("10");
    let max_age = max_age.filter(|v| !v.is_empty()).unwrap_or("30");
    let digits = |v: &str| v.bytes().all(|b| b.is_ascii_digit());
    if !digits(limit) {
        return Err(refuse(format!(
            "Backup limit must be a non-negative integer: {limit}"
        )));
    }
    if !digits(max_age) {
        return Err(refuse(format!(
            "Backup max age must be a non-negative integer: {max_age}"
        )));
    }
    Ok((
        limit.parse().unwrap_or(u64::MAX),
        max_age.parse().unwrap_or(u64::MAX),
    ))
}

/// `_backup_canonical_root`.
pub fn canonical_root(root: &str) -> Result<String, Error> {
    if !Path::new(root).is_dir() {
        return Err(refuse(format!("Backup root is not a directory: {root}")));
    }
    canonical_dir(Path::new(root)).map_err(|e| Error::io(root, e))
}

fn canonical_dir(dir: &Path) -> std::io::Result<String> {
    std::fs::canonicalize(dir).map(|p| paths::from_disk(&p))
}

fn exists_or_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok()
}

/// `_backup_validate_rel`.
pub(crate) fn validate_rel(rel: &str, allow_store_parent: bool) -> Result<(), Error> {
    if rel.is_empty() || rel == "." || crate::paths::is_absolute(rel) {
        let shown = if rel.is_empty() { "<empty>" } else { rel };
        return Err(refuse(format!("Refusing unsafe backup target: {shown}")));
    }
    let wrapped = format!("/{rel}/");
    if wrapped.contains("/../") || wrapped.contains("/./") {
        return Err(refuse(format!(
            "Refusing non-normalized backup target: {rel}"
        )));
    }
    if rel == ".ai" && !allow_store_parent {
        return Err(refuse(format!(
            "Refusing to back up a parent of the backup store: {rel}"
        )));
    }
    if rel == ".ai/backups" || rel.starts_with(".ai/backups/") {
        return Err(refuse(format!(
            "Refusing to back up the backup store itself: {rel}"
        )));
    }
    if rel.contains(['\t', '\n', '\r']) {
        return Err(refuse("Backup targets cannot contain tabs or newlines"));
    }
    Ok(())
}

/// `_backup_safe_target_path_r`: `<canonical_root>/<rel>`, once its nearest
/// existing ancestor is a directory inside the root.
pub(crate) fn safe_target_path(
    canonical_root: &str,
    rel: &str,
    allow_store_parent: bool,
) -> Result<String, Error> {
    validate_rel(rel, allow_store_parent)?;
    let abs = format!("{canonical_root}/{rel}");
    let mut probe = paths::parent(&abs);
    while !exists_or_link(Path::new(&probe)) {
        let up = paths::parent(&probe);
        if up == probe {
            break;
        }
        probe = up;
    }
    if !Path::new(&probe).is_dir() {
        let blocker = probe
            .strip_prefix(&format!("{canonical_root}/"))
            .unwrap_or(&probe);
        return Err(refuse(format!(
            "{blocker} is a file where {rel} needs a directory; move or rename it, then run the command again"
        )));
    }
    let resolved = canonical_dir(Path::new(&probe))
        .map_err(|_| refuse(format!("Could not resolve backup target parent: {rel}")))?;
    if !paths::is_within(&resolved, canonical_root) {
        return Err(refuse(format!(
            "Backup target resolves outside the repository root: {rel}"
        )));
    }
    Ok(abs)
}

/// `_backup_target_abs_r`.
fn target_abs(supplied_root: &str, canonical_root: &str, target: &str) -> Result<String, Error> {
    if target == supplied_root || target == canonical_root {
        return Err(refuse("Refusing to back up the repository root"));
    }
    let rel = if let Some(rest) = target.strip_prefix(&format!("{supplied_root}/")) {
        rest
    } else if let Some(rest) = target.strip_prefix(&format!("{canonical_root}/")) {
        rest
    } else if !crate::paths::is_absolute(target) {
        target
    } else {
        return Err(refuse(format!(
            "Backup target is outside the repository root: {target}"
        )));
    };
    validate_rel(rel, false)?;
    safe_target_path(canonical_root, rel, false)
}

/// `_backup_prepare_targets`: validated, deduplicated, and collapsed into their
/// shallowest roots, in first-seen order.
fn prepare_targets(supplied_root: &str, targets: &[String]) -> Result<Vec<String>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let mut prepared: Vec<String> = Vec::new();
    for target in targets {
        let candidate = target_abs(supplied_root, &canonical, target)?;
        let covered = prepared
            .iter()
            .any(|existing| paths::is_within(&candidate, existing));
        prepared.retain(|existing| !existing.starts_with(&format!("{candidate}/")));
        if !covered {
            prepared.push(candidate);
        }
    }
    if prepared.is_empty() {
        return Err(refuse("No backup targets were provided"));
    }
    Ok(prepared)
}

/// `_backup_validate_store`.
fn validate_store(canonical_root: &str) -> Result<String, Error> {
    safe_target_path(canonical_root, ".ai", true)?;
    let ai = PathBuf::from(format!("{canonical_root}/.ai"));
    let store = PathBuf::from(format!("{canonical_root}/.ai/backups"));
    if ai.is_symlink() {
        return Err(refuse("Exuno state directory cannot be a symlink: .ai"));
    }
    if ai.exists() && !ai.is_dir() {
        return Err(refuse("Exuno state path is not a directory: .ai"));
    }
    if ai.is_dir() && canonical_dir(&ai).ok().as_deref() != Some(&format!("{canonical_root}/.ai")) {
        return Err(refuse(
            "Exuno state directory resolves outside the repository root",
        ));
    }
    if store.is_symlink() {
        return Err(refuse("Backup store cannot be a symlink: .ai/backups"));
    }
    if store.exists() && !store.is_dir() {
        return Err(refuse("Backup store is not a directory: .ai/backups"));
    }
    if store.is_dir()
        && canonical_dir(&store).ok().as_deref() != Some(&format!("{canonical_root}/.ai/backups"))
    {
        return Err(refuse("Backup store resolves outside the repository root"));
    }
    Ok(store.disk_text())
}

/// The validated backup store, created with its `.gitignore` of `*`.
pub(crate) fn open_store(supplied_root: &str) -> Result<String, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    std::fs::create_dir_all(&store).map_err(|e| Error::io(&store, e))?;
    let store = validate_store(&canonical)?;
    write_store_file(&store, ".gitignore", b"*\n")?;
    Ok(store)
}

/// `mktemp "$store/<prefix>XXXXXX"` then `mv` onto `<store>/<name>`: a
/// symlink at either path is replaced, never followed.
pub(crate) fn write_store_file(store: &str, name: &str, bytes: &[u8]) -> Result<(), Error> {
    let staging = create_unique(
        store,
        &format!(".{}.tmp.", name.trim_start_matches('.')),
        false,
    )?;
    let written = OpenOptions::new()
        .write(true)
        .open(&staging)
        .and_then(|mut file| file.write_all(bytes))
        .and_then(|()| std::fs::rename(&staging, format!("{store}/{name}")));
    if let Err(e) = written {
        let _ = std::fs::remove_file(&staging);
        return Err(Error::io(&staging, e));
    }
    Ok(())
}

/// `mktemp [-d] "<dir>/<prefix>XXXXXX"`: created exclusively, mode `0600` or `0700`.
pub(crate) fn create_unique(dir: &str, prefix: &str, directory: bool) -> Result<PathBuf, Error> {
    let pid = std::process::id();
    let mut attempt = 0u64;
    loop {
        let path = PathBuf::from(format!("{dir}/{prefix}{pid}{attempt:04}"));
        let created = if directory {
            #[cfg(unix)]
            let builder = {
                let mut builder = std::fs::DirBuilder::new();
                std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
                builder
            };
            #[cfg(not(unix))]
            let builder = std::fs::DirBuilder::new();
            builder.create(&path)
        } else {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
            options.open(&path).map(|_| ())
        };
        match created {
            Ok(()) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => attempt += 1,
            Err(e) => return Err(Error::io(&path, e)),
        }
    }
}

/// `_backup_sweep_stale_staging`: staging older than a day, left by a run that
/// died before its `mv`.
fn sweep_stale_staging(store: &str, now: SystemTime, retention: Retention) {
    if retention == Retention::Preserve {
        return;
    }
    let Ok(entries) = std::fs::read_dir(store) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().disk_text();
        if !(name.starts_with(".tmp.")
            || name.starts_with(".latest.tmp.")
            || name.starts_with(".gitignore.tmp.")
            || name.starts_with(".pending.tmp."))
        {
            continue;
        }
        let stale = std::fs::symlink_metadata(entry.path())
            .and_then(|meta| meta.modified())
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= Duration::from_secs(24 * 60 * 60));
        if stale {
            let _ = remove_all(&entry.path());
        }
    }
}

fn remove_all(path: &Path) -> std::io::Result<()> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
    }
}

/// `cp -pPR src dst` and the `tar` pipe: links stay links; modes and
/// modification times are kept.
fn copy_preserving(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.is_symlink() {
        let link = std::fs::read_link(src)?;
        #[cfg(unix)]
        return std::os::unix::fs::symlink(link, dst);
        #[cfg(windows)]
        return if std::fs::metadata(src).is_ok_and(|m| m.is_dir()) {
            std::os::windows::fs::symlink_dir(link, dst)
        } else {
            std::os::windows::fs::symlink_file(link, dst)
        };
    }
    if meta.is_dir() {
        std::fs::create_dir(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            copy_preserving(&entry.path(), &dst.join(entry.file_name()))?;
        }
        std::fs::set_permissions(dst, meta.permissions())?;
        #[cfg(unix)]
        std::fs::File::open(dst)?.set_modified(meta.modified()?)?;
        return Ok(());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let kind = meta.file_type();
        if kind.is_fifo() {
            let made = std::process::Command::new("mkfifo").arg(dst).status()?;
            if !made.success() {
                return Err(std::io::Error::other(format!(
                    "mkfifo failed for {}",
                    dst.display()
                )));
            }
            return std::fs::set_permissions(dst, meta.permissions());
        }
        if kind.is_socket() || kind.is_block_device() || kind.is_char_device() {
            return Ok(());
        }
    }
    std::fs::copy(src, dst)?;
    // Windows refuses `set_modified` on a read-only handle.
    OpenOptions::new()
        .write(true)
        .open(dst)?
        .set_modified(meta.modified()?)
}

/// `backup_create`: the snapshot's path.
pub fn create(
    supplied_root: &str,
    operation: &str,
    targets: &[String],
    retention: Retention,
) -> Result<String, Error> {
    create_at(
        supplied_root,
        operation,
        targets,
        SystemTime::now(),
        retention,
    )
}

fn create_at(
    supplied_root: &str,
    operation: &str,
    targets: &[String],
    now: SystemTime,
    retention: Retention,
) -> Result<String, Error> {
    let valid_operation = operation
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_lowercase())
        && operation
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if !valid_operation {
        return Err(refuse(format!(
            "Invalid backup operation name: {operation}"
        )));
    }
    let canonical = canonical_root(supplied_root)?;
    let prepared = prepare_targets(supplied_root, targets)?;
    let store = open_store(supplied_root)?;
    sweep_stale_staging(&store, now, retention);

    let stage = create_unique(
        &store,
        &format!(".tmp.{operation}.{}.", std::process::id()),
        true,
    )?;
    let id = match stage_snapshot(&stage, &canonical, &prepared, operation, now) {
        Ok(id) => id,
        Err(e) => {
            let _ = remove_all(&stage);
            return Err(e);
        }
    };
    let mut snapshot_id = id.clone();
    let mut counter = 1;
    while Path::new(&format!("{store}/{snapshot_id}")).exists() {
        counter += 1;
        snapshot_id = format!("{id}-{counter}");
    }
    let snapshot = format!("{store}/{snapshot_id}");
    if let Err(e) = std::fs::rename(&stage, &snapshot) {
        let _ = remove_all(&stage);
        return Err(Error::io(&snapshot, e));
    }
    if let Err(e) = write_store_file(&store, ".latest", format!("{snapshot_id}\n").as_bytes()) {
        let _ = remove_all(Path::new(&snapshot));
        return Err(e);
    }
    Ok(snapshot)
}

fn stage_snapshot(
    stage: &Path,
    canonical_root: &str,
    prepared: &[String],
    operation: &str,
    now: SystemTime,
) -> Result<String, Error> {
    let io = |path: &Path| {
        let path = path.to_path_buf();
        move |e| Error::io(path, e)
    };
    let files = stage.join("files");
    std::fs::create_dir(&files).map_err(io(&files))?;
    let created = utc_stamp(now);
    let metadata = stage.join("metadata");
    std::fs::write(
        &metadata,
        format!("schema=1\noperation={operation}\ncreated_at={created}\n"),
    )
    .map_err(io(&metadata))?;

    let mut records = String::new();
    for abs in prepared {
        let rel = abs
            .strip_prefix(&format!("{canonical_root}/"))
            .unwrap_or(abs);
        let source = Path::new(abs);
        let present = exists_or_link(source);
        records.push_str(if present { "present\t" } else { "missing\t" });
        records.push_str(rel);
        records.push('\n');
        if present {
            let dest = files.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).map_err(io(parent))?;
            }
            copy_preserving(source, &dest).map_err(io(source))?;
        }
    }
    let targets = stage.join("targets.tsv");
    std::fs::write(&targets, records).map_err(io(&targets))?;
    let rels: Vec<&str> = prepared
        .iter()
        .map(|abs| {
            abs.strip_prefix(&format!("{canonical_root}/"))
                .unwrap_or(abs)
        })
        .collect();
    let folders = stage.join("dirs.tsv");
    std::fs::write(&folders, folder_records(canonical_root, &rels)).map_err(io(&folders))?;
    let complete = stage.join(".complete");
    std::fs::write(&complete, "").map_err(io(&complete))?;
    Ok(format!("{created}-{operation}-{}", std::process::id()))
}

/// A folder above a target: whether it existed, and its mode where the
/// platform has one.
#[derive(Debug, PartialEq, Eq)]
struct Folder {
    present: bool,
    mode: Option<u32>,
    rel: String,
}

/// Every folder between the root and `rel`, shallowest first.
fn ancestors(rel: &str) -> impl Iterator<Item = &str> {
    rel.match_indices('/').map(|(end, _)| &rel[..end])
}

/// The `dirs.tsv` text: one `present\t<mode>\t<rel>` or `missing\t-\t<rel>`
/// per folder above a target, a parent before its children. A link or a file
/// in a folder's place is left out, since restore never recreates it.
fn folder_records(canonical_root: &str, rels: &[&str]) -> String {
    let mut seen: Vec<&str> = Vec::new();
    let mut records = String::new();
    for folder in rels.iter().flat_map(|rel| ancestors(rel)) {
        if seen.contains(&folder) {
            continue;
        }
        seen.push(folder);
        match std::fs::symlink_metadata(format!("{canonical_root}/{folder}")) {
            Ok(meta) if meta.is_dir() => {
                records.push_str(&format!("present\t{}\t{folder}\n", mode_text(&meta)));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                records.push_str(&format!("missing\t-\t{folder}\n"));
            }
            _ => {}
        }
    }
    records
}

#[cfg(unix)]
fn mode_text(meta: &std::fs::Metadata) -> String {
    use std::os::unix::fs::PermissionsExt;
    format!("{:04o}", meta.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn mode_text(_meta: &std::fs::Metadata) -> String {
    "-".to_string()
}

/// The folder records of `<snapshot>/dirs.tsv`, each above a recorded target;
/// none for a snapshot taken before the file existed.
fn load_folders(snapshot: &str, targets: &[Target]) -> Result<Vec<Folder>, Error> {
    let tsv = format!("{snapshot}/dirs.tsv");
    let path = Path::new(&tsv);
    if !exists_or_link(path) {
        return Ok(Vec::new());
    }
    if path.is_symlink() || !path.is_file() {
        return Err(refuse("Snapshot folder list is unsafe"));
    }
    let bytes = std::fs::read(path).map_err(|e| Error::io(&tsv, e))?;
    let mut folders = Vec::new();
    for line in String::from_utf8_lossy(&bytes).split('\n') {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        let [state, mode, rel] = fields.as_slice() else {
            return Err(refuse(format!("Invalid folder record in snapshot: {line}")));
        };
        let mode = match (*state, *mode) {
            ("present" | "missing", "-") => Some(None),
            ("present", digits) => u32::from_str_radix(digits, 8)
                .ok()
                .filter(|mode| *mode <= 0o7777)
                .map(Some),
            _ => None,
        };
        let above_a_target = targets
            .iter()
            .any(|target| target.rel.starts_with(&format!("{rel}/")));
        let (Some(mode), true) = (mode, above_a_target) else {
            return Err(refuse(format!("Invalid folder record in snapshot: {rel}")));
        };
        validate_rel(rel, true)?;
        folders.push(Folder {
            present: *state == "present",
            mode,
            rel: rel.to_string(),
        });
    }
    Ok(folders)
}

/// Puts a recorded folder back in its recorded state once the targets are
/// restored: its mode when it existed, gone when it did not and is empty.
fn settle_folder(canonical_root: &str, folder: &Folder) -> Result<(), Error> {
    let path = safe_target_path(canonical_root, &folder.rel, true)?;
    let is_folder = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.is_dir());
    if !is_folder {
        return Ok(());
    }
    if !folder.present {
        return match std::fs::remove_dir(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::DirectoryNotEmpty => Err(Error::io(&path, e)),
            _ => Ok(()),
        };
    }
    #[cfg(unix)]
    if let Some(mode) = folder.mode {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .map_err(|e| Error::io(&path, e))?;
    }
    Ok(())
}

/// `_backup_snapshot_path`.
pub fn snapshot_path(supplied_root: &str, requested: &str) -> Result<String, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    let id = paths::leaf(requested);
    if id.is_empty() || id == "." || id == ".." {
        return Err(refuse(format!("Invalid backup snapshot: {requested}")));
    }
    let snapshot = format!("{store}/{id}");
    let path = Path::new(&snapshot);
    if path.is_symlink()
        || !path.is_dir()
        || !path.join(".complete").is_file()
        || path.join(".complete").is_symlink()
        || !path.join("targets.tsv").is_file()
        || path.join("targets.tsv").is_symlink()
    {
        return Err(refuse(format!(
            "Backup snapshot is missing or incomplete: {id}"
        )));
    }
    if canonical_dir(path).ok().as_deref() != Some(snapshot.as_str()) {
        return Err(refuse(format!(
            "Backup snapshot resolves outside the backup store: {id}"
        )));
    }
    Ok(snapshot)
}

/// `_backup_snapshot_source`.
fn snapshot_source(snapshot: &str, rel: &str) -> Result<String, Error> {
    let files_root = format!("{snapshot}/files");
    let files = Path::new(&files_root);
    if files.is_symlink()
        || !files.is_dir()
        || canonical_dir(files).ok().as_deref() != Some(files_root.as_str())
    {
        return Err(refuse("Snapshot files directory is unsafe"));
    }
    let source = format!("{files_root}/{rel}");
    let mut probe = paths::parent(&source);
    while !exists_or_link(Path::new(&probe)) {
        let up = paths::parent(&probe);
        if up == probe {
            break;
        }
        probe = up;
    }
    if !Path::new(&probe).is_dir() {
        return Err(refuse(format!(
            "Snapshot source parent is not a directory: {rel}"
        )));
    }
    let resolved = canonical_dir(Path::new(&probe)).map_err(|e| Error::io(&probe, e))?;
    if !paths::is_within(&resolved, &files_root) {
        return Err(refuse(format!(
            "Snapshot source resolves outside the backup store: {rel}"
        )));
    }
    Ok(source)
}

/// `backup_load_targets`: the validated records of `targets.tsv`, in order.
pub fn load_targets(supplied_root: &str, requested: &str) -> Result<Vec<Target>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let snapshot = snapshot_path(supplied_root, requested)?;
    let tsv = format!("{snapshot}/targets.tsv");
    let bytes = std::fs::read(&tsv).map_err(|e| Error::io(&tsv, e))?;
    let text = String::from_utf8_lossy(&bytes);
    let mut targets = Vec::new();
    for line in text.split('\n') {
        let fields = line.trim_matches('\t');
        let (state, rest) = fields.split_once('\t').unwrap_or((fields, ""));
        let rest = rest.trim_start_matches('\t');
        let (rel, extra) = rest.split_once('\t').unwrap_or((rest, ""));
        let extra = extra.trim_start_matches('\t');
        if state.is_empty() && rel.is_empty() && extra.is_empty() {
            continue;
        }
        if state != "present" && state != "missing" {
            return Err(refuse(format!("Invalid target state in snapshot: {state}")));
        }
        if !extra.is_empty() {
            return Err(refuse(format!("Invalid target record in snapshot: {rel}")));
        }
        validate_rel(rel, false)?;
        let present = state == "present";
        if present {
            let source = snapshot_source(&snapshot, rel)?;
            if !exists_or_link(Path::new(&source)) {
                return Err(refuse(format!(
                    "Snapshot content is missing for target: {rel}"
                )));
            }
        }
        safe_target_path(&canonical, rel, false)?;
        targets.push(Target {
            present,
            rel: rel.to_string(),
        });
    }
    if targets.is_empty() {
        return Err(refuse("Backup snapshot contains no targets"));
    }
    Ok(targets)
}

/// `backup_restore`: every recorded target is removed, then the present ones
/// are copied back, and the folders above them return to their recorded state.
pub fn restore(supplied_root: &str, requested: &str) -> Result<(), Error> {
    let canonical = canonical_root(supplied_root)?;
    let snapshot = snapshot_path(supplied_root, requested)?;
    let targets = load_targets(supplied_root, &snapshot)?;
    let folders = load_folders(&snapshot, &targets)?;
    for target in &targets {
        let path = safe_target_path(&canonical, &target.rel, false)?;
        remove_all(Path::new(&path)).map_err(|e| Error::io(&path, e))?;
    }
    for folder in folders.iter().filter(|folder| folder.present) {
        let path = safe_target_path(&canonical, &folder.rel, true)?;
        std::fs::create_dir_all(&path).map_err(|e| Error::io(&path, e))?;
    }
    for target in targets.iter().filter(|t| t.present) {
        let path = safe_target_path(&canonical, &target.rel, false)?;
        let parent = paths::parent(&path);
        std::fs::create_dir_all(&parent).map_err(|e| Error::io(&parent, e))?;
        let source = snapshot_source(&snapshot, &target.rel)?;
        copy_preserving(Path::new(&source), Path::new(&path)).map_err(|e| Error::io(&path, e))?;
    }
    for folder in folders.iter().rev() {
        settle_folder(&canonical, folder)?;
    }
    Ok(())
}

fn complete_snapshots(store: &str) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(store) else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().disk_text())
        .filter(|name| !name.starts_with('.'))
        .map(|name| format!("{store}/{name}"))
        .filter(|path| {
            let path = Path::new(path);
            !path.is_symlink() && path.is_dir() && path.join(".complete").is_file()
        })
        .collect();
    found.sort();
    found
}

/// `backup_latest`.
pub fn latest(supplied_root: &str) -> Result<Option<String>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    if !Path::new(&store).is_dir() {
        return Ok(None);
    }
    let pointer = PathBuf::from(format!("{store}/.latest"));
    if pointer.is_file() && !pointer.is_symlink() {
        let text = std::fs::read(&pointer).unwrap_or_default();
        let id = String::from_utf8_lossy(&text)
            .split('\n')
            .next()
            .unwrap_or("")
            .to_string();
        if !id.is_empty()
            && id == paths::leaf(&id)
            && Path::new(&format!("{store}/{id}/.complete")).is_file()
        {
            return Ok(Some(format!("{store}/{id}")));
        }
    }
    Ok(complete_snapshots(&store).pop())
}

/// `_rollback_discard_safety`: remove a safety snapshot no restore will use and
/// put `.latest` back.
pub fn discard_safety(store: &str, safety: &str, previous_latest: &str) -> std::io::Result<()> {
    remove_all(Path::new(safety))?;
    if previous_latest.is_empty() {
        return match std::fs::remove_file(format!("{store}/.latest")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    write_store_file(store, ".latest", format!("{previous_latest}\n").as_bytes())
        .map_err(|e| std::io::Error::other(e.to_string()))
}

/// `backup_list`: `(id, operation, created_at)` per complete snapshot.
pub fn list(supplied_root: &str) -> Result<Vec<(String, String, String)>, Error> {
    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    Ok(complete_snapshots(&store)
        .into_iter()
        .map(|path| {
            let metadata = std::fs::read(format!("{path}/metadata")).unwrap_or_default();
            let metadata = String::from_utf8_lossy(&metadata);
            let field = |key: &str| {
                metadata
                    .split('\n')
                    .find_map(|line| line.strip_prefix(key))
                    .unwrap_or("")
                    .to_string()
            };
            (
                paths::leaf(&path),
                field("operation="),
                field("created_at="),
            )
        })
        .collect())
}

/// `backup_prune`: a snapshot survives when it is among the newest `limit` and
/// at most `max_age` whole UTC days old; 0 disables a bound; the latest stays.
/// Under `preserve` the bounds are still validated and nothing is removed.
pub fn prune(
    supplied_root: &str,
    limit: Option<&str>,
    max_age: Option<&str>,
    retention: Retention,
) -> Result<(), Error> {
    prune_at(supplied_root, limit, max_age, SystemTime::now(), retention)
}

fn prune_at(
    supplied_root: &str,
    limit: Option<&str>,
    max_age: Option<&str>,
    now: SystemTime,
    retention: Retention,
) -> Result<(), Error> {
    let (limit, max_age) = validate_limits(limit, max_age)?;
    if retention == Retention::Preserve {
        return Ok(());
    }

    let canonical = canonical_root(supplied_root)?;
    let store = validate_store(&canonical)?;
    let snapshots = complete_snapshots(&store);
    if snapshots.is_empty() {
        return Ok(());
    }
    let latest = latest(&canonical)?.unwrap_or_default();

    let today = days_since_epoch(now);
    let mut survivors = Vec::new();
    for candidate in snapshots {
        let too_old = max_age > 0
            && candidate != latest
            && snapshot_day(&paths::leaf(&candidate))
                .is_some_and(|day| today - day > max_age as i64);
        if too_old {
            remove_all(Path::new(&candidate)).map_err(|e| Error::io(&candidate, e))?;
        } else {
            survivors.push(candidate);
        }
    }

    if limit == 0 || survivors.len() as u64 <= limit {
        return Ok(());
    }
    let mut remove_count = survivors.len() as u64 - limit;
    for candidate in survivors {
        if remove_count == 0 {
            break;
        }
        if candidate == latest {
            continue;
        }
        remove_all(Path::new(&candidate)).map_err(|e| Error::io(&candidate, e))?;
        remove_count -= 1;
    }
    Ok(())
}

/// `_backup_snapshot_age_days` without the subtraction: the UTC day of an id's
/// `YYYYMMDDTHHMMSSZ-` prefix.
fn snapshot_day(id: &str) -> Option<i64> {
    let bytes = id.as_bytes();
    let shaped = bytes.len() >= 17
        && bytes[..8].iter().all(u8::is_ascii_digit)
        && bytes[8] == b'T'
        && bytes[9..15].iter().all(u8::is_ascii_digit)
        && bytes[15] == b'Z'
        && bytes[16] == b'-';
    if !shaped {
        return None;
    }
    let number = |range: std::ops::Range<usize>| id[range].parse::<i64>().ok();
    Some(days_from_civil(number(0..4)?, number(4..6)?, number(6..8)?))
}

/// `_backup_days_from_civil`.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = if month > 2 {
        (153 * (month - 3) + 2) / 5 + day - 1
    } else {
        (153 * (month + 9) + 2) / 5 + day - 1
    };
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn days_since_epoch(now: SystemTime) -> i64 {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    (secs / 86_400) as i64
}

/// `date -u +%Y%m%dT%H%M%SZ`.
fn utc_stamp(now: SystemTime) -> String {
    let secs = now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

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

        fn path(&self, rel: &str) -> PathBuf {
            Path::new(&self.root).join(rel)
        }

        fn write(&self, rel: &str, text: &str) {
            let path = self.path(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        }

        fn read(&self, rel: &str) -> String {
            std::fs::read_to_string(self.path(rel)).unwrap()
        }

        fn abs(&self, rel: &str) -> String {
            format!("{}/{rel}", self.root)
        }

        fn fake_snapshot(&self, id: &str) {
            self.write(
                &format!(".ai/backups/{id}/metadata"),
                "schema=1\noperation=sync\n",
            );
            self.write(
                &format!(".ai/backups/{id}/targets.tsv"),
                "missing\tAGENTS.md\n",
            );
            self.write(&format!(".ai/backups/{id}/.complete"), "");
            std::fs::create_dir_all(self.path(&format!(".ai/backups/{id}/files"))).unwrap();
        }
    }

    fn at(stamp_secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(stamp_secs)
    }

    #[test]
    fn stamps_and_days_agree_with_date_and_days_from_civil() {
        assert_eq!(utc_stamp(at(0)), "19700101T000000Z");
        assert_eq!(utc_stamp(at(1_789_323_442)), "20260913T181722Z");
        assert_eq!(utc_stamp(at(951_782_400)), "20000229T000000Z");
        assert_eq!(days_from_civil(2020, 1, 1), 18_262);
        assert_eq!(snapshot_day("20200101T000000Z-sync-1"), Some(18_262));
        assert_eq!(snapshot_day("not-a-timestamp"), None);
    }

    #[test]
    fn a_file_in_the_way_of_a_target_is_named() {
        let project = Project::new();
        project.write(".clinerules", "rules\n");
        assert_eq!(
            safe_target_path(&project.root, ".clinerules/sub/00-context.md", false)
                .unwrap_err()
                .to_string(),
            ".clinerules is a file where .clinerules/sub/00-context.md needs a directory; move or rename it, then run the command again"
        );
    }

    #[test]
    fn a_restore_brings_back_existing_targets_and_removes_later_ones() {
        let p = Project::new();
        p.write("AGENTS.md", "before\n");
        p.write(".claude/settings.json", "settings-before\n");
        let snapshot = create_at(
            &p.root,
            "sync",
            &[
                p.abs("AGENTS.md"),
                p.abs(".claude/settings.json"),
                p.abs(".claude/rules"),
            ],
            at(1_789_323_442),
            Retention::Bounded,
        )
        .unwrap();
        let id = format!("20260913T181722Z-sync-{}", std::process::id());
        assert_eq!(snapshot, p.abs(&format!(".ai/backups/{id}")));
        assert_eq!(
            p.read(&format!(".ai/backups/{id}/targets.tsv")),
            "present\tAGENTS.md\npresent\t.claude/settings.json\nmissing\t.claude/rules\n"
        );
        assert_eq!(
            p.read(&format!(".ai/backups/{id}/metadata")),
            "schema=1\noperation=sync\ncreated_at=20260913T181722Z\n"
        );
        assert_eq!(p.read(".ai/backups/.latest"), format!("{id}\n"));
        assert_eq!(p.read(".ai/backups/.gitignore"), "*\n");

        p.write("AGENTS.md", "after\n");
        p.write(".claude/rules/core.md", "generated\n");
        restore(&p.root, &snapshot).unwrap();
        assert_eq!(p.read("AGENTS.md"), "before\n");
        assert_eq!(p.read(".claude/settings.json"), "settings-before\n");
        assert!(!p.path(".claude/rules").exists());
    }

    #[test]
    fn nested_and_duplicate_targets_collapse_into_the_shallowest_root() {
        let p = Project::new();
        p.write(".amazonq/rules/00-context.md", "context\n");
        let snapshot = create(
            &p.root,
            "sync",
            &[
                p.abs(".amazonq/rules/00-context.md"),
                p.abs(".amazonq/rules"),
                p.abs(".amazonq/rules"),
            ],
            Retention::Bounded,
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(format!("{snapshot}/targets.tsv")).unwrap(),
            "present\t.amazonq/rules\n"
        );
        assert!(Path::new(&format!("{snapshot}/files/.amazonq/rules/00-context.md")).is_file());
    }

    #[test]
    fn the_root_the_store_and_outside_paths_are_refused() {
        let p = Project::new();
        let refused = |target: String| {
            create(&p.root, "sync", &[target], Retention::Bounded)
                .unwrap_err()
                .to_string()
        };
        assert_eq!(
            refused(p.root.clone()),
            "Refusing to back up the repository root"
        );
        assert_eq!(
            refused(paths::parent(&p.root)),
            format!(
                "Backup target is outside the repository root: {}",
                paths::parent(&p.root)
            )
        );
        assert_eq!(
            refused(p.abs(".ai")),
            "Refusing to back up a parent of the backup store: .ai"
        );
        assert_eq!(
            refused(p.abs(".ai/backups/x")),
            "Refusing to back up the backup store itself: .ai/backups/x"
        );
    }

    #[test]
    fn links_modes_and_times_survive_a_round_trip() {
        use std::os::unix::fs::PermissionsExt;
        let p = Project::new();
        p.write("AGENTS.md", "agents\n");
        std::os::unix::fs::symlink("AGENTS.md", p.path("CLAUDE.md")).unwrap();
        p.write(".claude/hooks/guard.sh", "#!/bin/sh\n");
        let guard = p.path(".claude/hooks/guard.sh");
        std::fs::set_permissions(&guard, std::fs::Permissions::from_mode(0o755)).unwrap();
        let old = at(1_600_000_000);
        OpenOptions::new()
            .write(true)
            .open(&guard)
            .unwrap()
            .set_modified(old)
            .unwrap();

        let snapshot = create(
            &p.root,
            "sync",
            &[p.abs("CLAUDE.md"), p.abs(".claude")],
            Retention::Bounded,
        )
        .unwrap();
        std::fs::remove_file(p.path("CLAUDE.md")).unwrap();
        std::fs::remove_dir_all(p.path(".claude")).unwrap();
        restore(&p.root, &snapshot).unwrap();

        assert_eq!(
            std::fs::read_link(p.path("CLAUDE.md")).unwrap(),
            Path::new("AGENTS.md")
        );
        let meta = std::fs::metadata(&guard).unwrap();
        assert_eq!(meta.permissions().mode() & 0o777, 0o755);
        assert_eq!(meta.modified().unwrap(), old);
    }

    #[test]
    fn a_store_or_snapshot_reached_through_a_symlink_is_refused() {
        let p = Project::new();
        let outside = tempfile::tempdir().unwrap();
        std::os::unix::fs::symlink(outside.path(), p.path(".ai")).unwrap();
        p.write("AGENTS.md", "source\n");
        assert_eq!(
            create(&p.root, "sync", &[p.abs("AGENTS.md")], Retention::Bounded)
                .unwrap_err()
                .to_string(),
            "Exuno state directory cannot be a symlink: .ai"
        );
        assert!(!outside.path().join("backups").exists());

        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        create(&p.root, "init", &[p.abs("AGENTS.md")], Retention::Bounded).unwrap();
        let forged = tempfile::tempdir().unwrap();
        std::fs::create_dir(forged.path().join("files")).unwrap();
        std::fs::write(forged.path().join("targets.tsv"), "present\tAGENTS.md\n").unwrap();
        std::fs::write(forged.path().join("files/AGENTS.md"), "outside\n").unwrap();
        std::fs::write(forged.path().join(".complete"), "").unwrap();
        std::os::unix::fs::symlink(forged.path(), p.path(".ai/backups/forged")).unwrap();
        assert_eq!(
            restore(&p.root, "forged").unwrap_err().to_string(),
            "Backup snapshot is missing or incomplete: forged"
        );
        assert_eq!(p.read("AGENTS.md"), "source\n");
    }

    #[test]
    fn metadata_updates_replace_symlinks_instead_of_following_them() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        create(&p.root, "init", &[p.abs("AGENTS.md")], Retention::Bounded).unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("ignore"), "outside-ignore\n").unwrap();
        std::fs::remove_file(p.path(".ai/backups/.gitignore")).unwrap();
        std::os::unix::fs::symlink(
            outside.path().join("ignore"),
            p.path(".ai/backups/.gitignore"),
        )
        .unwrap();
        create(&p.root, "sync", &[p.abs("AGENTS.md")], Retention::Bounded).unwrap();
        assert_eq!(
            std::fs::read_to_string(outside.path().join("ignore")).unwrap(),
            "outside-ignore\n"
        );
        assert!(!p.path(".ai/backups/.gitignore").is_symlink());
    }

    #[test]
    fn pruning_bounds_count_and_age_and_always_keeps_the_latest() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.fake_snapshot("not-a-timestamp");
        let now = at(1_789_323_442);
        let first = create_at(
            &p.root,
            "init",
            &[p.abs("AGENTS.md")],
            now,
            Retention::Bounded,
        )
        .unwrap();
        let second = create_at(
            &p.root,
            "sync",
            &[p.abs("AGENTS.md")],
            now,
            Retention::Bounded,
        )
        .unwrap();
        let newest = create_at(
            &p.root,
            "sync",
            &[p.abs("AGENTS.md")],
            now,
            Retention::Bounded,
        )
        .unwrap();
        assert!(newest.ends_with(&format!("-sync-{}-2", std::process::id())));

        prune_at(&p.root, Some("10"), Some("0"), now, Retention::Bounded).unwrap();
        assert!(p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        prune_at(&p.root, None, None, now, Retention::Bounded).unwrap();
        assert!(!p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        assert!(p.path(".ai/backups/not-a-timestamp").exists());

        prune_at(&p.root, Some("2"), Some("30"), now, Retention::Bounded).unwrap();
        assert!(!Path::new(&first).exists());
        assert!(!Path::new(&second).exists());
        assert!(p.path(".ai/backups/not-a-timestamp").exists());
        assert_eq!(latest(&p.root).unwrap(), Some(newest));

        assert_eq!(
            prune_at(&p.root, Some("x"), None, now, Retention::Bounded)
                .unwrap_err()
                .to_string(),
            "Backup limit must be a non-negative integer: x"
        );
    }

    #[test]
    fn the_latest_survives_an_age_limit_and_stale_staging_is_swept() {
        let p = Project::new();
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.write(".ai/backups/.latest", "20200101T000000Z-sync-1\n");
        let now = at(1_789_323_442);
        prune_at(&p.root, Some("10"), Some("1"), now, Retention::Bounded).unwrap();
        assert_eq!(
            latest(&p.root).unwrap(),
            Some(p.abs(".ai/backups/20200101T000000Z-sync-1"))
        );

        p.write("AGENTS.md", "source\n");
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.abandoned/files")).unwrap();
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.inflight/files")).unwrap();
        p.write(".ai/backups/.latest.tmp.stale", "");
        let old = at(1_577_836_800);
        std::fs::File::open(p.path(".ai/backups/.tmp.sync.abandoned"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        OpenOptions::new()
            .write(true)
            .open(p.path(".ai/backups/.latest.tmp.stale"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        create_at(
            &p.root,
            "sync",
            &[p.abs("AGENTS.md")],
            now,
            Retention::Bounded,
        )
        .unwrap();
        assert!(!p.path(".ai/backups/.tmp.sync.abandoned").exists());
        assert!(!p.path(".ai/backups/.latest.tmp.stale").exists());
        assert!(p.path(".ai/backups/.tmp.sync.inflight").exists());

        let rows = list(&p.root).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(
            rows[0],
            (
                "20200101T000000Z-sync-1".to_string(),
                "sync".to_string(),
                String::new()
            )
        );
        assert_eq!(rows[1].1, "sync");
        assert_eq!(rows[1].2, "20260913T181722Z");
    }

    #[test]
    fn configure_reads_the_policy_and_the_bounds_like_backup_configure() {
        let path = "/p/.ai/agent_sync.yaml";
        let with = |text: &str| configure(Some((path, text)), None, None);
        assert_eq!(configure(None, None, None).unwrap(), Retention::Bounded);
        assert_eq!(
            with("tools:\n  enabled: [claude]\n").unwrap(),
            Retention::Bounded
        );
        assert_eq!(
            with("backup:\n  retention: preserve\n").unwrap(),
            Retention::Preserve
        );
        assert_eq!(
            with("backup:\n  retention: bounded\n").unwrap(),
            Retention::Bounded
        );
        assert_eq!(
            with("backup:\n  retention: \"preserve\" # keep\n").unwrap(),
            Retention::Preserve
        );
        assert_eq!(with("backup:\n  other: 1\n").unwrap(), Retention::Bounded);
        let refused = |text: &str| with(text).unwrap_err().to_string();
        assert_eq!(
            refused("backup:\n  retention: typo\n"),
            "Invalid backup.retention 'typo' in /p/.ai/agent_sync.yaml; expected bounded or preserve"
        );
        for empty in [
            "backup:\n  retention:\n",
            "backup:\n  retention: \"\"\n",
            "backup:\n  retention: # nothing\n",
        ] {
            assert_eq!(
                refused(empty),
                "Invalid backup.retention '<empty>' in /p/.ai/agent_sync.yaml; expected bounded or preserve"
            );
        }
        assert_eq!(
            refused("backup: preserve\n"),
            "backup must be a mapping with backup.retention: bounded or preserve in /p/.ai/agent_sync.yaml"
        );
        let bounds = |limit: &str, age: &str| {
            configure(None, Some(limit), Some(age)).map_err(|e| e.to_string())
        };
        assert_eq!(
            bounds("typo", "").unwrap_err(),
            "Backup limit must be a non-negative integer: typo"
        );
        assert_eq!(
            bounds("", "-1").unwrap_err(),
            "Backup max age must be a non-negative integer: -1"
        );
        assert_eq!(
            bounds("x", "y").unwrap_err(),
            "Backup limit must be a non-negative integer: x"
        );
        assert_eq!(bounds("0", "0").unwrap(), Retention::Bounded);
    }

    #[test]
    fn preserve_keeps_old_snapshots_and_stale_staging() {
        let p = Project::new();
        p.write("AGENTS.md", "source\n");
        p.fake_snapshot("20200101T000000Z-sync-1");
        p.fake_snapshot("20200102T000000Z-sync-2");
        p.write(".ai/backups/.latest", "20200102T000000Z-sync-2\n");
        std::fs::create_dir_all(p.path(".ai/backups/.tmp.sync.abandoned/files")).unwrap();
        std::fs::File::open(p.path(".ai/backups/.tmp.sync.abandoned"))
            .unwrap()
            .set_modified(at(1_577_836_800))
            .unwrap();
        let now = at(1_789_323_442);
        create_at(
            &p.root,
            "sync",
            &[p.abs("AGENTS.md")],
            now,
            Retention::Preserve,
        )
        .unwrap();
        assert!(p.path(".ai/backups/.tmp.sync.abandoned").exists());
        prune_at(&p.root, Some("1"), Some("1"), now, Retention::Preserve).unwrap();
        assert!(p.path(".ai/backups/20200101T000000Z-sync-1").exists());
        assert!(p.path(".ai/backups/20200102T000000Z-sync-2").exists());
        assert_eq!(
            prune_at(&p.root, Some("x"), None, now, Retention::Preserve)
                .unwrap_err()
                .to_string(),
            "Backup limit must be a non-negative integer: x"
        );
    }

    #[test]
    fn a_fifo_under_a_target_is_recreated_not_read() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::create_dir_all(format!("{root}/.claude/skills")).unwrap();
        let made = std::process::Command::new("mkfifo")
            .arg(format!("{root}/.claude/skills/pipe"))
            .status()
            .is_ok_and(|status| status.success());
        if !made {
            return;
        }
        let snapshot = create(
            &root,
            "sync",
            &[format!("{root}/.claude/skills")],
            Retention::Bounded,
        )
        .unwrap();
        use std::os::unix::fs::FileTypeExt;
        let copied =
            std::fs::symlink_metadata(format!("{snapshot}/files/.claude/skills/pipe")).unwrap();
        assert!(copied.file_type().is_fifo());
        std::fs::remove_file(format!("{root}/.claude/skills/pipe")).unwrap();
        restore(&root, &snapshot).unwrap();
        let restored = std::fs::symlink_metadata(format!("{root}/.claude/skills/pipe")).unwrap();
        assert!(restored.file_type().is_fifo());
    }

    #[test]
    fn a_symlinked_completion_marker_is_not_a_complete_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        std::fs::write(format!("{root}/CLAUDE.md"), "x\n").unwrap();
        let snapshot = create(
            &root,
            "sync",
            &[format!("{root}/CLAUDE.md")],
            Retention::Bounded,
        )
        .unwrap();
        std::fs::rename(format!("{snapshot}/.complete"), format!("{root}/marker")).unwrap();
        std::os::unix::fs::symlink(format!("{root}/marker"), format!("{snapshot}/.complete"))
            .unwrap();
        let id = paths::leaf(&snapshot);
        assert_eq!(
            snapshot_path(&root, &id).unwrap_err().to_string(),
            format!("Backup snapshot is missing or incomplete: {id}")
        );
    }

    #[test]
    fn a_restore_removes_folders_the_operation_created() {
        let p = Project::new();
        p.write(".claude/settings.json", "settings\n");
        let snapshot = create(
            &p.root,
            "sync",
            &[
                p.abs(".cursor/rules/core.mdc"),
                p.abs(".claude/settings.json"),
            ],
            Retention::Bounded,
        )
        .unwrap();
        p.write(".cursor/rules/core.mdc", "generated\n");
        restore(&p.root, &snapshot).unwrap();
        assert!(!p.path(".cursor").exists());
        assert_eq!(p.read(".claude/settings.json"), "settings\n");
    }

    #[test]
    fn a_created_folder_that_holds_other_files_stays() {
        let p = Project::new();
        let snapshot = create(
            &p.root,
            "sync",
            &[p.abs(".cursor/rules")],
            Retention::Bounded,
        )
        .unwrap();
        p.write(".cursor/rules/core.mdc", "generated\n");
        p.write(".cursor/mcp.json", "{}\n");
        restore(&p.root, &snapshot).unwrap();
        assert!(!p.path(".cursor/rules").exists());
        assert_eq!(p.read(".cursor/mcp.json"), "{}\n");
    }

    #[test]
    fn a_removed_folder_comes_back_with_its_mode() {
        use std::os::unix::fs::PermissionsExt;
        let p = Project::new();
        p.write(".cursor/rules/core.mdc", "rules\n");
        let mode =
            |rel: &str| std::fs::metadata(p.path(rel)).unwrap().permissions().mode() & 0o7777;
        let chmod = |rel: &str, mode: u32| {
            std::fs::set_permissions(p.path(rel), std::fs::Permissions::from_mode(mode)).unwrap();
        };
        chmod(".cursor", 0o700);
        chmod(".cursor/rules", 0o750);
        let snapshot = create(
            &p.root,
            "sync",
            &[p.abs(".cursor/rules/core.mdc")],
            Retention::Bounded,
        )
        .unwrap();
        std::fs::remove_dir_all(p.path(".cursor")).unwrap();
        restore(&p.root, &snapshot).unwrap();
        assert_eq!(p.read(".cursor/rules/core.mdc"), "rules\n");
        assert_eq!(mode(".cursor"), 0o700);
        assert_eq!(mode(".cursor/rules"), 0o750);

        chmod(".cursor", 0o755);
        restore(&p.root, &snapshot).unwrap();
        assert_eq!(mode(".cursor"), 0o700);
    }

    #[test]
    fn a_forged_folder_record_outside_the_targets_is_refused() {
        let p = Project::new();
        p.write("AGENTS.md", "agents\n");
        p.write("docs/keep.md", "keep\n");
        let snapshot = create(&p.root, "sync", &[p.abs("AGENTS.md")], Retention::Bounded).unwrap();
        std::fs::write(format!("{snapshot}/dirs.tsv"), "missing\t-\tdocs\n").unwrap();
        assert_eq!(
            restore(&p.root, &snapshot).unwrap_err().to_string(),
            "Invalid folder record in snapshot: docs"
        );
        assert_eq!(p.read("docs/keep.md"), "keep\n");
    }

    #[test]
    fn a_snapshot_without_a_folder_list_still_restores() {
        let p = Project::new();
        p.write(".claude/rules/core.md", "before\n");
        let snapshot = create(
            &p.root,
            "sync",
            &[p.abs(".claude/rules/core.md")],
            Retention::Bounded,
        )
        .unwrap();
        std::fs::remove_file(format!("{snapshot}/dirs.tsv")).unwrap();
        std::fs::remove_dir_all(p.path(".claude")).unwrap();
        restore(&p.root, &snapshot).unwrap();
        assert_eq!(p.read(".claude/rules/core.md"), "before\n");
    }

    #[test]
    fn targets_with_spaces_round_trip() {
        let p = Project::new();
        p.write("tool config/settings file.json", "before\n");
        let snapshot = create_at(
            &p.root,
            "sync",
            &[p.abs("tool config/settings file.json")],
            at(1_789_323_442),
            Retention::Bounded,
        )
        .unwrap();
        p.write("tool config/settings file.json", "after\n");
        restore(&p.root, &paths::leaf(&snapshot)).unwrap();
        assert_eq!(p.read("tool config/settings file.json"), "before\n");
    }
}
