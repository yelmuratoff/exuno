//! Path rules of `lib/helpers/paths.sh` and `display_path` from
//! `lib/helpers/logging.sh`, on `/`-separated strings as Bash holds them.

use std::path::Path;

use crate::output::log::Log;

/// Where `$DEFAULT_REPO_ROOT` points in the native engine: templates are
/// embedded, so the engine checkout is a virtual root served by the workspace.
pub const ENGINE_ROOT: &str = "/<agentsync>";

/// Parent of the virtual trees that replace the Bash overlay tmpdirs.
pub const OVERLAY_ROOT: &str = "/<agentsync-overlay>";

pub fn is_virtual(path: &str) -> bool {
    is_within(path, ENGINE_ROOT) || is_within(path, OVERLAY_ROOT)
}

/// `path` is `root` or lies below it.
pub fn is_within(path: &str, root: &str) -> bool {
    path == root
        || path
            .strip_prefix(root)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// The `X:` of a Windows drive-rooted path (`C:/Users`, `C:\Users`, `C:`).
fn drive_prefix(path: &str) -> Option<&str> {
    let bytes = path.as_bytes();
    (bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes.get(2), None | Some(b'/') | Some(b'\\')))
    .then(|| &path[..2])
}

/// A path the engine treats as absolute: `/`-rooted, or drive-rooted as the
/// Windows binary spells its own working directory.
pub fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || drive_prefix(path).is_some()
}

/// A path the operating system handed back, spelled the way the engine spells
/// paths: on Windows the `\\?\` prefix `canonicalize` adds is dropped and the
/// separators are `/`; elsewhere the bytes are unchanged.
pub fn from_disk(path: &Path) -> String {
    let text = path.to_string_lossy().into_owned();
    if !cfg!(windows) {
        return text;
    }
    let text = text
        .strip_prefix("\\\\?\\UNC\\")
        .map(|rest| format!("\\\\{rest}"))
        .or_else(|| text.strip_prefix("\\\\?\\").map(str::to_string))
        .unwrap_or(text);
    text.replace('\\', "/")
}

/// [`from_disk`] as a method, for every `Path`, `PathBuf`, and `OsStr` the
/// engine turns into one of its `/`-separated strings.
pub trait DiskText {
    fn disk_text(&self) -> String;
}

impl<T: AsRef<std::ffi::OsStr> + ?Sized> DiskText for T {
    fn disk_text(&self) -> String {
        from_disk(Path::new(self))
    }
}

/// A path Git Bash handed over: a `/`-rooted one is translated through
/// `cygpath -w` when `msystem` (the `MSYSTEM` variable) is set, so `/tmp/x`
/// and `/c/Users/x` reach the Windows binary as paths it can open; any other
/// path, or a host without `cygpath`, leaves it unchanged.
pub fn from_msys(path: &str, msystem: Option<&str>) -> String {
    if msystem.is_none() || !path.starts_with('/') {
        return path.to_string();
    }
    // `-l` asks for the long form: the runner's `TEMP` is spelled `RUNNER~1`.
    let output = std::process::Command::new("cygpath")
        .args(["-wl", path])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output();
    match output {
        Ok(out) if out.status.success() => {
            let translated = String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\r', '\n'])
                .to_string();
            if translated.is_empty() {
                path.to_string()
            } else {
                from_disk(Path::new(&translated))
            }
        }
        _ => path.to_string(),
    }
}

/// `_path_parent_r`: `dirname` without the process; a drive root is its own
/// parent, as `/` is.
pub fn parent(path: &str) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    if let Some(drive) = drive_prefix(path)
        && path[2..].trim_end_matches(['/', '\\']).is_empty()
    {
        return format!("{drive}/");
    }
    let trimmed = trim_trailing_slashes(path);
    if trimmed == "/" {
        return "/".to_string();
    }
    match trimmed.rfind('/') {
        None => ".".to_string(),
        Some(idx) => {
            let head = trim_trailing_slashes(&trimmed[..idx]);
            if head.is_empty() {
                "/".to_string()
            } else if drive_prefix(head).is_some() && head.len() == 2 {
                format!("{head}/")
            } else {
                head.to_string()
            }
        }
    }
}

/// `_path_leaf_r`: `basename` without a suffix argument.
pub fn leaf(path: &str) -> String {
    let trimmed = trim_trailing_slashes(path);
    if trimmed == "/" {
        return "/".to_string();
    }
    trimmed.rsplit('/').next().unwrap_or("").to_string()
}

fn trim_trailing_slashes(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && path.starts_with('/') {
        "/"
    } else {
        trimmed
    }
}

