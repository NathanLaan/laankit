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
use crate::fonts::{self, EditorFont, FontKind};
use crate::icons;
use crate::theme::{self, Colour, Look, Mode, Palette, ThemeState, SCALES};

/// `.modal-header { height: 32px; padding: 0 24px }`.
const HEADER_HEIGHT: f32 = 32.0;
/// `border-radius: 6px` on the settings controls.
/// A list's or checkbox row's height: [`CONTROL_HEIGHT`] in Classic.
fn control_height(ctx: &egui::Context) -> f32 {
    (CONTROL_HEIGHT * theme::shape(ctx).control_scale).round()
}

fn control_radius(ctx: &egui::Context) -> CornerRadius {
    theme::shape(ctx).round(6)
}
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
        let modal = Modal::new(Id::unique("laankit-settings"))
            .area(modal_area(Id::unique("laankit-settings"), rect.min))
            .backdrop_color(p.modal_overlay)
            .frame(modal_frame(&p, &theme::shape(ctx)))
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

/// The area every LaanKit modal lives in.
pub(crate) fn modal_area(id: Id, pos: egui::Pos2) -> Area {
    Area::new(id)
        .kind(egui::UiKind::Modal)
        .order(Order::Foreground)
        .fixed_pos(pos)
}

/// `.modal`: the surface with a hairline and 12 px corners.
pub(crate) fn modal_frame(p: &Palette, shape: &theme::Shape) -> Frame {
    Frame::new()
        .fill(p.bg_surface)
        .stroke(Stroke::new(1.0, p.border))
        .corner_radius(shape.dialog())
        .shadow(shape.popup_shadow())
}

