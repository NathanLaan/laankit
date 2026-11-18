//! The note editor: `laankit-editor`'s widget wrapped with the app's
//! autosave, status and frontmatter contract.
//!
//! - **The whole file.** The editor holds a note's file as it is on disk:
//!   the `---` frontmatter block at the top, then the text. What is typed in
//!   the block is saved as typed. Everything else that reads the note (the
//!   preview, slides, outline, word count, exports) reads [`NoteEditor::text`],
//!   which is the text after the block, and lines given to or taken from
//!   those are lines of that text ([`NoteEditor::scroll_to_line`],
//!   [`NoteEditor::body_line`]).
//! - **Frontmatter.** [`NoteEditor::frontmatter`] is the block as last read.
//!   While the block can't be read as YAML (a half-typed line) it keeps its
//!   last good value and [`NoteEditor::frontmatter_unreadable`] is set. When
//!   NoteLiner changes the block itself (a tag added in the Tags pane,
//!   Presentation Settings, a field a save put back),
//!   [`NoteEditor::set_frontmatter`] swaps it in: not an undo step, and the
//!   note isn't marked as edited.
//! - **Loading.** The app calls [`NoteEditor::load`] when a note's file has
//!   been read. Before switching notes it calls [`NoteEditor::flush`] and
//!   writes what it returns, so a pending edit lands in the note it was made
//!   in.
//! - **Autosave**: every edit marks the note `Unsaved` and restarts a 500 ms
//!   timer; when it fires, [`NoteEditor::show`] returns
//!   [`EditorAction::Save`] and the status is `Saving` until the app reports
//!   [`NoteEditor::saved`]. Edits made while saving keep it `Unsaved` and
//!   schedule another save.
//! - **Changes on disk** by another program: [`NoteEditor::replace`] swaps
//!   the file in when nothing is pending. With an edit pending the app asks
//!   first, and [`NoteEditor::hold`] stops the autosave until it has an
//!   answer. Slide edits go through [`NoteEditor::apply_slide_edit`]
//!   (replaced, then written at once).
//! - **Status bar**: [`NoteEditor::status`], [`NoteEditor::cursor`]
//!   (1-based line and column in the file), [`NoteEditor::selection_len`]
//!   and the text for word and character counts.
//! - **Everything else the widget reports** (pasted and dropped files,
//!   Ctrl+click on links, the context menu's exports, "Add to dictionary")
//!   comes back as [`EditorAction`]s for the app.
//!
//! laankit-editor needs the egui context for most calls (its caret lives in
//! egui's memory), so the app [`NoteEditor::attach`]es it once at startup.

use std::path::PathBuf;
use std::sync::Arc;

use egui::{ColorImage, Ui};
use laankit_editor::toolbar::{self, ToolbarAction, ToolbarProps};
use laankit_editor::{
    Command, CursorStatus, Editor, EditorEvent, EditorPrefs, EditorState, SaveFormat, SpellChecker,
};

use noteliner_core::frontmatter::{self, Block};
use serde_json::{json, Value};

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

/// A write for the app to send to the backend: the whole file.
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
    /// The frontmatter block, as of text version `front.version`.
    front: Front,
    /// The autosave waits (the app is asking about a change on disk).
    held: bool,
    /// A write was in flight when the hold began: it lands on top of the
    /// change on disk that the app is asking about.
    held_over_write: bool,
}

/// Where the frontmatter block ends, and what it holds.
#[derive(Clone, Debug, PartialEq)]
struct Front {
    /// The text version this was read from.
    version: Option<u64>,
    /// The byte where the note's text starts.
    start: usize,
    /// The lines before the note's text.
    lines: usize,
    /// The block as written, to notice when it changes.
    block: String,
    /// The block's fields: the last ones that could be read.
    data: Value,
    /// The block isn't YAML NoteLiner can read.
    unreadable: bool,
}

impl Default for Front {
    fn default() -> Self {
        Self {
            version: None,
            start: 0,
            lines: 0,
            block: String::new(),
            data: json!({}),
            unreadable: false,
        }
    }
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
            front: Front::default(),
            held: false,
            held_over_write: false,
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

