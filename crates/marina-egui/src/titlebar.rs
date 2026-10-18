//! The custom title bar: `TitleBar.svelte`, natively.
//!
//! An accent-coloured strip with, left to right: the toolbar toggle, the
//! app's own action buttons, the centred title, the UI-scale dropdown and the
//! window controls. Its empty space moves the window, and double-clicking it
//! maximizes ([`crate::window`]). The app handles what [`show`] returns.

use egui::{
    vec2, Align, Align2, Color32, Frame, Layout, Rect, Response, RichText, Sense, Stroke, Ui,
    UiBuilder,
};

use crate::theme::{self, ThemeState, SCALES, TITLEBAR_HEIGHT};
use crate::{icons, window};

/// A title bar button's width (`.titlebar-btn { width: 48px }`).
pub const BUTTON_WIDTH: f32 = 48.0;
/// `.titlebar-btn { font-size: 12px }`.
const ICON_SIZE: f32 = 12.0;
/// `.titlebar-title { font-size: 12px }`.
const TITLE_SIZE: f32 = 12.0;
/// `.titlebar-scale`: 22 px tall, 11 px text, 6 px margins.
const SCALE_HEIGHT: f32 = 22.0;
const SCALE_TEXT: f32 = 11.0;
const SCALE_MARGIN: f32 = 6.0;

/// What the user did in the title bar. Window controls are handled here.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TitleBarAction {
    /// The hamburger: show or hide the app's toolbar.
    ToggleToolbar,
    /// The UI scale was changed to this [`SCALES`] id. The theme state has
    /// already been updated and applied; persist it.
    ScaleChanged(String),
}

/// The title bar's inputs.
pub struct TitleBar<'a> {
    /// The centred title.
    pub app_name: &'a str,
    /// Whether the toolbar is showing (the toggle draws as active).
    pub toolbar_visible: bool,
}

/// Draws the title bar at the top of `ui`. `actions` adds the app's buttons
/// between the toolbar toggle and the title; use [`button`] for them.
pub fn show(
    ui: &mut Ui,
    bar: &TitleBar<'_>,
    theme: &mut ThemeState,
    actions: impl FnOnce(&mut Ui),
) -> Vec<TitleBarAction> {
    let p = theme::palette(ui.ctx());
    egui::Panel::top("marina-titlebar")
        .exact_size(TITLEBAR_HEIGHT)
        .resizable(false)
        .show_separator_line(false)
        .frame(Frame::NONE.fill(p.accent))
        .show(ui, |ui| contents(ui, bar, theme, actions))
        .inner
}

fn contents(
    ui: &mut Ui,
    bar: &TitleBar<'_>,
    theme: &mut ThemeState,
    actions: impl FnOnce(&mut Ui),
) -> Vec<TitleBarAction> {
    let mut out = Vec::new();
    let ink = theme::palette(ui.ctx()).accent_on;
    let full = ui.max_rect();
    window::title_bar_drag(ui);

    let left = ui.scope_builder(
        UiBuilder::new()
            .max_rect(full)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            ui.add_space(window::leading_inset());
            let toggle = button(ui, icons::BARS, "Toggle toolbar", bar.toolbar_visible)
                .on_hover_text("Toolbar (Ctrl+Shift+E)");
            if toggle.clicked() {
                out.push(TitleBarAction::ToggleToolbar);
            }
            actions(ui);
        },
    );

    let right = ui.scope_builder(
        UiBuilder::new()
            .max_rect(full)
            .layout(Layout::right_to_left(Align::Center)),
        |ui| {
            ui.spacing_mut().item_spacing.x = 0.0;
            if window::custom_caption_buttons() {
                window::caption_buttons(ui, TITLEBAR_HEIGHT, ink);
            }
            ui.add_space(SCALE_MARGIN);
            if let Some(id) = scale_dropdown(ui, theme, ink) {
                theme.set_scale(&id);
                theme.apply(ui.ctx());
                out.push(TitleBarAction::ScaleChanged(id));
            }
            ui.add_space(SCALE_MARGIN);
        },
    );

    // `.titlebar-title`: centred in the space between the two groups, elided.
    let gap = Rect::from_x_y_ranges(
        left.response.rect.right() + 8.0..=right.response.rect.left() - 8.0,
        full.y_range(),
    );
    if gap.width() > 0.0 {
        let galley = ui.painter().layout(
            bar.app_name.to_owned(),
            crate::fonts::sans(TITLE_SIZE),
            ink.gamma_multiply(0.9),
            f32::INFINITY,
        );
        let painter = ui.painter().with_clip_rect(gap);
        let pos = Align2::CENTER_CENTER
            .anchor_size(gap.center(), galley.size())
            .min;
        painter.galley(pos.max(gap.left_top()), galley, ink);
    }
    out
}

