//! Dialogs and their form controls: the `.modal-overlay-compact` /
//! `.modal-compact` and `.modal-overlay` / `.modal` styles of `global.css`,
//! plus the inputs, buttons and error text the apps' small forms use (Add
//! Feed, Edit Tag, …), and a confirmation dialog in place of
//! `window.confirm`.
//!
//! Draw a [`Dialog`] every frame while it's open. [`Dialog::show`] returns
//! the body's result and whether the user asked to close it (the X, Escape,
//! or a click on the backdrop).

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin,
    Modal, Rect, Response, RichText, Sense, Stroke, StrokeKind, TextEdit, Ui, Vec2,
};

use crate::settings::{modal_area, modal_frame, modal_header};
use crate::theme::{self, Palette};
use crate::{fonts, toast};

/// `.modal-compact { min-width: 340px; max-width: 500px }`; forms use this width.
pub const COMPACT_WIDTH: f32 = 420.0;
/// `border-radius: 6px` on inputs and buttons.
const CONTROL_RADIUS: CornerRadius = CornerRadius::same(6);
/// `.modal-overlay { padding: 10px 10px 10px 58px }`.
const OVERLAY_PAD: Margin = Margin {
    left: 58,
    right: 10,
    top: 10,
    bottom: 10,
};

/// How a dialog is placed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Size {
    /// Centred and as tall as its contents, `width` wide (`.modal-compact`).
    Compact { width: f32 },
    /// Fills the window below `top_inset` (`.modal`), leaving the toolbar
    /// column visible.
    Full { top_inset: f32 },
}

/// A modal dialog with the accent header.
pub struct Dialog<'a> {
    id: Id,
    title: &'a str,
    size: Size,
}

/// What [`Dialog::show`] returns.
pub struct DialogResponse<R> {
    pub inner: R,
    /// The X, Escape or a backdrop click: the app should close the dialog.
    pub close: bool,
}

impl<'a> Dialog<'a> {
    /// A compact, centred dialog (`.modal-compact`).
    pub fn compact(id: impl std::hash::Hash + std::fmt::Debug, title: &'a str) -> Self {
        Self {
            id: Id::unique(("marina-dialog", id)),
            title,
            size: Size::Compact {
                width: COMPACT_WIDTH,
            },
        }
    }

    /// A dialog filling the window (`.modal`).
    pub fn full(id: impl std::hash::Hash + std::fmt::Debug, title: &'a str) -> Self {
        Self {
            id: Id::unique(("marina-dialog", id)),
            title,
            size: Size::Full { top_inset: 0.0 },
        }
    }

    pub fn size(mut self, size: Size) -> Self {
        self.size = size;
        self
    }

    /// Draws the dialog; `body` fills it below the header, inside the
    /// `.modal-body` padding.
    pub fn show<R>(
        &self,
        ctx: &egui::Context,
        body: impl FnOnce(&mut Ui) -> R,
    ) -> DialogResponse<R> {
        let p = theme::palette(ctx);
        let screen = ctx.content_rect();
        let mut modal = Modal::new(self.id)
            .backdrop_color(p.modal_overlay)
            .frame(modal_frame(&p));
        let fixed = match self.size {
            Size::Compact { .. } => None,
            Size::Full { top_inset } => {
                let rect = Rect::from_min_max(
                    pos2(
                        screen.left() + OVERLAY_PAD.left as f32,
                        screen.top() + top_inset + OVERLAY_PAD.top as f32,
                    ),
                    pos2(
                        screen.right() - OVERLAY_PAD.right as f32,
                        screen.bottom() - OVERLAY_PAD.bottom as f32,
                    ),
                );
                modal = modal.area(modal_area(self.id, rect.min));
                Some(rect.size())
            }
        };
        let mut closed = false;
        let response = modal.show(ctx, |ui| {
            match (self.size, fixed) {
                (_, Some(size)) => {
                    ui.set_min_size(size);
                    ui.set_max_size(size);
                }
                (Size::Compact { width }, None) => {
                    let width = width.min(screen.width() - 32.0);
                    ui.set_min_width(width);
                    ui.set_max_width(width);
                }
                _ => {}
            }
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            closed |= modal_header(ui, &p, self.title);
            Frame::new()
                .inner_margin(Margin {
                    left: 24,
                    right: 24,
                    top: 24,
                    bottom: 20,
                })
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                    ui.set_width(ui.available_width());
                    if fixed.is_some() {
                        ui.set_min_height(ui.available_height());
                    }
                    body(ui)
                })
                .inner
        });
        let close = closed || response.should_close();
        DialogResponse {
            inner: response.inner,
            close,
        }
    }
}

