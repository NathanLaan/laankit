//! One slide on its fixed stage (1920 × 1080 for 16:9), scaled into any rect:
//! the native `Slide.svelte`. The thumbnails, the deck preview and the
//! presenter all draw through [`SlideView`], so a slide looks the same at
//! every size.
//!
//! Block Markdown goes through comrak → HTML → `marina_richtext`'s block
//! model (the same inline Markdown the note preview supports), cached per
//! block text with egui's [`FrameCache`]. Sizes are the Svelte stage's CSS
//! pixels times the scale; the slide themes are independent of the app
//! theme, as in the Svelte build, so a deck looks the same to its audience
//! whatever the presenter's UI theme.

use std::collections::HashMap;
use std::sync::Arc;

use egui::cache::{ComputerMut, FrameCache};
use egui::load::TexturePoll;
use egui::text::{LayoutJob, TextFormat};
use egui::{
    pos2, vec2, Align, Color32, CornerRadius, FontId, Galley, Painter, Pos2, Rect, Response, Sense,
    Stroke, StrokeKind, TextureId, Ui, Vec2,
};
use marina_egui::{fonts, icons};
use marina_richtext::{Block as RichBlock, Span, TextKind};

use crate::color;
use crate::deck::{substitute, Block, BlockKind, DirValue, Layout, Presentation, Slide};

/// `padding: 96px 120px` on `.slide-stage`.
const PAD_Y: f32 = 96.0;
const PAD_X: f32 = 120.0;
/// `gap: 48px` between the heading and the body, and between body blocks.
const GAP: f32 = 48.0;
/// `.layout-split .slide-body { gap: 72px }` / the grid's column gap.
const COLUMN_GAP: f32 = 72.0;
const BAND_SIZE: f32 = 28.0;
const BAND_PAD: f32 = 32.0;
const RULE: f32 = 2.0;
const PARAGRAPH_GAP: f32 = 24.0;
const ITEM_GAP: f32 = 20.0;
const LINE_HEIGHT: f32 = 1.4;
const CODE_SIZE: f32 = 34.0;
const CODE_PAD: f32 = 32.0;
const TABLE_SIZE: f32 = 38.0;
const CELL_PAD_Y: f32 = 20.0;
const CELL_PAD_X: f32 = 24.0;
const QUOTE_BAR: f32 = 8.0;
const QUOTE_PAD: f32 = 40.0;
/// Glyphs smaller than this aren't worth shaping (and size 0 isn't allowed).
const MIN_FONT: f32 = 0.5;

/// A slide theme's colours (`.theme-dark` / `.theme-light`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideTheme {
    pub bg: Color32,
    pub fg: Color32,
    pub muted: Color32,
    pub accent: Color32,
    pub rule: Color32,
    pub code_bg: Color32,
}

impl SlideTheme {
    pub const DARK: SlideTheme = SlideTheme {
        bg: Color32::from_rgb(0x14, 0x18, 0x1d),
        fg: Color32::from_rgb(0xf2, 0xf5, 0xf8),
        muted: Color32::from_rgb(0x8b, 0x98, 0xa5),
        accent: Color32::from_rgb(0x4d, 0xa3, 0xff),
        rule: Color32::from_rgb(0x2b, 0x33, 0x3d),
        code_bg: Color32::from_rgb(0x1e, 0x24, 0x2b),
    };
    pub const LIGHT: SlideTheme = SlideTheme {
        bg: Color32::WHITE,
        fg: Color32::from_rgb(0x16, 0x19, 0x1d),
        muted: Color32::from_rgb(0x5d, 0x66, 0x73),
        accent: Color32::from_rgb(0x0b, 0x62, 0xc4),
        rule: Color32::from_rgb(0xdf, 0xe4, 0xea),
        code_bg: Color32::from_rgb(0xf2, 0xf4, 0xf7),
    };

    /// The theme named in the deck's config; unknown names are dark, as the
    /// Svelte CSS (only `.theme-dark` and `.theme-light` exist) falls back.
    pub fn named(name: &str) -> SlideTheme {
        if name == "light" {
            Self::LIGHT
        } else {
            Self::DARK
        }
    }
}

/// Turns an image reference in a slide (`![](…)`) into a URI egui's image
/// loaders can load. The host maps attachments (`_attachments/x.png`) to a
/// `file://` or `bytes://` URI; `None` draws a placeholder with the alt text.
pub trait ImageSource {
    fn uri(&self, src: &str) -> Option<String>;
}

