//! Presenting: the deck full screen, one slide at a time, optionally with a
//! speaker view in a second window. The Svelte build has no presenter (its
//! plan called for a detached window later); this is the native take.
//!
//! **Audience view.** A foreground layer over the whole window, which goes
//! full screen while presenting (and is restored after, unless it already
//! was). Its keys are consumed so the host's shortcuts don't also fire.
//!
//! **Speaker view** ([`Presenter::with_speaker_view`]). A second native
//! window (an egui *immediate* viewport) with the current slide, the next
//! slide, the current slide's notes (rendered Markdown), an elapsed timer and
//! the clock. Both windows take the same keys and show the same slide: there
//! is one [`Presenter`], and the speaker window is drawn from it in the same
//! frame. An immediate viewport, rather than a deferred one, keeps it that
//! way: a deferred viewport runs on its own schedule and would need the deck,
//! the slide index and the timer shared behind locks.
//!
//! *Placement.* egui can't list monitors or say which one a window is on, so
//! the audience window goes full screen where it is and the speaker window
//! opens beside it as a normal window. To put each on the right display, drag
//! the speaker window to the presenter's monitor, or press **X** (or "Swap
//! displays") to move the full-screen audience window to the monitor the
//! speaker window is on and the speaker window to where the audience window
//! was. Exiting restores the main window's place and full-screen state, and
//! closes the speaker window. Closing the speaker window keeps presenting.
//!
//! | Key                                               | Does                    |
//! |---------------------------------------------------|-------------------------|
//! | →, ↓, Space, Page Down, Enter, N, or a click      | Next slide              |
//! | ←, ↑, Page Up, Backspace, P                       | Previous slide          |
//! | Home / End                                        | First / last slide      |
//! | S                                                 | Show or hide the notes on the audience view |
//! | T / R                                             | Pause or resume / reset the timer (speaker view) |
//! | X                                                 | Swap displays (speaker view) |
//! | Escape                                            | Stop presenting         |

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{
    pos2, vec2, Align2, Color32, FontId, Id, Key, Modifiers, Order, Rect, RichText, Sense, Ui,
    ViewportBuilder, ViewportClass, ViewportCommand, ViewportId,
};

use crate::deck::{Deck, Slide};
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
/// The speaker window's size when it opens.
const SPEAKER_SIZE: [f32; 2] = [1000.0, 640.0];
/// At most this many viewport commands are kept in [`Presenter::commands_sent`].
const COMMAND_LOG: usize = 64;

/// What the host should do after a frame of presenting.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresenterAction {
    /// The slide shown changed (1-based); e.g. to move the editor caret.
    Changed(usize),
    /// Stop presenting: drop the [`Presenter`].
    Exit,
}

/// A navigation step, from either window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    Next,
    Previous,
    First,
    Last,
    ToggleNotes,
    ToggleTimer,
    ResetTimer,
    SwapDisplays,
    Exit,
}

/// An elapsed-time stopwatch over egui's clock (`InputState::time`, seconds).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Timer {
    /// Seconds counted before the current run.
    banked: f64,
    /// When the current run started, if running.
    since: Option<f64>,
}

impl Timer {
    /// A timer running from `now`.
    pub fn started(now: f64) -> Self {
        Self {
            banked: 0.0,
            since: Some(now),
        }
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    pub fn elapsed(&self, now: f64) -> f64 {
        self.banked + self.since.map_or(0.0, |s| (now - s).max(0.0))
    }

    /// Pauses a running timer, resumes a paused one.
    pub fn toggle(&mut self, now: f64) {
        match self.since.take() {
            Some(s) => self.banked += (now - s).max(0.0),
            None => self.since = Some(now),
        }
    }

    /// Back to zero, keeping whether it runs.
    pub fn reset(&mut self, now: f64) {
        self.banked = 0.0;
        if self.since.is_some() {
            self.since = Some(now);
        }
    }

    /// `M:SS`, or `H:MM:SS` from an hour.
    pub fn format(&self, now: f64) -> String {
        let t = self.elapsed(now).floor() as u64;
        let (h, m, s) = (t / 3600, (t / 60) % 60, t % 60);
        if h > 0 {
            format!("{h}:{m:02}:{s:02}")
        } else {
            format!("{m}:{s:02}")
        }
    }
}

/// The speaker view's state.
#[derive(Clone)]
struct Speaker {
    open: bool,
    timer: Option<Timer>,
    /// Whether the displays were swapped (so exiting moves the main window back).
    swapped: bool,
    /// The speaker window's outer rect, as last seen.
    outer: Option<Rect>,
    /// The notes' rich-text view (scroll position and layout cache).
    notes: Arc<Mutex<laankit_richtext::RichView>>,
}

impl std::fmt::Debug for Speaker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Speaker")
            .field("open", &self.open)
            .field("timer", &self.timer)
            .field("swapped", &self.swapped)
            .field("outer", &self.outer)
            .finish_non_exhaustive()
    }
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
    /// The main window's outer rect before presenting (restored after a swap).
    root_outer: Option<Rect>,
    speaker: Option<Speaker>,
    sent: Vec<(ViewportId, ViewportCommand)>,
}

