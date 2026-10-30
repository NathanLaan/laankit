//! The New Diagram and Rename Diagram dialogs: one name field.

use marina_egui::dialog::{self, ButtonKind, Dialog};

pub struct NameState {
    /// The diagram being renamed; `None` for a new one.
    pub renaming: Option<String>,
    pub text: String,
    /// Why the last try failed (an invalid or taken name).
    pub error: Option<String>,
    focused: bool,
}

impl NameState {
    /// A new diagram, suggesting `name` (selected, so typing replaces it).
    pub fn new_diagram(name: String) -> Self {
        Self {
            renaming: None,
            text: name,
            error: None,
            focused: false,
        }
    }

    /// The name was refused: say why, and select it again for retyping.
    pub fn refuse(&mut self, reason: String) {
        self.error = Some(reason);
        self.focused = false;
    }

    pub fn rename(name: &str) -> Self {
        Self {
            renaming: Some(name.to_owned()),
            text: name.to_owned(),
            error: None,
            focused: false,
        }
    }
}

pub enum NameAction {
    Close,
    /// Create or rename with this (untrimmed) name.
    Submit(String),
}

pub fn show(ctx: &egui::Context, state: &mut NameState) -> Option<NameAction> {
    let (title, verb) = match state.renaming {
        Some(_) => ("Rename Diagram", "Rename"),
        None => ("New Diagram", "Create"),
    };
    let resp = Dialog::compact("blockliner-name", title).show(ctx, |ui| {
        dialog::field_label(ui, "Name");
        let field = dialog::text_input(ui, &mut state.text, "Diagram name", true);
        if !state.focused {
            state.focused = true;
            field.request_focus();
            select_all(ui, &field, &state.text);
        }
        if field.changed() {
            state.error = None;
        }
        if let Some(e) = &state.error {
            ui.add_space(4.0);
            dialog::error_text(ui, e);
        }
        dialog::muted_text(ui, "Saved as a Mermaid file, name.mmd, in the open folder.");
        let mut out = None;
        let ok = !state.text.trim().is_empty();
        if ok && dialog::submitted(ui, &field) {
            out = Some(NameAction::Submit(state.text.clone()));
        }
        dialog::actions(ui, |ui| {
            if dialog::button(ui, verb, ButtonKind::Primary, ok).clicked() {
                out = Some(NameAction::Submit(state.text.clone()));
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                out = Some(NameAction::Close);
            }
        });
        out
    });
    if resp.close {
        return Some(NameAction::Close);
    }
    resp.inner
}

/// Selects the whole name, so typing replaces it.
fn select_all(ui: &egui::Ui, field: &egui::Response, text: &str) {
    let mut s = egui::TextEdit::load_state(ui.ctx(), field.id).unwrap_or_default();
    s.cursor.set_char_range(Some(egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(text.chars().count()),
    )));
    s.store(ui.ctx(), field.id);
}
