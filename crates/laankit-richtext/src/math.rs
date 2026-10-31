//! Math in rich text: `$…$` inline and `$$…$$` on its own line, as NoteLiner's
//! preview and slides show them.
//!
//! **In the model.** [`crate::from_note_html`] recognizes the math comrak emits
//! (`<span data-math-style="inline|display">`, and `<pre data-math-style>` for
//! ```` ```math ```` blocks). Inline math becomes a span whose text is the LaTeX
//! source and whose [`Style::math`](crate::Style::math) says how to set it; a
//! paragraph holding nothing but display math becomes an image block whose
//! `src` is [`image_src`], so copying and search keep the LaTeX.
//!
//! **Rendering.** This crate doesn't typeset math itself: the app installs a
//! [`MathRenderer`] (NoteLiner's compiles with Typst) with [`install`]. The
//! [`MathLoader`] it registers turns formulas into textures on a worker
//! thread and caches them by formula, mode and text size. Formulas are
//! rendered as white glyphs on transparent and drawn tinted with the text
//! colour, so every theme reuses the same texture. Without a renderer, or
//! while a formula is rendering, or when it fails (unsupported LaTeX), the
//! source is shown as code instead (`$x^2$`).
//!
//! **Inline layout.** A layout job can't hold an image, so an inline formula
//! is laid out as invisible no-break spaces, one per character of its source
//! (so character positions match the model), spaced to the formula's width
//! and tall enough for it; the texture is then painted over them. Being
//! no-break spaces, a formula never wraps inside itself.

use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use base64::Engine;
use egui::load::{
    ImageLoadResult, ImageLoader, ImagePoll, LoadError, SizeHint, SizedTexture, TexturePoll,
};
use egui::text::TextFormat;
use egui::{Color32, ColorImage, FontId, Rect, TextureOptions};

/// Renders one formula to SVG. Implemented by the app (NoteLiner: Typst).
pub trait MathRenderer: Send + Sync {
    /// `latex` without `$`; `display` for a formula on its own line; `size`
    /// is the text size in points. Glyphs should be white on transparent; the
    /// view tints them. An error means "show the source instead".
    fn render_svg(&self, latex: &str, display: bool, size: f32) -> Result<String, String>;
}

/// How a formula is set.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MathMode {
    /// `$…$`: in the line of text.
    Inline,
    /// `$$…$$`: on its own line, with display-size operators.
    Display,
}

impl MathMode {
    fn is_display(self) -> bool {
        self == MathMode::Display
    }
}

/// Where a formula's texture stands.
#[derive(Clone, Copy, Debug)]
pub enum MathImage {
    /// Rendered: draw this (tinted), at `texture.size` points.
    Ready(SizedTexture),
    /// Rendering on the worker thread; show the source for now.
    Pending,
    /// No renderer, or it couldn't render this formula: show the source.
    Unavailable,
}

/// A rendering still running after this is given up (and the source shown).
pub const RENDER_TIMEOUT: Duration = Duration::from_secs(10);
/// Formulas longer than this aren't rendered.
pub const MAX_SOURCE_LEN: usize = 8 * 1024;
const SRC_PREFIX: &str = "math:";
const SCHEME: &str = "math://";
/// Ready textures unused for this many passes are dropped.
const EVICT_AFTER_PASSES: u64 = 600;

/// The image `src` the model uses for a display formula.
pub fn image_src(latex: &str) -> String {
    format!(
        "{SRC_PREFIX}{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(latex)
    )
}

/// The LaTeX of a display formula's image `src`, if it is one.
pub fn parse_image_src(src: &str) -> Option<String> {
    let b64 = src.strip_prefix(SRC_PREFIX)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(b64)
        .ok()?;
    String::from_utf8(bytes).ok()
}

/// Installs `renderer` for `ctx` (once; later calls replace the renderer).
pub fn install(ctx: &egui::Context, renderer: Arc<dyn MathRenderer>) {
    if let Some(loader) = loader(ctx) {
        *loader.shared.renderer.lock().expect("math renderer lock") = Some(renderer);
        loader.shared.clear();
        ctx.forget_all_images();
        return;
    }
    let loader = Arc::new(MathLoader::new(renderer, ctx.clone()));
    ctx.add_image_loader(loader.clone());
    ctx.data_mut(|d| d.insert_temp(key(), loader));
}

/// Whether a renderer is installed (otherwise math always shows its source).
pub fn is_installed(ctx: &egui::Context) -> bool {
    loader(ctx).is_some()
}

/// The texture for a formula at `size` points (requesting it if needed).
pub fn image(ctx: &egui::Context, latex: &str, mode: MathMode, size: f32) -> MathImage {
    if latex.trim().is_empty() || latex.len() > MAX_SOURCE_LEN || !is_installed(ctx) {
        return MathImage::Unavailable;
    }
    let uri = uri(latex, mode, size);
    let hint = SizeHint::Scale(ctx.pixels_per_point().into());
    match ctx.try_load_texture(&uri, TextureOptions::LINEAR, hint) {
        Ok(TexturePoll::Ready { texture }) => MathImage::Ready(texture),
        Ok(TexturePoll::Pending { .. }) => MathImage::Pending,
        Err(_) => MathImage::Unavailable,
    }
}

