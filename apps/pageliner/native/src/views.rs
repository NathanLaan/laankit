//! The library screens: `LibrarySidebar.svelte`, `LibraryGrid.svelte` with
//! its `BookCard`s, and `StatusBar.svelte`.

use std::collections::HashMap;

use egui::{pos2, vec2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui};
use laankit_egui::theme::{self, Palette};
use laankit_egui::{fonts, icons, settings};

use crate::commands::Act;
use crate::model::{self, Book, Counts, FormatFilter, SortMode};

/// `.grid`: `repeat(auto-fill, minmax(140px, 1fr))`, 12 px gaps.
const CARD_MIN_WIDTH: f32 = 140.0;
const GRID_GAP: f32 = 12.0;
/// `.card` padding and the meta lines under the 2:3 cover.
const CARD_PAD: f32 = 8.0;
const META_HEIGHT: f32 = 40.0;
/// `.app-sidebar { width: 240px }`.
pub const SIDEBAR_WIDTH: f32 = 240.0;
/// `.status-bar { height: 24px }`.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;

/// A book's cover as the grid knows it.
pub enum Cover {
    Loading,
    Ready(egui::TextureHandle),
    /// No cover, or it couldn't be decoded: the format icon.
    Missing,
}

/// What the user did in a view.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewAction {
    Filter(FormatFilter),
    Sort(SortMode),
    Open(String),
    Delete(String),
    /// Load this book's cover (the card scrolled into view).
    WantCover(String),
    Act(Act),
}

/// `LibrarySidebar.svelte`: the format filters with counts, and the footer.
pub fn sidebar(ui: &mut Ui, counts: Counts, filter: FormatFilter) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let footer_h = 49.0;
    let full = ui.max_rect();
    let list_rect = Rect::from_min_max(full.min, pos2(full.max.x, full.max.y - footer_h));
    ui.scope_builder(
        egui::UiBuilder::new().max_rect(list_rect.shrink2(vec2(8.0, 10.0))),
        |ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("LIBRARY")
                        .size(10.0)
                        .color(p.text_faint)
                        .extra_letter_spacing(0.5),
                );
            });
            ui.add_space(6.0);
            for f in FormatFilter::ALL {
                if nav_item(ui, &p, f, counts.get(f), f == filter).clicked() {
                    out.push(ViewAction::Filter(f));
                }
            }
        },
    );
    let footer = Rect::from_min_max(pos2(full.min.x, full.max.y - footer_h), full.max);
    ui.painter()
        .hline(footer.x_range(), footer.top(), Stroke::new(1.0, p.border));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(footer.shrink(8.0))
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            let gear_w = ui.available_width() - 2.0 * (32.0 + 6.0);
            if foot_button(
                ui,
                &p,
                icons::GEAR,
                Some("Settings"),
                "Settings (Ctrl+,)",
                gear_w,
            )
            .clicked()
            {
                out.push(ViewAction::Act(Act::Settings));
            }
            if foot_button(ui, &p, icons::ROTATE, None, "Git Sync (Ctrl+Shift+S)", 32.0).clicked() {
                out.push(ViewAction::Act(Act::Sync));
            }
            if foot_button(ui, &p, icons::CIRCLE_INFO, None, "About", 32.0).clicked() {
                out.push(ViewAction::Act(Act::About));
            }
        },
    );
    out
}

