//! `spike-editor [--lines N] [--line-galleys | --whole-job] [FILE]`: opens FILE, or a generated document of
//! N lines (default 5,000), in the egui Markdown editor.

use std::time::Instant;

use spike_common::Screenshot;
use spike_editor::editor::{Command, MarkdownEditor};
use spike_editor::highlight::Strategy;
use spike_editor::{editor_area, gen};

struct App {
    name: String,
    text: String,
    editor: MarkdownEditor,
    screenshot: Option<Screenshot>,
    /// CPU time of recent frames (ms), for the status bar.
    frame_ms: Vec<f32>,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let start = Instant::now();
        egui::Panel::top("toolbar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong(&self.name);
                ui.separator();
                ui.checkbox(&mut self.editor.plain, "Plain (no highlighting)");
                ui.separator();
                for (label, command) in [
                    ("• List", Command::Bulleted),
                    ("1. List", Command::Numbered),
                    ("Indent as sub-list", Command::SubList),
                ] {
                    if ui.button(label).clicked() {
                        self.editor.pending = Some(command);
                        ui.memory_mut(|m| m.request_focus(self.editor.id));
                    }
                }
            });
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            let mut sorted = self.frame_ms.clone();
            sorted.sort_by(f32::total_cmp);
            let p95 = sorted.get(sorted.len() * 95 / 100).copied().unwrap_or_default();
            let stats = self.editor.highlighter.stats;
            ui.label(format!(
                "{} lines · {} KB · frame p95 {p95:.1} ms (last {}) · layouter calls {} / rebuilds {} / lines scanned {}",
                self.text.split('\n').count(),
                self.text.len() / 1024,
                sorted.len(),
                stats.calls,
                stats.rebuilds,
                stats.lines_scanned
            ));
        });
        egui::CentralPanel::default().show(ui, |ui| {
            editor_area(ui, &mut self.editor, &mut self.text);
        });
        self.frame_ms.push(start.elapsed().as_secs_f32() * 1000.0);
        log::debug!(
            "frame {:.1} ms at t={:.2}",
            start.elapsed().as_secs_f32() * 1000.0,
            ui.input(|i| i.time)
        );
        if self.frame_ms.len() >= 120 {
            let mut sorted = std::mem::take(&mut self.frame_ms);
            sorted.sort_by(f32::total_cmp);
            log::info!(
                "ui() over 120 frames: p50 {:.2} ms, p95 {:.2} ms, max {:.2} ms",
                sorted[60],
                sorted[114],
                sorted[119]
            );
        }
        if let Some(s) = &mut self.screenshot {
            s.tick(ui.ctx());
        }
    }
}

fn main() -> eframe::Result {
    let mut lines = 5000;
    let mut strategy = Strategy::Incremental;
    let mut path = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--whole-job" => strategy = Strategy::WholeJob,
            "--line-galleys" => strategy = Strategy::LineGalleys,
            "--lines" => lines = args.next().and_then(|n| n.parse().ok()).unwrap_or(lines),
            _ => path = Some(arg),
        }
    }
    let (name, text) = match path {
        Some(p) => (
            p.clone(),
            std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("can't read {p}: {e}")),
        ),
        None => (format!("generated ({lines} lines)"), gen::document(lines)),
    };
    spike_common::run("Spike: Markdown editor", move |cc| {
        let mut editor = MarkdownEditor::new(egui::Id::unique("markdown-editor"));
        editor.highlighter.strategy = strategy;
        cc.egui_ctx.memory_mut(|m| m.request_focus(editor.id));
        App {
            name,
            text,
            editor,
            screenshot: Screenshot::from_env(),
            frame_ms: Vec::new(),
        }
    })
}
