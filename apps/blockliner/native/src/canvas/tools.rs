//! The tools and the pointer gestures they start.
//!
//! - Shape tools: drag corner to corner, or click for the default size.
//! - Line / Arrow, and their right-angle versions: drag from where the line
//!   starts to where it ends. An end dropped on a side's midpoint glues
//!   there; on a shape's body it glues dynamically (to whichever side faces
//!   the other end); elsewhere it stays free, on the grid.
//! - A selected right-angle line's middle handle drags its middle segment
//!   sideways; a double-click on it puts the route back to automatic.
//! - Select: click to select (Shift toggles); drag a shape or line to move
//!   the selection, a shape's handle to resize it, a selected line's end to
//!   re-glue or free it, and empty canvas to box-select.

use blockliner_core::geometry::{resize, Handle};
use blockliner_core::route::{drag_middle, middle_segment};
use blockliner_core::snap::{snap, snap_point};
use blockliner_core::{
    Connector, ConnectorKind, Diagram, End, Point, Rect, Route, Shape, ShapeKind,
};
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
    ElbowLine,
    ElbowArrow,
    Text,
}

impl Tool {
    /// In toolbar order (which the number keys follow).
    pub const ALL: [Tool; 8] = [
        Tool::Select,
        Tool::Rect,
        Tool::RoundRect,
        Tool::Line,
        Tool::Arrow,
        Tool::ElbowLine,
        Tool::ElbowArrow,
        Tool::Text,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Rect => "Rectangle",
            Tool::RoundRect => "Rounded Rectangle",
            Tool::Line => "Line",
            Tool::Arrow => "Arrow",
            Tool::ElbowLine => "Right-Angle Line",
            Tool::ElbowArrow => "Right-Angle Arrow",
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

    /// The connector a line tool makes.
    pub fn connector_kind(self) -> Option<ConnectorKind> {
        match self {
            Tool::Line | Tool::ElbowLine => Some(ConnectorKind::Line),
            Tool::Arrow | Tool::ElbowArrow => Some(ConnectorKind::Arrow),
            _ => None,
        }
    }

    /// How a line tool's connectors run.
    pub fn route(self) -> Route {
        match self {
            Tool::ElbowLine | Tool::ElbowArrow => Route::ELBOW,
            _ => Route::Straight,
        }
    }
}

/// What the pointer is over, with the Select tool.
#[derive(Clone, Debug, PartialEq)]
pub enum Hit {
    /// A handle of the (single) selected shape.
    Handle(String, Handle),
    /// An end of the (single) selected connector: `true` for its `to` end.
    Endpoint(usize, bool),
    /// The middle-segment handle of the (single) selected right-angle
    /// connector: `true` when that segment is vertical (it moves in x).
    Middle(usize, bool),
    Line(usize),
    Shape(String),
}

/// A primary-button drag in progress, in model coordinates.
#[derive(Clone, Debug, Default)]
pub enum Gesture {
    #[default]
    Idle,
    Create {
        kind: ShapeKind,
        start: Point,
        now: Point,
    },
    Connect {
        kind: ConnectorKind,
        route: Route,
        from: End,
        now: Point,
    },
    Move {
        start: Point,
        /// The selected shapes' rectangles when the drag began…
        shapes: Vec<(String, Rect)>,
        /// …and the selected connectors (their free ends move too).
        lines: Vec<(usize, Connector)>,
    },
    Resize {
        id: String,
        handle: Handle,
        start: Point,
        original: Rect,
    },
    Endpoint {
        index: usize,
        /// The `to` end, else the `from` end.
        to: bool,
        original: Connector,
        now: Point,
    },
    /// Dragging a right-angle connector's middle segment.
    Middle {
        index: usize,
        original: Connector,
        /// Its path and middle segment when the drag began.
        path: Vec<Point>,
        segment: usize,
    },
    Marquee {
        start: Point,
        now: Point,
        additive: bool,
    },
}

/// Per-frame input settings.
#[derive(Clone, Copy)]
pub struct Mods {
    /// Snap to the grid (Alt turns it off).
    pub snapping: bool,
    /// Add to / toggle the selection.
    pub shift: bool,
}

/// Hit distances, in model units (screen points over the zoom).
#[derive(Clone, Copy)]
struct Reach {
    handle: f32,
    line: f32,
    anchor: f32,
}

/// A selectable thing.
enum Item {
    Shape(String),
    Line(usize),
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
        let reach = Reach {
            handle: super::HANDLE_HIT / view.zoom,
            line: super::LINE_HIT / view.zoom,
            anchor: super::ANCHOR_HIT / view.zoom,
        };
        let hover = resp.hover_pos().map(to_model);
        let mut out = Output::default();

