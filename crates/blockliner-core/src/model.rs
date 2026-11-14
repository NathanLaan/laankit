//! The diagram model. Coordinates are diagram pixels at 100 % zoom, with y
//! pointing down; the canvas maps them to the screen.

use crate::ids;
use crate::style::{Colour, Style, NOTE_FILL, NOTE_STROKE};
use std::borrow::Cow;

/// The default grid spacing, in diagram pixels.
pub const DEFAULT_GRID: f32 = 20.0;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle: top-left corner plus size.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

/// The flowchart direction from the header (`flowchart LR`). It only steers
/// Mermaid's own layout; BlockLiner keeps it so the file renders the same
/// elsewhere.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Direction {
    TB,
    TD,
    BT,
    #[default]
    LR,
    RL,
}

impl Direction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::TB => "TB",
            Self::TD => "TD",
            Self::BT => "BT",
            Self::LR => "LR",
            Self::RL => "RL",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "TB" => Self::TB,
            "TD" => Self::TD,
            "BT" => Self::BT,
            "LR" => Self::LR,
            "RL" => Self::RL,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    /// `id["label"]`
    Rect,
    /// `id("label")`
    RoundRect,
    /// A borderless label: `id["label"]:::bl_text`.
    Text,
    /// A sticky note with a folded corner: `id["label"]:::bl_note`.
    Note,
    /// A decision: `id{"label"}`.
    Diamond,
    /// `id{{"label"}}`
    Hexagon,
    /// A cylinder: `id[("label")]`.
    Database,
    /// `id[["label"]]`
    Subroutine,
    /// A pill: `id(["label"])`.
    Stadium,
    /// `id(("label"))`, an ellipse when its box isn't square.
    Circle,
    /// A circle inside a circle: `id((("label")))`.
    DoubleCircle,
    /// Leaning right: `id[/"label"/]`.
    Parallelogram,
    /// Leaning left: `id[\"label"\]`.
    ReverseParallelogram,
    /// Wider at the bottom: `id[/"label"\]`.
    Trapezoid,
    /// Wider at the top: `id[\"label"/]`.
    InvertedTrapezoid,
    /// A flag notched on the left: `id>"label"]`.
    Asymmetric,
    /// Any other pair of Mermaid brackets, kept as written so it is written
    /// back unchanged. Drawn as a rectangle.
    Other { open: String, close: String },
}

impl ShapeKind {
    /// The shapes behind the toolbar's More Shapes button.
    pub const MORE: [ShapeKind; 12] = [
        ShapeKind::Diamond,
        ShapeKind::Hexagon,
        ShapeKind::Database,
        ShapeKind::Subroutine,
        ShapeKind::Stadium,
        ShapeKind::Circle,
        ShapeKind::DoubleCircle,
        ShapeKind::Parallelogram,
        ShapeKind::ReverseParallelogram,
        ShapeKind::Trapezoid,
        ShapeKind::InvertedTrapezoid,
        ShapeKind::Asymmetric,
    ];

    /// What a shape can be changed into (Change Shape), in toolbar order:
    /// the boxes, the More Shapes, then text and notes.
    pub const CHANGEABLE: [ShapeKind; 16] = [
        ShapeKind::Rect,
        ShapeKind::RoundRect,
        ShapeKind::Diamond,
        ShapeKind::Hexagon,
        ShapeKind::Database,
        ShapeKind::Subroutine,
        ShapeKind::Stadium,
        ShapeKind::Circle,
        ShapeKind::DoubleCircle,
        ShapeKind::Parallelogram,
        ShapeKind::ReverseParallelogram,
        ShapeKind::Trapezoid,
        ShapeKind::InvertedTrapezoid,
        ShapeKind::Asymmetric,
        ShapeKind::Text,
        ShapeKind::Note,
    ];

