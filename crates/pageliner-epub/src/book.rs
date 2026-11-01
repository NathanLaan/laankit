//! An open EPUB: metadata, spine, table of contents, and the whole book converted to one rich-text
//! [`Document`], so it scrolls continuously across chapters.
//!
//! The `epub` crate reads the container, OPF and NCX. Archive paths are kept `/`-separated and
//! percent-decoded on every platform (`OEBPS/text/ch01.xhtml`), which is also what the converter
//! resolves links and images to, so links, anchors and resources all use one spelling.

use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Context, Result};
use epub::doc::{EpubDoc, NavPoint};
use laankit_richtext::convert::{self, Base, Options};
use laankit_richtext::css::StyleSheet;
use laankit_richtext::{Document, Position};
use scraper::{ElementRef, Html, Selector};

use crate::state::Location;

pub struct Book {
    doc: EpubDoc<Cursor<Vec<u8>>>,
    pub title: String,
    pub author: Option<String>,
    /// The package's unique identifier, else the title.
    pub id: String,
    /// Archive paths of the spine documents, in reading order.
    pub spine: Vec<String>,
    pub toc: Vec<TocEntry>,
    /// Every spine document, one after another. Anchors are keyed `path#id`, and `path` for a
    /// chapter's start.
    pub document: Arc<Document>,
    /// The first block of each spine document in [`Self::document`].
    starts: Vec<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    pub label: String,
    /// `path` or `path#fragment`.
    pub target: String,
    pub depth: usize,
}

impl Book {
    /// Opens the EPUB at `path`.
    pub fn open(path: &Path) -> Result<Self> {
        let bytes =
            std::fs::read(path).with_context(|| format!("Couldn't read {}", path.display()))?;
        Self::from_bytes(bytes)
    }

    /// Opens an EPUB held in memory.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        let mut doc = EpubDoc::from_reader(Cursor::new(bytes))
            .map_err(|e| anyhow!("Not a readable EPUB: {e}"))?;
        let title = doc.get_title().unwrap_or_else(|| "Untitled".into());
        let author = doc
            .mdata("creator")
            .map(|m| m.value.trim().to_string())
            .filter(|a| !a.is_empty());
        let id = doc
            .unique_identifier
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| title.clone());
        let spine: Vec<String> = doc
            .spine
            .iter()
            .filter_map(|item| doc.resources.get(&item.idref))
            .map(|r| archive_path(&r.path))
            .collect();
        if spine.is_empty() {
            return Err(anyhow!("The book has no spine"));
        }
        let mut stylesheet = StyleSheet::default();
        let css: Vec<String> = doc
            .resources
            .values()
            .filter(|r| r.mime == "text/css")
            .map(|r| archive_path(&r.path))
            .collect();
        for path in css {
            if let Some(src) = doc.get_resource_str_by_path(&path) {
                stylesheet.append(StyleSheet::parse(&src));
            }
        }
        let mut toc = Vec::new();
        flatten_toc(&doc.toc, 0, &mut toc);
        if toc.is_empty() {
            toc = nav_document_toc(&mut doc);
        }

        let mut document = Document::default();
        let mut starts = Vec::with_capacity(spine.len());
        for path in &spine {
            starts.push(document.blocks.len());
            let Some(xhtml) = doc.get_resource_str_by_path(path) else {
                log::warn!("Missing spine document {path}");
                continue;
            };
            let opts = Options {
                base: Base::Path(path.clone()),
                stylesheet: Some(&stylesheet),
            };
            let chapter = convert::from_xhtml(&xhtml, &opts);
            let offset = document.blocks.len();
            document.anchors.entry(path.clone()).or_insert(offset);
            for (id, block) in chapter.anchors {
                document
                    .anchors
                    .insert(format!("{path}#{id}"), offset + block);
            }
            document.blocks.extend(chapter.blocks);
        }
        // A chapter with no blocks starts where the next one does; clamp anchors past the end.
        let last = document.blocks.len().saturating_sub(1);
        document
            .anchors
            .values_mut()
            .for_each(|b| *b = (*b).min(last));

        if toc.is_empty() {
            toc = spine
                .iter()
                .enumerate()
                .map(|(i, p)| TocEntry {
                    label: format!("Section {}", i + 1),
                    target: p.clone(),
                    depth: 0,
                })
                .collect();
        }
        Ok(Self {
            doc,
            title,
            author,
            id,
            spine,
            toc,
            document: Arc::new(document),
            starts,
        })
    }

    /// The bytes of an archive resource (images), by its `/`-separated path.
    pub fn resource(&mut self, path: &str) -> Option<Vec<u8>> {
        self.doc.get_resource_by_path(path)
    }

    /// The cover image, if the book names one: (bytes, mime type).
    pub fn cover(&mut self) -> Option<(Vec<u8>, String)> {
        self.doc.get_cover()
    }

    /// The spine document holding top-level block `block`.
    pub fn spine_of(&self, block: usize) -> usize {
        self.starts
            .partition_point(|start| *start <= block)
            .saturating_sub(1)
    }

    /// A position in [`Self::document`] as a spine-relative location.
    pub fn location(&self, pos: Position) -> Location {
        let spine = self.spine_of(pos.block);
        Location {
            spine,
            block: pos.block - self.starts.get(spine).copied().unwrap_or(0),
            offset: pos.offset,
        }
    }

    /// A saved location as a position in [`Self::document`], clamped to the book.
    pub fn position(&self, loc: Location) -> Position {
        let Some(&start) = self.starts.get(loc.spine) else {
            return Position::default();
        };
        let end = self
            .starts
            .get(loc.spine + 1)
            .copied()
            .unwrap_or(self.document.blocks.len());
        Position {
            block: (start + loc.block)
                .min(end.max(start + 1) - 1)
                .min(self.document.blocks.len().saturating_sub(1)),
            offset: loc.offset,
        }
    }

    /// Where a link inside the book (`OEBPS/ch2.xhtml#n3`) or a TOC target goes.
    pub fn target(&self, link: &str) -> Option<Position> {
        let block = self.document.anchors.get(link).or_else(|| {
            // An unknown fragment: the start of its chapter.
            let path = link.split_once('#').map_or(link, |(p, _)| p);
            self.document.anchors.get(path)
        })?;
        Some(Position {
            block: *block,
            offset: 0,
        })
    }

    /// The table-of-contents label for the chapter holding `block`: the last entry that starts at
    /// or before it, else the book's title.
    pub fn label_at(&self, block: usize) -> String {
        let mut best: Option<(usize, &TocEntry)> = None;
        for entry in &self.toc {
            if let Some(p) = self.target(&entry.target) {
                if p.block <= block && best.is_none_or(|(b, _)| p.block >= b) {
                    best = Some((p.block, entry));
                }
            }
        }
        best.map_or_else(|| self.title.clone(), |(_, e)| e.label.clone())
    }

    /// Converting the whole book: blocks per spine document, for diagnostics.
    pub fn chapter_lengths(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.starts.windows(2).map(|w| w[1] - w[0]).collect();
        if let Some(last) = self.starts.last() {
            out.push(self.document.blocks.len() - last);
        }
        out
    }
}

