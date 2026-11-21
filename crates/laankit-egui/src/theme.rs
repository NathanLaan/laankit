//! The theme system: the mode (light, dark, or the system's), the theme
//! (the Classic or Modern [`Shape`]), the colour (a light and a dark
//! [`Palette`] each), the UI scale steps, and how they map onto egui's
//! `Visuals` and zoom factor.
//!
//! The seven [`ThemeId`] palettes are generated from
//! `packages/desktop-ui/src/theme/index.svelte.js` (one field per CSS
//! variable, same names). They are the sets of Blue, Purple, Midnight,
//! Mulberry and Rasberryson that existed as themes, and they are still the
//! slide themes. Theme and scale ids are the same strings the Svelte builds
//! store, so a saved choice carries over between builds.

use egui::{Color32, CornerRadius, Shadow, Stroke, Visuals};
use serde::{Deserialize, Serialize};

/// `--radius` for buttons, inputs and menus (`border-radius: 4px` in `global.css`).
pub const RADIUS: CornerRadius = CornerRadius::same(4);
/// Modals and panes (`border-radius: 12px`).
pub const RADIUS_LARGE: CornerRadius = CornerRadius::same(12);
/// Body text size (`font-size: 14px` on `html, body`).
pub const BODY_SIZE: f32 = 14.0;
/// The title bar's height (`.titlebar { height: 32px }`).
pub const TITLEBAR_HEIGHT: f32 = 32.0;

/// The UI scale steps: (id, label, zoom factor). Ids match `UI_SCALES`.
pub const SCALES: [(&str, &str, f32); 10] = [
    ("50", "50%", 0.5),
    ("75", "75%", 0.75),
    ("100", "100%", 1.0),
    ("110", "110%", 1.1),
    ("120", "120%", 1.2),
    ("130", "130%", 1.3),
    ("140", "140%", 1.4),
    ("150", "150%", 1.5),
    ("175", "175%", 1.75),
    ("200", "200%", 2.0),
];
const DEFAULT_SCALE: &str = "100";

/// One theme's colours. Field names are the CSS variables without `--`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    /// Light themes are drawn on egui's light `Visuals`.
    pub dark: bool,
    /// `--bg-base`.
    pub bg_base: Color32,
    /// `--bg-surface`.
    pub bg_surface: Color32,
    /// `--bg-overlay`.
    pub bg_overlay: Color32,
    /// `--bg-button`.
    pub bg_button: Color32,
    /// `--bg-button-hover`.
    pub bg_button_hover: Color32,
    /// `--bg-selected`.
    pub bg_selected: Color32,
    /// `--bg-drag-over`.
    pub bg_drag_over: Color32,
    /// `--bg-item-hover`.
    pub bg_item_hover: Color32,
    /// `--border`.
    pub border: Color32,
    /// `--border-hover`.
    pub border_hover: Color32,
    /// `--text-primary`.
    pub text_primary: Color32,
    /// `--text-secondary`.
    pub text_secondary: Color32,
    /// `--text-tertiary`.
    pub text_tertiary: Color32,
    /// `--text-muted`.
    pub text_muted: Color32,
    /// `--text-faint`.
    pub text_faint: Color32,
    /// `--accent`.
    pub accent: Color32,
    /// `--accent-hover`.
    pub accent_hover: Color32,
    /// `--accent-on`.
    pub accent_on: Color32,
    /// `--scrollbar-track`.
    pub scrollbar_track: Color32,
    /// `--scrollbar-thumb`.
    pub scrollbar_thumb: Color32,
    /// `--scrollbar-thumb-hover`.
    pub scrollbar_thumb_hover: Color32,
    /// `--input-bg`.
    pub input_bg: Color32,
    /// `--input-border`.
    pub input_border: Color32,
    /// `--input-border-focus`.
    pub input_border_focus: Color32,
    /// `--modal-overlay`.
    pub modal_overlay: Color32,
    /// `--code-bg`.
    pub code_bg: Color32,
    /// `--pre-bg`.
    pub pre_bg: Color32,
    /// `--blockquote-border`.
    pub blockquote_border: Color32,
    /// `--tag-bg`.
    pub tag_bg: Color32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ThemeId {
    #[default]
    Midnight,
    Dark,
    DarkPurple,
    Light,
    LightPurple,
    LightMulberry,
    LightRasberryson,
}

impl ThemeId {
    pub const ALL: [ThemeId; 7] = [
        ThemeId::Midnight,
        ThemeId::Dark,
        ThemeId::DarkPurple,
        ThemeId::Light,
        ThemeId::LightPurple,
        ThemeId::LightMulberry,
        ThemeId::LightRasberryson,
    ];

    /// The id the Svelte builds store (`midnight`, `darkPurple`, …).
    pub fn id(self) -> &'static str {
        match self {
            ThemeId::Midnight => "midnight",
            ThemeId::Dark => "dark",
            ThemeId::DarkPurple => "darkPurple",
            ThemeId::Light => "light",
            ThemeId::LightPurple => "lightPurple",
            ThemeId::LightMulberry => "lightMulberry",
            ThemeId::LightRasberryson => "lightRasberryson",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ThemeId::Midnight => "Midnight",
            ThemeId::Dark => "Dark",
            ThemeId::DarkPurple => "Dark-Purple",
            ThemeId::Light => "Light",
            ThemeId::LightPurple => "Light-Purple",
            ThemeId::LightMulberry => "Light-Mulberry",
            ThemeId::LightRasberryson => "Light-Rasberryson",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.id() == id)
    }

    pub fn palette(self) -> &'static Palette {
        match self {
            ThemeId::Midnight => &MIDNIGHT,
            ThemeId::Dark => &DARK,
            ThemeId::DarkPurple => &DARK_PURPLE,
            ThemeId::Light => &LIGHT,
            ThemeId::LightPurple => &LIGHT_PURPLE,
            ThemeId::LightMulberry => &LIGHT_MULBERRY,
            ThemeId::LightRasberryson => &LIGHT_RASBERRYSON,
        }
    }
}

