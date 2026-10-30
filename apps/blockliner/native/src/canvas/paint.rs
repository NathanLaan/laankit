//! Drawing the diagram and the editing overlays (selection, handles, anchor
//! hints, marquee, the shape or line being created) in theme colours.

use blockliner_core::connect::{anchor_point, path_midpoint};
use blockliner_core::geometry::{self, Handle, SUBROUTINE_INSET};
use blockliner_core::route::middle_segment;
use blockliner_core::{Anchor, ConnectorKind, Diagram, End, Rect, Shape, ShapeKind};
use egui::text::LayoutJob;
use egui::{vec2, Align, Color32, CornerRadius, FontId, Painter, Pos2, Stroke, StrokeKind};
use marina_egui::icons;
use marina_egui::theme::Palette;

use super::view::View;
use super::{pos, tools::Hit};
use crate::views::arrowhead;

/// Label text size at 100 %, in diagram pixels.
pub const LABEL_SIZE: f32 = 14.0;
/// Space between a label and its shape's sides, at 100 %.
pub const LABEL_PAD: f32 = 6.0;
/// Edge label text size at 100 %.
pub const EDGE_LABEL_SIZE: f32 = 12.0;
/// The round rectangle's corner radius at 100 %.
pub const CORNER: f32 = 12.0;
/// Arrowhead length at 100 %.
pub const ARROW: f32 = 12.0;
/// Handles are this many screen points square, at any zoom.
pub const HANDLE_SIZE: f32 = 8.0;
/// Endpoint handles and anchor hints: radius in screen points.
const DOT: f32 = 4.5;
/// The limitation badge's text size, in screen points at any zoom.
const BADGE_SIZE: f32 = 10.0;

pub fn label_font(zoom: f32) -> FontId {
    FontId::proportional(LABEL_SIZE * zoom)
}

pub fn screen_rect(view: View, origin: Pos2, r: &Rect) -> egui::Rect {
    egui::Rect::from_min_max(
        view.to_screen(origin, pos(r.x, r.y)),
        view.to_screen(origin, pos(r.right(), r.bottom())),
    )
}

fn stroke_width(zoom: f32) -> f32 {
    (1.5 * zoom).clamp(1.0, 4.0)
}

/// What the overlays need besides the diagram.
pub struct Overlays<'a> {
    pub selection: &'a [String],
    pub lines: &'a [usize],
    pub hover: Option<&'a Hit>,
    /// Where a line end being drawn or dragged would glue.
    pub target: Option<&'a End>,
    /// The shape whose label is being edited (its text field draws the label).
    pub editing: Option<&'a str>,
}

