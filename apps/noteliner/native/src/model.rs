//! The project as the UI sees it: typed entries for the notes and diagrams
//! in the index (`noteliner.json`), and the tree the folders on disk make.
//!
//! The folders are the tree. A folder holds folders, notes and diagrams; a
//! note's folder is the folder part of its `filename`. Moving something is
//! a file operation the backend does, so nothing here moves a note. The
//! edits below change what the index alone holds: tags and their colours.
//!
//! The index itself stays a `serde_json::Value`, as the backend gave it, so
//! fields this build doesn't know survive.

use std::cmp::Ordering;
use std::collections::HashSet;

use serde_json::{json, Value};

pub use noteliner_core::project::FileKind;
use noteliner_core::tree;

/// `UNTAGGED_KEY`: the tag-filter row for files without tags. A leading
/// space can't appear in a real tag.
pub const UNTAGGED_KEY: &str = " __untagged__";

/// One note or diagram in the index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Entry {
    pub id: String,
    pub name: String,
    /// The file's path from the project folder, such as `work/kickoff.md`.
    pub filename: String,
    /// The folder the file is in, worked out from `filename`; empty at the
    /// top of the project.
    pub folder: String,
    pub tags: Vec<String>,
    pub attachments: usize,
    /// The derived `deck` flag: the note has a `presentation:` block.
    pub deck: bool,
    pub created_at: String,
    pub modified_at: String,
}

impl Entry {
    /// A note, or a diagram: read from the file's extension.
    pub fn kind(&self) -> FileKind {
        FileKind::of(&self.filename)
    }

    pub fn is_diagram(&self) -> bool {
        self.kind() == FileKind::Diagram
    }

    pub fn from_value(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let filename = s("filename");
        Some(Self {
            id: v["id"].as_str()?.to_string(),
            name: s("name"),
            folder: tree::folder_of(&filename).to_string(),
            filename,
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

/// The Files pane's sort (`filesSortMode`), for the notes and diagrams of
/// each folder. Folders come first, in name order, in every mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SortMode {
    #[default]
    NameAsc,
    NameDesc,
    ModifiedDesc,
    CreatedDesc,
}

impl SortMode {
    pub const ALL: [SortMode; 4] = [
        SortMode::NameAsc,
        SortMode::NameDesc,
        SortMode::ModifiedDesc,
        SortMode::CreatedDesc,
    ];

    pub fn id(self) -> &'static str {
        match self {
            SortMode::NameAsc => "name-asc",
            SortMode::NameDesc => "name-desc",
            SortMode::ModifiedDesc => "modified-desc",
            SortMode::CreatedDesc => "created-desc",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            SortMode::NameAsc => "Name A-Z",
            SortMode::NameDesc => "Name Z-A",
            SortMode::ModifiedDesc => "Last Modified",
            SortMode::CreatedDesc => "Created",
        }
    }

    /// The mode saved as `id`. A saved `user` (the order dragged by hand,
    /// which is gone) reads as Name A-Z.
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

/// The last part of a folder's path: its name.
pub fn folder_name(path: &str) -> &str {
    tree::file_name(path)
}

/// The folders directly inside `parent` (the project folder for `""`), in
/// name order.
pub fn child_folders<'a>(folders: &'a [String], parent: &str) -> Vec<&'a str> {
    let mut kids: Vec<&str> = folders
        .iter()
        .map(String::as_str)
        .filter(|f| tree::folder_of(f) == parent)
        .collect();
    kids.sort_by(|a, b| natural_cmp(folder_name(a), folder_name(b)).then_with(|| a.cmp(b)));
    kids
}

/// The notes and diagrams directly inside `folder`, filtered and sorted.
pub fn children<'a>(
    entries: &'a [Entry],
    folder: &str,
    mode: SortMode,
    hidden: &HashSet<String>,
) -> Vec<&'a Entry> {
    let mut kids: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.folder == folder && passes_tag_filter(e, hidden))
        .collect();
    sort_kids(&mut kids, mode);
    kids
}

/// Whether a folder shows: always with no tag filter, and otherwise only
/// while a note somewhere under it passes the filter.
pub fn folder_visible(entries: &[Entry], path: &str, hidden: &HashSet<String>) -> bool {
    hidden.is_empty()
        || entries
            .iter()
            .any(|e| tree::is_under(&e.filename, path) && passes_tag_filter(e, hidden))
}

