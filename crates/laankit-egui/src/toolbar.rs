//! The vertical icon toolbar: `Toolbar`, `ToolbarButton`, `ToolbarDivider`
//! and `ToolbarSpacer` from desktop-ui, natively.
//!
//! [`show`] draws the 48 px column at the left of `ui`. Its two closures fill
//! the top group and the bottom group; the space between them is the
//! `ToolbarSpacer`. Add [`button`]s and [`divider`]s to either.

use egui::{
    vec2, Align, Align2, Frame, Id, Layout, Margin, Rect, Response, Sense, Stroke, Ui, UiBuilder,
};

use crate::{icons, theme};

/// `.toolbar { width: 48px }`.
pub const WIDTH: f32 = 48.0;
/// `.toolbar-btn`: 36 × 36, 6 px corners, 16 px icons.
pub const BUTTON_SIZE: f32 = 36.0;
const ICON_SIZE: f32 = 16.0;
/// `gap: 4px`.
const GAP: f32 = 4.0;
const DIVIDER_HEIGHT: f32 = 5.0;
/// `padding: 8px 0`.
const PADDING: i8 = 8;

/// One toolbar button (`ToolbarButton.svelte`).
#[derive(Clone, Debug)]
pub struct ToolbarButton<'a> {
    /// A [`crate::icons`] glyph.
    pub icon: &'a str,
    /// The tooltip.
    pub title: &'a str,
    /// The accessible label; falls back to `title`.
    pub label: Option<&'a str>,
    pub active: bool,
    pub disabled: bool,
    /// Tints the icon with the accent while inactive (`extraClass`, e.g.
    /// ThreadLiner's `sync-pending`).
    pub attention: bool,
}

impl<'a> ToolbarButton<'a> {
    pub fn new(icon: &'a str, title: &'a str) -> Self {
        Self {
            icon,
            title,
            label: None,
            active: false,
            disabled: false,
            attention: false,
        }
    }

    pub fn attention(mut self, attention: bool) -> Self {
        self.attention = attention;
        self
    }

    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }
}

/// The toolbar's width, its buttons' size and their icons' size, which are
/// [`WIDTH`], [`BUTTON_SIZE`] and 16 px in Classic and larger in Modern.
fn sizes(ctx: &egui::Context) -> (f32, f32, f32) {
    let k = theme::shape(ctx).control_scale;
    (
        (WIDTH * k).round(),
        (BUTTON_SIZE * k).round(),
        ICON_SIZE * k,
    )
}

fn limit_id() -> Id {
    Id::unique("laankit-toolbar-top-limit")
}

/// Whether the top group has room for `buttons` more buttons (and a divider)
/// above the bottom group. Call it inside [`show`]'s `top`: the toolbar
/// doesn't scroll, so an app leaves out buttons that are only offered for
/// some documents when the window is too short for them.
pub fn fits(ui: &Ui, buttons: usize) -> bool {
    let limit: f32 = ui.data(|d| d.get_temp(limit_id())).unwrap_or(f32::INFINITY);
    let (_, size, _) = sizes(ui.ctx());
    let needed = DIVIDER_HEIGHT + GAP + buttons as f32 * (size + GAP);
    ui.cursor().top() + needed <= limit
}

/// Draws the toolbar at the left of `ui`: `top` from the top down, then the
/// spacer, then `bottom` against the bottom edge.
pub fn show(ui: &mut Ui, top: impl FnOnce(&mut Ui), bottom: impl FnOnce(&mut Ui)) {
    let p = theme::palette(ui.ctx());
    let modern = theme::shape(ui.ctx()).modern;
    // Modern: the toolbar and the sidebar share one colour, with no line between.
    let fill = if modern { p.bg_base } else { p.bg_overlay };
    egui::Panel::left("laankit-toolbar")
        .exact_size(sizes(ui.ctx()).0)
        .resizable(false)
        .show_separator_line(!modern)
        .frame(
            Frame::NONE
                .fill(fill)
                // The toolbar reaches the window's bottom-left corner.
                .corner_radius(egui::CornerRadius {
                    sw: crate::window::corner_radius(ui.ctx()),
                    ..Default::default()
                })
                .inner_margin(Margin::symmetric(0, PADDING)),
        )
        .show(ui, |ui| {
            let full = ui.max_rect();
            // The bottom group keeps its natural order: it is placed by last frame's height.
            let height_id = Id::unique("laankit-toolbar-bottom-height");
            let last: f32 = ui.data(|d| d.get_temp(height_id)).unwrap_or(0.0);
            ui.data_mut(|d| d.insert_temp(limit_id(), full.bottom() - last));
            let column = |ui: &mut Ui| {
                ui.spacing_mut().item_spacing.y = GAP;
            };
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(full)
                    .layout(Layout::top_down(Align::Center)),
                |ui| {
                    column(ui);
                    top(ui);
                },
            );
            let rect = Rect::from_min_max(
                egui::pos2(full.left(), (full.bottom() - last).max(full.top())),
                full.max,
            );
            let drawn = ui.scope_builder(
                UiBuilder::new()
                    .max_rect(rect)
                    .layout(Layout::top_down(Align::Center)),
                |ui| {
                    column(ui);
                    bottom(ui);
                },
            );
            let height = drawn.response.rect.height();
            if (height - last).abs() > 0.5 {
                ui.data_mut(|d| d.insert_temp(height_id, height));
                ui.ctx().request_discard("toolbar bottom group resized");
            }
        });
}

