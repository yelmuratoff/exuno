//! The `manifests` target: template files a tool ships or the project keeps
//! in `<tools dir>/<slug>/manifests/`, each written below the target's dest
//! with its `${key}` placeholders filled from `targets.manifests.vars`.

use std::collections::BTreeSet;

use crate::engine::workspace::Workspace;
use crate::paths::ENGINE_ROOT;

/// Why a template could not be filled; the text is the reason the user sees.
#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum FillError {
    #[error("no value for ${{{0}}} (set targets.manifests.vars.{0})")]
    MissingKey(String),
    #[error("filled template is not valid JSON: {0}")]
    InvalidJson(String),
}

/// `template` with every `${key}` (`[a-z0-9_]+`) replaced by `value(key)`;
/// anything else after a `$` stays as written. With `json`, each value goes
/// in JSON-string-escaped and the result must parse as JSON.
pub fn fill(
    template: &str,
    json: bool,
    value: &dyn Fn(&str) -> Option<String>,
) -> Result<String, FillError> {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let key = after
            .find('}')
            .map(|end| &after[..end])
            .filter(|key| is_key(key));
        let Some(key) = key else {
            out.push_str("${");
            rest = after;
            continue;
        };
        let found = value(key).ok_or_else(|| FillError::MissingKey(key.to_string()))?;
        if json {
            let quoted = serde_json::Value::String(found).to_string();
            out.push_str(&quoted[1..quoted.len() - 1]);
        } else {
            out.push_str(&found);
        }
        rest = &after[key.len() + 1..];
    }
    out.push_str(rest);
    if json && let Err(error) = serde_json::from_str::<serde_json::Value>(&out) {
        return Err(FillError::InvalidJson(error.to_string()));
    }
    Ok(out)
}

fn is_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

fn shipped_dir(slug: &str) -> String {
    format!("{ENGINE_ROOT}/lib/templates/manifests/{slug}")
}

fn override_dir(tools_dir: &str, slug: &str) -> String {
    format!("{tools_dir}/{slug}/manifests")
}

/// The directory a tool's templates come from: the project's override when it
/// exists, else the shipped set; `None` when the tool has neither.
pub fn template_dir(ws: &Workspace, tools_dir: &str, slug: &str) -> Option<String> {
    [override_dir(tools_dir, slug), shipped_dir(slug)]
        .into_iter()
        .find(|dir| ws.is_dir(dir))
}

/// Every template file below `dir`, relative to it, in byte order.
pub fn templates(ws: &Workspace, dir: &str) -> Vec<String> {
    let prefix = format!("{dir}/");
    let rels: BTreeSet<String> = ws
        .files_under(dir)
        .into_iter()
        .filter_map(|path| path.strip_prefix(&prefix).map(str::to_string))
        .collect();
    rels.into_iter().collect()
}

/// The paths a `manifests` target at `dest` owns: `dest` itself when it is a
/// directory of its own, and below the project root only the files a template
/// of either set (override or shipped) maps to, so nothing else there is
/// ever snapshotted, cleaned, or pruned as the target's.
pub fn owned_paths(
    ws: &Workspace,
    tools_dir: &str,
    slug: &str,
    dest: &str,
    root: &str,
) -> Vec<String> {
    if dest != root {
        return vec![dest.to_string()];
    }
    let rels: BTreeSet<String> = [override_dir(tools_dir, slug), shipped_dir(slug)]
        .iter()
        .flat_map(|dir| templates(ws, dir))
        .collect();
    rels.into_iter()
        .map(|rel| format!("{dest}/{rel}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FillError, fill};

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |key| pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    #[test]
    fn fills_every_placeholder() {
        let value = vars(&[("name", "my-skills"), ("owner_2", "exuno")]);
        assert_eq!(
            fill("${name} by ${owner_2}: ${name}\n", false, &value).unwrap(),
            "my-skills by exuno: my-skills\n"
        );
        assert_eq!(
            fill("$5 ${Name} ${} ${a-b} $name ${name", false, &value).unwrap(),
            "$5 ${Name} ${} ${a-b} $name ${name"
        );
    }

    #[test]
    fn reports_the_first_missing_key() {
        let value = vars(&[("name", "x")]);
        assert_eq!(
            fill("${name} ${zeta} ${alpha}", false, &value),
            Err(FillError::MissingKey("zeta".into()))
        );
        assert_eq!(
            FillError::MissingKey("owner".into()).to_string(),
            "no value for ${owner} (set targets.manifests.vars.owner)"
        );
    }

    #[test]
    fn escapes_quotes_and_backslashes_in_json() {
        let value = vars(&[("d", "say \"hi\" \\ C:\\path\nnext")]);
        let filled = fill("{\"d\": \"${d}\"}\n", true, &value).unwrap();
        assert_eq!(
            filled,
            "{\"d\": \"say \\\"hi\\\" \\\\ C:\\\\path\\nnext\"}\n"
        );
        let parsed: serde_json::Value = serde_json::from_str(&filled).unwrap();
        assert_eq!(parsed["d"], "say \"hi\" \\ C:\\path\nnext");
    }

    #[test]
    fn rejects_a_json_template_that_does_not_parse() {
        let value = vars(&[("n", "a b")]);
        for template in ["{\"n\": ${n}}", "{\"n\": \"${n}\",}"] {
            let error = fill(template, true, &value).unwrap_err();
            assert!(
                matches!(&error, FillError::InvalidJson(reason) if !reason.is_empty()),
                "{template}: {error:?}"
            );
            assert!(
                error
                    .to_string()
                    .starts_with("filled template is not valid JSON: ")
            );
        }
    }

    #[test]
    fn leaves_non_json_templates_unescaped() {
        let value = vars(&[("d", "a \"b\" \\c")]);
        assert_eq!(
            fill("x: ${d}\n", false, &value).unwrap(),
            "x: a \"b\" \\c\n"
        );
        assert_eq!(
            fill("{\"n\": ${d}", false, &value).unwrap(),
            "{\"n\": a \"b\" \\c"
        );
    }
}