/// `.nav-item`: icon, label and a count pill.
fn nav_item(
    ui: &mut Ui,
    p: &Palette,
    filter: FormatFilter,
    count: usize,
    active: bool,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
    let label = filter.label();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            active,
            format!("{label} ({count})"),
        )
    });
    let hovered = resp.hovered();
    let (fill, ink) = if active {
        (p.bg_selected, p.accent)
    } else if hovered {
        (p.bg_item_hover, p.text_primary)
    } else {
        (Color32::TRANSPARENT, p.text_secondary)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), fill);
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y),
        egui::Align2::CENTER_CENTER,
        filter.icon(),
        icons::font(12.0),
        ink,
    );
    painter.text(
        pos2(rect.left() + 36.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::proportional(13.0),
        ink,
    );
    let count_galley = painter.layout_no_wrap(
        count.to_string(),
        FontId::proportional(11.0),
        if active { p.accent } else { p.text_muted },
    );
    let pill = Rect::from_center_size(
        pos2(
            rect.right() - 10.0 - (count_galley.size().x + 16.0) / 2.0,
            rect.center().y,
        ),
        vec2(count_galley.size().x + 16.0, 17.0),
    );
    painter.rect_filled(pill, CornerRadius::same(10), p.bg_button);
    painter.galley(
        pill.center() - count_galley.size() / 2.0,
        count_galley,
        p.text_muted,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `.foot-btn`.
fn foot_button(
    ui: &mut Ui,
    p: &Palette,
    glyph: &str,
    label: Option<&str>,
    title: &str,
    width: f32,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(width, 33.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, label.unwrap_or(title))
    });
    let hovered = resp.hovered();
    let ink = if hovered {
        p.text_primary
    } else {
        p.text_secondary
    };
    if hovered {
        ui.painter()
            .rect_filled(rect, CornerRadius::same(6), p.bg_item_hover);
    }
    let icon_x = if label.is_some() {
        rect.left() + 18.0
    } else {
        rect.center().x
    };
    ui.painter().text(
        pos2(icon_x, rect.center().y),
        egui::Align2::CENTER_CENTER,
        glyph,
        icons::font(13.0),
        ink,
    );
    if let Some(label) = label {
        ui.painter().text(
            pos2(rect.left() + 34.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            ink,
        );
    }
    resp.on_hover_text(title)
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// What the grid shows.
pub struct GridProps<'a> {
    pub books: &'a [Book],
    pub sort: SortMode,
    pub filter: FormatFilter,
    pub loading: bool,
    pub selected: Option<&'a str>,
    pub covers: &'a HashMap<String, Cover>,
}

/// `LibraryGrid.svelte`: the header (title, count, sort, Import) and the
/// card grid, virtualized by rows.
pub fn library(ui: &mut Ui, props: &GridProps<'_>) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    // Header.
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(20, 16))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(fonts::bold(
                    RichText::new("Library").size(16.0).color(p.text_primary),
                ));
                count_pill(ui, &p, props.books.len());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if import_button(ui, &p, "Import").clicked() {
                        out.push(ViewAction::Act(Act::Import));
                    }
                    ui.allocate_ui(vec2(170.0, 30.0), |ui| {
                        ui.set_width(170.0);
                        if let Some(v) =
                            settings::select(ui, "Sort books", &SortMode::OPTIONS, props.sort.id())
                        {
                            out.push(ViewAction::Sort(SortMode::from_id(&v)));
                        }
                    });
                });
            });
        });
    let line = ui.available_rect_before_wrap();
    ui.painter()
        .hline(line.x_range(), line.top(), Stroke::new(1.0, p.border));

    if props.loading {
        centered(ui, |ui| {
            ui.label(RichText::new("Loading…").color(p.text_muted));
        });
        return out;
    }
    if props.books.is_empty() {
        centered(ui, |ui| {
            ui.label(
                icons::icon_sized(icons::BOOK_OPEN, 40.0).color(p.text_muted.gamma_multiply(0.5)),
            );
            ui.add_space(12.0);
            ui.label(RichText::new("Your library is empty.").color(p.text_muted));
            ui.add_space(12.0);
            if import_button(ui, &p, "Import a book…").clicked() {
                out.push(ViewAction::Act(Act::Import));
            }
            ui.add_space(12.0);
            ui.label(
                RichText::new("EPUB and PDF supported.")
                    .size(12.0)
                    .color(p.text_faint),
            );
        });
        return out;
    }

    let books = model::visible_books(props.books, props.sort, props.filter);
    let inner = ui.available_width() - 40.0;
    let cols = (((inner + GRID_GAP) / (CARD_MIN_WIDTH + GRID_GAP)).floor() as usize).max(1);
    let card_w = (inner - GRID_GAP * (cols - 1) as f32) / cols as f32;
    let cover_h = (card_w - 2.0 * CARD_PAD) * 1.5;
    let card_h = cover_h + 2.0 * CARD_PAD + META_HEIGHT;
    let rows = books.len().div_ceil(cols);
    egui::ScrollArea::vertical()
        .id_salt("library-grid")
        .auto_shrink(false)
        .show_rows(ui, card_h + GRID_GAP, rows, |ui, range| {
            for row in range {
                ui.horizontal(|ui| {
                    ui.add_space(20.0);
                    ui.spacing_mut().item_spacing.x = GRID_GAP;
                    for book in books.iter().skip(row * cols).take(cols) {
                        let cover = props.covers.get(&book.id);
                        if cover.is_none() && book.cover.is_some() {
                            out.push(ViewAction::WantCover(book.id.clone()));
                        }
                        let selected = props.selected == Some(book.id.as_str());
                        match book_card(ui, &p, book, cover, selected, vec2(card_w, card_h)) {
                            Some(CardAction::Open) => out.push(ViewAction::Open(book.id.clone())),
                            Some(CardAction::Delete) => {
                                out.push(ViewAction::Delete(book.id.clone()))
                            }
                            None => {}
                        }
                    }
                });
                ui.add_space(GRID_GAP);
            }
        });
    out
}

