//! The PDF reader (`PdfReader.svelte`), on hayro: continuous scroll instead
//! of pdf.js's one page at a time, zoom and fit width, page jump, drag-to-
//! select text and highlights, bookmarks, the outline, and search. Positions,
//! bookmarks and highlights are saved in the Svelte reader's format
//! ([`annotations`]), so either build can open the book where the other left it.

pub mod annotations;
pub mod doc;
#[cfg(test)]
pub mod fixtures;
pub mod render;
pub mod text;

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use egui::{pos2, vec2, Color32, Key, Modifiers, Pos2, Rect, Sense, Ui, Vec2};
use laankit_egui::panes::{self, Pane, PaneLayout};
use laankit_egui::theme;
use laankit_egui::{dialog, icons};
use serde_json::{Map, Value};

use super::{
    annotations_pane, back_bar, bar_frame, list_row, pane_empty, tool_button, AnnotationAction,
    AnnotationRow, ReaderAction, ToolState, HIGHLIGHT,
};
use crate::model::Book;
use annotations::{Bookmark, Highlight, PdfState};
use doc::{OutlineItem, PdfInfo};
use render::{Done, Key as TexKey, TextureCache, Workers, CACHE_BYTES, SCALE_STEPS};
use text::{PageText, Selection};

/// Space between pages, and around them (`.canvas-scroll { padding: 16px }`).
const GAP: f32 = 12.0;
const PAD: f32 = 16.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 5.0;
const ZOOM_STEP: f32 = 0.2;
/// Frames before a requested page jump is applied: the panels size themselves
/// over the first frames, which changes the fit-width zoom.
const SETTLE_FRAMES: u64 = 3;
/// `saveProgress` waits 400 ms.
const SAVE_DELAY_SECS: f64 = 0.4;
/// `.sidebar { width: 260px }`.
const SIDEBAR_WIDTH: f32 = 260.0;
/// `::selection { background: rgba(0, 100, 255, 0.3) }`.
const SELECTION: Color32 = Color32::from_rgba_premultiplied(0, 30, 77, 77);
const PANES: [(&str, &str); 3] = [
    ("contents", "Contents"),
    ("annotations", "Annotations"),
    ("search", "Search"),
];

/// A page jumped to, and the scroll offset the jump landed on.
#[derive(Clone, Copy, Debug)]
struct Pin {
    page: usize,
    offset: Option<f32>,
}

/// A drag across a page's text, in page units.
#[derive(Clone, Copy, Debug)]
struct Drag {
    page: usize,
    from: [f32; 2],
    to: [f32; 2],
}

#[derive(Default)]
struct Search {
    query: String,
    results: Vec<usize>,
    searching: bool,
    ran: bool,
    generation: u64,
}

pub struct PdfReader {
    title: String,
    sizes: Vec<[f32; 2]>,
    outline: Vec<OutlineItem>,
    workers: Workers,
    textures: TextureCache,
    texts: HashMap<usize, PageText>,
    text_requested: HashSet<usize>,
    frame: u64,
    zoom: f32,
    fit_width: bool,
    /// 0-based page at the top of the view.
    current: usize,
    /// The 1-based page last written to the state file.
    saved_page: usize,
    dirty_since: Option<f64>,
    jump_to: Option<usize>,
    /// After a jump, the page stays current until the user scrolls (the last
    /// pages can't scroll to the top of the view).
    pin: Option<Pin>,
    drag: Option<Drag>,
    pending: Option<(usize, Selection)>,
    bookmarks: Vec<Bookmark>,
    highlights: Vec<Highlight>,
    search: Search,
    page_input: Option<String>,
    status: Option<String>,
    /// Diagnostics: `LAANKIT_PDF_SELECT=<page>:<x0>,<y0>,<x1>,<y1>` (page
    /// units) selects and highlights that range once its text is in.
    scripted: bool,
}