/// `/`-separated, percent-decoded and with `.`/`..` folded, whatever the platform's separator.
fn archive_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = percent_encoding::percent_decode_str(&s)
        .decode_utf8_lossy()
        .into_owned();
    let mut parts: Vec<&str> = Vec::new();
    for part in s.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn flatten_toc(points: &[NavPoint], depth: usize, out: &mut Vec<TocEntry>) {
    for p in points {
        let content = p.content.to_string_lossy();
        // Keep the fragment out of the path folding.
        let (path, frag) = content
            .split_once('#')
            .map_or((content.as_ref(), None), |(a, b)| (a, Some(b)));
        let mut target = archive_path(Path::new(path));
        if let Some(f) = frag {
            target = format!("{target}#{f}");
        }
        out.push(TocEntry {
            label: p.label.split_whitespace().collect::<Vec<_>>().join(" "),
            target,
            depth,
        });
        flatten_toc(&p.children, depth + 1, out);
    }
}

/// EPUB 3 books without an NCX: the `<nav epub:type="toc">` list in the navigation document.
fn nav_document_toc(doc: &mut EpubDoc<Cursor<Vec<u8>>>) -> Vec<TocEntry> {
    let Some(id) = doc.get_nav_id() else {
        return Vec::new();
    };
    let Some(path) = doc.resources.get(&id).map(|r| archive_path(&r.path)) else {
        return Vec::new();
    };
    let Some(src) = doc.get_resource_str_by_path(&path) else {
        return Vec::new();
    };
    let html = Html::parse_document(&src);
    let nav_sel = Selector::parse("nav").expect("selector");
    let navs: Vec<ElementRef<'_>> = html.select(&nav_sel).collect();
    let toc_nav = navs
        .iter()
        .find(|n| {
            n.value()
                .attrs()
                .any(|(k, v)| k.ends_with("type") && v.contains("toc"))
        })
        .or(navs.first());
    let Some(nav) = toc_nav else {
        return Vec::new();
    };
    let a_sel = Selector::parse("a[href]").expect("selector");
    let opts = Options {
        base: Base::Path(path),
        stylesheet: None,
    };
    nav.select(&a_sel)
        .map(|a| {
            let depth = a
                .ancestors()
                .filter_map(ElementRef::wrap)
                .filter(|e| e.value().name() == "ol")
                .count()
                .saturating_sub(1);
            let label = a.text().collect::<String>();
            TocEntry {
                label: label.split_whitespace().collect::<Vec<_>>().join(" "),
                target: convert::resolve(&opts, a.value().attr("href").unwrap_or("")),
                depth,
            }
        })
        .collect()
}