    /// Its name in the UI: "Rectangle", "Double Circle", …
    pub fn name(&self) -> &'static str {
        match self {
            ShapeKind::Rect => "Rectangle",
            ShapeKind::RoundRect => "Rounded Rectangle",
            ShapeKind::Text => "Text",
            ShapeKind::Note => "Note",
            ShapeKind::Diamond => "Diamond",
            ShapeKind::Hexagon => "Hexagon",
            ShapeKind::Database => "Database",
            ShapeKind::Subroutine => "Subroutine",
            ShapeKind::Stadium => "Stadium",
            ShapeKind::Circle => "Circle",
            ShapeKind::DoubleCircle => "Double Circle",
            ShapeKind::Parallelogram => "Parallelogram",
            ShapeKind::ReverseParallelogram => "Reverse Parallelogram",
            ShapeKind::Trapezoid => "Trapezoid",
            ShapeKind::InvertedTrapezoid => "Inverted Trapezoid",
            ShapeKind::Asymmetric => "Asymmetric",
            ShapeKind::Other { .. } => "Shape",
        }
    }

    /// The Mermaid brackets around the label.
    pub fn delimiters(&self) -> (&str, &str) {
        match self {
            ShapeKind::Rect | ShapeKind::Text | ShapeKind::Note => ("[", "]"),
            ShapeKind::RoundRect => ("(", ")"),
            ShapeKind::Diamond => ("{", "}"),
            ShapeKind::Hexagon => ("{{", "}}"),
            ShapeKind::Database => ("[(", ")]"),
            ShapeKind::Subroutine => ("[[", "]]"),
            ShapeKind::Stadium => ("([", "])"),
            ShapeKind::Circle => ("((", "))"),
            ShapeKind::DoubleCircle => ("(((", ")))"),
            ShapeKind::Parallelogram => ("[/", "/]"),
            ShapeKind::ReverseParallelogram => ("[\\", "\\]"),
            ShapeKind::Trapezoid => ("[/", "\\]"),
            ShapeKind::InvertedTrapezoid => ("[\\", "/]"),
            ShapeKind::Asymmetric => (">", "]"),
            ShapeKind::Other { open, close } => (open, close),
        }
    }

    /// The kind a pair of Mermaid brackets makes.
    pub fn from_delimiters(open: &str, close: &str) -> Self {
        match (open, close) {
            ("[", _) => ShapeKind::Rect,
            ("(", _) => ShapeKind::RoundRect,
            ("{", _) => ShapeKind::Diamond,
            ("{{", _) => ShapeKind::Hexagon,
            ("[(", _) => ShapeKind::Database,
            ("[[", _) => ShapeKind::Subroutine,
            ("([", _) => ShapeKind::Stadium,
            ("((", _) => ShapeKind::Circle,
            ("(((", _) => ShapeKind::DoubleCircle,
            ("[/", "/]") => ShapeKind::Parallelogram,
            ("[\\", "\\]") => ShapeKind::ReverseParallelogram,
            ("[/", "\\]") => ShapeKind::Trapezoid,
            ("[\\", "/]") => ShapeKind::InvertedTrapezoid,
            (">", _) => ShapeKind::Asymmetric,
            _ => ShapeKind::Other {
                open: open.into(),
                close: close.into(),
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The Mermaid node id. Stable once created; the label can change freely.
    pub id: String,
    pub kind: ShapeKind,
    /// Plain text, `\n` between lines.
    pub label: String,
    pub rect: Rect,
    /// A `:::class` from the file other than `bl_text` and `bl_note`,
    /// written back as-is.
    pub class: Option<String>,
    /// Declared only in a statement BlockLiner keeps verbatim (a subgraph or
    /// an unsupported edge). It is positioned and drawn, but its declaration
    /// stays in that statement, so it is not editable.
    pub external: bool,
    /// Its `style` line: fill, stroke and label colours.
    pub style: Style,
}

impl Shape {
    pub fn new(
        id: impl Into<String>,
        kind: ShapeKind,
        label: impl Into<String>,
        rect: Rect,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            label: label.into(),
            rect,
            class: None,
            external: false,
            style: Style::default(),
        }
    }

    /// The style it's drawn with: its own, with a note's yellow fill and
    /// stroke where it doesn't set them. Only the drawing uses the
    /// defaults; nothing extra is saved.
    pub fn drawn_style(&self) -> Cow<'_, Style> {
        let style = &self.style;
        if self.kind != ShapeKind::Note || (style.fill.is_some() && style.stroke.is_some()) {
            return Cow::Borrowed(style);
        }
        let mut style = style.clone();
        style.fill.get_or_insert_with(|| Colour::parse(NOTE_FILL));
        style
            .stroke
            .get_or_insert_with(|| Colour::parse(NOTE_STROKE));
        Cow::Owned(style)
    }

    /// A new shape's size, in grid cells: 8 × 3 for boxes, 10 × 2 for text,
    /// 8 × 4 for a note,
    /// 8 × 4 for a diamond (its label sits in the middle half), 6 × 4
    /// for a database, 4 × 4 for a circle and 5 × 5 for a double circle
    /// (its label sits inside the inner ring).
    pub fn default_size(kind: &ShapeKind, grid: f32) -> (f32, f32) {
        let (w, h) = match kind {
            ShapeKind::Text => (10.0, 2.0),
            ShapeKind::Note => (8.0, 4.0),
            ShapeKind::Diamond => (8.0, 4.0),
            ShapeKind::Database => (6.0, 4.0),
            ShapeKind::Circle => (4.0, 4.0),
            ShapeKind::DoubleCircle => (5.0, 5.0),
            _ => (8.0, 3.0),
        };
        (w * grid, h * grid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConnectorKind {
    /// `a --- b`
    Line,
    /// `a --> b`
    Arrow,
}

/// A side midpoint of a shape, where a connector can attach.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Anchor {
    N,
    E,
    S,
    W,
}

impl Anchor {
    pub const ALL: [Anchor; 4] = [Anchor::N, Anchor::E, Anchor::S, Anchor::W];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::N => "N",
            Self::E => "E",
            Self::S => "S",
            Self::W => "W",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "N" => Self::N,
            "E" => Self::E,
            "S" => Self::S,
            "W" => Self::W,
            _ => return None,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum End {
    /// Attached to a shape. `anchor: None` attaches to whichever side faces
    /// the other end (Visio's dynamic glue), which is what imported edges get.
    Glued {
        shape: String,
        anchor: Option<Anchor>,
    },
    Free(Point),
}

impl End {
    pub fn glued(shape: impl Into<String>, anchor: Option<Anchor>) -> Self {
        Self::Glued {
            shape: shape.into(),
            anchor,
        }
    }

    pub fn shape(&self) -> Option<&str> {
        match self {
            Self::Glued { shape, .. } => Some(shape),
            Self::Free(_) => None,
        }
    }
}

/// How a connector runs between its ends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Route {
    /// One straight segment.
    #[default]
    Straight,
    /// Horizontal and vertical segments, recomputed as things move
    /// (`crate::route`). `mid` pins the middle segment where the user
    /// dragged it.
    Elbow { mid: Option<Mid> },
}

impl Route {
    pub const ELBOW: Route = Route::Elbow { mid: None };

    pub fn is_elbow(self) -> bool {
        matches!(self, Route::Elbow { .. })
    }
}

/// A right-angle route's pinned middle segment: a vertical one at this x,
/// or a horizontal one at this y. Written `x:240` / `y:120`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mid {
    X(f32),
    Y(f32),
}

impl Mid {
    pub fn parse(s: &str) -> Option<Self> {
        let (axis, v) = s.split_once(':')?;
        let v: f32 = v.parse().ok().filter(|v: &f32| v.is_finite())?;
        match axis {
            "x" => Some(Mid::X(v)),
            "y" => Some(Mid::Y(v)),
            _ => None,
        }
    }

    /// Moved with the rest of a selection.
    pub fn translated(self, dx: f32, dy: f32) -> Self {
        match self {
            Mid::X(x) => Mid::X(x + dx),
            Mid::Y(y) => Mid::Y(y + dy),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Connector {
    pub kind: ConnectorKind,
    pub from: End,
    pub to: End,
    /// An edge label from the file (`a -->|label| b`). Only Mermaid edges
    /// carry one; free lines have nowhere to store it.
    pub label: Option<String>,
    pub route: Route,
    /// Its colour (`stroke`) and other properties: a Mermaid edge's
    /// `linkStyle` line, or a free line's `stroke=` field.
    pub style: Style,
}

impl Connector {
    pub fn new(kind: ConnectorKind, from: End, to: End) -> Self {
        Self {
            kind,
            from,
            to,
            label: None,
            route: Route::Straight,
            style: Style::default(),
        }
    }

    pub fn with_route(mut self, route: Route) -> Self {
        self.route = route;
        self
    }

    /// Both ends glued, so the connector is a Mermaid edge. Otherwise it
    /// lives only in the layout comments and other renderers don't show it.
    pub fn is_edge(&self) -> bool {
        self.from.shape().is_some() && self.to.shape().is_some()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Diagram {
    pub direction: Direction,
    pub grid: f32,
    /// In creation order, which is the order they are written in.
    pub shapes: Vec<Shape>,
    /// In creation order. The writer puts Mermaid edges before free lines,
    /// so a diagram read back has its edges first.
    pub connectors: Vec<Connector>,
    /// Lines before the `flowchart` header (front matter, `%%{init}%%`),
    /// verbatim.
    pub preamble: Vec<String>,
    /// Body statements BlockLiner doesn't model (`style`, `subgraph` blocks,
    /// other edge styles, …), verbatim and in file order. Written after the
    /// nodes and edges.
    pub passthrough: Vec<String>,
}

impl Default for Diagram {
    fn default() -> Self {
        Self {
            direction: Direction::default(),
            grid: DEFAULT_GRID,
            shapes: Vec::new(),
            connectors: Vec::new(),
            preamble: Vec::new(),
            passthrough: Vec::new(),
        }
    }
}

impl Diagram {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn shape(&self, id: &str) -> Option<&Shape> {
        self.shapes.iter().find(|s| s.id == id)
    }

    pub fn shape_mut(&mut self, id: &str) -> Option<&mut Shape> {
        self.shapes.iter_mut().find(|s| s.id == id)
    }

    /// An unused node id for a shape labelled `label`.
    pub fn new_id(&self, label: &str) -> String {
        ids::unique(&ids::from_label(label), |id| self.shape(id).is_some())
    }

    /// Whether any connector exists only in BlockLiner (an end not glued).
    pub fn has_free_lines(&self) -> bool {
        self.connectors.iter().any(|c| !c.is_edge())
    }

    /// Adds a shape on top of the others, with an id made from `label`.
    /// Returns the id.
    pub fn add_shape(&mut self, kind: ShapeKind, rect: Rect, label: &str) -> String {
        let id = self.new_id(label);
        self.shapes.push(Shape::new(id.clone(), kind, label, rect));
        id
    }

    /// Removes the shapes with these ids, and the connectors glued to them
    /// (a Mermaid edge can't have a missing end). Returns how many shapes
    /// went.
    pub fn remove_shapes(&mut self, ids: &[String]) -> usize {
        let before = self.shapes.len();
        self.shapes.retain(|s| !ids.contains(&s.id));
        let gone = |e: &End| e.shape().is_some_and(|id| ids.iter().any(|x| x == id));
        self.connectors.retain(|c| !gone(&c.from) && !gone(&c.to));
        before - self.shapes.len()
    }

    /// Makes the shapes with these ids `kind` (Change Shape), keeping their
    /// box, label, colours, class and glued connectors. Shapes declared in
    /// kept Mermaid statements are skipped: their brackets aren't
    /// BlockLiner's to rewrite. Returns how many changed.
    pub fn set_shape_kind(&mut self, ids: &[String], kind: &ShapeKind) -> usize {
        let mut changed = 0;
        for s in &mut self.shapes {
            if !s.external && s.kind != *kind && ids.contains(&s.id) {
                s.kind = kind.clone();
                changed += 1;
            }
        }
        changed
    }

    /// Makes the connectors at these indexes lines or arrows, keeping
    /// everything else about them. Returns how many changed.
    pub fn set_connector_kind(&mut self, indexes: &[usize], kind: ConnectorKind) -> usize {
        let mut changed = 0;
        for &i in indexes {
            if let Some(c) = self.connectors.get_mut(i).filter(|c| c.kind != kind) {
                c.kind = kind;
                changed += 1;
            }
        }
        changed
    }

    /// Gives a shape a new id, updating the connectors glued to it. `false`
    /// if `from` doesn't exist or `to` is taken.
    pub fn rename_shape(&mut self, from: &str, to: &str) -> bool {
        if from == to {
            return self.shape(from).is_some();
        }
        if self.shape(to).is_some() {
            return false;
        }
        let Some(shape) = self.shape_mut(from) else {
            return false;
        };
        shape.id = to.to_string();
        for c in &mut self.connectors {
            for end in [&mut c.from, &mut c.to] {
                if let End::Glued { shape, .. } = end {
                    if shape == from {
                        *shape = to.to_string();
                    }
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits() {
        let mut d = Diagram::new();
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let api = d.add_shape(ShapeKind::Rect, r, "API");
        let api2 = d.add_shape(ShapeKind::Rect, r, "API");
        let db = d.add_shape(ShapeKind::RoundRect, r, "Orders DB");
        assert_eq!(
            (api.as_str(), api2.as_str(), db.as_str()),
            ("api", "api_2", "orders_db")
        );
        d.connectors.push(Connector::new(
            ConnectorKind::Arrow,
            End::glued("api", None),
            End::glued("orders_db", None),
        ));
        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::glued("api_2", None),
            End::Free(Point::new(5.0, 5.0)),
        ));

        assert!(d.rename_shape("orders_db", "db"));
        assert_eq!(d.connectors[0].to, End::glued("db", None));
        assert!(!d.rename_shape("db", "api"), "taken");
        assert!(!d.rename_shape("nope", "x"));

        assert_eq!(d.remove_shapes(&["api_2".into(), "nope".into()]), 1);
        assert_eq!(d.connectors.len(), 1, "the line glued to api_2 went too");
        assert_eq!(d.remove_shapes(&["db".into()]), 1);
        assert!(d.connectors.is_empty());
    }

    #[test]
    fn changing_kinds() {
        let mut d = Diagram::new();
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let a = d.add_shape(ShapeKind::Rect, r, "a");
        let b = d.add_shape(ShapeKind::Diamond, r, "b");
        let kept = d.add_shape(ShapeKind::Rect, r, "kept");
        d.shape_mut(&kept).unwrap().external = true;
        let ids = [a.clone(), b.clone(), kept.clone()];
        // b is already a diamond; the kept-statement shape is skipped.
        assert_eq!(d.set_shape_kind(&ids, &ShapeKind::Diamond), 1);
        assert_eq!(d.shape(&a).unwrap().kind, ShapeKind::Diamond);
        assert_eq!(d.shape(&kept).unwrap().kind, ShapeKind::Rect);
        assert_eq!(d.set_shape_kind(&ids, &ShapeKind::Diamond), 0);

        d.connectors.push(Connector::new(
            ConnectorKind::Line,
            End::glued(a.as_str(), None),
            End::glued(b.as_str(), None),
        ));
        // A missing index is ignored.
        assert_eq!(d.set_connector_kind(&[0, 9], ConnectorKind::Arrow), 1);
        assert_eq!(d.connectors[0].kind, ConnectorKind::Arrow);
        assert_eq!(d.set_connector_kind(&[0], ConnectorKind::Arrow), 0);

        // Every changeable kind has a name of its own.
        let mut names: Vec<_> = ShapeKind::CHANGEABLE.iter().map(|k| k.name()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), ShapeKind::CHANGEABLE.len());
    }

    #[test]
    fn notes_draw_yellow_unless_coloured() {
        let r = Rect::new(0.0, 0.0, 160.0, 80.0);
        let note = Shape::new("n", ShapeKind::Note, "", r);
        let drawn = note.drawn_style();
        assert_eq!(drawn.fill, Some(Colour::parse(NOTE_FILL)));
        assert_eq!(drawn.stroke, Some(Colour::parse(NOTE_STROKE)));
        // Dark text on the yellow, in either theme.
        assert_eq!(drawn.label_rgba(), Some([0x11, 0x18, 0x27, 255]));
        // Its own fill wins; the stroke stays yellow.
        let mut blue = note.clone();
        blue.style.fill = Some(Colour::parse("#dbeafe"));
        assert_eq!(blue.drawn_style().fill, Some(Colour::parse("#dbeafe")));
        assert_eq!(blue.drawn_style().stroke, Some(Colour::parse(NOTE_STROKE)));
        // Other shapes keep the theme's.
        let rect = Shape::new("r", ShapeKind::Rect, "", r);
        assert!(rect.drawn_style().is_empty());
        assert_eq!(Shape::default_size(&ShapeKind::Note, 20.0), (160.0, 80.0));
        assert_eq!(ShapeKind::CHANGEABLE.len(), 16);
        assert_eq!(
            ShapeKind::CHANGEABLE[14..],
            [ShapeKind::Text, ShapeKind::Note]
        );
    }
}
