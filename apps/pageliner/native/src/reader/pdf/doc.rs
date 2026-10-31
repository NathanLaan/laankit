//! Opening, measuring and rasterising PDFs with hayro, and reading their
//! outline (hayro has no outline API, so it's walked here with
//! `hayro_syntax`, as pdf.js's `getOutline` + `getDestination` do).
//!
//! Page units are pdf.js's scale-1 viewport: PDF points, origin at the top
//! left of the rotated and cropped page. hayro's `render_dimensions` and
//! `initial_transform(true)` use the same space, so PageLiner's stored
//! highlight rectangles carry over unchanged.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use hayro::hayro_interpret::InterpreterSettings;
use hayro::hayro_syntax::object::{Array, Dict, MaybeRef, Name, Object, ObjectIdentifier};
use hayro::hayro_syntax::page::Page;
use hayro::hayro_syntax::Pdf;
use hayro::vello_cpu::color::palette::css::WHITE;
use hayro::{PixmapSettings, RenderCache, RenderSettings};

/// Outline entries read at most (malformed files can loop).
const MAX_OUTLINE_ITEMS: usize = 5_000;
const MAX_OUTLINE_DEPTH: usize = 32;
/// Name-tree nodes visited at most per lookup.
const MAX_NAME_TREE_NODES: usize = 2_000;

/// What the reader needs before the first page renders.
#[derive(Clone, Debug, Default)]
pub struct PdfInfo {
    /// Every page's size in page units, in page order.
    pub sizes: Vec<[f32; 2]>,
    pub outline: Vec<OutlineItem>,
}

/// One flattened outline entry (`{ title, dest, depth }` in the Svelte reader,
/// with the destination already resolved to a page).
#[derive(Clone, Debug, PartialEq)]
pub struct OutlineItem {
    pub title: String,
    pub depth: usize,
    /// 0-based page index, when the destination resolves.
    pub page: Option<usize>,
}

/// Loads a PDF from shared bytes; each worker opens its own [`Pdf`] over them.
pub fn open(bytes: Arc<Vec<u8>>) -> anyhow::Result<Pdf> {
    Pdf::new(bytes).map_err(|e| anyhow::anyhow!("Couldn't read the PDF: {e:?}"))
}

/// Opens the PDF once to measure it and read its outline.
pub fn inspect(bytes: Arc<Vec<u8>>) -> anyhow::Result<PdfInfo> {
    let pdf = open(bytes)?;
    if pdf.pages().is_empty() {
        anyhow::bail!("The PDF has no pages.");
    }
    Ok(PdfInfo {
        sizes: page_sizes(&pdf),
        outline: outline(&pdf),
    })
}

/// Every page's size in page units (rotation and crop box applied).
pub fn page_sizes(pdf: &Pdf) -> Vec<[f32; 2]> {
    pdf.pages()
        .iter()
        .map(|p| {
            let (w, h) = p.render_dimensions();
            [w, h]
        })
        .collect()
}

/// Renders `page` at `scale` device pixels per page unit, on white.
pub fn render_page<'a>(
    page: &'a Page<'a>,
    cache: &RenderCache<'a>,
    settings: &InterpreterSettings,
    scale: f32,
) -> egui::ColorImage {
    let pixmap = hayro::render(
        page,
        cache,
        settings,
        &RenderSettings::default(),
        &PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: WHITE,
        },
    );
    let size = [pixmap.width() as usize, pixmap.height() as usize];
    egui::ColorImage::from_rgba_premultiplied(size, pixmap.data_as_u8_slice())
}

/// The document outline, flattened depth-first, like the Svelte reader's
/// `loadOutline`. Empty when there is none.
pub fn outline(pdf: &Pdf) -> Vec<OutlineItem> {
    let xref = pdf.xref();
    let Some(root) = xref.get::<Dict<'_>>(xref.root_id()) else {
        return Vec::new();
    };
    let Some(first) = root
        .get::<Dict<'_>>("Outlines")
        .and_then(|o| o.get::<Dict<'_>>("First"))
    else {
        return Vec::new();
    };
    let pages: HashMap<ObjectIdentifier, usize> = pdf
        .pages()
        .iter()
        .enumerate()
        .filter_map(|(i, p)| p.raw().obj_id().map(|id| (id, i)))
        .collect();
    let mut walk = Walk {
        root: &root,
        pages: &pages,
        items: Vec::new(),
        seen: HashSet::new(),
    };
    walk.siblings(first, 0);
    walk.items
}

struct Walk<'r, 'a> {
    root: &'r Dict<'a>,
    pages: &'r HashMap<ObjectIdentifier, usize>,
    items: Vec<OutlineItem>,
    /// Outline nodes already visited (cycles in broken files).
    seen: HashSet<ObjectIdentifier>,
}