impl PdfReader {
    pub fn new(
        ctx: &egui::Context,
        book: &Book,
        bytes: Arc<Vec<u8>>,
        info: PdfInfo,
        state: &Map<String, Value>,
    ) -> Self {
        let saved = PdfState::from_state(state);
        let pages = info.sizes.len().max(1);
        let page = saved.page.clamp(1, pages);
        let mut reader = Self {
            title: book.title.clone(),
            sizes: info.sizes,
            outline: info.outline,
            workers: Workers::start(bytes, ctx),
            textures: TextureCache::default(),
            texts: HashMap::new(),
            text_requested: HashSet::new(),
            frame: 0,
            zoom: 1.0,
            fit_width: true,
            current: page - 1,
            saved_page: saved.page,
            dirty_since: None,
            jump_to: (page > 1).then_some(page - 1),
            pin: None,
            drag: None,
            pending: None,
            bookmarks: saved.bookmarks,
            highlights: saved.highlights,
            search: Search::default(),
            page_input: None,
            status: None,
            scripted: false,
        };
        if let Some(p) = std::env::var("LAANKIT_PDF_PAGE")
            .ok()
            .and_then(|p| p.parse::<usize>().ok())
        {
            reader.jump_to = Some(p.clamp(1, pages) - 1);
        }
        if let Some((page, from, to)) = std::env::var("LAANKIT_PDF_SELECT")
            .ok()
            .and_then(|v| parse_select(&v))
        {
            reader.jump_to = Some(page.min(pages - 1));
            reader.drag = Some(Drag { page, from, to });
            reader.scripted = true;
            reader.want_text(page);
        }
        reader
    }

    pub fn page_count(&self) -> usize {
        self.sizes.len()
    }

    #[cfg(test)]
    pub fn current_page(&self) -> usize {
        self.current + 1
    }

    #[cfg(test)]
    pub fn highlights(&self) -> &[Highlight] {
        &self.highlights
    }

    #[cfg(test)]
    pub fn has_text(&self, page: usize) -> bool {
        self.texts.contains_key(&page)
    }

    /// Selects text as if dragged from `from` to `to` on 0-based `page` (page
    /// units). Tests and diagnostics use it; the text must be extracted.
    #[cfg(test)]
    pub fn select(&mut self, page: usize, from: [f32; 2], to: [f32; 2]) {
        self.drag = Some(Drag { page, from, to });
        self.want_text(page);
        self.resolve_drag();
    }

    /// The debounced page position, if it hasn't been written yet.
    pub fn flush(&mut self) -> Option<Value> {
        let page = self.current + 1;
        (page != self.saved_page).then(|| {
            self.saved_page = page;
            self.dirty_since = None;
            annotations::progress_patch(page)
        })
    }

    pub fn show(&mut self, ui: &mut Ui, layout: &mut PaneLayout, keys: bool) -> Vec<ReaderAction> {
        self.frame += 1;
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        let mut out = Vec::new();
        self.receive(&ctx, &mut out);
        if keys && !ctx.egui_wants_keyboard_input() {
            self.keyboard(&ctx);
        }

        egui::Panel::top("pdf-toolbar")
            .frame(bar_frame(&p))
            .show(ui, |ui| self.toolbar(ui, &mut out));
        egui::Panel::left("pdf-sidebar")
            .exact_size(SIDEBAR_WIDTH)
            .resizable(false)
            .frame(egui::Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| self.sidebar(ui, layout, &mut out));
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| self.pages(ui));
        self.textures.evict(self.frame, CACHE_BYTES);

