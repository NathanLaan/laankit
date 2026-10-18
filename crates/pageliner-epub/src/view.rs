//! [`EpubView`]: the reading screen (`EpubReader.svelte`). A toolbar, a sidebar with the
//! Contents and Annotations panes, and the book in one continuous scroll.
//!
//! The view doesn't do I/O. [`EpubView::show`] returns [`EpubEvent`]s the host acts on: patches
//! to persist (per-book state, reading preferences), links to open, the status text, and closing.
//!
//! Paging: Page Down / Space / → and Page Up / ← move a screen at a time, keeping a little of
//! the previous one, across chapter boundaries (the book is one document). egui has no paginated
//! text layout, and scrolling keeps positions simple (spine + block + character offset).

use std::sync::Arc;

use egui::{vec2, Align2, Color32, Frame, Key, Margin, Rect, RichText, Sense, Stroke, Ui};
use marina_egui::panes::{self, Pane, PaneLayout};
use marina_egui::{icons, theme};
use marina_richtext::resolve::{data_uri, LinkAction, Resolver};
use marina_richtext::{Document, Face, Mark, Position, RichOutput, RichStyle, RichView};
use serde_json::Value;

use crate::annotations::{self, AnnotationAction};
use crate::book::Book;
use crate::state::{
    Bookmark, Highlight, Location, NativeState, ReaderPrefs, ReadingTheme, FONT_SIZE_MAX,
    FONT_SIZE_MIN, FONT_SIZE_STEP,
};

/// The position is saved this long after it last changed (`saveProgress`'s debounce).
const SAVE_DELAY: f64 = 0.5;
/// Paging keeps this much of the previous screen visible.
const PAGE_OVERLAP: f32 = 48.0;
/// The body text size at 100%.
const BASE_SIZE: f32 = 17.0;
/// The reading column's widest width.
const COLUMN: f32 = 680.0;
/// Highlights on the page (`HL_STYLE`: `#f0c800` at 30%).
pub const HIGHLIGHT: Color32 = Color32::from_rgba_unmultiplied_const(240, 200, 0, 77);
/// Highlight snippets are cut to this many characters.
const SNIPPET: usize = 120;
const SIDEBAR_WIDTH: f32 = 260.0;
const EDGE_WIDTH: f32 = 36.0;

/// Something for the host to do.
#[derive(Clone, Debug, PartialEq)]
pub enum EpubEvent {
    /// Back to the library. Persist [`EpubView::flush`] first.
    Close,
    /// Merge into the book's state: `library.set_state(&book_id, &patch)`.
    SaveState(Value),
    /// Merge into the UI preferences (`epubFontSize`, `epubTheme`, `epubFace`).
    SavePrefs(Value),
    /// An external link (`https:`, `mailto:`).
    OpenUrl(String),
    /// The chapter being read changed: the status bar text (`libraryState.readingStatus`).
    Status(String),
}

pub struct EpubView {
    book: Book,
    document: Arc<Document>,
    state: NativeState,
    prefs: ReaderPrefs,
    view: RichView,
    panes: PaneLayout,
    sidebar: bool,
    /// The scroll area last frame: offset, viewport height, content height.
    scroll: (f32, f32, f32),
    /// The position written to disk, and when the current one was first seen unsaved.
    saved: Option<Location>,
    unsaved_since: Option<f64>,
    status: String,
    /// The highlight clicked on the page, shown selected in the Annotations pane.
    active_highlight: Option<String>,
    /// Whether the selection is non-empty (for the Highlight button).
    has_selection: bool,
}

impl EpubView {
    /// A reader for `book`, at the saved position.
    pub fn new(book: Book, state: NativeState, prefs: ReaderPrefs) -> Self {
        let document = book.document.clone();
        let mut view = RichView::default().id_salt(("pageliner-epub", book.id.clone()));
        if let Some(loc) = state.position {
            view.restore(book.position(loc));
        }
        // `EpubReader.svelte`'s `paneOrder` and `paneHeights`.
        let panes = PaneLayout {
            order: vec!["contents".into(), "annotations".into()],
            heights: [("contents".into(), 340.0), ("annotations".into(), 240.0)].into(),
        };
        let mut me = Self {
            document,
            saved: state.position,
            state,
            prefs,
            view,
            panes,
            sidebar: true,
            scroll: (0.0, 0.0, 0.0),
            unsaved_since: None,
            status: String::new(),
            active_highlight: None,
            has_selection: false,
            book,
        };
        me.sync_marks();
        me
    }