/// Passes URLs and URIs through; relative paths get no image.
#[derive(Clone, Copy, Debug, Default)]
pub struct PassThrough;

impl ImageSource for PassThrough {
    fn uri(&self, src: &str) -> Option<String> {
        let lower = src.to_ascii_lowercase();
        ["http://", "https://", "file://", "bytes://", "data:"]
            .iter()
            .any(|p| lower.starts_with(p))
            .then(|| src.to_string())
    }
}

impl<F: Fn(&str) -> Option<String>> ImageSource for F {
    fn uri(&self, src: &str) -> Option<String> {
        self(src)
    }
}

/// One slide, drawn into a rect of the deck's aspect ratio.
pub struct SlideView<'a> {
    slide: &'a Slide,
    config: &'a Presentation,
    total: usize,
    vars: Option<&'a HashMap<String, String>>,
    images: &'a dyn ImageSource,
}

impl<'a> SlideView<'a> {
    pub fn new(slide: &'a Slide, config: &'a Presentation) -> Self {
        Self {
            slide,
            config,
            total: 1,
            vars: None,
            images: &PassThrough,
        }
    }

    /// The number of slides (`{{total}}`).
    pub fn total(mut self, total: usize) -> Self {
        self.total = total;
        self
    }

    /// Extra header/footer placeholder values (`name`, `title`, `date`,
    /// `time`, `datetime` in the Svelte preview). `page` and `total` are filled in.
    pub fn vars(mut self, vars: &'a HashMap<String, String>) -> Self {
        self.vars = Some(vars);
        self
    }

    pub fn images(mut self, images: &'a dyn ImageSource) -> Self {
        self.images = images;
        self
    }

    /// The stage's aspect, height over width.
    pub fn aspect(&self) -> f32 {
        let stage = crate::deck::stage_size(&self.config.aspect);
        stage.h as f32 / stage.w as f32
    }

    /// Allocates `width` × the stage's aspect and paints the slide there.
    pub fn show(&self, ui: &mut Ui, width: f32) -> Response {
        let (rect, resp) =
            ui.allocate_exact_size(vec2(width, width * self.aspect()), Sense::click());
        if ui.is_rect_visible(rect) {
            self.paint(ui, rect);
        }
        resp
    }

    /// Paints the slide into `rect` (the stage is scaled to its width).
    pub fn paint(&self, ui: &Ui, rect: Rect) {
        let stage = crate::deck::stage_size(&self.config.aspect);
        let s = rect.width() / stage.w as f32;
        let theme = SlideTheme::named(&self.config.theme);
        let painter = ui.painter_at(rect);
        let bg = self
            .slide
            .directives
            .get_str("bg")
            .and_then(color::parse)
            .unwrap_or(theme.bg);
        painter.rect_filled(rect, 0.0, theme.bg);
        painter.rect_filled(rect, 0.0, bg);
        let cx = Cx {
            ui,
            s,
            theme,
            images: self.images,
        };
        let laid = self.layout(&cx, rect.size());
        laid.paint(&painter, rect.min, theme.fg);
    }

