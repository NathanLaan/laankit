//! The open diagram: `blockliner-egui`'s editor wrapped with the app's
//! autosave and status contract, as [`crate::editor::NoteEditor`] wraps the
//! note editor.
//!
//! - **Loading.** The app calls [`DiagramDoc::load`] when a diagram's
//!   `file:read` arrives. A diagram's file is the whole diagram: BlockLiner's
//!   Mermaid flowchart, with no frontmatter.
//! - **Autosave.** An edit that changes the diagram starts a 500 ms timer;
//!   when it fires, [`DiagramDoc::due`] returns the write. Before switching
//!   tabs the app calls [`DiagramDoc::flush`] and writes what it returns.
//! - **Changes on disk.** With no edit pending, [`DiagramDoc::replace`]
//!   takes the file as one undo step. With one, or when the backend refused
//!   a save ([`DiagramDoc::conflicted`]), the app asks:
//!   [`DiagramDoc::hold`] stops the autosave until it has an answer,
//!   [`DiagramDoc::reload`] takes the file, and `hold(false)` then
//!   [`DiagramDoc::flush`] writes the diagram over it.
//! - **Tabs.** [`DiagramDoc::take`] puts the diagram, its undo history and
//!   its view aside, and [`DiagramDoc::restore`] brings them back.

use blockliner_core::Diagram;
use blockliner_egui::canvas::Canvas;
use blockliner_egui::editor::{self, DiagramEditor};
use blockliner_egui::properties::PropertiesState;

use crate::config::Prefs;
use crate::editor::{SaveRequest, SaveStatus, SAVE_DELAY_SECS};

/// The diagram file being edited.
struct Open {
    id: String,
    name: String,
    filename: String,
    /// The diagram as last written.
    saved: Diagram,
    /// When it last differed from `saved` (egui time), while a save waits.
    dirty_since: Option<f64>,
    /// A write is in flight.
    saving: bool,
    /// The backend refused the write of `saved`, because the file changed
    /// on disk: `saved` is not what the file holds.
    unwritten: bool,
    /// The autosave waits (the app is asking about a change on disk).
    held: bool,
    /// A write was in flight when the hold began: it lands on top of the
    /// change on disk that the app is asking about.
    held_over_write: bool,
}

impl Open {
    fn new(id: &str, name: &str, filename: &str, saved: Diagram) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            filename: filename.to_string(),
            saved,
            dirty_since: None,
            saving: false,
            unwritten: false,
            held: false,
            held_over_write: false,
        }
    }

    /// An edit is waiting to be written, or being written.
    fn is_dirty(&self) -> bool {
        self.dirty_since.is_some() || self.saving || self.unwritten
    }

    fn hold(&mut self, held: bool) {
        self.held = held;
        self.held_over_write = held && self.saving;
    }

    /// The backend did not write the diagram: another program changed the
    /// file first. False if no write of this diagram was in flight.
    fn conflicted(&mut self, id: &str) -> bool {
        if self.id != id || !self.saving {
            return false;
        }
        self.saving = false;
        self.unwritten = true;
        self.held = true;
        self.held_over_write = false;
        true
    }

    /// The save that was sent and has no answer yet.
    fn in_flight(&self) -> Option<SaveRequest> {
        self.saving.then(|| SaveRequest {
            id: self.id.clone(),
            filename: self.filename.clone(),
            content: blockliner_core::write(&self.saved),
        })
    }

    /// The file holds `d`, and so does the canvas.
    fn in_sync(&mut self, d: Diagram) {
        self.saved = d;
        self.dirty_since = None;
        self.saving = false;
        self.unwritten = false;
        self.held = false;
        self.held_over_write = false;
    }

    /// The write of `d` as `content`, over whatever is on disk.
    fn write(&mut self, d: Diagram, content: String) -> SaveRequest {
        self.saved = d;
        self.dirty_since = None;
        self.saving = true;
        self.unwritten = false;
        SaveRequest {
            id: self.id.clone(),
            filename: self.filename.clone(),
            content,
        }
    }
}

/// A diagram in a background tab.
pub struct DiagramStash {
    editor: editor::Stash,
    open: Open,
}

/// Reads a diagram's file. An empty file gets `grid`, the preferred grid, as
/// a new diagram does.
fn parse(text: &str, grid: f32) -> Result<Diagram, String> {
    let mut d = blockliner_core::read(text).map_err(|e| e.to_string())?;
    if d.shapes.is_empty() && d.connectors.is_empty() && text.trim().is_empty() {
        d.grid = grid;
    }
    Ok(d)
}

