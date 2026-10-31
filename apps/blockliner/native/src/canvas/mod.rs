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
//! | Select: double-click a line or its label| Edits its edge label (Mermaid edges only) |
//! | Line / Arrow: drag                      | Draws a connector (ends glue to shapes)  |
//! | Select: drag a selected line's end      | Re-glues or frees that end               |
//! | Select: drag a right-angle line's middle| Moves its middle segment (double-click: automatic again) |
//! | Right-click                             | Selects what's under it; the context menu |
//! | Copy / Cut / Paste (egui's events)      | Handled by the app (`App::clipboard_events`) |
//! | Alt while dragging                      | No snapping                              |

pub mod grid;
mod label;
pub mod paint;
mod tools;
pub mod view;

use blockliner_core::geometry::Handle;
use blockliner_core::snap::snap_point;
use blockliner_core::style::{common, preset_index, Common, Style, PRESETS};
use blockliner_core::{Connector, Diagram, End, Point, Rect as ModelRect};
use egui::{CursorIcon, Key, PointerButton, Pos2, Rect, Sense, Ui, Vec2};
use marina_egui::theme;

pub use tools::Tool;
use tools::{Gesture, Hit, Mods};
use view::View;

/// How close (in screen points) the pointer must be to grab a handle…
const HANDLE_HIT: f32 = 6.0;
/// …to pick a line…
const LINE_HIT: f32 = 5.0;
/// …and for a line end to glue to an anchor rather than the shape's body.
const ANCHOR_HIT: f32 = 8.0;

fn pt(p: Pos2) -> Point {
    Point::new(p.x, p.y)
}

fn pos(x: f32, y: f32) -> Pos2 {
    egui::pos2(x, y)
}

/// What a frame of the canvas reports back.
#[derive(Default)]
pub struct Output {
    /// A shape or line tool made its shape or line; the app goes back to
    /// Select unless the tool was picked with Shift.
    pub tool_done: bool,
    /// Picked in the right-click menu.
    pub menu: Option<MenuAction>,
}

/// The canvas's right-click menu. Each is also a command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    EditLabel,
    Cut,
    Copy,
    Paste,
    RouteElbow,
    RouteStraight,
    Fill(Swatch),
    Stroke(Swatch),
    ResetStyle,
    Delete,
    SelectAll,
}

/// What [`Canvas::selected_style`] reports.
#[derive(Clone, Debug, PartialEq)]
pub struct SelectionStyle {
    pub fill: Common,
    pub stroke: Common,
    /// Selected shapes that can take colours…
    pub shapes: usize,
    /// …selected lines (a stroke only)…
    pub lines: usize,
    /// …and selected shapes declared in kept statements (which can't).
    pub external: Vec<String>,
}

/// A colour choice for the selected shapes' fill or stroke.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Swatch {
    /// Back to the theme's colour.
    Default,
    /// One of `blockliner_core::style::PRESETS`.
    Preset(usize),
    /// Ask for a hex value.
    Custom,
}

