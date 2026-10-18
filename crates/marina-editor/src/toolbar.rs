//! The bar above the editor (`.editor-toolbar` in `Editor.svelte`): the file
//! name, the list buttons (highlighted when the caret's lines are already
//! that kind of list), "Indent as Sub-list", and the History and Preview
//! toggles, which the host handles.

use egui::{vec2, Align, Frame, Layout, Margin, RichText, Sense, Stroke, Ui};
use marina_egui::{fonts, icons, theme};

use crate::list_edits::ListKind;
use crate::state::{Command, EditorState};

/// `.editor-toolbar { min-height: 44px }`.
const HEIGHT: f32 = 44.0;
/// `.editor-btn { width: 28px; height: 28px }`.
const BUTTON: f32 = 28.0;

/// What the toolbar shows that the editor doesn't know.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolbarProps<'a> {
    /// The open note's name; `None` shows "No file selected".
    pub file_name: Option<&'a str>,
    pub show_history: bool,
    pub show_preview: bool,
}

/// A toolbar button for the host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolbarAction {
    /// Ctrl+H.
    ToggleHistory,
    /// Ctrl+P.
    TogglePreview,
}

/// Draws the toolbar across the available width.
pub fn show(ui: &mut Ui, state: &mut EditorState, props: &ToolbarProps<'_>) -> Vec<ToolbarAction> {
    let p = theme::palette(ui.ctx());
    let kind = state.list_kind(ui.ctx());
    let mut out = Vec::new();
    let frame = Frame::NONE
        .fill(p.bg_base)
        .inner_margin(Margin::symmetric(12, 8))
        .stroke(Stroke::NONE);
    let response = frame.show(ui, |ui| {
        ui.set_min_height(HEIGHT - 16.0);
        ui.horizontal_centered(|ui| {
            let (text, italic) = match props.file_name {
                Some(name) => (name, false),
                None => ("No file selected", true),
            };
            let label = RichText::new(text)
                .size(11.0)
                .extra_letter_spacing(0.5)
                .color(p.text_muted);
            let style = fonts::Style {
                weight: Some(fonts::SEMIBOLD),
                italic,
                mono: false,
            };
            ui.add(egui::Label::new(fonts::styled(label, style)).truncate());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if button(
                    ui,
                    icons::EYE,
                    "Toggle Preview (Ctrl+P)",
                    props.show_preview,
                ) {
                    out.push(ToolbarAction::TogglePreview);
                }
                if button(
                    ui,
                    icons::CLOCK_ROTATE_LEFT,
                    "Toggle History (Ctrl+H)",
                    props.show_history,
                ) {
                    out.push(ToolbarAction::ToggleHistory);
                }
                let (r, _) = ui.allocate_exact_size(vec2(5.0, 18.0), Sense::hover());
                ui.painter()
                    .vline(r.center().x, r.y_range(), Stroke::new(1.0, p.border));
                let mut run = |ui: &mut Ui, icon, title, active, command| {
                    if button(ui, icon, title, active) {
                        state.run(command);
                        state.focus();
                    }
                };
                run(
                    ui,
                    icons::INDENT,
                    "Indent as Sub-list",
                    false,
                    Command::IndentSubList,
                );
                run(
                    ui,
                    icons::LIST_UL,
                    "Unordered List (Ctrl+Shift+8)",
                    kind == ListKind::Unordered,
                    Command::UnorderedList,
                );
                run(
                    ui,
                    icons::LIST_OL,
                    "Ordered List (Ctrl+Shift+7)",
                    kind == ListKind::Ordered,
                    Command::OrderedList,
                );
            });
        });
    });
    // `border-bottom: 1px solid var(--border)`.
    let r = response.response.rect;
    ui.painter()
        .hline(r.x_range(), r.bottom() - 0.5, Stroke::new(1.0, p.border));
    out
}

/// A 28 × 28 icon button: muted, the text colour on a button fill when
/// hovered, the accent when active. Clicking it doesn't take keyboard focus,
/// so the editor keeps its selection (`keepEditorFocus`).
fn button(ui: &mut Ui, icon: &str, title: &str, active: bool) -> bool {
    let p = theme::palette(ui.ctx());
    let (rect, response) = ui.allocate_exact_size(vec2(BUTTON, BUTTON), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(rect, theme::RADIUS, p.bg_button);
    }
    let color = if active {
        p.accent
    } else if hovered {
        p.text_primary
    } else {
        p.text_muted
    };
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        icon,
        icons::font(13.0),
        color,
    );
    let label = title.split(" (").next().unwrap_or(title);
    response.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
    response.on_hover_text(title).clicked()
}
