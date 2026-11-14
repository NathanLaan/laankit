//! Settings: Folders, Merges, Storage, UI, and Keyboard Shortcuts.

use std::path::PathBuf;

use chrono::DateTime;
use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::settings::{self, SettingsShell, Tab};
use laankit_egui::theme::{self, ThemeState};
use medialiner_core::info::human_size;
use medialiner_core::merge::Journal;
use medialiner_core::scan::file_name;

const TABS: [Tab<'static>; 5] = [
    Tab {
        id: "folders",
        label: "Folders",
    },
    Tab {
        id: "merges",
        label: "Merges",
    },
    Tab {
        id: "storage",
        label: "Storage",
    },
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
    pub custom_titlebar: bool,
    pub custom_titlebar_initial: bool,
    pub status_bar_visible: bool,
    /// The thumbnail cache's size and count, once read.
    pub cache: Option<(u64, usize)>,
    /// The merge journals, newest first, once read.
    pub journals: Option<Vec<Journal>>,
}

impl SettingsState {
    pub fn new(tab: &str, custom_titlebar: bool, status_bar_visible: bool) -> Self {
        Self {
            tab: tab.to_string(),
            custom_titlebar,
            custom_titlebar_initial: custom_titlebar,
            status_bar_visible,
            cache: None,
            journals: None,
        }
    }
}

pub struct SettingsProps<'a> {
    pub folders: &'a [PathBuf],
    pub merging: bool,
    pub cache_dir: &'a std::path::Path,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    Close,
    ThemeChanged,
    SetCustomTitlebar(bool),
    Restart,
    SetStatusBar(bool),
    AddFolder,
    RemoveFolder(PathBuf),
    ClearCache,
    /// Undo the merge with this journal.
    Undo(PathBuf),
}

pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    props: &SettingsProps<'_>,
    top_inset: f32,
) -> Vec<SettingsAction> {
    let mut out = Vec::new();
    let mut tab = std::mem::take(&mut state.tab);
    let close =
        SettingsShell::new(&TABS)
            .top_inset(top_inset)
            .show(ctx, &mut tab, |ui, id| match id {
                "folders" => folders_tab(ui, props, &mut out),
                "merges" => merges_tab(ui, state, props, &mut out),
                "storage" => storage_tab(ui, state, props, &mut out),
                "ui" => ui_tab(ui, state, theme_state, &mut out),
                _ => settings::shortcuts_list(ui, &crate::commands::shortcut_list(), None),
            });
    state.tab = tab;
    if close {
        out.push(SettingsAction::Close);
    }
    out
}

fn folders_tab(ui: &mut egui::Ui, props: &SettingsProps<'_>, out: &mut Vec<SettingsAction>) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(
        ui,
        "Your Folders",
        Some("MediaLiner shows the photos in these folders and their subfolders. Removing a folder from the list doesn't change anything on disk."),
        |ui| {
            if props.folders.is_empty() {
                dialog::muted_text(ui, "No folders yet.");
            }
            for folder in props.folders {
                ui.horizontal(|ui| {
                    let missing = !folder.is_dir();
                    ui.label(RichText::new(folder.display().to_string()).color(if missing {
                        p.text_faint
                    } else {
                        p.text_primary
                    }));
                    if missing {
                        ui.label(RichText::new("Not found").size(11.0).color(p.text_muted));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if dialog::button(ui, "Remove", ButtonKind::Secondary, true).clicked() {
                            out.push(SettingsAction::RemoveFolder(folder.clone()));
                        }
                    });
                });
                ui.add_space(4.0);
            }
            ui.add_space(6.0);
            if dialog::button(ui, "Add Folder…", ButtonKind::Primary, true).clicked() {
                out.push(SettingsAction::AddFolder);
            }
        },
    );
}

fn merges_tab(
    ui: &mut egui::Ui,
    state: &SettingsState,
    props: &SettingsProps<'_>,
    out: &mut Vec<SettingsAction>,
) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(
        ui,
        "Recent Merges",
        Some("The last 10 merges, newest first. Undo moves the files back and puts back the folder. A file that has been moved or deleted since stays where it is."),
        |ui| {
            let Some(journals) = &state.journals else {
                dialog::muted_text(ui, "Reading…");
                return;
            };
            if journals.is_empty() {
                dialog::muted_text(ui, "No merges yet.");
            }
            for j in journals {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(format!("{} → {}", file_name(&j.source), file_name(&j.dest)))
                                .color(p.text_primary),
                        );
                        let when = DateTime::parse_from_rfc3339(&j.time)
                            .map(|t| t.format("%Y-%m-%d %H:%M").to_string())
                            .unwrap_or_default();
                        let mut detail = format!(
                            "{when} · {} {}",
                            j.moves.len(),
                            if j.moves.len() == 1 { "file" } else { "files" }
                        );
                        if j.undone {
                            detail.push_str(" · Undone");
                        } else if !j.finished {
                            detail.push_str(" · Interrupted");
                        }
                        ui.label(RichText::new(detail).size(12.0).color(p.text_muted));
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can = !j.undone && !props.merging;
                        if dialog::button(ui, "Undo", ButtonKind::Secondary, can).clicked() {
                            out.push(SettingsAction::Undo(j.path.clone()));
                        }
                    });
                });
                ui.add_space(6.0);
            }
        },
    );
}

fn storage_tab(
    ui: &mut egui::Ui,
    state: &SettingsState,
    props: &SettingsProps<'_>,
    out: &mut Vec<SettingsAction>,
) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(
        ui,
        "Thumbnail Cache",
        Some("Thumbnails are kept so folders open quickly the next time. They're made again when needed, and ones not shown for 90 days are removed."),
        |ui| {
            let text = match state.cache {
                None => "Reading…".to_string(),
                Some((bytes, n)) => format!(
                    "{} {} ({})",
                    n,
                    if n == 1 { "thumbnail" } else { "thumbnails" },
                    human_size(bytes)
                ),
            };
            ui.label(RichText::new(text).color(p.text_primary));
            ui.label(
                RichText::new(props.cache_dir.display().to_string())
                    .size(12.0)
                    .color(p.text_muted),
            );
            ui.add_space(8.0);
            let any = state.cache.is_some_and(|(_, n)| n > 0);
            if dialog::button(ui, "Clear Thumbnail Cache", ButtonKind::Secondary, any).clicked() {
                out.push(SettingsAction::ClearCache);
            }
        },
    );
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
    });
    settings::setting_group(ui, "Window", None, |ui| {
        if settings::toggle_option(ui, "Show Status Bar", &mut state.status_bar_visible, true)
            .changed()
        {
            out.push(SettingsAction::SetStatusBar(state.status_bar_visible));
        }
        ui.add_space(8.0);
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
