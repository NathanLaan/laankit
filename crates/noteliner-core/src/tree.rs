//! The folders of a project: the walk that reads them from disk, and the
//! reconcile that brings `noteliner.json` in line with what the walk found.
//!
//! The folders say what exists and where. The index only adds what a file
//! can't hold: an id, a name, a diagram's tags, and a note's attachments.
//! Paths are kept from the project folder, with `/` between the parts, on
//! every system.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;
use std::time::SystemTime;

use anyhow::{bail, Result};
use serde_json::{json, Value};

use crate::frontmatter;
use crate::project::{stat_times, FileKind, INDEX_FILE};

/// The walk stops after this many files and folders, so that opening a home
/// folder by mistake doesn't hang the app.
pub const MAX_FILES: usize = 20_000;

/// One note or diagram the walk found.
#[derive(Clone, Debug, PartialEq)]
pub struct WalkFile {
    /// The path from the project folder, such as `work/kickoff.md`.
    pub path: String,
    pub size: u64,
}

/// What is on disk under a project folder.
#[derive(Clone, Debug, Default)]
pub struct Walk {
    /// Every folder, by path, parents before children.
    pub folders: Vec<String>,
    pub files: Vec<WalkFile>,
    /// The walk stopped at [`MAX_FILES`].
    pub truncated: bool,
    /// Each folder's modification time, the project folder as `""`.
    pub times: HashMap<String, SystemTime>,
}

/// A folder or file NoteLiner leaves alone: its name starts with `.` or `_`.
pub fn skipped(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('_')
}

/// A note or a diagram, by its extension.
pub fn is_project_file(name: &str) -> bool {
    matches!(
        crate::project::extname(name).to_lowercase().as_str(),
        ".md" | ".mmd" | ".mermaid"
    )
}

/// The folder part of a path: `work` for `work/kickoff.md`, empty at the top.
pub fn folder_of(path: &str) -> &str {
    path.rfind('/').map_or("", |i| &path[..i])
}

/// The last part of a path.
pub fn file_name(path: &str) -> &str {
    path.rfind('/').map_or(path, |i| &path[i + 1..])
}

/// `name` inside `folder`.
pub fn join(folder: &str, name: &str) -> String {
    if folder.is_empty() {
        name.to_string()
    } else {
        format!("{folder}/{name}")
    }
}

/// Whether `path` is `folder` or under it.
pub fn is_under(path: &str, folder: &str) -> bool {
    folder.is_empty()
        || path == folder
        || (path.len() > folder.len()
            && path.starts_with(folder)
            && path.as_bytes()[folder.len()] == b'/')
}

/// The rules for a folder's name. The name on disk is the name typed.
pub fn check_folder_name(name: &str) -> Result<()> {
    let name = name.trim();
    if name.is_empty() {
        bail!("A folder name can't be empty");
    }
    if name == "." || name == ".." {
        bail!("A folder can't be named {name}");
    }
    if skipped(name) {
        bail!("A folder name can't start with . or _");
    }
    if let Some(c) = name.chars().find(|c| {
        matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control()
    }) {
        bail!("A folder name can't contain {c}");
    }
    Ok(())
}

/// Checks a folder path a tool or dialog gave: relative, no `..`, and no
/// part NoteLiner skips. The empty path is the project folder.
pub fn check_folder_path(path: &str) -> Result<()> {
    if path.is_empty() {
        return Ok(());
    }
    if path.starts_with('/') || path.contains('\\') || Path::new(path).is_absolute() {
        bail!("Invalid folder path: {path}");
    }
    for part in path.split('/') {
        check_folder_name(part)?;
        if part != part.trim() {
            bail!("Invalid folder path: {path}");
        }
    }
    Ok(())
}

/// Reads the folders and files under `root`. Folders and files whose names
/// start with `.` or `_` are skipped with everything in them, and a symbolic
/// link to a folder is not followed.
pub fn walk(root: &Path) -> Walk {
    let mut out = Walk::default();
    let mut seen = 0usize;
    let mut stack = vec![String::new()];
    while let Some(folder) = stack.pop() {
        let dir = if folder.is_empty() {
            root.to_path_buf()
        } else {
            root.join(&folder)
        };
        if let Ok(t) = fs::metadata(&dir).and_then(|m| m.modified()) {
            out.times.insert(folder.clone(), t);
        }
        let Ok(read) = fs::read_dir(&dir) else {
            continue;
        };
        let mut names: Vec<(String, fs::DirEntry)> = read
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok().map(|n| (n, e)))
            .filter(|(n, _)| !skipped(n))
            .collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        let mut subfolders = Vec::new();
        for (name, entry) in names {
            seen += 1;
            if seen > MAX_FILES {
                out.truncated = true;
                break;
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = join(&folder, &name);
            if kind.is_dir() {
                out.folders.push(path.clone());
                subfolders.push(path);
            } else if is_project_file(&name) && !(folder.is_empty() && name == INDEX_FILE) {
                // A link to a file is read; a link to a folder is not followed.
                let Ok(meta) = fs::metadata(entry.path()) else {
                    continue;
                };
                if meta.is_file() {
                    out.files.push(WalkFile {
                        path,
                        size: meta.len(),
                    });
                }
            }
        }
        if out.truncated {
            break;
        }
        // Depth-first, in name order.
        stack.extend(subfolders.into_iter().rev());
    }
    out.folders.sort();
    out
}