/// The text an inline formula is laid out as, and its format, when its
/// texture is ready: one no-break space per source character, spaced to the
/// texture's width. `None` means lay out the source instead (see the module
/// docs). `base` is the span's own format (font, colour, line height).
pub fn inline_placeholder(
    ctx: &egui::Context,
    latex: &str,
    mode: MathMode,
    base: &TextFormat,
) -> Option<(String, TextFormat, SizedTexture)> {
    let MathImage::Ready(texture) = image(ctx, latex, mode, base.font_id.size) else {
        return None;
    };
    let n = latex.chars().count().max(1);
    let space = ctx.fonts_mut(|f| f.glyph_width(&base.font_id, NBSP));
    let mut format = base.clone();
    format.color = Color32::TRANSPARENT;
    format.background = Color32::TRANSPARENT;
    format.underline = egui::Stroke::NONE;
    format.strikethrough = egui::Stroke::NONE;
    format.extra_letter_spacing = texture.size.x / n as f32 - space;
    let line = format.line_height.unwrap_or(format.font_id.size * 1.2);
    format.line_height = Some(line.max(texture.size.y + 2.0));
    format.valign = egui::Align::Center;
    Some((NBSP.to_string().repeat(n), format, texture))
}

/// The source shown when a formula can't be drawn: `$x$` / `$$x$$`.
pub fn fallback_text(latex: &str, mode: MathMode) -> String {
    let d = if mode.is_display() { "$$" } else { "$" };
    format!("{d}{latex}{d}")
}

/// Paints a formula's texture centred in `rect` (the placeholder's box),
/// tinted `color`.
pub fn paint(painter: &egui::Painter, rect: Rect, texture: SizedTexture, color: Color32) {
    let at = Rect::from_center_size(rect.center(), texture.size);
    painter.image(
        texture.id,
        at,
        Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
        color,
    );
}

/// A no-break space: the placeholder character (egui never wraps at it).
pub const NBSP: char = '\u{00A0}';

/// The font of the fallback source text.
pub fn fallback_font(size: f32) -> FontId {
    FontId::monospace(size * 0.9)
}

fn uri(latex: &str, mode: MathMode, size: f32) -> String {
    let m = if mode.is_display() { 'd' } else { 'i' };
    // Hundredths of a point: different zoom levels get their own render.
    let centi = (size * 100.0).round().clamp(100.0, 50_000.0) as u32;
    format!(
        "{SCHEME}{m}/{centi}/{}",
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(latex)
    )
}

fn parse_uri(uri: &str) -> Option<(String, bool, f32)> {
    let rest = uri.strip_prefix(SCHEME)?;
    let mut parts = rest.splitn(3, '/');
    let display = match parts.next()? {
        "d" => true,
        "i" => false,
        _ => return None,
    };
    let size = parts.next()?.parse::<u32>().ok()? as f32 / 100.0;
    let latex = String::from_utf8(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(parts.next()?)
            .ok()?,
    )
    .ok()?;
    Some((latex, display, size))
}

fn key() -> egui::Id {
    egui::Id::unique("laankit-richtext-math-loader")
}

fn loader(ctx: &egui::Context) -> Option<Arc<MathLoader>> {
    ctx.data(|d| d.get_temp::<Arc<MathLoader>>(key()))
}

/// The image loader for `math://` URIs (see the module docs).
pub struct MathLoader {
    shared: Arc<Shared>,
}

struct Shared {
    renderer: Mutex<Option<Arc<dyn MathRenderer>>>,
    inner: Mutex<Inner>,
    jobs: Mutex<VecDeque<Job>>,
    wake: Condvar,
}

impl Shared {
    fn clear(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.cache.clear();
        }
    }
}

struct Job {
    uri: String,
    hint: SizeHint,
    tx: Sender<Result<ColorImage, String>>,
}

#[derive(Default)]
struct Inner {
    cache: HashMap<(String, SizeHint), Entry>,
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

impl MathLoader {
    pub const ID: &'static str = egui::generate_loader_id!(MathLoader);

