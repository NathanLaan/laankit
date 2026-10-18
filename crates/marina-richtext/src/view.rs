//! Lays out a [`Document`] in egui, progressively.
//!
//! [`RichView::show`] draws a document in its own vertical scroll area. Every frame it:
//!
//! 1. lays out (and paints) the blocks in the viewport, and the already-measured ones within
//!    [`OVERSCAN`] of it;
//! 2. measures blocks it hasn't measured yet, invisibly, until the frame's layout budget
//!    ([`DEFAULT_BUDGET`]) is spent;
//! 3. reserves the remaining blocks' heights: measured where known, otherwise estimated from
//!    their text length.
//!
//! So opening a long document costs one screenful plus the budget, and the estimates turn into
//! real heights over the next few frames. When a block above the viewport changes height, the
//! view scrolls by the same amount, which keeps whatever is at the top of the viewport in place.
//! It discards that frame and sets the corrected offset before the next pass lays anything out,
//! so the jump is never painted. The pass that follows a discard only
//! redraws, so a frame costs at most the visible blocks twice plus one budget.
//!
//! The view also reports the reading position (top block + character offset) and can scroll
//! back to one, or to an `#anchor`.

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, LayoutJob};
use egui::{
    Color32, CursorIcon, FontFamily, FontId, Galley, IdSalt, Label, Rect, ScrollArea, Sense,
    Stroke, TextFormat, Ui, UiBuilder,
};

use crate::fonts::Families;
use crate::model::{Align, Block, Document, Span, TextKind};
use crate::resolve::{LinkAction, Resolver};
use crate::style::RichStyle;

/// Off-screen measuring allowed per frame, on top of laying out what's visible. At least one
/// block is measured per frame, so a document always settles.
pub const DEFAULT_BUDGET: Duration = Duration::from_millis(4);
/// Extra height around the viewport that is laid out like the viewport (smooth scrolling,
/// keyboard paging).
pub const OVERSCAN: f32 = 600.0;
/// Space after a paragraph, in ems.
const PARAGRAPH_GAP: f32 = 0.4;
/// Images are at most this share of the window's height.
const IMAGE_MAX_SCREEN: f32 = 0.85;
const LIST_INDENT: f32 = 28.0;
const QUOTE_INDENT: f32 = 16.0;
const CODE_MARGIN: f32 = 8.0;
/// Average glyph width in ems, for height estimates.
const EST_CHAR_WIDTH: f32 = 0.5;
/// An image of unknown size is estimated at this share of the column width.
const EST_IMAGE_RATIO: f32 = 0.5;

/// A place in a document: the top-level block, and a character offset inside it (text blocks).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Position {
    pub block: usize,
    pub offset: usize,
}

/// What [`RichView::show`] did this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RichOutput {
    /// Links clicked this frame for the app to handle.
    pub actions: Vec<Action>,
    /// The scroll offset.
    pub offset: f32,
    /// The visible height.
    pub viewport_height: f32,
    /// The document's height (partly estimated until [`RichView::settled`]).
    pub content_height: f32,
}

/// Something the reader did that the app handles.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// A link the [`Resolver`] reported ([`LinkAction::Open`]).
    Open(String),
}

/// What the last [`RichView::show`] did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameStats {
    /// Blocks laid out near the viewport.
    pub visible: usize,
    /// Off-screen blocks measured within the budget.
    pub measured: usize,
    /// Blocks still estimated.
    pub estimated: usize,
    /// How far the view scrolled to keep the top of the viewport in place.
    pub anchor_shift: f32,
}

/// Per-document view state that outlives a frame. Call [`RichView::reset`] when the document
/// changes.
pub struct RichView {
    budget: Duration,
    id_salt: IdSalt,
    /// An offset to apply before the next pass lays anything out.
    next_offset: Option<f32>,
    width: f32,
    heights: Vec<f32>,
    measured: Vec<bool>,
    tops: Vec<f32>,
    /// The block at the top of the viewport and its content offset, as of the last frame.
    anchor: Option<(usize, f32)>,
    top: Position,
    pending: Option<Position>,
    last: FrameStats,
}

