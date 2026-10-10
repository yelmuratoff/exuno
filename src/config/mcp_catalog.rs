//! Bounded, read-only MCP catalog parsing.

use std::collections::HashSet;
use std::fmt;
use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value};

pub const MAX_MANIFEST_BYTES: u64 = 131_072;
pub const MAX_ENTRIES: usize = 256;
const MAX_DEPTH: usize = 16;

pub struct Entry {
    pub id: String,
    pub title: String,
    pub raw: Vec<u8>,
}

pub fn read(catalog: &Path, selected: Option<&str>) -> Result<Vec<Entry>, String> {
    let catalog = fs::canonicalize(catalog).map_err(|e| {
        format!(
            "Cannot read MCP library {}: {e}",
            escaped_title(&catalog.to_string_lossy())
        )
    })?;
    if !catalog.is_dir() {
        return Err(format!(
            "MCP library is not a directory: {}",
            escaped_title(&catalog.to_string_lossy())
        ));
    }
    if let Some(id) = selected {
        if !valid_id(id) {
            return Err(format!("Unsafe MCP library id: {}", escaped_title(id)));
        }
        return read_entry(&catalog, id).map(|entry| vec![entry]);
    }

    let mut ids = Vec::new();
    for item in fs::read_dir(&catalog).map_err(|e| {
        format!(
            "Cannot read MCP library {}: {e}",
            escaped_title(&catalog.to_string_lossy())
        )
    })? {
        let item = item.map_err(|e| format!("Cannot list MCP library: {e}"))?;
        let kind = item
            .file_type()
            .map_err(|e| format!("Cannot inspect MCP library entry: {e}"))?;
        if !kind.is_dir() && !(kind.is_symlink() && item.path().is_dir()) {
            continue;
        }
        let id = item
            .file_name()
            .into_string()
            .map_err(|_| "MCP library entry name is not UTF-8".to_string())?;
        if !valid_id(&id) {
            return Err(format!("Unsafe MCP library id: {}", escaped_title(&id)));
        }
        if kind.is_symlink() {
            return Err(format!("MCP library entry is a symlink: {id}"));
        }
        ids.push(id);
        if ids.len() > MAX_ENTRIES {
            return Err(format!("MCP library exceeds the {MAX_ENTRIES} entry limit"));
        }
    }
    ids.sort();
    ids.into_iter()
        .map(|id| read_entry(&catalog, &id))
        .collect()
}

pub fn render(catalog: &Path, id: &str, variant: &str) -> Result<Vec<u8>, String> {
    let entry = read(catalog, Some(id))?
        .pop()
        .expect("selected entry exists");
    let manifest: Value = serde_json::from_slice(&entry.raw)
        .map_err(|e| format!("Invalid MCP manifest for {id}: {e}"))?;
    let selected = match variant {
        "default" => "default",
        "recommended" => manifest
            .pointer("/guidance/recommended")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("MCP entry {id} has no attributed recommendation"))?,
        name => name,
    };
    let connection = if selected == "default" {
        &manifest["connection"]
    } else {
        manifest
            .get("alternatives")
            .and_then(|alternatives| alternatives.get(selected))
            .and_then(|alternative| alternative.get("connection"))
            .ok_or_else(|| format!("Unknown MCP variant for {id}: {}", escaped_title(selected)))?
    };
    let mut server = Map::new();
    match connection["type"]
        .as_str()
        .expect("validated connection type")
    {
        "stdio" => {
            server.insert("command".to_string(), connection["command"].clone());
            server.insert("args".to_string(), connection["args"].clone());
        }
        "http" => {
            server.insert("type".to_string(), Value::String("http".to_string()));
            server.insert("url".to_string(), connection["url"].clone());
        }
        _ => unreachable!(),
    }
    let mut servers = Map::new();
    servers.insert(id.to_string(), Value::Object(server));
    let mut source = Map::new();
    source.insert("mcpServers".to_string(), Value::Object(servers));
    let mut output = serde_json::to_vec(&Value::Object(source))
        .map_err(|e| format!("Cannot serialize MCP connection for {id}: {e}"))?;
    output.push(b'\n');
    Ok(output)
}

