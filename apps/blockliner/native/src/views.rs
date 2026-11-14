//! The chrome around the canvas: the toolbar (tools at the top; Open
//! Folder, Export, Settings and About at the bottom), the sidebar's Project
//! and Files lists, the tab strip, the empty states and the status bar.

use egui::text::{LayoutJob, TextWrapping};
use egui::{
    pos2, vec2, Align2, Color32, FontId, Pos2, Rect, RichText, ScrollArea, Sense, Stroke,
    StrokeKind, Ui,
};
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::panes;
use laankit_egui::theme::Palette;
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::{fonts, icons, theme};

use blockliner_egui::tools::{more_shapes_button, tool_button};

use crate::canvas::Tool;
use crate::commands::Act;
use blockliner_egui::EditAct;

/// `.status-bar { height: 24px }`, as in the other apps.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// The sidebar's (min, default, max) width.
pub const SIDEBAR_WIDTH: (f32, f32, f32) = (160.0, 220.0, 420.0);
const ROW_HEIGHT: f32 = 30.0;

pub struct ToolbarProps {
    pub tool: Tool,
    /// The shape the More Shapes button shows and picks.
    pub more_shape: Tool,
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
                if tool.is_more() {
                    // One button for the four.
                    if tool == Tool::MORE[0] {
                        if let Some(t) = more_shapes_button(ui, props.more_shape, props.tool) {
                            out.push(Act::Edit(EditAct::Tool(t)));
                        }
                    }
                    continue;
                }
                if tool_button(ui, tool, props.tool == tool) {
                    out.push(Act::Edit(EditAct::Tool(tool)));
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

/// What the sidebar asks for.
#[derive(Clone, Debug, PartialEq)]
pub enum SidebarAction {
    Open(String),
    Rename(String),
    Delete(String),
    /// Show the diagram's folder in the OS file manager.
    OpenContainingFolder(String),
}

/// The Project pane's body: a row per diagram in the folder (right-click to
/// rename, delete or open its folder). The pane stack draws the header,
/// with the folder's name and the New button.
pub fn project_list(ui: &mut Ui, names: &[String], open: Option<&str>) -> Vec<SidebarAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
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
                let resp = row(ui, &p, &Row::plain(name, active));
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
                    ui.separator();
                    if ui.button("Open Containing Folder…").clicked() {
                        out.push(SidebarAction::OpenContainingFolder(name.clone()));
                        ui.close();
                    }
                });
            }
            ui.add_space(4.0);
        });
    out
}

/// A 24 px icon button for a pane header (inside the header's
/// right-to-left extras). `true` when clicked.
pub fn pane_button(ui: &mut Ui, id: &str, glyph: &str, label: &str) -> bool {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, 24.0), Sense::hover());
    panes::header_button(ui, &p, ui.scope_id().with(id), rect, glyph, label).clicked()
}

/// A row of the Files pane.
pub struct FileRow {
    pub name: String,
    /// The folder, muted after the name: for a file outside the project or
    /// one whose name another open file shares.
    pub hint: Option<String>,
    /// The full path, for the tooltip.
    pub path: String,
    /// In the active tab.
    pub active: bool,
    /// Has a tab.
    pub tabbed: bool,
    /// Deleted or moved outside BlockLiner.
    pub missing: bool,
}

/// What the Files pane asks for, by row index.
#[derive(Clone, Debug, PartialEq)]
pub enum FilesAction {
    /// Open its tab (if closed) and make it active.
    Open(usize),
    /// Remove it from the list and close its tab.
    Close(usize),
    OpenContainingFolder(usize),
}

/// The Files pane's body: every open file, in the order opened. The pane
/// stack draws the header, with the Open File button.
pub fn files_list(ui: &mut Ui, rows: &[FileRow]) -> Vec<FilesAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    ScrollArea::vertical()
        .id_salt("blockliner-files")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add_space(4.0);
            if rows.is_empty() {
                ui.add_space(12.0);
                ui.vertical_centered(|ui| dialog::muted_text(ui, "No files open."));
            }
            for (i, f) in rows.iter().enumerate() {
                let look = Row {
                    name: &f.name,
                    hint: f.hint.as_deref(),
                    active: f.active,
                    marker: f.tabbed && !f.active,
                    missing: f.missing,
                };
                let resp = row(ui, &p, &look);
                let tip = if f.missing {
                    format!("{} (missing)", f.path)
                } else {
                    f.path.clone()
                };
                let resp = resp.on_hover_text(tip);
                if resp.clicked() && !f.active {
                    out.push(FilesAction::Open(i));
                }
                resp.context_menu(|ui| {
                    if ui.button("Close").clicked() {
                        out.push(FilesAction::Close(i));
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("Open Containing Folder…").clicked() {
                        out.push(FilesAction::OpenContainingFolder(i));
                        ui.close();
                    }
                });
            }
            ui.add_space(4.0);
        });
    out
}

