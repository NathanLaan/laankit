//! An open EPUB: spine, table of contents, stylesheet and chapters converted to rich text.
//!
//! The `epub` crate reads the container, OPF and NCX. Paths are kept as `/`-separated archive
//! paths (`OEBPS/text/ch01.xhtml`), percent-decoded, which is also what the converter resolves
//! links and images to.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::Path;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use epub::doc::{EpubDoc, NavPoint};
use scraper::{ElementRef, Html, Selector};
use spike_richtext::convert::{self, Base, Options};
use spike_richtext::css::StyleSheet;
use spike_richtext::Document;

pub struct Book<R: Read + Seek> {
    doc: EpubDoc<R>,
    pub title: String,
    /// A stable key for saved positions: the package's unique identifier, else the title.
    pub id: String,
    /// Archive paths of the spine documents, in reading order.
    pub spine: Vec<String>,
    pub toc: Vec<TocEntry>,
    stylesheet: StyleSheet,
    chapters: HashMap<usize, Arc<Document>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TocEntry {
    pub label: String,
    /// `path` or `path#fragment`.
    pub target: String,
    pub depth: usize,
}

/// Where a link inside the book goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub chapter: usize,
    pub fragment: Option<String>,
}

impl Book<std::io::BufReader<std::fs::File>> {
    pub fn open(path: &Path) -> Result<Self> {
        Book::from_doc(EpubDoc::new(path).map_err(|e| anyhow!("{e}"))?)
    }
}

impl<R: Read + Seek> Book<R> {
    pub fn from_doc(mut doc: EpubDoc<R>) -> Result<Self> {
        let title = doc.get_title().unwrap_or_else(|| "Untitled".into());
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
        let css: Vec<_> = doc
            .resources
            .values()
            .filter(|r| r.mime == "text/css")
            .map(|r| r.path.clone())
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
            id,
            spine,
            toc,
            stylesheet,
            chapters: HashMap::new(),
        })
    }

    /// Chapter `i`, converted on first use.
    pub fn chapter(&mut self, i: usize) -> Arc<Document> {
        if let Some(doc) = self.chapters.get(&i) {
            return doc.clone();
        }
        let doc = Arc::new(self.convert_chapter(i));
        self.chapters.insert(i, doc.clone());
        doc
    }

    /// Reads and converts chapter `i` (uncached; for timing).
    pub fn convert_chapter(&mut self, i: usize) -> Document {
        let Some(path) = self.spine.get(i).cloned() else {
            return Document::default();
        };
        let Some(xhtml) = self.doc.get_resource_str_by_path(&path) else {
            log::warn!("Missing spine document {path}");
            return Document::default();
        };
        let opts = Options {
            base: Base::Path(path),
            stylesheet: Some(&self.stylesheet),
        };
        convert::from_xhtml(&xhtml, &opts)
    }

    /// The bytes of an archive resource (images).
    pub fn resource(&mut self, path: &str) -> Option<Vec<u8>> {
        self.doc.get_resource_by_path(path)
    }

    /// Resolves an internal link (`OEBPS/ch2.xhtml#n3`) to a chapter.
    pub fn target(&self, link: &str) -> Option<Target> {
        let (path, fragment) = link
            .split_once('#')
            .map_or((link, None), |(p, f)| (p, Some(f.to_string())));
        let chapter = self.spine.iter().position(|p| p == path)?;
        Some(Target { chapter, fragment })
    }

    /// The chapter a table-of-contents entry is in (for highlighting the current one).
    pub fn toc_chapter(&self, entry: &TocEntry) -> Option<usize> {
        self.target(&entry.target).map(|t| t.chapter)
    }
}

/// `/`-separated and percent-decoded, whatever the platform's separator.
fn archive_path(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    percent_encoding::percent_decode_str(&s)
        .decode_utf8_lossy()
        .into_owned()
}

fn flatten_toc(points: &[NavPoint], depth: usize, out: &mut Vec<TocEntry>) {
    for p in points {
        out.push(TocEntry {
            label: p.label.trim().to_string(),
            target: archive_path(&p.content),
            depth,
        });
        flatten_toc(&p.children, depth + 1, out);
    }
}

