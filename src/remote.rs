//! A `.ai/` fetched from a git remote: any URL git understands, a GitHub or
//! GitLab web link to a ref and folder, or a GitHub `owner/repo`. One shallow,
//! blobless fetch of the ref, checked out sparse when a folder is named; the
//! user's own git config and credentials reach private repositories.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// A remote and what of it to fetch.
#[derive(Debug, PartialEq, Eq)]
pub struct Remote {
    pub url: String,
    /// A web link's `<ref>/<folder>` tail, split once the remote's refs are
    /// known; `None` for a bare repository URL.
    pub tree: Option<String>,
}

/// `source` as a remote, or `None` when it names none. A local path that
/// exists is never a remote; the caller checks that first.
pub fn parse(source: &str) -> Option<Remote> {
    let source = source.trim_end_matches('/');
    if let Some(rest) = source
        .strip_prefix("https://")
        .or_else(|| source.strip_prefix("http://"))
    {
        return Some(web(source, rest));
    }
    if ["ssh://", "git://", "file://"]
        .iter()
        .any(|scheme| source.starts_with(scheme))
        || scp_like(source)
    {
        return Some(Remote {
            url: source.to_string(),
            tree: None,
        });
    }
    if let Some(rest) = source.strip_prefix("github.com/") {
        return parse(&format!("https://github.com/{rest}"));
    }
    shorthand(source).then(|| Remote {
        url: format!("https://github.com/{source}.git"),
        tree: None,
    })
}

/// An `https` URL: a GitHub or GitLab page for a ref and folder becomes its
/// repository and tail; any other URL is a repository as given.
fn web(source: &str, rest: &str) -> Remote {
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    if let Some(path) = rest.strip_prefix("github.com/") {
        let segments: Vec<&str> = path.split('/').collect();
        if segments.len() >= 2 {
            let repo = segments[1].strip_suffix(".git").unwrap_or(segments[1]);
            let tree = match segments.get(2) {
                Some(&"tree" | &"blob") if segments.len() > 3 => Some(segments[3..].join("/")),
                _ => None,
            };
            return Remote {
                url: format!("https://github.com/{}/{repo}.git", segments[0]),
                tree,
            };
        }
    }
    for marker in ["/-/tree/", "/-/blob/"] {
        if let Some((repo, tail)) = source.split_once(marker) {
            return Remote {
                url: format!("{repo}.git"),
                tree: Some(tail.to_string()),
            };
        }
    }
    Remote {
        url: source.to_string(),
        tree: None,
    }
}

/// `user@host:path`, git's scp-like SSH form.
fn scp_like(source: &str) -> bool {
    let Some((head, _)) = source.split_once(':') else {
        return false;
    };
    head.contains('@') && !head.contains('/')
}

/// `owner/repo`: exactly two names of the characters GitHub allows.
fn shorthand(source: &str) -> bool {
    let named = |part: &str| {
        !part.is_empty()
            && !part.starts_with('.')
            && part
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    };
    matches!(source.split('/').collect::<Vec<_>>()[..], [owner, repo] if named(owner) && named(repo))
}

/// Splits a web link's tail into its ref and folder: the longest branch or
/// tag the tail starts with, so a ref may hold `/`; else the first segment,
/// which may be a commit.
pub fn split_tree(tree: &str, refs: &[String]) -> (String, String) {
    let matched = refs
        .iter()
        .filter(|name| tree == name.as_str() || tree.starts_with(&format!("{name}/")))
        .max_by_key(|name| name.len());
    let (reference, folder) = match matched {
        Some(name) => (name.as_str(), &tree[name.len()..]),
        None => tree.split_once('/').unwrap_or((tree, "")),
    };
    (reference.to_string(), folder.trim_matches('/').to_string())
}

/// What to fetch: `reference` empty for the remote's default branch, and
/// `folder` empty for the whole repository.
pub struct Request<'a> {
    pub url: &'a str,
    pub reference: &'a str,
    pub folder: &'a str,
}