        self.hover = match (tool, hover) {
            (Tool::Select, Some(p)) => self.hit(d, p, reach),
            _ => None,
        };

        if resp.drag_started_by(PointerButton::Primary) {
            if let Some(start) = ui.input(|i| i.pointer.press_origin()).map(to_model) {
                self.commit_label(d);
                self.gesture = self.begin(d, tool, start, reach, mods);
            }
        }
        if resp.dragged_by(PointerButton::Primary) {
            if let Some(now) = resp.interact_pointer_pos().map(to_model) {
                self.update(d, now, reach, mods.snapping);
            }
        }
        if resp.drag_stopped_by(PointerButton::Primary) {
            out.tool_done = self.finish(d, reach, mods.snapping);
        } else if resp.clicked_by(PointerButton::Primary) {
            if let Some(at) = resp.interact_pointer_pos().map(to_model) {
                self.commit_label(d);
                out.tool_done = self.click(d, tool, at, reach, mods);
            }
        }
        if resp.double_clicked_by(PointerButton::Primary) && tool == Tool::Select {
            let at = resp.interact_pointer_pos().map(to_model);
            if let Some(Hit::Middle(i, _)) = at.and_then(|p| self.hit(d, p, reach)) {
                // Back to the automatic route.
                if let Some(c) = d.connectors.get_mut(i) {
                    c.route = Route::ELBOW;
                }
            } else if let Some(s) = at.and_then(|p| d.shape_at(p)).filter(|s| !s.external) {
                self.edit_label(s.id.clone(), s.label.clone(), false);
            }
        }
        // A right-click selects what's under it (or clears the selection on
        // empty canvas), so the context menu acts on that.
        if resp.secondary_clicked() && tool == Tool::Select {
            if let Some(at) = resp.interact_pointer_pos().map(to_model) {
                self.commit_label(d);
                let item = match d.connector_at(at, reach.line) {
                    Some(i) => Some(Item::Line(i)),
                    None => d.shape_at(at).map(|s| Item::Shape(s.id.clone())),
                };
                match item {
                    Some(Item::Line(i)) if self.lines.contains(&i) => {}
                    Some(Item::Shape(id)) if self.selection.contains(&id) => {}
                    Some(item) => self.select(item, false),
                    None => {
                        self.selection.clear();
                        self.lines.clear();
                    }
                }
            }
        }

        // Where a line end would glue, for the anchor hints.
        let aim = match &self.gesture {
            Gesture::Connect { now, .. } | Gesture::Endpoint { now, .. } => Some(*now),
            _ if tool.connector_kind().is_some() => hover,
            _ => None,
        };
        self.target = aim.and_then(|p| d.glue_at(p, reach.anchor));
        out
    }

    /// In order: an end of the single selected line, a handle of the single
    /// selected shape, a line, a shape.
    fn hit(&self, d: &Diagram, p: Point, reach: Reach) -> Option<Hit> {
        let near = |c: Point, r: f32| (c.x - p.x).abs() <= r && (c.y - p.y).abs() <= r;
        if let ([], [i]) = (self.selection.as_slice(), self.lines.as_slice()) {
            let c = d.connectors.get(*i);
            if let Some(path) = c.and_then(|c| d.connector_path(c)) {
                let (a, b) = (path[0], path[path.len() - 1]);
                if near(b, reach.handle) {
                    return Some(Hit::Endpoint(*i, true));
                }
                if near(a, reach.handle) {
                    return Some(Hit::Endpoint(*i, false));
                }
                let elbow = c.is_some_and(|c| c.route.is_elbow());
                if let (true, Some(at), Some(seg)) = (
                    elbow,
                    super::paint::middle_handle(&path),
                    middle_segment(&path),
                ) {
                    if near(at, reach.handle) {
                        return Some(Hit::Middle(*i, path[seg].x == path[seg + 1].x));
                    }
                }
            }
        }
        if let ([id], []) = (self.selection.as_slice(), self.lines.as_slice()) {
            if let Some(s) = d.shape(id) {
                let handle = Handle::ALL
                    .into_iter()
                    .find(|h| near(h.position(&s.rect), reach.handle));
                if let Some(h) = handle {
                    return Some(Hit::Handle(id.clone(), h));
                }
            }
        }
        if let Some(i) = d.connector_at(p, reach.line) {
            return Some(Hit::Line(i));
        }
        d.shape_at(p).map(|s| Hit::Shape(s.id.clone()))
    }