/// `.modal-header`: the accent bar with the upper-case title and the close
/// button. Returns `true` when the close button is clicked.
pub(crate) fn modal_header(ui: &mut Ui, p: &Palette, title: &str) -> bool {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), HEADER_HEIGHT), Sense::hover());
    let shape = theme::shape(ui.ctx());
    let r = shape.dialog_radius;
    let top = CornerRadius {
        nw: r,
        ne: r,
        sw: 0,
        se: 0,
    };
    // Classic: the accent bar, with the title in capitals. Modern: the
    // dialog's own colour, the title as written, and a line under it.
    let ink = if shape.modern {
        ui.painter().rect_filled(rect, top, p.bg_surface);
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            Stroke::new(1.0, p.border),
        );
        p.text_primary
    } else {
        ui.painter().rect_filled(rect, top, p.accent);
        p.accent_on
    };
    let title = if shape.modern {
        semibold_galley(ui, title, 14.0, ink)
    } else {
        semibold_galley(ui, &title.to_uppercase(), 14.0, ink)
    };
    ui.painter().galley(
        pos2(rect.left() + 24.0, rect.center().y - title.size().y / 2.0),
        title,
        ink,
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
        let hover = if shape.modern {
            p.bg_button
        } else {
            Color32::from_black_alpha(46)
        };
        ui.painter().rect_filled(button, shape.control(), hover);
    }
    ui.painter().text(
        button.center(),
        Align2::CENTER_CENTER,
        icons::XMARK,
        icons::font(14.0),
        ink.gamma_multiply(if resp.hovered() { 1.0 } else { 0.8 }),
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
    let pad = pad * theme::shape(ui.ctx()).control_scale;
    let galley = ui.painter().layout_no_wrap(
        label.to_string(),
        FontId::proportional(theme::BODY_SIZE),
        Color32::PLACEHOLDER,
    );
    let size = galley.size() + pad * 2.0;
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    // Modern: filled with the accent, with no outline, as a dialog's
    // primary button is.
    let modern = theme::shape(ui.ctx()).modern;
    let (fill, ink) = match (modern, resp.hovered()) {
        (true, true) => (p.accent_hover, p.accent_on),
        (true, false) | (false, true) => (p.accent, p.accent_on),
        (false, false) => (p.bg_selected, p.accent),
    };
    let outline = if modern {
        Stroke::NONE
    } else {
        Stroke::new(1.0, p.accent)
    };
    ui.painter().rect(
        rect,
        control_radius(ui.ctx()),
        fill,
        outline,
        StrokeKind::Outside,
    );
    ui.painter().galley(rect.min + pad, galley, ink);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// The caption above a setting or a field: 11 px capitals in Classic, and
/// the label as written, a little larger, in Modern.
pub fn caption(ctx: &egui::Context, label: &str) -> RichText {
    let p = theme::palette(ctx);
    if theme::shape(ctx).modern {
        fonts::weight(label, fonts::SEMIBOLD)
            .size(12.0)
            .color(p.text_secondary)
    } else {
        fonts::weight(label.to_uppercase(), fonts::SEMIBOLD)
            .size(11.0)
            .color(p.text_muted)
            .extra_letter_spacing(0.5)
    }
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
        ui.label(caption(ui.ctx(), label));
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

/// The Mode, Theme and Colour groups, each a full-width dropdown that
/// changes its own part of `state` and leaves the other two. `colours` are
/// the ones the app offers (ThreadLiner offers Blue and Midnight). The Theme
/// group is shown when `looks` is set: an app sets it once its own screens
/// are drawn from the [`theme::Shape`]. Applies a choice to the context and
/// returns `true` when one changed, so the app can persist it.
pub fn appearance(ui: &mut Ui, state: &mut ThemeState, colours: &[Colour], looks: bool) -> bool {
    let before = state.clone();
    let unreported = state.mode == Mode::System && !state.system_reported();
    setting_group(
        ui,
        "Mode",
        unreported.then_some(
            "This computer does not report a light or dark setting, so System uses Dark.",
        ),
        |ui| {
            let p = theme::palette(ui.ctx());
            let mut picked = None;
            dropdown(ui, &p, "Mode", state.mode.name(), |ui| {
                for m in Mode::ALL {
                    if swatch_row(ui, m.name(), p.bg_surface, p.text_primary, m == state.mode)
                        .clicked()
                    {
                        picked = Some(m);
                    }
                }
            });
            if let Some(m) = picked {
                state.mode = m;
            }
        },
    );
    if looks {
        setting_group(ui, "Theme", None, |ui| {
            let p = theme::palette(ui.ctx());
            let mut picked = None;
            dropdown(ui, &p, "Theme", state.look.name(), |ui| {
                for l in Look::ALL {
                    if swatch_row(ui, l.name(), p.bg_surface, p.text_primary, l == state.look)
                        .clicked()
                    {
                        picked = Some(l);
                    }
                }
            });
            if let Some(l) = picked {
                state.look = l;
            }
        });
    }
    setting_group(ui, "Colour", None, |ui| {
        let p = theme::palette(ui.ctx());
        let dark = state.dark();
        let mut picked = None;
        dropdown(ui, &p, "Colour", state.colour.name(), |ui| {
            for &c in colours {
                // Each option is painted in its own accent, in the mode in use.
                let cp = c.palette(dark);
                if swatch_row(ui, c.name(), cp.accent, cp.accent_on, c == state.colour).clicked() {
                    picked = Some(c);
                }
            }
        });
        if let Some(c) = picked {
            state.colour = c;
        }
    });
    if *state == before {
        return false;
    }
    state.apply(ui.ctx());
    true
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

/// A full-width `<select>` styled like [`scale_list`]: `options` are
/// `(value, label)` pairs; returns the newly picked value.
pub fn select(
    ui: &mut Ui,
    label: &str,
    options: &[(&str, &str)],
    selected: &str,
) -> Option<String> {
    let p = theme::palette(ui.ctx());
    let current = options
        .iter()
        .find(|o| o.0 == selected)
        .map_or(selected, |o| o.1);
    let mut picked = None;
    dropdown(ui, &p, label, current, |ui| {
        for (value, text) in options {
            if swatch_row(ui, text, p.bg_surface, p.text_primary, *value == selected).clicked() {
                picked = Some(value.to_string());
            }
        }
    });
    picked.filter(|v| v != selected)
}

/// A full-width select of fonts, with each font's name drawn in that font.
/// `fonts` are the ones offered; the list has a heading for each kind when
/// it offers both. Returns the newly picked font.
pub fn font_select(
    ui: &mut Ui,
    label: &str,
    fonts: &[EditorFont],
    selected: EditorFont,
) -> Option<EditorFont> {
    let p = theme::palette(ui.ctx());
    let list = ui.scope_id().with(("font-select", label));
    let mut picked = None;
    let mut shown = false;
    let kinds = [
        (FontKind::Proportional, "Proportional"),
        (FontKind::Monospace, "Monospace"),
    ];
    let both = kinds
        .iter()
        .all(|(k, _)| fonts.iter().any(|f| f.kind() == *k));
    dropdown(ui, &p, label, selected.name(), |ui| {
        shown = true;
        for (kind, heading) in kinds {
            if both {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 24.0), Sense::hover());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, heading));
                ui.painter().text(
                    pos2(rect.left() + 10.0, rect.center().y),
                    Align2::LEFT_CENTER,
                    heading,
                    FontId::proportional(11.0),
                    p.text_muted,
                );
            }
            for &font in fonts.iter().filter(|f| f.kind() == kind) {
                let face = FontId::new(theme::BODY_SIZE, font.preview_family());
                let row = option_row(
                    ui,
                    font.name(),
                    face,
                    p.bg_surface,
                    p.text_primary,
                    font == selected,
                );
                if row.clicked() {
                    picked = Some(font);
                }
            }
        }
    });
    // The fonts that are not chosen are loaded only while the list shows.
    fonts::set_previews(ui.ctx(), list, shown);
    picked.filter(|f| *f != selected)
}

/// The tallest a select's list gets (ten rows) before it scrolls.
const DROPDOWN_MAX_HEIGHT: f32 = 280.0;

