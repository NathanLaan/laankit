//! Reading folders: a folder's subfolders for the tree, and its files for
//! the Files pane and the grid. Hidden entries (names starting with `.`,
//! and on Windows those with the hidden attribute) are left out, and
//! symbolic links to folders aren't followed, so a scan can't loop.

use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::media::MediaKind;
use crate::natural;

/// A file in a folder.
#[derive(Clone, Debug, PartialEq)]
pub struct FileEntry {
    pub path: PathBuf,
    pub name: String,
    pub kind: MediaKind,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

impl FileEntry {
    fn new(path: PathBuf, meta: &Metadata) -> Self {
        Self {
            name: file_name(&path),
            kind: MediaKind::of(&path),
            size: meta.len(),
            modified: meta.modified().ok(),
            path,
        }
    }
}

/// The last part of `path`, or the whole path for a drive root.
pub fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Whether a directory entry is hidden.
pub fn is_hidden(path: &Path, meta: &Metadata) -> bool {
    if path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.starts_with('.'))
    {
        return true;
    }
    hidden_attribute(meta)
}

#[cfg(windows)]
fn hidden_attribute(meta: &Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
    meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0
}

#[cfg(not(windows))]
fn hidden_attribute(_meta: &Metadata) -> bool {
    false
}

/// `dir`'s entries that aren't hidden, with their metadata (not following
/// symbolic links).
fn entries(dir: &Path) -> io::Result<Vec<(PathBuf, Metadata)>> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !is_hidden(&path, &meta) {
            out.push((path, meta));
        }
    }
    Ok(out)
}

fn sort_paths(v: &mut [PathBuf]) {
    v.sort_by(|a, b| natural::compare(&file_name(a), &file_name(b)));
}

/// `dir`'s subfolders, sorted naturally.
pub fn subfolders(dir: &Path) -> io::Result<Vec<PathBuf>> {
    let mut out: Vec<PathBuf> = entries(dir)?
        .into_iter()
        .filter(|(_, m)| m.is_dir())
        .map(|(p, _)| p)
        .collect();
    sort_paths(&mut out);
    Ok(out)
}

/// Whether `dir` has a subfolder that isn't hidden. Stops at the first.
pub fn has_subfolders(dir: &Path) -> bool {
    let Ok(rd) = fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| {
        e.file_type().is_ok_and(|t| t.is_dir())
            && fs::symlink_metadata(e.path()).is_ok_and(|m| !is_hidden(&e.path(), &m))
    })
}

/// The files in `dir` itself, sorted naturally by name.
pub fn files(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut out: Vec<FileEntry> = entries(dir)?
        .into_iter()
        .filter(|(_, m)| m.is_file())
        .map(|(p, m)| FileEntry::new(p, &m))
        .collect();
    out.sort_by(|a, b| natural::compare(&a.name, &b.name));
    Ok(out)
}

/// The files in `dir` and every folder below it: `dir`'s own first, then
/// each subfolder's in natural order, depth first.
pub fn files_deep(dir: &Path) -> io::Result<Vec<FileEntry>> {
    let mut out = files(dir)?;
    for sub in subfolders(dir)? {
        // A subfolder that can't be read is left out, not an error.
        if let Ok(more) = files_deep(&sub) {
            out.extend(more);
        }
    }
    Ok(out)
}

/// A folder's listing, as `(name, size, modified)`: compared to see
/// whether it changed since the last look.
pub fn fingerprint(files: &[FileEntry]) -> Vec<(PathBuf, u64, Option<SystemTime>)> {
    files
        .iter()
        .map(|f| (f.path.clone(), f.size, f.modified))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"x").unwrap();
    }

    #[test]
    fn lists_folders_and_files_without_hidden_ones() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        for f in ["Day 10/a.jpg", "Day 2/b.png", ".cache/c.jpg", "img10.jpg", "img9.jpg", ".hidden.jpg", "notes.txt"] {
            touch(&d.join(f));
        }
        let names = |v: Vec<PathBuf>| v.iter().map(|p| file_name(p)).collect::<Vec<_>>();
        assert_eq!(names(subfolders(d).unwrap()), ["Day 2", "Day 10"]);
        let f = files(d).unwrap();
        let n: Vec<&str> = f.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(n, ["img9.jpg", "img10.jpg", "notes.txt"]);
        assert_eq!(f[2].kind, MediaKind::Other);
        assert!(has_subfolders(d));
        assert!(!has_subfolders(&d.join("Day 2")));
        let deep: Vec<String> = files_deep(d).unwrap().into_iter().map(|f| f.name).collect();
        assert_eq!(deep, ["img9.jpg", "img10.jpg", "notes.txt", "b.png", "a.jpg"]);
    }
}