/// A title bar icon button (`.titlebar-btn`): 48 × 32, the accent-on ink at
/// 75%, darkened on hover; `active` keeps it darkened (the toolbar toggle).
pub fn button(ui: &mut Ui, glyph: &str, label: &str, active: bool) -> Response {
    let ink = theme::palette(ui.ctx()).accent_on;
    let (rect, resp) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, TITLEBAR_HEIGHT), Sense::click());
    let hovered = resp.hovered();
    let fill = if hovered {
        Color32::from_black_alpha(46)
    } else if active {
        Color32::from_black_alpha(31)
    } else {
        Color32::TRANSPARENT
    };
    let ink = if hovered || active {
        ink
    } else {
        ink.gamma_multiply(0.75)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, fill);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        glyph,
        icons::font(ICON_SIZE),
        ink,
    );
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, active, label));
    resp
}

/// `.titlebar-scale`: a compact dropdown of the [`SCALES`]. Returns the id
/// picked this frame.
fn scale_dropdown(ui: &mut Ui, theme: &ThemeState, ink: Color32) -> Option<String> {
    let current = SCALES
        .iter()
        .find(|s| s.0 == theme.scale)
        .map_or("100%", |s| s.1);
    let mut picked = None;
    ui.scope(|ui| {
        let v = ui.visuals_mut();
        // `background: rgba(0, 0, 0, 0.18)`, darker on hover; accent-on text.
        for (w, alpha) in [
            (&mut v.widgets.inactive, 46),
            (&mut v.widgets.hovered, 71),
            (&mut v.widgets.active, 71),
            (&mut v.widgets.open, 71),
        ] {
            w.weak_bg_fill = Color32::from_black_alpha(alpha);
            w.bg_fill = Color32::from_black_alpha(alpha);
            w.bg_stroke = Stroke::new(1.0, Color32::from_black_alpha(46));
            w.fg_stroke = Stroke::new(1.0, ink);
            w.expansion = 0.0;
        }
        ui.spacing_mut().interact_size.y = SCALE_HEIGHT;
        ui.spacing_mut().button_padding = vec2(6.0, 0.0);
        let resp = egui::ComboBox::from_id_salt("marina-titlebar-scale")
            .selected_text(RichText::new(current).size(SCALE_TEXT).color(ink))
            .width(58.0)
            .show_ui(ui, |ui| {
                // The options use the normal theme (`.titlebar-scale option`).
                ui.style_mut().visuals = ui.ctx().global_style().visuals.clone();
                for (id, label, _) in SCALES {
                    if ui.selectable_label(theme.scale == id, label).clicked() {
                        picked = Some(id.to_string());
                    }
                }
            });
        resp.response
            .widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, "UI scale"));
        resp.response.on_hover_text("UI scale");
    });
    picked.filter(|id| *id != theme.scale)
}

#[cfg(test)]
mod tests {
    use egui::accesskit::Role;
    use egui_kittest::kittest::{NodeT, Queryable};
    use egui_kittest::Harness;

    use super::*;

    struct State {
        theme: ThemeState,
        toolbar_visible: bool,
        actions: Vec<TitleBarAction>,
        app_clicks: usize,
    }

    fn harness() -> Harness<'static, State> {
        let state = State {
            theme: ThemeState::default(),
            toolbar_visible: true,
            actions: Vec::new(),
            app_clicks: 0,
        };
        Harness::builder()
            .with_size(vec2(900.0, 300.0))
            .with_context_setup(crate::fonts::install)
            .build_ui_state(
                |ui, s: &mut State| {
                    let bar = TitleBar {
                        app_name: "NoteLiner",
                        toolbar_visible: s.toolbar_visible,
                    };
                    let mut clicked = false;
                    let out = show(ui, &bar, &mut s.theme, |ui| {
                        clicked = button(ui, icons::PLUS, "New note", false).clicked();
                    });
                    if clicked {
                        s.app_clicks += 1;
                    }
                    for a in &out {
                        if *a == TitleBarAction::ToggleToolbar {
                            s.toolbar_visible = !s.toolbar_visible;
                        }
                    }
                    s.actions.extend(out);
                },
                state,
            )
    }

    #[test]
    fn toolbar_toggle_and_app_actions() {
        let mut h = harness();
        h.run();
        h.get_by_label("Toggle toolbar").click();
        h.run();
        assert_eq!(h.state().actions, [TitleBarAction::ToggleToolbar]);
        assert!(!h.state().toolbar_visible);
        h.get_by_label("New note").click();
        h.run();
        assert_eq!(h.state().app_clicks, 1);
    }

    #[test]
    fn window_controls_are_labelled() {
        let mut h = harness();
        h.run();
        if window::custom_caption_buttons() {
            for label in ["Minimize", "Maximize", "Close"] {
                assert_eq!(h.get_by_label(label).accesskit_node().role(), Role::Button);
            }
        }
    }

    #[test]
    fn scale_dropdown_changes_the_scale() {
        let mut h = harness();
        h.run();
        h.get_by_label("UI scale").click();
        h.run();
        h.get_by_label("150%").click();
        h.run();
        assert_eq!(
            h.state().actions,
            [TitleBarAction::ScaleChanged("150".into())]
        );
        assert_eq!(h.state().theme.scale, "150");
        assert_eq!(h.ctx.zoom_factor(), 1.5);
    }
}