/// Whether the light or the dark set of a [`Colour`] is used.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Mode {
    Light,
    #[default]
    Dark,
    /// Follows the computer's setting, and Dark where it reports none.
    System,
}

impl Mode {
    pub const ALL: [Mode; 3] = [Mode::Light, Mode::Dark, Mode::System];

    pub fn id(self) -> &'static str {
        match self {
            Mode::Light => "light",
            Mode::Dark => "dark",
            Mode::System => "system",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Mode::Light => "Light",
            Mode::Dark => "Dark",
            Mode::System => "System",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|m| m.id() == id)
    }
}

/// The Theme setting: which [`Shape`] the widgets are drawn with.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Look {
    #[default]
    Classic,
    Modern,
}

impl Look {
    pub const ALL: [Look; 2] = [Look::Classic, Look::Modern];

    pub fn id(self) -> &'static str {
        match self {
            Look::Classic => "classic",
            Look::Modern => "modern",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Look::Classic => "Classic",
            Look::Modern => "Modern",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|l| l.id() == id)
    }

    pub fn shape(self) -> &'static Shape {
        match self {
            Look::Classic => &CLASSIC,
            Look::Modern => &MODERN,
        }
    }
}

/// A family of colours, with a light and a dark [`Palette`].
///
/// | Colour      | Dark set              | Light set                |
/// |-------------|-----------------------|--------------------------|
/// | Blue        | [`ThemeId::Dark`]     | [`ThemeId::Light`]       |
/// | Purple      | [`ThemeId::DarkPurple`] | [`ThemeId::LightPurple`] |
/// | Midnight    | [`ThemeId::Midnight`] | Its own                  |
/// | Mulberry    | Its own               | [`ThemeId::LightMulberry`] |
/// | Rasberryson | Its own               | [`ThemeId::LightRasberryson`] |
/// | Neutral     | Its own               | Its own                  |
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Colour {
    Blue,
    Purple,
    #[default]
    Midnight,
    Mulberry,
    Rasberryson,
    Neutral,
}

impl Colour {
    pub const ALL: [Colour; 6] = [
        Colour::Blue,
        Colour::Purple,
        Colour::Midnight,
        Colour::Mulberry,
        Colour::Rasberryson,
        Colour::Neutral,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Colour::Blue => "blue",
            Colour::Purple => "purple",
            Colour::Midnight => "midnight",
            Colour::Mulberry => "mulberry",
            Colour::Rasberryson => "rasberryson",
            Colour::Neutral => "neutral",
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Colour::Blue => "Blue",
            Colour::Purple => "Purple",
            Colour::Midnight => "Midnight",
            Colour::Mulberry => "Mulberry",
            Colour::Rasberryson => "Rasberryson",
            Colour::Neutral => "Neutral",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.id() == id)
    }

    /// The colour's dark or light set.
    pub fn palette(self, dark: bool) -> &'static Palette {
        match (self, dark) {
            (Colour::Blue, true) => &DARK,
            (Colour::Blue, false) => &LIGHT,
            (Colour::Purple, true) => &DARK_PURPLE,
            (Colour::Purple, false) => &LIGHT_PURPLE,
            (Colour::Midnight, true) => &MIDNIGHT,
            (Colour::Midnight, false) => &MIDNIGHT_LIGHT,
            (Colour::Mulberry, true) => &MULBERRY_DARK,
            (Colour::Mulberry, false) => &LIGHT_MULBERRY,
            (Colour::Rasberryson, true) => &RASBERRYSON_DARK,
            (Colour::Rasberryson, false) => &LIGHT_RASBERRYSON,
            (Colour::Neutral, true) => &NEUTRAL_DARK,
            (Colour::Neutral, false) => &NEUTRAL_LIGHT,
        }
    }
}

impl ThemeId {
    /// The mode and colour that draw this theme, with the same palette.
    pub fn parts(self) -> (Mode, Colour) {
        match self {
            ThemeId::Midnight => (Mode::Dark, Colour::Midnight),
            ThemeId::Dark => (Mode::Dark, Colour::Blue),
            ThemeId::DarkPurple => (Mode::Dark, Colour::Purple),
            ThemeId::Light => (Mode::Light, Colour::Blue),
            ThemeId::LightPurple => (Mode::Light, Colour::Purple),
            ThemeId::LightMulberry => (Mode::Light, Colour::Mulberry),
            ThemeId::LightRasberryson => (Mode::Light, Colour::Rasberryson),
        }
    }

    /// The nearest of the seven themes to a colour in light or dark: the
    /// same palette where there is one, and otherwise Dark or Light.
    pub fn nearest(colour: Colour, dark: bool) -> ThemeId {
        match (colour, dark) {
            (Colour::Midnight, true) => ThemeId::Midnight,
            (Colour::Purple, true) => ThemeId::DarkPurple,
            (_, true) => ThemeId::Dark,
            (Colour::Purple, false) => ThemeId::LightPurple,
            (Colour::Mulberry, false) => ThemeId::LightMulberry,
            (Colour::Rasberryson, false) => ThemeId::LightRasberryson,
            (_, false) => ThemeId::Light,
        }
    }
}

