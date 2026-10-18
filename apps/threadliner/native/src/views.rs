//! The main window's panes: `Sidebar.svelte`, `EntryList.svelte`,
//! `ContentViewer.svelte`, plus the first-run `SetupDialog.svelte` and the
//! loading screen. Each draws from the app's data and returns what the user
//! did as [`ViewAction`]s; the app applies them.

use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, Ui,
};
use marina_egui::dialog::{self, ButtonKind};
use marina_egui::theme::{self, Palette};
use marina_egui::{fonts, icons};
use marina_richtext::{Action, Document, RichStyle, RichView, WebResolver};

use crate::model::{self, Entry, Feed, Tag};

/// `.sidebar` width limits and default (`Math.max(180, Math.min(500, …))`).
pub const SIDEBAR_WIDTH: (f32, f32, f32) = (180.0, 240.0, 500.0);
/// `.entry-list` width limits and default.
pub const ENTRY_LIST_WIDTH: (f32, f32, f32) = (220.0, 320.0, 600.0);
/// `.entry-item`: 10 px padding, a 13 px title line, 4 px gap, an 11 px meta
/// line and the bottom hairline.
const ENTRY_ROW_HEIGHT: f32 = 58.0;
/// `.feed-item { padding: 8px 16px }` around 13 px text.
const FEED_ROW_HEIGHT: f32 = 34.0;
/// `.tag-toggle { padding: 6px 16px }`.
const TAG_ROW_HEIGHT: f32 = 30.0;
/// `.sidebar-header { padding: 12px 16px }` around the 11 px caption.
const HEADER_HEIGHT: f32 = 40.0;
/// `.article { max-width: 720px; padding: 32px 24px }`.
const ARTICLE_WIDTH: f32 = 720.0;

/// What the user did in a pane.
#[derive(Clone, Debug, PartialEq)]
pub enum ViewAction {
    SelectFeed(String),
    SelectEntry(String),
    /// Toggle this tag on the selected feed.
    ToggleTag(String),
    OpenUrl(String),
    /// The setup dialog's Browse… button.
    Browse,
    /// The setup dialog's Get Started button.
    Setup,
}

/// The article open in the content viewer.
pub struct Article {
    pub doc: Document,
    pub view: RichView,
}

impl Article {
    pub fn new(entry: &Entry) -> Self {
        let doc = entry
            .content
            .as_deref()
            .map(|html| marina_richtext::from_article_html(html, entry.link.as_deref()))
            .unwrap_or_default();
        Self {
            doc,
            view: RichView::default().id_salt(("article", entry.id.clone())),
        }
    }
}

/// `SetupDialog.svelte`'s fields.
#[derive(Default)]
pub struct SetupForm {
    pub data_dir: String,
    pub remote_url: String,
    pub busy: bool,
    pub error: Option<String>,
}

/// A pane header (`.sidebar-header h2`): an 11 px upper-case caption on the
/// base colour with a hairline below.
fn pane_header(ui: &mut Ui, p: &Palette, title: &str) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, p.bg_base);
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let galley = ui.painter().layout(
        title.to_uppercase(),
        fonts::Style {
            weight: Some(fonts::SEMIBOLD),
            ..Default::default()
        }
        .font(11.0),
        p.text_muted,
        rect.width() - 32.0,
    );
    let galley = elide(
        ui,
        galley,
        title,
        11.0,
        p.text_muted,
        rect.width() - 32.0,
        true,
    );
    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_muted,
    );
}

