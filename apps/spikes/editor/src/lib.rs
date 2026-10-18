//! Option C spike 1: can egui's `TextEdit::multiline` with a
//! Markdown-highlighting layouter carry NoteLiner's editor?
//! See `docs/plans/plan-native-rust-egui.md`, Phase 0, and `RESULTS.md`.

pub mod editor;
pub mod gen;
pub mod highlight;
pub mod list_edits;

#[cfg(test)]
mod ui_tests;

/// The editor's text area: the same layout the GUI and the benchmark use.
pub fn editor_area(
    ui: &mut egui::Ui,
    editor: &mut editor::MarkdownEditor,
    text: &mut String,
) -> egui::text_edit::TextEditOutput {
    egui::ScrollArea::vertical()
        .id_salt("editor-scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| editor.show(ui, text))
        .inner
}