/// Shapes, then connectors on top, then the overlays.
pub fn diagram(
    painter: &Painter,
    view: View,
    origin: Pos2,
    d: &Diagram,
    p: &Palette,
    o: &Overlays<'_>,
) {
    let to_screen = |x: f32, y: f32| view.to_screen(origin, pos(x, y));
    for s in &d.shapes {
        let r = screen_rect(view, origin, &s.rect);
        if painter.clip_rect().intersects(r) {
            let area = screen_rect(view, origin, &s.label_rect());
            shape(
                painter,
                view.zoom,
                (r, area),
                s,
                p,
                o.editing == Some(s.id.as_str()),
            );
        }
    }

    let width = stroke_width(view.zoom);
    for (i, c) in d.connectors.iter().enumerate() {
        let Some(path) = d.connector_path(c) else {
            continue;
        };
        let points: Vec<Pos2> = path.iter().map(|q| to_screen(q.x, q.y)).collect();
        let selected = o.lines.contains(&i);
        if !selected && o.hover == Some(&Hit::Line(i)) {
            painter.add(egui::Shape::line(
                points.clone(),
                Stroke::new(width + 4.0, p.accent.gamma_multiply(0.3)),
            ));
        }
        let ink = if selected { p.accent } else { p.text_secondary };
        connector(painter, &points, c.kind, Stroke::new(width, ink), view.zoom);
        if let (Some(text), Some(at)) = (&c.label, path_midpoint(&path)) {
            edge_label(painter, to_screen(at.x, at.y), text, view.zoom, p);
        }
    }

    // Hover and selection outlines.
    if let Some(Hit::Shape(id) | Hit::Handle(id, _)) = o.hover {
        if !o.selection.contains(id) {
            if let Some(s) = d.shape(id) {
                let r = screen_rect(view, origin, &s.rect).expand(2.0);
                let stroke = Stroke::new(1.0, p.accent.gamma_multiply(0.5));
                painter.rect_stroke(r, 0.0, stroke, StrokeKind::Middle);
            }
        }
    }
    for id in o.selection {
        if let Some(s) = d.shape(id) {
            let r = screen_rect(view, origin, &s.rect).expand(2.0);
            painter.rect_stroke(r, 0.0, Stroke::new(1.5, p.accent), StrokeKind::Middle);
        }
    }

    // A single selected shape has resize handles; a single selected line has
    // endpoint handles, filled where the end is glued.
    match (o.selection, o.lines) {
        ([id], []) => {
            if let Some(s) = d.shape(id) {
                for h in Handle::ALL {
                    let c = h.position(&s.rect);
                    let r = egui::Rect::from_center_size(
                        to_screen(c.x, c.y),
                        vec2(HANDLE_SIZE, HANDLE_SIZE),
                    );
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
        ([], [i]) => {
            if let Some(c) = d.connectors.get(*i) {
                let path = d.connector_path(c).unwrap_or_default();
                // A right-angle route's middle segment can be dragged.
                if let Some(at) = c.route.is_elbow().then(|| middle_handle(&path)).flatten() {
                    let r = egui::Rect::from_center_size(
                        to_screen(at.x, at.y),
                        vec2(HANDLE_SIZE, HANDLE_SIZE),
                    );
                    painter.rect(
                        r,
                        1.0,
                        p.bg_base,
                        Stroke::new(1.5, p.accent),
                        StrokeKind::Middle,
                    );
                }
                if let (Some(&a), Some(&b)) = (path.first(), path.last()) {
                    for (end, at) in [(&c.from, a), (&c.to, b)] {
                        let fill = if end.shape().is_some() {
                            p.accent
                        } else {
                            p.bg_base
                        };
                        painter.circle(
                            to_screen(at.x, at.y),
                            DOT,
                            fill,
                            Stroke::new(1.5, p.accent),
                        );
                    }
                }
            }
        }
        _ => {}
    }

    // Anchor hints on the shape a line end would glue to: its four anchors,
    // the one it would take filled, or the whole shape outlined for dynamic
    // glue.
    if let Some(End::Glued { shape, anchor }) = o.target {
        if let Some(s) = d.shape(shape) {
            if anchor.is_none() {
                let r = screen_rect(view, origin, &s.rect).expand(3.0);
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, p.accent), StrokeKind::Middle);
            }
            for a in Anchor::ALL {
                let at = anchor_point(&s.rect, a);
                let fill = if *anchor == Some(a) {
                    p.accent
                } else {
                    p.bg_base
                };
                painter.circle(to_screen(at.x, at.y), DOT, fill, Stroke::new(1.5, p.accent));
            }
        }
    }
}

/// A style colour, if it has one BlockLiner can draw.
pub fn colour(c: Option<&blockliner_core::style::Colour>) -> Option<Color32> {
    let [r, g, b, a] = c?.rgba?;
    Some(Color32::from_rgba_unmultiplied(r, g, b, a))
}

/// How a colour swatch looks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SwatchLook {
    /// The theme's colour: an empty square.
    Default,
    /// A colour: a fill with its edge, or (`outline`) only the edge, for a
    /// stroke.
    Colour {
        fill: Color32,
        edge: Color32,
        outline: bool,
    },
    /// Different values: split diagonally.
    Mixed,
}

/// Preset `i` as a swatch: a fill shows with its paired stroke, as a shape
/// would; a stroke as an outline.
pub fn preset_look(i: usize, stroke: bool, p: &Palette) -> SwatchLook {
    use blockliner_core::style::{Colour, PRESETS};
    let c = |hex: &str| colour(Some(&Colour::parse(hex))).unwrap_or(p.text_faint);
    let preset = PRESETS[i];
    if stroke {
        SwatchLook::Colour {
            fill: p.bg_base,
            edge: c(preset.stroke),
            outline: true,
        }
    } else {
        SwatchLook::Colour {
            fill: c(preset.fill),
            edge: c(preset.stroke),
            outline: false,
        }
    }
}

/// A swatch square; `ring` marks it as the current value.
pub fn swatch(painter: &Painter, square: egui::Rect, look: SwatchLook, ring: bool, p: &Palette) {
    match look {
        SwatchLook::Default => {
            let edge = Stroke::new(1.0, p.text_faint);
            painter.rect_stroke(square, 2.0, edge, StrokeKind::Inside);
        }
        SwatchLook::Colour {
            fill,
            edge,
            outline,
        } => {
            let width = if outline { 2.5 } else { 1.5 };
            painter.rect(
                square,
                2.0,
                fill,
                Stroke::new(width, edge),
                StrokeKind::Inside,
            );
        }
        SwatchLook::Mixed => {
            painter.rect_filled(square, 2.0, p.bg_base);
            let half = vec![square.left_top(), square.right_top(), square.left_bottom()];
            painter.add(egui::Shape::convex_polygon(
                half,
                p.text_muted,
                Stroke::NONE,
            ));
            let edge = Stroke::new(1.0, p.text_faint);
            painter.rect_stroke(square, 2.0, edge, StrokeKind::Inside);
        }
    }
    if ring {
        let ring = Stroke::new(1.5, p.accent);
        painter.rect_stroke(square.expand(3.0), 4.0, ring, StrokeKind::Inside);
    }
}

