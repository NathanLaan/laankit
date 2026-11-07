//! The main window's parts: the toolbar, the status bar, the banners, the
//! signed-out welcome, and the printer grid.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use egui::{
    pos2, vec2, Align, Align2, Color32, FontId, Frame, Layout, Margin, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui,
};
use laankit_egui::dialog::{self, ButtonKind};
use laankit_egui::theme::{self, Palette};
use laankit_egui::toolbar::{self, ToolbarButton};
use laankit_egui::{fonts, icons, toast};
use printliner_core::hms::Severity;
use printliner_core::Task;

use crate::backend::Connection;
use crate::commands::Act;
use crate::model::{self, Printer, Tone};

pub const STATUS_BAR_HEIGHT: f32 = 24.0;
pub const CARD_WIDTH: f32 = 320.0;
const CARD_HEIGHT: f32 = 206.0;
const RING: f32 = 76.0;

pub const GREEN: Color32 = Color32::from_rgb(0x2e, 0xa0, 0x43);
pub const AMBER: Color32 = Color32::from_rgb(0xd9, 0x8e, 0x04);

pub fn tone_colour(p: &Palette, tone: Tone) -> Color32 {
    match tone {
        Tone::Busy => p.accent,
        Tone::Good => GREEN,
        Tone::Warn => AMBER,
        Tone::Bad => toast::DANGER,
        Tone::Quiet => p.text_muted,
    }
}

pub fn severity_colour(p: &Palette, s: Severity) -> Color32 {
    match s {
        Severity::Fatal | Severity::Serious => toast::DANGER,
        Severity::Common => AMBER,
        Severity::Info | Severity::Unknown => p.text_muted,
    }
}

pub struct ToolbarProps {
    pub detail_open: bool,
    pub can_refresh: bool,
    pub settings_open: bool,
    pub about_open: bool,
}

pub fn toolbar(ui: &mut Ui, props: &ToolbarProps) -> Vec<Act> {
    let mut out = Vec::new();
    let mut bottom = Vec::new();
    toolbar::show(
        ui,
        |ui| {
            let printers = ToolbarButton::new(icons::TABLE_CELLS_LARGE, "All Printers (Esc)")
                .label("All Printers")
                .active(!props.detail_open);
            if toolbar::button(ui, printers).clicked() {
                out.push(Act::Printers);
            }
            let refresh = ToolbarButton::new(icons::ARROWS_ROTATE, "Refresh (F5)")
                .label("Refresh")
                .disabled(!props.can_refresh);
            if toolbar::button(ui, refresh).clicked() {
                out.push(Act::Refresh);
            }
        },
        |ui| {
            let settings = ToolbarButton::new(icons::GEAR, "Settings (Ctrl+,)")
                .label("Settings")
                .active(props.settings_open);
            if toolbar::button(ui, settings).clicked() {
                bottom.push(Act::Settings);
            }
            let about = ToolbarButton::new(icons::CIRCLE_INFO, "About")
                .label("About")
                .active(props.about_open);
            if toolbar::button(ui, about).clicked() {
                bottom.push(Act::About);
            }
        },
    );
    out.extend(bottom);
    out
}

pub struct StatusProps<'a> {
    pub connection: &'a Connection,
    pub signed_in: bool,
    pub demo: bool,
    pub printers: usize,
    pub printing: usize,
    /// The token is in a file, not the keychain.
    pub token_in_file: bool,
}

