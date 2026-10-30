//! A `Diagram` as a Mermaid flowchart. The flowchart says what exists; `%%`
//! comment lines at the end say where it is. Mermaid ignores the comments, so
//! the file still renders anywhere Mermaid does:
//!
//! ```text
//! flowchart LR
//!   api["API Gateway"]
//!   orders("Orders Service")
//!   note_1["Payments are async"]:::bl_text
//!   api --> orders
//!   classDef bl_text fill:none,stroke:none
//!
//! %% blockliner 1 grid=20
//! %% @shape api x=40 y=80 w=160 h=60
//! %% @shape orders x=280 y=80 w=160 h=60
//! %% @shape note_1 x=280 y=180 w=200 h=40
//! %% @edge api->orders from=E to=W
//! %% @line arrow from=600,40 to=orders:N
//! ```
//!
//! - `@shape <id> x= y= w= h=`: every shape's rectangle.
//! - `@edge <from>-><to>[#n] [from=<anchor>] [to=<anchor>]`: the anchors of
//!   the nth edge between two nodes, when not dynamic. Keyed by endpoints,
//!   not position, so a hand-inserted edge doesn't shift the others.
//! - `@line <line|arrow> from=<end> to=<end>`: a connector with a free end,
//!   which Mermaid can't express. An end is `x,y` or `id[:anchor]`.

mod label;
mod lex;
mod read;
mod write;

pub use read::{read, ReadError};
pub use write::write;

/// The `classDef` that makes a node a borderless text box.
pub(crate) const TEXT_CLASS: &str = "bl_text";
/// The `%% blockliner <version>` the writer emits.
pub(crate) const FORMAT_VERSION: u32 = 1;

/// The `@edge` key of the `n`th (1-based) edge from `from` to `to`.
pub(crate) fn edge_key(from: &str, to: &str, n: usize) -> String {
    if n > 1 {
        format!("{from}->{to}#{n}")
    } else {
        format!("{from}->{to}")
    }
}
