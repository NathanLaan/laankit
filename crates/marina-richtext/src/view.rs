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
use crate::math::MathMode;
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
/// What a task checkbox span is laid out as (its box is drawn over it; see `decorate`).
const CHECKBOX_TEXT: &str = "[x]";
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
    /// Where the last plain click landed: Shift+click extends from here when nothing is selected.
    caret: Option<Position>,
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
            caret: None,
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

    /// Selection by dragging, double-click (word), triple-click (paragraph), Shift+click
    /// (extend), Ctrl/Cmd+A (everything); clicks on marks; Ctrl+C and Escape. `inner` is the
    /// viewport on screen. Returns the clicked mark and whether the selection changed.
    ///
    /// Ctrl/Cmd+A belongs to this view when no text field has keyboard focus and the pointer is
    /// over it (or it already has a selection), so it never takes Select All from a field or
    /// from another view in the same window.
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
        let (pressed, down, released, pos, shift, clicks) = ctx.input(|i| {
            let primary = egui::PointerButton::Primary;
            let clicks = if i.pointer.button_triple_clicked(primary) {
                3
            } else if i.pointer.button_double_clicked(primary) {
                2
            } else {
                1
            };
            (
                i.pointer.primary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
                i.modifiers.shift,
                clicks,
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
                let hit = self.hit(p);
                let from = self.selection.map(|(a, _)| a).or(self.caret);
                match (shift, from, hit) {
                    // Shift+click extends; dragging on from there keeps extending.
                    (true, Some(anchor), Some(focus)) => {
                        self.selection = (anchor != focus).then_some((anchor, focus));
                        self.drag = Some(Drag {
                            anchor,
                            origin: p,
                            moved: true,
                        });
                    }
                    _ => {
                        self.selection = None;
                        self.caret = hit;
                        self.drag = hit.map(|anchor| Drag {
                            anchor,
                            origin: p,
                            moved: false,
                        });
                    }
                }
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
                if !drag.moved && clicks > 1 {
                    let range = if clicks == 2 {
                        self.word_at(drag.anchor)
                    } else {
                        self.part_at(drag.anchor)
                    };
                    self.selection = range.filter(|(a, b)| a != b);
                }
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
        let hovered = ctx.pointer_hover_pos().is_some_and(ours);
        if !focused
            && (hovered || self.selection.is_some())
            && ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::A))
        {
            self.selection = whole(doc);
        }
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

    /// The word around `pos` (letters, digits, `_` and inner apostrophes), from this frame's
    /// layout; `None` on a space or punctuation.
    fn word_at(&self, pos: Position) -> Option<(Position, Position)> {
        let (hit, i) = self.part_hit(pos)?;
        let chars: Vec<char> = hit.galley.text().chars().collect();
        let word = |k: usize| {
            chars.get(k).is_some_and(|c| {
                c.is_alphanumeric()
                    || *c == '_'
                    || (matches!(c, '\'' | '’')
                        && k > 0
                        && chars[k - 1].is_alphanumeric()
                        && chars.get(k + 1).is_some_and(|n| n.is_alphanumeric()))
            })
        };
        // A click just past a word's last letter still means that word.
        let i = if word(i) {
            i
        } else {
            i.checked_sub(1).filter(|k| word(*k))?
        };
        let mut a = i;
        while a > 0 && word(a - 1) {
            a -= 1;
        }
        let mut b = i + 1;
        while word(b) {
            b += 1;
        }
        let at = |k: usize| Position {
            block: pos.block,
            offset: hit.base + k,
        };
        Some((at(a), at(b)))
    }

    /// The whole text part (paragraph, list item, cell, code block) holding `pos`.
    fn part_at(&self, pos: Position) -> Option<(Position, Position)> {
        let (hit, _) = self.part_hit(pos)?;
        let len = hit.galley.text().chars().count();
        let at = |k: usize| Position {
            block: pos.block,
            offset: hit.base + k,
        };
        Some((at(0), at(len)))
    }

    /// The text part laid out this frame that holds `pos`, and `pos`'s index in it.
    fn part_hit(&self, pos: Position) -> Option<(&TextHit, usize)> {
        self.hits.iter().find_map(|h| {
            let len = h.galley.text().chars().count();
            (h.block == pos.block && h.base <= pos.offset && pos.offset <= h.base + len)
                .then(|| (h, pos.offset - h.base))
        })
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
                    first_line_px(*indent, style),
                    style,
                    families,
                    self.width - left,
                    Some(ui.ctx()),
                );
                let galley = layout_text(ui, job, *align);
                gap_above(*kind, *indent, style)
                    + galley.pos_from_cursor(CCursor::new(pos.offset)).top()
            }
            _ => 0.0,
        };
        top + extra
    }
}

