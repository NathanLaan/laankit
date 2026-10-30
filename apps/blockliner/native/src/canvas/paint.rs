//! Drawing the diagram and the editing overlays (selection, handles,
//! marquee, the shape being created) in theme colours.

use blockliner_core::geometry::Handle;
use blockliner_core::{Diagram, Rect, Shape, ShapeKind};
use egui::text::LayoutJob;
use egui::{vec2, Align, Color32, CornerRadius, FontId, Painter, Pos2, Stroke, StrokeKind};
use marina_egui::theme::Palette;

use super::view::View;
use super::{pos, tools::Hit};

/// Label text size at 100 %, in diagram pixels.
pub const LABEL_SIZE: f32 = 14.0;
/// Space between a label and its shape's sides, at 100 %.
pub const LABEL_PAD: f32 = 6.0;
/// The round rectangle's corner radius at 100 %.
const CORNER: f32 = 12.0;
/// Handles are this many screen points square, at any zoom.
pub const HANDLE_SIZE: f32 = 8.0;

pub fn label_font(zoom: f32) -> FontId {
    FontId::proportional(LABEL_SIZE * zoom)
}

pub fn screen_rect(view: View, origin: Pos2, r: &Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        view.to_screen(origin, pos(r.x, r.y)),
        view.to_screen(origin, pos(r.right(), r.bottom())),
    )
}

/// What the overlays need besides the diagram.
pub struct Overlays<'a> {
    pub selection: &'a [String],
    pub hover: Option<&'a Hit>,
    /// The shape whose label is being edited (its text field draws the label).
    pub editing: Option<&'a str>,
}

pub fn diagram(
    painter: &Painter,
    view: View,
    origin: Pos2,
    d: &Diagram,
    p: &Palette,
    o: &Overlays<'_>,
) {
    for s in &d.shapes {
        let r = screen_rect(view, origin, &s.rect);
        if !painter.clip_rect().intersects(r) {
            continue;
        }
        shape(
            painter,
            view.zoom,
            r,
            s,
            p,
            o.editing == Some(s.id.as_str()),
        );
    }
    if let Some(Hit::Shape(id) | Hit::Handle(id, _)) = o.hover {
        if !o.selection.contains(id) {
            if let Some(s) = d.shape(id) {
                let r = screen_rect(view, origin, &s.rect).expand(2.0);
                painter.rect_stroke(
                    r,
                    0.0,
                    Stroke::new(1.0, p.accent.gamma_multiply(0.5)),
                    StrokeKind::Middle,
                );
            }
        }
    }
    for id in o.selection {
        if let Some(s) = d.shape(id) {
            let r = screen_rect(view, origin, &s.rect).expand(2.0);
            painter.rect_stroke(r, 0.0, Stroke::new(1.5, p.accent), StrokeKind::Middle);
        }
    }
    if let [id] = o.selection {
        if let Some(s) = d.shape(id) {
            for h in Handle::ALL {
                let c = h.position(&s.rect);
                let at = view.to_screen(origin, pos(c.x, c.y));
                let r = egui::Rect::from_center_size(at, vec2(HANDLE_SIZE, HANDLE_SIZE));
                painter.rect(
                    r,
                    1.0,
                    p.bg_base,
                    Stroke::new(1.5, p.accent),
                    StrokeKind::Middle,
                );
            }
        }
    }
}

fn shape(painter: &Painter, zoom: f32, r: egui::Rect, s: &Shape, p: &Palette, editing: bool) {
    let stroke = Stroke::new((1.5 * zoom).clamp(1.0, 4.0), p.text_secondary);
    match &s.kind {
        ShapeKind::Rect => {
            painter.rect(r, 0.0, p.bg_surface, stroke, StrokeKind::Inside);
        }
        ShapeKind::RoundRect => {
            let radius = (CORNER * zoom).min(r.height() / 2.0).min(255.0);
            painter.rect(
                r,
                CornerRadius::same(radius as u8),
                p.bg_surface,
                stroke,
                StrokeKind::Inside,
            );
        }
        // Other Mermaid shapes are drawn as rectangles, dashed so they read
        // as "not quite this".
        ShapeKind::Other { .. } => {
            painter.rect_filled(r, 0.0, p.bg_surface);
            dashed_rect(painter, r, stroke);
        }
        // Bare text; an empty one keeps a faint outline so it can be found.
        ShapeKind::Text => {
            if s.label.is_empty() && !editing {
                dashed_rect(painter, r, Stroke::new(1.0, p.text_faint));
            }
        }
    }
    if !editing {
        label(painter, r, &s.label, zoom, p.text_primary);
    }
}

/// The label, centred line by line, wrapped to the shape and clipped to it.
fn label(painter: &Painter, r: egui::Rect, text: &str, zoom: f32, color: Color32) {
    let size = LABEL_SIZE * zoom;
    if text.is_empty() || size < 4.0 {
        return;
    }
    let wrap = (r.width() - 2.0 * LABEL_PAD * zoom).max(1.0);
    let mut job = LayoutJob::simple(text.to_owned(), label_font(zoom), color, wrap);
    job.halign = Align::Center;
    let galley = painter.layout_job(job);
    let at = r.center() - galley.rect.center().to_vec2();
    painter
        .with_clip_rect(r.intersect(painter.clip_rect()))
        .galley(at, galley, color);
}

pub fn dashed_rect(painter: &Painter, r: egui::Rect, stroke: Stroke) {
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    painter.extend(egui::Shape::dashed_line(&corners, stroke, 5.0, 4.0));
}

/// The box selection.
pub fn marquee(painter: &Painter, r: egui::Rect, p: &Palette) {
    painter.rect(
        r,
        0.0,
        p.accent.gamma_multiply(0.08),
        Stroke::new(1.0, p.accent),
        StrokeKind::Middle,
    );
}

/// The shape a drag will create.
pub fn preview(painter: &Painter, r: egui::Rect, kind: &ShapeKind, zoom: f32, p: &Palette) {
    let stroke = Stroke::new(1.5, p.accent);
    match kind {
        ShapeKind::RoundRect => {
            let radius = (CORNER * zoom).min(r.height() / 2.0).min(255.0);
            painter.rect_stroke(
                r,
                CornerRadius::same(radius as u8),
                stroke,
                StrokeKind::Middle,
            );
        }
        _ => dashed_rect(painter, r, stroke),
    }
}
