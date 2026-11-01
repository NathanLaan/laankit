//! YAML frontmatter mirror (`apps/noteliner/src/main/frontmatter-service.js`), with
//! gray-matter's parse/stringify rules ported exactly.
//!
//! `noteliner.json` stays authoritative; each note carries a derived YAML
//! block (id, name, tags, created, updated) so other tools can read it.
//! User-added fields are preserved untouched.

use serde_json::Value;

use crate::yaml::{self, Yaml};

/// Fields NoteLiner manages; anything else belongs to the user.
pub const MIRROR_FIELDS: [&str; 5] = ["id", "name", "tags", "created", "updated"];

pub type Data = Vec<(String, Yaml)>;

pub struct Parsed {
    pub data: Data,
    pub body: String,
}

fn get<'a>(data: &'a Data, key: &str) -> Option<&'a Yaml> {
    data.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

pub fn set(data: &mut Data, key: &str, value: Yaml) {
    match data.iter_mut().find(|(k, _)| k == key) {
        Some((_, v)) => *v = value,
        None => data.push((key.to_string(), value)),
    }
}

pub fn remove(data: &mut Data, key: &str) {
    data.retain(|(k, _)| k != key);
}

/// gray-matter `matter(raw)`; malformed frontmatter yields `{}` + raw body.
pub fn parse(raw: &str) -> Parsed {
    match parse_matter(raw) {
        Ok(p) => p,
        Err(_) => Parsed {
            data: Vec::new(),
            body: raw.to_string(),
        },
    }
}

fn parse_matter(input: &str) -> Result<Parsed, String> {
    if input.is_empty() {
        return Ok(Parsed {
            data: Vec::new(),
            body: String::new(),
        });
    }
    let content = input.strip_prefix('\u{feff}').unwrap_or(input);
    let plain = || Parsed {
        data: Vec::new(),
        body: content.to_string(),
    };
    if !content.starts_with("---") || content[3..].starts_with('-') {
        return Ok(plain());
    }
    let mut s = &content[3..];
    let len = s.len();
    // A language name may follow the opening delimiter ("---yaml").
    let first_line = match s.find('\n') {
        Some(i) => {
            &s[..if i > 0 && s.as_bytes()[i - 1] == b'\r' {
                i - 1
            } else {
                i
            }]
        }
        // JS: str.slice(0, -1) when no newline exists.
        None => &s[..s.len().saturating_sub(1)],
    };
    let lang = first_line.trim();
    if !lang.is_empty() {
        if lang != "yaml" && lang != "yml" {
            return Err(format!("unsupported frontmatter language: {lang}"));
        }
        s = &s[first_line.len()..];
    }
    let close = s.find("\n---").unwrap_or(len.min(s.len()));
    let matter = &s[..close];
    let block: String = matter
        .lines()
        .filter(|l| !(l.trim_start().starts_with('#') && l.trim_start().len() > 1))
        .collect::<Vec<_>>()
        .join("\n");
    let data = if block.trim().is_empty() {
        Vec::new()
    } else {
        match yaml::load(matter)? {
            Yaml::Map(m) => m,
            _ => Vec::new(),
        }
    };
    let body = if close >= s.len() {
        String::new()
    } else {
        let mut rest = &s[close + 4..];
        rest = rest.strip_prefix('\r').unwrap_or(rest);
        rest = rest.strip_prefix('\n').unwrap_or(rest);
        rest.to_string()
    };
    Ok(Parsed { data, body })
}

pub fn strip_body(raw: &str) -> String {
    parse(raw).body
}

