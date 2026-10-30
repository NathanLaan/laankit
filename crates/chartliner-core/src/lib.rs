//! ChartLiner's backend, independent of the UI shell: the diagram model and
//! its file format, a Mermaid flowchart with the layout kept in `%%` comments.
//! See `docs/plans/plan-chartliner.md`.
//!
//! | Module    | Holds                                                              |
//! |-----------|--------------------------------------------------------------------|
//! | `model`   | `Diagram`, shapes, connectors and the geometry types               |
//! | `mermaid` | `read` / `write` between a `Diagram` and `.mmd` text               |
//! | `ids`     | Mermaid node ids made from labels                                  |
//! | `layout`  | Positions for shapes the file gives none for                       |

pub mod ids;
pub mod layout;
pub mod mermaid;
pub mod model;

pub use mermaid::{read, write, ReadError};
pub use model::{
    Anchor, Connector, ConnectorKind, Diagram, Direction, End, Point, Rect, Shape, ShapeKind,
};
