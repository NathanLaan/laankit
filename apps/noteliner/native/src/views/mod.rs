//! The screens and panels: `OpenScreen.svelte`, the sidebar and its file
//! tree, the preview, `StatusBar.svelte` and `LogPanel.svelte`.

pub mod attachments;
pub mod history;
pub mod panes;
pub mod preview;
pub mod sidebar;
pub mod tree;

use egui::{pos2, vec2, Align2, Color32, CornerRadius, FontId, Rect, RichText, Sense, Stroke, Ui};
use laankit_core::git::SyncStatus;
use laankit_egui::theme::{self, Palette};
use laankit_egui::{fonts, icons};

use crate::config::Recent;
use crate::editor::SaveStatus;

/// `.status-bar { height: 24px }`.
pub const STATUS_BAR_HEIGHT: f32 = 24.0;
/// The Log panel keeps the latest lines.
pub const LOG_CAP: usize = 1000;

/// What the user did on the open screen.
#[derive(Clone, Debug, PartialEq)]
pub enum OpenAction {
    NewProject,
    OpenFolder,
    OpenRecent(String),
    RemoveRecent(Recent),
}

/// `OpenScreen.svelte`: the title, New / Open Project, and recent projects.
pub fn open_screen(ui: &mut Ui, recent: &[Recent]) -> Option<OpenAction> {
    let p = theme::palette(ui.ctx());
    let mut out = None;
    egui::ScrollArea::vertical()
        .id_salt("noteliner-open")
        .auto_shrink(false)
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::same(48))
                .show(ui, |ui| {
                    ui.set_max_width(520.0);
                    ui.label(fonts::weight(
                        RichText::new("NoteLiner").size(36.0).color(p.text_primary),
                        300.0,
                    ));
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new("An outliner for your thoughts")
                            .size(16.0)
                            .color(p.text_muted),
                    );
                    ui.add_space(28.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        if action_button(ui, &p, icons::PLUS, "New Project").clicked() {
                            out = Some(OpenAction::NewProject);
                        }
                        if action_button(ui, &p, icons::FOLDER_OPEN, "Open Project").clicked() {
                            out = Some(OpenAction::OpenFolder);
                        }
                    });
                    if recent.is_empty() {
                        return;
                    }
                    ui.add_space(36.0);
                    ui.label(
                        fonts::weight("RECENT PROJECTS", fonts::SEMIBOLD)
                            .size(11.0)
                            .color(p.text_muted)
                            .extra_letter_spacing(0.5),
                    );
                    ui.add_space(8.0);
                    for r in recent {
                        if let Some(a) = recent_row(ui, &p, r) {
                            out = Some(a);
                        }
                    }
                });
        });
    out
}

