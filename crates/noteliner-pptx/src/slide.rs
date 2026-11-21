//! One slide: its parts placed in boxes, in stage pixels, and the boxes as
//! shapes. The boxes follow `noteliner_slides::render`'s layout, with the
//! height of text estimated, since PowerPoint wraps the text itself.

use std::collections::HashMap;

use laankit_richtext::math::{self, MathMode};
use laankit_richtext::{Align, Block as RichBlock, Span};
use noteliner_slides::render::{self, class_overrides, markdown_blocks, SlideTheme};
use noteliner_slides::{substitute, Block, BlockKind, Deck, DirValue, Layout, Slide};

use crate::text::{self, rgb, Flow, Fonts, Para, Run};
use crate::xml::{emu, esc, hex, Rels, Rgb, NS, REL_IMAGE, TREE_HEAD};
use crate::{Assets, Formula, Picture};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    fn new(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect {
            x,
            y,
            w: w.max(0.0),
            h: h.max(0.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    Top,
    Middle,
    Bottom,
}

/// A table cell: its text and its alignment.
pub struct Cell {
    pub para: Para,
}

pub enum Shape {
    Text {
        rect: Rect,
        paras: Vec<Para>,
        fill: Option<Rgb>,
        line: Option<Rgb>,
        radius: f32,
        /// Left, top, right, bottom.
        inset: [f32; 4],
        anchor: Anchor,
        title: bool,
        name: &'static str,
    },
    Fill {
        rect: Rect,
        color: Rgb,
    },
    Picture {
        rect: Rect,
        /// The picture's place in the file's media.
        media: usize,
        alt: String,
        /// The part cut from the left, top, right and bottom, as fractions.
        crop: [f32; 4],
        radius: f32,
    },
    Table {
        rect: Rect,
        rows: Vec<Vec<Cell>>,
        row_heights: Vec<f32>,
        header_rows: usize,
        rule: Rgb,
    },
}

impl Shape {
    pub fn rect(&self) -> Rect {
        match self {
            Shape::Text { rect, .. }
            | Shape::Fill { rect, .. }
            | Shape::Picture { rect, .. }
            | Shape::Table { rect, .. } => *rect,
        }
    }
}

/// The pictures a file holds, each once.
#[derive(Default)]
pub struct Media {
    pub items: Vec<Picture>,
    by_key: HashMap<String, usize>,
}

impl Media {
    fn add(&mut self, key: String, picture: &Picture) -> usize {
        if let Some(i) = self.by_key.get(&key) {
            return *i;
        }
        self.items.push(picture.clone());
        self.by_key.insert(key, self.items.len() - 1);
        self.items.len() - 1
    }
}

/// A part of a slide's body before it has a place.
enum Piece {
    Text(Vec<Para>),
    Quote(Vec<Para>),
    Code(Vec<Para>),
    Table {
        rows: Vec<Vec<Vec<Span>>>,
        header_rows: usize,
    },
    Picture {
        media: usize,
        natural: (f32, f32),
        alt: String,
        centre: bool,
    },
    /// A picture that could not be read: its alt text and address.
    Missing {
        alt: String,
        src: String,
    },
    Video {
        label: String,
        src: String,
    },
}

pub struct Built {
    pub shapes: Vec<Shape>,
    pub bg: Rgb,
}

pub struct Cx<'a> {
    pub deck: &'a Deck,
    pub assets: &'a Assets,
    pub vars: &'a HashMap<String, String>,
    pub media: &'a mut Media,
    pub warnings: &'a mut Vec<String>,
}

pub fn theme_of(slide: &Slide, deck: &Deck) -> SlideTheme {
    class_overrides(slide)
        .0
        .unwrap_or_else(|| SlideTheme::named(&deck.config.theme))
}

pub fn layout_of(slide: &Slide) -> Layout {
    class_overrides(slide).1.unwrap_or(slide.layout)
}

/// A colour with transparency, as it looks over `under`.
fn over(c: egui::Color32, under: Rgb) -> Rgb {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    let a = a as f32 / 255.0;
    let mix = |top: u8, bottom: u8| (top as f32 * a + bottom as f32 * (1.0 - a)).round() as u8;
    [mix(r, under[0]), mix(g, under[1]), mix(b, under[2])]
}

/// The formula a slide's display math needs, as [`crate::needs`] lists it.
pub fn formula(latex: &str, layout: Layout, theme: &SlideTheme) -> Formula {
    let (size, color) = render::body_text(layout, theme);
    Formula {
        latex: latex.to_string(),
        size: size.round() as u32,
        color: rgb(color),
    }
}

fn is_picture(block: &Block) -> bool {
    matches!(block.kind, BlockKind::Image | BlockKind::Video)
}

pub fn build(slide: &Slide, cx: &mut Cx) -> Built {
    let stage = cx.deck.stage;
    let (w, h) = (stage.w as f32, stage.h as f32);
    let theme = theme_of(slide, cx.deck);
    let layout = layout_of(slide);
    let full_bleed = layout == Layout::FullBleed;
    let theme_bg = rgb(theme.bg);
    let bg = slide
        .directives
        .get_str("bg")
        .and_then(noteliner_slides::parse_color)
        .map_or(theme_bg, |c| over(c, theme_bg));
    let mut shapes = Vec::new();

    let (pad_x, pad_y) = if full_bleed {
        (0.0, 0.0)
    } else {
        (render::PAD_X, render::PAD_Y)
    };
    let mut top = pad_y;
    let mut bottom = h - pad_y;
    let inner = w - 2.0 * pad_x;
    let muted = rgb(theme.muted);
    let rule = rgb(theme.rule);

    let mut vars = HashMap::from([
        ("page".to_string(), slide.index.to_string()),
        ("total".to_string(), cx.deck.slides.len().to_string()),
    ]);
    vars.extend(cx.vars.iter().map(|(k, v)| (k.clone(), v.clone())));
    let band_text = |key: &str, fallback: &str| {
        let raw = match slide.directives.get(key) {
            Some(DirValue::Str(v)) => v.clone(),
            Some(DirValue::Flag) => "true".to_string(),
            None => fallback.to_string(),
        };
        substitute(&raw, &vars)
    };
    let header = band_text("header", &cx.deck.config.header);
    let footer = band_text("footer", &cx.deck.config.footer);
    let band_h = render::BAND_SIZE * 1.25;
    let band = |text: String, rect: Rect, align: Align, number: bool| {
        let mut run = Run::plain(text, render::BAND_SIZE, muted);
        run.slide_number = number;
        let mut p = Para::new(vec![run], render::BAND_SIZE, muted, 1.2);
        p.align = align;
        Shape::Text {
            rect,
            paras: vec![p],
            fill: None,
            line: None,
            radius: 0.0,
            inset: [0.0; 4],
            anchor: Anchor::Top,
            title: false,
            name: if number { "Slide Number" } else { "Band" },
        }
    };
    if !full_bleed && !header.is_empty() {
        shapes.push(band(
            header,
            Rect::new(pad_x, top, inner, band_h),
            Align::Left,
            false,
        ));
        let rule_y = top + band_h + render::BAND_PAD;
        shapes.push(Shape::Fill {
            rect: Rect::new(pad_x, rule_y, inner, render::RULE),
            color: rule,
        });
        top = rule_y + render::RULE + render::GAP;
    }
    let numbers = cx.deck.config.slide_numbers;
    if !full_bleed && (!footer.is_empty() || numbers) {
        let number_w = if numbers { 160.0 } else { 0.0 };
        let text_y = bottom - band_h;
        let rule_y = text_y - render::BAND_PAD - render::RULE;
        shapes.push(Shape::Fill {
            rect: Rect::new(pad_x, rule_y, inner, render::RULE),
            color: rule,
        });
        if !footer.is_empty() {
            shapes.push(band(
                footer,
                Rect::new(pad_x, text_y, inner - number_w, band_h),
                Align::Left,
                false,
            ));
        }
        if numbers {
            shapes.push(band(
                slide.index.to_string(),
                Rect::new(pad_x + inner - number_w, text_y, number_w, band_h),
                Align::Right,
                true,
            ));
        }
        bottom = rule_y - render::GAP;
    }
    let content = Rect::new(pad_x, top, inner, (bottom - top).max(0.0));

    // The title: on a band of the accent colour, except where the slide is
    // centred on its heading or a picture owns it.
    let banded = !matches!(layout, Layout::Hero | Layout::Quote | Layout::FullBleed);
    let hero = layout == Layout::Hero;
    let on_band = SlideTheme {
        bg: theme.accent,
        fg: theme.bg,
        muted: theme.bg,
        accent: theme.bg,
        rule: theme.bg,
        code_bg: theme.accent,
    };
    let title_theme = if banded { on_band } else { theme };
    let title_color = if banded {
        rgb(theme.bg)
    } else {
        rgb(theme.accent)
    };
    let mut title = Vec::new();
    for block in slide.headings() {
        let (size, lh) = render::title_size(block.level.unwrap_or(1).clamp(1, 6), hero);
        let flow = Flow {
            bold: true,
            ..Flow::new(size, title_color, title_theme)
        };
        let spans = match markdown_blocks(&block.text).into_iter().next() {
            Some(RichBlock::Text { spans, .. }) => spans,
            _ => vec![Span {
                text: block.text.clone(),
                style: Default::default(),
                link: None,
            }],
        };
        title.push(Para::new(text::runs(&spans, &flow), size, title_color, lh));
    }
    let title_h = text::height(&title, content.w);
    let title_shape =
        |rect: Rect, anchor: Anchor, fill: Option<Rgb>, inset: [f32; 4]| Shape::Text {
            rect,
            paras: title.clone(),
            fill,
            line: None,
            radius: 0.0,
            inset,
            anchor,
            title: true,
            name: "Title",
        };

    let body_blocks: Vec<&Block> = slide.body().collect();
    let (size, color) = render::body_text(layout, &theme);
    let flow = Flow::new(size, rgb(color), theme);
    let plain_quotes = layout == Layout::Quote;

    if banded && !title.is_empty() {
        let band_top = if top > pad_y { top - render::GAP } else { 0.0 };
        let band_bottom = band_top + 2.0 * render::TITLE_PAD + title_h;
        shapes.push(title_shape(
            Rect::new(0.0, band_top, w, band_bottom - band_top),
            Anchor::Middle,
            Some(rgb(theme.accent)),
            [content.x, render::TITLE_PAD, content.x, render::TITLE_PAD],
        ));
        let body_top = band_bottom + render::GAP;
        let area = Rect::new(
            content.x,
            body_top,
            content.w,
            (content.y + content.h - body_top).max(0.0),
        );
        body(
            &body_blocks,
            layout,
            area,
            &flow,
            plain_quotes,
            bg,
            cx,
            &mut shapes,
        );
        return Built { shapes, bg };
    }

    let gap = if title.is_empty() { 0.0 } else { render::GAP };
    let free = (content.h - title_h - gap).max(0.0);
    let centred = matches!(layout, Layout::Hero | Layout::Quote);
    let area = Rect::new(content.x, content.y + title_h + gap, content.w, free);
    if centred {
        // The title box ends where the text is centred and grows upwards, and
        // the body box starts there and grows downwards, so a wrong estimate
        // of a height moves the pair and never overlaps them.
        let body_h = body_height(&body_blocks, layout, area, &flow, plain_quotes, cx);
        let total = title_h
            + if body_blocks.is_empty() {
                0.0
            } else {
                gap + body_h
            };
        let y0 = if total < content.h {
            content.y + (content.h - total) / 2.0
        } else {
            content.y
        };
        if !title.is_empty() {
            let anchor = if body_blocks.is_empty() {
                Anchor::Middle
            } else {
                Anchor::Bottom
            };
            let rect = if body_blocks.is_empty() {
                content
            } else {
                Rect::new(content.x, content.y, content.w, y0 + title_h - content.y)
            };
            shapes.push(title_shape(rect, anchor, None, [0.0; 4]));
        }
        let start = y0 + title_h + gap;
        let area = Rect::new(
            content.x,
            start,
            content.w,
            (content.y + content.h - start).max(0.0),
        );
        body(
            &body_blocks,
            layout,
            area,
            &flow,
            plain_quotes,
            bg,
            cx,
            &mut shapes,
        );
    } else {
        if !title.is_empty() {
            shapes.push(title_shape(
                Rect::new(content.x, content.y, content.w, title_h),
                Anchor::Top,
                None,
                [0.0; 4],
            ));
        }
        body(
            &body_blocks,
            layout,
            area,
            &flow,
            plain_quotes,
            bg,
            cx,
            &mut shapes,
        );
    }
    Built { shapes, bg }
}

/// A deck block as pieces, with the gap above each.
fn pieces(block: &Block, flow: &Flow, plain_quotes: bool, cx: &mut Cx) -> Vec<(Piece, f32)> {
    let mut out: Vec<(Piece, f32)> = Vec::new();
    match block.kind {
        BlockKind::Image => {
            let src = block.src.clone().unwrap_or_default();
            let alt = block.alt.clone().unwrap_or_default();
            out.push((picture(&src, alt, cx), 0.0));
        }
        BlockKind::Video => {
            let src = block.src.clone().unwrap_or_default();
            let label = block
                .alt
                .clone()
                .filter(|a| !a.is_empty())
                .unwrap_or_else(|| src.clone());
            out.push((Piece::Video { label, src }, 0.0));
        }
        _ => {
            for rich in markdown_blocks(&block.text) {
                let gap = if out.is_empty() {
                    0.0
                } else {
                    render::PARAGRAPH_GAP
                };
                let one = std::slice::from_ref(&rich);
                let mut paras = Vec::new();
                match &rich {
                    RichBlock::Quote(inner) if !plain_quotes => {
                        text::paragraphs(inner, flow, &mut paras);
                        out.push((Piece::Quote(paras), gap));
                    }
                    RichBlock::Quote(inner) => {
                        let italic = Flow {
                            italic: true,
                            ..*flow
                        };
                        text::paragraphs(inner, &italic, &mut paras);
                        push_text(&mut out, paras, gap);
                    }
                    RichBlock::Code(code) => {
                        out.push((
                            Piece::Code(text::code_paragraphs(code, 0.0, &flow.theme)),
                            gap,
                        ));
                    }
                    RichBlock::Table { rows, header_rows } => out.push((
                        Piece::Table {
                            rows: rows.clone(),
                            header_rows: *header_rows,
                        },
                        gap,
                    )),
                    RichBlock::Image { src, alt, .. } => match math::parse_image_src(src) {
                        Some(latex) => {
                            let key = Formula {
                                latex: latex.clone(),
                                size: flow.size.round() as u32,
                                color: flow.color,
                            };
                            match cx.assets.formulas.get(&key) {
                                Some(p) => {
                                    let media = cx.media.add(format!("formula:{key:?}"), p);
                                    out.push((
                                        Piece::Picture {
                                            media,
                                            natural: p.natural,
                                            alt: latex,
                                            centre: true,
                                        },
                                        gap,
                                    ));
                                }
                                None => {
                                    let mut run = Run::plain(
                                        math::fallback_text(&latex, MathMode::Display),
                                        flow.size * 0.9,
                                        flow.color,
                                    );
                                    run.mono = true;
                                    let mut p = Para::new(
                                        vec![run],
                                        flow.size,
                                        flow.color,
                                        render::LINE_HEIGHT,
                                    );
                                    p.align = Align::Center;
                                    push_text(&mut out, vec![p], gap);
                                }
                            }
                        }
                        None => out.push((picture(src, alt.clone(), cx), gap)),
                    },
                    RichBlock::Rule => {}
                    _ => {
                        text::paragraphs(one, flow, &mut paras);
                        push_text(&mut out, paras, gap);
                    }
                }
            }
        }
    }
    out
}

/// Adds text to the pieces, into the text before it when there is some.
fn push_text(out: &mut Vec<(Piece, f32)>, mut paras: Vec<Para>, gap: f32) {
    if paras.is_empty() {
        return;
    }
    if let Some((Piece::Text(before), _)) = out.last_mut() {
        paras[0].before = paras[0].before.max(gap);
        before.extend(paras);
    } else {
        paras[0].before = 0.0;
        out.push((Piece::Text(paras), gap));
    }
}

fn picture(src: &str, alt: String, cx: &mut Cx) -> Piece {
    match cx.assets.pictures.get(src) {
        Some(p) if p.natural.0 > 0.0 && p.natural.1 > 0.0 => Piece::Picture {
            media: cx.media.add(format!("picture:{src}"), p),
            natural: p.natural,
            alt,
            centre: false,
        },
        _ => {
            let shown: String = src.chars().take(80).collect();
            cx.warnings
                .push(format!("A picture could not be read: {shown}"));
            Piece::Missing {
                alt,
                src: src.to_string(),
            }
        }
    }
}

/// A picture's size: its natural size on the stage, shrunk to fit.
fn contain(natural: (f32, f32), w: f32, h: f32) -> (f32, f32) {
    let fit = (w / natural.0).min(h / natural.1).clamp(0.0, 1.0);
    (natural.0 * fit, natural.1 * fit)
}

fn table_heights(rows: &[Vec<Vec<Span>>], header_rows: usize, width: f32, flow: &Flow) -> Vec<f32> {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
    let inner = width / cols as f32 - 2.0 * render::CELL_PAD_X;
    rows.iter()
        .enumerate()
        .map(|(r, row)| {
            let lines = row
                .iter()
                .map(|cell| text::lines(&cell_para(cell, r < header_rows, flow), inner))
                .max()
                .unwrap_or(1);
            lines as f32 * render::TABLE_SIZE * render::LINE_HEIGHT + 2.0 * render::CELL_PAD_Y
        })
        .collect()
}

fn cell_para(cell: &[Span], header: bool, flow: &Flow) -> Para {
    let color = if header {
        rgb(flow.theme.muted)
    } else {
        flow.color
    };
    let f = Flow {
        size: render::TABLE_SIZE,
        color,
        bold: header,
        left: 0.0,
        ..*flow
    };
    let mut p = Para::new(
        text::runs(cell, &f),
        render::TABLE_SIZE,
        color,
        render::LINE_HEIGHT,
    );
    p.align = cell
        .iter()
        .find_map(|s| s.style.cell_align)
        .unwrap_or(Align::Left);
    p
}

impl Piece {
    /// The height the piece takes in a column `w` wide with `max_h` free.
    fn height(&self, w: f32, max_h: f32, flow: &Flow) -> f32 {
        match self {
            Piece::Text(paras) => text::height(paras, w),
            Piece::Quote(paras) => text::height(paras, w - render::QUOTE_PAD),
            Piece::Code(paras) => {
                text::height(paras, w - 2.0 * render::CODE_PAD) + 2.0 * render::CODE_PAD
            }
            Piece::Table { rows, header_rows } => {
                table_heights(rows, *header_rows, w, flow).iter().sum()
            }
            Piece::Picture { natural, .. } => contain(*natural, w, max_h.max(1.0)).1,
            Piece::Missing { .. } => contain((1600.0, 900.0), w, max_h.max(1.0)).1,
            Piece::Video { .. } => flow.size * render::LINE_HEIGHT + 64.0,
        }
    }

    fn place(self, rect: Rect, flow: &Flow, bg: Rgb, out: &mut Vec<Shape>) {
        let theme = &flow.theme;
        let text = |rect: Rect, paras: Vec<Para>| Shape::Text {
            rect,
            paras,
            fill: None,
            line: None,
            radius: 0.0,
            inset: [0.0; 4],
            anchor: Anchor::Top,
            title: false,
            name: "Text",
        };
        match self {
            Piece::Text(paras) => out.push(text(rect, paras)),
            Piece::Quote(paras) => {
                out.push(Shape::Fill {
                    rect: Rect::new(rect.x, rect.y, render::QUOTE_BAR, rect.h),
                    color: rgb(theme.accent),
                });
                out.push(text(
                    Rect::new(
                        rect.x + render::QUOTE_PAD,
                        rect.y,
                        rect.w - render::QUOTE_PAD,
                        rect.h,
                    ),
                    paras,
                ));
            }
            Piece::Code(paras) => out.push(Shape::Text {
                rect,
                paras,
                fill: Some(rgb(theme.code_bg)),
                line: None,
                radius: 12.0,
                inset: [render::CODE_PAD; 4],
                anchor: Anchor::Top,
                title: false,
                name: "Code",
            }),
            Piece::Table { rows, header_rows } => {
                let mut heights = table_heights(&rows, header_rows, rect.w, flow);
                let total: f32 = heights.iter().sum();
                if total > rect.h && total > 0.0 {
                    let k = rect.h / total;
                    heights.iter_mut().for_each(|h| *h *= k);
                }
                let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
                let cells = rows
                    .iter()
                    .enumerate()
                    .map(|(r, row)| {
                        (0..cols)
                            .map(|c| Cell {
                                para: cell_para(
                                    row.get(c).map_or(&[][..], Vec::as_slice),
                                    r < header_rows,
                                    flow,
                                ),
                            })
                            .collect()
                    })
                    .collect();
                out.push(Shape::Table {
                    rect: Rect::new(rect.x, rect.y, rect.w, heights.iter().sum()),
                    rows: cells,
                    row_heights: heights,
                    header_rows,
                    rule: rgb(theme.rule),
                });
            }
            Piece::Picture {
                media,
                natural,
                alt,
                centre,
            } => {
                let (w, h) = contain(natural, rect.w, rect.h.max(1.0));
                let x = if centre {
                    rect.x + (rect.w - w) / 2.0
                } else {
                    rect.x
                };
                out.push(Shape::Picture {
                    rect: Rect::new(x, rect.y, w, h),
                    media,
                    alt,
                    crop: [0.0; 4],
                    radius: if centre { 0.0 } else { render::IMAGE_RADIUS },
                });
            }
            Piece::Missing { alt, src } => {
                let (w, h) = contain((1600.0, 900.0), rect.w, rect.h.max(1.0));
                let grey: Rgb = [128, 128, 128];
                let size = (h / 8.0).clamp(12.0, 28.0);
                let mut paras = Vec::new();
                for line in [alt, src.chars().take(120).collect()] {
                    if !line.is_empty() {
                        let mut p = Para::new(
                            vec![Run::plain(line, size, [150, 150, 150])],
                            size,
                            grey,
                            1.2,
                        );
                        p.align = Align::Center;
                        paras.push(p);
                    }
                }
                out.push(Shape::Text {
                    rect: Rect::new(rect.x, rect.y, w, h),
                    paras,
                    fill: Some(over(egui::Color32::from_black_alpha(40), bg)),
                    line: Some(grey),
                    radius: render::IMAGE_RADIUS,
                    inset: [16.0; 4],
                    anchor: Anchor::Middle,
                    title: false,
                    name: "Missing Picture",
                });
            }
            Piece::Video { label, src } => {
                let muted = rgb(theme.muted);
                let mut run = Run::plain(format!("▶  {label}"), flow.size, muted);
                if text::linkable(&src) {
                    run.link = Some(src);
                }
                out.push(Shape::Text {
                    rect,
                    paras: vec![Para::new(vec![run], flow.size, muted, render::LINE_HEIGHT)],
                    fill: None,
                    line: Some(rgb(theme.rule)),
                    radius: 12.0,
                    inset: [40.0, 32.0, 40.0, 32.0],
                    anchor: Anchor::Middle,
                    title: false,
                    name: "Video",
                });
            }
        }
    }
}

/// Stacks pieces from the top of `rect`. Pieces that would run past the
/// bottom are all made shorter by the same share. Returns the height used.
fn stack(
    mut pieces: Vec<(Piece, f32)>,
    rect: Rect,
    flow: &Flow,
    bg: Rgb,
    out: Option<&mut Vec<Shape>>,
) -> f32 {
    // Text that follows text goes in the same box.
    let mut merged: Vec<(Piece, f32)> = Vec::new();
    for (piece, gap) in pieces.drain(..) {
        match (merged.last_mut(), piece) {
            (Some((Piece::Text(before), _)), Piece::Text(mut paras)) => {
                if let Some(p) = paras.first_mut() {
                    p.before = gap;
                }
                before.extend(paras);
            }
            (_, piece) => merged.push((piece, gap)),
        }
    }
    let mut heights = Vec::with_capacity(merged.len());
    let mut y = 0.0;
    for (i, (piece, gap)) in merged.iter().enumerate() {
        if i > 0 {
            y += gap;
        }
        let h = piece.height(rect.w, (rect.h - y).max(0.0), flow);
        heights.push(h);
        y += h;
    }
    let total = y;
    let k = if total > rect.h && total > 0.0 {
        rect.h / total
    } else {
        1.0
    };
    let Some(out) = out else {
        return total.min(rect.h);
    };
    let count = merged.len();
    let mut y = rect.y;
    for (i, ((piece, gap), h)) in merged.into_iter().zip(heights).enumerate() {
        if i > 0 {
            y += gap * k;
        }
        let mut h = h * k;
        // The last text takes the room that is left, so a low estimate of its
        // height does not matter.
        if i + 1 == count && matches!(piece, Piece::Text(_)) {
            h = h.max(rect.y + rect.h - y);
        }
        piece.place(Rect::new(rect.x, y, rect.w, h), flow, bg, out);
        y += h;
    }
    total.min(rect.h)
}

/// The cells a layout gives the body's blocks: each block's place, in order.
fn cells(blocks: &[&Block], layout: Layout, area: Rect, heights: &[f32]) -> Vec<Rect> {
    match layout {
        Layout::Split => {
            let n = blocks.len() as f32;
            let col_w = (area.w - render::COLUMN_GAP * (n - 1.0)) / n;
            (0..blocks.len())
                .map(|i| {
                    Rect::new(
                        area.x + i as f32 * (col_w + render::COLUMN_GAP),
                        area.y,
                        col_w,
                        area.h,
                    )
                })
                .collect()
        }
        Layout::Grid => {
            let col_w = (area.w - render::COLUMN_GAP) / 2.0;
            let mut rows: Vec<f32> = heights
                .chunks(2)
                .map(|r| r.iter().copied().fold(0.0, f32::max))
                .collect();
            let gaps = render::GAP * rows.len().saturating_sub(1) as f32;
            let total: f32 = rows.iter().sum();
            let room = (area.h - gaps).max(0.0);
            if total > room && total > 0.0 {
                rows.iter_mut().for_each(|h| *h *= room / total);
            }
            let mut out = Vec::new();
            let mut y = area.y;
            for (r, row_h) in rows.iter().enumerate() {
                for i in 0..2 {
                    if r * 2 + i < blocks.len() {
                        out.push(Rect::new(
                            area.x + i as f32 * (col_w + render::COLUMN_GAP),
                            y,
                            col_w,
                            *row_h,
                        ));
                    }
                }
                y += row_h + render::GAP;
            }
            out
        }
        _ => Vec::new(),
    }
}

fn columned(layout: Layout, blocks: &[&Block]) -> bool {
    matches!(layout, Layout::Split | Layout::Grid) && !blocks.is_empty()
}

/// The estimated height of the body, for centring.
fn body_height(
    blocks: &[&Block],
    layout: Layout,
    area: Rect,
    flow: &Flow,
    plain_quotes: bool,
    cx: &mut Cx,
) -> f32 {
    if blocks.is_empty() {
        return 0.0;
    }
    let mut warnings = Vec::new();
    let mut media = Media::default();
    let mut quiet = Cx {
        deck: cx.deck,
        assets: cx.assets,
        vars: cx.vars,
        media: &mut media,
        warnings: &mut warnings,
    };
    let mut all = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        for (k, (piece, gap)) in pieces(block, flow, plain_quotes, &mut quiet)
            .into_iter()
            .enumerate()
        {
            let gap = if k == 0 && i > 0 { render::GAP } else { gap };
            all.push((piece, gap));
        }
    }
    let _ = layout;
    stack(all, area, flow, [0; 3], None)
}

