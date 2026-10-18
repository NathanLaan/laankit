//! The project index (`noteliner.json`) as the UI sees it: typed file
//! entries for drawing, and the tree edits of `stores/project.svelte.js` and
//! `Sidebar.svelte` (drag-and-drop, move, clear tags).
//!
//! The index itself stays a `serde_json::Value`, exactly as read from disk,
//! so fields this build doesn't know (and key order) survive the
//! `project:saveIndex` round trip to the Tauri and Electron builds. The edits
//! below change only the fields the Svelte code changes.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde_json::{json, Value};

/// `UNTAGGED_KEY`: the tag-filter row for files without tags. A leading
/// space can't appear in a real tag.
pub const UNTAGGED_KEY: &str = " __untagged__";

/// One note in the index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub filename: String,
    pub parent_id: Option<String>,
    pub order: f64,
    pub tags: Vec<String>,
    pub attachments: usize,
    /// The derived `deck` flag: the note has a `presentation:` block.
    pub deck: bool,
    pub created_at: String,
    pub modified_at: String,
}

impl Entry {
    pub fn from_value(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        Some(Self {
            id: v["id"].as_str()?.to_string(),
            name: s("name"),
            filename: s("filename"),
            parent_id: v["parentId"].as_str().map(str::to_string),
            order: v["order"].as_f64().unwrap_or(0.0),
            tags: v["tags"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|t| t.as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default(),
            attachments: v["attachments"].as_array().map_or(0, Vec::len),
            deck: v["deck"].as_bool().unwrap_or(false),
            created_at: s("createdAt"),
            modified_at: s("modifiedAt"),
        })
    }
}

/// The entries of an index, in file order.
pub fn entries(index: &Value) -> Vec<Entry> {
    index["files"]
        .as_array()
        .map(|files| files.iter().filter_map(Entry::from_value).collect())
        .unwrap_or_default()
}

/// `projectState.load`: stamps `tagMeta` and seeds `tagPalette` from the
/// files' tags on older indexes.
pub fn normalize_index(index: &mut Value) {
    if !index["files"].is_array() {
        index["files"] = json!([]);
    }
    if !index["tagMeta"].is_object() {
        index["tagMeta"] = json!({});
    }
    if !index["tagPalette"].is_array() {
        let mut seed: Vec<String> = Vec::new();
        for e in entries(index) {
            for t in e.tags {
                if !seed.contains(&t) {
                    seed.push(t);
                }
            }
        }
        index["tagPalette"] = json!(seed);
    }
}

/// The Files pane's sort (`filesSortMode`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    #[default]
    User,
    NameAsc,
    NameDesc,
    ModifiedDesc,
    CreatedDesc,
}

impl SortMode {
    pub const ALL: [SortMode; 5] = [
        SortMode::User,
        SortMode::NameAsc,
        SortMode::NameDesc,
        SortMode::ModifiedDesc,
        SortMode::CreatedDesc,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SortMode::User => "user",
            SortMode::NameAsc => "name-asc",
            SortMode::NameDesc => "name-desc",
            SortMode::ModifiedDesc => "modified-desc",
            SortMode::CreatedDesc => "created-desc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::User => "User Order",
            SortMode::NameAsc => "Name A-Z",
            SortMode::NameDesc => "Name Z-A",
            SortMode::ModifiedDesc => "Last Modified",
            SortMode::CreatedDesc => "Created",
        }
    }

    pub fn from_id(id: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|m| m.id() == id)
            .unwrap_or_default()
    }
}

/// `localeCompare(b, undefined, { numeric: true, sensitivity: 'base' })`,
/// near enough: case-insensitive, with digit runs compared as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let take = |it: &mut std::iter::Peekable<std::str::Chars<'_>>| {
                    let mut n = String::new();
                    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
                        n.push(c);
                        it.next();
                    }
                    n
                };
                let (na, nb) = (take(&mut a), take(&mut b));
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

/// `Date.parse(v) || 0`, in milliseconds.
fn timestamp(v: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(v)
        .map(|d| d.timestamp_millis())
        .unwrap_or(0)
}

