//! A standalone deck window for trying the widgets: the SLIDES pane on the
//! left, the deck preview in the middle, edits applied to the note in memory.
//!
//! ```bash
//! cargo run -p noteliner-slides --example deck -- crates/noteliner-slides/examples/sample-deck.md
//! ```
//!
//! P presents from the selected slide (Escape stops), Ctrl+, opens the
//! presentation settings, right-click a slide for its menu. For screenshots:
//! `MARINA_SCREENSHOT` / `MARINA_THEME` as everywhere, `MARINA_SLIDE=<n>`
//! selects a slide, and `MARINA_OPEN=present|settings` opens one at launch.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{Key, Modifiers};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::fonts;
use marina_egui::theme::{self, ThemeState};
use noteliner_slides::{
    normalize_presentation, parse_deck, slide_at_line, Deck, DeckPreview, ImageSource, PaneAction,
    PresentationSettings, Presenter, PresenterAction, SettingsOutcome, SlidesPane,
};
use serde_json::{Map, Value};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("examples/sample-deck.md")
        });
    let markdown = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        eprintln!("Couldn't read {}: {e}", path.display());
        std::process::exit(1);
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NoteLiner slides")
            .with_inner_size([1280.0, 820.0]),
        ..Default::default()
    };
    eframe::run_native(
        "noteliner-slides-deck",
        options,
        Box::new(move |cc| Ok(Box::new(DeckApp::new(&cc.egui_ctx, path, markdown)))),
    )
}

struct DeckApp {
    files: Files,
    markdown: String,
    presentation: Value,
    deck: Deck,
    caret: usize,
    pane: SlidesPane,
    preview: DeckPreview,
    settings: Option<PresentationSettings>,
    presenter: Option<Presenter>,
    vars: HashMap<String, String>,
    screenshot: Option<Screenshot>,
}

impl DeckApp {
    fn new(ctx: &egui::Context, path: PathBuf, markdown: String) -> Self {
        fonts::install(ctx);
        egui_extras::install_image_loaders(ctx);
        let mut theme = ThemeState::default();
        diagnostics::theme_overrides(&mut theme);
        theme.apply(ctx);

        let presentation = frontmatter_presentation(&markdown);
        let deck = parse_deck(&markdown, Some(&presentation));
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let vars = HashMap::from([
            ("name".to_string(), name.clone()),
            ("title".to_string(), name),
        ]);
        let slide = std::env::var("MARINA_SLIDE")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .and_then(|i| deck.slide(i))
            .map_or(1, |s| s.source_range.from_line);
        let mut app = Self {
            files: Files(path.parent().unwrap_or(Path::new(".")).to_path_buf()),
            markdown,
            presentation,
            deck,
            caret: slide,
            pane: SlidesPane::default(),
            preview: DeckPreview::default(),
            settings: None,
            presenter: None,
            vars,
            screenshot: Screenshot::from_env(),
        };
        match std::env::var("MARINA_OPEN").as_deref() {
            Ok("present") => app.present(),
            Ok("settings") => app.open_settings(),
            _ => {}
        }
        app
    }

    fn active(&self) -> Option<usize> {
        slide_at_line(&self.deck, self.caret).map(|s| s.index)
    }

    fn set_markdown(&mut self, markdown: String) {
        self.markdown = markdown;
        self.deck = parse_deck(&self.markdown, Some(&self.presentation));
    }

    fn present(&mut self) {
        self.presenter = Some(Presenter::new(self.active().unwrap_or(1)));
    }

    fn open_settings(&mut self) {
        self.settings = Some(PresentationSettings::new(&self.deck.config));
    }
}

/// Images relative to the note's folder, as `file://` URIs.
struct Files(PathBuf);

impl ImageSource for Files {
    fn uri(&self, src: &str) -> Option<String> {
        if src.contains("://") {
            return Some(src.to_string());
        }
        let file = self.0.join(src).canonicalize().ok()?;
        Some(format!("file://{}", file.display()))
    }
}