pub fn status_bar(ui: &mut Ui, props: &StatusProps<'_>) {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, p.bg_surface);
    painter.hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    let (dot, text) = match (props.demo, props.signed_in, props.connection) {
        (true, _, _) => (p.accent, "Demo printers".to_string()),
        (_, false, _) => (p.text_muted, "Not signed in".to_string()),
        (_, _, Connection::Connected) => (GREEN, "Connected to Bambu Lab".to_string()),
        (_, _, Connection::Connecting) => (AMBER, "Connecting…".to_string()),
        (_, _, Connection::Lost(why)) => (toast::DANGER, format!("Connection lost: {why}")),
        (_, _, Connection::Idle) => (p.text_muted, "Not connected".to_string()),
    };
    painter.circle_filled(pos2(rect.left() + 14.0, y), 4.0, dot);
    let mut last = painter.text(
        pos2(rect.left() + 24.0, y),
        Align2::LEFT_CENTER,
        text,
        font.clone(),
        p.text_secondary,
    );
    if props.signed_in {
        let count = match props.printers {
            1 => "1 printer".to_string(),
            n => format!("{n} printers"),
        };
        let count = if props.printing > 0 {
            format!("{count}, {} printing", props.printing)
        } else {
            count
        };
        last = painter.text(
            pos2(last.right() + 16.0, y),
            Align2::LEFT_CENTER,
            count,
            font.clone(),
            p.text_muted,
        );
    }
    if props.token_in_file {
        painter.text(
            pos2(last.right() + 16.0, y),
            Align2::LEFT_CENTER,
            "Sign-in token stored in a file",
            font,
            AMBER,
        );
    }
}

/// A bar across the top of the content with a message and one button.
/// Returns whether the button was clicked.
pub fn banner(ui: &mut Ui, message: &str, button: &str, tone: Tone) -> bool {
    let p = theme::palette(ui.ctx());
    let colour = tone_colour(&p, tone);
    let mut clicked = false;
    Frame::new()
        .fill(colour.gamma_multiply(0.12))
        .stroke(Stroke::new(1.0, colour.gamma_multiply(0.6)))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(14, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(icons::icon(icons::TRIANGLE_EXCLAMATION).color(colour));
                ui.label(RichText::new(message).color(p.text_primary));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    clicked = dialog::button(ui, button, ButtonKind::Primary, true).clicked();
                });
            });
        });
    ui.add_space(12.0);
    clicked
}

/// The signed-out screen. Returns whether Sign In was pressed.
pub fn welcome(ui: &mut Ui) -> bool {
    let p = theme::palette(ui.ctx());
    let mut clicked = false;
    ui.vertical_centered(|ui| {
        ui.set_max_width(520.0);
        ui.add_space((ui.available_height() * 0.25).max(24.0));
        ui.label(icons::icon_sized(icons::LAYER_GROUP, 48.0).color(p.accent));
        ui.add_space(12.0);
        ui.label(fonts::weight("PrintLiner", fonts::BOLD).size(24.0).color(p.text_primary));
        ui.add_space(6.0);
        ui.label(
            RichText::new("Sign in to your Bambu Lab account to see your printers.")
                .color(p.text_secondary),
        );
        ui.add_space(16.0);
        clicked = dialog::button(ui, "Sign In", ButtonKind::Primary, true).clicked();
        ui.add_space(12.0);
        dialog::muted_text(
            ui,
            "Your password is used only to sign in and is never saved. To try PrintLiner without an account, run printliner --demo.",
        );
    });
    clicked
}

/// A centred message for an empty grid.
pub fn empty(ui: &mut Ui, message: &str) {
    ui.vertical_centered(|ui| {
        ui.add_space((ui.available_height() * 0.3).max(24.0));
        dialog::muted_text(ui, message);
    });
}

/// What the grid needs besides the printers.
pub struct GridProps<'a> {
    pub now: DateTime<Utc>,
    pub connected: bool,
    pub tasks: &'a HashMap<String, Vec<Task>>,
    pub covers: &'a HashMap<String, Arc<[u8]>>,
}

/// The printer cards. Returns the serial of a clicked card, and the covers
/// the cards would show but haven't got (task id, url).
pub fn printer_grid(
    ui: &mut Ui,
    printers: &[Printer],
    props: &GridProps<'_>,
) -> (Option<String>, Vec<(String, String)>) {
    let mut clicked = None;
    let mut wanted = Vec::new();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(16.0, 16.0);
        for printer in printers {
            let task = current_task(printer, props.tasks);
            let cover = task.and_then(|t| props.covers.get(&t.id).map(|b| (t.id.as_str(), b)));
            if let (Some(t), None) = (task, cover) {
                if let Some(url) = &t.cover {
                    wanted.push((t.id.clone(), url.clone()));
                }
            }
            if card(ui, printer, props, cover).clicked() {
                clicked = Some(printer.device.serial.clone());
            }
        }
    });
    (clicked, wanted)
}

