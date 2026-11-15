//! A note's YAML frontmatter: the `---` block at the top of its file.
//!
//! NoteLiner writes two fields there, `tags` and `created`, so other tools
//! can read them. Everything else in the block belongs to the user and is
//! kept as it is, in the order it is in. The block is read with
//! gray-matter's rules, ported from the Electron build.

use serde_json::Value;

use crate::yaml::{self, Yaml};

/// The fields NoteLiner writes; anything else belongs to the user.
pub const MIRROR_FIELDS: [&str; 2] = ["tags", "created"];

pub type Data = Vec<(String, Yaml)>;

pub struct Parsed {
    pub data: Data,
    pub body: String,
}

/// A note's file, cut at its frontmatter block.
pub struct Split<'a> {
    /// The block as written: from the opening `---` to the line break after
    /// the closing one.
    pub block: &'a str,
    /// The YAML between the two `---` lines.
    pub matter: &'a str,
    /// The note's text.
    pub body: &'a str,
}

/// What a note's block holds.
pub enum Block {
    /// The file has no block.
    Missing,
    /// The block isn't YAML NoteLiner can read (a half-typed line).
    Unreadable,
    Data(Data),
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

/// Finds the block: an opening `---` on the first line (a language name may
/// follow it, `---yaml`) and a closing `---` line. `None` when the file has
/// no block, or one that is never closed.
pub fn split(raw: &str) -> Option<Split<'_>> {
    let content = raw.strip_prefix('\u{feff}').unwrap_or(raw);
    if !content.starts_with("---") || content[3..].starts_with('-') {
        return None;
    }
    let mut s = &content[3..];
    let first_line = s[..s.find('\n')?].trim_end_matches('\r');
    let lang = first_line.trim();
    if !lang.is_empty() {
        if lang != "yaml" && lang != "yml" {
            return None;
        }
        s = &s[first_line.len()..];
    }
    // The closing delimiter is a line of its own.
    let mut from = 0;
    let close = loop {
        let at = from + s[from..].find("\n---")?;
        let after = &s[at + 4..];
        if after.is_empty() || after.starts_with('\n') || after.starts_with("\r\n") {
            break at;
        }
        from = at + 4;
    };
    let mut body = &s[close + 4..];
    body = body.strip_prefix('\r').unwrap_or(body);
    body = body.strip_prefix('\n').unwrap_or(body);
    Some(Split {
        block: &raw[..raw.len() - body.len()],
        matter: &s[..close],
        body,
    })
}

/// The YAML of a block, as a map. A block of comments or blank lines is empty.
fn load_matter(matter: &str) -> Result<Data, String> {
    let blank = matter
        .lines()
        .all(|l| l.trim().is_empty() || (l.trim_start().starts_with('#') && l.trim().len() > 1));
    if blank {
        return Ok(Vec::new());
    }
    match yaml::load(matter)? {
        Yaml::Map(m) => Ok(m),
        Yaml::Null => Ok(Vec::new()),
        _ => Err("frontmatter is not a map".into()),
    }
}

/// What the file's block holds.
pub fn block(raw: &str) -> Block {
    match split(raw) {
        None => Block::Missing,
        Some(s) => match load_matter(s.matter) {
            Ok(data) => Block::Data(data),
            Err(_) => Block::Unreadable,
        },
    }
}

/// The block's data and the note's text. A file with no block, or with one
/// that can't be read, is all text.
pub fn parse(raw: &str) -> Parsed {
    if let Some(s) = split(raw) {
        if let Ok(data) = load_matter(s.matter) {
            return Parsed {
                data,
                body: s.body.to_string(),
            };
        }
    }
    Parsed {
        data: Vec::new(),
        body: raw.strip_prefix('\u{feff}').unwrap_or(raw).to_string(),
    }
}

/// The note's text, without its block. A block that can't be read is still
/// left out.
pub fn strip_body(raw: &str) -> String {
    match split(raw) {
        Some(s) => s.body.to_string(),
        None => raw.strip_prefix('\u{feff}').unwrap_or(raw).to_string(),
    }
}

/// The byte where the note's text starts in `raw`.
pub fn body_start(raw: &str) -> usize {
    split(raw).map_or(0, |s| s.block.len())
}