/// `.theme-select` / `.scale-select`: the closed select, opening a popup
/// list below it. A long list scrolls.
fn dropdown(ui: &mut Ui, p: &Palette, label: &str, current: &str, options: impl FnOnce(&mut Ui)) {
    let (rect, resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), control_height(ui.ctx())),
        Sense::click(),
    );
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
        control_radius(ui.ctx()),
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
        // A click on the scroll bar must not close the list, so the popup
        // only closes itself on a click outside; a click on a row closes it
        // below.
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let out = egui::ScrollArea::vertical()
                .max_height(DROPDOWN_MAX_HEIGHT)
                .auto_shrink([false, true])
                .show(ui, options);
            // `open`: the click that opened the list is not a pick.
            let on_row = open
                && ui.input(|i| {
                    i.pointer.primary_clicked()
                        && i.pointer
                            .interact_pos()
                            .is_some_and(|pos| out.inner_rect.contains(pos))
                });
            if on_row {
                ui.close();
            }
        });
}

/// An option row filled with `fill` and labelled in `ink`.
fn swatch_row(ui: &mut Ui, label: &str, fill: Color32, ink: Color32, selected: bool) -> Response {
    let font = FontId::proportional(theme::BODY_SIZE);
    option_row(ui, label, font, fill, ink, selected)
}

/// [`swatch_row`] with its label in `font`.
fn option_row(
    ui: &mut Ui,
    label: &str,
    font: FontId,
    fill: Color32,
    ink: Color32,
    selected: bool,
) -> Response {
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
        font,
        ink,
    );
    resp
}

/// `ToggleOption`: a full-width row with a checkbox glyph, highlighted
/// when on. Flips `checked` when clicked (unless disabled).
pub fn toggle_option(ui: &mut Ui, label: &str, checked: &mut bool, enabled: bool) -> Response {
    let p = theme::palette(ui.ctx());
    let (rect, mut resp) = ui.allocate_exact_size(
        vec2(ui.available_width(), control_height(ui.ctx())),
        Sense::click(),
    );
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
        control_radius(ui.ctx()),
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
        .corner_radius(control_radius(ui.ctx()))
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
                    ui.painter()
                        .rect_filled(rect, theme::shape(ui.ctx()).control(), fill);
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
            ui.painter()
                .rect_filled(rect, control_radius(ui.ctx()), p.bg_base);
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
            theme::shape(ui.ctx()).control(),
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
        // Tall enough for every group at 150%, with none scrolled out of reach.
        Harness::builder()
            .with_context_setup(crate::fonts::install)
            .with_size(vec2(1000.0, 1100.0))
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
                                *changed += appearance(ui, theme, &Colour::ALL, true) as usize;
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
    fn each_of_mode_theme_and_colour_changes_its_own_part() {
        let mut h = harness();
        h.run();
        let start = h.state().theme.clone();
        assert_eq!(
            (start.mode, start.look, start.colour),
            (Mode::Dark, Look::Classic, Colour::Midnight)
        );

        h.get_by_label("Colour").click();
        h.run();
        h.get_by_label("Mulberry").click();
        h.run();
        let s = h.state().theme.clone();
        assert_eq!(
            (s.mode, s.look, s.colour),
            (Mode::Dark, Look::Classic, Colour::Mulberry)
        );
        assert_eq!(h.state().changed, 1);
        assert!(!h.state().closed, "picking a colour keeps settings open");
        assert_eq!(
            theme::palette(&h.ctx),
            *Colour::Mulberry.palette(true),
            "applied"
        );

        h.get_by_label("Mode").click();
        h.run();
        h.get_by_label("Light").click();
        h.run();
        let s = h.state().theme.clone();
        assert_eq!(
            (s.mode, s.look, s.colour),
            (Mode::Light, Look::Classic, Colour::Mulberry)
        );
        assert_eq!(
            theme::palette(&h.ctx),
            *theme::ThemeId::LightMulberry.palette(),
            "the old Light-Mulberry theme"
        );

        h.get_by_label("Theme").click();
        h.run();
        h.get_by_label("Modern").click();
        h.run();
        let s = h.state().theme.clone();
        assert_eq!(
            (s.mode, s.look, s.colour),
            (Mode::Light, Look::Modern, Colour::Mulberry)
        );
        assert_eq!(theme::shape(&h.ctx), theme::MODERN, "applied");
        assert_eq!(h.state().changed, 3);
    }

    #[test]
    fn system_mode_says_so_when_the_computer_reports_nothing() {
        let mut h = harness();
        h.run();
        h.get_by_label("Mode").click();
        h.run();
        h.get_by_label("System").click();
        h.run();
        assert_eq!(h.state().theme.mode, Mode::System);
        // The test window reports no setting: Dark is used.
        assert!(h.state().theme.dark());
        h.get_by_label_contains("so System uses Dark");
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
