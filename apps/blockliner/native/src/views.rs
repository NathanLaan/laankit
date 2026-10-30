//! The chrome around the canvas: the toolbar (tools at the top, Settings and
//! About at the bottom) and the status bar.

use egui::{pos2, vec2, Align2, Color32, FontId, Pos2, Rect, Shape, Stroke, Ui};
use marina_egui::toolbar::{self, ToolbarButton};
use marina_egui::{icons, theme};

use crate::canvas::Tool;
use crate::commands::{tool_key, Act};

/// `.status-bar { height: 24px }`, as in the other apps.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

pub struct ToolbarProps {
    pub tool: Tool,
    pub settings_open: bool,
    pub about_open: bool,
}

pub fn toolbar(ui: &mut Ui, props: &ToolbarProps) -> Vec<Act> {
    let mut out = Vec::new();
    let mut bottom = Vec::new();
    toolbar::show(
        ui,
        |ui| {
            for tool in Tool::ALL {
                if tool_button(ui, tool, props.tool == tool) {
                    out.push(Act::Tool(tool));
                }
                if tool == Tool::Select {
                    toolbar::divider(ui);
                }
            }
        },
        |ui| {
            let mut b = |ui: &mut Ui, glyph, title, label, active: bool, act: Act| {
                let button = ToolbarButton::new(glyph, title).label(label).active(active);
                if toolbar::button(ui, button).clicked() {
                    bottom.push(act);
                }
            };
            b(
                ui,
                icons::GEAR,
                "Settings (Ctrl+,)",
                "Settings",
                props.settings_open,
                Act::Settings,
            );
            b(
                ui,
                icons::CIRCLE_INFO,
                "About (Ctrl+Shift+I)",
                "About",
                props.about_open,
                Act::About,
            );
        },
    );
    out.extend(bottom);
    out
}

/// A tool button. Select and Text use Font Awesome glyphs; the shape and
/// line tools draw what they make, since no icon says "rounded rectangle".
fn tool_button(ui: &mut Ui, tool: Tool, active: bool) -> bool {
    let title = format!("{} ({})", tool.name(), tool_key(tool).1);
    let glyph = match tool {
        Tool::Select => icons::ARROW_POINTER,
        Tool::Text => icons::FONT,
        _ => "",
    };
    let resp = toolbar::button(
        ui,
        ToolbarButton::new(glyph, &title)
            .label(tool.name())
            .active(active),
    );
    if glyph.is_empty() {
        // The toolbar button's own ink: accent when active, brighter on hover.
        let p = theme::palette(ui.ctx());
        let ink = if active {
            p.accent
        } else if resp.hovered() {
            p.text_primary
        } else {
            p.text_secondary
        };
        paint_tool_icon(ui, tool, resp.rect.center(), ink);
    }
    resp.clicked()
}

fn paint_tool_icon(ui: &Ui, tool: Tool, c: Pos2, ink: Color32) {
    let painter = ui.painter();
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
        Tool::Select | Tool::Text => {}
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

pub struct StatusProps<'a> {
    pub tool: Tool,
    /// The pointer in model coordinates, when over the canvas.
    pub pointer: Option<Pos2>,
    pub selected: usize,
    pub grid: f32,
    pub zoom: f32,
    /// The UI scale id (`100`, `125`, …).
    pub scale: &'a str,
}

/// Tool on the left; pointer position (in grid cells), grid, canvas zoom and
/// UI scale on the right.
pub fn status_bar(ui: &mut Ui, props: &StatusProps<'_>) {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.bg_surface);
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    painter.text(
        pos2(rect.left() + 10.0, y),
        Align2::LEFT_CENTER,
        props.tool.name(),
        font.clone(),
        p.text_secondary,
    );

    let mut parts = Vec::new();
    if props.selected > 0 {
        parts.push(format!("{} selected", props.selected));
    }
    if let Some(at) = props.pointer {
        let cell = |v: f32| (v / props.grid).floor() as i64;
        parts.push(format!("{}, {}", cell(at.x), cell(at.y)));
    }
    parts.push(format!("Grid {} px", props.grid));
    parts.push(format!("Zoom {:.0}%", props.zoom * 100.0));
    parts.push(format!("UI {}%", props.scale));
    let mut right = rect.right() - 10.0;
    for text in parts.iter().rev() {
        let r = painter.text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            text,
            font.clone(),
            p.text_muted,
        );
        right = r.left() - 16.0;
    }
}
