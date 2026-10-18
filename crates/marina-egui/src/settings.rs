//! The settings building blocks, natively: `settings/*.svelte`.
//!
//! [`SettingsShell`] is the modal chrome (accent header, tab strip,
//! scrolling body, OK footer); the app draws each tab's body with the
//! atoms here: [`setting_group`], [`theme_list`], [`scale_list`],
//! [`toggle_option`], [`restart_banner`] and [`shortcuts_list`].
//!
//! The modal header and the OK button are shared with [`crate::about`].

use egui::text::LayoutJob;
use egui::{
    pos2, vec2, Align, Align2, Area, Color32, CornerRadius, FontId, Frame, Id, Key, Layout, Margin,
    Modal, Order, Rect, Response, RichText, ScrollArea, Sense, Stroke, StrokeKind, Ui, Vec2,
};

use crate::commands::ShortcutEntry;
use crate::fonts;
use crate::icons;
use crate::theme::{self, Palette, ThemeId, ThemeState, SCALES};

/// `.modal-header { height: 32px; padding: 0 24px }`.
const HEADER_HEIGHT: f32 = 32.0;
/// `border-radius: 6px` on the settings controls.
const CONTROL_RADIUS: CornerRadius = CornerRadius::same(6);
/// `padding: 10px 14px` around 14 px text.
const CONTROL_HEIGHT: f32 = 38.0;
/// `.modal-overlay { padding: 10px 10px 10px 58px }`: the left margin
/// leaves the apps' activity bar visible.
const OVERLAY_PAD: Margin = Margin {
    left: 58,
    right: 10,
    top: 10,
    bottom: 10,
};

/// A tab of the settings modal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tab<'a> {
    pub id: &'a str,
    pub label: &'a str,
}

/// The settings modal. Draw it every frame while open; [`SettingsShell::show`]
/// returns `true` once it should close (OK, the X, Escape, Enter outside a
/// text field, or a click on the backdrop).
pub struct SettingsShell<'a> {
    pub title: &'a str,
    pub tabs: &'a [Tab<'a>],
    /// Space above the overlay, e.g. the app's title bar
    /// ([`theme::TITLEBAR_HEIGHT`]), which stays visible as in the Svelte
    /// `top: var(--titlebar-height)`.
    pub top_inset: f32,
}

impl<'a> SettingsShell<'a> {
    pub fn new(tabs: &'a [Tab<'a>]) -> Self {
        Self {
            title: "Settings",
            tabs,
            top_inset: 0.0,
        }
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    pub fn top_inset(mut self, inset: f32) -> Self {
        self.top_inset = inset;
        self
    }

    /// Draws the modal; `body(ui, tab_id)` fills the active tab. `active`
    /// is the active tab id, kept by the app (`bind:activeTab`).
    pub fn show(
        &self,
        ctx: &egui::Context,
        active: &mut String,
        body: impl FnOnce(&mut Ui, &str),
    ) -> bool {
        let p = theme::palette(ctx);
        if !self.tabs.iter().any(|t| t.id == active.as_str()) {
            if let Some(first) = self.tabs.first() {
                *active = first.id.to_string();
            }
        }
        let screen = ctx.content_rect();
        let rect = Rect::from_min_max(
            pos2(
                screen.left() + OVERLAY_PAD.left as f32,
                screen.top() + self.top_inset + OVERLAY_PAD.top as f32,
            ),
            pos2(
                screen.right() - OVERLAY_PAD.right as f32,
                screen.bottom() - OVERLAY_PAD.bottom as f32,
            ),
        );
        let mut close = false;
        let modal = Modal::new(Id::unique("marina-settings"))
            .area(modal_area(Id::unique("marina-settings"), rect.min))
            .backdrop_color(p.modal_overlay)
            .frame(modal_frame(&p))
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                close |= modal_header(ui, &p, self.title);
                self.tab_bar(ui, &p, active);
                let footer_h = 57.0;
                let body_h = (ui.available_height() - footer_h).max(0.0);
                ui.allocate_ui(vec2(ui.available_width(), body_h), |ui| {
                    ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            Frame::new()
                                .inner_margin(Margin {
                                    left: 24,
                                    right: 24,
                                    top: 24,
                                    bottom: 20,
                                })
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing = vec2(8.0, 6.0);
                                    ui.set_width(ui.available_width());
                                    body(ui, active.as_str());
                                });
                        });
                });
                close |= modal_footer(ui, &p);
            });
        let enter =
            ctx.memory(|m| m.focused().is_none()) && ctx.input(|i| i.key_pressed(Key::Enter));
        close || enter || modal.should_close()
    }

    /// `.tab-bar`: text tabs with an accent underline on the active one.
    fn tab_bar(&self, ui: &mut Ui, p: &Palette, active: &mut String) {
        let (bar, _) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
        ui.painter().hline(
            bar.x_range(),
            bar.bottom() - 0.5,
            Stroke::new(1.0, p.border),
        );
        let mut x = bar.left() + 24.0;
        for tab in self.tabs {
            let galley = ui.painter().layout_no_wrap(
                tab.label.to_string(),
                FontId::proportional(13.0),
                Color32::PLACEHOLDER,
            );
            let rect = Rect::from_min_size(
                pos2(x, bar.top()),
                vec2(galley.size().x + 32.0, bar.height()),
            );
            let resp = ui.interact(rect, ui.scope_id().with(("tab", tab.id)), Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::Role::Button, true, *active == tab.id, tab.label)
            });
            if resp.clicked() {
                *active = tab.id.to_string();
            }
            let selected = *active == tab.id;
            let color = if selected {
                p.accent
            } else if resp.hovered() {
                p.text_primary
            } else {
                p.text_muted
            };
            ui.painter().galley(
                pos2(rect.left() + 16.0, rect.center().y - galley.size().y / 2.0),
                galley,
                color,
            );
            if selected {
                ui.painter().rect_filled(
                    Rect::from_min_max(pos2(rect.left(), rect.bottom() - 2.0), rect.max),
                    0.0,
                    p.accent,
                );
            }
            x = rect.right();
        }
    }
}