    /// A freshly read note, with its frontmatter: in sync with disk. The
    /// caret starts on the note's text, below the block.
    pub fn load(&mut self, id: &str, name: &str, filename: &str, content: String) {
        self.id = Some(id.to_string());
        self.name = name.to_string();
        self.filename = filename.to_string();
        self.state.set_text(&self.ctx, content);
        self.status = SaveStatus::Saved;
        self.dirty_since = None;
        self.saving = None;
        self.held = false;
        self.held_over_write = false;
        self.front = Front::default();
        self.sync();
        let start = self.state.text()[..self.front.start].chars().count();
        self.state.set_cursor(&self.ctx, start, start);
        self.state.set_scroll_offset(egui::Vec2::ZERO);
        self.caret = CursorStatus {
            line: self.front.lines + 1,
            col: 1,
            ..Default::default()
        };
    }

    /// Reads the frontmatter block again if the text changed.
    fn sync(&mut self) {
        let version = self.state.version();
        if self.front.version == Some(version) {
            return;
        }
        self.front.version = Some(version);
        let text = self.state.text();
        let start = frontmatter::body_start(text);
        self.front.start = start;
        if text[..start] == self.front.block {
            return;
        }
        self.front.block = text[..start].to_string();
        self.front.lines = self.front.block.bytes().filter(|b| *b == b'\n').count();
        match frontmatter::block(text) {
            Block::Missing => {
                self.front.data = json!({});
                self.front.unreadable = false;
            }
            Block::Data(data) => {
                self.front.data = frontmatter::to_json(&data);
                self.front.unreadable = false;
            }
            // The last good fields stay until the block reads again.
            Block::Unreadable => self.front.unreadable = true,
        }
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
        self.held = false;
        self.held_over_write = false;
        self.front = Front::default();
    }

    /// An empty editor set up as this one (context, display preferences),
    /// to take its place while it waits in a background tab.
    pub fn sibling(&self) -> NoteEditor {
        let mut e = NoteEditor {
            ctx: self.ctx.clone(),
            ..Default::default()
        };
        e.state.set_prefs(self.state.prefs());
        e
    }

    pub fn file_id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    /// The note's text, without its frontmatter: what the preview, slides,
    /// outline and counts read.
    pub fn text(&self) -> &str {
        let text = self.state.text();
        text.get(self.front.start..).unwrap_or(text)
    }

    /// The whole file, as the editor shows it and as it is saved.
    pub fn file_text(&self) -> &str {
        self.state.text()
    }

    /// The frontmatter's fields, as last read.
    pub fn frontmatter(&self) -> &Value {
        &self.front.data
    }

    /// The frontmatter can't be read as YAML just now (a half-typed line).
    pub fn frontmatter_unreadable(&self) -> bool {
        self.id.is_some() && self.front.unreadable
    }

    /// The note has a `presentation:` block.
    pub fn is_deck(&self) -> bool {
        self.id.is_some()
            && self
                .front
                .data
                .get("presentation")
                .is_some_and(|p| !p.is_null())
    }

    /// The note's name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// An edit is waiting to be written, or being written.
    pub fn is_dirty(&self) -> bool {
        self.dirty_since.is_some() || self.saving.is_some()
    }

    /// Stops (or restarts) the autosave, while the app asks what to do about
    /// a change on disk.
    pub fn hold(&mut self, held: bool) {
        self.held = held;
        self.held_over_write = held && self.saving.is_some();
        if !held && self.dirty_since.is_some() {
            self.ctx.request_repaint();
        }
    }

    /// Replaces the frontmatter block with the one at the top of `file`
    /// (a whole file, as NoteLiner wrote it). Not an undo step, and the note
    /// isn't marked as edited. Returns whether the block changed.
    pub fn set_frontmatter(&mut self, file: &str) -> bool {
        if self.id.is_none() {
            return false;
        }
        self.sync();
        let head = &file[..frontmatter::body_start(file)];
        let changed = self.state.replace_head(&self.ctx, self.front.start, head);
        if changed {
            self.sync();
            self.ctx.request_repaint();
        }
        changed
    }

