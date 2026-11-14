//! BlockLiner's diagram editor as egui widgets, over `blockliner-core`: the
//! canvas, the tools, the Properties pane and the SVG and PNG export. Used by
//! BlockLiner, and by NoteLiner for the diagrams in a notes project.
//!
//! | Module       | Holds                                                        |
//! |--------------|--------------------------------------------------------------|
//! | `editor`     | `DiagramEditor`: a diagram, its undo history, canvas and tool |
//! | `canvas`     | the canvas: pan and zoom, the tools' gestures, painting, labels |
//! | `commands`   | the edit commands (`EditAct`) and their registrations         |
//! | `tools`      | the tool buttons, the More Shapes list and the tools' keys    |
//! | `properties` | the Properties pane                                          |
//! | `glyphs`     | the icons in the lists: tools, stroke styles, weights, text  |
//! | `export`     | the SVG writer and the PNG rasteriser                        |
//! | `dialogs`    | the Custom colour and Export dialogs                         |
//! | `units`      | the units of the dimension lines                             |

pub mod canvas;
pub mod commands;
pub mod dialogs;
pub mod editor;
pub mod export;
pub mod glyphs;
pub mod properties;
pub mod tools;
pub mod units;

pub use commands::{EditAct, EditView};
pub use editor::DiagramEditor;
