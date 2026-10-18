//! The About dialog, natively: `components/AboutModal.svelte`.
//!
//! App name and version, an optional update check (driven by the app's
//! updater state, see [`UpdateStatus`]), a description, a repository link
//! and an icon tile. Draw it every frame while open; [`AboutModal::show`]
//! returns what the user asked for.

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Image, ImageSource,
    Layout, Margin, Modal, RichText, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::icons;
use crate::settings::{modal_area, modal_frame, modal_header, outline_button};
use crate::theme::{self, Palette};

/// `.about-modal { width: 50%; height: 50%; min-width: 480px; min-height: 380px }`.
const MIN_SIZE: Vec2 = vec2(480.0, 380.0);
/// `.app-icon { width: 96px; height: 96px }`.
const ICON_SIZE: f32 = 96.0;

/// The updater's state, as the Svelte `updateState.state` machine.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateStatus {
    Idle,
    Checking,
    Available {
        version: String,
        notes: Option<String>,
    },
    Downloading {
        percent: f32,
    },
    Downloaded {
        version: String,
        notes: Option<String>,
    },
    /// `dev`: updates are disabled in development builds.
    Unavailable {
        dev: bool,
    },
    Error(String),
}

impl UpdateStatus {
    /// The status line under the version.
    pub fn line(&self) -> Option<String> {
        Some(match self {
            UpdateStatus::Idle => return None,
            UpdateStatus::Checking => "Checking for updates…".into(),
            UpdateStatus::Available { version, .. } => format!("Version {version} is available."),
            UpdateStatus::Downloading { percent } => {
                format!("Downloading update… {}%", percent.round())
            }
            UpdateStatus::Downloaded { version, .. } => {
                format!("Version {version} is ready to install.")
            }
            UpdateStatus::Unavailable { dev: true } => {
                "Updates are disabled in development builds.".into()
            }
            UpdateStatus::Unavailable { dev: false } => {
                "You are running the latest version.".into()
            }
            UpdateStatus::Error(e) => format!("Update error: {e}"),
        })
    }

    fn busy(&self) -> bool {
        matches!(
            self,
            UpdateStatus::Checking | UpdateStatus::Downloading { .. }
        )
    }
}

/// The icon in the right-hand column.
#[derive(Clone)]
pub enum AboutIcon<'a> {
    /// A Font Awesome glyph on an accent tile (`iconClass`).
    Glyph(&'static str),
    /// An image, e.g. the app's SVG or PNG icon (`iconSvg`).
    Image(ImageSource<'a>),
}

/// What the user did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AboutAction {
    Close,
    CheckForUpdates,
    Download,
    Install,
    /// Open this URL in the browser (the app may route it, e.g. through its
    /// own opener).
    OpenUrl(String),
}

/// The dialog's content (`AboutModal` props).
#[derive(Clone)]
pub struct AboutModal<'a> {
    pub app_name: &'a str,
    pub version: &'a str,
    pub description: &'a str,
    pub repo_url: Option<&'a str>,
    pub repo_label: Option<&'a str>,
    pub icon: Option<AboutIcon<'a>>,
    /// `None` hides all update UI.
    pub update: Option<&'a UpdateStatus>,
}

impl<'a> AboutModal<'a> {
    pub fn new(app_name: &'a str, version: &'a str) -> Self {
        Self {
            app_name,
            version,
            description: "",
            repo_url: None,
            repo_label: None,
            icon: None,
            update: None,
        }
    }

    pub fn description(mut self, text: &'a str) -> Self {
        self.description = text;
        self
    }

    pub fn repo(mut self, url: &'a str, label: Option<&'a str>) -> Self {
        self.repo_url = Some(url);
        self.repo_label = label;
        self
    }

    pub fn icon(mut self, icon: AboutIcon<'a>) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn update(mut self, status: &'a UpdateStatus) -> Self {
        self.update = Some(status);
        self
    }

