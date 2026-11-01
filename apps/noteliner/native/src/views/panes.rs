//! The sidebar's SEARCH, BACKLINKS and TAGS panes, and the FILES pane's tag
//! filter: `SearchPane.svelte`, `BacklinksPane.svelte`, `TagsPane.svelte`
//! and `TagFilterPopover.svelte`.
//!
//! The panes keep their own view state in [`PaneStates`]; the app owns it,
//! feeds it backend results ([`SearchState::results_arrived`],
//! [`BacklinksState::arrived`]) and applies the [`PaneAction`]s.

use std::collections::HashSet;

use egui::{pos2, vec2, Align2, Color32, FontId, Id, Key, Rect, RichText, Sense, Stroke, Ui};
use laankit_egui::panes;
use laankit_egui::theme::{self, Palette};
use laankit_egui::{fonts, icons};
use serde_json::Value;

use crate::model::{self, Entry, UNTAGGED_KEY};
use crate::svg_colors;
use crate::views::tree;

/// `SearchPane`'s debounce before `search:query`.
pub const SEARCH_DEBOUNCE_SECS: f64 = 0.3;
const ROW_HEIGHT: f32 = 22.0;

/// What the panes ask the app to do.
#[derive(Clone, Debug, PartialEq)]
pub enum PaneAction {
    /// Open this note and put the caret on `line` (1-based).
    GoTo {
        id: String,
        line: usize,
    },
    /// `search:query` for this (trimmed) query.
    Search(String),
    /// TAGS chip: put the tag on the selected note, or take it off.
    ToggleTag(String),
    /// TAGS: a new tag for the selected note.
    AddTag(String),
    /// A note was dragged from FILES onto a tag chip.
    TagDropped {
        file: String,
        tag: String,
    },
    OpenTagEditor,
    /// TAGS `+`: show the "Tag name..." input.
    StartAddTag,
    /// The tag filter: check / uncheck a row ([`UNTAGGED_KEY`] for "No Tags").
    FilterToggle(String),
    FilterShowAll,
    FilterHideAll,
}

/// The panes' view state, kept across frames.
#[derive(Default)]
pub struct PaneStates {
    pub search: SearchState,
    pub backlinks: BacklinksState,
    pub tags: TagsState,
    /// The tag filter popover is open.
    pub filter_open: bool,
}

// --- SEARCH -------------------------------------------------------------------

/// One file's matches (`search:query`'s `{ fileId, fileName, matches }`).
#[derive(Clone, Debug, PartialEq)]
pub struct SearchGroup {
    pub file_id: String,
    pub file_name: String,
    /// (1-based line, the line's text).
    pub matches: Vec<(usize, String)>,
}

impl SearchGroup {
    pub fn from_value(v: &Value) -> Option<Self> {
        Some(Self {
            file_id: v["fileId"].as_str()?.to_string(),
            file_name: v["fileName"].as_str().unwrap_or_default().to_string(),
            matches: v["matches"]
                .as_array()?
                .iter()
                .filter_map(|m| {
                    Some((
                        m["line"].as_u64()? as usize,
                        m["text"].as_str().unwrap_or_default().to_string(),
                    ))
                })
                .collect(),
        })
    }
}

#[derive(Default)]
pub struct SearchState {
    pub query: String,
    pub results: Vec<SearchGroup>,
    /// A search is pending or in flight: "Searching...".
    pub searching: bool,
    /// When the query last changed, until its search is sent.
    changed_at: Option<f64>,
    /// The trimmed query the shown results (or the request in flight) are for.
    sent: String,
    /// Focus the input next frame (`focusRequest`).
    pub focus: bool,
}

impl SearchState {
    /// Results for `query`; ignored when the query has moved on, as the
    /// Svelte pane only takes results that still match.
    pub fn results_arrived(&mut self, query: &str, results: &[Value]) {
        if self.query.trim() != query {
            return;
        }
        self.results = results.iter().filter_map(SearchGroup::from_value).collect();
        self.searching = false;
    }

    /// Sets the query as if typed at `now` (`LAANKIT_OPEN=search:<query>`).
    pub fn set_query(&mut self, query: &str, now: f64) {
        self.query = query.to_string();
        self.edited(now);
    }