/// One line of text, cut with an ellipsis to `width` (`text-overflow: ellipsis`).
fn elide(
    ui: &Ui,
    galley: std::sync::Arc<egui::Galley>,
    text: &str,
    size: f32,
    color: Color32,
    width: f32,
    semibold: bool,
) -> std::sync::Arc<egui::Galley> {
    if galley.rows.len() <= 1 {
        return galley;
    }
    let style = fonts::Style {
        weight: semibold.then_some(fonts::SEMIBOLD),
        ..Default::default()
    };
    let mut job = egui::text::LayoutJob::single_section(
        if semibold {
            text.to_uppercase()
        } else {
            text.to_string()
        },
        fonts::format(style, size, color),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width(width);
    ui.fonts_mut(|f| f.layout_job(job))
}

/// One elided line of text in `style`.
fn line(
    ui: &Ui,
    text: &str,
    style: fonts::Style,
    size: f32,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job =
        egui::text::LayoutJob::single_section(text.to_string(), fonts::format(style, size, color));
    job.wrap = egui::text::TextWrapping::truncate_at_width(width.max(1.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// A full-width selectable row: hover wash, and the selected wash with an
/// accent outline (`.feed-item.active`, `.entry-item.active`).
fn row(
    ui: &mut Ui,
    p: &Palette,
    height: f32,
    selected: bool,
    label: &str,
) -> (Rect, egui::Response) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::ListItem, true, selected, label));
    if selected {
        ui.painter().rect(
            rect,
            0.0,
            p.bg_selected,
            Stroke::new(1.0, p.accent),
            StrokeKind::Inside,
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    (rect, resp)
}

/// `Sidebar.svelte`: the feeds with unread badges, then the tag toggles for
/// the selected feed.
pub fn sidebar(
    ui: &mut Ui,
    feeds: &[Feed],
    tags: &[Tag],
    selected: Option<&Feed>,
) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    pane_header(ui, &p, "Feeds");
    let tags_height = if tags.is_empty() {
        0.0
    } else {
        (HEADER_HEIGHT + tags.len() as f32 * TAG_ROW_HEIGHT + 4.0).min(ui.available_height() * 0.4)
    };
    let list_height = (ui.available_height() - tags_height).max(0.0);
    ui.allocate_ui(vec2(ui.available_width(), list_height), |ui| {
        ScrollArea::vertical()
            .id_salt("feed-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(4.0);
                if feeds.is_empty() {
                    ui.add_space(12.0);
                    ui.vertical_centered(|ui| dialog::muted_text(ui, "No feeds added yet."));
                }
                for feed in feeds {
                    let active = selected.is_some_and(|s| s.id == feed.id);
                    let label = format!("{} ({} unread)", feed.title, feed.unread_count);
                    let (rect, resp) = row(ui, &p, FEED_ROW_HEIGHT, active, &label);
                    if resp.clicked() {
                        out.push(ViewAction::SelectFeed(feed.id.clone()));
                    }
                    feed_row(ui, &p, rect, feed);
                }
                ui.add_space(4.0);
            });
    });
    if !tags.is_empty() {
        let top = ui.cursor().top();
        ui.painter()
            .hline(ui.max_rect().x_range(), top, Stroke::new(1.0, p.border));
        let (rect, _) =
            ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
        ui.painter().rect_filled(rect, 0.0, p.bg_base);
        let caption = line(
            ui,
            "TAGS",
            fonts::Style {
                weight: Some(fonts::SEMIBOLD),
                ..Default::default()
            },
            11.0,
            p.text_muted,
            rect.width(),
        );
        ui.painter().galley(
            pos2(rect.left() + 16.0, rect.center().y - caption.size().y / 2.0),
            caption,
            p.text_muted,
        );
        ScrollArea::vertical()
            .id_salt("tag-list")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for tag in tags {
                    if tag_toggle(ui, &p, tag, selected).clicked() {
                        out.push(ViewAction::ToggleTag(tag.id.clone()));
                    }
                }
            });
    }
    out
}

