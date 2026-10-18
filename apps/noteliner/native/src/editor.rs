//! The note editor: `marina-editor`'s widget (`Editor.svelte`) wrapped with
//! the app's autosave and status contract.
//!
//! - **Loading.** The app calls [`NoteEditor::load`] when a note's
//!   `file:read` arrives (body only; frontmatter stays on disk). Before
//!   switching notes it calls [`NoteEditor::flush`] and writes what it
//!   returns, so a pending edit lands in the note it was made in. (The
//!   Svelte timer wrote to whatever note was selected when it fired.)
//! - **Autosave** (`scheduleSave` in `Editor.svelte`): every edit marks the
//!   note `Unsaved` and restarts a 500 ms timer; when it fires,
//!   [`NoteEditor::show`] returns [`EditorAction::Save`] and the status is
//!   `Saving` until the app reports [`NoteEditor::saved`]. Edits made while
//!   saving keep it `Unsaved` and schedule another save.
//! - **External changes** (`git:resetToRemote`, a history restore):
//!   [`NoteEditor::replace`] swaps the text in when nothing is pending, as
//!   `updateEditorContent` does. Slide edits go through
//!   [`NoteEditor::apply_slide_edit`] (`applySlideEdit`: replaced, then
//!   written at once).
//! - **Status bar**: [`NoteEditor::status`], [`NoteEditor::cursor`]
//!   (1-based line and column), [`NoteEditor::selection_len`] and the text
//!   for word and character counts.
//! - **Navigation**: [`NoteEditor::scroll_to_line`] (outline, backlinks,
//!   search results, slides) moves the caret to a line and scrolls it to the
//!   top.
//! - **Everything else the widget reports** (pasted and dropped files,
//!   Ctrl+click on links, the context menu's exports, "Add to dictionary")
//!   comes back as [`EditorAction`]s for the app.
//!
//! marina-editor needs the egui context for most calls (its caret lives in
//! egui's memory), so the app [`NoteEditor::attach`]es it once at startup.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{ColorImage, Ui};
use marina_editor::toolbar::{self, ToolbarAction, ToolbarProps};
use marina_editor::{
    Command, CursorStatus, Editor, EditorEvent, EditorPrefs, EditorState, SaveFormat, SpellChecker,
};

use crate::backend::ExportKind;

/// `scheduleSave`'s delay.
pub const SAVE_DELAY_SECS: f64 = 0.5;

/// `projectState.saveStatus`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SaveStatus {
    #[default]
    Saved,
    Saving,
    Unsaved,
}

impl SaveStatus {
    /// The status bar's label (`saveLabel`).
    pub fn label(self) -> &'static str {
        match self {
            SaveStatus::Saved => "Saved",
            SaveStatus::Saving => "Saving…",
            SaveStatus::Unsaved => "Unsaved",
        }
    }
}

/// A write for the app to send to the backend (`file:write`).
#[derive(Clone, Debug, PartialEq)]
pub struct SaveRequest {
    pub id: String,
    pub filename: String,
    pub content: String,
}

/// What the editor asks the app to do.
#[derive(Clone, Debug)]
pub enum EditorAction {
    Save(SaveRequest),
    TogglePreview,
    ToggleHistory,
    /// Right-click → Save to HTML / PDF / Markdown.
    Export(ExportKind),
    /// An image was pasted: save it as an attachment of `file_id`, then
    /// insert its Markdown (`processFiles`).
    PasteImage {
        file_id: String,
        image: Arc<ColorImage>,
    },
    /// Files were pasted or dropped on the editor: attach them likewise.
    AttachFiles {
        file_id: String,
        paths: Vec<PathBuf>,
    },
    /// Ctrl/Cmd+click on `[[name]]`.
    OpenWikilink(String),
    /// Ctrl/Cmd+click on a Markdown link.
    OpenLink(String),
    /// "Add to dictionary" on a misspelled word.
    AddToDictionary(String),
}

/// The note being edited.
pub struct NoteEditor {
    /// The app's egui context (see [`NoteEditor::attach`]).
    ctx: egui::Context,
    state: EditorState,
    id: Option<String>,
    name: String,
    filename: String,
    status: SaveStatus,
    /// When the last edit happened (egui time), while a save is pending.
    dirty_since: Option<f64>,
    /// The content of the write in flight.
    saving: Option<String>,
    /// The caret and selection as of the last frame.
    caret: CursorStatus,
    /// Where the pointer was over the text on the last drag, click or release
    /// frame, and the text area's rect (for drops from the file tree).
    char_at_pointer: Option<usize>,
    text_rect: Option<egui::Rect>,
}