    pub fn book(&self) -> &Book {
        &self.book
    }

    /// The rich-text view (selection, marks, layout state), for diagnostics and tests.
    pub fn rich_view(&self) -> &RichView {
        &self.view
    }

    pub fn state(&self) -> &NativeState {
        &self.state
    }

    pub fn prefs(&self) -> ReaderPrefs {
        self.prefs
    }

    /// The sidebar's pane order and heights (the host may persist them).
    pub fn panes(&self) -> &PaneLayout {
        &self.panes
    }

    pub fn set_panes(&mut self, panes: PaneLayout) {
        self.panes = panes;
    }

    /// Where the reader is (the top of the screen).
    pub fn location(&self) -> Location {
        self.book.location(self.view.position())
    }

    /// The state patch to write before closing, if the position moved since the last save.
    pub fn flush(&mut self) -> Option<Value> {
        self.position_patch(true)
    }

    /// Opens at a location (the Contents pane, an annotation, the host's own navigation).
    pub fn go_to(&mut self, loc: Location) {
        self.view.restore(self.book.position(loc));
    }

    /// A screen forward (`true`) or back.
    pub fn page(&mut self, forward: bool) {
        let (offset, viewport, content) = self.scroll;
        let step = (viewport - PAGE_OVERLAP).max(viewport * 0.5);
        let next = if forward {
            (offset + step).min((content - viewport).max(0.0))
        } else {
            (offset - step).max(0.0)
        };
        self.view.scroll_to_offset(next);
    }

    /// Whether the current location is bookmarked (same chapter and block).
    pub fn bookmarked(&self) -> bool {
        let here = self.location();
        self.state
            .bookmarks
            .iter()
            .any(|b| b.location.spine == here.spine && b.location.block == here.block)
    }

    /// Adds or removes a bookmark at the current location. Returns the state patch.
    pub fn toggle_bookmark(&mut self) -> Value {
        let here = self.location();
        if self.bookmarked() {
            self.state
                .bookmarks
                .retain(|b| !(b.location.spine == here.spine && b.location.block == here.block));
        } else {
            let label = self.book.label_at(self.view.position().block);
            self.state.bookmarks.push(Bookmark {
                id: uuid::Uuid::new_v4().to_string(),
                location: here,
                label,
                created_at: now_iso(),
            });
        }
        self.state.patch()
    }

    /// Highlights the selection. Returns the state patch, or `None` when nothing is selected or
    /// that range is already highlighted.
    pub fn highlight_selection(&mut self) -> Option<Value> {
        let sel = self.view.selection(&self.document)?;
        self.view.clear_selection();
        self.has_selection = false;
        let (start, end) = (self.book.location(sel.start), self.book.location(sel.end));
        if self
            .state
            .highlights
            .iter()
            .any(|h| h.start == start && h.end == end)
        {
            return None;
        }
        let text = sel.text.split_whitespace().collect::<Vec<_>>().join(" ");
        let mut snippet: String = text.chars().take(SNIPPET).collect();
        if snippet.is_empty() {
            snippet = "Highlight".into();
        }
        self.state.highlights.push(Highlight {
            id: uuid::Uuid::new_v4().to_string(),
            start,
            end,
            text: snippet,
            created_at: now_iso(),
        });
        self.sync_marks();
        Some(self.state.patch())
    }

    /// Removes a bookmark or highlight by id. Returns the state patch if one was removed.
    pub fn delete_annotation(&mut self, id: &str) -> Option<Value> {
        let before = (self.state.bookmarks.len(), self.state.highlights.len());
        self.state.bookmarks.retain(|b| b.id != id);
        self.state.highlights.retain(|h| h.id != id);
        if before == (self.state.bookmarks.len(), self.state.highlights.len()) {
            return None;
        }
        if self.active_highlight.as_deref() == Some(id) {
            self.active_highlight = None;
        }
        self.sync_marks();
        Some(self.state.patch())
    }