/// `.setting-label`: the 11 px upper-case caption above a field.
pub fn field_label(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.label(
        fonts::weight(text.to_uppercase(), fonts::SEMIBOLD)
            .size(11.0)
            .color(p.text_muted)
            .extra_letter_spacing(0.5),
    );
    ui.add_space(2.0);
}

/// A full-width text input (`input { padding: 8px 12px; border-radius: 6px }`).
/// Pressing Enter in it sets `lost_focus()` with Enter down, which
/// [`submitted`] checks.
pub fn text_input(ui: &mut Ui, value: &mut String, hint: &str, enabled: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let edit = TextEdit::singleline(value)
        .hint_text(RichText::new(hint).color(p.text_muted))
        .desired_width(f32::INFINITY)
        .margin(Margin::symmetric(12, 8))
        .background_color(p.input_bg);
    ui.scope(|ui| {
        // TextEdit draws its frame from these: the input border at rest and on
        // hover, the focus colour (`selection.stroke`) while focused.
        let v = &mut ui.visuals_mut().widgets;
        for w in [&mut v.inactive, &mut v.hovered, &mut v.active, &mut v.open] {
            w.bg_stroke = Stroke::new(1.0, p.input_border);
            w.corner_radius = CONTROL_RADIUS;
        }
        ui.visuals_mut().selection.stroke = Stroke::new(1.0, p.input_border_focus);
        ui.add_enabled(enabled, edit)
    })
    .inner
}

/// Whether `resp` (a [`text_input`]) was submitted with Enter this frame.
pub fn submitted(ui: &Ui, resp: &Response) -> bool {
    resp.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
}

/// The look of a dialog button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ButtonKind {
    /// `.btn-primary`: accent outline and text on the selected wash, filled
    /// with the accent on hover.
    Primary,
    /// `.btn-secondary`: the plain button fill.
    Secondary,
    /// `.confirm-danger-btn`: red, for destructive confirmations.
    Danger,
}

/// A dialog button (`.btn { padding: 8px 16px; font-size: 13px }`). A
/// disabled button is drawn at half opacity and never clicks.
pub fn button(ui: &mut Ui, label: &str, kind: ButtonKind, enabled: bool) -> Response {
    icon_button(ui, None, label, kind, enabled)
}

/// A dialog button with a Font Awesome glyph before the label.
pub fn icon_button(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    kind: ButtonKind,
    enabled: bool,
) -> Response {
    let p = theme::palette(ui.ctx());
    let text = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(13.0),
        Color32::PLACEHOLDER,
    );
    let pad = vec2(16.0, 8.0);
    let icon_w = if icon.is_some() { 20.0 } else { 0.0 };
    let size = vec2(text.size().x + icon_w, text.size().y.max(15.0)) + pad * 2.0;
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, enabled, label));
    let hovered = enabled && resp.hovered();
    let (fill, ink, stroke) = colors(&p, kind, hovered);
    let alpha = if enabled { 1.0 } else { 0.5 };
    ui.painter().rect(
        rect,
        CONTROL_RADIUS,
        fill.gamma_multiply(alpha),
        Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha)),
        StrokeKind::Inside,
    );
    let mut x = rect.left() + pad.x;
    if let Some(glyph) = icon {
        ui.painter().text(
            pos2(x + 6.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            crate::icons::font(12.0),
            ink.gamma_multiply(alpha),
        );
        x += icon_w;
    }
    ui.painter().galley(
        pos2(x, rect.center().y - text.size().y / 2.0),
        text,
        ink.gamma_multiply(alpha),
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

fn colors(p: &Palette, kind: ButtonKind, hovered: bool) -> (Color32, Color32, Stroke) {
    match (kind, hovered) {
        (ButtonKind::Primary, false) => (p.bg_selected, p.accent, Stroke::new(1.0, p.accent)),
        (ButtonKind::Primary, true) => (p.accent, p.accent_on, Stroke::new(1.0, p.accent)),
        (ButtonKind::Secondary, false) => (p.bg_button, p.text_primary, Stroke::NONE),
        (ButtonKind::Secondary, true) => (p.bg_button_hover, p.text_primary, Stroke::NONE),
        (ButtonKind::Danger, false) => (toast::DANGER, Color32::WHITE, Stroke::NONE),
        (ButtonKind::Danger, true) => (
            toast::DANGER.linear_multiply(0.85),
            Color32::WHITE,
            Stroke::NONE,
        ),
    }
}

/// `.actions`: buttons right-aligned in a row. `add` adds them right to
/// left, so add the primary button first.
pub fn actions(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.add_space(8.0);
    ui.allocate_ui_with_layout(
        vec2(ui.available_width(), 34.0),
        Layout::right_to_left(Align::Center),
        |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            add(ui);
        },
    );
}

/// `.error`: a message in the danger colour.
pub fn error_text(ui: &mut Ui, message: &str) {
    ui.label(RichText::new(message).size(13.0).color(toast::DANGER));
}

/// Muted help or empty-state text.
pub fn muted_text(ui: &mut Ui, message: &str) {
    let p = theme::palette(ui.ctx());
    ui.label(RichText::new(message).size(13.0).color(p.text_muted));
}

/// What the user chose in a [`confirm`] dialog.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Confirm {
    Yes,
    No,
}

