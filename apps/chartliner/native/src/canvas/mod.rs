//! The drawing canvas: the grid, the diagram, pan and zoom, and the tools
//! that edit it.
//!
//! | Input                                   | Does                                     |
//! |-----------------------------------------|------------------------------------------|
//! | Wheel / trackpad scroll                 | Pans                                     |
//! | Middle drag, or Space + drag            | Pans                                     |
//! | Ctrl + wheel, pinch                     | Zooms about the pointer                  |
//! | Ctrl+= / Ctrl+- / Ctrl+0 (commands)     | Zooms about the centre                   |
//! | Shape tool: drag, or click              | Creates a shape (click: default size)    |
//! | Select: click, Shift+click              | Selects, toggles                         |
//! | Select: drag a shape / handle / empty   | Moves / resizes / box-selects            |
//! | Select: double-click a shape            | Edits its label                          |
//! | Alt while dragging                      | No snapping                              |

mod grid;
mod label;
mod paint;
mod tools;
pub mod view;

use chartliner_core::geometry::Handle;
use chartliner_core::snap::snap_point;
use chartliner_core::{Diagram, Point, Rect as ModelRect};
use egui::{CursorIcon, Key, PointerButton, Pos2, Rect, Sense, Ui, Vec2};
use marina_egui::theme;

pub use tools::Tool;
use tools::{Gesture, Hit, Mods};
use view::View;

/// How close (in screen points) the pointer must be to grab a handle.
const HANDLE_HIT: f32 = 6.0;

fn pt(p: Pos2) -> Point {
    Point::new(p.x, p.y)
}

fn pos(x: f32, y: f32) -> Pos2 {
    egui::pos2(x, y)
}

/// What a frame of the canvas reports back.
#[derive(Default)]
pub struct Output {
    /// A shape tool made its shape; the app goes back to Select unless the
    /// tool was picked with Shift.
    pub tool_done: bool,
}

pub struct Canvas {
    pub view: View,
    pub show_grid: bool,
    /// The pointer in model coordinates, while it is over the canvas.
    pub pointer: Option<Pos2>,
    /// Selected shape ids, in the order they were selected.
    pub selection: Vec<String>,
    gesture: Gesture,
    hover: Option<Hit>,
    label: Option<label::LabelEdit>,
    /// Last frame's rectangle, for zooming about the centre from the keyboard.
    rect: Rect,
}

impl Canvas {
    pub fn new(show_grid: bool) -> Self {
        Self {
            view: View::default(),
            show_grid,
            pointer: None,
            selection: Vec::new(),
            gesture: Gesture::Idle,
            hover: None,
            label: None,
            rect: Rect::ZERO,
        }
    }

    /// The canvas's top-left corner on screen (model origin at 100 % and no
    /// pan), as of the last frame.
    #[cfg(test)]
    pub fn origin(&self) -> Pos2 {
        self.rect.min
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
    pub fn show(&mut self, ui: &mut Ui, d: &mut Diagram, tool: Tool, keys: bool) -> Output {
        let p = theme::palette(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.rect = rect;
        self.selection.retain(|id| d.shape(id).is_some());

        let (space, alt, shift) = ui.input(|i| {
            (
                keys && i.key_down(Key::Space),
                i.modifiers.alt,
                i.modifiers.shift,
            )
        });
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
        self.pointer = resp
            .interact_pointer_pos()
            .or(resp.hover_pos())
            .map(|s| self.view.to_model(rect.min, s));

        let mut out = Output::default();
        if !space && !panning {
            let mods = Mods {
                snapping: !alt,
                shift,
            };
            out = self.input(ui, &resp, d, tool, mods);
        } else {
            self.hover = None;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.bg_base);
        if self.show_grid {
            let minor = p.border.gamma_multiply(0.45);
            grid::paint(&painter, rect, self.view, d.grid, minor, p.border);
        }
        let overlays = paint::Overlays {
            selection: &self.selection,
            hover: self.hover.as_ref(),
            editing: self.label.as_ref().map(|l| l.id.as_str()),
        };
        paint::diagram(&painter, self.view, rect.min, d, &p, &overlays);
        match &self.gesture {
            Gesture::Marquee { start, now, .. } => {
                let r = ModelRect::from_corners(*start, *now);
                paint::marquee(&painter, paint::screen_rect(self.view, rect.min, &r), &p);
            }
            // Where the shape will land: snapped, unless Alt is down.
            Gesture::Create { kind, start, now } => {
                let r = if alt {
                    ModelRect::from_corners(*start, *now)
                } else {
                    ModelRect::from_corners(snap_point(*start, d.grid), snap_point(*now, d.grid))
                };
                let r = paint::screen_rect(self.view, rect.min, &r);
                paint::preview(&painter, r, kind, self.view.zoom, &p);
            }
            _ => {}
        }
        self.label_editor(ui, d);
        self.cursor(ui, tool, space, panning);
        out
    }

    fn cursor(&self, ui: &Ui, tool: Tool, space: bool, panning: bool) {
        let hovered = self.pointer.is_some();
        let icon = if panning {
            CursorIcon::Grabbing
        } else if space && hovered {
            CursorIcon::Grab
        } else if let Gesture::Resize { handle, .. } = &self.gesture {
            resize_cursor(*handle)
        } else if matches!(self.gesture, Gesture::Move { .. }) {
            CursorIcon::Move
        } else if !hovered {
            return;
        } else if tool != Tool::Select {
            CursorIcon::Crosshair
        } else {
            match &self.hover {
                Some(Hit::Handle(_, h)) => resize_cursor(*h),
                Some(Hit::Shape(_)) => CursorIcon::Move,
                None => return,
            }
        };
        ui.ctx().set_cursor_icon(icon);
    }
}

fn resize_cursor(h: Handle) -> CursorIcon {
    match h {
        Handle::NW | Handle::SE => CursorIcon::ResizeNwSe,
        Handle::NE | Handle::SW => CursorIcon::ResizeNeSw,
        Handle::N | Handle::S => CursorIcon::ResizeVertical,
        Handle::E | Handle::W => CursorIcon::ResizeHorizontal,
    }
}