/// `.action-btn`: an outlined button with an icon.
fn action_button(ui: &mut Ui, p: &Palette, glyph: &str, label: &str) -> egui::Response {
    let font = FontId::proportional(14.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font, p.text_primary);
    let size = vec2(galley.size().x + 52.0, 40.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let hovered = resp.hovered();
    ui.painter().rect(
        rect,
        CornerRadius::same(6),
        if hovered {
            p.bg_button_hover
        } else {
            p.bg_button
        },
        Stroke::new(1.0, if hovered { p.accent } else { p.border }),
        egui::StrokeKind::Inside,
    );
    ui.painter().text(
        pos2(rect.left() + 20.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        icons::font(13.0),
        p.accent,
    );
    ui.painter().galley(
        pos2(rect.left() + 36.0, rect.center().y - galley.size().y / 2.0),
        galley,
        p.text_primary,
    );
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `.recent-item`: folder icon, name and path, and an X to remove it.
fn recent_row(ui: &mut Ui, p: &Palette, r: &Recent) -> Option<OpenAction> {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 52.0), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &r.name));
    let x_rect =
        Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), vec2(26.0, 26.0));
    let x = ui.interact(
        x_rect,
        ui.scope_id().with(("recent-x", &r.path)),
        Sense::click(),
    );
    x.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Remove from list"));
    let painter = ui.painter();
    if resp.hovered() || x.hovered() {
        painter.rect_filled(rect, CornerRadius::same(6), p.bg_item_hover);
    }
    painter.text(
        pos2(rect.left() + 18.0, rect.center().y),
        Align2::CENTER_CENTER,
        icons::FOLDER,
        icons::font(14.0),
        p.accent,
    );
    painter.text(
        pos2(rect.left() + 40.0, rect.top() + 16.0),
        Align2::LEFT_CENTER,
        &r.name,
        FontId::proportional(14.0),
        p.text_primary,
    );
    painter.text(
        pos2(rect.left() + 40.0, rect.top() + 36.0),
        Align2::LEFT_CENTER,
        tilde_path(&r.path),
        FontId::proportional(12.0),
        p.text_muted,
    );
    if resp.hovered() || x.hovered() {
        painter.text(
            x_rect.center(),
            Align2::CENTER_CENTER,
            icons::XMARK,
            icons::font(12.0),
            if x.hovered() {
                p.text_primary
            } else {
                p.text_muted
            },
        );
    }
    let x = x.on_hover_text("Remove from list");
    if x.clicked() {
        return Some(OpenAction::RemoveRecent(r.clone()));
    }
    resp.clicked()
        .then(|| OpenAction::OpenRecent(r.path.clone()))
}

/// `formatPath`: the home folder as `~`.
pub fn tilde_path(path: &str) -> String {
    let home = laankit_core::paths::home_dir().display().to_string();
    match path.strip_prefix(&home) {
        Some(rest) if !home.is_empty() && home != "/" => format!("~{rest}"),
        _ => path.to_string(),
    }
}

/// What the status bar shows (`StatusBar.svelte`).
pub struct StatusProps<'a> {
    pub project_open: bool,
    pub note_count: usize,
    /// The open note: its name and save status.
    pub note: Option<(&'a str, SaveStatus)>,
    pub text: &'a str,
    pub cursor: (usize, usize),
    pub selection_len: usize,
    pub spell_check: bool,
    /// A deck's current slide, slide count and speaking minutes (`deckInfo`).
    pub deck: Option<(usize, usize, usize)>,
    pub mcp_running: bool,
    pub branch: Option<&'a str>,
    pub sync: Option<&'a SyncStatus>,
}

const GREEN: Color32 = Color32::from_rgb(0x4c, 0xaf, 0x50);
const BLUE: Color32 = Color32::from_rgb(0x42, 0x8b, 0xe8);
const ORANGE: Color32 = Color32::from_rgb(0xf0, 0x9a, 0x2b);
const GREY: Color32 = Color32::from_gray(0x88);

/// `syncDotClass`.
fn sync_dot(s: Option<&SyncStatus>) -> Color32 {
    match s {
        Some(SyncStatus::Synced) => GREEN,
        Some(SyncStatus::Ahead { .. }) => BLUE,
        Some(SyncStatus::Behind { .. }) => ORANGE,
        Some(SyncStatus::Diverged { .. } | SyncStatus::Error { .. }) => laankit_egui::toast::DANGER,
        _ => GREY,
    }
}

/// `syncLabel`.
pub fn sync_label(s: Option<&SyncStatus>) -> String {
    match s {
        Some(SyncStatus::Synced) => "Synced".into(),
        Some(SyncStatus::Ahead { count }) => format!("↑{count}"),
        Some(SyncStatus::Behind { count }) => format!("↓{count}"),
        Some(SyncStatus::Diverged { ahead, behind }) => format!("↑{ahead} ↓{behind}"),
        Some(SyncStatus::NoUpstream) => "No upstream".into(),
        Some(SyncStatus::Error { .. }) => "Sync error".into(),
        _ => String::new(),
    }
}

/// Word count as `/\S+/g` counts it.
pub fn word_count(text: &str) -> usize {
    text.split_whitespace().count()
}