pub struct Canvas {
    pub view: View,
    pub show_grid: bool,
    /// The pointer in model coordinates, while it is over the canvas.
    pub pointer: Option<Pos2>,
    /// Selected shape ids, in the order they were selected.
    pub selection: Vec<String>,
    /// Selected connectors, by index.
    pub lines: Vec<usize>,
    gesture: Gesture,
    hover: Option<Hit>,
    /// Where a line end being drawn or dragged would glue (anchor hints).
    target: Option<End>,
    label: Option<label::LabelEdit>,
    /// Each edge label's patch, as of this frame (see `edge_labels`).
    labels: Vec<(usize, ModelRect)>,
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
            lines: Vec::new(),
            gesture: Gesture::Idle,
            hover: None,
            target: None,
            label: None,
            labels: Vec::new(),
            rect: Rect::ZERO,
        }
    }

    /// Whether a selected shape can take a fill: one BlockLiner declares
    /// (an external shape's `style` line isn't its to write).
    pub fn fillable(&self, d: &Diagram) -> bool {
        self.selection
            .iter()
            .any(|id| d.shape(id).is_some_and(|s| !s.external))
    }

    /// Whether anything selected can take a stroke: such a shape, or a
    /// line.
    pub fn stylable(&self, d: &Diagram) -> bool {
        self.fillable(d) || !self.lines.is_empty()
    }

    /// What the selection's fills (shapes) and strokes (shapes and lines)
    /// have in common, and how many shapes, lines and kept-statement shapes
    /// are selected.
    pub fn selected_style(&self, d: &Diagram) -> SelectionStyle {
        let shapes: Vec<&blockliner_core::Shape> =
            self.selection.iter().filter_map(|id| d.shape(id)).collect();
        let (own, external): (Vec<_>, Vec<_>) = shapes.into_iter().partition(|s| !s.external);
        let lines = self.lines.iter().filter_map(|&i| d.connectors.get(i));
        SelectionStyle {
            fill: common(own.iter().map(|s| s.style.fill.as_ref())),
            stroke: common(
                own.iter()
                    .map(|s| s.style.stroke.as_ref())
                    .chain(lines.map(|c| c.style.stroke.as_ref())),
            ),
            shapes: own.len(),
            lines: self.lines.len(),
            external: external.iter().map(|s| s.id.clone()).collect(),
        }
    }

    /// Changes the style of each selected shape BlockLiner declares, and
    /// with `lines` each selected line's too.
    pub fn style_selection(&self, d: &mut Diagram, lines: bool, f: impl Fn(&mut Style)) {
        for id in &self.selection {
            if let Some(s) = d.shape_mut(id).filter(|s| !s.external) {
                f(&mut s.style);
            }
        }
        if lines {
            for &i in &self.lines {
                if let Some(c) = d.connectors.get_mut(i) {
                    f(&mut c.style);
                }
            }
        }
    }

    /// The canvas's top-left corner on screen (model origin at 100 % and no
    /// pan), as of the last frame.
    #[cfg(test)]
    pub fn origin(&self) -> Pos2 {
        self.rect.min
    }

    /// Forgets the selection, any gesture or label edit and the hover, for a
    /// diagram that was replaced (opened, or reloaded from disk). `view`:
    /// also back to 100 % at the origin.
    pub fn clear(&mut self, view: bool) {
        self.selection.clear();
        self.lines.clear();
        self.gesture = Gesture::Idle;
        self.hover = None;
        self.target = None;
        self.label = None;
        if view {
            self.view = View::default();
        }
    }

    pub fn has_selection(&self) -> bool {
        !self.selection.is_empty() || !self.lines.is_empty()
    }

    /// Why the shape under the pointer, else the one selected shape, isn't
    /// fully editable (see [`paint::limitation`]).
    pub fn limitation(&self, d: &Diagram) -> Option<(&'static str, String)> {
        let hovered = match &self.hover {
            Some(Hit::Shape(id) | Hit::Handle(id, _)) => Some(id),
            _ => None,
        };
        let id = hovered.or(match (self.selection.as_slice(), self.lines.as_slice()) {
            ([id], []) => Some(id),
            _ => None,
        })?;
        paint::limitation(d.shape(id)?)
    }

    pub fn selected_count(&self) -> usize {
        self.selection.len() + self.lines.len()
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

    /// The part of the diagram on screen, in model coordinates (as of last
    /// frame).
    pub fn visible(&self) -> ModelRect {
        let size = self.rect.size() / self.view.zoom;
        ModelRect::new(self.view.pan.x, self.view.pan.y, size.x, size.y)
    }

    /// Selects exactly these shapes and lines (a paste's).
    pub fn select_only(&mut self, shapes: Vec<String>, lines: Vec<usize>) {
        self.selection = shapes;
        self.lines = lines;
    }

    /// Fills the rest of `ui`. `keys`: whether Space may start a pan (no
    /// dialog or text field has the keyboard).
    pub fn show(&mut self, ui: &mut Ui, d: &mut Diagram, tool: Tool, keys: bool) -> Output {
        let p = theme::palette(ui.ctx());
        let (rect, resp) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
        self.rect = rect;
        self.selection.retain(|id| d.shape(id).is_some());
        self.lines.retain(|&i| i < d.connectors.len());

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
            self.target = None;
        }

        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, p.bg_base);
        if self.show_grid {
            let minor = p.border.gamma_multiply(0.45);
            grid::paint(&painter, rect, self.view, d.grid, minor, p.border);
        }
        let overlays = paint::Overlays {
            selection: &self.selection,
            lines: &self.lines,
            hover: self.hover.as_ref(),
            target: self.target.as_ref(),
            editing: self.editing_shape(),
            editing_line: self.editing_line(),
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
            // The line being drawn, to where it would end if let go now.
            Gesture::Connect {
                kind,
                route,
                from,
                now,
            } => {
                let to = self.target.clone().unwrap_or_else(|| {
                    End::Free(if alt { *now } else { snap_point(*now, d.grid) })
                });
                let line = Connector::new(*kind, from.clone(), to).with_route(*route);
                if let Some(path) = d.connector_path(&line) {
                    let points: Vec<Pos2> = path
                        .iter()
                        .map(|q| self.view.to_screen(rect.min, pos(q.x, q.y)))
                        .collect();
                    paint::line_preview(&painter, &points, *kind, self.view.zoom, &p);
                }
            }
            _ => {}
        }
        self.label_editor(ui, d);
        self.cursor(ui, tool, space, panning);
        out.menu = self.context_menu(&resp, d);
        out
    }

    /// The right-click menu, on what the right-click selected.
    fn context_menu(&self, resp: &egui::Response, d: &Diagram) -> Option<MenuAction> {
        let (straight, elbow) = self.selected_routes(d);
        let (lines, any) = (!self.lines.is_empty(), self.has_selection());
        let (stylable, fillable) = (self.stylable(d), self.fillable(d));
        let style = self.selected_style(d);
        let labelled = self.can_edit_label(d);
        let mut picked = None;
        resp.context_menu(|ui| {
            let mut item = |ui: &mut Ui, text: &str, keys: &str, on: bool, act: MenuAction| {
                if menu_item(ui, text, keys, on) {
                    picked = Some(act);
                    ui.close();
                }
            };
            // A single shape or line; off for a free line.
            if self.selection.len() + self.lines.len() == 1 {
                item(ui, "Edit Label", "Enter", labelled, MenuAction::EditLabel);
                ui.separator();
            }
            if any {
                item(ui, "Cut", "Ctrl+X", true, MenuAction::Cut);
                item(ui, "Copy", "Ctrl+C", true, MenuAction::Copy);
            }
            item(ui, "Paste", "Ctrl+V", true, MenuAction::Paste);
            ui.separator();
            if stylable {
                for (title, stroke) in [("Fill", false), ("Stroke", true)] {
                    // Lines take only a stroke.
                    if !stroke && !fillable {
                        continue;
                    }
                    ui.menu_button(title, |ui| {
                        let current = if stroke { &style.stroke } else { &style.fill };
                        if let Some(s) = swatches(ui, stroke, current) {
                            picked = Some(if stroke {
                                MenuAction::Stroke(s)
                            } else {
                                MenuAction::Fill(s)
                            });
                            ui.close();
                        }
                    });
                }
                if ui.button("Reset Style").clicked() {
                    picked = Some(MenuAction::ResetStyle);
                    ui.close();
                }
                ui.separator();
            }
            let mut item = |ui: &mut Ui, text: &str, keys: &str, on: bool, act: MenuAction| {
                if menu_item(ui, text, keys, on) {
                    picked = Some(act);
                    ui.close();
                }
            };
            if lines {
                item(ui, "Right-Angle", "", straight, MenuAction::RouteElbow);
                item(ui, "Straight", "", elbow, MenuAction::RouteStraight);
                ui.separator();
            }
            item(ui, "Delete", "Delete", any, MenuAction::Delete);
            item(ui, "Select All", "Ctrl+A", true, MenuAction::SelectAll);
        });
        picked
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
        } else if let Gesture::Middle { path, segment, .. } = &self.gesture {
            if path[*segment].x == path[*segment + 1].x {
                CursorIcon::ResizeHorizontal
            } else {
                CursorIcon::ResizeVertical
            }
        } else if matches!(self.gesture, Gesture::Endpoint { .. }) {
            CursorIcon::Crosshair
        } else if !hovered {
            return;
        } else if tool != Tool::Select {
            CursorIcon::Crosshair
        } else {
            match &self.hover {
                Some(Hit::Handle(_, h)) => resize_cursor(*h),
                Some(Hit::Endpoint(..)) => CursorIcon::Crosshair,
                Some(Hit::Middle(_, true)) => CursorIcon::ResizeHorizontal,
                Some(Hit::Middle(_, false)) => CursorIcon::ResizeVertical,
                Some(Hit::Shape(_) | Hit::Line(_)) => CursorIcon::Move,
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

/// A menu row with its shortcut on the right; `true` when clicked.
fn menu_item(ui: &mut Ui, text: &str, keys: &str, on: bool) -> bool {
    let mut button = egui::Button::new(text);
    if !keys.is_empty() {
        button = button.shortcut_text(keys);
    }
    ui.add_enabled(on, button).clicked()
}

/// A Fill or Stroke submenu: Default, the presets with their swatches, and
/// Custom…, the current value (`current`) ringed. Returns the one clicked.
fn swatches(ui: &mut Ui, stroke: bool, current: &Common) -> Option<Swatch> {
    let p = theme::palette(ui.ctx());
    let ringed = current_swatch(current, stroke);
    let mut picked = None;
    // Each row: its square, its name, what it picks.
    let mut row = |ui: &mut Ui, look: paint::SwatchLook, text: &str, s: Swatch| {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(150.0, 24.0), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, text));
        if resp.hovered() {
            ui.painter().rect_filled(rect, 3.0, p.bg_item_hover);
        }
        let square = Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            egui::vec2(14.0, 14.0),
        );
        paint::swatch(ui.painter(), square, look, ringed == Some(s), &p);
        ui.painter().text(
            egui::pos2(square.right() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            p.text_primary,
        );
        if resp.clicked() {
            picked = Some(s);
        }
    };
    row(ui, paint::SwatchLook::Default, "Default", Swatch::Default);
    for (i, preset) in PRESETS.iter().enumerate() {
        let look = paint::preset_look(i, stroke, &p);
        row(ui, look, preset.name, Swatch::Preset(i));
    }
    ui.separator();
    row(ui, paint::SwatchLook::Default, "Custom…", Swatch::Custom);
    picked
}

/// The swatch that shows `current`: Default, a preset, or none (a custom
/// colour, or Mixed).
pub fn current_swatch(current: &Common, stroke: bool) -> Option<Swatch> {
    match current {
        Common::Same(None) => Some(Swatch::Default),
        Common::Same(Some(c)) => preset_index(c, stroke).map(Swatch::Preset),
        Common::None | Common::Mixed => None,
    }
}
