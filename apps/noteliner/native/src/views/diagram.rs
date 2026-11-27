//! The bar above a diagram, in the note toolbar's place: the file name,
//! BlockLiner's tools in a row, Undo and Redo, the grid and zoom, and the
//! Properties and Export toggles.

use blockliner_core::Order;
use blockliner_egui::canvas::Tool;
use blockliner_egui::commands::order_keys;
use blockliner_egui::glyphs::paint_order_icon;
use blockliner_egui::properties::{self, PropertiesAction, PropertiesProps, PropertiesState};
use blockliner_egui::tools::tool_buttons;
use blockliner_egui::EditAct;
use egui::{vec2, Align, Frame, Layout, Margin, RichText, ScrollArea, Sense, Stroke, Ui};
use laankit_egui::{fonts, icons, theme};

/// `.editor-toolbar { min-height: 44px }`, as the note toolbar.
const HEIGHT: f32 = 44.0;
/// The small buttons, as the note toolbar's.
const BUTTON: f32 = 28.0;
/// The most room the file name takes.
const NAME_WIDTH: f32 = 160.0;
/// The room the tools, Undo and Redo, the four order buttons, and the grid
/// group take.
const BUTTONS_WIDTH: f32 = 734.0;
/// The canvas zoom control: room for "600%" and the chevron.
const ZOOM_WIDTH: f32 = 60.0;

/// What the toolbar shows.
pub struct ToolbarProps<'a> {
    pub file_name: &'a str,
    pub tool: Tool,
    /// The shape the More Shapes button shows and picks.
    pub more_shape: Tool,
    pub can_undo: bool,
    pub can_redo: bool,
    /// Which stacking-order moves would move a selected shape.
    pub orderable: [bool; 4],
    pub show_grid: bool,
    pub show_dimensions: bool,
    /// The canvas zoom; 1.0 is 100%.
    pub zoom: f32,
    pub show_properties: bool,
    /// The diagram has something to export.
    pub can_export: bool,
}

/// A toolbar button for the app.
#[derive(Clone, Debug, PartialEq)]
pub enum ToolbarAction {
    Edit(EditAct),
    ToggleGrid,
    ToggleDimensions,
    ToggleProperties,
    Export,
}

/// Draws the toolbar across the available width. The tools scroll sideways
/// when the window is too narrow for them.
pub fn toolbar(ui: &mut Ui, props: &ToolbarProps<'_>) -> Vec<ToolbarAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let frame = Frame::NONE
        .fill(p.bg_base)
        .inner_margin(Margin::symmetric(12, 4))
        .stroke(Stroke::NONE);
    let response = frame.show(ui, |ui| {
        ui.set_min_height(HEIGHT - 8.0);
        ui.horizontal_centered(|ui| {
            // The right-hand group keeps its place; the rest scrolls.
            let right = 3.0 * BUTTON + 2.0 * 4.0 + 12.0 + ZOOM_WIDTH + 9.0 + 2.0 * 4.0;
            // The name gives way first when the window is narrow: the tab
            // above shows it too.
            if ui.available_width() >= NAME_WIDTH + 17.0 + BUTTONS_WIDTH + right {
                let label = RichText::new(props.file_name)
                    .size(11.0)
                    .extra_letter_spacing(0.5)
                    .color(p.text_muted);
                let style = fonts::Style {
                    weight: Some(fonts::SEMIBOLD),
                    italic: false,
                    mono: false,
                };
                ui.scope(|ui| {
                    ui.set_max_width(NAME_WIDTH);
                    ui.add(egui::Label::new(fonts::styled(label, style)).truncate());
                });
                divider(ui);
            }
            ScrollArea::horizontal()
                .id_salt("diagram-toolbar")
                .max_width((ui.available_width() - right).max(BUTTON))
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .show(ui, |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        tools(ui, props, &mut out);
                        divider(ui);
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if button(
                            ui,
                            icons::ROTATE_LEFT,
                            "Undo (Ctrl+Z)",
                            false,
                            props.can_undo,
                        ) {
                            out.push(ToolbarAction::Edit(EditAct::Undo));
                        }
                        if button(
                            ui,
                            icons::ROTATE_RIGHT,
                            "Redo (Ctrl+Shift+Z)",
                            false,
                            props.can_redo,
                        ) {
                            out.push(ToolbarAction::Edit(EditAct::Redo));
                        }
                        divider(ui);
                        for order in Order::ALL {
                            if order_button(ui, order, props.orderable[order.index()]) {
                                out.push(ToolbarAction::Edit(EditAct::Order(order)));
                            }
                        }
                        divider(ui);
                        if button(
                            ui,
                            icons::TABLE_CELLS_LARGE,
                            "Toggle Grid",
                            props.show_grid,
                            true,
                        ) {
                            out.push(ToolbarAction::ToggleGrid);
                        }
                        if button(
                            ui,
                            icons::RULER_COMBINED,
                            "Toggle Dimension Lines",
                            props.show_dimensions,
                            true,
                        ) {
                            out.push(ToolbarAction::ToggleDimensions);
                        }
                    });
                });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if button(
                    ui,
                    icons::FILE_EXPORT,
                    "Export Diagram...",
                    false,
                    props.can_export,
                ) {
                    out.push(ToolbarAction::Export);
                }
                if button(
                    ui,
                    icons::SLIDERS,
                    "Toggle Properties (Ctrl+I)",
                    props.show_properties,
                    true,
                ) {
                    out.push(ToolbarAction::ToggleProperties);
                }
                divider(ui);
                // Here, not with the buttons that scroll: always in view.
                if let Some(zoom) = zoom_control(ui, props.zoom) {
                    out.push(ToolbarAction::Edit(EditAct::ZoomTo(zoom)));
                }
            });
        });
    });
    // `border-bottom: 1px solid var(--border)`.
    let r = response.response.rect;
    ui.painter()
        .hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.border));
    out
}