/// The cloud task for the printer's current job, matched by id.
pub fn current_task<'a>(
    printer: &Printer,
    tasks: &'a HashMap<String, Vec<Task>>,
) -> Option<&'a Task> {
    let job = printer.status.as_ref()?.job_id.as_deref()?;
    tasks
        .get(&printer.device.serial)?
        .iter()
        .find(|t| t.id == job)
}

fn card(
    ui: &mut Ui,
    printer: &Printer,
    props: &GridProps<'_>,
    cover: Option<(&str, &Arc<[u8]>)>,
) -> egui::Response {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(CARD_WIDTH, CARD_HEIGHT), Sense::hover());
    let id = ui.scope_id().with(("card", &printer.device.serial));
    let resp = ui.interact(rect, id, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &printer.name));
    let hovered = resp.hovered();
    ui.painter().rect(
        rect,
        theme::RADIUS_LARGE,
        if hovered {
            p.bg_item_hover
        } else {
            p.bg_surface
        },
        Stroke::new(1.0, if hovered { p.border_hover } else { p.border }),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(vec2(16.0, 14.0));
    let mut child = ui.new_child(
        egui::UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::top_down(Align::Min)),
    );
    child.style_mut().interaction.selectable_labels = false;
    card_contents(&mut child, &p, printer, props, cover);
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

fn card_contents(
    ui: &mut Ui,
    p: &Palette,
    printer: &Printer,
    props: &GridProps<'_>,
    cover: Option<(&str, &Arc<[u8]>)>,
) {
    let (state, tone) = printer.state(props.now, props.connected);
    let colour = tone_colour(p, tone);
    // Name and model, the state on the right.
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.label(
                fonts::weight(printer.name.as_str(), fonts::SEMIBOLD)
                    .size(15.0)
                    .color(p.text_primary),
            );
            ui.label(
                RichText::new(printer.device.model_label())
                    .size(12.0)
                    .color(p.text_muted),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Min), |ui| {
            chip(ui, state, colour);
        });
    });
    ui.add_space(10.0);
    let status = printer.status.as_ref();
    let active = status.is_some_and(|s| s.state.is_active()) && printer.fresh(props.now);
    ui.horizontal(|ui| {
        let percent = status.and_then(|s| s.percent).filter(|_| {
            status.is_some_and(|s| {
                s.state.is_active() || s.state == printliner_core::GcodeState::Finish
            })
        });
        match cover {
            Some((task_id, bytes)) if active => {
                let image = egui::Image::from_bytes(
                    format!("bytes://cover/{task_id}"),
                    egui::load::Bytes::Shared(bytes.clone()),
                )
                .fit_to_exact_size(vec2(RING, RING))
                .corner_radius(theme::RADIUS);
                ui.add(image);
            }
            _ => ring(ui, p, percent, colour),
        }
        ui.add_space(8.0);
        ui.vertical(|ui| {
            let line = |ui: &mut Ui, text: String, strong: bool| {
                let t = RichText::new(text).size(13.0);
                ui.add(
                    egui::Label::new(if strong {
                        t.color(p.text_primary)
                    } else {
                        t.color(p.text_secondary)
                    })
                    .truncate(),
                );
            };
            match status {
                Some(s)
                    if s.state.is_active()
                        || matches!(
                            s.state,
                            printliner_core::GcodeState::Finish
                                | printliner_core::GcodeState::Failed
                        ) =>
                {
                    if let Some(job) = &s.job_name {
                        line(ui, job.clone(), true);
                    }
                    if s.state.is_active() {
                        if let Some(m) = s.remaining_min {
                            line(ui, format!("{} left", model::minutes(m)), false);
                        }
                        if let (Some(l), Some(t)) = (s.layer, s.total_layers) {
                            line(ui, format!("Layer {l} of {t}"), false);
                        }
                    }
                }
                Some(s) => {
                    line(
                        ui,
                        format!(
                            "Nozzle {}",
                            model::temperature(s.nozzle_temp, s.nozzle_target)
                        ),
                        false,
                    );
                    line(
                        ui,
                        format!("Bed {}", model::temperature(s.bed_temp, s.bed_target)),
                        false,
                    );
                }
                None => line(ui, "No status yet".into(), false),
            }
        });
    });
    // Filament and alerts along the bottom.
    let bottom = ui.max_rect().bottom();
    let row = Rect::from_min_max(pos2(ui.max_rect().left(), bottom - 20.0), ui.max_rect().max);
    ui.scope_builder(egui::UiBuilder::new().max_rect(row), |ui| {
        ui.horizontal(|ui| {
            if let Some(tray) = status.and_then(|s| s.ams.active_tray()) {
                swatch(ui, p, tray.colour);
                ui.label(
                    RichText::new(tray.material.as_deref().unwrap_or("Filament"))
                        .size(12.0)
                        .color(p.text_secondary),
                );
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(worst) = printer.worst_alert() {
                    let n = status.map_or(0, |s| s.hms.len());
                    let text = if n == 1 {
                        "1 alert".to_string()
                    } else {
                        format!("{n} alerts")
                    };
                    let c = severity_colour(p, worst);
                    ui.label(RichText::new(text).size(12.0).color(c));
                    ui.label(icons::icon_sized(icons::TRIANGLE_EXCLAMATION, 12.0).color(c));
                }
            });
        });
    });
}