    /// Draws the dialog, anchored to the bottom centre like the Svelte one.
    pub fn show(&self, ctx: &egui::Context) -> Option<AboutAction> {
        let p = theme::palette(ctx);
        let screen = ctx.content_rect();
        let size = (screen.size() * 0.5).max(MIN_SIZE).min(screen.size());
        let pos = pos2(screen.center().x - size.x / 2.0, screen.bottom() - size.y);
        let mut action = None;
        let modal = Modal::new(Id::unique("marina-about"))
            .area(modal_area(Id::unique("marina-about"), pos))
            .backdrop_color(p.modal_overlay)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                ui.set_min_size(size);
                ui.set_max_size(size);
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                if modal_header(ui, &p, "About") {
                    action = Some(AboutAction::Close);
                }
                Frame::new()
                    .inner_margin(Margin {
                        left: 24,
                        right: 24,
                        top: 24,
                        bottom: 20,
                    })
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        // Right to left: the icon tile at the edge, the details filling the rest.
                        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
                            ui.spacing_mut().item_spacing.x = 24.0;
                            if let Some(icon) = &self.icon {
                                self.icon_tile(ui, &p, icon);
                            }
                            ui.with_layout(Layout::top_down(Align::Min), |ui| {
                                if let Some(a) = self.details(ui, &p) {
                                    action = Some(a);
                                }
                            });
                        });
                        ui.with_layout(Layout::bottom_up(Align::Max), |ui| {
                            if outline_button(ui, &p, "OK", vec2(24.0, 8.0)).clicked() {
                                action = Some(AboutAction::Close);
                            }
                        });
                    });
            });
        if action.is_none() && modal.should_close() {
            action = Some(AboutAction::Close);
        }
        action
    }

    /// `.header-left`: name, version and update UI, description, link.
    fn details(&self, ui: &mut Ui, p: &Palette) -> Option<AboutAction> {
        let mut action = None;
        ui.label(
            RichText::new(self.app_name)
                .size(22.0)
                .color(p.text_primary),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            ui.label(RichText::new(format!("Version {}", self.version)).color(p.text_muted));
            if let Some(status) = self.update {
                if check_button(ui, p, status).clicked() {
                    action = Some(AboutAction::CheckForUpdates);
                }
            }
        });
        ui.add_space(8.0);
        if let Some(status) = self.update {
            if let Some(line) = status.line() {
                let color = if matches!(status, UpdateStatus::Error(_)) {
                    Color32::from_rgb(0xcc, 0x33, 0x33)
                } else {
                    p.text_secondary
                };
                ui.label(RichText::new(line).size(13.0).color(color));
                ui.add_space(12.0);
            }
            let (label, act) = match status {
                UpdateStatus::Available { .. } => ("Download", AboutAction::Download),
                UpdateStatus::Downloaded { .. } => ("Restart and Install", AboutAction::Install),
                _ => ("", AboutAction::Close),
            };
            if !label.is_empty() {
                if outline_button(ui, p, label, vec2(14.0, 6.0)).clicked() {
                    action = Some(act);
                }
                ui.add_space(12.0);
            }
            if let UpdateStatus::Available {
                notes: Some(notes), ..
            }
            | UpdateStatus::Downloaded {
                notes: Some(notes), ..
            } = status
            {
                egui::CollapsingHeader::new(
                    RichText::new("Release notes").size(13.0).color(p.accent),
                )
                .id_salt("marina-about-notes")
                .show(ui, |ui| {
                    Frame::new()
                        .fill(p.bg_overlay)
                        .stroke(Stroke::new(1.0, p.border))
                        .corner_radius(CornerRadius::same(6))
                        .inner_margin(Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .max_height(160.0)
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(strip_tags(notes))
                                            .size(13.0)
                                            .color(p.text_secondary),
                                    );
                                });
                        });
                });
                ui.add_space(12.0);
            }
        }
        if !self.description.is_empty() {
            ui.label(RichText::new(self.description).color(p.text_secondary));
            ui.add_space(16.0);
        }
        if let Some(url) = self.repo_url {
            let link = ui.add(egui::Link::new(
                RichText::new(self.repo_label.unwrap_or(url)).color(p.accent),
            ));
            if link.clicked() {
                action = Some(AboutAction::OpenUrl(url.to_string()));
            }
        }
        action
    }

    /// `.app-icon`: the image, or the glyph on a rounded accent tile.
    fn icon_tile(&self, ui: &mut Ui, p: &Palette, icon: &AboutIcon<'a>) {
        let (rect, resp) = ui.allocate_exact_size(Vec2::splat(ICON_SIZE), Sense::hover());
        let name = self.app_name.to_string();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &name));
        match icon {
            AboutIcon::Glyph(glyph) => {
                ui.painter()
                    .rect_filled(rect, CornerRadius::same(18), p.accent);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    *glyph,
                    icons::font(44.0),
                    p.accent_on,
                );
            }
            AboutIcon::Image(source) => {
                Image::new(source.clone()).paint_at(ui, rect);
            }
        }
    }
}