    fn new(renderer: Arc<dyn MathRenderer>, ctx: egui::Context) -> Self {
        let shared = Arc::new(Shared {
            renderer: Mutex::new(Some(renderer)),
            inner: Mutex::new(Inner::default()),
            jobs: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
        });
        let worker = shared.clone();
        // One worker: typesetting is quick once warm, and one thread keeps a
        // long note from starting dozens of compiles at once.
        let spawned = std::thread::Builder::new()
            .name("math-render".into())
            .spawn(move || work(&worker, &ctx));
        if let Err(e) = spawned {
            log::warn!("couldn't start the math renderer thread: {e}");
        }
        Self { shared }
    }
}

fn work(shared: &Shared, ctx: &egui::Context) {
    loop {
        let job = {
            let mut jobs = shared.jobs.lock().expect("math jobs lock");
            loop {
                if let Some(job) = jobs.pop_front() {
                    break job;
                }
                jobs = shared.wake.wait(jobs).expect("math jobs lock");
            }
        };
        let renderer = shared.renderer.lock().ok().and_then(|r| r.clone());
        let result = match (renderer, parse_uri(&job.uri)) {
            (Some(r), Some((latex, display, size))) => r
                .render_svg(&latex, display, size)
                .and_then(|svg| crate::svg::render(&points_as_pixels(&svg), job.hint)),
            (None, _) => Err("no math renderer".into()),
            (_, None) => Err("bad math URI".into()),
        };
        let _ = job.tx.send(result);
        ctx.request_repaint();
    }
}

/// Typst sizes its SVG in points; a CSS renderer would scale those by 4/3.
/// The text was set at the egui text size in points, so read them as pixels
/// (egui points).
fn points_as_pixels(svg: &str) -> String {
    let Some(end) = svg.find('>') else {
        return svg.to_string();
    };
    let (head, tail) = svg.split_at(end);
    let mut head = head.to_string();
    for attr in ["width=\"", "height=\""] {
        if let Some(i) = head.find(attr) {
            let start = i + attr.len();
            if let Some(len) = head[start..].find('"') {
                let value = &head[start..start + len];
                if let Some(number) = value.strip_suffix("pt").map(str::to_string) {
                    head.replace_range(start..start + len, &number);
                }
            }
        }
    }
    head + tail
}

impl ImageLoader for MathLoader {
    fn id(&self) -> &str {
        Self::ID
    }

    fn load(&self, _ctx: &egui::Context, uri: &str, size_hint: SizeHint) -> ImageLoadResult {
        if !uri.starts_with(SCHEME) {
            return Err(LoadError::NotSupported);
        }
        let mut inner = self.shared.inner.lock().expect("math loader lock");
        let pass = inner.pass;
        let entry = inner
            .cache
            .entry((uri.to_string(), size_hint))
            .or_insert_with(|| {
                let (tx, rx) = channel();
                if let Ok(mut jobs) = self.shared.jobs.lock() {
                    jobs.push_back(Job {
                        uri: uri.to_string(),
                        hint: size_hint,
                        tx,
                    });
                    self.shared.wake.notify_one();
                }
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
                    entry.state = State::Failed("math render timed out".into())
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
        if let Ok(mut inner) = self.shared.inner.lock() {
            inner.cache.retain(|(u, _), _| u != uri);
        }
    }

    fn forget_all(&self) {
        self.shared.clear();
    }

    fn end_pass(&self, pass_index: u64) {
        if let Ok(mut inner) = self.shared.inner.lock() {
            inner.pass = pass_index;
            inner.cache.retain(|_, e| {
                matches!(e.state, State::Pending { .. })
                    || pass_index.saturating_sub(e.last_used) < EVICT_AFTER_PASSES
            });
        }
    }

    fn byte_size(&self) -> usize {
        self.shared.inner.lock().map_or(0, |inner| {
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
        self.shared.inner.lock().is_ok_and(|inner| {
            inner
                .cache
                .values()
                .any(|e| matches!(e.state, State::Pending { .. }))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sources_and_uris_round_trip() {
        let latex = "\\frac{a}{b} + é/日";
        assert_eq!(parse_image_src(&image_src(latex)).as_deref(), Some(latex));
        assert_eq!(parse_image_src("data:image/png;base64,xx"), None);
        let (l, d, s) = parse_uri(&uri(latex, MathMode::Display, 15.5)).unwrap();
        assert_eq!((l.as_str(), d, s), (latex, true, 15.5));
        assert_ne!(
            uri(latex, MathMode::Inline, 14.0),
            uri(latex, MathMode::Inline, 16.0),
            "sizes render separately"
        );
    }

    #[test]
    fn typst_points_read_as_pixels() {
        let svg = r#"<svg viewBox="0 0 18.4 14.9" width="18.4pt" height="14.9pt" xmlns="http://www.w3.org/2000/svg"><path d="M0 0"/></svg>"#;
        let out = points_as_pixels(svg);
        assert!(
            out.contains(r#"width="18.4""#) && out.contains(r#"height="14.9""#),
            "{out}"
        );
        assert!(out.ends_with("<path d=\"M0 0\"/></svg>"));
    }

    #[test]
    fn without_a_renderer_math_is_unavailable() {
        let ctx = egui::Context::default();
        assert!(!is_installed(&ctx));
        assert!(matches!(
            image(&ctx, "x^2", MathMode::Inline, 14.0),
            MathImage::Unavailable
        ));
    }
}
