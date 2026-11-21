//! Window chrome for undecorated windows: moving and maximizing from the
//! title bar, the caption buttons, and resizing from the edges. Together
//! these stand in for the Electron frame options and `-webkit-app-region:
//! drag` that `TitleBar.svelte` relies on.
//!
//! Adapted from ArtCraft's `artcraft-native/src/ui/window.rs` (MIT OR
//! Apache-2.0, see `NOTICE`). The caption buttons draw Font Awesome's window
//! icons, as the Svelte title bar does.

use egui::{vec2, Color32, Rect, Response, RichText, Sense, Ui};

use crate::icons;

/// How far in from the window's edges a drag resizes. Title bar controls leave this band alone.
pub const RESIZE_EDGE: f32 = 5.0;
/// A caption button's width (`.titlebar-btn { width: 48px }`).
pub const BUTTON_WIDTH: f32 = 48.0;
/// The close button's hover red (`.titlebar-btn.close:hover`).
pub const CLOSE_RED: Color32 = Color32::from_rgb(0xe8, 0x11, 0x23);
/// Caption button icons (`font-size: 12px`).
const ICON_SIZE: f32 = 12.0;

/// Whether this platform draws its own caption buttons; macOS keeps its traffic lights.
pub fn custom_caption_buttons() -> bool {
    !cfg!(target_os = "macos")
}

/// Space to leave at the title bar's left for macOS's traffic lights.
pub fn leading_inset() -> f32 {
    if cfg!(target_os = "macos") {
        76.0
    } else {
        0.0
    }
}

/// Window options for an undecorated window with this crate's chrome. On
/// macOS the title bar is hidden but the traffic lights stay.
pub fn undecorated(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    if cfg!(target_os = "macos") {
        viewport
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false)
    } else {
        viewport.with_decorations(false)
    }
}

/// [`undecorated`], and see-through where nothing is drawn, so that the
/// window's corners can be rounded. An app that uses this also calls
/// [`allow_rounded_corners`] each frame and returns [`CLEAR`] from
/// `eframe::App::clear_color`. [`window_edges`] then cuts the corners out
/// of whatever the app drew, so no panel has to round its own.
pub fn undecorated_rounded(viewport: egui::ViewportBuilder) -> egui::ViewportBuilder {
    undecorated(viewport).with_transparent(true)
}

/// The clear colour of a window made with [`undecorated_rounded`]: nothing.
pub const CLEAR: [f32; 4] = [0.0; 4];

/// What an app returns from `eframe::App::clear_color`: nothing where the
/// theme rounds the window's corners and the window is `undecorated`, and
/// the panel colour otherwise.
pub fn clear_color(
    theme: &crate::theme::ThemeState,
    undecorated: bool,
    visuals: &egui::Visuals,
) -> [f32; 4] {
    if undecorated && custom_caption_buttons() && theme.shape().window_radius > 0 {
        CLEAR
    } else {
        visuals.panel_fill.to_normalized_gamma_f32()
    }
}

fn rounded_id() -> egui::Id {
    egui::Id::unique("laankit-window-rounded")
}

/// Says that the window was made with [`undecorated_rounded`], so its
/// corners may be rounded.
pub fn allow_rounded_corners(ctx: &egui::Context) {
    ctx.data_mut(|d| d.insert_temp(rounded_id(), true));
}

/// The radius of the window's corners now: the theme's
/// ([`crate::theme::Shape::window_radius`], none in Classic), and none while
/// the window is maximized or full screen, where the app didn't call
/// [`allow_rounded_corners`], or on macOS, which rounds its windows itself.
pub fn corner_radius(ctx: &egui::Context) -> u8 {
    let allowed: bool = ctx.data(|d| d.get_temp(rounded_id())).unwrap_or(false);
    let filling = ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    });
    if !allowed || filling || !custom_caption_buttons() {
        0
    } else {
        crate::theme::shape(ctx).window_radius
    }
}

/// Dragging the title bar's empty space moves the window; double-clicking
/// maximizes or restores. Call before drawing the title bar's controls so
/// they stay on top.
pub fn title_bar_drag(ui: &mut Ui) {
    let r = ui.max_rect();
    let rect = Rect::from_min_max(
        r.min + vec2(RESIZE_EDGE, RESIZE_EDGE),
        r.max - vec2(RESIZE_EDGE, 0.0),
    );
    let resp = ui.interact(
        rect,
        ui.scope_id().with("title-bar-drag"),
        Sense::click_and_drag(),
    );
    if resp.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
    }
    if resp.double_clicked() {
        toggle_maximized(ui.ctx());
    }
}

/// Maximizes the window, or restores it when it already is.
pub fn toggle_maximized(ctx: &egui::Context) {
    let maximized = is_maximized(ctx);
    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
}

