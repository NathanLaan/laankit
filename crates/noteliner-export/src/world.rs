//! The Typst [`World`] an export compiles in: one generated main file,
//! read-only access to images under the export root, and fonts from memory
//! (the faces the app bundles) plus, when a note needs them, the system's
//! fonts. No packages and no network.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime};
use typst::syntax::{FileId, RootedPath, Source, VirtualPath, VirtualRoot};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_kit::fonts::FontStore;

/// Fonts and the standard library, built once and shared by every export.
pub(crate) struct FontSet {
    library: LazyHash<Library>,
    store: FontStore,
}

impl FontSet {
    /// The given font files (each may hold several faces), and with `system`
    /// also every font the operating system has. System fonts are only
    /// indexed here; a file is read the first time Typst needs a glyph from it.
    pub fn new(faces: &[&'static [u8]], system: bool) -> Self {
        let mut store = FontStore::new();
        for data in faces {
            for font in Font::iter(Bytes::new(*data)) {
                let info = font.info().clone();
                store.push((font, info));
            }
        }
        if system {
            let start = std::time::Instant::now();
            store.extend(typst_kit::fonts::system());
            log::info!(
                "indexed system fonts for PDF export in {:.0} ms",
                start.elapsed().as_secs_f64() * 1000.0
            );
        }
        Self {
            library: LazyHash::new(Library::default()),
            store,
        }
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.store.book()
    }

    pub fn is_empty(&self) -> bool {
        self.book().info(0).is_none()
    }

    /// The characters of `text` that no font in this set has (whitespace and
    /// control characters aside).
    pub fn missing(&self, text: &str) -> BTreeSet<char> {
        let chars: BTreeSet<char> = text
            .chars()
            .filter(|c| !c.is_whitespace() && !c.is_control())
            .collect();
        let book = self.book();
        chars
            .into_iter()
            .filter(|&c| {
                !(0..)
                    .map_while(|i| book.info(i))
                    .any(|info| info.coverage.contains(c as u32))
            })
            .collect()
    }

    /// Family names, for logging and tests.
    pub fn families(&self) -> Vec<String> {
        self.book()
            .families()
            .map(|(name, _)| name.to_string())
            .collect()
    }
}

pub(crate) struct ExportWorld {
    fonts: Arc<FontSet>,
    main: Source,
    /// Images resolve under this folder; `None` means no file access.
    root: Option<PathBuf>,
}

impl ExportWorld {
    pub fn new(fonts: Arc<FontSet>, markup: String, root: Option<PathBuf>) -> Self {
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
        self.fonts.book()
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
        self.fonts.store.font(index)
    }

    fn today(&self, _offset: Option<typst::foundations::Duration>) -> Option<Datetime> {
        None
    }
}