    /// Selects `item`: Shift toggles it; otherwise it replaces the selection.
    fn select(&mut self, item: Item, shift: bool) {
        if !shift {
            self.selection.clear();
            self.lines.clear();
        }
        match item {
            Item::Shape(id) => toggle(&mut self.selection, id),
            Item::Line(i) => toggle(&mut self.lines, i),
        }
    }

    /// Where a line end dropped at `p` goes: glued to an anchor or shape, or
    /// free on the grid.
    fn end_at(d: &Diagram, p: Point, reach: Reach, snapping: bool) -> End {
        d.glue_at(p, reach.anchor)
            .unwrap_or_else(|| End::Free(if snapping { snap_point(p, d.grid) } else { p }))
    }

    fn begin(
        &mut self,
        d: &Diagram,
        tool: Tool,
        start: Point,
        reach: Reach,
        mods: Mods,
    ) -> Gesture {
        if let Some(kind) = tool.shape_kind() {
            return Gesture::Create {
                kind,
                start,
                now: start,
            };
        }
        if let Some(kind) = tool.connector_kind() {
            return Gesture::Connect {
                kind,
                route: tool.route(),
                from: Self::end_at(d, start, reach, mods.snapping),
                now: start,
            };
        }
        let item = match self.hit(d, start, reach) {
            Some(Hit::Endpoint(index, to)) => {
                return Gesture::Endpoint {
                    index,
                    to,
                    original: d.connectors[index].clone(),
                    now: start,
                };
            }
            Some(Hit::Middle(index, _)) => {
                let original = d.connectors[index].clone();
                let path = d.connector_path(&original).unwrap_or_default();
                if let Some(segment) = middle_segment(&path) {
                    return Gesture::Middle {
                        index,
                        original,
                        path,
                        segment,
                    };
                }
                (!self.lines.contains(&index)).then_some(Item::Line(index))
            }
            Some(Hit::Handle(id, handle)) => {
                let original = d.shape(&id).map(|s| s.rect).unwrap_or_default();
                return Gesture::Resize {
                    id,
                    handle,
                    start,
                    original,
                };
            }
            None => {
                return Gesture::Marquee {
                    start,
                    now: start,
                    additive: mods.shift,
                };
            }
            Some(Hit::Line(i)) => (!self.lines.contains(&i)).then_some(Item::Line(i)),
            Some(Hit::Shape(id)) => (!self.selection.contains(&id)).then_some(Item::Shape(id)),
        };
        // Dragging something unselected selects it first (adding with Shift).
        if let Some(item) = item {
            self.select(item, mods.shift);
        }
        Gesture::Move {
            start,
            shapes: self
                .selection
                .iter()
                .filter_map(|id| d.shape(id).map(|s| (id.clone(), s.rect)))
                .collect(),
            lines: self
                .lines
                .iter()
                .filter_map(|&i| d.connectors.get(i).map(|c| (i, c.clone())))
                .collect(),
        }
    }

