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
//!
//! Text is selected across blocks by dragging (the view scrolls when the pointer leaves it), and
//! copied with Ctrl+C. Positions inside a block count through its text parts
//! ([`Block::text_parts`]): one per paragraph, code block or table cell, in layout order. The
//! selection is reported as a [`Selection`]; [`Mark`]s (highlights) are painted behind the text,
//! and a click on one is reported as [`RichOutput::clicked_mark`].

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange, LayoutJob};
use egui::text_selection::visuals::paint_text_selection;
use egui::{
    Color32, CursorIcon, FontFamily, FontId, Galley, IdSalt, Label, Pos2, Rect, ScrollArea, Sense,
    Stroke, TextFormat, Ui, UiBuilder, Visuals,
};

use crate::fonts::Families;
use crate::model::{Align, Block, Document, Indent, Span, TextKind};
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
/// A press that moves less than this is a click, not a selection.
const DRAG_THRESHOLD: f32 = 4.0;
/// Scrolling while a selection is dragged past the view's edge, per point beyond it per frame.
const AUTOSCROLL_RATE: f32 = 0.35;
/// Superscripts and subscripts, relative to their text.
const SCRIPT_SCALE: f32 = 0.7;

/// A place in a document: the top-level block, and a character offset inside it, counted through
/// the block's text parts ([`Block::part_text`]). Ordered by block, then offset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Position {
    pub block: usize,
    pub offset: usize,
}

/// Selected text: `start` < `end`, and the text between them (parts and blocks on their own
/// lines).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Selection {
    pub start: Position,
    pub end: Position,
    pub text: String,
}

/// A highlighted range, painted behind the text.
#[derive(Clone, Debug, PartialEq)]
pub struct Mark {
    /// Reported back when the mark is clicked.
    pub id: String,
    pub start: Position,
    pub end: Position,
    /// Usually translucent, so the text stays readable.
    pub color: Color32,
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
    /// The [`Mark`] clicked this frame (a click, not the end of a selection drag).
    pub clicked_mark: Option<String>,
    /// The selection started, changed or was cleared this frame.
    pub selection_changed: bool,
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
    marks: Vec<Mark>,
    /// (anchor, focus): where the drag started and where it is.
    selection: Option<(Position, Position)>,
    /// A press on text: where it started (document and screen), and whether it has moved.
    drag: Option<Drag>,
    /// The text laid out last frame, for hit-testing the pointer.
    hits: Vec<TextHit>,
}

#[derive(Clone, Copy, Debug)]
struct Drag {
    anchor: Position,
    origin: Pos2,
    moved: bool,
}