pub fn render_kimi_source(canonical: &[u8], id: &str) -> Vec<u8> {
    let mut source: Value = serde_json::from_slice(canonical)
        .expect("canonical MCP source was just serialized from validated JSON");
    let server = source
        .get_mut("mcpServers")
        .and_then(|value| value.get_mut(id))
        .and_then(Value::as_object_mut)
        .expect("canonical MCP source contains selected server");
    if server.get("type").and_then(Value::as_str) == Some("http") {
        server.remove("type");
    }
    let mut output = serde_json::to_vec(&source).expect("JSON value serializes");
    output.push(b'\n');
    output
}

pub fn valid_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes[0].is_ascii_alphanumeric()
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn escaped_title(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        match ch {
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            ch if ch.is_control()
                || (ch != ' ' && ch.is_whitespace())
                || is_default_ignorable(ch) =>
            {
                if (ch as u32) <= 0xffff {
                    out.push_str(&format!("\\u{:04x}", ch as u32));
                } else {
                    out.push_str(&format!("\\u{{{:x}}}", ch as u32));
                }
            }
            ch => out.push(ch),
        }
    }
    out
}

// Unicode 17.0 DerivedCoreProperties.txt: Default_Ignorable_Code_Point.
fn is_default_ignorable(ch: char) -> bool {
    matches!(ch as u32,
        0x00ad | 0x034f | 0x061c | 0x115f..=0x1160 | 0x17b4..=0x17b5 |
        0x180b..=0x180f | 0x200b..=0x200f | 0x202a..=0x202e |
        0x2060..=0x206f | 0x3164 | 0xfe00..=0xfe0f | 0xfeff |
        0xffa0 | 0xfff0..=0xfff8 | 0x1bca0..=0x1bca3 |
        0x1d173..=0x1d17a | 0xe0000..=0xe0fff
    )
}

fn read_entry(catalog: &Path, id: &str) -> Result<Entry, String> {
    let dir = catalog.join(id);
    let kind = fs::symlink_metadata(&dir)
        .map_err(|e| format!("Cannot inspect MCP entry {id}: {e}"))?
        .file_type();
    if !kind.is_dir() {
        return Err(format!("MCP entry is not a regular directory: {id}"));
    }
    let path = dir.join("manifest.json");
    let kind = fs::symlink_metadata(&path)
        .map_err(|e| format!("Cannot inspect MCP manifest for {id}: {e}"))?
        .file_type();
    if !kind.is_file() {
        return Err(format!("MCP manifest is not a regular file: {id}"));
    }
    let mut raw = Vec::new();
    File::open(&path)
        .map_err(|e| format!("Cannot open MCP manifest for {id}: {e}"))?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut raw)
        .map_err(|e| format!("Cannot read MCP manifest for {id}: {e}"))?;
    if raw.len() as u64 > MAX_MANIFEST_BYTES {
        return Err(format!("MCP manifest for {id} exceeds the byte limit"));
    }
    let value = parse_strict(&raw).map_err(|e| format!("Invalid MCP manifest for {id}: {e}"))?;
    let title = validate(&value, id).map_err(|e| format!("Invalid MCP manifest for {id}: {e}"))?;
    Ok(Entry {
        id: id.to_string(),
        title: title.to_string(),
        raw,
    })
}

pub(crate) fn parse_strict(raw: &[u8]) -> Result<Value, String> {
    let mut parser = serde_json::Deserializer::from_slice(raw);
    StrictSeed { depth: 0 }
        .deserialize(&mut parser)
        .map_err(|e| e.to_string())?;
    parser.end().map_err(|e| e.to_string())?;
    serde_json::from_slice(raw).map_err(|e| e.to_string())
}

struct StrictSeed {
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for StrictSeed {
    type Value = ();

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(StrictVisitor { depth: self.depth })
    }
}

struct StrictVisitor {
    depth: usize,
}

