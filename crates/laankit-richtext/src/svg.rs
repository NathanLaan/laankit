//! Inline `<svg>`: charts, diagrams and icons drawn in the document itself (articles, EPUB
//! chapters, the NoteLiner preview), shown as images.
//!
//! **Cleaning (at conversion).** [`clean`] re-serializes the `<svg>` element as standalone XML,
//! keeping only drawing: `<script>`, `<foreignObject>` and embedded media are dropped with their
//! content, `on*` event attributes are dropped, and `href`s survive only as same-document
//! references (`#id`) or embedded raster images (`data:image/…`). So nothing in the result can run
//! or fetch anything. Elements past [`MAX_ELEMENTS`] or markup past [`MAX_SOURCE_BYTES`] drop the
//! whole picture. The result becomes an ordinary image block whose `src` is a
//! `data:image/svg+xml;base64,…` URI, so the model and the apps' resolvers don't change.
//!
//! **Rendering (in the view).** [`image_uri`] registers such a source with [`InlineSvgLoader`], an
//! egui image loader that rasterizes with `resvg` on a worker thread (never the UI thread) at the
//! size the view asks for, capped at [`MAX_SIDE`] and [`MAX_PIXELS`], and caches the result.
//! `usvg` itself fetches nothing: external image references resolve to nothing, and only
//! embedded `data:` images are decoded. A render that takes longer than [`RENDER_TIMEOUT`] is
//! reported as failed (its thread finishes in the background and is ignored).

use std::collections::HashMap;
use std::sync::mpsc::{channel, Receiver};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use egui::load::{ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint};
use egui::{ColorImage, Vec2};
use resvg::usvg;
use scraper::{ElementRef, Node};

/// Markup larger than this (after cleaning) is dropped.
pub const MAX_SOURCE_BYTES: usize = 512 * 1024;
/// Pictures with more elements than this are dropped (a complexity guard for the renderer).
pub const MAX_ELEMENTS: usize = 20_000;
/// The longest side of a rendered picture, in pixels.
pub const MAX_SIDE: f32 = 2048.0;
/// The most pixels a rendered picture may have.
pub const MAX_PIXELS: f32 = 4_000_000.0;
/// A render still running after this is reported as failed.
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(3);

/// The URI scheme the loader answers to.
const SCHEME: &str = "inline-svg://";
const DATA_PREFIX: &str = "data:image/svg+xml";
const SVG_NS: &str = "http://www.w3.org/2000/svg";
/// Elements dropped with everything inside them.
const DROPPED: &[&str] = &[
    "script",
    "foreignObject",
    "iframe",
    "object",
    "embed",
    "audio",
    "video",
    "canvas",
    "handler",
    "listener",
];
/// Ready images unused for this many passes are dropped from the cache.
const EVICT_AFTER_PASSES: u64 = 600;

/// A cleaned `<svg>`.
#[derive(Clone, Debug, PartialEq)]
pub struct Clean {
    /// Standalone SVG XML.
    pub markup: String,
    /// The declared size in CSS pixels (`width`/`height`, else the `viewBox`), if any.
    pub width: Option<f32>,
    pub height: Option<f32>,
    /// `<title>` or `aria-label`: the picture's alt text.
    pub title: String,
}

/// Cleans an `<svg>` element (see the module docs), or `None` when it's empty or over the caps.
pub fn clean(el: ElementRef<'_>) -> Option<Clean> {
    let mut markup = String::new();
    let mut count = 0;
    if !has_drawing(el) || !write_element(el, &mut markup, &mut count, true) {
        return None;
    }
    if markup.len() > MAX_SOURCE_BYTES {
        return None;
    }
    let e = el.value();
    let (mut width, mut height) = (length(e.attr("width")), length(e.attr("height")));
    if width.is_none() || height.is_none() {
        if let Some((w, h)) = view_box(e.attr("viewBox")) {
            match (width, height) {
                (None, None) => (width, height) = (Some(w), Some(h)),
                (Some(cw), None) if w > 0.0 => height = Some(cw * h / w),
                (None, Some(ch)) if h > 0.0 => width = Some(ch * w / h),
                _ => {}
            }
        }
    }
    let title = el
        .children()
        .filter_map(ElementRef::wrap)
        .find(|c| c.value().name() == "title")
        .map(|t| t.text().collect::<String>())
        .or_else(|| e.attr("aria-label").map(str::to_string))
        .unwrap_or_default()
        .trim()
        .to_string();
    Some(Clean {
        markup,
        width,
        height,
        title,
    })
}

/// `markup` as a `data:` URI (the image block's `src`).
pub fn data_uri(markup: &str) -> String {
    use base64::Engine;
    format!(
        "{DATA_PREFIX};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(markup)
    )
}