    /// The query changed at `now`: clear or schedule a search.
    fn edited(&mut self, now: f64) {
        if self.query.trim().is_empty() {
            self.results.clear();
            self.searching = false;
            self.changed_at = None;
            self.sent.clear();
        } else {
            self.searching = true;
            self.changed_at = Some(now);
        }
    }

    /// The debounced search to send at `now`, if one is due.
    pub fn due(&mut self, now: f64) -> Option<String> {
        let at = self.changed_at?;
        if now - at < SEARCH_DEBOUNCE_SECS {
            return None;
        }
        self.changed_at = None;
        let q = self.query.trim().to_string();
        if q == self.sent && !self.results.is_empty() {
            self.searching = false;
            return None;
        }
        self.sent = q.clone();
        Some(q)
    }

    /// `totalMatches`.
    pub fn total_matches(&self) -> usize {
        self.results.iter().map(|g| g.matches.len()).sum()
    }
}

/// `SearchPane.svelte`.
pub fn search_pane(ui: &mut Ui, state: &mut SearchState, out: &mut Vec<PaneAction>) {
    let p = theme::palette(ui.ctx());
    let now = ui.input(|i| i.time);
    egui::Frame::NONE
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(icons::icon_sized(icons::MAGNIFYING_GLASS, 11.0).color(p.text_muted));
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut state.query)
                        .hint_text("Search all files...")
                        .desired_width(f32::INFINITY)
                        .font(FontId::proportional(13.0)),
                );
                labeled_input(&resp, &state.query, "Search all files");
                if state.focus {
                    state.focus = false;
                    resp.request_focus();
                    select_all(ui, &resp, &state.query);
                }
                if resp.changed() {
                    state.edited(now);
                }
            });
        });
    if let Some(q) = state.due(now) {
        out.push(PaneAction::Search(q));
    }
    if state.changed_at.is_some() {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs_f64(SEARCH_DEBOUNCE_SECS));
    }
    let q = state.query.trim().to_string();
    if !q.is_empty() && !state.searching && !state.results.is_empty() {
        let n = state.total_matches();
        let files = state.results.len();
        ui.horizontal(|ui| {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "{n} match{} in {files} file{}",
                    if n == 1 { "" } else { "es" },
                    if files == 1 { "" } else { "s" }
                ))
                .size(11.0)
                .color(p.text_muted),
            );
        });
    }
    egui::ScrollArea::vertical()
        .id_salt("noteliner-search-results")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if state.searching {
                status(ui, &p, "Searching...");
                return;
            }
            if !q.is_empty() && state.results.is_empty() {
                status(ui, &p, "No results");
                return;
            }
            for group in &state.results {
                group_header(
                    ui,
                    &p,
                    icons::FILE_LINES,
                    &group.file_name,
                    Some(group.matches.len()),
                );
                for (line, text) in &group.matches {
                    if line_row(
                        ui,
                        &p,
                        &format!("{line}"),
                        text,
                        Some(&q),
                        (&group.file_id, *line),
                    ) {
                        out.push(PaneAction::GoTo {
                            id: group.file_id.clone(),
                            line: *line,
                        });
                    }
                }
            }
        });
}

/// Selects the whole query, as `inputEl.select()` does when Ctrl+F refocuses it.
fn select_all(ui: &Ui, resp: &egui::Response, text: &str) {
    if let Some(mut s) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
        let all = egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        s.cursor.set_char_range(Some(all));
        s.store(ui.ctx(), resp.id);
    }
}

// --- BACKLINKS ------------------------------------------------------------------

/// One note linking here (`links:getBacklinks`' `{ sourceId, sourceName, matches }`).
#[derive(Clone, Debug, PartialEq)]
pub struct Backlink {
    pub source_id: String,
    pub source_name: String,
    pub matches: Vec<(usize, String)>,
}

impl Backlink {
    pub fn from_value(v: &Value) -> Option<Self> {
        let group = SearchGroup::from_value(&serde_json::json!({
            "fileId": v["sourceId"],
            "fileName": v["sourceName"],
            "matches": v["matches"],
        }))?;
        Some(Self {
            source_id: group.file_id,
            source_name: group.file_name,
            matches: group.matches,
        })
    }
}

