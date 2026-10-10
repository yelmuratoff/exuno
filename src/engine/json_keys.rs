//! The JSON side of `keyed`. `serde_json` keeps no formatting, so an unchanged
//! file comes back byte for byte and a changed one is written pretty-printed
//! with its keys sorted.

use serde_json::{Map, Value};

use crate::engine::keyed::{self, KeyPath, Merged, Owned, UNIT_ROOTS};

pub fn merge(live: &str, desired: &str, previous: Option<&Owned>) -> Result<Merged, String> {
    let declared = declared(desired)?;
    let mut doc = parse(live, LIVE)?;
    let same = |live: &Value, value: &Value| canonical(live) == canonical(value);
    let drifted = match previous {
        Some(previous) => previous.changed(&owned_now(&doc, previous.keys())),
        None => declared
            .iter()
            .filter(|(path, value)| lookup(&doc, path).is_some_and(|live| !same(live, value)))
            .map(|(path, _)| path.clone())
            .collect(),
    };
    let mut changed = false;
    for path in previous.into_iter().flat_map(Owned::keys) {
        if !declared
            .iter()
            .any(|(declared, _)| path.starts_with(declared))
            && remove(&mut doc, path)
        {
            changed = true;
        }
    }
    for (path, value) in &declared {
        if lookup(&doc, path).is_some_and(|live| same(live, value)) {
            continue;
        }
        set(&mut doc, path, value.clone());
        changed = true;
    }
    Ok(Merged {
        text: if changed {
            render(&doc)
        } else {
            live.to_string()
        },
        owned: Owned::from_pairs(
            declared
                .iter()
                .map(|(path, value)| (path.clone(), hash_value(Some(value)))),
        ),
        drifted,
    })
}

pub fn owned_in(live: &str, record: &Owned) -> Result<Owned, String> {
    Ok(owned_now(&parse(live, LIVE)?, record.keys()))
}

pub fn adopt(live: &str, source: &str, keys: &[KeyPath]) -> Result<String, String> {
    let live = parse(live, LIVE)?;
    let mut doc = parse(source, SOURCE)?;
    for key in keys {
        match lookup(&live, key) {
            Some(value) => set(&mut doc, key, value.clone()),
            None => {
                remove(&mut doc, key);
            }
        }
    }
    Ok(render(&doc))
}

/// `source` with `value` set under the dotted `parent` object for every name,
/// creating the objects along the way; a non-object on that path is an error.
pub fn with_values(
    source: &str,
    parent: &str,
    names: &[String],
    value: &str,
) -> Result<String, String> {
    if has_comments(source) {
        return Err(format!("{SOURCE} has comments, which JSON does not allow"));
    }
    let mut doc = parse(source, SOURCE)?;
    let mut map = &mut doc;
    for key in parent.split('.') {
        map = map
            .entry(key)
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .ok_or_else(|| format!("in {SOURCE}, {parent} is not a JSON object"))?;
    }
    for name in names {
        map.insert(name.clone(), Value::String(value.to_string()));
    }
    Ok(render(&doc))
}

/// Every leaf of `desired` is one owned key; an array and each entry of a
/// server map count as one value.
fn declared(desired: &str) -> Result<Vec<(KeyPath, Value)>, String> {
    let doc = parse(desired, SOURCE)?;
    let mut out = Vec::new();
    collect_leaves(&doc, &mut Vec::new(), &mut out);
    Ok(out)
}

const LIVE: &str = "the live file";
const SOURCE: &str = "the source";

fn has_comments(text: &str) -> bool {
    text.lines().any(|line| {
        let line = line.trim_start();
        line.starts_with("//") || line.starts_with("/*")
    })
}

fn parse(text: &str, which: &str) -> Result<Map<String, Value>, String> {
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(_) => Err(format!("in {which}, the top level is not a JSON object")),
        Err(e) => {
            let message = e.to_string();
            let message = message.split(" at line ").next().unwrap_or_default();
            let mut reason = format!(
                "in {which}, line {}, column {}: {message}",
                e.line(),
                e.column()
            );
            if has_comments(text) {
                reason.push_str(
                    "; comments are not JSON, so set ownership: file on this target to own it whole",
                );
            }
            Err(reason)
        }
    }
}

/// A value's meaning without its formatting: key order drops out, and a float
/// with no fraction reads as the integer an app may write back for it.
fn canonical(value: &Value) -> String {
    match value {
        Value::Number(number) => match number.as_f64() {
            Some(float) if float.fract() == 0.0 && float.abs() < 9.0e15 => {
                format!("{}", float as i64)
            }
            _ => number.to_string(),
        },
        Value::Array(items) => format!(
            "[{}]",
            items.iter().map(canonical).collect::<Vec<_>>().join(",")
        ),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!(
                    "{}:{}",
                    serde_json::to_string(k).unwrap_or_default(),
                    canonical(v)
                ))
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => other.to_string(),
    }
}

fn render(doc: &Map<String, Value>) -> String {
    let mut text = serde_json::to_string_pretty(doc).unwrap_or_default();
    text.push('\n');
    text
}

