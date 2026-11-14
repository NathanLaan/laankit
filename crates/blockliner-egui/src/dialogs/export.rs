//! The Export dialog: format (PNG or SVG), which objects (all, or only the
//! selected ones), the grid, and a PNG's scale.

use egui::{vec2, Align2, FontId, Sense, Stroke, StrokeKind, Ui};
use laankit_egui::dialog::{self, ButtonKind, Dialog};
use laankit_egui::{settings, theme};

use crate::export::{Format, Options, SCALES};

pub struct ExportState {
    pub options: Options,
    /// Only the selected objects; offered only when there is a selection.
    pub selected_only: bool,
    pub has_selection: bool,
}

pub enum ExportAction {
    Close,
    Export,
}

/// `size`: the output's size with the current options (`None`: nothing to
/// export). `asking`: the save dialog is open, so Export… waits.
pub fn show(
    ctx: &egui::Context,
    state: &mut ExportState,
    size: Option<(u32, u32)>,
    asking: bool,
) -> Option<ExportAction> {
    let resp = Dialog::compact("blockliner-export", "Export Diagram").show(ctx, |ui| {
        let o = &mut state.options;
        dialog::field_label(ui, "Format");
        if let Some(f) = segmented(
            ui,
            &[Format::Png, Format::Svg].map(|f| (f, f.label(), true)),
            o.format,
        ) {
            o.format = f;
        }
        ui.add_space(12.0);
        dialog::field_label(ui, "Objects");
        let choices = [
            (false, "All Objects", true),
            (true, "Selected Objects Only", state.has_selection),
        ];
        if let Some(s) = segmented(ui, &choices, state.selected_only) {
            state.selected_only = s;
        }
        if o.format == Format::Png {
            ui.add_space(12.0);
            dialog::field_label(ui, "Scale");
            let labels: Vec<(u32, String)> = SCALES.iter().map(|&s| (s, format!("{s}×"))).collect();
            let choices: Vec<(u32, &str, bool)> =
                labels.iter().map(|(s, l)| (*s, l.as_str(), true)).collect();
            if let Some(s) = segmented(ui, &choices, o.scale) {
                o.scale = s;
            }
        }
        ui.add_space(12.0);
        settings::toggle_option(ui, "Show grid", &mut o.grid, true);
        ui.add_space(8.0);
        let note = match (size, o.format) {
            (None, _) => "Nothing to export.".to_owned(),
            (Some((w, h)), Format::Png) => format!("{w} × {h} px"),
            (Some((w, h)), Format::Svg) => format!("{w} × {h}"),
        };
        dialog::muted_text(ui, &note);
        let mut out = None;
        dialog::actions(ui, |ui| {
            if dialog::button(
                ui,
                "Export…",
                ButtonKind::Primary,
                size.is_some() && !asking,
            )
            .clicked()
            {
                out = Some(ExportAction::Export);
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                out = Some(ExportAction::Close);
            }
        });
        out
    });
    if resp.close {
        return Some(ExportAction::Close);
    }
    resp.inner
}

/// A row of equal buttons, one chosen (`(value, label, enabled)`); returns
/// a newly chosen value.
fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    choices: &[(T, &str, bool)],
    chosen: T,
) -> Option<T> {
    let p = theme::palette(ui.ctx());
    let height = 30.0;
    let (row, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let w = row.width() / choices.len() as f32;
    let mut picked = None;
    for (i, &(value, label, enabled)) in choices.iter().enumerate() {
        let rect = egui::Rect::from_min_size(row.min + vec2(i as f32 * w, 0.0), vec2(w, height));
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let resp = ui.interact(rect, ui.scope_id().with(("segment", label)), sense);
        let on = value == chosen;
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::Role::RadioButton, enabled, on, label)
        });
        let fill = if on {
            p.bg_selected
        } else if enabled && resp.hovered() {
            p.bg_button_hover
        } else {
            p.bg_button
        };
        let first = i == 0;
        let last = i + 1 == choices.len();
        let r = theme::RADIUS.nw;
        let corners = egui::CornerRadius {
            nw: if first { r } else { 0 },
            sw: if first { r } else { 0 },
            ne: if last { r } else { 0 },
            se: if last { r } else { 0 },
        };
        let stroke = if on {
            Stroke::new(1.0, p.accent)
        } else {
            Stroke::new(1.0, p.border)
        };
        ui.painter()
            .rect(rect, corners, fill, stroke, StrokeKind::Inside);
        let ink = match (on, enabled) {
            (true, _) => p.accent,
            (false, true) => p.text_primary,
            (false, false) => p.text_faint,
        };
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            label,
            FontId::proportional(13.0),
            ink,
        );
        if resp.clicked() && !on {
            picked = Some(value);
        }
    }
    picked
}
