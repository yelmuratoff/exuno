//! Agent memory: the stores a tool's `memory:` block declares and the notes in
//! them. No tool is named here; every location comes from the tool YAML.

use std::path::{Path, PathBuf};

use super::content;
use crate::config::tool::Tool;
use crate::config::{skill_metadata, yaml_subset};
use crate::paths::DiskText;

const PROJECT: &str = "{project}";

/// The `memory:` block of one tool, its home resolved.
pub struct Spec {
    pub tool: String,
    home: PathBuf,
    home_from: HomeFrom,
    /// `memory.home`, for when the variable points at a variant's home.
    declared_home: Option<PathBuf>,
    dirs: Vec<String>,
    project_key: String,
    pub index: String,
    pub index_max_lines: Option<usize>,
    pub index_max_bytes: Option<usize>,
    pub types: Vec<String>,
}

/// Where a spec looks: the process's home and variables, and the project.
pub struct Place<'a> {
    pub root: &'a Path,
    pub home: Option<&'a str>,
    pub var: &'a dyn Fn(&str) -> Option<String>,
}

impl Spec {
    /// `None` when the tool declares no `memory.dirs`, or its home needs a
    /// `$HOME` the process does not have.
    pub fn read(tool: &Tool, place: &Place) -> Option<Self> {
        let dirs: Vec<String> = tool
            .filter("memory.dirs")
            .split_whitespace()
            .map(str::to_string)
            .collect();
        if dirs.is_empty() {
            return None;
        }
        let number = |key: &str| tool.value(key).parse::<usize>().ok();
        let declared_home = declared_home(tool, place);
        let (home, home_from) = home(tool, place, declared_home.as_ref())?;
        Some(Self {
            tool: tool.slug.clone(),
            home,
            home_from,
            declared_home,
            dirs,
            project_key: tool.value("memory.project_key"),
            index: tool.value("memory.index"),
            index_max_lines: number("memory.index_max_lines"),
            index_max_bytes: number("memory.index_max_bytes"),
            types: tool
                .filter("memory.types")
                .split_whitespace()
                .map(str::to_string)
                .collect(),
        })
    }

    /// The home's own path, symlinks resolved, when it is a variant's `profile_home`.
    pub fn profile_home(&self) -> Option<PathBuf> {
        matches!(self.home_from, HomeFrom::Profile).then(|| canonical(&self.home))
    }

    /// Reads `memory.home` instead when `memory.home_env` points at a home a
    /// profile variant reads: a session started for that profile sets the
    /// variable, and the variant already lists that home.
    pub fn leave_profile_homes(&mut self, profile_homes: &[PathBuf]) {
        if matches!(self.home_from, HomeFrom::Variable)
            && profile_homes.contains(&canonical(&self.home))
            && let Some(declared) = self.declared_home.take()
        {
            self.home = declared;
            self.home_from = HomeFrom::Declared;
        }
    }
}

enum HomeFrom {
    Profile,
    Variable,
    Declared,
}

/// A variant's `profile_home`, else the directory `memory.home_env` names,
/// else the declared `memory.home`.
fn home(tool: &Tool, place: &Place, declared: Option<&PathBuf>) -> Option<(PathBuf, HomeFrom)> {
    let profile_home = tool.value("profile_home");
    if !profile_home.is_empty() {
        return Some((place.root.join(profile_home), HomeFrom::Profile));
    }
    let variable = tool.value("memory.home_env");
    if !variable.is_empty()
        && let Some(dir) = (place.var)(&variable).filter(|dir| !dir.is_empty())
    {
        return Some((PathBuf::from(dir), HomeFrom::Variable));
    }
    Some((declared?.clone(), HomeFrom::Declared))
}

/// `memory.home`: a leading `~` is `$HOME`, anything else is relative to the
/// project root, and an empty value is the root itself.
fn declared_home(tool: &Tool, place: &Place) -> Option<PathBuf> {
    let declared = tool.value("memory.home");
    match declared.strip_prefix('~') {
        Some(rest) => Some(Path::new(place.home?).join(rest.trim_start_matches(['/', '\\']))),
        None if declared.is_empty() => Some(place.root.to_path_buf()),
        None => Some(place.root.join(declared)),
    }
}

/// `path` with symlinks resolved, or as given when it does not exist.
pub fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// One directory of notes; `project` is empty for a store every project shares.
pub struct Store {
    pub dir: PathBuf,
    pub project: String,
}

