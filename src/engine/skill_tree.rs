//! Skills below a skills source: a directory holding `SKILL.md` is a skill,
//! any other directory is a category walked for more skills. Every host
//! installs skills flat by name, so `rel` is where a skill lives in the source
//! and `name` is the directory it lands in.

use std::collections::{BTreeMap, BTreeSet};

use crate::config::skill_metadata;
use crate::engine::workspace::Workspace;
use crate::paths;

/// Category levels walked below the skills root.
pub const MAX_CATEGORY_DEPTH: usize = 4;

/// Text a project appends to the `SKILL.md` of an inherited skill it extends.
pub const APPENDIX: &str = "SKILL.append.md";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub rel: String,
}

impl Skill {
    /// The skill at source path `rel`, named after its last segment.
    pub fn at(rel: &str) -> Self {
        Self {
            name: paths::leaf(rel).to_string(),
            rel: rel.to_string(),
        }
    }

    /// The category path, empty for a skill at the root.
    pub fn category(&self) -> &str {
        self.rel
            .rsplit_once('/')
            .map_or("", |(category, _)| category)
    }
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Tree {
    pub skills: Vec<Skill>,
    /// Regular files at the root, synced as they are.
    pub files: Vec<String>,
    /// Categories holding no skill at any depth.
    pub empty_categories: Vec<String>,
    /// Directories below `MAX_CATEGORY_DEPTH` categories, never walked.
    pub too_deep: Vec<String>,
}

impl Tree {
    pub fn find(&self, name: &str) -> Option<&Skill> {
        self.skills.iter().find(|skill| skill.name == name)
    }

    /// The category named `name` that holds no skill: it extends an inherited
    /// skill of that name instead of replacing it.
    pub fn extension(&self, name: &str) -> Option<&str> {
        self.empty_categories
            .iter()
            .map(String::as_str)
            .find(|rel| paths::leaf(rel) == name)
    }

    /// `rel` below the skills root as a flat `<name>/…` path, moved into the
    /// category that holds the skill or extension of that name; unchanged when
    /// none does.
    pub fn locate(&self, rel: &str) -> String {
        let (name, rest) = rel.split_once('/').unwrap_or((rel, ""));
        let found = self
            .find(name)
            .map(|skill| skill.rel.as_str())
            .or_else(|| self.extension(name));
        match found {
            Some(at) if rest.is_empty() => at.to_string(),
            Some(at) => format!("{at}/{rest}"),
            None => rel.to_string(),
        }
    }

    /// `empty_categories` without the extensions of the skills `inherited`
    /// names, the directories inside them, and the categories holding them.
    pub fn empty_categories_besides(&self, inherited: &[String]) -> Vec<&str> {
        let extensions: Vec<String> = inherited
            .iter()
            .filter_map(|name| self.extension(name).map(str::to_string))
            .collect();
        self.empty_categories
            .iter()
            .map(String::as_str)
            .filter(|rel| {
                !extensions
                    .iter()
                    .any(|ext| ext == rel || below(rel, ext) || below(ext, rel))
            })
            .collect()
    }