    /// Jumps to a bookmark or highlight by id.
    pub fn jump_to_annotation(&mut self, id: &str) {
        let target = self
            .state
            .bookmarks
            .iter()
            .find(|b| b.id == id)
            .map(|b| b.location)
            .or_else(|| {
                self.state
                    .highlights
                    .iter()
                    .find(|h| h.id == id)
                    .map(|h| h.start)
            });
        if let Some(loc) = target {
            self.go_to(loc);
        }
    }

    /// Draws the reader in the available space.
    pub fn show(&mut self, ui: &mut Ui) -> Vec<EpubEvent> {
        let mut events = Vec::new();
        self.keys(ui, &mut events);

        egui::Panel::top(ui.scope_id().with("epub-toolbar"))
            .frame(
                Frame::NONE
                    .fill(theme::palette(ui.ctx()).bg_base)
                    .inner_margin(Margin::symmetric(12, 8))
                    .stroke(Stroke::new(1.0, theme::palette(ui.ctx()).border)),
            )
            .show(ui, |ui| self.toolbar(ui, &mut events));

        if self.sidebar {
            let p = theme::palette(ui.ctx());
            egui::Panel::left(ui.scope_id().with("epub-sidebar"))
                .resizable(true)
                .default_size(SIDEBAR_WIDTH)
                .frame(Frame::NONE.fill(p.bg_surface))
                .show(ui, |ui| self.sidebar_ui(ui, &mut events));
        }

        let (_, bg) = self.prefs.theme.colors();
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(bg))
            .show(ui, |ui| self.reading_area(ui, &mut events));