fn centered(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() / 2.0 - 90.0).max(24.0));
        add(ui);
    });
}

fn count_pill(ui: &mut Ui, p: &Palette, n: usize) {
    let galley =
        ui.painter()
            .layout_no_wrap(n.to_string(), FontId::proportional(12.0), p.text_muted);
    let (rect, _) = ui.allocate_exact_size(vec2(galley.size().x + 16.0, 18.0), Sense::hover());
    ui.painter()
        .rect_filled(rect, CornerRadius::same(10), p.bg_button);
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, p.text_muted);
}

/// `.import-btn`: accent outline, filling with the accent on hover.
fn import_button(ui: &mut Ui, p: &Palette, label: &str) -> egui::Response {
    let galley = ui.painter().layout_no_wrap(
        format!("{}  {label}", icons::PLUS),
        FontId::proportional(12.0),
        Color32::PLACEHOLDER,
    );
    let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 28.0, 28.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let hovered = resp.hovered();
    let (fill, ink) = if hovered {
        (p.accent, p.accent_on)
    } else {
        (p.bg_selected, p.accent)
    };
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        fill,
        Stroke::new(1.0, p.accent),
        egui::StrokeKind::Inside,
    );
    ui.painter()
        .galley(rect.center() - galley.size() / 2.0, galley, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

enum CardAction {
    Open,
    Delete,
}

/// `BookCard.svelte`: the cover (or format icon) with the format badge and,
/// on hover, the remove button; then title and author.
fn book_card(
    ui: &mut Ui,
    p: &Palette,
    book: &Book,
    cover: Option<&Cover>,
    selected: bool,
    size: egui::Vec2,
) -> Option<CardAction> {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &book.title));
    let hovered = resp.hovered();
    let painter = ui.painter().clone();
    if selected {
        painter.rect(
            rect,
            CornerRadius::same(8),
            p.bg_selected,
            Stroke::new(1.0, p.accent),
            egui::StrokeKind::Inside,
        );
    } else if hovered {
        painter.rect_filled(rect, CornerRadius::same(8), p.bg_item_hover);
    }
    let inner = rect.shrink(CARD_PAD);
    let cover_rect = Rect::from_min_size(inner.min, vec2(inner.width(), inner.width() * 1.5));
    let radius = CornerRadius::same(6);
    painter.rect_filled(cover_rect, radius, p.bg_button);
    match cover {
        Some(Cover::Ready(tex)) => {
            // `object-fit: cover`: crop the texture to the 2:3 frame.
            let [tw, th] = tex.size().map(|v| v as f32);
            let (fw, fh) = (cover_rect.width(), cover_rect.height());
            let scale = (fw / tw).max(fh / th);
            let (vw, vh) = (fw / (tw * scale), fh / (th * scale));
            let uv = Rect::from_center_size(pos2(0.5, 0.5), vec2(vw, vh));
            egui::Image::new((tex.id(), cover_rect.size()))
                .uv(uv)
                .corner_radius(radius)
                .paint_at(ui, cover_rect);
        }
        _ => {
            painter.text(
                cover_rect.center(),
                egui::Align2::CENTER_CENTER,
                model::format_icon(&book.format),
                icons::font(40.0),
                p.text_faint,
            );
        }
    }
    painter.rect_stroke(
        cover_rect,
        radius,
        Stroke::new(1.0, p.border),
        egui::StrokeKind::Inside,
    );

    // Format badge.
    let badge = painter.layout_no_wrap(
        book.format.to_uppercase(),
        FontId::proportional(9.0),
        Color32::WHITE,
    );
    let badge_rect = Rect::from_min_size(
        cover_rect.min + vec2(6.0, 6.0),
        badge.size() + vec2(12.0, 4.0),
    );
    painter.rect_filled(
        badge_rect,
        CornerRadius::same(3),
        Color32::from_black_alpha(140),
    );
    painter.galley(badge_rect.min + vec2(6.0, 2.0), badge, Color32::WHITE);

    // Remove button (on hover).
    let del_rect = Rect::from_min_size(
        pos2(cover_rect.right() - 30.0, cover_rect.top() + 6.0),
        vec2(24.0, 24.0),
    );
    let del = ui
        .interact(del_rect, resp.id.with("delete"), Sense::click())
        .on_hover_text("Remove from library");
    del.widget_info(|| {
        egui::WidgetInfo::labeled(egui::Role::Button, true, format!("Remove {}", book.title))
    });
    if hovered || del.hovered() {
        let fill = if del.hovered() {
            Color32::from_rgb(0xe0, 0x48, 0x4d)
        } else {
            Color32::from_black_alpha(140)
        };
        painter.rect_filled(del_rect, CornerRadius::same(4), fill);
        painter.text(
            del_rect.center(),
            egui::Align2::CENTER_CENTER,
            icons::TRASH,
            icons::font(11.0),
            Color32::WHITE,
        );
    }

    // Title and author, one line each.
    let meta = Rect::from_min_max(
        pos2(inner.left() + 2.0, cover_rect.bottom() + 6.0),
        pos2(inner.right() - 2.0, rect.bottom()),
    );
    let clip = painter.with_clip_rect(meta);
    let title = elide(ui, &book.title, FontId::proportional(13.0), meta.width());
    clip.text(
        meta.min,
        egui::Align2::LEFT_TOP,
        title,
        FontId::proportional(13.0),
        p.text_primary,
    );
    let author = elide(
        ui,
        book.author.as_deref().unwrap_or("Unknown author"),
        FontId::proportional(11.0),
        meta.width(),
    );
    clip.text(
        meta.min + vec2(0.0, 18.0),
        egui::Align2::LEFT_TOP,
        author,
        FontId::proportional(11.0),
        p.text_muted,
    );
    resp.clone().on_hover_text(&book.title);

    if del.clicked() {
        Some(CardAction::Delete)
    } else if resp.clicked() {
        Some(CardAction::Open)
    } else {
        None
    }
}

