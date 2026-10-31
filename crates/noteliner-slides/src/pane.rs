//! The SLIDES sidebar pane (`SlidesPane.svelte`) and the deck branch of the
//! preview column (`DeckPreview.svelte`).
//!
//! Neither holds the note: both take the current Markdown and deck each
//! frame and return actions. Edits come back as finished [`EditResult`]s
//! (computed through [`crate::edits`]), which the host applies as it does a
//! typed edit: replace the editor text, save, move the caret.

use std::collections::HashMap;

use egui::{
    pos2, vec2, Align2, CornerRadius, FontId, Id, Key, Rect, Response, RichText, ScrollArea, Sense,
    Stroke, StrokeKind, TextEdit, Ui, WidgetInfo,
};
use marina_egui::{fonts, icons, theme};

use crate::deck::{Deck, Layout, Slide};
use crate::edits::{Direction, EditResult, SlideEdit};
use crate::render::{ImageSource, SlideView};

/// `THUMB_WIDTH`: narrow, so a default-width sidebar still fits the title.
const THUMB_WIDTH: f32 = 104.0;
/// The layouts the context menu offers (`table` is inferred only).
const MENU_LAYOUTS: [Layout; 7] = [
    Layout::Auto,
    Layout::Hero,
    Layout::TitleBody,
    Layout::Split,
    Layout::Grid,
    Layout::FullBleed,
    Layout::Quote,
];
/// `.deck-rail`: room for the slide number left of each preview slide.
const RAIL: f32 = 28.0;

/// What the host should do after a frame of the pane or preview.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PaneAction {
    /// Scroll the editor to this 1-based line (the slide's first line).
    Jump { line: usize },
    /// Replace the note with this result and move the caret.
    Edit(EditResult),
    /// "Presentation Settings…" from the context menu.
    OpenSettings,
}

/// The SLIDES pane: one row per slide with a live thumbnail; click to jump,
/// double-click to rename, drag to reorder, right-click for the rest.
#[derive(Default)]
pub struct SlidesPane {
    rename: Option<Rename>,
    /// The slide being dragged (1-based).
    drag: Option<usize>,
}

struct Rename {
    index: usize,
    title: String,
    focus: bool,
}