/// A text part laid out this frame, where it is on screen.
#[derive(Clone)]
struct TextHit {
    block: usize,
    /// The part's first character, as an offset in its block.
    base: usize,
    galley: Arc<Galley>,
    /// Where the galley's origin is.
    pos: Pos2,
    rect: Rect,
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
            marks: Vec::new(),
            selection: None,
            drag: None,
            hits: Vec::new(),
        }
    }

    /// Distinguishes this view's scroll area from others in the same `Ui` (e.g. per chapter).
    pub fn id_salt(mut self, salt: impl egui::AsIdSalt) -> Self {
        self.id_salt = IdSalt::new(salt);
        self
    }

    /// Forgets everything about the current document (keeps the budget, id and marks).
    pub fn reset(&mut self) {
        *self = Self {
            id_salt: self.id_salt,
            marks: std::mem::take(&mut self.marks),
            ..Self::with_budget(self.budget)
        };
    }

    /// The highlights to paint (replacing the previous ones).
    pub fn set_marks(&mut self, marks: Vec<Mark>) {
        self.marks = marks;
    }

    pub fn marks(&self) -> &[Mark] {
        &self.marks
    }

    /// The current selection of `doc`, if any text is selected.
    pub fn selection(&self, doc: &Document) -> Option<Selection> {
        let (a, b) = self.selection?;
        let (start, end) = (a.min(b), a.max(b));
        let text = selected_text(doc, start, end);
        (start != end).then_some(Selection { start, end, text })
    }

    /// Selects `start..end` (e.g. to restore a selection, or in tests).
    pub fn select(&mut self, start: Position, end: Position) {
        self.selection = (start != end).then_some((start, end));
    }

    pub fn clear_selection(&mut self) {
        self.selection = None;
        self.drag = None;
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

    /// Whether a [`Self::restore`] or scroll is still to land: until then [`Self::position`] is
    /// where the view was, not where it's going.
    pub fn restoring(&self) -> bool {
        self.pending.is_some() || self.next_offset.is_some()
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
        // Dragging selects text; it only scrolls on touch screens (egui's default).
        let mut area = ScrollArea::vertical()
            .id_salt(self.id_salt)
            .auto_shrink(false)
            .animated(false);
        if let Some(y) = self.next_offset.take() {
            area = area.vertical_scroll_offset(y);
        }
        let mut links = Vec::new();
        self.hits.clear();
        let out = area.show_viewport(ui, |ui, viewport| {
            self.layout(ui, viewport, doc, style, resolver, &mut links);
        });
        let offset = out.state.clamped_offset().y;
        let (clicked_mark, selection_changed) = self.pointer(ui, out.inner_rect, offset, doc);
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
            offset,
            viewport_height: out.inner_rect.height(),
            content_height: out.content_size.y,
            clicked_mark,
            selection_changed,
        }
    }

    /// Selection by dragging, clicks on marks, Ctrl+C and Escape. `inner` is the viewport on
    /// screen. Returns the clicked mark and whether the selection changed.
    fn pointer(
        &mut self,
        ui: &Ui,
        inner: Rect,
        offset: f32,
        doc: &Document,
    ) -> (Option<String>, bool) {
        let ctx = ui.ctx().clone();
        let before = self.selection;
        let mut clicked = None;
        let (pressed, down, released, pos) = ctx.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
            )
        });
        // Only presses meant for this view: not on a dialog or menu above it.
        let ours = |p: Pos2| {
            inner.contains(p)
                && ctx
                    .layer_id_at(p)
                    .is_none_or(|l| l == ui.layer_id() || l.order == egui::Order::Tooltip)
        };
        if pressed {
            if let Some(p) = pos.filter(|p| ours(*p)) {
                self.selection = None;
                self.drag = self.hit(p).map(|anchor| Drag {
                    anchor,
                    origin: p,
                    moved: false,
                });
            }
        }
        if let (Some(drag), Some(p)) = (self.drag.as_mut(), pos) {
            if down {
                drag.moved |= p.distance(drag.origin) > DRAG_THRESHOLD;
                if drag.moved {
                    let anchor = drag.anchor;
                    if let Some(focus) = self.hit(p) {
                        self.selection = (focus != anchor).then_some((anchor, focus));
                    }
                    // Past the top or bottom edge: scroll towards the pointer.
                    let beyond = if p.y < inner.top() {
                        p.y - inner.top()
                    } else if p.y > inner.bottom() {
                        p.y - inner.bottom()
                    } else {
                        0.0
                    };
                    if beyond != 0.0 {
                        self.next_offset = Some((offset + beyond * AUTOSCROLL_RATE).max(0.0));
                        ctx.request_repaint();
                    }
                }
            }
        }
        if released {
            if let Some(drag) = self.drag.take() {
                if !drag.moved {
                    clicked = self
                        .marks
                        .iter()
                        .rev()
                        .find(|m| {
                            m.start.min(m.end) <= drag.anchor && drag.anchor < m.end.max(m.start)
                        })
                        .map(|m| m.id.clone());
                }
            }
        }
        let focused = ctx.memory(|m| m.focused().is_some());
        if !focused && self.selection.is_some() {
            let (copy, escape) = ctx.input(|i| {
                (
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if copy {
                if let Some(sel) = self.selection(doc) {
                    ctx.copy_text(sel.text);
                }
            }
            if escape {
                self.selection = None;
            }
        }
        let changed = before != self.selection;
        if changed {
            ctx.request_repaint();
        }
        (clicked, changed)
    }

    /// The document position under (or nearest to) a screen point, from last layout.
    fn hit(&self, p: Pos2) -> Option<Position> {
        let row = self
            .hits
            .iter()
            .filter(|h| h.rect.y_range().contains(p.y))
            .min_by(|a, b| {
                let dx = |h: &TextHit| (h.rect.left() - p.x).max(p.x - h.rect.right()).max(0.0);
                dx(a).total_cmp(&dx(b))
            });
        let hit = row.or_else(|| {
            self.hits.iter().min_by(|a, b| {
                let dy = |h: &TextHit| (h.rect.top() - p.y).max(p.y - h.rect.bottom());
                dy(a).total_cmp(&dy(b))
            })
        })?;
        let c = hit.galley.cursor_from_pos(p - hit.pos).index.0;
        Some(Position {
            block: hit.block,
            offset: hit.base + c,
        })
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
        let visuals = ui.visuals().clone();
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
                            let mut base = 0;
                            let mut cx = Cx {
                                style,
                                families: &families,
                                resolver: &mut *resolver,
                                links: &mut *links,
                                galley: visible.then_some(&mut galley),
                                block: i,
                                base: &mut base,
                                paint: Paint {
                                    selection: self.selection.map(|(a, b)| (a.min(b), a.max(b))),
                                    marks: &self.marks,
                                    visuals: &visuals,
                                },
                                hits: visible.then_some(&mut self.hits),
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
            Block::Text {
                kind,
                spans,
                align,
                indent,
            } if pos.offset > 0 => {
                let left = indent_px(*indent, style);
                let (job, _) = text_job(
                    spans,
                    *kind,
                    *align,
                    indent.first_line * style.size,
                    style,
                    families,
                    self.width - left,
                );
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                heading_gap(*kind, style) + galley.pos_from_cursor(CCursor::new(pos.offset)).top()
            }
            _ => 0.0,
        };
        top + extra
    }
}

/// The text from `start` to `end`: each block's parts on their own lines.
fn selected_text(doc: &Document, start: Position, end: Position) -> String {
    let mut out = Vec::new();
    for b in start.block..=end.block.min(doc.blocks.len().saturating_sub(1)) {
        let text = doc.blocks[b].part_text();
        let from = if b == start.block { start.offset } else { 0 };
        let to = if b == end.block {
            end.offset
        } else {
            usize::MAX
        };
        let slice: String = text
            .chars()
            .skip(from)
            .take(to.saturating_sub(from))
            .collect();
        if !slice.trim().is_empty() {
            out.push(slice);
        }
    }
    out.join("\n")
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
    /// The top-level block being laid out.
    block: usize,
    /// The next text part's offset in the block.
    base: &'a mut usize,
    paint: Paint<'a>,
    /// Where visible text parts are recorded for hit-testing (`None` while measuring).
    hits: Option<&'a mut Vec<TextHit>>,
}

/// What's painted behind the text: the selection and the marks.
#[derive(Clone, Copy)]
struct Paint<'a> {
    /// Sorted.
    selection: Option<(Position, Position)>,
    marks: &'a [Mark],
    visuals: &'a Visuals,
}