/// The area every Marina modal lives in.
pub(crate) fn modal_area(id: Id, pos: egui::Pos2) -> Area {
    Area::new(id)
        .kind(egui::UiKind::Modal)
        .order(Order::Foreground)
        .fixed_pos(pos)
}

/// `.modal`: the surface with a hairline and 12 px corners.
pub(crate) fn modal_frame(p: &Palette) -> Frame {
    Frame::new()
        .fill(p.bg_surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(theme::RADIUS_LARGE)
}

/// `.modal-header`: the accent bar with the upper-case title and the close
/// button. Returns `true` when the close button is clicked.
pub(crate) fn modal_header(ui: &mut Ui, p: &Palette, title: &str) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let r = theme::RADIUS_LARGE.nw;
    ui.painter().rect_filled(
        rect,
        CornerRadius {
            nw: r,
            ne: r,
            sw: 0,
            se: 0,
        },
        p.accent,
    );
    let title = semibold_galley(ui, &title.to_uppercase(), 14.0, p.accent_on);
    ui.painter().galley(
        pos2(rect.left() + 24.0, rect.center().y - title.size().y / 2.0),
        title,
        p.accent_on,
    );
    let button = Rect::from_center_size(
        pos2(rect.right() - 24.0 - 12.0, rect.center().y),
        vec2(24.0, 24.0),
    );
    let resp = ui
        .interact(button, ui.scope_id().with("modal-close"), Sense::click())
        .on_hover_text("Close (Esc)");
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, "Close"));
    if resp.hovered() {
        ui.painter()
            .rect_filled(button, theme::RADIUS, Color32::from_black_alpha(46));
    }
    ui.painter().text(
        button.center(),
        Align2::CENTER_CENTER,
        icons::XMARK,
        icons::font(14.0),
        p.accent_on
            .gamma_multiply(if resp.hovered() { 1.0 } else { 0.8 }),
    );
    resp.clicked()
}

/// `text` laid out on one line at [`fonts::SEMIBOLD`], for painting.
pub(crate) fn semibold_galley(
    ui: &Ui,
    text: &str,
    size: f32,
    color: Color32,
) -> std::sync::Arc<egui::Galley> {
    let style = fonts::Style {
        weight: Some(fonts::SEMIBOLD),
        ..Default::default()
    };
    let job = LayoutJob::single_section(text.to_string(), fonts::format(style, size, color));
    ui.fonts_mut(|f| f.layout_job(job))
}

/// `.modal-footer` with the OK button, right-aligned under a hairline.
/// Returns `true` when OK is clicked.
pub(crate) fn modal_footer(ui: &mut Ui, p: &Palette) -> bool {
    let top = ui.cursor().top();
    ui.painter()
        .hline(ui.max_rect().x_range(), top, Stroke::new(1.0, p.border));
    let mut ok = false;
    Frame::new()
        .inner_margin(Margin::symmetric(24, 12))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ok = outline_button(ui, p, "OK", vec2(24.0, 8.0)).clicked();
            });
        });
    ok
}