/// `sortKids`.
fn sort_kids(kids: &mut [&Entry], mode: SortMode) {
    let by_name = |a: &&Entry, b: &&Entry| natural_cmp(&a.name, &b.name);
    match mode {
        SortMode::NameAsc => kids.sort_by(by_name),
        SortMode::NameDesc => kids.sort_by(|a, b| by_name(b, a)),
        SortMode::ModifiedDesc => kids.sort_by(|a, b| {
            timestamp(&b.modified_at)
                .cmp(&timestamp(&a.modified_at))
                .then_with(|| by_name(a, b))
        }),
        SortMode::CreatedDesc => kids.sort_by(|a, b| {
            timestamp(&b.created_at)
                .cmp(&timestamp(&a.created_at))
                .then_with(|| by_name(a, b))
        }),
        SortMode::User => kids.sort_by(|a, b| a.order.total_cmp(&b.order)),
    }
}

/// `passesTagFilter`: visible if any of its tags is checked; untagged files
/// follow the "No Tags" row.
pub fn passes_tag_filter(e: &Entry, hidden: &HashSet<String>) -> bool {
    if e.tags.is_empty() {
        return !hidden.contains(UNTAGGED_KEY);
    }
    e.tags.iter().any(|t| !hidden.contains(t))
}

/// `getChildren`: filtered and sorted, for the tree.
pub fn children<'a>(
    entries: &'a [Entry],
    parent: Option<&str>,
    mode: SortMode,
    hidden: &HashSet<String>,
) -> Vec<&'a Entry> {
    let mut kids: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.parent_id.as_deref() == parent && passes_tag_filter(e, hidden))
        .collect();
    sort_kids(&mut kids, mode);
    kids
}

/// `getAllChildren`: unfiltered, in user order (drag-reorder renumbers these).
pub fn all_children<'a>(entries: &'a [Entry], parent: Option<&str>) -> Vec<&'a Entry> {
    let mut kids: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.parent_id.as_deref() == parent)
        .collect();
    sort_kids(&mut kids, SortMode::User);
    kids
}

/// `getFlatFileList`: depth-first in display order, with each file's depth.
pub fn flat_list<'a>(
    entries: &'a [Entry],
    mode: SortMode,
    hidden: &HashSet<String>,
) -> Vec<(&'a Entry, usize)> {
    fn walk<'a>(
        entries: &'a [Entry],
        parent: Option<&str>,
        depth: usize,
        mode: SortMode,
        hidden: &HashSet<String>,
        out: &mut Vec<(&'a Entry, usize)>,
        seen: &mut HashSet<&'a str>,
    ) {
        for e in children(entries, parent, mode, hidden) {
            // A corrupt index with a parent cycle mustn't recurse forever.
            if !seen.insert(e.id.as_str()) {
                continue;
            }
            out.push((e, depth));
            walk(entries, Some(&e.id), depth + 1, mode, hidden, out, seen);
        }
    }
    let mut out = Vec::new();
    walk(
        entries,
        None,
        0,
        mode,
        hidden,
        &mut out,
        &mut HashSet::new(),
    );
    out
}

/// The parent picker's options (`parentOptions`): every file depth-first in
/// user order, minus `exclude` and its subtree (MoveFileModal).
pub fn parent_options(entries: &[Entry], exclude: Option<&str>) -> Vec<(String, String, usize)> {
    let skip = exclude.map(|id| subtree(entries, id)).unwrap_or_default();
    let mut out = Vec::new();
    fn walk(
        entries: &[Entry],
        parent: Option<&str>,
        depth: usize,
        skip: &HashSet<String>,
        out: &mut Vec<(String, String, usize)>,
    ) {
        for e in all_children(entries, parent) {
            if skip.contains(&e.id) || out.iter().any(|o| o.0 == e.id) {
                continue;
            }
            out.push((e.id.clone(), e.name.clone(), depth));
            walk(entries, Some(&e.id), depth + 1, skip, out);
        }
    }
    walk(entries, None, 0, &skip, &mut out);
    out
}