/// The user's mode, theme, colour and scale. Apps persist it (eframe storage,
/// and for ThreadLiner also the synced settings file, as `themeState.set`
/// does), call [`ThemeState::apply`] whenever it changes, and call
/// [`ThemeState::follow_system`] each frame.
///
/// Saved as `{ theme, mode, style, colour, scale }`. `theme` is the nearest
/// of the seven [`ThemeId`]s, which is all an older build reads; a state
/// saved by an older build has only `theme` and `scale`, and opens as that
/// theme's [`ThemeId::parts`] in Classic.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(from = "Saved", into = "Saved")]
pub struct ThemeState {
    pub mode: Mode,
    /// The Theme setting.
    pub look: Look,
    pub colour: Colour,
    /// A [`SCALES`] id.
    pub scale: String,
    /// What the computer last reported, for [`Mode::System`].
    system_dark: Option<bool>,
}

/// [`ThemeState`] as it is saved. Ids that aren't known are ignored.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Saved {
    theme: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    colour: Option<String>,
    scale: Option<String>,
}

impl From<Saved> for ThemeState {
    fn from(s: Saved) -> Self {
        let mut state = Self::from_ids(s.theme.as_deref(), s.scale.as_deref());
        if let Some(m) = s.mode.as_deref().and_then(Mode::from_id) {
            state.mode = m;
        }
        if let Some(l) = s.style.as_deref().and_then(Look::from_id) {
            state.look = l;
        }
        if let Some(c) = s.colour.as_deref().and_then(Colour::from_id) {
            state.colour = c;
        }
        state
    }
}

impl From<ThemeState> for Saved {
    fn from(s: ThemeState) -> Self {
        Self {
            theme: Some(s.legacy().id().to_string()),
            mode: Some(s.mode.id().to_string()),
            style: Some(s.look.id().to_string()),
            colour: Some(s.colour.id().to_string()),
            scale: Some(s.scale),
        }
    }
}

impl Default for ThemeState {
    fn default() -> Self {
        Self::of(ThemeId::default())
    }
}

impl ThemeState {
    /// One of the seven themes, in Classic at 100%.
    pub fn of(theme: ThemeId) -> Self {
        let (mode, colour) = theme.parts();
        Self {
            mode,
            look: Look::Classic,
            colour,
            scale: DEFAULT_SCALE.into(),
            system_dark: None,
        }
    }

    /// Builds a state from the ids an older build stores, ignoring unknown
    /// ones (as `init` does).
    pub fn from_ids(theme: Option<&str>, scale: Option<&str>) -> Self {
        let mut state = Self::default();
        if let Some(t) = theme.and_then(ThemeId::from_id) {
            state.set_legacy(t);
        }
        if let Some(s) = scale.filter(|s| scale_index(s).is_some()) {
            state.scale = s.to_string();
        }
        state
    }

    /// Sets the mode and colour to those of one of the seven themes.
    pub fn set_legacy(&mut self, theme: ThemeId) {
        (self.mode, self.colour) = theme.parts();
    }

    /// The nearest of the seven themes: what an older build is told, and
    /// what a new presentation's slide theme starts as.
    pub fn legacy(&self) -> ThemeId {
        ThemeId::nearest(self.colour, self.dark())
    }

    /// Whether the dark set is in use.
    pub fn dark(&self) -> bool {
        match self.mode {
            Mode::Light => false,
            Mode::Dark => true,
            Mode::System => self.system_dark.unwrap_or(true),
        }
    }

    pub fn palette(&self) -> &'static Palette {
        self.colour.palette(self.dark())
    }

    pub fn shape(&self) -> &'static Shape {
        self.look.shape()
    }

    /// The zoom factor for [`Self::scale`].
    pub fn zoom(&self) -> f32 {
        scale_index(&self.scale).map_or(1.0, |i| SCALES[i].2)
    }

    /// Sets the scale by id; unknown ids are ignored.
    pub fn set_scale(&mut self, id: &str) {
        if scale_index(id).is_some() {
            self.scale = id.to_string();
        }
    }

    pub fn zoom_in(&mut self) {
        let i = scale_index(&self.scale).unwrap_or(2);
        self.scale = SCALES[(i + 1).min(SCALES.len() - 1)].0.to_string();
    }

    pub fn zoom_out(&mut self) {
        let i = scale_index(&self.scale).unwrap_or(2);
        self.scale = SCALES[i.saturating_sub(1)].0.to_string();
    }

    pub fn zoom_reset(&mut self) {
        self.scale = DEFAULT_SCALE.to_string();
    }

    /// Reads the computer's light or dark setting, and applies the theme
    /// again when [`Mode::System`] is chosen and it changed. Call it each
    /// frame. Returns whether what is drawn changed.
    pub fn follow_system(&mut self, ctx: &egui::Context) -> bool {
        let reported = ctx.system_theme().map(|t| t == egui::Theme::Dark);
        if reported == self.system_dark {
            return false;
        }
        let before = self.dark();
        self.system_dark = reported;
        if self.dark() == before {
            return false;
        }
        self.apply(ctx);
        true
    }

    /// Whether the computer reports a light or dark setting, as of the last
    /// [`Self::follow_system`].
    pub fn system_reported(&self) -> bool {
        self.system_dark.is_some()
    }

    /// Applies the mode, theme, colour and scale to `ctx`, and makes the
    /// palette and shape available to widgets through [`palette`] and
    /// [`shape`].
    pub fn apply(&self, ctx: &egui::Context) {
        let p = *self.palette();
        let shape = *self.shape();
        ctx.data_mut(|d| {
            d.insert_temp(egui::Id::NULL, p);
            d.insert_temp(egui::Id::NULL, shape);
        });
        // egui keeps a light and a dark style and picks by the system's
        // setting unless told: name the one these visuals are for.
        ctx.set_theme(if p.dark {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        });
        ctx.set_visuals(visuals(&p, &shape));
        // Classic is egui's own spacing, set again in case Modern was on.
        ctx.all_styles_mut(|style| {
            let egui = egui::style::Spacing::default();
            let s = &mut style.spacing;
            s.button_padding = if shape.modern {
                egui::vec2(12.0, 7.0)
            } else {
                egui.button_padding
            };
            s.scroll.bar_width = if shape.modern {
                6.0
            } else {
                egui.scroll.bar_width
            };
        });
        // Ctrl +/-/0 step through SCALES (the app's zoom commands), not egui's own zoom.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.set_zoom_factor(self.zoom());
    }
}