#[derive(Default)]
pub struct BacklinksState {
    /// The note and file count the list was last requested for: it
    /// refreshes when the selection or the set of files changes.
    requested: Option<(String, usize)>,
    pub entries: Vec<Backlink>,
    pub loading: bool,
}

impl BacklinksState {
    /// The note to ask `links:getBacklinks` for now, if the list is stale.
    pub fn stale(&mut self, selected: Option<&str>, files: usize) -> Option<String> {
        let Some(id) = selected else {
            self.requested = None;
            self.entries.clear();
            self.loading = false;
            return None;
        };
        let key = (id.to_string(), files);
        if self.requested.as_ref() == Some(&key) {
            return None;
        }
        if self.requested.as_ref().is_some_and(|(i, _)| i != id) {
            self.entries.clear();
        }
        self.requested = Some(key);
        self.loading = true;
        Some(id.to_string())
    }

    /// Forget the list so it's fetched again (after a save changed links).
    pub fn invalidate(&mut self) {
        self.requested = None;
    }

    pub fn arrived(&mut self, id: &str, links: &[Value]) {
        if self.requested.as_ref().map(|(i, _)| i.as_str()) != Some(id) {
            return;
        }
        self.entries = links.iter().filter_map(Backlink::from_value).collect();
        self.loading = false;
    }
}

/// `BacklinksPane.svelte`.
pub fn backlinks_pane(
    ui: &mut Ui,
    state: &BacklinksState,
    has_selection: bool,
    out: &mut Vec<PaneAction>,
) {
    let p = theme::palette(ui.ctx());
    egui::ScrollArea::vertical()
        .id_salt("noteliner-backlinks")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            if !has_selection {
                status(ui, &p, "Select a note to see its backlinks.");
            } else if state.loading && state.entries.is_empty() {
                status(ui, &p, "Loading…");
            } else if state.entries.is_empty() {
                status(ui, &p, "No notes link here yet.");
            } else {
                for entry in &state.entries {
                    group_header(ui, &p, icons::LINK, &entry.source_name, None);
                    for (line, text) in &entry.matches {
                        let resp = line_row(
                            ui,
                            &p,
                            &format!("L{line}"),
                            text,
                            None,
                            (&entry.source_id, *line),
                        );
                        if resp {
                            out.push(PaneAction::GoTo {
                                id: entry.source_id.clone(),
                                line: *line,
                            });
                        }
                    }
                }
            }
        });
}

// --- TAGS -----------------------------------------------------------------------

#[derive(Default)]
pub struct TagsState {
    /// The "Tag name..." input, while adding.
    pub adding: Option<String>,
    focus_add: bool,
    /// The note the add was started for (a selection change cancels it).
    adding_for: Option<String>,
}

impl TagsState {
    /// The `+` button or Ctrl+T (`tagAction: 'add'`).
    pub fn start_add(&mut self, selected: Option<&str>) {
        if let Some(id) = selected {
            self.adding = Some(String::new());
            self.focus_add = true;
            self.adding_for = Some(id.to_string());
        }
    }
}

/// What the TAGS pane and the tag filter draw from.
pub struct TagProps<'a> {
    pub index: &'a Value,
    pub entries: &'a [Entry],
    pub all_tags: &'a [String],
    pub selected: Option<&'a str>,
    /// A note being dragged from the FILES tree (for dropping on a chip).
    pub dragging: Option<&'a str>,
}

