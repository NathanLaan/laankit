//! The readers: [`pdf::PdfReader`] (hayro) and [`epub::EpubReader`]
//! (`pageliner-epub`). `App.svelte` routes a selected book to one
//! of them by `book.format`; here [`Reader`] does.
//!
//! A reader never touches disk: it reports what to persist and show as
//! [`ReaderAction`]s, which the app turns into `library:setState` calls.

pub mod epub;
pub mod pdf;

use egui::{vec2, Color32, FontId, Response, Sense, Stroke, Ui};
use marina_egui::panes::PaneLayout;
use marina_egui::theme::{self, Palette};
use serde_json::Value;

/// Something a reader asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum ReaderAction {
    /// Back to the library (`onClose`).
    Close,
    /// Merge this patch into the book's `state/<id>.json` (`setBookState`).
    SaveState(Value),
    /// The status bar's reading context (`libraryState.readingStatus`).
    Status(Option<String>),
    /// Merge this patch into `ui-preferences.json` (the EPUB reading prefs).
    SavePrefs(Value),
}

/// The open book's reader.
pub enum Reader {
    Pdf(Box<pdf::PdfReader>),
    Epub(Box<epub::EpubReader>),
    /// A format neither reader handles (the Svelte "Unsupported format" view).
    Unsupported {
        title: String,
        format: String,
    },
}

impl Reader {
    pub fn show(&mut self, ui: &mut Ui, panes: &mut PaneLayout, keys: bool) -> Vec<ReaderAction> {
        match self {
            Reader::Pdf(r) => r.show(ui, panes, keys),
            Reader::Epub(r) => r.show(ui, panes, keys),
            Reader::Unsupported { title, format } => unsupported(ui, title, format),
        }
    }

    /// State not yet saved (the debounced page position), to flush on close.
    pub fn flush(&mut self) -> Option<Value> {
        match self {
            Reader::Pdf(r) => r.flush(),
            Reader::Epub(r) => r.flush(),
            Reader::Unsupported { .. } => None,
        }
    }
}

/// The reader bar's "← Library" button and the book title.
pub fn back_bar(ui: &mut Ui, title: &str) -> bool {
    let p = theme::palette(ui.ctx());
    let back = tool_button(
        ui,
        Some(marina_egui::icons::ARROW_LEFT),
        Some("Library"),
        "Back to Library",
        ToolState::default(),
    )
    .clicked();
    // `.tb-title { max-width: 40% }`.
    let max = ui.available_width() * 0.4;
    ui.allocate_ui(vec2(max, 28.0), |ui| {
        ui.add(
            egui::Label::new(
                egui::RichText::new(title)
                    .size(13.0)
                    .color(p.text_secondary),
            )
            .truncate(),
        )
        .on_hover_text(title);
    });
    back
}

/// `.reader` for an unsupported format.
fn unsupported(ui: &mut Ui, title: &str, format: &str) -> Vec<ReaderAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    egui::Panel::top("unsupported-bar")
        .frame(bar_frame(&p))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if back_bar(ui, title) {
                    out.push(ReaderAction::Close);
                }
            });
        });
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE.fill(p.bg_base))
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((ui.available_height() / 2.0 - 70.0).max(24.0));
                ui.label(
                    marina_egui::icons::icon_sized(marina_egui::icons::FILE_CIRCLE_QUESTION, 44.0)
                        .color(p.text_muted.gamma_multiply(0.5)),
                );
                ui.add_space(8.0);
                ui.label(egui::RichText::new(title).size(22.0).color(p.text_primary));
                ui.label(
                    egui::RichText::new(format!("Unsupported format: {}", format.to_uppercase()))
                        .color(p.text_muted),
                );
            });
        });
    out
}

/// `.toolbar` / `.reader-bar`: padded, with a bottom border.
pub fn bar_frame(p: &Palette) -> egui::Frame {
    egui::Frame::NONE
        .fill(p.bg_base)
        .inner_margin(egui::Margin::symmetric(12, 8))
        .stroke(Stroke::NONE)
}