fn feed_row(ui: &Ui, p: &Palette, rect: Rect, feed: &Feed) {
    let mut right = rect.right() - 16.0;
    if feed.unread_count > 0 {
        // `.unread-badge`: an accent pill, 18 px tall, at least 20 px wide.
        let text = ui.painter().layout_no_wrap(
            feed.unread_count.to_string(),
            fonts::Style {
                weight: Some(fonts::SEMIBOLD),
                ..Default::default()
            }
            .font(11.0),
            p.accent_on,
        );
        let w = (text.size().x + 12.0).max(20.0);
        let badge = Rect::from_center_size(pos2(right - w / 2.0, rect.center().y), vec2(w, 18.0));
        ui.painter().rect_filled(badge, 9.0, p.accent);
        ui.painter()
            .galley(badge.center() - text.size() / 2.0, text, p.accent_on);
        right = badge.left() - 8.0;
    }
    let title = line(
        ui,
        &feed.title,
        fonts::Style::default(),
        13.0,
        p.text_primary,
        right - rect.left() - 16.0,
    );
    ui.painter().galley(
        pos2(rect.left() + 16.0, rect.center().y - title.size().y / 2.0),
        title,
        p.text_primary,
    );
}

/// `.tag-toggle`: a checkbox glyph and the name; accent when assigned,
/// dimmed and inert with no feed selected.
fn tag_toggle(ui: &mut Ui, p: &Palette, tag: &Tag, feed: Option<&Feed>) -> egui::Response {
    let assigned = feed.is_some_and(|f| f.tag_ids.contains(&tag.id));
    let enabled = feed.is_some();
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), TAG_ROW_HEIGHT), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, assigned, &tag.name)
    });
    let hovered = enabled && resp.hovered();
    if hovered {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    let ink = if assigned {
        p.accent
    } else if hovered {
        p.text_primary
    } else {
        p.text_muted
    };
    let ink = if enabled {
        ink
    } else {
        ink.gamma_multiply(0.3)
    };
    ui.painter().text(
        pos2(rect.left() + 16.0 + 6.0, rect.center().y),
        Align2::CENTER_CENTER,
        if assigned {
            icons::SQUARE_CHECK
        } else {
            icons::SQUARE
        },
        icons::font(13.0),
        ink,
    );
    let name = line(
        ui,
        &tag.name,
        fonts::Style::default(),
        13.0,
        ink,
        rect.width() - 52.0,
    );
    ui.painter().galley(
        pos2(
            rect.left() + 16.0 + 20.0,
            rect.center().y - name.size().y / 2.0,
        ),
        name,
        ink,
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// `EntryList.svelte`: the selected feed's entries, newest first, virtualized.
pub fn entry_list(
    ui: &mut Ui,
    feed: Option<&Feed>,
    entries: &[Entry],
    loading: bool,
    selected: Option<&str>,
) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
    pane_header(ui, &p, feed.map_or("Entries", |f| f.title.as_str()));
    let empty = |ui: &mut Ui, spinner: bool, text: &str| {
        ui.add_space(32.0);
        ui.vertical_centered(|ui| {
            if spinner {
                ui.spinner();
                ui.add_space(8.0);
            }
            dialog::muted_text(ui, text);
        });
    };
    if feed.is_none() {
        empty(ui, false, "Select a feed to view entries.");
        return out;
    }
    if loading {
        empty(ui, true, "Loading entries...");
        return out;
    }
    if entries.is_empty() {
        empty(ui, false, "No entries in this feed.");
        return out;
    }
    ScrollArea::vertical()
        .id_salt(("entries", feed.map(|f| f.id.clone())))
        .auto_shrink([false, false])
        .show_rows(ui, ENTRY_ROW_HEIGHT, entries.len(), |ui, range| {
            for entry in &entries[range] {
                let active = selected == Some(entry.id.as_str());
                let (rect, resp) = row(ui, &p, ENTRY_ROW_HEIGHT, active, entry.display_title());
                if resp.clicked() {
                    out.push(ViewAction::SelectEntry(entry.id.clone()));
                }
                entry_row(ui, &p, rect, entry);
            }
        });
    out
}

fn entry_row(ui: &Ui, p: &Palette, rect: Rect, entry: &Entry) {
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let width = rect.width() - 32.0;
    // Unread titles are semibold in the primary colour; read ones muted.
    let (style, color) = if entry.is_read {
        (fonts::Style::default(), p.text_muted)
    } else {
        (
            fonts::Style {
                weight: Some(fonts::SEMIBOLD),
                ..Default::default()
            },
            p.text_primary,
        )
    };
    let title = line(ui, entry.display_title(), style, 13.0, color, width);
    let title_h = title.size().y;
    ui.painter()
        .galley(pos2(rect.left() + 16.0, rect.top() + 10.0), title, color);
    let meta_color = p.text_muted.gamma_multiply(0.7);
    let mut x = rect.left() + 16.0;
    let y = rect.top() + 10.0 + title_h + 4.0;
    if let Some(author) = &entry.author {
        let a = line(ui, author, fonts::Style::default(), 11.0, meta_color, 140.0);
        let w = a.size().x;
        ui.painter().galley(pos2(x, y), a, meta_color);
        x += w + 8.0;
    }
    if let Some(date) = &entry.published_at {
        let d = line(
            ui,
            &model::short_date(date),
            fonts::Style::default(),
            11.0,
            meta_color,
            rect.right() - x,
        );
        ui.painter().galley(pos2(x, y), d, meta_color);
    }
}

/// `ContentViewer.svelte`: the article header, then its content through
/// marina-richtext (sanitized with ThreadLiner's DOMPurify allow-list).
pub fn content(
    ui: &mut Ui,
    entry: Option<&Entry>,
    article: Option<&mut Article>,
) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let (Some(entry), Some(article)) = (entry, article) else {
        ui.with_layout(
            Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space((ui.available_height() / 2.0 - 50.0).max(0.0));
                    ui.label(icons::icon_sized(icons::RSS, 42.0).color(p.text_muted));
                    ui.add_space(16.0);
                    ui.label(
                        RichText::new("Select an entry to read")
                            .size(15.0)
                            .color(p.text_muted),
                    );
                });
            },
        );
        return out;
    };
    let full = ui.available_width();
    let width = full.min(ARTICLE_WIDTH);
    let inset = ((full - width) / 2.0).max(0.0);
    Frame::new()
        .inner_margin(Margin {
            left: (inset + 24.0) as i8,
            right: (inset + 24.0) as i8,
            top: 32,
            bottom: 0,
        })
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = vec2(16.0, 0.0);
            let title = fonts::styled(
                RichText::new(entry.display_title())
                    .size(24.0)
                    .color(p.text_primary),
                fonts::Style::BOLD,
            );
            ui.add(egui::Label::new(title).wrap());
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                let meta = |ui: &mut Ui, glyph: &str, text: &str| {
                    ui.label(icons::icon_sized(glyph, 11.0).color(p.text_muted));
                    ui.add_space(-12.0);
                    ui.label(RichText::new(text).size(13.0).color(p.text_muted));
                };
                if let Some(author) = &entry.author {
                    meta(ui, icons::USER, author);
                }
                if let Some(date) = entry.published_at.as_deref().map(model::long_date) {
                    meta(ui, icons::CALENDAR, &date);
                }
                if let Some(link) = &entry.link {
                    let text = format!("{}  View Original", icons::EXTERNAL_LINK_ALT);
                    let resp = ui
                        .add(
                            egui::Label::new(RichText::new(text).size(13.0).color(p.accent))
                                .sense(Sense::click()),
                        )
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .on_hover_text(link);
                    if resp.clicked() {
                        out.push(ViewAction::OpenUrl(link.clone()));
                    }
                }
            });
            ui.add_space(16.0);
            ui.painter().hline(
                ui.max_rect().x_range(),
                ui.cursor().top(),
                Stroke::new(1.0, p.border),
            );
        });
    if article.doc.blocks.is_empty() {
        ui.add_space(48.0);
        ui.vertical_centered(|ui| match &entry.link {
            Some(link) => {
                dialog::muted_text(ui, "This entry has no inline content.");
                ui.add_space(16.0);
                let text = format!("Read on the original site  {}", icons::ARROW_RIGHT);
                if ui
                    .add(
                        egui::Label::new(RichText::new(text).color(p.accent)).sense(Sense::click()),
                    )
                    .on_hover_cursor(egui::CursorIcon::PointingHand)
                    .clicked()
                {
                    out.push(ViewAction::OpenUrl(link.clone()));
                }
            }
            None => dialog::muted_text(ui, "No content available for this entry."),
        });
        return out;
    }
    ui.add_space(24.0);
    let mut style = RichStyle::from_ctx(ui.ctx());
    style.size = 15.0;
    style.max_width = ARTICLE_WIDTH - 48.0;
    // `.article { padding: 0 24px }`; the view centres its column in what's left.
    let resp = Frame::new()
        .inner_margin(Margin::symmetric(24, 0))
        .show(ui, |ui| {
            article
                .view
                .show(ui, &article.doc, &style, &mut WebResolver)
        })
        .inner;
    for Action::Open(url) in resp.actions {
        out.push(ViewAction::OpenUrl(url));
    }
    out
}