/// The whole of `doc`, as a selection range (`None` when it has no text).
fn whole(doc: &Document) -> Option<(Position, Position)> {
    let last = doc.blocks.len().checked_sub(1)?;
    let end = Position {
        block: last,
        offset: doc.blocks[last].part_text().chars().count(),
    };
    let start = Position::default();
    (start != end).then_some((start, end))
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
        Block::Text {
            kind,
            spans,
            indent,
            ..
        } => {
            let scale = heading_scale(*kind).0;
            let chars = spans.iter().map(|s| s.text.chars().count()).sum();
            gap_above(*kind, *indent, style)
                + lines(chars, scale) * line * scale
                + gap_below(*kind, *indent, style)
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
            ui.add_space(gap_above(*kind, *indent, style));
            let left = indent_px(*indent, style);
            let (job, links) = text_job(
                spans,
                *kind,
                *align,
                first_line_px(*indent, style),
                style,
                cx.families,
                ui.available_width() - left,
                Some(ui.ctx()),
            );
            let mut galley = layout_text(ui, job, *align);
            let base = cx.part(&mut galley);
            let behind = ui.painter().add(egui::Shape::Noop);
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
            decorate(
                ui.painter(),
                behind,
                &galley,
                galley_pos,
                spans,
                style,
                *kind,
            );
            cx.record(base, galley.clone(), galley_pos, resp.rect);
            if let Some(slot) = cx.galley.take() {
                *slot = Some((galley.clone(), resp.rect.top()));
            }
            ui.add_space(gap_below(*kind, *indent, style));
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
        Block::Image { src, .. } if crate::math::parse_image_src(src).is_some() => {
            let latex = crate::math::parse_image_src(src).unwrap_or_default();
            display_math(ui, &latex, style);
            ui.add_space(style.size * PARAGRAPH_GAP);
        }
        Block::Image {
            src,
            alt,
            title,
            width,
            fraction,
            ..
        } => {
            match crate::svg::image_uri(ui.ctx(), src).or_else(|| cx.resolver.image(ui.ctx(), src))
            {
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
            }
        }
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
            // Lay every cell out first (in reading order, which `part` counts in), so each
            // column's width is known before aligning its cells.
            let mut laid: Vec<Vec<LaidCell>> = Vec::new();
            let mut col_widths = vec![0.0f32; columns];
            for (r, row) in rows.iter().enumerate() {
                let mut cells = Vec::new();
                for (c, cell) in row.iter().enumerate() {
                    let mut spans = cell.clone();
                    if r < *header_rows {
                        spans.iter_mut().for_each(|s| s.style.bold = true);
                    }
                    let align = cell
                        .first()
                        .and_then(|s| s.style.cell_align)
                        .unwrap_or_default();
                    let (job, _) = text_job(
                        &spans,
                        TextKind::Paragraph,
                        Align::Left,
                        0.0,
                        style,
                        cx.families,
                        cell_width,
                        Some(ui.ctx()),
                    );
                    // A galley keeps the cell's own wrap width (a job would be re-wrapped to the grid's).
                    let mut galley = ui.fonts_mut(|f| f.layout_job(job));
                    let base = cx.part(&mut galley);
                    col_widths[c] = col_widths[c].max(galley.size().x);
                    cells.push(LaidCell {
                        galley,
                        base,
                        align,
                        spans,
                    });
                }
                laid.push(cells);
            }
            egui::ScrollArea::horizontal()
                .id_salt(("table", id))
                .show(ui, |ui| {
                    egui::Grid::new(("grid", id))
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for row in laid {
                                for (c, cell) in row.into_iter().enumerate() {
                                    let LaidCell {
                                        galley,
                                        base,
                                        align,
                                        spans,
                                    } = cell;
                                    let width = col_widths[c].max(galley.size().x);
                                    let x = match align {
                                        Align::Left | Align::Justify => egui::Align::Min,
                                        Align::Center => egui::Align::Center,
                                        Align::Right => egui::Align::Max,
                                    };
                                    let behind = ui.painter().add(egui::Shape::Noop);
                                    let resp = ui
                                        .allocate_ui_with_layout(
                                            egui::vec2(width, galley.size().y),
                                            egui::Layout::top_down(x),
                                            |ui| {
                                                ui.set_width(width);
                                                ui.add(
                                                    Label::new(egui::WidgetText::Galley(
                                                        galley.clone(),
                                                    ))
                                                    .selectable(false),
                                                )
                                            },
                                        )
                                        .inner;
                                    let pos = resp.rect.left_top();
                                    decorate(
                                        ui.painter(),
                                        behind,
                                        &galley,
                                        pos,
                                        &spans,
                                        style,
                                        TextKind::Paragraph,
                                    );
                                    cx.record(base, galley, pos, resp.rect);
                                }
                                ui.end_row();
                            }
                        });
                });
            ui.add_space(style.size * PARAGRAPH_GAP);
        }
    }
}

