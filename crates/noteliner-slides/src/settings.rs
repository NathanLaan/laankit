//! The Presentation Settings dialog (`PresentationSettingsModal.svelte`).
//! Everything is stored in the note's own `presentation:` frontmatter, so the
//! dialog edits a draft and hands back a [`Presentation`] on Save; the host
//! writes it (`set_presentation`) and closes the dialog, or puts the error in
//! [`PresentationSettings::error`] and keeps it open.

use egui::{Key, Modifiers};
use laankit_egui::dialog::{self, ButtonKind, Dialog};
use laankit_egui::settings;

use crate::deck::{Presentation, ASPECTS};

const THEMES: [(&str, &str); 2] = [("dark", "Dark"), ("light", "Light")];
const SLIDE_LEVELS: [(&str, &str); 4] = [
    ("0", "Only --- separators"),
    ("1", "# starts a slide"),
    ("2", "## and above (recommended)"),
    ("3", "### and above"),
];
const WIDTH: f32 = 480.0;

/// The dialog's state while it's open.
#[derive(Clone, Debug)]
pub struct PresentationSettings {
    /// The settings being edited; Cancel discards them.
    pub draft: Presentation,
    /// A save error to show (e.g. "Set your git name and email…").
    pub error: Option<String>,
    /// The host is writing the settings: the Save button reads "Saving...".
    pub saving: bool,
}

/// What the user did this frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SettingsOutcome {
    Open,
    /// Cancel, Escape, the X or a click outside.
    Cancel,
    /// Save (or Ctrl+Enter): write these settings.
    Save(Presentation),
}

impl PresentationSettings {
    /// A dialog editing a snapshot of `current`.
    pub fn new(current: &Presentation) -> Self {
        Self {
            draft: current.clone(),
            error: None,
            saving: false,
        }
    }

    /// Draws the dialog; `slide_count` is the deck's current number of slides.
    pub fn show(&mut self, ctx: &egui::Context, slide_count: usize) -> SettingsOutcome {
        let shown = Dialog::compact("noteliner-presentation-settings", "Presentation Settings")
            .size(dialog::Size::Compact { width: WIDTH })
            .show(ctx, |ui| self.body(ui, slide_count));
        let save_keys = ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
        if shown.close {
            return SettingsOutcome::Cancel;
        }
        match shown.inner {
            Some(outcome) => outcome,
            None if save_keys && !self.saving => SettingsOutcome::Save(self.draft.clone()),
            None => SettingsOutcome::Open,
        }
    }

    fn body(&mut self, ui: &mut egui::Ui, slide_count: usize) -> Option<SettingsOutcome> {
        let d = &mut self.draft;
        ui.columns(2, |cols| {
            dialog::field_label(&mut cols[0], "Theme");
            if let Some(v) = settings::select(&mut cols[0], "Theme", &THEMES, &d.theme) {
                d.theme = v;
            }
            dialog::field_label(&mut cols[1], "Aspect Ratio");
            let aspects: Vec<(&str, &str)> = ASPECTS.iter().map(|(k, _)| (*k, *k)).collect();
            if let Some(v) = settings::select(&mut cols[1], "Aspect Ratio", &aspects, &d.aspect) {
                d.aspect = v;
            }
        });
        ui.add_space(10.0);
        dialog::field_label(ui, "New slide starts at");
        let level = d.slide_level.to_string();
        // Levels 4–6 can come from frontmatter; show them rather than a blank.
        let mut levels: Vec<(&str, &str)> = SLIDE_LEVELS.to_vec();
        let custom = format!("{level} (custom)");
        if !SLIDE_LEVELS.iter().any(|(v, _)| *v == level) {
            levels.push((&level, &custom));
        }
        if let Some(v) = settings::select(ui, "New slide starts at", &levels, &level) {
            d.slide_level = v.parse().unwrap_or(d.slide_level);
        }
        let s = if slide_count == 1 { "" } else { "s" };
        dialog::muted_text(
            ui,
            &format!("Currently {slide_count} slide{s}. A --- line always starts a new slide."),
        );
        ui.add_space(10.0);
        ui.columns(2, |cols| {
            dialog::field_label(&mut cols[0], "Header");
            dialog::text_input(&mut cols[0], &mut d.header, "(none)", true);
            dialog::field_label(&mut cols[1], "Footer");
            dialog::text_input(&mut cols[1], &mut d.footer, "(none)", true);
        });
        dialog::muted_text(ui, "Placeholders: {{name}} {{date}} {{page}} {{total}}");
        ui.add_space(8.0);
        settings::toggle_option(ui, "Show slide numbers", &mut d.slide_numbers, true);
        settings::toggle_option(
            ui,
            "Style a leading # heading as a title slide",
            &mut d.first_slide_title,
            true,
        );
        if let Some(error) = &self.error {
            dialog::error_text(ui, error);
        }
        let mut outcome = None;
        let saving = self.saving;
        dialog::actions(ui, |ui| {
            let label = if saving { "Saving..." } else { "Save" };
            if dialog::button(ui, label, ButtonKind::Primary, !saving).clicked() {
                outcome = Some(SettingsOutcome::Save(self.draft.clone()));
            }
            if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                outcome = Some(SettingsOutcome::Cancel);
            }
        });
        outcome
    }
}