/// What the Theme setting decides: the sizes and corners of the shared
/// widgets, and how a few of them are drawn. Classic holds the values the
/// widgets had before there was a choice.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Shape {
    /// The Modern drawing of the title bar, toolbar, pane headers, tabs,
    /// buttons and scroll bars.
    pub modern: bool,
    /// Corners of buttons and inputs.
    pub control_radius: u8,
    /// Corners of a row in a list, tree or menu.
    pub item_radius: u8,
    /// How far a row's hover and selection fill sits in from each side.
    pub item_inset: f32,
    /// Corners of menus and popups.
    pub menu_radius: u8,
    /// Corners of dialogs.
    pub dialog_radius: u8,
    /// The height of a tree row.
    pub row_height: f32,
    /// The height of a pane's header.
    pub pane_header_height: f32,
    /// Whether menus, popups, dialogs and toasts cast the soft shadow of
    /// [`Shape::popup_shadow`].
    pub shadow: bool,
}

impl Shape {
    /// The corner for a widget whose Classic corner is `classic` px: that
    /// in Classic, and the next rounder step in Modern.
    pub fn round(&self, classic: u8) -> CornerRadius {
        CornerRadius::same(match (self.modern, classic) {
            (false, r) | (true, r @ 0) => r,
            (true, 1..=6) => self.control_radius,
            (true, 7..=10) => 12,
            (true, r) => r.max(self.dialog_radius),
        })
    }

    pub fn control(&self) -> CornerRadius {
        CornerRadius::same(self.control_radius)
    }

    pub fn item(&self) -> CornerRadius {
        CornerRadius::same(self.item_radius)
    }

    pub fn menu(&self) -> CornerRadius {
        CornerRadius::same(self.menu_radius)
    }

    pub fn dialog(&self) -> CornerRadius {
        CornerRadius::same(self.dialog_radius)
    }

    /// The shadow under a menu, popup, dialog or toast that a widget draws
    /// itself. Classic draws none; egui's own windows and popups keep
    /// egui's shadow there.
    pub fn popup_shadow(&self) -> Shadow {
        if self.shadow {
            Shadow {
                offset: [0, 6],
                blur: 20,
                spread: 0,
                color: Color32::from_black_alpha(70),
            }
        } else {
            Shadow::NONE
        }
    }

    /// The rectangle a row's hover or selection fill is drawn in.
    pub fn item_rect(&self, row: egui::Rect) -> egui::Rect {
        if self.item_inset > 0.0 {
            row.shrink2(egui::vec2(self.item_inset, 1.0))
        } else {
            row
        }
    }
}

pub const CLASSIC: Shape = Shape {
    modern: false,
    control_radius: 4,
    item_radius: 0,
    item_inset: 0.0,
    menu_radius: 4,
    dialog_radius: 12,
    row_height: 26.0,
    pane_header_height: 44.0,
    shadow: false,
};

pub const MODERN: Shape = Shape {
    modern: true,
    control_radius: 8,
    item_radius: 6,
    item_inset: 6.0,
    menu_radius: 10,
    dialog_radius: 14,
    row_height: 28.0,
    pane_header_height: 32.0,
    shadow: true,
};

/// The shape last applied to `ctx` (Classic before any [`ThemeState::apply`]).
pub fn shape(ctx: &egui::Context) -> Shape {
    ctx.data(|d| d.get_temp(egui::Id::NULL)).unwrap_or(CLASSIC)
}

/// The palette last applied to `ctx` (Midnight before any [`ThemeState::apply`]).
pub fn palette(ctx: &egui::Context) -> Palette {
    ctx.data(|d| d.get_temp(egui::Id::NULL))
        .unwrap_or(*ThemeId::default().palette())
}

fn scale_index(id: &str) -> Option<usize> {
    SCALES.iter().position(|s| s.0 == id)
}

/// egui's widget styling from a palette, following `global.css`.
pub fn visuals(p: &Palette, shape: &Shape) -> Visuals {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg_base;
    v.window_fill = p.bg_surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = shape.dialog();
    v.menu_corner_radius = shape.menu();
    if shape.shadow {
        v.window_shadow = shape.popup_shadow();
        v.popup_shadow = shape.popup_shadow();
    }
    v.extreme_bg_color = p.input_bg;
    v.text_edit_bg_color = Some(p.input_bg);
    v.faint_bg_color = p.bg_item_hover;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    // The outline of an input with the focus: a ring in Modern.
    let focus = if shape.modern { 2.0 } else { 1.0 };
    v.selection.stroke = Stroke::new(focus, p.input_border_focus);

    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.bg_surface;
    w.noninteractive.weak_bg_fill = p.bg_surface;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.border);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.inactive.bg_fill = p.bg_button;
    w.inactive.weak_bg_fill = p.bg_button;
    w.inactive.bg_stroke = Stroke::new(1.0, p.input_border);
    w.inactive.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.hovered.bg_fill = p.bg_button_hover;
    w.hovered.weak_bg_fill = p.bg_button_hover;
    w.hovered.bg_stroke = Stroke::new(1.0, p.border_hover);
    w.hovered.fg_stroke = Stroke::new(1.0, p.text_primary);
    w.active.bg_fill = p.accent;
    w.active.weak_bg_fill = p.accent;
    w.active.bg_stroke = Stroke::new(1.0, p.input_border_focus);
    w.active.fg_stroke = Stroke::new(1.0, p.accent_on);
    w.open.bg_fill = p.bg_button_hover;
    w.open.weak_bg_fill = p.bg_button_hover;
    w.open.bg_stroke = Stroke::new(1.0, p.border_hover);
    w.open.fg_stroke = Stroke::new(1.0, p.text_primary);
    for s in [
        &mut w.noninteractive,
        &mut w.inactive,
        &mut w.hovered,
        &mut w.active,
        &mut w.open,
    ] {
        s.corner_radius = shape.control();
    }
    v
}

