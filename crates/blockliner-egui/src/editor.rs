//! The diagram editor an app embeds: one diagram with its undo history,
//! canvas and active tool, and the glue between them (running an
//! [`EditAct`], copy and paste, the keys that aren't shortcuts).
//!
//! The app owns the file: it loads a diagram with [`DiagramEditor::load`],
//! writes [`DiagramEditor::text`] when it differs from what was saved, and
//! puts a background tab's state aside with [`DiagramEditor::take`]. Each
//! frame it calls, in order:
//!
//! 1. [`DiagramEditor::begin_frame`], then [`DiagramEditor::keys`] while a
//!    diagram is open and no dialog or text field has the keyboard;
//! 2. [`DiagramEditor::show`] where the canvas goes, and
//!    [`DiagramEditor::apply`] for its own buttons and shortcuts;
//! 3. [`DiagramEditor::observe`] once the frame's edits are in, and
//!    [`DiagramEditor::end_frame`] for the clipboard.

use blockliner_core::clipboard;
use blockliner_core::history::History;
use blockliner_core::style::{Agreement, Colour, Style, PRESETS};
use blockliner_core::Diagram;
use egui::Ui;

use crate::canvas::view::View;
use crate::canvas::{Canvas, MenuAction, Swatch, Tool};
use crate::commands::{EditAct, EditView};
use crate::dialogs::colour::ColourState;
use crate::properties::PropertiesAction;

/// A background tab's diagram: what [`DiagramEditor::take`] hands over and
/// [`DiagramEditor::restore`] takes back.
pub struct Stash {
    pub diagram: Diagram,
    pub history: History,
    pub view: View,
}

/// One diagram being edited.
pub struct DiagramEditor {
    pub diagram: Diagram,
    pub history: History,
    pub canvas: Canvas,
    pub tool: Tool,
    /// The shape the More Shapes button shows and picks.
    pub more_shape: Tool,
    /// The tool stays picked after drawing (it was chosen with Shift).
    sticky_tool: bool,
    /// Shift is down this frame.
    shift: bool,
    /// The last text copied or pasted, and how many times it's been pasted
    /// (each paste lands a step further).
    clip: Option<(String, usize)>,
    /// Text for the system clipboard at the end of the frame.
    copy_out: Option<String>,
    /// Ask the system for its clipboard at the end of the frame.
    paste_request: bool,
}

impl DiagramEditor {
    /// An editor showing `diagram` on `canvas`.
    pub fn new(canvas: Canvas, diagram: Diagram) -> Self {
        Self {
            history: History::new(&diagram),
            diagram,
            canvas,
            tool: Tool::default(),
            more_shape: Tool::MORE[0],
            sticky_tool: false,
            shift: false,
            clip: None,
            copy_out: None,
            paste_request: false,
        }
    }

    /// Puts `d` on the canvas as a fresh start: no selection, no undo.
    pub fn load(&mut self, d: Diagram) {
        self.canvas.clear(true);
        self.history.reset(&d);
        self.diagram = d;
    }

    /// The diagram as Mermaid, for the app to write.
    pub fn text(&self) -> String {
        blockliner_core::write(&self.diagram)
    }

    /// Hands over the diagram, its undo history and its view, leaving
    /// `blank` on a cleared canvas.
    pub fn take(&mut self, blank: Diagram) -> Stash {
        let history = std::mem::replace(&mut self.history, History::new(&blank));
        let stash = Stash {
            diagram: std::mem::replace(&mut self.diagram, blank),
            history,
            view: self.canvas.view,
        };
        self.canvas.clear(true);
        stash
    }

    /// Puts a diagram back as [`DiagramEditor::take`] left it; the selection
    /// starts empty.
    pub fn restore(&mut self, stash: Stash) {
        self.canvas.clear(false);
        self.canvas.view = stash.view;
        self.diagram = stash.diagram;
        self.history = stash.history;
    }