/// A row of the tree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Node<'a> {
    /// A folder, by path.
    Folder(&'a str),
    File(&'a Entry),
}

/// The tree in display order, with each row's depth: in every folder, its
/// folders first, then its notes and diagrams. The rows inside a folder
/// that isn't in `open` are left out; `None` leaves none out.
pub fn rows<'a>(
    entries: &'a [Entry],
    folders: &'a [String],
    mode: SortMode,
    hidden: &HashSet<String>,
    open: Option<&HashSet<String>>,
) -> Vec<(Node<'a>, usize)> {
    #[allow(clippy::too_many_arguments)]
    fn walk<'a>(
        entries: &'a [Entry],
        folders: &'a [String],
        parent: &str,
        depth: usize,
        mode: SortMode,
        hidden: &HashSet<String>,
        open: Option<&HashSet<String>>,
        out: &mut Vec<(Node<'a>, usize)>,
    ) {
        for folder in child_folders(folders, parent) {
            if !folder_visible(entries, folder, hidden) {
                continue;
            }
            out.push((Node::Folder(folder), depth));
            if open.is_none_or(|o| o.contains(folder)) {
                walk(entries, folders, folder, depth + 1, mode, hidden, open, out);
            }
        }
        for e in children(entries, parent, mode, hidden) {
            out.push((Node::File(e), depth));
        }
    }
    let mut out = Vec::new();
    walk(entries, folders, "", 0, mode, hidden, open, &mut out);
    // A note whose folder the walk didn't list (a folder made a moment ago)
    // still shows, at the top.
    for e in entries {
        if !e.folder.is_empty() && !folders.contains(&e.folder) && passes_tag_filter(e, hidden) {
            out.push((Node::File(e), 0));
        }
    }
    out
}

/// Every note and diagram in display order, for Previous File and Next File.
pub fn flat_list<'a>(
    entries: &'a [Entry],
    folders: &'a [String],
    mode: SortMode,
    hidden: &HashSet<String>,
) -> Vec<&'a Entry> {
    rows(entries, folders, mode, hidden, None)
        .into_iter()
        .filter_map(|(node, _)| match node {
            Node::File(e) => Some(e),
            Node::Folder(_) => None,
        })
        .collect()
}

/// The folders to choose from, as (path, name, depth) in tree order, minus
/// `exclude` and the folders under it (a folder can't move into itself).
pub fn folder_options(folders: &[String], exclude: Option<&str>) -> Vec<(String, String, usize)> {
    fn walk(
        folders: &[String],
        parent: &str,
        depth: usize,
        exclude: Option<&str>,
        out: &mut Vec<(String, String, usize)>,
    ) {
        for f in child_folders(folders, parent) {
            if exclude.is_some_and(|x| tree::is_under(f, x)) {
                continue;
            }
            out.push((f.to_string(), folder_name(f).to_string(), depth));
            walk(folders, f, depth + 1, exclude, out);
        }
    }
    let mut out = Vec::new();
    walk(folders, "", 0, exclude, &mut out);
    out
}

/// Whether a dragged folder may land in `into`: not in itself, not in a
/// folder under it, and not where it already is.
pub fn folder_can_move(path: &str, into: &str) -> bool {
    !tree::is_under(into, path) && tree::folder_of(path) != into
}

fn file_mut<'a>(index: &'a mut Value, id: &str) -> Option<&'a mut Value> {
    index["files"]
        .as_array_mut()?
        .iter_mut()
        .find(|f| f["id"].as_str() == Some(id))
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

