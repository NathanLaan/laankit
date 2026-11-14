//! The open files: the Files pane's list, the tabs above the canvas, and the
//! active tab. Pure bookkeeping over paths; the app loads, saves, and
//! watches the files themselves.
//!
//! Every tab is a file in the list. Closing a tab keeps the file in the
//! list; closing the file closes its tab too.

use std::path::{Path, PathBuf};

/// A row of the Files pane.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenFile {
    pub path: PathBuf,
    /// Deleted or moved outside BlockLiner since it was opened.
    pub missing: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Files {
    /// In the order they were opened.
    pub list: Vec<OpenFile>,
    /// The files with tabs, in tab order.
    pub tabs: Vec<PathBuf>,
    /// The tab on the canvas.
    pub active: Option<PathBuf>,
}

impl Files {
    pub fn contains(&self, path: &Path) -> bool {
        self.list.iter().any(|f| f.path == path)
    }

    pub fn has_tab(&self, path: &Path) -> bool {
        self.tabs.iter().any(|t| t == path)
    }

    /// Adds `path` to the list (once) and gives it a tab (once), at the end.
    pub fn open(&mut self, path: &Path) {
        match self.list.iter_mut().find(|f| f.path == path) {
            Some(f) => f.missing = false,
            None => self.list.push(OpenFile {
                path: path.to_owned(),
                missing: false,
            }),
        }
        if !self.has_tab(path) {
            self.tabs.push(path.to_owned());
        }
    }

    /// Closes `path`'s tab. When it was the active one, the tab to its
    /// right (else left) becomes active and is returned.
    pub fn close_tab(&mut self, path: &Path) -> Option<PathBuf> {
        let i = self.tabs.iter().position(|t| t == path)?;
        self.tabs.remove(i);
        if self.active.as_deref() != Some(path) {
            return None;
        }
        self.active = self
            .tabs
            .get(i)
            .or_else(|| i.checked_sub(1).and_then(|j| self.tabs.get(j)))
            .cloned();
        self.active.clone()
    }

    /// Removes `path` from the list and closes its tab; returns the tab to
    /// activate, as [`Files::close_tab`].
    pub fn close(&mut self, path: &Path) -> Option<PathBuf> {
        self.list.retain(|f| f.path != path);
        self.close_tab(path)
    }

    /// `from` was renamed to `to`.
    pub fn rename(&mut self, from: &Path, to: &Path) {
        for f in &mut self.list {
            if f.path == from {
                f.path = to.to_owned();
            }
        }
        for t in &mut self.tabs {
            if t == from {
                *t = to.to_owned();
            }
        }
        if self.active.as_deref() == Some(from) {
            self.active = Some(to.to_owned());
        }
    }

    /// The tab `step` places after (or, negative, before) the active one,
    /// wrapping round.
    pub fn step(&self, step: isize) -> Option<PathBuf> {
        let n = self.tabs.len() as isize;
        if n < 2 {
            return None;
        }
        let i = self
            .tabs
            .iter()
            .position(|t| Some(t) == self.active.as_ref())? as isize;
        Some(self.tabs[(i + step).rem_euclid(n) as usize].clone())
    }

    /// The files in `folder` itself (not in folders below it).
    pub fn in_folder(&self, folder: &Path) -> Vec<PathBuf> {
        self.list
            .iter()
            .filter(|f| f.path.parent() == Some(folder))
            .map(|f| f.path.clone())
            .collect()
    }
}

/// A diagram's name: its file name without `.mmd`.
pub fn name(path: &Path) -> String {
    path.file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folder hint shown after a name, muted: for a file outside the
/// project, or one whose name another open file shares. The home folder
/// is shortened to `~`.
pub fn hint(path: &Path, project: Option<&Path>, files: &Files) -> Option<String> {
    let parent = path.parent()?;
    let shared = files
        .list
        .iter()
        .any(|f| f.path != path && name(&f.path).eq_ignore_ascii_case(&name(path)));
    if project == Some(parent) && !shared {
        return None;
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    Some(
        match home.as_deref().and_then(|h| parent.strip_prefix(h).ok()) {
            Some(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
            Some(rest) => format!("~/{}", rest.display()),
            None => parent.display().to_string(),
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(s: &str) -> PathBuf {
        PathBuf::from(format!("/d/{s}.mmd"))
    }

    fn three() -> Files {
        let mut f = Files::default();
        for s in ["a", "b", "c"] {
            f.open(&p(s));
        }
        f
    }

    #[test]
    fn opening_twice_adds_once() {
        let mut f = three();
        f.open(&p("b"));
        assert_eq!(f.list.len(), 3);
        assert_eq!(f.tabs, [p("a"), p("b"), p("c")]);
    }

    #[test]
    fn closing_a_tab_keeps_the_file_and_picks_a_neighbour() {
        let mut f = three();
        f.active = Some(p("b"));
        assert_eq!(f.close_tab(&p("b")), Some(p("c")));
        assert!(f.contains(&p("b")));
        assert_eq!(f.active, Some(p("c")));
        // The last tab: its left neighbour.
        assert_eq!(f.close_tab(&p("c")), Some(p("a")));
        // Not the active one: nothing to activate.
        f.open(&p("b"));
        assert_eq!(f.close_tab(&p("b")), None);
        assert_eq!(f.active, Some(p("a")));
        assert_eq!(f.close_tab(&p("a")), None);
        assert_eq!(f.active, None);
    }

    #[test]
    fn closing_a_file_closes_its_tab() {
        let mut f = three();
        f.active = Some(p("a"));
        assert_eq!(f.close(&p("a")), Some(p("b")));
        assert!(!f.contains(&p("a")));
        assert_eq!(f.tabs, [p("b"), p("c")]);
    }

    #[test]
    fn rename_and_step() {
        let mut f = three();
        f.active = Some(p("c"));
        assert_eq!(f.step(1), Some(p("a")));
        assert_eq!(f.step(-1), Some(p("b")));
        f.rename(&p("c"), &p("z"));
        assert_eq!(f.active, Some(p("z")));
        assert_eq!(f.tabs[2], p("z"));
        assert_eq!(f.list[2].path, p("z"));
    }

    #[test]
    fn hints() {
        let mut f = three();
        let other = PathBuf::from("/e/x.mmd");
        f.open(&other);
        let project = Some(Path::new("/d"));
        assert_eq!(hint(&p("a"), project, &f), None);
        assert_eq!(hint(&other, project, &f).as_deref(), Some("/e"));
        // A name shared with another open file shows its folder.
        let twin = PathBuf::from("/e/a.mmd");
        f.open(&twin);
        assert_eq!(hint(&p("a"), project, &f).as_deref(), Some("/d"));
        assert_eq!(f.in_folder(Path::new("/d")).len(), 3);
    }
}