/// A display formula, centred: its rendering tinted with the text colour, scaled down to fit the
/// column, or its source as code while it renders or when it can't be.
fn display_math(ui: &mut Ui, latex: &str, style: &RichStyle) {
    use crate::math::{MathImage, MathMode};
    match crate::math::image(ui.ctx(), latex, MathMode::Display, style.size) {
        MathImage::Ready(texture) => {
            let column = ui.available_width().max(1.0);
            let size = if texture.size.x > column {
                texture.size * (column / texture.size.x)
            } else {
                texture.size
            };
            ui.vertical_centered(|ui| {
                let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
                ui.painter().image(
                    texture.id,
                    rect,
                    Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    style.text,
                );
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, latex));
            });
        }
        MathImage::Pending | MathImage::Unavailable => {
            ui.vertical_centered(|ui| {
                ui.add(
                    Label::new(
                        egui::RichText::new(crate::math::fallback_text(latex, MathMode::Display))
                            .font(crate::math::fallback_font(style.size))
                            .color(style.text)
                            .background_color(style.code_bg),
                    )
                    .wrap(),
                );
            });
        }
    }
}

/// A table cell laid out ahead of the grid: its galley, its part's offset, alignment and spans.
struct LaidCell {
    galley: Arc<Galley>,
    base: usize,
    align: Align,
    spans: Vec<Span>,
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

/// Draws what a layout job can't: task checkboxes over their `[ ]` / `[x]` text, and key-cap
/// frames around `<kbd>` text. `pos` is where `galley`'s origin is on screen; `spans` are the
/// ones the galley was laid out from (in order). `behind` is a shape slot reserved before the
/// text was added (`Painter::add(Shape::Noop)`): key-cap fills go there, under the glyphs.
fn decorate(
    painter: &egui::Painter,
    behind: egui::layers::ShapeIdx,
    galley: &Galley,
    pos: Pos2,
    spans: &[Span],
    style: &RichStyle,
    kind: TextKind,
) {
    let mut under = Vec::new();
    let mut start = 0;
    for span in spans {
        let end = start + span.text.chars().count();
        let (a, b) = (start, end);
        start = end;
        let st = span.style;
        if let Some(mode) = st.math {
            paint_math(painter, galley, pos, span, mode, a..b, style, kind);
            continue;
        }
        if st.checkbox.is_none() && !st.kbd {
            continue;
        }
        let ra = galley.pos_from_cursor(CCursor::new(a));
        let rb = galley.pos_from_cursor(CCursor::new(b));
        if (ra.top() - rb.top()).abs() > 0.5 {
            continue; // Wrapped over two rows: leave it plain.
        }
        let range = Rect::from_min_max(
            pos + egui::vec2(ra.left(), ra.top()),
            pos + egui::vec2(rb.left(), ra.bottom()),
        );
        if let Some(checked) = st.checkbox {
            paint_checkbox(painter, range, checked, style);
        } else {
            // The glyphs' box: the row minus its line-height padding.
            let pad = (range.height() - style.size * 1.15).max(0.0) / 2.0;
            let frame = Rect::from_min_max(
                range.min + egui::vec2(-2.0, pad),
                range.max + egui::vec2(2.0, -pad),
            );
            under.push(egui::Shape::rect_filled(frame, 3.0, style.code_bg));
            under.push(egui::Shape::rect_stroke(
                frame,
                3.0,
                Stroke::new(1.0, style.kbd_border),
                egui::StrokeKind::Inside,
            ));
        }
    }
    if !under.is_empty() {
        painter.set(behind, egui::Shape::Vec(under));
    }
}

/// An inline formula over its placeholder (`chars` in `galley`), if `text_job` laid one out.
#[allow(clippy::too_many_arguments)]
fn paint_math(
    painter: &egui::Painter,
    galley: &Galley,
    pos: Pos2,
    span: &Span,
    mode: MathMode,
    chars: Range<usize>,
    style: &RichStyle,
    kind: TextKind,
) {
    let placeholder = galley
        .job
        .text
        .chars()
        .nth(chars.start)
        .is_some_and(|c| c == crate::math::NBSP);
    if !placeholder {
        return; // Laid out as its source.
    }
    let size = span_size(&span.style, heading_scale(kind).0, style);
    let crate::math::MathImage::Ready(texture) =
        crate::math::image(painter.ctx(), &span.text, mode, size)
    else {
        return;
    };
    let ra = galley.pos_from_cursor(CCursor::new(chars.start));
    let rb = galley.pos_from_cursor(CCursor::new(chars.end));
    let rect = Rect::from_min_max(
        pos + egui::vec2(ra.left(), ra.top()),
        pos + egui::vec2(rb.left(), ra.bottom()),
    );
    let color = if span.link.is_some() {
        style.link
    } else {
        style.text
    };
    crate::math::paint(painter, rect, texture, color);
}

/// A checkbox centred in `range` (the `[ ]` text's box), sized to the body text.
fn paint_checkbox(painter: &egui::Painter, range: Rect, checked: bool, style: &RichStyle) {
    let side = (style.size * 0.85).min(range.height());
    let rect = Rect::from_center_size(range.center(), egui::vec2(side, side));
    let radius = (side * 0.2).round();
    if checked {
        painter.rect_filled(rect, radius, style.check_fill);
        let w = side * 0.12;
        let tick = [
            rect.left_top() + egui::vec2(side * 0.24, side * 0.52),
            rect.left_top() + egui::vec2(side * 0.43, side * 0.70),
            rect.left_top() + egui::vec2(side * 0.77, side * 0.32),
        ];
        painter.line_segment([tick[0], tick[1]], Stroke::new(w, style.check_mark));
        painter.line_segment([tick[1], tick[2]], Stroke::new(w, style.check_mark));
    } else {
        painter.rect_stroke(
            rect,
            radius,
            Stroke::new(1.5, style.check_border),
            egui::StrokeKind::Inside,
        );
    }
}

/// A span's text size: the block's `scale` (headings), small text and super/subscripts.
fn span_size(st: &crate::Style, scale: f32, style: &RichStyle) -> f32 {
    let script = st.sup || st.sub;
    style.size * scale * if st.small { 0.8 } else { 1.0 } * if script { SCRIPT_SCALE } else { 1.0 }
}

/// The layout job for a run of spans, and the character ranges that are links. `first_line` is
/// the first line's indent, in points.
/// `ctx` lays formulas out as their rendered size (see [`crate::math`]); without it, or while
/// they render, formulas show their source.
#[allow(clippy::too_many_arguments)]
pub fn text_job(
    spans: &[Span],
    kind: TextKind,
    align: Align,
    first_line: f32,
    style: &RichStyle,
    families: &Families,
    width: f32,
    ctx: Option<&egui::Context>,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let (scale, heading) = heading_scale(kind);
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    job.halign = match align {
        Align::Left | Align::Justify => egui::Align::LEFT,
        Align::Center => egui::Align::Center,
        Align::Right => egui::Align::RIGHT,
    };
    // `Align::Justify` is laid out left-aligned here; `layout_text` then widens the word gaps.
    // Negative is a hanging first line (see `layout_text`).
    let mut leading = first_line;
    let mut links = Vec::new();
    let mut chars = 0;
    for span in spans {
        let st = span.style;
        let bold = st.bold || heading;
        let italic = st.italic || kind == TextKind::Caption;
        let size = span_size(&st, scale, style);
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
        if st.mark {
            color = style.mark_text;
        }
        if span.link.is_some() {
            color = style.link;
        }
        if st.checkbox.is_some() {
            // The `[ ]` / `[x]` text keeps its place (and positions); `decorate` draws the box.
            color = Color32::TRANSPARENT;
        }
        let mut format = TextFormat {
            font_id: FontId::new(if st.code { size * 0.9 } else { size }, font.family),
            line_height: Some(size * style.line_height),
            color,
            background: if st.mark {
                style.mark_bg
            } else if st.code && !st.kbd {
                style.code_bg
            } else {
                Color32::TRANSPARENT
            },
            // Keep the code background to the glyphs rather than the whole (1.5×) line.
            expand_bg: 0.0,
            italics: font.fake_italic,
            underline: if st.checkbox.is_none() && (st.underline || span.link.is_some()) {
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
        if let Some(mode) = st.math {
            // Rendered: invisible no-break spaces the formula is painted over (`decorate`).
            if let Some((text, placeholder, _)) =
                ctx.and_then(|c| crate::math::inline_placeholder(c, &span.text, mode, &format))
            {
                job.append(&text, std::mem::take(&mut leading), placeholder);
                continue;
            }
            // Not (yet) rendered: the source, as code, with the same characters.
            format.font_id = crate::math::fallback_font(size);
            format.background = style.code_bg;
        }
        // A checkbox is laid out as the same three characters whatever its state, so checked and
        // unchecked boxes line up; the model keeps `[ ]` / `[x]` for copying.
        let text = match st.checkbox {
            Some(_) if n == CHECKBOX_TEXT.chars().count() => CHECKBOX_TEXT,
            _ => span.text.as_str(),
        };
        job.append(text, std::mem::take(&mut leading), format);
    }
    (job, links)
}

/// Lays out a text block's job: `Align::Justify` rows get word spacing (see [`justify`]).
///
/// A hanging first line (a negative `first_line` given to [`text_job`]) starts left of the
/// others and is as much wider, as in CSS. The galley's rect still starts at 0, so a label
/// keeps the column's width and the first line draws into the block's left margin (which the
/// callers never let it pass).
pub fn layout_text(ui: &Ui, job: LayoutJob, align: Align) -> Arc<Galley> {
    let mut galley = ui.fonts_mut(|f| f.layout_job(job.clone()));
    if align == Align::Justify {
        if let Some(justified) = justify(&job, &galley) {
            let laid = ui.fonts_mut(|f| f.layout_job(justified));
            // Same rows, or the first layout (the widened spaces must not move a line break).
            let same = laid.rows.len() == galley.rows.len()
                && laid
                    .rows
                    .iter()
                    .zip(&galley.rows)
                    .all(|(a, b)| a.row.glyphs.len() == b.row.glyphs.len());
            if same {
                galley = laid;
            }
        }
    }
    if galley.rect.min.x < 0.0 {
        let mut g = (*galley).clone();
        g.rect.min.x = 0.0;
        galley = Arc::new(g);
    }
    galley
}

/// `job` with its justified rows filled to the wrap width by widening the gaps between words
/// (never between letters): every row but a paragraph's last, and not before a line break, as
/// CSS does. The extra space is the `leading_space` of a section starting at each word that
/// follows a space, so the text, and every position in it, stays the same. `None` when no row
/// needs it.
///
/// egui's own `LayoutJob::justify` puts whole pixels between every pair of letters once a row is
/// short by a pixel per letter, which reads as letter-spacing on sparse lines.
fn justify(job: &LayoutJob, galley: &Galley) -> Option<LayoutJob> {
    use egui::epaint::text::ByteIndex;
    let width = job.wrap.max_width;
    if !width.is_finite() || galley.rows.len() < 2 {
        return None;
    }
    let byte_of: Vec<usize> = job.text.char_indices().map(|(b, _)| b).collect();
    let mut widen: Vec<(usize, f32)> = Vec::new();
    let mut slack = 1.0f32;
    let mut first_char = 0;
    for (i, placed) in galley.rows.iter().enumerate() {
        let glyphs = &placed.row.glyphs;
        let last = i + 1 == galley.rows.len() || placed.ends_with_newline;
        if !last {
            if let Some(end) = glyphs.iter().rposition(|g| !g.chr.is_whitespace()) {
                let right = glyphs[end].pos.x + glyphs[end].advance_width;
                let full = glyphs.last().map_or(right, |g| g.pos.x + g.advance_width);
                slack = slack.max(full - right + 1.0);
                let words: Vec<usize> = (1..=end)
                    .filter(|&g| glyphs[g - 1].chr == ' ' && glyphs[g].chr != ' ')
                    .collect();
                let extra = width - right;
                if !words.is_empty() && extra > 0.0 {
                    let each = extra / words.len() as f32;
                    widen.extend(
                        words
                            .iter()
                            .filter_map(|&g| byte_of.get(first_char + g).map(|&b| (b, each))),
                    );
                }
            }
        }
        first_char += glyphs.len() + usize::from(placed.ends_with_newline);
    }
    if widen.is_empty() {
        return None;
    }
    let mut out = job.clone();
    out.wrap.max_width = width + slack;
    out.sections.clear();
    let mut next = widen.iter().peekable();
    for section in &job.sections {
        let range = section.byte_range.start.0..section.byte_range.end.0;
        let mut start = range.start;
        let mut leading = section.leading_space;
        while let Some(&&(b, each)) = next.peek() {
            if b >= range.end {
                break;
            }
            next.next();
            if b < start {
                continue;
            }
            if b == start {
                leading += each;
                continue;
            }
            out.sections.push(egui::text::LayoutSection {
                leading_space: leading,
                byte_range: ByteIndex(start)..ByteIndex(b),
                format: section.format.clone(),
            });
            (start, leading) = (b, each);
        }
        out.sections.push(egui::text::LayoutSection {
            leading_space: leading,
            byte_range: ByteIndex(start)..ByteIndex(range.end),
            format: section.format.clone(),
        });
    }
    Some(out)
}

/// The space above a text block: its own `margin-top` when the document sets one, else the
/// heading gap.
fn gap_above(kind: TextKind, indent: Indent, style: &RichStyle) -> f32 {
    indent
        .top
        .map_or_else(|| heading_gap(kind, style), |m| m * style.size)
}

/// The space below a text block: its own `margin-bottom` when set, else the paragraph gap
/// (captions sit on what follows).
fn gap_below(kind: TextKind, indent: Indent, style: &RichStyle) -> f32 {
    match indent.bottom {
        Some(m) => m * style.size,
        None if kind == TextKind::Caption => 0.0,
        None => style.size * PARAGRAPH_GAP,
    }
}

/// The first line's indent in points: `text-indent`, where a negative (hanging) one reaches at
/// most back to the column's edge.
fn first_line_px(indent: Indent, style: &RichStyle) -> f32 {
    (indent.first_line * style.size).max(-indent_px(indent, style))
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

    const LOREM: &str = "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod \
        tempor incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud \
        exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat.";

    /// Lays `LOREM` out at `width` as left-aligned and justified galleys (and with `hang`).
    fn lorem(width: f32, hang: f32) -> (Arc<Galley>, Arc<Galley>) {
        let ctx = ctx();
        let spans = [Span {
            text: LOREM.into(),
            style: crate::model::Style::default(),
            link: None,
        }];
        let mut out = None;
        for _ in 0..2 {
            // The first pass loads the fonts.
            ctx.run_ui(RawInput::default(), |ui| {
                let style = RichStyle::from_ctx(ui.ctx());
                let fams = Families::of(ui.ctx());
                let job = |align| {
                    text_job(
                        &spans,
                        TextKind::Paragraph,
                        align,
                        -hang,
                        &style,
                        &fams,
                        width,
                        None,
                    )
                    .0
                };
                out = Some((
                    layout_text(ui, job(Align::Left), Align::Left),
                    layout_text(ui, job(Align::Justify), Align::Justify),
                ));
            })
            .drop_without_applying_deltas();
        }
        out.expect("laid out")
    }

    fn content_right(row: &egui::epaint::text::PlacedRow) -> f32 {
        let g = row
            .row
            .glyphs
            .iter()
            .rfind(|g| !g.chr.is_whitespace())
            .expect("glyph");
        row.pos.x + g.pos.x + g.advance_width
    }

    #[test]
    fn justify_spreads_words_not_letters() {
        let width = 320.0;
        let (left, just) = lorem(width, 0.0);
        assert!(left.rows.len() > 2, "{}", left.rows.len());
        assert_eq!(left.rows.len(), just.rows.len());
        let last = just.rows.len() - 1;
        for (i, (l, j)) in left.rows.iter().zip(&just.rows).enumerate() {
            assert_eq!(l.row.glyphs.len(), j.row.glyphs.len(), "row {i}");
            if i < last {
                // Within egui's pixel snapping of the glyphs.
                assert!(
                    (content_right(j) - width).abs() < 1.5,
                    "row {i} ends at {} of {width}",
                    content_right(j)
                );
            } else {
                assert!(
                    (content_right(j) - content_right(l)).abs() < 0.01,
                    "last row ragged"
                );
            }
            // Inside a word, letters keep their spacing (to within egui's pixel snapping of
            // each glyph; egui's own justify adds whole pixels between every pair).
            for (a, b) in l.row.glyphs.windows(2).zip(j.row.glyphs.windows(2)) {
                if !a[0].chr.is_whitespace() && !a[1].chr.is_whitespace() {
                    let dl = a[1].pos.x - a[0].pos.x;
                    let dj = b[1].pos.x - b[0].pos.x;
                    assert!(
                        (dl - dj).abs() <= 1.01,
                        "row {i}: {:?}{:?}",
                        a[0].chr,
                        a[1].chr
                    );
                }
            }
        }
    }

    #[test]
    fn hanging_first_line_starts_left_and_is_wider() {
        let width = 320.0;
        let (hung, hung_justified) = lorem(width, 24.0);
        let first_x = |g: &Galley| g.rows[0].pos.x + g.rows[0].row.glyphs[0].pos.x;
        let second_x = |g: &Galley| g.rows[1].pos.x + g.rows[1].row.glyphs[0].pos.x;
        assert!((first_x(&hung) - (second_x(&hung) - 24.0)).abs() < 1.01);
        // The galley stays column-wide, starting at 0.
        assert_eq!(hung.rect.min.x, 0.0);
        // Justified, the hanging line still ends at the column's edge.
        assert!((content_right(&hung_justified.rows[0]) - width).abs() < 1.5);
        assert!((first_x(&hung_justified) - first_x(&hung)).abs() < 0.01);
        // Positions still map through the hanging row.
        let g = &hung.rows[0].row.glyphs[3];
        let at = hung.rows[0].pos + g.pos.to_vec2() + egui::vec2(1.0, 2.0);
        assert_eq!(hung.cursor_from_pos(at.to_vec2()).index.0, 3);
    }

    /// `cargo test -p marina-richtext --release justify_cost -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn justify_cost() {
        let ctx = ctx();
        let paragraphs: Vec<[Span; 1]> = (0..300)
            .map(|i| {
                [Span {
                    text: format!("{i} {LOREM}"),
                    style: crate::model::Style::default(),
                    link: None,
                }]
            })
            .collect();
        for align in [Align::Left, Align::Justify] {
            let mut took = Duration::ZERO;
            ctx.run_ui(RawInput::default(), |_| {})
                .drop_without_applying_deltas();
            ctx.run_ui(RawInput::default(), |ui| {
                let style = RichStyle::from_ctx(ui.ctx());
                let fams = Families::of(ui.ctx());
                let start = Instant::now();
                for spans in &paragraphs {
                    let job = text_job(
                        spans,
                        TextKind::Paragraph,
                        align,
                        0.0,
                        &style,
                        &fams,
                        520.0,
                        None,
                    )
                    .0;
                    let _ = layout_text(ui, job, align);
                }
                took = start.elapsed();
            })
            .drop_without_applying_deltas();
            println!(
                "{align:?}: 300 paragraphs in {:.1} ms",
                took.as_secs_f64() * 1000.0
            );
        }
    }
}