/// How a [`tool_button`] looks.
#[derive(Clone, Copy, Debug, Default)]
pub struct ToolState {
    pub active: bool,
    pub disabled: bool,
}

/// `.tb-btn`: a bordered 12 px button with an optional glyph and label;
/// `title` is its tooltip and accessible name.
pub fn tool_button(
    ui: &mut Ui,
    glyph: Option<&str>,
    label: Option<&str>,
    title: &str,
    state: ToolState,
) -> Response {
    let p = theme::palette(ui.ctx());
    let ink = if state.active {
        p.accent
    } else {
        p.text_primary
    };
    let glyph_galley = glyph.map(|g| {
        ui.painter()
            .layout_no_wrap(g.to_string(), marina_egui::icons::font(12.0), ink)
    });
    let label_galley = label.map(|l| {
        ui.painter()
            .layout_no_wrap(l.to_string(), FontId::proportional(12.0), ink)
    });
    let gap = if glyph_galley.is_some() && label_galley.is_some() {
        6.0
    } else {
        0.0
    };
    let content_w = glyph_galley.as_ref().map_or(0.0, |g| g.size().x)
        + gap
        + label_galley.as_ref().map_or(0.0, |g| g.size().x);
    let pad_x = if label.is_some() { 10.0 } else { 9.0 };
    let size = vec2(content_w + 2.0 * pad_x, 28.0);
    let sense = if state.disabled {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, !state.disabled, state.active, title)
    });
    let hovered = !state.disabled && resp.hovered();
    let (fill, stroke) = if state.active {
        (p.bg_selected, Stroke::new(1.0, p.accent))
    } else if hovered {
        (p.bg_button_hover, Stroke::new(1.0, p.border))
    } else {
        (p.bg_button, Stroke::new(1.0, p.border))
    };
    let alpha = if state.disabled { 0.4 } else { 1.0 };
    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(6),
        fill.gamma_multiply(alpha),
        Stroke::new(stroke.width, stroke.color.gamma_multiply(alpha)),
        egui::StrokeKind::Inside,
    );
    let mut x = rect.left() + pad_x;
    for galley in [glyph_galley, label_galley].into_iter().flatten() {
        let y = rect.center().y - galley.size().y / 2.0;
        let w = galley.size().x;
        painter.galley_with_override_text_color(
            egui::pos2(x, y),
            galley,
            ink.gamma_multiply(alpha),
        );
        x += w + gap;
    }
    let resp = resp.on_hover_text(title);
    if !state.disabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// A row in the Contents or Search pane (`.toc-item`, `.result-item`).
pub fn list_row(ui: &mut Ui, text: &str, indent: f32, current: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 24.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_item_hover);
    }
    let color = if current {
        p.accent
    } else if resp.hovered() {
        p.text_primary
    } else {
        p.text_secondary
    };
    let galley = ui.painter().layout(
        text.to_string(),
        FontId::proportional(12.0),
        color,
        f32::INFINITY,
    );
    let clip = rect.shrink2(vec2(2.0, 0.0));
    ui.painter().with_clip_rect(clip).galley(
        egui::pos2(
            rect.left() + 10.0 + indent,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        color,
    );
    resp.on_hover_text(text)
}

/// `.pane-empty`.
pub fn pane_empty(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(12))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text).size(12.0).color(p.text_muted));
        });
}

/// One annotation in [`annotations_pane`].
pub struct AnnotationRow<'a> {
    pub id: &'a str,
    pub label: &'a str,
}

/// What the user did in [`annotations_pane`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnotationAction {
    JumpBookmark(String),
    DeleteBookmark(String),
    JumpHighlight(String),
    DeleteHighlight(String),
}