        if let Some(patch) = self.position_patch(false) {
            events.push(EpubEvent::SaveState(patch));
        }
        let label = self.book.label_at(self.view.position().block);
        if label != self.status {
            self.status = label.clone();
            events.push(EpubEvent::Status(label));
        }
        events
    }

    /// Paging keys, when no text field has focus.
    fn keys(&mut self, ui: &Ui, events: &mut Vec<EpubEvent>) {
        if ui.ctx().memory(|m| m.focused().is_some()) {
            return;
        }
        let (forward, back) = ui.input(|i| {
            if i.modifiers.any() {
                return (false, false);
            }
            (
                i.key_pressed(Key::PageDown)
                    || i.key_pressed(Key::Space)
                    || i.key_pressed(Key::ArrowRight),
                i.key_pressed(Key::PageUp) || i.key_pressed(Key::ArrowLeft),
            )
        });
        if forward || back {
            self.page(forward);
        }
        let _ = events;
    }

    fn toolbar(&mut self, ui: &mut Ui, events: &mut Vec<EpubEvent>) {
        let p = theme::palette(ui.ctx());
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if tb_button(
                ui,
                Some(icons::ARROW_LEFT),
                "Library",
                "Back to Library",
                false,
                true,
            )
            .clicked()
            {
                if let Some(patch) = self.flush() {
                    events.push(EpubEvent::SaveState(patch));
                }
                events.push(EpubEvent::Close);
            }
            if tb_button(
                ui,
                Some(icons::BARS),
                "",
                "Toggle sidebar",
                self.sidebar,
                true,
            )
            .clicked()
            {
                self.sidebar = !self.sidebar;
            }
            let title = if self.status.is_empty() {
                self.book.title.clone()
            } else {
                self.status.clone()
            };
            ui.add(
                egui::Label::new(
                    RichText::new(title)
                        .color(p.text_secondary)
                        .font(marina_egui::fonts::Style::BOLD.font(13.0)),
                )
                .truncate(),
            )
            .on_hover_text(&self.book.title);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if tb_button(ui, Some(icons::CHEVRON_RIGHT), "", "Next page", false, true).clicked()
                {
                    self.page(true);
                }
                if tb_button(
                    ui,
                    Some(icons::CHEVRON_LEFT),
                    "",
                    "Previous page",
                    false,
                    true,
                )
                .clicked()
                {
                    self.page(false);
                }
                ui.add_space(8.0);
                let mut prefs = self.prefs;
                let face_label = match prefs.face {
                    Face::Serif => "Serif",
                    Face::Sans => "Sans",
                };
                if tb_button(
                    ui,
                    None,
                    face_label,
                    "Switch between serif and sans-serif text",
                    false,
                    true,
                )
                .clicked()
                {
                    prefs.face = match prefs.face {
                        Face::Serif => Face::Sans,
                        Face::Sans => Face::Serif,
                    };
                }
                ui.add_space(4.0);
                egui::ComboBox::from_id_salt("epub-reading-theme")
                    .selected_text(prefs.theme.label())
                    .width(84.0)
                    .show_ui(ui, |ui| {
                        for t in ReadingTheme::ALL {
                            ui.selectable_value(&mut prefs.theme, t, t.label());
                        }
                    })
                    .response
                    .on_hover_text("Reading theme");
                ui.add_space(4.0);
                if tb_button(
                    ui,
                    Some(icons::PLUS),
                    "",
                    "Larger text",
                    false,
                    prefs.font_size < FONT_SIZE_MAX,
                )
                .clicked()
                {
                    prefs.font_size = (prefs.font_size + FONT_SIZE_STEP).min(FONT_SIZE_MAX);
                }
                ui.add_sized(
                    vec2(42.0, 20.0),
                    egui::Label::new(
                        RichText::new(format!("{}%", prefs.font_size))
                            .size(12.0)
                            .color(p.text_secondary),
                    ),
                );
                if tb_button(
                    ui,
                    Some(icons::MINUS),
                    "",
                    "Smaller text",
                    false,
                    prefs.font_size > FONT_SIZE_MIN,
                )
                .clicked()
                {
                    prefs.font_size = prefs
                        .font_size
                        .saturating_sub(FONT_SIZE_STEP)
                        .max(FONT_SIZE_MIN);
                }
                ui.add_space(8.0);
                let bookmarked = self.bookmarked();
                let (title, glyph) = if bookmarked {
                    ("Remove bookmark", icons::BOOKMARK)
                } else {
                    ("Bookmark this location", icons::BOOKMARK)
                };
                if tb_button(ui, Some(glyph), "", title, bookmarked, true).clicked() {
                    events.push(EpubEvent::SaveState(self.toggle_bookmark()));
                }
                if tb_button(
                    ui,
                    Some(icons::HIGHLIGHTER),
                    "",
                    "Highlight selection",
                    self.has_selection,
                    self.has_selection,
                )
                .clicked()
                {
                    if let Some(patch) = self.highlight_selection() {
                        events.push(EpubEvent::SaveState(patch));
                    }
                }
                if prefs != self.prefs {
                    self.prefs = prefs;
                    events.push(EpubEvent::SavePrefs(prefs.patch()));
                }
            });
        });
    }

    fn sidebar_ui(&mut self, ui: &mut Ui, events: &mut Vec<EpubEvent>) {
        let list = [
            Pane::new("contents", "CONTENTS").closable(false),
            Pane::new("annotations", "ANNOTATIONS").closable(false),
        ];
        let current = self.view.position().block;
        let mut go = None;
        let mut action = None;
        let book = &self.book;
        let state = &self.state;
        let active = self.active_highlight.as_deref();
        panes::show(
            ui,
            ui.scope_id().with("epub-panes"),
            &mut self.panes,
            &list,
            |ui, id| match id {
                "contents" => go = toc_ui(ui, book, current),
                _ => action = annotations::show(ui, &state.bookmarks, &state.highlights, active),
            },
            |_, _| {},
        );
        if let Some(pos) = go {
            self.view.restore(pos);
        }
        match action {
            Some(AnnotationAction::Jump(id)) => self.jump_to_annotation(&id),
            Some(AnnotationAction::Delete(id)) => {
                if let Some(patch) = self.delete_annotation(&id) {
                    events.push(EpubEvent::SaveState(patch));
                }
            }
            None => {}
        }
    }

    fn reading_area(&mut self, ui: &mut Ui, events: &mut Vec<EpubEvent>) {
        let area = ui.max_rect();
        let style = self.rich_style(ui.ctx());
        let doc = self.document.clone();
        let mut resolver = EpubResolver {
            book: &mut self.book,
        };
        let inner = Rect::from_min_max(
            area.min + vec2(EDGE_WIDTH, 0.0),
            area.max - vec2(EDGE_WIDTH, 0.0),
        );
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
        let out: RichOutput = self.view.show(&mut child, &doc, &style, &mut resolver);
        self.scroll = (out.offset, out.viewport_height, out.content_height);
        if out.selection_changed {
            self.has_selection = self.view.selection(&doc).is_some();
        }
        if let Some(id) = out.clicked_mark {
            self.active_highlight = Some(id);
        }
        for action in out.actions {
            let marina_richtext::Action::Open(url) = action;
            events.push(EpubEvent::OpenUrl(url));
        }
        // `.edge` page buttons on both sides of the text.
        let (text, _) = self.prefs.theme.colors();
        for (forward, rect) in [
            (
                false,
                Rect::from_min_size(area.min, vec2(EDGE_WIDTH, area.height())),
            ),
            (
                true,
                Rect::from_min_size(
                    area.right_top() - vec2(EDGE_WIDTH, 0.0),
                    vec2(EDGE_WIDTH, area.height()),
                ),
            ),
        ] {
            let label = if forward {
                "Next page"
            } else {
                "Previous page"
            };
            let resp = ui.interact(rect, ui.scope_id().with(("edge", forward)), Sense::click());
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
            let alpha = if resp.hovered() { 0.6 } else { 0.25 };
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                if forward {
                    icons::CHEVRON_RIGHT
                } else {
                    icons::CHEVRON_LEFT
                },
                icons::font(14.0),
                text.gamma_multiply(alpha),
            );
            if resp.clicked() {
                self.page(forward);
            }
        }
    }

    /// The page's look: the reading theme's colours over the app theme's other choices.
    fn rich_style(&self, ctx: &egui::Context) -> RichStyle {
        let (text, bg) = self.prefs.theme.colors();
        let mut s = RichStyle::from_ctx(ctx);
        s.size = BASE_SIZE * self.prefs.font_size as f32 / 100.0;
        s.face = self.prefs.face;
        s.max_width = COLUMN;
        s.line_height = 1.6;
        s.text = text;
        s.strong = text;
        s.muted = text.gamma_multiply(0.65);
        // Links keep the text colour (`a { color: inherit }`); the underline marks them.
        s.link = text;
        let tint = |a: f32| blend(bg, text, a);
        s.code_bg = tint(0.08);
        s.pre_bg = tint(0.05);
        s.quote_bar = tint(0.3);
        s.rule = tint(0.2);
        s
    }

    fn sync_marks(&mut self) {
        let marks = self
            .state
            .highlights
            .iter()
            .map(|h| Mark {
                id: h.id.clone(),
                start: self.book.position(h.start),
                end: self.book.position(h.end),
                color: HIGHLIGHT,
            })
            .collect();
        self.view.set_marks(marks);
    }

    /// The position patch, when the position has settled for [`SAVE_DELAY`] (or `now`).
    fn position_patch(&mut self, now: bool) -> Option<Value> {
        if self.view.restoring() {
            return None;
        }
        let here = self.location();
        if self.saved == Some(here) {
            self.unsaved_since = None;
            return None;
        }
        let t = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0.0, |d| d.as_secs_f64());
        let since = *self.unsaved_since.get_or_insert(t);
        if !now && t - since < SAVE_DELAY {
            return None;
        }
        self.saved = Some(here);
        self.unsaved_since = None;
        self.state.position = Some(here);
        Some(self.state.patch())
    }
}

