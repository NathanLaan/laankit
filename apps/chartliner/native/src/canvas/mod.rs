//! The drawing canvas: the grid, pan and zoom, and (from plan phase 2) the
//! diagram and the tools that edit it.
//!
//! | Input                                   | Does                          |
//! |-----------------------------------------|-------------------------------|
//! | Wheel / trackpad scroll                 | Pans                          |
//! | Middle drag, or Space + drag            | Pans                          |
//! | Ctrl + wheel, pinch                     | Zooms about the pointer       |
//! | Ctrl+= / Ctrl+- / Ctrl+0 (commands)     | Zooms about the centre        |

mod grid;
pub mod view;

use chartliner_core::Diagram;
use egui::{CursorIcon, Key, PointerButton, Pos2, Rect, Response, Sense, Ui, Vec2};
use marina_egui::theme;

use view::View;

/// The drawing tools (the toolbar's top group).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Select,
    Rect,
    RoundRect,
    Line,
    Arrow,
    Text,
}

impl Tool {
    pub const ALL: [Tool; 6] = [
        Tool::Select,
        Tool::Rect,
        Tool::RoundRect,
        Tool::Line,
        Tool::Arrow,
        Tool::Text,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Rect => "Rectangle",
            Tool::RoundRect => "Rounded Rectangle",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::Text => "Text",
        }
    }
}

pub struct Canvas {
    pub view: View,
    pub show_grid: bool,
    /// The pointer in model coordinates, while it is over the canvas.
    pub pointer: Option<Pos2>,
    /// Last frame's rectangle, for zooming about the centre from the keyboard.
    rect: Rect,
}

impl Canvas {
    pub fn new(show_grid: bool) -> Self {
        Self {
            view: View::default(),
            show_grid,
            pointer: None,
            rect: Rect::ZERO,
        }
    }

    /// One [`view::ZOOM_STEPS`] step in (`dir > 0`) or out, about the centre.
    pub fn zoom_step(&mut self, dir: i32) {
        let zoom = self.view.step(dir);
        self.view.zoom_at(self.rect.min, self.rect.center(), zoom);
    }

    /// Back to 100 %, about the centre.
    pub fn zoom_reset(&mut self) {
        self.view.zoom_at(self.rect.min, self.rect.center(), 1.0);
    }

    /// Fills the rest of `ui`. `keys`: whether Space may start a pan (no
    /// dialog or text field has the keyboard).
    pub fn show(&mut self, ui: &mut Ui, diagram: &Diagram, tool: Tool, keys: bool) -> Response {
        let p = theme::palette(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.rect = rect;

        let space = keys && ui.input(|i| i.key_down(Key::Space));
        let panning = resp.dragged_by(PointerButton::Middle)
            || (space && resp.dragged_by(PointerButton::Primary));
        if panning {
            self.view.pan_by(resp.drag_delta());
        }
        if let Some(hover) = resp.hover_pos() {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0 {
                self.view.zoom_at(rect.min, hover, self.view.zoom * zoom);
            } else if scroll != Vec2::ZERO {
                self.view.pan_by(scroll);
            }
        }
        self.pointer = resp.hover_pos().map(|s| self.view.to_model(rect.min, s));

        if panning {
            ui.ctx().set_cursor_icon(CursorIcon::Grabbing);
        } else if resp.hovered() && space {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        } else if resp.hovered() && tool != Tool::Select {
            ui.ctx().set_cursor_icon(CursorIcon::Crosshair);
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.bg_base);
        if self.show_grid {
            let minor = p.border.gamma_multiply(0.45);
            grid::paint(&painter, rect, self.view, diagram.grid, minor, p.border);
        }
        resp
    }
}
