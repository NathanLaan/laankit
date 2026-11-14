//! The small drawings beside a name in the editor's lists and buttons: a
//! tool's icon, a stroke style's sample, a weight, a text style.

use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui};
use laankit_egui::{icons, theme};

use crate::canvas::{paint, Tool};

/// A row of an icon list (More Shapes, Change Shape, Change Line): `icon`'s
/// toolbar icon, `name`, and a key letter on the right if it has one;
/// ticked (in the accent) when `active`.
pub fn icon_row(
    ui: &mut Ui,
    icon: impl Into<Glyph>,
    name: &str,
    letter: Option<&str>,
    active: bool,
    width: f32,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItemRadio, true, active, name));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_button);
    }
    let ink = if active { p.accent } else { p.text_primary };
    let at = pos2(rect.left() + 18.0, rect.center().y);
    paint_glyph(ui.painter(), &icon.into(), at, ink);
    let font = FontId::proportional(theme::BODY_SIZE);
    ui.painter().text(
        pos2(rect.left() + 38.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        font.clone(),
        ink,
    );
    if let Some(letter) = letter {
        ui.painter().text(
            pos2(rect.right() - 10.0, rect.center().y),
            Align2::RIGHT_CENTER,
            letter,
            font,
            p.text_muted,
        );
    }
    resp
}

/// What a list row or select shows beside its name: a tool's icon, or a
/// stroke style's sample.
#[derive(Clone, Debug, PartialEq)]
pub enum Glyph {
    Tool(Tool),
    /// Solid (`None`), Dashed, Dotted or Custom.
    Dash(Option<blockliner_core::style::Dash>),
    /// A stroke weight (`None`: the default), as a line that thick.
    Weight(Option<blockliner_core::style::Weight>),
    /// "Aa" in Bold and/or Italic.
    Text {
        bold: bool,
        italic: bool,
    },
}

impl From<Tool> for Glyph {
    fn from(t: Tool) -> Self {
        Glyph::Tool(t)
    }
}

/// A glyph, centred on `c`.
pub fn paint_glyph(painter: &egui::Painter, glyph: &Glyph, c: Pos2, ink: Color32) {
    use blockliner_core::style::{Dash, Weight};
    match glyph {
        Glyph::Tool(t) => paint_tool_icon(painter, *t, c, ink),
        Glyph::Dash(dash) => {
            let line = [c + vec2(-10.0, 0.0), c + vec2(10.0, 0.0)];
            let stroke = Stroke::new(1.6, ink);
            match dash {
                None => {
                    painter.line_segment(line, stroke);
                }
                Some(Dash::Dotted) => {
                    painter.extend(egui::Shape::dotted_line(&line, ink, 4.0, 1.1));
                }
                Some(_) => {
                    painter.extend(egui::Shape::dashed_line(&line, stroke, 5.0, 2.5));
                }
            }
        }
        Glyph::Weight(weight) => {
            let width = weight
                .as_ref()
                .and_then(|w| w.px())
                .unwrap_or(Weight::DEFAULT_PX);
            let line = [c + vec2(-10.0, 0.0), c + vec2(10.0, 0.0)];
            painter.line_segment(line, Stroke::new(width.min(6.0), ink));
        }
        Glyph::Text { bold, italic } => {
            let face = laankit_egui::fonts::Style {
                weight: bold.then_some(laankit_egui::fonts::BOLD),
                italic: *italic,
                mono: false,
            };
            painter.text(
                c,
                Align2::CENTER_CENTER,
                "Aa",
                face.font(theme::BODY_SIZE),
                ink,
            );
        }
    }
}

/// A tool's toolbar icon, centred on `c`.
pub fn paint_tool_icon(painter: &egui::Painter, tool: Tool, c: Pos2, ink: Color32) {
    let stroke = Stroke::new(1.6, ink);
    let shape = Rect::from_center_size(c, vec2(18.0, 12.0));
    match tool {
        Tool::Rect => {
            painter.rect_stroke(shape, 0.0, stroke, egui::StrokeKind::Middle);
        }
        Tool::RoundRect => {
            painter.rect_stroke(shape, 4.0, stroke, egui::StrokeKind::Middle);
        }
        Tool::Line => {
            painter.line_segment([c + vec2(-8.0, 7.0), c + vec2(8.0, -7.0)], stroke);
        }
        Tool::Arrow => {
            let (from, to) = (c + vec2(-8.0, 7.0), c + vec2(8.0, -7.0));
            painter.line_segment([from, to - (to - from).normalized() * 3.0], stroke);
            painter.add(arrowhead(from, to, 8.0, ink));
        }
        // Across, up, across: a right-angle route.
        Tool::ElbowLine => {
            let points = [
                vec2(-9.0, 6.0),
                vec2(0.0, 6.0),
                vec2(0.0, -6.0),
                vec2(9.0, -6.0),
            ];
            painter.add(Shape::line(points.map(|v| c + v).to_vec(), stroke));
        }
        Tool::ElbowArrow => {
            let points = [
                vec2(-9.0, 6.0),
                vec2(-1.0, 6.0),
                vec2(-1.0, -6.0),
                vec2(6.0, -6.0),
            ];
            painter.add(Shape::line(points.map(|v| c + v).to_vec(), stroke));
            painter.add(arrowhead(
                c + vec2(-1.0, -6.0),
                c + vec2(10.0, -6.0),
                7.0,
                ink,
            ));
        }
        // The canvas's own outlines, small.
        Tool::Note
        | Tool::Diamond
        | Tool::Hexagon
        | Tool::Database
        | Tool::Subroutine
        | Tool::Stadium
        | Tool::Circle
        | Tool::DoubleCircle
        | Tool::Parallelogram
        | Tool::ReverseParallelogram
        | Tool::Trapezoid
        | Tool::InvertedTrapezoid
        | Tool::Asymmetric => {
            let size = match tool {
                Tool::Diamond => vec2(20.0, 14.0),
                Tool::Database => vec2(14.0, 16.0),
                Tool::Circle | Tool::DoubleCircle => vec2(15.0, 15.0),
                _ => vec2(18.0, 12.0),
            };
            if let Some(kind) = tool.shape_kind() {
                let r = Rect::from_center_size(c, size);
                paint::outline(painter, r, &kind, 0.4, Color32::TRANSPARENT, stroke);
            }
        }
        // The toolbar draws these as glyphs; lists draw them here.
        Tool::Select | Tool::Text => {
            let glyph = if tool == Tool::Text {
                icons::FONT
            } else {
                icons::ARROW_POINTER
            };
            painter.text(c, Align2::CENTER_CENTER, glyph, icons::font(14.0), ink);
        }
    }
}

/// A filled arrowhead with its tip at `to`, pointing away from `from`.
pub fn arrowhead(from: Pos2, to: Pos2, size: f32, fill: Color32) -> Shape {
    let dir = (to - from).normalized();
    let normal = vec2(-dir.y, dir.x);
    let base = to - dir * size;
    Shape::convex_polygon(
        vec![to, base + normal * size * 0.45, base - normal * size * 0.45],
        fill,
        Stroke::NONE,
    )
}