/// Collapses empty, `.` and `..` segments of an absolute path. A drive-rooted
/// path keeps its `X:` and reads `\` as a separator too.
pub fn normalize(path: &str) -> String {
    let (root, rest) = match drive_prefix(path) {
        Some(drive) => (format!("{drive}/"), &path[2..]),
        None => ("/".to_string(), path),
    };
    let mut segments: Vec<&str> = Vec::new();
    for segment in rest.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop();
            }
            other => segments.push(other),
        }
    }
    format!("{root}{}", segments.join("/"))
}

/// The project root as Bash's `cd "$REPO_ROOT" && pwd` spells it: the logical
/// `$PWD` when it names the working directory, so a symlinked root keeps its
/// spelling in display and manifest paths.
pub fn logical_root(env_root: Option<&str>, cwd: &Path, pwd: Option<&str>) -> String {
    let cwd_text = from_disk(cwd);
    let logical_cwd = match pwd {
        Some(pwd)
            if pwd.starts_with('/')
                && std::fs::canonicalize(pwd).ok() == std::fs::canonicalize(cwd).ok() =>
        {
            pwd.to_string()
        }
        _ => cwd_text,
    };
    let base = match env_root {
        Some(root) if is_absolute(root) || Path::new(root).is_absolute() => {
            from_disk(Path::new(root))
        }
        Some(root) => format!("{logical_cwd}/{root}"),
        None => logical_cwd,
    };
    if is_absolute(&base) {
        normalize(&base)
    } else {
        base
    }
}

/// `ai_dir_enclosing_root`: the parent of the shallowest `.ai` segment of a
/// logical directory path, when there is one.
pub fn ai_dir_enclosing_root(dir: &str) -> Option<String> {
    let mut shallowest = None;
    let mut current = dir.to_string();
    while !current.is_empty() {
        if leaf(&current) == ".ai" {
            shallowest = Some(current.clone());
        }
        let up = parent(&current);
        if up == current {
            break;
        }
        current = up;
    }
    shallowest.map(|ai| parent(&ai))
}

/// `find_parent_ai_src`: the nearest ancestor's `.ai/src` above a logical
/// directory, never the directory's own and never outside its git repository.
pub fn find_parent_ai_src(start: &str) -> Option<String> {
    if !Path::new(start).is_dir() {
        return None;
    }
    let mut git_root = None;
    let mut probe = start.to_string();
    // Stop at the root by its fixpoint, not by spelling: `/` is its own parent
    // on Unix and `C:/` is on Windows, where a `probe != "/"` test never ends.
    while !probe.is_empty() {
        let dot_git = Path::new(&probe).join(".git");
        if dot_git.is_dir() || dot_git.is_file() {
            git_root = Some(probe);
            break;
        }
        let up = parent(&probe);
        if up == probe {
            break;
        }
        probe = up;
    }
    let mut current = start.to_string();
    let mut up = parent(&current);
    while up != current {
        if let Some(root) = &git_root
            && !is_within(&up, root)
        {
            return None;
        }
        let candidate = format!("{up}/.ai/src");
        if Path::new(&candidate).is_dir() {
            return Some(candidate);
        }
        current = up;
        up = parent(&current);
    }
    None
}

