//! The snapping grid, drawn behind the diagram: a hairline every cell and a
//! stronger line every [`MAJOR`] cells. When cells get too small to see at
//! low zoom, only the major lines are drawn.

use egui::{Color32, Painter, Rect, Stroke};

use super::view::View;

/// Cells per major line.
pub const MAJOR: i64 = 5;
/// Lines closer than this many screen points are not drawn.
const MIN_SPACING: f32 = 6.0;

pub fn paint(painter: &Painter, rect: Rect, view: View, grid: f32, minor: Color32, major: Color32) {
    let spacing = grid * view.zoom;
    let every = if spacing >= MIN_SPACING {
        1
    } else if spacing * MAJOR as f32 >= MIN_SPACING {
        MAJOR
    } else {
        return;
    };
    let top_left = view.to_model(rect.min, rect.min);
    let bottom_right = view.to_model(rect.min, rect.max);
    let lines = |from: f32, to: f32| {
        let first = (from / grid).floor() as i64;
        let last = (to / grid).ceil() as i64;
        (first..=last).filter(move |i| i % every == 0)
    };
    let stroke = |i: i64| Stroke::new(1.0, if i % MAJOR == 0 { major } else { minor });
    for i in lines(top_left.x, bottom_right.x) {
        let x = view.to_screen(rect.min, egui::pos2(i as f32 * grid, 0.0)).x;
        painter.vline(x, rect.y_range(), stroke(i));
    }
    for i in lines(top_left.y, bottom_right.y) {
        let y = view.to_screen(rect.min, egui::pos2(0.0, i as f32 * grid)).y;
        painter.hline(rect.x_range(), y, stroke(i));
    }
}