const fn hex(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Color32 {
    Color32::from_rgba_unmultiplied_const(r, g, b, a)
}

const MIDNIGHT: Palette = Palette {
    dark: true,
    bg_base: hex(0x181825),
    bg_surface: hex(0x1e1e2e),
    bg_overlay: hex(0x11111b),
    bg_button: hex(0x313244),
    bg_button_hover: hex(0x45475a),
    bg_selected: rgba(137, 180, 250, 26),
    bg_drag_over: rgba(137, 180, 250, 38),
    bg_item_hover: rgba(255, 255, 255, 10),
    border: hex(0x313244),
    border_hover: hex(0x585b70),
    text_primary: hex(0xcdd6f4),
    text_secondary: hex(0xa6adc8),
    text_tertiary: hex(0xbac2de),
    text_muted: hex(0x6c7086),
    text_faint: hex(0x45475a),
    accent: hex(0x89b4fa),
    accent_hover: hex(0x74c7ec),
    accent_on: hex(0x1e1e2e),
    scrollbar_track: hex(0x1e1e2e),
    scrollbar_thumb: hex(0x45475a),
    scrollbar_thumb_hover: hex(0x585b70),
    input_bg: hex(0x11111b),
    input_border: hex(0x313244),
    input_border_focus: hex(0x89b4fa),
    modal_overlay: rgba(0, 0, 0, 153),
    code_bg: hex(0x313244),
    pre_bg: hex(0x11111b),
    blockquote_border: hex(0x45475a),
    tag_bg: hex(0x313244),
};
const DARK: Palette = Palette {
    dark: true,
    bg_base: hex(0x1a1a1a),
    bg_surface: hex(0x242424),
    bg_overlay: hex(0x111111),
    bg_button: hex(0x333333),
    bg_button_hover: hex(0x444444),
    bg_selected: rgba(100, 160, 255, 31),
    bg_drag_over: rgba(100, 160, 255, 38),
    bg_item_hover: rgba(255, 255, 255, 13),
    border: hex(0x333333),
    border_hover: hex(0x555555),
    text_primary: hex(0xe0e0e0),
    text_secondary: hex(0xaaaaaa),
    text_tertiary: hex(0xcccccc),
    text_muted: hex(0x777777),
    text_faint: hex(0x555555),
    accent: hex(0x64a0ff),
    accent_hover: hex(0x5090ee),
    accent_on: hex(0x111111),
    scrollbar_track: hex(0x242424),
    scrollbar_thumb: hex(0x444444),
    scrollbar_thumb_hover: hex(0x555555),
    input_bg: hex(0x111111),
    input_border: hex(0x333333),
    input_border_focus: hex(0x64a0ff),
    modal_overlay: rgba(0, 0, 0, 166),
    code_bg: hex(0x333333),
    pre_bg: hex(0x111111),
    blockquote_border: hex(0x444444),
    tag_bg: hex(0x333333),
};
const DARK_PURPLE: Palette = Palette {
    dark: true,
    bg_base: hex(0x1a1a1a),
    bg_surface: hex(0x242424),
    bg_overlay: hex(0x111111),
    bg_button: hex(0x333333),
    bg_button_hover: hex(0x444444),
    bg_selected: rgba(98, 92, 150, 46),
    bg_drag_over: rgba(98, 92, 150, 56),
    bg_item_hover: rgba(255, 255, 255, 13),
    border: hex(0x333333),
    border_hover: hex(0x555555),
    text_primary: hex(0xe0e0e0),
    text_secondary: hex(0xaaaaaa),
    text_tertiary: hex(0xcccccc),
    text_muted: hex(0x777777),
    text_faint: hex(0x555555),
    accent: hex(0x625c96),
    accent_hover: hex(0x524c86),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0x242424),
    scrollbar_thumb: hex(0x444444),
    scrollbar_thumb_hover: hex(0x555555),
    input_bg: hex(0x111111),
    input_border: hex(0x333333),
    input_border_focus: hex(0x625c96),
    modal_overlay: rgba(0, 0, 0, 166),
    code_bg: hex(0x333333),
    pre_bg: hex(0x111111),
    blockquote_border: hex(0x444444),
    tag_bg: hex(0x333333),
};
const LIGHT: Palette = Palette {
    dark: false,
    bg_base: hex(0xf5f5f5),
    bg_surface: hex(0xffffff),
    bg_overlay: hex(0xe8e8e8),
    bg_button: hex(0xe0e0e0),
    bg_button_hover: hex(0xd0d0d0),
    bg_selected: rgba(50, 120, 220, 26),
    bg_drag_over: rgba(50, 120, 220, 31),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xd4d4d4),
    border_hover: hex(0xb0b0b0),
    text_primary: hex(0x1a1a1a),
    text_secondary: hex(0x555555),
    text_tertiary: hex(0x333333),
    text_muted: hex(0x888888),
    text_faint: hex(0xbbbbbb),
    accent: hex(0x3278dc),
    accent_hover: hex(0x2868c8),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0xf0f0f0),
    scrollbar_thumb: hex(0xc0c0c0),
    scrollbar_thumb_hover: hex(0xa0a0a0),
    input_bg: hex(0xf0f0f0),
    input_border: hex(0xd4d4d4),
    input_border_focus: hex(0x3278dc),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xe8e8e8),
    pre_bg: hex(0xf0f0f0),
    blockquote_border: hex(0xd4d4d4),
    tag_bg: hex(0xe0e0e0),
};
const LIGHT_PURPLE: Palette = Palette {
    dark: false,
    bg_base: hex(0xf5f5f5),
    bg_surface: hex(0xffffff),
    bg_overlay: hex(0xe8e8e8),
    bg_button: hex(0xe0e0e0),
    bg_button_hover: hex(0xd0d0d0),
    bg_selected: rgba(50, 45, 88, 26),
    bg_drag_over: rgba(50, 45, 88, 31),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xd4d4d4),
    border_hover: hex(0xb0b0b0),
    text_primary: hex(0x1a1a1a),
    text_secondary: hex(0x555555),
    text_tertiary: hex(0x333333),
    text_muted: hex(0x888888),
    text_faint: hex(0xbbbbbb),
    accent: hex(0x322d58),
    accent_hover: hex(0x221d48),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0xf0f0f0),
    scrollbar_thumb: hex(0xc0c0c0),
    scrollbar_thumb_hover: hex(0xa0a0a0),
    input_bg: hex(0xf0f0f0),
    input_border: hex(0xd4d4d4),
    input_border_focus: hex(0x322d58),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xe8e8e8),
    pre_bg: hex(0xf0f0f0),
    blockquote_border: hex(0xd4d4d4),
    tag_bg: hex(0xe0e0e0),
};
const LIGHT_MULBERRY: Palette = Palette {
    dark: false,
    bg_base: hex(0xf5f5f5),
    bg_surface: hex(0xffffff),
    bg_overlay: hex(0xe8e8e8),
    bg_button: hex(0xe0e0e0),
    bg_button_hover: hex(0xd0d0d0),
    bg_selected: rgba(126, 13, 79, 26),
    bg_drag_over: rgba(126, 13, 79, 31),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xd4d4d4),
    border_hover: hex(0xb0b0b0),
    text_primary: hex(0x1a1a1a),
    text_secondary: hex(0x555555),
    text_tertiary: hex(0x333333),
    text_muted: hex(0x888888),
    text_faint: hex(0xbbbbbb),
    accent: hex(0x7e0d4f),
    accent_hover: hex(0x6e003f),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0xf0f0f0),
    scrollbar_thumb: hex(0xc0c0c0),
    scrollbar_thumb_hover: hex(0xa0a0a0),
    input_bg: hex(0xf0f0f0),
    input_border: hex(0xd4d4d4),
    input_border_focus: hex(0x7e0d4f),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xe8e8e8),
    pre_bg: hex(0xf0f0f0),
    blockquote_border: hex(0xd4d4d4),
    tag_bg: hex(0xe0e0e0),
};
const LIGHT_RASBERRYSON: Palette = Palette {
    dark: false,
    bg_base: hex(0xf5f5f5),
    bg_surface: hex(0xffffff),
    bg_overlay: hex(0xe8e8e8),
    bg_button: hex(0xe0e0e0),
    bg_button_hover: hex(0xd0d0d0),
    bg_selected: rgba(212, 38, 78, 26),
    bg_drag_over: rgba(212, 38, 78, 31),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xd4d4d4),
    border_hover: hex(0xb0b0b0),
    text_primary: hex(0x1a1a1a),
    text_secondary: hex(0x555555),
    text_tertiary: hex(0x333333),
    text_muted: hex(0x888888),
    text_faint: hex(0xbbbbbb),
    accent: hex(0xd4264e),
    accent_hover: hex(0xc4163e),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0xf0f0f0),
    scrollbar_thumb: hex(0xc0c0c0),
    scrollbar_thumb_hover: hex(0xa0a0a0),
    input_bg: hex(0xf0f0f0),
    input_border: hex(0xd4d4d4),
    input_border_focus: hex(0xd4264e),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xe8e8e8),
    pre_bg: hex(0xf0f0f0),
    blockquote_border: hex(0xd4d4d4),
    tag_bg: hex(0xe0e0e0),
};