/// `find_workspace_ai_dirs`: every `.ai` directory below `root` holding `src/`
/// or a project config, deepest first and then in byte order. `.git` and
/// `node_modules` are not entered, nor is a `.ai` once found, nor a symlink.
pub fn find_workspace_ai_dirs(root: &str) -> Vec<String> {
    fn walk(dir: &str, found: &mut Vec<String>) {
        let name = leaf(dir);
        if name == ".git" || name == "node_modules" {
            return;
        }
        let Ok(meta) = std::fs::symlink_metadata(dir) else {
            return;
        };
        if !meta.is_dir() {
            return;
        }
        if name == ".ai" {
            found.push(dir.to_string());
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.filter_map(|e| e.ok()) {
            let child = entry.file_name().disk_text();
            walk(&format!("{}/{child}", dir.trim_end_matches('/')), found);
        }
    }
    if !Path::new(root).is_dir() {
        return Vec::new();
    }
    let mut found = Vec::new();
    walk(root, &mut found);
    found.retain(|ai| {
        Path::new(&format!("{ai}/src")).is_dir()
            || crate::config::names::CONFIG_CANDIDATES
                .iter()
                .filter_map(|rel| rel.strip_prefix(".ai/"))
                .any(|name| Path::new(&format!("{ai}/{name}")).is_file())
    });
    found.sort_by(|a, b| {
        let depth = |p: &str| p.split('/').count();
        depth(b).cmp(&depth(a)).then_with(|| a.cmp(b))
    });
    found
}

#[derive(Clone, Debug)]
pub struct Paths {
    pub root: String,
    pub root_canonical: String,
    home: Option<String>,
    lexical_below_root: bool,
    external: Vec<String>,
    explicit: Vec<String>,
}

impl Paths {
    pub fn new(root: &str, root_canonical: &str, home: Option<&str>) -> Self {
        Self {
            root: root.to_string(),
            root_canonical: root_canonical.to_string(),
            home: home.filter(|h| !h.is_empty()).map(str::to_string),
            lexical_below_root: true,
            external: Vec::new(),
            explicit: Vec::new(),
        }
    }

    /// Paths for a root on disk, canonicalised the way `cd -P && pwd` does.
    pub fn for_disk_root(root: &str) -> Self {
        let canonical = std::fs::canonicalize(root)
            .map(|p| from_disk(&p))
            .unwrap_or_else(|_| root.to_string());
        let home = std::env::var("HOME")
            .ok()
            .map(|home| from_msys(&home, std::env::var("MSYSTEM").ok().as_deref()))
            .or_else(|| {
                cfg!(windows)
                    .then(|| std::env::var("USERPROFILE").ok())
                    .flatten()
                    .map(|profile| from_disk(Path::new(&profile)))
            });
        Self::new(root, &canonical, home.as_deref())
    }

    /// Paths for a project a render writes in place: below the root, too, a
    /// path is canonicalised through its nearest existing ancestor on disk, so
    /// a symlinked directory cannot carry a destination out of the project.
    pub fn on_disk(root: &str) -> Self {
        Self {
            lexical_below_root: false,
            ..Self::for_disk_root(root)
        }
    }

    /// `normalize_absolute_path_r`: relative paths are taken from the root.
    pub fn absolute(&self, path: &str) -> String {
        if is_absolute(path) {
            normalize(path)
        } else {
            normalize(&format!("{}/{path}", self.root))
        }
    }

    /// `canonicalize_with_existing_ancestor_r`. Below the root of an in-memory
    /// render the result is lexical: `check` renders into a workspace that, like
    /// the tar copy Bash rendered into, has no symlinks under the root. Virtual
    /// roots are their own canonical form; anything else resolves through the disk.
    pub fn canonicalize_with_existing_ancestor(&self, abs: &str) -> Option<String> {
        if is_virtual(abs) {
            return Some(abs.to_string());
        }
        if let Some(rest) = abs.strip_prefix(&self.root)
            && self.lexical_below_root
            && (rest.is_empty() || rest.starts_with('/'))
        {
            return Some(format!("{}{rest}", self.root_canonical));
        }
        disk_canonical(abs)
    }

    /// `resolve_dest_path_r`: the normalised path, or `None` after logging why.
    pub fn resolve_dest(&self, raw: &str, label: &str, log: &mut Log) -> Option<String> {
        if raw.is_empty() {
            log.error(&format!("{label} is empty"));
            return None;
        }
        let abs = self.absolute(raw);
        let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs) else {
            log.error(&format!("Failed to canonicalize {label} path: {raw}"));
            return None;
        };
        if !is_within(&canonical, &self.root_canonical) {
            log.error(&format!(
                "{label} resolves outside repository root: {raw} -> {canonical}"
            ));
            return None;
        }
        Some(abs)
    }

    /// `is_path_safe_source`: the project, the engine, the overlay trees, and
    /// the explicit roots a config registered.
    pub fn is_safe_source(&self, canonical: &str) -> bool {
        is_within(canonical, &self.root_canonical)
            || is_virtual(canonical)
            || self.explicit.iter().any(|root| is_within(canonical, root))
    }

    /// The directories `AGENTSYNC_EXTERNAL_SOURCE_ROOTS` lists, colon-separated;
    /// a relative entry, or one that is not a directory, trusts nothing.
    pub fn trust_external_roots(&mut self, raw: Option<&str>) {
        // `main` rejoins translated Windows entries with `;`, since `C:/…` holds a colon.
        let separator = if cfg!(windows) { ';' } else { ':' };
        self.external = raw
            .unwrap_or_default()
            .split(separator)
            .filter(|entry| is_absolute(entry) && Path::new(entry).is_dir())
            .filter_map(canonical_dir)
            .collect();
    }

    /// `_external_source_trusted`.
    pub fn is_trusted_external(&self, canonical: &str) -> bool {
        self.external.iter().any(|root| is_within(canonical, root))
    }

    /// `explicit_source_root_r` for a `source.*` value.
    pub fn classify_explicit_source(&self, raw: &str) -> ExplicitSource {
        let abs = self.absolute(raw);
        if abs.starts_with(&format!("{}/", self.root)) {
            return ExplicitSource::Inside;
        }
        let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs) else {
            return ExplicitSource::Inside;
        };
        if canonical.starts_with(&format!("{}/", self.root_canonical)) {
            return ExplicitSource::Inside;
        }
        let home = self.home.as_deref().and_then(canonical_dir);
        let is_filesystem_root = canonical == "/"
            || drive_prefix(&canonical).is_some_and(|drive| canonical == format!("{drive}/"));
        if is_filesystem_root
            || home.as_deref() == Some(canonical.as_str())
            || canonical == self.root_canonical
            || self.root_canonical.starts_with(&format!("{canonical}/"))
        {
            return ExplicitSource::Refused(canonical);
        }
        if !self.is_trusted_external(&canonical) {
            return ExplicitSource::Untrusted(canonical);
        }
        ExplicitSource::Outside(canonical)
    }

    /// `EXPLICIT_SOURCE_ROOTS`: canonical roots `is_safe_source` admits.
    pub fn register_explicit_roots(&mut self, roots: Vec<String>) {
        self.explicit = roots;
    }

    /// `refuse_escaping_source_links`: every symlink at or below `roots`, and
    /// below each safe directory a link reaches, must resolve to a safe source
    /// or a trusted external root. `Err` is the message to log.
    pub fn escaping_source_link(&self, roots: &[String]) -> Result<(), String> {
        let mut pending: Vec<String> = roots.to_vec();
        let mut visited: Vec<String> = Vec::new();
        let mut index = 0usize;
        while index < pending.len() {
            let root = pending[index].clone();
            index += 1;
            if root.is_empty() {
                continue;
            }
            if index > 256 {
                return Err(
                    "Too many nested symlinks under the source directories to check them safely"
                        .to_string(),
                );
            }
            let mut links = Vec::new();
            if is_symlink(&root) {
                links.push(root);
            } else if Path::new(&root).is_dir() {
                collect_links(&root, &mut links);
            }
            for link in links {
                let shown = link
                    .strip_prefix(&format!("{}/", self.root))
                    .unwrap_or(&link)
                    .to_string();
                let Some(target) = link_target(&link) else {
                    return Err(format!("Cannot resolve source symlink: {shown}"));
                };
                if !self.is_safe_source(&target) && !self.is_trusted_external(&target) {
                    return Err(format!(
                        "Source symlink {shown} resolves outside the project: {target}; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"
                    ));
                }
                if Path::new(&target).is_dir() && !visited.contains(&target) {
                    visited.push(target.clone());
                    pending.push(target);
                }
            }
        }
        Ok(())
    }

    /// `resolve_source_path_r`: the normalised path, or `None` after logging an
    /// unsafe root. A missing path is not an error; callers test existence.
    pub fn resolve_source(&self, raw: &str, label: &str, log: &mut Log) -> Option<String> {
        if raw.is_empty() {
            log.error(&format!("{label} is empty"));
            return None;
        }
        let abs = self.absolute(raw);
        if let Some(canonical) = self.canonicalize_with_existing_ancestor(&abs)
            && !self.is_safe_source(&canonical)
        {
            log.error(&format!(
                "{label} resolves outside safe source roots: {raw} -> {canonical}"
            ));
            return None;
        }
        Some(abs)
    }

    /// `to_repo_relative_path_r` without its log line: `.` for the root.
    pub fn to_repo_relative(&self, abs: &str) -> Option<String> {
        if abs == self.root {
            return Some(".".to_string());
        }
        abs.strip_prefix(&self.root)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(str::to_string)
    }

    /// Whether the project root is the home directory, where tool dests are
    /// the tools' own config homes.
    pub fn root_is_home(&self) -> bool {
        self.home
            .as_deref()
            .and_then(canonical_dir)
            .is_some_and(|home| home == self.root_canonical)
    }

    /// `display_path_r`: root-relative, else `~/`-folded, else unchanged. A
    /// virtual path is named by what it holds, never by its engine root: an
    /// overlay entry by its category (`rules/`, `AGENTS.md`), a shipped file
    /// by its template path (`templates/guard/claude.sh`).
    pub fn display(&self, path: &str) -> String {
        if let Some(rel) = self.to_repo_relative(path) {
            return rel;
        }
        if path == self.root_canonical {
            return ".".to_string();
        }
        if let Some(rest) = path
            .strip_prefix(&self.root_canonical)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            return rest.to_string();
        }
        if let Some(rest) = path.strip_prefix(&format!("{OVERLAY_ROOT}/")) {
            let after_name = rest.split_once('/').map_or("", |(_, tail)| tail);
            return after_name
                .strip_prefix("src/")
                .unwrap_or(after_name)
                .to_string();
        }
        if let Some(rest) = path.strip_prefix(&format!("{ENGINE_ROOT}/")) {
            return rest.strip_prefix("lib/").unwrap_or(rest).to_string();
        }
        if let Some(home) = &self.home
            && let Some(rest) = path
                .strip_prefix(home.as_str())
                .and_then(|r| r.strip_prefix('/'))
        {
            return format!("~/{rest}");
        }
        path.to_string()
    }
}