/// The branch and tag names the remote advertises.
pub fn refs(url: &str) -> Result<Vec<String>, String> {
    let output = git(None)
        .args(["ls-remote", "--heads", "--tags", url])
        .output()
        .map_err(|e| format!("git is required to fetch a remote ({e})"))?;
    if !output.status.success() {
        return Err(failure(url, "", &output.stderr));
    }
    let listing = String::from_utf8_lossy(&output.stdout);
    let mut names: Vec<String> = listing
        .lines()
        .filter_map(|line| line.split_once('\t').map(|(_, name)| name))
        .filter_map(|name| {
            name.strip_prefix("refs/heads/")
                .or_else(|| name.strip_prefix("refs/tags/"))
        })
        .map(|name| name.trim_end_matches("^{}").to_string())
        .collect();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Fetches `request` into the empty directory `into`; the directory that
/// holds the requested folder.
pub fn fetch(request: &Request, into: &Path) -> Result<PathBuf, String> {
    let wanted = if request.reference.is_empty() {
        "HEAD"
    } else {
        request.reference
    };
    let mut steps: Vec<Vec<&str>> = vec![
        vec!["init", "-q"],
        vec!["remote", "add", "origin", request.url],
        vec![
            "fetch",
            "-q",
            "--depth",
            "1",
            "--filter=blob:none",
            "origin",
            wanted,
        ],
    ];
    if !request.folder.is_empty() {
        steps.push(vec!["sparse-checkout", "set", request.folder]);
    }
    steps.push(vec!["checkout", "-q", "FETCH_HEAD"]);
    for step in steps {
        let output = git(Some(into))
            .args(&step)
            .output()
            .map_err(|e| format!("git is required to fetch a remote ({e})"))?;
        if !output.status.success() {
            return Err(failure(request.url, request.reference, &output.stderr));
        }
    }
    let root = into.join(request.folder);
    if root.is_dir() {
        Ok(root)
    } else {
        Err(format!(
            "{}: no folder '{}' at {wanted}",
            request.url, request.folder
        ))
    }
}

/// `git` that never prompts, runs no hook, and refuses the `ext` transport,
/// while keeping the user's config so credential helpers and `insteadOf`
/// rewrites apply.
fn git(dir: Option<&Path>) -> Command {
    let null = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let mut cmd = Command::new("git");
    cmd.args([
        "-c",
        &format!("core.hooksPath={null}"),
        "-c",
        "protocol.ext.allow=never",
        "-c",
        "advice.detachedHead=false",
    ]);
    if let Some(dir) = dir {
        cmd.arg("-C").arg(dir);
    }
    cmd.env("GIT_TERMINAL_PROMPT", "0").stdin(Stdio::null());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        cmd.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    cmd
}

/// The message for a failed git step: what was fetched and git's own last
/// word on why.
fn failure(url: &str, reference: &str, stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let reason = text
        .lines()
        .map(str::trim)
        .rfind(|line| !line.is_empty())
        .unwrap_or("git failed")
        .trim_start_matches("fatal: ")
        .trim_start_matches("error: ");
    let at = if reference.is_empty() {
        String::new()
    } else {
        format!(" at {reference}")
    };
    format!("could not fetch {url}{at}: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(url: &str, tree: Option<&str>) -> Option<Remote> {
        Some(Remote {
            url: url.to_string(),
            tree: tree.map(str::to_string),
        })
    }

    #[test]
    fn github_links_name_the_repository_and_tail() {
        assert_eq!(
            parse("https://github.com/acme/kit"),
            remote("https://github.com/acme/kit.git", None)
        );
        assert_eq!(
            parse("https://www.github.com/acme/kit.git/"),
            remote("https://github.com/acme/kit.git", None)
        );
        assert_eq!(
            parse("https://github.com/acme/kit/tree/feature/x/packages/app"),
            remote(
                "https://github.com/acme/kit.git",
                Some("feature/x/packages/app")
            )
        );
        assert_eq!(
            parse("github.com/acme/kit/blob/v1/.ai/src/AGENTS.md"),
            remote(
                "https://github.com/acme/kit.git",
                Some("v1/.ai/src/AGENTS.md")
            )
        );
    }

    #[test]
    fn other_remotes_pass_through() {
        assert_eq!(
            parse("https://gitlab.com/group/sub/kit/-/tree/main/app"),
            remote("https://gitlab.com/group/sub/kit.git", Some("main/app"))
        );
        assert_eq!(
            parse("https://git.example.com/kit.git"),
            remote("https://git.example.com/kit.git", None)
        );
        assert_eq!(
            parse("git@github.com:acme/kit.git"),
            remote("git@github.com:acme/kit.git", None)
        );
        assert_eq!(
            parse("ssh://git@host/kit.git"),
            remote("ssh://git@host/kit.git", None)
        );
    }

    #[test]
    fn owner_slash_repo_means_github() {
        assert_eq!(
            parse("acme/kit"),
            remote("https://github.com/acme/kit.git", None)
        );
        for local in [
            "bundle.tar.gz",
            "a/b/c",
            "./a/b",
            "../kit",
            "acme/",
            "C:\\kit",
        ] {
            assert_eq!(parse(local), None, "{local}");
        }
    }

    #[test]
    fn a_tail_splits_at_the_longest_ref_it_starts_with() {
        let refs = ["main", "feature", "feature/x", "v1"].map(String::from);
        assert_eq!(
            split_tree("feature/x/packages/app", &refs),
            ("feature/x".into(), "packages/app".into())
        );
        assert_eq!(split_tree("main", &refs), ("main".into(), String::new()));
        assert_eq!(
            split_tree("4f2a9c1/app", &refs),
            ("4f2a9c1".into(), "app".into())
        );
    }

    #[test]
    fn a_failure_names_the_remote_the_ref_and_gits_reason() {
        assert_eq!(
            failure(
                "https://x/y.git",
                "v9",
                b"warning: noise\nfatal: couldn't find remote ref v9\n"
            ),
            "could not fetch https://x/y.git at v9: couldn't find remote ref v9"
        );
    }
}