        self.track_position(&ctx, &mut out);
        let status = Some(format!("Page {} / {}", self.current + 1, self.page_count()));
        if status != self.status {
            self.status = status.clone();
            out.push(ReaderAction::Status(status));
        }
        out
    }

    // --- Worker results --------------------------------------------------------

    fn receive(&mut self, ctx: &egui::Context, out: &mut Vec<ReaderAction>) {
        for done in self.workers.poll() {
            match done {
                Done::Image(key, image) => {
                    self.workers.rendered(key);
                    self.textures.insert(ctx, key, image, self.frame);
                }
                Done::Text(page, text) => {
                    self.texts.insert(page, text);
                    self.resolve_drag();
                    if std::mem::take(&mut self.scripted) {
                        if let Some(patch) = self.add_highlight() {
                            out.push(ReaderAction::SaveState(patch));
                        }
                    }
                }
                Done::SearchHit { generation, page } if generation == self.search.generation => {
                    self.search.results.push(page);
                }
                Done::Searched { generation } if generation == self.search.generation => {
                    self.search.searching = false;
                    if let Some(&first) = self.search.results.first() {
                        self.goto(first);
                    }
                }
                Done::SearchHit { .. } | Done::Searched { .. } => {}
            }
        }
    }

    /// `saveProgress`: write the page 400 ms after it last changed.
    fn track_position(&mut self, ctx: &egui::Context, out: &mut Vec<ReaderAction>) {
        if self.jump_to.is_some() || self.current + 1 == self.saved_page {
            self.dirty_since = None;
            return;
        }
        let now = ctx.input(|i| i.time);
        let since = *self.dirty_since.get_or_insert(now);
        if now - since >= SAVE_DELAY_SECS {
            if let Some(patch) = self.flush() {
                out.push(ReaderAction::SaveState(patch));
            }
        } else {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(
                SAVE_DELAY_SECS - (now - since),
            ));
        }
    }

    // --- Navigation ------------------------------------------------------------

    fn goto(&mut self, page: usize) {
        let page = page.min(self.page_count().saturating_sub(1));
        self.jump_to = Some(page);
        self.current = page;
    }

    /// `onKeydown`: arrows and Page Up/Down step pages, Home/End jump; Ctrl+C
    /// copies the selection.
    fn keyboard(&mut self, ctx: &egui::Context) {
        let none = Modifiers::NONE;
        let mut step: Option<isize> = None;
        let mut to: Option<usize> = None;
        let copy = ctx.input_mut(|i| {
            if i.consume_key(none, Key::ArrowRight) || i.consume_key(none, Key::PageDown) {
                step = Some(1);
            } else if i.consume_key(none, Key::ArrowLeft) || i.consume_key(none, Key::PageUp) {
                step = Some(-1);
            } else if i.consume_key(none, Key::Home) {
                to = Some(0);
            } else if i.consume_key(none, Key::End) {
                to = Some(usize::MAX);
            }
            i.events.iter().any(|e| matches!(e, egui::Event::Copy))
        });
        if let Some(s) = step {
            to = Some(self.current.saturating_add_signed(s));
        }
        if let Some(page) = to {
            self.goto(page);
        }
        if copy {
            if let Some((_, sel)) = &self.pending {
                ctx.copy_text(sel.text.clone());
            }
        }
    }

    // --- Toolbar ---------------------------------------------------------------

    fn toolbar(&mut self, ui: &mut Ui, out: &mut Vec<ReaderAction>) {
        let n = self.page_count();
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            if back_bar(ui, &self.title) {
                out.push(ReaderAction::Close);
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let plain = ToolState::default();
                // Right to left: zoom (+, %, −), the pager, bookmark, highlight.
                if tool_button(ui, Some(icons::PLUS), None, "Zoom in", plain).clicked() {
                    self.set_zoom(self.zoom + ZOOM_STEP);
                }
                let pct = format!("{}%", (self.zoom * 100.0).round());
                let fit = ToolState {
                    active: self.fit_width,
                    ..plain
                };
                if tool_button(ui, None, Some(&pct), "Fit width", fit).clicked() {
                    self.fit_width = true;
                    self.jump_to = Some(self.current);
                }
                if tool_button(ui, Some(icons::MINUS), None, "Zoom out", plain).clicked() {
                    self.set_zoom(self.zoom - ZOOM_STEP);
                }
                ui.add_space(8.0);
                let last = ToolState {
                    disabled: self.current + 1 >= n,
                    ..plain
                };
                if tool_button(ui, Some(icons::CHEVRON_RIGHT), None, "Next page", last).clicked() {
                    self.goto(self.current + 1);
                }
                self.page_indicator(ui);
                let first = ToolState {
                    disabled: self.current == 0,
                    ..plain
                };
                if tool_button(ui, Some(icons::CHEVRON_LEFT), None, "Previous page", first)
                    .clicked()
                {
                    self.goto(self.current.saturating_sub(1));
                }
                ui.add_space(8.0);
                let page = self.current + 1;
                let marked = self.bookmarks.iter().any(|b| b.page == page);
                let title = if marked {
                    "Remove bookmark"
                } else {
                    "Bookmark this page"
                };
                let bookmark = ToolState {
                    active: marked,
                    ..plain
                };
                if tool_button(ui, Some(icons::BOOKMARK), None, title, bookmark).clicked() {
                    out.push(ReaderAction::SaveState(self.toggle_bookmark()));
                }
                let highlight = ToolState {
                    active: self.pending.is_some(),
                    disabled: self.pending.is_none(),
                };
                if tool_button(
                    ui,
                    Some(icons::HIGHLIGHTER),
                    None,
                    "Highlight selection",
                    highlight,
                )
                .clicked()
                {
                    if let Some(patch) = self.add_highlight() {
                        out.push(ReaderAction::SaveState(patch));
                    }
                }
            });
        });
    }

    /// `.page-ind`: "n / N"; click it to type a page number.
    fn page_indicator(&mut self, ui: &mut Ui) {
        let p = theme::palette(ui.ctx());
        let n = self.page_count();
        match &mut self.page_input {
            Some(input) => {
                let resp = ui.add(
                    egui::TextEdit::singleline(input)
                        .hint_text(format!("{} / {n}", self.current + 1))
                        .desired_width(64.0)
                        .align(egui::Align2::CENTER_CENTER)
                        .font(egui::FontId::proportional(12.0)),
                );
                if input.is_empty() && !resp.has_focus() && !resp.lost_focus() {
                    resp.request_focus();
                }
                if dialog::submitted(ui, &resp) {
                    if let Ok(page) = input.trim().parse::<usize>() {
                        self.goto(page.clamp(1, n.max(1)) - 1);
                    }
                    self.page_input = None;
                } else if resp.lost_focus() {
                    self.page_input = None;
                }
            }
            None => {
                let text = format!("{} / {}", self.current + 1, n);
                let resp = ui
                    .add(
                        egui::Label::new(
                            egui::RichText::new(text).size(12.0).color(p.text_secondary),
                        )
                        .sense(Sense::click()),
                    )
                    .on_hover_text("Go to page");
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Go to page")
                });
                if resp.clicked() {
                    self.page_input = Some(String::new());
                }
            }
        }
    }

    fn set_zoom(&mut self, zoom: f32) {
        self.fit_width = false;
        self.zoom = zoom.clamp(MIN_ZOOM, MAX_ZOOM);
        self.jump_to = Some(self.current);
    }

    // --- Annotations -----------------------------------------------------------

    fn toggle_bookmark(&mut self) -> Value {
        let page = self.current + 1;
        if self.bookmarks.iter().any(|b| b.page == page) {
            self.bookmarks.retain(|b| b.page != page);
        } else {
            self.bookmarks.push(Bookmark::new(page));
            self.bookmarks.sort_by_key(|b| b.page);
        }
        annotations::bookmarks_patch(&self.bookmarks)
    }

    fn add_highlight(&mut self) -> Option<Value> {
        let (page, sel) = self.pending.take()?;
        self.drag = None;
        self.highlights
            .push(Highlight::new(page + 1, &sel.rects, &sel.text));
        Some(annotations::highlights_patch(&self.highlights))
    }

    fn want_text(&mut self, page: usize) {
        if !self.texts.contains_key(&page) && self.text_requested.insert(page) {
            self.workers.request_text(page);
        }
    }

    fn resolve_drag(&mut self) {
        let Some(d) = self.drag else { return };
        if let Some(sel) = self.texts.get(&d.page).and_then(|t| t.select(d.from, d.to)) {
            self.pending = Some((d.page, sel));
        }
    }

    // --- Sidebar ---------------------------------------------------------------

    fn sidebar(&mut self, ui: &mut Ui, layout: &mut PaneLayout, out: &mut Vec<ReaderAction>) {
        if layout.order.is_empty() {
            *layout = default_layout();
        }
        let list: Vec<Pane<'_>> = PANES
            .iter()
            .map(|(id, title)| Pane::new(id, title).closable(false))
            .collect();
        let mut jump = None;
        let mut annotation = None;
        let mut run_search = false;
        let current = self.current;
        let outline = &self.outline;
        let search = &mut self.search;
        let bookmarks: Vec<AnnotationRow<'_>> = self
            .bookmarks
            .iter()
            .map(|b| AnnotationRow {
                id: &b.id,
                label: &b.label,
            })
            .collect();
        let highlights: Vec<AnnotationRow<'_>> = self
            .highlights
            .iter()
            .map(|h| AnnotationRow {
                id: &h.id,
                label: &h.text,
            })
            .collect();
        panes::show(
            ui,
            egui::Id::unique("pdf-panes"),
            layout,
            &list,
            |ui, id| match id {
                "contents" => {
                    if outline.is_empty() {
                        pane_empty(ui, "No table of contents.");
                        return;
                    }
                    egui::ScrollArea::vertical()
                        .id_salt("pdf-toc")
                        .auto_shrink(false)
                        .show(ui, |ui| {
                            ui.add_space(6.0);
                            for item in outline {
                                let indent = item.depth as f32 * 14.0;
                                if list_row(ui, &item.title, indent, false).clicked() {
                                    jump = item.page;
                                }
                            }
                        });
                }
                "annotations" => annotation = annotations_pane(ui, &bookmarks, &highlights),
                _ => run_search |= search_pane(ui, search, current, &mut jump),
            },
            |_, _| {},
        );
        drop((bookmarks, highlights));
        if run_search {
            self.start_search();
        }
        if let Some(page) = jump {
            self.goto(page);
        }
        match annotation {
            Some(AnnotationAction::JumpBookmark(id)) => {
                if let Some(b) = self.bookmarks.iter().find(|b| b.id == id) {
                    self.goto(b.page.saturating_sub(1));
                }
            }
            Some(AnnotationAction::JumpHighlight(id)) => {
                if let Some(h) = self.highlights.iter().find(|h| h.id == id) {
                    self.goto(h.page.saturating_sub(1));
                }
            }
            Some(AnnotationAction::DeleteBookmark(id)) => {
                self.bookmarks.retain(|b| b.id != id);
                out.push(ReaderAction::SaveState(annotations::bookmarks_patch(
                    &self.bookmarks,
                )));
            }
            Some(AnnotationAction::DeleteHighlight(id)) => {
                self.highlights.retain(|h| h.id != id);
                out.push(ReaderAction::SaveState(annotations::highlights_patch(
                    &self.highlights,
                )));
            }
            None => {}
        }
    }

    /// `runSearch`: every page whose text contains the query.
    fn start_search(&mut self) {
        let q = self.search.query.trim().to_lowercase();
        self.search.results.clear();
        self.search.generation += 1;
        self.search.ran = !q.is_empty();
        self.search.searching = !q.is_empty();
        if !q.is_empty() {
            self.workers.search(self.search.generation, q);
        }
    }

    // --- Pages -----------------------------------------------------------------

    /// Lays out the pages top to bottom; each page's rect in content
    /// coordinates, and the content size.
    fn layout(&self, width: f32) -> (Vec<Rect>, Vec2) {
        let mut y = PAD;
        let mut rects = Vec::with_capacity(self.sizes.len());
        let mut max_w: f32 = 0.0;
        for [w, h] in &self.sizes {
            let size = vec2(w * self.zoom, h * self.zoom);
            let x = ((width - size.x) / 2.0).max(PAD);
            rects.push(Rect::from_min_size(pos2(x, y), size));
            y += size.y + GAP;
            max_w = max_w.max(size.x + 2.0 * PAD);
        }
        (rects, vec2(max_w.max(width), y - GAP + PAD))
    }

    fn pages(&mut self, ui: &mut Ui) {
        let avail = ui.available_width();
        if self.fit_width {
            let widest = self.sizes.iter().map(|s| s[0]).fold(1.0, f32::max);
            // `(containerWidth - 32) / base.width`, less a scrollbar.
            self.zoom = ((avail - 2.0 * PAD - 12.0) / widest).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        let (rects, content) = self.layout(avail);
        let mut area = egui::ScrollArea::both()
            .id_salt("pdf-pages")
            .auto_shrink(false);
        let jumped = self
            .jump_to
            .filter(|p| self.frame > SETTLE_FRAMES && *p < rects.len());
        if let Some(page) = jumped {
            self.jump_to = None;
            area = area.vertical_scroll_offset(rects[page].min.y - PAD);
        }
        let ppp = ui.ctx().pixels_per_point();
        let scale = ((self.zoom * ppp * SCALE_STEPS).round() as u32).max(1);
        let mut wanted: Vec<TexKey> = Vec::new();
        // The page at 30% of the view, and the last one in view.
        let mut top_page = None;
        let mut last_page = None;
        let output = area.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(content);
            let origin = ui.min_rect().min.to_vec2();
            let preload = viewport.expand2(vec2(0.0, viewport.height() * 0.5));
            let visible: Vec<usize> = rects
                .iter()
                .enumerate()
                .filter(|(_, r)| r.intersects(preload))
                .map(|(i, _)| i)
                .collect();
            top_page = visible
                .iter()
                .copied()
                .find(|&i| rects[i].max.y > viewport.min.y + viewport.height() * 0.3);
            last_page = visible
                .iter()
                .copied()
                .rfind(|&i| rects[i].intersects(viewport));
            for &i in &visible {
                wanted.push((i, scale));
                self.page(ui, i, rects[i].translate(origin), scale);
            }
        });
        self.track_current(jumped, &output, top_page, last_page);
        let cur = self.current;
        wanted.sort_by_key(|(i, _)| i.abs_diff(cur));
        let textures = &self.textures;
        self.workers
            .request_renders(wanted, |k| textures.contains(k));
    }

    /// Which page is current: a jumped-to page until the user scrolls away
    /// from it; otherwise the page at 30% of the view, or the last page in
    /// view once scrolled to the end.
    fn track_current(
        &mut self,
        jumped: Option<usize>,
        output: &egui::scroll_area::ScrollAreaOutput<()>,
        top_page: Option<usize>,
        last_page: Option<usize>,
    ) {
        let offset = output.state.clamped_offset().y;
        if let Some(page) = jumped {
            self.pin = Some(Pin { page, offset: None });
        }
        if let Some(pin) = &mut self.pin {
            match pin.offset {
                None => pin.offset = Some(offset),
                Some(o) if (o - offset).abs() > 1.0 => self.pin = None,
                Some(_) => {}
            }
        }
        if self.jump_to.is_some() {
            return;
        }
        let max = (output.content_size.y - output.inner_rect.height()).max(0.0);
        self.current = match self.pin {
            Some(pin) => pin.page,
            None if max > 0.0 && offset >= max - 1.0 => last_page.unwrap_or(self.current),
            None => top_page.unwrap_or(self.current),
        };
    }

    fn page(&mut self, ui: &mut Ui, i: usize, rect: Rect, scale: u32) {
        let painter = ui.painter_at(ui.clip_rect());
        painter.add(
            egui::epaint::Shadow {
                offset: [0, 2],
                blur: 12,
                spread: 0,
                color: Color32::from_black_alpha(90),
            }
            .as_shape(rect, 0),
        );
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        match self.textures.best((i, scale), self.frame) {
            Some(tex) => {
                painter.image(
                    tex.id(),
                    rect,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("Page {}", i + 1),
                    egui::FontId::proportional(14.0),
                    Color32::GRAY,
                );
            }
        }
        let zoom = self.zoom;
        let to_page = |p: Pos2| [(p.x - rect.min.x) / zoom, (p.y - rect.min.y) / zoom];
        let to_screen = |r: [f32; 4]| {
            Rect::from_min_size(
                pos2(rect.min.x + r[0] * zoom, rect.min.y + r[1] * zoom),
                vec2(r[2] * zoom, r[3] * zoom),
            )
        };
        for h in self.highlights.iter().filter(|h| h.page == i + 1) {
            for r in &h.rects {
                painter.rect_filled(to_screen([r.x, r.y, r.w, r.h]), 1.0, HIGHLIGHT);
            }
        }
        if let Some((_, sel)) = self.pending.as_ref().filter(|(p, _)| *p == i) {
            for r in &sel.rects {
                painter.rect_filled(to_screen(*r), 0.0, SELECTION);
            }
        }

        let resp = ui
            .interact(
                rect,
                ui.scope_id().with(("pdf-page", i)),
                Sense::click_and_drag(),
            )
            .on_hover_cursor(egui::CursorIcon::Text);
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.drag = Some(Drag {
                    page: i,
                    from: to_page(p),
                    to: to_page(p),
                });
                self.pending = None;
            }
            self.want_text(i);
        }
        if resp.dragged() {
            if let Some(p) = resp.interact_pointer_pos() {
                if let Some(d) = self.drag.as_mut().filter(|d| d.page == i) {
                    d.to = to_page(p);
                }
                self.resolve_drag();
            }
        }
        if resp.clicked() {
            self.drag = None;
            self.pending = None;
        }
    }
}

