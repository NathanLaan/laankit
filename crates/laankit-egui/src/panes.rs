//! The pane stack, natively: `panels/PaneHost.svelte`.
//!
//! A vertical stack of panes, each with a header (title, optional extra
//! buttons, close button) and a body the app draws. Drag a header onto
//! another to reorder; drag the 4 px divider between two panes to resize
//! them. The last pane grows to fill the space left over.
//!
//! Which panes show is up to the app: it passes the visible [`Pane`]s each
//! frame. The order and heights live in [`PaneLayout`], which the app
//! persists (it's serde). The layout logic is pure functions
//! ([`visible_order`], [`reorder`], [`resize_pair`], [`clamp_heights`]) so it
//! can be tested without a UI.

use std::collections::HashMap;

use egui::{pos2, vec2, Align2, Id, Rect, Sense, Stroke, Ui, UiBuilder};
use serde::{Deserialize, Serialize};

use crate::icons;
use crate::theme::{self, Palette};

/// `.pane-header { min-height: 44px }`.
pub const HEADER_HEIGHT: f32 = 44.0;
/// `.pane-resizer { height: 4px }`.
pub const RESIZER_HEIGHT: f32 = 4.0;
/// A pane's default minimum height: just its header.
pub const DEFAULT_MIN_HEIGHT: f32 = HEADER_HEIGHT;
/// Height for a pane the layout has never seen.
const DEFAULT_HEIGHT: f32 = 200.0;

/// A pane to show this frame.
#[derive(Clone, Debug)]
pub struct Pane<'a> {
    /// Stable across frames and sessions.
    pub id: &'a str,
    pub title: &'a str,
    /// Defaults to [`DEFAULT_MIN_HEIGHT`].
    pub min_height: f32,
    /// Hides the close button when `false`.
    pub closable: bool,
}

impl<'a> Pane<'a> {
    pub fn new(id: &'a str, title: &'a str) -> Self {
        Self {
            id,
            title,
            min_height: DEFAULT_MIN_HEIGHT,
            closable: true,
        }
    }

    pub fn min_height(mut self, h: f32) -> Self {
        self.min_height = h;
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

/// The persisted layout: pane order and stored heights (the Svelte
/// `order` prop plus each pane's `height`).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PaneLayout {
    pub order: Vec<String>,
    pub heights: HashMap<String, f32>,
}

impl PaneLayout {
    pub fn height(&self, id: &str) -> f32 {
        self.heights.get(id).copied().unwrap_or(DEFAULT_HEIGHT)
    }
}

/// What the user did this frame.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PaneHostOutput {
    /// The close button of this pane was clicked (`onClosePane`).
    pub closed: Option<String>,
    /// The layout changed (a drag finished or a divider moved); persist it.
    pub layout_changed: bool,
}

/// Drop position relative to the pane under the pointer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DropSide {
    Before,
    After,
}

