//! The desktop-ui theme system, natively: the six themes, the UI scale steps,
//! and how they map onto egui's `Visuals` and zoom factor.
//!
//! The palettes are generated from `packages/desktop-ui/src/theme/index.svelte.js`
//! (one field per CSS variable, same names), so both UI stacks stay in step.
//! Theme and scale ids are the same strings the Svelte builds store, so a
//! saved choice carries over between builds.

use egui::{Color32, CornerRadius, Stroke, Visuals};
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
}

impl ThemeId {
    pub const ALL: [ThemeId; 6] = [
        ThemeId::Midnight,
        ThemeId::Dark,
        ThemeId::DarkPurple,
        ThemeId::Light,
        ThemeId::LightPurple,
        ThemeId::LightMulberry,
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
        }
    }
}

/// The user's theme and scale. Apps persist it (eframe storage, and for
/// ThreadLiner also the synced settings file, as `themeState.set` does) and
/// call [`ThemeState::apply`] whenever it changes.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ThemeState {
    pub theme: ThemeId,
    /// A [`SCALES`] id.
    pub scale: String,
}

impl Default for ThemeState {
    fn default() -> Self {
        Self {
            theme: ThemeId::default(),
            scale: DEFAULT_SCALE.into(),
        }
    }
}

impl ThemeState {
    /// Builds a state from stored ids, ignoring unknown ones (as `init` does).
    pub fn from_ids(theme: Option<&str>, scale: Option<&str>) -> Self {
        let mut state = Self::default();
        if let Some(t) = theme.and_then(ThemeId::from_id) {
            state.theme = t;
        }
        if let Some(s) = scale.filter(|s| scale_index(s).is_some()) {
            state.scale = s.to_string();
        }
        state
    }

    pub fn palette(&self) -> &'static Palette {
        self.theme.palette()
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

    /// Applies the theme and scale to `ctx`, and makes the palette available
    /// to widgets through [`palette`].
    pub fn apply(&self, ctx: &egui::Context) {
        let p = *self.palette();
        ctx.data_mut(|d| d.insert_temp(egui::Id::NULL, p));
        ctx.set_visuals(visuals(&p));
        // Ctrl +/-/0 step through SCALES (the app's zoom commands), not egui's own zoom.
        ctx.options_mut(|o| o.zoom_with_keyboard = false);
        ctx.set_zoom_factor(self.zoom());
    }
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
pub fn visuals(p: &Palette) -> Visuals {
    let mut v = if p.dark {
        Visuals::dark()
    } else {
        Visuals::light()
    };
    v.panel_fill = p.bg_base;
    v.window_fill = p.bg_surface;
    v.window_stroke = Stroke::new(1.0, p.border);
    v.window_corner_radius = RADIUS_LARGE;
    v.menu_corner_radius = RADIUS;
    v.extreme_bg_color = p.input_bg;
    v.text_edit_bg_color = Some(p.input_bg);
    v.faint_bg_color = p.bg_item_hover;
    v.code_bg_color = p.code_bg;
    v.hyperlink_color = p.accent;
    v.selection.bg_fill = p.accent.gamma_multiply(0.35);
    v.selection.stroke = Stroke::new(1.0, p.input_border_focus);

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
        s.corner_radius = RADIUS;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_round_trip() {
        for t in ThemeId::ALL {
            assert_eq!(ThemeId::from_id(t.id()), Some(t));
        }
        assert_eq!(ThemeId::from_id("nope"), None);
    }

    #[test]
    fn serde_uses_svelte_ids() {
        let json = serde_json::to_string(&ThemeState {
            theme: ThemeId::DarkPurple,
            scale: "120".into(),
        })
        .unwrap();
        assert_eq!(json, r#"{"theme":"darkPurple","scale":"120"}"#);
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
    fn light_themes_use_light_visuals() {
        for t in ThemeId::ALL {
            assert_eq!(
                visuals(t.palette()).dark_mode,
                t.palette().dark,
                "{}",
                t.name()
            );
        }
    }
}
