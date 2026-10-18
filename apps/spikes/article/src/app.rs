//! The GUI: the cached entries on the left, the selected article rendered natively on the right.

use std::time::{Duration, Instant};

use base64::Engine;
use spike_common::Screenshot;
use spike_richtext::fonts::{self, Face, Installed};
use spike_richtext::render::{Action, Position, RichView, Theme};
use spike_richtext::{convert, Block, Document};

use crate::corpus::Article;

const SCREENSHOT_SETTLE_SECS: f64 = 8.0;

pub struct ArticleApp {
    articles: Vec<Article>,
    selected: usize,
    doc: Document,
    convert_time: Duration,
    view: RichView,
    face: Face,
    fonts: Installed,
    screenshot: Option<Screenshot>,
}

impl ArticleApp {
    pub fn new(cc: &eframe::CreationContext<'_>, articles: Vec<Article>) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let fonts = fonts::install(&cc.egui_ctx);
        let selected = std::env::var("MARINA_ARTICLE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0)
            .min(articles.len().saturating_sub(1));
        let mut app = Self {
            articles,
            selected,
            doc: Document::default(),
            convert_time: Duration::ZERO,
            view: RichView::default(),
            face: Face::Sans,
            fonts,
            screenshot: Screenshot::from_env(),
        };
        app.select(selected);
        // Diagnostics: `MARINA_FIND=table|quote|image|list|code` scrolls to the first such block.
        if let Ok(kind) = std::env::var("MARINA_FIND") {
            if let Some(block) = app.doc.blocks.iter().position(|b| block_kind(b) == kind) {
                app.view.restore(Position { block, offset: 0 });
            }
        }
        app
    }

    fn select(&mut self, i: usize) {
        self.selected = i;
        self.view.reset();
        let Some(a) = self.articles.get(i) else {
            return;
        };
        let start = Instant::now();
        self.doc = convert::from_article_html(&a.content, a.link.as_deref());
        self.convert_time = start.elapsed();
    }
}

impl eframe::App for ArticleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let mut select = None;
        egui::Panel::left("entries")
            .resizable(true)
            .default_size(320.0)
            .show(ui, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (i, a) in self.articles.iter().enumerate() {
                        let text = format!("{i}. {} — {}", a.feed, a.title);
                        if ui.selectable_label(i == self.selected, text).clicked() {
                            select = Some(i);
                        }
                    }
                });
            });
        if let Some(i) = select {
            self.select(i);
        }
        let article = self.articles.get(self.selected).cloned();
        egui::Panel::top("bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                if let Some(a) = &article {
                    ui.strong(&a.title);
                    if let Some(link) = &a.link {
                        ui.hyperlink_to("Open in browser", link);
                    }
                }
                ui.separator();
                ui.selectable_value(&mut self.face, Face::Sans, "Sans");
                ui.selectable_value(&mut self.face, Face::Serif, "Serif");
                ui.separator();
                ui.weak(format!(
                    "{} blocks · converted in {:.2} ms · laid out {}",
                    self.doc.blocks.len(),
                    self.convert_time.as_secs_f64() * 1000.0,
                    self.view.laid_out
                ));
            });
        });
        let real = match self.face {
            Face::Sans => self.fonts.sans,
            Face::Serif => self.fonts.serif,
        };
        let theme = Theme::from_visuals(ui.visuals(), self.face, real);
        let mut actions = Vec::new();
        egui::CentralPanel::default_margins().show(ui, |ui| {
            let mut area = egui::ScrollArea::vertical()
                .id_salt(("article", self.selected))
                .auto_shrink(false);
            if let Some(y) = self.view.take_scroll_target() {
                area = area.vertical_scroll_offset(y);
            }
            area.show_viewport(ui, |ui, viewport| {
                actions = self
                    .view
                    .show(ui, viewport, &self.doc, &theme, &mut image_uri);
            });
        });
        for action in actions {
            match action {
                Action::Link(url) => ui.ctx().open_url(egui::OpenUrl::new_tab(url)),
            }
        }
        // Remote images need longer than `Screenshot`'s own settle time.
        if let Some(s) = &mut self.screenshot {
            ui.ctx().request_repaint();
            if ui.input(|i| i.time) >= SCREENSHOT_SETTLE_SECS {
                s.tick(ui.ctx());
            }
        }
    }
}

fn block_kind(block: &Block) -> &'static str {
    match block {
        Block::Text { .. } => "text",
        Block::List { .. } => "list",
        Block::Quote(_) => "quote",
        Block::Code(_) => "code",
        Block::Image { .. } => "image",
        Block::Rule => "rule",
        Block::Table { .. } => "table",
    }
}

/// Remote images load through egui_extras' HTTP loader; `data:` images are decoded here.
fn image_uri(ctx: &egui::Context, src: &str) -> Option<String> {
    if src.starts_with("http://") || src.starts_with("https://") {
        return Some(src.to_string());
    }
    let (meta, data) = src.strip_prefix("data:")?.split_once(',')?;
    if !meta.ends_with(";base64") {
        return None;
    }
    let uri = format!("bytes://data/{:x}", fxhash(src));
    if ctx.try_load_bytes(&uri).is_err() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.trim())
            .ok()?;
        ctx.include_bytes(uri.clone(), bytes);
    }
    Some(uri)
}

fn fxhash(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}