#[allow(clippy::too_many_arguments)]
fn body(
    blocks: &[&Block],
    layout: Layout,
    area: Rect,
    flow: &Flow,
    plain_quotes: bool,
    bg: Rgb,
    cx: &mut Cx,
    out: &mut Vec<Shape>,
) {
    if blocks.is_empty() {
        return;
    }
    if layout == Layout::FullBleed && blocks.len() == 1 && is_picture(blocks[0]) {
        let block = blocks[0];
        if block.kind == BlockKind::Image {
            let src = block.src.clone().unwrap_or_default();
            let alt = block.alt.clone().unwrap_or_default();
            match picture(&src, alt, cx) {
                Piece::Picture {
                    media,
                    natural,
                    alt,
                    ..
                } => out.push(Shape::Picture {
                    rect: area,
                    media,
                    alt,
                    crop: cover(natural, area),
                    radius: 0.0,
                }),
                other => other.place(area, flow, bg, out),
            }
            return;
        }
    }
    if columned(layout, blocks) {
        let col_w = match layout {
            Layout::Split => {
                let n = blocks.len() as f32;
                (area.w - render::COLUMN_GAP * (n - 1.0)) / n
            }
            _ => (area.w - render::COLUMN_GAP) / 2.0,
        };
        let mut sets = Vec::new();
        let mut heights = Vec::new();
        for block in blocks {
            let set = pieces(block, flow, plain_quotes, cx);
            // Measured on a copy: placing uses the pieces up.
            let probe = Rect::new(0.0, 0.0, col_w, area.h);
            let copy = pieces(
                block,
                flow,
                plain_quotes,
                &mut Cx {
                    deck: cx.deck,
                    assets: cx.assets,
                    vars: cx.vars,
                    media: &mut Media::default(),
                    warnings: &mut Vec::new(),
                },
            );
            heights.push(stack(copy, probe, flow, bg, None));
            sets.push(set);
        }
        for (set, rect) in sets.into_iter().zip(cells(blocks, layout, area, &heights)) {
            stack(set, rect, flow, bg, Some(out));
        }
        return;
    }
    let mut all = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        for (k, (piece, gap)) in pieces(block, flow, plain_quotes, cx)
            .into_iter()
            .enumerate()
        {
            let gap = if k == 0 && i > 0 { render::GAP } else { gap };
            all.push((piece, gap));
        }
    }
    stack(all, area, flow, bg, Some(out));
}