/// A compact confirmation (the Svelte `confirm(…)` and the Sync modal's
/// confirm dialogs): `message`, optional `detail`, then Cancel and a danger
/// button labelled `confirm_label`. Returns `None` while it's still open.
pub fn confirm(
    ctx: &egui::Context,
    id: impl std::hash::Hash + std::fmt::Debug,
    title: &str,
    message: &str,
    detail: Option<&str>,
    confirm_label: &str,
) -> Option<Confirm> {
    let resp = Dialog::compact(id, title).show(ctx, |ui| {
        let p = theme::palette(ui.ctx());
        ui.label(RichText::new(message).color(p.text_primary));
        if let Some(detail) = detail {
            ui.add_space(4.0);
            muted_text(ui, detail);
        }
        ui.add_space(8.0);
        let mut choice = None;
        actions(ui, |ui| {
            if button(ui, confirm_label, ButtonKind::Danger, true).clicked() {
                choice = Some(Confirm::Yes);
            }
            if button(ui, "Cancel", ButtonKind::Secondary, true).clicked() {
                choice = Some(Confirm::No);
            }
        });
        choice
    });
    if resp.close {
        return Some(Confirm::No);
    }
    resp.inner
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    #[derive(Default)]
    struct State {
        name: String,
        submitted: Vec<String>,
        closed: bool,
        answer: Option<Confirm>,
        confirming: bool,
    }

    fn harness(confirming: bool) -> Harness<'static, State> {
        Harness::builder()
            .with_size(vec2(800.0, 600.0))
            .with_context_setup(crate::fonts::install)
            .build_ui_state(
                |ui, s: &mut State| {
                    let ctx = &ui.ctx().clone();
                    if s.confirming {
                        if let Some(a) =
                            confirm(ctx, "c", "Delete Tag", "Delete tag?", Some("x"), "Delete")
                        {
                            s.answer = Some(a);
                            s.confirming = false;
                        }
                        return;
                    }
                    let resp = Dialog::compact("add", "Add Tag").show(ctx, |ui| {
                        field_label(ui, "Tag Name");
                        let input = text_input(ui, &mut s.name, "e.g. Technology", true);
                        let enter = submitted(ui, &input);
                        let mut add = false;
                        actions(ui, |ui| {
                            add = button(ui, "Add", ButtonKind::Primary, !s.name.trim().is_empty())
                                .clicked();
                            button(ui, "Cancel", ButtonKind::Secondary, true);
                        });
                        add || enter
                    });
                    if resp.inner {
                        s.submitted.push(s.name.clone());
                    }
                    s.closed |= resp.close;
                },
                State {
                    confirming,
                    ..Default::default()
                },
            )
    }

    #[test]
    fn form_submits_and_disables() {
        let mut h = harness(false);
        h.run();
        h.get_by_label("Add").click();
        h.run();
        assert!(h.state().submitted.is_empty(), "disabled while empty");
        h.state_mut().name = "News".into();
        h.run();
        h.get_by_label("Add").click();
        h.run();
        assert_eq!(h.state().submitted, ["News"]);
        h.get_by_label("Close").click();
        h.run();
        assert!(h.state().closed);
    }

    #[test]
    fn confirm_answers() {
        let mut h = harness(true);
        h.run();
        h.get_by_label("Delete").click();
        h.run();
        assert_eq!(h.state().answer, Some(Confirm::Yes));
        let mut h = harness(true);
        h.run();
        h.get_by_label("Cancel").click();
        h.run();
        assert_eq!(h.state().answer, Some(Confirm::No));
    }
}