    fn update(&mut self, d: &mut Diagram, now: Point, reach: Reach, snapping: bool) {
        let grid = d.grid;
        match &mut self.gesture {
            Gesture::Idle => {}
            Gesture::Create { now: n, .. }
            | Gesture::Connect { now: n, .. }
            | Gesture::Marquee { now: n, .. } => *n = now,
            // The delta snaps, not each position, so things placed off the
            // grid (with Alt) keep their offset. Glued ends follow their
            // shapes; free ends of selected lines move with the rest.
            Gesture::Move {
                start,
                shapes,
                lines,
            } => {
                let (mut dx, mut dy) = (now.x - start.x, now.y - start.y);
                if snapping {
                    (dx, dy) = (snap(dx, grid), snap(dy, grid));
                }
                for (id, r) in shapes.iter() {
                    if let Some(s) = d.shape_mut(id) {
                        s.rect = r.translated(dx, dy);
                    }
                }
                let shift = |e: &End| match e {
                    End::Free(p) => End::Free(Point::new(p.x + dx, p.y + dy)),
                    glued => glued.clone(),
                };
                for (i, c) in lines.iter() {
                    if let Some(cur) = d.connectors.get_mut(*i) {
                        cur.from = shift(&c.from);
                        cur.to = shift(&c.to);
                        cur.route = translated(c.route, dx, dy);
                    }
                }
            }
            Gesture::Resize {
                id,
                handle,
                start,
                original,
            } => {
                let (dx, dy) = (now.x - start.x, now.y - start.y);
                let r = resize(*original, *handle, dx, dy, grid, snapping);
                if let Some(s) = d.shape_mut(id) {
                    s.rect = r;
                }
            }
            Gesture::Endpoint {
                index,
                to,
                original,
                now: n,
            } => {
                *n = now;
                let other = if *to { &original.from } else { &original.to };
                let mut end = Self::end_at(d, now, reach, snapping);
                // Both ends on one shape isn't a connection.
                if end.shape().is_some() && end.shape() == other.shape() {
                    end = End::Free(if snapping { snap_point(now, grid) } else { now });
                }
                if let Some(c) = d.connectors.get_mut(*index) {
                    *if *to { &mut c.to } else { &mut c.from } = end;
                }
            }
            // The middle segment follows the pointer across, on the grid.
            Gesture::Middle {
                index,
                path,
                segment,
                ..
            } => {
                let at = if snapping { snap_point(now, grid) } else { now };
                if let (Some(mid), Some(c)) = (
                    drag_middle(path, *segment, at),
                    d.connectors.get_mut(*index),
                ) {
                    c.route = Route::Elbow { mid: Some(mid) };
                }
            }
        }
    }