/// Whether an image `src` is an SVG `data:` URI (inline SVG, or an `<img>` with one).
pub fn is_svg_data(src: &str) -> bool {
    src.starts_with(DATA_PREFIX)
}

/// The egui image URI for an SVG `data:` `src`, registering it with the [`InlineSvgLoader`]
/// (installed on first use). `None` for anything else, or an undecodable URI.
pub fn image_uri(ctx: &egui::Context, src: &str) -> Option<String> {
    if !is_svg_data(src) {
        return None;
    }
    let loader = loader(ctx);
    let id = loader.register(src)?;
    Some(format!("{SCHEME}{id:016x}"))
}

/// The context's loader, installed on first use.
fn loader(ctx: &egui::Context) -> Arc<InlineSvgLoader> {
    let key = egui::Id::unique("laankit-richtext-inline-svg");
    if let Some(l) = ctx.data(|d| d.get_temp::<Arc<InlineSvgLoader>>(key)) {
        return l;
    }
    let l = Arc::new(InlineSvgLoader::default());
    ctx.add_image_loader(l.clone());
    ctx.data_mut(|d| d.insert_temp(key, l.clone()));
    l
}

/// Rasterizes registered SVG sources on worker threads (see the module docs).
#[derive(Default)]
pub struct InlineSvgLoader {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Source id → markup.
    sources: HashMap<u64, Arc<str>>,
    /// A cheap key of a `src` string (address, length, ends) → source id, so a document's `src`
    /// isn't decoded and hashed every frame.
    known: HashMap<(usize, usize, u64), u64>,
    cache: HashMap<(u64, SizeHint), Entry>,
    pass: u64,
}

struct Entry {
    state: State,
    last_used: u64,
}

enum State {
    Pending {
        since: Instant,
        rx: Receiver<Result<ColorImage, String>>,
    },
    Ready(Arc<ColorImage>),
    Failed(String),
}

impl InlineSvgLoader {
    pub const ID: &'static str = egui::generate_loader_id!(InlineSvgLoader);

    /// Registers a `data:` source; returns its id.
    fn register(&self, src: &str) -> Option<u64> {
        let quick = (src.as_ptr() as usize, src.len(), quick_hash(src));
        let mut inner = self.inner.lock().ok()?;
        if let Some(&id) = inner.known.get(&quick) {
            if inner.sources.contains_key(&id) {
                return Some(id);
            }
        }
        let markup = decode(src)?;
        let id = fnv1a(markup.as_bytes());
        inner.sources.entry(id).or_insert_with(|| markup.into());
        inner.known.insert(quick, id);
        Some(id)
    }
}

impl ImageLoader for InlineSvgLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, ctx: &egui::Context, uri: &str, size_hint: SizeHint) -> ImageLoadResult {
        let Some(id) = uri
            .strip_prefix(SCHEME)
            .and_then(|h| u64::from_str_radix(h, 16).ok())
        else {
            return Err(LoadError::NotSupported);
        };
        let mut inner = self.inner.lock().expect("svg loader lock");
        let pass = inner.pass;
        let Some(markup) = inner.sources.get(&id).cloned() else {
            return Err(LoadError::Loading("unknown inline SVG".into()));
        };
        let entry = inner.cache.entry((id, size_hint)).or_insert_with(|| {
            let (tx, rx) = channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(render(&markup, size_hint));
                ctx.request_repaint();
            });
            Entry {
                state: State::Pending {
                    since: Instant::now(),
                    rx,
                },
                last_used: pass,
            }
        });
        entry.last_used = pass;
        if let State::Pending { since, rx } = &entry.state {
            match rx.try_recv() {
                Ok(Ok(image)) => entry.state = State::Ready(Arc::new(image)),
                Ok(Err(err)) => entry.state = State::Failed(err),
                Err(_) if since.elapsed() > RENDER_TIMEOUT => {
                    entry.state = State::Failed("SVG render timed out".into())
                }
                Err(_) => {}
            }
        }
        match &entry.state {
            State::Pending { .. } => Ok(ImagePoll::Pending { size: None }),
            State::Ready(image) => Ok(ImagePoll::Ready {
                image: image.clone(),
            }),
            State::Failed(err) => Err(LoadError::Loading(err.clone())),
        }
    }

    fn forget(&self, uri: &str) {
        if let Some(id) = uri
            .strip_prefix(SCHEME)
            .and_then(|h| u64::from_str_radix(h, 16).ok())
        {
            if let Ok(mut inner) = self.inner.lock() {
                inner.cache.retain(|(i, _), _| *i != id);
            }
        }
    }

    fn forget_all(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.cache.clear();
        }
    }

    fn end_pass(&self, pass_index: u64) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.pass = pass_index;
            inner.cache.retain(|_, e| {
                matches!(e.state, State::Pending { .. })
                    || pass_index.saturating_sub(e.last_used) < EVICT_AFTER_PASSES
            });
        }
    }

    fn byte_size(&self) -> usize {
        self.inner.lock().map_or(0, |inner| {
            inner
                .cache
                .values()
                .map(|e| match &e.state {
                    State::Ready(image) => image.pixels.len() * 4,
                    _ => 0,
                })
                .sum()
        })
    }

    fn has_pending(&self) -> bool {
        self.inner.lock().is_ok_and(|inner| {
            inner
                .cache
                .values()
                .any(|e| matches!(e.state, State::Pending { .. }))
        })
    }
}