/// The visible panes' ids in display order: `order` first, then any
/// visible pane missing from it, at the end.
pub fn visible_order<'p>(order: &[String], panes: &'p [Pane<'p>]) -> Vec<&'p str> {
    let mut out: Vec<&str> = order
        .iter()
        .filter_map(|id| panes.iter().find(|p| p.id == id.as_str()).map(|p| p.id))
        .collect();
    for p in panes {
        if !out.contains(&p.id) {
            out.push(p.id);
        }
    }
    out
}

/// `order` with `dragged` moved before or after `target`. Unchanged when
/// either is missing or they're the same pane.
pub fn reorder(order: &[String], dragged: &str, target: &str, side: DropSide) -> Vec<String> {
    if dragged == target || !order.iter().any(|id| id == dragged) {
        return order.to_vec();
    }
    let mut out: Vec<String> = order.iter().filter(|id| *id != dragged).cloned().collect();
    let Some(at) = out.iter().position(|id| id == target) else {
        return order.to_vec();
    };
    let insert = if side == DropSide::Before { at } else { at + 1 };
    out.insert(insert, dragged.to_string());
    out
}

/// Moves the divider between two panes by `delta`, keeping their combined
/// height and both minimums. Returns the new (above, below) heights.
pub fn resize_pair(start: (f32, f32), min: (f32, f32), delta: f32) -> (f32, f32) {
    let combined = start.0 + start.1;
    let mut above = start.0 + delta;
    let mut below = start.1 - delta;
    if above < min.0 {
        above = min.0;
        below = combined - above;
    } else if below < min.1 {
        below = min.1;
        above = combined - below;
    }
    (above, below)
}

/// Scales the heights down to fit `available` (after the dividers) when
/// they add up to more, never below each pane's minimum. Returns `true` if
/// anything changed.
pub fn clamp_heights(layout: &mut PaneLayout, panes: &[(&str, f32)], available: f32) -> bool {
    let budget = available - panes.len().saturating_sub(1) as f32 * RESIZER_HEIGHT;
    let sum: f32 = panes.iter().map(|(id, _)| layout.height(id)).sum();
    if sum <= budget || sum <= 0.0 {
        return false;
    }
    let scale = budget / sum;
    for (id, min) in panes {
        let scaled = (layout.height(id) * scale).floor().max(*min);
        layout.heights.insert(id.to_string(), scaled);
    }
    true
}

/// A drag in progress, kept in egui memory between frames.
#[derive(Clone, Debug, Default)]
struct DragState {
    /// The pane whose header is being dragged.
    pane: Option<String>,
    /// The divider being dragged: (index of the pane below, start heights).
    resize: Option<(usize, (f32, f32))>,
}

/// Draws the pane stack in the available space. `body(ui, pane_id)` draws
/// a pane's body; `header_extra(ui, pane_id)` adds buttons left of the
/// close button (`headerExtra`).
pub fn show(
    ui: &mut Ui,
    id: Id,
    layout: &mut PaneLayout,
    panes: &[Pane<'_>],
    mut body: impl FnMut(&mut Ui, &str),
    mut header_extra: impl FnMut(&mut Ui, &str),
) -> PaneHostOutput {
    let p = theme::palette(ui.ctx());
    let mut out = PaneHostOutput::default();
    let ids = visible_order(&layout.order, panes);
    // Keep the stored order complete, so a reorder can place any pane.
    for pane_id in &ids {
        if !layout.order.iter().any(|o| o == pane_id) {
            layout.order.push(pane_id.to_string());
        }
    }
    let pane = |pid: &str| panes.iter().find(|p| p.id == pid).expect("visible pane");
    let host = ui.available_rect_before_wrap();
    let mins: Vec<(&str, f32)> = ids.iter().map(|pid| (*pid, pane(pid).min_height)).collect();
    out.layout_changed |= clamp_heights(layout, &mins, host.height());

    // Rendered heights: stored ones, with the last pane taking what's left.
    let mut heights: Vec<f32> = ids.iter().map(|pid| layout.height(pid)).collect();
    if let Some(last) = heights.len().checked_sub(1) {
        let others: f32 = heights[..last].iter().sum::<f32>() + last as f32 * RESIZER_HEIGHT;
        heights[last] = heights[last].max(host.height() - others);
    }

    let mut drag: DragState = ui.ctx().data(|d| d.get_temp(id)).unwrap_or_default();
    let pointer = ui.ctx().pointer_interact_pos();
    let mut header_rects: Vec<Rect> = Vec::with_capacity(ids.len());
    let mut y = host.top();
    for (i, pane_id) in ids.iter().enumerate() {
        let spec = pane(pane_id);
        if i > 0 {
            let divider =
                Rect::from_min_size(pos2(host.left(), y), vec2(host.width(), RESIZER_HEIGHT));
            let resp = ui.interact(divider, id.with(("divider", i)), Sense::drag());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Splitter,
                    true,
                    format!("Resize {}", spec.title),
                )
            });
            let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
            ui.painter().rect_filled(
                divider,
                0.0,
                if resp.hovered() || resp.dragged() {
                    p.border_hover
                } else {
                    p.border
                },
            );
            if resp.drag_started() {
                drag.resize = Some((i, (heights[i - 1], heights[i])));
            }
            if resp.dragged() {
                if let Some((at, start)) = drag.resize.filter(|(at, _)| *at == i) {
                    let total = resp.total_drag_delta().map_or(0.0, |d| d.y);
                    let (above, below) = resize_pair(
                        start,
                        (pane(ids[at - 1]).min_height, spec.min_height),
                        total,
                    );
                    layout.heights.insert(ids[at - 1].to_string(), above);
                    layout.heights.insert(spec.id.to_string(), below);
                    heights[at - 1] = above;
                    heights[at] = below;
                    out.layout_changed = true;
                }
            }
            if resp.drag_stopped() {
                drag.resize = None;
            }
            y += RESIZER_HEIGHT;
        }
        let rect = Rect::from_min_size(pos2(host.left(), y), vec2(host.width(), heights[i]));
        let header = Rect::from_min_size(
            rect.min,
            vec2(rect.width(), HEADER_HEIGHT.min(rect.height())),
        );
        header_rects.push(header);
        let closed = draw_header(ui, &p, id, spec, header, &mut drag, &mut header_extra);
        if closed {
            out.closed = Some(spec.id.to_string());
        }
        let body_rect = Rect::from_min_max(pos2(rect.left(), header.bottom()), rect.max);
        if body_rect.height() > 0.0 {
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id_salt(("pane-body", spec.id))
                    .max_rect(body_rect),
            );
            child.set_clip_rect(body_rect.intersect(ui.clip_rect()));
            body(&mut child, spec.id);
        }
        y = rect.bottom();
    }

    // Header drag: show where it would land, and move it on release.
    if let (Some(dragged), Some(pos)) = (drag.pane.clone(), pointer) {
        let target = ids
            .iter()
            .zip(&header_rects)
            .find(|(pid, r)| **pid != dragged && r.contains(pos));
        if let Some((target, r)) = target {
            let side = if pos.y < r.center().y {
                DropSide::Before
            } else {
                DropSide::After
            };
            let line_y = if side == DropSide::Before {
                r.top() + 1.0
            } else {
                r.bottom() - 1.0
            };
            ui.painter()
                .hline(r.x_range(), line_y, Stroke::new(2.0, p.accent));
            if ui.input(|i| i.pointer.any_released()) {
                let new_order = reorder(&layout.order, &dragged, target, side);
                if new_order != layout.order {
                    layout.order = new_order;
                    out.layout_changed = true;
                }
            }
        }
    }
    if ui.input(|i| i.pointer.any_released()) {
        drag.pane = None;
    }
    ui.ctx().data_mut(|d| d.insert_temp(id, drag));
    ui.allocate_rect(host, Sense::hover());
    out
}