impl SlidesPane {
    /// Draws the pane. `active` is the slide holding the caret
    /// ([`crate::slide_at_line`]).
    pub fn show(
        &mut self,
        ui: &mut Ui,
        markdown: &str,
        deck: Option<&Deck>,
        active: Option<usize>,
        images: &dyn ImageSource,
    ) -> Vec<PaneAction> {
        let mut out = Vec::new();
        let Some(deck) = deck else {
            empty(ui, "Not a presentation.");
            return out;
        };
        if deck.slides.is_empty() {
            empty(ui, "No slides yet — add a heading or a --- separator.");
            return out;
        }
        let pointer = ui.input(|i| i.pointer.interact_pos());
        let mut drop_target = None;
        let mut rows = Vec::with_capacity(deck.slides.len());
        ui.spacing_mut().item_spacing.y = 0.0;
        for slide in &deck.slides {
            let row = self.row(ui, markdown, deck, slide, active, images, &mut out);
            if self.drag.is_some() && pointer.is_some_and(|p| row.rect.contains(p)) {
                drop_target = Some(slide.index);
            }
            rows.push((slide.index, row.rect));
        }
        // Drop marker and the drop itself.
        if let Some(from) = self.drag {
            if let Some(to) = drop_target.filter(|&to| to != from) {
                if let Some((_, rect)) = rows.iter().find(|(i, _)| *i == to) {
                    let p = theme::palette(ui.ctx());
                    ui.painter().hline(
                        rect.x_range(),
                        rect.top() + 1.0,
                        Stroke::new(2.0, p.accent),
                    );
                }
            }
            if ui.input(|i| i.pointer.any_released()) {
                self.drag = None;
                if let Some(to) = drop_target.filter(|&to| to != from) {
                    out.push(PaneAction::Edit(
                        SlideEdit::Reorder { from, to }.apply(markdown, deck),
                    ));
                }
            }
        }
        if deck.skipped > 0 {
            empty(ui, &format!("{} hidden by notes-only.", deck.skipped));
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn row(
        &mut self,
        ui: &mut Ui,
        markdown: &str,
        deck: &Deck,
        slide: &Slide,
        active: Option<usize>,
        images: &dyn ImageSource,
        out: &mut Vec<PaneAction>,
    ) -> Response {
        let p = theme::palette(ui.ctx());
        let total = deck.slides.len();
        let view = SlideView::new(slide, &deck.config)
            .total(total)
            .images(images);
        let thumb_h = THUMB_WIDTH * view.aspect();
        let height = (thumb_h + 8.0).max(36.0);
        let renaming = self.rename.as_ref().is_some_and(|r| r.index == slide.index);
        let sense = if renaming {
            Sense::click()
        } else {
            Sense::click_and_drag()
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), sense);
        let is_active = active == Some(slide.index);
        let label = match &slide.title {
            Some(t) => format!("Slide {} — {t}", slide.index),
            None => format!("Slide {}", slide.index),
        };
        resp.widget_info(|| WidgetInfo::selected(egui::Role::Button, true, is_active, &label));

        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            if is_active {
                painter.rect_filled(rect, 0.0, p.bg_selected);
                painter.rect_filled(
                    Rect::from_min_size(rect.min, vec2(2.0, rect.height())),
                    0.0,
                    p.accent,
                );
            } else if resp.hovered() {
                painter.rect_filled(rect, 0.0, p.bg_button_hover);
            }
            let num_color = if is_active { p.accent } else { p.text_muted };
            painter.text(
                pos2(rect.left() + 24.0, rect.center().y),
                Align2::RIGHT_CENTER,
                slide.index.to_string(),
                FontId::proportional(10.0),
                num_color,
            );
            let thumb = Rect::from_min_size(
                pos2(rect.left() + 32.0, rect.center().y - thumb_h / 2.0),
                vec2(THUMB_WIDTH, thumb_h),
            );
            view.paint(ui, thumb);
            painter.rect_stroke(
                thumb,
                CornerRadius::same(3),
                Stroke::new(1.0, p.border),
                StrokeKind::Outside,
            );
            let meta = Rect::from_min_max(
                pos2(thumb.right() + 8.0, rect.top() + 4.0),
                pos2(rect.right() - 8.0, rect.bottom() - 4.0),
            );
            if !renaming {
                paint_meta(ui, meta, slide);
            }
            if self.drag == Some(slide.index) {
                painter.rect_filled(rect, 0.0, p.bg_base.gamma_multiply(0.6));
            }
        }

        if renaming {
            self.rename_field(ui, rect, markdown, deck, out);
        } else {
            if resp.double_clicked() {
                self.rename = Some(Rename {
                    index: slide.index,
                    title: slide.title.clone().unwrap_or_default(),
                    focus: true,
                });
            } else if resp.clicked() {
                out.push(PaneAction::Jump {
                    line: slide.source_range.from_line,
                });
            }
            if resp.drag_started() {
                self.drag = Some(slide.index);
            }
        }
        resp.context_menu(|ui| self.menu(ui, markdown, deck, slide, out));
        resp.on_hover_text(label)
    }

    fn rename_field(
        &mut self,
        ui: &mut Ui,
        row: Rect,
        markdown: &str,
        deck: &Deck,
        out: &mut Vec<PaneAction>,
    ) {
        let Some(rename) = &mut self.rename else {
            return;
        };
        let field = Rect::from_min_max(
            pos2(row.left() + 32.0 + THUMB_WIDTH + 8.0, row.center().y - 11.0),
            pos2(row.right() - 8.0, row.center().y + 11.0),
        );
        let id = Id::unique(("noteliner-slides-rename", rename.index));
        let resp = ui.put(
            field,
            TextEdit::singleline(&mut rename.title)
                .id(id)
                .font(FontId::proportional(12.0)),
        );
        let value = rename.title.clone();
        resp.widget_info(|| {
            let mut info = WidgetInfo::text_edit(true, "", value.as_str(), "");
            info.label = Some("Slide title".into());
            info
        });
        if rename.focus {
            resp.request_focus();
            rename.focus = false;
            return;
        }
        let escape = ui.input(|i| i.key_pressed(Key::Escape));
        if escape {
            self.rename = None;
            return;
        }
        if resp.lost_focus() {
            let Some(rename) = self.rename.take() else {
                return;
            };
            let title = rename.title.trim();
            let current = deck.slide(rename.index).and_then(|s| s.title.as_deref());
            if title != current.unwrap_or("") {
                out.push(PaneAction::Edit(
                    SlideEdit::Retitle(rename.index, title.to_string()).apply(markdown, deck),
                ));
            }
        }
    }

