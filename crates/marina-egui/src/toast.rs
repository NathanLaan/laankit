//! Toasts: brief notices stacked at the bottom centre of the window, in the
//! style of ThreadLiner's `.error-toast` (12 px corners, 13 px text, a
//! dismiss button, click anywhere to dismiss). Errors are red; info and
//! success notices use the theme's surface colours.
//!
//! Adapted from ArtCraft's `artcraft-native/src/ui/toast.rs` (MIT OR
//! Apache-2.0, see `NOTICE`).

use std::time::Duration;

use egui::{pos2, Align, Align2, Color32, Frame, Id, Layout, Margin, Order, Sense, Stroke};

use crate::{icons, theme};

const DEFAULT_DURATION: Duration = Duration::from_secs(4);
/// Errors stay up longer: they usually need reading.
const ERROR_DURATION: Duration = Duration::from_secs(8);
/// `max-width: 500px`.
const MAX_WIDTH: f32 = 500.0;
/// `bottom: 20px`.
const BOTTOM_OFFSET: f32 = 20.0;
const GAP: f32 = 8.0;
const TEXT_SIZE: f32 = 13.0;
/// The error background (`--danger`).
pub const DANGER: Color32 = Color32::from_rgb(0xd9, 0x3f, 0x3f);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

#[derive(Debug)]
struct Toast {
    id: u64,
    kind: ToastKind,
    message: String,
    expires_at: f64,
}

/// The app's toast queue. Push with [`Toasts::info`], [`Toasts::success`] or
/// [`Toasts::error`] from anywhere; call [`Toasts::show`] once per frame.
#[derive(Default)]
pub struct Toasts {
    next_id: u64,
    items: Vec<Toast>,
    pending: Vec<(ToastKind, String)>,
}

impl Toasts {
    pub fn info(&mut self, message: impl Into<String>) {
        self.pending.push((ToastKind::Info, message.into()));
    }

    pub fn success(&mut self, message: impl Into<String>) {
        self.pending.push((ToastKind::Success, message.into()));
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.pending.push((ToastKind::Error, message.into()));
    }

    /// The messages showing now (oldest first), for tests and diagnostics.
    pub fn messages(&self) -> Vec<(ToastKind, &str)> {
        self.items
            .iter()
            .map(|t| (t.kind, t.message.as_str()))
            .collect()
    }

    /// Draws the stack over everything else and drops expired toasts.
    pub fn show(&mut self, ctx: &egui::Context) {
        let now = ctx.input(|i| i.time);
        self.update(now);
        if self.items.is_empty() {
            return;
        }
        let p = theme::palette(ctx);
        let screen = ctx.content_rect();
        let mut y = screen.bottom() - BOTTOM_OFFSET;
        let mut dismissed = None;
        // Newest at the bottom, older ones stacked above it.
        for toast in self.items.iter().rev() {
            let (fill, ink, stroke, glyph) = match toast.kind {
                ToastKind::Error => (
                    DANGER,
                    Color32::WHITE,
                    Stroke::NONE,
                    icons::EXCLAMATION_CIRCLE,
                ),
                ToastKind::Success => (
                    p.bg_surface,
                    p.text_primary,
                    Stroke::new(1.0, p.border),
                    icons::CIRCLE_CHECK,
                ),
                ToastKind::Info => (
                    p.bg_surface,
                    p.text_primary,
                    Stroke::new(1.0, p.border),
                    icons::CIRCLE_INFO,
                ),
            };
            let icon_ink = if toast.kind == ToastKind::Error {
                ink
            } else {
                p.accent
            };
            let area = egui::Area::new(Id::unique(("marina-toast", toast.id)))
                .order(Order::Tooltip)
                .pivot(Align2::CENTER_BOTTOM)
                .fixed_pos(pos2(screen.center().x, y))
                .interactable(true);
            let inner = area.show(ctx, |ui| {
                let frame = Frame::NONE
                    .fill(fill)
                    .stroke(stroke)
                    .corner_radius(theme::RADIUS_LARGE)
                    .inner_margin(Margin::symmetric(18, 12))
                    .shadow(egui::Shadow {
                        offset: [0, 8],
                        blur: 24,
                        spread: 0,
                        color: Color32::from_black_alpha(64),
                    });
                let shown = frame.show(ui, |ui| {
                    ui.set_max_width(MAX_WIDTH - 36.0);
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        ui.label(icons::icon_sized(glyph, TEXT_SIZE).color(icon_ink));
                        ui.add(
                            egui::Label::new(
                                egui::RichText::new(&toast.message)
                                    .size(TEXT_SIZE)
                                    .color(ink),
                            )
                            .wrap()
                            .selectable(false),
                        );
                        let close = ui.add(
                            egui::Button::new(
                                icons::icon_sized(icons::TIMES, TEXT_SIZE)
                                    .color(ink.gamma_multiply(0.75)),
                            )
                            .frame(false),
                        );
                        close.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, "Dismiss")
                        });
                        close.clicked()
                    })
                    .inner
                });
                // Click anywhere on the toast to dismiss it, as in ThreadLiner.
                let whole = ui.interact(
                    shown.response.rect,
                    Id::unique(("marina-toast-body", toast.id)),
                    Sense::click(),
                );
                shown.inner || whole.clicked()
            });
            if inner.inner {
                dismissed = Some(toast.id);
            }
            y -= inner.response.rect.height() + GAP;
        }
        if let Some(id) = dismissed {
            self.items.retain(|t| t.id != id);
        }
        // Wake up to expire the oldest toast on time.
        if let Some(next) = self.items.iter().map(|t| t.expires_at).reduce(f64::min) {
            ctx.request_repaint_after(Duration::from_secs_f64((next - now).max(0.0)));
        }
    }

    /// Takes pending toasts and drops expired ones, at time `now` (seconds).
    fn update(&mut self, now: f64) {
        for (kind, message) in self.pending.drain(..) {
            // The same message twice in a row (e.g. a held key) shows once, with a fresh timer.
            self.items
                .retain(|t| !(t.kind == kind && t.message == message));
            self.next_id += 1;
            let duration = if kind == ToastKind::Error {
                ERROR_DURATION
            } else {
                DEFAULT_DURATION
            };
            self.items.push(Toast {
                id: self.next_id,
                kind,
                message,
                expires_at: now + duration.as_secs_f64(),
            });
        }
        self.items.retain(|t| t.expires_at > now);
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    #[test]
    fn duplicates_collapse_and_toasts_expire() {
        let mut t = Toasts::default();
        t.info("Saved");
        t.info("Saved");
        t.error("Sync failed");
        t.update(0.0);
        assert_eq!(
            t.messages(),
            [
                (ToastKind::Info, "Saved"),
                (ToastKind::Error, "Sync failed")
            ]
        );
        t.update(5.0);
        assert_eq!(
            t.messages(),
            [(ToastKind::Error, "Sync failed")],
            "errors outlast info"
        );
        t.update(9.0);
        assert!(t.messages().is_empty());
    }

    #[test]
    fn dismiss_button_removes_the_toast() {
        let mut toasts = Toasts::default();
        toasts.error("Couldn't reach the server");
        let mut h = Harness::builder()
            .with_size(egui::vec2(800.0, 400.0))
            .with_context_setup(crate::fonts::install)
            .build_ui_state(|ui, t: &mut Toasts| t.show(ui.ctx()), toasts);
        h.run();
        assert_eq!(h.state().messages().len(), 1);
        h.get_by_label("Dismiss").click();
        h.run();
        assert!(h.state().messages().is_empty());
    }
}