/// Parses and rasterizes `markup` at `hint`, within the size caps.
pub fn render(markup: &str, hint: SizeHint) -> Result<ColorImage, String> {
    let options = usvg::Options {
        resources_dir: None,
        fontdb: fonts(),
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: usvg::ImageHrefResolver::default_data_resolver(),
            // External files and URLs: nothing.
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = usvg::Tree::from_str(markup, &options).map_err(|e| e.to_string())?;
    let source = Vec2::new(tree.size().width(), tree.size().height());
    let mut size = match hint {
        SizeHint::Size {
            width,
            height,
            maintain_aspect_ratio: true,
        } => {
            let mut s = source * (width as f32 / source.x);
            if s.y > height as f32 {
                s *= height as f32 / s.y;
            }
            s
        }
        SizeHint::Size { width, height, .. } => Vec2::new(width as f32, height as f32),
        SizeHint::Width(w) => source * (w as f32 / source.x),
        SizeHint::Height(h) => source * (h as f32 / source.y),
        SizeHint::Scale(s) => source * s.into_inner(),
    };
    let scale = (MAX_SIDE / size.max_elem())
        .min((MAX_PIXELS / (size.x * size.y)).sqrt())
        .min(1.0);
    size = (size * scale).round().max(Vec2::splat(1.0));
    let (w, h) = (size.x as u32, size.y as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("empty SVG")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(size.x / source.x, size.y / source.y),
        &mut pixmap.as_mut(),
    );
    let mut image = ColorImage::from_rgba_premultiplied([w as usize, h as usize], pixmap.data());
    // Layout sizes the picture by its own (CSS-pixel) size, not the raster's.
    image.source_size = source;
    Ok(image)
}

/// System fonts for `<text>`, loaded once (on a worker thread, by the first render that needs
/// them).
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// Writes `el` as XML; `false` when the element cap was hit.
fn write_element(el: ElementRef<'_>, out: &mut String, count: &mut usize, root: bool) -> bool {
    let name = el.value().name();
    if DROPPED.iter().any(|d| d.eq_ignore_ascii_case(name)) {
        return true;
    }
    *count += 1;
    if *count > MAX_ELEMENTS {
        return false;
    }
    // Links can't be followed in a picture; keep what they wrap.
    let tag = if name == "a" { "g" } else { name };
    out.push('<');
    out.push_str(tag);
    if root {
        out.push_str(" xmlns=\"");
        out.push_str(SVG_NS);
        out.push('"');
    }
    for (k, v) in el.value().attrs() {
        let lower = k.to_ascii_lowercase();
        if lower.starts_with("on")
            || lower.starts_with("xmlns")
            || v.to_ascii_lowercase().contains("javascript:")
            || (lower == "href" && !safe_href(v))
            || !k
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            continue;
        }
        out.push(' ');
        out.push_str(k);
        out.push_str("=\"");
        escape(v, out);
        out.push('"');
    }
    out.push('>');
    for child in el.children() {
        if let Node::Text(t) = child.value() {
            escape(t, out);
        } else if let Some(child) = ElementRef::wrap(child) {
            if !write_element(child, out, count, false) {
                return false;
            }
        }
    }
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    true
}

/// An `<svg>` with only a `<title>` or nothing draws nothing.
fn has_drawing(el: ElementRef<'_>) -> bool {
    el.descendants()
        .filter_map(ElementRef::wrap)
        .any(|d| !matches!(d.value().name(), "svg" | "title" | "desc" | "metadata"))
}

/// Same-document references and embedded raster images.
fn safe_href(v: &str) -> bool {
    let v = v.trim();
    v.starts_with('#')
        || ["png", "jpeg", "jpg", "gif", "webp"]
            .iter()
            .any(|t| v.starts_with(&format!("data:image/{t}")))
}

fn escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            c => out.push(c),
        }
    }
}

/// A CSS-pixel length attribute (`120`, `120px`); `None` for relative units.
fn length(v: Option<&str>) -> Option<f32> {
    let v = v?.trim();
    let n = v.strip_suffix("px").unwrap_or(v).trim();
    n.parse::<f32>().ok().filter(|n| *n > 0.0)
}