/// The Search pane; returns true when a search should start.
fn search_pane(ui: &mut Ui, search: &mut Search, current: usize, jump: &mut Option<usize>) -> bool {
    let p = theme::palette(ui.ctx());
    let mut run = false;
    egui::Frame::NONE
        .inner_margin(egui::Margin::same(10))
        .show(ui, |ui| {
            let resp = ui.add(
                egui::TextEdit::singleline(&mut search.query)
                    .hint_text("Search in document…")
                    .desired_width(f32::INFINITY),
            );
            if dialog::submitted(ui, &resp) {
                run = true;
            }
            if search.searching && search.results.is_empty() {
                pane_empty(ui, "Searching…");
            } else if search.ran && !search.searching && search.results.is_empty() {
                pane_empty(ui, "No matches.");
            } else if !search.results.is_empty() {
                let n = search.results.len();
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(format!(
                        "{n} page{} match{}",
                        if n == 1 { "" } else { "s" },
                        if search.searching { " so far" } else { "" }
                    ))
                    .size(11.0)
                    .color(p.text_muted),
                );
                egui::ScrollArea::vertical()
                    .id_salt("pdf-search")
                    .auto_shrink(false)
                    .show(ui, |ui| {
                        for &page in &search.results {
                            if list_row(ui, &format!("Page {}", page + 1), 0.0, page == current)
                                .clicked()
                            {
                                *jump = Some(page);
                            }
                        }
                    });
            }
        });
    run
}

/// The Svelte reader's pane heights: Contents 280, Annotations 200, Search 200.
fn default_layout() -> PaneLayout {
    PaneLayout {
        order: PANES.iter().map(|(id, _)| id.to_string()).collect(),
        heights: [
            ("contents", 280.0),
            ("annotations", 200.0),
            ("search", 200.0),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect(),
    }
}

/// `<page>:<x0>,<y0>,<x1>,<y1>` with a 1-based page.
fn parse_select(v: &str) -> Option<(usize, [f32; 2], [f32; 2])> {
    let (page, coords) = v.split_once(':')?;
    let n: Vec<f32> = coords
        .split(',')
        .filter_map(|c| c.trim().parse().ok())
        .collect();
    let [x0, y0, x1, y1] = n[..] else { return None };
    Some((
        page.parse::<usize>().ok()?.checked_sub(1)?,
        [x0, y0],
        [x1, y1],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_scripted_selections() {
        assert_eq!(
            parse_select("2:72,90,140,110"),
            Some((1, [72.0, 90.0], [140.0, 110.0]))
        );
        assert_eq!(parse_select("0:1,2,3,4"), None);
        assert_eq!(parse_select("1:1,2"), None);
    }
}
