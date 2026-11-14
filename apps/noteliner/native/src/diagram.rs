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
}

/// A diagram in a background tab.
pub struct DiagramStash {
    editor: editor::Stash,
    open: Open,
}

impl DiagramStash {
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
        let mut d = blockliner_core::read(text).map_err(|e| e.to_string())?;
        // An empty file gets the preferred grid, as a new diagram does.
        if d.shapes.is_empty() && d.connectors.is_empty() && text.trim().is_empty() {
            d.grid = self.grid_size;
        }
        self.open = Some(Open {
            id: id.to_string(),
            name: name.to_string(),
            filename: filename.to_string(),
            saved: d.clone(),
            dirty_since: None,
            saving: false,
        });
        self.editor.load(d);
        Ok(())
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
            Some(o) if o.dirty_since.is_some() => SaveStatus::Unsaved,
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
        if self.editor.diagram == o.saved {
            o.dirty_since = None;
            return None;
        }
        let since = *o.dirty_since.get_or_insert(now);
        if now - since < SAVE_DELAY_SECS || self.editor.canvas.is_busy() {
            return None;
        }
        self.flush()
    }

    /// A pending edit to write now (before switching tabs or quitting).
    pub fn flush(&mut self) -> Option<SaveRequest> {
        let o = self.open.as_mut()?;
        // A label still being typed is part of the diagram.
        self.editor.canvas.commit_label(&mut self.editor.diagram);
        o.dirty_since = None;
        if self.editor.diagram == o.saved {
            return None;
        }
        o.saved = self.editor.diagram.clone();
        o.saving = true;
        Some(SaveRequest {
            id: o.id.clone(),
            filename: o.filename.clone(),
            content: self.editor.text(),
        })
    }

    /// Whether an edit is waiting for its autosave.
    pub fn is_dirty(&self) -> bool {
        self.open.as_ref().is_some_and(|o| o.dirty_since.is_some())
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
    fn renames_follow_through() {
        let mut d = doc();
        d.renamed("d1", "Big Map", "big-map.mmd");
        d.editor.paste("x");
        assert_eq!(d.flush().unwrap().filename, "big-map.mmd");
        assert_eq!(d.name(), Some("Big Map"));
    }
}
