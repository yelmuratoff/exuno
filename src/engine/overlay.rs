//! Source overlays of `lib/helpers/shared.sh`: the engine-owned skill layer,
//! per-profile overlays, and the `shared:` parent files `lib/check.sh` merges
//! into its workspace. Overlay trees live under the virtual overlay root.

use crate::paths::DiskText;
use std::path::{Path, PathBuf};

use crate::engine::session::Session;
use crate::engine::skill_tree;
use crate::engine::workspace::{Content, Workspace};
use crate::paths::{self, ENGINE_ROOT, OVERLAY_ROOT};
use crate::{Error, config::profiles, config::yaml_subset};

const CATEGORIES: [&str; 4] = ["rules", "skills", "commands", "agents"];

/// The `SOURCE_*` paths a render reads from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Sources {
    pub agents: String,
    pub rules: String,
    pub skills: String,
    pub commands: String,
    pub subagents: String,
}

/// `build_overlay_tree`: mirror the child's `AGENTS.md` and category trees,
/// then fill each category with parent files the child lacks. Returns the
/// overlay directory; its `src/` holds the tree.
pub fn build_tree(
    ws: &mut Workspace,
    name: &str,
    child_src: &str,
    parent_src: &str,
    categories: &[&str],
) -> Result<String, Error> {
    let dir = format!("{OVERLAY_ROOT}/{name}");
    ws.remove(&dir)?;
    let src = format!("{dir}/src");
    ws.create_dir_all(&src)?;

    if ws.is_dir(child_src) {
        let agents = format!("{child_src}/AGENTS.md");
        if ws.is_file(&agents) {
            ws.copy(&agents, &format!("{src}/AGENTS.md"))?;
        }
        for item in CATEGORIES {
            let from = format!("{child_src}/{item}");
            if ws.is_dir(&from) {
                ws.copy(&from, &format!("{src}/{item}"))?;
            }
        }
    }

    fill_parent(ws, &src, parent_src, categories, &[])?;
    Ok(dir)
}

/// `build_source_overlay_tree`: mirror each resolved source, then fill the
/// inherited categories from the parent. A category whose source resolves
/// outside the safe roots is neither mirrored nor filled.
pub fn build_source_tree(
    s: &mut Session,
    name: &str,
    sources: &Sources,
    parent_src: &str,
    categories: &[&str],
) -> Result<String, Error> {
    let dir = format!("{OVERLAY_ROOT}/{name}");
    s.ws.remove(&dir)?;
    let src = format!("{dir}/src");
    s.ws.create_dir_all(&src)?;
    mirror_source(s, true, &sources.agents, &format!("{src}/AGENTS.md"))?;
    let mut refused = Vec::new();
    for (category, raw) in [
        ("rules", &sources.rules),
        ("skills", &sources.skills),
        ("commands", &sources.commands),
        ("agents", &sources.subagents),
    ] {
        if !mirror_source(s, false, raw, &format!("{src}/{category}"))? {
            refused.push(category);
        }
    }
    fill_parent(&mut s.ws, &src, parent_src, categories, &refused)?;
    Ok(dir)
}

/// `_overlay_mirror_source`: `false`, copying nothing, when the source
/// resolves outside the safe roots.
fn mirror_source(s: &mut Session, file: bool, raw: &str, dest: &str) -> Result<bool, Error> {
    if raw.is_empty() {
        return Ok(true);
    }
    let abs = s.paths.absolute(raw);
    let present = if file {
        s.ws.is_file(&abs)
    } else {
        s.ws.is_dir(&abs)
    };
    if !present {
        return Ok(true);
    }
    let Some(canonical) = s.paths.canonicalize_with_existing_ancestor(&abs) else {
        return Ok(false);
    };
    if !s.paths.is_safe_source(&canonical) {
        return Ok(false);
    }
    s.ws.copy(&abs, dest)?;
    Ok(true)
}