/// The `viewBox`'s width and height.
fn view_box(v: Option<&str>) -> Option<(f32, f32)> {
    let parts: Vec<f32> = v?
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().ok())
        .collect::<Option<_>>()?;
    match parts.as_slice() {
        [_, _, w, h] if *w > 0.0 && *h > 0.0 => Some((*w, *h)),
        _ => None,
    }
}

/// The markup inside an SVG `data:` URI (base64, or percent-encoded / plain).
fn decode(src: &str) -> Option<String> {
    use base64::Engine;
    let (meta, data) = src.split_once(',')?;
    if meta.ends_with(";base64") {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .ok()?;
        String::from_utf8(bytes).ok()
    } else {
        Some(
            percent_encoding::percent_decode_str(data)
                .decode_utf8_lossy()
                .into_owned(),
        )
    }
}

/// Hashes the ends of a long string (cheap; the full hash is taken once on registration).
fn quick_hash(s: &str) -> u64 {
    let b = s.as_bytes();
    let n = b.len().min(256);
    fnv1a(&[&b[..n], &b[b.len() - n..]].concat())
}

fn fnv1a(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    h
}

#[cfg(test)]
mod tests {
    use scraper::Html;

    use super::*;

    fn clean_html(html: &str) -> Option<Clean> {
        let doc = Html::parse_fragment(html);
        let svg = doc
            .root_element()
            .descendants()
            .filter_map(ElementRef::wrap)
            .find(|e| e.value().name() == "svg")
            .expect("svg");
        clean(svg)
    }

    #[test]
    fn strips_scripts_handlers_and_external_refs() {
        let c = clean_html(
            r##"<svg width="100" height="50" onload="alert(1)"><title>Chart</title>
            <script>alert(2)</script><foreignObject><p>html</p></foreignObject>
            <rect width="10" height="10" onclick="x()" fill="red"/>
            <image href="https://tracker.example/p.png"/><image xlink:href="file:///etc/passwd"/>
            <use href="#r"/><a href="javascript:alert(3)"><circle r="3"/></a></svg>"##,
        )
        .expect("clean");
        let m = &c.markup;
        assert!(
            m.starts_with(r#"<svg xmlns="http://www.w3.org/2000/svg""#),
            "{m}"
        );
        for bad in [
            "script",
            "alert",
            "onload",
            "onclick",
            "foreignObject",
            "tracker",
            "passwd",
            "javascript",
        ] {
            assert!(!m.contains(bad), "{bad} survived: {m}");
        }
        assert!(m.contains(r##"<use href="#r">"##), "{m}");
        assert!(m.contains("<g><circle"), "{m}");
        assert_eq!(
            (c.width, c.height, c.title.as_str()),
            (Some(100.0), Some(50.0), "Chart")
        );
        assert!(render(m, SizeHint::Scale(1.0.into())).is_ok());
    }

    #[test]
    fn size_from_view_box_and_caps() {
        let c = clean_html(r#"<svg viewBox="0 0 400 200"><rect width="400" height="200"/></svg>"#)
            .expect("clean");
        assert_eq!((c.width, c.height), (Some(400.0), Some(200.0)));
        let many = format!("<svg>{}</svg>", "<rect/>".repeat(MAX_ELEMENTS + 1));
        assert_eq!(clean_html(&many), None, "element cap");
        assert_eq!(
            clean_html("<svg><title>x</title></svg>"),
            None,
            "nothing to draw"
        );
    }

    #[test]
    fn renders_within_the_pixel_caps() {
        let c = clean_html(
            r#"<svg width="10000" height="10000"><rect width="10000" height="10000" fill="blue"/></svg>"#,
        )
        .expect("clean");
        let image = render(&c.markup, SizeHint::Scale(1.0.into())).expect("render");
        assert!(image.size[0] as f32 <= MAX_SIDE && image.size[1] as f32 <= MAX_SIDE);
        assert!((image.size[0] * image.size[1]) as f32 <= MAX_PIXELS);
        // Layout still sees the picture's own size.
        assert_eq!(image.source_size, Vec2::new(10000.0, 10000.0));
        let px = image.pixels[image.pixels.len() / 2];
        assert!(px.b() > 200 && px.r() < 50, "{px:?}");
    }

    #[test]
    fn data_uri_round_trips() {
        let uri = data_uri("<svg/>");
        assert!(is_svg_data(&uri));
        assert_eq!(decode(&uri).as_deref(), Some("<svg/>"));
        assert_eq!(
            decode("data:image/svg+xml,%3Csvg%2F%3E").as_deref(),
            Some("<svg/>")
        );
    }
}