    /// Everything on the slide, positioned relative to its top-left corner.
    fn layout(&self, cx: &Cx, size: Vec2) -> Laid {
        let s = cx.s;
        let slide = self.slide;
        let layout = slide.layout;
        let full_bleed = layout == Layout::FullBleed;
        let mut out = Laid::default();

        let (pad_x, pad_y) = if full_bleed {
            (0.0, 0.0)
        } else {
            (PAD_X * s, PAD_Y * s)
        };
        let mut top = pad_y;
        let mut bottom = size.y - pad_y;
        let inner = size.x - 2.0 * pad_x;

        // Bands (hidden on full-bleed slides, which the picture owns).
        let placeholders = self.placeholders();
        let band_text = |key: &str, fallback: &str| {
            let raw = match slide.directives.get(key) {
                Some(DirValue::Str(v)) => v.clone(),
                Some(DirValue::Flag) => "true".to_string(),
                None => fallback.to_string(),
            };
            substitute(&raw, &placeholders)
        };
        let header = band_text("header", &self.config.header);
        let footer = band_text("footer", &self.config.footer);
        if !full_bleed && !header.is_empty() {
            let g = cx.single_line(&header, BAND_SIZE * s, cx.theme.muted, inner);
            let h = g.size().y;
            out.text(pos2(pad_x, top), g);
            let rule_y = top + h + BAND_PAD * s;
            out.fill(
                Rect::from_min_size(pos2(pad_x, rule_y), vec2(inner, RULE * s)),
                cx.theme.rule,
            );
            top = rule_y + RULE * s + GAP * s;
        }
        if !full_bleed && (!footer.is_empty() || self.config.slide_numbers) {
            let size_px = BAND_SIZE * s;
            let number = self
                .config
                .slide_numbers
                .then(|| cx.single_line(&slide.index.to_string(), size_px, cx.theme.muted, inner));
            let number_w = number.as_ref().map_or(0.0, |g| g.size().x + 24.0 * s);
            let text = (!footer.is_empty())
                .then(|| cx.single_line(&footer, size_px, cx.theme.muted, inner - number_w));
            let h = text
                .as_ref()
                .map_or(0.0, |g| g.size().y)
                .max(number.as_ref().map_or(0.0, |g| g.size().y));
            let text_y = bottom - h;
            let rule_y = text_y - BAND_PAD * s - RULE * s;
            out.fill(
                Rect::from_min_size(pos2(pad_x, rule_y), vec2(inner, RULE * s)),
                cx.theme.rule,
            );
            if let Some(g) = text {
                out.text(pos2(pad_x, text_y), g);
            }
            if let Some(g) = number {
                out.text(pos2(pad_x + inner - g.size().x, text_y), g);
            }
            bottom = rule_y - GAP * s;
        }

        let content = Rect::from_min_max(pos2(pad_x, top), pos2(pad_x + inner, bottom.max(top)));
        let heading = self.layout_headings(cx, content.width());
        let body_style = body_style(layout, &cx.theme);
        let headings_h = heading.h;
        let gap = if heading.items.is_empty() {
            0.0
        } else {
            GAP * s
        };
        let body_top_free = content.height() - headings_h - gap;
        let body = self.layout_body(cx, content.width(), body_top_free.max(0.0), &body_style);
        let body_h = body.h;

        let centred = matches!(layout, Layout::Hero | Layout::Quote);
        let total = headings_h
            + if body.items.is_empty() {
                0.0
            } else {
                gap + body_h
            };
        let y0 = if centred && total < content.height() {
            content.top() + (content.height() - total) / 2.0
        } else {
            content.top()
        };
        out.place(heading, vec2(content.left(), y0));
        out.place(body, vec2(content.left(), y0 + headings_h + gap));
        out
    }

    fn placeholders(&self) -> HashMap<String, String> {
        let mut vars = HashMap::from([
            ("page".to_string(), self.slide.index.to_string()),
            ("total".to_string(), self.total.to_string()),
        ]);
        if let Some(extra) = self.vars {
            vars.extend(extra.iter().map(|(k, v)| (k.clone(), v.clone())));
        }
        vars
    }

    fn layout_headings(&self, cx: &Cx, width: f32) -> Laid {
        let mut out = Laid::default();
        let hero = self.slide.layout == Layout::Hero;
        for block in self.slide.headings() {
            let level = block.level.unwrap_or(1).clamp(1, 6);
            let (size, lh) = match level {
                1 | 2 if hero => (132.0, 1.05),
                1 => (104.0, 1.1),
                2 => (80.0, 1.15),
                3 => (60.0, 1.2),
                _ => (48.0, 1.2),
            };
            let spans = inline_spans(cx.ui, &block.text);
            let style = TextStyle {
                size: size * cx.s,
                line_height: lh,
                color: cx.theme.fg,
                weight: Some(fonts::SEMIBOLD),
                italic: false,
            };
            let g = cx.text(&spans, &style, width, Align::LEFT);
            out.text(pos2(0.0, out.h), g.clone());
            out.h += g.size().y;
        }
        out
    }

