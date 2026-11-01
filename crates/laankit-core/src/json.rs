//! JSON file helpers. Output matches `JSON.stringify(value, null, 2)` so
//! files written by the Rust and Electron builds diff cleanly in git.

use std::fs;
use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{Map, Value};

/// Read and parse a JSON file, returning `None` if it is missing or invalid.
pub fn read<T: DeserializeOwned>(path: &Path) -> Option<T> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Read a JSON object, falling back to an empty object.
pub fn read_object(path: &Path) -> Map<String, Value> {
    match read::<Value>(path) {
        Some(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

pub fn to_pretty<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    Ok(serde_json::to_string_pretty(value)?)
}

/// Write `value` as pretty JSON. Writes to a sibling temp file and renames,
/// so a crash mid-write never leaves a truncated index behind.
pub fn write<T: Serialize + ?Sized>(path: &Path, value: &T) -> Result<()> {
    write_string(path, &to_pretty(value)?)
}

pub fn write_string(path: &Path, contents: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    {
        let mut f = fs::File::create(&tmp).with_context(|| format!("writing {}", tmp.display()))?;
        f.write_all(contents.as_bytes())?;
        f.sync_all().ok();
    }
    fs::rename(&tmp, path).with_context(|| format!("renaming into {}", path.display()))?;
    Ok(())
}

/// Shallow-merge `patch` into `base` (the `{ ...base, ...patch }` idiom).
pub fn merge(base: &mut Map<String, Value>, patch: &Value) {
    if let Value::Object(p) = patch {
        for (k, v) in p {
            base.insert(k.clone(), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pretty_output_matches_json_stringify() {
        let v = json!({ "version": 1, "books": [{ "id": "a", "tags": [] }] });
        assert_eq!(
            to_pretty(&v).unwrap(),
            "{\n  \"version\": 1,\n  \"books\": [\n    {\n      \"id\": \"a\",\n      \"tags\": []\n    }\n  ]\n}"
        );
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("nested/x.json");
        write(&p, &json!({ "a": 1 })).unwrap();
        assert_eq!(read::<Value>(&p).unwrap(), json!({ "a": 1 }));
        assert!(!dir.path().join("nested/x.json.tmp").exists());
    }

    #[test]
    fn merge_overwrites_top_level_keys() {
        let mut base = json!({ "a": 1, "b": 2 }).as_object().unwrap().clone();
        merge(&mut base, &json!({ "b": 3, "c": 4 }));
        assert_eq!(Value::Object(base), json!({ "a": 1, "b": 3, "c": 4 }));
    }
}