impl Default for RichView {
    fn default() -> Self {
        Self::with_budget(DEFAULT_BUDGET)
    }
}

impl RichView {
    /// A view that spends at most `budget` per frame measuring off-screen blocks.
    /// `Duration::MAX` measures everything in the first frame.
    pub fn with_budget(budget: Duration) -> Self {
        Self {
            budget,
            id_salt: IdSalt::new("marina-richtext"),
            next_offset: None,
            width: 0.0,
            heights: Vec::new(),
            measured: Vec::new(),
            tops: Vec::new(),
            anchor: None,
            top: Position::default(),
            pending: None,
            last: FrameStats::default(),
        }
    }

    /// Distinguishes this view's scroll area from others in the same `Ui` (e.g. per chapter).
    pub fn id_salt(mut self, salt: impl egui::AsIdSalt) -> Self {
        self.id_salt = IdSalt::new(salt);
        self
    }

    /// Forgets everything about the current document (keeps the budget and id).
    pub fn reset(&mut self) {
        *self = Self {
            id_salt: self.id_salt,
            ..Self::with_budget(self.budget)
        };
    }

    /// Scrolls to a content offset on the next frame (paging, "go to end").
    pub fn scroll_to_offset(&mut self, offset: f32) {
        self.next_offset = Some(offset.max(0.0));
    }

    /// The reading position at the top of the viewport.
    pub fn position(&self) -> Position {
        self.top
    }

    /// Scrolls to `pos` on the next frame.
    pub fn restore(&mut self, pos: Position) {
        self.pending = Some(pos);
    }

    /// Scrolls to the block holding `#anchor`; false if the document has no such anchor.
    pub fn scroll_to_anchor(&mut self, doc: &Document, anchor: &str) -> bool {
        match doc.anchors.get(anchor) {
            Some(&block) => {
                self.restore(Position { block, offset: 0 });
                true
            }
            None => false,
        }
    }

    /// Whether every block has its real height (no estimates left) and no scroll is pending.
    pub fn settled(&self) -> bool {
        self.pending.is_none() && self.next_offset.is_none() && self.measured.iter().all(|m| *m)
    }

    /// What the last frame did.
    pub fn last_frame(&self) -> FrameStats {
        self.last
    }

    /// The content offset of each block's top, as of the last frame.
    pub fn block_tops(&self) -> &[f32] {
        &self.tops
    }

