//! The chrome around the canvas: the toolbar (tools at the top; Open
//! Folder, Export, Settings and About at the bottom), the sidebar listing the
//! folder's diagrams, the empty states and the status bar.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Sense, Shape, Stroke,
    StrokeKind, Ui,
};
use marina_egui::dialog::{self, ButtonKind};
use marina_egui::panes::{self, HEADER_HEIGHT};
use marina_egui::theme::Palette;
use marina_egui::toolbar::{self, ToolbarButton};
use marina_egui::{fonts, icons, theme};

use crate::canvas::Tool;
use crate::commands::{tool_digit, tool_key, Act};

/// `.status-bar { height: 24px }`, as in the other apps.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// The sidebar's (min, default, max) width.
pub const SIDEBAR_WIDTH: (f32, f32, f32) = (160.0, 220.0, 420.0);
const ROW_HEIGHT: f32 = 30.0;

pub struct ToolbarProps {
    pub tool: Tool,
    /// A folder is open.
    pub can_close_folder: bool,
    /// The Properties pane is showing.
    pub properties_open: bool,
    /// A diagram with something in it is open.
    pub can_export: bool,
    pub export_open: bool,
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
                let disabled = match act {
                    Act::Export => !props.can_export,
                    Act::CloseFolder => !props.can_close_folder,
                    _ => false,
                };
                let button = ToolbarButton::new(glyph, title)
                    .label(label)
                    .active(active)
                    .disabled(disabled);
                if toolbar::button(ui, button).clicked() {
                    bottom.push(act);
                }
            };
            b(
                ui,
                icons::FOLDER_OPEN,
                "Open Folder (Ctrl+O)",
                "Open Folder",
                false,
                Act::OpenFolder,
            );
            b(
                ui,
                icons::FOLDER_MINUS,
                "Close Folder",
                "Close Folder",
                false,
                Act::CloseFolder,
            );
            b(
                ui,
                icons::SLIDERS,
                "Properties (Ctrl+I)",
                "Properties",
                props.properties_open,
                Act::ToggleProperties,
            );
            b(
                ui,
                icons::FILE_EXPORT,
                "Export (Ctrl+Shift+E)",
                "Export",
                props.export_open,
                Act::Export,
            );
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
    let number = tool_digit(tool).1;
    let title = format!("{} — {} or {number}", tool.name(), tool_key(tool).1);
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
    // The number, so the shortcut shows without hovering (as in Excalidraw).
    let p = theme::palette(ui.ctx());
    ui.painter().text(
        resp.rect.right_bottom() + vec2(-4.0, -1.0),
        egui::Align2::RIGHT_BOTTOM,
        number,
        egui::FontId::proportional(9.0),
        p.text_faint,
    );
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

/// What the sidebar asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarAction {
    Open(String),
    New,
    Rename(String),
    Delete(String),
}

/// The open folder's diagrams: a header with the folder's name and a New
/// button, then a row per diagram (right-click to rename or delete).
pub fn sidebar(
    ui: &mut Ui,
    folder: &str,
    names: &[String],
    open: Option<&str>,
) -> Vec<SidebarAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;

    let (header, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(header, 0.0, p.bg_base);
    painter.hline(
        header.x_range(),
        header.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let button = Rect::from_center_size(
        pos2(header.right() - 22.0, header.center().y),
        vec2(24.0, 24.0),
    );
    let title = one_line(
        ui,
        &folder.to_uppercase(),
        fonts::Style {
            weight: Some(fonts::SEMIBOLD),
            ..Default::default()
        }
        .font(11.0),
        p.text_muted,
        button.left() - header.left() - 20.0,
    );
    ui.painter().galley(
        pos2(
            header.left() + 16.0,
            header.center().y - title.size().y / 2.0,
        ),
        title,
        p.text_muted,
    );
    let new = panes::header_button(
        ui,
        &p,
        ui.scope_id().with("new-diagram"),
        button,
        icons::PLUS,
        "New Diagram (Ctrl+N)",
    );
    if new.clicked() {
        out.push(SidebarAction::New);
    }

    ScrollArea::vertical()
        .id_salt("blockliner-diagrams")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            if names.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| dialog::muted_text(ui, "No diagrams yet."));
            }
            for name in names {
                let active = open == Some(name.as_str());
                let resp = row(ui, &p, name, active);
                if resp.clicked() && !active {
                    out.push(SidebarAction::Open(name.clone()));
                }
                resp.context_menu(|ui| {
                    if ui.button("Rename…").clicked() {
                        out.push(SidebarAction::Rename(name.clone()));
                        ui.close();
                    }
                    if ui.button("Delete…").clicked() {
                        out.push(SidebarAction::Delete(name.clone()));
                        ui.close();
                    }
                });
            }
            ui.add_space(4.0);
        });
    out
}

