//! Lays out a [`Document`] in egui.
//!
//! [`RichView::show`] runs inside `ScrollArea::show_viewport`: it measures every block once per
//! width, then lays out only the blocks near the viewport and reserves the measured height for
//! the rest, so long chapters cost what's on screen. It also reports the reading position (top
//! block + character offset) and can scroll back to one.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, LayoutJob};
use egui::{
    Color32, CursorIcon, FontFamily, FontId, Galley, Label, Rect, Sense, Stroke, TextFormat, Ui,
    UiBuilder,
};

use crate::fonts::{self, Face, Variant};
use crate::model::{Align, Block, Document, Span, TextKind};

/// Extra height around the viewport that is still laid out (smooth scrolling, keyboard paging).
const OVERSCAN: f32 = 600.0;
const LINE_HEIGHT: f32 = 1.5;
/// Space after a paragraph, in ems.
const PARAGRAPH_GAP: f32 = 0.4;
/// Images are at most this share of the window's height.
const IMAGE_MAX_SCREEN: f32 = 0.85;
const LIST_INDENT: f32 = 28.0;
const QUOTE_INDENT: f32 = 16.0;

/// Colours and sizes, usually from the egui visuals.
#[derive(Clone, Debug)]
pub struct Theme {
    pub size: f32,
    pub face: Face,
    /// Whether `face` has real bold/italic files ([`fonts::install`]); otherwise they're faked.
    pub real_faces: bool,
    pub text: Color32,
    pub strong: Color32,
    pub muted: Color32,
    pub link: Color32,
    pub code_bg: Color32,
    pub rule: Color32,
    /// The reading column's widest width.
    pub max_width: f32,
}

impl Theme {
    pub fn from_visuals(v: &egui::Visuals, face: Face, real_faces: bool) -> Self {
        Self {
            size: 16.0,
            face,
            real_faces,
            text: v.text_color(),
            strong: v.strong_text_color(),
            muted: v.weak_text_color(),
            link: v.hyperlink_color,
            code_bg: v.code_bg_color,
            rule: v.widgets.noninteractive.bg_stroke.color,
            max_width: 720.0,
        }
    }
}

/// A place in a document: the top-level block, and a character offset inside it (text blocks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Position {
    pub block: usize,
    pub offset: usize,
}

/// Something the reader did.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    Link(String),
}

/// Maps an image `src` to an egui image URI (or `None` to show its alt text).
pub type ImageUri<'a> = dyn FnMut(&egui::Context, &str) -> Option<String> + 'a;

/// Per-document view state that outlives a frame. Call [`RichView::reset`] when the document
/// changes.
#[derive(Default)]
pub struct RichView {
    width: f32,
    heights: Vec<f32>,
    tops: Vec<f32>,
    top: Position,
    restore: Option<Position>,
    scroll_target: Option<f32>,
    /// Blocks laid out in the last frame.
    pub laid_out: usize,
}

impl RichView {
    pub fn reset(&mut self) {
        *self = RichView::default();
    }

    /// The reading position at the top of the viewport.
    pub fn position(&self) -> Position {
        self.top
    }

    /// Scrolls to `pos` once the document has been measured.
    pub fn restore(&mut self, pos: Position) {
        self.restore = Some(pos);
    }

    /// Whether a [`RichView::restore`] hasn't landed yet.
    pub fn restoring(&self) -> bool {
        self.restore.is_some() || self.scroll_target.is_some()
    }

    /// A vertical offset the scroll area should jump to (pass to `vertical_scroll_offset`).
    pub fn take_scroll_target(&mut self) -> Option<f32> {
        self.scroll_target.take()
    }