fn newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// Body with frontmatter prepended — managed fields first, then the user's.
/// Empty data (after dropping nulls) returns the body unchanged.
pub fn serialize(body: &str, data: &Data) -> String {
    let cleaned: Data = data
        .iter()
        .filter(|(_, v)| *v != Yaml::Null)
        .cloned()
        .collect();
    if cleaned.is_empty() {
        return body.to_string();
    }
    let mut ordered: Data = Vec::new();
    for k in MIRROR_FIELDS {
        if let Some(v) = get(&cleaned, k) {
            ordered.push((k.to_string(), v.clone()));
        }
    }
    for (k, v) in &cleaned {
        if !MIRROR_FIELDS.contains(&k.as_str()) {
            ordered.push((k.clone(), v.clone()));
        }
    }
    let matter = yaml::dump(&Yaml::Map(ordered));
    let matter = matter.trim();
    let mut out = String::new();
    if matter != "{}" {
        out = format!("---\n{}---\n", newline(matter));
    }
    out + &newline(body)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn truthy(v: Option<&Yaml>) -> bool {
    match v {
        None | Some(Yaml::Null) => false,
        Some(Yaml::Bool(b)) => *b,
        Some(Yaml::Int(i)) => *i != 0,
        Some(Yaml::Float(f)) => *f != 0.0 && !f.is_nan(),
        Some(Yaml::Str(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// Data block from an index entry, preserving the user's existing fields.
pub fn mirror_from_entry(entry: &Value, existing: &Data) -> Data {
    let mut data = existing.clone();
    set(&mut data, "id", Yaml::from_json(&entry["id"]));
    set(&mut data, "name", Yaml::from_json(&entry["name"]));
    let tags = match &entry["tags"] {
        Value::Array(a) => Yaml::Seq(a.iter().map(Yaml::from_json).collect()),
        _ => Yaml::Seq(Vec::new()),
    };
    set(&mut data, "tags", tags);
    if !truthy(get(&data, "created")) {
        set(&mut data, "created", Yaml::Str(now_iso()));
    }
    set(&mut data, "updated", Yaml::Str(now_iso()));
    data
}

/// Whether any mirrored field (ignoring `updated`) differs. Lists compare
/// element-wise, a missing/non-list side counting as empty (as in JS).
pub fn mirror_diverges(a: &Data, b: &Data) -> bool {
    let empty: Vec<Yaml> = Vec::new();
    let as_list = |v: Option<&'_ Yaml>| match v {
        Some(Yaml::Seq(s)) => Some(s.clone()),
        _ => None,
    };
    MIRROR_FIELDS.iter().filter(|k| **k != "updated").any(|k| {
        let (av, bv) = (get(a, k), get(b, k));
        if matches!(av, Some(Yaml::Seq(_))) || matches!(bv, Some(Yaml::Seq(_))) {
            as_list(av).unwrap_or_else(|| empty.clone())
                != as_list(bv).unwrap_or_else(|| empty.clone())
        } else {
            av != bv
        }
    })
}

pub fn to_json(data: &Data) -> Value {
    Yaml::Map(data.clone()).to_json()
}

pub fn get_field<'a>(data: &'a Data, key: &str) -> Option<&'a Yaml> {
    get(data, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parse_like_gray_matter() {
        let p = parse("---\nid: a\ntags:\n  - x\n---\n# Body\n");
        assert_eq!(p.body, "# Body\n");
        assert_eq!(to_json(&p.data), json!({ "id": "a", "tags": ["x"] }));

        let p = parse("no frontmatter");
        assert!(p.data.is_empty());
        assert_eq!(p.body, "no frontmatter");

        let p = parse("----\nnot: fm\n");
        assert!(p.data.is_empty());

        let p = parse("---\nbad: [\n---\nbody");
        assert!(p.data.is_empty());
        assert_eq!(p.body, "---\nbad: [\n---\nbody");

        let p = parse("---\n# just a comment\n---\nbody");
        assert!(p.data.is_empty());
        assert_eq!(p.body, "body");

        let p = parse("---\na: 1\n");
        assert_eq!(p.body, "");
    }

    #[test]
    fn serialize_like_gray_matter() {
        let data: Data = vec![
            ("custom".into(), Yaml::Str("keep".into())),
            ("name".into(), Yaml::Str("Note".into())),
            ("id".into(), Yaml::Str("abc".into())),
            ("gone".into(), Yaml::Null),
        ];
        assert_eq!(
            serialize("body", &data),
            "---\nid: abc\nname: Note\ncustom: keep\n---\nbody\n"
        );
        assert_eq!(
            serialize("", &data),
            "---\nid: abc\nname: Note\ncustom: keep\n---\n\n"
        );
        assert_eq!(serialize("x", &Vec::new()), "x");
    }

    #[test]
    fn mirror_preserves_user_fields_and_created() {
        let existing = parse("---\nid: old\ncreated: 2020-01-01\nmine: 1\n---\n").data;
        let entry = json!({ "id": "new", "name": "N", "tags": ["t"] });
        let m = mirror_from_entry(&entry, &existing);
        assert_eq!(get(&m, "id"), Some(&Yaml::Str("new".into())));
        assert!(matches!(get(&m, "created"), Some(Yaml::Date(_))));
        assert_eq!(get(&m, "mine"), Some(&Yaml::Int(1)));
        assert!(mirror_diverges(&existing, &m));
        assert!(!mirror_diverges(&m, &mirror_from_entry(&entry, &m)));
    }
}
