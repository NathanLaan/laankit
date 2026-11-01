//! BlockLiner's backend, independent of the UI shell: the diagram model and
//! its file format, a Mermaid flowchart with the layout kept in `%%` comments.
//! The format is described in `apps/blockliner/README.md`.
//!
//! | Module    | Holds                                                              |
//! |-----------|--------------------------------------------------------------------|
//! | `clipboard`| Copy and paste: the selection as Mermaid, pasted with new ids   |
//! | `model`   | `Diagram`, shapes, connectors, the geometry types and edits        |
//! | `geometry`| Rectangle helpers, resize handles, hit-testing                     |
//! | `connect` | Where connectors meet shapes, their paths, and hit-testing them    |
//! | `route`   | Right-angle routes between two ends                                |
//! | `snap`    | Grid snapping                                                      |
//! | `mermaid` | `read` / `write` between a `Diagram` and `.mmd` text               |
//! | `ids`     | Mermaid node ids made from labels                                  |
//! | `history` | Undo and redo                                                      |
//! | `layout`  | Positions for shapes the file gives none for                       |
//! | `style`   | Shape colours: `style` lines, presets, label contrast              |

pub mod clipboard;
pub mod connect;
pub mod export;
pub mod geometry;
pub mod history;
pub mod ids;
pub mod layout;
pub mod mermaid;
pub mod model;
pub mod route;
pub mod snap;
pub mod style;

pub use mermaid::{read, write, ReadError};
pub use model::{
    Anchor, Connector, ConnectorKind, Diagram, Direction, End, Mid, Point, Rect, Route, Shape,
    ShapeKind,
};