    /// The block, when a save of `saved` came back as `rewritten`: taken only
    /// if the block is still the one that was saved (the user hasn't typed
    /// in it since).
    pub fn frontmatter_rewritten(&mut self, saved: &str, rewritten: &str) -> bool {
        self.sync();
        if saved[..frontmatter::body_start(saved)] != self.front.block {
            return false;
        }
        self.set_frontmatter(rewritten)
    }

    /// Writes an index entry's tags into the block, as the backend does in
    /// the file. A block that can't be read is left alone.
    pub fn retag(&mut self, entry: &Value) -> bool {
        self.sync();
        let no_tags = entry["tags"].as_array().is_none_or(Vec::is_empty);
        if self.id.is_none() || (no_tags && self.front.start == 0) {
            return false;
        }
        match frontmatter::retag(self.state.text(), entry) {
            Some(next) => self.set_frontmatter(&next),
            None => false,
        }
    }

    /// Sets, replaces or (with null) removes the block's `presentation:`.
    pub fn set_presentation(&mut self, presentation: &Value) -> bool {
        if self.id.is_none() {
            return false;
        }
        match frontmatter::with_presentation(self.state.text(), presentation) {
            Some(next) => self.set_frontmatter(&next),
            None => false,
        }
    }

    /// Bumped on every change of the text (to cache what's derived from it).
    pub fn version(&self) -> u64 {
        self.state.version()
    }

    pub fn status(&self) -> SaveStatus {
        self.status
    }

    /// The caret: 1-based line and column in the file, frontmatter counted.
    pub fn cursor(&self) -> (usize, usize) {
        (self.caret.line.max(1), self.caret.col.max(1))
    }

    /// The caret's line in the note's text (1-based). 0 while the caret is
    /// in the frontmatter.
    pub fn body_line(&self) -> usize {
        self.caret.line.max(1).saturating_sub(self.front.lines)
    }

    /// Characters selected; 0 for a bare caret.
    pub fn selection_len(&self) -> usize {
        self.caret.selection_len
    }

