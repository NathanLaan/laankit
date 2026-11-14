//! Settings: Account, Printers (display names and order), UI, and Keyboard
//! Shortcuts.

use chrono::{DateTime, Local, Utc};
use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::settings::{self, SettingsShell, Tab};
use laankit_egui::theme::{self, ThemeState};
use printliner_core::Region;

use crate::backend::Account;

const TABS: [Tab<'static>; 4] = [
    Tab {
        id: "account",
        label: "Account",
    },
    Tab {
        id: "printers",
        label: "Printers",
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

/// A printer row on the Printers tab.
pub struct PrinterRow {
    pub serial: String,
    /// The name on the Bambu account.
    pub account_name: String,
    /// The name PrintLiner shows, as being edited.
    pub name: String,
}

pub struct SettingsState {
    pub tab: String,
    pub custom_titlebar: bool,
    pub custom_titlebar_initial: bool,
    pub status_bar_visible: bool,
    pub printers: Vec<PrinterRow>,
}

impl SettingsState {
    pub fn new(
        tab: &str,
        custom_titlebar: bool,
        built_with: bool,
        status_bar_visible: bool,
        printers: Vec<PrinterRow>,
    ) -> Self {
        Self {
            tab: tab.to_string(),
            custom_titlebar,
            custom_titlebar_initial: built_with,
            status_bar_visible,
            printers,
        }
    }
}

/// What the Account tab shows.
pub struct AccountProps<'a> {
    pub account: &'a Account,
    pub region: Region,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    Close,
    ThemeChanged,
    SetCustomTitlebar(bool),
    Restart,
    SetStatusBar(bool),
    SignIn,
    SignOut,
    /// A display name (empty: the account's name).
    Rename(String, String),
    /// The printers' new order, by serial.
    Reorder(Vec<String>),
}

pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    account: &AccountProps<'_>,
    top_inset: f32,
) -> Vec<SettingsAction> {
    let mut out = Vec::new();
    let mut tab = std::mem::take(&mut state.tab);
    let close =
        SettingsShell::new(&TABS)
            .top_inset(top_inset)
            .show(ctx, &mut tab, |ui, id| match id {
                "account" => account_tab(ui, account, &mut out),
                "printers" => printers_tab(ui, state, &mut out),
                "ui" => ui_tab(ui, state, theme_state, &mut out),
                _ => settings::shortcuts_list(ui, &crate::commands::shortcut_list(), None),
            });
    state.tab = tab;
    if close {
        out.push(SettingsAction::Close);
    }
    out
}

fn expiry_text(at: DateTime<Utc>) -> String {
    let days = (at - Utc::now()).num_days();
    let date = at.with_timezone(&Local).format("%B %-d, %Y");
    match days {
        d if d < 0 => format!("Expired on {date}"),
        0 => format!("Expires today ({date})"),
        1 => format!("Expires tomorrow ({date})"),
        d => format!("Expires in {d} days ({date})"),
    }
}

fn account_tab(ui: &mut egui::Ui, props: &AccountProps<'_>, out: &mut Vec<SettingsAction>) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(ui, "Bambu Lab Account", None, |ui| {
        let line = |ui: &mut egui::Ui, label: &str, value: String| {
            dialog::field_label(ui, label);
            ui.label(RichText::new(value).color(p.text_primary));
            ui.add_space(6.0);
        };
        match props.account {
            Account::SignedIn { demo: true, .. } => {
                dialog::muted_text(
                    ui,
                    "Demo mode: the printers are made up, and there's no account.",
                );
            }
            Account::SignedIn {
                email,
                expires_at,
                token_in_file,
                ..
            } => {
                line(ui, "Signed In As", email.clone());
                line(ui, "Region", props.region.label().to_string());
                if let Some(at) = expires_at {
                    line(ui, "Sign-In", expiry_text(*at));
                }
                let stored = match token_in_file {
                    Some(true) => "In a file only you can read (no system keychain was available)",
                    Some(false) => "In the system keychain",
                    None => "In memory only, until PrintLiner closes",
                };
                line(ui, "Token Stored", stored.to_string());
                dialog::actions(ui, |ui| {
                    if dialog::button(ui, "Sign Out", ButtonKind::Secondary, true).clicked() {
                        out.push(SettingsAction::SignOut);
                    }
                });
            }
            Account::SignInNeeded { email } => {
                line(ui, "Account", email.clone());
                dialog::error_text(ui, "Bambu Lab needs you to sign in again.");
                dialog::actions(ui, |ui| {
                    if dialog::button(ui, "Sign In", ButtonKind::Primary, true).clicked() {
                        out.push(SettingsAction::SignIn);
                    }
                    if dialog::button(ui, "Sign Out", ButtonKind::Secondary, true).clicked() {
                        out.push(SettingsAction::SignOut);
                    }
                });
            }
            Account::SignedOut | Account::Starting => {
                dialog::muted_text(ui, "Not signed in.");
                dialog::actions(ui, |ui| {
                    if dialog::button(ui, "Sign In", ButtonKind::Primary, true).clicked() {
                        out.push(SettingsAction::SignIn);
                    }
                });
            }
        }
    });
}

fn printers_tab(ui: &mut egui::Ui, state: &mut SettingsState, out: &mut Vec<SettingsAction>) {
    settings::setting_group(
        ui,
        "Names and Order",
        Some("Changes how PrintLiner shows your printers. The names on your Bambu Lab account stay as they are."),
        |ui| {
            if state.printers.is_empty() {
                dialog::muted_text(ui, "No printers yet.");
                return;
            }
            let mut moved: Option<(usize, usize)> = None;
            let n = state.printers.len();
            for (i, row) in state.printers.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    if dialog::icon_button(ui, Some(laankit_egui::icons::ARROW_UP), "Up", ButtonKind::Secondary, i > 0).clicked() {
                        moved = Some((i, i - 1));
                    }
                    if dialog::icon_button(ui, Some(laankit_egui::icons::ARROW_DOWN), "Down", ButtonKind::Secondary, i + 1 < n).clicked() {
                        moved = Some((i, i + 1));
                    }
                    let resp = dialog::text_input(ui, &mut row.name, &row.account_name, true);
                    if resp.lost_focus() || dialog::submitted(ui, &resp) {
                        out.push(SettingsAction::Rename(row.serial.clone(), row.name.trim().to_string()));
                    }
                });
                ui.add_space(4.0);
            }
            if let Some((from, to)) = moved {
                state.printers.swap(from, to);
                out.push(SettingsAction::Reorder(
                    state.printers.iter().map(|r| r.serial.clone()).collect(),
                ));
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
