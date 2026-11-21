//! Diagnostics for checking the UI without a person at the keyboard, driven
//! by environment variables (as ArtCraft's `ARTCRAFT_SCREENSHOT` is):
//!
//! | Variable                  | Effect                                                      |
//! |---------------------------|-------------------------------------------------------------|
//! | `LAANKIT_SCREENSHOT=<png>` | Saves a screenshot once the window has settled              |
//! | `LAANKIT_SCREENSHOT_EXIT`  | Closes the app after saving it                              |
//! | `LAANKIT_THEME=<id>`       | One of the seven older themes (`midnight`, `lightPurple`, …): sets the mode and colour |
//! | `LAANKIT_MODE=<id>`        | Overrides the saved mode (`light`, `dark`, `system`)        |
//! | `LAANKIT_STYLE=<id>`       | Overrides the saved theme (`classic`, `modern`)             |
//! | `LAANKIT_COLOUR=<id>`      | Overrides the saved colour (`blue`, `purple`, `midnight`, `mulberry`, `rasberryson`, `neutral`) |
//! | `LAANKIT_SCALE=<id>`       | Overrides the saved UI scale (`100`, `150`, …)              |
//!
//! On Wayland the compositor stops sending frame callbacks while the session
//! is locked or the window is hidden, so nothing is painted and the capture
//! never happens. Run with `WAYLAND_DISPLAY=` (X11/XWayland) or with the
//! session unlocked. Headless runners (CI) should use `egui_kittest` instead.

use std::path::PathBuf;

use crate::theme::ThemeState;

/// Seconds to wait before a capture, so loading work can finish.
const SETTLE_SECS: f64 = 3.0;

/// `LAANKIT_SCREENSHOT`: call [`Screenshot::tick`] at the end of every frame.
pub struct Screenshot {
    path: PathBuf,
    exit: bool,
    requested: bool,
}

impl Screenshot {
    pub fn from_env() -> Option<Self> {
        let path = std::env::var_os("LAANKIT_SCREENSHOT")?.into();
        Some(Self {
            path,
            exit: std::env::var_os("LAANKIT_SCREENSHOT_EXIT").is_some(),
            requested: false,
        })
    }

    pub fn tick(&mut self, ctx: &egui::Context) {
        ctx.request_repaint();
        if self.requested || ctx.input(|i| i.time) < SETTLE_SECS {
            return;
        }
        self.requested = true;
        let (path, exit, ctx2) = (self.path.clone(), self.exit, ctx.clone());
        ctx.request_screenshot(move |image| {
            let [w, h] = image.size;
            let pixels: Vec<u8> = image.pixels.iter().flat_map(|c| c.to_array()).collect();
            match image::RgbaImage::from_raw(w as u32, h as u32, pixels).map(|img| img.save(&path))
            {
                Some(Ok(())) => log::info!("Saved a screenshot to {}", path.display()),
                other => log::error!("Couldn't save the screenshot: {other:?}"),
            }
            if exit {
                ctx2.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ctx2.request_repaint();
        });
    }
}

/// Applies `LAANKIT_THEME`, `LAANKIT_MODE`, `LAANKIT_STYLE`,
/// `LAANKIT_COLOUR` and `LAANKIT_SCALE`, if set, to a loaded theme state.
pub fn theme_overrides(state: &mut ThemeState) {
    use crate::theme::{Colour, Look, Mode, ThemeId};
    // Unknown ids are ignored, keeping the saved value.
    let var = |name: &str| std::env::var(name).ok();
    if let Some(theme) = var("LAANKIT_THEME").and_then(|t| ThemeId::from_id(&t)) {
        state.set_legacy(theme);
    }
    if let Some(mode) = var("LAANKIT_MODE").and_then(|m| Mode::from_id(&m)) {
        state.mode = mode;
    }
    if let Some(look) = var("LAANKIT_STYLE").and_then(|l| Look::from_id(&l)) {
        state.look = look;
    }
    if let Some(colour) = var("LAANKIT_COLOUR").and_then(|c| Colour::from_id(&c)) {
        state.colour = colour;
    }
    if let Some(scale) = var("LAANKIT_SCALE") {
        state.set_scale(&scale);
    }
}