/// `.update-btn`: the "Check" button, disabled while busy, with a spinning
/// arrow while checking.
fn check_button(ui: &mut Ui, p: &Palette, status: &UpdateStatus) -> egui::Response {
    let enabled = !status.busy();
    let galley = ui.painter().layout_no_wrap(
        "Check".into(),
        FontId::proportional(12.0),
        Color32::PLACEHOLDER,
    );
    let pad = vec2(10.0, 4.0);
    let size = vec2(galley.size().x + 12.0 + 6.0, galley.size().y) + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, enabled, "Check for Updates")
    });
    let hovered = enabled && resp.hovered();
    let (fill, ink) = if hovered {
        (p.bg_selected, p.accent)
    } else {
        (p.bg_button, p.text_primary)
    };
    let alpha = if enabled { 1.0 } else { 0.5 };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill.gamma_multiply(alpha),
        Stroke::NONE,
        StrokeKind::Inside,
    );
    let icon_center = pos2(rect.left() + pad.x + 6.0, rect.center().y);
    if matches!(status, UpdateStatus::Checking) {
        // The glyph can't rotate as text; draw a spinner arc instead.
        let t = ui.input(|i| i.time) as f32;
        let points: Vec<_> = (0..16)
            .map(|k| {
                let a = t * std::f32::consts::TAU + k as f32 * 0.3;
                icon_center + vec2(a.cos(), a.sin()) * 5.0
            })
            .collect();
        ui.painter().add(egui::Shape::line(
            points,
            Stroke::new(1.5, ink.gamma_multiply(alpha)),
        ));
        ui.ctx().request_repaint();
    } else {
        ui.painter().text(
            icon_center,
            Align2::CENTER_CENTER,
            icons::ARROWS_ROTATE,
            icons::font(12.0),
            ink.gamma_multiply(alpha),
        );
    }
    ui.painter().galley(
        pos2(
            rect.left() + pad.x + 18.0,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        ink.gamma_multiply(alpha),
    );
    resp.on_hover_text("Check for Updates")
}

/// Release notes arrive as HTML; show their text.
fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    fn harness(update: Option<UpdateStatus>) -> Harness<'static, Vec<AboutAction>> {
        Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(1000.0, 800.0))
            .build_ui_state(
                move |ui, actions: &mut Vec<AboutAction>| {
                    let mut modal = AboutModal::new("NoteLiner", "2.0.1")
                        .description("Outliner-style notes.")
                        .repo("https://github.com/NathanLaan/marina", Some("GitHub"))
                        .icon(AboutIcon::Glyph(icons::BOOK));
                    if let Some(u) = &update {
                        modal = modal.update(u);
                    }
                    if let Some(a) = modal.show(ui.ctx()) {
                        actions.push(a);
                    }
                },
                Vec::new(),
            )
    }

    #[test]
    fn renders_with_labels_and_reports_actions() {
        let mut h = harness(Some(UpdateStatus::Available {
            version: "2.1.0".into(),
            notes: Some("<p>Fixes</p>".into()),
        }));
        h.run();
        assert_eq!(
            h.query_all_by_label("NoteLiner").count(),
            2,
            "the name and the icon"
        );
        h.get_by_label("Version 2.1.0 is available.");
        h.get_by_label("Check for Updates").click();
        h.run();
        h.get_by_label("Download").click();
        h.run();
        h.get_by_label("GitHub").click();
        h.run();
        h.get_by_label("OK").click();
        h.run();
        assert_eq!(
            h.state().as_slice(),
            [
                AboutAction::CheckForUpdates,
                AboutAction::Download,
                AboutAction::OpenUrl("https://github.com/NathanLaan/marina".into()),
                AboutAction::Close,
            ]
        );
    }

    #[test]
    fn no_update_ui_without_status() {
        let mut h = harness(None);
        h.run();
        assert!(h.query_by_label("Check for Updates").is_none());
        h.get_by_label("Close").click();
        h.run();
        assert_eq!(h.state().as_slice(), [AboutAction::Close]);
    }

    #[test]
    fn status_lines() {
        assert_eq!(UpdateStatus::Idle.line(), None);
        assert_eq!(
            UpdateStatus::Downloading { percent: 41.6 }.line().unwrap(),
            "Downloading update… 42%"
        );
        assert_eq!(
            UpdateStatus::Unavailable { dev: true }.line().unwrap(),
            "Updates are disabled in development builds."
        );
        assert_eq!(strip_tags("<p>A &amp; B</p>"), "A & B");
    }
}
