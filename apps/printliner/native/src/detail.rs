//! One printer in full: status, temperatures, AMS, HMS alerts, the trends
//! chart, and task history.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Local, Utc};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, RichText, Sense,
    Shape, Stroke, Ui,
};
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::theme::{self, Palette};
use laankit_egui::{fonts, icons, toast};
use printliner_core::history::Sample;
use printliner_core::{hms, PrinterStatus, Task};

use crate::config::TrendRange;
use crate::model::{self, Printer};
use crate::views::{self, AMBER, GREEN};

/// Two columns from this width.
const TWO_COLUMNS: f32 = 900.0;
const CHART_HEIGHT: f32 = 180.0;
const THUMB: f32 = 48.0;

pub struct DetailProps<'a> {
    pub now: DateTime<Utc>,
    pub connected: bool,
    pub tasks: Option<&'a Vec<Task>>,
    pub covers: &'a HashMap<String, Arc<[u8]>>,
    pub samples: Option<&'a Vec<Sample>>,
    pub range: TrendRange,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DetailAction {
    Back,
    Range(TrendRange),
    /// Covers the history list would show but hasn't got (task id, url).
    WantCovers(Vec<(String, String)>),
    OpenUrl(String),
}

pub fn show(ui: &mut Ui, printer: &Printer, props: &DetailProps<'_>) -> Vec<DetailAction> {
    let p = theme::palette(ui.ctx());
    let mut out = Vec::new();
    // Header: back, name, model, state.
    ui.horizontal(|ui| {
        if dialog::icon_button(
            ui,
            Some(icons::ARROW_LEFT),
            "All Printers",
            ButtonKind::Secondary,
            true,
        )
        .clicked()
        {
            out.push(DetailAction::Back);
        }
        ui.add_space(12.0);
        ui.label(
            fonts::weight(printer.name.as_str(), fonts::BOLD)
                .size(20.0)
                .color(p.text_primary),
        );
        ui.label(
            RichText::new(format!(
                "{}  ·  {}",
                printer.device.model_label(),
                printer.device.serial
            ))
            .color(p.text_muted),
        );
        let (state, tone) = printer.state(props.now, props.connected);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            views::chip(ui, state, views::tone_colour(&p, tone));
        });
    });
    ui.add_space(12.0);
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let status = printer.status.clone().unwrap_or_default();
            if ui.available_width() >= TWO_COLUMNS {
                ui.columns(2, |cols| {
                    left_column(&mut cols[0], &p, &status, &mut out);
                    right_column(&mut cols[1], &p, &status, props, &mut out);
                });
            } else {
                left_column(ui, &p, &status, &mut out);
                right_column(ui, &p, &status, props, &mut out);
            }
        });
    out
}

fn left_column(ui: &mut Ui, p: &Palette, s: &PrinterStatus, out: &mut Vec<DetailAction>) {
    section(ui, p, "Job", |ui| job(ui, p, s));
    section(ui, p, "Temperatures", |ui| {
        rows(
            ui,
            p,
            &[
                ("Nozzle", model::temperature(s.nozzle_temp, s.nozzle_target)),
                ("Bed", model::temperature(s.bed_temp, s.bed_target)),
                ("Chamber", model::temperature(s.chamber_temp, None)),
            ],
        );
    });
    section(ui, p, "Filament", |ui| ams(ui, p, s));
    section(ui, p, "Alerts", |ui| alerts(ui, p, s, out));
}

fn right_column(
    ui: &mut Ui,
    p: &Palette,
    s: &PrinterStatus,
    props: &DetailProps<'_>,
    out: &mut Vec<DetailAction>,
) {
    let _ = s;
    section(ui, p, "Trends", |ui| {
        ui.horizontal(|ui| {
            for r in [TrendRange::Day, TrendRange::Week] {
                let kind = if r == props.range {
                    ButtonKind::Primary
                } else {
                    ButtonKind::Secondary
                };
                if dialog::button(ui, r.label(), kind, true).clicked() && r != props.range {
                    out.push(DetailAction::Range(r));
                }
            }
        });
        ui.add_space(8.0);
        match props.samples {
            Some(samples) if !samples.is_empty() => {
                chart(ui, p, samples, props.now, props.range);
            }
            Some(_) => dialog::muted_text(
                ui,
                "No history yet. PrintLiner records a sample every minute while it's open.",
            ),
            None => dialog::muted_text(ui, "Loading…"),
        }
    });
    section(ui, p, "Task History", |ui| match props.tasks {
        Some(tasks) if !tasks.is_empty() => {
            let mut wanted = Vec::new();
            for t in tasks {
                task_row(ui, p, t, props.covers, &mut wanted);
            }
            if !wanted.is_empty() {
                out.push(DetailAction::WantCovers(wanted));
            }
        }
        Some(_) => dialog::muted_text(ui, "No tasks on this printer yet."),
        None => dialog::muted_text(ui, "Loading…"),
    });
}

