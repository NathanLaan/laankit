//! The folders you added. Each is the root of a tree in the sidebar.
//!
//! A folder is listed once. Adding one inside a listed folder doesn't list
//! it again (the app selects it in that tree instead), and adding the
//! parent of listed folders replaces them, since its tree includes them.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    /// In the order they were added.
    pub roots: Vec<PathBuf>,
}

/// What [`Library::add`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum Added {
    Added,
    /// Already listed.
    Already,
    /// Inside this listed folder.
    Inside(PathBuf),
    /// Added in place of these listed folders, which it holds.
    Replaced(Vec<PathBuf>),
}

impl Library {
    pub fn new(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }

    pub fn add(&mut self, folder: &Path) -> Added {
        if self.roots.iter().any(|r| r == folder) {
            return Added::Already;
        }
        if let Some(root) = self.root_of(folder) {
            return Added::Inside(root.to_owned());
        }
        let inside: Vec<PathBuf> = self
            .roots
            .iter()
            .filter(|r| r.starts_with(folder))
            .cloned()
            .collect();
        match inside.first() {
            None => {
                self.roots.push(folder.to_owned());
                Added::Added
            }
            Some(first) => {
                let at = self.roots.iter().position(|r| r == first).unwrap_or(0);
                self.roots.retain(|r| !r.starts_with(folder));
                self.roots
                    .insert(at.min(self.roots.len()), folder.to_owned());
                Added::Replaced(inside)
            }
        }
    }

    pub fn remove(&mut self, folder: &Path) {
        self.roots.retain(|r| r != folder);
    }

    /// The listed folder `path` is in (or is).
    pub fn root_of(&self, path: &Path) -> Option<&Path> {
        self.roots
            .iter()
            .filter(|r| path.starts_with(r))
            .max_by_key(|r| r.components().count())
            .map(PathBuf::as_path)
    }

    pub fn is_root(&self, path: &Path) -> bool {
        self.roots.iter().any(|r| r == path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn adding_inside_around_and_twice() {
        let mut l = Library::default();
        assert_eq!(l.add(&p("/a/photos")), Added::Added);
        assert_eq!(l.add(&p("/b")), Added::Added);
        assert_eq!(l.add(&p("/a/photos")), Added::Already);
        assert_eq!(l.add(&p("/a/photos/2025")), Added::Inside(p("/a/photos")));
        assert_eq!(l.add(&p("/a")), Added::Replaced(vec![p("/a/photos")]));
        assert_eq!(l.roots, [p("/a"), p("/b")]);
        // "/ab" isn't inside "/a".
        assert_eq!(l.add(&p("/ab")), Added::Added);
        assert_eq!(l.root_of(&p("/a/photos/x.jpg")), Some(Path::new("/a")));
        l.remove(&p("/a"));
        assert_eq!(l.roots, [p("/b"), p("/ab")]);
    }
}