fn canonical_dir(dir: &str) -> Option<String> {
    std::fs::canonicalize(dir).ok().map(|p| from_disk(&p))
}

/// What `explicit_source_root_r` returns: 0, 1, 2, and 3, with the canonical root.
#[derive(Debug, PartialEq, Eq)]
pub enum ExplicitSource {
    Inside,
    Outside(String),
    Refused(String),
    Untrusted(String),
}

/// `canonicalize_with_existing_ancestor_r` through the disk: the nearest
/// existing ancestor canonicalised, the missing rest appended lexically.
fn disk_canonical(abs: &str) -> Option<String> {
    let mut ancestor = abs.to_string();
    while !Path::new(&ancestor).exists() {
        let up = parent(&ancestor);
        if up == ancestor {
            break;
        }
        ancestor = up;
    }
    let ancestor_canonical = if Path::new(&ancestor).is_dir() {
        canonical_dir(&ancestor)?
    } else {
        format!("{}/{}", canonical_dir(&parent(&ancestor))?, leaf(&ancestor))
    };
    if abs == ancestor {
        return Some(ancestor_canonical);
    }
    let mut suffix = abs[ancestor.len()..].to_string();
    if !suffix.is_empty() && !suffix.starts_with('/') {
        suffix.insert(0, '/');
    }
    Some(normalize(&format!("{ancestor_canonical}{suffix}")))
}

