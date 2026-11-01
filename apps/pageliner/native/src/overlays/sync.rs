//! `SyncModal.svelte`: opt-in Git sync of the library index and reading
//! state. Enable/disable, the remote URL, Sync Now and Check status.

use egui::RichText;
use laankit_core::git::SyncStatus;
use laankit_egui::dialog::{self, ButtonKind, Dialog, Size};
use laankit_egui::{icons, theme};

use crate::backend::{GitInfo, GitOp};

/// `.sync-modal { max-width: 520px }`.
const WIDTH: f32 = 520.0;
const WARN: egui::Color32 = egui::Color32::from_rgb(0xd0, 0x87, 0x00);
const OK: egui::Color32 = egui::Color32::from_rgb(0x2e, 0xa0, 0x43);
const ERR: egui::Color32 = egui::Color32::from_rgb(0xe0, 0x48, 0x4d);

#[derive(Default)]
pub struct SyncState {
    /// `None` while loading (`git:getInfo`).
    pub info: Option<GitInfo>,
    pub remote_input: String,
    pub status: Option<SyncStatus>,
    /// The operation running, if any (`busy`).
    pub busy: Option<GitOp>,
    pub message: Option<String>,
    pub error: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SyncAction {
    Close,
    SetEnabled(bool),
    SaveRemote(Option<String>),
    SyncNow,
    CheckStatus,
}

impl SyncState {
    /// `refresh`: new info replaces the remote field.
    pub fn set_info(&mut self, info: GitInfo) {
        self.remote_input = info.remote_url.clone().unwrap_or_default();
        self.info = Some(info);
    }

    pub fn start(&mut self, op: GitOp) {
        self.busy = Some(op);
        self.error = None;
        if op != GitOp::Status {
            self.message = None;
        }
    }