/// The stores of `spec` for the current project, or for every project when
/// `all`; a warning when a `{project}` directory cannot be resolved.
pub fn stores(spec: &Spec, root: &Path, all: bool) -> (Vec<Store>, Vec<String>) {
    let mut stores = Vec::new();
    let mut warnings = Vec::new();
    for template in &spec.dirs {
        let Some((prefix, suffix)) = template.split_once(PROJECT) else {
            push_dir(&mut stores, spec.home.join(template), "");
            continue;
        };
        let suffix = suffix.trim_start_matches('/');
        let parent = spec.home.join(prefix);
        if all {
            for dir in crate::cli::sorted_entries(&parent) {
                let project = dir.file_name().map(|n| n.disk_text()).unwrap_or_default();
                push_dir(&mut stores, dir.join(suffix), &project);
            }
            continue;
        }
        match project_key(&spec.project_key, &canonical(&git_root(root))) {
            Some(project) => push_dir(&mut stores, parent.join(&project).join(suffix), &project),
            None => warnings.push(format!(
                "{}: memory.project_key '{}' is not one of: path-dashed",
                spec.tool, spec.project_key
            )),
        }
    }
    (stores, warnings)
}

/// The project key `spec` gives the project at `root`, when it has one.
pub fn current_project(spec: &Spec, root: &Path) -> Option<String> {
    project_key(&spec.project_key, &canonical(&git_root(root)))
}

fn push_dir(stores: &mut Vec<Store>, dir: PathBuf, project: &str) {
    if dir.is_dir() {
        stores.push(Store {
            dir,
            project: project.to_string(),
        });
    }
}

/// The repository `root` is in, else `root` itself. Claude Code shares one
/// memory store across a repository's worktrees and subdirectories
/// (https://code.claude.com/docs/en/memory).
fn git_root(root: &Path) -> PathBuf {
    let Some(top) = root.ancestors().find(|dir| dir.join(".git").exists()) else {
        return root.to_path_buf();
    };
    main_worktree(top).unwrap_or_else(|| top.to_path_buf())
}

/// A linked worktree's main repository, which its memory shares: `.git` is a
/// file reading `gitdir: <dir>`, and `<dir>/commondir` names the main `.git`
/// relative to `<dir>`. `None` for a main worktree.
fn main_worktree(top: &Path) -> Option<PathBuf> {
    let link = std::fs::read_to_string(top.join(".git")).ok()?;
    let gitdir = top.join(link.strip_prefix("gitdir:")?.trim());
    let common = std::fs::read_to_string(gitdir.join("commondir")).ok()?;
    let main_git = crate::paths::normalize(&gitdir.join(common.trim()).disk_text());
    Some(Path::new(&main_git).parent()?.to_path_buf())
}

/// `path-dashed`: every character outside ASCII letters and digits becomes `-`,
/// so `/Users/me/.ai` is `-Users-me--ai`.
pub fn project_key(kind: &str, root: &Path) -> Option<String> {
    match kind {
        "path-dashed" => Some(
            root.disk_text()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect(),
        ),
        _ => None,
    }
}

/// One note, with what its frontmatter declares.
pub struct Note {
    pub path: PathBuf,
    pub rel: String,
    pub name: String,
    pub kind: String,
    pub modified: String,
    pub description: String,
    /// The whole file, lossily decoded.
    pub text: String,
}

impl Note {
    /// The text after the frontmatter, or all of it when there is none.
    pub fn body(&self) -> &str {
        let mut lines = self.text.split_inclusive('\n');
        if lines.next().map(str::trim) != Some("---") {
            return &self.text;
        }
        let mut offset = self.text.find('\n').map_or(self.text.len(), |i| i + 1);
        for line in lines {
            offset += line.len();
            if line.trim() == "---" {
                return &self.text[offset..];
            }
        }
        &self.text
    }
}

/// The Markdown files of a store in byte order, its index left out.
pub fn notes(store: &Store, index: &str) -> Vec<Note> {
    let mut files = Vec::new();
    crate::cli::files_below(&store.dir, &mut files);
    let mut notes: Vec<Note> = files
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "md"))
        .filter_map(|path| {
            let rel = path.strip_prefix(&store.dir).ok()?.disk_text();
            (rel != index).then(|| note(path, rel))
        })
        .collect();
    notes.sort_by(|a, b| a.rel.cmp(&b.rel));
    notes
}

fn note(path: PathBuf, rel: String) -> Note {
    let bytes = std::fs::read(&path).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes);
    let frontmatter = skill_metadata::frontmatter(&text)
        .map(|lines| lines.join("\n"))
        .unwrap_or_default();
    let field = |key: &str| {
        [key.to_string(), format!("metadata.{key}")]
            .iter()
            .map(|path| yaml_subset::value(&frontmatter, path))
            .find(|value| !value.is_empty())
            .unwrap_or_default()
    };
    Note {
        name: skill_metadata::name(&bytes).unwrap_or_default(),
        description: skill_metadata::description(&bytes).unwrap_or_default(),
        kind: field("type"),
        modified: field("modified"),
        text: text.into_owned(),
        rel,
        path,
    }
}

/// What a `check` run holds the same for every store.
pub struct Context<'a> {
    /// Today and the `--stale-days` limit, both in days.
    pub stale: Option<(i64, i64)>,
    pub root: &'a Path,
    pub home: Option<&'a Path>,
}

