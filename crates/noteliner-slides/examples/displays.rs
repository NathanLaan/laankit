//! A trial for presenting on a chosen display (see
//! `docs/plans/plan-noteliner-present-display.md`, P0). It lists the
//! displays the window reports and puts the window full screen on the one
//! you pick, so you can see what a desktop does with
//! `ViewportCommand::SetMonitor`.
//!
//! ```bash
//! cargo run -p noteliner-slides --example displays
//! ```
//!
//! | Key        | Does                                        |
//! |------------|---------------------------------------------|
//! | 1 to 9     | Full screen on that display                 |
//! | D          | Full screen on the next display             |
//! | F          | Out of full screen                          |
//! | Escape     | Quit                                        |
//!
//! Every change in the list, the window's display and its place is printed
//! to the terminal with the time, so a run can be pasted into the plan.

use std::time::Instant;

use egui::{Key, ViewportCommand};
use noteliner_slides::Display;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("NoteLiner displays trial")
            .with_inner_size([720.0, 420.0]),
        ..Default::default()
    };
    eframe::run_native(
        "noteliner-displays-trial",
        options,
        Box::new(|_| Ok(Box::new(Trial::default()))),
    )
}

struct Trial {
    started: Instant,
    displays: Vec<Display>,
    /// The display last asked for.
    asked: Option<usize>,
    /// What was last printed, to print only changes.
    last: String,
    log: Vec<String>,
}

impl Default for Trial {
    fn default() -> Self {
        Self {
            started: Instant::now(),
            displays: Vec::new(),
            asked: None,
            last: String::new(),
            log: Vec::new(),
        }
    }
}

/// The displays the window reports, in the order `SetMonitor` counts them.
fn displays(frame: &eframe::Frame) -> Vec<Display> {
    let Some(window) = frame.winit_window() else {
        return Vec::new();
    };
    let current = window.current_monitor();
    window
        .available_monitors()
        .enumerate()
        .map(|(index, m)| Display {
            index,
            name: m.name(),
            size: [m.size().width, m.size().height],
            current: current.as_ref() == Some(&m),
        })
        .collect()
}

impl Trial {
    fn note(&mut self, line: String) {
        let line = format!("{:7.2}s  {line}", self.started.elapsed().as_secs_f32());
        println!("{line}");
        self.log.push(line);
        if self.log.len() > 12 {
            self.log.remove(0);
        }
    }

    fn go(&mut self, ctx: &egui::Context, index: usize) {
        self.asked = Some(index);
        self.note(format!("asked for SetMonitor({index})"));
        ctx.send_viewport_cmd(ViewportCommand::SetMonitor(index));
    }
}

impl eframe::App for Trial {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.displays = displays(frame);
        let (fullscreen, outer, scale) = ctx.input(|i| {
            (
                i.viewport().fullscreen,
                i.viewport().outer_rect,
                i.pixels_per_point,
            )
        });
        let state = format!(
            "displays {:?}; full screen {fullscreen:?}; window at {outer:?}; scale {scale}",
            self.displays
                .iter()
                .map(|d| format!("{}{}", d.label(), if d.current { " *" } else { "" }))
                .collect::<Vec<_>>()
        );
        if state != self.last {
            self.last = state.clone();
            self.note(state);
        }

        const DIGITS: [Key; 9] = [
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        let pressed = |k: Key| ctx.input(|i| i.key_pressed(k));
        if let Some(n) = DIGITS.iter().position(|k| pressed(*k)) {
            self.go(&ctx, n);
        }
        if pressed(Key::D) && !self.displays.is_empty() {
            let here = self
                .asked
                .or_else(|| self.displays.iter().position(|d| d.current))
                .unwrap_or(0);
            self.go(&ctx, (here + 1) % self.displays.len());
        }
        if pressed(Key::F) {
            self.asked = None;
            self.note("asked for Fullscreen(false)".into());
            ctx.send_viewport_cmd(ViewportCommand::Fullscreen(false));
        }
        if pressed(Key::Escape) {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }

        egui::CentralPanel::default().show(ui, |ui| {
            ui.heading("Displays");
            if self.displays.is_empty() {
                ui.label("The window reports no displays.");
            }
            for d in &self.displays {
                let mark = if d.current { "  (this window)" } else { "" };
                ui.label(format!("{}{mark}", d.label()));
            }
            ui.add_space(8.0);
            ui.label("1 to 9: full screen on that display.  D: the next display.  F: out of full screen.  Escape: quit.");
            ui.add_space(8.0);
            // Small text, to judge sharpness on a display with another scale.
            ui.label(egui::RichText::new("The quick brown fox jumps over the lazy dog. 0123456789").size(9.0));
            ui.separator();
            for line in &self.log {
                ui.monospace(line);
            }
        });
        // The list is read again each second, to catch a display being unplugged.
        ctx.request_repaint_after(std::time::Duration::from_secs(1));
    }
}