// The sets below were not themes before. Mulberry and Rasberryson in dark
// are Dark with the accent lightened to read on a dark surface, as
// Dark-Purple is. Midnight is Catppuccin Mocha, so its light set is
// Catppuccin Latte with the same roles. Neutral is greyscale: every colour
// in it, the accent included, is a grey.

const MULBERRY_DARK: Palette = Palette {
    bg_selected: rgba(224, 106, 168, 31),
    bg_drag_over: rgba(224, 106, 168, 38),
    accent: hex(0xe06aa8),
    accent_hover: hex(0xd0589a),
    accent_on: hex(0x111111),
    input_border_focus: hex(0xe06aa8),
    ..DARK
};

const RASBERRYSON_DARK: Palette = Palette {
    bg_selected: rgba(255, 111, 139, 31),
    bg_drag_over: rgba(255, 111, 139, 38),
    accent: hex(0xff6f8b),
    accent_hover: hex(0xee5d7a),
    accent_on: hex(0x111111),
    input_border_focus: hex(0xff6f8b),
    ..DARK
};

const MIDNIGHT_LIGHT: Palette = Palette {
    dark: false,
    bg_base: hex(0xe6e9ef),
    bg_surface: hex(0xeff1f5),
    bg_overlay: hex(0xdce0e8),
    bg_button: hex(0xccd0da),
    bg_button_hover: hex(0xbcc0cc),
    bg_selected: rgba(24, 88, 224, 26),
    bg_drag_over: rgba(24, 88, 224, 31),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xccd0da),
    border_hover: hex(0xacb0be),
    text_primary: hex(0x4c4f69),
    text_secondary: hex(0x5c5f77),
    text_tertiary: hex(0x55586f),
    text_muted: hex(0x7c7f93),
    text_faint: hex(0xbcc0cc),
    accent: hex(0x1858e0),
    accent_hover: hex(0x209fb5),
    accent_on: hex(0xeff1f5),
    scrollbar_track: hex(0xe6e9ef),
    scrollbar_thumb: hex(0xbcc0cc),
    scrollbar_thumb_hover: hex(0xacb0be),
    input_bg: hex(0xdce0e8),
    input_border: hex(0xccd0da),
    input_border_focus: hex(0x1858e0),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xccd0da),
    pre_bg: hex(0xdce0e8),
    blockquote_border: hex(0xbcc0cc),
    tag_bg: hex(0xccd0da),
};