/// How a sidebar row looks.
struct Row<'a> {
    name: &'a str,
    hint: Option<&'a str>,
    active: bool,
    /// A dot on the right: the file has a tab.
    marker: bool,
    /// Struck through and muted.
    missing: bool,
}

impl<'a> Row<'a> {
    fn plain(name: &'a str, active: bool) -> Self {
        Self {
            name,
            hint: None,
            active,
            marker: false,
            missing: false,
        }
    }
}

/// A diagram row: icon, name and an optional folder hint, the selected wash
/// with an accent outline when it's open.
fn row(ui: &mut Ui, p: &Palette, r: &Row<'_>) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, r.active, r.name));
    if r.active {
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
    let ink = if r.missing {
        p.text_muted
    } else if r.active {
        p.text_primary
    } else {
        p.text_secondary
    };
    let icon = ui.painter().text(
        pos2(rect.left() + 16.0, rect.center().y),
        Align2::LEFT_CENTER,
        icons::OBJECT_GROUP,
        icons::font(12.0),
        if r.active { p.accent } else { p.text_muted },
    );
    let right = rect.right() - if r.marker { 24.0 } else { 10.0 };
    let text = one_line(
        ui,
        r.name,
        FontId::proportional(13.0),
        ink,
        right - icon.right() - 8.0,
    );
    let at = pos2(icon.right() + 8.0, rect.center().y - text.size().y / 2.0);
    let name = Rect::from_min_size(at, text.size());
    ui.painter().galley(at, text, ink);
    if r.missing {
        ui.painter()
            .hline(name.x_range(), name.center().y, Stroke::new(1.0, ink));
    }
    if let Some(hint) = r.hint {
        let room = right - name.right() - 6.0;
        if room > 24.0 {
            let hint = one_line(ui, hint, FontId::proportional(11.0), p.text_muted, room);
            ui.painter().galley(
                pos2(name.right() + 6.0, rect.center().y - hint.size().y / 2.0),
                hint,
                p.text_muted,
            );
        }
    }
    if r.marker {
        ui.painter().circle_filled(
            pos2(rect.right() - 14.0, rect.center().y),
            3.0,
            p.text_muted,
        );
    }
    resp
}

pub use laankit_egui::tabs::{strip as tab_strip, TabAction, TabItem, HEIGHT as TAB_HEIGHT};

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

/// Where the canvas goes when there's nothing to draw: no folder or file
/// open, or no tab.
pub fn empty_state(ui: &mut Ui, project_open: bool, files_open: bool) -> Option<Act> {
    let p = theme::palette(ui.ctx());
    ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg_base);
    let (glyph, title, detail, button, act) = if project_open {
        (
            icons::OBJECT_GROUP,
            "No diagram open",
            "Open a file from the Files or Project pane, or start a new one.",
            "New Diagram",
            Act::NewDiagram,
        )
    } else if files_open {
        (
            icons::OBJECT_GROUP,
            "No diagram open",
            "Open a file from the Files pane, or open another.",
            "Open File…",
            Act::OpenFile,
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
    pub unit: crate::units::Unit,
    pub zoom: f32,
    /// The UI scale id (`100`, `125`, …).
    pub scale: &'a str,
}

/// Tool on the left; pointer position (in grid cells), grid and its unit, canvas zoom and
/// UI scale on the right. The zoom is a button back to 100 %:
/// `Some(Act::Edit(EditAct::ZoomReset))` when clicked.
pub fn status_bar(ui: &mut Ui, props: &StatusProps<'_>) -> Option<Act> {
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
    parts.push(format!("Grid {} px = 1 {}", props.grid, props.unit.id()));
    let zoom = format!("Zoom {:.0}%", props.zoom * 100.0);
    parts.push(zoom.clone());
    parts.push(format!("UI {}%", props.scale));
    let mut right = rect.right() - 10.0;
    let mut out = None;
    for text in parts.iter().rev() {
        let galley = painter.layout_no_wrap(text.clone(), font.clone(), p.text_muted);
        let at = Rect::from_min_size(
            pos2(right - galley.size().x, y - galley.size().y / 2.0),
            galley.size(),
        );
        let mut ink = p.text_muted;
        if *text == zoom {
            // A button: a highlight on hover, back to 100 % on a click.
            let resp = ui.interact(
                at.expand2(vec2(6.0, 2.0)),
                ui.scope_id().with("zoom-reset"),
                Sense::click(),
            );
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &zoom));
            if resp.hovered() {
                painter.rect_filled(resp.rect, 4.0, p.bg_item_hover);
                ink = p.text_primary;
            }
            let resp = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text("Reset zoom to 100% (Ctrl+0)");
            if resp.clicked() {
                out = Some(Act::Edit(EditAct::ZoomReset));
            }
        }
        painter.galley(at.min, galley, ink);
        right = at.left() - 16.0;
    }
    out
}
