//! Settings: the UI tab (theme, scale, window, canvas) and Keyboard Shortcuts.

use marina_egui::dialog;
use marina_egui::settings::{self, SettingsShell, Tab};
use marina_egui::theme::ThemeState;

use crate::config::GRID_SIZES;

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
    pub show_grid: bool,
    pub grid_size: f32,
}

impl SettingsState {
    pub fn new(
        tab: &str,
        custom_titlebar: bool,
        built_with: bool,
        show_grid: bool,
        grid_size: f32,
    ) -> Self {
        Self {
            tab: tab.to_string(),
            custom_titlebar,
            custom_titlebar_initial: built_with,
            show_grid,
            grid_size,
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
    SetShowGrid(bool),
    SetGridSize(f32),
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
        dialog::muted_text(
            ui,
            "Ctrl+= / Ctrl+- / Ctrl+0 and Ctrl+MouseWheel zoom the diagram, not the UI.",
        );
    });
    settings::setting_group(ui, "Canvas", None, |ui| {
        if settings::toggle_option(ui, "Show Grid", &mut state.show_grid, true).changed() {
            out.push(SettingsAction::SetShowGrid(state.show_grid));
        }
        ui.add_space(8.0);
        dialog::field_label(ui, "Grid Size");
        let options: Vec<(String, String)> = GRID_SIZES
            .iter()
            .map(|g| (g.to_string(), format!("{g} px")))
            .collect();
        let options: Vec<(&str, &str)> = options
            .iter()
            .map(|(v, l)| (v.as_str(), l.as_str()))
            .collect();
        let current = state.grid_size.to_string();
        if let Some(g) = settings::select(ui, "Grid Size", &options, &current)
            .and_then(|v| v.parse::<f32>().ok())
        {
            state.grid_size = g;
            out.push(SettingsAction::SetGridSize(g));
        }
        ui.add_space(4.0);
        dialog::muted_text(ui, "Applies to the open diagram and to new ones.");
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