/// EPUB 3 books without an NCX: the `<nav epub:type="toc">` list in the navigation document.
fn nav_document_toc<R: Read + Seek>(doc: &mut EpubDoc<R>) -> Vec<TocEntry> {
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
    let mut out = Vec::new();
    let a_sel = Selector::parse("a[href]").expect("selector");
    let opts = Options {
        base: Base::Path(path),
        stylesheet: None,
    };
    for a in nav.select(&a_sel) {
        let depth = a
            .ancestors()
            .filter_map(ElementRef::wrap)
            .filter(|e| e.value().name() == "ol")
            .count()
            .saturating_sub(1);
        let label = a
            .text()
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let href = a.value().attr("href").unwrap_or("");
        out.push(TocEntry {
            label,
            target: convert::resolve(&opts, href),
            depth,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use std::io::{Cursor, Write};

    use spike_richtext::{Align, Block};
    use zip::write::SimpleFileOptions;

    use super::*;

    const CONTAINER: &str = r#"<?xml version="1.0"?>
<container version="1.0" xmlns="urn:oasis:names:tc:opendocument:xmlns:container">
  <rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles>
</container>"#;
    const OPF: &str = r#"<?xml version="1.0"?>
<package xmlns="http://www.idpf.org/2007/opf" version="2.0" unique-identifier="uid">
  <metadata xmlns:dc="http://purl.org/dc/elements/1.1/">
    <dc:title>Synthetic Book</dc:title><dc:identifier id="uid">urn:uuid:spike-epub-test</dc:identifier>
  </metadata>
  <manifest>
    <item id="ncx" href="toc.ncx" media-type="application/x-dtbncx+xml"/>
    <item id="css" href="style/book.css" media-type="text/css"/>
    <item id="c1" href="text/chapter%201.xhtml" media-type="application/xhtml+xml"/>
    <item id="c2" href="text/ch2.xhtml" media-type="application/xhtml+xml"/>
    <item id="img" href="images/dot.png" media-type="image/png"/>
  </manifest>
  <spine toc="ncx"><itemref idref="c1"/><itemref idref="c2"/></spine>
</package>"#;
    const NCX: &str = r#"<?xml version="1.0"?>
<ncx xmlns="http://www.daisy.org/z3986/2005/ncx/" version="2005-1">
  <navMap>
    <navPoint id="n1" playOrder="1"><navLabel><text>One</text></navLabel><content src="text/chapter%201.xhtml"/>
      <navPoint id="n1a" playOrder="2"><navLabel><text>One A</text></navLabel><content src="text/chapter%201.xhtml#a"/></navPoint>
    </navPoint>
    <navPoint id="n2" playOrder="3"><navLabel><text>Two</text></navLabel><content src="text/ch2.xhtml"/></navPoint>
  </navMap>
</ncx>"#;
    const CSS: &str =
        ".title { text-align: center; font-weight: bold } span.it { font-style: italic }";
    const CH1: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>1</title><link rel="stylesheet" href="../style/book.css"/></head>
<body><h1 class="title">Chapter One</h1><p>Plain <span class="it">italic</span> text.</p>
<p id="a">See <a href="ch2.xhtml#end">the end</a>.</p><div><img src="../images/dot.png" alt="dot"/></div></body></html>"#;
    const CH2: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<html xmlns="http://www.w3.org/1999/xhtml"><head><title>2</title></head>
<body><p>Two.</p><p class="title" id="end">The End</p></body></html>"#;

    /// A small EPUB 2 in memory (the books used for measuring can't be committed).
    pub(crate) fn synthetic_epub() -> Vec<u8> {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(64, 48, image::Rgba([200, 30, 30, 255]))
            .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(b"application/epub+zip").unwrap();
        let files: [(&str, &[u8]); 7] = [
            ("META-INF/container.xml", CONTAINER.as_bytes()),
            ("OEBPS/content.opf", OPF.as_bytes()),
            ("OEBPS/toc.ncx", NCX.as_bytes()),
            ("OEBPS/style/book.css", CSS.as_bytes()),
            ("OEBPS/text/chapter 1.xhtml", CH1.as_bytes()),
            ("OEBPS/text/ch2.xhtml", CH2.as_bytes()),
            ("OEBPS/images/dot.png", &png),
        ];
        for (name, data) in files {
            zip.start_file(name, SimpleFileOptions::default()).unwrap();
            zip.write_all(data).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }

    fn book() -> Book<Cursor<Vec<u8>>> {
        Book::from_doc(EpubDoc::from_reader(Cursor::new(synthetic_epub())).unwrap()).unwrap()
    }

    #[test]
    fn opens_spine_and_toc() {
        let b = book();
        assert_eq!(b.title, "Synthetic Book");
        assert_eq!(b.id, "urn:uuid:spike-epub-test");
        assert_eq!(
            b.spine,
            ["OEBPS/text/chapter 1.xhtml", "OEBPS/text/ch2.xhtml"]
        );
        let toc: Vec<_> = b
            .toc
            .iter()
            .map(|t| (t.label.as_str(), t.target.as_str(), t.depth))
            .collect();
        assert_eq!(
            toc,
            [
                ("One", "OEBPS/text/chapter 1.xhtml", 0),
                ("One A", "OEBPS/text/chapter 1.xhtml#a", 1),
                ("Two", "OEBPS/text/ch2.xhtml", 0)
            ]
        );
        assert_eq!(b.toc_chapter(&b.toc[2]), Some(1));
    }

    #[test]
    fn converts_chapters_with_css_links_and_images() {
        let mut b = book();
        let ch = b.chapter(0);
        let Block::Text {
            align: Align::Center,
            spans,
            ..
        } = &ch.blocks[0]
        else {
            panic!("{:?}", ch.blocks[0])
        };
        assert!(spans[0].style.bold);
        let Block::Text { spans, .. } = &ch.blocks[1] else {
            panic!()
        };
        assert!(spans.iter().any(|s| s.style.italic && s.text == "italic"));
        let Block::Text { spans, .. } = &ch.blocks[2] else {
            panic!()
        };
        let link = spans.iter().find_map(|s| s.link.clone()).unwrap();
        assert_eq!(
            b.target(&link),
            Some(Target {
                chapter: 1,
                fragment: Some("end".into())
            })
        );
        assert_eq!(ch.anchors.get("a"), Some(&2));
        let Block::Image { src, .. } = &ch.blocks[3] else {
            panic!("{:?}", ch.blocks[3])
        };
        assert_eq!(src, "OEBPS/images/dot.png");
        assert!(b
            .resource(src)
            .is_some_and(|bytes| bytes.starts_with(b"\x89PNG")));
        let ch2 = b.chapter(1);
        assert_eq!(ch2.anchors.get("end"), Some(&1));
    }
}