fn section(ui: &mut Ui, p: &Palette, title: &str, body: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(p.bg_surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(theme::RADIUS_LARGE)
        .inner_margin(Margin::same(16))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            dialog::field_label(ui, title);
            ui.add_space(6.0);
            body(ui);
        });
    ui.add_space(12.0);
}

/// Label and value pairs, the values right-aligned.
fn rows(ui: &mut Ui, p: &Palette, items: &[(&str, String)]) {
    for (label, value) in items {
        ui.horizontal(|ui| {
            ui.label(RichText::new(*label).color(p.text_secondary));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(value).color(p.text_primary));
            });
        });
    }
}

fn job(ui: &mut Ui, p: &Palette, s: &PrinterStatus) {
    let name = s.job_name.clone().unwrap_or_else(|| "-".into());
    ui.label(
        fonts::weight(name, fonts::SEMIBOLD)
            .size(15.0)
            .color(p.text_primary),
    );
    if let Some(pc) = s.percent.filter(|_| s.state.is_active()) {
        ui.add_space(6.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 8.0), Sense::hover());
        ui.painter().rect_filled(rect, 4.0, p.bg_button);
        let mut done = rect;
        done.set_width(rect.width() * f32::from(pc) / 100.0);
        ui.painter().rect_filled(done, 4.0, p.accent);
        ui.add_space(6.0);
    }
    let mut items = vec![("State", s.state.label().to_string())];
    if s.state.is_active() {
        if let Some(pc) = s.percent {
            items.push(("Progress", format!("{pc}%")));
        }
        if let Some(m) = s.remaining_min {
            items.push(("Time Left", model::minutes(m)));
        }
    }
    if let (Some(l), Some(t)) = (s.layer, s.total_layers) {
        items.push(("Layer", format!("{l} of {t}")));
    }
    if let Some(speed) = s.speed_label() {
        items.push(("Speed", speed.to_string()));
    }
    if let Some(w) = &s.wifi_signal {
        items.push(("Wi-Fi", w.clone()));
    }
    if let Some(e) = s.print_error {
        items.push(("Error Code", format!("{e:08X}")));
    }
    rows(ui, p, &items);
}

fn ams(ui: &mut Ui, p: &Palette, s: &PrinterStatus) {
    if s.ams.units.is_empty() && s.ams.external.is_none() {
        dialog::muted_text(ui, "No AMS or spool reported.");
        return;
    }
    for unit in &s.ams.units {
        let mut title = format!("AMS {}", unit.id + 1);
        if let Some(h) = &unit.humidity {
            title.push_str(&format!("  ·  humidity {h} of 5"));
        }
        ui.label(RichText::new(title).size(12.0).color(p.text_muted));
        for tray in &unit.trays {
            tray_row(ui, p, &format!("Slot {}", tray.id + 1), tray);
        }
        ui.add_space(6.0);
    }
    if let Some(ext) = &s.ams.external {
        tray_row(ui, p, "External Spool", ext);
    }
}

fn tray_row(ui: &mut Ui, p: &Palette, label: &str, tray: &printliner_core::ams::Tray) {
    ui.horizontal(|ui| {
        views::swatch(ui, p, tray.colour);
        ui.label(RichText::new(label).color(p.text_secondary));
        let material = tray.material.as_deref().unwrap_or("Empty");
        ui.label(RichText::new(material).color(if tray.is_empty() {
            p.text_muted
        } else {
            p.text_primary
        }));
        if tray.active {
            views::chip(ui, "In Use", p.accent);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(r) = tray.remain {
                ui.label(RichText::new(format!("{r}%")).color(p.text_secondary));
            }
        });
    });
}