impl<'a> Walk<'_, 'a> {
    fn siblings(&mut self, first: Dict<'a>, depth: usize) {
        let mut node = Some(first);
        while let Some(item) = node {
            if self.items.len() >= MAX_OUTLINE_ITEMS || depth > MAX_OUTLINE_DEPTH {
                return;
            }
            if let Some(id) = item.obj_id() {
                if !self.seen.insert(id) {
                    return;
                }
            }
            let title = item
                .get::<hayro::hayro_syntax::object::String<'_>>("Title")
                .map(|s| decode_text(s.as_bytes()))
                .unwrap_or_default();
            let page = self.destination(&item);
            self.items.push(OutlineItem {
                title: title.trim().to_string(),
                depth,
                page,
            });
            if let Some(child) = item.get::<Dict<'a>>("First") {
                self.siblings(child, depth + 1);
            }
            node = item.get::<Dict<'a>>("Next");
        }
    }

    /// The page an outline item points at: `/Dest`, or a `/GoTo` action's `/D`.
    fn destination(&self, item: &Dict<'a>) -> Option<usize> {
        if let Some(dest) = item.get::<Object<'a>>("Dest") {
            return self.resolve(dest);
        }
        let action = item.get::<Dict<'a>>("A")?;
        let kind = action.get::<Name<'_>>("S")?;
        if kind.as_str() != "GoTo" {
            return None;
        }
        self.resolve(action.get::<Object<'a>>("D")?)
    }

    /// An explicit destination array, or a named one looked up in the
    /// catalog's `/Names /Dests` tree or legacy `/Dests` dictionary.
    fn resolve(&self, dest: Object<'a>) -> Option<usize> {
        match dest {
            Object::Array(a) => self.explicit(&a),
            Object::Name(n) => self.named(n.as_str().as_bytes()),
            Object::String(s) => self.named(s.as_bytes()),
            Object::Dict(d) => self.resolve(d.get::<Object<'a>>("D")?),
            _ => None,
        }
    }

    /// `[page /XYZ …]`: the page is a reference to a page object (or, in
    /// some files, a 0-based page number).
    fn explicit(&self, array: &Array<'a>) -> Option<usize> {
        match array.raw_iter().next()? {
            MaybeRef::Ref(r) => self.pages.get(&r.into()).copied(),
            MaybeRef::NotRef(Object::Number(n)) => {
                let i = n.as_f64();
                (i >= 0.0 && i < self.pages.len() as f64).then_some(i as usize)
            }
            MaybeRef::NotRef(_) => None,
        }
    }

    fn named(&self, name: &[u8]) -> Option<usize> {
        let tree = self
            .root
            .get::<Dict<'a>>("Names")
            .and_then(|n| n.get::<Dict<'a>>("Dests"));
        if let Some(found) = tree.and_then(|t| name_tree_lookup(t, name)) {
            return self.resolve(found);
        }
        let legacy = self.root.get::<Dict<'a>>("Dests")?;
        self.resolve(legacy.get::<Object<'a>>(name)?)
    }
}

/// Looks `key` up in a PDF name tree (`/Names` leaves, `/Kids` nodes).
fn name_tree_lookup<'a>(node: Dict<'a>, key: &[u8]) -> Option<Object<'a>> {
    let mut stack = vec![node];
    let mut visited = 0;
    while let Some(node) = stack.pop() {
        visited += 1;
        if visited > MAX_NAME_TREE_NODES {
            return None;
        }
        if let Some(names) = node.get::<Array<'a>>("Names") {
            let mut it = names.flex_iter();
            loop {
                let Some(k) = it.next::<hayro::hayro_syntax::object::String<'_>>() else {
                    break;
                };
                let value = it.next::<Object<'a>>();
                if k.as_bytes() == key {
                    return value;
                }
            }
        }
        if let Some(kids) = node.get::<Array<'a>>("Kids") {
            stack.extend(kids.iter::<Dict<'a>>());
        }
    }
    None
}

/// A PDF text string: UTF-16BE with a byte-order mark, UTF-8 with one, or
/// PDFDocEncoding (Latin-1 is close enough for titles).
pub fn decode_text(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        let units: Vec<u16> = rest
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| u16::from_be_bytes(*c))
            .collect();
        return String::from_utf16_lossy(&units);
    }
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    bytes.iter().map(|&b| b as char).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_pdf_text_strings() {
        assert_eq!(decode_text(b"Chapter 1"), "Chapter 1");
        assert_eq!(decode_text(&[0xfe, 0xff, 0x00, 0x43, 0x00, 0xe9]), "Cé");
        assert_eq!(decode_text(&[0xef, 0xbb, 0xbf, b'h', b'i']), "hi");
    }

    #[test]
    fn reads_nested_outlines_with_named_and_action_destinations() {
        let bytes = crate::reader::pdf::fixtures::outlined_pdf();
        let info = inspect(Arc::new(bytes)).unwrap();
        assert_eq!(info.sizes.len(), 3);
        assert_eq!(
            info.outline,
            vec![
                OutlineItem {
                    title: "Intro".into(),
                    depth: 0,
                    page: Some(0)
                },
                OutlineItem {
                    title: "Details".into(),
                    depth: 1,
                    page: Some(2)
                },
                OutlineItem {
                    title: "Named".into(),
                    depth: 0,
                    page: Some(1)
                },
            ]
        );
    }
}