    fn layout_body(&self, cx: &Cx, width: f32, height: f32, style: &BodyStyle) -> Laid {
        let s = cx.s;
        let blocks: Vec<&Block> = self.slide.body().collect();
        let mut out = Laid::default();
        if blocks.is_empty() {
            return out;
        }
        match self.slide.layout {
            Layout::Split => {
                let n = blocks.len() as f32;
                let col_w = (width - COLUMN_GAP * s * (n - 1.0)) / n;
                for (i, block) in blocks.iter().enumerate() {
                    let laid = self.layout_block(cx, block, col_w, height, style);
                    let h = laid.h;
                    out.place(laid, vec2(i as f32 * (col_w + COLUMN_GAP * s), 0.0));
                    out.h = out.h.max(h);
                }
            }
            Layout::Grid => {
                let col_w = (width - COLUMN_GAP * s) / 2.0;
                let mut y = 0.0;
                for row in blocks.chunks(2) {
                    let mut row_h: f32 = 0.0;
                    for (i, block) in row.iter().enumerate() {
                        let laid = self.layout_block(cx, block, col_w, height - y, style);
                        row_h = row_h.max(laid.h);
                        out.place(laid, vec2(i as f32 * (col_w + COLUMN_GAP * s), y));
                    }
                    y += row_h + GAP * s;
                }
                out.h = (y - GAP * s).max(0.0);
            }
            Layout::FullBleed if blocks.len() == 1 && is_picture(blocks[0]) => {
                let block = blocks[0];
                let rect = Rect::from_min_size(Pos2::ZERO, vec2(width, height));
                if block.kind == BlockKind::Image {
                    out.image(
                        cx,
                        rect,
                        block.src.as_deref(),
                        block.alt.as_deref(),
                        Fit::Cover,
                    );
                } else {
                    let laid = video_card(cx, block, width, style);
                    out.h = laid.h;
                    out.place(laid, Vec2::ZERO);
                }
                out.h = height;
            }
            _ => {
                let mut y = 0.0;
                for (i, block) in blocks.iter().enumerate() {
                    if i > 0 {
                        y += GAP * s;
                    }
                    let laid = self.layout_block(cx, block, width, (height - y).max(0.0), style);
                    let h = laid.h;
                    out.place(laid, vec2(0.0, y));
                    y += h;
                }
                out.h = y;
            }
        }
        out
    }

    /// One body block, `width` wide; pictures are capped at `max_h`.
    fn layout_block(
        &self,
        cx: &Cx,
        block: &Block,
        width: f32,
        max_h: f32,
        style: &BodyStyle,
    ) -> Laid {
        match block.kind {
            BlockKind::Image => {
                let mut out = Laid::default();
                let size = block
                    .src
                    .as_deref()
                    .and_then(|src| cx.images.uri(src))
                    .and_then(|uri| texture(cx.ui, &uri))
                    .map(|(_, size)| size);
                let rect = contain(size, vec2(width, max_h.max(1.0)), cx.s);
                out.image(
                    cx,
                    rect,
                    block.src.as_deref(),
                    block.alt.as_deref(),
                    Fit::Contain,
                );
                out.h = rect.height();
                out
            }
            BlockKind::Video => video_card(cx, block, width, style),
            _ => {
                let rich = rich_blocks(cx.ui, &block.text);
                layout_rich(cx, &rich, width, max_h, style)
            }
        }
    }
}

fn is_picture(block: &Block) -> bool {
    matches!(block.kind, BlockKind::Image | BlockKind::Video)
}

/// What a layout does to the body text (`.layout-* .slide-body`).
struct BodyStyle {
    text: TextStyle,
    /// `.layout-quote`: block quotes are big italics with no bar.
    plain_quotes: bool,
    theme: SlideTheme,
}

fn body_style(layout: Layout, theme: &SlideTheme) -> BodyStyle {
    let (size, color) = match layout {
        Layout::Hero => (52.0, theme.muted),
        Layout::Grid => (38.0, theme.fg),
        Layout::Quote => (64.0, theme.fg),
        _ => (44.0, theme.fg),
    };
    BodyStyle {
        text: TextStyle {
            size,
            line_height: LINE_HEIGHT,
            color,
            weight: None,
            italic: false,
        },
        plain_quotes: layout == Layout::Quote,
        theme: *theme,
    }
}

#[derive(Clone, Copy)]
struct TextStyle {
    /// In stage px for body styles; already scaled for [`Cx::text`].
    size: f32,
    line_height: f32,
    color: Color32,
    weight: Option<f32>,
    italic: bool,
}

/// Painting context: the ui (fonts, textures), the scale and the theme.
struct Cx<'a> {
    ui: &'a Ui,
    s: f32,
    theme: SlideTheme,
    images: &'a dyn ImageSource,
}

