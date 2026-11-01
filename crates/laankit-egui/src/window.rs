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
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("window-border"),
    ))
    .rect_stroke(
        r,
        0.0,
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