    /// Category paths whose last segment breaks the lowercase-kebab naming
    /// `exuno add --category` enforces, each reported once.
    pub fn nonstandard_categories(&self) -> Vec<String> {
        let categories = self
            .skills
            .iter()
            .map(Skill::category)
            .chain(self.empty_categories.iter().map(String::as_str));
        let mut found = BTreeSet::new();
        for category in categories.filter(|category| !category.is_empty()) {
            let mut prefix = String::new();
            for segment in category.split('/') {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(segment);
                if !skill_metadata::valid_name(segment) {
                    found.insert(prefix.clone());
                    break;
                }
            }
        }
        found.into_iter().collect()
    }
}

/// Paths of `parent` skills a `child` skill of the same name replaces from
/// another path; a skill at the same path merges file by file instead.
pub fn shadowed(child: &Tree, parent: &Tree) -> Vec<String> {
    parent
        .skills
        .iter()
        .filter(|skill| {
            child
                .find(&skill.name)
                .is_some_and(|own| own.rel != skill.rel)
        })
        .map(|skill| skill.rel.clone())
        .collect()
}

/// Where each `parent` skill lands in the `child` tree when the child has no
/// `SKILL.md` of that name, as `(parent path, child path)`: at the child's
/// extension of that name, else at the parent's own path.
pub fn placements(child: &Tree, parent: &Tree) -> Vec<(String, String)> {
    parent
        .skills
        .iter()
        .filter(|skill| child.find(&skill.name).is_none())
        .map(|skill| {
            let target = child.extension(&skill.name).unwrap_or(&skill.rel);
            (skill.rel.clone(), target.to_string())
        })
        .collect()
}

/// `rel` below the parent skills root, moved to where `placements` lands its skill.
pub fn relocate(rel: &str, placements: &[(String, String)]) -> String {
    placements
        .iter()
        .find(|(from, _)| below(rel, from))
        .map_or_else(
            || rel.to_string(),
            |(from, to)| format!("{to}{}", &rel[from.len()..]),
        )
}

/// Whether the source path `rel` lies inside one of the skill paths in `skills`.
pub fn is_inside(rel: &str, skills: &[String]) -> bool {
    skills.iter().any(|skill| below(rel, skill))
}

fn below(rel: &str, dir: &str) -> bool {
    rel.strip_prefix(dir)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Every name more than one skill claims, with the paths that claim it.
pub fn collisions(skills: &[Skill]) -> Vec<(&str, Vec<&str>)> {
    let mut by_name: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for skill in skills {
        by_name.entry(&skill.name).or_default().push(&skill.rel);
    }
    by_name
        .into_iter()
        .filter(|(_, rels)| rels.len() > 1)
        .collect()
}

pub fn discover(ws: &Workspace, root: &str) -> Tree {
    let mut walk = Walk {
        ws,
        root,
        tree: Tree::default(),
    };
    for name in ws.glob(root) {
        let path = format!("{root}/{name}");
        if ws.is_dir(&path) {
            walk.dir(&name, 0);
        } else if ws.is_file(&path) {
            walk.tree.files.push(name);
        }
    }
    walk.tree
}

struct Walk<'a> {
    ws: &'a Workspace,
    root: &'a str,
    tree: Tree,
}

impl Walk<'_> {
    /// Whether `rel` holds a skill or a too-deep directory, so its parent is
    /// not reported empty as well.
    fn dir(&mut self, rel: &str, depth: usize) -> bool {
        let dir = format!("{}/{rel}", self.root);
        if self.ws.is_file(&format!("{dir}/SKILL.md")) {
            self.tree.skills.push(Skill::at(rel));
            return true;
        }
        if depth >= MAX_CATEGORY_DEPTH {
            self.tree.too_deep.push(rel.to_string());
            return true;
        }
        let mut occupied = false;
        for child in self.ws.glob(&dir) {
            let child_rel = format!("{rel}/{child}");
            if self.ws.is_dir(&format!("{}/{child_rel}", self.root)) {
                occupied |= self.dir(&child_rel, depth + 1);
            }
        }
        if !occupied {
            self.tree.empty_categories.push(rel.to_string());
        }
        occupied
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::workspace::Content;

    fn tree(files: &[&str]) -> Tree {
        let mut ws = Workspace::new("/proj");
        for path in files {
            ws.insert_file(
                &format!("/proj/skills/{path}"),
                Content::Bytes(b"x".to_vec()),
            );
        }
        discover(&ws, "/proj/skills")
    }

    fn skill(name: &str, rel: &str) -> Skill {
        Skill {
            name: name.into(),
            rel: rel.into(),
        }
    }

    #[test]
    fn a_flat_source_is_one_skill_per_directory() {
        let found = tree(&["a/SKILL.md", "b/SKILL.md", "b/references/r.md", "README.md"]);
        assert_eq!(found.skills, [skill("a", "a"), skill("b", "b")]);
        assert_eq!(found.files, ["README.md"]);
        assert!(found.empty_categories.is_empty());
    }

    #[test]
    fn a_directory_without_skill_md_is_a_category() {
        let found = tree(&[
            "flutter/bloc/SKILL.md",
            "flutter/ui/slivers/SKILL.md",
            "flutter/ui/slivers/references/nested/SKILL.md",
            "flutter/notes.md",
            "commit/SKILL.md",
        ]);
        assert_eq!(
            found.skills,
            [
                skill("commit", "commit"),
                skill("bloc", "flutter/bloc"),
                skill("slivers", "flutter/ui/slivers"),
            ]
        );
        assert_eq!(found.skills[2].category(), "flutter/ui");
        assert_eq!(found.skills[0].category(), "");
        assert!(found.files.is_empty());
    }

    #[test]
    fn a_directory_named_after_an_inherited_skill_without_skill_md_extends_it() {
        let parent = tree(&["base/SKILL.md", "other/SKILL.md", "gone/SKILL.md"]);
        let child = tree(&[
            "meta/base/SKILL.append.md",
            "meta/base/references/r.md",
            "cat/gone/SKILL.md",
        ]);
        assert_eq!(child.extension("base"), Some("meta/base"));
        assert_eq!(
            child.empty_categories_besides(&["base".to_string()]),
            Vec::<&str>::new()
        );
        assert_eq!(
            child.empty_categories_besides(&[]),
            ["meta/base/references", "meta/base", "meta"]
        );
        let placed = placements(&child, &parent);
        assert_eq!(
            placed,
            [
                ("base".to_string(), "meta/base".to_string()),
                ("other".to_string(), "other".to_string()),
            ]
        );
        assert_eq!(relocate("base/SKILL.md", &placed), "meta/base/SKILL.md");
        assert_eq!(relocate("base-x/SKILL.md", &placed), "base-x/SKILL.md");
        assert_eq!(relocate("gone/SKILL.md", &placed), "gone/SKILL.md");
        assert_eq!(
            child.locate("base/references/r.md"),
            "meta/base/references/r.md"
        );
    }

    #[test]
    fn hidden_directories_are_skipped() {
        let found = tree(&[".git/x/SKILL.md", "cat/.draft/SKILL.md", "cat/a/SKILL.md"]);
        assert_eq!(found.skills, [skill("a", "cat/a")]);
    }

    #[test]
    fn empty_and_too_deep_categories_are_reported_once() {
        let found = tree(&["empty/inner/notes.md", "full/a/SKILL.md", "full/b/notes.md"]);
        assert_eq!(found.skills, [skill("a", "full/a")]);
        assert_eq!(found.empty_categories, ["empty/inner", "empty", "full/b"]);
        assert!(found.too_deep.is_empty());

        let found = tree(&["a/b/c/d/e/SKILL.md", "a/b/c/d/f/g/SKILL.md"]);
        assert_eq!(found.skills, [skill("e", "a/b/c/d/e")]);
        assert_eq!(found.too_deep, ["a/b/c/d/f"]);
        assert!(found.empty_categories.is_empty());
    }

    #[test]
    fn a_child_skill_shadows_a_parent_skill_of_its_name_at_another_path() {
        let child = tree(&["meta/agentsync/SKILL.md", "a/SKILL.md"]);
        let parent = tree(&["agentsync/SKILL.md", "a/SKILL.md", "b/SKILL.md"]);
        let shadowed = shadowed(&child, &parent);
        assert_eq!(shadowed, ["agentsync"]);
        assert!(is_inside("agentsync/SKILL.md", &shadowed));
        assert!(is_inside("agentsync/references/r.md", &shadowed));
        assert!(!is_inside("agentsync-extra/SKILL.md", &shadowed));
        assert!(!is_inside("agentsync", &shadowed));
    }

    #[test]
    fn a_flat_path_is_located_in_the_category_holding_its_skill() {
        let found = tree(&["git/commit/SKILL.md", "debug/SKILL.md"]);
        assert_eq!(found.locate("commit/SKILL.md"), "git/commit/SKILL.md");
        assert_eq!(
            found.locate("commit/references/r.md"),
            "git/commit/references/r.md"
        );
        assert_eq!(found.locate("commit"), "git/commit");
        assert_eq!(found.locate("debug/SKILL.md"), "debug/SKILL.md");
        assert_eq!(found.locate("gone/SKILL.md"), "gone/SKILL.md");
    }

    #[test]
    fn nonstandard_categories_name_the_first_offending_segment_once() {
        let found = tree(&[
            "Flutter/bloc/SKILL.md",
            "Flutter/ui/slivers/SKILL.md",
            "backend/My_API/auth/SKILL.md",
            "ok-cat/skill/SKILL.md",
            "Empty_Dir/notes.md",
            "root-skill/SKILL.md",
        ]);
        assert_eq!(
            found.nonstandard_categories(),
            ["Empty_Dir", "Flutter", "backend/My_API"]
        );
    }

    #[test]
    fn collisions_list_every_path_claiming_a_name() {
        let found = tree(&[
            "backend/auth/SKILL.md",
            "flutter/auth/SKILL.md",
            "bloc/SKILL.md",
        ]);
        assert_eq!(
            collisions(&found.skills),
            [("auth", vec!["backend/auth", "flutter/auth"])]
        );
        assert_eq!(
            found.find("auth").map(|s| s.rel.as_str()),
            Some("backend/auth")
        );
        assert!(found.find("nope").is_none());
    }
}