/// Puts `tag` in the project's palette, if it isn't there (a tag typed in a
/// note's frontmatter).
pub fn add_to_palette(index: &mut Value, tag: &str) {
    if !index["tagPalette"].is_array() {
        index["tagPalette"] = json!([]);
    }
    if let Some(palette) = index["tagPalette"].as_array_mut() {
        if !palette.iter().any(|t| t == tag) {
            palette.push(json!(tag));
        }
    }
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
            "version": 4,
            "files": [
                { "id": "a", "name": "Alpha", "filename": "alpha.md", "tags": ["x"], "attachments": [], "modifiedAt": "2026-01-03T00:00:00.000Z" },
                { "id": "b", "name": "Beta", "filename": "beta.md", "tags": [], "attachments": [], "extra": true, "modifiedAt": "2026-01-09T00:00:00.000Z" },
                { "id": "c", "name": "item10", "filename": "Work/c.md", "tags": ["y"], "attachments": [{}] },
                { "id": "d", "name": "item2", "filename": "Work/d.mmd", "tags": [], "attachments": [] },
                { "id": "e", "name": "Deep", "filename": "Work/Meetings/e.md", "tags": ["x"], "attachments": [] }
            ]
        })
    }

    fn folders() -> Vec<String> {
        ["Archive", "Work", "Work/Meetings", "work2"]
            .map(String::from)
            .to_vec()
    }

    fn labels(rows: &[(Node<'_>, usize)]) -> Vec<String> {
        rows.iter()
            .map(|(n, d)| match n {
                Node::Folder(f) => format!("{d}:{f}/"),
                Node::File(e) => format!("{d}:{}", e.id),
            })
            .collect()
    }

    #[test]
    fn entries_know_their_folder() {
        let list = entries(&index());
        assert_eq!(list[0].folder, "");
        assert_eq!(list[2].folder, "Work");
        assert_eq!(list[4].folder, "Work/Meetings");
        assert!(list[3].is_diagram());
        assert_eq!(folder_name("Work/Meetings"), "Meetings");
        assert_eq!(folder_name("Work"), "Work");
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
        let kids = children(&list, "Work", SortMode::NameAsc, &HashSet::new());
        assert_eq!(kids[0].name, "item2");
        assert_eq!(natural_cmp("Work", "archive"), Ordering::Greater);
        assert_eq!(natural_cmp("a01", "a1"), Ordering::Equal);
    }

    #[test]
    fn folders_come_first_in_every_sort() {
        let (list, folders) = (entries(&index()), folders());
        let none = HashSet::new();
        let all = rows(&list, &folders, SortMode::NameAsc, &none, None);
        assert_eq!(
            labels(&all),
            [
                "0:Archive/",
                "0:Work/",
                "1:Work/Meetings/",
                "2:e",
                "1:d",
                "1:c",
                "0:work2/",
                "0:a",
                "0:b"
            ]
        );
        // The sort changes the notes, not the folders.
        let by_date = rows(&list, &folders, SortMode::ModifiedDesc, &none, None);
        assert_eq!(labels(&by_date)[6..], ["0:work2/", "0:b", "0:a"]);
        let z_a = rows(&list, &folders, SortMode::NameDesc, &none, None);
        assert_eq!(labels(&z_a)[..2], ["0:Archive/", "0:Work/"]);
        assert_eq!(labels(&z_a)[3..6], ["2:e", "1:c", "1:d"]);
        // A saved User Order reads as Name A-Z.
        assert_eq!(SortMode::from_id("user"), SortMode::NameAsc);
        assert_eq!(SortMode::from_id("created-desc"), SortMode::CreatedDesc);
        assert!(SortMode::ALL.iter().all(|m| m.label() != "User Order"));
    }

    #[test]
    fn closed_folders_hide_their_rows() {
        let (list, folders) = (entries(&index()), folders());
        let none = HashSet::new();
        let mut open: HashSet<String> = HashSet::new();
        let shut = rows(&list, &folders, SortMode::NameAsc, &none, Some(&open));
        assert_eq!(
            labels(&shut),
            ["0:Archive/", "0:Work/", "0:work2/", "0:a", "0:b"]
        );
        open.insert("Work".into());
        let one = rows(&list, &folders, SortMode::NameAsc, &none, Some(&open));
        assert_eq!(
            labels(&one),
            [
                "0:Archive/",
                "0:Work/",
                "1:Work/Meetings/",
                "1:d",
                "1:c",
                "0:work2/",
                "0:a",
                "0:b"
            ]
        );
        // Previous File and Next File step through every note, open or not.
        let flat: Vec<&str> = flat_list(&list, &folders, SortMode::NameAsc, &none)
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(flat, ["e", "d", "c", "a", "b"]);
    }

    #[test]
    fn a_tag_filter_hides_folders_with_nothing_to_show() {
        let (list, folders) = (entries(&index()), folders());
        let mut hidden = HashSet::new();
        hidden.insert(UNTAGGED_KEY.to_string());
        hidden.insert("y".to_string());
        // Only notes tagged x are left: a at the top, e in Work/Meetings.
        let shown = rows(&list, &folders, SortMode::NameAsc, &hidden, None);
        assert_eq!(
            labels(&shown),
            ["0:Work/", "1:Work/Meetings/", "2:e", "0:a"]
        );
        // With no filter, empty folders show.
        assert!(folder_visible(&list, "Archive", &HashSet::new()));
        assert!(!folder_visible(&list, "Archive", &hidden));
    }

    #[test]
    fn folder_choices_leave_out_the_moved_folder() {
        let folders = folders();
        let all: Vec<(String, usize)> = folder_options(&folders, None)
            .into_iter()
            .map(|(p, _, d)| (p, d))
            .collect();
        assert_eq!(
            all,
            [
                ("Archive".to_string(), 0),
                ("Work".to_string(), 0),
                ("Work/Meetings".to_string(), 1),
                ("work2".to_string(), 0)
            ]
        );
        let moving: Vec<String> = folder_options(&folders, Some("Work"))
            .into_iter()
            .map(|(p, _, _)| p)
            .collect();
        assert_eq!(moving, ["Archive", "work2"]);
        assert!(!folder_can_move("Work", "Work"));
        assert!(!folder_can_move("Work", "Work/Meetings"));
        assert!(!folder_can_move("Work/Meetings", "Work"));
        assert!(folder_can_move("Work/Meetings", ""));
        assert!(folder_can_move("Work", "Archive"));
    }

    #[test]
    fn clearing_tags() {
        let mut i = index();
        assert!(clear_tags(&mut i, "a"));
        assert!(!clear_tags(&mut i, "a"));
        // Unknown keys survive.
        assert_eq!(i["files"][1]["extra"], json!(true));
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