/// The Contents pane: the table of contents, the current chapter marked. Returns where to go.
fn toc_ui(ui: &mut Ui, book: &Book, current: usize) -> Option<Position> {
    let p = theme::palette(ui.ctx());
    if book.toc.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new("No table of contents.")
                .size(12.0)
                .color(p.text_muted),
        );
        return None;
    }
    let current_label = book.label_at(current);
    let mut go = None;
    egui::ScrollArea::vertical()
        .id_salt("pageliner-toc")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for entry in &book.toc {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::click());
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, &entry.label)
                });
                let selected = entry.label == current_label
                    && book
                        .target(&entry.target)
                        .is_some_and(|t| t.block <= current);
                if selected {
                    ui.painter().rect_filled(rect, 0.0, p.bg_selected);
                } else if resp.hovered() {
                    ui.painter().rect_filled(rect, 0.0, p.bg_item_hover);
                }
                let mut job = egui::text::LayoutJob::simple_singleline(
                    entry.label.clone(),
                    egui::FontId::proportional(12.5),
                    if selected { p.accent } else { p.text_secondary },
                );
                let left = 8.0 + entry.depth as f32 * 14.0;
                job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - left - 8.0);
                let galley = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(
                    egui::pos2(rect.left() + left, rect.center().y - galley.size().y / 2.0),
                    galley,
                    p.text_secondary,
                );
                let resp = resp.on_hover_text(&entry.label);
                if resp.clicked() {
                    go = book.target(&entry.target);
                }
            }
        });
    go
}