/// The part of a picture cut away so that it covers `area`.
fn cover(natural: (f32, f32), area: Rect) -> [f32; 4] {
    let image = natural.0 / natural.1;
    let target = area.w / area.h.max(1e-3);
    if image > target {
        let cut = (1.0 - target / image) / 2.0;
        [cut, 0.0, cut, 0.0]
    } else {
        let cut = (1.0 - image / target) / 2.0;
        [0.0, cut, 0.0, cut]
    }
}

// ─── XML ─────────────────────────────────────────────────────────────────────

fn xfrm(tag: &str, r: Rect) -> String {
    format!(
        "<{tag}><a:off x=\"{}\" y=\"{}\"/><a:ext cx=\"{}\" cy=\"{}\"/></{tag}>",
        emu(r.x),
        emu(r.y),
        emu(r.w).max(0),
        emu(r.h).max(0)
    )
}

fn geometry(r: Rect, radius: f32) -> String {
    let short = r.w.min(r.h);
    if radius <= 0.0 || short <= 0.0 {
        return "<a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom>".to_string();
    }
    let adj = ((radius / short) * 100_000.0).round().clamp(0.0, 50_000.0) as i64;
    format!("<a:prstGeom prst=\"roundRect\"><a:avLst><a:gd name=\"adj\" fmla=\"val {adj}\"/></a:avLst></a:prstGeom>")
}