/// `.pane-header`: title, extra buttons and close button on the base
/// background, with a hairline below. Returns `true` when closed.
fn draw_header(
    ui: &mut Ui,
    p: &Palette,
    id: Id,
    spec: &Pane<'_>,
    rect: Rect,
    drag: &mut DragState,
    header_extra: &mut impl FnMut(&mut Ui, &str),
) -> bool {
    let resp = ui.interact(rect, id.with(("header", spec.id)), Sense::drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Heading, true, spec.title));
    let dragging = drag.pane.as_deref() == Some(spec.id);
    let resp = resp.on_hover_cursor(if dragging {
        egui::CursorIcon::Grabbing
    } else {
        egui::CursorIcon::Grab
    });
    if resp.drag_started() {
        drag.pane = Some(spec.id.to_string());
    }
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, p.bg_base);
    painter.hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let ink = if dragging {
        p.text_muted.gamma_multiply(0.5)
    } else {
        p.text_muted
    };
    let title = crate::settings::semibold_galley(ui, spec.title, 11.0, ink);
    painter.galley(
        pos2(rect.left() + 12.0, rect.center().y - title.size().y / 2.0),
        title,
        ink,
    );

    // Actions, right to left: close, then the app's extras.
    let mut closed = false;
    let mut right = rect.right() - 12.0;
    if spec.closable {
        let button = Rect::from_center_size(pos2(right - 12.0, rect.center().y), vec2(24.0, 24.0));
        if header_button(
            ui,
            p,
            id.with(("close", spec.id)),
            button,
            icons::XMARK,
            "Close pane",
        )
        .clicked()
        {
            closed = true;
        }
        right = button.left() - 2.0;
    }
    let extras = Rect::from_min_max(
        pos2(rect.left() + 12.0, rect.top()),
        pos2(right, rect.bottom()),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt(("pane-extra", spec.id))
            .max_rect(extras)
            .layout(egui::Layout::right_to_left(egui::Align::Center)),
    );
    child.spacing_mut().item_spacing.x = 2.0;
    header_extra(&mut child, spec.id);
    closed
}