impl Cx<'_> {
    /// Rich spans laid out at `style` (size already scaled), wrapped at `width`.
    fn text(&self, spans: &[Span], style: &TextStyle, width: f32, align: Align) -> Arc<Galley> {
        let size = style.size.max(MIN_FONT);
        let mut job = LayoutJob::default();
        job.wrap.max_width = width.max(1.0);
        job.halign = align;
        for span in spans {
            let st = span.style;
            let font_style = fonts::Style {
                weight: if st.bold {
                    Some(fonts::BOLD)
                } else {
                    style.weight
                },
                italic: st.italic || style.italic,
                mono: st.code,
            };
            let mut span_size = size;
            if st.code {
                span_size *= 0.9;
            }
            if st.small || st.sup || st.sub {
                span_size *= 0.8;
            }
            let color = if span.link.is_some() {
                self.theme.accent
            } else {
                style.color
            };
            let mut format: TextFormat = fonts::format(font_style, span_size.max(MIN_FONT), color);
            format.line_height = Some(size * style.line_height);
            if st.code {
                format.background = self.theme.code_bg;
            }
            if st.strike {
                format.strikethrough = Stroke::new((size / 16.0).max(0.5), color);
            }
            if st.underline {
                format.underline = Stroke::new((size / 16.0).max(0.5), color);
            }
            job.append(&span.text, 0.0, format);
        }
        if job.sections.is_empty() {
            job.append(
                "",
                0.0,
                fonts::format(fonts::Style::default(), size, style.color),
            );
        }
        self.ui.ctx().fonts_mut(|f| f.layout_job(job))
    }

    /// One line of plain text, truncated to `width`.
    fn single_line(&self, text: &str, size: f32, color: Color32, width: f32) -> Arc<Galley> {
        let mut job = LayoutJob::single_section(
            text.to_string(),
            fonts::format(fonts::Style::default(), size.max(MIN_FONT), color),
        );
        job.wrap.max_width = width.max(1.0);
        job.wrap.max_rows = 1;
        job.wrap.break_anywhere = true;
        self.ui.ctx().fonts_mut(|f| f.layout_job(job))
    }

    fn icon(&self, glyph: &str, size: f32, color: Color32) -> Arc<Galley> {
        let job = LayoutJob::simple_singleline(
            glyph.to_string(),
            FontId::new(size.max(MIN_FONT), icons::font(size).family),
            color,
        );
        self.ui.ctx().fonts_mut(|f| f.layout_job(job))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fit {
    Contain,
    /// `object-fit: cover`: fill the rect, cropping the image.
    Cover,
}

enum Item {
    Text(Pos2, Arc<Galley>),
    Fill(Rect, CornerRadius, Color32),
    Frame(Rect, CornerRadius, Stroke),
    Image {
        rect: Rect,
        uri: Option<String>,
        alt: String,
        fit: Fit,
    },
}

/// Laid-out items relative to an origin, and the height they take.
#[derive(Default)]
struct Laid {
    items: Vec<Item>,
    h: f32,
}

impl Laid {
    fn text(&mut self, pos: Pos2, galley: Arc<Galley>) {
        self.items.push(Item::Text(pos, galley));
    }

    fn fill(&mut self, rect: Rect, color: Color32) {
        self.items.push(Item::Fill(rect, CornerRadius::ZERO, color));
    }

    fn image(&mut self, cx: &Cx, rect: Rect, src: Option<&str>, alt: Option<&str>, fit: Fit) {
        self.items.push(Item::Image {
            rect,
            uri: src.and_then(|s| cx.images.uri(s)),
            alt: alt.unwrap_or_default().to_string(),
            fit,
        });
    }

    /// Adds `other`'s items moved by `offset` (its height isn't added).
    fn place(&mut self, other: Laid, offset: Vec2) {
        for item in other.items {
            self.items.push(match item {
                Item::Text(p, g) => Item::Text(p + offset, g),
                Item::Fill(r, c, col) => Item::Fill(r.translate(offset), c, col),
                Item::Frame(r, c, st) => Item::Frame(r.translate(offset), c, st),
                Item::Image {
                    rect,
                    uri,
                    alt,
                    fit,
                } => Item::Image {
                    rect: rect.translate(offset),
                    uri,
                    alt,
                    fit,
                },
            });
        }
    }

    fn paint(self, painter: &Painter, origin: Pos2, fallback: Color32) {
        let offset = origin.to_vec2();
        let ui_ctx = painter.ctx().clone();
        for item in self.items {
            match item {
                Item::Fill(r, c, col) => {
                    painter.rect_filled(r.translate(offset), c, col);
                }
                Item::Frame(r, c, st) => {
                    painter.rect_stroke(r.translate(offset), c, st, StrokeKind::Inside);
                }
                Item::Text(p, g) => {
                    painter.galley(p + offset, g, fallback);
                }
                Item::Image {
                    rect,
                    uri,
                    alt,
                    fit,
                } => paint_image(
                    &ui_ctx,
                    painter,
                    rect.translate(offset),
                    uri.as_deref(),
                    &alt,
                    fit,
                ),
            }
        }
    }
}

fn paint_image(
    ctx: &egui::Context,
    painter: &Painter,
    rect: Rect,
    uri: Option<&str>,
    alt: &str,
    fit: Fit,
) {
    let loaded = uri.and_then(|u| texture_in(ctx, u));
    match loaded {
        Some((id, size)) => {
            let uv = match fit {
                Fit::Contain => Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Fit::Cover => cover_uv(size, rect.size()),
            };
            painter.image(id, rect, uv, Color32::WHITE);
        }
        None => {
            // A placeholder: a framed box with the alt text.
            let p = painter.with_clip_rect(rect.intersect(painter.clip_rect()));
            p.rect_filled(rect, 0.0, Color32::from_black_alpha(40));
            p.rect_stroke(
                rect,
                0.0,
                Stroke::new(1.0, Color32::from_gray(128)),
                StrokeKind::Inside,
            );
            if !alt.is_empty() {
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    alt,
                    FontId::proportional((rect.height() / 8.0).clamp(MIN_FONT, 28.0)),
                    Color32::from_gray(150),
                );
            }
        }
    }
}