    /// An operation finished.
    pub fn finish(&mut self, op: GitOp, result: Result<Option<SyncStatus>, String>) {
        if self.busy == Some(op) {
            self.busy = None;
        }
        match result {
            Err(e) => self.error = Some(e),
            Ok(status) => {
                if status.is_some() {
                    self.status = status;
                }
                self.message = match op {
                    GitOp::SyncNow => Some("Synced.".into()),
                    GitOp::SetRemote if self.remote_input.trim().is_empty() => {
                        Some("Remote removed.".into())
                    }
                    GitOp::SetRemote => Some("Remote saved.".into()),
                    _ => self.message.take(),
                };
            }
        }
    }
}

/// `statusText`.
pub fn status_text(s: &SyncStatus) -> String {
    match s {
        SyncStatus::Synced => "Up to date with remote.".into(),
        SyncStatus::Ahead { count } => format!("{count} commit(s) ahead — push to publish."),
        SyncStatus::Behind { count } => format!("{count} commit(s) behind — sync to update."),
        SyncStatus::Diverged { ahead, behind } => {
            format!("Diverged: {ahead} ahead, {behind} behind.")
        }
        SyncStatus::NoUpstream => "No upstream yet — Sync Now to publish.".into(),
        SyncStatus::NoRepo => "Sync not set up.".into(),
        SyncStatus::Error { message } => format!("Error: {message}"),
    }
}

pub fn show(ctx: &egui::Context, state: &mut SyncState) -> Option<SyncAction> {
    let mut action = None;
    let resp = Dialog::compact("pageliner-sync", "Git Sync")
        .size(Size::Compact { width: WIDTH })
        .show(ctx, |ui| {
            let p = theme::palette(ui.ctx());
            let busy = state.busy.is_some();
            match &state.info {
                None => {
                    dialog::muted_text(ui, "Loading…");
                }
                Some(info) if !info.git_installed => {
                    ui.label(
                        RichText::new(format!(
                            "{}  Git is not installed or not on your PATH. Install Git to enable sync.",
                            icons::TRIANGLE_EXCLAMATION
                        ))
                        .size(13.0)
                        .color(WARN),
                    );
                }
                Some(info) => {
                    let enabled = info.enabled;
                    let branch = info.branch.clone();
                    ui.label(
                        RichText::new(
                            "Sync versions your library index and reading state (positions, \
                             bookmarks, highlights) through a Git remote. Book files stay on \
                             this device — only metadata syncs, so other devices see the list \
                             and your progress.",
                        )
                        .size(13.0)
                        .color(p.text_secondary),
                    );
                    ui.add_space(8.0);
                    let mut on = enabled;
                    let toggle = ui.add_enabled(
                        !busy,
                        egui::Checkbox::new(&mut on, RichText::new("Enable Git sync").size(14.0)),
                    );
                    if toggle.changed() {
                        action = Some(SyncAction::SetEnabled(on));
                    }
                    if enabled {
                        ui.add_space(8.0);
                        ui.label(RichText::new("Remote URL").size(12.0).color(p.text_secondary));
                        ui.horizontal(|ui| {
                            let save_w = 70.0;
                            ui.allocate_ui(egui::vec2(ui.available_width() - save_w - 8.0, 32.0), |ui| {
                                let resp = dialog::text_input(
                                    ui,
                                    &mut state.remote_input,
                                    "git@github.com:you/library.git",
                                    !busy,
                                );
                                if dialog::submitted(ui, &resp) {
                                    action = Some(save_remote(&state.remote_input));
                                }
                            });
                            if dialog::button(ui, "Save", ButtonKind::Secondary, !busy).clicked() {
                                action = Some(save_remote(&state.remote_input));
                            }
                        });
                        if let Some(branch) = branch {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Branch:").size(12.0).color(p.text_muted));
                                ui.label(RichText::new(branch).size(12.0).monospace().color(p.text_muted));
                            });
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let glyph = if state.busy == Some(GitOp::SyncNow) {
                                icons::SPINNER
                            } else {
                                icons::ROTATE
                            };
                            if dialog::icon_button(ui, Some(glyph), "Sync Now", ButtonKind::Primary, !busy)
                                .clicked()
                            {
                                action = Some(SyncAction::SyncNow);
                            }
                            if dialog::button(ui, "Check status", ButtonKind::Secondary, !busy).clicked() {
                                action = Some(SyncAction::CheckStatus);
                            }
                        });
                        if let Some(status) = &state.status {
                            ui.add_space(4.0);
                            ui.label(RichText::new(status_text(status)).size(13.0).color(p.text_secondary));
                        }
                    }
                }
            }
            if let Some(m) = &state.message {
                ui.add_space(4.0);
                ui.label(RichText::new(m).size(13.0).color(OK));
            }
            if let Some(e) = &state.error {
                ui.add_space(4.0);
                ui.label(RichText::new(e).size(13.0).color(ERR));
            }
            ui.add_space(8.0);
            dialog::actions(ui, |ui| {
                if dialog::button(ui, "Close", ButtonKind::Secondary, true).clicked() {
                    action = Some(SyncAction::Close);
                }
            });
        });
    if resp.close {
        action = Some(SyncAction::Close);
    }
    action
}

/// `saveRemote`: an empty field removes the remote.
fn save_remote(input: &str) -> SyncAction {
    let url = input.trim();
    SyncAction::SaveRemote((!url.is_empty()).then(|| url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_text_matches_the_svelte_wording() {
        assert_eq!(
            status_text(&SyncStatus::Diverged {
                ahead: 2,
                behind: 1
            }),
            "Diverged: 2 ahead, 1 behind."
        );
        assert_eq!(status_text(&SyncStatus::NoRepo), "Sync not set up.");
    }

    #[test]
    fn finishing_shows_the_outcome() {
        let mut s = SyncState::default();
        s.start(GitOp::SyncNow);
        s.finish(GitOp::SyncNow, Ok(Some(SyncStatus::Synced)));
        assert_eq!(s.busy, None);
        assert_eq!(s.message.as_deref(), Some("Synced."));
        assert_eq!(s.status, Some(SyncStatus::Synced));
        s.start(GitOp::SyncNow);
        s.finish(GitOp::SyncNow, Err("Pull failed: nope".into()));
        assert_eq!(s.error.as_deref(), Some("Pull failed: nope"));
        assert_eq!(save_remote("  "), SyncAction::SaveRemote(None));
    }
}