/// `TagsPane.svelte`: every tag as a chip, checked when it's on the
/// selected note; clicking toggles it. Notes dragged from FILES onto a chip
/// get that tag.
pub fn tags_pane(
    ui: &mut Ui,
    state: &mut TagsState,
    props: &TagProps<'_>,
    out: &mut Vec<PaneAction>,
) {
    let p = theme::palette(ui.ctx());
    if state.adding_for.as_deref() != props.selected {
        state.adding = None;
        state.adding_for = None;
    }
    let file_tags: Vec<String> = props
        .selected
        .and_then(|id| props.entries.iter().find(|e| e.id == id))
        .map(|e| e.tags.clone())
        .unwrap_or_default();
    let released = ui.input(|i| i.pointer.any_released());
    egui::ScrollArea::vertical()
        .id_salt("noteliner-tags")
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                        if let Some(value) = &mut state.adding {
                            let resp = ui.add(
                                egui::TextEdit::singleline(value)
                                    .hint_text("Tag name...")
                                    .desired_width(140.0)
                                    .font(FontId::proportional(12.0)),
                            );
                            labeled_input(&resp, value, "New tag name");
                            if state.focus_add {
                                state.focus_add = false;
                                resp.request_focus();
                            }
                            let enter = ui.input(|i| i.key_pressed(Key::Enter));
                            let escape = ui.input(|i| i.key_pressed(Key::Escape));
                            if resp.lost_focus() || escape {
                                // `commitAdd` on Enter; Escape or a blur cancels.
                                if enter && !escape && !value.trim().is_empty() {
                                    out.push(PaneAction::AddTag(value.trim().to_string()));
                                }
                                state.adding = None;
                                state.adding_for = None;
                            }
                        }
                        for tag in props.all_tags {
                            let active = file_tags.contains(tag);
                            let swatch = svg_colors::lookup(model::tag_color(props.index, tag));
                            let resp = chip(ui, &p, tag, active, props.selected.is_some(), swatch);
                            if resp.clicked() {
                                out.push(PaneAction::ToggleTag(tag.clone()));
                            }
                            if let Some(dragged) = props.dragging {
                                let over = ui
                                    .ctx()
                                    .pointer_interact_pos()
                                    .is_some_and(|pos| resp.rect.contains(pos));
                                if over {
                                    ui.painter().rect_stroke(
                                        resp.rect.expand(1.0),
                                        theme::RADIUS,
                                        Stroke::new(2.0, p.accent),
                                        egui::StrokeKind::Outside,
                                    );
                                    if released {
                                        out.push(PaneAction::TagDropped {
                                            file: dragged.to_string(),
                                            tag: tag.clone(),
                                        });
                                    }
                                }
                            }
                        }
                        if props.all_tags.is_empty() && state.adding.is_none() {
                            ui.label(RichText::new("No tags").size(12.0).color(p.text_muted));
                        }
                    });
                });
        });
}

/// A tag chip: check box and name, in the tag's colour when it has one
/// (full swatch when on the note, a 30% wash when not).
fn chip(
    ui: &mut Ui,
    p: &Palette,
    tag: &str,
    active: bool,
    enabled: bool,
    swatch: Option<&svg_colors::Swatch>,
) -> egui::Response {
    let font = FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(tag.to_string(), font, Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 34.0, 24.0);
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    let label = if !enabled {
        "Select a file to edit tags".to_string()
    } else if active {
        format!("Remove \"{tag}\" from this file")
    } else {
        format!("Add \"{tag}\" to this file")
    };
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, active, tag));
    let resp = resp.on_hover_text(label);
    let (fill, ink) = match (swatch, active) {
        (Some(s), true) => (s.color(), s.ink()),
        (Some(s), false) => (s.color().gamma_multiply(0.3), p.text_primary),
        (None, true) => (p.bg_selected, p.accent),
        (None, false) => (p.tag_bg, p.text_secondary),
    };
    let alpha = if enabled { 1.0 } else { 0.5 };
    let hovered = enabled && resp.hovered();
    ui.painter().rect(
        rect,
        theme::RADIUS,
        fill.gamma_multiply(alpha),
        if active {
            Stroke::new(1.0, swatch.map_or(p.accent, |s| s.color()))
        } else if hovered {
            Stroke::new(1.0, p.border_hover)
        } else {
            Stroke::NONE
        },
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        pos2(rect.left() + 13.0, rect.center().y),
        Align2::CENTER_CENTER,
        if active {
            icons::SQUARE_CHECK
        } else {
            icons::SQUARE
        },
        icons::font(11.0),
        ink.gamma_multiply(if active { alpha } else { alpha * 0.6 }),
    );
    ui.painter().galley(
        pos2(rect.left() + 24.0, rect.center().y - galley.size().y / 2.0),
        galley,
        ink.gamma_multiply(alpha),
    );
    resp
}