impl Presenter {
    /// Starts at slide `start` (1-based; clamped to the deck).
    pub fn new(start: usize) -> Self {
        Self {
            index: start.max(1),
            notes: false,
            started: false,
            was_fullscreen: false,
            root_outer: None,
            speaker: None,
            sent: Vec::new(),
        }
    }

    /// Also shows the speaker view in a second window.
    pub fn with_speaker_view(mut self, on: bool) -> Self {
        self.speaker = on.then(|| Speaker {
            open: true,
            timer: None,
            swapped: false,
            outer: None,
            notes: Arc::default(),
        });
        self
    }

    /// The slide on screen (1-based).
    pub fn index(&self) -> usize {
        self.index
    }

    /// Whether the speaker notes are showing on the audience view.
    pub fn notes_visible(&self) -> bool {
        self.notes
    }

    /// Whether the speaker window is open.
    pub fn speaker_view_open(&self) -> bool {
        self.speaker.as_ref().is_some_and(|s| s.open)
    }

    /// The speaker timer, once presenting has started.
    pub fn timer(&self) -> Option<Timer> {
        self.speaker.as_ref().and_then(|s| s.timer)
    }

    /// The viewport commands this presenter sent, oldest first (the last
    /// [`COMMAND_LOG`]): for tests and logs.
    pub fn commands_sent(&self) -> &[(ViewportId, ViewportCommand)] {
        &self.sent
    }

    /// The speaker window's viewport id.
    pub fn speaker_viewport_id() -> ViewportId {
        ViewportId::from_hash_of("noteliner-speaker-view")
    }

    pub fn show(
        &mut self,
        ctx: &egui::Context,
        deck: &Deck,
        vars: &HashMap<String, String>,
        images: &dyn ImageSource,
    ) -> Vec<PresenterAction> {
        let mut out = Vec::new();
        let now = ctx.input(|i| i.time);
        if !self.started {
            self.started = true;
            self.was_fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
            self.root_outer = ctx.input(|i| i.viewport().outer_rect);
            if !self.was_fullscreen {
                self.send(ctx, ViewportId::ROOT, ViewportCommand::Fullscreen(true));
            }
            if let Some(s) = &mut self.speaker {
                s.timer = Some(Timer::started(now));
            }
        }
        let total = deck.slides.len();
        let before = self.index;
        self.index = self.index.clamp(1, total.max(1));

        let mut navs = read_keys(ctx, false);
        if audience(ctx, deck, vars, images, self.index, self.notes) {
            navs.push(Nav::Next);
        }
        if self.speaker_view_open() {
            navs.extend(self.speaker_window(ctx, deck, vars, images, now));
        }

        for nav in navs {
            if nav == Nav::Exit {
                self.exit(ctx);
                out.push(PresenterAction::Exit);
                return out;
            }
            self.apply(ctx, nav, total, now);
        }
        if self.index != before {
            out.push(PresenterAction::Changed(self.index));
        }
        if self
            .speaker
            .as_ref()
            .and_then(|s| s.timer)
            .is_some_and(|t| t.running())
        {
            // The timer and clock tick.
            ctx.request_repaint_after(std::time::Duration::from_millis(500));
        }
        out
    }

    /// Applies one navigation step (pure apart from swap's viewport commands).
    pub fn apply(&mut self, ctx: &egui::Context, nav: Nav, total: usize, now: f64) {
        match nav {
            Nav::Next if total > 0 => self.index = (self.index + 1).min(total),
            Nav::Previous if total > 0 => self.index = self.index.saturating_sub(1).max(1),
            Nav::First if total > 0 => self.index = 1,
            Nav::Last if total > 0 => self.index = total,
            Nav::ToggleNotes => self.notes = !self.notes,
            Nav::ToggleTimer => {
                if let Some(t) = self.speaker.as_mut().and_then(|s| s.timer.as_mut()) {
                    t.toggle(now);
                }
            }
            Nav::ResetTimer => {
                if let Some(t) = self.speaker.as_mut().and_then(|s| s.timer.as_mut()) {
                    t.reset(now);
                }
            }
            Nav::SwapDisplays => self.swap_displays(ctx),
            Nav::Exit => self.exit(ctx),
            _ => {}
        }
    }

