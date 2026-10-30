//! The tools and the pointer gestures they start: create a shape (drag
//! corner to corner, or click for the default size), and with Select: click
//! to select, drag a shape to move the selection, drag a handle to resize,
//! drag on empty canvas for a box selection. Line and Arrow come in plan
//! phase 3.

use blockliner_core::geometry::{resize, Handle};
use blockliner_core::snap::{snap, snap_point};
use blockliner_core::{Diagram, Point, Rect, Shape, ShapeKind};
use egui::{PointerButton, Response, Ui};

use super::{pt, Canvas, Output};

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

    /// The shape a shape tool makes.
    pub fn shape_kind(self) -> Option<ShapeKind> {
        match self {
            Tool::Rect => Some(ShapeKind::Rect),
            Tool::RoundRect => Some(ShapeKind::RoundRect),
            Tool::Text => Some(ShapeKind::Text),
            _ => None,
        }
    }
}

/// What the pointer is over, with the Select tool.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// A handle of the (single) selected shape.
    Handle(String, Handle),
    Shape(String),
}

/// A primary-button drag in progress. `now` is the latest pointer position,
/// all in model coordinates.
#[derive(Clone, Debug, Default)]
pub enum Gesture {
    #[default]
    Idle,
    Create {
        kind: ShapeKind,
        start: Point,
        now: Point,
    },
    Move {
        start: Point,
        /// The selection's rectangles when the drag began.
        originals: Vec<(String, Rect)>,
    },
    Resize {
        id: String,
        handle: Handle,
        start: Point,
        original: Rect,
    },
    Marquee {
        start: Point,
        now: Point,
        additive: bool,
    },
}

/// Modifier state for one frame of input.
#[derive(Clone, Copy)]
pub struct Mods {
    /// Snap to the grid (Alt turns it off).
    pub snapping: bool,
    /// Add to / toggle the selection.
    pub shift: bool,
}