/// The TAGS header: edit colours (`onOpenTagEditor`) and add (`onTagAction('add')`).
pub fn tags_header(ui: &mut Ui, has_tags: bool, has_selection: bool, out: &mut Vec<PaneAction>) {
    let p = theme::palette(ui.ctx());
    if header_button(ui, &p, "tags-add", icons::PLUS, "Add Tag", has_selection) {
        out.push(PaneAction::StartAddTag);
    }
    if header_button(ui, &p, "tags-edit", icons::PEN, "Edit tag colors", has_tags) {
        out.push(PaneAction::OpenTagEditor);
    }
}

// --- The FILES tag filter ---------------------------------------------------------

/// The FILES header's filter button, with the count of unchecked rows, and
/// its popover (`TagFilterPopover.svelte`).
pub fn filter_button(
    ui: &mut Ui,
    open: &mut bool,
    hidden: &HashSet<String>,
    props: &TagProps<'_>,
    out: &mut Vec<PaneAction>,
) {
    let p = theme::palette(ui.ctx());
    let untagged = model::untagged_count(props.entries);
    let enabled = !props.all_tags.is_empty() || untagged > 0;
    let (rect, _) = ui.allocate_exact_size(vec2(26.0, 20.0), Sense::hover());
    let resp = panes::header_button(
        ui,
        &p,
        Id::unique("noteliner-tag-filter-btn"),
        rect,
        icons::FILTER,
        "Filter by tags",
    );
    if !hidden.is_empty() {
        ui.painter()
            .rect_filled(rect.shrink(1.0), theme::RADIUS, p.bg_selected);
        ui.painter().text(
            rect.center() - vec2(4.0, 0.0),
            Align2::CENTER_CENTER,
            icons::FILTER,
            icons::font(11.0),
            p.accent,
        );
        ui.painter().text(
            pos2(rect.right() - 4.0, rect.top() + 6.0),
            Align2::CENTER_CENTER,
            hidden.len().to_string(),
            FontId::proportional(9.0),
            p.accent,
        );
    }
    if enabled && resp.clicked() {
        *open = !*open;
    }
    if *open && enabled {
        filter_popover(ui, rect, open, hidden, props, out);
    }
}

fn filter_popover(
    ui: &Ui,
    anchor: Rect,
    open: &mut bool,
    hidden: &HashSet<String>,
    props: &TagProps<'_>,
    out: &mut Vec<PaneAction>,
) {
    let p = theme::palette(ui.ctx());
    let area = egui::Area::new(Id::unique("noteliner-tag-filter"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos2(anchor.left() - 180.0, anchor.bottom() + 4.0))
        .show(ui.ctx(), |ui| {
            egui::Frame::NONE
                .fill(p.bg_surface)
                .stroke(Stroke::new(1.0, p.border))
                .corner_radius(theme::RADIUS)
                .shadow(ui.visuals().popup_shadow)
                .show(ui, |ui| {
                    ui.set_width(220.0);
                    ui.spacing_mut().item_spacing.y = 0.0;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(12, 8))
                        .show(ui, |ui| {
                            ui.label(fonts::weight(
                                RichText::new("Filter by tags")
                                    .size(12.0)
                                    .color(p.text_secondary),
                                fonts::SEMIBOLD,
                            ));
                        });
                    egui::ScrollArea::vertical()
                        .id_salt("noteliner-tag-filter-rows")
                        .max_height(280.0)
                        .show(ui, |ui| {
                            let untagged = model::untagged_count(props.entries);
                            if filter_row(
                                ui,
                                &p,
                                "No Tags",
                                untagged,
                                !hidden.contains(UNTAGGED_KEY),
                            ) {
                                out.push(PaneAction::FilterToggle(UNTAGGED_KEY.to_string()));
                            }
                            for tag in props.all_tags {
                                let n = model::tag_count(props.entries, tag);
                                if filter_row(ui, &p, tag, n, !hidden.contains(tag)) {
                                    out.push(PaneAction::FilterToggle(tag.clone()));
                                }
                            }
                        });
                    ui.separator();
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                if ui.button(RichText::new("Show all").size(12.0)).clicked() {
                                    out.push(PaneAction::FilterShowAll);
                                }
                                if ui.button(RichText::new("Hide all").size(12.0)).clicked() {
                                    out.push(PaneAction::FilterHideAll);
                                }
                            });
                        });
                });
        });
    // Escape or a click outside closes it (the button toggles it itself).
    let escape = ui.input(|i| i.key_pressed(Key::Escape));
    let clicked_outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|pos| !area.response.rect.contains(pos) && !anchor.contains(pos))
    });
    if escape || clicked_outside {
        *open = false;
    }
}