    /// What the edit commands' `when`s read.
    pub fn view(&self) -> EditView {
        let d = &self.diagram;
        let kinds = self.canvas.selected_kinds(d);
        let (straight, elbow) = self.canvas.selected_routes(d);
        let fillable = self.canvas.fillable(d);
        EditView {
            has_selection: self.canvas.has_selection(),
            straight_selected: straight,
            elbow_selected: elbow,
            stylable: self.canvas.stylable(d),
            fillable,
            labellable: self.canvas.can_edit_label(d),
            changeable: fillable,
            plain_line_selected: kinds.any_line(),
            arrow_selected: kinds.any_arrow(),
            can_undo: self.history.can_undo(),
            can_redo: self.history.can_redo(),
        }
    }

    /// The last text copied or pasted here.
    pub fn clip_text(&self) -> Option<&str> {
        self.clip.as_ref().map(|(text, _)| text.as_str())
    }

    /// Runs an edit. A Custom… fill or stroke returns the colour dialog for
    /// the app to show; its answer goes to [`DiagramEditor::apply_colour`].
    pub fn apply(&mut self, act: EditAct) -> Option<ColourState> {
        match act {
            EditAct::Tool(tool) => {
                self.tool = tool;
                self.sticky_tool = self.shift;
                if tool.is_more() {
                    self.more_shape = tool;
                }
            }
            EditAct::MoreShapes => {
                self.tool = self.more_shape;
                self.sticky_tool = self.shift;
            }
            EditAct::Undo => {
                self.canvas.cancel(&mut self.diagram);
                if self.history.undo(&mut self.diagram) {
                    // Line indices may now name other lines.
                    self.canvas.lines.clear();
                }
            }
            EditAct::Redo => {
                self.canvas.cancel(&mut self.diagram);
                if self.history.redo(&mut self.diagram) {
                    self.canvas.lines.clear();
                }
            }
            EditAct::SelectAll => self.canvas.select_all(&self.diagram),
            EditAct::SelectShapes => self.canvas.select_shapes(&self.diagram, false),
            EditAct::SelectLines => self.canvas.select_lines(&self.diagram),
            EditAct::SelectText => self.canvas.select_shapes(&self.diagram, true),
            EditAct::Nudge(dx, dy) => {
                let g = self.diagram.grid;
                self.canvas
                    .nudge(&mut self.diagram, f32::from(dx) * g, f32::from(dy) * g);
            }
            EditAct::Copy => {
                self.copy();
            }
            EditAct::Cut => {
                if self.copy() {
                    self.canvas.delete_selection(&mut self.diagram);
                }
            }
            EditAct::Paste => self.paste_request = true,
            EditAct::Delete => {
                self.canvas.delete_selection(&mut self.diagram);
            }
            EditAct::EditLabel => {
                self.tool = Tool::Select;
                self.canvas.edit_selected_label(&self.diagram);
            }
            EditAct::RouteElbow => {
                self.canvas.set_route(&mut self.diagram, true);
            }
            EditAct::RouteStraight => {
                self.canvas.set_route(&mut self.diagram, false);
            }
            EditAct::ChangeShape(kind) => {
                self.canvas.change_shape(&mut self.diagram, &kind);
            }
            EditAct::ChangeLine(tool) => {
                self.canvas.change_line(&mut self.diagram, tool);
            }
            EditAct::AddArrowheads => {
                self.canvas.set_arrowheads(&mut self.diagram, true);
            }
            EditAct::RemoveArrowheads => {
                self.canvas.set_arrowheads(&mut self.diagram, false);
            }
            EditAct::Fill(swatch) => return self.style(swatch, false),
            EditAct::Stroke(swatch) => return self.style(swatch, true),
            EditAct::StrokeStyle(dash) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_dash(dash.clone()));
            }
            EditAct::StrokeWeight(weight) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_weight(weight.clone()));
            }
            EditAct::TextWeight(bold) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_bold(bold));
            }
            EditAct::TextStyle(italic) => {
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_italic(italic));
            }
            // Normal when everything is Bold, else all Bold.
            EditAct::ToggleBold => {
                let all = self.canvas.selected_style(&self.diagram).bold == Agreement::Same(true);
                self.canvas
                    .style_selection(&mut self.diagram, true, |s| s.set_bold(!all));
            }
            EditAct::ResetStyle => {
                self.canvas
                    .style_selection(&mut self.diagram, true, Style::reset)
            }
            EditAct::Escape => self.canvas.escape(&mut self.diagram),
            EditAct::ZoomIn => self.canvas.zoom_step(1),
            EditAct::ZoomOut => self.canvas.zoom_step(-1),
            EditAct::ZoomReset => self.canvas.zoom_reset(),
        }
        None
    }

    /// What the Properties pane asked for, as [`DiagramEditor::apply`].
    pub fn apply_properties(&mut self, action: PropertiesAction) -> Option<ColourState> {
        let act = match action {
            PropertiesAction::Fill(s) => EditAct::Fill(s),
            PropertiesAction::Stroke(s) => EditAct::Stroke(s),
            PropertiesAction::FillHex(c) => {
                self.apply_colour(Some(c), false);
                return None;
            }
            PropertiesAction::StrokeHex(c) => {
                self.apply_colour(Some(c), true);
                return None;
            }
            PropertiesAction::StrokeStyle(d) => EditAct::StrokeStyle(d),
            PropertiesAction::StrokeWeight(w) => EditAct::StrokeWeight(w),
            PropertiesAction::TextWeight(b) => EditAct::TextWeight(b),
            PropertiesAction::TextStyle(i) => EditAct::TextStyle(i),
            PropertiesAction::ResetStyle => EditAct::ResetStyle,
            PropertiesAction::ChangeShape(kind) => EditAct::ChangeShape(kind),
            PropertiesAction::ChangeLine(tool) => EditAct::ChangeLine(tool),
        };
        self.apply(act)
    }

    /// The selection to the clipboard as Mermaid (`clipboard::copy`).
    /// `false` when nothing is selected.
    pub fn copy(&mut self) -> bool {
        let Some(clip) = clipboard::copy(&self.diagram, &self.canvas.selection, &self.canvas.lines)
        else {
            return false;
        };
        let text = blockliner_core::write(&clip);
        self.clip = Some((text.clone(), 0));
        self.copy_out = Some(text);
        true
    }

    /// Clipboard text into the diagram, selected: a flowchart (a copy, or
    /// one from elsewhere) where `clipboard::offset` puts it, else a text
    /// box in the middle of the view.
    pub fn paste(&mut self, text: &str) {
        if self.canvas.is_busy() {
            return;
        }
        let n = match &self.clip {
            Some((last, n)) if last == text => n + 1,
            _ => 1,
        };
        self.clip = Some((text.to_owned(), n));
        let view = self.canvas.visible();
        let (shapes, lines) = match clipboard::parse(text) {
            Some(clip) => {
                let (dx, dy) = clipboard::offset(&clip, n, view, self.diagram.grid);
                clipboard::paste(&mut self.diagram, &clip, dx, dy)
            }
            None => match clipboard::paste_text(&mut self.diagram, text, view.center()) {
                Some(id) => (vec![id], Vec::new()),
                None => return,
            },
        };
        self.canvas.select_only(shapes, lines);
        self.tool = Tool::Select;
    }

    /// The selected shapes' fill (or `stroke`): a preset, the default, or
    /// the Custom… dialog.
    fn style(&mut self, swatch: Swatch, stroke: bool) -> Option<ColourState> {
        let colour = match swatch {
            Swatch::Default => None,
            Swatch::Preset(i) => {
                let preset = PRESETS[i];
                Some(Colour::parse(if stroke {
                    preset.stroke
                } else {
                    preset.fill
                }))
            }
            Swatch::Custom => {
                // The first selected shape's (or, for a stroke, line's).
                let d = &self.diagram;
                let shapes = self
                    .canvas
                    .selection
                    .iter()
                    .filter_map(|id| d.shape(id).filter(|s| !s.external))
                    .map(|s| &s.style);
                let lines = self
                    .canvas
                    .lines
                    .iter()
                    .filter_map(|&i| d.connectors.get(i))
                    .filter(|_| stroke)
                    .map(|c| &c.style);
                let current = shapes.chain(lines).next().and_then(|s| {
                    if stroke {
                        s.stroke.as_ref()
                    } else {
                        s.fill.as_ref()
                    }
                });
                return Some(ColourState::new(stroke, current));
            }
        };
        self.apply_colour(colour, stroke);
        None
    }

    /// Sets the selection's fill (or `stroke`); `None` is the default.
    pub fn apply_colour(&mut self, colour: Option<Colour>, stroke: bool) {
        self.canvas.style_selection(&mut self.diagram, stroke, |s| {
            if stroke {
                s.set_stroke(colour.clone());
            } else {
                s.set_fill(colour.clone());
            }
        });
    }

    /// Reads this frame's modifiers (Shift makes a picked tool stay).
    pub fn begin_frame(&mut self, ctx: &egui::Context) {
        self.shift = ctx.input(|i| i.modifiers.shift);
    }

    /// The keys that aren't registered shortcuts: the arrow keys nudge the
    /// selection, and egui's copy, cut and paste events. Call it only while
    /// a diagram is open and no dialog or text field has the keyboard (a
    /// label being edited keeps them).
    pub fn keys(&mut self, ctx: &egui::Context) {
        self.nudge_keys(ctx);
        self.clipboard_events(ctx);
    }

    /// egui's copy, cut and paste events: the platform's keys, or the
    /// clipboard a Paste command asked for.
    fn clipboard_events(&mut self, ctx: &egui::Context) {
        let events = ctx.input_mut(|i| {
            let mut out = Vec::new();
            i.events.retain(|e| match e {
                egui::Event::Copy | egui::Event::Cut | egui::Event::Paste(_) => {
                    out.push(e.clone());
                    false
                }
                _ => true,
            });
            out
        });
        for e in events {
            match e {
                egui::Event::Copy => {
                    self.apply(EditAct::Copy);
                }
                egui::Event::Cut => {
                    self.apply(EditAct::Cut);
                }
                egui::Event::Paste(text) => self.paste(&text),
                _ => {}
            }
        }
    }

    /// The arrow keys nudge the selection: one grid step, five with Shift,
    /// one pixel with Alt. Read from the key events themselves, which carry
    /// their modifiers.
    fn nudge_keys(&mut self, ctx: &egui::Context) {
        if !self.canvas.has_selection() {
            return;
        }
        let grid = self.diagram.grid;
        let moves: Vec<(f32, f32)> = ctx.input_mut(|i| {
            let mut moves = Vec::new();
            i.events.retain(|e| {
                let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } = e
                else {
                    return true;
                };
                let (dx, dy) = match key {
                    egui::Key::ArrowLeft => (-1.0, 0.0),
                    egui::Key::ArrowRight => (1.0, 0.0),
                    egui::Key::ArrowUp => (0.0, -1.0),
                    egui::Key::ArrowDown => (0.0, 1.0),
                    _ => return true,
                };
                let step = if modifiers.alt {
                    1.0
                } else if modifiers.shift {
                    5.0 * grid
                } else {
                    grid
                };
                moves.push((dx * step, dy * step));
                false
            });
            moves
        });
        for (dx, dy) in moves {
            self.canvas.nudge(&mut self.diagram, dx, dy);
        }
    }

    /// The canvas, filling `ui`. `keys`: no dialog or text field has the
    /// keyboard, so Space may pan. Returns the right-click menu's choice,
    /// for the app to run with [`DiagramEditor::apply`] once its panels are
    /// drawn.
    pub fn show(&mut self, ui: &mut Ui, keys: bool) -> Option<EditAct> {
        let out = self.canvas.show(ui, &mut self.diagram, self.tool, keys);
        if out.tool_done && !self.sticky_tool {
            self.tool = Tool::Select;
        }
        out.menu.map(|m| match m {
            MenuAction::EditLabel => EditAct::EditLabel,
            MenuAction::ChangeShape(kind) => EditAct::ChangeShape(kind),
            MenuAction::AddArrowheads => EditAct::AddArrowheads,
            MenuAction::RemoveArrowheads => EditAct::RemoveArrowheads,
            MenuAction::Cut => EditAct::Cut,
            MenuAction::Copy => EditAct::Copy,
            MenuAction::Paste => EditAct::Paste,
            MenuAction::RouteElbow => EditAct::RouteElbow,
            MenuAction::RouteStraight => EditAct::RouteStraight,
            MenuAction::Fill(s) => EditAct::Fill(s),
            MenuAction::Stroke(s) => EditAct::Stroke(s),
            MenuAction::StrokeStyle(d) => EditAct::StrokeStyle(d),
            MenuAction::StrokeWeight(w) => EditAct::StrokeWeight(w),
            MenuAction::TextWeight(b) => EditAct::TextWeight(b),
            MenuAction::TextStyle(i) => EditAct::TextStyle(i),
            MenuAction::ResetStyle => EditAct::ResetStyle,
            MenuAction::Delete => EditAct::Delete,
            MenuAction::SelectAll => EditAct::SelectAll,
            MenuAction::SelectShapes => EditAct::SelectShapes,
            MenuAction::SelectLines => EditAct::SelectLines,
            MenuAction::SelectText => EditAct::SelectText,
        })
    }

    /// One undo step per finished change (see `History`). `true` when no
    /// gesture is in progress, which is when the app checks its autosave.
    pub fn observe(&mut self) -> bool {
        if self.canvas.is_busy() {
            return false;
        }
        self.history.observe(&self.diagram);
        true
    }

    /// Sends what was copied to the system clipboard, and asks for its text
    /// when a Paste command wants it.
    pub fn end_frame(&mut self, ctx: &egui::Context) {
        if let Some(text) = self.copy_out.take() {
            ctx.copy_text(text);
        }
        if std::mem::take(&mut self.paste_request) {
            ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::Unit;

    fn editor() -> DiagramEditor {
        DiagramEditor::new(Canvas::new(true, false, Unit::default()), Diagram::new())
    }

    #[test]
    fn an_edit_is_one_undo_step_and_survives_a_stash() {
        let mut ed = editor();
        // Pasted text that isn't a flowchart becomes a text box, selected.
        ed.paste("hello");
        assert_eq!(ed.diagram.shapes.len(), 1);
        assert!(ed.view().has_selection);
        assert!(ed.observe());
        assert!(ed.view().can_undo);
        assert!(ed.text().contains("hello"));

        // Put aside and brought back: the diagram, its history and its view.
        ed.canvas.view.zoom = 2.0;
        let stash = ed.take(Diagram::new());
        assert!(ed.diagram.shapes.is_empty() && !ed.view().can_undo);
        ed.restore(stash);
        assert_eq!(ed.diagram.shapes.len(), 1);
        assert_eq!(ed.canvas.view.zoom, 2.0);
        assert!(!ed.view().has_selection);

        assert!(ed.apply(EditAct::Undo).is_none());
        assert!(ed.diagram.shapes.is_empty());
        assert!(ed.view().can_redo);
        ed.apply(EditAct::Redo);
        assert_eq!(ed.diagram.shapes.len(), 1);
    }

    #[test]
    fn a_custom_colour_asks_for_the_dialog() {
        let mut ed = editor();
        ed.paste("hello");
        assert!(ed.apply(EditAct::Fill(Swatch::Custom)).is_some());
        assert!(ed.apply(EditAct::Fill(Swatch::Default)).is_none());
    }
}
