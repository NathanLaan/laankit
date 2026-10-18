//! Presenting: the deck full screen, one slide at a time. The Svelte build
//! has no presenter yet (its plan calls for a detached window later); this is
//! the native take. It covers the window with a foreground layer, switches
//! the window to full screen while it runs (restoring it after, unless it
//! was already full screen), and consumes its keys so the host's shortcuts
//! don't also fire.
//!
//! | Key                                               | Does                    |
//! |---------------------------------------------------|-------------------------|
//! | →, ↓, Space, Page Down, Enter, N, or a click      | Next slide              |
//! | ←, ↑, Page Up, Backspace, P                       | Previous slide          |
//! | Home / End                                        | First / last slide      |
//! | S                                                 | Show or hide the notes  |
//! | Escape                                            | Stop presenting         |

use std::collections::HashMap;

use egui::{
    pos2, vec2, Align2, Color32, FontId, Id, Key, Modifiers, Order, Rect, RichText, Sense,
    ViewportCommand,
};

use crate::deck::Deck;
use crate::render::{ImageSource, SlideView};

const NEXT: [Key; 6] = [
    Key::ArrowRight,
    Key::ArrowDown,
    Key::Space,
    Key::PageDown,
    Key::Enter,
    Key::N,
];
const PREVIOUS: [Key; 5] = [
    Key::ArrowLeft,
    Key::ArrowUp,
    Key::PageUp,
    Key::Backspace,
    Key::P,
];

/// What the host should do after a frame of presenting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenterAction {
    /// The slide shown changed (1-based); e.g. to move the editor caret.
    Changed(usize),
    /// Stop presenting: drop the [`Presenter`].
    Exit,
}

/// A running presentation. Keep it while presenting and call [`Presenter::show`]
/// every frame, after the rest of the UI.
#[derive(Clone, Debug)]
pub struct Presenter {
    /// 1-based.
    index: usize,
    notes: bool,
    started: bool,
    was_fullscreen: bool,
}

impl Presenter {
    /// Starts at slide `start` (1-based; clamped to the deck).
    pub fn new(start: usize) -> Self {
        Self {
            index: start.max(1),
            notes: false,
            started: false,
            was_fullscreen: false,
        }
    }

    /// The slide on screen (1-based).
    pub fn index(&self) -> usize {
        self.index
    }

    /// Whether the speaker notes are showing.
    pub fn notes_visible(&self) -> bool {
        self.notes
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        deck: &Deck,
        vars: &HashMap<String, String>,
        images: &dyn ImageSource,
    ) -> Vec<PresenterAction> {
        let mut out = Vec::new();
        if !self.started {
            self.started = true;
            self.was_fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            if !self.was_fullscreen {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(true));
            }
        }
        let total = deck.slides.len();
        let before = self.index;
        self.index = self.index.clamp(1, total.max(1));

        let (next, previous, first, last, notes, exit) = ctx.input_mut(|i| {
            // Consume every listed key pressed this frame, not just the first.
            let mut take = |keys: &[Key]| {
                keys.iter()
                    .filter(|k| i.consume_key(Modifiers::NONE, **k))
                    .count()
                    > 0
            };
            (
                take(&NEXT),
                take(&PREVIOUS),
                take(&[Key::Home]),
                take(&[Key::End]),
                take(&[Key::S]),
                take(&[Key::Escape]),
            )
        });

        let screen = ctx.content_rect();
        let clicked = egui::Area::new(Id::unique("noteliner-presenter"))
            .order(Order::Foreground)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
                ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
                match deck.slide(self.index) {
                    Some(slide) => {
                        let view = SlideView::new(slide, &deck.config)
                            .total(total)
                            .vars(vars)
                            .images(images);
                        let notes_h = if self.notes && !slide.notes.is_empty() {
                            rect.height() * 0.22
                        } else {
                            0.0
                        };
                        let area = Rect::from_min_max(
                            rect.min,
                            pos2(rect.right(), rect.bottom() - notes_h),
                        );
                        let width = area.width().min(area.height() / view.aspect());
                        let slide_rect = Rect::from_center_size(
                            area.center(),
                            vec2(width, width * view.aspect()),
                        );
                        view.paint(ui, slide_rect);
                        if notes_h > 0.0 {
                            notes_panel(
                                ui,
                                Rect::from_min_max(pos2(rect.left(), area.bottom()), rect.max),
                                &slide.notes,
                            );
                        }
                    }
                    None => {
                        ui.painter().text(
                            rect.center(),
                            Align2::CENTER_CENTER,
                            "No slides to present. Press Escape.",
                            FontId::proportional(24.0),
                            Color32::from_gray(160),
                        );
                    }
                }
                // The counter grows with the screen, so it stays legible on a projector.
                let counter = (rect.height() / 50.0).clamp(14.0, 40.0);
                ui.painter().text(
                    rect.right_bottom() - vec2(counter * 1.2, counter),
                    Align2::RIGHT_BOTTOM,
                    format!("{} / {}", self.index.min(total), total),
                    FontId::proportional(counter),
                    Color32::from_white_alpha(110),
                );
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        format!("Slide {} of {total}", self.index.min(total)),
                    )
                });
                resp.clicked()
            })
            .inner;

        if exit {
            if !self.was_fullscreen {
                ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
            }
            out.push(PresenterAction::Exit);
            return out;
        }
        if notes {
            self.notes = !self.notes;
        }
        if total > 0 {
            if next || clicked {
                self.index = (self.index + 1).min(total);
            }
            if previous {
                self.index = self.index.saturating_sub(1).max(1);
            }
            if first {
                self.index = 1;
            }
            if last {
                self.index = total;
            }
        }
        if self.index != before {
            log::debug!(
                "presenter: {before} → {} (next {next}, click {clicked}, prev {previous})",
                self.index
            );
            out.push(PresenterAction::Changed(self.index));
        }
        out
    }
}

fn notes_panel(ui: &mut egui::Ui, rect: Rect, notes: &str) {
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(24));
    let inner = rect.shrink2(vec2(32.0, 16.0));
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.label(
            RichText::new(notes)
                .size(18.0)
                .color(Color32::from_gray(200)),
        );
    });
}
