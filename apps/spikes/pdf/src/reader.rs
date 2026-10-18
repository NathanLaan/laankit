//! The reader window: continuous scroll, zoom, page jump, drag-to-select and highlights.

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use egui::{pos2, vec2, Color32, Pos2, Rect, Sense, TextureHandle, TextureOptions, Vec2};
use hayro::hayro_interpret::{InterpreterCache, InterpreterSettings};
use hayro::RenderCache;
use spike_common::Screenshot;
use spike_pdf::annotations::{self, Highlight};
use spike_pdf::doc;
use spike_pdf::text::{PageText, Selection};

/// Page textures kept (the visible ones are never evicted).
const CACHE_PAGES: usize = 20;
const WORKERS: usize = 2;
/// Space between pages and around them, in points.
const GAP: f32 = 12.0;
const MIN_ZOOM: f32 = 0.2;
const MAX_ZOOM: f32 = 5.0;
const ZOOM_STEP: f32 = 0.2;
/// Render scales are rounded to eighths, so small zoom changes reuse textures.
const SCALE_STEPS: f32 = 8.0;
/// Frames before a requested page jump is applied.
const SETTLE_FRAMES: u64 = 3;
const SELECTION: Color32 = Color32::from_rgba_premultiplied(40, 90, 200, 70);
const HIGHLIGHT: Color32 = Color32::from_rgba_premultiplied(120, 110, 0, 90);

/// A texture key: page index and render scale in eighths.
type Key = (usize, u32);

enum Job {
    Render(Key),
    Text(usize),
}

enum Done {
    Image(Key, egui::ColorImage, f64),
    Text(usize, PageText),
}

/// Jobs waiting for a worker; the UI replaces the list every frame with what it still needs, so
/// pages scrolled past are never rendered.
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct QueueState {
    jobs: VecDeque<Job>,
    /// Renders a worker has taken and not finished.
    rendering: HashSet<Key>,
}

pub struct Reader {
    title: String,
    sizes: Vec<[f32; 2]>,
    queue: Arc<Queue>,
    rx: Receiver<Done>,
    textures: HashMap<Key, (TextureHandle, u64)>,
    texts: HashMap<usize, PageText>,
    text_requested: HashSet<usize>,
    frame: u64,
    zoom: f32,
    fit_width: bool,
    current_page: usize,
    jump_to: Option<usize>,
    jump_input: String,
    drag: Option<(usize, [f32; 2], [f32; 2])>,
    pending: Option<(usize, Selection)>,
    highlights: Vec<Highlight>,
    state_path: Option<PathBuf>,
    opened_at: Instant,
    first_page_ms: Option<f64>,
    last_render_ms: f64,
    screenshot: Option<Screenshot>,
    /// Diagnostics: `MARINA_PDF_SELECT=<page>:<x0>,<y0>,<x1>,<y1>` (page units) selects and
    /// highlights that range once the page's text is in, as if dragged.
    scripted_highlight: bool,
}