/// What a reconcile changed in the index.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Changes {
    /// The ids of entries added for files the index didn't list.
    pub added: Vec<String>,
    /// Entries whose file is at a new path: (id, old path, new path).
    pub moved: Vec<(String, String, String)>,
    /// Entries whose file is gone.
    pub removed: Vec<Value>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.moved.is_empty() && self.removed.is_empty()
    }
}

fn s(v: &Value) -> &str {
    v.as_str().unwrap_or("")
}

/// A file's name without its extension.
fn stem(path: &str) -> &str {
    let name = file_name(path);
    let ext = crate::project::extname(name);
    &name[..name.len() - ext.len()]
}

/// A new entry for a file the index doesn't list: a new id, the file's name,
/// and for a note the tags and created date in its frontmatter.
fn entry_for(root: &Path, path: &str) -> Value {
    let (created, modified) = stat_times(&root.join(path));
    let mut entry = json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "name": stem(path),
        "filename": path,
        "tags": [],
        "attachments": [],
        "createdAt": created,
        "modifiedAt": modified,
    });
    if FileKind::of(path) == FileKind::Note {
        if let Ok(raw) = fs::read_to_string(root.join(path)) {
            let data = frontmatter::parse(&raw).data;
            if let Some(tags) = frontmatter::tags_of(&data) {
                entry["tags"] = json!(tags);
            }
            if let Some(created) = frontmatter::get_field(&data, "created")
                .map(crate::yaml::Yaml::to_json)
                .and_then(|v| v.as_str().map(str::to_string))
                .filter(|c| chrono::DateTime::parse_from_rfc3339(c).is_ok())
            {
                entry["createdAt"] = json!(created);
            }
            crate::project::apply_deck_flag(&mut entry, &data);
        }
    }
    entry
}

/// Brings `files` (the index's entries) in line with `walk`.
///
/// - An entry whose file is where it says stays.
/// - A file the index doesn't list, with the same file name as exactly one
///   entry whose file is gone, is that entry, moved.
/// - With `sizes` (the sizes at the last walk, while a project is open), one
///   file gone and one of the same kind and size new is a rename: the entry
///   keeps its id and tags and takes the new file's name.
/// - Any other new file gets an entry, and any other entry whose file is
///   gone is removed. A walk that stopped early removes nothing.
pub fn reconcile(
    files: &mut Vec<Value>,
    walk: &Walk,
    sizes: Option<&HashMap<String, u64>>,
    root: &Path,
) -> Changes {
    let mut changes = Changes::default();
    let present: HashSet<&str> = walk.files.iter().map(|f| f.path.as_str()).collect();
    let listed: HashSet<String> = files
        .iter()
        .map(|f| s(&f["filename"]).to_string())
        .collect();
    // Indexes into `files` of entries whose file is gone.
    let mut gone: Vec<usize> = files
        .iter()
        .enumerate()
        .filter(|(_, f)| !present.contains(s(&f["filename"])))
        .map(|(i, _)| i)
        .collect();
    let mut new: Vec<&WalkFile> = walk
        .files
        .iter()
        .filter(|f| !listed.contains(&f.path))
        .collect();

    // Moved: matched by file name, when the match is one to one.
    let mut matched: Vec<(usize, usize)> = Vec::new();
    for (n, file) in new.iter().enumerate() {
        let name = file_name(&file.path);
        if new.iter().filter(|o| file_name(&o.path) == name).count() != 1 {
            continue;
        }
        let mut candidates = gone
            .iter()
            .filter(|&&g| file_name(s(&files[g]["filename"])) == name);
        if let (Some(&g), None) = (candidates.next(), candidates.next()) {
            matched.push((g, n));
        }
    }
    // Renamed: one gone and one new of the same kind and size.
    if let Some(sizes) = sizes {
        for kind in [FileKind::Note, FileKind::Diagram] {
            let mut g = gone
                .iter()
                .filter(|&&g| !matched.iter().any(|m| m.0 == g))
                .filter(|&&g| FileKind::of_entry(&files[g]) == kind);
            let mut n = (0..new.len())
                .filter(|n| !matched.iter().any(|m| m.1 == *n))
                .filter(|n| FileKind::of(&new[*n].path) == kind);
            if let (Some(&g), None, Some(n), None) = (g.next(), g.next(), n.next(), n.next()) {
                if sizes.get(s(&files[g]["filename"])) == Some(&new[n].size) {
                    files[g]["name"] = json!(stem(&new[n].path));
                    matched.push((g, n));
                }
            }
        }
    }
    for &(g, n) in &matched {
        let old = s(&files[g]["filename"]).to_string();
        files[g]["filename"] = json!(new[n].path);
        changes
            .moved
            .push((s(&files[g]["id"]).to_string(), old, new[n].path.clone()));
    }
    gone.retain(|g| !matched.iter().any(|m| m.0 == *g));
    let taken: HashSet<usize> = matched.iter().map(|m| m.1).collect();
    let mut n = 0;
    new.retain(|_| {
        n += 1;
        !taken.contains(&(n - 1))
    });

    if !walk.truncated {
        for &g in gone.iter().rev() {
            changes.removed.push(files.remove(g));
        }
        changes.removed.reverse();
    }
    for file in new {
        let entry = entry_for(root, &file.path);
        changes.added.push(s(&entry["id"]).to_string());
        files.push(entry);
    }
    changes
}

