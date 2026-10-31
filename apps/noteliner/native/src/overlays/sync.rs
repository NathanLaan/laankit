//! `SyncModal.svelte`: the remote URL, branch and status, Pull / Pull &
//! Rebase / Push / Reset from Remote, and Connect / Disconnect, with the
//! same confirmations.

use egui::{pos2, vec2, Align, Color32, Layout, RichText, Sense};
use laankit_core::git::SyncStatus;
use laankit_egui::dialog::{self, ButtonKind, Confirm, Dialog, Size};
use laankit_egui::{icons, theme};

use crate::backend::Op;

#[derive(Default)]
pub struct SyncState {
    /// Waiting for the remote URL and branch.
    pub loading: bool,
    pub remote_input: String,
    pub saved_url: String,
    pub branch: Option<String>,
    /// `None` while checking.
    pub status: Option<SyncStatus>,
    /// What's running ("Pushing..."), and which operation it is.
    pub operating: Option<(Op, &'static str)>,
    pub error: Option<String>,
    pub confirm: Option<ConfirmKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmKind {
    Disconnect,
    Reset,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SyncAction {
    Close,
    RefreshStatus,
    Connect(String),
    Disconnect,
    Pull,
    PullRebase,
    Push,
    Reset,
}

impl SyncState {
    pub fn new() -> Self {
        Self {
            loading: true,
            ..Default::default()
        }
    }

    pub fn has_remote(&self) -> bool {
        !self.saved_url.is_empty()
    }

    fn busy(&self) -> bool {
        self.operating.is_some()
    }

    /// Starts `op` with its progress message; the app calls the backend.
    pub fn start(&mut self, op: Op, message: &'static str) {
        self.error = None;
        self.operating = Some((op, message));
    }

    /// An operation finished. Returns whether it was this dialog's.
    pub fn finish(&mut self, op: Op, result: &Result<(), String>) -> bool {
        if self.operating.map(|o| o.0) != Some(op) {
            return false;
        }
        self.operating = None;
        if let Err(e) = result {
            let what = match op {
                Op::Connect => "Connect",
                Op::Disconnect => "Disconnect",
                Op::Push => "Push",
                Op::Pull => "Pull",
                Op::PullRebase => "Pull & rebase",
                Op::Reset => "Reset",
            };
            self.error = Some(format!("{what} failed: {e}"));
        }
        true
    }
}

/// `statusDotClass`.
fn dot(status: Option<&SyncStatus>) -> Color32 {
    match status {
        Some(SyncStatus::Synced) => Color32::from_rgb(0x4c, 0xaf, 0x50),
        Some(SyncStatus::Ahead { .. }) => Color32::from_rgb(0x42, 0x8b, 0xe8),
        Some(SyncStatus::Behind { .. }) => Color32::from_rgb(0xf0, 0x9a, 0x2b),
        Some(SyncStatus::Diverged { .. } | SyncStatus::Error { .. }) => laankit_egui::toast::DANGER,
        _ => Color32::from_gray(0x88),
    }
}

/// `statusText`.
pub fn status_text(status: Option<&SyncStatus>) -> String {
    let plural = |n: u64| if n == 1 { "" } else { "s" };
    match status {
        None => "Checking...".into(),
        Some(SyncStatus::Synced) => "Synced — local and remote match".into(),
        Some(SyncStatus::Ahead { count }) => {
            format!("Local is {count} commit{} ahead", plural(*count))
        }
        Some(SyncStatus::Behind { count }) => {
            format!("Local is {count} commit{} behind", plural(*count))
        }
        Some(SyncStatus::Diverged { ahead, behind }) => {
            format!("Diverged — {ahead} ahead, {behind} behind")
        }
        Some(SyncStatus::NoUpstream) => "No upstream branch configured".into(),
        Some(SyncStatus::NoRepo) => "Unknown status".into(),
        Some(SyncStatus::Error { message }) => {
            if message.is_empty() {
                "Unknown error".into()
            } else {
                message.clone()
            }
        }
    }
}

pub fn show(ctx: &egui::Context, state: &mut SyncState, top_inset: f32) -> Option<SyncAction> {
    let p = theme::palette(ctx);
    let mut out = None;
    let busy = state.busy();
    let resp = Dialog::full("sync", "Remote Sync")
        .size(Size::Full { top_inset })
        .show(ctx, |ui| {
            if state.loading {
                dialog::muted_text(ui, "Loading...");
                return;
            }
            if !state.has_remote() {
                dialog::muted_text(
                    ui,
                    "No remote repository configured. Enter a URL to connect to GitHub, GitLab, or another Git remote.",
                );
                ui.add_space(12.0);
            }
            if let Some(e) = &state.error {
                dialog::error_text(ui, e);
                ui.add_space(12.0);
            }
            dialog::field_label(ui, "Remote URL");
            ui.horizontal(|ui| {
                let button_w = if state.has_remote() { 40.0 } else { 0.0 };
                ui.allocate_ui(vec2(ui.available_width() - button_w, 34.0), |ui| {
                    dialog::text_input(
                        ui,
                        &mut state.remote_input,
                        "https://github.com/user/repo.git",
                        !busy,
                    );
                });
                if state.has_remote()
                    && small_icon(ui, icons::XMARK, "Disconnect remote", !busy).clicked()
                {
                    state.confirm = Some(ConfirmKind::Disconnect);
                }
            });
            ui.add_space(16.0);
            dialog::field_label(ui, "Branch");
            ui.label(
                RichText::new(state.branch.as_deref().unwrap_or("unknown"))
                    .monospace()
                    .color(p.text_primary),
            );
            if state.has_remote() {
                ui.add_space(16.0);
                dialog::field_label(ui, "Status");
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 5.0, dot(state.status.as_ref()));
                    ui.label(RichText::new(status_text(state.status.as_ref())).color(p.text_primary));
                    if small_icon(ui, icons::ARROWS_ROTATE, "Refresh status", !busy).clicked() {
                        out = Some(SyncAction::RefreshStatus);
                    }
                });
            }
            if let Some((_, message)) = state.operating {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.spinner();
                    dialog::muted_text(ui, message);
                });
            }
            // `.modal-footer`: pinned to the bottom of the dialog.
            let footer_top = ui.max_rect().bottom() - 40.0;
            let rest = (footer_top - ui.cursor().top()).max(12.0);
            ui.add_space(rest);
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top() - 6.0,
                egui::Stroke::new(1.0, p.border),
            );
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 8.0;
                if state.has_remote() {
                    let mut action = |ui: &mut egui::Ui, glyph, label, a: SyncAction| {
                        if dialog::icon_button(ui, Some(glyph), label, ButtonKind::Secondary, !busy)
                            .clicked()
                        {
                            out = Some(a);
                        }
                    };
                    action(ui, icons::CLOUD_ARROW_DOWN, "Pull", SyncAction::Pull);
                    action(ui, icons::CODE_BRANCH, "Pull & Rebase", SyncAction::PullRebase);
                    action(ui, icons::CLOUD_ARROW_UP, "Push", SyncAction::Push);
                    if dialog::icon_button(
                        ui,
                        Some(icons::ROTATE_LEFT),
                        "Reset from Remote",
                        ButtonKind::Danger,
                        !busy,
                    )
                    .clicked()
                    {
                        state.confirm = Some(ConfirmKind::Reset);
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if dialog::button(ui, "Close", ButtonKind::Primary, true).clicked() {
                            out = Some(SyncAction::Close);
                        }
                    });
                } else {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let url = state.remote_input.trim().to_string();
                        if dialog::button(ui, "Connect", ButtonKind::Primary, !busy && !url.is_empty())
                            .clicked()
                        {
                            out = Some(SyncAction::Connect(url));
                        }
                        if dialog::button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                            out = Some(SyncAction::Close);
                        }
                    });
                }
            });
        });
    match state.confirm {
        Some(ConfirmKind::Disconnect) => {
            match dialog::confirm(
                ctx,
                "sync-disconnect",
                "Disconnect Remote",
                "Disconnect from the remote repository?",
                Some(&format!("{}\nYour local files will not be deleted.", state.saved_url)),
                "Disconnect",
            ) {
                Some(Confirm::Yes) => {
                    state.confirm = None;
                    out = Some(SyncAction::Disconnect);
                }
                Some(Confirm::No) => state.confirm = None,
                None => {}
            }
        }
        Some(ConfirmKind::Reset) => {
            match dialog::confirm(
                ctx,
                "sync-reset",
                "Reset from Remote",
                "This will discard ALL local changes and replace your project with the latest version from the remote repository.",
                Some("This action cannot be undone."),
                "Reset",
            ) {
                Some(Confirm::Yes) => {
                    state.confirm = None;
                    out = Some(SyncAction::Reset);
                }
                Some(Confirm::No) => state.confirm = None,
                None => {}
            }
        }
        None => {
            if resp.close {
                out = Some(SyncAction::Close);
            }
        }
    }
    out
}

/// `.icon-btn`: a 28 px square icon button.
fn small_icon(ui: &mut egui::Ui, glyph: &str, label: &str, enabled: bool) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(28.0, 28.0), sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    if enabled && resp.hovered() {
        ui.painter()
            .rect_filled(rect, theme::RADIUS, p.bg_button_hover);
    }
    ui.painter().text(
        pos2(rect.center().x, rect.center().y),
        egui::Align2::CENTER_CENTER,
        glyph,
        icons::font(13.0),
        if enabled {
            p.text_muted
        } else {
            p.text_muted.gamma_multiply(0.4)
        },
    );
    resp.on_hover_text(label)
}