    /// The selected lines of the note's text (1-based, inclusive), or the
    /// caret's line.
    pub fn selected_lines(&self) -> (usize, usize) {
        let (from, to) = self
            .caret
            .selection_lines
            .unwrap_or((self.caret.line.max(1), self.caret.line.max(1)));
        let body = |l: usize| l.saturating_sub(self.front.lines).max(1);
        (body(from), body(to))
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

    /// The file changed on disk: take it, unless an edit is pending. The
    /// caret stays where it can, and the note is in sync with disk.
    pub fn replace(&mut self, id: &str, content: String) -> bool {
        if self.id.as_deref() != Some(id) || self.is_dirty() || content == self.state.text() {
            return false;
        }
        self.state.replace_text(&self.ctx, content);
        self.status = SaveStatus::Saved;
        self.sync();
        self.ctx.request_repaint();
        true
    }

    /// Takes the file from disk over a pending edit (the user chose Reload).
    /// If a write of the edit was already in flight when the change was
    /// found, it has landed on top of the file: the content is returned to
    /// be written back.
    pub fn reload(&mut self, id: &str, content: String) -> Option<SaveRequest> {
        if self.id.as_deref() != Some(id) {
            return None;
        }
        let write_back = self.held_over_write;
        self.dirty_since = None;
        self.saving = None;
        self.held = false;
        self.held_over_write = false;
        self.state.replace_text(&self.ctx, content);
        self.status = SaveStatus::Saved;
        self.sync();
        self.ctx.request_repaint();
        if !write_back {
            return None;
        }
        let request = self.request()?;
        self.saving = Some(request.content.clone());
        self.status = SaveStatus::Saving;
        Some(request)
    }

    /// A slide edit: the note's text becomes `markdown` (one undo step) and
    /// its frontmatter stays, the caret goes to `caret_line` of the text,
    /// and the write starts at once.
    pub fn apply_slide_edit(
        &mut self,
        markdown: String,
        caret_line: Option<usize>,
    ) -> Option<SaveRequest> {
        self.id.as_ref()?;
        self.sync();
        if markdown == self.text() {
            return None;
        }
        let file = format!("{}{markdown}", self.front.block);
        self.state.replace_text(&self.ctx, file);
        self.sync();
        if let Some(line) = caret_line.filter(|l| *l > 0) {
            self.state
                .scroll_to_line(&self.ctx, line + self.front.lines);
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

    /// The open note's id, caret (anchor, head in characters) and scroll
    /// offset, for remembering where it was left.
    pub fn view(&self) -> Option<(&str, usize, usize, egui::Vec2)> {
        let id = self.id.as_deref()?;
        let (anchor, head) = self.state.cursor(&self.ctx);
        Some((id, anchor, head, self.state.scroll_offset()))
    }

    /// Puts the caret and scroll back where `view` said they were (the note
    /// must be loaded); offsets past the end are clamped.
    pub fn restore_view(&mut self, anchor: usize, head: usize, scroll: egui::Vec2) {
        self.place_view(anchor, head, scroll);
        self.state.focus();
    }

    /// As [`NoteEditor::restore_view`], leaving the keyboard focus where it
    /// is (a tab shown from the file tree keeps the tree's keys).
    pub fn place_view(&mut self, anchor: usize, head: usize, scroll: egui::Vec2) {
        let n = self.state.text().chars().count();
        self.state
            .restore_view(&self.ctx, anchor.min(n), head.min(n), scroll);
        self.ctx.request_repaint();
    }

    pub fn can_undo(&self) -> bool {
        self.id.is_some() && self.state.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.id.is_some() && self.state.can_redo()
    }

    /// Undo, from outside the editor (the toolbar, the palette, or Ctrl+Z
    /// while the editor doesn't have the keyboard). The autosave follows.
    pub fn undo(&mut self) {
        if self.can_undo() {
            self.state.run(Command::Undo);
            self.ctx.request_repaint();
        }
    }

    /// Redo, as [`NoteEditor::undo`].
    pub fn redo(&mut self) {
        if self.can_redo() {
            self.state.run(Command::Redo);
            self.ctx.request_repaint();
        }
    }

    /// Bytes held by the undo history.
    pub fn history_bytes(&self) -> usize {
        self.state.history_bytes()
    }

    /// Drops the oldest undo steps until at most `max` bytes are held.
    pub fn trim_history(&mut self, max: usize) {
        self.state.trim_history(max);
    }

    /// Opens the editor's Go to line bar (Ctrl+Alt+G, or the palette).
    pub fn go_to_line(&mut self) {
        if self.id.is_some() {
            self.state.open_goto();
            self.ctx.request_repaint();
        }
    }

    /// Move the caret to `line` of the note's text (1-based, the frontmatter
    /// not counted), scroll it to the top and focus.
    pub fn scroll_to_line(&mut self, line: usize) {
        self.sync();
        self.state
            .scroll_to_line(&self.ctx, line.max(1) + self.front.lines);
        self.ctx.request_repaint();
    }

    /// Where character `c` of the file was painted last frame.
    #[cfg(test)]
    pub fn char_rect(&mut self, c: usize) -> Option<egui::Rect> {
        self.state.screen_rect_of(c)
    }

    /// Replaces the whole file as the user would by typing: the note is
    /// unsaved from `now`, and the autosave follows.
    #[cfg(test)]
    pub fn type_file(&mut self, text: &str, now: f64) {
        self.state.replace_text(&self.ctx, text);
        self.status = SaveStatus::Unsaved;
        self.dirty_since = Some(now);
        self.sync();
    }

    fn request(&self) -> Option<SaveRequest> {
        Some(SaveRequest {
            id: self.id.clone()?,
            filename: self.filename.clone(),
            content: self.state.text().to_string(),
        })
    }

    /// The toolbar and the text; returns what the app should do.
    pub fn show(&mut self, ui: &mut Ui, preview_on: bool) -> Vec<EditorAction> {
        let mut out = Vec::new();
        let props = ToolbarProps {
            file_name: self.id.as_ref().map(|_| self.name.as_str()),
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
                self.sync();
            }
            None => {
                self.text_rect = None;
                let p = laankit_egui::theme::palette(ui.ctx());
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
        if self.held || now - since < SAVE_DELAY_SECS || self.saving.is_some() {
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
    fn flush_before_switching_and_failed_saves() {
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
        assert!(e.replace("a", "from outside".into()));
        assert_eq!(e.text(), "from outside");
        edit(&mut e, "mine", 0.0);
        assert!(!e.replace("a", "theirs".into()));
        assert!(!e.replace("b", "other".into()));
        // While the app asks, the autosave waits; Reload takes the file.
        e.hold(true);
        assert_eq!(e.due(99.0), None);
        assert_eq!(e.reload("a", "theirs".into()), None);
        assert_eq!(e.text(), "theirs");
        assert_eq!(e.status(), SaveStatus::Saved);
        assert!(!e.is_dirty());
        // A write that was in flight when the change was found lands on top
        // of it, so Reload writes the file back.
        edit(&mut e, "mine", 0.0);
        let late = e.due(1.0).unwrap();
        e.hold(true);
        e.saved("a", &late.content, true);
        let back = e.reload("a", "theirs".into()).unwrap();
        assert_eq!(back.content, "theirs");
        assert_eq!(e.status(), SaveStatus::Saving);
        e.saved("a", "theirs", true);
        assert_eq!(e.status(), SaveStatus::Saved);
        assert!(!e.is_dirty());
        // Keep Mine: the autosave carries on.
        edit(&mut e, "mine again", 0.0);
        e.hold(true);
        e.hold(false);
        assert_eq!(e.due(1.0).unwrap().content, "mine again");
    }

    const NOTE: &str =
        "---\ntags:\n  - a\ncreated: '2026-01-02T03:04:05.000Z'\n---\n# Title\n\nbody\n";

    fn note() -> NoteEditor {
        let mut e = NoteEditor::default();
        e.load("a", "Alpha", "alpha.md", NOTE.into());
        e
    }

    #[test]
    fn the_editor_holds_the_whole_file() {
        let mut e = note();
        assert_eq!(e.file_text(), NOTE);
        assert_eq!(e.text(), "# Title\n\nbody\n");
        assert_eq!(e.frontmatter()["tags"], json!(["a"]));
        assert!(!e.frontmatter_unreadable());
        assert!(!e.is_deck());
        // The caret starts on the text, below the block.
        assert_eq!(e.cursor(), (6, 1));
        assert_eq!(e.body_line(), 1);
        // A save writes the whole file.
        edit(&mut e, &format!("{NOTE}more\n"), 0.0);
        assert_eq!(e.due(1.0).unwrap().content, format!("{NOTE}more\n"));

        // A note with no frontmatter shows none.
        let mut plain = NoteEditor::default();
        plain.load("b", "Plain", "plain.md", "just text\n".into());
        assert_eq!(plain.text(), "just text\n");
        assert_eq!(plain.frontmatter(), &json!({}));
        assert_eq!((plain.cursor(), plain.body_line()), ((1, 1), 1));
    }

    #[test]
    fn a_half_typed_block_keeps_its_last_good_fields() {
        let mut e = note();
        let broken = NOTE.replace("  - a", "  - a\n  - [");
        edit(&mut e, &broken, 0.0);
        e.sync();
        assert!(e.frontmatter_unreadable());
        assert_eq!(e.frontmatter()["tags"], json!(["a"]));
        // The text is still the note's text, not the block.
        assert_eq!(e.text(), "# Title\n\nbody\n");
        // NoteLiner doesn't rewrite a block it can't read.
        assert!(!e.retag(&json!({ "tags": ["z"] })));
        assert_eq!(e.file_text(), broken);
        // Fixed: the new fields are read.
        edit(&mut e, &NOTE.replace("  - a", "  - a\n  - b"), 0.0);
        e.sync();
        assert!(!e.frontmatter_unreadable());
        assert_eq!(e.frontmatter()["tags"], json!(["a", "b"]));
    }

    #[test]
    fn the_app_rewrites_the_block_without_marking_the_note_edited() {
        let mut e = note();
        let entry = json!({ "tags": ["a", "work"], "createdAt": "2026-01-02T03:04:05.000Z" });
        assert!(e.retag(&entry));
        assert_eq!(
            e.file_text(),
            "---\ntags:\n  - a\n  - work\ncreated: '2026-01-02T03:04:05.000Z'\n---\n# Title\n\nbody\n"
        );
        assert_eq!(e.status(), SaveStatus::Saved);
        assert!(!e.is_dirty());
        assert_eq!(e.flush(), None);
        assert!(!e.state.can_undo());
        assert_eq!(e.frontmatter()["tags"], json!(["a", "work"]));
        assert!(!e.retag(&entry), "nothing to change the second time");

        // Presentation Settings add and remove the deck's block.
        assert!(e.set_presentation(&json!({ "theme": "dark" })));
        assert!(e.is_deck());
        assert_eq!(e.text(), "# Title\n\nbody\n");
        assert!(e.set_presentation(&Value::Null));
        assert!(!e.is_deck());
        assert!(!e.is_dirty());

        // A note with no block and no tags gets none.
        let mut plain = NoteEditor::default();
        plain.load("b", "Plain", "plain.md", "text\n".into());
        assert!(!plain.retag(&json!({ "tags": [] })));
        assert_eq!(plain.file_text(), "text\n");
    }

    #[test]
    fn a_block_a_save_put_back_appears_at_the_top() {
        let mut e = NoteEditor::default();
        e.load("b", "Plain", "plain.md", "text".into());
        edit(&mut e, "text!", 0.0);
        let save = e.due(1.0).unwrap();
        // The user keeps typing in the text while the save is out.
        edit(&mut e, "text!!", 1.1);
        let rewritten = "---\ntags: []\ncreated: '2026-01-02T03:04:05.000Z'\n---\ntext!";
        e.saved("b", &save.content, true);
        assert!(e.frontmatter_rewritten(&save.content, rewritten));
        assert_eq!(
            e.file_text(),
            "---\ntags: []\ncreated: '2026-01-02T03:04:05.000Z'\n---\ntext!!"
        );
        assert_eq!(e.text(), "text!!");
        // The later edit is still waiting to be saved, with the block.
        assert_eq!(e.due(2.0).unwrap().content, e.file_text());

        // A block typed in since the save went out is not overwritten.
        let mut e = note();
        let sent = e.file_text().to_string();
        edit(&mut e, &NOTE.replace("  - a", "  - mine"), 0.0);
        assert!(!e.frontmatter_rewritten(&sent, "---\ntags: []\n---\nx"));
        assert!(e.file_text().contains("  - mine"));
    }

    #[test]
    fn lines_given_and_taken_are_lines_of_the_text() {
        let mut e = note();
        // The outline's first heading is line 1 of the text, line 6 of the file.
        e.scroll_to_line(1);
        assert_eq!(e.state.status(&e.ctx.clone()).line, 6);
        e.caret = e.state.status(&e.ctx.clone());
        assert_eq!(e.body_line(), 1);
        assert_eq!(e.selected_lines(), (1, 1));
        e.scroll_to_line(3);
        e.caret = e.state.status(&e.ctx.clone());
        assert_eq!((e.cursor().0, e.body_line()), (8, 3));
        // In the block, the caret is on no line of the text.
        e.caret.line = 2;
        assert_eq!(e.body_line(), 0);
    }

    #[test]
    fn slide_edits_save_at_once() {
        let mut e = editor();
        let req = e.apply_slide_edit("# One\n\n---\n\n# Two\n".into(), Some(5));
        let req = req.expect("written at once");
        assert_eq!(req.content, "# One\n\n---\n\n# Two\n");
        assert_eq!(e.status(), SaveStatus::Saving);
        assert_eq!(e.apply_slide_edit(e.text().to_string(), None), None);

        // A deck's frontmatter stays above the slides.
        let mut e = note();
        let block = &NOTE[..NOTE.find("# Title").unwrap()];
        let req = e
            .apply_slide_edit("# One\n\n## Two\n".into(), Some(3))
            .unwrap();
        assert_eq!(req.content, format!("{block}# One\n\n## Two\n"));
        assert_eq!(e.text(), "# One\n\n## Two\n");
        assert_eq!(e.state.status(&e.ctx.clone()).line, 8);
    }
}
