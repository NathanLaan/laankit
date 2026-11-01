//! NoteLiner's PDF export without a webview: frontmatter stripped →
//! Markdown (comrak AST) → Typst markup ([`markup`]) → `typst::compile` in an
//! in-memory world → `typst_pdf::pdf`.
//!
//! ```ignore
//! // Once, at startup: built and warmed up on a background thread.
//! let exporter = LazyExporter::spawn(faces);
//! // Per export (blocks only if the exporter isn't ready yet):
//! let pdf = exporter.get().export(&raw_note, &ExportOptions {
//!     title: Title::Text(note_name),
//!     base_dir: Some(project_dir),
//!     ..Default::default()
//! })?;
//! ```
//!
//! `faces` are the font files the app bundles
//! (`laankit_egui::fonts::bundled_faces()`), so PDFs use the same Inter and
//! JetBrains Mono as the app. Characters they lack (CJK, emoji, …) fall back
//! to the system's fonts, which are indexed only when a note needs them.
//!
//! HTML export stays on comrak in `noteliner-core` ([`export_html`]).
//!
//! Math: `$…$` and `$$…$$` (and ```` ```math ```` blocks) are converted from
//! LaTeX syntax by [`math`] and typeset by Typst in New Computer Modern Math,
//! which this crate bundles. [`Exporter::math_svg`] renders one formula to
//! SVG for the app's preview and slides.
//!
//! Typst is behind the default `pdf` feature. Without it the crate still
//! builds, [`markup`] and [`math`] still work, and exporting returns
//! [`Error::PdfUnavailable`].

pub mod markup;
pub mod math;
#[cfg(feature = "pdf")]
mod world;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::JoinHandle;

use noteliner_core::frontmatter;
use noteliner_core::yaml::Yaml;

pub use markup::{FontFamilies, PageSetup, Paper, WikiLinks};

/// The note's title heading (and the PDF's title metadata).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Title {
    /// No title heading; Markdown headings keep their levels.
    #[default]
    None,
    /// This text, e.g. the note's name (what today's export shows).
    Text(String),
    /// The frontmatter's `title`, else its `name` (NoteLiner's mirror field),
    /// else `fallback`.
    Frontmatter { fallback: Option<String> },
}

/// How to export one note.
#[derive(Clone, Debug, Default)]
pub struct ExportOptions {
    pub title: Title,
    /// Where relative image paths resolve: the project folder, whose
    /// `_attachments/` holds pasted images. `None`: images become placeholders.
    pub base_dir: Option<PathBuf>,
    pub page: PageSetup,
    pub fonts: FontFamilies,
    pub wikilinks: WikiLinks,
    /// Font files for [`export_pdf`], which builds a one-off [`Exporter`].
    /// An [`Exporter`] gets its fonts when it's built and ignores this.
    pub faces: Vec<&'static [u8]>,
}