impl Default for NoteEditor {
    fn default() -> Self {
        Self {
            ctx: egui::Context::default(),
            state: EditorState::new("noteliner-editor"),
            id: None,
            name: String::new(),
            filename: String::new(),
            status: SaveStatus::Saved,
            dirty_since: None,
            saving: None,
            caret: CursorStatus {
                line: 1,
                col: 1,
                ..Default::default()
            },
            char_at_pointer: None,
            text_rect: None,
        }
    }
}

impl NoteEditor {
    /// Uses the app's context from now on (call once, before any note loads).
    pub fn attach(&mut self, ctx: &egui::Context) {
        if self.ctx != *ctx {
            self.ctx = ctx.clone();
            let text = self.state.text().to_string();
            self.state.set_text(ctx, text);
        }
    }

    /// A freshly read note (`selectFile`): in sync with disk.
    pub fn load(&mut self, id: &str, name: &str, filename: &str, content: String) {
        self.id = Some(id.to_string());
        self.name = name.to_string();
        self.filename = filename.to_string();
        self.state.set_text(&self.ctx, content);
        self.status = SaveStatus::Saved;
        self.dirty_since = None;
        self.saving = None;
        self.caret = CursorStatus {
            line: 1,
            col: 1,
            ..Default::default()
        };
    }

    /// No note open (the note was deleted, or the project closed).
    pub fn clear(&mut self) {
        self.id = None;
        self.name.clear();
        self.filename.clear();
        self.state.set_text(&self.ctx, "");
        self.status = SaveStatus::Saved;
        self.dirty_since = None;
        self.saving = None;
        self.caret = CursorStatus {
            line: 1,
            col: 1,
            ..Default::default()
        };
        self.char_at_pointer = None;
        self.text_rect = None;
    }

    pub fn file_id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn text(&self) -> &str {
        self.state.text()
    }

    /// Bumped on every change of the text (to cache what's derived from it).
    pub fn version(&self) -> u64 {
        self.state.version()
    }

    pub fn status(&self) -> SaveStatus {
        self.status
    }

    /// The caret: 1-based line and column.
    pub fn cursor(&self) -> (usize, usize) {
        (self.caret.line.max(1), self.caret.col.max(1))
    }

    /// Characters selected; 0 for a bare caret.
    pub fn selection_len(&self) -> usize {
        self.caret.selection_len
    }

    /// The selected lines (1-based, inclusive), or the caret's line.
    pub fn selected_lines(&self) -> (usize, usize) {
        self.caret
            .selection_lines
            .unwrap_or((self.caret.line.max(1), self.caret.line.max(1)))
    }

    /// The editor's display preferences (`spellCheckEnabled` and the fixed
    /// CodeMirror setup: 14 px, line numbers, wrapping).
    pub fn set_spell_check(&mut self, on: bool) {
        let prefs = self.state.prefs();
        if prefs.spell_check != on {
            self.state.set_prefs(EditorPrefs {
                spell_check: on,
                ..prefs
            });
        }
    }

    pub fn set_spell_checker(&mut self, checker: Option<Arc<dyn SpellChecker>>) {
        self.state.set_spell_checker(checker);
    }

    /// Re-checks every line (after "Add to dictionary").
    pub fn refresh_spelling(&mut self) {
        self.state.refresh_spelling();
    }

    /// The note names `[[` completes (the open note left out).
    pub fn set_link_targets(&mut self, names: Vec<String>) {
        self.state.set_link_targets(names);
    }

    /// The note was renamed: later saves go to its new file.
    pub fn renamed(&mut self, id: &str, name: &str, filename: &str) {
        if self.id.as_deref() == Some(id) {
            self.name = name.to_string();
            self.filename = filename.to_string();
        }
    }

    /// A pending edit to write now (before switching notes or quitting).
    pub fn flush(&mut self) -> Option<SaveRequest> {
        self.dirty_since.take()?;
        let request = self.request()?;
        self.saving = Some(request.content.clone());
        self.status = SaveStatus::Saving;
        Some(request)
    }

    /// The backend finished a write of `content` for note `id`.
    pub fn saved(&mut self, id: &str, content: &str, ok: bool) {
        if self.id.as_deref() != Some(id) || self.saving.as_deref() != Some(content) {
            return;
        }
        self.saving = None;
        if self.dirty_since.is_some() {
            return;
        }
        self.status = if ok && self.state.text() == content {
            SaveStatus::Saved
        } else {
            SaveStatus::Unsaved
        };
    }