    /// Moves the full-screen audience window to the speaker window's display
    /// and the speaker window to where the audience window was.
    fn swap_displays(&mut self, ctx: &egui::Context) {
        let root = ctx.input(|i| i.viewport().outer_rect);
        let Some(speaker) = self.speaker.as_mut() else {
            return;
        };
        let (Some(root), Some(other)) = (root, speaker.outer) else {
            return; // Positions unknown (e.g. Wayland): nothing to swap to.
        };
        speaker.swapped = !speaker.swapped;
        let id = Self::speaker_viewport_id();
        self.send(ctx, ViewportId::ROOT, ViewportCommand::Fullscreen(false));
        self.send(
            ctx,
            ViewportId::ROOT,
            ViewportCommand::OuterPosition(other.min),
        );
        self.send(ctx, ViewportId::ROOT, ViewportCommand::Fullscreen(true));
        self.send(ctx, id, ViewportCommand::OuterPosition(root.min));
    }

    /// Stops presenting: the main window back to its place and state. The
    /// speaker window closes because it's no longer shown.
    fn exit(&mut self, ctx: &egui::Context) {
        if !self.was_fullscreen {
            self.send(ctx, ViewportId::ROOT, ViewportCommand::Fullscreen(false));
        }
        let swapped = self.speaker.as_ref().is_some_and(|s| s.swapped);
        if let (true, Some(r)) = (swapped, self.root_outer) {
            self.send(ctx, ViewportId::ROOT, ViewportCommand::OuterPosition(r.min));
        }
        if let Some(s) = &mut self.speaker {
            s.open = false;
        }
    }

    fn send(&mut self, ctx: &egui::Context, id: ViewportId, cmd: ViewportCommand) {
        ctx.send_viewport_cmd_to(id, cmd.clone());
        self.sent.push((id, cmd));
        if self.sent.len() > COMMAND_LOG {
            self.sent.remove(0);
        }
    }

    /// The speaker window for this frame; returns the steps taken in it.
    fn speaker_window(
        &mut self,
        ctx: &egui::Context,
        deck: &Deck,
        vars: &HashMap<String, String>,
        images: &dyn ImageSource,
        now: f64,
    ) -> Vec<Nav> {
        let id = Self::speaker_viewport_id();
        let builder = ViewportBuilder::default()
            .with_title("Speaker View")
            .with_inner_size(SPEAKER_SIZE)
            .with_min_inner_size([480.0, 320.0]);
        let index = self.index;
        let timer = self.timer();
        let notes = self
            .speaker
            .as_ref()
            .map(|s| s.notes.clone())
            .unwrap_or_default();
        let draw = |ui: &mut Ui| {
            let mut navs = read_keys(ui.ctx(), true);
            let mut notes = notes.lock().unwrap_or_else(|e| e.into_inner());
            speaker_ui(
                ui, deck, vars, images, index, timer, now, &mut notes, &mut navs,
            );
            navs
        };
        let (navs, outer, closed) = if ctx.embed_viewports() {
            // No second window (tests, or a backend without multiple viewports): the speaker
            // view as a panel above the audience view, bottom right.
            let screen = ctx.content_rect();
            let size = egui::Vec2::from(SPEAKER_SIZE).min(screen.size() * 0.9);
            let pos = screen.right_bottom() - size - vec2(16.0, 16.0);
            let navs = egui::Area::new(Id::unique("noteliner-speaker-embedded"))
                .order(Order::Tooltip)
                .fixed_pos(pos)
                .show(ctx, |ui| {
                    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
                    ui.scope_builder(egui::UiBuilder::new().max_rect(rect), draw)
                        .inner
                })
                .inner;
            (navs, None, false)
        } else {
            ctx.show_viewport_immediate(id, builder, |ui, class| {
                let navs = draw(ui);
                let (outer, closed) = if class == ViewportClass::Immediate {
                    ui.input(|i| (i.viewport().outer_rect, i.viewport().close_requested()))
                } else {
                    (None, false)
                };
                (navs, outer, closed)
            })
        };
        if let Some(s) = &mut self.speaker {
            if outer.is_some() {
                s.outer = outer;
            }
            if closed {
                // Closing the speaker window keeps presenting.
                s.open = false;
            }
        }
        navs
    }
}