/// `id` and everything below it.
pub fn subtree(entries: &[Entry], id: &str) -> HashSet<String> {
    let mut set = HashSet::new();
    let mut stack = vec![id.to_string()];
    while let Some(cur) = stack.pop() {
        if !set.insert(cur.clone()) {
            continue;
        }
        for e in entries
            .iter()
            .filter(|e| e.parent_id.as_deref() == Some(cur.as_str()))
        {
            stack.push(e.id.clone());
        }
    }
    set
}

/// Where a dragged file lands relative to the row under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropPos {
    Before,
    After,
    Child,
}

fn file_mut<'a>(index: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    index["files"]
        .as_array_mut()?
        .iter_mut()
        .find(|f| f["id"].as_str() == Some(id))
}

/// `Sidebar.handleDrop`: re-parent and renumber. Returns whether the index
/// changed. Unlike the Svelte build, a file can't be dropped into its own
/// subtree (that would detach it from the tree).
pub fn apply_drop(index: &mut Value, dragged: &str, target: &str, pos: DropPos) -> bool {
    let list = entries(index);
    let (Some(_), Some(t)) = (
        list.iter().find(|e| e.id == dragged),
        list.iter().find(|e| e.id == target),
    ) else {
        return false;
    };
    if dragged == target || subtree(&list, dragged).contains(target) {
        return false;
    }
    match pos {
        DropPos::Child => {
            let n = all_children(&list, Some(target))
                .iter()
                .filter(|e| e.id != dragged)
                .count();
            let Some(f) = file_mut(index, dragged) else {
                return false;
            };
            f["parentId"] = json!(target);
            f["order"] = json!(n);
        }
        DropPos::Before | DropPos::After => {
            let parent = t.parent_id.clone();
            let mut siblings: Vec<String> = all_children(&list, parent.as_deref())
                .iter()
                .filter(|e| e.id != dragged)
                .map(|e| e.id.clone())
                .collect();
            let at = siblings.iter().position(|id| id == target).unwrap_or(0);
            let insert = if pos == DropPos::Before { at } else { at + 1 };
            siblings.insert(insert.min(siblings.len()), dragged.to_string());
            if let Some(f) = file_mut(index, dragged) {
                f["parentId"] = json!(parent);
            }
            for (i, id) in siblings.iter().enumerate() {
                if let Some(f) = file_mut(index, id) {
                    f["order"] = json!(i);
                }
            }
        }
    }
    true
}

/// `handleMoveFileConfirm`: re-parent to the end of the new parent's
/// children. Returns whether anything changed.
pub fn move_to(index: &mut Value, id: &str, parent: Option<&str>) -> bool {
    let list = entries(index);
    let Some(e) = list.iter().find(|e| e.id == id) else {
        return false;
    };
    if e.parent_id.as_deref() == parent {
        return false;
    }
    if let Some(p) = parent {
        if subtree(&list, id).contains(p) {
            return false;
        }
    }
    let n = all_children(&list, parent)
        .iter()
        .filter(|s| s.id != id)
        .count();
    let Some(f) = file_mut(index, id) else {
        return false;
    };
    f["parentId"] = json!(parent);
    f["order"] = json!(n);
    true
}

/// `handleClearTagsConfirm`.
pub fn clear_tags(index: &mut Value, id: &str) -> bool {
    match file_mut(index, id) {
        Some(f) if f["tags"].as_array().is_some_and(|t| !t.is_empty()) => {
            f["tags"] = json!([]);
            true
        }
        _ => false,
    }
}

/// `projectState.addFile`.
pub fn add_file(index: &mut Value, entry: Value) {
    if let Some(files) = index["files"].as_array_mut() {
        files.push(entry);
    }
}

/// `projectState.removeFile`.
pub fn remove_file(index: &mut Value, id: &str) {
    if let Some(files) = index["files"].as_array_mut() {
        files.retain(|f| f["id"].as_str() != Some(id));
    }
}

/// `projectState.updateFile`: merges `updates`' keys into the entry.
pub fn update_file(index: &mut Value, id: &str, updates: &Value) {
    if let (Some(f), Some(u)) = (file_mut(index, id), updates.as_object()) {
        for (k, v) in u {
            f[k] = v.clone();
        }
    }
}

