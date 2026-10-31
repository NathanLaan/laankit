//! The History panel (`HistoryPanel.svelte`): the selected note's git
//! commits (`file:getHistory`); picking one shows the note as it was at
//! that commit, rendered like the Preview (`file:getHistoryContent`). It's
//! read-only, as in the Svelte build.

use egui::{pos2, vec2, Align2, FontId, Frame, Margin, RichText, Sense, Ui};
use laankit_core::git::LogEntry;
use laankit_egui::theme::{self, Palette};
use laankit_egui::{fonts, icons};
use laankit_richtext::{Action, Document, RichStyle, RichView};

use crate::views::preview::{self, AttachmentResolver};

/// `commitListHeight`'s default and floor.
const LIST_HEIGHT: f32 = 200.0;
const MIN_LIST_HEIGHT: f32 = 60.0;
const COMMIT_HEIGHT: f32 = 52.0;

/// What the panel asks the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum HistoryAction {
    Close,
    /// `file:getHistory` for this note file.
    Load(String),
    /// `file:getHistoryContent` for this commit of the note file.
    LoadContent {
        hash: String,
        filename: String,
    },
    /// A link in the old version, for the browser.
    OpenUrl(String),
}

pub struct HistoryState {
    /// The note file the list is for (requested or loaded).
    filename: Option<String>,
    pub commits: Vec<LogEntry>,
    pub loading: bool,
    pub selected: Option<String>,
    content_loading: bool,
    doc: Option<Document>,
    view: RichView,
    list_height: f32,
}

impl Default for HistoryState {
    fn default() -> Self {
        Self {
            filename: None,
            commits: Vec::new(),
            loading: false,
            selected: None,
            content_loading: false,
            doc: None,
            view: RichView::default(),
            list_height: LIST_HEIGHT,
        }
    }
}

impl HistoryState {
    /// The selected note changed (or was renamed): reload for `filename`.
    /// Returns the `Load` to send, if the list is stale.
    pub fn follow(&mut self, filename: Option<&str>) -> Option<HistoryAction> {
        if self.filename.as_deref() == filename {
            return None;
        }
        self.filename = filename.map(str::to_string);
        self.commits.clear();
        self.selected = None;
        self.doc = None;
        self.content_loading = false;
        self.loading = filename.is_some();
        filename.map(|f| HistoryAction::Load(f.to_string()))
    }

    /// Forget the list so it's fetched again (the panel was reopened).
    pub fn invalidate(&mut self) {
        self.filename = None;
    }

    pub fn loaded(&mut self, filename: &str, commits: Vec<LogEntry>) {
        if self.filename.as_deref() != Some(filename) {
            return;
        }
        self.commits = commits;
        self.loading = false;
    }

    pub fn content_loaded(&mut self, hash: &str, content: Option<String>) {
        if self.selected.as_deref() != Some(hash) {
            return;
        }
        self.content_loading = false;
        self.doc = content.map(|c| preview::document(&c));
        self.view.reset();
    }

    #[cfg(test)]
    pub fn has_content(&self) -> bool {
        self.doc.is_some()
    }
}

/// `formatDate`: local `YYYY-MM-DD HH:MM`.
pub fn format_date(date: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(date)
        .or_else(|_| chrono::DateTime::parse_from_str(date, "%Y-%m-%d %H:%M:%S %z"))
        .map(|d| {
            d.with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string()
        })
        .unwrap_or_else(|_| date.to_string())
}