impl Reader {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        path: PathBuf,
        bytes: Arc<Vec<u8>>,
        state_path: Option<PathBuf>,
    ) -> Self {
        let opened_at = Instant::now();
        let sizes = doc::open(bytes.clone())
            .map(|pdf| doc::page_sizes(&pdf))
            .unwrap_or_default();
        let queue = Arc::new(Queue::default());
        let (tx, rx) = channel();
        for _ in 0..WORKERS {
            spawn_worker(
                bytes.clone(),
                queue.clone(),
                tx.clone(),
                cc.egui_ctx.clone(),
            );
        }
        let highlights = state_path
            .as_deref()
            .map(annotations::load)
            .unwrap_or_default();
        let title = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let scripted = std::env::var("MARINA_PDF_SELECT")
            .ok()
            .and_then(|v| parse_select(&v));
        let jump_to = scripted.map(|(page, ..)| page).or_else(|| {
            std::env::var("MARINA_PDF_PAGE")
                .ok()
                .and_then(|p| p.parse::<usize>().ok())
                .map(|p| p.saturating_sub(1))
        });
        let mut reader = Self {
            title,
            sizes,
            queue,
            rx,
            textures: HashMap::new(),
            texts: HashMap::new(),
            text_requested: HashSet::new(),
            frame: 0,
            zoom: 1.0,
            fit_width: true,
            current_page: 0,
            jump_to,
            jump_input: String::new(),
            drag: None,
            pending: None,
            highlights,
            state_path,
            opened_at,
            first_page_ms: None,
            last_render_ms: 0.0,
            screenshot: Screenshot::from_env(),
            scripted_highlight: scripted.is_some(),
        };
        if let Some((page, from, to)) = scripted {
            reader.drag = Some((page, from, to));
            reader.request(Vec::new(), vec![page]);
        }
        reader
    }

    fn receive(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.rx.try_recv() {
            match done {
                Done::Image(key, image, ms) => {
                    self.queue
                        .state
                        .lock()
                        .expect("queue")
                        .rendering
                        .remove(&key);
                    let tex = ctx.load_texture(
                        format!("page-{}-{}", key.0, key.1),
                        image,
                        TextureOptions::LINEAR,
                    );
                    self.textures.insert(key, (tex, self.frame));
                    self.last_render_ms = ms;
                    self.first_page_ms
                        .get_or_insert_with(|| self.opened_at.elapsed().as_secs_f64() * 1000.0);
                }
                Done::Text(page, text) => {
                    self.texts.insert(page, text);
                    self.resolve_drag();
                    if std::mem::take(&mut self.scripted_highlight) {
                        self.add_highlight();
                    }
                }
            }
        }
    }

    /// Drops the least recently drawn textures beyond the cache size.
    fn evict(&mut self) {
        while self.textures.len() > CACHE_PAGES {
            let oldest = self
                .textures
                .iter()
                .filter(|(_, (_, used))| *used < self.frame)
                .min_by_key(|(_, (_, used))| *used)
                .map(|(k, _)| *k);
            match oldest {
                Some(k) => self.textures.remove(&k),
                None => break,
            };
        }
    }

    /// Asks the workers for exactly `wanted` (newest first), dropping stale requests.
    fn request(&mut self, wanted: Vec<Key>, texts: Vec<usize>) {
        let mut state = self.queue.state.lock().expect("queue");
        let QueueState { jobs, rendering } = &mut *state;
        jobs.clear();
        for page in texts {
            if self.text_requested.insert(page) {
                jobs.push_back(Job::Text(page));
            }
        }
        for key in wanted {
            if !self.textures.contains_key(&key) && !rendering.contains(&key) {
                jobs.push_back(Job::Render(key));
            }
        }
        drop(state);
        self.queue.ready.notify_all();
    }

    fn resolve_drag(&mut self) {
        let Some((page, from, to)) = self.drag else {
            return;
        };
        if let Some(sel) = self.texts.get(&page).and_then(|t| t.select(from, to)) {
            self.pending = Some((page, sel));
        }
    }

    fn add_highlight(&mut self) {
        let Some((page, sel)) = self.pending.take() else {
            return;
        };
        self.highlights
            .push(Highlight::new(page, &sel.rects, &sel.text));
        self.drag = None;
        if let Some(path) = &self.state_path {
            if let Err(e) = annotations::save(path, &self.highlights) {
                log::error!("Couldn't save highlights: {e}");
            }
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong(&self.title);
            ui.separator();
            if ui.button("−").on_hover_text("Zoom out").clicked() {
                self.fit_width = false;
                self.zoom = (self.zoom - ZOOM_STEP).max(MIN_ZOOM);
            }
            ui.label(format!("{:.0}%", self.zoom * 100.0));
            if ui.button("+").on_hover_text("Zoom in").clicked() {
                self.fit_width = false;
                self.zoom = (self.zoom + ZOOM_STEP).min(MAX_ZOOM);
            }
            ui.toggle_value(&mut self.fit_width, "Fit width");
            ui.separator();
            ui.label(format!(
                "Page {} / {}",
                self.current_page + 1,
                self.sizes.len()
            ));
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.jump_input)
                    .desired_width(40.0)
                    .hint_text("#"),
            );
            let go = ui.button("Go").clicked()
                || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if go {
                if let Ok(n) = self.jump_input.trim().parse::<usize>() {
                    self.jump_to = Some(n.clamp(1, self.sizes.len().max(1)) - 1);
                }
            }
            ui.separator();
            if ui
                .add_enabled(self.pending.is_some(), egui::Button::new("Highlight"))
                .clicked()
            {
                self.add_highlight();
            }
            let first = self
                .first_page_ms
                .map_or("…".into(), |ms| format!("{ms:.0} ms"));
            ui.weak(format!(
                "first page {first} · last render {:.0} ms · {} textures",
                self.last_render_ms,
                self.textures.len()
            ));
        });
    }

    fn side_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Selection");
        match &self.pending {
            Some((page, sel)) => {
                ui.label(format!(
                    "Page {}: {} line rect(s)",
                    page + 1,
                    sel.rects.len()
                ));
                ui.add(egui::Label::new(&sel.text).wrap());
            }
            None => {
                ui.weak("Drag across text on a page.");
            }
        }
        ui.separator();
        ui.heading(format!("Highlights ({})", self.highlights.len()));
        egui::ScrollArea::vertical().show(ui, |ui| {
            for h in &self.highlights {
                if ui.link(format!("p.{} — {}", h.page, h.text)).clicked() {
                    self.jump_to = Some(h.page - 1);
                }
            }
        });
    }

    /// Lays out the pages top to bottom; returns each page's rect in content coordinates.
    fn layout(&self, width: f32) -> (Vec<Rect>, Vec2) {
        let mut y = GAP;
        let mut rects = Vec::with_capacity(self.sizes.len());
        let mut max_w: f32 = 0.0;
        for [w, h] in &self.sizes {
            let size = vec2(w * self.zoom, h * self.zoom);
            let x = ((width - size.x) / 2.0).max(GAP);
            rects.push(Rect::from_min_size(pos2(x, y), size));
            y += size.y + GAP;
            max_w = max_w.max(size.x + 2.0 * GAP);
        }
        (rects, vec2(max_w.max(width), y))
    }

    fn pages(&mut self, ui: &mut egui::Ui) {
        let avail = ui.available_width();
        if self.fit_width {
            let widest = self.sizes.iter().map(|s| s[0]).fold(1.0, f32::max);
            self.zoom = ((avail - 2.0 * GAP - 16.0) / widest).clamp(MIN_ZOOM, MAX_ZOOM);
        }
        let (rects, content) = self.layout(avail);
        let mut area = egui::ScrollArea::both().auto_shrink(false);
        // Panels size themselves over the first frames, which changes the fit-width zoom.
        let settled = self.frame > SETTLE_FRAMES;
        if let Some(page) = self.jump_to.filter(|p| settled && *p < rects.len()) {
            self.jump_to = None;
            area = area.vertical_scroll_offset(rects[page].min.y - GAP);
        }
        let ppp = ui.ctx().pixels_per_point();
        let scale_key = ((self.zoom * ppp * SCALE_STEPS).round() as u32).max(1);
        let mut wanted = Vec::new();
        let mut want_text = Vec::new();
        area.show_viewport(ui, |ui, viewport| {
            ui.set_min_size(content);
            let origin = ui.min_rect().min.to_vec2();
            let visible: Vec<usize> = rects
                .iter()
                .enumerate()
                .filter(|(_, r)| r.intersects(viewport.expand(viewport.height() * 0.5)))
                .map(|(i, _)| i)
                .collect();
            if let Some(&first) = visible
                .iter()
                .find(|&&i| rects[i].max.y > viewport.min.y + viewport.height() * 0.3)
            {
                self.current_page = first;
            }
            for &i in &visible {
                let rect = rects[i].translate(origin);
                wanted.push((i, scale_key));
                self.page(ui, i, rect, scale_key, &mut want_text);
            }
        });
        // Nearest pages first: the one in view, then the pre-rendered neighbours.
        let cur = self.current_page;
        wanted.sort_by_key(|(i, _)| i.abs_diff(cur));
        self.request(wanted, want_text);
    }

    fn page(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        rect: Rect,
        scale_key: u32,
        want_text: &mut Vec<usize>,
    ) {
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, Color32::WHITE);
        // The exact scale, or the closest one already rendered while it's on its way.
        let best = self
            .textures
            .get(&(i, scale_key))
            .map(|_| (i, scale_key))
            .or_else(|| {
                self.textures
                    .keys()
                    .filter(|k| k.0 == i)
                    .min_by_key(|k| k.1.abs_diff(scale_key))
                    .copied()
            });
        if let Some(key) = best {
            let (tex, used) = self.textures.get_mut(&key).expect("cached texture");
            *used = self.frame;
            painter.image(
                tex.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("Page {}", i + 1),
                egui::FontId::proportional(14.0),
                Color32::GRAY,
            );
        }
        let to_page = |p: Pos2| {
            [
                (p.x - rect.min.x) / self.zoom,
                (p.y - rect.min.y) / self.zoom,
            ]
        };
        let to_screen = |r: [f32; 4]| {
            Rect::from_min_size(
                pos2(rect.min.x + r[0] * self.zoom, rect.min.y + r[1] * self.zoom),
                vec2(r[2] * self.zoom, r[3] * self.zoom),
            )
        };
        for h in self.highlights.iter().filter(|h| h.page == i + 1) {
            for r in &h.rects {
                painter.rect_filled(to_screen([r.x, r.y, r.w, r.h]), 0.0, HIGHLIGHT);
            }
        }
        if let Some((_, sel)) = self.pending.as_ref().filter(|(p, _)| *p == i) {
            for r in &sel.rects {
                painter.rect_filled(to_screen(*r), 0.0, SELECTION);
            }
        }

        let resp = ui
            .interact(rect, ui.scope_id().with(("page", i)), Sense::drag())
            .on_hover_cursor(egui::CursorIcon::Text);
        if resp.drag_started() {
            if let Some(p) = resp.interact_pointer_pos() {
                self.drag = Some((i, to_page(p), to_page(p)));
                self.pending = None;
            }
            if !self.texts.contains_key(&i) {
                want_text.push(i);
            }
        }
        if resp.dragged() {
            if let (Some(p), Some(drag)) = (
                resp.interact_pointer_pos(),
                self.drag.as_mut().filter(|d| d.0 == i),
            ) {
                drag.2 = to_page(p);
                self.resolve_drag();
            }
        }
    }
}