/// `allTags`: the palette plus every file's tags, natural-sorted.
pub fn all_tags(index: &Value) -> Vec<String> {
    let mut tags: Vec<String> = index["tagPalette"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    for e in entries(index) {
        for t in e.tags {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
    }
    tags.sort_by(|a, b| natural_cmp(a, b));
    tags
}

/// `addTag`: puts `tag` (trimmed) on the file and in the palette. Returns
/// whether the index changed.
pub fn add_tag(index: &mut Value, id: &str, tag: &str) -> bool {
    let tag = tag.trim();
    if tag.is_empty() {
        return false;
    }
    let Some(f) = file_mut(index, id) else {
        return false;
    };
    let mut changed = false;
    if !f["tags"].is_array() {
        f["tags"] = json!([]);
    }
    let tags = f["tags"].as_array_mut().expect("tags is an array");
    if !tags.iter().any(|t| t == tag) {
        tags.push(json!(tag));
        changed = true;
    }
    if !index["tagPalette"].is_array() {
        index["tagPalette"] = json!([]);
    }
    let palette = index["tagPalette"]
        .as_array_mut()
        .expect("tagPalette is an array");
    if !palette.iter().any(|t| t == tag) {
        palette.push(json!(tag));
        changed = true;
    }
    changed
}

/// `removeTag`: takes `tag` off the file but leaves it in the palette, so
/// its chip stays and can be re-checked.
pub fn remove_tag(index: &mut Value, id: &str, tag: &str) -> bool {
    let Some(tags) = file_mut(index, id).and_then(|f| f["tags"].as_array_mut()) else {
        return false;
    };
    let before = tags.len();
    tags.retain(|t| t != tag);
    tags.len() != before
}

/// `getTagColor`: the colour name in `tagMeta`, if any.
pub fn tag_color<'a>(index: &'a Value, tag: &str) -> Option<&'a str> {
    index["tagMeta"][tag]["color"]
        .as_str()
        .filter(|c| !c.is_empty())
}

/// `setTagColor`: `None` drops the tag's metadata entirely, so "no
/// metadata" round-trips as no entry.
pub fn set_tag_color(index: &mut Value, tag: &str, color: Option<&str>) {
    if !index["tagMeta"].is_object() {
        index["tagMeta"] = json!({});
    }
    let meta = index["tagMeta"]
        .as_object_mut()
        .expect("tagMeta is an object");
    match color {
        None => {
            meta.remove(tag);
        }
        Some(c) => {
            let entry = meta.entry(tag.to_string()).or_insert_with(|| json!({}));
            if !entry.is_object() {
                *entry = json!({});
            }
            entry["color"] = json!(c);
        }
    }
}

/// `untaggedCount`: files with no tags (the filter's "No Tags" row).
pub fn untagged_count(entries: &[Entry]) -> usize {
    entries.iter().filter(|e| e.tags.is_empty()).count()
}