/// A new, empty index.
pub fn empty_index() -> Value {
    json!({ "version": crate::project::INDEX_VERSION, "files": [] })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(root: &Path, path: &str, text: &str) {
        let p = root.join(path);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, text).unwrap();
    }

    fn paths(w: &Walk) -> Vec<&str> {
        w.files.iter().map(|f| f.path.as_str()).collect()
    }

    #[test]
    fn path_helpers() {
        assert_eq!(folder_of("a/b/c.md"), "a/b");
        assert_eq!(folder_of("c.md"), "");
        assert_eq!(file_name("a/b/c.md"), "c.md");
        assert_eq!(join("", "c.md"), "c.md");
        assert_eq!(join("a", "c.md"), "a/c.md");
        assert!(is_under("a/b/c.md", "a"));
        assert!(is_under("a/b", "a/b"));
        assert!(!is_under("ab/c.md", "a"));
        assert!(is_under("x.md", ""));
        assert_eq!(stem("a/Kick.Off.md"), "Kick.Off");
    }

    #[test]
    fn folder_name_rules() {
        for ok in ["Work", "2026 plans", "a.b", "Été"] {
            assert!(check_folder_name(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "  ",
            ".",
            "..",
            ".git",
            "_attachments",
            "a/b",
            "a\\b",
            "a:b",
            "what?",
            "a*",
        ] {
            assert!(check_folder_name(bad).is_err(), "{bad}");
        }
        assert!(check_folder_path("").is_ok());
        assert!(check_folder_path("work/Meetings").is_ok());
        for bad in ["/abs", "../up", "a/../b", "a//b", "a/.git", "a/_x", "a\\b"] {
            assert!(check_folder_path(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn walk_reads_folders_and_skips_the_rest() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "top.md", "a");
        touch(root, "map.mmd", "flowchart LR\n");
        touch(root, "noteliner.json", "{}");
        touch(root, "report.pdf", "x");
        touch(root, "work/kickoff.md", "abc");
        touch(root, "work/meetings/deep/one.MERMAID", "flowchart LR\n");
        touch(root, "work/_attachments/att-1.png", "x");
        touch(root, "_templates/t.md", "x");
        touch(root, ".git/config.md", "x");
        touch(root, ".hidden.md", "x");
        fs::create_dir_all(root.join("empty")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("work"), root.join("link")).unwrap();

        let w = walk(root);
        assert!(!w.truncated);
        assert_eq!(
            w.folders,
            ["empty", "work", "work/meetings", "work/meetings/deep"]
        );
        let mut files = paths(&w);
        files.sort();
        assert_eq!(
            files,
            [
                "map.mmd",
                "top.md",
                "work/kickoff.md",
                "work/meetings/deep/one.MERMAID"
            ]
        );
        assert_eq!(
            w.files
                .iter()
                .find(|f| f.path == "work/kickoff.md")
                .unwrap()
                .size,
            3
        );
        // Every folder was stamped, the project folder as "".
        for f in ["", "empty", "work", "work/meetings", "work/meetings/deep"] {
            assert!(w.times.contains_key(f), "{f}");
        }
        // Paths use `/` on every system.
        assert!(w.files.iter().all(|f| !f.path.contains('\\')));
    }

    fn entry(id: &str, path: &str) -> Value {
        json!({ "id": id, "name": id.to_uppercase(), "filename": path, "tags": ["t"], "attachments": [] })
    }

    #[test]
    fn reconcile_keeps_adds_and_removes() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "kept.md", "x");
        touch(
            root,
            "work/new-note.md",
            "---\ntags:\n  - a\n  - b\ncreated: '2020-02-03T04:05:06.000Z'\nname: Ignored\npresentation:\n  theme: dark\n---\nbody\n",
        );
        touch(root, "work/new.mmd", "flowchart LR\n");
        let mut files = vec![entry("kept", "kept.md"), entry("lost", "lost.md")];
        let c = reconcile(&mut files, &walk(root), None, root);
        assert_eq!(c.removed.len(), 1);
        assert_eq!(c.removed[0]["id"], "lost");
        assert!(c.moved.is_empty());
        assert_eq!(c.added.len(), 2);
        assert_eq!(files.len(), 3);
        assert_eq!(files[0]["id"], "kept");
        let note = files
            .iter()
            .find(|f| f["filename"] == "work/new-note.md")
            .unwrap();
        // The name is the file's, not the frontmatter's.
        assert_eq!(note["name"], "new-note");
        assert_eq!(note["tags"], json!(["a", "b"]));
        assert_eq!(note["createdAt"], "2020-02-03T04:05:06.000Z");
        assert_eq!(note["deck"], true);
        let diagram = files
            .iter()
            .find(|f| f["filename"] == "work/new.mmd")
            .unwrap();
        assert_eq!(diagram["name"], "new");
        assert_eq!(diagram["tags"], json!([]));
        // A second pass changes nothing.
        let before = files.clone();
        assert!(reconcile(&mut files, &walk(root), None, root).is_empty());
        assert_eq!(files, before);
    }

    #[test]
    fn a_move_is_found_by_its_file_name() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "work/a.md", "x");
        touch(root, "archive/map.mmd", "flowchart LR\n");
        let mut files = vec![entry("a", "a.md"), entry("map", "map.mmd")];
        let c = reconcile(&mut files, &walk(root), None, root);
        assert!(c.added.is_empty() && c.removed.is_empty());
        assert_eq!(
            c.moved,
            [
                (
                    "map".to_string(),
                    "map.mmd".to_string(),
                    "archive/map.mmd".to_string()
                ),
                ("a".to_string(), "a.md".to_string(), "work/a.md".to_string()),
            ]
        );
        // The entries keep their names and tags.
        assert_eq!(files[0]["name"], "A");
        assert_eq!(files[0]["tags"], json!(["t"]));
        assert_eq!(files[0]["filename"], "work/a.md");
    }

    #[test]
    fn two_gone_files_with_one_name_are_not_matched() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "c/index.md", "x");
        let mut files = vec![entry("one", "a/index.md"), entry("two", "b/index.md")];
        let c = reconcile(&mut files, &walk(root), None, root);
        assert!(c.moved.is_empty());
        assert_eq!(c.removed.len(), 2);
        assert_eq!(c.added.len(), 1);

        // Nor are two new files with the name of one that is gone.
        touch(root, "d/index.md", "x");
        fs::remove_dir_all(root.join("c")).unwrap();
        touch(root, "e/index.md", "x");
        let c = reconcile(&mut files, &walk(root), None, root);
        assert!(c.moved.is_empty());
        assert_eq!((c.removed.len(), c.added.len()), (1, 2));
    }

    #[test]
    fn a_rename_is_found_by_kind_and_size_while_open() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "new-name.md", "12345");
        touch(root, "other.mmd", "12345");
        let sizes: HashMap<String, u64> = [("old.md".to_string(), 5)].into();

        // At open there are no sizes: a removed note and a new one.
        let mut files = vec![entry("n", "old.md")];
        let c = reconcile(&mut files, &walk(root), None, root);
        assert_eq!((c.removed.len(), c.added.len(), c.moved.len()), (1, 2, 0));

        // While open: the note is matched, the diagram of the same size is not.
        let mut files = vec![entry("n", "old.md")];
        let c = reconcile(&mut files, &walk(root), Some(&sizes), root);
        assert_eq!(
            c.moved,
            [(
                "n".to_string(),
                "old.md".to_string(),
                "new-name.md".to_string()
            )]
        );
        assert_eq!(c.added.len(), 1);
        assert!(c.removed.is_empty());
        assert_eq!(files[0]["name"], "new-name");
        assert_eq!(files[0]["tags"], json!(["t"]));

        // A different size is a removed file and a new one.
        let sizes: HashMap<String, u64> = [("old.md".to_string(), 9)].into();
        let mut files = vec![entry("n", "old.md")];
        let c = reconcile(&mut files, &walk(root), Some(&sizes), root);
        assert!(c.moved.is_empty());
        assert_eq!(c.removed.len(), 1);
    }

    #[test]
    fn a_walk_that_stopped_removes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let mut files = vec![entry("a", "a.md")];
        let w = Walk {
            truncated: true,
            ..Default::default()
        };
        let c = reconcile(&mut files, &w, None, tmp.path());
        assert!(c.is_empty());
        assert_eq!(files.len(), 1);
    }
}