fn solid(c: Rgb) -> String {
    format!("<a:solidFill><a:srgbClr val=\"{}\"/></a:solidFill>", hex(c))
}

fn shape_xml(shape: &Shape, id: u32, fonts: &Fonts, rels: &mut Rels, media_ext: &[&str]) -> String {
    match shape {
        Shape::Text {
            rect,
            paras,
            fill,
            line,
            radius,
            inset,
            anchor,
            title,
            name,
        } => {
            let nv = if *title {
                format!("<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr><a:spLocks noGrp=\"1\"/></p:cNvSpPr><p:nvPr><p:ph type=\"title\"/></p:nvPr></p:nvSpPr>")
            } else {
                format!("<p:nvSpPr><p:cNvPr id=\"{id}\" name=\"{name} {id}\"/><p:cNvSpPr txBox=\"1\"/><p:nvPr/></p:nvSpPr>")
            };
            let anchor = match anchor {
                Anchor::Top => "t",
                Anchor::Middle => "ctr",
                Anchor::Bottom => "b",
            };
            format!(
                "<p:sp>{nv}<p:spPr>{}{}{}{}</p:spPr><p:txBody><a:bodyPr vert=\"horz\" wrap=\"square\" lIns=\"{}\" tIns=\"{}\" rIns=\"{}\" bIns=\"{}\" rtlCol=\"0\" anchor=\"{anchor}\"><a:normAutofit/></a:bodyPr><a:lstStyle/>{}</p:txBody></p:sp>",
                xfrm("a:xfrm", *rect),
                geometry(*rect, *radius),
                fill.map_or("<a:noFill/>".to_string(), solid),
                line.map_or("<a:ln><a:noFill/></a:ln>".to_string(), |c| format!(
                    "<a:ln w=\"{}\">{}</a:ln>",
                    emu(render::RULE),
                    solid(c)
                )),
                emu(inset[0]),
                emu(inset[1]),
                emu(inset[2]),
                emu(inset[3]),
                text::paras_xml(paras, fonts, rels),
            )
        }
        Shape::Fill { rect, color } => format!(
            "<p:sp><p:nvSpPr><p:cNvPr id=\"{id}\" name=\"Rule {id}\"/><p:cNvSpPr/><p:nvPr/></p:nvSpPr><p:spPr>{}{}{}<a:ln><a:noFill/></a:ln></p:spPr></p:sp>",
            xfrm("a:xfrm", *rect),
            geometry(*rect, 0.0),
            solid(*color),
        ),
        Shape::Picture {
            rect,
            media,
            alt,
            crop,
            radius,
        } => {
            let rid = rels.part(
                REL_IMAGE,
                &format!("../media/image{}.{}", media + 1, media_ext[*media]),
            );
            let pct = |f: f32| (f.clamp(0.0, 0.99) * 100_000.0).round() as i64;
            let src = if crop.iter().any(|c| *c > 0.0) {
                format!(
                    "<a:srcRect l=\"{}\" t=\"{}\" r=\"{}\" b=\"{}\"/>",
                    pct(crop[0]),
                    pct(crop[1]),
                    pct(crop[2]),
                    pct(crop[3])
                )
            } else {
                String::new()
            };
            format!(
                "<p:pic><p:nvPicPr><p:cNvPr id=\"{id}\" name=\"Picture {id}\" descr=\"{}\"/><p:cNvPicPr><a:picLocks noChangeAspect=\"1\"/></p:cNvPicPr><p:nvPr/></p:nvPicPr>\
<p:blipFill><a:blip r:embed=\"{rid}\"/>{src}<a:stretch><a:fillRect/></a:stretch></p:blipFill><p:spPr>{}{}</p:spPr></p:pic>",
                esc(alt),
                xfrm("a:xfrm", *rect),
                geometry(*rect, *radius),
            )
        }
        Shape::Table {
            rect,
            rows,
            row_heights,
            header_rows,
            rule,
        } => {
            let cols = rows.iter().map(Vec::len).max().unwrap_or(0).max(1);
            let col_w = emu(rect.w / cols as f32).max(1);
            let grid: String = (0..cols)
                .map(|_| format!("<a:gridCol w=\"{col_w}\"/>"))
                .collect();
            let none = |tag: &str| format!("<a:{tag} w=\"0\"><a:noFill/></a:{tag}>");
            let mut body = String::new();
            for (row, h) in rows.iter().zip(row_heights) {
                body.push_str(&format!("<a:tr h=\"{}\">", emu(*h).max(1)));
                for cell in row {
                    body.push_str(&format!(
                        "<a:tc><a:txBody><a:bodyPr/><a:lstStyle/>{}</a:txBody><a:tcPr marL=\"{mx}\" marR=\"{mx}\" marT=\"{my}\" marB=\"{my}\" anchor=\"t\">{}{}{}<a:lnB w=\"{}\" cap=\"flat\" cmpd=\"sng\" algn=\"ctr\">{}<a:prstDash val=\"solid\"/></a:lnB><a:noFill/></a:tcPr></a:tc>",
                        text::para_xml(&cell.para, fonts, rels),
                        none("lnL"),
                        none("lnR"),
                        none("lnT"),
                        emu(render::RULE),
                        solid(*rule),
                        mx = emu(render::CELL_PAD_X),
                        my = emu(render::CELL_PAD_Y),
                    ));
                }
                body.push_str("</a:tr>");
            }
            format!(
                "<p:graphicFrame><p:nvGraphicFramePr><p:cNvPr id=\"{id}\" name=\"Table {id}\"/><p:cNvGraphicFramePr><a:graphicFrameLocks noGrp=\"1\"/></p:cNvGraphicFramePr><p:nvPr/></p:nvGraphicFramePr>{}\
<a:graphic><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/table\"><a:tbl><a:tblPr{}/><a:tblGrid>{grid}</a:tblGrid>{body}</a:tbl></a:graphicData></a:graphic></p:graphicFrame>",
                xfrm("p:xfrm", *rect),
                if *header_rows > 0 { " firstRow=\"1\"" } else { "" },
            )
        }
    }
}

/// The slide part. A picture's relationship names its file by `media_ext`.
pub fn slide_xml(built: &Built, fonts: &Fonts, rels: &mut Rels, media_ext: &[&str]) -> String {
    let mut tree = String::new();
    for (i, shape) in built.shapes.iter().enumerate() {
        tree.push_str(&shape_xml(shape, i as u32 + 2, fonts, rels, media_ext));
    }
    format!(
        "{}<p:sld {NS}><p:cSld><p:bg><p:bgPr>{}<a:effectLst/></p:bgPr></p:bg><p:spTree>{TREE_HEAD}{tree}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>",
        crate::xml::DECL,
        solid(built.bg),
    )
}

/// The speaker notes as plain paragraphs: one for each line of the notes.
pub fn notes_paragraphs(notes: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for line in notes.trim().lines() {
        out.push(line.trim_end().to_string());
    }
    out
}