#[derive(Debug)]
pub enum Error {
    /// Built without the `pdf` feature.
    PdfUnavailable,
    /// Typst rejected the generated document (a bug in [`markup`]) or
    /// couldn't lay it out.
    Compile(String),
    /// Writing the PDF failed.
    Pdf(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::PdfUnavailable => write!(f, "PDF export isn't included in this build"),
            Error::Compile(e) => write!(f, "couldn't lay out the note: {e}"),
            Error::Pdf(e) => write!(f, "couldn't write the PDF: {e}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Exports one note with a fresh [`Exporter`] (fonts from `options.faces`).
/// The first export in a process takes about 200 ms; keep an [`Exporter`]
/// (or a [`LazyExporter`]) for repeated exports.
pub fn export_pdf(raw_markdown: &str, options: &ExportOptions) -> Result<Vec<u8>> {
    Exporter::new(options.faces.iter().copied()).export(raw_markdown, options)
}

/// The standalone HTML export (`file:convertToHtml`), unchanged:
/// `noteliner_core::export::html_document` on the body without frontmatter.
pub fn export_html(name: &str, raw_markdown: &str) -> String {
    noteliner_core::export::html_document(name, &frontmatter::strip_body(raw_markdown))
}

/// The Typst source an export compiles, for debugging or saving a `.typ` file.
pub fn typst_source(raw_markdown: &str, options: &ExportOptions) -> String {
    let parsed = frontmatter::parse(raw_markdown);
    let title = resolve_title(&options.title, &parsed.data);
    let settings = markup::DocumentSettings {
        title: title.as_deref(),
        page: options.page.clone(),
        fonts: options.fonts.clone(),
        wikilinks: options.wikilinks.clone(),
    };
    match &options.base_dir {
        Some(root) => markup::document(&parsed.body, &settings, &markup::DirImages { root }),
        None => markup::document(&parsed.body, &settings, &markup::NoImages),
    }
}

fn resolve_title(title: &Title, data: &frontmatter::Data) -> Option<String> {
    match title {
        Title::None => None,
        Title::Text(t) => Some(t.clone()),
        Title::Frontmatter { fallback } => ["title", "name"]
            .iter()
            .find_map(|key| match frontmatter::get_field(data, key) {
                Some(Yaml::Str(s)) if !s.trim().is_empty() => Some(s.clone()),
                _ => None,
            })
            .or_else(|| fallback.clone()),
    }
}

/// New Computer Modern Math (GUST Font License; see `assets/fonts/NewCMMath-LICENSE.txt`),
/// always added to the faces an [`Exporter`] is given: Typst sets math in it.
#[cfg(feature = "pdf")]
const MATH_FONT: &[u8] = include_bytes!("../assets/fonts/NewCMMath-Regular.otf");

/// Exports notes to PDF. Keep one per app: the fonts and Typst's caches are
/// reused, so later exports take milliseconds rather than ~200 ms.
pub struct Exporter {
    #[cfg(feature = "pdf")]
    faces: Vec<&'static [u8]>,
    #[cfg(feature = "pdf")]
    system_fallback: bool,
    /// The bundled faces only.
    #[cfg(feature = "pdf")]
    bundled: Arc<world::FontSet>,
    /// The bundled faces plus system fonts, built the first time a note has
    /// characters the bundled faces lack.
    #[cfg(feature = "pdf")]
    with_system: OnceLock<Arc<world::FontSet>>,
}

impl Exporter {
    /// An exporter with these font files (e.g.
    /// `laankit_egui::fonts::bundled_faces()`), falling back to system fonts.
    pub fn new(faces: impl IntoIterator<Item = &'static [u8]>) -> Self {
        #[cfg_attr(not(feature = "pdf"), allow(unused_mut))]
        let mut faces: Vec<_> = faces.into_iter().collect();
        #[cfg(feature = "pdf")]
        {
            if !faces.contains(&MATH_FONT) {
                faces.push(MATH_FONT);
            }
            Self {
                bundled: Arc::new(world::FontSet::new(&faces, false)),
                faces,
                system_fallback: true,
                with_system: OnceLock::new(),
            }
        }
        #[cfg(not(feature = "pdf"))]
        {
            let _ = faces;
            Self {}
        }
    }

    /// Whether glyphs the bundled faces lack may come from system fonts (on
    /// by default). Off: they render as tofu, but exports never scan the disk.
    #[cfg_attr(not(feature = "pdf"), allow(unused_mut))]
    pub fn with_system_fallback(mut self, on: bool) -> Self {
        #[cfg(feature = "pdf")]
        {
            self.system_fallback = on;
        }
        #[cfg(not(feature = "pdf"))]
        let _ = on;
        self
    }

    /// Exports a note (its raw text, frontmatter and all) as PDF bytes.
    pub fn export(&self, raw_markdown: &str, options: &ExportOptions) -> Result<Vec<u8>> {
        #[cfg(feature = "pdf")]
        {
            let source = typst_source(raw_markdown, options);
            self.compile(source, options.base_dir.clone())
        }
        #[cfg(not(feature = "pdf"))]
        {
            let _ = (raw_markdown, options);
            Err(Error::PdfUnavailable)
        }
    }

    /// Compiles a tiny document so Typst's one-time setup (the standard
    /// library, syntax highlighting, font parsing) is paid before the first
    /// real export.
    pub fn warm_up(&self) {
        let options = ExportOptions {
            title: Title::Text("Warm-up".into()),
            ..Default::default()
        };
        let note = "**Bold** _italic_ `code`\n\n```rust\nfn main() {}\n```\n\n| a | b |\n|---|---|\n| 1 | 2 |\n";
        if let Err(e) = self.export(note, &options) {
            log::warn!("PDF export warm-up failed: {e}");
        }
    }

    /// Renders one formula (LaTeX syntax, without `$`) to SVG: white glyphs on
    /// a transparent page that fits the formula, `size` points tall text, so
    /// a viewer can tint it to any text colour. `display` sets it as a block
    /// (larger operators, limits above and below). Unsupported LaTeX is
    /// [`Error::Compile`]; callers show the source instead.
    pub fn math_svg(&self, latex: &str, display: bool, size: f32) -> Result<String> {
        let equation =
            math::typst_equation(latex, display).map_err(|e| Error::Compile(e.to_string()))?;
        #[cfg(feature = "pdf")]
        {
            use typst_layout::PagedDocument;

            let size = if size.is_finite() {
                size.clamp(4.0, 200.0)
            } else {
                14.0
            };
            let source = format!(
                "#set page(width: auto, height: auto, margin: (x: 1pt, y: 3pt), fill: none)\n\
                 #set text(size: {size:.2}pt, fill: white, top-edge: \"bounds\", bottom-edge: \"bounds\")\n\
                 {equation}\n"
            );
            let fonts = self.fonts_for(&source);
            let world = world::ExportWorld::new(fonts, source, None);
            let doc: PagedDocument = typst::compile(&world)
                .output
                .map_err(|errs| Error::Compile(join_messages(errs.iter().map(|e| &e.message))))?;
            let page = doc
                .pages()
                .first()
                .ok_or_else(|| Error::Compile("empty formula".into()))?;
            Ok(math::straight_strokes_as_rects(&typst_svg::svg(
                page,
                &typst_svg::SvgOptions::default(),
            )))
        }
        #[cfg(not(feature = "pdf"))]
        {
            let _ = (equation, size);
            Err(Error::PdfUnavailable)
        }
    }

    #[cfg(feature = "pdf")]
    fn compile(&self, source: String, root: Option<PathBuf>) -> Result<Vec<u8>> {
        use typst_layout::PagedDocument;

        let fonts = self.fonts_for(&source);
        let world = world::ExportWorld::new(fonts, source, root);
        let doc: PagedDocument = typst::compile(&world)
            .output
            .map_err(|errs| Error::Compile(join_messages(errs.iter().map(|e| &e.message))))?;
        let pdf = typst_pdf::pdf(&doc, &typst_pdf::PdfOptions::default())
            .map_err(|errs| Error::Pdf(join_messages(errs.iter().map(|e| &e.message))))?;
        // Bound Typst's memoization cache between exports (as the Typst CLI's watcher does).
        typst::comemo::evict(10);
        Ok(pdf)
    }

    /// The bundled faces, or bundled plus system fonts when `source` has
    /// characters the bundled faces lack (or there are no bundled faces).
    #[cfg(feature = "pdf")]
    fn fonts_for(&self, source: &str) -> Arc<world::FontSet> {
        if !self.system_fallback {
            return self.bundled.clone();
        }
        let needs_system = self.bundled.is_empty() || !self.bundled.missing(source).is_empty();
        if !needs_system {
            return self.bundled.clone();
        }
        self.with_system
            .get_or_init(|| Arc::new(world::FontSet::new(&self.faces, true)))
            .clone()
    }

    /// The font families available without system fonts (for tests and logs).
    pub fn bundled_families(&self) -> Vec<String> {
        #[cfg(feature = "pdf")]
        {
            self.bundled.families()
        }
        #[cfg(not(feature = "pdf"))]
        Vec::new()
    }
}

#[cfg(feature = "pdf")]
fn join_messages<'a>(messages: impl Iterator<Item = &'a typst::ecow::EcoString>) -> String {
    messages
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join("; ")
}

/// An [`Exporter`] built and warmed up on a background thread, so the
/// ~200 ms first-use cost never lands on the UI thread. Create it at startup;
/// [`LazyExporter::get`] only blocks if an export is requested before it's
/// ready.
pub struct LazyExporter {
    cell: Arc<OnceLock<Exporter>>,
    builder: Mutex<Option<JoinHandle<()>>>,
    faces: Vec<&'static [u8]>,
}

impl LazyExporter {
    pub fn spawn(faces: impl IntoIterator<Item = &'static [u8]>) -> Self {
        let faces: Vec<_> = faces.into_iter().collect();
        let cell = Arc::new(OnceLock::new());
        let (target, thread_faces) = (cell.clone(), faces.clone());
        let builder = std::thread::Builder::new()
            .name("pdf-export-warmup".into())
            .spawn(move || {
                let exporter = Exporter::new(thread_faces);
                exporter.warm_up();
                let _ = target.set(exporter);
            })
            .map_err(|e| log::warn!("couldn't start the PDF export thread: {e}"))
            .ok();
        Self {
            cell,
            builder: Mutex::new(builder),
            faces,
        }
    }

    /// The exporter if it's ready, without blocking.
    pub fn try_get(&self) -> Option<&Exporter> {
        self.cell.get()
    }

    /// The exporter, waiting for the background build if it's still running
    /// (or building it here if that thread failed).
    pub fn get(&self) -> &Exporter {
        if let Some(e) = self.cell.get() {
            return e;
        }
        let handle = self.builder.lock().ok().and_then(|mut b| b.take());
        if let Some(handle) = handle {
            let _ = handle.join();
        }
        self.cell
            .get_or_init(|| Exporter::new(self.faces.iter().copied()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exporter_is_send_and_sync() {
        fn check<T: Send + Sync>() {}
        check::<Exporter>();
        check::<LazyExporter>();
    }

    #[test]
    fn titles_from_frontmatter() {
        let opts = |title| ExportOptions {
            title,
            ..Default::default()
        };
        let note = "---\nname: Mirror Name\ntitle: Real Title\n---\n# Body\n";
        let src = typst_source(note, &opts(Title::Frontmatter { fallback: None }));
        assert!(src.contains("#heading(level: 1)[Real Title]"), "{src}");
        assert!(src.contains("#heading(level: 2)[Body]"));
        assert!(!src.contains("Mirror Name"), "frontmatter leaked: {src}");

        let note = "---\nname: Mirror Name\n---\nText\n";
        let src = typst_source(note, &opts(Title::Frontmatter { fallback: None }));
        assert!(src.contains("#heading(level: 1)[Mirror Name]"));

        let src = typst_source(
            "Text",
            &opts(Title::Frontmatter {
                fallback: Some("File".into()),
            }),
        );
        assert!(src.contains("#heading(level: 1)[File]"));

        // No title: headings keep their levels, and nothing about the title is set.
        let src = typst_source("# H\n", &opts(Title::None));
        assert!(src.contains("#heading(level: 1)[H]") && !src.contains("set document"));
    }

    #[test]
    fn page_setup_and_fonts_are_configurable() {
        let src = typst_source(
            "x",
            &ExportOptions {
                page: PageSetup {
                    paper: Paper::A4,
                    margin_in: 1.0,
                },
                ..Default::default()
            },
        );
        assert!(
            src.contains("paper: \"a4\", margin: (y: 1in, x: 1in + 18pt)"),
            "{src}"
        );
        assert!(src.contains("font: (\"Inter\""), "{src}");
        assert!(src.contains("font: (\"JetBrains Mono NL\""), "{src}");
    }

    #[cfg(not(feature = "pdf"))]
    #[test]
    fn without_the_pdf_feature_export_says_so() {
        let err = export_pdf("x", &ExportOptions::default()).unwrap_err();
        assert!(matches!(err, Error::PdfUnavailable), "{err}");
    }

    #[test]
    fn html_export_strips_frontmatter() {
        let html = export_html("Note", "---\nid: x\n---\n# Hi\n");
        assert!(
            html.contains("<h1>Hi</h1>") && !html.contains("id: x"),
            "{html}"
        );
    }
}