    /// Draws `doc` in a vertical scroll area filling the available space.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        doc: &Document,
        style: &RichStyle,
        resolver: &mut dyn Resolver,
    ) -> RichOutput {
        // Programmatic scrolls are this view's own corrections: they must land at once.
        let mut area = ScrollArea::vertical()
            .id_salt(self.id_salt)
            .auto_shrink(false)
            .animated(false);
        if let Some(y) = self.next_offset.take() {
            area = area.vertical_scroll_offset(y);
        }
        let mut links = Vec::new();
        let out = area.show_viewport(ui, |ui, viewport| {
            self.layout(ui, viewport, doc, style, resolver, &mut links);
        });
        let mut actions = Vec::new();
        for target in links {
            match resolver.link(&target) {
                LinkAction::Open(t) => actions.push(Action::Open(t)),
                LinkAction::Anchor(a) => {
                    self.scroll_to_anchor(doc, &a);
                }
                LinkAction::Ignore => {}
            }
        }
        RichOutput {
            actions,
            offset: out.state.clamped_offset().y,
            viewport_height: out.inner_rect.height(),
            content_height: out.content_size.y,
        }
    }

    /// Lays `doc` out in the scroll area's content `ui`; `viewport` is in content coordinates.
    fn layout(
        &mut self,
        ui: &mut Ui,
        viewport: Rect,
        doc: &Document,
        style: &RichStyle,
        resolver: &mut dyn Resolver,
        links: &mut Vec<String>,
    ) {
        let families = Families::of(ui.ctx());
        let full = ui.available_width();
        let width = full.min(style.max_width);
        let left = ui.max_rect().left() + ((full - width) / 2.0).max(0.0);
        let column = Rect::from_min_size(
            egui::pos2(left, ui.cursor().top()),
            egui::vec2(width, f32::INFINITY),
        );
        ui.scope_builder(UiBuilder::new().max_rect(column), |ui| {
            ui.set_width(width);
            let n = doc.blocks.len();
            if (self.width - width).abs() > 0.5 || self.heights.len() != n {
                self.width = width;
                self.heights = doc
                    .blocks
                    .iter()
                    .map(|b| estimate(b, style, width))
                    .collect();
                self.measured = vec![false; n];
            }
            self.tops.resize(n, 0.0);
            let origin = ui.max_rect().top();
            let near = (viewport.min.y - OVERSCAN)..(viewport.max.y + OVERSCAN);
            let mut stats = FrameStats::default();
            let mut top_block = None;
            let mut galleys: HashMap<usize, (Arc<Galley>, f32)> = HashMap::new();
            let mut shift = 0.0;
            // A pass after a discarded one (see below) has no pass left to correct a shift, so it
            // measures nothing new above the viewport and nothing off-screen.
            let first_pass = ui.ctx().current_pass_index() == 0;
            let budget = if first_pass {
                self.budget
            } else {
                Duration::ZERO
            };
            let mut spent = Duration::ZERO;
            for (i, block) in doc.blocks.iter().enumerate() {
                let y = ui.cursor().top() - origin;
                self.tops[i] = y;
                if let Some((a, old_top)) = self.anchor {
                    if a == i {
                        shift = y - old_top;
                    }
                }
                let h = self.heights[i];
                let above = y + h < viewport.min.y;
                let on_screen = y + h >= viewport.min.y && y <= viewport.max.y;
                let in_overscan = y + h >= near.start && y <= near.end;
                // Overscan blocks seen for the first time are measured within the budget; once
                // measured they're laid out every frame, which keeps their galleys cached.
                let visible = (on_screen || (in_overscan && self.measured[i]))
                    && (self.measured[i] || !above || first_pass);
                let measure = !visible && !self.measured[i] && spent < budget;
                if visible || measure {
                    let started = Instant::now();
                    let mut galley = None;
                    let builder = if visible {
                        UiBuilder::new()
                    } else {
                        UiBuilder::new().invisible()
                    };
                    let rect = ui
                        .scope_builder(builder, |ui| {
                            let mut cx = Cx {
                                style,
                                families: &families,
                                resolver: &mut *resolver,
                                links: &mut *links,
                                galley: visible.then_some(&mut galley),
                            };
                            block_ui(ui, block, i, &mut cx);
                        })
                        .response
                        .rect;
                    self.heights[i] = rect.height();
                    self.measured[i] = true;
                    if visible {
                        stats.visible += 1;
                    } else {
                        spent += started.elapsed();
                        stats.measured += 1;
                    }
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
            stats.estimated = self.measured.iter().filter(|m| !**m).count();

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
            let mut scroll = 0.0;
            if let Some(pos) = self.pending.take().filter(|p| p.block < n) {
                scroll = self.offset_of(ui, doc, style, &families, pos) - viewport.min.y;
                self.anchor = Some((pos.block, self.tops[pos.block]));
            } else if shift.abs() > 0.5 {
                // Blocks above the top one changed height: follow it.
                scroll = shift;
                stats.anchor_shift = shift;
                self.anchor = top_block.map(|i| (i, self.tops[i]));
            } else {
                self.anchor = top_block.map(|i| (i, self.tops[i]));
            }
            if scroll.abs() > 0.5 {
                self.next_offset = Some((viewport.min.y + scroll).max(0.0));
                ui.ctx()
                    .request_discard("marina-richtext: keep the reading position");
                if !ui.ctx().will_discard() {
                    // Out of passes: correct on the next frame.
                    ui.ctx().request_repaint();
                }
            }
            if stats.estimated > 0 {
                ui.ctx().request_repaint();
            }
            if !first_pass {
                // Report the frame, not just its redraw pass.
                stats.measured = self.last.measured;
                stats.anchor_shift = self.last.anchor_shift;
            }
            self.last = stats;
        });
    }

    /// The content offset of `pos`.
    fn offset_of(
        &self,
        ui: &Ui,
        doc: &Document,
        style: &RichStyle,
        families: &Families,
        pos: Position,
    ) -> f32 {
        let Some(&top) = self.tops.get(pos.block) else {
            return 0.0;
        };
        let extra = match &doc.blocks[pos.block] {
            Block::Text { kind, spans, align } if pos.offset > 0 => {
                let (job, _) = text_job(spans, *kind, *align, style, families, self.width);
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                heading_gap(*kind, style) + galley.pos_from_cursor(CCursor::new(pos.offset)).top()
            }
            _ => 0.0,
        };
        top + extra
    }
}

/// A block's height before it's laid out: its text spread over the column at an average glyph
/// width. Wrong by a few lines at most; the anchor keeps the view steady when it's corrected.
fn estimate(block: &Block, style: &RichStyle, width: f32) -> f32 {
    let size = style.size;
    let line = size * style.line_height;
    let lines = |chars: usize, scale: f32| {
        let per_line = (width / (size * scale * EST_CHAR_WIDTH)).max(1.0);
        (chars as f32 / per_line).ceil().max(1.0)
    };
    match block {
        Block::Text { kind, spans, .. } => {
            let scale = heading_scale(*kind).0;
            let chars = spans.iter().map(|s| s.text.chars().count()).sum();
            heading_gap(*kind, style) + lines(chars, scale) * line * scale + size * PARAGRAPH_GAP
        }
        Block::List { items, .. } => items
            .iter()
            .map(|item| {
                item.iter()
                    .map(|b| estimate(b, style, width - LIST_INDENT))
                    .sum::<f32>()
                    .max(line)
            })
            .sum(),
        Block::Quote(blocks) => blocks
            .iter()
            .map(|b| estimate(b, style, width - QUOTE_INDENT))
            .sum(),
        Block::Code(code) => code.lines().count().max(1) as f32 * size + CODE_MARGIN * 2.0,
        Block::Image {
            width: w,
            height: h,
            ..
        } => {
            let image = match (w, h) {
                (Some(w), Some(h)) if *w > 0.0 => h * (width / w).min(1.0),
                _ => width * EST_IMAGE_RATIO,
            };
            image + size * PARAGRAPH_GAP
        }
        Block::Rule => size * 1.2,
        Block::Table { rows, .. } => rows.len() as f32 * (line + 6.0) + size * PARAGRAPH_GAP,
    }
}

struct Cx<'a> {
    style: &'a RichStyle,
    families: &'a Families,
    resolver: &'a mut dyn Resolver,
    /// Link targets clicked this frame.
    links: &'a mut Vec<String>,
    /// Where a top-level text block reports its galley (for positions).
    galley: Option<&'a mut Option<(Arc<Galley>, f32)>>,
}

