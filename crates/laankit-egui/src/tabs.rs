//! A strip of document tabs, above an editor or a canvas: BlockLiner's
//! diagrams, NoteLiner's notes and diagrams.
//!
//! The app owns which tabs are open and which is active; [`strip`] draws
//! them and returns what the user asked for.

use egui::text::{LayoutJob, TextWrapping};
use egui::{pos2, vec2, Align2, Color32, FontId, Rect, ScrollArea, Sense, Stroke, Ui};

use crate::icons;
use crate::theme::{self, Palette};

/// The room a tab's icon takes, with the gap after it.
const ICON_WIDTH: f32 = 20.0;

/// The tab strip's height.
pub const HEIGHT: f32 = 32.0;

/// A tab.
#[derive(Clone, Debug, Default)]
pub struct TabItem {
    pub name: String,
    /// The tooltip, e.g. the file's full path. Empty for none.
    pub path: String,
    pub active: bool,
    /// A Font Awesome glyph before the name, e.g. the document's type.
    pub icon: Option<&'static str>,
}

/// What the tab strip asks for, by tab index.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabAction {
    Activate(usize),
    Close(usize),
}

/// The open tabs, scrolling sideways when they don't fit. A tab's close
/// button shows on the active tab and under the pointer; a middle click
/// closes it too. `reveal` scrolls the active tab into view. `id` keeps the
/// strip's scroll position.
pub fn strip(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    tabs: &[TabItem],
    reveal: bool,
) -> Option<TabAction> {
    let p = theme::palette(ui.ctx());
    let strip = ui.max_rect();
    ui.painter().rect_filled(strip, 0.0, p.bg_surface);
    ui.painter().hline(
        strip.x_range(),
        strip.bottom() - 0.5,
        Stroke::new(1.0, p.border),
    );
    let mut out = None;
    ScrollArea::horizontal()
        .id_salt(id)
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
            // A vertical wheel scrolls the strip sideways.
            if ui.rect_contains_pointer(strip) {
                let wheel = ui.input(|i| i.smooth_scroll_delta);
                if wheel.y != 0.0 && wheel.x == 0.0 {
                    ui.scroll_with_delta(vec2(wheel.y, 0.0));
                }
            }
            ui.horizontal(|ui| {
                for (i, tab) in tabs.iter().enumerate() {
                    if let Some(a) = tab_widget(ui, &p, i, tab, reveal) {
                        out = Some(a);
                    }
                }
            });
        });
    out
}

fn tab_widget(
    ui: &mut Ui,
    p: &Palette,
    i: usize,
    tab: &TabItem,
    reveal: bool,
) -> Option<TabAction> {
    let font = FontId::proportional(13.0);
    let ink = if tab.active {
        p.text_primary
    } else {
        p.text_secondary
    };
    let text = one_line(ui, &tab.name, font, ink, 220.0);
    let icon = if tab.icon.is_some() { ICON_WIDTH } else { 0.0 };
    let width = 14.0 + icon + text.size().x + 8.0 + 18.0 + 8.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Tab, true, tab.active, &tab.name));
    if tab.active && reveal {
        resp.scroll_to_me(None);
    }
    let painter = ui.painter();
    if tab.active {
        painter.rect_filled(rect, 0.0, p.bg_base);
        painter.hline(rect.x_range(), rect.top() + 1.0, Stroke::new(2.0, p.accent));
    } else if resp.hovered() {
        painter.rect_filled(rect.shrink2(vec2(0.0, 0.5)), 0.0, p.bg_item_hover);
    }
    painter.vline(
        rect.right() - 0.5,
        rect.y_range(),
        Stroke::new(1.0, p.border),
    );
    if let Some(glyph) = tab.icon {
        painter.text(
            pos2(rect.left() + 14.0, rect.center().y),
            Align2::LEFT_CENTER,
            glyph,
            icons::font(12.0),
            if tab.active { p.accent } else { p.text_muted },
        );
    }
    painter.galley(
        pos2(
            rect.left() + 14.0 + icon,
            rect.center().y - text.size().y / 2.0,
        ),
        text,
        ink,
    );
    let close =
        Rect::from_center_size(pos2(rect.right() - 17.0, rect.center().y), vec2(18.0, 18.0));
    let label = format!("Close {}", tab.name);
    let x = ui.interact(close, ui.scope_id().with(("tab-close", i)), Sense::click());
    x.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &label));
    let show_x = tab.active || resp.hovered() || x.hovered();
    if show_x {
        if x.hovered() {
            ui.painter().rect_filled(close, 4.0, p.bg_button_hover);
        }
        ui.painter().text(
            close.center(),
            Align2::CENTER_CENTER,
            icons::XMARK,
            icons::font(10.0),
            if x.hovered() {
                p.text_primary
            } else {
                p.text_muted
            },
        );
    }
    let resp = if tab.path.is_empty() {
        resp
    } else {
        resp.on_hover_text(&tab.path)
    };
    if x.clicked() || resp.middle_clicked() {
        Some(TabAction::Close(i))
    } else if resp.clicked() && !tab.active {
        Some(TabAction::Activate(i))
    } else {
        None
    }
}

/// One line of text, cut with an ellipsis at `width`.
fn one_line(
    ui: &Ui,
    text: &str,
    font: FontId,
    color: Color32,
    width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
    job.wrap = TextWrapping::truncate_at_width(width.max(1.0));
    ui.fonts_mut(|f| f.layout_job(job))
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    fn harness() -> Harness<'static, Vec<TabAction>> {
        Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(600.0, 200.0))
            .build_ui_state(
                |ui, actions: &mut Vec<TabAction>| {
                    let tabs = [
                        TabItem {
                            name: "Roadmap".into(),
                            active: true,
                            icon: Some(icons::FILE_LINES),
                            ..Default::default()
                        },
                        TabItem {
                            name: "Architecture".into(),
                            path: "/notes/architecture.mmd".into(),
                            ..Default::default()
                        },
                    ];
                    egui::Panel::top("tabs").exact_size(HEIGHT).show(ui, |ui| {
                        if let Some(a) = strip(ui, "test-tabs", &tabs, false) {
                            actions.push(a);
                        }
                    });
                },
                Vec::new(),
            )
    }

    #[test]
    fn activates_and_closes() {
        let mut h = harness();
        h.run_steps(2);
        h.get_by_label("Architecture").click();
        h.run_steps(2);
        h.get_by_label("Close Roadmap").click();
        h.run_steps(2);
        // The active tab's own click does nothing.
        h.get_by_label("Roadmap").click();
        h.run_steps(2);
        assert_eq!(
            h.state().as_slice(),
            [TabAction::Activate(1), TabAction::Close(0)]
        );
    }
}