pub fn is_maximized(ctx: &egui::Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Caption {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Minimize, maximize (or restore) and close, `height` tall, in a
/// right-to-left layout so close ends up at the right edge. `ink` is the
/// icon colour (the title bar's `--accent-on`).
pub fn caption_buttons(ui: &mut Ui, height: f32, ink: Color32) {
    let ctx = ui.ctx().clone();
    let maximized = is_maximized(&ctx);
    if caption_button(ui, Caption::Close, height, ink).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
    let size = if maximized {
        Caption::Restore
    } else {
        Caption::Maximize
    };
    if caption_button(ui, size, height, ink).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
    }
    if caption_button(ui, Caption::Minimize, height, ink).clicked() {
        ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }
}

fn caption_button(ui: &mut Ui, button: Caption, height: f32, ink: Color32) -> Response {
    let (rect, _) = ui.allocate_exact_size(vec2(BUTTON_WIDTH, height), Sense::hover());
    // Clickable below the resize band along the top (and, for close, the right edge).
    let right = if button == Caption::Close {
        RESIZE_EDGE
    } else {
        0.0
    };
    let hit = Rect::from_min_max(
        rect.min + vec2(0.0, RESIZE_EDGE),
        rect.max - vec2(right, 0.0),
    );
    let resp = ui.interact(
        hit,
        ui.scope_id().with(("caption", button as u8)),
        Sense::click(),
    );
    let (label, glyph) = match button {
        Caption::Minimize => ("Minimize", icons::WINDOW_MINIMIZE),
        Caption::Maximize => ("Maximize", icons::WINDOW_MAXIMIZE),
        Caption::Restore => ("Restore", icons::WINDOW_RESTORE),
        Caption::Close => ("Close", icons::XMARK),
    };
    let hovered = resp.hovered();
    let (fill, ink) = match (button, hovered) {
        (Caption::Close, true) => (CLOSE_RED, Color32::WHITE),
        // Modern's bar is the window's colour, where black doesn't show.
        (_, true) if crate::theme::shape(ui.ctx()).modern => {
            (crate::theme::palette(ui.ctx()).bg_button, ink)
        }
        // `background: rgba(0, 0, 0, 0.18); opacity: 1`
        (_, true) => (Color32::from_black_alpha(46), ink),
        // `opacity: 0.75`
        (_, false) => (Color32::TRANSPARENT, ink.gamma_multiply(0.75)),
    };
    let p = ui.painter();
    p.rect_filled(rect, 0.0, fill);
    p.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        icons::font(ICON_SIZE),
        ink,
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, label));
    resp.on_hover_text(RichText::new(label))
}

/// Clears the window's four corners outside a `radius` curve, so the desktop
/// shows there. egui can only draw over what is there, so this clears the
/// pixels directly, a row at a time. The hairline drawn after it smooths
/// the edge.
fn cut_corners(rect: Rect, radius: f32) -> egui::Shape {
    use eframe::glow::{self, HasContext as _};
    let cut = move |info: egui::PaintCallbackInfo, painter: &eframe::egui_glow::Painter| {
        let gl = painter.gl();
        let [w, h] = info.screen_size_px.map(|v| v as i32);
        let r = (radius * info.pixels_per_point).round();
        // Safety: plain state changes and clears on the context being painted.
        unsafe {
            gl.enable(glow::SCISSOR_TEST);
            gl.clear_color(0.0, 0.0, 0.0, 0.0);
            for row in 0..r as i32 {
                let dy = r - (row as f32 + 0.5);
                let width = (r - (r * r - dy * dy).sqrt()).round() as i32;
                if width <= 0 {
                    continue;
                }
                for y in [h - 1 - row, row] {
                    for x in [0, w - width] {
                        gl.scissor(x, y, width, 1);
                        gl.clear(glow::COLOR_BUFFER_BIT);
                    }
                }
            }
        }
    };
    egui::Shape::Callback(egui::PaintCallback {
        rect,
        callback: std::sync::Arc::new(eframe::egui_glow::CallbackFn::new(cut)),
    })
}

/// A hairline round the window, and resizing from its edges and corners
/// (when not maximized). Call once per frame, after the panels.
pub fn window_edges(ctx: &egui::Context) {
    use egui::viewport::ResizeDirection as D;
    use egui::CursorIcon as C;
    let (maximized, fullscreen) = ctx.input(|i| {
        (
            i.viewport().maximized.unwrap_or(false),
            i.viewport().fullscreen.unwrap_or(false),
        )
    });
    if maximized || fullscreen || !custom_caption_buttons() {
        return;
    }
    let r = ctx.content_rect();
    let border = crate::theme::palette(ctx).border;
    let radius = corner_radius(ctx);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("window-border"),
    ));
    if radius > 0 {
        painter.add(cut_corners(r, f32::from(radius)));
    }
    painter.rect_stroke(
        r,
        radius,
        egui::Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
    let Some(p) = ctx
        .input(|i| i.pointer.hover_pos())
        .filter(|p| r.contains(*p))
    else {
        return;
    };
    let near = |d: f32| d < RESIZE_EDGE;
    // A corner takes a little of each edge next to it.
    let corner = |d: f32| d < 3.0 * RESIZE_EDGE;
    let (left, right, top, bottom) = (
        p.x - r.left(),
        r.right() - p.x,
        p.y - r.top(),
        r.bottom() - p.y,
    );
    let hit = if (near(top) && corner(left)) || (near(left) && corner(top)) {
        Some((D::NorthWest, C::ResizeNorthWest))
    } else if (near(top) && corner(right)) || (near(right) && corner(top)) {
        Some((D::NorthEast, C::ResizeNorthEast))
    } else if (near(bottom) && corner(left)) || (near(left) && corner(bottom)) {
        Some((D::SouthWest, C::ResizeSouthWest))
    } else if (near(bottom) && corner(right)) || (near(right) && corner(bottom)) {
        Some((D::SouthEast, C::ResizeSouthEast))
    } else if near(top) {
        Some((D::North, C::ResizeNorth))
    } else if near(bottom) {
        Some((D::South, C::ResizeSouth))
    } else if near(left) {
        Some((D::West, C::ResizeWest))
    } else if near(right) {
        Some((D::East, C::ResizeEast))
    } else {
        None
    };
    if let Some((direction, cursor)) = hit {
        ctx.set_cursor_icon(cursor);
        if ctx.input(|i| i.pointer.primary_pressed()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
        }
    }
}
