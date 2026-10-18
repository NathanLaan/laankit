//! The EPUB reader in a window of its own, for trying books by hand.
//!
//! ```bash
//! cargo run -p pageliner-epub --example reader -- book.epub
//! ```
//!
//! It plays the host's part with plain files: the book's state and the reading preferences are
//! kept in `<temp dir>/pageliner-epub-example/` (`state-<id>.json`, `prefs.json`), with the same
//! merge rules PageLiner uses. For screenshots (see `marina_egui::diagnostics`):
//! `MARINA_SCREENSHOT=shot.png MARINA_SCREENSHOT_EXIT=1 MARINA_THEME=midnight`, plus
//! `MARINA_READING_THEME=light|sepia|dark`, `MARINA_FONT_SIZE=<percent>`, `MARINA_FACE=sans|serif`
//! and `MARINA_TOC=<n>` (jump to the n-th contents entry).

use std::path::{Path, PathBuf};
use std::time::Instant;

use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::theme::ThemeState;
use pageliner_epub::{Book, EpubEvent, EpubView, NativeState, ReaderPrefs, ReadingTheme};
use serde_json::{Map, Value};

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: reader <book.epub>");
        std::process::exit(2);
    };
    let started = Instant::now();
    let book = match Book::open(Path::new(&path)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(1);
        }
    };
    log::info!(
        "Opened “{}” in {:.1} ms: {} spine documents, {} blocks, {} contents entries",
        book.title,
        started.elapsed().as_secs_f64() * 1000.0,
        book.spine.len(),
        book.document.blocks.len(),
        book.toc.len()
    );
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("{} — PageLiner reader", book.title))
            .with_inner_size([1200.0, 800.0]),
        ..Default::default()
    };
    eframe::run_native(
        "pageliner-epub-reader",
        options,
        Box::new(|cc| Ok(Box::new(Reader::new(cc, book)))),
    )
}

struct Reader {
    view: EpubView,
    dir: PathBuf,
    state_file: PathBuf,
    screenshot: Option<Screenshot>,
}

impl Reader {
    fn new(cc: &eframe::CreationContext<'_>, book: Book) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        marina_egui::fonts::install(&cc.egui_ctx);
        let mut theme = ThemeState::default();
        diagnostics::theme_overrides(&mut theme);
        theme.apply(&cc.egui_ctx);

        let dir = std::env::temp_dir().join("pageliner-epub-example");
        let state_file = dir.join(format!("state-{:x}.json", fnv1a(&book.id)));
        let state = NativeState::from_book_state(&read_object(&state_file));
        let mut prefs = ReaderPrefs::from_ui_prefs(&read_object(&dir.join("prefs.json")));
        let env = |k: &str| std::env::var(k).ok();
        if let Some(t) = env("MARINA_READING_THEME") {
            prefs.theme = ReadingTheme::ALL
                .into_iter()
                .find(|r| r.id() == t)
                .unwrap_or(prefs.theme);
        }
        if let Some(n) = env("MARINA_FONT_SIZE").and_then(|n| n.parse().ok()) {
            prefs.font_size = n;
        }
        match env("MARINA_FACE").as_deref() {
            Some("sans") => prefs.face = marina_richtext::Face::Sans,
            Some("serif") => prefs.face = marina_richtext::Face::Serif,
            _ => {}
        }
        let toc = env("MARINA_TOC").and_then(|n| n.parse::<usize>().ok());
        let target = toc
            .and_then(|i| book.toc.get(i))
            .and_then(|e| book.target(&e.target));
        let mut view = EpubView::new(book, state, prefs);
        if let Some(pos) = target {
            let loc = view.book().location(pos);
            view.go_to(loc);
        }
        Self {
            view,
            dir,
            state_file,
            screenshot: Screenshot::from_env(),
        }
    }

    fn merge(&self, file: &Path, patch: &Value) {
        let mut obj = read_object(file);
        if let Value::Object(p) = patch {
            obj.extend(p.clone());
        }
        let _ = std::fs::create_dir_all(&self.dir);
        if let Err(e) = std::fs::write(file, serde_json::to_string_pretty(&obj).unwrap_or_default())
        {
            log::warn!("Couldn't save {}: {e}", file.display());
        }
    }
}

impl eframe::App for Reader {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        for event in self.view.show(ui) {
            match event {
                EpubEvent::Close => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                EpubEvent::SaveState(patch) => self.merge(&self.state_file.clone(), &patch),
                EpubEvent::SavePrefs(patch) => self.merge(&self.dir.join("prefs.json"), &patch),
                EpubEvent::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
                EpubEvent::Status(s) => log::info!("Reading: {s}"),
            }
        }
        if let Some(shot) = &mut self.screenshot {
            shot.tick(&ctx);
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        if let Some(patch) = self.view.flush() {
            self.merge(&self.state_file.clone(), &patch);
        }
    }
}

fn read_object(path: &Path) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn fnv1a(s: &str) -> u64 {
    s.bytes().fold(0xcbf29ce484222325, |h, b| {
        (h ^ b as u64).wrapping_mul(0x100000001b3)
    })
}