/// `SetupDialog.svelte`: the first-run card, centred on the base colour.
pub fn setup(ui: &mut Ui, form: &mut SetupForm) -> Vec<ViewAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    let width = 520.0_f32.min(ui.available_width() - 32.0);
    ui.vertical_centered(|ui| {
        ui.add_space(((ui.available_height() - 480.0) / 2.0).max(16.0));
        Frame::new()
            .fill(p.bg_surface)
            .stroke(Stroke::new(1.0, p.border))
            .corner_radius(theme::RADIUS_LARGE)
            .inner_margin(Margin::same(32))
            .show(ui, |ui| {
                ui.set_width(width);
                ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                ui.vertical_centered(|ui| {
                    ui.label(icons::icon_sized(icons::RSS, 32.0).color(p.accent));
                    ui.add_space(8.0);
                    ui.label(fonts::styled(
                        RichText::new("Welcome to ThreadLiner").size(22.0).color(p.text_primary),
                        fonts::Style::BOLD,
                    ));
                    dialog::muted_text(ui, "Let's set up your data storage.");
                });
                ui.add_space(20.0);
                ui.with_layout(Layout::top_down(Align::Min), |ui| {
                    dialog::field_label(ui, "Data Folder");
                    dialog::muted_text(
                        ui,
                        "Choose a local folder where ThreadLiner will store your feeds and settings.",
                    );
                    ui.horizontal(|ui| {
                        let browse_w = 100.0;
                        ui.allocate_ui(vec2(ui.available_width() - browse_w - 8.0, 34.0), |ui| {
                            dialog::text_input(ui, &mut form.data_dir, "/path/to/threadliner-data", !form.busy);
                        });
                        if dialog::button(ui, "Browse...", ButtonKind::Secondary, !form.busy).clicked() {
                            out.push(ViewAction::Browse);
                        }
                    });
                    ui.add_space(16.0);
                    dialog::field_label(ui, "Git Remote URL (optional)");
                    dialog::muted_text(
                        ui,
                        "SSH URL for syncing across devices. Leave blank for local-only use.",
                    );
                    dialog::text_input(
                        ui,
                        &mut form.remote_url,
                        "git@github.com:user/threadliner-data.git",
                        !form.busy,
                    );
                    if let Some(e) = &form.error {
                        ui.add_space(8.0);
                        dialog::error_text(ui, e);
                    }
                    dialog::actions(ui, |ui| {
                        let label = if form.busy { "Setting up..." } else { "Get Started" };
                        let ready = !form.busy && !form.data_dir.trim().is_empty();
                        if dialog::button(ui, label, ButtonKind::Primary, ready).clicked() {
                            out.push(ViewAction::Setup);
                        }
                        if form.busy {
                            ui.spinner();
                        }
                    });
                });
            });
    });
    out
}

/// The loading screen: the RSS glyph, muted, in the middle.
pub fn loading(ui: &mut Ui) {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        icons::RSS,
        FontId::new(42.0, egui::FontFamily::Name(icons::FAMILY.into())),
        p.text_muted,
    );
}
