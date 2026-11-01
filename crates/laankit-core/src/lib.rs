//! Framework-independent backend logic shared by the LaanKit apps.
//!
//! Nothing in this crate depends on a UI toolkit, so the apps' backends stay
//! testable on their own.

pub mod debounce;
pub mod git;
pub mod json;
pub mod paths;
pub mod updates;