    fn menu(
        &mut self,
        ui: &mut Ui,
        markdown: &str,
        deck: &Deck,
        slide: &Slide,
        out: &mut Vec<PaneAction>,
    ) {
        let index = slide.index;
        let total = deck.slides.len();
        let edit = |e: SlideEdit, out: &mut Vec<PaneAction>| {
            out.push(PaneAction::Edit(e.apply(markdown, deck)));
        };
        if menu_item(ui, icons::PLUS, "New Slide Below", true) {
            edit(
                SlideEdit::Insert {
                    after: Some(index),
                    title: String::new(),
                    body: String::new(),
                },
                out,
            );
        }
        if menu_item(ui, icons::CLONE, "Duplicate Slide", true) {
            edit(SlideEdit::Duplicate(index), out);
        }
        if menu_item(ui, icons::PEN, "Rename...", true) {
            self.rename = Some(Rename {
                index,
                title: slide.title.clone().unwrap_or_default(),
                focus: true,
            });
        }
        ui.separator();
        if menu_item(ui, icons::ARROW_UP, "Move Up", index > 1) {
            edit(SlideEdit::Move(index, Direction::Up), out);
        }
        if menu_item(ui, icons::ARROW_DOWN, "Move Down", index < total) {
            edit(SlideEdit::Move(index, Direction::Down), out);
        }
        if menu_item(ui, icons::COMPRESS, "Merge Into Previous", index > 1) {
            edit(SlideEdit::MergeIntoPrevious(index), out);
        }
        ui.separator();
        // The check marks what the slide *declares*; Automatic names the inference.
        let explicit = slide.explicit_layout();
        for layout in MENU_LAYOUTS {
            let label = if layout == Layout::Auto {
                format!("Layout: Automatic ({})", slide.layout.id())
            } else {
                format!("Layout: {}", layout.id())
            };
            let checked = match explicit {
                Some(e) => e == layout.id(),
                None => layout == Layout::Auto,
            };
            let icon = if checked {
                icons::CIRCLE_CHECK
            } else {
                icons::TABLE_CELLS_LARGE
            };
            if menu_item(ui, icon, &label, true) {
                edit(
                    SlideEdit::SetLayout(index, Some(layout.id().to_string())),
                    out,
                );
            }
        }
        ui.separator();
        if menu_item(ui, icons::SLIDERS, "Presentation Settings...", true) {
            out.push(PaneAction::OpenSettings);
        }
        if menu_item(ui, icons::TRASH, "Delete Slide", true) {
            edit(SlideEdit::Delete(index), out);
        }
    }
}

/// A context-menu row with a Font Awesome glyph; closes the menu when clicked.
fn menu_item(ui: &mut Ui, glyph: &str, label: &str, enabled: bool) -> bool {
    let text = RichText::new(format!("{glyph}  {label}")).size(13.0);
    let clicked = ui
        .add_enabled(enabled, egui::Button::new(text).frame(false))
        .clicked();
    if clicked {
        ui.close();
    }
    clicked
}

/// Title (or "Untitled") and the layout tag, with the notes icon.
fn paint_meta(ui: &Ui, rect: Rect, slide: &Slide) {
    let p = theme::palette(ui.ctx());
    let painter = ui.painter_at(rect);
    let (title, color, italic) = match &slide.title {
        Some(t) => (t.as_str(), p.text_primary, false),
        None => ("Untitled", p.text_muted, true),
    };
    let style = fonts::Style {
        italic,
        ..Default::default()
    };
    let mut job =
        egui::text::LayoutJob::single_section(title.to_string(), fonts::format(style, 12.0, color));
    job.wrap.max_width = rect.width().max(1.0);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.ctx().fonts_mut(|f| f.layout_job(job));
    let title_h = galley.size().y;
    let top = rect.center().y - (title_h + 14.0) / 2.0;
    painter.galley(pos2(rect.left(), top), galley, color);
    let tag_y = top + title_h + 2.0;
    let tag = painter.text(
        pos2(rect.left(), tag_y),
        Align2::LEFT_TOP,
        slide.layout.id(),
        FontId::proportional(10.0),
        p.text_muted,
    );
    if !slide.notes.is_empty() {
        painter.text(
            pos2(tag.right() + 6.0, tag_y),
            Align2::LEFT_TOP,
            icons::COMMENT_DOTS,
            icons::font(10.0),
            p.text_muted,
        );
    }
}

