//! The diagram model. Coordinates are diagram pixels at 100 % zoom, with y
//! pointing down; the canvas maps them to the screen.

use crate::ids;

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
    /// Any other Mermaid node shape (`[( )]`, `{{ }}`, …), kept with its
    /// delimiters so it is written back unchanged. Drawn as a rectangle.
    Other { open: String, close: String },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    /// The Mermaid node id. Stable once created; the label can change freely.
    pub id: String,
    pub kind: ShapeKind,
    /// Plain text, `\n` between lines.
    pub label: String,
    pub rect: Rect,
    /// A `:::class` from the file other than `bl_text`, written back as-is.
    pub class: Option<String>,
    /// Declared only in a statement BlockLiner keeps verbatim (a subgraph or
    /// an unsupported edge). It is positioned and drawn, but its declaration
    /// stays in that statement, so it is not editable.
    pub external: bool,
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
        }
    }

    /// A new shape's size, in grid cells: 8 × 3 for boxes, 10 × 2 for text.
    pub fn default_size(kind: &ShapeKind, grid: f32) -> (f32, f32) {
        match kind {
            ShapeKind::Text => (10.0 * grid, 2.0 * grid),
            _ => (8.0 * grid, 3.0 * grid),
        }
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

#[derive(Clone, Debug, PartialEq)]
pub struct Connector {
    pub kind: ConnectorKind,
    pub from: End,
    pub to: End,
    /// An edge label from the file (`a -->|label| b`). Only Mermaid edges
    /// carry one; free lines have nowhere to store it.
    pub label: Option<String>,
}

impl Connector {
    pub fn new(kind: ConnectorKind, from: End, to: End) -> Self {
        Self {
            kind,
            from,
            to,
            label: None,
        }
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
}