/// A diagram row: icon and name, the selected wash with an accent outline
/// when it's open.
fn row(ui: &mut Ui, p: &Palette, name: &str, active: bool) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, active, name));
    if active {
        ui.painter().rect(
            rect,
            0.0,
            p.bg_selected,
            Stroke::new(1.0, p.accent),
            StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    let ink = if active {
        p.text_primary
    } else {
        p.text_secondary
    };
    let icon = ui.painter().text(
        pos2(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        icons::OBJECT_GROUP,
        icons::font(12.0),
        if active { p.accent } else { p.text_muted },
    );
    let text = one_line(
        ui,
        name,
        FontId::proportional(13.0),
        ink,
        rect.right() - icon.right() - 18.0,
    );
    ui.painter().galley(
        pos2(icon.right() + 8.0, rect.center().y - text.size().y / 2.0),
        text,
        ink,
    );
    resp
}

/// One line of text, cut with an ellipsis at `width`.
fn one_line(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// Where the canvas goes when there's nothing to draw: no folder open, or
/// no diagram in it.
pub fn empty_state(ui: &mut Ui, project_open: bool) -> Option<Act> {
    let p = theme::palette(ui.ctx());
    ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg_base);
    let (glyph, title, detail, button, act) = if project_open {
        (
            icons::OBJECT_GROUP,
            "No diagram open",
            "Pick one in the sidebar, or start a new one.",
            "New Diagram",
            Act::NewDiagram,
        )
    } else {
        (
            icons::FOLDER_OPEN,
            "No folder open",
            "BlockLiner keeps each diagram as a Mermaid file (.mmd) in a folder you choose.",
            "Open Folder…",
            Act::OpenFolder,
        )
    };
    let mut out = None;
    let top = (ui.available_height() / 2.0 - 90.0).max(16.0);
    ui.vertical_centered(|ui| {
        ui.add_space(top);
        ui.label(
            RichText::new(glyph)
                .font(icons::font(40.0))
                .color(p.text_faint),
        );
        ui.add_space(12.0);
        ui.label(
            fonts::weight(title, fonts::SEMIBOLD)
                .size(16.0)
                .color(p.text_primary),
        );
        ui.add_space(4.0);
        dialog::muted_text(ui, detail);
        ui.add_space(14.0);
        if dialog::button(ui, button, ButtonKind::Primary, true).clicked() {
            out = Some(act);
        }
    });
    out
}

/// Whether the open diagram is on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveState {
    /// No diagram open.
    None,
    Saved,
    /// Edits are waiting for the autosave.
    Saving,
}

pub struct StatusProps<'a> {
    pub tool: Tool,
    pub save: SaveState,
    /// The pointer in model coordinates, when over the canvas.
    pub pointer: Option<Pos2>,
    pub selected: usize,
    /// Some connector has an end that isn't glued, so Mermaid won't show it.
    pub free_lines: bool,
    /// Why the shape under the pointer or selected isn't fully editable.
    /// With its icon.
    pub limitation: Option<(&'static str, String)>,
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
    let mut tool = painter.text(
        pos2(rect.left() + 10.0, y),
        Align2::LEFT_CENTER,
        props.tool.name(),
        font.clone(),
        p.text_secondary,
    );
    let save = match props.save {
        SaveState::None => None,
        SaveState::Saved => Some("Saved"),
        SaveState::Saving => Some("Edited"),
    };
    if let Some(save) = save {
        tool = painter.text(
            pos2(tool.right() + 16.0, y),
            Align2::LEFT_CENTER,
            save,
            font.clone(),
            p.text_muted,
        );
    }
    let warning = match &props.limitation {
        Some((glyph, text)) => Some((*glyph, text.as_str())),
        None if props.free_lines => Some((
            icons::TRIANGLE_EXCLAMATION,
            "Unattached lines show only in BlockLiner, not in Mermaid",
        )),
        None => None,
    };
    if let Some((glyph, text)) = warning {
        let icon = painter.text(
            pos2(tool.right() + 16.0, y),
            Align2::LEFT_CENTER,
            glyph,
            icons::font(10.0),
            p.text_muted,
        );
        painter.text(
            pos2(icon.right() + 6.0, y),
            Align2::LEFT_CENTER,
            text,
            font.clone(),
            p.text_muted,
        );
    }

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