fn newline(s: &str) -> String {
    if s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// The block for `data`, with its two `---` lines. Empty for no data.
pub fn block_text(data: &Data) -> String {
    let cleaned: Data = data
        .iter()
        .filter(|(_, v)| *v != Yaml::Null)
        .cloned()
        .collect();
    if cleaned.is_empty() {
        return String::new();
    }
    let matter = yaml::dump(&Yaml::Map(cleaned));
    let matter = matter.trim();
    if matter == "{}" {
        return String::new();
    }
    format!("---\n{}---\n", newline(matter))
}

/// Body with frontmatter prepended, the fields in the order `data` has them.
/// Empty data (after dropping nulls) returns the body unchanged.
pub fn serialize(body: &str, data: &Data) -> String {
    let block = block_text(data);
    if block.is_empty() {
        return body.to_string();
    }
    block + &newline(body)
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn tag_list(entry: &Value) -> Yaml {
    match &entry["tags"] {
        Value::Array(a) => Yaml::Seq(a.iter().map(Yaml::from_json).collect()),
        _ => Yaml::Seq(Vec::new()),
    }
}

fn created_of(entry: &Value) -> Yaml {
    match entry["createdAt"].as_str().filter(|c| !c.is_empty()) {
        Some(c) => Yaml::Str(c.to_string()),
        None => Yaml::Str(now_iso()),
    }
}

/// The block for an index entry: its tags, and a `created` date if the block
/// has none. The user's fields, and `created` once written, stay as they are.
pub fn mirror_from_entry(entry: &Value, existing: &Data) -> Data {
    let mut data = Vec::with_capacity(existing.len() + 2);
    // A new block starts with the two fields NoteLiner writes.
    if get(existing, "tags").is_none() {
        data.push(("tags".to_string(), tag_list(entry)));
    }
    if get(existing, "created").is_none() {
        data.push(("created".to_string(), created_of(entry)));
    }
    data.extend(existing.iter().cloned());
    set(&mut data, "tags", tag_list(entry));
    data
}

/// The fields a save puts back when the block has lost them. `None` when the
/// block has both `tags` and `created`, so the file is saved as typed.
pub fn restored(entry: &Value, existing: &Data) -> Option<Data> {
    let (tags, created) = (
        get(existing, "tags").is_some(),
        get(existing, "created").is_some(),
    );
    if tags && created {
        return None;
    }
    let mut data = Vec::with_capacity(existing.len() + 2);
    if !tags {
        data.push(("tags".to_string(), tag_list(entry)));
    }
    if !created {
        data.push(("created".to_string(), created_of(entry)));
    }
    data.extend(existing.iter().cloned());
    Some(data)
}

/// The tags typed in a block: `None` when it has no `tags` field. A list
/// gives its text items, a single value is one tag, and an empty field is
/// no tags.
pub fn tags_of(data: &Data) -> Option<Vec<String>> {
    let scalar = |v: &Yaml| match v {
        Yaml::Str(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Yaml::Int(i) => Some(i.to_string()),
        Yaml::Float(f) => Some(f.to_string()),
        Yaml::Bool(b) => Some(b.to_string()),
        _ => None,
    };
    let mut out: Vec<String> = Vec::new();
    match get(data, "tags")? {
        Yaml::Seq(items) => {
            for t in items.iter().filter_map(scalar) {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
        other => out.extend(scalar(other)),
    }
    Some(out)
}

/// `raw` with its block's tags set from `entry`. `None` when the block can't
/// be read: NoteLiner doesn't rewrite a block it can't read.
pub fn retag(raw: &str, entry: &Value) -> Option<String> {
    let data = match block(raw) {
        Block::Unreadable => return None,
        Block::Missing => Vec::new(),
        Block::Data(d) => d,
    };
    Some(serialize(
        &strip_body(raw),
        &mirror_from_entry(entry, &data),
    ))
}

/// `raw` with its `presentation:` block added, replaced or (with null)
/// removed. `None` when the block can't be read.
pub fn with_presentation(raw: &str, presentation: &Value) -> Option<String> {
    let mut data = match block(raw) {
        Block::Unreadable => return None,
        Block::Missing => Vec::new(),
        Block::Data(d) => d,
    };
    if presentation.is_null() {
        remove(&mut data, "presentation");
    } else {
        set(&mut data, "presentation", Yaml::from_json(presentation));
    }
    Some(serialize(&strip_body(raw), &data))
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

        // A block that is never closed is the note's text.
        let p = parse("---\na: 1\n");
        assert!(p.data.is_empty());
        assert_eq!(p.body, "---\na: 1\n");
    }

    #[test]
    fn split_finds_the_block_as_written() {
        let raw = "---\ntags: [a]\n---\n# Body\n---\nrule\n";
        let s = split(raw).unwrap();
        assert_eq!(s.block, "---\ntags: [a]\n---\n");
        assert_eq!(s.matter, "\ntags: [a]");
        assert_eq!(s.body, "# Body\n---\nrule\n");
        assert_eq!(body_start(raw), s.block.len());

        // The closing line is `---` and nothing else.
        let s = split("---\na: 1\n----\n---\nb").unwrap();
        assert_eq!(s.body, "b");
        let s = split("---\r\na: 1\r\n---\r\nb").unwrap();
        assert_eq!(s.body, "b");
        let s = split("---yaml\na: 1\n---").unwrap();
        assert_eq!((s.block, s.body), ("---yaml\na: 1\n---", ""));

        assert!(split("# Title\n---\nx\n---\n").is_none());
        assert!(split("---\nnever closed\n").is_none());
        assert!(split("---toml\na = 1\n---\n").is_none());
        assert_eq!(body_start("plain"), 0);

        // A block that can't be read is still not the note's text.
        let raw = "---\ntags: [\n---\nbody\n";
        assert!(matches!(block(raw), Block::Unreadable));
        assert_eq!(strip_body(raw), "body\n");
        assert!(matches!(block("body"), Block::Missing));
        assert!(matches!(block("---\n---\nbody"), Block::Data(d) if d.is_empty()));
    }

    #[test]
    fn serialize_keeps_the_order() {
        let data: Data = vec![
            ("custom".into(), Yaml::Str("keep".into())),
            ("name".into(), Yaml::Str("Note".into())),
            ("id".into(), Yaml::Str("abc".into())),
            ("gone".into(), Yaml::Null),
        ];
        assert_eq!(
            serialize("body", &data),
            "---\ncustom: keep\nname: Note\nid: abc\n---\nbody\n"
        );
        assert_eq!(
            serialize("", &data),
            "---\ncustom: keep\nname: Note\nid: abc\n---\n\n"
        );
        assert_eq!(serialize("x", &Vec::new()), "x");
    }

    #[test]
    fn mirror_writes_tags_and_created_only() {
        let entry = json!({ "id": "new", "name": "N", "tags": ["t"], "createdAt": "2026-01-02T03:04:05.000Z" });
        // A new block: tags, then created.
        let m = mirror_from_entry(&entry, &Vec::new());
        assert_eq!(
            block_text(&m),
            "---\ntags:\n  - t\ncreated: '2026-01-02T03:04:05.000Z'\n---\n"
        );
        // An older block keeps its lines where they are; only tags change.
        let existing = parse(
            "---\nid: old\nname: Old\ntags: []\ncreated: 2020-01-01\nupdated: x\nmine: 1\n---\n",
        )
        .data;
        let m = mirror_from_entry(&entry, &existing);
        let keys: Vec<&str> = m.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["id", "name", "tags", "created", "updated", "mine"]);
        assert_eq!(get(&m, "id"), Some(&Yaml::Str("old".into())));
        assert_eq!(get(&m, "name"), Some(&Yaml::Str("Old".into())));
        assert!(matches!(get(&m, "created"), Some(Yaml::Date(_))));
        assert_eq!(
            get(&m, "tags"),
            Some(&Yaml::Seq(vec![Yaml::Str("t".into())]))
        );
    }

    #[test]
    fn a_save_puts_back_only_what_is_missing() {
        let entry = json!({ "tags": ["t"], "createdAt": "2026-01-02T03:04:05.000Z" });
        let whole = parse("---\ntags: [a]\ncreated: 2020-01-01\n---\n").data;
        assert!(restored(&entry, &whole).is_none());
        // An empty `tags:` is no tags, not a missing field.
        let empty = parse("---\ntags:\ncreated: 2020-01-01\n---\n").data;
        assert!(restored(&entry, &empty).is_none());
        assert_eq!(tags_of(&empty), Some(Vec::new()));

        let no_tags = parse("---\ncreated: 2020-01-01\nmine: 1\n---\n").data;
        let r = restored(&entry, &no_tags).unwrap();
        let keys: Vec<&str> = r.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["tags", "created", "mine"]);
        assert_eq!(tags_of(&r), Some(vec!["t".to_string()]));
        assert!(tags_of(&no_tags).is_none());

        let r = restored(&entry, &Vec::new()).unwrap();
        assert_eq!(
            get(&r, "created"),
            Some(&Yaml::Str("2026-01-02T03:04:05.000Z".into()))
        );
    }

    #[test]
    fn tags_typed_in_a_block() {
        let tags = |raw: &str| tags_of(&parse(raw).data);
        assert_eq!(
            tags("---\ntags:\n  - a\n  - b\n  - a\n  -\n---\n"),
            Some(vec!["a".to_string(), "b".to_string()])
        );
        assert_eq!(
            tags("---\ntags: work\n---\n"),
            Some(vec!["work".to_string()])
        );
        assert_eq!(
            tags("---\ntags: [1, x]\n---\n"),
            Some(vec!["1".to_string(), "x".to_string()])
        );
        assert_eq!(tags("---\nother: 1\n---\n"), None);
    }

    #[test]
    fn retag_and_presentation_leave_unreadable_blocks() {
        let entry = json!({ "tags": ["x"], "createdAt": "2026-01-02T03:04:05.000Z" });
        let raw = "---\ntags: []\ncreated: '2026-01-02T03:04:05.000Z'\nmine: 1\n---\nbody\n";
        assert_eq!(
            retag(raw, &entry).unwrap(),
            "---\ntags:\n  - x\ncreated: '2026-01-02T03:04:05.000Z'\nmine: 1\n---\nbody\n"
        );
        assert!(retag("---\ntags: [\n---\nbody\n", &entry).is_none());
        let deck = with_presentation(raw, &json!({ "theme": "dark" })).unwrap();
        assert!(deck.contains("presentation:\n  theme: dark\n---\nbody\n"));
        assert_eq!(with_presentation(&deck, &Value::Null).unwrap(), raw);
        assert!(with_presentation("---\na: [\n---\n", &Value::Null).is_none());
    }
}