fn empty(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(12.0).color(p.text_muted));
    });
}

/// The preview column for a deck: every slide scaled to the column width,
/// speaker notes under each, the caret's slide outlined and kept in view.
#[derive(Default)]
pub struct DeckPreview {
    followed: Option<usize>,
}

impl DeckPreview {
    /// Draws the column; returns a line to jump the editor to when a slide is clicked.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        deck: Option<&Deck>,
        active: Option<usize>,
        vars: &HashMap<String, String>,
        images: &dyn ImageSource,
    ) -> Option<usize> {
        let p = theme::palette(ui.ctx());
        let mut jump = None;
        ScrollArea::vertical()
            .id_salt("noteliner-deck-preview")
            .auto_shrink(false)
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        let Some(deck) = deck.filter(|d| !d.slides.is_empty()) else {
                            ui.label(
                                RichText::new(
                                    "No slides yet — add a heading or a --- separator to start one.",
                                )
                                .color(p.text_muted),
                            );
                            return;
                        };
                        ui.spacing_mut().item_spacing.y = 20.0;
                        let total = deck.slides.len();
                        for slide in &deck.slides {
                            let is_active = active == Some(slide.index);
                            let rect = self.slide(ui, deck, slide, total, is_active, vars, images, &mut jump);
                            // Follow the caret's slide once per change: `nearest`
                            // and instant, like `scrollIntoView`.
                            if is_active && self.followed != active {
                                ui.scroll_to_rect_animation(
                                    rect,
                                    None,
                                    egui::style::ScrollAnimation::none(),
                                );
                            }
                        }
                        if deck.skipped > 0 {
                            let s = if deck.skipped == 1 { "" } else { "s" };
                            ui.label(
                                RichText::new(format!(
                                    "{} slide{s} hidden by notes-only.",
                                    deck.skipped
                                ))
                                .size(12.0)
                                .color(p.text_muted),
                            );
                        }
                    });
            });
        self.followed = active;
        jump
    }

    #[allow(clippy::too_many_arguments)]
    fn slide(
        &self,
        ui: &mut Ui,
        deck: &Deck,
        slide: &Slide,
        total: usize,
        is_active: bool,
        vars: &HashMap<String, String>,
        images: &dyn ImageSource,
        jump: &mut Option<usize>,
    ) -> Rect {
        let p = theme::palette(ui.ctx());
        let width = (ui.available_width() - RAIL).max(120.0);
        let view = SlideView::new(slide, &deck.config)
            .total(total)
            .vars(vars)
            .images(images);
        let inner = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 6.0;
            let (row, resp) =
                ui.allocate_exact_size(vec2(RAIL + width, width * view.aspect()), Sense::click());
            let slide_rect = Rect::from_min_size(
                pos2(row.left() + RAIL, row.top()),
                vec2(width, row.height()),
            );
            if ui.is_rect_visible(row) {
                view.paint(ui, slide_rect);
                let (stroke, color) = if is_active {
                    (Stroke::new(2.0, p.accent), p.accent)
                } else {
                    (Stroke::new(1.0, p.border), p.text_muted)
                };
                ui.painter()
                    .rect_stroke(slide_rect, 0.0, stroke, StrokeKind::Outside);
                ui.painter().text(
                    pos2(row.left() + RAIL / 2.0, row.top() + 4.0),
                    Align2::CENTER_TOP,
                    slide.index.to_string(),
                    FontId::proportional(11.0),
                    color,
                );
            }
            let resp = resp.on_hover_text(format!("Jump to slide {} in the editor", slide.index));
            resp.widget_info(|| {
                WidgetInfo::selected(
                    egui::Role::Button,
                    true,
                    is_active,
                    format!("Slide {}", slide.index),
                )
            });
            if resp.clicked() {
                *jump = Some(slide.source_range.from_line);
            }
            if !slide.notes.is_empty() {
                ui.horizontal_top(|ui| {
                    ui.add_space(RAIL);
                    ui.label(icons::icon_sized(icons::COMMENT_DOTS, 12.0).color(p.text_muted));
                    ui.add(
                        egui::Label::new(
                            RichText::new(&slide.notes)
                                .size(12.0)
                                .color(p.text_secondary),
                        )
                        .wrap(),
                    );
                });
            }
            row
        });
        inner.inner
    }
}