/// The steps pressed this frame in the current viewport (keys consumed).
/// `speaker` adds the speaker view's own keys (timer, swap).
fn read_keys(ctx: &egui::Context, speaker: bool) -> Vec<Nav> {
    ctx.input_mut(|i| {
        let mut out = Vec::new();
        let mut take = |keys: &[Key], nav: Nav| {
            // Consume every listed key pressed this frame, not just the first.
            let n = keys
                .iter()
                .filter(|k| i.consume_key(Modifiers::NONE, **k))
                .count();
            out.extend(std::iter::repeat_n(nav, n.min(1)));
        };
        take(&NEXT, Nav::Next);
        take(&PREVIOUS, Nav::Previous);
        take(&[Key::Home], Nav::First);
        take(&[Key::End], Nav::Last);
        take(&[Key::S], Nav::ToggleNotes);
        if speaker {
            take(&[Key::T], Nav::ToggleTimer);
            take(&[Key::R], Nav::ResetTimer);
            take(&[Key::X], Nav::SwapDisplays);
        }
        take(&[Key::Escape], Nav::Exit);
        out
    })
}

/// The audience view over the whole window; returns whether it was clicked.
fn audience(
    ctx: &egui::Context,
    deck: &Deck,
    vars: &HashMap<String, String>,
    images: &dyn ImageSource,
    index: usize,
    notes: bool,
) -> bool {
    let total = deck.slides.len();
    let screen = ctx.content_rect();
    egui::Area::new(Id::unique("noteliner-presenter"))
        .order(Order::Foreground)
        .fixed_pos(screen.min)
        .show(ctx, |ui| {
            let (rect, resp) = ui.allocate_exact_size(screen.size(), Sense::click());
            ui.painter().rect_filled(rect, 0.0, Color32::BLACK);
            match deck.slide(index) {
                Some(slide) => {
                    let view = SlideView::new(slide, &deck.config)
                        .total(total)
                        .vars(vars)
                        .images(images);
                    let notes_h = if notes && !slide.notes.is_empty() {
                        rect.height() * 0.22
                    } else {
                        0.0
                    };
                    let area =
                        Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - notes_h));
                    view.paint(ui, fit(area, view.aspect()));
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
                format!("{} / {}", index.min(total), total),
                FontId::proportional(counter),
                Color32::from_white_alpha(110),
            );
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Button,
                    true,
                    format!("Slide {} of {total}", index.min(total)),
                )
            });
            resp.clicked()
        })
        .inner
}

/// The largest rect of `aspect` (height / width) centred in `area`.
fn fit(area: Rect, aspect: f32) -> Rect {
    let width = area.width().min(area.height() / aspect);
    Rect::from_center_size(area.center(), vec2(width, width * aspect))
}