impl<'de> Visitor<'de> for StrictVisitor {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("JSON within the MCP catalog limits")
    }

    fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(de::Error::custom("JSON depth limit exceeded"));
        }
        let mut keys = HashSet::new();
        while let Some(key) = map.next_key::<String>()? {
            if !keys.insert(key.clone()) {
                return Err(de::Error::custom(format!(
                    "duplicate JSON key: {}",
                    escaped_title(&key)
                )));
            }
            map.next_value_seed(StrictSeed {
                depth: self.depth + 1,
            })?;
        }
        Ok(())
    }

    fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<(), S::Error> {
        if self.depth >= MAX_DEPTH {
            return Err(de::Error::custom("JSON depth limit exceeded"));
        }
        while seq
            .next_element_seed(StrictSeed {
                depth: self.depth + 1,
            })?
            .is_some()
        {}
        Ok(())
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_string<E: de::Error>(self, _: String) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
}

fn object<'a>(
    value: &'a Value,
    label: &str,
    required: &[&str],
    optional: &[&str],
) -> Result<&'a Map<String, Value>, String> {
    let map = value
        .as_object()
        .ok_or_else(|| format!("{label} must be an object"))?;
    for key in map.keys() {
        if !required.contains(&key.as_str()) && !optional.contains(&key.as_str()) {
            return Err(format!("unknown {label} field: {}", escaped_title(key)));
        }
    }
    for key in required {
        if !map.contains_key(*key) {
            return Err(format!("{label} requires field '{key}'"));
        }
    }
    Ok(map)
}

fn text<'a>(map: &'a Map<String, Value>, key: &str, label: &str) -> Result<&'a str, String> {
    map.get(key)
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("{label}.{key} must be a non-empty string"))
}

fn strings(map: &Map<String, Value>, key: &str, label: &str) -> Result<(), String> {
    let Some(values) = map.get(key).and_then(Value::as_array) else {
        return Err(format!("{label}.{key} must be an array of strings"));
    };
    if values.iter().any(|value| !value.is_string()) {
        return Err(format!("{label}.{key} must be an array of strings"));
    }
    Ok(())
}

fn connection(value: &Value) -> Result<(), String> {
    let map = value
        .as_object()
        .ok_or_else(|| "connection must be an object".to_string())?;
    match text(map, "type", "connection")? {
        "stdio" => {
            object(value, "stdio connection", &["type", "command", "args"], &[])?;
            text(map, "command", "connection")?;
            strings(map, "args", "connection")
        }
        "http" => {
            object(value, "http connection", &["type", "url"], &[])?;
            text(map, "url", "connection")?;
            Ok(())
        }
        _ => Err("connection.type must be stdio or http".to_string()),
    }
}

fn requirements(value: &Value) -> Result<(), String> {
    let map = object(value, "requirements", &["binaries", "inputs"], &[])?;
    strings(map, "binaries", "requirements")?;
    strings(map, "inputs", "requirements")?;
    if !map["inputs"].as_array().expect("checked array").is_empty() {
        return Err("requirements.inputs is unsupported".to_string());
    }
    Ok(())
}

fn variant(value: &Value) -> Result<(), String> {
    let map = object(
        value,
        "variant",
        &["connection", "requirements"],
        &["description"],
    )?;
    if let Some(description) = map.get("description")
        && !description.is_string()
    {
        return Err("variant.description must be a string".to_string());
    }
    connection(&map["connection"])?;
    requirements(&map["requirements"])
}

fn provenance(value: &Value) -> Result<(), String> {
    let fields = object(
        value,
        "provenance",
        &[],
        &["homepage", "repository", "artifact"],
    )?;
    for (key, value) in fields {
        if !value.is_string() && !value.is_null() {
            return Err(format!("provenance.{key} must be a string or null"));
        }
    }
    Ok(())
}

fn extensions(value: &Value) -> Result<(), String> {
    let fields = value
        .as_object()
        .ok_or_else(|| "extensions must be an object".to_string())?;
    for key in fields.keys() {
        if !key.contains('.') || key.starts_with('.') || key.ends_with('.') {
            return Err(format!(
                "extension key must be namespaced: {}",
                escaped_title(key)
            ));
        }
    }
    Ok(())
}

