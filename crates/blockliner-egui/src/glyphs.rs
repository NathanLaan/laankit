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
    icon_row_enabled(ui, icon, name, letter, active, width, true)
}

/// [`icon_row`], greyed and not clickable when `enabled` is off.
pub fn icon_row_enabled(
    ui: &mut Ui,
    icon: impl Into<Glyph>,
    name: &str,
    letter: Option<&str>,
    active: bool,
    width: f32,
    enabled: bool,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 30.0), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::MenuItemRadio, enabled, active, name)
    });
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 4.0, p.bg_button);
    }
    let ink = if !enabled {
        p.text_faint
    } else if active {
        p.accent
    } else {
        p.text_primary
    };
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
    /// A move in the stacking order: two overlapping squares, the moving
    /// one filled.
    Order(blockliner_core::Order),
}

impl From<blockliner_core::Order> for Glyph {
    fn from(o: blockliner_core::Order) -> Self {
        Glyph::Order(o)
    }
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
        Glyph::Order(order) => paint_order_icon(painter, *order, c, ink),
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

/// A stacking-order move's icon, centred on `c`: two overlapping squares.
/// The filled one is the shape that moves: in front of the outlined one for
/// Up and Front, behind it for Down and Back. Front and Back add a bar at
/// the end they go to.
pub fn paint_order_icon(
    painter: &egui::Painter,
    order: blockliner_core::Order,
    c: Pos2,
    ink: Color32,
) {
    use blockliner_core::Order;
    let stroke = Stroke::new(1.4, ink);
    let far = matches!(order, Order::Front | Order::Back);
    // The bar takes room at its end, so the squares shift the other way.
    let c = match order {
        Order::Front => c + vec2(0.0, 1.5),
        Order::Back => c + vec2(0.0, -1.5),
        _ => c,
    };
    let behind = Rect::from_min_max(c + vec2(-7.0, -7.0), c + vec2(2.0, 2.0));
    let front = Rect::from_min_max(c + vec2(-2.0, -2.0), c + vec2(7.0, 7.0));
    match order {
        Order::Front | Order::Up => {
            painter.rect_stroke(behind, 1.0, stroke, egui::StrokeKind::Inside);
            painter.rect_filled(front, 1.0, ink);
        }
        Order::Down | Order::Back => {
            // The part of the filled square the front one doesn't cover.
            let top = Rect::from_min_max(behind.min, pos2(behind.right(), front.top() - 1.5));
            let left = Rect::from_min_max(
                pos2(behind.left(), top.bottom()),
                pos2(front.left() - 1.5, behind.bottom()),
            );
            painter.rect_filled(top, 0.0, ink);
            painter.rect_filled(left, 0.0, ink);
            painter.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Inside);
        }
    }
    if far {
        let y = if order == Order::Front {
            c.y - 10.0
        } else {
            c.y + 10.0
        };
        painter.line_segment([pos2(c.x - 7.0, y), pos2(c.x + 7.0, y)], stroke);
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