/// `text-overflow: ellipsis` for one line.
fn elide(ui: &Ui, text: &str, font: FontId, width: f32) -> String {
    let fits = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
                .size()
                .x
        }) <= width
    };
    if fits(text) {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let (mut lo, mut hi) = (0, chars.len());
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let candidate: String = chars[..mid].iter().collect::<String>() + "…";
        if fits(&candidate) {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    chars[..lo].iter().collect::<String>() + "…"
}

/// `StatusBar.svelte`: the open book or the book count; the reading status;
/// the format and UI scale.
pub fn status_bar(
    ui: &mut Ui,
    book: Option<&Book>,
    count: usize,
    reading: Option<&str>,
    scale: &str,
) {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.bg_surface);
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    let mut left = rect.left() + 10.0;
    match book {
        Some(b) => {
            painter.text(
                pos2(left, y),
                egui::Align2::LEFT_CENTER,
                model::format_icon(&b.format),
                icons::font(11.0),
                p.text_secondary,
            );
            left += 18.0;
            painter.text(
                pos2(left, y),
                egui::Align2::LEFT_CENTER,
                &b.title,
                font.clone(),
                p.text_secondary,
            );
        }
        None => {
            let s = if count == 1 { "" } else { "s" };
            painter.text(
                pos2(left, y),
                egui::Align2::LEFT_CENTER,
                format!("{count} book{s}"),
                font.clone(),
                p.text_muted,
            );
        }
    }
    if let (Some(_), Some(status)) = (book, reading) {
        painter.text(
            pos2(rect.center().x, y),
            egui::Align2::CENTER_CENTER,
            status,
            font.clone(),
            p.text_secondary,
        );
    }
    let mut right = rect.right() - 10.0;
    let scale_text = format!("{scale}%");
    let r = painter.text(
        pos2(right, y),
        egui::Align2::RIGHT_CENTER,
        scale_text,
        font.clone(),
        p.text_muted,
    );
    right = r.left() - 12.0;
    if let Some(b) = book {
        painter.text(
            pos2(right, y),
            egui::Align2::RIGHT_CENTER,
            b.format.to_uppercase(),
            font,
            p.text_muted,
        );
    }
}

/// The overlay shown while files are dragged over the window.
pub fn drop_overlay(ctx: &egui::Context) {
    let p = theme::palette(ctx);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("pageliner-drop"),
    ));
    painter.rect_filled(screen, 0.0, p.modal_overlay);
    let target = screen.shrink(24.0);
    painter.rect_stroke(
        target,
        CornerRadius::same(12),
        Stroke::new(2.0, p.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        target.center() - vec2(0.0, 24.0),
        egui::Align2::CENTER_CENTER,
        icons::FILE_IMPORT,
        icons::font(40.0),
        p.accent,
    );
    painter.text(
        target.center() + vec2(0.0, 24.0),
        egui::Align2::CENTER_CENTER,
        "Drop EPUB or PDF files to import them",
        FontId::proportional(16.0),
        Color32::WHITE,
    );
}