/// The alternative variant ids, each variant checked.
fn alternatives(value: &Value) -> Result<Vec<&str>, String> {
    let fields = value
        .as_object()
        .ok_or_else(|| "alternatives must be an object".to_string())?;
    let mut ids = Vec::new();
    for (id, value) in fields {
        if !valid_id(id) || matches!(id.as_str(), "default" | "recommended") {
            return Err(format!(
                "alternative needs a non-reserved variant id: {}",
                escaped_title(id)
            ));
        }
        variant(value)?;
        ids.push(id.as_str());
    }
    Ok(ids)
}

fn guidance(value: &Value, alternatives: &[&str]) -> Result<(), String> {
    let fields = object(
        value,
        "guidance",
        &["recommended", "authority", "source", "checked_at", "reason"],
        &[],
    )?;
    let recommended = text(fields, "recommended", "guidance")?;
    if recommended != "default" && !alternatives.contains(&recommended) {
        return Err("guidance.recommended names an unavailable alternative".to_string());
    }
    if !matches!(
        text(fields, "authority", "guidance")?,
        "vendor" | "maintainer"
    ) {
        return Err("guidance.authority must be vendor or maintainer".to_string());
    }
    if !https_source(text(fields, "source", "guidance")?) {
        return Err("guidance.source must be an HTTPS URL".to_string());
    }
    let date = text(fields, "checked_at", "guidance")?;
    if !valid_date(date) {
        return Err("guidance.checked_at must be a real YYYY-MM-DD date".to_string());
    }
    text(fields, "reason", "guidance")?;
    Ok(())
}

/// An `https://` URL with a plain host, an optional numeric port, and no
/// control or whitespace character.
fn https_source(source: &str) -> bool {
    let authority = source
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?', '#']).next())
        .unwrap_or("");
    let host = authority.split(':').next().unwrap_or("");
    !host.is_empty()
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-'))
        && !source
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
        && !authority
            .split_once(':')
            .is_some_and(|(_, port)| port.is_empty() || !port.bytes().all(|b| b.is_ascii_digit()))
}

fn validate<'a>(value: &'a Value, expected_id: &str) -> Result<&'a str, String> {
    let map = value
        .as_object()
        .ok_or_else(|| "manifest root must be an object".to_string())?;
    let version = match map.get("schema_version").and_then(Value::as_u64) {
        Some(1) => 1,
        Some(2) => 2,
        _ => return Err("unsupported schema_version".to_string()),
    };
    let optional_v1 = ["description", "provenance", "extensions"];
    let optional_v2 = [
        "description",
        "provenance",
        "extensions",
        "alternatives",
        "guidance",
    ];
    let map = object(
        value,
        "manifest",
        &[
            "schema_version",
            "id",
            "title",
            "connection",
            "requirements",
        ],
        if version == 1 {
            &optional_v1
        } else {
            &optional_v2
        },
    )?;
    let id = text(map, "id", "manifest")?;
    if !valid_id(id) || id != expected_id {
        return Err(format!(
            "manifest.id must match entry directory {expected_id}"
        ));
    }
    let title = text(map, "title", "manifest")?;
    if let Some(description) = map.get("description")
        && !description.is_string()
    {
        return Err("manifest.description must be a string".to_string());
    }
    connection(&map["connection"])?;
    requirements(&map["requirements"])?;
    if let Some(value) = map.get("provenance") {
        provenance(value)?;
    }
    if let Some(value) = map.get("extensions") {
        extensions(value)?;
    }
    let alternatives = match map.get("alternatives") {
        Some(value) => alternatives(value)?,
        None => Vec::new(),
    };
    if let Some(value) = map.get("guidance") {
        guidance(value, &alternatives)?;
    }
    Ok(title)
}

