//! `SettingsModal.svelte`: the UI and Keyboard Shortcuts tabs.

use laankit_egui::dialog;
use laankit_egui::settings::{self, SettingsShell, Tab};
use laankit_egui::theme::ThemeState;

const TABS: [Tab<'static>; 2] = [
    Tab {
        id: "ui",
        label: "UI",
    },
    Tab {
        id: "shortcuts",
        label: "Keyboard Shortcuts",
    },
];

pub struct SettingsState {
    pub tab: String,
    /// The `customTitlebar` pref as the user has it now…
    pub custom_titlebar: bool,
    /// …and as the window was built with (a change needs a restart).
    pub custom_titlebar_initial: bool,
}

impl SettingsState {
    pub fn new(tab: &str, custom_titlebar: bool, built_with: bool) -> Self {
        Self {
            tab: tab.to_string(),
            custom_titlebar,
            custom_titlebar_initial: built_with,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    Close,
    /// Theme or scale changed (already applied): persist it.
    ThemeChanged,
    SetCustomTitlebar(bool),
    Restart,
}

pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    top_inset: f32,
) -> Vec<SettingsAction> {
    let mut out = Vec::new();
    let mut tab = std::mem::take(&mut state.tab);
    let close =
        SettingsShell::new(&TABS)
            .top_inset(top_inset)
            .show(ctx, &mut tab, |ui, id| match id {
                "ui" => ui_tab(ui, state, theme_state, &mut out),
                _ => settings::shortcuts_list(ui, &crate::commands::shortcut_list(), None),
            });
    state.tab = tab;
    if close {
        out.push(SettingsAction::Close);
    }
    out
}

fn ui_tab(
    ui: &mut egui::Ui,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    out: &mut Vec<SettingsAction>,
) {
    settings::setting_group(ui, "Theme", None, |ui| {
        if settings::theme_list(ui, theme_state) {
            out.push(SettingsAction::ThemeChanged);
        }
    });
    settings::setting_group(ui, "UI Scale", None, |ui| {
        if settings::scale_list(ui, theme_state) {
            out.push(SettingsAction::ThemeChanged);
        }
        ui.add_space(4.0);
        dialog::muted_text(ui, "Or use Ctrl+= / Ctrl+- / Ctrl+0, or Ctrl+MouseWheel.");
    });
    settings::setting_group(ui, "Window", None, |ui| {
        if settings::toggle_option(
            ui,
            "Custom Window Titlebar",
            &mut state.custom_titlebar,
            true,
        )
        .changed()
        {
            out.push(SettingsAction::SetCustomTitlebar(state.custom_titlebar));
        }
        if state.custom_titlebar != state.custom_titlebar_initial
            && settings::restart_banner(ui, Some("Restart required to apply titlebar change."))
        {
            out.push(SettingsAction::Restart);
        }
    });
}