    /// The text changed outside the editor: take it unless an edit is pending.
    /// (For an external reload such as a history restore; reset-from-remote
    /// reloads the note instead.)
    #[allow(dead_code)]
    pub fn replace(&mut self, id: &str, content: String) -> bool {
        if self.id.as_deref() != Some(id)
            || self.dirty_since.is_some()
            || content == self.state.text()
        {
            return false;
        }
        self.state.replace_text(&self.ctx, content);
        true
    }

    /// `applySlideEdit`: the note becomes `markdown` (one undo step), the
    /// caret goes to `caret_line`, and the write starts at once.
    pub fn apply_slide_edit(
        &mut self,
        markdown: String,
        caret_line: Option<usize>,
    ) -> Option<SaveRequest> {
        self.id.as_ref()?;
        if markdown == self.state.text() {
            return None;
        }
        self.state.replace_text(&self.ctx, markdown);
        if let Some(line) = caret_line.filter(|l| *l > 0) {
            self.state.scroll_to_line(&self.ctx, line);
        }
        self.dirty_since = Some(0.0);
        self.flush()
    }

    /// Inserts `snippet` at the caret as an edit (autosave follows): the
    /// Attachments panel's "Insert into Note", and pasted attachments.
    pub fn insert(&mut self, snippet: &str, now: f64) -> bool {
        if self.id.is_none() {
            return false;
        }
        self.state.run(Command::Insert(snippet.to_string()));
        self.status = SaveStatus::Unsaved;
        self.dirty_since = Some(now);
        self.ctx.request_repaint();
        true
    }

    /// Inserts `snippet` at character `at` (a note dropped from the tree).
    pub fn insert_at(&mut self, at: usize, snippet: &str, now: f64) -> bool {
        if self.id.is_none() {
            return false;
        }
        self.state.run(Command::InsertAt(at, snippet.to_string()));
        self.status = SaveStatus::Unsaved;
        self.dirty_since = Some(now);
        self.ctx.request_repaint();
        true
    }

    /// The character under the pointer, while the pointer is over the text
    /// on a drag or release frame.
    pub fn drop_target(&self, pointer: Option<egui::Pos2>) -> Option<usize> {
        let over = pointer
            .zip(self.text_rect)
            .is_some_and(|(p, r)| r.contains(p));
        self.char_at_pointer.filter(|_| over)
    }

    /// Whether `pos` is over the text area (for the window's drop overlay).
    pub fn contains(&self, pos: egui::Pos2) -> bool {
        self.text_rect.is_some_and(|r| r.contains(pos))
    }

    /// Move the caret to `line` (1-based), scroll it to the top and focus.
    pub fn scroll_to_line(&mut self, line: usize) {
        self.state.scroll_to_line(&self.ctx, line.max(1));
        self.ctx.request_repaint();
    }

    fn request(&self) -> Option<SaveRequest> {
        Some(SaveRequest {
            id: self.id.clone()?,
            filename: self.filename.clone(),
            content: self.state.text().to_string(),
        })
    }

