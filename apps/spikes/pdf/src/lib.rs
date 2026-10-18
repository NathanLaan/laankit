//! Spike 3 (Option C, Phase 0): can `hayro` drive PageLiner's PDF reader?
//! See `docs/plans/plan-native-rust-egui.md` and `RESULTS.md` next to this crate.
//!
//! All coordinates are PageLiner's "page units": pdf.js's scale-1 viewport, i.e. PDF points
//! with the origin at the top left of the (rotated, cropped) page. Highlights stored by the
//! Svelte reader use the same units, so they carry over unchanged.

pub mod annotations;
pub mod doc;
pub mod text;
