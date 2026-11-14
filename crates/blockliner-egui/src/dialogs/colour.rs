//! The Custom… colour dialog: one hex field (`#rgb` or `#rrggbb`) with a
//! preview, for the selected shapes' fill or stroke.

use blockliner_core::style::Colour;
use egui::{vec2, Sense, Stroke, StrokeKind};
use laankit_egui::dialog::{self, ButtonKind, Dialog};
use laankit_egui::theme;

use crate::canvas::paint;

pub struct ColourState {
    /// The stroke, else the fill.
    pub stroke: bool,
    pub text: String,
    /// Why the last try was refused.
    pub error: Option<String>,
    focused: bool,
}

impl ColourState {
    /// `current`: the first selected shape's colour, to start from.
    pub fn new(stroke: bool, current: Option<&Colour>) -> Self {
        let text = current
            .filter(|c| Colour::custom(&c.text).is_some())
            .map(|c| c.text.clone())
            .unwrap_or_default();
        Self {
            stroke,
            text,
            error: None,
            focused: false,
        }
    }
}

pub enum ColourAction {
    Close,
    Apply(Colour),
}

/// Why a value is refused.
pub const INVALID: &str = "Use a hex colour: #rgb or #rrggbb.";

pub fn show(ctx: &egui::Context, state: &mut ColourState) -> Option<ColourAction> {
    let title = if state.stroke {
        "Custom Stroke"
    } else {
        "Custom Fill"
    };
    let resp = Dialog::compact("blockliner-colour", title).show(ctx, |ui| {
        dialog::field_label(ui, "Hex colour");
        let parsed = Colour::custom(&state.text);
        let submitted = ui
            .horizontal(|ui| {
                let p = theme::palette(ui.ctx());
                let (rect, _) = ui.allocate_exact_size(vec2(30.0, 30.0), Sense::hover());
                match paint::colour(parsed.as_ref()) {
                    Some(c) => ui.painter().rect(
                        rect,
                        theme::RADIUS.nw,
                        c,
                        Stroke::new(1.0, p.border),
                        StrokeKind::Inside,
                    ),
                    None => ui.painter().rect_stroke(
                        rect,
                        theme::RADIUS.nw,
                        Stroke::new(1.0, p.text_faint),
                        StrokeKind::Inside,
                    ),
                };
                let field = dialog::text_input(ui, &mut state.text, "#3b82f6", true);
                if !state.focused {
                    state.focused = true;
                    field.request_focus();
                    select_all(ui, &field, &state.text);
                }
                if field.changed() {
                    state.error = None;
                }
                dialog::submitted(ui, &field)
            })
            .inner;
        if let Some(e) = &state.error {
            ui.add_space(4.0);
            dialog::error_text(ui, e);
        }
        let mut out = None;
        let ok = !state.text.trim().is_empty();
        let mut apply = ok && submitted;
        dialog::actions(ui, |ui| {
            if dialog::button(ui, "Apply", ButtonKind::Primary, ok).clicked() {
                apply = true;
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                out = Some(ColourAction::Close);
            }
        });
        if apply {
            match Colour::custom(&state.text) {
                Some(c) => out = Some(ColourAction::Apply(c)),
                // Refused: say why, and select it for retyping.
                None => {
                    state.error = Some(INVALID.into());
                    state.focused = false;
                }
            }
        }
        out
    });
    if resp.close {
        return Some(ColourAction::Close);
    }
    resp.inner
}

/// Selects the whole value, so typing replaces it.
fn select_all(ui: &egui::Ui, field: &egui::Response, text: &str) {
    let mut s = egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
    s.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    )));
    s.store(ui.ctx(), field.id);
}