/// `rects`: the shape's and its label's, on screen.
fn shape(
    painter: &Painter,
    zoom: f32,
    (r, area): (egui::Rect, egui::Rect),
    s: &Shape,
    p: &Palette,
    editing: bool,
) {
    // The shape's own colours where it has them, else the theme's.
    let fill = colour(s.style.fill.as_ref());
    let ink = colour(s.style.stroke.as_ref());
    let stroke = Stroke::new(stroke_width(zoom), ink.unwrap_or(p.text_secondary));
    let body = fill.unwrap_or(p.bg_surface);
    match &s.kind {
        ShapeKind::Rect => {
            painter.rect(r, 0.0, body, stroke, StrokeKind::Inside);
        }
        ShapeKind::RoundRect => {
            painter.rect(r, corner(r, zoom), body, stroke, StrokeKind::Inside);
        }
        // Other Mermaid shapes are drawn as rectangles, dashed so they read
        // as "not quite this".
        ShapeKind::Other { .. } => {
            painter.rect_filled(r, 0.0, body);
            dashed_rect(painter, r, stroke);
        }
        // Bare text, unless it's been given colours; an empty one keeps a
        // faint outline so it can be found.
        ShapeKind::Text => {
            let none = egui::Stroke::NONE;
            let outline = if ink.is_some() { stroke } else { none };
            if fill.is_some() || ink.is_some() {
                let fill = fill.unwrap_or(Color32::TRANSPARENT);
                painter.rect(r, 0.0, fill, outline, StrokeKind::Inside);
            } else if s.label.is_empty() && !editing {
                dashed_rect(painter, r, Stroke::new(1.0, p.text_faint));
            }
        }
        kind => {
            outline(painter, r, kind, zoom, body, stroke);
        }
    }
    if !editing {
        let text = s
            .style
            .label_rgba()
            .map(|[r, g, b, a]| Color32::from_rgba_unmultiplied(r, g, b, a))
            .unwrap_or(p.text_primary);
        label(painter, r, area, &s.label, zoom, text);
    }
    badge(painter, r, s, p);
}

/// What BlockLiner can't fully edit about a shape, as the badge on its
/// top-right corner shows it: a lock for a shape declared in a kept
/// statement, or the Mermaid brackets of a shape drawn as a box. With the
/// icon for the status bar.
pub fn limitation(s: &Shape) -> Option<(&'static str, String)> {
    if s.external {
        return Some((icons::LOCK, format!(
            "“{}” is declared in a Mermaid statement BlockLiner keeps as is: it can be moved and resized, not renamed or deleted",
            s.id
        )));
    }
    match &s.kind {
        ShapeKind::Other { open, close } => Some((icons::CIRCLE_INFO, format!(
            "“{}” is a Mermaid {open} {close} shape, drawn here as a box; it keeps its shape when saved",
            s.id
        ))),
        _ => None,
    }
}

fn badge(painter: &Painter, r: egui::Rect, s: &Shape, p: &Palette) {
    let (text, font) = match &s.kind {
        _ if s.external => (icons::LOCK.to_owned(), icons::font(BADGE_SIZE - 1.0)),
        ShapeKind::Other { open, close } => {
            (format!("{open} {close}"), FontId::monospace(BADGE_SIZE))
        }
        _ => return,
    };
    // Only where it fits beside the label.
    if r.width() < 48.0 || r.height() < 24.0 {
        return;
    }
    let galley = painter.layout_no_wrap(text, font, p.text_muted);
    let size = galley.size() + vec2(8.0, 4.0);
    let at = egui::Rect::from_min_size(r.right_top() + vec2(-size.x - 3.0, 3.0), size);
    painter.rect_filled(at, 3.0, p.bg_base.gamma_multiply(0.85));
    painter.galley(at.center() - galley.size() / 2.0, galley, p.text_muted);
}

