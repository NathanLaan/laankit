//! Dimension lines: each shape's width above it and its height to its
//! right, measured in the grid's unit (see [`crate::units`]). Off by default;
//! the Toggle Dimension Lines command (Ctrl+Shift+D) or Settings turns them on.
//! They are drawn on the canvas only, never exported.

use blockliner_core::Diagram;
use egui::{pos2, Align2, Color32, FontId, Painter, Pos2, Stroke};

use super::paint::screen_rect;
use super::view::View;
use crate::units::Unit;

/// The gap between a shape's edge and its dimension line, in screen points.
const OFFSET: f32 = 10.0;
/// Half the length of the tick across each end.
const TICK: f32 = 4.0;
/// Shapes smaller than this on screen get no dimension lines.
const MIN_SIDE: f32 = 24.0;

pub fn paint(painter: &Painter, view: View, origin: Pos2, d: &Diagram, unit: Unit, ink: Color32) {
    let stroke = Stroke::new(1.0, ink);
    let font = FontId::proportional(11.0);
    for s in &d.shapes {
        let r = screen_rect(view, origin, &s.rect);
        if r.width() < MIN_SIDE || r.height() < MIN_SIDE {
            continue;
        }
        if !painter.clip_rect().intersects(r.expand(OFFSET + 20.0)) {
            continue;
        }
        // Width, above the top edge.
        let y = r.top() - OFFSET;
        painter.hline(r.x_range(), y, stroke);
        for x in [r.left(), r.right()] {
            painter.vline(x, (y - TICK)..=(y + TICK), stroke);
        }
        painter.text(
            pos2(r.center().x, y - 2.0),
            Align2::CENTER_BOTTOM,
            unit.measure(s.rect.w, d.grid),
            font.clone(),
            ink,
        );
        // Height, right of the right edge.
        let x = r.right() + OFFSET;
        painter.vline(x, r.y_range(), stroke);
        for y in [r.top(), r.bottom()] {
            painter.hline((x - TICK)..=(x + TICK), y, stroke);
        }
        painter.text(
            pos2(x + 4.0, r.center().y),
            Align2::LEFT_CENTER,
            unit.measure(s.rect.h, d.grid),
            font.clone(),
            ink,
        );
    }
}