/// The speaker view: current and next slide, notes, timer and clock.
#[allow(clippy::too_many_arguments)]
fn speaker_ui(
    ui: &mut Ui,
    deck: &Deck,
    vars: &HashMap<String, String>,
    images: &dyn ImageSource,
    index: usize,
    timer: Option<Timer>,
    now: f64,
    notes: &mut laankit_richtext::RichView,
    navs: &mut Vec<Nav>,
) {
    let total = deck.slides.len();
    let p = laankit_egui::theme::palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, p.bg_base);
    let pad = 16.0;
    let inner = rect.shrink(pad);
    let bar_h = 44.0;

    // Top bar: counter, timer and its buttons, clock, swap, exit.
    let bar = Rect::from_min_size(inner.min, vec2(inner.width(), bar_h));
    ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
        ui.horizontal_centered(|ui| {
            ui.label(
                RichText::new(format!("Slide {} / {total}", index.min(total)))
                    .size(20.0)
                    .color(p.text_primary),
            );
            ui.add_space(24.0);
            if let Some(t) = timer {
                ui.label(RichText::new(t.format(now)).size(26.0).monospace().color(
                    if t.running() {
                        p.text_primary
                    } else {
                        p.text_muted
                    },
                ))
                .on_hover_text("Elapsed time");
                let label = if t.running() {
                    "Pause (T)"
                } else {
                    "Resume (T)"
                };
                if ui.button(label).clicked() {
                    navs.push(Nav::ToggleTimer);
                }
                if ui.button("Reset (R)").clicked() {
                    navs.push(Nav::ResetTimer);
                }
            }
            ui.add_space(24.0);
            ui.label(
                RichText::new(chrono::Local::now().format("%H:%M").to_string())
                    .size(20.0)
                    .color(p.text_secondary),
            )
            .on_hover_text("Clock");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("End (Esc)").clicked() {
                    navs.push(Nav::Exit);
                }
                if ui.button("Swap displays (X)").clicked() {
                    navs.push(Nav::SwapDisplays);
                }
                if ui.button("Next ▶").clicked() {
                    navs.push(Nav::Next);
                }
                if ui.button("◀ Previous").clicked() {
                    navs.push(Nav::Previous);
                }
            });
        });
    });

    // Current slide (left, large), next slide and notes (right).
    let body = Rect::from_min_max(pos2(inner.left(), bar.bottom() + pad), inner.max);
    let split = body.left() + body.width() * 0.6;
    let left = Rect::from_min_max(body.min, pos2(split - pad / 2.0, body.bottom()));
    let right = Rect::from_min_max(pos2(split + pad / 2.0, body.top()), body.max);
    let current = deck.slide(index);
    match current {
        Some(slide) => {
            let view = SlideView::new(slide, &deck.config)
                .total(total)
                .vars(vars)
                .images(images);
            let at = fit(left, view.aspect());
            let r = Rect::from_min_size(left.min, at.size());
            view.paint(ui, r);
            let resp = ui.interact(r, ui.scope_id().with("speaker-current"), Sense::hover());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Image,
                    true,
                    format!("Current slide {}", index.min(total)),
                )
            });
        }
        None => {
            ui.painter().text(
                left.center(),
                Align2::CENTER_CENTER,
                "No slides",
                FontId::proportional(20.0),
                p.text_muted,
            );
        }
    }

    // Next slide: a third of the right column's height at most.
    let caption = 22.0;
    let next_area = Rect::from_min_max(
        pos2(right.left(), right.top() + caption),
        pos2(right.right(), right.top() + caption + right.height() * 0.4),
    );
    ui.painter().text(
        right.left_top(),
        Align2::LEFT_TOP,
        "NEXT",
        FontId::proportional(13.0),
        p.text_muted,
    );
    let next = deck.slide(index + 1);
    let next_bottom = match next {
        Some(slide) => {
            let view = SlideView::new(slide, &deck.config)
                .total(total)
                .vars(vars)
                .images(images);
            let at = fit(next_area, view.aspect());
            let r = Rect::from_min_size(next_area.min, at.size());
            view.paint(ui, r);
            let resp = ui.interact(r, ui.scope_id().with("speaker-next"), Sense::hover());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::Role::Image,
                    true,
                    format!("Next slide {}", slide.index),
                )
            });
            r.bottom()
        }
        None => {
            let r = Rect::from_min_size(next_area.min, vec2(next_area.width(), 28.0));
            ui.put(
                r,
                egui::Label::new(
                    RichText::new("End of presentation")
                        .size(16.0)
                        .color(p.text_muted),
                ),
            );
            r.bottom()
        }
    };

    // Notes for the current slide.
    let notes_rect = Rect::from_min_max(pos2(right.left(), next_bottom + pad), right.max);
    ui.painter().text(
        notes_rect.left_top(),
        Align2::LEFT_TOP,
        "NOTES",
        FontId::proportional(13.0),
        p.text_muted,
    );
    let notes_body = Rect::from_min_max(notes_rect.min + vec2(0.0, caption), notes_rect.max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(notes_body), |ui| {
        notes_view(ui, current, notes);
    });
}

/// The current slide's notes as rich text, or a muted "No notes".
fn notes_view(ui: &mut Ui, slide: Option<&Slide>, view: &mut laankit_richtext::RichView) {
    let p = laankit_egui::theme::palette(ui.ctx());
    let notes = slide.map(|s| s.notes.trim()).unwrap_or_default();
    if notes.is_empty() {
        ui.label(RichText::new("No notes for this slide.").color(p.text_muted));
        return;
    }
    let doc = notes_document(ui, notes);
    let mut style = laankit_richtext::RichStyle::from_ctx(ui.ctx());
    style.size = 18.0;
    let _ = view.show(ui, &doc, &style, &mut laankit_richtext::WebResolver);
}

