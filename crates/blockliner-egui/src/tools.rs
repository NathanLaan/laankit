//! The tool buttons: Select, the shapes, the lines, Text and Note, with the
//! More Shapes list, and the tools' keys. An app lays them out in a column
//! (BlockLiner's toolbar) or a row (above a diagram in NoteLiner).

use egui::{vec2, Color32, Key, Rect, Sense, Shape, Stroke, Ui};
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::{icons, theme};

use crate::canvas::Tool;
use crate::glyphs::{icon_row, paint_tool_icon};

/// A tool's letter (Excalidraw's; never fires while a text field has the
/// keyboard). The later more shapes have none: they're in the More Shapes
/// list and the palette.
pub fn tool_key(tool: Tool) -> Option<(Key, &'static str)> {
    Some(match tool {
        Tool::Select => (Key::V, "V"),
        Tool::Rect => (Key::R, "R"),
        Tool::RoundRect => (Key::O, "O"),
        // C for a cylinder, S for a subroutine.
        Tool::Diamond => (Key::D, "D"),
        Tool::Hexagon => (Key::H, "H"),
        Tool::Database => (Key::C, "C"),
        Tool::Subroutine => (Key::S, "S"),
        Tool::Line => (Key::L, "L"),
        Tool::Arrow => (Key::A, "A"),
        // An elbow looks like a J; E for (right-angle) arrows, the most used.
        Tool::ElbowLine => (Key::J, "J"),
        Tool::ElbowArrow => (Key::E, "E"),
        Tool::Text => (Key::T, "T"),
        Tool::Note => (Key::N, "N"),
        Tool::Stadium
        | Tool::Circle
        | Tool::DoubleCircle
        | Tool::Parallelogram
        | Tool::ReverseParallelogram
        | Tool::Trapezoid
        | Tool::InvertedTrapezoid
        | Tool::Asymmetric => return None,
    })
}

/// A tool's number: its toolbar button ([`Tool::slot`]), so the more
/// shapes share 4, which picks the last one used, and the tenth button
/// (Note) is 0. Bound to the physical
/// key, so the number row works on layouts that put the digits behind
/// Shift.
pub fn tool_digit(tool: Tool) -> (Key, &'static str) {
    const DIGITS: [(Key, &str); 10] = [
        (Key::Num1, "1"),
        (Key::Num2, "2"),
        (Key::Num3, "3"),
        (Key::Num4, "4"),
        (Key::Num5, "5"),
        (Key::Num6, "6"),
        (Key::Num7, "7"),
        (Key::Num8, "8"),
        (Key::Num9, "9"),
        (Key::Num0, "0"),
    ];
    DIGITS[tool.slot()]
}

/// A tool button. Select and Text use Font Awesome glyphs; the shape and
/// line tools draw what they make, since no icon says "rounded rectangle".
pub fn tool_button(ui: &mut Ui, tool: Tool, active: bool) -> bool {
    let number = tool_digit(tool).1;
    let title = match tool_key(tool) {
        Some((_, letter)) => format!("{} — {letter} or {number}", tool.name()),
        None => format!("{} — {number}", tool.name()),
    };
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
        paint_tool_icon(
            ui.painter(),
            tool,
            resp.rect.center(),
            tool_ink(ui, &resp, active),
        );
    }
    tool_number(ui, &resp, number);
    resp.clicked()
}

/// The toolbar button's own ink: accent when active, brighter on hover.
fn tool_ink(ui: &Ui, resp: &egui::Response, active: bool) -> Color32 {
    let p = theme::palette(ui.ctx());
    if active {
        p.accent
    } else if resp.hovered() {
        p.text_primary
    } else {
        p.text_secondary
    }
}

/// The number, so the shortcut shows without hovering (as in Excalidraw).
fn tool_number(ui: &Ui, resp: &egui::Response, number: &str) {
    let p = theme::palette(ui.ctx());
    ui.painter().text(
        resp.rect.right_bottom() + vec2(-4.0, -1.0),
        egui::Align2::RIGHT_BOTTOM,
        number,
        egui::FontId::proportional(9.0),
        p.text_faint,
    );
}

/// The More Shapes button: shows and picks the last of [`Tool::MORE`] used
/// (`shown`). Its corner triangle, or a right-click, opens them all.
pub fn more_shapes_button(ui: &mut Ui, shown: Tool, tool: Tool) -> Option<Tool> {
    let number = tool_digit(shown).1;
    let keys = match tool_key(shown) {
        Some((_, letter)) => format!("{letter} or {number}"),
        None => number.to_owned(),
    };
    let title = format!(
        "{} — {keys}. More shapes: right-click, or the corner",
        shown.name()
    );
    let label = format!("More Shapes: {}", shown.name());
    let resp = toolbar::button(
        ui,
        ToolbarButton::new("", &title)
            .label(&label)
            .active(tool.is_more()),
    );
    let ink = tool_ink(ui, &resp, tool == shown);
    paint_tool_icon(ui.painter(), shown, resp.rect.center(), ink);
    tool_number(ui, &resp, number);

    // The corner opens the list; it sits over the button, so it gets the
    // click there.
    let corner = Rect::from_min_size(resp.rect.right_top() + vec2(-13.0, 0.0), vec2(13.0, 13.0));
    let open = ui.interact(corner, resp.id.with("more"), Sense::click());
    open.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Show More Shapes"));
    let p = theme::palette(ui.ctx());
    let tip = resp.rect.right_top() + vec2(-4.0, 4.0);
    let triangle = vec![tip, tip + vec2(0.0, 6.0), tip + vec2(-6.0, 0.0)];
    let fill = if open.hovered() {
        p.text_primary
    } else {
        p.text_muted
    };
    ui.painter()
        .add(Shape::convex_polygon(triangle, fill, Stroke::NONE));

    let set = if open.clicked() || resp.secondary_clicked() {
        Some(egui::SetOpenCommand::Toggle)
    } else if resp.clicked() {
        Some(egui::SetOpenCommand::Bool(false))
    } else {
        None
    };
    let mut picked = resp.clicked().then_some(shown);
    egui::Popup::from_response(&resp)
        .kind(egui::PopupKind::Menu)
        .align(egui::RectAlign::RIGHT_START)
        .gap(4.0)
        .open_memory(set)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for t in Tool::MORE {
                if more_shapes_row(ui, t, t == tool) {
                    picked = Some(t);
                }
            }
        });
    picked
}

/// A row of the More Shapes list: the shape's icon, name and letter (if it
/// has one).
fn more_shapes_row(ui: &mut Ui, tool: Tool, active: bool) -> bool {
    let letter = tool_key(tool).map(|(_, letter)| letter);
    icon_row(ui, tool, tool.name(), letter, active, 190.0).clicked()
}