fn valid_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes[..4].iter().any(|b| !b.is_ascii_digit())
        || bytes[5..7].iter().any(|b| !b.is_ascii_digit())
        || bytes[8..].iter().any(|b| !b.is_ascii_digit())
    {
        return false;
    }
    let year: u32 = date[..4].parse().unwrap_or(0);
    let month: usize = date[5..7].parse().unwrap_or(0);
    let day: usize = date[8..].parse().unwrap_or(0);
    if year == 0 || !(1..=12).contains(&month) {
        return false;
    }
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    (1..=days[month - 1]).contains(&day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strict_parser_rejects_equivalent_escaped_keys_at_any_depth() {
        assert!(
            parse_strict(br#"{"extensions":{"example.dev":{"key":1,"\u006bey":2}}}"#)
                .unwrap_err()
                .contains("duplicate JSON key: key")
        );
    }

    #[test]
    fn strict_parser_limits_container_depth() {
        let deep = format!("{}0{}", "[".repeat(17), "]".repeat(17));
        assert!(
            parse_strict(deep.as_bytes())
                .unwrap_err()
                .contains("depth limit")
        );
        let allowed = format!("{}0{}", "[".repeat(16), "]".repeat(16));
        assert!(parse_strict(allowed.as_bytes()).is_ok());
    }

    #[test]
    fn date_rejects_nonexistent_calendar_days() {
        assert!(valid_date("2024-02-29"));
        assert!(!valid_date("2026-02-29"));
    }

    fn manifest() -> Value {
        serde_json::json!({
            "schema_version": 2,
            "id": "demo",
            "title": "Demo",
            "connection": {"type": "http", "url": "https://mcp.example.com"},
            "requirements": {"binaries": [], "inputs": []},
            "provenance": {"homepage": "https://example.com", "artifact": null},
            "extensions": {"example.dev": {}},
            "alternatives": {
                "local": {
                    "connection": {"type": "stdio", "command": "demo", "args": []},
                    "requirements": {"binaries": ["demo"], "inputs": []}
                }
            },
            "guidance": {
                "recommended": "local",
                "authority": "vendor",
                "source": "https://docs.example.com:8443/mcp?x#y",
                "checked_at": "2026-09-01",
                "reason": "Runs offline."
            }
        })
    }

    fn with(path: &[&str], value: Value) -> Result<String, String> {
        let mut manifest = manifest();
        let (last, parents) = path.split_last().unwrap();
        let mut node = &mut manifest;
        for key in parents {
            node = &mut node[*key];
        }
        node[*last] = value;
        validate(&manifest, "demo").map(str::to_string)
    }

    #[test]
    fn validate_checks_each_optional_section() {
        assert_eq!(validate(&manifest(), "demo"), Ok("Demo"));
        assert_eq!(
            with(&["provenance", "homepage"], Value::from(1)),
            Err("provenance.homepage must be a string or null".to_string())
        );
        assert_eq!(
            with(&["extensions"], serde_json::json!({"plain": {}})),
            Err("extension key must be namespaced: plain".to_string())
        );
        assert_eq!(
            with(&["alternatives"], serde_json::json!({"default": {}})),
            Err("alternative needs a non-reserved variant id: default".to_string())
        );
        assert_eq!(
            with(&["guidance", "recommended"], Value::from("remote")),
            Err("guidance.recommended names an unavailable alternative".to_string())
        );
        assert_eq!(
            with(&["guidance", "authority"], Value::from("blog")),
            Err("guidance.authority must be vendor or maintainer".to_string())
        );
        assert_eq!(
            with(&["guidance", "checked_at"], Value::from("2026-02-30")),
            Err("guidance.checked_at must be a real YYYY-MM-DD date".to_string())
        );
    }

    #[test]
    fn guidance_source_must_be_a_plain_https_url() {
        for source in ["https://example.com", "https://ex-ample.com:8443/a?b#c"] {
            assert_eq!(
                with(&["guidance", "source"], Value::from(source)),
                Ok("Demo".to_string()),
                "{source}"
            );
        }
        for source in [
            "http://example.com",
            "https://",
            "https://ex_ample.com",
            "https://example.com:",
            "https://example.com:80a",
            "https://example.com/a b",
            "https://example.com/\u{7}",
        ] {
            assert_eq!(
                with(&["guidance", "source"], Value::from(source)),
                Err("guidance.source must be an HTTPS URL".to_string()),
                "{source}"
            );
        }
    }
}