/// Notes Markdown → a rich-text document (math included), cached per text.
fn notes_document(ui: &Ui, notes: &str) -> std::sync::Arc<laankit_richtext::Document> {
    type Cache =
        egui::cache::FrameCache<std::sync::Arc<laankit_richtext::Document>, NotesToDocument>;
    ui.ctx()
        .memory_mut(|m| m.caches.cache::<Cache>().get(notes).clone())
}

#[derive(Default)]
struct NotesToDocument;

impl egui::cache::ComputerMut<&str, std::sync::Arc<laankit_richtext::Document>>
    for NotesToDocument
{
    fn compute(&mut self, notes: &str) -> std::sync::Arc<laankit_richtext::Document> {
        let blocks = crate::render::markdown_blocks(notes);
        std::sync::Arc::new(laankit_richtext::Document {
            blocks,
            anchors: HashMap::new(),
        })
    }
}

fn notes_panel(ui: &mut Ui, rect: Rect, notes: &str) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timer_pauses_resumes_and_resets() {
        let mut t = Timer::started(10.0);
        assert_eq!(t.elapsed(70.0), 60.0);
        assert_eq!(t.format(70.0), "1:00");
        t.toggle(70.0);
        assert!(!t.running());
        assert_eq!(t.elapsed(1000.0), 60.0, "paused");
        t.toggle(1000.0);
        assert_eq!(t.elapsed(1005.0), 65.0);
        t.reset(1005.0);
        assert_eq!(t.elapsed(1010.0), 5.0);
        assert!(t.running());
        assert_eq!(Timer::started(0.0).format(3725.0), "1:02:05");
    }

    /// Swapping moves the audience window to the speaker window's place (full screen there) and
    /// the speaker window to the audience window's; ending puts the main window back.
    #[test]
    fn swap_displays_and_restore_on_exit() {
        let ctx = egui::Context::default();
        let root = Rect::from_min_size(pos2(0.0, 0.0), vec2(1920.0, 1080.0));
        let speaker_at = Rect::from_min_size(pos2(1920.0, 100.0), vec2(1000.0, 640.0));
        let mut input = egui::RawInput::default();
        input
            .viewports
            .entry(ViewportId::ROOT)
            .or_default()
            .outer_rect = Some(root);
        let mut p = Presenter::new(1).with_speaker_view(true);
        p.root_outer = Some(root);
        p.speaker.as_mut().unwrap().outer = Some(speaker_at);
        ctx.run_ui(input, |ui| {
            p.apply(ui.ctx(), Nav::SwapDisplays, 3, 0.0);
            p.apply(ui.ctx(), Nav::Exit, 3, 0.0);
        })
        .drop_without_applying_deltas();
        let speaker = Presenter::speaker_viewport_id();
        assert_eq!(
            p.commands_sent(),
            [
                (ViewportId::ROOT, ViewportCommand::Fullscreen(false)),
                (
                    ViewportId::ROOT,
                    ViewportCommand::OuterPosition(speaker_at.min)
                ),
                (ViewportId::ROOT, ViewportCommand::Fullscreen(true)),
                (speaker, ViewportCommand::OuterPosition(root.min)),
                // Exit: out of full screen, back to where it was.
                (ViewportId::ROOT, ViewportCommand::Fullscreen(false)),
                (ViewportId::ROOT, ViewportCommand::OuterPosition(root.min)),
            ]
        );
        assert!(!p.speaker_view_open());
    }

    #[test]
    fn swap_needs_known_positions() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(1).with_speaker_view(true);
        p.apply(&ctx, Nav::SwapDisplays, 3, 0.0);
        assert!(
            p.commands_sent().is_empty(),
            "positions unknown (e.g. Wayland): no-op"
        );
    }

    #[test]
    fn steps_clamp_to_the_deck() {
        let ctx = egui::Context::default();
        let mut p = Presenter::new(2).with_speaker_view(true);
        p.apply(&ctx, Nav::Next, 3, 0.0);
        p.apply(&ctx, Nav::Next, 3, 0.0);
        assert_eq!(p.index(), 3);
        p.apply(&ctx, Nav::First, 3, 0.0);
        p.apply(&ctx, Nav::Previous, 3, 0.0);
        assert_eq!(p.index(), 1);
        p.apply(&ctx, Nav::Last, 3, 0.0);
        assert_eq!(p.index(), 3);
        p.apply(&ctx, Nav::ToggleNotes, 3, 0.0);
        assert!(p.notes_visible());
    }
}