    /// Draws `doc` in the viewport given by `ScrollArea::show_viewport`.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        viewport: Rect,
        doc: &Document,
        theme: &Theme,
        images: &mut ImageUri<'_>,
    ) -> Vec<Action> {
        let mut actions = Vec::new();
        let full = ui.available_width();
        let width = full.min(theme.max_width);
        let left = ui.max_rect().left() + ((full - width) / 2.0).max(0.0);
        let column = Rect::from_min_size(
            egui::pos2(left, ui.cursor().top()),
            egui::vec2(width, f32::INFINITY),
        );
        ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
            ui.set_width(width);
            let measure =
                (self.width - width).abs() > 0.5 || self.heights.len() != doc.blocks.len();
            if measure {
                self.width = width;
                self.heights = vec![0.0; doc.blocks.len()];
            }
            self.tops.resize(doc.blocks.len(), 0.0);
            let origin = ui.max_rect().top();
            let visible = (viewport.min.y - OVERSCAN)..(viewport.max.y + OVERSCAN);
            let mut top_block = None;
            let mut galleys: HashMap<usize, (Arc<Galley>, f32)> = HashMap::new();
            self.laid_out = 0;
            for (i, block) in doc.blocks.iter().enumerate() {
                let y = ui.cursor().top() - origin;
                self.tops[i] = y;
                let h = self.heights[i];
                if measure || (y + h >= visible.start && y <= visible.end) {
                    self.laid_out += 1;
                    let mut galley = None;
                    let rect = ui
                        .scope(|ui| {
                            let mut cx = Cx {
                                theme,
                                images: &mut *images,
                                actions: &mut actions,
                                galley: Some(&mut galley),
                            };
                            block_ui(ui, block, i, &mut cx);
                        })
                        .response
                        .rect;
                    self.heights[i] = rect.height();
                    if let Some((g, gy)) = galley {
                        galleys.insert(i, (g, gy - origin - y));
                    }
                } else {
                    ui.allocate_space(egui::vec2(width, h));
                }
                if top_block.is_none() && y + self.heights[i] > viewport.min.y {
                    top_block = Some(i);
                }
            }
            if let Some(i) = top_block {
                let offset = galleys
                    .get(&i)
                    // Rows of centred text start left of x = 0, so probe from the galley's left edge.
                    .map(|(g, gy)| {
                        g.cursor_from_pos(egui::vec2(
                            g.rect.left(),
                            viewport.min.y - self.tops[i] - gy,
                        ))
                        .index
                        .0
                    })
                    .unwrap_or(0);
                self.top = Position { block: i, offset };
            }
            if !measure {
                if let Some(pos) = self.restore.take() {
                    self.scroll_target = Some(self.offset_of(ui, doc, theme, pos));
                }
            }
        });
        actions
    }

    /// The content offset of `pos`.
    fn offset_of(&self, ui: &Ui, doc: &Document, theme: &Theme, pos: Position) -> f32 {
        let Some(&top) = self.tops.get(pos.block) else {
            return 0.0;
        };
        let extra = match &doc.blocks[pos.block] {
            Block::Text { kind, spans, align } if pos.offset > 0 => {
                let (job, _) = text_job(spans, *kind, *align, theme, self.width);
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                heading_gap(*kind, theme) + galley.pos_from_cursor(CCursor::new(pos.offset)).top()
            }
            _ => 0.0,
        };
        top + extra
    }
}

struct Cx<'a, 'b> {
    theme: &'a Theme,
    images: &'a mut ImageUri<'b>,
    actions: &'a mut Vec<Action>,
    /// Where a top-level text block reports its galley (for positions).
    galley: Option<&'a mut Option<(Arc<Galley>, f32)>>,
}