    /// The toolbar and the text; returns what the app should do.
    pub fn show(&mut self, ui: &mut Ui, preview_on: bool, history_on: bool) -> Vec<EditorAction> {
        let mut out = Vec::new();
        let props = ToolbarProps {
            file_name: self.id.as_ref().map(|_| self.name.as_str()),
            show_history: history_on,
            show_preview: preview_on,
        };
        // The toolbar centers its row in the height it's given: a top panel.
        let state = &mut self.state;
        let actions = egui::Panel::top(ui.scope_id().with("editor-toolbar"))
            .frame(egui::Frame::NONE)
            .show(ui, |ui| toolbar::show(ui, state, &props))
            .inner;
        for a in actions {
            out.push(match a {
                ToolbarAction::TogglePreview => EditorAction::TogglePreview,
                ToolbarAction::ToggleHistory => EditorAction::ToggleHistory,
            });
        }
        let now = ui.input(|i| i.time);
        match self.id.clone() {
            Some(id) => {
                let shown = Editor::new(&mut self.state).show(ui);
                self.text_rect = Some(shown.response.rect);
                self.char_at_pointer = shown.char_at_pointer;
                for event in shown.events {
                    self.event(event, &id, now, &mut out);
                }
                self.caret = self.state.status(ui.ctx());
            }
            None => {
                self.text_rect = None;
                let p = marina_egui::theme::palette(ui.ctx());
                ui.add_space(16.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("No file selected").color(p.text_muted));
                });
            }
        }
        if let Some(save) = self.due(now) {
            out.push(EditorAction::Save(save));
        }
        // Keep the autosave timer ticking without input.
        if self.dirty_since.is_some() {
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(SAVE_DELAY_SECS));
        }
        out
    }

    fn event(&mut self, event: EditorEvent, id: &str, now: f64, out: &mut Vec<EditorAction>) {
        let file_id = id.to_string();
        match event {
            EditorEvent::Changed => {
                self.status = SaveStatus::Unsaved;
                self.dirty_since = Some(now);
            }
            EditorEvent::PasteImage(image) => out.push(EditorAction::PasteImage { file_id, image }),
            EditorEvent::PasteFiles(paths) | EditorEvent::DropFiles(paths) => {
                out.push(EditorAction::AttachFiles { file_id, paths })
            }
            EditorEvent::OpenLink(url) => out.push(EditorAction::OpenLink(url)),
            EditorEvent::OpenWikilink(name) => out.push(EditorAction::OpenWikilink(name)),
            EditorEvent::Save(format) => out.push(EditorAction::Export(match format {
                SaveFormat::Html => ExportKind::Html,
                SaveFormat::Pdf => ExportKind::Pdf,
                SaveFormat::Markdown => ExportKind::Markdown,
            })),
            EditorEvent::AddToDictionary(word) => out.push(EditorAction::AddToDictionary(word)),
        }
    }

    /// The autosave timer: a save once the text has been still for 500 ms.
    fn due(&mut self, now: f64) -> Option<SaveRequest> {
        let since = self.dirty_since?;
        if now - since < SAVE_DELAY_SECS || self.saving.is_some() {
            return None;
        }
        self.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor() -> NoteEditor {
        let mut e = NoteEditor::default();
        e.load("a", "Alpha", "alpha.md", "hello".into());
        e
    }

    fn edit(e: &mut NoteEditor, text: &str, at: f64) {
        let ctx = e.ctx.clone();
        e.state.set_text(&ctx, text);
        e.status = SaveStatus::Unsaved;
        e.dirty_since = Some(at);
    }

    #[test]
    fn autosave_waits_500ms_then_saves_once() {
        let mut e = editor();
        edit(&mut e, "hello!", 1.0);
        assert_eq!(e.due(1.3), None);
        let save = e.due(1.6).expect("due");
        assert_eq!(save.content, "hello!");
        assert_eq!(save.filename, "alpha.md");
        assert_eq!(e.status(), SaveStatus::Saving);
        assert_eq!(e.due(5.0), None, "nothing new to save");
        e.saved("a", "hello!", true);
        assert_eq!(e.status(), SaveStatus::Saved);
    }

    #[test]
    fn edits_during_a_save_stay_unsaved() {
        let mut e = editor();
        edit(&mut e, "one", 0.0);
        let first = e.due(1.0).unwrap();
        edit(&mut e, "two", 1.1);
        e.saved("a", &first.content, true);
        assert_eq!(e.status(), SaveStatus::Unsaved);
        assert_eq!(e.due(2.0).unwrap().content, "two");
    }

    #[test]
    fn flush_before_switching_and_git_errors() {
        let mut e = editor();
        edit(&mut e, "pending", 0.0);
        let f = e.flush().unwrap();
        assert_eq!((f.id.as_str(), f.content.as_str()), ("a", "pending"));
        assert_eq!(e.flush(), None);
        e.saved("a", "pending", false);
        assert_eq!(e.status(), SaveStatus::Unsaved);
    }

    #[test]
    fn external_changes_only_replace_clean_text() {
        let mut e = editor();
        assert!(e.replace("a", "from remote".into()));
        assert_eq!(e.text(), "from remote");
        edit(&mut e, "mine", 0.0);
        assert!(!e.replace("a", "theirs".into()));
        assert!(!e.replace("b", "other".into()));
    }

    #[test]
    fn slide_edits_save_at_once() {
        let mut e = editor();
        let req = e.apply_slide_edit("# One\n\n---\n\n# Two\n".into(), Some(5));
        let req = req.expect("written at once");
        assert_eq!(req.content, "# One\n\n---\n\n# Two\n");
        assert_eq!(e.status(), SaveStatus::Saving);
        assert_eq!(e.apply_slide_edit(e.text().to_string(), None), None);
    }
}
