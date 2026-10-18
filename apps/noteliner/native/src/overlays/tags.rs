//! The Tag Editor (`TagEditorModal.svelte`): the project's tags on the
//! left, each previewed as a chip in its colour; the SVG named-colour grid
//! on the right paints the focused tag (`index.tagMeta[tag].color`).
//! Every click is saved at once, as in the Svelte build; OK just closes.

use egui::{vec2, Align2, FontId, RichText, Sense, Stroke, Ui};
use marina_egui::dialog::{self, ButtonKind, Dialog};
use marina_egui::theme;
use marina_egui::{fonts, icons};
use serde_json::Value;

use crate::model;
use crate::svg_colors::{self, SVG_COLORS};

const SWATCH: f32 = 22.0;

#[derive(Default)]
pub struct TagEditorState {
    /// The focused tag (defaults to the first one).
    pub selected: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TagEditorAction {
    Close,
    /// Paint `tag` with this colour name, or clear it (`None`).
    SetColor {
        tag: String,
        color: Option<String>,
    },
}

pub fn show(
    ctx: &egui::Context,
    state: &mut TagEditorState,
    index: &Value,
    top_inset: f32,
) -> Option<TagEditorAction> {
    let tags = model::all_tags(index);
    if !state.selected.as_ref().is_some_and(|t| tags.contains(t)) {
        state.selected = tags.first().cloned();
    }
    let resp = Dialog::full("tag-editor", "Tag Editor")
        .size(dialog::Size::Full { top_inset })
        .show(ctx, |ui| {
            let mut out = None;
            let p = theme::palette(ui.ctx());
            if tags.is_empty() {
                ui.label(
                    RichText::new("No tags yet. Add a tag to a file to start customizing colors.")
                        .color(p.text_muted),
                );
            } else {
                let height = (ui.available_height() - 110.0).max(120.0);
                // A fixed-height row: the vertical separator spans whatever
                // height its row offers, and in an unbounded row it pushed the
                // actions (OK) below the window.
                let row = vec2(ui.available_width(), height);
                ui.allocate_ui_with_layout(
                    row,
                    egui::Layout::left_to_right(egui::Align::Min),
                    |ui| {
                        ui.allocate_ui(vec2(220.0, height), |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("tag-editor-list")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.vertical(|ui| {
                                        for tag in &tags {
                                            let active =
                                                state.selected.as_deref() == Some(tag.as_str());
                                            if tag_row(ui, tag, active, index).clicked() {
                                                state.selected = Some(tag.clone());
                                            }
                                        }
                                    });
                                });
                        });
                        ui.separator();
                        ui.vertical(|ui| {
                            out = color_panel(ui, state.selected.as_deref(), index, height);
                        });
                    },
                );
            }
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "OK", ButtonKind::Primary, true).clicked() {
                    out = Some(TagEditorAction::Close);
                }
            });
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                out = Some(TagEditorAction::Close);
            }
            out
        });
    if resp.close {
        return Some(TagEditorAction::Close);
    }
    resp.inner
}

fn tag_row(ui: &mut Ui, tag: &str, active: bool, index: &Value) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, active, tag));
    if active {
        ui.painter().rect_filled(rect, theme::RADIUS, p.bg_selected);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, p.bg_item_hover);
    }
    chip(ui, rect.left() + 8.0, rect.center().y, tag, index);
    resp
}

/// Paints `tag` as a chip starting at `x`, centred on `y`.
fn chip(ui: &Ui, x: f32, y: f32, tag: &str, index: &Value) {
    let p = theme::palette(ui.ctx());
    // The tag's swatch, or the default chip.
    let (fill, ink) = svg_colors::lookup(model::tag_color(index, tag))
        .map_or((p.tag_bg, p.text_primary), |s| (s.color(), s.ink()));
    let galley = ui
        .painter()
        .layout_no_wrap(tag.to_string(), FontId::proportional(12.0), ink);
    let rect =
        egui::Rect::from_min_size(egui::pos2(x, y - 11.0), vec2(galley.size().x + 16.0, 22.0));
    ui.painter().rect_filled(rect, theme::RADIUS, fill);
    ui.painter()
        .galley(egui::pos2(x + 8.0, y - galley.size().y / 2.0), galley, ink);
}

/// The right column: "Editing <chip>", Clear color, and the swatch grid.
fn color_panel(
    ui: &mut Ui,
    selected: Option<&str>,
    index: &Value,
    height: f32,
) -> Option<TagEditorAction> {
    let p = theme::palette(ui.ctx());
    let mut out = None;
    let current = selected.and_then(|t| svg_colors::lookup(model::tag_color(index, t)));
    ui.horizontal(|ui| {
        ui.label(fonts::weight(
            RichText::new("Editing").size(11.0).color(p.text_muted),
            fonts::SEMIBOLD,
        ));
        if let Some(tag) = selected {
            let (rect, _) = ui.allocate_exact_size(vec2(160.0, 24.0), Sense::hover());
            chip(ui, rect.left(), rect.center().y, tag, index);
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let enabled = selected.is_some() && current.is_some();
            if dialog::button(ui, "Clear color", ButtonKind::Secondary, enabled)
                .on_hover_text("Remove the color from this tag")
                .clicked()
            {
                if let Some(tag) = selected {
                    out = Some(TagEditorAction::SetColor {
                        tag: tag.to_string(),
                        color: None,
                    });
                }
            }
        });
    });
    ui.add_space(8.0);
    egui::ScrollArea::vertical()
        .id_salt("tag-editor-swatches")
        .max_height(height - 40.0)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 4.0);
                for s in &SVG_COLORS {
                    let active = current.is_some_and(|c| c.name == s.name);
                    let (rect, resp) = ui.allocate_exact_size(vec2(SWATCH, SWATCH), Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::Role::RadioButton,
                            selected.is_some(),
                            active,
                            s.name,
                        )
                    });
                    let resp = resp.on_hover_text(format!("{} (#{:06X})", s.name, s.rgb));
                    ui.painter().rect(
                        rect,
                        3.0,
                        s.color(),
                        if active || resp.hovered() {
                            Stroke::new(2.0, p.text_primary)
                        } else {
                            Stroke::new(1.0, p.border)
                        },
                        egui::StrokeKind::Inside,
                    );
                    if active {
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            icons::CHECK,
                            icons::font(11.0),
                            s.ink(),
                        );
                    }
                    if resp.clicked() {
                        if let Some(tag) = selected {
                            out = Some(TagEditorAction::SetColor {
                                tag: tag.to_string(),
                                color: Some(s.name.to_string()),
                            });
                        }
                    }
                }
            });
        });
    out
}