impl eframe::App for Reader {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frame += 1;
        let ctx = ui.ctx().clone();
        self.receive(&ctx);
        egui::Panel::top("toolbar").show(ui, |ui| self.toolbar(ui));
        egui::Panel::right("side")
            .default_size(260.0)
            .show(ui, |ui| self.side_panel(ui));
        egui::CentralPanel::default().show(ui, |ui| self.pages(ui));
        self.evict();
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
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

/// A render thread: its own parsed copy of the PDF and caches, taking jobs from the queue.
fn spawn_worker(bytes: Arc<Vec<u8>>, queue: Arc<Queue>, tx: Sender<Done>, ctx: egui::Context) {
    std::thread::spawn(move || {
        let Ok(pdf) = doc::open(bytes) else { return };
        let cache = RenderCache::new();
        let interp = InterpreterCache::new();
        let settings = InterpreterSettings::default();
        loop {
            let job = {
                let mut state = queue.state.lock().expect("queue");
                loop {
                    if let Some(job) = state.jobs.pop_front() {
                        if let Job::Render(key) = job {
                            state.rendering.insert(key);
                        }
                        break job;
                    }
                    state = queue.ready.wait(state).expect("queue");
                }
            };
            let done = match job {
                Job::Render((page, key)) => {
                    let start = Instant::now();
                    let image = doc::render_page(
                        &pdf.pages()[page],
                        &cache,
                        &settings,
                        key as f32 / SCALE_STEPS,
                    );
                    Done::Image((page, key), image, start.elapsed().as_secs_f64() * 1000.0)
                }
                Job::Text(page) => Done::Text(
                    page,
                    spike_pdf::text::extract(&pdf.pages()[page], &interp, &settings),
                ),
            };
            if tx.send(done).is_err() {
                return;
            }
            ctx.request_repaint();
        }
    });
}