impl Canvas {
    /// The primary-button input for `tool`. Space+drag and the middle
    /// button pan instead, before this runs.
    pub(super) fn input(
        &mut self,
        ui: &Ui,
        resp: &Response,
        d: &mut Diagram,
        tool: Tool,
        mods: Mods,
    ) -> Output {
        let (view, origin) = (self.view, self.rect.min);
        let to_model = |s: egui::Pos2| pt(view.to_model(origin, s));
        let tolerance = super::HANDLE_HIT / view.zoom;
        let mut out = Output::default();

        self.hover = match (tool, resp.hover_pos()) {
            (Tool::Select, Some(s)) => self.hit(d, to_model(s), tolerance),
            _ => None,
        };

        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(start) = ui.input(|i| i.pointer.press_origin()).map(to_model) {
                self.commit_label(d);
                self.gesture = self.begin(d, tool, start, tolerance, mods.shift);
            }
        }
        if resp.dragged_by(PointerButton::Primary) {
            if let Some(now) = resp.interact_pointer_pos().map(to_model) {
                self.update(d, now, mods.snapping);
            }
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            out.tool_done = self.finish(d, mods.snapping);
        } else if resp.clicked_by(PointerButton::Primary) {
            if let Some(at) = resp.interact_pointer_pos().map(to_model) {
                self.commit_label(d);
                out.tool_done = self.click(d, tool, at, mods);
            }
        }
        if resp.double_clicked_by(PointerButton::Primary) && tool == Tool::Select {
            let at = resp.interact_pointer_pos().map(to_model);
            if let Some(s) = at.and_then(|p| d.shape_at(p)).filter(|s| !s.external) {
                self.edit_label(s.id.clone(), s.label.clone(), false);
            }
        }
        out
    }

    /// A handle of the single selected shape, else the topmost shape.
    pub(super) fn hit(&self, d: &Diagram, p: Point, tolerance: f32) -> Option<Hit> {
        if let [id] = self.selection.as_slice() {
            if let Some(s) = d.shape(id) {
                let near = |h: &Handle| {
                    let c = h.position(&s.rect);
                    (c.x - p.x).abs() <= tolerance && (c.y - p.y).abs() <= tolerance
                };
                if let Some(h) = Handle::ALL.into_iter().find(near) {
                    return Some(Hit::Handle(id.clone(), h));
                }
            }
        }
        d.shape_at(p).map(|s| Hit::Shape(s.id.clone()))
    }

    fn begin(
        &mut self,
        d: &Diagram,
        tool: Tool,
        start: Point,
        tolerance: f32,
        shift: bool,
    ) -> Gesture {
        if let Some(kind) = tool.shape_kind() {
            return Gesture::Create {
                kind,
                start,
                now: start,
            };
        }
        if tool != Tool::Select {
            return Gesture::Idle;
        }
        match self.hit(d, start, tolerance) {
            Some(Hit::Handle(id, handle)) => {
                let original = d.shape(&id).map(|s| s.rect).unwrap_or_default();
                Gesture::Resize {
                    id,
                    handle,
                    start,
                    original,
                }
            }
            Some(Hit::Shape(id)) => {
                if !self.selection.contains(&id) {
                    if !shift {
                        self.selection.clear();
                    }
                    self.selection.push(id);
                }
                let originals = self
                    .selection
                    .iter()
                    .filter_map(|id| d.shape(id).map(|s| (id.clone(), s.rect)))
                    .collect();
                Gesture::Move { start, originals }
            }
            None => Gesture::Marquee {
                start,
                now: start,
                additive: shift,
            },
        }
    }

    fn update(&mut self, d: &mut Diagram, now: Point, snapping: bool) {
        let grid = d.grid;
        match &mut self.gesture {
            Gesture::Idle => {}
            Gesture::Create { now: n, .. } | Gesture::Marquee { now: n, .. } => *n = now,
            // The delta snaps, not each position, so shapes placed off the
            // grid (with Alt) keep their offset.
            Gesture::Move { start, originals } => {
                let (mut dx, mut dy) = (now.x - start.x, now.y - start.y);
                if snapping {
                    (dx, dy) = (snap(dx, grid), snap(dy, grid));
                }
                for (id, r) in originals.iter() {
                    if let Some(s) = d.shape_mut(id) {
                        s.rect = r.translated(dx, dy);
                    }
                }
            }
            Gesture::Resize {
                id,
                handle,
                start,
                original,
            } => {
                let r = resize(
                    *original,
                    *handle,
                    now.x - start.x,
                    now.y - start.y,
                    grid,
                    snapping,
                );
                if let Some(s) = d.shape_mut(id) {
                    s.rect = r;
                }
            }
        }
    }

    /// The drag ended. `true` if it created a shape (the tool is done).
    fn finish(&mut self, d: &mut Diagram, snapping: bool) -> bool {
        let grid = d.grid;
        match std::mem::take(&mut self.gesture) {
            Gesture::Create { kind, start, now } => {
                let r = if snapping {
                    Rect::from_corners(snap_point(start, grid), snap_point(now, grid))
                } else {
                    Rect::from_corners(start, now)
                };
                // Smaller than a cell: treat it as a click.
                if r.w < grid || r.h < grid {
                    self.create_default(d, kind, start, snapping);
                } else {
                    self.create(d, kind, r);
                }
                true
            }
            Gesture::Marquee {
                start,
                now,
                additive,
            } => {
                if !additive {
                    self.selection.clear();
                }
                let area = Rect::from_corners(start, now);
                for s in d.shapes_in(area) {
                    if !self.selection.contains(&s.id) {
                        self.selection.push(s.id.clone());
                    }
                }
                false
            }
            Gesture::Idle | Gesture::Move { .. } | Gesture::Resize { .. } => false,
        }
    }

    /// A click without a drag. `true` if it created a shape.
    fn click(&mut self, d: &mut Diagram, tool: Tool, at: Point, mods: Mods) -> bool {
        if let Some(kind) = tool.shape_kind() {
            self.create_default(d, kind, at, mods.snapping);
            return true;
        }
        if tool != Tool::Select {
            return false;
        }
        match d.shape_at(at).map(|s| s.id.clone()) {
            Some(id) if mods.shift => {
                if let Some(i) = self.selection.iter().position(|s| *s == id) {
                    self.selection.remove(i);
                } else {
                    self.selection.push(id);
                }
            }
            Some(id) => self.selection = vec![id],
            None if mods.shift => {}
            None => self.selection.clear(),
        }
        false
    }

    /// A default-size shape centred on `at`.
    fn create_default(&mut self, d: &mut Diagram, kind: ShapeKind, at: Point, snapping: bool) {
        let (w, h) = Shape::default_size(&kind, d.grid);
        let mut corner = Point::new(at.x - w / 2.0, at.y - h / 2.0);
        if snapping {
            corner = snap_point(corner, d.grid);
        }
        self.create(d, kind, Rect::new(corner.x, corner.y, w, h));
    }

    /// Adds the shape, selects it and starts editing its label.
    fn create(&mut self, d: &mut Diagram, kind: ShapeKind, r: Rect) {
        let id = d.add_shape(kind, r, "");
        self.selection = vec![id.clone()];
        self.edit_label(id, String::new(), true);
    }

    /// Esc: cancel a label edit, else a drag (putting moved or resized
    /// shapes back), else clear the selection. egui drops text focus on Esc
    /// before the label editor sees the key, so the edit is cancelled here.
    pub fn escape(&mut self, d: &mut Diagram) {
        if self.label.take().is_some() {
            return;
        }
        match std::mem::take(&mut self.gesture) {
            Gesture::Move { originals, .. } => {
                for (id, r) in originals {
                    if let Some(s) = d.shape_mut(&id) {
                        s.rect = r;
                    }
                }
            }
            Gesture::Resize { id, original, .. } => {
                if let Some(s) = d.shape_mut(&id) {
                    s.rect = original;
                }
            }
            Gesture::Idle => self.selection.clear(),
            Gesture::Create { .. } | Gesture::Marquee { .. } => {}
        }
    }

    /// Deletes the selected shapes (and connectors glued to them). Shapes
    /// declared in kept Mermaid statements stay: their declaration isn't
    /// BlockLiner's to remove. Returns how many went.
    pub fn delete_selection(&mut self, d: &mut Diagram) -> usize {
        let ids: Vec<String> = self
            .selection
            .iter()
            .filter(|id| d.shape(id).is_some_and(|s| !s.external))
            .cloned()
            .collect();
        let n = d.remove_shapes(&ids);
        self.selection.retain(|id| d.shape(id).is_some());
        n
    }
}