impl Cx<'_> {
    fn child(&mut self) -> Cx<'_> {
        Cx {
            style: self.style,
            families: self.families,
            resolver: &mut *self.resolver,
            links: &mut *self.links,
            galley: None,
        }
    }
}

fn block_ui(ui: &mut Ui, block: &Block, id: usize, cx: &mut Cx<'_>) {
    let style = cx.style;
    match block {
        Block::Text { kind, spans, align } => {
            ui.add_space(heading_gap(*kind, style));
            let (job, links) = text_job(
                spans,
                *kind,
                *align,
                style,
                cx.families,
                ui.available_width(),
            );
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
                ui.add_space(style.size * PARAGRAPH_GAP);
            }
            if !links.is_empty() {
                if let Some(pointer) = resp.hover_pos() {
                    let c = galley.cursor_from_pos(pointer - galley_pos).index.0;
                    if let Some((_, target)) = links.iter().find(|(r, _)| r.contains(&c)) {
                        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
                        let resp = resp.on_hover_text_at_pointer(target.as_str());
                        if resp.clicked() {
                            cx.links.push(target.clone());
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
            let line = style.size * style.line_height;
            let marker_font = FontId::new(
                style.size,
                cx.families.resolve(style.face, false, false).family,
            );
            for (n, item) in items.iter().enumerate() {
                ui.horizontal_top(|ui| {
                    let marker = if *ordered {
                        format!("{}.", *start as usize + n)
                    } else {
                        "•".to_string()
                    };
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(LIST_INDENT, line), Sense::hover());
                    ui.painter().text(
                        rect.right_top() + egui::vec2(-8.0, (line - style.size) / 2.0),
                        egui::Align2::RIGHT_TOP,
                        marker,
                        marker_font.clone(),
                        style.text,
                    );
                    ui.vertical(|ui| {
                        for (k, b) in item.iter().enumerate() {
                            block_ui(ui, b, id * 1000 + n * 10 + k, &mut cx.child());
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
                        block_ui(ui, b, id * 1000 + k, &mut cx.child());
                    }
                })
                .response;
            let r = resp.rect;
            ui.painter().vline(
                r.left() + 3.0,
                r.y_range(),
                Stroke::new(3.0, style.quote_bar),
            );
        }
        Block::Code(code) => {
            egui::Frame::new()
                .fill(style.pre_bg)
                .inner_margin(CODE_MARGIN)
                .corner_radius(marina_egui::theme::RADIUS)
                .show(ui, |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt(("code", id))
                        .show(ui, |ui| {
                            let format = TextFormat::simple(
                                FontId::new(style.size * 0.85, FontFamily::Monospace),
                                style.text,
                            );
                            let mut job = LayoutJob::single_section(code.clone(), format);
                            job.wrap.max_width = f32::INFINITY;
                            ui.add(Label::new(job).selectable(true).extend());
                        });
                });
            ui.add_space(style.size * PARAGRAPH_GAP);
        }
        Block::Image {
            src,
            alt,
            title,
            width,
            ..
        } => match cx.resolver.image(ui.ctx(), src) {
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
                ui.add_space(style.size * PARAGRAPH_GAP);
            }
            None if !alt.is_empty() => {
                ui.label(
                    egui::RichText::new(format!("[image: {alt}]"))
                        .italics()
                        .color(style.muted),
                );
            }
            None => {}
        },
        Block::Rule => {
            ui.add_space(style.size * 0.5);
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(ui.available_width(), 1.0), Sense::hover());
            ui.painter().hline(
                rect.x_range(),
                rect.center().y,
                Stroke::new(1.0, style.rule),
            );
            ui.add_space(style.size * 0.5);
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
                                        style,
                                        cx.families,
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
            ui.add_space(style.size * PARAGRAPH_GAP);
        }
    }
}

/// Space above a heading.
fn heading_gap(kind: TextKind, style: &RichStyle) -> f32 {
    match kind {
        TextKind::Heading(_) => style.size * 0.6,
        _ => 0.0,
    }
}

/// A text kind's size multiplier, and whether it's a heading (bold).
fn heading_scale(kind: TextKind) -> (f32, bool) {
    match kind {
        TextKind::Heading(1) => (1.7, true),
        TextKind::Heading(2) => (1.45, true),
        TextKind::Heading(3) => (1.25, true),
        TextKind::Heading(_) => (1.1, true),
        TextKind::Paragraph => (1.0, false),
        TextKind::Caption => (0.85, false),
    }
}

/// The layout job for a run of spans, and the character ranges that are links.
pub fn text_job(
    spans: &[Span],
    kind: TextKind,
    align: Align,
    style: &RichStyle,
    families: &Families,
    width: f32,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let (scale, heading) = heading_scale(kind);
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
        let size = style.size * scale * if st.small { 0.8 } else { 1.0 };
        let font = if st.code {
            families.code(bold, italic)
        } else {
            families.resolve(style.face, bold, italic)
        };
        let mut color = if kind == TextKind::Caption {
            style.muted
        } else {
            style.text
        };
        if font.fake_bold {
            color = style.strong;
        }
        if span.link.is_some() {
            color = style.link;
        }
        let format = TextFormat {
            font_id: FontId::new(if st.code { size * 0.9 } else { size }, font.family),
            line_height: Some(size * style.line_height),
            color,
            background: if st.code {
                style.code_bg
            } else {
                Color32::TRANSPARENT
            },
            // Keep the code background to the glyphs rather than the whole (1.5×) line.
            expand_bg: 0.0,
            italics: font.fake_italic,
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use egui::{Pos2, RawInput};

    use super::*;
    use crate::convert::{from_html, Options};
    use crate::resolve::WebResolver;

    /// Runs one frame (all its passes) of `view` showing `doc`; returns the scroll offset.
    fn frame(ctx: &egui::Context, view: &mut RichView, doc: &Document) -> f32 {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 700.0))),
            ..Default::default()
        };
        let mut offset = 0.0;
        let out = ctx.run_ui(input, |ui| {
            let style = RichStyle::from_ctx(ui.ctx());
            offset = view.show(ui, doc, &style, &mut WebResolver).offset;
        });
        out.drop_without_applying_deltas();
        offset
    }

    fn long_doc(blocks: usize) -> Document {
        let html: String = (0..blocks)
            .map(|i| match i % 5 {
                0 => format!("<h2 id=\"s{i}\">Section {i}</h2>"),
                1 => format!("<ul><li>Item {i}</li><li>Another <em>item</em></li></ul>"),
                2 => "<pre>fn main() {\n    println!(\"hi\");\n}</pre>".to_string(),
                _ => format!(
                    "<p>{i} {}</p>",
                    "Lorem ipsum <b>dolor</b> sit amet, consectetur adipiscing elit. ".repeat(8)
                ),
            })
            .collect();
        from_html(&html, &Options::default())
    }

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        marina_egui::fonts::install(&ctx);
        marina_egui::theme::ThemeState::default().apply(&ctx);
        ctx
    }

    #[test]
    fn progressive_layout_finishes() {
        let doc = long_doc(600);
        let ctx = ctx();
        let mut view = RichView::default();
        frame(&ctx, &mut view, &doc);
        assert!(
            !view.settled(),
            "600 blocks shouldn't all fit in one budget"
        );
        let mut frames = 1;
        while !view.settled() {
            frame(&ctx, &mut view, &doc);
            frames += 1;
            assert!(frames < 2000, "never settled");
        }
        // The same heights as measuring everything at once.
        let mut eager = RichView::with_budget(Duration::MAX);
        frame(&ctx, &mut eager, &doc);
        frame(&ctx, &mut eager, &doc);
        assert!(eager.settled());
        for (a, b) in view.block_tops().iter().zip(eager.block_tops()) {
            assert!((a - b).abs() < 0.5, "{a} vs {b}");
        }
    }

    #[test]
    fn anchor_keeps_the_top_block_in_place() {
        let doc = long_doc(600);
        let ctx = ctx();
        let mut view = RichView::default();
        frame(&ctx, &mut view, &doc);
        view.restore(Position {
            block: 300,
            offset: 0,
        });
        let mut shifted = false;
        for _ in 0..2000 {
            let offset = frame(&ctx, &mut view, &doc);
            let top = view.block_tops()[300];
            eprintln!(
                "offset {offset} top {top} pos {:?} {:?}",
                view.position(),
                view.last_frame()
            );
            assert!(
                (top - offset).abs() < 1.0,
                "block 300 at {top}, viewport at {offset}"
            );
            assert_eq!(view.position().block, 300);
            shifted |= view.last_frame().anchor_shift != 0.0;
            if view.settled() {
                break;
            }
        }
        assert!(view.settled());
        assert!(
            shifted,
            "blocks above should have been measured and corrected for"
        );
    }

    #[test]
    fn anchor_links_scroll() {
        let doc = long_doc(100);
        let ctx = ctx();
        let mut view = RichView::default();
        frame(&ctx, &mut view, &doc);
        assert!(view.scroll_to_anchor(&doc, "s50"));
        assert!(!view.scroll_to_anchor(&doc, "nope"));
        let offset = frame(&ctx, &mut view, &doc);
        assert!((view.block_tops()[50] - offset).abs() < 1.0);
    }
}