    /// The drag ended. `true` if it made a shape or line (the tool is done).
    fn finish(&mut self, d: &mut Diagram, reach: Reach, snapping: bool) -> bool {
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
            Gesture::Connect {
                kind,
                route,
                from,
                now,
            } => {
                let to = Self::end_at(d, now, reach, snapping);
                let same_shape = from.shape().is_some() && from.shape() == to.shape();
                if same_shape || from == to {
                    return false;
                }
                d.connectors
                    .push(Connector::new(kind, from, to).with_route(route));
                self.selection.clear();
                self.lines = vec![d.connectors.len() - 1];
                true
            }
            Gesture::Marquee {
                start,
                now,
                additive,
            } => {
                if !additive {
                    self.selection.clear();
                    self.lines.clear();
                }
                let area = Rect::from_corners(start, now);
                for s in d.shapes_in(area) {
                    if !self.selection.contains(&s.id) {
                        self.selection.push(s.id.clone());
                    }
                }
                for i in d.connectors_in(area) {
                    if !self.lines.contains(&i) {
                        self.lines.push(i);
                    }
                }
                false
            }
            Gesture::Idle
            | Gesture::Move { .. }
            | Gesture::Resize { .. }
            | Gesture::Endpoint { .. }
            | Gesture::Middle { .. } => false,
        }
    }

    /// A click without a drag. `true` if it created a shape.
    fn click(&mut self, d: &mut Diagram, tool: Tool, at: Point, reach: Reach, mods: Mods) -> bool {
        if let Some(kind) = tool.shape_kind() {
            self.create_default(d, kind, at, mods.snapping);
            return true;
        }
        if tool != Tool::Select {
            return false;
        }
        let item = match d.connector_at(at, reach.line) {
            Some(i) => Some(Item::Line(i)),
            None => d.shape_at(at).map(|s| Item::Shape(s.id.clone())),
        };
        match item {
            Some(item) => self.select(item, mods.shift),
            None if mods.shift => {}
            None => {
                self.selection.clear();
                self.lines.clear();
            }
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
        self.lines.clear();
        self.edit_label(id, String::new(), true);
    }

    /// Esc: cancel a label edit or a drag, else clear the selection. egui
    /// drops text focus on Esc before the label editor sees the key, so the
    /// edit is cancelled here.
    pub fn escape(&mut self, d: &mut Diagram) {
        if !self.cancel(d) {
            self.selection.clear();
            self.lines.clear();
        }
    }

    /// Cancels a label edit (keeping the old label), else a drag (putting
    /// moved, resized or re-glued things back). `false` if there was
    /// neither.
    pub fn cancel(&mut self, d: &mut Diagram) -> bool {
        if self.label.take().is_some() {
            return true;
        }
        match std::mem::take(&mut self.gesture) {
            Gesture::Move { shapes, lines, .. } => {
                for (id, r) in shapes {
                    if let Some(s) = d.shape_mut(&id) {
                        s.rect = r;
                    }
                }
                for (i, c) in lines {
                    if let Some(cur) = d.connectors.get_mut(i) {
                        *cur = c;
                    }
                }
            }
            Gesture::Resize { id, original, .. } => {
                if let Some(s) = d.shape_mut(&id) {
                    s.rect = original;
                }
            }
            Gesture::Endpoint {
                index, original, ..
            }
            | Gesture::Middle {
                index, original, ..
            } => {
                if let Some(c) = d.connectors.get_mut(index) {
                    *c = original;
                }
            }
            Gesture::Idle => return false,
            Gesture::Create { .. } | Gesture::Connect { .. } | Gesture::Marquee { .. } => {}
        }
        true
    }

    /// A drag or label edit is in progress, so the diagram is mid-change
    /// (the history waits for it to finish).
    pub fn is_busy(&self) -> bool {
        self.label.is_some() || !matches!(self.gesture, Gesture::Idle)
    }

    /// Moves the selected shapes, and the free ends of the selected lines,
    /// by (`dx`, `dy`).
    pub fn nudge(&mut self, d: &mut Diagram, dx: f32, dy: f32) {
        for id in &self.selection {
            if let Some(s) = d.shape_mut(id) {
                s.rect = s.rect.translated(dx, dy);
            }
        }
        for &i in &self.lines {
            if let Some(c) = d.connectors.get_mut(i) {
                for end in [&mut c.from, &mut c.to] {
                    if let End::Free(p) = end {
                        *p = Point::new(p.x + dx, p.y + dy);
                    }
                }
                c.route = translated(c.route, dx, dy);
            }
        }
    }

    /// Makes the selected connectors right-angle (`elbow`) or straight.
    /// One already right-angle keeps its pinned middle. `false` if none
    /// changed.
    pub fn set_route(&mut self, d: &mut Diagram, elbow: bool) -> bool {
        let mut changed = false;
        for &i in &self.lines {
            if let Some(c) = d.connectors.get_mut(i) {
                if c.route.is_elbow() != elbow {
                    c.route = if elbow { Route::ELBOW } else { Route::Straight };
                    changed = true;
                }
            }
        }
        changed
    }

    /// The selected connectors' routes: (any straight, any right-angle).
    pub fn selected_routes(&self, d: &Diagram) -> (bool, bool) {
        let routes = self.lines.iter().filter_map(|&i| d.connectors.get(i));
        routes.fold((false, false), |(s, e), c| {
            (s || !c.route.is_elbow(), e || c.route.is_elbow())
        })
    }

    pub fn select_all(&mut self, d: &Diagram) {
        self.selection = d.shapes.iter().map(|s| s.id.clone()).collect();
        self.lines = (0..d.connectors.len()).collect();
    }

    /// Deletes the selected lines and shapes (and the connectors glued to
    /// those shapes). Shapes declared in kept Mermaid statements stay: their
    /// declaration isn't BlockLiner's to remove. Returns how many things
    /// went.
    pub fn delete_selection(&mut self, d: &mut Diagram) -> usize {
        let lines = d.remove_connectors(&std::mem::take(&mut self.lines));
        let ids: Vec<String> = self
            .selection
            .iter()
            .filter(|id| d.shape(id).is_some_and(|s| !s.external))
            .cloned()
            .collect();
        let shapes = d.remove_shapes(&ids);
        self.selection.retain(|id| d.shape(id).is_some());
        lines + shapes
    }
}

/// A route moved with the rest of a selection: a pinned middle moves too.
fn translated(r: Route, dx: f32, dy: f32) -> Route {
    match r {
        Route::Elbow { mid: Some(m) } => Route::Elbow {
            mid: Some(m.translated(dx, dy)),
        },
        r => r,
    }
}

/// Adds `x` to `list`, or removes it if it's there.
fn toggle<T: PartialEq>(list: &mut Vec<T>, x: T) {
    match list.iter().position(|y| *y == x) {
        Some(i) => {
            list.remove(i);
        }
        None => list.push(x),
    }
}