impl Cx<'_> {
    fn child(&mut self) -> Cx<'_> {
        Cx {
            style: self.style,
            families: self.families,
            resolver: &mut *self.resolver,
            links: &mut *self.links,
            galley: None,
            block: self.block,
            base: &mut *self.base,
            paint: self.paint,
            hits: self.hits.as_deref_mut(),
        }
    }

    /// Claims the next text part: its offset in the block. Paints the selection and marks into
    /// `galley`.
    fn part(&mut self, galley: &mut Arc<Galley>) -> usize {
        let base = *self.base;
        let len = galley.text().chars().count();
        *self.base += len + 1;
        let local = |start: Position, end: Position| -> Option<CCursorRange> {
            let from = match start.block.cmp(&self.block) {
                std::cmp::Ordering::Less => 0,
                std::cmp::Ordering::Equal => start.offset,
                std::cmp::Ordering::Greater => return None,
            };
            let to = match end.block.cmp(&self.block) {
                std::cmp::Ordering::Greater => usize::MAX,
                std::cmp::Ordering::Equal => end.offset,
                std::cmp::Ordering::Less => return None,
            };
            let (a, b) = (
                from.max(base) - base,
                to.min(base + len).saturating_sub(base),
            );
            (a < b && from <= base + len)
                .then(|| CCursorRange::two(CCursor::new(a), CCursor::new(b)))
        };
        for mark in self.paint.marks {
            if let Some(range) = local(mark.start.min(mark.end), mark.end.max(mark.start)) {
                let mut v = self.paint.visuals.clone();
                v.selection.bg_fill = mark.color;
                v.selection.stroke.color = self.style.text;
                paint_text_selection(galley, &v, &range, None);
            }
        }
        if let Some((start, end)) = self.paint.selection {
            if let Some(range) = local(start, end) {
                paint_text_selection(galley, self.paint.visuals, &range, None);
            }
        }
        base
    }

    /// Records a laid-out text part for hit-testing.
    fn record(&mut self, base: usize, galley: Arc<Galley>, pos: Pos2, rect: Rect) {
        if let Some(hits) = self.hits.as_deref_mut() {
            hits.push(TextHit {
                block: self.block,
                base,
                galley,
                pos,
                rect,
            });
        }
    }
}