/// A toolbar button. A disabled button never reports clicks.
pub fn button(ui: &mut Ui, b: ToolbarButton<'_>) -> Response {
    let p = theme::palette(ui.ctx());
    let sense = if b.disabled {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (_, size, icon_size) = sizes(ui.ctx());
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), sense);
    let hovered = resp.hovered() && !b.disabled;
    let (fill, ink, outline) = if b.active && theme::shape(ui.ctx()).modern {
        // A tinted fill alone marks the selected button.
        (p.bg_selected, p.accent, Stroke::NONE)
    } else if b.active {
        (p.bg_selected, p.accent, Stroke::new(1.0, p.accent))
    } else if hovered {
        (p.bg_button, p.text_primary, Stroke::NONE)
    } else if b.attention {
        (egui::Color32::TRANSPARENT, p.accent, Stroke::NONE)
    } else {
        (egui::Color32::TRANSPARENT, p.text_secondary, Stroke::NONE)
    };
    let ink = if b.disabled {
        ink.gamma_multiply(0.3)
    } else {
        ink
    };
    let painter = ui.painter();
    let radius = theme::shape(ui.ctx()).round(6);
    painter.rect(rect, radius, fill, outline, egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        b.icon,
        icons::font(icon_size),
        ink,
    );
    let label = b.label.unwrap_or(b.title).to_owned();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::Button, !b.disabled, b.active, &label)
    });
    if b.title.is_empty() {
        resp
    } else {
        resp.on_hover_text(b.title)
    }
}

/// `ToolbarDivider`: a 24 px hairline.
pub fn divider(ui: &mut Ui) {
    let p = theme::palette(ui.ctx());
    let (rect, _) = ui.allocate_exact_size(vec2(24.0, DIVIDER_HEIGHT), Sense::hover());
    ui.painter()
        .hline(rect.x_range(), rect.center().y, Stroke::new(1.0, p.border));
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    use super::*;

    #[derive(Default)]
    struct State {
        clicks: Vec<&'static str>,
        sidebar: bool,
    }

    fn harness() -> Harness<'static, State> {
        Harness::builder()
            .with_size(vec2(400.0, 500.0))
            .with_context_setup(crate::fonts::install)
            .build_ui_state(
                |ui, s: &mut State| {
                    let (mut top_clicks, mut bottom_clicks) = (Vec::new(), Vec::new());
                    let sidebar = &mut s.sidebar;
                    show(
                        ui,
                        |ui| {
                            let new = ToolbarButton::new(icons::PLUS, "New file (Ctrl+N)")
                                .label("New file");
                            if button(ui, new).clicked() {
                                top_clicks.push("new");
                            }
                            if button(
                                ui,
                                ToolbarButton::new(icons::FOLDER, "Sidebar").active(*sidebar),
                            )
                            .clicked()
                            {
                                *sidebar = !*sidebar;
                            }
                            divider(ui);
                            if button(
                                ui,
                                ToolbarButton::new(icons::TRASH, "Delete").disabled(true),
                            )
                            .clicked()
                            {
                                top_clicks.push("delete");
                            }
                        },
                        |ui| {
                            if button(ui, ToolbarButton::new(icons::GEAR, "Settings")).clicked() {
                                bottom_clicks.push("settings");
                            }
                        },
                    );
                    s.clicks.extend(top_clicks);
                    s.clicks.extend(bottom_clicks);
                },
                State::default(),
            )
    }

    #[test]
    fn buttons_click_by_label() {
        let mut h = harness();
        h.run();
        h.get_by_label("New file").click();
        h.get_by_label("Settings").click();
        h.run();
        assert_eq!(h.state().clicks, ["new", "settings"]);
    }

    #[test]
    fn disabled_buttons_ignore_clicks() {
        let mut h = harness();
        h.run();
        let delete = h.get_by_label("Delete");
        assert!(delete.accesskit_node().is_disabled());
        delete.click();
        h.run();
        assert!(h.state().clicks.is_empty());
    }

    #[test]
    fn active_toggles_and_bottom_group_sits_at_the_bottom() {
        let mut h = harness();
        h.run();
        h.get_by_label("Sidebar").click();
        h.run();
        assert!(h.state().sidebar);
        let toggled = h.get_by_label("Sidebar").accesskit_node().toggled();
        assert_eq!(toggled, Some(egui::accesskit::Toggled::True));
        let settings = h.get_by_label("Settings").rect();
        assert!(settings.max.y > 450.0, "settings at {settings:?}");
    }
}