/// A small rounded label in the state's colour.
pub fn chip(ui: &mut Ui, text: &str, colour: Color32) {
    Frame::new()
        .fill(colour.gamma_multiply(0.15))
        .corner_radius(theme::RADIUS)
        .inner_margin(Margin::symmetric(8, 3))
        .show(ui, |ui| {
            ui.label(
                fonts::weight(text, fonts::SEMIBOLD)
                    .size(12.0)
                    .color(colour),
            );
        });
}

/// A filament colour dot.
pub fn swatch(ui: &mut Ui, p: &Palette, colour: Option<[u8; 4]>) {
    let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
    let fill = colour
        .map(|[r, g, b, _]| Color32::from_rgb(r, g, b))
        .unwrap_or(Color32::TRANSPARENT);
    ui.painter()
        .circle(rect.center(), 5.5, fill, Stroke::new(1.0, p.border_hover));
}

/// The progress ring, with the percentage in the middle.
fn ring(ui: &mut Ui, p: &Palette, percent: Option<u8>, colour: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(RING, RING), Sense::hover());
    let painter = ui.painter();
    let centre = rect.center();
    let radius = RING / 2.0 - 5.0;
    painter.circle_stroke(centre, radius, Stroke::new(6.0, p.bg_button));
    if let Some(pc) = percent {
        let frac = f32::from(pc) / 100.0;
        if frac > 0.0 {
            let steps = (64.0 * frac).ceil().max(2.0) as usize;
            let start = -std::f32::consts::FRAC_PI_2;
            let points: Vec<egui::Pos2> = (0..=steps)
                .map(|i| {
                    let a = start + std::f32::consts::TAU * frac * i as f32 / steps as f32;
                    centre + radius * vec2(a.cos(), a.sin())
                })
                .collect();
            painter.add(Shape::line(points, Stroke::new(6.0, colour)));
        }
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            format!("{pc}%"),
            FontId::proportional(16.0),
            p.text_primary,
        );
    } else {
        painter.text(
            centre,
            Align2::CENTER_CENTER,
            "-",
            FontId::proportional(16.0),
            p.text_muted,
        );
    }
}