const NEUTRAL_DARK: Palette = Palette {
    dark: true,
    bg_base: hex(0x171717),
    bg_surface: hex(0x1f1f1f),
    bg_overlay: hex(0x121212),
    bg_button: hex(0x2b2b2b),
    bg_button_hover: hex(0x383838),
    bg_selected: rgba(255, 255, 255, 31),
    bg_drag_over: rgba(255, 255, 255, 46),
    bg_item_hover: rgba(255, 255, 255, 13),
    border: hex(0x2f2f2f),
    border_hover: hex(0x4d4d4d),
    text_primary: hex(0xe6e6e6),
    text_secondary: hex(0xababab),
    text_tertiary: hex(0xcacaca),
    text_muted: hex(0x8c8c8c),
    text_faint: hex(0x4d4d4d),
    accent: hex(0xd6d6d6),
    accent_hover: hex(0xc2c2c2),
    accent_on: hex(0x141414),
    scrollbar_track: hex(0x1f1f1f),
    scrollbar_thumb: hex(0x3b3b3b),
    scrollbar_thumb_hover: hex(0x4d4d4d),
    input_bg: hex(0x121212),
    input_border: hex(0x2f2f2f),
    input_border_focus: hex(0xd6d6d6),
    modal_overlay: rgba(0, 0, 0, 166),
    code_bg: hex(0x2b2b2b),
    pre_bg: hex(0x121212),
    blockquote_border: hex(0x3b3b3b),
    tag_bg: hex(0x2b2b2b),
};