/// `.close-btn` / `.primary-btn`: accent outline and text on the selected
/// wash, filling with the accent on hover.
pub(crate) fn outline_button(ui: &mut Ui, p: &Palette, label: &str, pad: Vec2) -> Response {
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(theme::BODY_SIZE),
        Color32::PLACEHOLDER,
    );
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    let (fill, ink) = if resp.hovered() {
        (p.accent, p.accent_on)
    } else {
        (p.bg_selected, p.accent)
    };
    ui.painter().rect(
        rect,
        CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, p.accent),
        StrokeKind::Outside,
    );
    ui.painter().galley(rect.min + pad, galley, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// `SettingGroup`: an upper-case caption, optional help text, then the
/// contents, with 24 px below.
pub fn setting_group<R>(
    ui: &mut Ui,
    label: &str,
    help: Option<&str>,
    add_contents: impl FnOnce(&mut Ui) -> R,
) -> R {
    let p = theme::palette(ui.ctx());
    if !label.is_empty() {
        ui.label(
            fonts::weight(label.to_uppercase(), fonts::SEMIBOLD)
                .size(11.0)
                .color(p.text_muted)
                .extra_letter_spacing(0.5),
        );
        ui.add_space(4.0);
    }
    if let Some(help) = help {
        ui.label(RichText::new(help).size(12.0).color(p.text_muted));
        ui.add_space(4.0);
    }
    let inner = add_contents(ui);
    ui.add_space(24.0);
    inner
}

/// `ThemeList`: a full-width dropdown of the themes, each option painted in
/// its own theme's accent colours. Applies the choice to the context and
/// returns `true` when it changed, so the app can persist it.
pub fn theme_list(ui: &mut Ui, state: &mut ThemeState) -> bool {
    let p = theme::palette(ui.ctx());
    let mut picked = None;
    dropdown(ui, &p, "Theme", state.theme.name(), |ui| {
        for t in ThemeId::ALL {
            let tp = t.palette();
            if swatch_row(ui, t.name(), tp.accent, tp.accent_on, t == state.theme).clicked() {
                picked = Some(t);
            }
        }
    });
    match picked {
        Some(t) if t != state.theme => {
            state.theme = t;
            state.apply(ui.ctx());
            true
        }
        _ => false,
    }
}

/// `ScaleList`: a full-width dropdown of the UI scale steps. Applies the
/// choice and returns `true` when it changed.
pub fn scale_list(ui: &mut Ui, state: &mut ThemeState) -> bool {
    let p = theme::palette(ui.ctx());
    let current = SCALES
        .iter()
        .find(|s| s.0 == state.scale)
        .map_or("100%", |s| s.1);
    let mut picked = None;
    dropdown(ui, &p, "UI scale", current, |ui| {
        for (id, label, _) in SCALES {
            if swatch_row(ui, label, p.bg_surface, p.text_primary, id == state.scale).clicked() {
                picked = Some(id);
            }
        }
    });
    match picked {
        Some(id) if id != state.scale => {
            state.set_scale(id);
            state.apply(ui.ctx());
            true
        }
        _ => false,
    }
}

/// `.theme-select` / `.scale-select`: the closed select, opening a popup
/// list below it.
fn dropdown(ui: &mut Ui, p: &Palette, label: &str, current: &str, options: impl FnOnce(&mut Ui)) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::ComboBox, true, label));
    let open = egui::Popup::is_id_open(ui.ctx(), egui::Popup::default_response_id(&resp));
    let fill = if resp.hovered() {
        p.bg_button_hover
    } else {
        p.bg_button
    };
    let stroke = if open || resp.has_focus() {
        p.accent
    } else {
        p.border
    };
    ui.painter().rect(
        rect,
        CONTROL_RADIUS,
        fill,
        Stroke::new(1.0, stroke),
        StrokeKind::Inside,
    );
    ui.painter().text(
        pos2(rect.left() + 14.0, rect.center().y),
        Align2::LEFT_CENTER,
        current,
        FontId::proportional(theme::BODY_SIZE),
        p.text_primary,
    );
    ui.painter().text(
        pos2(rect.right() - 14.0, rect.center().y),
        Align2::RIGHT_CENTER,
        icons::CHEVRON_DOWN,
        icons::font(11.0),
        p.text_muted,
    );
    egui::Popup::menu(&resp)
        .width(rect.width())
        .close_behavior(egui::PopupCloseBehavior::CloseOnClick)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            options(ui);
        });
}