fn block_ui(ui: &mut Ui, block: &Block, id: usize, cx: &mut Cx<'_, '_>) {
    let theme = cx.theme;
    match block {
        Block::Text { kind, spans, align } => {
            ui.add_space(heading_gap(*kind, theme));
            let (job, links) = text_job(spans, *kind, *align, theme, ui.available_width());
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            let label = Label::new(egui::WidgetText::Galley(galley.clone()))
                .selectable(true)
                .sense(Sense::click());
            let resp = match align {
                Align::Left => ui.add(label),
                Align::Center => ui.vertical_centered(|ui| ui.add(label)).inner,
                Align::Right => {
                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| ui.add(label))
                        .inner
                }
            };
            let galley_pos = match align {
                Align::Left => resp.rect.left_top(),
                Align::Center => resp.rect.center_top(),
                Align::Right => resp.rect.right_top(),
            };
            if let Some(slot) = cx.galley.take() {
                *slot = Some((galley.clone(), resp.rect.top()));
            }
            if *kind != TextKind::Caption {
                ui.add_space(theme.size * PARAGRAPH_GAP);
            }
            if !links.is_empty() {
                if let Some(pointer) = resp.hover_pos() {
                    let c = galley.cursor_from_pos(pointer - galley_pos).index.0;
                    if let Some((_, target)) = links.iter().find(|(r, _)| r.contains(&c)) {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        let resp = resp.on_hover_text_at_pointer(target.as_str());
                        if resp.clicked() {
                            cx.actions.push(Action::Link(target.clone()));
                        }
                    }
                }
            }
        }
        Block::List {
            ordered,
            start,
            items,
        } => {
            for (n, item) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    let marker = if *ordered {
                        format!("{}.", *start as usize + n)
                    } else {
                        "•".to_string()
                    };
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(LIST_INDENT, theme.size * LINE_HEIGHT),
                        Sense::hover(),
                    );
                    ui.painter().text(
                        rect.right_top()
                            + egui::vec2(-8.0, (theme.size * LINE_HEIGHT - theme.size) / 2.0),
                        egui::Align2::RIGHT_TOP,
                        marker,
                        FontId::new(theme.size, fonts::family(theme.face, Variant::Regular)),
                        theme.text,
                    );
                    ui.vertical(|ui| {
                        for (k, b) in item.iter().enumerate() {
                            let mut sub = Cx {
                                theme,
                                images: &mut *cx.images,
                                actions: &mut *cx.actions,
                                galley: None,
                            };
                            block_ui(ui, b, id * 1000 + n * 10 + k, &mut sub);
                        }
                    });
                });
            }
        }
        Block::Quote(blocks) => {
            let resp = egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: QUOTE_INDENT as i8,
                    ..Default::default()
                })
                .show(ui, |ui| {
                    for (k, b) in blocks.iter().enumerate() {
                        let mut sub = Cx {
                            theme,
                            images: &mut *cx.images,
                            actions: &mut *cx.actions,
                            galley: None,
                        };
                        block_ui(ui, b, id * 1000 + k, &mut sub);
                    }
                })
                .response;
            let r = resp.rect;
            ui.painter()
                .vline(r.left() + 3.0, r.y_range(), Stroke::new(3.0, theme.rule));
        }
        Block::Code(code) => {
            egui::Frame::new()
                .fill(theme.code_bg)
                .inner_margin(8.0)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt(("code", id))
                        .show(ui, |ui| {
                            let format = TextFormat::simple(
                                FontId::new(theme.size * 0.85, FontFamily::Monospace),
                                theme.text,
                            );
                            let mut job = LayoutJob::single_section(code.clone(), format);
                            job.wrap.max_width = f32::INFINITY;
                            ui.add(Label::new(job).selectable(true).extend());
                        });
                });
        }
        Block::Image {
            src,
            alt,
            title,
            width,
            ..
        } => match (cx.images)(ui.ctx(), src) {
            Some(uri) => {
                let max = ui.available_width().min(width.unwrap_or(f32::INFINITY));
                // Covers and full-page plates fit the screen.
                let max_height = ui.ctx().content_rect().height() * IMAGE_MAX_SCREEN;
                ui.vertical_centered(|ui| {
                    let image = egui::Image::new(uri)
                        .max_width(max)
                        .max_height(max_height)
                        .alt_text(alt.clone());
                    let resp = ui.add(image);
                    if !title.is_empty() {
                        resp.on_hover_text(title.as_str());
                    }
                });
                ui.add_space(theme.size * PARAGRAPH_GAP);
            }
            None if !alt.is_empty() => {
                ui.label(
                    egui::RichText::new(format!("[image: {alt}]"))
                        .italics()
                        .color(theme.muted),
                );
            }
            None => {}
        },
        Block::Rule => {
            ui.add_space(theme.size * 0.5);
            ui.separator();
            ui.add_space(theme.size * 0.5);
        }
        Block::Table { rows, header_rows } => {
            let columns = rows.iter().map(Vec::len).max().unwrap_or(1).max(1);
            let cell_width = (ui.available_width() / columns as f32 - 12.0).max(80.0);
            egui::ScrollArea::horizontal()
                .id_salt(("table", id))
                .show(ui, |ui| {
                    egui::Grid::new(("grid", id))
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for (r, row) in rows.iter().enumerate() {
                                for cell in row {
                                    let mut spans = cell.clone();
                                    if r < *header_rows {
                                        spans.iter_mut().for_each(|s| s.style.bold = true);
                                    }
                                    let (job, _) = text_job(
                                        &spans,
                                        TextKind::Paragraph,
                                        Align::Left,
                                        theme,
                                        cell_width,
                                    );
                                    // A galley keeps the cell's own wrap width (a job would be re-wrapped to the grid's).
                                    let galley = ui.fonts_mut(|f| f.layout_job(job));
                                    ui.add(
                                        Label::new(egui::WidgetText::Galley(galley))
                                            .selectable(true),
                                    );
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.add_space(theme.size * PARAGRAPH_GAP);
        }
    }
}

/// Space above a heading.
fn heading_gap(kind: TextKind, theme: &Theme) -> f32 {
    match kind {
        TextKind::Heading(_) => theme.size * 0.6,
        _ => 0.0,
    }
}

/// The layout job for a run of spans, and the character ranges that are links.
pub fn text_job(
    spans: &[Span],
    kind: TextKind,
    align: Align,
    theme: &Theme,
    width: f32,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let (scale, heading) = match kind {
        TextKind::Heading(1) => (1.7, true),
        TextKind::Heading(2) => (1.45, true),
        TextKind::Heading(3) => (1.25, true),
        TextKind::Heading(_) => (1.1, true),
        TextKind::Paragraph => (1.0, false),
        TextKind::Caption => (0.85, false),
    };
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    job.halign = match align {
        Align::Left => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    };
    let mut links = Vec::new();
    let mut chars = 0;
    for span in spans {
        let st = span.style;
        let bold = st.bold || heading;
        let italic = st.italic || kind == TextKind::Caption;
        let size = theme.size * scale * if st.small { 0.8 } else { 1.0 };
        let family = if st.code {
            FontFamily::Monospace
        } else {
            let variant = match (bold, italic) {
                (false, false) => Variant::Regular,
                (true, false) => Variant::Bold,
                (false, true) => Variant::Italic,
                (true, true) => Variant::BoldItalic,
            };
            fonts::family(theme.face, variant)
        };
        let mut color = if kind == TextKind::Caption {
            theme.muted
        } else {
            theme.text
        };
        if bold && !theme.real_faces {
            color = theme.strong;
        }
        if span.link.is_some() {
            color = theme.link;
        }
        let format = TextFormat {
            font_id: FontId::new(if st.code { size * 0.9 } else { size }, family),
            line_height: Some(size * LINE_HEIGHT),
            color,
            background: if st.code {
                theme.code_bg
            } else {
                Color32::TRANSPARENT
            },
            // Keep the code background to the glyphs rather than the whole (1.5×) line.
            expand_bg: 0.0,
            italics: italic && !theme.real_faces && !st.code,
            underline: if st.underline || span.link.is_some() {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            strikethrough: if st.strike {
                Stroke::new(1.0, color)
            } else {
                Stroke::NONE
            },
            valign: egui::Align::Center,
            ..Default::default()
        };
        let n = span.text.chars().count();
        if let Some(link) = &span.link {
            links.push((chars..chars + n, link.clone()));
        }
        chars += n;
        job.append(&span.text, 0.0, format);
    }
    (job, links)
}
