//! BlockLiner's backend, independent of the UI shell: the diagram model and
//! its file format, a Mermaid flowchart with the layout kept in `%%` comments.
//! See `docs/plans/plan-blockliner.md`.
//!
//! | Module    | Holds                                                              |
//! |-----------|--------------------------------------------------------------------|
//! | `model`   | `Diagram`, shapes, connectors, the geometry types and edits        |
//! | `geometry`| Rectangle helpers, resize handles, hit-testing                     |
//! | `connect` | Where connectors meet shapes, and hit-testing them                 |
//! | `snap`    | Grid snapping                                                      |
//! | `mermaid` | `read` / `write` between a `Diagram` and `.mmd` text               |
//! | `ids`     | Mermaid node ids made from labels                                  |
//! | `layout`  | Positions for shapes the file gives none for                       |

pub mod connect;
pub mod geometry;
pub mod ids;
pub mod layout;
pub mod mermaid;
pub mod model;
pub mod snap;

pub use mermaid::{read, write, ReadError};
pub use model::{
    Anchor, Connector, ConnectorKind, Diagram, Direction, End, Point, Rect, Shape, ShapeKind,
};
