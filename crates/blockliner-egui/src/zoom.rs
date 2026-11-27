//! The canvas zoom control's list: the [`ZOOM_STEPS`], opened from the
//! zoom readout an app draws (in NoteLiner's diagram toolbar, in
//! BlockLiner's status bar).
//!
//! The wheel zooms freely, so the canvas can be between steps: the readout
//! shows the zoom as it is, and no step is ticked then.

use egui::{pos2, vec2, Align2, FontId, Response, Sense, Ui};
use laankit_egui::theme;

use crate::canvas::view::ZOOM_STEPS;

/// The list's width.
const WIDTH: f32 = 92.0;

/// A zoom as the readout and the list show it: "125%".
pub fn label(zoom: f32) -> String {
    format!("{:.0}%", zoom * 100.0)
}

/// Whether `resp`'s list is showing.
pub fn is_open(resp: &Response) -> bool {
    egui::Popup::is_id_open(&resp.ctx, egui::Popup::default_response_id(resp))
}

/// The list of steps, opened by a click on `resp` (the readout) and shown
/// while it's open. `zoom` is the canvas's now. Returns the step picked,
/// unless the canvas is already at it.
pub fn menu(resp: &Response, zoom: f32) -> Option<f32> {
    let mut picked = None;
    egui::Popup::menu(resp)
        .width(WIDTH)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for step in ZOOM_STEPS {
                if row(ui, &label(step), same(step, zoom)).clicked() {
                    picked = Some(step);
                }
            }
        });
    picked.filter(|&step| !same(step, zoom))
}

/// Whether the canvas is at `step`, give or take what the wheel leaves.
fn same(step: f32, zoom: f32) -> bool {
    (step - zoom).abs() < 1e-3
}

/// One step: ticked (in the accent) when the canvas is at it.
fn row(ui: &mut Ui, name: &str, active: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(WIDTH - 8.0, 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::MenuItemRadio, true, active, name));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_button);
    }
    let ink = if active { p.accent } else { p.text_primary };
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.center().y),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(theme::BODY_SIZE),
        ink,
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_and_steps() {
        assert_eq!(label(1.0), "100%");
        assert_eq!(label(1.37), "137%");
        assert!(same(1.25, 1.2501));
        assert!(!same(1.25, 1.3));
    }
}