/// Draws the status bar; returns `true` when the branch chip was clicked.
pub fn status_bar(ui: &mut Ui, s: &StatusProps<'_>) -> bool {
    let p = theme::palette(ui.ctx());
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, p.bg_surface);
    ui.painter()
        .hline(rect.x_range(), rect.top(), Stroke::new(1.0, p.border));
    let font = FontId::proportional(12.0);
    let y = rect.center().y;
    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });

    // Left: project and note.
    let mut x = rect.left() + 8.0;
    let mut seg = |ui: &Ui, text: &str, color: Color32, dot: Option<Color32>| {
        if let Some(c) = dot {
            ui.painter().circle_filled(pos2(x + 4.0, y), 4.0, c);
            x += 12.0;
        }
        let r = ui
            .painter()
            .text(pos2(x, y), Align2::LEFT_CENTER, text, font.clone(), color);
        x = r.right() + 12.0;
    };
    if !s.project_open {
        seg(ui, "No project open", p.text_muted, None);
    } else {
        seg(ui, &plural(s.note_count, "note"), p.text_muted, None);
        match s.note {
            Some((name, status)) => {
                let dot = match status {
                    SaveStatus::Saved => GREEN,
                    SaveStatus::Saving => ORANGE,
                    SaveStatus::Unsaved => GREY,
                };
                seg(ui, status.label(), p.text_secondary, Some(dot));
                seg(ui, name, p.text_secondary, None);
            }
            None => seg(ui, "No file selected", p.text_muted, None),
        }
    }

    // Center: the selection.
    if s.selection_len > 0 {
        ui.painter().text(
            pos2(rect.center().x, y),
            Align2::CENTER_CENTER,
            format!("{} selected", s.selection_len),
            font.clone(),
            p.text_secondary,
        );
    }

    // Right, laid out from the right edge: branch chip, MCP, then the note's details.
    let mut right = rect.right() - 8.0;
    let mut clicked = false;
    if let Some(branch) = s.branch {
        let label = sync_label(s.sync);
        let text = if label.is_empty() {
            branch.to_string()
        } else {
            format!("{branch}  {label}")
        };
        let galley = ui
            .painter()
            .layout_no_wrap(text, font.clone(), p.text_secondary);
        let w = galley.size().x + 36.0;
        let chip = Rect::from_min_max(
            pos2(right - w, rect.top() + 2.0),
            pos2(right, rect.bottom() - 2.0),
        );
        let resp = ui.interact(chip, ui.scope_id().with("branch-chip"), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Remote Sync"));
        if resp.hovered() {
            ui.painter()
                .rect_filled(chip, CornerRadius::same(3), p.bg_item_hover);
        }
        ui.painter().text(
            pos2(chip.left() + 10.0, y),
            Align2::CENTER_CENTER,
            icons::CODE_BRANCH,
            icons::font(10.0),
            p.text_secondary,
        );
        ui.painter()
            .circle_filled(pos2(chip.right() - 8.0, y), 4.0, sync_dot(s.sync));
        ui.painter().galley(
            pos2(chip.left() + 20.0, y - galley.size().y / 2.0),
            galley,
            p.text_secondary,
        );
        clicked = resp.on_hover_text("Open Remote Sync").clicked();
        right = chip.left() - 12.0;
    }
    let mut seg_right = |ui: &Ui, text: String, color: Color32| {
        let r = ui.painter().text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            text,
            font.clone(),
            color,
        );
        right = r.left() - 12.0;
    };
    if s.mcp_running {
        seg_right(ui, format!("{} MCP", icons::PLUG), p.text_muted);
    }
    if s.note.is_some() {
        if let Some((current, total, minutes)) = s.deck {
            seg_right(ui, format!("~{minutes} min"), p.text_muted);
            seg_right(
                ui,
                format!("{}  Slide {current}/{total}", icons::PERSON_CHALKBOARD),
                p.text_secondary,
            );
        }
        let chars = s.text.chars().count();
        seg_right(ui, plural(chars, "char"), p.text_muted);
        seg_right(ui, plural(word_count(s.text), "word"), p.text_secondary);
        seg_right(
            ui,
            format!("Ln {}, Col {}", s.cursor.0, s.cursor.1),
            p.text_secondary,
        );
        let spell = format!("Spell {}", if s.spell_check { "On" } else { "Off" });
        let r = ui.painter().text(
            pos2(right, y),
            Align2::RIGHT_CENTER,
            spell,
            font.clone(),
            p.text_secondary,
        );
        ui.painter().circle_filled(
            pos2(r.left() - 8.0, y),
            4.0,
            if s.spell_check { GREEN } else { GREY },
        );
    }
    clicked
}