fn owned_now<'a>(doc: &Map<String, Value>, keys: impl Iterator<Item = &'a KeyPath>) -> Owned {
    Owned::from_pairs(keys.map(|path| (path.clone(), hash_value(lookup(doc, path)))))
}

fn collect_leaves(map: &Map<String, Value>, path: &mut KeyPath, out: &mut Vec<(KeyPath, Value)>) {
    for (key, value) in map {
        path.push(key.clone());
        let unit_root = path.len() == 1 && UNIT_ROOTS.contains(&key.as_str());
        match value {
            Value::Object(entries) if unit_root => {
                for (entry, server) in entries {
                    out.push((vec![key.clone(), entry.clone()], server.clone()));
                }
            }
            Value::Object(child) => collect_leaves(child, path, out),
            _ => out.push((path.clone(), value.clone())),
        }
        path.pop();
    }
}

fn lookup<'a>(doc: &'a Map<String, Value>, path: &[String]) -> Option<&'a Value> {
    let (last, parents) = path.split_last()?;
    let mut map = doc;
    for key in parents {
        map = map.get(key)?.as_object()?;
    }
    map.get(last)
}

fn set(doc: &mut Map<String, Value>, path: &[String], value: Value) {
    let Some((last, parents)) = path.split_last() else {
        return;
    };
    let mut map = doc;
    for key in parents {
        let slot = map
            .entry(key.clone())
            .or_insert_with(|| Value::Object(Map::new()));
        if !slot.is_object() {
            *slot = Value::Object(Map::new());
        }
        let Some(next) = slot.as_object_mut() else {
            return;
        };
        map = next;
    }
    map.insert(last.clone(), value);
}

/// Removes `path` and any object it leaves empty; `true` when something went.
fn remove(doc: &mut Map<String, Value>, path: &[String]) -> bool {
    let Some((last, parents)) = path.split_last() else {
        return false;
    };
    let removed = {
        let mut map = &mut *doc;
        for key in parents {
            let Some(next) = map.get_mut(key).and_then(Value::as_object_mut) else {
                return false;
            };
            map = next;
        }
        map.remove(last).is_some()
    };
    if removed {
        for depth in (1..=parents.len()).rev() {
            let prefix = &parents[..depth];
            if lookup(doc, prefix)
                .and_then(Value::as_object)
                .is_some_and(Map::is_empty)
            {
                remove(doc, prefix);
            } else {
                break;
            }
        }
    }
    removed
}