/// `AnnotationsPane.svelte`: bookmarks, then highlights, each with jump and
/// remove.
pub fn annotations_pane(
    ui: &mut Ui,
    bookmarks: &[AnnotationRow<'_>],
    highlights: &[AnnotationRow<'_>],
) -> Option<AnnotationAction> {
    let p = theme::palette(ui.ctx());
    let mut out = None;
    let section = |ui: &mut Ui, title: &str| {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(
                marina_egui::fonts::weight(title.to_uppercase(), marina_egui::fonts::SEMIBOLD)
                    .size(10.0)
                    .color(p.text_faint),
            );
        });
        ui.add_space(2.0);
    };
    egui::ScrollArea::vertical()
        .id_salt("annotations")
        .auto_shrink(false)
        .show(ui, |ui| {
            section(ui, "Bookmarks");
            if bookmarks.is_empty() {
                pane_empty(ui, "No bookmarks yet.");
            }
            for b in bookmarks {
                match annotation_row(ui, &p, b, false) {
                    Some(true) => out = Some(AnnotationAction::JumpBookmark(b.id.to_string())),
                    Some(false) => out = Some(AnnotationAction::DeleteBookmark(b.id.to_string())),
                    None => {}
                }
            }
            section(ui, "Highlights");
            if highlights.is_empty() {
                pane_empty(ui, "Select text while reading to highlight it.");
            }
            for h in highlights {
                match annotation_row(ui, &p, h, true) {
                    Some(true) => out = Some(AnnotationAction::JumpHighlight(h.id.to_string())),
                    Some(false) => out = Some(AnnotationAction::DeleteHighlight(h.id.to_string())),
                    None => {}
                }
            }
        });
    out
}

/// The highlight swatch colour (`.hl-rect`: rgba(240, 200, 0, 0.38)).
pub const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(91, 76, 0, 97);

/// A row: `Some(true)` to jump, `Some(false)` to remove.
fn annotation_row(
    ui: &mut Ui,
    p: &Palette,
    row: &AnnotationRow<'_>,
    highlight: bool,
) -> Option<bool> {
    let width = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 26.0), Sense::click());
    let del_rect = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 16.0, rect.center().y),
        vec2(20.0, 20.0),
    );
    let del = ui
        .interact(del_rect, resp.id.with("del"), Sense::click())
        .on_hover_text(if highlight {
            "Remove highlight"
        } else {
            "Remove bookmark"
        });
    let what = if highlight { "Highlight" } else { "Bookmark" };
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("{what}: {}", row.label))
    });
    del.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::Role::Button,
            true,
            format!("Remove {}: {}", what.to_lowercase(), row.label),
        )
    });
    if resp.hovered() || del.hovered() {
        ui.painter().rect_filled(rect, 4.0, p.bg_item_hover);
    }
    let painter = ui.painter();
    let x = rect.left() + 10.0;
    if highlight {
        let swatch =
            egui::Rect::from_min_size(egui::pos2(x, rect.center().y - 6.0), vec2(4.0, 12.0));
        painter.rect_filled(swatch, 1.0, Color32::from_rgb(240, 200, 0));
    } else {
        painter.text(
            egui::pos2(x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            marina_egui::icons::BOOKMARK,
            marina_egui::icons::font(11.0),
            p.accent,
        );
    }
    let galley = painter.layout_no_wrap(
        row.label.to_string(),
        FontId::proportional(12.0),
        p.text_secondary,
    );
    let text_rect = egui::Rect::from_min_max(
        egui::pos2(x + 16.0, rect.top()),
        egui::pos2(del_rect.left() - 4.0, rect.bottom()),
    );
    painter.with_clip_rect(text_rect).galley(
        egui::pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_secondary,
    );
    if resp.hovered() || del.hovered() {
        let ink = if del.hovered() {
            p.text_primary
        } else {
            p.text_muted
        };
        painter.text(
            del_rect.center(),
            egui::Align2::CENTER_CENTER,
            marina_egui::icons::XMARK,
            marina_egui::icons::font(11.0),
            ink,
        );
    }
    if del.clicked() {
        Some(false)
    } else if resp.clicked() {
        Some(true)
    } else {
        None
    }
}