/// An option row filled with `fill` and labelled in `ink`.
fn swatch_row(ui: &mut Ui, label: &str, fill: Color32, ink: Color32, selected: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::Role::ListBoxOption, true, selected, label)
    });
    ui.painter().rect_filled(rect, 0.0, fill);
    if resp.hovered() || selected {
        ui.painter().rect_stroke(
            rect.shrink(1.0),
            0.0,
            Stroke::new(1.0, ink.gamma_multiply(0.6)),
            StrokeKind::Inside,
        );
    }
    ui.painter().text(
        pos2(rect.left() + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::BODY_SIZE),
        ink,
    );
    resp
}

/// `ToggleOption`: a full-width row with a checkbox glyph, highlighted
/// when on. Flips `checked` when clicked (unless disabled).
pub fn toggle_option(ui: &mut Ui, label: &str, checked: &mut bool, enabled: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, mut resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), CONTROL_HEIGHT), Sense::click());
    let on = *checked;
    resp.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, enabled, on, label));
    if enabled && resp.clicked() {
        *checked = !*checked;
        resp.mark_changed();
    }
    let hovered = enabled && resp.hovered();
    let (fill, stroke) = if *checked {
        (p.bg_selected, Stroke::new(1.0, p.accent))
    } else if hovered {
        (p.bg_button_hover, Stroke::NONE)
    } else {
        (p.bg_button, Stroke::NONE)
    };
    let alpha = if enabled { 1.0 } else { 0.5 };
    ui.painter().rect(
        rect,
        CONTROL_RADIUS,
        fill.gamma_multiply(alpha),
        stroke,
        StrokeKind::Outside,
    );
    let glyph = if *checked {
        icons::SQUARE_CHECK
    } else {
        icons::SQUARE
    };
    ui.painter().text(
        pos2(rect.left() + 14.0 + 9.0, rect.center().y),
        Align2::CENTER_CENTER,
        glyph,
        icons::font(15.0),
        if *checked { p.accent } else { p.text_muted }.gamma_multiply(alpha),
    );
    ui.painter().text(
        pos2(rect.left() + 14.0 + 18.0 + 10.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(theme::BODY_SIZE),
        p.text_primary.gamma_multiply(alpha),
    );
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

/// `RestartBanner`: the message and a "Restart now" button on the selected
/// wash. Returns `true` when the button is clicked.
pub fn restart_banner(ui: &mut Ui, message: Option<&str>) -> bool {
    let p = theme::palette(ui.ctx());
    let message = message.unwrap_or("Restart required to apply changes.");
    ui.add_space(10.0);
    let mut restart = false;
    Frame::new()
        .fill(p.bg_selected)
        .stroke(Stroke::new(1.0, p.accent))
        .corner_radius(CONTROL_RADIUS)
        .inner_margin(Margin::symmetric(14, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(message).size(13.0).color(p.text_primary));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let galley = semibold_galley(ui, "Restart now", 12.0, Color32::PLACEHOLDER);
                    let pad = vec2(14.0, 6.0);
                    let (rect, resp) =
                        ui.allocate_exact_size(galley.size() + pad * 2.0, Sense::click());
                    resp.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::Role::Button, true, "Restart now")
                    });
                    let fill = if resp.hovered() {
                        p.accent_hover
                    } else {
                        p.accent
                    };
                    ui.painter().rect_filled(rect, theme::RADIUS, fill);
                    ui.painter().galley(rect.min + pad, galley, p.accent_on);
                    restart = resp
                        .on_hover_cursor(egui::CursorIcon::PointingHand)
                        .clicked();
                });
            });
        });
    restart
}