impl DiagramStash {
    pub fn file_id(&self) -> &str {
        &self.open.id
    }

    /// The file changed on disk while the tab was in the background: the
    /// new diagram is one undo step. Text BlockLiner can't read is refused,
    /// and nothing changes.
    pub fn replace(&mut self, text: &str, grid: f32) -> Result<(), String> {
        let d = parse(text, grid)?;
        self.editor.replace(d.clone());
        self.open.saved = d;
        self.open.dirty_since = None;
        Ok(())
    }

    /// A save is waiting for an answer about a change on disk.
    pub fn is_dirty(&self) -> bool {
        self.open.is_dirty()
    }

    pub fn name(&self) -> &str {
        &self.open.name
    }

    /// As [`DiagramDoc::conflicted`], for a tab left before its save was
    /// refused.
    pub fn conflicted(&mut self, id: &str) -> bool {
        self.open.conflicted(id)
    }

    /// The save that was sent and has no answer yet.
    pub fn in_flight(&self) -> Option<SaveRequest> {
        self.open.in_flight()
    }

    /// The tab is closing with no answer: the diagram is written over the
    /// file.
    pub fn keep_mine(&mut self) -> Option<SaveRequest> {
        self.open.hold(false);
        if !self.open.unwritten && self.editor.diagram == self.open.saved {
            return None;
        }
        let content = blockliner_core::write(&self.editor.diagram);
        Some(self.open.write(self.editor.diagram.clone(), content))
    }

    /// Bytes held by the undo history.
    pub fn history_bytes(&self) -> usize {
        self.editor.history.bytes()
    }

    /// Drops the oldest undo steps until at most `max` bytes are held.
    pub fn trim_history(&mut self, max: usize) {
        self.editor.history.trim_to(max);
    }

    pub fn renamed(&mut self, id: &str, name: &str, filename: &str) {
        if self.open.id == id {
            self.open.name = name.to_string();
            self.open.filename = filename.to_string();
        }
    }

    pub fn saved(&mut self, id: &str) {
        if self.open.id == id {
            self.open.saving = false;
        }
    }
}

/// The diagram editor and the file it holds.
pub struct DiagramDoc {
    pub editor: DiagramEditor,
    pub properties: PropertiesState,
    open: Option<Open>,
    /// The grid size of a new, empty diagram.
    grid_size: f32,
}

impl DiagramDoc {
    pub fn new(prefs: &Prefs) -> Self {
        let canvas = Canvas::new(
            prefs.diagram_show_grid,
            prefs.diagram_show_dimensions,
            prefs.diagram_grid_unit,
        );
        let mut doc = Self {
            editor: DiagramEditor::new(canvas, Diagram::new()),
            properties: PropertiesState::default(),
            open: None,
            grid_size: prefs.diagram_grid_size,
        };
        doc.editor.load(doc.blank());
        doc
    }

    /// The Diagrams settings changed.
    pub fn set_prefs(&mut self, prefs: &Prefs) {
        self.editor.canvas.show_grid = prefs.diagram_show_grid;
        self.editor.canvas.show_dimensions = prefs.diagram_show_dimensions;
        self.editor.canvas.unit = prefs.diagram_grid_unit;
        self.grid_size = prefs.diagram_grid_size;
    }

    fn blank(&self) -> Diagram {
        Diagram {
            grid: self.grid_size,
            ..Diagram::new()
        }
    }