/// `.tb-btn`: icon and/or label, bordered; `active` is the accent outline.
fn tb_button(
    ui: &mut Ui,
    icon: Option<&str>,
    label: &str,
    title: &str,
    active: bool,
    enabled: bool,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let font = egui::FontId::proportional(13.0);
    let text = (!label.is_empty()).then(|| {
        ui.painter()
            .layout_no_wrap(label.to_string(), font, p.text_primary)
    });
    let icon_w = if icon.is_some() { 14.0 } else { 0.0 };
    let gap = if icon.is_some() && text.is_some() {
        6.0
    } else {
        0.0
    };
    let text_w = text.as_ref().map_or(0.0, |t| t.size().x);
    let pad = if text.is_some() { 10.0 } else { 9.0 };
    let size = vec2(icon_w + gap + text_w + pad * 2.0, 28.0);
    let (rect, resp) = ui.allocate_exact_size(
        size,
        if enabled {
            Sense::click()
        } else {
            Sense::hover()
        },
    );
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::Role::Button,
            enabled,
            active,
            if label.is_empty() { title } else { label },
        )
    });
    let resp = resp.on_hover_text(title);
    let fill = if active {
        p.bg_selected
    } else if resp.hovered() && enabled {
        p.bg_button_hover
    } else {
        p.bg_button
    };
    let stroke = if active { p.accent } else { p.border };
    ui.painter().rect(
        rect,
        theme::RADIUS,
        fill,
        Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    let ink = if active { p.accent } else { p.text_primary };
    let ink = if enabled {
        ink
    } else {
        ink.gamma_multiply(0.4)
    };
    let mut x = rect.left() + pad;
    if let Some(glyph) = icon {
        ui.painter().text(
            egui::pos2(x + icon_w / 2.0, rect.center().y),
            Align2::CENTER_CENTER,
            glyph,
            icons::font(12.0),
            ink,
        );
        x += icon_w + gap;
    }
    if let Some(t) = text {
        ui.painter()
            .galley(egui::pos2(x, rect.center().y - t.size().y / 2.0), t, ink);
    }
    resp
}

/// Images from the book's archive; links inside the book scroll to their anchor, external ones
/// go to the host.
struct EpubResolver<'a> {
    book: &'a mut Book,
}

impl Resolver for EpubResolver<'_> {
    fn image(&mut self, ctx: &egui::Context, src: &str) -> Option<String> {
        if src.starts_with("data:") {
            return data_uri(ctx, src);
        }
        // The extension stays last so egui_extras picks the right decoder.
        let uri = format!("bytes://epub-{:x}/{src}", fnv1a(&self.book.id));
        if ctx.try_load_bytes(&uri).is_err() {
            ctx.include_bytes(uri.clone(), self.book.resource(src)?);
        }
        Some(uri)
    }

    fn link(&mut self, target: &str) -> LinkAction {
        let external = target.contains("://") || target.starts_with("mailto:");
        if external {
            return LinkAction::Open(target.to_string());
        }
        let anchors = &self.book.document.anchors;
        if anchors.contains_key(target) {
            return LinkAction::Anchor(target.to_string());
        }
        let path = target.split_once('#').map_or(target, |(p, _)| p);
        if anchors.contains_key(path) {
            LinkAction::Anchor(path.to_string())
        } else {
            log::warn!("Unresolved link {target}");
            LinkAction::Ignore
        }
    }
}

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(mix(a.r(), b.r()), mix(a.g(), b.g()), mix(a.b(), b.b()))
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}

fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