fn texture(ui: &Ui, uri: &str) -> Option<(TextureId, Vec2)> {
    texture_in(ui.ctx(), uri)
}

fn texture_in(ctx: &egui::Context, uri: &str) -> Option<(TextureId, Vec2)> {
    match egui::Image::new(uri.to_string()).load_for_size(ctx, vec2(2048.0, 2048.0)) {
        Ok(TexturePoll::Ready { texture }) => Some((texture.id, texture.size)),
        _ => None,
    }
}

/// `max-width: 100%; max-height: 100%`: the image at its natural size on the
/// stage (`s` screen px per stage px), shrunk to fit `bounds`, top-left
/// aligned. An image not loaded yet gets a 16:9 box filling the width.
fn contain(size: Option<Vec2>, bounds: Vec2, s: f32) -> Rect {
    let fit = |natural: Vec2| (bounds.x / natural.x).min(bounds.y / natural.y).max(0.0);
    let rect_size = match size {
        Some(natural) => natural * fit(natural).min(s),
        None => vec2(16.0, 9.0) * fit(vec2(16.0, 9.0)),
    };
    Rect::from_min_size(Pos2::ZERO, rect_size)
}

/// The part of an image (in uv) that covers a rect of `target` size.
fn cover_uv(image: Vec2, target: Vec2) -> Rect {
    let image_aspect = image.x / image.y;
    let target_aspect = target.x / target.y.max(1e-3);
    if image_aspect > target_aspect {
        let w = target_aspect / image_aspect;
        Rect::from_min_max(pos2((1.0 - w) / 2.0, 0.0), pos2((1.0 + w) / 2.0, 1.0))
    } else {
        let h = image_aspect / target_aspect;
        Rect::from_min_max(pos2(0.0, (1.0 - h) / 2.0), pos2(1.0, (1.0 + h) / 2.0))
    }
}

/// A link card for a video (`.slide-video-link`); playback isn't native yet.
fn video_card(cx: &Cx, block: &Block, width: f32, style: &BodyStyle) -> Laid {
    let s = cx.s;
    let mut out = Laid::default();
    let label = block
        .alt
        .as_deref()
        .filter(|a| !a.is_empty())
        .or(block.src.as_deref())
        .unwrap_or_default();
    let size = style.text.size * s;
    let icon = cx.icon(icons::CIRCLE_PLAY, size, style.theme.muted);
    let text_style = TextStyle {
        size,
        color: style.theme.muted,
        ..style.text
    };
    let text_w = width - 80.0 * s - icon.size().x - 24.0 * s;
    let text = cx.text(&[plain_span(label)], &text_style, text_w, Align::LEFT);
    let inner_h = icon.size().y.max(text.size().y);
    let h = inner_h + 64.0 * s;
    out.items.push(Item::Frame(
        Rect::from_min_size(Pos2::ZERO, vec2(width, h)),
        CornerRadius::same((12.0 * s).round() as u8),
        Stroke::new(RULE * s, style.theme.rule),
    ));
    let icon_y = 32.0 * s + (inner_h - icon.size().y) / 2.0;
    let icon_w = icon.size().x;
    out.text(pos2(40.0 * s, icon_y), icon);
    out.text(
        pos2(
            40.0 * s + icon_w + 24.0 * s,
            32.0 * s + (inner_h - text.size().y) / 2.0,
        ),
        text,
    );
    out.h = h;
    out
}

fn plain_span(text: &str) -> Span {
    Span {
        text: text.to_string(),
        style: Default::default(),
        link: None,
    }
}