/// `_overlay_fill_parent`: parent files of the inherited categories the tree
/// lacks, except categories in `skipped`.
fn fill_parent(
    ws: &mut Workspace,
    src: &str,
    parent_src: &str,
    categories: &[&str],
    skipped: &[&str],
) -> Result<(), Error> {
    for category in categories {
        if skipped.contains(category) {
            continue;
        }
        let parent_dir = format!("{parent_src}/{category}");
        if !ws.is_dir(&parent_dir) {
            continue;
        }
        let (shadowed, placements) = if *category == "skills" {
            let child = skill_tree::discover(ws, &format!("{src}/skills"));
            let parent = skill_tree::discover(ws, &parent_dir);
            (
                skill_tree::shadowed(&child, &parent),
                skill_tree::placements(&child, &parent),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        for file in ws.files_under(&parent_dir) {
            let rel = &file[parent_dir.len() + 1..];
            if skill_tree::is_inside(rel, &shadowed) {
                continue;
            }
            let target = format!(
                "{src}/{category}/{}",
                skill_tree::relocate(rel, &placements)
            );
            if ws.exists(&target) {
                continue;
            }
            ws.create_dir_all(&paths::parent(&target))?;
            ws.copy(&file, &target)?;
        }
        for (_, rel) in &placements {
            append_skill_additions(ws, &format!("{src}/skills/{rel}"))?;
        }
    }
    Ok(())
}

/// Appends the project's `SKILL.append.md` to the inherited `SKILL.md` it
/// extends; the appendix itself never reaches a tool.
fn append_skill_additions(ws: &mut Workspace, skill: &str) -> Result<(), Error> {
    let appendix = format!("{skill}/{}", skill_tree::APPENDIX);
    let skill_md = format!("{skill}/SKILL.md");
    if !ws.is_file(&appendix) || !ws.is_file(&skill_md) {
        return Ok(());
    }
    let mut text = ws.read(&skill_md)?;
    text.truncate(text.trim_ascii_end().len());
    text.extend_from_slice(b"\n\n");
    text.extend(ws.read(&appendix)?);
    if !text.ends_with(b"\n") {
        text.push(b'\n');
    }
    ws.write(&skill_md, text)?;
    ws.remove(&appendix)
}

/// `_overlay_rewrite_sources`: only the paths the overlay materialised.
pub fn rewrite_sources(ws: &Workspace, dir: &str, sources: &mut Sources) {
    let src = format!("{dir}/src");
    if ws.is_file(&format!("{src}/AGENTS.md")) {
        sources.agents = format!("{src}/AGENTS.md");
    }
    for (category, slot) in [
        ("rules", &mut sources.rules),
        ("skills", &mut sources.skills),
        ("commands", &mut sources.commands),
        ("agents", &mut sources.subagents),
    ] {
        let path = format!("{src}/{category}");
        if ws.is_dir(&path) {
            *slot = path;
        }
    }
}

/// `shared_setup_overlay`: the parent's files of the inherited categories fill
/// what the project lacks. Returns the overlay directory when one was built.
pub fn setup_shared(
    s: &mut Session,
    config: &str,
    sources: &mut Sources,
) -> Result<Option<String>, Error> {
    let raw_path = yaml_subset::value(config, "shared.path");
    let raw_inherit = yaml_subset::value(config, "shared.inherit");
    if raw_path.is_empty() || raw_inherit.is_empty() {
        return Ok(None);
    }
    let root = s.paths.root.clone();
    let parent_root = if crate::paths::is_absolute(&raw_path) {
        raw_path.clone()
    } else {
        format!("{root}/{raw_path}")
    };
    if !Path::new(&parent_root).is_dir() {
        s.log.warning(&format!(
            "shared.path does not exist: {raw_path} — overlay skipped"
        ));
        return Ok(None);
    }
    let parent_root = paths::normalize(&parent_root);
    let nested = format!("{parent_root}/.ai/src");
    let parent_src = if Path::new(&nested).is_dir() {
        nested
    } else if paths::leaf(&parent_root) == "src" {
        parent_root
    } else {
        s.log.warning(&format!(
            "shared.path has no .ai/src/: {raw_path} — overlay skipped"
        ));
        return Ok(None);
    };
    if parent_src == format!("{root}/.ai/src") {
        s.log
            .warning("shared.path resolves to this project — overlay skipped");
        return Ok(None);
    }

    let mut categories: Vec<&str> = Vec::new();
    for token in raw_inherit
        .split([',', ' ', '\t', '\n'])
        .filter(|t| !t.is_empty())
    {
        match token {
            "subagents" | "agents" => categories.push("agents"),
            "rules" => categories.push("rules"),
            "skills" => categories.push("skills"),
            "commands" => categories.push("commands"),
            unknown => s.log.warning(&format!(
                "shared.inherit: unknown category '{unknown}' — skipped"
            )),
        }
    }
    if categories.is_empty() {
        return Ok(None);
    }
    let dir = build_source_tree(s, "shared", &sources.clone(), &parent_src, &categories)?;
    rewrite_sources(&s.ws, &dir, sources);
    s.log.info(&format!(
        "Shared overlay active: {parent_src} ({})",
        categories.join(",")
    ));
    Ok(Some(dir))
}

/// `base_src_setup_overlay`: engine-owned skills fill paths the project, and a
/// `shared:` parent composed into `child_src`, lack, unless `base_skills: false`.
/// Returns the overlay directory when one was built.
pub fn setup_base_src(
    s: &mut Session,
    config: Option<&str>,
    child_src: &str,
    sources: &mut Sources,
) -> Result<Option<String>, Error> {
    let base_src = format!("{ENGINE_ROOT}/lib/templates/base-src");
    if !s.ws.is_dir(&format!("{base_src}/skills")) {
        return Ok(None);
    }
    if config.is_some_and(|text| yaml_subset::value(text, "base_skills") == "false") {
        return Ok(None);
    }
    if !s.ws.is_dir(child_src) {
        return Ok(None);
    }
    let dir = build_source_tree(s, "base-src", &sources.clone(), &base_src, &["skills"])?;
    rewrite_sources(&s.ws, &dir, sources);
    Ok(Some(dir))
}

/// `profile_setup_overlay`: false when the profile has no `src/` of its own.
pub fn setup_profile(
    s: &mut Session,
    config: &str,
    name: &str,
    base_src: &str,
    sources: &mut Sources,
) -> Result<bool, Error> {
    let overlay = profiles::overlay_dir(config, name);
    let overlay_root = if crate::paths::is_absolute(&overlay) {
        overlay
    } else {
        format!("{}/{overlay}", s.paths.root)
    };
    let profile_src = format!("{overlay_root}/src");
    if !s.ws.is_dir(&profile_src) {
        return Ok(false);
    }
    let dir = build_tree(&mut s.ws, "profile", &profile_src, base_src, &CATEGORIES)?;
    rewrite_sources(&s.ws, &dir, sources);
    s.log
        .info(&format!("Profile overlay active: {name} ({profile_src})"));
    Ok(true)
}

pub fn cleanup_profile(ws: &mut Workspace) -> Result<(), Error> {
    ws.remove(&format!("{OVERLAY_ROOT}/profile"))
}

/// `shared_parent_src`: the parent's `.ai/src/`, resolved on disk from the root.
pub fn shared_parent_src(config: &str, root: &str) -> Option<String> {
    let raw = yaml_subset::value(config, "shared.path");
    if raw.is_empty() {
        return None;
    }
    let parent_root = if crate::paths::is_absolute(&raw) {
        raw
    } else {
        format!("{root}/{raw}")
    };
    if !Path::new(&parent_root).is_dir() {
        return None;
    }
    let parent_root = paths::normalize(&parent_root);
    let nested = format!("{parent_root}/.ai/src");
    let parent_src = if Path::new(&nested).is_dir() {
        nested
    } else if paths::leaf(&parent_root) == "src" {
        parent_root
    } else {
        return None;
    };
    (parent_src != format!("{root}/.ai/src")).then_some(parent_src)
}

/// `shared_inherit_categories`: the inherit tokens sync materialises.
pub fn inherit_categories(raw: &str) -> Vec<&'static str> {
    raw.split([',', ' ', '\t', '\n'])
        .filter_map(|token| match token {
            "subagents" | "agents" => Some("agents"),
            "rules" => Some("rules"),
            "skills" => Some("skills"),
            "commands" => Some("commands"),
            _ => None,
        })
        .collect()
}

/// The `shared:` block of `lib/check.sh`: parent files of the inherited
/// categories are merged into the workspace's `.ai/src/` where the project
/// has no file at that path, read from disk as `find -type f` lists them.
pub fn merge_shared_parent(
    ws: &mut Workspace,
    parent_src: &str,
    categories: &[&str],
) -> Result<(), Error> {
    let child_src = format!("{}/.ai/src", ws.root());
    ws.create_dir_all(&child_src)?;
    for category in categories {
        let parent_dir = PathBuf::from(format!("{parent_src}/{category}"));
        if !parent_dir.is_dir() {
            continue;
        }
        let (shadowed, placements) = if *category == "skills" {
            let child = skill_tree::discover(ws, &format!("{child_src}/skills"));
            let parent = skill_tree::discover(
                &Workspace::on_disk(parent_src),
                &format!("{parent_src}/skills"),
            );
            (
                skill_tree::shadowed(&child, &parent),
                skill_tree::placements(&child, &parent),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        let mut files = Vec::new();
        collect_regular_files(&parent_dir, "", &mut files);
        for (rel, disk) in files {
            if skill_tree::is_inside(&rel, &shadowed) {
                continue;
            }
            let target = format!(
                "{child_src}/{category}/{}",
                skill_tree::relocate(&rel, &placements)
            );
            if !ws.exists(&target) {
                ws.insert_file(&target, Content::Disk(disk));
            }
        }
        for (_, rel) in &placements {
            append_skill_additions(ws, &format!("{child_src}/skills/{rel}"))?;
        }
    }
    Ok(())
}

fn collect_regular_files(dir: &Path, rel: &str, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let name = entry.file_name().disk_text();
        let child_rel = if rel.is_empty() {
            name
        } else {
            format!("{rel}/{name}")
        };
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.is_dir() {
            collect_regular_files(&entry.path(), &child_rel, out);
        } else if meta.is_file() {
            out.push((child_rel, entry.path()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::session::test_session;

    fn file(ws: &mut Workspace, path: &str, text: &str) {
        ws.insert_file(path, Content::Bytes(text.as_bytes().to_vec()));
    }

    #[test]
    fn the_child_wins_and_the_parent_fills_only_listed_categories() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "child");
        file(&mut s.ws, "/proj/.ai/src/skills/a/SKILL.md", "child a");
        file(&mut s.ws, "/proj/parent/skills/a/SKILL.md", "parent a");
        file(&mut s.ws, "/proj/parent/skills/a/extra.md", "parent extra");
        file(&mut s.ws, "/proj/parent/rules/p.md", "parent rule");
        let dir = build_tree(&mut s.ws, "t", "/proj/.ai/src", "/proj/parent", &["skills"]).unwrap();
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/SKILL.md")).unwrap(),
            b"child a"
        );
        assert_eq!(
            s.ws.read(&format!("{dir}/src/skills/a/extra.md")).unwrap(),
            b"parent extra"
        );
        assert!(!s.ws.exists(&format!("{dir}/src/rules")));

        let mut sources = Sources {
            rules: "/proj/.ai/src/rules".into(),
            ..Sources::default()
        };
        rewrite_sources(&s.ws, &dir, &mut sources);
        assert_eq!(sources.agents, "/<agentsync-overlay>/t/src/AGENTS.md");
        assert_eq!(sources.skills, "/<agentsync-overlay>/t/src/skills");
        assert_eq!(sources.rules, "/proj/.ai/src/rules");
    }

    #[test]
    fn the_base_skill_layer_is_skipped_by_base_skills_false() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "a");
        let mut sources = Sources::default();
        setup_base_src(
            &mut s,
            Some("base_skills: false\n"),
            "/proj/.ai/src",
            &mut sources,
        )
        .unwrap();
        assert_eq!(sources, Sources::default());
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();
        assert_eq!(sources.skills, "/<agentsync-overlay>/base-src/src/skills");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/exuno/SKILL.md"));
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_parent_fills_the_inherited_categories_and_explains_every_skip() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("child").disk_text();
        std::fs::create_dir_all(dir.path().join("child/.ai/src/rules")).unwrap();
        std::fs::write(dir.path().join("child/.ai/src/AGENTS.md"), "child").unwrap();
        std::fs::create_dir_all(dir.path().join("parent/.ai/src/skills/p")).unwrap();
        std::fs::write(dir.path().join("parent/.ai/src/skills/p/SKILL.md"), "p").unwrap();
        let mut s = Session::new(
            Workspace::on_disk(&root),
            crate::paths::Paths::on_disk(&root),
        );
        let mut sources = Sources {
            agents: ".ai/src/AGENTS.md".into(),
            rules: ".ai/src/rules".into(),
            ..Sources::default()
        };

        let config = "shared:\n  path: \"../parent\"\n  inherit: skills, tools\n";
        let overlay = setup_shared(&mut s, config, &mut sources).unwrap();
        assert_eq!(overlay.as_deref(), Some("/<agentsync-overlay>/shared"));
        assert_eq!(sources.skills, "/<agentsync-overlay>/shared/src/skills");
        assert_eq!(sources.agents, "/<agentsync-overlay>/shared/src/AGENTS.md");
        assert!(s.ws.is_file("/<agentsync-overlay>/shared/src/skills/p/SKILL.md"));
        let parent = format!("{}/parent/.ai/src", dir.path().disk_text());
        assert_eq!(
            s.log.tail(2),
            [
                "[WARNING] shared.inherit: unknown category 'tools' — skipped".to_string(),
                format!("[INFO] Shared overlay active: {parent} (skills)"),
            ]
        );

        for (config, line) in [
            (
                "shared:\n  path: missing\n  inherit: rules\n",
                "[WARNING] shared.path does not exist: missing — overlay skipped",
            ),
            (
                "shared:\n  path: \"..\"\n  inherit: rules\n",
                "[WARNING] shared.path has no .ai/src/: .. — overlay skipped",
            ),
            (
                "shared:\n  path: \".\"\n  inherit: rules\n",
                "[WARNING] shared.path resolves to this project — overlay skipped",
            ),
        ] {
            assert_eq!(setup_shared(&mut s, config, &mut sources).unwrap(), None);
            assert_eq!(s.log.tail(1), [line]);
        }
    }

    #[test]
    fn the_engine_skill_layer_keeps_configured_sources() {
        let mut s = test_session();
        file(&mut s.ws, "/proj/.ai/src/AGENTS.md", "# Project\n");
        file(&mut s.ws, "/proj/shared-rules/r.md", "r\n");
        let mut sources = Sources {
            agents: ".ai/src/AGENTS.md".into(),
            rules: "shared-rules".into(),
            ..Sources::default()
        };
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();
        assert_eq!(sources.rules, "/<agentsync-overlay>/base-src/src/rules");
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/rules/r.md"));
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/exuno/SKILL.md"));
    }

    #[test]
    fn an_engine_skill_extension_in_a_category_adds_files_and_appends_to_skill_md() {
        let mut s = test_session();
        let ext = "/proj/.ai/src/skills/meta/exuno";
        file(
            &mut s.ws,
            &format!("{ext}/SKILL.append.md"),
            "## Local\n\nOurs.",
        );
        file(&mut s.ws, &format!("{ext}/references/local.md"), "local\n");
        file(
            &mut s.ws,
            &format!("{ext}/references/maintenance.md"),
            "mine\n",
        );
        file(
            &mut s.ws,
            "/proj/.ai/src/skills/meta/sub/own/SKILL.md",
            "own\n",
        );
        let mut sources = Sources {
            skills: ".ai/src/skills".into(),
            ..Sources::default()
        };
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();

        let skill = "/<agentsync-overlay>/base-src/src/skills/meta/exuno";
        let bundled =
            s.ws.read("/<agentsync>/lib/templates/base-src/skills/exuno/SKILL.md")
                .unwrap();
        let mut expected = bundled.trim_ascii_end().to_vec();
        expected.extend_from_slice(b"\n\n## Local\n\nOurs.\n");
        assert_eq!(s.ws.read(&format!("{skill}/SKILL.md")).unwrap(), expected);
        assert!(!s.ws.exists(&format!("{skill}/SKILL.append.md")));
        assert_eq!(
            s.ws.read(&format!("{skill}/references/maintenance.md"))
                .unwrap(),
            b"mine\n"
        );
        assert_eq!(
            s.ws.read(&format!("{skill}/references/local.md")).unwrap(),
            b"local\n"
        );
        assert!(s.ws.is_file(&format!("{skill}/references/writing-skills.md")));
        assert!(
            !s.ws
                .exists("/<agentsync-overlay>/base-src/src/skills/exuno")
        );
        assert!(s.ws.is_file("/<agentsync-overlay>/base-src/src/skills/meta/sub/own/SKILL.md"));
    }

    #[test]
    fn an_engine_skill_copy_with_its_own_skill_md_in_a_category_replaces_it() {
        let mut s = test_session();
        let copy = "/proj/.ai/src/skills/meta/exuno";
        file(&mut s.ws, &format!("{copy}/SKILL.md"), "mine\n");
        file(
            &mut s.ws,
            &format!("{copy}/SKILL.append.md"),
            "kept as a file\n",
        );
        let mut sources = Sources {
            skills: ".ai/src/skills".into(),
            ..Sources::default()
        };
        setup_base_src(&mut s, None, "/proj/.ai/src", &mut sources).unwrap();

        let skills = "/<agentsync-overlay>/base-src/src/skills";
        assert_eq!(
            s.ws.files_under(skills),
            [
                format!("{skills}/meta/exuno/SKILL.append.md"),
                format!("{skills}/meta/exuno/SKILL.md"),
            ]
        );
        assert_eq!(
            s.ws.read(&format!("{skills}/meta/exuno/SKILL.md")).unwrap(),
            b"mine\n"
        );
    }

    #[test]
    fn a_profile_without_src_leaves_sources_alone() {
        let mut s = test_session();
        let config = "profiles:\n  hub:\n    tools: [claude-hub]\n";
        let mut sources = Sources::default();
        assert!(!setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        file(&mut s.ws, "/proj/.ai/profiles/hub/src/rules/hub.md", "h");
        assert!(setup_profile(&mut s, config, "hub", "/proj/.ai/src", &mut sources).unwrap());
        assert_eq!(sources.rules, "/<agentsync-overlay>/profile/src/rules");
        assert_eq!(
            s.log.tail(1),
            ["[INFO] Profile overlay active: hub (/proj/.ai/profiles/hub/src)"]
        );
    }

    #[test]
    fn inherit_tokens_are_validated_like_shared_setup_overlay() {
        assert_eq!(
            inherit_categories("rules, tools,subagents"),
            ["rules", "agents"]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_shared_parent_resolves_on_disk_and_never_to_the_project_itself() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("child");
        std::fs::create_dir_all(root.join(".ai/src")).unwrap();
        std::fs::create_dir_all(dir.path().join(".ai/src/rules")).unwrap();
        std::fs::write(dir.path().join(".ai/src/rules/p.md"), "p").unwrap();
        let root = root.disk_text();
        let parent = shared_parent_src("shared:\n  path: \"../\"\n", &root).unwrap();
        assert_eq!(parent, format!("{}/.ai/src", dir.path().disk_text()));
        assert_eq!(shared_parent_src("shared:\n  path: \".\"\n", &root), None);

        let mut ws = Workspace::new(&root);
        merge_shared_parent(&mut ws, &parent, &["rules"]).unwrap();
        assert_eq!(
            ws.read(&format!("{root}/.ai/src/rules/p.md")).unwrap(),
            b"p"
        );
    }
}
