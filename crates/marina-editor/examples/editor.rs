//! A window with the editor, its toolbar and a status bar, for trying it by hand.
//!
//! ```bash
//! cargo run -p marina-editor --example editor -- note.md     # or --lines 20000 for a generated note
//! cargo run -p marina-editor --example editor --features spellcheck -- note.md
//! ```
//!
//! The file is never written back. With `spellcheck`, the example loads
//! `en_US` from the system's Hunspell folder if it can find it.
//! `MARINA_THEME`, `MARINA_SCALE` and `MARINA_SCREENSHOT` work as in every
//! Marina app (`marina_egui::diagnostics`); `MARINA_FIND=<query>` opens the
//! find panel, `MARINA_LINE=<n>` scrolls to a line, `MARINA_SELECT=<a>,<b>`
//! selects characters a..b (a caret when a = b), and `MARINA_GOTO=1` opens
//! the Go to line bar.

use egui::{Frame, Margin, RichText};
use marina_editor::toolbar::{self, ToolbarProps};
use marina_editor::{gen, Editor, EditorEvent, EditorState};
use marina_egui::diagnostics::{self, Screenshot};
use marina_egui::theme::{self, ThemeState};

struct App {
    name: String,
    state: EditorState,
    show_preview: bool,
    show_history: bool,
    last_event: String,
    screenshot: Option<Screenshot>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let p = theme::palette(&ctx);
        egui::Panel::top("toolbar")
            .frame(Frame::NONE)
            .show(ui, |ui| {
                let props = ToolbarProps {
                    file_name: Some(&self.name),
                    show_history: self.show_history,
                    show_preview: self.show_preview,
                };
                for action in toolbar::show(ui, &mut self.state, &props) {
                    match action {
                        toolbar::ToolbarAction::TogglePreview => self.show_preview ^= true,
                        toolbar::ToolbarAction::ToggleHistory => self.show_history ^= true,
                    }
                }
            });
        egui::Panel::bottom("status")
            .frame(
                Frame::NONE
                    .fill(p.bg_base)
                    .inner_margin(Margin::symmetric(12, 4)),
            )
            .show(ui, |ui| {
                let s = self.state.status(&ctx);
                let sel = s
                    .selection_lines
                    .map(|(a, b)| format!(" · {} selected (lines {a}–{b})", s.selection_len))
                    .unwrap_or_default();
                ui.label(
                    RichText::new(format!(
                        "Ln {}, Col {}{sel} · {} lines · {}",
                        s.line,
                        s.col,
                        self.state.text().matches('\n').count() + 1,
                        self.last_event
                    ))
                    .small()
                    .color(p.text_muted),
                );
            });
        egui::CentralPanel::default()
            .frame(Frame::NONE)
            .show(ui, |ui| {
                let out = Editor::new(&mut self.state).show(ui);
                for event in out.events {
                    self.last_event = match event {
                        EditorEvent::Changed => "edited (not saved: example)".into(),
                        other => format!("{other:?}"),
                    };
                }
            });
        if let Some(s) = &mut self.screenshot {
            s.tick(&ctx);
        }
    }
}

fn main() -> eframe::Result {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let mut lines = None;
    let mut path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lines" => lines = args.next().and_then(|n| n.parse().ok()),
            _ => path = Some(arg),
        }
    }
    let (name, text) = match (path, lines) {
        (Some(p), _) => (
            std::path::Path::new(&p)
                .file_stem()
                .map_or(p.clone(), |s| s.to_string_lossy().into_owned()),
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("can't read {p}: {e}")),
        ),
        (None, n) => {
            let n = n.unwrap_or(400);
            (format!("Generated ({n} lines)"), gen::document(n))
        }
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("marina-editor")
            .with_inner_size([1000.0, 760.0]),
        ..Default::default()
    };
    eframe::run_native(
        "marina-editor",
        options,
        Box::new(move |cc| {
            let ctx = &cc.egui_ctx;
            marina_egui::fonts::install(ctx);
            let mut theme = ThemeState::default();
            diagnostics::theme_overrides(&mut theme);
            theme.apply(ctx);
            let mut state = EditorState::new("example");
            state.set_text(ctx, text);
            state.set_link_targets(
                [
                    "Meeting notes",
                    "Reading list",
                    "Rust 1.92 release",
                    "Weekly review",
                ]
                .map(String::from)
                .to_vec(),
            );
            #[cfg(feature = "spellcheck")]
            load_dictionary(&mut state);
            if let Ok(query) = std::env::var("MARINA_FIND") {
                state.set_find_query(query);
                state.set_find_open(true);
                state.run(marina_editor::Command::FindNext);
            }
            if let Some(line) = std::env::var("MARINA_LINE")
                .ok()
                .and_then(|l| l.parse().ok())
            {
                state.scroll_to_line(ctx, line);
            }
            if let Some((a, b)) = std::env::var("MARINA_SELECT").ok().and_then(|v| {
                let (a, b) = v.split_once(',')?;
                Some((a.trim().parse().ok()?, b.trim().parse().ok()?))
            }) {
                state.set_cursor(ctx, a, b);
            }
            if std::env::var_os("MARINA_GOTO").is_some() {
                state.open_goto();
            }
            state.focus();
            Ok(Box::new(App {
                name,
                state,
                show_preview: false,
                show_history: false,
                last_event: String::new(),
                screenshot: Screenshot::from_env(),
            }))
        }),
    )
}

#[cfg(feature = "spellcheck")]
fn load_dictionary(state: &mut EditorState) {
    for dir in [
        "/usr/share/hunspell",
        "/usr/share/myspell",
        "/Library/Spelling",
    ] {
        let dir = std::path::Path::new(dir);
        let (aff, dic) = (dir.join("en_US.aff"), dir.join("en_US.dic"));
        if let (Ok(aff), Ok(dic)) = (std::fs::read_to_string(aff), std::fs::read_to_string(dic)) {
            match marina_editor::Hunspell::new(&aff, &dic) {
                Ok(h) => {
                    state.set_spell_checker(Some(std::sync::Arc::new(h)));
                    log::info!("Spell checking with {}", dir.display());
                }
                Err(e) => log::warn!("Couldn't load the dictionary: {e}"),
            }
            return;
        }
    }
    log::info!("No en_US Hunspell dictionary found; spell checking is off");
}
