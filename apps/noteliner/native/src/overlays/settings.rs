//! `SettingsModal.svelte`: the UI, MCP and Keyboard Shortcuts tabs.
//!
//! Turning the MCP server on asks first, with `McpWalkthroughModal`
//! ([`super::mcp::walkthrough`]): MCP stays off until Enable is clicked.

use egui::RichText;
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::settings::{self, SettingsShell, Tab};
use laankit_egui::theme::ThemeState;
use laankit_egui::{fonts, theme};
use serde_json::{json, Value};

use crate::backend::McpStatus;
use crate::config::Prefs;

const TABS: [Tab<'static>; 3] = [
    Tab {
        id: "ui",
        label: "UI",
    },
    Tab {
        id: "mcp",
        label: "MCP",
    },
    Tab {
        id: "shortcuts",
        label: "Keyboard Shortcuts",
    },
];

pub struct SettingsState {
    pub tab: String,
    pub custom_titlebar: bool,
    /// As the window was built (a change needs a restart).
    pub custom_titlebar_initial: bool,
    pub write_frontmatter: bool,
    pub spell_check: bool,
    pub mcp_enabled: bool,
    pub mcp_confirm_writes: bool,
    pub mcp_disabled_tools: Vec<String>,
    pub mcp_status: Option<McpStatus>,
    /// The MCP walkthrough is open (asking before turning MCP on).
    pub confirm_mcp: bool,
    /// The walkthrough's "Copied" shows until this time.
    walkthrough_copied_until: f64,
    /// "Copied" shows until this time.
    copied_until: f64,
}

impl SettingsState {
    pub fn new(tab: &str, prefs: &Prefs, built_with_titlebar: bool) -> Self {
        Self {
            tab: tab.to_string(),
            custom_titlebar: prefs.custom_titlebar,
            custom_titlebar_initial: built_with_titlebar,
            write_frontmatter: prefs.write_frontmatter,
            spell_check: prefs.spell_check,
            mcp_enabled: prefs.mcp_enabled,
            mcp_confirm_writes: prefs.mcp_confirm_writes,
            mcp_disabled_tools: prefs.mcp_disabled_tools.clone(),
            mcp_status: None,
            confirm_mcp: false,
            walkthrough_copied_until: 0.0,
            copied_until: 0.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsAction {
    Close,
    /// Theme or scale changed (already applied): persist it.
    ThemeChanged,
    /// `ui:setPrefs` with this patch.
    SetPrefs(Value),
    RefreshMcp,
    Restart,
}

/// The client config snippet (`mcpConfigSnippet`).
pub fn mcp_config_snippet(status: &McpStatus) -> String {
    let v = json!({
        "mcpServers": {
            "noteliner": {
                "command": status.bridge_command,
                "args": ["--mcp-bridge"],
            }
        }
    });
    serde_json::to_string_pretty(&v).unwrap_or_default()
}

/// `mcpStatusText`.
pub fn mcp_status_text(status: Option<&McpStatus>) -> &'static str {
    match status {
        None => "…",
        Some(s) if !s.enabled => "Disabled",
        Some(s) if !s.project_open => "Idle (no project open)",
        Some(s) if s.running => "Running",
        Some(_) => "Stopped",
    }
}

pub fn show(
    ctx: &egui::Context,
    state: &mut SettingsState,
    theme_state: &mut ThemeState,
    top_inset: f32,
) -> Vec<SettingsAction> {
    let mut out = Vec::new();
    let before = state.tab.clone();
    let mut tab = std::mem::take(&mut state.tab);
    let close =
        SettingsShell::new(&TABS)
            .top_inset(top_inset)
            .show(ctx, &mut tab, |ui, id| match id {
                "ui" => ui_tab(ui, state, theme_state, &mut out),
                "mcp" => mcp_tab(ui, state, &mut out),
                _ => settings::shortcuts_list(ui, &crate::commands::shortcut_list(), None),
            });
    if tab == "mcp" && before != "mcp" {
        out.push(SettingsAction::RefreshMcp);
    }
    state.tab = tab;
    if state.confirm_mcp {
        match super::mcp::walkthrough(
            ctx,
            state.mcp_status.as_ref(),
            &mut state.walkthrough_copied_until,
        ) {
            Some(super::mcp::Walkthrough::Enable) => {
                state.confirm_mcp = false;
                state.mcp_enabled = true;
                out.push(SettingsAction::SetPrefs(json!({ "mcpEnabled": true })));
            }
            Some(super::mcp::Walkthrough::Cancel) => state.confirm_mcp = false,
            None => {}
        }
    } else if close {
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
            out.push(SettingsAction::SetPrefs(
                json!({ "customTitlebar": state.custom_titlebar }),
            ));
        }
        if state.custom_titlebar != state.custom_titlebar_initial
            && settings::restart_banner(ui, Some("Restart required to apply titlebar change."))
        {
            out.push(SettingsAction::Restart);
        }
    });
    settings::setting_group(
        ui,
        "Storage",
        Some("Mirrors id, name, and tags into a YAML block at the top of each note so external tools can read them. Applies to future saves; files are reconciled on next project open."),
        |ui| {
            if settings::toggle_option(
                ui,
                "Write YAML frontmatter to .md files",
                &mut state.write_frontmatter,
                true,
            )
            .changed()
            {
                out.push(SettingsAction::SetPrefs(
                    json!({ "writeFrontmatter": state.write_frontmatter }),
                ));
            }
        },
    );
    settings::setting_group(
        ui,
        "Editor",
        Some("Underlines misspelled words in the editor. Toggle with F7. (The native editor's spell checker arrives with laankit-editor; the setting is shared with the other builds.)"),
        |ui| {
            if settings::toggle_option(ui, "Check spelling", &mut state.spell_check, true).changed() {
                out.push(SettingsAction::SetPrefs(
                    json!({ "spellCheckEnabled": state.spell_check }),
                ));
            }
        },
    );
}

