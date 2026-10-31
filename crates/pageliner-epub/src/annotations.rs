//! The Annotations pane (`AnnotationsPane.svelte`): bookmarks and highlights, each with jump and
//! delete. Purely presentational: the reader owns the data and acts on the returned action.

use egui::{vec2, Align2, Color32, CursorIcon, FontId, Rect, RichText, Sense, Stroke, Ui};
use laankit_egui::{fonts, icons, theme};

use crate::state::{Bookmark, Highlight};

/// `.swatch`: the highlight colour, a little stronger than on the page.
const SWATCH_FILL: Color32 = Color32::from_rgba_unmultiplied_const(240, 200, 0, 140);
const SWATCH_BORDER: Color32 = Color32::from_rgba_unmultiplied_const(180, 150, 0, 153);
/// `.del:hover`.
const DELETE_RED: Color32 = Color32::from_rgb(0xe0, 0x48, 0x4d);
const ROW_HEIGHT: f32 = 26.0;

/// What the user asked for, by annotation id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AnnotationAction {
    Jump(String),
    Delete(String),
}

/// Draws both sections. `active` marks a highlight (e.g. the one just clicked on the page).
pub fn show(
    ui: &mut Ui,
    bookmarks: &[Bookmark],
    highlights: &[Highlight],
    active: Option<&str>,
) -> Option<AnnotationAction> {
    let mut action = None;
    egui::ScrollArea::vertical()
        .id_salt("pageliner-annotations")
        .auto_shrink(false)
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            section_head(ui, "Bookmarks");
            if bookmarks.is_empty() {
                empty(ui, "No bookmarks yet.");
            }
            for b in bookmarks {
                if let Some(a) = row(ui, &b.id, Marker::Bookmark, &b.label, false) {
                    action = Some(a);
                }
            }
            ui.add_space(8.0);
            section_head(ui, "Highlights");
            if highlights.is_empty() {
                empty(ui, "Select text while reading to highlight it.");
            }
            for h in highlights {
                if let Some(a) = row(ui, &h.id, Marker::Swatch, &h.text, active == Some(&h.id)) {
                    action = Some(a);
                }
            }
        });
    action
}

fn section_head(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(
            RichText::new(text.to_uppercase())
                .size(10.0)
                .color(p.text_faint)
                .extra_letter_spacing(0.5),
        );
    });
    ui.add_space(4.0);
}

fn empty(ui: &mut Ui, text: &str) {
    let p = theme::palette(ui.ctx());
    ui.horizontal(|ui| {
        ui.add_space(10.0);
        ui.label(RichText::new(text).size(12.0).color(p.text_muted));
    });
    ui.add_space(4.0);
}

enum Marker {
    Bookmark,
    Swatch,
}

/// `li`: a jump button (marker + label) and a delete button.
fn row(
    ui: &mut Ui,
    id: &str,
    marker: Marker,
    label: &str,
    active: bool,
) -> Option<AnnotationAction> {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let del_rect = Rect::from_min_size(
        rect.right_top() + vec2(-ROW_HEIGHT, 0.0),
        vec2(ROW_HEIGHT, ROW_HEIGHT),
    )
    .shrink(1.0);
    let jump_rect = Rect::from_min_max(rect.min, egui::pos2(del_rect.left(), rect.bottom()));
    let jump = ui
        .interact(jump_rect, ui.scope_id().with(("jump", id)), Sense::click())
        .on_hover_cursor(CursorIcon::PointingHand)
        .on_hover_text(label);
    jump.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let del = ui
        .interact(del_rect, ui.scope_id().with(("delete", id)), Sense::click())
        .on_hover_text(match marker {
            Marker::Bookmark => "Remove bookmark",
            Marker::Swatch => "Remove highlight",
        });
    let del_label = match marker {
        Marker::Bookmark => format!("Remove bookmark {label}"),
        Marker::Swatch => format!("Remove highlight {label}"),
    };
    del.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &del_label));

    let painter = ui.painter_at(rect);
    if active {
        painter.rect_filled(rect, 0.0, p.bg_selected);
    }
    let row_hovered = ui.rect_contains_pointer(rect);
    let ink = if jump.hovered() || active {
        p.text_primary
    } else {
        p.text_secondary
    };
    let mx = rect.left() + 10.0;
    let cy = rect.center().y;
    match marker {
        Marker::Bookmark => {
            painter.text(
                egui::pos2(mx, cy),
                Align2::LEFT_CENTER,
                icons::BOOKMARK,
                icons::font(11.0),
                p.accent,
            );
        }
        Marker::Swatch => {
            let sw = Rect::from_min_size(egui::pos2(mx, cy - 5.5), vec2(11.0, 11.0));
            painter.rect(
                sw,
                2.0,
                SWATCH_FILL,
                Stroke::new(1.0, SWATCH_BORDER),
                egui::StrokeKind::Inside,
            );
        }
    }
    let text_left = mx + 19.0;
    let max_w = (del_rect.left() - text_left - 4.0).max(10.0);
    let galley = ui.painter().layout(
        label.to_string(),
        FontId::new(12.0, fonts::Style::default().family()),
        ink,
        f32::INFINITY,
    );
    let galley = if galley.size().x > max_w {
        let mut job = egui::text::LayoutJob::simple_singleline(
            label.to_string(),
            FontId::new(12.0, fonts::Style::default().family()),
            ink,
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        ui.fonts_mut(|f| f.layout_job(job))
    } else {
        galley
    };
    painter.galley(
        egui::pos2(text_left, cy - galley.size().y / 2.0),
        galley,
        ink,
    );
    if del.hovered() {
        painter.rect_filled(del_rect, theme::RADIUS, p.bg_item_hover);
    }
    painter.text(
        del_rect.center(),
        Align2::CENTER_CENTER,
        icons::XMARK,
        icons::font(11.0),
        if del.hovered() {
            DELETE_RED
        } else if row_hovered {
            p.text_secondary
        } else {
            p.text_faint
        },
    );
    if del.clicked() {
        Some(AnnotationAction::Delete(id.to_string()))
    } else if jump.clicked() {
        Some(AnnotationAction::Jump(id.to_string()))
    } else {
        None
    }
}