/// `getFilesWithTag(tag).length`.
pub fn tag_count(entries: &[Entry], tag: &str) -> usize {
    entries
        .iter()
        .filter(|e| e.tags.iter().any(|t| t == tag))
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index() -> Value {
        json!({
            "version": 3,
            "files": [
                { "id": "a", "name": "Alpha", "filename": "alpha.md", "parentId": null, "order": 0, "tags": ["x"], "attachments": [] },
                { "id": "b", "name": "Beta", "filename": "beta.md", "parentId": null, "order": 1, "tags": [], "attachments": [], "extra": true },
                { "id": "c", "name": "item10", "filename": "c.md", "parentId": "a", "order": 0, "tags": ["y"], "attachments": [{}] },
                { "id": "d", "name": "item2", "filename": "d.md", "parentId": "a", "order": 1, "tags": [], "attachments": [] }
            ]
        })
    }

    fn order_of(index: &Value) -> Vec<(String, Option<String>, i64)> {
        entries(index)
            .into_iter()
            .map(|e| (e.id, e.parent_id, e.order as i64))
            .collect()
    }

    #[test]
    fn normalize_seeds_the_palette() {
        let mut i = index();
        normalize_index(&mut i);
        assert_eq!(i["tagPalette"], json!(["x", "y"]));
        assert_eq!(i["tagMeta"], json!({}));
        assert_eq!(all_tags(&i), ["x", "y"]);
    }

    #[test]
    fn natural_sorting() {
        let list = entries(&index());
        let kids = children(&list, Some("a"), SortMode::NameAsc, &HashSet::new());
        assert_eq!(kids[0].name, "item2");
        assert_eq!(natural_cmp("Work", "archive"), Ordering::Greater);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Equal);
    }

    #[test]
    fn drop_before_after_and_child() {
        let mut i = index();
        assert!(apply_drop(&mut i, "b", "a", DropPos::Before));
        assert_eq!(
            order_of(&i)[..2],
            [("a".into(), None, 1), ("b".into(), None, 0)]
        );
        assert!(apply_drop(&mut i, "d", "b", DropPos::After));
        let o = order_of(&i);
        assert_eq!(o[3], ("d".into(), None, 1));
        assert_eq!(o[0].2, 2, "a was renumbered after d");
        assert!(apply_drop(&mut i, "b", "c", DropPos::Child));
        assert_eq!(order_of(&i)[1], ("b".into(), Some("c".into()), 0));
        // Unknown keys survive.
        assert_eq!(i["files"][1]["extra"], json!(true));
    }

    #[test]
    fn no_drop_into_own_subtree() {
        let mut i = index();
        assert!(!apply_drop(&mut i, "a", "c", DropPos::Child));
        assert!(!apply_drop(&mut i, "a", "a", DropPos::After));
        assert!(!move_to(&mut i, "a", Some("d")));
        assert_eq!(order_of(&i), order_of(&index()));
    }

    #[test]
    fn move_and_clear_tags() {
        let mut i = index();
        assert!(move_to(&mut i, "b", Some("a")));
        assert_eq!(order_of(&i)[1], ("b".into(), Some("a".into()), 2));
        assert!(!move_to(&mut i, "b", Some("a")));
        assert!(clear_tags(&mut i, "a"));
        assert!(!clear_tags(&mut i, "a"));
        let opts = parent_options(&entries(&i), Some("a"));
        assert_eq!(opts.len(), 0, "a's subtree is everything but nothing else");
    }

    #[test]
    fn tag_filter_and_flat_list() {
        let list = entries(&index());
        let mut hidden = HashSet::new();
        let flat: Vec<&str> = flat_list(&list, SortMode::User, &hidden)
            .iter()
            .map(|(e, _)| e.id.as_str())
            .collect();
        assert_eq!(flat, ["a", "c", "d", "b"]);
        hidden.insert(UNTAGGED_KEY.to_string());
        let flat: Vec<&str> = flat_list(&list, SortMode::User, &hidden)
            .iter()
            .map(|(e, _)| e.id.as_str())
            .collect();
        assert_eq!(flat, ["a", "c"]);
    }

    #[test]
    fn tags_add_remove_and_colors() {
        let mut i = index();
        normalize_index(&mut i);
        assert!(add_tag(&mut i, "b", "  new "));
        assert!(!add_tag(&mut i, "b", "new"));
        assert!(!add_tag(&mut i, "b", "   "));
        assert_eq!(i["files"][1]["tags"], json!(["new"]));
        assert_eq!(i["tagPalette"], json!(["x", "y", "new"]));
        assert!(remove_tag(&mut i, "b", "new"));
        assert!(!remove_tag(&mut i, "b", "new"));
        // The chip stays in the palette after the last file drops it.
        assert_eq!(all_tags(&i), ["new", "x", "y"]);
        set_tag_color(&mut i, "x", Some("Tomato"));
        assert_eq!(tag_color(&i, "x"), Some("Tomato"));
        set_tag_color(&mut i, "x", None);
        assert_eq!(i["tagMeta"], json!({}));
        let list = entries(&i);
        assert_eq!(untagged_count(&list), 2);
        assert_eq!(tag_count(&list, "y"), 1);
    }
}