/// A popover row: check box, label and count; returns whether it was clicked.
fn filter_row(ui: &mut Ui, p: &Palette, label: &str, count: usize, checked: bool) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::CheckBox,
            true,
            checked,
            format!("Filter: {label}"),
        )
    });
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    let ink = if checked {
        p.text_primary
    } else {
        p.text_muted
    };
    ui.painter().text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::CENTER_CENTER,
        if checked {
            icons::SQUARE_CHECK
        } else {
            icons::SQUARE
        },
        icons::font(12.0),
        if checked { p.accent } else { p.text_faint },
    );
    let text = tree::elide(ui, label, FontId::proportional(13.0), rect.width() - 80.0);
    ui.painter().text(
        pos2(rect.left() + 32.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        ink,
    );
    ui.painter().text(
        pos2(rect.right() - 12.0, rect.center().y),
        Align2::RIGHT_CENTER,
        count.to_string(),
        FontId::proportional(11.0),
        p.text_muted,
    );
    resp.clicked()
}

/// Names a text field for accessibility (and kittest).
fn labeled_input(resp: &egui::Response, value: &str, label: &str) {
    resp.widget_info(|| {
        let mut info = egui::WidgetInfo::text_edit(true, "", value, "");
        info.label = Some(label.to_string());
        info
    });
}

// --- Shared row painting ------------------------------------------------------------

/// A pane header button (`.pane-header-btn`); disabled ones are dimmed and
/// never click.
pub fn header_button(
    ui: &mut Ui,
    p: &Palette,
    id: &str,
    glyph: &str,
    label: &str,
    enabled: bool,
) -> bool {
    let (rect, _) = ui.allocate_exact_size(vec2(22.0, 20.0), Sense::hover());
    if !enabled {
        let resp = ui.interact(rect, Id::unique(("noteliner-pane-btn", id)), Sense::hover());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, false, label));
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            glyph,
            icons::font(11.0),
            p.text_faint,
        );
        return false;
    }
    panes::header_button(
        ui,
        p,
        Id::unique(("noteliner-pane-btn", id)),
        rect,
        glyph,
        label,
    )
    .clicked()
}

fn status(ui: &mut Ui, p: &Palette, text: &str) {
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(12.0);
        ui.label(RichText::new(text).size(12.5).color(p.text_muted));
    });
}

/// A result group's file row: icon, name and an optional count.
fn group_header(ui: &mut Ui, p: &Palette, glyph: &str, name: &str, count: Option<usize>) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT + 2.0), Sense::hover());
    ui.painter().text(
        pos2(rect.left() + 16.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        icons::font(10.0),
        p.text_muted,
    );
    let font = FontId::new(12.5, egui::FontFamily::Name(fonts::SEMIBOLD_FAMILY.into()));
    let right = if count.is_some() { 36.0 } else { 8.0 };
    let text = tree::elide(ui, name, font.clone(), rect.width() - 28.0 - right);
    ui.painter().text(
        pos2(rect.left() + 28.0, rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        p.text_primary,
    );
    if let Some(n) = count {
        ui.painter().text(
            pos2(rect.right() - 12.0, rect.center().y),
            Align2::RIGHT_CENTER,
            n.to_string(),
            FontId::proportional(11.0),
            p.text_muted,
        );
    }
}

/// A clickable match line: the line number and the text, with `highlight`
/// marked case-insensitively. Returns whether it was clicked.
fn line_row(
    ui: &mut Ui,
    p: &Palette,
    num: &str,
    text: &str,
    highlight: Option<&str>,
    key: (&str, usize),
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::click());
    let label = format!("Line {}: {text}", key.1);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let resp = resp.on_hover_text(format!("Go to line {}", key.1));
    if resp.hovered() {
        ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
    }
    ui.painter().text(
        pos2(rect.left() + 44.0, rect.center().y),
        Align2::RIGHT_CENTER,
        num,
        FontId::monospace(10.5),
        p.text_faint,
    );
    let left = rect.left() + 52.0;
    let width = (rect.right() - left - 6.0).max(10.0);
    let job = highlighted(text.trim(), highlight, p);
    let mut job = job;
    job.wrap.max_width = width;
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');
    let galley = ui.painter().layout_job(job);
    ui.painter().galley(
        pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_secondary,
    );
    resp.clicked()
}