fn mcp_tab(ui: &mut egui::Ui, state: &mut SettingsState, out: &mut Vec<SettingsAction>) {
    let p = theme::palette(ui.ctx());
    settings::setting_group(
        ui,
        "Model Context Protocol Server",
        Some("Lets external AI assistants (Claude Code, Claude Desktop, Cursor) list, read, search, and write notes in the currently-open project. All writes go through the normal save path and are committed to git. The server is local-only — no network port is opened."),
        |ui| {
            let mut on = state.mcp_enabled;
            if settings::toggle_option(ui, "Enable MCP server", &mut on, true).changed() {
                if on {
                    // Off → on asks first (the walkthrough's gate).
                    state.confirm_mcp = true;
                } else {
                    state.mcp_enabled = false;
                    out.push(SettingsAction::SetPrefs(json!({ "mcpEnabled": false })));
                }
            }
        },
    );
    settings::setting_group(ui, "Status", None, |ui| {
        ui.horizontal(|ui| {
            let running = state.mcp_status.as_ref().is_some_and(|s| s.running);
            let (r, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
            ui.painter().circle_filled(
                r.center(),
                5.0,
                if running {
                    egui::Color32::from_rgb(0x4c, 0xaf, 0x50)
                } else {
                    egui::Color32::from_gray(0x88)
                },
            );
            ui.label(
                RichText::new(mcp_status_text(state.mcp_status.as_ref())).color(p.text_primary),
            );
            if ui.link("Refresh").clicked() {
                out.push(SettingsAction::RefreshMcp);
            }
        });
        if let Some(s) = &state.mcp_status {
            if let Some(sock) = &s.socket_path {
                kv(ui, &p, "Socket", &sock.display().to_string());
            }
            kv(
                ui,
                &p,
                "Bridge",
                &format!("{} --mcp-bridge", s.bridge_command.display()),
            );
        }
    });
    if let (true, Some(status)) = (state.mcp_enabled, state.mcp_status.clone()) {
        settings::setting_group(ui, "Client Configuration", None, |ui| {
            dialog::muted_text(
                ui,
                "Paste this into your MCP client's config file (for Claude Code: .mcp.json in the project; for Claude Desktop: claude_desktop_config.json).",
            );
            ui.add_space(6.0);
            let snippet = mcp_config_snippet(&status);
            egui::Frame::NONE
                .fill(p.pre_bg)
                .stroke(egui::Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .inner_margin(egui::Margin::same(10))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(&snippet)
                            .font(fonts::mono(12.0))
                            .color(p.text_primary),
                    );
                });
            let now = ui.input(|i| i.time);
            let label = if now < state.copied_until {
                "Copied"
            } else {
                "Copy"
            };
            if dialog::button(ui, label, ButtonKind::Secondary, true).clicked() {
                ui.ctx().copy_text(snippet);
                state.copied_until = now + 1.5;
            }
        });
        settings::setting_group(ui, "Write Safety", None, |ui| {
            if settings::toggle_option(
                ui,
                "Confirm before each write",
                &mut state.mcp_confirm_writes,
                true,
            )
            .changed()
            {
                out.push(SettingsAction::SetPrefs(
                    json!({ "mcpConfirmWrites": state.mcp_confirm_writes }),
                ));
            }
        });
        settings::setting_group(
            ui,
            "Tools",
            Some("Disabled tools are hidden from clients."),
            |ui| {
                for (section, tools) in
                    [("Read", &status.read_tools), ("Write", &status.write_tools)]
                {
                    ui.label(RichText::new(section).size(12.0).color(p.text_muted));
                    for tool in tools {
                        let mut enabled = !state.mcp_disabled_tools.contains(tool);
                        if ui
                            .checkbox(&mut enabled, RichText::new(tool).font(fonts::mono(12.0)))
                            .changed()
                        {
                            if enabled {
                                state.mcp_disabled_tools.retain(|t| t != tool);
                            } else {
                                state.mcp_disabled_tools.push(tool.clone());
                            }
                            out.push(SettingsAction::SetPrefs(
                                json!({ "mcpDisabledTools": state.mcp_disabled_tools }),
                            ));
                        }
                    }
                    ui.add_space(6.0);
                }
            },
        );
    }
}

fn kv(ui: &mut egui::Ui, p: &theme::Palette, k: &str, v: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(k).size(12.0).color(p.text_muted));
        ui.label(
            RichText::new(v)
                .font(fonts::mono(12.0))
                .color(p.text_secondary),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_texts() {
        let s = McpStatus {
            enabled: true,
            project_open: true,
            running: true,
            bridge_command: "/usr/bin/noteliner".into(),
            ..Default::default()
        };
        assert_eq!(mcp_status_text(Some(&s)), "Running");
        assert_eq!(mcp_status_text(None), "…");
        let snippet = mcp_config_snippet(&s);
        let v: Value = serde_json::from_str(&snippet).unwrap();
        assert_eq!(
            v["mcpServers"]["noteliner"]["args"],
            json!(["--mcp-bridge"])
        );
        assert_eq!(
            v["mcpServers"]["noteliner"]["command"],
            json!("/usr/bin/noteliner")
        );
    }
}