/// BlockLiner's tool buttons, in its toolbar's order.
fn tools(ui: &mut Ui, props: &ToolbarProps<'_>, out: &mut Vec<ToolbarAction>) {
    if let Some(t) = tool_buttons(ui, props.tool, props.more_shape, divider) {
        out.push(ToolbarAction::Edit(EditAct::Tool(t)));
    }
}

fn divider(ui: &mut Ui) {
    let p = theme::palette(ui.ctx());
    let (r, _) = ui.allocate_exact_size(vec2(9.0, 18.0), Sense::hover());
    ui.painter()
        .vline(r.center().x, r.y_range(), Stroke::new(1.0, p.border));
}

/// A 28 × 28 icon button, as the note toolbar's: muted, the text colour on a
/// button fill when hovered, the accent when active, faint when disabled.
fn button(ui: &mut Ui, icon: &str, title: &str, active: bool, enabled: bool) -> bool {
    let (rect, color, response) = button_frame(ui, title, active, enabled);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        icons::font(13.0),
        color,
    );
    response.on_hover_text(title).clicked()
}

/// A button for one move in the stacking order, with BlockLiner's icon for
/// it; faint when it would move nothing.
fn order_button(ui: &mut Ui, order: Order, enabled: bool) -> bool {
    let (rect, color, response) = button_frame(ui, order.name(), false, enabled);
    paint_order_icon(ui.painter(), order, rect.center(), color);
    response
        .on_hover_text(format!("{} ({})", order.name(), order_keys(order)))
        .clicked()
}

/// A toolbar button's place, its ink, and its response, for the caller to
/// draw the icon. `label` is its accessible name.
fn button_frame(
    ui: &mut Ui,
    label: &str,
    active: bool,
    enabled: bool,
) -> (egui::Rect, egui::Color32, egui::Response) {
    let title = label;
    let p = theme::palette(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), sense);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, enabled, active, title));
    let hovered = enabled && response.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect,
            laankit_egui::theme::shape(ui.ctx()).control(),
            p.bg_button,
        );
    }
    let color = if !enabled {
        p.text_faint
    } else if active {
        p.accent
    } else if hovered {
        p.text_primary
    } else {
        p.text_muted
    };
    (rect, color, response)
}

/// The canvas zoom control: the zoom as a percentage, opening the list of
/// zoom steps. Returns the step picked.
fn zoom_control(ui: &mut Ui, zoom: f32) -> Option<f32> {
    let p = theme::palette(ui.ctx());
    let text = blockliner_egui::zoom::label(zoom);
    let (rect, response) = ui.allocate_exact_size(vec2(ZOOM_WIDTH, BUTTON), Sense::click());
    response.widget_info(|| {
        let mut info = egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "Canvas Zoom");
        info.current_text_value = Some(text.clone());
        info
    });
    let open = blockliner_egui::zoom::is_open(&response);
    if open || response.hovered() {
        ui.painter().rect_filled(
            rect,
            laankit_egui::theme::shape(ui.ctx()).control(),
            p.bg_button,
        );
    }
    ui.painter().text(
        egui::pos2(rect.left() + 8.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        &text,
        egui::FontId::proportional(12.0),
        p.text_secondary,
    );
    ui.painter().text(
        egui::pos2(rect.right() - 7.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        icons::CHEVRON_DOWN,
        icons::font(9.0),
        p.text_muted,
    );
    let picked = blockliner_egui::zoom::menu(&response, zoom);
    if !open {
        response.on_hover_text("Canvas Zoom (Ctrl+Shift+=, Ctrl+Shift+-, Ctrl+Shift+0)");
    }
    picked
}

/// The Properties panel, to the right of the canvas: a header with a close
/// button, then BlockLiner's Properties pane. Returns what the pane asked
/// for, and whether the panel was closed.
pub fn properties_panel(
    ui: &mut Ui,
    state: &mut PropertiesState,
    props: &PropertiesProps,
) -> (Option<PropertiesAction>, bool) {
    let p = theme::palette(ui.ctx());
    let mut close = false;
    egui::Panel::top(ui.scope_id().with("properties-header"))
        .frame(
            Frame::NONE
                .fill(p.bg_surface)
                .inner_margin(Margin::symmetric(12, 6))
                .stroke(Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(laankit_egui::panes::heading(ui.ctx(), "PROPERTIES"))
                        .size(11.0)
                        .color(p.text_muted),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    close = button(ui, icons::XMARK, "Close properties", false, true);
                });
            });
        });
    let action = properties::show(ui, state, props);
    (action, close)
}