fn hash_value(value: Option<&Value>) -> String {
    match value {
        Some(value) => keyed::short_hash(&canonical(value)),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(path: &str) -> KeyPath {
        path.split('.').map(str::to_string).collect()
    }

    #[test]
    fn with_values_sets_each_name_under_a_nested_parent_and_keeps_siblings() {
        let source = r#"{"permission": {"edit": "ask", "skill": {"bloc": "allow"}}}"#;
        let names = ["bloc".to_string(), "deploy".to_string()];
        assert_eq!(
            with_values(source, "permission.skill", &names, "deny").unwrap(),
            "{\n  \"permission\": {\n    \"edit\": \"ask\",\n    \"skill\": {\n      \"bloc\": \"deny\",\n      \"deploy\": \"deny\"\n    }\n  }\n}\n"
        );
        assert_eq!(
            with_values("", "skillOverrides", &names[..1], "off").unwrap(),
            "{\n  \"skillOverrides\": {\n    \"bloc\": \"off\"\n  }\n}\n"
        );
    }

    #[test]
    fn with_values_refuses_a_parent_that_is_not_an_object() {
        let source = r#"{"permission": {"skill": "ask"}}"#;
        assert_eq!(
            with_values(source, "permission.skill", &["bloc".to_string()], "deny").unwrap_err(),
            "in the source, permission.skill is not a JSON object"
        );
        assert_eq!(
            with_values("{\n  // note\n}\n", "skillOverrides", &[], "off").unwrap_err(),
            "the source has comments, which JSON does not allow"
        );
    }

    #[test]
    fn keeps_keys_it_does_not_own() {
        let live = "{\n  \"model\": \"old\",\n  \"feedbackSurveyState\": {\"last\": 3}\n}\n";
        let merged = merge(live, r#"{"model": "new"}"#, None).unwrap();
        assert_eq!(
            merged.text,
            "{\n  \"feedbackSurveyState\": {\n    \"last\": 3\n  },\n  \"model\": \"new\"\n}\n"
        );
    }

    #[test]
    fn leaves_bytes_unchanged_when_values_match() {
        let live = "{\"theme\":\"dark\",   \"model\": \"a\", \"env\": {\"X\": \"1\"}}";
        let merged = merge(live, "{\"env\": {\"X\": \"1\"}, \"model\": \"a\"}", None).unwrap();
        assert_eq!(merged.text, live);
        assert!(merged.drifted.is_empty());
    }

    #[test]
    fn owns_leaves_so_the_app_can_add_siblings() {
        let desired = r#"{"enabledPlugins": {"a@m": true}, "permissions": {"allow": ["Read"]}}"#;
        let first = merge("{}", desired, None).unwrap();
        let live = first
            .text
            .replace("\"a@m\": true", "\"a@m\": true,\n    \"b@m\": true");
        let second = merge(&live, desired, Some(&first.owned)).unwrap();
        assert_eq!(second.text, live);
        assert!(second.drifted.is_empty());
        assert_eq!(
            first.owned.keys().collect::<Vec<_>>(),
            [&key("enabledPlugins.a@m"), &key("permissions.allow")]
        );
    }

    #[test]
    fn owns_each_server_entry_as_one_value() {
        let live = r#"{"mcpServers": {"ui-added": {"command": "x"}}}"#;
        let desired = r#"{"mcpServers": {"dart": {"command": "dart", "args": ["mcp-server"]}}}"#;
        let first = merge(live, desired, None).unwrap();
        assert_eq!(
            first.owned.keys().collect::<Vec<_>>(),
            [&key("mcpServers.dart")]
        );
        assert!(first.text.contains("\"ui-added\""));
        let second = merge(&first.text, r#"{"mcpServers": {}}"#, Some(&first.owned)).unwrap();
        assert_eq!(
            second.text,
            "{\n  \"mcpServers\": {\n    \"ui-added\": {\n      \"command\": \"x\"\n    }\n  }\n}\n"
        );
    }

    #[test]
    fn removes_owned_keys_and_the_objects_they_empty() {
        let first = merge("{}", r#"{"a": {"b": 1}, "model": "m"}"#, None).unwrap();
        let second = merge(&first.text, r#"{"model": "m"}"#, Some(&first.owned)).unwrap();
        assert_eq!(second.text, "{\n  \"model\": \"m\"\n}\n");
    }

    #[test]
    fn removes_nothing_without_a_previous_record() {
        let live = "{\"model\": \"a\", \"theme\": \"dark\"}\n";
        assert_eq!(merge(live, r#"{"model": "a"}"#, None).unwrap().text, live);
    }

    #[test]
    fn reports_changed_owned_keys_and_first_run_differences() {
        let first = merge("{}", r#"{"model": "a", "tui": "full"}"#, None).unwrap();
        let live = first.text.replace("\"a\"", "\"b\"");
        let second = merge(
            &live,
            r#"{"model": "a", "tui": "full"}"#,
            Some(&first.owned),
        )
        .unwrap();
        assert_eq!(second.drifted, [key("model")]);
        let fresh = merge(r#"{"model": "ui"}"#, r#"{"model": "src"}"#, None).unwrap();
        assert_eq!(fresh.drifted, [key("model")]);
    }

    #[test]
    fn an_empty_live_file_starts_from_an_empty_object() {
        let merged = merge("", r#"{"model": "a"}"#, None).unwrap();
        assert_eq!(merged.text, "{\n  \"model\": \"a\"\n}\n");
    }

    #[test]
    fn refuses_jsonc_with_a_hint() {
        let reason = merge("{\n  // mine\n  \"a\": 1\n}\n", r#"{"a": 1}"#, None).unwrap_err();
        assert!(
            reason.starts_with("in the live file, line 2, column "),
            "{reason}"
        );
        assert!(reason.contains("ownership: file"), "{reason}");
        assert_eq!(
            merge("[1]", "{}", None).unwrap_err(),
            "in the live file, the top level is not a JSON object"
        );
    }

    #[test]
    fn names_the_source_and_skips_the_comment_hint_for_urls() {
        let source = "{\"$schema\": \"https://x.test/s.json\", \"a\": 1,}";
        let reason = merge("{}", source, None).unwrap_err();
        assert!(
            reason.starts_with("in the source, line 1, column "),
            "{reason}"
        );
        assert!(!reason.contains("comments"), "{reason}");
    }

    #[test]
    fn integral_floats_equal_their_integers() {
        let merged = merge("{\"timeout\": 60}", "{\"timeout\": 60.0}", None).unwrap();
        assert_eq!(merged.text, "{\"timeout\": 60}");
        assert!(merged.drifted.is_empty());
        let now = owned_in("{\"timeout\": 60.0}", &merged.owned).unwrap();
        assert!(merged.owned.changed(&now).is_empty());
    }

    #[test]
    fn hashes_ignore_key_order_inside_a_server() {
        let desired = r#"{"mcpServers": {"d": {"command": "d", "args": []}}}"#;
        let first = merge("{}", desired, None).unwrap();
        let reordered = r#"{"mcpServers": {"d": {"args": [], "command": "d"}}}"#;
        let now = owned_in(reordered, &first.owned).unwrap();
        assert!(first.owned.changed(&now).is_empty());
    }

    #[test]
    fn adopt_copies_live_values_into_the_source() {
        let live = r#"{"model": "ui", "theme": "light"}"#;
        let source = r#"{"model": "src", "effortLevel": "high"}"#;
        let adopted = adopt(live, source, &[key("model"), key("effortLevel")]).unwrap();
        assert_eq!(adopted, "{\n  \"model\": \"ui\"\n}\n");
    }
}
