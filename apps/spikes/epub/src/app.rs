//! The reader: table of contents on the left, the chapter on the right.
//!
//! Scroll mode, with paging keys (Page Down/Space/→ and Page Up/←) that move a screen at a time
//! and cross chapter boundaries. egui has no multi-column or paginated text layout, and a
//! scrolled chapter keeps positions simple (block + offset), so real pagination waits until
//! there's a reason for it.

use std::fs::File;
use std::io::BufReader;
use std::time::{Duration, Instant};

use spike_common::Screenshot;
use spike_richtext::fonts::{self, Face, Installed};
use spike_richtext::render::{self, Action, RichView, Theme};

use crate::book::Book;
use crate::positions::{Position, Positions};

/// Positions are written at most this often while reading (and on exit).
const SAVE_INTERVAL: Duration = Duration::from_secs(2);
/// Paging keeps this much of the previous screen visible.
const PAGE_OVERLAP: f32 = 48.0;

pub struct ReaderApp {
    book: Book<BufReader<File>>,
    chapter: usize,
    view: RichView,
    face: Face,
    fonts: Installed,
    positions: Positions,
    last_saved: Instant,
    /// The scroll area's state last frame: offset, viewport height, content height.
    scroll: (f32, f32, f32),
    /// Jump to the end of the chapter once it's measured (paging back across a boundary).
    to_end: bool,
    pending_offset: Option<f32>,
    screenshot: Option<Screenshot>,
}

impl ReaderApp {
    pub fn new(cc: &eframe::CreationContext<'_>, book: Book<BufReader<File>>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let fonts = fonts::install(&cc.egui_ctx);
        let positions = Positions::load(Positions::default_path());
        let saved = positions.get(&book.id).unwrap_or_default();
        let mut app = Self {
            book,
            chapter: 0,
            view: RichView::default(),
            face: Face::Serif,
            fonts,
            positions,
            last_saved: Instant::now(),
            scroll: (0.0, 0.0, 0.0),
            to_end: false,
            pending_offset: None,
            screenshot: Screenshot::from_env(),
        };
        // Diagnostics: `MARINA_CHAPTER=<n>` opens a chapter instead of the saved position.
        match std::env::var("MARINA_CHAPTER")
            .ok()
            .and_then(|c| c.parse().ok())
        {
            Some(chapter) => app.go(chapter, None),
            None => {
                app.go(saved.chapter, None);
                app.view.restore(render::Position {
                    block: saved.block,
                    offset: saved.offset,
                });
            }
        }
        app
    }

    /// Opens `chapter`, at the block `fragment` anchors (or the top).
    fn go(&mut self, chapter: usize, fragment: Option<&str>) {
        let chapter = chapter.min(self.book.spine.len() - 1);
        self.chapter = chapter;
        self.view.reset();
        self.to_end = false;
        let doc = self.book.chapter(chapter);
        if let Some(block) = fragment.and_then(|f| doc.anchors.get(f)) {
            self.view.restore(render::Position {
                block: *block,
                offset: 0,
            });
        }
    }

    /// [`Self::go`], for the reader's own navigation (saved straight away).
    fn navigate(&mut self, chapter: usize, fragment: Option<&str>) {
        self.go(chapter, fragment);
        self.save_position(true);
    }

    fn save_position(&mut self, now: bool) {
        // Until a restore lands, the view's position is the top, not where the reader is.
        if self.view.restoring() {
            return;
        }
        let p = self.view.position();
        let pos = Position {
            chapter: self.chapter,
            block: p.block,
            offset: p.offset,
        };
        if self.positions.set(&self.book.id, pos)
            && (now || self.last_saved.elapsed() >= SAVE_INTERVAL)
        {
            if let Err(e) = self.positions.save() {
                log::warn!("Couldn't save the reading position: {e:#}");
            }
            self.last_saved = Instant::now();
        }
    }

    fn page(&mut self, down: bool) {
        let (offset, viewport, content) = self.scroll;
        let step = (viewport - PAGE_OVERLAP).max(viewport * 0.5);
        if down {
            if offset + viewport >= content - 1.0 {
                if self.chapter + 1 < self.book.spine.len() {
                    self.navigate(self.chapter + 1, None);
                }
            } else {
                self.pending_offset = Some(offset + step);
            }
        } else if offset <= 0.5 {
            if self.chapter > 0 {
                self.navigate(self.chapter - 1, None);
                self.to_end = true;
            }
        } else {
            self.pending_offset = Some((offset - step).max(0.0));
        }
    }