fn is_symlink(path: &str) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// `_source_link_target_r`: follow a chain of at most 40 links, a relative
/// target resolving from the link's physical directory.
fn link_target(link: &str) -> Option<String> {
    let mut path = link.to_string();
    let mut hops = 0;
    while is_symlink(&path) {
        if hops >= 40 {
            return None;
        }
        hops += 1;
        let target = std::fs::read_link(&path).ok()?;
        let target = from_disk(&target);
        path = if is_absolute(&target) {
            target
        } else {
            format!("{}/{target}", canonical_dir(&parent(&path))?)
        };
    }
    disk_canonical(&path)
}

/// `find <dir> -type l`: links in walk order, directories entered without
/// following links.
fn collect_links(dir: &str, links: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = format!("{dir}/{}", entry.file_name().disk_text());
        match entry.file_type() {
            Ok(kind) if kind.is_symlink() => links.push(path),
            Ok(kind) if kind.is_dir() => collect_links(&path, links),
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Paths {
        Paths::new("/proj", "/private/proj", Some("/home/me"))
    }

    #[test]
    fn parent_and_leaf_agree_with_dirname_and_basename() {
        let cases = [
            ("/", "/", "/"),
            ("/a", "/", "a"),
            ("/a/", "/", "a"),
            ("/a//b//", "/a", "b"),
            ("a", ".", "a"),
            ("a/b", "a", "b"),
            ("", ".", ""),
        ];
        for (input, dir, base) in cases {
            assert_eq!(parent(input), dir, "dirname {input}");
            assert_eq!(leaf(input), base, "basename {input}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn workspace_projects_are_listed_deepest_first_and_skip_vendored_trees() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        for rel in [
            ".ai/src",
            "b/.ai/src",
            "a/.ai",
            "a/deep/.ai/src/.ai/src",
            "node_modules/pkg/.ai/src",
            ".git/odd/.ai/src",
            ".ai/backups/x/files/.ai/src",
            "bare/.ai",
        ] {
            std::fs::create_dir_all(dir.path().join(rel)).unwrap();
        }
        std::fs::write(dir.path().join("a/.ai/agent_sync.yaml"), "").unwrap();
        std::os::unix::fs::symlink(dir.path().join("b"), dir.path().join("link")).unwrap();
        assert_eq!(
            find_workspace_ai_dirs(&root),
            [
                format!("{root}/a/deep/.ai"),
                format!("{root}/a/.ai"),
                format!("{root}/b/.ai"),
                format!("{root}/.ai"),
            ]
        );
    }

    #[test]
    fn a_workspace_ai_holding_only_exuno_yaml_is_a_project() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().disk_text();
        std::fs::create_dir_all(dir.path().join("c/.ai")).unwrap();
        std::fs::write(dir.path().join("c/.ai/exuno.yaml"), "").unwrap();
        assert_eq!(find_workspace_ai_dirs(&root), [format!("{root}/c/.ai")]);
    }

    #[test]
    fn a_directory_inside_an_ai_tree_names_the_project_above_its_shallowest_ai() {
        assert_eq!(
            ai_dir_enclosing_root("/p/.ai/src/.ai/x").as_deref(),
            Some("/p")
        );
        assert_eq!(ai_dir_enclosing_root("/p/.ai").as_deref(), Some("/p"));
        assert_eq!(ai_dir_enclosing_root("/.ai").as_deref(), Some("/"));
        assert_eq!(ai_dir_enclosing_root("/p/.aix"), None);
    }

    #[test]
    fn normalisation_collapses_dot_segments_lexically() {
        assert_eq!(paths().absolute(".claude/./rules/"), "/proj/.claude/rules");
        assert_eq!(paths().absolute("a/../../x"), "/x");
        assert_eq!(paths().absolute("/a//b/.."), "/a");
    }

    /// `find_parent_ai_src` walks up looking for a `.git`, and stops at the
    /// root by its fixpoint. Windows has no `/` to compare against: a probe
    /// that reaches `C:/` gets `C:/` back forever. The walk-up in a tree with
    /// no repository anywhere is the case that spun (CI run 35457693442).
    #[test]
    fn walking_up_terminates_at_a_root_that_is_its_own_parent() {
        for root in ["/", "C:/", "c:\\"] {
            assert_eq!(parent(root), parent(&parent(root)), "{root} must be stable");
        }
        let dir = tempfile::tempdir().unwrap();
        let start = dir.path().join("a/b");
        std::fs::create_dir_all(&start).unwrap();
        assert_eq!(find_parent_ai_src(&start.disk_text()), None);
    }

    #[test]
    fn a_drive_rooted_path_is_absolute_and_keeps_its_drive() {
        assert!(is_absolute("/x"));
        assert!(is_absolute("C:/Users"));
        assert!(is_absolute("c:\\Users\\x"));
        assert!(is_absolute("C:"));
        assert!(!is_absolute("x/y"));
        assert!(!is_absolute("C"));
        assert!(!is_absolute("Cd:/x"));
        assert_eq!(normalize("C:\\Users\\me\\proj/."), "C:/Users/me/proj");
        assert_eq!(normalize("C:/a/../b//c/"), "C:/b/c");
        assert_eq!(normalize("C:/"), "C:/");
        assert_eq!(parent("C:/Users/me"), "C:/Users");
        assert_eq!(parent("C:/Users"), "C:/");
        assert_eq!(parent("C:/"), "C:/");
        assert_eq!(parent("C:"), "C:/");
        assert_eq!(leaf("C:/Users/me"), "me");
        assert!(is_within("C:/Users/me/.ai", "C:/Users/me"));
        assert_eq!(from_msys("C:/x", Some("MINGW64")), "C:/x");
        assert_eq!(from_msys("/tmp/x", None), "/tmp/x");
    }

    #[test]
    fn a_dest_below_the_root_keeps_its_logical_spelling() {
        let mut log = Log::default();
        let dest = paths().resolve_dest(".claude/rules", "targets.rules.dest for Claude", &mut log);
        assert_eq!(dest.as_deref(), Some("/proj/.claude/rules"));
        assert!(log.lines().is_empty());
    }

    #[test]
    fn an_empty_dest_is_logged_and_rejected() {
        let mut log = Log::default();
        assert_eq!(
            paths().resolve_dest("", "targets.rules.dest for X", &mut log),
            None
        );
        assert_eq!(log.tail(1), ["[ERROR] targets.rules.dest for X is empty"]);
    }

    #[test]
    fn sources_in_the_project_the_engine_and_overlays_are_safe() {
        let p = paths();
        assert!(p.is_safe_source("/private/proj/.ai/src/rules"));
        assert!(p.is_safe_source("/<agentsync>/lib/templates/rules"));
        assert!(p.is_safe_source("/<agentsync-overlay>/base-src/src/skills"));
        assert!(!p.is_safe_source("/private/projection"));
        let mut log = Log::default();
        assert_eq!(
            p.resolve_source(".ai/src/missing", "source.rules", &mut log),
            Some("/proj/.ai/src/missing".to_string())
        );
    }

    #[test]
    fn display_paths_strip_the_root_then_fold_home() {
        let p = paths();
        assert_eq!(p.display("/proj/.claude/rules"), ".claude/rules");
        assert_eq!(p.display("/proj"), ".");
        assert_eq!(p.display("/home/me/x"), "~/x");
        assert_eq!(p.to_repo_relative("/elsewhere"), None);
    }

    #[test]
    fn display_strips_the_canonical_root_when_the_spelled_root_differs() {
        let p = paths();
        assert_eq!(
            p.display("/private/proj/.ai/backups/20260921T130000Z-sync-1"),
            ".ai/backups/20260921T130000Z-sync-1"
        );
        assert_eq!(p.display("/private/proj"), ".");
        assert_eq!(p.display("/private/project/x"), "/private/project/x");
        assert_eq!(p.to_repo_relative("/private/proj/x"), None);
    }

    #[test]
    fn display_names_virtual_paths_by_content_not_engine_root() {
        let p = paths();
        assert_eq!(
            p.display("/<agentsync>/lib/templates/guard/claude.sh"),
            "templates/guard/claude.sh"
        );
        assert_eq!(
            p.display("/<agentsync-overlay>/base-src/src/rules"),
            "rules"
        );
        assert_eq!(
            p.display("/<agentsync-overlay>/shared/src/AGENTS.md"),
            "AGENTS.md"
        );
        assert_eq!(p.display("/elsewhere/x"), "/elsewhere/x");
    }

    #[cfg(unix)]
    #[test]
    fn a_dest_outside_the_root_is_rejected_through_the_disk() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir(&root).unwrap();
        let root = root.disk_text();
        let p = Paths::for_disk_root(&root);
        let mut log = Log::default();
        assert_eq!(
            p.resolve_dest("../outside/x", "targets.rules.dest for X", &mut log),
            None
        );
        assert!(log.tail(1)[0].contains("resolves outside repository root: ../outside/x -> "));
    }

    #[cfg(unix)]
    #[test]
    fn a_symlinked_directory_below_the_root_cannot_carry_a_dest_outside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("proj");
        std::fs::create_dir_all(dir.path().join("outside")).unwrap();
        std::fs::create_dir(&root).unwrap();
        std::os::unix::fs::symlink(dir.path().join("outside"), root.join(".claude")).unwrap();
        let root = root.disk_text();
        let mut log = Log::default();

        let lexical = Paths::for_disk_root(&root);
        assert!(
            lexical
                .resolve_dest(
                    ".claude/rules",
                    "targets.rules.dest for Claude Code",
                    &mut log
                )
                .is_some()
        );

        let on_disk = Paths::on_disk(&root);
        assert_eq!(
            on_disk.resolve_dest(
                ".claude/rules",
                "targets.rules.dest for Claude Code",
                &mut log
            ),
            None
        );
        let outside = std::fs::canonicalize(dir.path().join("outside")).unwrap();
        assert_eq!(
            log.tail(1),
            [format!(
                "[ERROR] targets.rules.dest for Claude Code resolves outside repository root: .claude/rules -> {}/rules",
                outside.disk_text()
            )]
        );
        assert_eq!(
            on_disk.resolve_dest("CLAUDE.md", "targets.agents.dest for Claude Code", &mut log),
            Some(format!("{root}/CLAUDE.md"))
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_logical_root_prefers_pwd_when_it_names_the_same_directory() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        std::fs::create_dir(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let link_text = link.disk_text();
        assert_eq!(logical_root(None, &real, Some(&link_text)), link_text);
        assert_eq!(
            logical_root(None, &real, Some("/nonexistent")),
            real.disk_text()
        );
        assert_eq!(logical_root(Some("/x/y/.."), &real, None), "/x");
    }

    #[cfg(unix)]
    fn disk() -> (tempfile::TempDir, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let base = std::fs::canonicalize(dir.path()).unwrap().disk_text();
        let root = format!("{base}/proj");
        std::fs::create_dir_all(format!("{root}/.ai/src/rules")).unwrap();
        std::fs::create_dir_all(format!("{base}/outside/rules")).unwrap();
        std::fs::write(format!("{base}/outside/rules/o.md"), "o\n").unwrap();
        (dir, base, root)
    }

    #[cfg(unix)]
    #[test]
    fn explicit_sources_are_inside_outside_refused_or_untrusted() {
        let (_dir, base, root) = disk();
        let mut p = Paths::new(&root, &root, Some(&base));
        assert_eq!(
            p.classify_explicit_source(".ai/src/rules"),
            ExplicitSource::Inside
        );
        let outside = format!("{base}/outside/rules");
        assert_eq!(
            p.classify_explicit_source(&outside),
            ExplicitSource::Untrusted(outside.clone())
        );
        assert_eq!(
            p.classify_explicit_source("../outside/rules"),
            ExplicitSource::Untrusted(outside.clone())
        );
        p.trust_external_roots(Some(&format!(
            "relative:/nonexistent:{base}/outside/rules/o.md:{base}/outside"
        )));
        assert_eq!(
            p.classify_explicit_source(&outside),
            ExplicitSource::Outside(outside.clone())
        );
        assert_eq!(
            p.classify_explicit_source("/"),
            ExplicitSource::Refused("/".into())
        );
        assert_eq!(
            p.classify_explicit_source(".."),
            ExplicitSource::Refused(base.clone())
        );
        assert_eq!(
            p.classify_explicit_source(&base),
            ExplicitSource::Refused(base.clone())
        );

        assert!(!p.is_safe_source(&format!("{outside}/o.md")));
        p.register_explicit_roots(vec![outside.clone()]);
        assert!(p.is_safe_source(&format!("{outside}/o.md")));
    }

    #[cfg(unix)]
    #[test]
    fn only_a_directory_entry_is_trusted() {
        let (_dir, base, root) = disk();
        let mut p = Paths::new(&root, &root, None);
        let file = format!("{base}/outside/rules/o.md");
        p.trust_external_roots(Some(&file));
        assert!(!p.is_trusted_external(&file));
    }

    #[cfg(unix)]
    #[test]
    fn the_parent_ai_src_is_the_nearest_ancestors_inside_the_git_repository() {
        let dir = tempfile::tempdir().unwrap();
        let t = disk_canonical(&dir.path().disk_text()).unwrap();
        for sub in [
            "a/.ai/src",
            "a/b/c/.ai/src",
            "a/repo/.git",
            "a/repo/d",
            "a/sub/.git",
            "a/sub/.ai/src",
            "a/sub/e",
            "a/worktree/f",
        ] {
            std::fs::create_dir_all(format!("{t}/{sub}")).unwrap();
        }
        std::fs::write(format!("{t}/a/worktree/.git"), "gitdir: elsewhere\n").unwrap();
        let found = |start: &str| find_parent_ai_src(&format!("{t}/{start}"));
        assert_eq!(found("a/b/c"), Some(format!("{t}/a/.ai/src")));
        assert_eq!(found("a/b"), Some(format!("{t}/a/.ai/src")));
        assert_eq!(found("a"), None);
        assert_eq!(found("a/repo/d"), None);
        assert_eq!(found("a/sub/e"), Some(format!("{t}/a/sub/.ai/src")));
        assert_eq!(found("a/worktree/f"), None);
        assert_eq!(found("a/missing"), None);
    }

    #[cfg(unix)]
    #[test]
    fn a_source_link_escaping_the_project_is_named_unless_trusted() {
        use std::os::unix::fs::symlink;
        let (_dir, base, root) = disk();
        let mut p = Paths::new(&root, &root, None);
        std::fs::create_dir_all(format!("{root}/docs")).unwrap();
        std::fs::write(format!("{root}/docs/shared.md"), "s\n").unwrap();
        symlink(
            format!("{root}/docs/shared.md"),
            format!("{root}/.ai/src/rules/shared.md"),
        )
        .unwrap();
        let roots = vec![format!("{root}/.ai/src")];
        assert_eq!(p.escaping_source_link(&roots), Ok(()));

        symlink(
            "../../../../outside/rules/o.md",
            format!("{root}/.ai/src/rules/leak.md"),
        )
        .unwrap();
        assert_eq!(
            p.escaping_source_link(&roots),
            Err(format!(
                "Source symlink .ai/src/rules/leak.md resolves outside the project: {base}/outside/rules/o.md; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"
            ))
        );
        p.trust_external_roots(Some(&format!("{base}/outside")));
        assert_eq!(p.escaping_source_link(&roots), Ok(()));

        let p = Paths::new(&root, &root, None);
        std::fs::remove_file(format!("{root}/.ai/src/rules/leak.md")).unwrap();
        std::fs::create_dir_all(format!("{root}/vendor/skill")).unwrap();
        symlink(
            format!("{base}/outside/rules/o.md"),
            format!("{root}/vendor/skill/leak.md"),
        )
        .unwrap();
        symlink(
            format!("{root}/vendor/skill"),
            format!("{root}/.ai/src/skill"),
        )
        .unwrap();
        assert_eq!(
            p.escaping_source_link(&roots),
            Err(format!(
                "Source symlink vendor/skill/leak.md resolves outside the project: {base}/outside/rules/o.md; add that directory (or a parent) to AGENTSYNC_EXTERNAL_SOURCE_ROOTS to read it"
            ))
        );

        std::fs::remove_file(format!("{root}/vendor/skill/leak.md")).unwrap();
        symlink("a", format!("{root}/vendor/skill/a")).unwrap();
        assert_eq!(
            p.escaping_source_link(&roots),
            Err("Cannot resolve source symlink: vendor/skill/a".to_string())
        );
    }
}