/// Rich blocks laid out top to bottom, `width` wide.
fn layout_rich(cx: &Cx, blocks: &[RichBlock], width: f32, max_h: f32, style: &BodyStyle) -> Laid {
    let s = cx.s;
    let mut out = Laid::default();
    let mut y = 0.0;
    for (i, block) in blocks.iter().enumerate() {
        let last = i + 1 == blocks.len();
        let laid = layout_rich_block(cx, block, width, (max_h - y).max(0.0), style);
        let h = laid.h;
        out.place(laid, vec2(0.0, y));
        y += h;
        if !last && matches!(block, RichBlock::Text { .. }) {
            y += PARAGRAPH_GAP * s;
        }
    }
    out.h = y;
    out
}

fn layout_rich_block(
    cx: &Cx,
    block: &RichBlock,
    width: f32,
    max_h: f32,
    style: &BodyStyle,
) -> Laid {
    let s = cx.s;
    let mut out = Laid::default();
    let body = TextStyle {
        size: style.text.size * s,
        ..style.text
    };
    match block {
        RichBlock::Text {
            kind, spans, align, ..
        } => {
            let text_style = match kind {
                TextKind::Heading(level) => TextStyle {
                    size: match level {
                        1 => 104.0,
                        2 => 80.0,
                        3 => 60.0,
                        _ => 48.0,
                    } * s,
                    line_height: 1.15,
                    weight: Some(fonts::SEMIBOLD),
                    ..body
                },
                TextKind::Caption => TextStyle {
                    size: body.size * 0.8,
                    color: style.theme.muted,
                    ..body
                },
                TextKind::Paragraph => body,
            };
            let align = match align {
                marina_richtext::Align::Center => Align::Center,
                marina_richtext::Align::Right => Align::RIGHT,
                _ => Align::LEFT,
            };
            let g = cx.text(spans, &text_style, width, align);
            out.h = g.size().y;
            out.text(Pos2::ZERO, g);
        }
        RichBlock::List {
            ordered,
            start,
            items,
        } => {
            // `ul, ol { padding-left: 1.4em }`, `li { margin-bottom: 20px }`.
            let indent = 1.4 * body.size;
            let mut y = 0.0;
            for (k, item) in items.iter().enumerate() {
                let marker = if *ordered {
                    format!("{}.", *start as usize + k)
                } else {
                    "•".to_string()
                };
                let mg = cx.text(&[plain_span(&marker)], &body, indent, Align::LEFT);
                let content = layout_rich(cx, item, width - indent, (max_h - y).max(0.0), style);
                let h = content.h.max(mg.size().y);
                let mx = (indent - mg.size().x - 0.3 * body.size).max(0.0);
                out.text(pos2(mx, y), mg);
                out.place(content, vec2(indent, y));
                y += h;
                if k + 1 < items.len() {
                    y += ITEM_GAP * s;
                }
            }
            out.h = y;
        }
        RichBlock::Quote(inner) => {
            if style.plain_quotes {
                let quote = BodyStyle {
                    text: TextStyle {
                        italic: true,
                        ..style.text
                    },
                    plain_quotes: true,
                    theme: style.theme,
                };
                return layout_rich(cx, inner, width, max_h, &quote);
            }
            let pad = QUOTE_PAD * s;
            let content = layout_rich(cx, inner, width - pad, max_h, style);
            out.fill(
                Rect::from_min_size(Pos2::ZERO, vec2(QUOTE_BAR * s, content.h)),
                style.theme.accent,
            );
            out.h = content.h;
            out.place(content, vec2(pad, 0.0));
        }
        RichBlock::Code(code) => {
            let pad = CODE_PAD * s;
            let mut span = plain_span(code.trim_end_matches('\n'));
            span.style.code = true;
            let code_style = TextStyle {
                size: CODE_SIZE * s / 0.9,
                color: style.theme.fg,
                ..body
            };
            let g = cx.text(&[span], &code_style, width - 2.0 * pad, Align::LEFT);
            let h = g.size().y + 2.0 * pad;
            out.items.push(Item::Fill(
                Rect::from_min_size(Pos2::ZERO, vec2(width, h)),
                CornerRadius::same((12.0 * s).round() as u8),
                style.theme.code_bg,
            ));
            out.text(pos2(pad, pad), g);
            out.h = h;
        }
        RichBlock::Image { src, alt, .. } => {
            let size = cx
                .images
                .uri(src)
                .and_then(|u| texture(cx.ui, &u))
                .map(|(_, s)| s);
            let rect = contain(size, vec2(width, max_h.max(1.0)), cx.s);
            out.image(cx, rect, Some(src), Some(alt), Fit::Contain);
            out.h = rect.height();
        }
        RichBlock::Rule => {}
        RichBlock::Table { rows, header_rows } => {
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
            let col_w = width / cols as f32;
            let table = TextStyle {
                size: TABLE_SIZE * s,
                ..body
            };
            let mut y = 0.0;
            for (r, row) in rows.iter().enumerate() {
                let header = r < *header_rows;
                let cell_style = if header {
                    TextStyle {
                        color: style.theme.muted,
                        weight: Some(fonts::SEMIBOLD),
                        ..table
                    }
                } else {
                    table
                };
                let galleys: Vec<Arc<Galley>> = row
                    .iter()
                    .map(|cell| {
                        cx.text(cell, &cell_style, col_w - 2.0 * CELL_PAD_X * s, Align::LEFT)
                    })
                    .collect();
                let row_h =
                    galleys.iter().map(|g| g.size().y).fold(0.0, f32::max) + 2.0 * CELL_PAD_Y * s;
                for (c, g) in galleys.into_iter().enumerate() {
                    out.text(
                        pos2(c as f32 * col_w + CELL_PAD_X * s, y + CELL_PAD_Y * s),
                        g,
                    );
                }
                out.fill(
                    Rect::from_min_size(pos2(0.0, y + row_h - RULE * s), vec2(width, RULE * s)),
                    style.theme.rule,
                );
                y += row_h;
            }
            out.h = y;
        }
    }
    out
}