fn alerts(ui: &mut Ui, p: &Palette, s: &PrinterStatus, out: &mut Vec<DetailAction>) {
    if s.hms.is_empty() {
        ui.horizontal(|ui| {
            ui.label(icons::icon(icons::CIRCLE_CHECK).color(GREEN));
            ui.label(RichText::new("No alerts").color(p.text_secondary));
        });
        return;
    }
    for h in &s.hms {
        ui.horizontal(|ui| {
            let c = views::severity_colour(p, h.severity());
            ui.label(icons::icon(icons::TRIANGLE_EXCLAMATION).color(c));
            ui.label(RichText::new(h.severity().label()).color(c));
            ui.label(
                RichText::new(h.display_code())
                    .monospace()
                    .color(p.text_primary),
            );
        });
    }
    ui.add_space(6.0);
    if ui
        .link(RichText::new("Look up these codes on Bambu Lab's HMS wiki").color(p.accent))
        .clicked()
    {
        out.push(DetailAction::OpenUrl(hms::WIKI_URL.to_string()));
    }
}

fn task_row(
    ui: &mut Ui,
    p: &Palette,
    t: &Task,
    covers: &HashMap<String, Arc<[u8]>>,
    wanted: &mut Vec<(String, String)>,
) {
    ui.horizontal(|ui| {
        match covers.get(&t.id) {
            Some(bytes) => {
                ui.add(
                    egui::Image::from_bytes(
                        format!("bytes://cover/{}", t.id),
                        egui::load::Bytes::Shared(bytes.clone()),
                    )
                    .fit_to_exact_size(vec2(THUMB, THUMB))
                    .corner_radius(theme::RADIUS),
                );
            }
            None => {
                if let Some(url) = &t.cover {
                    wanted.push((t.id.clone(), url.clone()));
                }
                let (rect, _) = ui.allocate_exact_size(vec2(THUMB, THUMB), Sense::hover());
                ui.painter().rect_filled(rect, theme::RADIUS, p.bg_button);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    icons::LAYER_GROUP,
                    icons::font(16.0),
                    p.text_faint,
                );
            }
        }
        ui.vertical(|ui| {
            let title = if t.title.is_empty() {
                "Untitled"
            } else {
                &t.title
            };
            ui.add(egui::Label::new(RichText::new(title).color(p.text_primary)).truncate());
            let mut bits = Vec::new();
            if let Some(start) = t.start {
                bits.push(
                    start
                        .with_timezone(&Local)
                        .format("%b %-d, %H:%M")
                        .to_string(),
                );
            }
            if let Some(d) = t.duration_secs {
                bits.push(model::duration(d));
            }
            if let Some(g) = t.weight_g {
                bits.push(format!("{g:.1} g"));
            }
            ui.label(
                RichText::new(bits.join("  ·  "))
                    .size(12.0)
                    .color(p.text_muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let (text, colour) = if t.succeeded() {
                ("Finished", GREEN)
            } else if t.failed() {
                ("Failed", toast::DANGER)
            } else if t.cancelled() {
                ("Cancelled", AMBER)
            } else if t.status.is_empty() {
                ("-", p.text_muted)
            } else {
                ("In Progress", p.accent)
            };
            views::chip(ui, text, colour);
        });
    });
    ui.add_space(6.0);
}

/// A chart line: its name, colour, and value.
type Series = (&'static str, Color32, fn(&Sample) -> Option<f32>);

/// Temperatures as lines and progress as a shaded band, over the range.
fn chart(ui: &mut Ui, p: &Palette, samples: &[Sample], now: DateTime<Utc>, range: TrendRange) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), CHART_HEIGHT), Sense::hover());
    let painter = ui.painter_at(rect);
    let plot = Rect::from_min_max(rect.min + vec2(36.0, 6.0), rect.max - vec2(6.0, 22.0));
    let start = now - range.duration();
    let span = range.duration().num_seconds().max(1) as f32;
    let x_of =
        |t: DateTime<Utc>| plot.left() + plot.width() * ((t - start).num_seconds() as f32 / span);
    let top = samples
        .iter()
        .flat_map(|s| [s.nozzle, s.bed, s.chamber])
        .flatten()
        .fold(50.0_f32, f32::max);
    // Four steps of a round size.
    let step = [10.0, 20.0, 25.0, 50.0, 60.0, 75.0, 100.0]
        .into_iter()
        .find(|s| s * 4.0 >= top)
        .unwrap_or(100.0);
    let top = step * 4.0;
    let y_of = |v: f32| plot.bottom() - plot.height() * (v / top).clamp(0.0, 1.0);
    let small = FontId::proportional(11.0);
    // Grid and axis labels.
    for i in 0..=4 {
        let v = top * i as f32 / 4.0;
        let y = y_of(v);
        painter.hline(plot.x_range(), y, Stroke::new(1.0, p.border));
        painter.text(
            pos2(plot.left() - 6.0, y),
            Align2::RIGHT_CENTER,
            format!("{v:.0}°"),
            small.clone(),
            p.text_muted,
        );
    }
    let ticks = match range {
        TrendRange::Day => (0..=4)
            .map(|i| start + chrono::Duration::hours(6 * i))
            .collect::<Vec<_>>(),
        TrendRange::Week => (0..=7).map(|i| start + chrono::Duration::days(i)).collect(),
    };
    let last = ticks.len() - 1;
    for (i, t) in ticks.into_iter().enumerate() {
        let align = match i {
            0 => Align2::LEFT_TOP,
            i if i == last => Align2::RIGHT_TOP,
            _ => Align2::CENTER_TOP,
        };
        let label = match range {
            TrendRange::Day => t.with_timezone(&Local).format("%H:%M").to_string(),
            TrendRange::Week => t.with_timezone(&Local).format("%a").to_string(),
        };
        painter.text(
            pos2(x_of(t), plot.bottom() + 4.0),
            align,
            label,
            small.clone(),
            p.text_muted,
        );
    }
    // Progress: a band from the bottom, scaled to the plot's height.
    let band = p.accent.gamma_multiply(0.12);
    for pair in samples.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if let (Some(pa), Some(_)) = (a.percent, b.percent) {
            let h = plot.height() * f32::from(pa) / 100.0;
            let r = Rect::from_min_max(
                pos2(x_of(a.at), plot.bottom() - h),
                pos2(x_of(b.at).max(x_of(a.at) + 1.0), plot.bottom()),
            );
            painter.rect_filled(r, 0.0, band);
        }
    }
    // The temperature lines, broken where a sample is missing or there's a
    // gap of more than 10 minutes (PrintLiner was closed).
    let series: [Series; 3] = [
        ("Nozzle", toast::DANGER, |s| s.nozzle),
        ("Bed", AMBER, |s| s.bed),
        ("Chamber", p.accent, |s| s.chamber),
    ];
    for (_, colour, value) in series {
        let mut line: Vec<egui::Pos2> = Vec::new();
        let mut last: Option<DateTime<Utc>> = None;
        for s in samples {
            let gap = last.is_some_and(|l| (s.at - l).num_minutes() > 10);
            match value(s) {
                Some(v) if !gap => line.push(pos2(x_of(s.at), y_of(v))),
                Some(v) => {
                    flush_line(&painter, &mut line, colour);
                    line.push(pos2(x_of(s.at), y_of(v)));
                }
                None => flush_line(&painter, &mut line, colour),
            }
            last = Some(s.at);
        }
        flush_line(&painter, &mut line, colour);
    }
    // Legend.
    ui.horizontal(|ui| {
        for (name, colour, _) in series {
            let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
            ui.painter().rect_filled(r, 2.0, colour);
            ui.label(RichText::new(name).size(12.0).color(p.text_secondary));
            ui.add_space(8.0);
        }
        let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
        ui.painter().rect_filled(r, 2.0, band.gamma_multiply(3.0));
        ui.label(RichText::new("Progress").size(12.0).color(p.text_secondary));
    });
    // The value under the pointer.
    if let Some(pos) = resp.hover_pos() {
        if let Some(s) = samples
            .iter()
            .min_by_key(|s| (x_of(s.at) - pos.x).abs() as i64)
        {
            let mut text =
                s.at.with_timezone(&Local)
                    .format("%b %-d %H:%M")
                    .to_string();
            text.push_str(&format!("\n{}", s.state));
            if let Some(v) = s.nozzle {
                text.push_str(&format!("\nNozzle {v:.0}°"));
            }
            if let Some(v) = s.bed {
                text.push_str(&format!("\nBed {v:.0}°"));
            }
            if let Some(pc) = s.percent {
                text.push_str(&format!("\nProgress {pc}%"));
            }
            resp.on_hover_text_at_pointer(text);
        }
    }
}

fn flush_line(painter: &egui::Painter, line: &mut Vec<egui::Pos2>, colour: Color32) {
    if line.len() >= 2 {
        painter.add(Shape::line(std::mem::take(line), Stroke::new(1.5, colour)));
    } else {
        line.clear();
    }
}