pub fn show(
    ui: &mut Ui,
    state: &mut HistoryState,
    attachments: Option<std::path::PathBuf>,
) -> Vec<HistoryAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    egui::Panel::top(ui.scope_id().with("history-toolbar"))
        .frame(
            Frame::NONE
                .fill(p.bg_surface)
                .inner_margin(Margin::symmetric(12, 6))
                .stroke(egui::Stroke::new(1.0, p.border)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("HISTORY").size(11.0).color(p.text_muted));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let close = ui
                        .add(egui::Button::new(icons::icon_sized(icons::XMARK, 12.0)).frame(false))
                        .on_hover_text("Close History");
                    close.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, "Close History")
                    });
                    if close.clicked() {
                        out.push(HistoryAction::Close);
                    }
                });
            });
        });

    // The commit list, then a draggable divider, then the old version.
    let list_height = state.list_height.clamp(
        MIN_LIST_HEIGHT,
        (ui.available_height() - 80.0).max(MIN_LIST_HEIGHT),
    );
    egui::Panel::top(ui.scope_id().with("history-commits"))
        .exact_size(list_height)
        .resizable(false)
        .frame(Frame::NONE.fill(p.bg_surface))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("noteliner-history-commits")
                .auto_shrink(false)
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 0.0;
                    if state.loading && state.commits.is_empty() {
                        message(ui, &p, "Loading...");
                    } else if state.commits.is_empty() {
                        message(ui, &p, "No history");
                    }
                    for commit in &state.commits {
                        let selected = state.selected.as_deref() == Some(commit.hash.as_str());
                        if commit_row(ui, &p, commit, selected) && !selected {
                            state.selected = Some(commit.hash.clone());
                            state.content_loading = true;
                            state.doc = None;
                            if let Some(filename) = &state.filename {
                                out.push(HistoryAction::LoadContent {
                                    hash: commit.hash.clone(),
                                    filename: filename.clone(),
                                });
                            }
                        }
                    }
                });
        });
    let (divider, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 5.0), Sense::drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Splitter, true, "Resize history"));
    let resp = resp.on_hover_cursor(egui::CursorIcon::ResizeVertical);
    if resp.dragged() {
        state.list_height = (list_height + resp.drag_delta().y).max(MIN_LIST_HEIGHT);
    }
    ui.painter().hline(
        divider.x_range(),
        divider.center().y,
        egui::Stroke::new(1.0, if resp.hovered() { p.accent } else { p.border }),
    );

    Frame::NONE
        .fill(p.bg_base)
        .inner_margin(Margin::symmetric(16, 8))
        .show(ui, |ui| {
            ui.set_min_size(ui.available_size());
            if state.content_loading {
                message(ui, &p, "Loading...");
                return;
            }
            let Some(doc) = &state.doc else {
                message(ui, &p, "Select a commit to preview");
                return;
            };
            let mut resolver = AttachmentResolver { dir: attachments };
            let style = RichStyle::from_ctx(ui.ctx());
            for a in state.view.show(ui, doc, &style, &mut resolver).actions {
                let Action::Open(target) = a;
                if !target.starts_with(preview::WIKILINK_SCHEME) {
                    out.push(HistoryAction::OpenUrl(target));
                }
            }
        });
    out
}

fn message(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(12.5).color(p.text_muted));
    });
}

/// `.commit-item`: date and author on top, the message below.
fn commit_row(ui: &mut Ui, p: &Palette, c: &LogEntry, selected: bool) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), COMMIT_HEIGHT), Sense::click());
    let date = format_date(&c.date);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            true,
            selected,
            format!("{date} {}: {}", c.author, c.message),
        )
    });
    if selected {
        ui.painter().rect_filled(rect, 0.0, p.bg_selected);
        ui.painter().vline(
            rect.left() + 1.0,
            rect.y_range(),
            egui::Stroke::new(2.0, p.accent),
        );
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    ui.painter().hline(
        rect.x_range(),
        rect.bottom() - 0.5,
        egui::Stroke::new(1.0, p.border),
    );
    let mono = FontId::monospace(11.0);
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 14.0),
        Align2::LEFT_CENTER,
        &date,
        mono,
        p.text_secondary,
    );
    let author_font = FontId::proportional(11.0);
    let author = crate::views::tree::elide(ui, &c.author, author_font.clone(), rect.width() / 2.0);
    ui.painter().text(
        pos2(rect.right() - 12.0, rect.top() + 14.0),
        Align2::RIGHT_CENTER,
        author,
        author_font,
        p.text_muted,
    );
    let msg_font = FontId::new(12.5, egui::FontFamily::Proportional);
    let msg = crate::views::tree::elide(ui, &c.message, msg_font.clone(), rect.width() - 24.0);
    ui.painter().text(
        pos2(rect.left() + 12.0, rect.top() + 34.0),
        Align2::LEFT_CENTER,
        msg,
        if selected {
            FontId::new(12.5, egui::FontFamily::Name(fonts::SEMIBOLD_FAMILY.into()))
        } else {
            msg_font
        },
        p.text_primary,
    );
    resp.clicked()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn commit(hash: &str) -> LogEntry {
        LogEntry {
            hash: hash.into(),
            date: "2026-10-17T09:05:00-06:00".into(),
            author: "Me".into(),
            message: "Edit".into(),
        }
    }

    #[test]
    fn follows_the_selection() {
        let mut h = HistoryState::default();
        assert_eq!(
            h.follow(Some("a.md")),
            Some(HistoryAction::Load("a.md".into()))
        );
        assert_eq!(h.follow(Some("a.md")), None);
        assert!(h.loading);
        h.loaded("b.md", vec![commit("x")]);
        assert!(h.commits.is_empty(), "a list for another note is ignored");
        h.loaded("a.md", vec![commit("x"), commit("y")]);
        assert_eq!(h.commits.len(), 2);
        h.selected = Some("y".into());
        h.content_loaded("x", Some("old".into()));
        assert!(!h.has_content());
        h.content_loaded("y", Some("# Old\n".into()));
        assert!(h.has_content());
        assert_eq!(h.follow(None), None);
        assert!(h.commits.is_empty() && h.selected.is_none());
    }

    #[test]
    fn dates_like_format_date() {
        let d = format_date("2026-10-17T09:05:00+00:00");
        assert_eq!(d.len(), 16, "{d}");
        assert!(d.starts_with("2026-10-1"));
        assert_eq!(format_date("garbage"), "garbage");
    }
}