    fn open_link(&mut self, ctx: &egui::Context, link: &str) {
        if link.contains("://") || link.starts_with("mailto:") {
            ctx.open_url(egui::OpenUrl::new_tab(link));
        } else if let Some(t) = self.book.target(link) {
            self.navigate(t.chapter, t.fragment.as_deref());
        } else {
            log::warn!("Unresolved link {link}");
        }
    }
}

impl eframe::App for ReaderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let (down, up) = ctx.input(|i| {
            let down = i.key_pressed(egui::Key::PageDown)
                || i.key_pressed(egui::Key::Space)
                || i.key_pressed(egui::Key::ArrowRight);
            let up = i.key_pressed(egui::Key::PageUp) || i.key_pressed(egui::Key::ArrowLeft);
            (down, up)
        });
        if down || up {
            self.page(down);
        }

        let mut go_to = None;
        egui::Panel::left("toc")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.heading(&self.book.title);
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for entry in &self.book.toc {
                        let current = self.book.toc_chapter(entry) == Some(self.chapter);
                        ui.horizontal(|ui| {
                            ui.add_space(entry.depth as f32 * 14.0);
                            if ui.selectable_label(current, &entry.label).clicked() {
                                go_to = Some(entry.target.clone());
                            }
                        });
                    }
                });
            });
        if let Some(target) = go_to {
            self.open_link(&ctx, &target);
        }

        let mut nav = None;
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(self.chapter > 0, egui::Button::new("< Previous"))
                    .clicked()
                {
                    nav = Some(self.chapter - 1);
                }
                ui.label(format!(
                    "Section {} of {}",
                    self.chapter + 1,
                    self.book.spine.len()
                ));
                if ui
                    .add_enabled(
                        self.chapter + 1 < self.book.spine.len(),
                        egui::Button::new("Next >"),
                    )
                    .clicked()
                {
                    nav = Some(self.chapter + 1);
                }
                ui.separator();
                ui.selectable_value(&mut self.face, Face::Serif, "Serif");
                ui.selectable_value(&mut self.face, Face::Sans, "Sans");
                ui.separator();
                let p = self.view.position();
                ui.weak(format!(
                    "block {} · offset {} · laid out {}",
                    p.block, p.offset, self.view.laid_out
                ));
            });
        });
        if let Some(chapter) = nav {
            self.navigate(chapter, None);
        }

        let real = match self.face {
            Face::Sans => self.fonts.sans,
            Face::Serif => self.fonts.serif,
        };
        let mut theme = Theme::from_visuals(ui.visuals(), self.face, real);
        theme.size = 18.0;
        let mut actions = Vec::new();
        let chapter = self.chapter;
        egui::CentralPanel::default_margins().show(ui, |ui| {
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("chapter", chapter))
                .auto_shrink(false);
            if let Some(y) = self
                .view
                .take_scroll_target()
                .or(self.pending_offset.take())
            {
                area = area.vertical_scroll_offset(y);
            }
            let doc = self.book.chapter(chapter);
            let book = &mut self.book;
            let view = &mut self.view;
            let out = area.show_viewport(ui, |ui, viewport| {
                view.show(ui, viewport, &doc, &theme, &mut |ctx, src| {
                    let uri = format!("bytes://epub/{src}");
                    if ctx.try_load_bytes(&uri).is_err() {
                        ctx.include_bytes(uri.clone(), book.resource(src)?);
                    }
                    Some(uri)
                })
            });
            actions = out.inner;
            self.scroll = (
                out.state.clamped_offset().y,
                out.inner_rect.height(),
                out.content_size.y,
            );
        });
        if self.to_end && self.scroll.2 > 0.0 {
            self.to_end = false;
            self.pending_offset = Some((self.scroll.2 - self.scroll.1).max(0.0));
        }
        for action in actions {
            let Action::Link(link) = action;
            self.open_link(&ctx, &link);
        }
        self.save_position(false);
        if let Some(s) = &mut self.screenshot {
            s.tick(&ctx);
        }
    }

    fn save(&mut self, _storage: &mut dyn eframe::Storage) {
        self.save_position(true);
    }
}
