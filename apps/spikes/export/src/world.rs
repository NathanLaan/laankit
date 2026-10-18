//! An in-memory Typst [`World`]: one generated main file, read-only access to
//! images under the export root, and fonts from memory only (Typst's embedded
//! set plus any the app bundles). No system fonts, packages or network.

use std::path::PathBuf;
use std::sync::Arc;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};

/// Fonts and the standard library, built once and shared by every export.
pub struct Fonts {
    library: LazyHash<Library>,
    book: LazyHash<FontBook>,
    fonts: Vec<Font>,
}

impl Fonts {
    /// Typst's embedded fonts (Libertinus Serif, New Computer Modern, DejaVu
    /// Sans Mono) plus `extra` font files (e.g. the app's bundled UI sans).
    pub fn new(extra: impl IntoIterator<Item = Vec<u8>>) -> Self {
        let mut fonts: Vec<Font> = typst_assets::fonts()
            .flat_map(|data| Font::iter(Bytes::new(data)))
            .collect();
        for data in extra {
            fonts.extend(Font::iter(Bytes::new(data)));
        }
        let book = FontBook::from_fonts(&fonts);
        Self {
            library: LazyHash::new(Library::default()),
            book: LazyHash::new(book),
            fonts,
        }
    }

    /// Family names available, for logging.
    pub fn families(&self) -> Vec<String> {
        self.book
            .families()
            .map(|(name, _)| name.to_string())
            .collect()
    }
}

pub struct ExportWorld {
    fonts: Arc<Fonts>,
    main: Source,
    /// Images resolve under this folder; `None` means no file access.
    root: Option<PathBuf>,
}

impl ExportWorld {
    pub fn new(fonts: Arc<Fonts>, markup: String, root: Option<PathBuf>) -> Self {
        let vpath = VirtualPath::new("/main.typ").expect("valid path");
        let id = RootedPath::new(VirtualRoot::Project, vpath).intern();
        Self {
            fonts,
            main: Source::new(id, markup),
            root,
        }
    }
}

impl World for ExportWorld {
    fn library(&self) -> &LazyHash<Library> {
        &self.fonts.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        &self.fonts.book
    }

    fn main(&self) -> FileId {
        self.main.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.main.id() {
            Ok(self.main.clone())
        } else {
            Err(FileError::NotSource)
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        if id == self.main.id() {
            return Ok(Bytes::from_string(self.main.text().to_string()));
        }
        let rooted = id.get();
        if !matches!(rooted.root(), VirtualRoot::Project) {
            return Err(FileError::AccessDenied);
        }
        let root = self.root.as_ref().ok_or(FileError::AccessDenied)?;
        let path = rooted.vpath().realize(root).map_err(FileError::Realize)?;
        std::fs::read(&path)
            .map(Bytes::new)
            .map_err(|_| FileError::NotFound(path))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}