/// `ShortcutsList`: shortcuts grouped by section (in input order) with
/// zebra rows and key chips. Usually fed [`crate::commands::Registry::shortcut_list`].
pub fn shortcuts_list(ui: &mut Ui, shortcuts: &[ShortcutEntry], empty_message: Option<&str>) {
    let p = theme::palette(ui.ctx());
    if shortcuts.is_empty() {
        ui.add_space(12.0);
        ui.label(
            RichText::new(empty_message.unwrap_or("No keyboard shortcuts registered."))
                .size(13.0)
                .color(p.text_muted),
        );
        return;
    }
    let mut row = 0;
    for (i, s) in shortcuts.iter().enumerate() {
        if i == 0 || s.section != shortcuts[i - 1].section {
            if i > 0 {
                ui.add_space(12.0);
            }
            ui.horizontal(|ui| {
                ui.add_space(12.0);
                ui.label(
                    fonts::weight(s.section.to_uppercase(), fonts::SEMIBOLD)
                        .size(11.0)
                        .color(p.text_muted)
                        .extra_letter_spacing(0.5),
                );
            });
            ui.add_space(4.0);
            row = 0;
        }
        let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::hover());
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Label, true, format!("{}: {}", s.action, s.keys))
        });
        if row % 2 == 0 {
            ui.painter().rect_filled(rect, CONTROL_RADIUS, p.bg_base);
        }
        row += 1;
        ui.painter().text(
            pos2(rect.left() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            &s.action,
            FontId::proportional(13.0),
            p.text_primary,
        );
        let galley =
            ui.painter()
                .layout_no_wrap(s.keys.clone(), FontId::monospace(12.0), p.text_secondary);
        let chip = Rect::from_min_size(
            pos2(
                rect.right() - 12.0 - galley.size().x - 16.0,
                rect.center().y - galley.size().y / 2.0 - 3.0,
            ),
            galley.size() + vec2(16.0, 6.0),
        );
        ui.painter().rect(
            chip,
            theme::RADIUS,
            p.bg_button,
            Stroke::new(1.0, p.border),
            StrokeKind::Inside,
        );
        ui.painter()
            .galley(chip.min + vec2(8.0, 3.0), galley, p.text_secondary);
        ui.add_space(2.0);
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::kittest::Queryable;
    use egui_kittest::Harness;

    use super::*;

    struct State {
        theme: ThemeState,
        active: String,
        closed: bool,
        toggle: bool,
        restarted: bool,
        changed: usize,
    }

    fn harness() -> Harness<'static, State> {
        let state = State {
            theme: ThemeState::default(),
            active: "ui".into(),
            closed: false,
            toggle: false,
            restarted: false,
            changed: 0,
        };
        Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(1000.0, 800.0))
            .build_ui_state(
                |ui, s: &mut State| {
                    if s.closed {
                        return;
                    }
                    let tabs = [
                        Tab {
                            id: "ui",
                            label: "Interface",
                        },
                        Tab {
                            id: "keys",
                            label: "Shortcuts",
                        },
                    ];
                    let (theme, toggle, restarted, changed) = (
                        &mut s.theme,
                        &mut s.toggle,
                        &mut s.restarted,
                        &mut s.changed,
                    );
                    s.closed = SettingsShell::new(&tabs).show(
                        ui.ctx(),
                        &mut s.active,
                        |ui, tab| match tab {
                            "ui" => {
                                setting_group(ui, "Theme", None, |ui| {
                                    *changed += theme_list(ui, theme) as usize
                                });
                                setting_group(
                                    ui,
                                    "UI scale",
                                    Some("Zoom the whole window."),
                                    |ui| *changed += scale_list(ui, theme) as usize,
                                );
                                toggle_option(ui, "Spell check", toggle, true);
                                *restarted |= restart_banner(ui, None);
                            }
                            _ => shortcuts_list(
                                ui,
                                &[ShortcutEntry {
                                    keys: "Ctrl+N".into(),
                                    action: "New File".into(),
                                    section: "File".into(),
                                }],
                                None,
                            ),
                        },
                    );
                },
                state,
            )
    }

    #[test]
    fn theme_selection_changes_state() {
        let mut h = harness();
        h.run();
        h.get_by_label("Theme").click();
        h.run();
        h.get_by_label("Light-Mulberry").click();
        h.run();
        assert_eq!(h.state().theme.theme, ThemeId::LightMulberry);
        assert_eq!(h.state().changed, 1);
        assert!(!h.state().closed, "picking a theme keeps settings open");
        assert_eq!(
            theme::palette(&h.ctx).accent,
            ThemeId::LightMulberry.palette().accent,
            "applied"
        );
    }

    #[test]
    fn scale_toggle_restart_and_tabs() {
        let mut h = harness();
        h.run();
        h.get_by_label("UI scale").click();
        h.run();
        h.get_by_label("150%").click();
        h.run();
        assert_eq!(h.state().theme.scale, "150");

        h.get_by_label("Spell check").click();
        h.run();
        assert!(h.state().toggle);
        h.get_by_label("Restart now").click();
        h.run();
        assert!(h.state().restarted);

        h.get_by_label("Shortcuts").click();
        h.run();
        assert_eq!(h.state().active, "keys");
        h.get_by_label("New File: Ctrl+N");
    }

    #[test]
    fn ok_and_escape_close() {
        let mut h = harness();
        h.run();
        h.get_by_label("OK").click();
        h.run();
        assert!(h.state().closed);

        let mut h = harness();
        h.run();
        h.key_press(Key::Escape);
        h.run();
        assert!(h.state().closed);
    }
}
