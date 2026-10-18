//! Framework-independent backend logic shared by the Marina apps.
//!
//! Nothing in this crate depends on Tauri or a UI toolkit, so it is shared
//! by the Tauri builds (plan Option A) and the native egui builds (Option C).
//! See `docs/plans/plan-native-linux.md` and `docs/plans/plan-native-rust-egui.md`.

pub mod debounce;
pub mod git;
pub mod json;
pub mod paths;
pub mod updates;