/// `highlightMatch`: every case-insensitive occurrence of `q` marked.
pub fn highlighted(text: &str, q: Option<&str>, p: &Palette) -> egui::text::LayoutJob {
    let plain = egui::TextFormat {
        font_id: FontId::proportional(12.0),
        color: p.text_secondary,
        ..Default::default()
    };
    let mark = egui::TextFormat {
        background: p.accent.gamma_multiply(0.3),
        color: p.text_primary,
        ..plain.clone()
    };
    let mut job = egui::text::LayoutJob::default();
    let ranges = match q {
        Some(q) if !q.is_empty() => match_ranges(text, q),
        _ => Vec::new(),
    };
    let mut at = 0;
    for (start, end) in ranges {
        job.append(&text[at..start], 0.0, plain.clone());
        job.append(&text[start..end], 0.0, mark.clone());
        at = end;
    }
    job.append(&text[at..], 0.0, plain);
    job
}

/// Byte ranges of `q` in `text`, ignoring case (char-boundary safe).
pub fn match_ranges(text: &str, q: &str) -> Vec<(usize, usize)> {
    let needle: Vec<char> = q.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let mut j = i;
        let mut k = 0;
        while j < chars.len() && k < needle.len() {
            let lower: Vec<char> = chars[j].1.to_lowercase().collect();
            if needle[k..].starts_with(&lower) {
                k += lower.len();
                j += 1;
            } else {
                break;
            }
        }
        if k == needle.len() {
            let end = chars.get(j).map_or(text.len(), |c| c.0);
            out.push((chars[i].0, end));
            i = j.max(i + 1);
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn search_debounce_and_stale_results() {
        let mut s = SearchState {
            query: "rust".into(),
            ..Default::default()
        };
        s.edited(1.0);
        assert!(s.searching);
        assert_eq!(s.due(1.1), None);
        assert_eq!(s.due(1.31).as_deref(), Some("rust"));
        assert_eq!(s.due(2.0), None, "sent once");
        s.results_arrived(
            "old",
            &[json!({ "fileId": "a", "fileName": "A", "matches": [] })],
        );
        assert!(
            s.results.is_empty() && s.searching,
            "stale results are ignored"
        );
        s.results_arrived(
            "rust",
            &[json!({ "fileId": "a", "fileName": "A", "matches": [{ "line": 3, "text": "Rust!" }] })],
        );
        assert!(!s.searching);
        assert_eq!(s.total_matches(), 1);
        s.query = "  ".into();
        s.edited(3.0);
        assert!(s.results.is_empty() && !s.searching);
    }

    #[test]
    fn backlinks_refresh_on_selection_and_files() {
        let mut b = BacklinksState::default();
        assert_eq!(b.stale(Some("a"), 3).as_deref(), Some("a"));
        assert_eq!(b.stale(Some("a"), 3), None);
        b.arrived(
            "a",
            &[json!({ "sourceId": "b", "sourceName": "B", "matches": [{ "line": 2, "text": "[[A]]" }] })],
        );
        assert_eq!(b.entries.len(), 1);
        assert!(!b.loading);
        assert_eq!(
            b.stale(Some("a"), 4).as_deref(),
            Some("a"),
            "a file was added"
        );
        assert_eq!(b.entries.len(), 1, "kept while refreshing the same note");
        assert_eq!(b.stale(Some("c"), 4).as_deref(), Some("c"));
        assert!(b.entries.is_empty());
        b.arrived("a", &[]);
        assert!(b.loading, "results for another note are ignored");
        assert_eq!(b.stale(None, 4), None);
    }

    #[test]
    fn highlight_ranges_ignore_case() {
        assert_eq!(match_ranges("Rust and rust", "RUST"), [(0, 4), (9, 13)]);
        assert_eq!(match_ranges("Über über", "üb"), [(0, 3), (6, 9)]);
        assert!(match_ranges("abc", "").is_empty());
    }
}