/// What `check` finds in one store, as `(path, message)` pairs.
pub fn findings(
    spec: &Spec,
    store: &Store,
    notes: &[Note],
    context: &Context,
) -> Vec<(PathBuf, String)> {
    let stale = context.stale;
    let mut found = Vec::new();
    let index_path = store.dir.join(&spec.index);
    let index = (!spec.index.is_empty())
        .then(|| std::fs::read(&index_path).ok())
        .flatten();
    let listed = index.as_ref().map(|bytes| {
        let text = String::from_utf8_lossy(bytes);
        if let Some(message) = oversized(spec, &text, bytes.len()) {
            found.push((index_path.clone(), message));
        }
        let targets = links(&text);
        for target in &targets {
            if !store.dir.join(target).is_file() {
                found.push((
                    index_path.clone(),
                    format!("links to {target}, which does not exist"),
                ));
            }
        }
        targets
    });
    for note in notes {
        if let Some(targets) = &listed
            && !targets.contains(&note.rel)
        {
            found.push((note.path.clone(), format!("not listed in {}", spec.index)));
        }
        if !spec.types.is_empty() {
            if note.kind.is_empty() {
                found.push((note.path.clone(), "no type".to_string()));
            } else if !spec.types.contains(&note.kind) {
                found.push((
                    note.path.clone(),
                    format!(
                        "type '{}' is not one of {}",
                        note.kind,
                        spec.types.join(", ")
                    ),
                ));
            }
        }
        if let Some((today, days)) = stale
            && let Some(day) = day_of(&note.modified)
            && today - day > days
        {
            found.push((
                note.path.clone(),
                format!(
                    "last modified {}, more than {days} days ago",
                    &note.modified[..10]
                ),
            ));
        }
        for path in content::missing_paths(note.body(), context.home) {
            found.push((
                note.path.clone(),
                format!("mentions {path}, which does not exist"),
            ));
        }
        for line in content::credential_lines(&note.text) {
            found.push((
                note.path.clone(),
                format!("line {line} looks like a credential; move it out of memory"),
            ));
        }
    }
    found
}

fn oversized(spec: &Spec, text: &str, bytes: usize) -> Option<String> {
    let lines = text.lines().count();
    let long = spec.index_max_lines.filter(|max| lines > *max);
    let big = spec.index_max_bytes.filter(|max| bytes > *max);
    let size = match (long, big) {
        (None, None) => return None,
        (Some(_), None) => format!("{lines} lines"),
        (None, Some(_)) => format!("{bytes} bytes"),
        (Some(_), Some(_)) => format!("{lines} lines and {bytes} bytes"),
    };
    let limit = match (spec.index_max_lines, spec.index_max_bytes) {
        (Some(lines), Some(bytes)) => format!("{lines} lines or {bytes} bytes"),
        (Some(lines), None) => format!("{lines} lines"),
        (None, Some(bytes)) => format!("{bytes} bytes"),
        (None, None) => unreachable!("a size is over a limit only when one is set"),
    };
    Some(format!("{size}; only the first {limit} load"))
}

/// Local Markdown targets of `[text](target.md)` links, without a leading
/// `./` or a `#fragment`.
fn links(text: &str) -> Vec<String> {
    text.split("](")
        .skip(1)
        .filter_map(|rest| rest.split_once(')').map(|(target, _)| target.trim()))
        .map(|target| target.split_once('#').map_or(target, |(path, _)| path))
        .map(|target| target.strip_prefix("./").unwrap_or(target))
        .filter(|target| target.ends_with(".md") && !target.contains("://"))
        .map(str::to_string)
        .collect()
}

/// The day number of a value starting `YYYY-MM-DD`.
fn day_of(value: &str) -> Option<i64> {
    let date = value.get(..10)?;
    let shaped = date.bytes().enumerate().all(|(i, byte)| match i {
        4 | 7 => byte == b'-',
        _ => byte.is_ascii_digit(),
    });
    if !shaped {
        return None;
    }
    let number = |range: std::ops::Range<usize>| date[range].parse::<i64>().ok();
    Some(crate::transaction::backup::days_from_civil(
        number(0..4)?,
        number(5..7)?,
        number(8..10)?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_dashed_turns_every_non_alphanumeric_into_a_dash() {
        assert_eq!(
            project_key("path-dashed", Path::new("/Users/me/.ai")).as_deref(),
            Some("-Users-me--ai")
        );
        assert_eq!(project_key("unknown", Path::new("/x")), None);
    }

    #[test]
    fn links_keep_local_markdown_targets_only() {
        assert_eq!(
            links(
                "- [a](a.md) — x\n- [b](https://e.com/b.md)\n- [c](c.txt)\n- [d]( d.md )\n- [e](./e.md#top)\n"
            ),
            ["a.md", "d.md", "e.md"]
        );
    }

    #[test]
    fn day_of_reads_a_leading_date() {
        assert_eq!(day_of("1970-01-02T00:00:00Z"), Some(1));
        assert_eq!(day_of("2026-11-20"), day_of("2026-11-20T10:00:00.000Z"));
        assert_eq!(day_of("yesterday"), None);
        assert_eq!(day_of("2026/11/20"), None);
    }
}
