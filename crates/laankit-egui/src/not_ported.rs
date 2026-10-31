//! The "not in the native build yet" panel, after ArtCraft's coming-soon
//! page (`artcraft-native/src/pages/other.rs`).
//!
//! While an app is being ported, a feature that hasn't arrived yet still
//! gets its place in the UI: this panel names it and points to the build
//! that has it (Tauri or Electron), optionally with a link, so a
//! native build can ship before it has everything.

use egui::{vec2, Align2, FontId, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder};

use crate::icons;
use crate::theme;

/// The panel's content. Show it with [`NotPorted::show`].
#[derive(Clone, Debug)]
pub struct NotPorted<'a> {
    /// What's missing, e.g. "Slides" or "DOCX import".
    pub feature: &'a str,
    /// Where it works today, e.g. "the Tauri or Electron build".
    pub build: &'a str,
    /// An optional extra sentence (a workaround, a timeline).
    pub note: Option<&'a str>,
    /// An optional (label, url) button, e.g. the release page.
    pub link: Option<(&'a str, &'a str)>,
}

impl<'a> NotPorted<'a> {
    pub fn new(feature: &'a str) -> Self {
        Self {
            feature,
            build: "the Tauri or Electron build",
            note: None,
            link: None,
        }
    }

    pub fn build(mut self, build: &'a str) -> Self {
        self.build = build;
        self
    }

    pub fn note(mut self, note: &'a str) -> Self {
        self.note = Some(note);
        self
    }

    pub fn link(mut self, label: &'a str, url: &'a str) -> Self {
        self.link = Some((label, url));
        self
    }

    /// The headline, e.g. "Slides isn't in the native build yet".
    pub fn headline(&self) -> String {
        format!("{} isn't in the native build yet", self.feature)
    }

    /// Draws a bordered card centred in the available space. Opens the
    /// link (if any) when its button is clicked.
    pub fn show(&self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let area = ui.available_rect_before_wrap();
        let width = (area.width() - 48.0).clamp(0.0, 560.0);
        let height: f32 = if self.link.is_some() { 220.0 } else { 170.0 }
            + if self.note.is_some() { 24.0 } else { 0.0 };
        let card = Rect::from_center_size(area.center(), vec2(width, height.min(area.height())));
        ui.painter().rect(
            card,
            theme::RADIUS_LARGE,
            p.bg_surface,
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
        let mut child = ui.new_child(UiBuilder::new().max_rect(card.shrink2(vec2(32.0, 28.0))));
        child.spacing_mut().item_spacing.y = 8.0;
        child.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
            ui.painter().text(
                r.center(),
                Align2::CENTER_CENTER,
                icons::PLUG,
                icons::font(20.0),
                p.accent,
            );
            ui.label(
                RichText::new(self.headline())
                    .size(18.0)
                    .color(p.text_primary),
            );
        });
        child.add_space(4.0);
        child.label(
            RichText::new(format!(
                "You can use {} in {} in the meantime.",
                self.feature, self.build
            ))
            .size(theme::BODY_SIZE)
            .color(p.text_secondary),
        );
        if let Some(note) = self.note {
            child.label(RichText::new(note).size(13.0).color(p.text_muted));
        }
        if let Some((label, url)) = self.link {
            child.add_space(12.0);
            let galley = child.painter().layout_no_wrap(
                label.to_string(),
                FontId::proportional(theme::BODY_SIZE),
                p.accent_on,
            );
            let pad = vec2(14.0, 8.0);
            let (r, resp) = child
                .allocate_exact_size(galley.size() + pad * 2.0 + vec2(20.0, 0.0), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let fill = if resp.hovered() {
                p.accent_hover
            } else {
                p.accent
            };
            child.painter().rect_filled(r, theme::RADIUS, fill);
            child.painter().text(
                r.left_center() + vec2(pad.x + 6.0, 0.0),
                Align2::CENTER_CENTER,
                icons::EXTERNAL_LINK_ALT,
                icons::font(12.0),
                p.accent_on,
            );
            child
                .painter()
                .galley(r.min + pad + vec2(20.0, 0.0), galley, p.accent_on);
            if resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .clicked()
            {
                child.ctx().open_url(egui::OpenUrl::new_tab(url));
            }
        }
        ui.allocate_rect(area, Sense::hover());
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    #[test]
    fn renders_headline_and_link() {
        let mut h = Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(800.0, 500.0))
            .build_ui(|ui| {
                NotPorted::new("Slides")
                    .note("Presenting is planned for Phase 4.")
                    .link(
                        "Open the release page",
                        "https://github.com/NathanLaan/laankit/releases",
                    )
                    .show(ui);
            });
        h.run();
        h.get_by_label("Slides isn't in the native build yet");
        h.get_by_label("You can use Slides in the Tauri or Electron build in the meantime.");
        h.get_by_label("Open the release page").click();
        h.step();
        let opened =
            h.output().platform_output.commands.iter().any(
                |c| matches!(c, egui::OutputCommand::OpenUrl(u) if u.url.ends_with("/releases")),
            );
        assert!(opened);
    }
}