const NEUTRAL_LIGHT: Palette = Palette {
    dark: false,
    bg_base: hex(0xf4f4f4),
    bg_surface: hex(0xffffff),
    bg_overlay: hex(0xeaeaea),
    bg_button: hex(0xe7e7e7),
    bg_button_hover: hex(0xdadada),
    bg_selected: rgba(0, 0, 0, 26),
    bg_drag_over: rgba(0, 0, 0, 38),
    bg_item_hover: rgba(0, 0, 0, 10),
    border: hex(0xe4e4e4),
    border_hover: hex(0xbcbcbc),
    text_primary: hex(0x1e1e1e),
    text_secondary: hex(0x545454),
    text_tertiary: hex(0x353535),
    text_muted: hex(0x707070),
    text_faint: hex(0xbcbcbc),
    accent: hex(0x3a3a3a),
    accent_hover: hex(0x262626),
    accent_on: hex(0xffffff),
    scrollbar_track: hex(0xf1f1f1),
    scrollbar_thumb: hex(0xc6c6c6),
    scrollbar_thumb_hover: hex(0xa8a8a8),
    input_bg: hex(0xf1f1f1),
    input_border: hex(0xd9d9d9),
    input_border_focus: hex(0x3a3a3a),
    modal_overlay: rgba(0, 0, 0, 76),
    code_bg: hex(0xeaeaea),
    pre_bg: hex(0xf1f1f1),
    blockquote_border: hex(0xd9d9d9),
    tag_bg: hex(0xe7e7e7),
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::from_id(t.id()), Some(t));
        }
        assert_eq!(ThemeId::from_id("nope"), None);
        for m in Mode::ALL {
            assert_eq!(Mode::from_id(m.id()), Some(m));
        }
        for l in Look::ALL {
            assert_eq!(Look::from_id(l.id()), Some(l));
        }
        for c in Colour::ALL {
            assert_eq!(Colour::from_id(c.id()), Some(c));
        }
    }

    #[test]
    fn each_old_theme_is_a_mode_and_a_colour_with_the_same_palette() {
        for t in ThemeId::ALL {
            let state = ThemeState::of(t);
            assert_eq!(state.look, Look::Classic);
            assert!(std::ptr::eq(state.palette(), t.palette()), "{}", t.name());
            assert_eq!(state.legacy(), t);
        }
        assert_eq!(ThemeState::default(), ThemeState::of(ThemeId::Midnight));
    }

    #[test]
    fn a_state_saved_by_an_older_build_opens_as_its_theme() {
        let s: ThemeState =
            serde_json::from_str(r#"{"theme":"lightMulberry","scale":"120"}"#).unwrap();
        assert_eq!(
            (s.mode, s.look, s.colour, s.scale.as_str()),
            (Mode::Light, Look::Classic, Colour::Mulberry, "120")
        );
        // Ids that aren't known fall back, one by one.
        let s: ThemeState = serde_json::from_str(
            r#"{"theme":"neon","mode":"dusk","colour":"purple","scale":"33"}"#,
        )
        .unwrap();
        assert_eq!(
            s,
            ThemeState {
                colour: Colour::Purple,
                ..ThemeState::default()
            }
        );
    }

    #[test]
    fn a_state_is_saved_with_the_nearest_old_theme() {
        let mut state = ThemeState::of(ThemeId::DarkPurple);
        state.scale = "120".into();
        let json = serde_json::to_string(&state).unwrap();
        assert_eq!(
            json,
            r#"{"theme":"darkPurple","mode":"dark","style":"classic","colour":"purple","scale":"120"}"#
        );
        assert_eq!(serde_json::from_str::<ThemeState>(&json).unwrap(), state);

        for colour in Colour::ALL {
            for mode in [Mode::Light, Mode::Dark] {
                for look in Look::ALL {
                    let state = ThemeState {
                        mode,
                        look,
                        colour,
                        ..ThemeState::default()
                    };
                    let json = serde_json::to_string(&state).unwrap();
                    assert_eq!(serde_json::from_str::<ThemeState>(&json).unwrap(), state);
                    // The old theme is of the same mode, and of the same
                    // colour where that colour was a theme in that mode.
                    let old = state.legacy();
                    assert_eq!(old.palette().dark, state.dark());
                    if std::ptr::eq(old.palette(), state.palette()) {
                        assert_eq!(old.parts(), (mode, colour));
                    }
                }
            }
        }
        // A colour that was a theme in one mode only.
        let mut state = ThemeState::of(ThemeId::LightMulberry);
        state.mode = Mode::Dark;
        assert_eq!(state.legacy(), ThemeId::Dark);
    }

    #[test]
    fn changing_the_mode_keeps_the_colour() {
        for colour in Colour::ALL {
            assert!(colour.palette(true).dark, "{}", colour.name());
            assert!(!colour.palette(false).dark, "{}", colour.name());
        }
        // System is Dark until the computer reports otherwise.
        let state = ThemeState {
            mode: Mode::System,
            ..ThemeState::default()
        };
        assert!(state.dark() && !state.system_reported());
    }

    /// WCAG contrast of two opaque colours.
    fn contrast(a: Color32, b: Color32) -> f32 {
        let lum = |c: Color32| {
            let f = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.03928 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * f(c.r()) + 0.7152 * f(c.g()) + 0.0722 * f(c.b())
        };
        let (hi, lo) = if lum(a) > lum(b) {
            (lum(a), lum(b))
        } else {
            (lum(b), lum(a))
        };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn the_new_sets_can_be_read() {
        // Text at 4.5 to 1. Muted text and the accent at 3 to 1, which is
        // where the seven older sets have them.
        for p in [
            &MULBERRY_DARK,
            &RASBERRYSON_DARK,
            &MIDNIGHT_LIGHT,
            &NEUTRAL_DARK,
            &NEUTRAL_LIGHT,
        ] {
            for bg in [p.bg_surface, p.bg_base] {
                for (ink, least) in [
                    (p.text_primary, 4.5),
                    (p.text_secondary, 4.5),
                    (p.text_tertiary, 4.5),
                    (p.text_muted, 3.0),
                    (p.accent, 3.0),
                ] {
                    assert!(
                        contrast(ink, bg) >= least,
                        "{ink:?} on {bg:?}: {}",
                        contrast(ink, bg)
                    );
                }
            }
            assert!(contrast(p.accent_on, p.accent) >= 4.5, "{:?}", p.accent);
        }
    }

    #[test]
    fn neutral_is_greyscale() {
        for p in [&NEUTRAL_DARK, &NEUTRAL_LIGHT] {
            for c in [
                p.bg_base,
                p.bg_surface,
                p.bg_overlay,
                p.bg_button,
                p.bg_button_hover,
                p.bg_selected,
                p.bg_drag_over,
                p.bg_item_hover,
                p.border,
                p.border_hover,
                p.text_primary,
                p.text_secondary,
                p.text_tertiary,
                p.text_muted,
                p.text_faint,
                p.accent,
                p.accent_hover,
                p.accent_on,
                p.scrollbar_track,
                p.scrollbar_thumb,
                p.scrollbar_thumb_hover,
                p.input_bg,
                p.input_border,
                p.input_border_focus,
                p.modal_overlay,
                p.code_bg,
                p.pre_bg,
                p.blockquote_border,
                p.tag_bg,
            ] {
                assert!(c.r() == c.g() && c.g() == c.b(), "{c:?}");
            }
        }
    }

    #[test]
    fn classic_holds_the_values_the_widgets_had() {
        assert_eq!(CLASSIC.control(), RADIUS);
        assert_eq!(CLASSIC.menu(), RADIUS);
        assert_eq!(CLASSIC.dialog(), RADIUS_LARGE);
        assert_eq!(CLASSIC.item(), CornerRadius::ZERO);
        let row = egui::Rect::from_min_size(egui::pos2(3.0, 4.0), egui::vec2(100.0, 26.0));
        assert_eq!(CLASSIC.item_rect(row), row);
        assert_eq!(CLASSIC.popup_shadow(), Shadow::NONE);
        for r in [0, 4, 6, 10, 12, 18] {
            assert_eq!(CLASSIC.round(r), CornerRadius::same(r));
            assert!(MODERN.round(r).nw >= r);
        }
        assert_eq!(ThemeState::default().shape(), &CLASSIC);
    }

    #[test]
    fn zoom_steps_clamp() {
        let mut s = ThemeState::from_ids(Some("light"), Some("200"));
        s.zoom_in();
        assert_eq!(s.scale, "200");
        s.zoom_out();
        assert_eq!(s.scale, "175");
        s.zoom_reset();
        assert_eq!(s.zoom(), 1.0);
        assert_eq!(
            ThemeState::from_ids(Some("bogus"), Some("33")),
            ThemeState::default()
        );
    }

    #[test]
    fn light_sets_use_light_visuals() {
        for colour in Colour::ALL {
            for dark in [false, true] {
                for look in Look::ALL {
                    let p = colour.palette(dark);
                    assert_eq!(
                        visuals(p, look.shape()).dark_mode,
                        dark,
                        "{}",
                        colour.name()
                    );
                }
            }
        }
    }
}