impl eframe::App for DeckApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if let Some(mut presenter) = self.presenter.take() {
            let actions = presenter.show(&ctx, &self.deck, &self.vars, &self.files);
            let mut keep = true;
            for action in actions {
                match action {
                    PresenterAction::Changed(i) => {
                        if let Some(s) = self.deck.slide(i) {
                            self.caret = s.source_range.from_line;
                        }
                    }
                    PresenterAction::Exit => keep = false,
                }
            }
            if keep {
                self.presenter = Some(presenter);
            }
        } else if self.settings.is_none() && ctx.memory(|m| m.focused().is_none()) {
            if ctx.input_mut(|i| i.consume_key(Modifiers::NONE, Key::P)) {
                self.present();
            }
            if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Comma)) {
                self.open_settings();
            }
        }

        let p = theme::palette(&ctx);
        let active = self.active();
        let mut actions = Vec::new();
        egui::Panel::left("slides")
            .resizable(true)
            .default_size(300.0)
            .frame(egui::Frame::NONE.fill(p.bg_surface))
            .show(ui, |ui| {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(12.0);
                    ui.label(
                        fonts::weight("SLIDES", fonts::SEMIBOLD)
                            .size(11.0)
                            .color(p.text_muted),
                    );
                });
                ui.add_space(4.0);
                egui::ScrollArea::vertical().show(ui, |ui| {
                    actions =
                        self.pane
                            .show(ui, &self.markdown, Some(&self.deck), active, &self.files);
                });
            });
        let mut jump = None;
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(p.bg_base))
            .show(ui, |ui| {
                jump = self
                    .preview
                    .show(ui, Some(&self.deck), active, &self.vars, &self.files);
            });
        if let Some(line) = jump {
            self.caret = line;
        }
        for action in actions {
            match action {
                PaneAction::Jump { line } => self.caret = line,
                PaneAction::Edit(result) => {
                    self.set_markdown(result.markdown);
                    if let Some(line) = result.caret_line {
                        self.caret = line;
                    }
                }
                PaneAction::OpenSettings => self.open_settings(),
            }
        }

        if let Some(settings) = &mut self.settings {
            match settings.show(&ctx, self.deck.slides.len()) {
                SettingsOutcome::Open => {}
                SettingsOutcome::Cancel => self.settings = None,
                SettingsOutcome::Save(p) => {
                    self.presentation = p.to_json();
                    self.settings = None;
                    let markdown = std::mem::take(&mut self.markdown);
                    self.set_markdown(markdown);
                }
            }
        }
        if let Some(s) = &mut self.screenshot {
            s.tick(&ctx);
        }
    }
}

/// The `presentation:` block of a note's frontmatter, read with a deliberately
/// tiny YAML subset (`key: value` lines indented under it); the app reads it
/// with `noteliner-core`'s frontmatter parser instead.
fn frontmatter_presentation(markdown: &str) -> Value {
    let mut lines = markdown.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Value::Null;
    }
    let mut map = Map::new();
    let mut inside = false;
    for line in lines {
        if line.trim() == "---" {
            break;
        }
        if line.trim_end() == "presentation:" {
            inside = true;
            continue;
        }
        if inside {
            let Some((key, value)) = line.strip_prefix("  ").and_then(|l| l.split_once(':')) else {
                inside = false;
                continue;
            };
            let value = value.trim().trim_matches('"');
            let json = match value {
                "true" => Value::Bool(true),
                "false" => Value::Bool(false),
                v => v
                    .parse::<i64>()
                    .map_or_else(|_| Value::String(v.to_string()), Value::from),
            };
            map.insert(key.trim().to_string(), json);
        }
    }
    let value = Value::Object(map);
    // Normalize once so an unknown value shows up as the default it means.
    normalize_presentation(Some(&value)).map_or(Value::Null, |p| p.to_json())
}