/// `.pane-header-btn`: a 24 px icon button. Also used by apps for header
/// extras, so they match.
pub fn header_button(
    ui: &mut Ui,
    p: &Palette,
    id: Id,
    rect: Rect,
    glyph: &str,
    label: &str,
) -> egui::Response {
    let resp = ui.interact(rect, id, Sense::click()).on_hover_text(label);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, p.bg_button_hover);
    }
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        icons::font(11.0),
        if resp.hovered() {
            p.text_primary
        } else {
            p.text_muted
        },
    );
    resp
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn visible_order_skips_hidden_and_appends_new() {
        let panes = [
            Pane::new("c", "C"),
            Pane::new("a", "A"),
            Pane::new("new", "New"),
        ];
        let order = ids(&["a", "b", "c"]);
        assert_eq!(visible_order(&order, &panes), ["a", "c", "new"]);
    }

    #[test]
    fn reorder_before_and_after() {
        let order = ids(&["a", "b", "c", "d"]);
        assert_eq!(
            reorder(&order, "d", "b", DropSide::Before),
            ids(&["a", "d", "b", "c"])
        );
        assert_eq!(
            reorder(&order, "a", "c", DropSide::After),
            ids(&["b", "c", "a", "d"])
        );
        assert_eq!(reorder(&order, "a", "a", DropSide::After), order);
        assert_eq!(reorder(&order, "x", "a", DropSide::After), order);
        assert_eq!(reorder(&order, "a", "x", DropSide::After), order);
    }

    #[test]
    fn resize_keeps_total_and_minimums() {
        assert_eq!(
            resize_pair((100.0, 200.0), (44.0, 44.0), 50.0),
            (150.0, 150.0)
        );
        assert_eq!(
            resize_pair((100.0, 200.0), (44.0, 44.0), -80.0),
            (44.0, 256.0)
        );
        assert_eq!(
            resize_pair((100.0, 200.0), (44.0, 60.0), 500.0),
            (240.0, 60.0)
        );
    }

    #[test]
    fn clamp_scales_down_to_fit() {
        let mut layout = PaneLayout::default();
        layout.heights.insert("a".into(), 400.0);
        layout.heights.insert("b".into(), 400.0);
        let panes = [("a", 44.0), ("b", 44.0)];
        assert!(!clamp_heights(&mut layout, &panes, 1000.0));
        assert!(clamp_heights(&mut layout, &panes, 404.0));
        assert_eq!(layout.height("a"), 200.0);
        assert_eq!(layout.height("b"), 200.0);
        // Never below the minimum.
        assert!(clamp_heights(&mut layout, &panes, 50.0));
        assert_eq!(layout.height("a"), 44.0);
    }

    #[test]
    fn serde_round_trip() {
        let mut layout = PaneLayout {
            order: ids(&["a", "b"]),
            ..Default::default()
        };
        layout.heights.insert("a".into(), 120.0);
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<PaneLayout>(&json).unwrap(), layout);
    }

    #[test]
    fn close_button_and_bodies_render() {
        struct S {
            layout: PaneLayout,
            closed: Option<String>,
        }
        let mut h = Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(300.0, 500.0))
            .build_ui_state(
                |ui, s: &mut S| {
                    let panes = [
                        Pane::new("files", "FILES").closable(false),
                        Pane::new("tags", "TAGS"),
                    ];
                    let out = show(
                        ui,
                        Id::unique("panes"),
                        &mut s.layout,
                        &panes,
                        |ui, id| {
                            ui.label(format!("{id} body"));
                        },
                        |_, _| {},
                    );
                    if out.closed.is_some() {
                        s.closed = out.closed;
                    }
                },
                S {
                    layout: PaneLayout::default(),
                    closed: None,
                },
            );
        h.run();
        h.get_by_label("files body");
        h.get_by_label("tags body");
        assert_eq!(h.state().layout.order, ids(&["files", "tags"]));
        // Only the closable pane has a close button.
        h.get_by_label("Close pane").click();
        h.run();
        assert_eq!(h.state().closed.as_deref(), Some("tags"));
    }
}