/// A diamond, hexagon, database or subroutine filling `r` (on screen), its
/// stroke inside `r` as a box's is. `false` for the other kinds, which
/// draw themselves.
pub fn outline(
    painter: &Painter,
    r: egui::Rect,
    kind: &ShapeKind,
    zoom: f32,
    fill: Color32,
    stroke: Stroke,
) -> bool {
    let inner = r.shrink(stroke.width / 2.0);
    let m = Rect::new(inner.left(), inner.top(), inner.width(), inner.height());
    let screen = |points: Vec<blockliner_core::Point>| -> Vec<Pos2> {
        points.into_iter().map(|q| Pos2::new(q.x, q.y)).collect()
    };
    match kind {
        ShapeKind::Diamond | ShapeKind::Hexagon => {
            let points = screen(geometry::polygon(kind, &m).unwrap_or_default());
            painter.add(egui::Shape::convex_polygon(points, fill, stroke));
        }
        ShapeKind::Database => {
            // The cap scales with the zoom, as the model's does.
            let cap = (r.height() / 5.0).min(12.0 * zoom);
            let (body, lid) = geometry::cylinder(&m, cap, 24);
            painter.add(egui::Shape::convex_polygon(screen(body), fill, stroke));
            painter.add(egui::Shape::line(screen(lid), stroke));
        }
        ShapeKind::Subroutine => {
            painter.rect(r, 0.0, fill, stroke, StrokeKind::Inside);
            let k = (SUBROUTINE_INSET * zoom).min(r.width() / 4.0);
            for x in [inner.left() + k, inner.right() - k] {
                painter.line_segment(
                    [Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
                    stroke,
                );
            }
        }
        _ => return false,
    }
    true
}

fn corner(r: egui::Rect, zoom: f32) -> CornerRadius {
    CornerRadius::same((CORNER * zoom).min(r.height() / 2.0).min(255.0) as u8)
}

/// Where a right-angle route's middle-segment handle sits: half-way along
/// that segment, in model coordinates.
pub fn middle_handle(path: &[blockliner_core::Point]) -> Option<blockliner_core::Point> {
    let i = middle_segment(path)?;
    let (a, b) = (path[i], path[i + 1]);
    Some(blockliner_core::Point::new(
        (a.x + b.x) / 2.0,
        (a.y + b.y) / 2.0,
    ))
}

/// A connector through `points`; an arrow's last segment stops at the
/// head's base so the tip stays sharp.
fn connector(painter: &Painter, points: &[Pos2], kind: ConnectorKind, stroke: Stroke, zoom: f32) {
    let [.., a, b] = points else {
        return;
    };
    let mut line = points.to_vec();
    if kind == ConnectorKind::Arrow {
        let len = (*b - *a).length();
        if len > 0.0 {
            let head = (ARROW * zoom).clamp(6.0, 24.0).min(len);
            if let Some(end) = line.last_mut() {
                *end = *b - (*b - *a) / len * head * 0.8;
            }
            painter.add(arrowhead(*a, *b, head, stroke.color));
        }
    }
    painter.add(egui::Shape::line(line, stroke));
}

/// An edge label from the file, on a patch of background at the midpoint.
fn edge_label(painter: &Painter, at: Pos2, text: &str, zoom: f32, p: &Palette) {
    let size = EDGE_LABEL_SIZE * zoom;
    if text.is_empty() || size < 4.0 {
        return;
    }
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        FontId::proportional(size),
        p.text_secondary,
    );
    let r = egui::Rect::from_center_size(at, galley.size()).expand(3.0 * zoom);
    painter.rect_filled(r, 2.0, p.bg_base);
    painter.galley(r.center() - galley.size() / 2.0, galley, p.text_secondary);
}

/// The label, centred line by line in its `area`, wrapped to it and
/// clipped to the shape `r`.
fn label(
    painter: &Painter,
    r: egui::Rect,
    area: egui::Rect,
    text: &str,
    zoom: f32,
    color: Color32,
) {
    let size = LABEL_SIZE * zoom;
    if text.is_empty() || size < 4.0 {
        return;
    }
    let wrap = (area.width() - 2.0 * LABEL_PAD * zoom).max(1.0);
    let mut job = LayoutJob::simple(text.to_owned(), label_font(zoom), color, wrap);
    job.halign = Align::Center;
    let galley = painter.layout_job(job);
    let at = area.center() - galley.rect.center().to_vec2();
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
    let stroke = Stroke::new(1.0, p.accent);
    painter.rect(
        r,
        0.0,
        p.accent.gamma_multiply(0.08),
        stroke,
        StrokeKind::Middle,
    );
}

/// The shape a drag will create.
pub fn preview(painter: &Painter, r: egui::Rect, kind: &ShapeKind, zoom: f32, p: &Palette) {
    let stroke = Stroke::new(1.5, p.accent);
    match kind {
        ShapeKind::RoundRect => {
            painter.rect_stroke(r, corner(r, zoom), stroke, StrokeKind::Middle);
        }
        kind if outline(painter, r, kind, zoom, Color32::TRANSPARENT, stroke) => {}
        _ => dashed_rect(painter, r, stroke),
    }
}

/// The line a drag will create.
pub fn line_preview(
    painter: &Painter,
    points: &[Pos2],
    kind: ConnectorKind,
    zoom: f32,
    p: &Palette,
) {
    connector(
        painter,
        points,
        kind,
        Stroke::new(stroke_width(zoom), p.accent),
        zoom,
    );
}