/// One line of the Log panel.
#[derive(Clone, Debug, PartialEq)]
pub struct LogLine {
    pub time: String,
    pub message: String,
}

/// What the user did in the Log panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogAction {
    Clear,
    Close,
}

/// `LogPanel.svelte`: "Sync Log", newest at the bottom, auto-scrolled.
pub fn log_panel(ui: &mut Ui, lines: &[LogLine]) -> Option<LogAction> {
    let p = theme::palette(ui.ctx());
    let mut out = None;
    let full = ui.max_rect();
    ui.painter().rect_filled(full, 0.0, p.bg_overlay);
    egui::Frame::NONE
        .fill(p.bg_base)
        .inner_margin(egui::Margin::symmetric(12, 6))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    fonts::weight("Sync Log", fonts::SEMIBOLD)
                        .size(12.0)
                        .color(p.text_secondary),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let small = |ui: &mut Ui, glyph: &str, label: &str| {
                        let b = ui
                            .add(egui::Button::new(icons::icon_sized(glyph, 11.0)).frame(false))
                            .on_hover_text(label);
                        b.widget_info(|| {
                            egui::WidgetInfo::labeled(egui::Role::Button, true, label)
                        });
                        b.clicked()
                    };
                    if small(ui, icons::XMARK, "Close pane") {
                        out = Some(LogAction::Close);
                    }
                    if small(ui, icons::TRASH_CAN, "Clear log") {
                        out = Some(LogAction::Clear);
                    }
                });
            });
        });
    egui::ScrollArea::vertical()
        .id_salt("noteliner-log")
        .auto_shrink(false)
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
                    if lines.is_empty() {
                        ui.label(
                            RichText::new("No sync activity yet.")
                                .size(12.0)
                                .color(p.text_muted),
                        );
                    }
                    for l in lines {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(
                                RichText::new(&l.time)
                                    .font(fonts::mono(11.5))
                                    .color(p.text_muted),
                            );
                            ui.label(
                                RichText::new(&l.message)
                                    .font(fonts::mono(11.5))
                                    .color(p.text_secondary),
                            );
                        });
                    }
                });
        });
    out
}

/// The overlay shown while files are dragged over the window.
pub fn drop_overlay(ctx: &egui::Context, message: &str) {
    let p = theme::palette(ctx);
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("noteliner-drop"),
    ));
    painter.rect_filled(screen, 0.0, p.modal_overlay);
    let target = screen.shrink(24.0);
    painter.rect_stroke(
        target,
        CornerRadius::same(12),
        Stroke::new(2.0, p.accent),
        egui::StrokeKind::Inside,
    );
    painter.text(
        target.center(),
        Align2::CENTER_CENTER,
        message,
        FontId::proportional(18.0),
        p.text_primary,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_labels() {
        assert_eq!(word_count("  one two\nthree "), 3);
        assert_eq!(sync_label(Some(&SyncStatus::Ahead { count: 2 })), "↑2");
        assert_eq!(
            sync_label(Some(&SyncStatus::Diverged {
                ahead: 1,
                behind: 3
            })),
            "↑1 ↓3"
        );
        assert_eq!(sync_label(None), "");
    }
}
