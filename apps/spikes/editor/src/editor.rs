//! The Markdown editor widget: `TextEdit::multiline` with the cached
//! highlighting layouter, plus NoteLiner's list keys handled before
//! `TextEdit` sees them.
//!
//! List keys are applied to the buffer directly and the cursor written back
//! into `TextEditState`. Each one adds an undo point first, so it's a single
//! Ctrl+Z step, as `Editor.svelte` makes it with one CodeMirror transaction.

use egui::text::{CCursor, CCursorRange};
use egui::text_edit::TextEditOutput;
use egui::{Event, Id, Key, KeyboardShortcut, Modifiers, TextEdit, Ui};

use crate::highlight::Highlighter;
use crate::list_edits::{self, to_ordered_list, to_unordered_list};

/// A list command, from a key or the toolbar.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Enter,
    Indent,
    Outdent,
    Bulleted,
    Numbered,
    SubList,
}

const BULLETED: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Num8);
const NUMBERED: KeyboardShortcut =
    KeyboardShortcut::new(Modifiers::COMMAND.plus(Modifiers::SHIFT), Key::Num7);

pub struct MarkdownEditor {
    pub id: Id,
    pub highlighter: Highlighter,
    /// Lay out without highlighting (for comparison).
    pub plain: bool,
    /// A toolbar command to run on the next frame.
    pub pending: Option<Command>,
    /// An IME composition is open (preedit text showing). `TextEditState`
    /// tracks this too, but privately.
    composing: bool,
}

impl MarkdownEditor {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            highlighter: Highlighter::default(),
            plain: false,
            pending: None,
            composing: false,
        }
    }

    /// Draws the editor at the available width.
    pub fn show(&mut self, ui: &mut Ui, text: &mut String) -> TextEditOutput {
        if ui.memory(|m| m.has_focus(self.id)) {
            let ime_this_frame = self.track_ime(ui);
            let keys = if ime_this_frame || self.composing {
                None
            } else {
                key_command(ui)
            };
            if let Some(command) = self.pending.take().or(keys) {
                self.apply(ui, text, command);
            }
        }
        let (highlighter, plain) = (&mut self.highlighter, self.plain);
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            highlighter.layout(ui, buf.as_str(), wrap_width, plain)
        };
        let output = TextEdit::multiline(text)
            .id(self.id)
            .desired_width(f32::INFINITY)
            .lock_focus(true)
            .layouter(&mut layouter)
            .show(ui);
        // Nothing visible names the field, so name it for screen readers.
        output
            .response
            .response
            .clone()
            .accessible_name("Note text");
        output
    }

    /// Follows the IME composition; true when this frame has IME events.
    fn track_ime(&mut self, ui: &Ui) -> bool {
        ui.input(|i| {
            let mut any = false;
            for e in &i.events {
                if let Event::Ime(ime) = e {
                    any = true;
                    self.composing = match ime {
                        egui::ImeEvent::Preedit { text, .. } => !text.is_empty(),
                        _ => false,
                    };
                }
            }
            any
        })
    }

    /// Runs `command` against the buffer; consumes its key when it applied.
    fn apply(&mut self, ui: &Ui, text: &mut String, command: Command) {
        let Some(mut state) = TextEdit::load_state(ui.ctx(), self.id) else {
            return;
        };
        let Some(range) = state.cursor.char_range() else {
            return;
        };
        let (anchor, head) = (range.secondary.index.0, range.primary.index.0);
        let edit = match command {
            Command::Enter if anchor == head => list_edits::enter_in_text(text, head),
            Command::Enter => None,
            Command::Indent => list_edits::indent_in_text(text, anchor, head, false),
            Command::Outdent => list_edits::indent_in_text(text, anchor, head, true),
            Command::Bulleted => list_edits::span_in_text(text, anchor, head, to_unordered_list),
            Command::Numbered => list_edits::span_in_text(text, anchor, head, to_ordered_list),
            Command::SubList => list_edits::sub_list_in_text(text, head),
        };
        // Tab and Shift+Tab are always ours (TextEdit would insert a tab character).
        let consume = edit.is_some() || matches!(command, Command::Indent | Command::Outdent);
        if consume {
            ui.input_mut(|i| match command {
                Command::Enter => i.consume_key(Modifiers::NONE, Key::Enter),
                Command::Indent => i.consume_key(Modifiers::NONE, Key::Tab),
                Command::Outdent => i.consume_key(Modifiers::SHIFT, Key::Tab),
                Command::Bulleted => i.consume_shortcut(&BULLETED),
                Command::Numbered => i.consume_shortcut(&NUMBERED),
                Command::SubList => false,
            });
        }
        let Some(edit) = edit else { return };
        let mut undoer = state.undoer();
        undoer.add_undo(&(range, text.clone()));
        state.set_undoer(undoer);
        *text = edit.text;
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(edit.anchor),
            CCursor::new(edit.head),
        )));
        state.store(ui.ctx(), self.id);
    }
}

/// The list key pressed this frame, if any. The caller skips this while an
/// IME is composing: that Enter belongs to the IME.
fn key_command(ui: &Ui) -> Option<Command> {
    ui.input(|i| {
        let pressed = |key, mods: Modifiers| i.events.iter().any(|e| matches!(e, Event::Key { key: k, pressed: true, modifiers, .. } if *k == key && modifiers.matches_exact(mods)));
        if pressed(Key::Enter, Modifiers::NONE) {
            Some(Command::Enter)
        } else if pressed(Key::Tab, Modifiers::NONE) {
            Some(Command::Indent)
        } else if pressed(Key::Tab, Modifiers::SHIFT) {
            Some(Command::Outdent)
        } else if pressed(Key::Num8, BULLETED.modifiers) {
            Some(Command::Bulleted)
        } else if pressed(Key::Num7, NUMBERED.modifiers) {
            Some(Command::Numbered)
        } else {
            None
        }
    })
}