// ─── Markdown → rich blocks ──────────────────────────────────────────────────

/// A slide block's Markdown as rich blocks, cached while it's on screen.
fn rich_blocks(ui: &Ui, markdown: &str) -> Arc<Vec<RichBlock>> {
    ui.ctx()
        .memory_mut(|m| m.caches.cache::<MarkdownCache>().get(markdown).clone())
}

/// A heading's inline Markdown as spans.
fn inline_spans(ui: &Ui, text: &str) -> Vec<Span> {
    let blocks = rich_blocks(ui, text);
    match blocks.first() {
        Some(RichBlock::Text { spans, .. }) => spans.clone(),
        _ => vec![plain_span(text)],
    }
}

type MarkdownCache = FrameCache<Arc<Vec<RichBlock>>, MarkdownToBlocks>;

#[derive(Default)]
struct MarkdownToBlocks;

impl ComputerMut<&str, Arc<Vec<RichBlock>>> for MarkdownToBlocks {
    fn compute(&mut self, markdown: &str) -> Arc<Vec<RichBlock>> {
        Arc::new(markdown_blocks(markdown))
    }
}

/// Markdown → HTML (comrak, with GFM tables, strikethrough, autolinks and
/// task lists, as `marked` renders them) → `marina_richtext` blocks.
pub fn markdown_blocks(markdown: &str) -> Vec<RichBlock> {
    let mut options = comrak::Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.tasklist = true;
    options.render.unsafe_ = true;
    let html = comrak::markdown_to_html(markdown, &options);
    marina_richtext::from_html(&html, &marina_richtext::Options::default()).blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_rich_blocks() {
        let blocks = markdown_blocks("- **one**\n- two");
        assert!(matches!(blocks.as_slice(), [RichBlock::List { items, .. }] if items.len() == 2));
        let table = markdown_blocks("| a | b |\n|---|---|\n| 1 | 2 |");
        assert!(matches!(
            table.as_slice(),
            [RichBlock::Table { header_rows: 1, .. }]
        ));
    }

    #[test]
    fn cover_crops_and_contain_fits() {
        let uv = cover_uv(vec2(200.0, 100.0), vec2(100.0, 100.0));
        assert!((uv.width() - 0.5).abs() < 1e-6 && uv.height() == 1.0);
        let r = contain(Some(vec2(200.0, 100.0)), vec2(100.0, 100.0), 1.0);
        assert_eq!(r.size(), vec2(100.0, 50.0));
        // Never larger than its natural size on the stage.
        let small = contain(Some(vec2(20.0, 10.0)), vec2(100.0, 100.0), 0.5);
        assert_eq!(small.size(), vec2(10.0, 5.0));
    }

    #[test]
    fn themes_and_passthrough() {
        assert_eq!(SlideTheme::named("light"), SlideTheme::LIGHT);
        assert_eq!(SlideTheme::named("neon"), SlideTheme::DARK);
        assert_eq!(PassThrough.uri("img/a.png"), None);
        assert!(PassThrough.uri("https://x/a.png").is_some());
    }
}