/// A paragraph's left indent, in points (at most what a `Margin` holds).
fn indent_px(indent: Indent, style: &RichStyle) -> f32 {
    (indent.left * style.size).clamp(0.0, 120.0)
}

fn block_ui(ui: &mut Ui, block: &Block, id: usize, cx: &mut Cx<'_>) {
    let style = cx.style;
    match block {
        Block::Text {
            kind,
            spans,
            align,
            indent,
        } => {
            ui.add_space(heading_gap(*kind, style));
            let left = indent_px(*indent, style);
            let (job, links) = text_job(
                spans,
                *kind,
                *align,
                indent.first_line * style.size,
                style,
                cx.families,
                ui.available_width() - left,
            );
            let mut galley = ui.fonts_mut(|f| f.layout_job(job));
            let base = cx.part(&mut galley);
            let label = Label::new(egui::WidgetText::Galley(galley.clone()))
                .selectable(false)
                .sense(Sense::click());
            let resp = egui::Frame::new()
                .inner_margin(egui::Margin {
                    left: left as i8,
                    ..Default::default()
                })
                .show(ui, |ui| match align {
                    Align::Left | Align::Justify => ui.add(label),
                    Align::Center => ui.vertical_centered(|ui| ui.add(label)).inner,
                    Align::Right => {
                        ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| ui.add(label))
                            .inner
                    }
                })
                .inner;
            let galley_pos = match align {
                Align::Left | Align::Justify => resp.rect.left_top(),
                Align::Center => resp.rect.center_top(),
                Align::Right => resp.rect.right_top(),
            };
            cx.record(base, galley.clone(), galley_pos, resp.rect);
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
                            let mut galley = ui.fonts_mut(|f| f.layout_job(job));
                            let base = cx.part(&mut galley);
                            let resp = ui.add(
                                Label::new(egui::WidgetText::Galley(galley.clone()))
                                    .selectable(false),
                            );
                            cx.record(base, galley, resp.rect.left_top(), resp.rect);
                        });
                });
            ui.add_space(style.size * PARAGRAPH_GAP);
        }
        Block::Image {
            src,
            alt,
            title,
            width,
            fraction,
            ..
        } => match cx.resolver.image(ui.ctx(), src) {
            Some(uri) => {
                let column = ui.available_width();
                let max = fraction
                    .map_or(column, |f| column * f)
                    .min(width.unwrap_or(f32::INFINITY));
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
                                        0.0,
                                        style,
                                        cx.families,
                                        cell_width,
                                    );
                                    // A galley keeps the cell's own wrap width (a job would be re-wrapped to the grid's).
                                    let mut galley = ui.fonts_mut(|f| f.layout_job(job));
                                    let base = cx.part(&mut galley);
                                    let resp = ui.add(
                                        Label::new(egui::WidgetText::Galley(galley.clone()))
                                            .selectable(false),
                                    );
                                    cx.record(base, galley, resp.rect.left_top(), resp.rect);
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

/// The layout job for a run of spans, and the character ranges that are links. `first_line` is
/// the first line's indent, in points.
pub fn text_job(
    spans: &[Span],
    kind: TextKind,
    align: Align,
    first_line: f32,
    style: &RichStyle,
    families: &Families,
    width: f32,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let (scale, heading) = heading_scale(kind);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    job.halign = match align {
        Align::Left | Align::Justify => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    };
    // `Align::Justify` is set ragged-right: egui's justification spreads the space between every
    // glyph rather than between words, which reads worse than no justification.
    let mut leading = first_line.max(0.0);
    let mut links = Vec::new();
    let mut chars = 0;
    for span in spans {
        let st = span.style;
        let bold = st.bold || heading;
        let italic = st.italic || kind == TextKind::Caption;
        let script = st.sup || st.sub;
        let size = style.size
            * scale
            * if st.small { 0.8 } else { 1.0 }
            * if script { SCRIPT_SCALE } else { 1.0 };
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
            valign: if st.sup {
                egui::Align::TOP
            } else if st.sub {
                egui::Align::BOTTOM
            } else {
                egui::Align::Center
            },
            ..Default::default()
        };
        let n = span.text.chars().count();
        if let Some(link) = &span.link {
            links.push((chars..chars + n, link.clone()));
        }
        chars += n;
        job.append(&span.text, std::mem::take(&mut leading), format);
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
