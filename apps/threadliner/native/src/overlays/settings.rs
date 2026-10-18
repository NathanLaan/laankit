//! `SettingsModal.svelte`: the UI, Feeds, Sync and Keyboard Shortcuts tabs.

use egui::{Frame, Margin, RichText, ScrollArea, Stroke};
use marina_egui::settings::{self, SettingsShell, Tab};
use marina_egui::theme::{self, ThemeId, ThemeState};
use marina_egui::{dialog, toast};
use serde_json::{json, Value};
use threadliner_core::sync::LogEntry;

use crate::backend::Settings;
use crate::model;

/// ThreadLiner ships these themes (`themeState.init({ themes })` in `main.js`).
pub const THEMES: [ThemeId; 3] = [ThemeId::Light, ThemeId::Dark, ThemeId::Midnight];

const TABS: [Tab<'static>; 4] = [
    Tab {
        id: "ui",
        label: "UI",
    },
    Tab {
        id: "feeds",
        label: "Feeds",
    },
    Tab {
        id: "sync",
        label: "Sync",
    },
    Tab {
        id: "shortcuts",
        label: "Keyboard Shortcuts",
    },
];

const WAIT_TIMES: [(&str, &str); 4] = [
    ("5", "5 seconds"),
    ("10", "10 seconds"),
    ("30", "30 seconds"),
    ("60", "60 seconds"),
];

const POLL_INTERVALS: [(&str, &str); 5] = [
    ("1", "1 minute"),
    ("5", "5 minutes"),
    ("10", "10 minutes"),
    ("30", "30 minutes"),
    ("60", "60 minutes"),
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
    /// Theme or scale changed (already applied): persist and mirror.
    ThemeChanged,
    /// `settings:set`.
    SetSetting(&'static str, Value),
    SetCustomTitlebar(bool),
    Restart,
}

/// What the Sync tab shows.
pub struct SyncView<'a> {
    pub last_sync_time: Option<&'a str>,
    pub log: &'a [LogEntry],
}

pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    current: &Settings,
    sync: SyncView<'_>,
    top_inset: f32,
) -> Vec<SettingsAction> {
    let mut out = Vec::new();
    let mut tab = std::mem::take(&mut state.tab);
    let close =
        SettingsShell::new(&TABS)
            .top_inset(top_inset)
            .show(ctx, &mut tab, |ui, id| match id {
                "ui" => ui_tab(ui, state, theme_state, &mut out),
                "feeds" => feeds_tab(ui, current, &mut out),
                "sync" => sync_tab(ui, current, &sync, &mut out),
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
        if settings::theme_list_among(ui, theme_state, &THEMES) {
            out.push(SettingsAction::ThemeChanged);
        }
    });
    settings::setting_group(ui, "UI Scale", None, |ui| {
        if settings::scale_list(ui, theme_state) {
            out.push(SettingsAction::ThemeChanged);
        }
        ui.add_space(4.0);
        dialog::muted_text(ui, "Or use Ctrl+= / Ctrl+- / Ctrl+0.");
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

fn feeds_tab(ui: &mut egui::Ui, current: &Settings, out: &mut Vec<SettingsAction>) {
    settings::setting_group(
        ui,
        "Poll Interval",
        Some("How often ThreadLiner checks all RSS feeds for new entries."),
        |ui| {
            if let Some(v) =
                settings::select(ui, "Poll Interval", &POLL_INTERVALS, &current.poll_interval)
            {
                out.push(SettingsAction::SetSetting("pollInterval", json!(v)));
            }
        },
    );
    settings::setting_group(
        ui,
        "Notifications",
        Some("Show a system notification when new entries arrive."),
        |ui| {
            let mut on = current.notifications;
            if settings::toggle_option(ui, "Show Toast Notifications", &mut on, true).changed() {
                out.push(SettingsAction::SetSetting(
                    "pollNotificationsEnabled",
                    json!(on),
                ));
            }
        },
    );
}

fn sync_tab(
    ui: &mut egui::Ui,
    current: &Settings,
    sync: &SyncView<'_>,
    out: &mut Vec<SettingsAction>,
) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(
        ui,
        "Sync Wait Time",
        Some("How long to wait after a local change before pushing to the remote repository."),
        |ui| {
            if let Some(v) =
                settings::select(ui, "Sync Wait Time", &WAIT_TIMES, &current.sync_wait_time)
            {
                out.push(SettingsAction::SetSetting("syncWaitTime", json!(v)));
            }
        },
    );
    settings::setting_group(ui, "Last Sync", None, |ui| {
        ui.label(RichText::new(model::last_sync(sync.last_sync_time)).color(p.text_primary));
    });
    settings::setting_group(ui, "Activity", None, |ui| {
        Frame::new()
            .fill(p.pre_bg)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(theme::RADIUS)
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ScrollArea::vertical()
                    .max_height(260.0)
                    .auto_shrink([false, true])
                    .stick_to_bottom(true)
                    .show(ui, |ui| {
                        if sync.log.is_empty() {
                            dialog::muted_text(ui, "No sync activity yet.");
                        }
                        for entry in sync.log {
                            let color = if entry.level == "error" {
                                toast::DANGER
                            } else {
                                p.text_secondary
                            };
                            ui.horizontal_wrapped(|ui| {
                                ui.label(
                                    RichText::new(model::log_time(entry.timestamp))
                                        .monospace()
                                        .size(12.0)
                                        .color(p.text_muted),
                                );
                                ui.label(
                                    RichText::new(&entry.message)
                                        .monospace()
                                        .size(12.0)
                                        .color(color),
                                );
                            });
                            if let Some(detail) = &entry.detail {
                                ui.label(
                                    RichText::new(detail)
                                        .monospace()
                                        .size(12.0)
                                        .color(p.text_muted),
                                );
                            }
                        }
                    });
            });
    });
}