    pub fn file_id(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.id.as_str())
    }

    pub fn name(&self) -> Option<&str> {
        self.open.as_ref().map(|o| o.name.as_str())
    }

    /// A freshly read diagram: in sync with disk, with no undo history.
    /// Text BlockLiner can't read is refused, and nothing changes.
    pub fn load(&mut self, id: &str, name: &str, filename: &str, text: &str) -> Result<(), String> {
        let d = parse(text, self.grid_size)?;
        self.open = Some(Open::new(id, name, filename, d.clone()));
        self.editor.load(d);
        Ok(())
    }

    /// The open diagram `id` changed on disk, with no edit pending: the new
    /// diagram is one undo step, and the view and the history stay. Text
    /// BlockLiner can't read is refused, and nothing changes. True if the
    /// canvas changed.
    pub fn replace(&mut self, id: &str, text: &str) -> Result<bool, String> {
        let grid = self.grid_size;
        let Some(o) = self.open.as_mut().filter(|o| o.id == id) else {
            return Ok(false);
        };
        let d = parse(text, grid)?;
        let changed = d != self.editor.diagram;
        if changed {
            self.editor.replace(d.clone());
        }
        o.saved = d;
        o.dirty_since = None;
        Ok(changed)
    }

    /// Stops (or restarts) the autosave, while the app asks what to do about
    /// a change on disk.
    pub fn hold(&mut self, held: bool) {
        if let Some(o) = &mut self.open {
            o.hold(held);
        }
    }

    /// The backend did not write diagram `id`: another program changed the
    /// file first. The edit is unsaved again, and the autosave waits until
    /// the app has an answer. False if no write of it was in flight.
    pub fn conflicted(&mut self, id: &str) -> bool {
        self.open.as_mut().is_some_and(|o| o.conflicted(id))
    }

    /// The save that was sent and has no answer yet.
    pub fn in_flight(&self) -> Option<SaveRequest> {
        self.open.as_ref()?.in_flight()
    }

    /// The file on disk reads as the diagram on the canvas: there is
    /// nothing to ask, and the two are in sync.
    pub fn settle_if_same(&mut self, id: &str, text: &str) -> bool {
        let grid = self.grid_size;
        let Some(o) = self.open.as_mut().filter(|o| o.id == id) else {
            return false;
        };
        match parse(text, grid) {
            Ok(d) if d == self.editor.diagram => {
                o.in_sync(d);
                true
            }
            _ => false,
        }
    }

    /// Takes the file from disk over a pending edit (the user chose Reload),
    /// as one undo step. Text BlockLiner can't read is refused, and nothing
    /// changes. If a write of the edit was already in flight when the change
    /// was found, it has landed on top of the file: the file's text is
    /// returned to be written back.
    pub fn reload(&mut self, id: &str, text: &str) -> Result<Option<SaveRequest>, String> {
        let grid = self.grid_size;
        let Some(o) = self.open.as_mut().filter(|o| o.id == id) else {
            return Ok(None);
        };
        let d = parse(text, grid)?;
        if d != self.editor.diagram {
            self.editor.replace(d.clone());
        }
        let write_back = o.held_over_write;
        o.in_sync(d.clone());
        Ok(write_back.then(|| o.write(d, text.to_string())))
    }

    /// The grid size of a new, empty diagram.
    pub fn grid_size(&self) -> f32 {
        self.grid_size
    }

    /// Bytes held by the undo history.
    pub fn history_bytes(&self) -> usize {
        self.editor.history.bytes()
    }

    /// Drops the oldest undo steps until at most `max` bytes are held.
    pub fn trim_history(&mut self, max: usize) {
        self.editor.history.trim_to(max);
    }

    /// No diagram open.
    pub fn clear(&mut self) {
        if self.open.take().is_some() {
            let blank = self.blank();
            self.editor.load(blank);
        }
    }

    /// The diagram was renamed: later saves go to its new file.
    pub fn renamed(&mut self, id: &str, name: &str, filename: &str) {
        if let Some(o) = self.open.as_mut().filter(|o| o.id == id) {
            o.name = name.to_string();
            o.filename = filename.to_string();
        }
    }

    pub fn status(&self) -> SaveStatus {
        match &self.open {
            Some(o) if o.dirty_since.is_some() || o.unwritten => SaveStatus::Unsaved,
            Some(o) if o.saving => SaveStatus::Saving,
            _ => SaveStatus::Saved,
        }
    }

    /// The backend finished a write of diagram `id`.
    pub fn saved(&mut self, id: &str) {
        if let Some(o) = self.open.as_mut().filter(|o| o.id == id) {
            o.saving = false;
        }
    }

    /// Notes that the diagram changed (call once the frame's edits are in
    /// and no gesture is in progress), and returns the write once it has
    /// been still for 500 ms.
    pub fn due(&mut self, now: f64) -> Option<SaveRequest> {
        let o = self.open.as_mut()?;
        if self.editor.diagram == o.saved && !o.unwritten {
            o.dirty_since = None;
            return None;
        }
        let since = *o.dirty_since.get_or_insert(now);
        if o.held || now - since < SAVE_DELAY_SECS || self.editor.canvas.is_busy() {
            return None;
        }
        self.flush()
    }

    /// A pending edit to write now (before switching tabs or quitting).
    /// Nothing is written while the app asks about a change on disk: the
    /// edit stays pending.
    pub fn flush(&mut self) -> Option<SaveRequest> {
        let o = self.open.as_mut()?;
        // A label still being typed is part of the diagram.
        self.editor.canvas.commit_label(&mut self.editor.diagram);
        if o.held {
            return None;
        }
        o.dirty_since = None;
        if self.editor.diagram == o.saved && !o.unwritten {
            return None;
        }
        let content = self.editor.text();
        Some(o.write(self.editor.diagram.clone(), content))
    }

    /// An edit is waiting to be written, being written, or being made (a
    /// drag or a label being typed).
    pub fn is_dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| {
            o.is_dirty() || self.editor.canvas.is_busy() || self.editor.diagram != o.saved
        })
    }

    /// Puts the open diagram aside for a background tab ([`DiagramDoc::flush`]
    /// first: only the active tab has unsaved edits).
    pub fn take(&mut self) -> Option<DiagramStash> {
        let open = self.open.take()?;
        let blank = self.blank();
        Some(DiagramStash {
            editor: self.editor.take(blank),
            open,
        })
    }

    /// Brings a background tab's diagram back.
    pub fn restore(&mut self, stash: DiagramStash) {
        self.editor.restore(stash.editor);
        self.open = Some(stash.open);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blockliner_core::{Rect, ShapeKind};
    use blockliner_egui::commands::EditAct;

    const FLOW: &str = "flowchart LR\n  a[\"Alpha\"]\n";

    fn doc() -> DiagramDoc {
        let mut d = DiagramDoc::new(&Prefs::default());
        d.load("d1", "Map", "map.mmd", FLOW).unwrap();
        d
    }

    #[test]
    fn autosave_waits_500ms_then_saves_once() {
        let mut d = doc();
        assert_eq!(d.due(0.0), None);
        assert_eq!(d.status(), SaveStatus::Saved);
        d.editor.paste("note");
        assert_eq!(d.due(1.0), None);
        assert_eq!(d.status(), SaveStatus::Unsaved);
        assert_eq!(d.due(1.3), None);
        let save = d.due(1.6).expect("due");
        assert_eq!(
            (save.id.as_str(), save.filename.as_str()),
            ("d1", "map.mmd")
        );
        assert!(save.content.contains("Alpha") && save.content.contains("note"));
        assert_eq!(d.status(), SaveStatus::Saving);
        assert_eq!(d.due(5.0), None, "nothing new to save");
        d.saved("d1");
        assert_eq!(d.status(), SaveStatus::Saved);
    }

    #[test]
    fn flush_before_switching_and_stash() {
        let mut d = doc();
        d.editor.paste("note");
        d.editor.observe();
        let f = d.flush().expect("a pending edit");
        assert!(f.content.contains("note"));
        assert_eq!(d.flush(), None);

        let stash = d.take().expect("open");
        assert_eq!(d.file_id(), None);
        assert!(d.editor.diagram.shapes.is_empty());
        d.restore(stash);
        assert_eq!(d.file_id(), Some("d1"));
        assert_eq!(d.editor.diagram.shapes.len(), 2);
        assert!(d.editor.view().can_undo);
    }

    #[test]
    fn text_that_is_not_a_diagram_is_refused() {
        let mut d = doc();
        assert!(d.load("d2", "Bad", "bad.mmd", "# A note").is_err());
        assert_eq!(d.file_id(), Some("d1"));
    }

    #[test]
    fn a_change_on_disk_is_one_undo_step() {
        let mut d = doc();
        d.editor
            .diagram
            .add_shape(ShapeKind::Rect, Rect::new(0.0, 200.0, 80.0, 40.0), "Mine");
        d.editor.history.observe(&d.editor.diagram);
        let _ = d.flush();
        d.saved("d1");
        let mine = d.editor.diagram.clone();
        // Another program rewrites the file.
        let mut theirs = mine.clone();
        theirs.add_shape(
            ShapeKind::Rect,
            Rect::new(200.0, 200.0, 80.0, 40.0),
            "Theirs",
        );
        d.replace("d1", &blockliner_core::write(&theirs)).unwrap();
        assert_eq!(d.editor.diagram.shapes.len(), mine.shapes.len() + 1);
        assert_eq!(d.status(), SaveStatus::Saved, "in sync with disk");
        assert!(d.due(0.0).is_none());
        // Undo shows the diagram as it was, and the step before still works.
        d.editor.apply(EditAct::Undo);
        assert_eq!(d.editor.diagram, mine);
        d.editor.apply(EditAct::Undo);
        assert_eq!(d.editor.diagram.shapes.len(), mine.shapes.len() - 1);
        // Text that isn't a diagram is refused and nothing changes.
        let before = d.editor.diagram.clone();
        assert!(d.replace("d1", "# A note").is_err());
        assert_eq!(d.editor.diagram, before);

        // The same for a tab in the background.
        let mut d = doc();
        let shapes = d.editor.diagram.shapes.len();
        let mut stash = d.take().unwrap();
        stash
            .replace(&blockliner_core::write(&theirs), 10.0)
            .unwrap();
        assert!(stash.history_bytes() > 0);
        d.restore(stash);
        assert_eq!(d.editor.diagram.shapes.len(), theirs.shapes.len());
        d.editor.apply(EditAct::Undo);
        assert_eq!(d.editor.diagram.shapes.len(), shapes);
    }

    #[test]
    fn a_refused_save_waits_for_an_answer() {
        let mut d = doc();
        d.editor.paste("mine");
        let mine = d.editor.diagram.clone();
        let mut theirs = blockliner_core::read(FLOW).unwrap();
        theirs.add_shape(ShapeKind::Rect, Rect::new(0.0, 200.0, 80.0, 40.0), "Theirs");
        let theirs_text = blockliner_core::write(&theirs);

        // The backend refused the write: unsaved, and the autosave waits.
        assert!(!d.conflicted("d1"), "no write in flight");
        assert!(d.flush().is_some());
        assert!(!d.conflicted("d2"));
        assert!(d.conflicted("d1"));
        assert_eq!(d.status(), SaveStatus::Unsaved);
        assert!(d.is_dirty());
        assert_eq!(d.due(99.0), None);
        assert_eq!(d.flush(), None, "nothing is written while it asks");
        assert!(!d.settle_if_same("d1", &theirs_text));

        // A file that can't be read is refused, and it still asks.
        assert!(d.reload("d1", "# A note").is_err());
        assert_eq!(d.editor.diagram, mine);
        assert!(d.is_dirty());

        // Reload takes the file as one undo step.
        assert_eq!(d.reload("d1", &theirs_text), Ok(None));
        assert_eq!(d.editor.diagram, theirs);
        assert_eq!(d.status(), SaveStatus::Saved);
        assert!(!d.is_dirty());
        assert_eq!(d.due(99.0), None);
        d.editor.apply(EditAct::Undo);
        assert_eq!(d.editor.diagram, mine);

        // Keep Mine writes the diagram over the file.
        let mut d = doc();
        d.editor.paste("mine");
        let _ = d.flush();
        assert!(d.conflicted("d1"));
        d.hold(false);
        assert!(d.flush().unwrap().content.contains("mine"));
        d.saved("d1");
        assert_eq!(d.status(), SaveStatus::Saved);

        // The file reads as the diagram on the canvas: nothing to ask.
        let mut d = doc();
        d.editor.paste("mine");
        let text = d.flush().unwrap().content;
        assert!(d.conflicted("d1"));
        assert!(d.settle_if_same("d1", &text));
        assert_eq!(d.status(), SaveStatus::Saved);
        assert_eq!(d.due(99.0), None);

        // A write in flight when the change was found lands on top of it,
        // so Reload writes the file back.
        let mut d = doc();
        d.editor.paste("mine");
        let _ = d.flush();
        assert!(d.is_dirty(), "a write in flight is an unsaved edit");
        d.hold(true);
        d.saved("d1");
        let back = d.reload("d1", &theirs_text).unwrap().unwrap();
        assert_eq!(back.content, theirs_text);
        assert_eq!(d.status(), SaveStatus::Saving);
        d.saved("d1");
        assert_eq!(d.status(), SaveStatus::Saved);

        // A tab left before its save was refused, then closed: Keep Mine.
        let mut d = doc();
        d.editor.paste("mine");
        let _ = d.flush();
        let mut stash = d.take().unwrap();
        assert!(stash.conflicted("d1"));
        assert!(stash.is_dirty());
        assert!(stash.keep_mine().unwrap().content.contains("mine"));
        assert_eq!(stash.keep_mine(), None);
    }

    #[test]
    fn renames_follow_through() {
        let mut d = doc();
        d.renamed("d1", "Big Map", "big-map.mmd");
        d.editor.paste("x");
        assert_eq!(d.flush().unwrap().filename, "big-map.mmd");
        assert_eq!(d.name(), Some("Big Map"));
    }
}
