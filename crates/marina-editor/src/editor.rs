//! [`Editor`]: the widget. `TextEdit::multiline` over [`EditorState`]'s
//! text with the cached highlighting layouter, plus what `Editor.svelte` and
//! CodeMirror's `basicSetup` add around it: list keys, emphasis toggles,
//! bracket auto-closing, the line-number gutter and active line, find and
//! replace, `[[` completion, the context menu, and the attachment and link
//! hooks the host handles.
//!
//! Keys the editor handles itself are taken out of the input before
//! `TextEdit` sees them, so each is one undoable step of our own
//! [`crate::history::History`]. `TextEdit`'s own undoer is emptied every
//! frame: it would keep whole-text snapshots.

use std::path::PathBuf;
use std::sync::Arc;

use egui::text::CCursor;
use egui::{
    pos2, vec2, Align, Align2, Color32, ColorImage, Event, FontFamily, FontId, Frame, Key, Margin,
    Modifiers, Order, Pos2, Rect, Response, RichText, ScrollArea, Shape, Stroke, TextEdit, Ui,
};
use marina_egui::{icons, theme};

use crate::edits;
use crate::find;
use crate::highlight::{link_at, EditorColors, LayoutOptions, LinkTarget};
use crate::list_edits::{self, byte_at, chars_in, to_ordered_list, to_unordered_list, DocEdit};
use crate::spell::{self, SpellChecker};
use crate::state::{Command, Completion, EditorState};

/// Padding around the text (`.cm-content { padding: 16px 0 }`, `.cm-line { padding: 0 16px }`).
const PAD: f32 = 16.0;
/// The gutter's minimum width (`.cm-lineNumbers { minWidth: 44px }`).
const GUTTER_MIN: f32 = 44.0;
/// Completion entries shown at most (`.slice(0, 50)`).
const MAX_COMPLETIONS: usize = 50;

/// Which document format the context menu's "Save to …" asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SaveFormat {
    Html,
    Pdf,
    Markdown,
}

/// Something for the host to handle.
#[derive(Clone, Debug)]
pub enum EditorEvent {
    /// The user changed the text (typing, a command, paste…). Debounce and
    /// save (`scheduleSave`, 500 ms).
    Changed,
    /// An image was pasted (no text on the clipboard). Save it as an
    /// attachment, then insert [`crate::attachment_markdown`] with
    /// [`Command::Insert`].
    PasteImage(Arc<ColorImage>),
    /// Files were pasted (the clipboard held only `file://` URIs).
    PasteFiles(Vec<PathBuf>),
    /// Files were dropped on the editor (their paths). Save them as
    /// attachments and insert their Markdown, as for [`Self::PasteImage`].
    DropFiles(Vec<PathBuf>),
    /// Ctrl/Cmd+click on a Markdown link: its URL as written.
    OpenLink(String),
    /// Ctrl/Cmd+click on a `[[wikilink]]`: the page name (without an alias).
    OpenWikilink(String),
    /// The context menu's "Save to HTML/PDF/Markdown".
    Save(SaveFormat),
    /// "Add to dictionary" on a misspelled word. Add it to the checker (and
    /// the user's word list), then call [`EditorState::refresh_spelling`].
    AddToDictionary(String),
}

/// What [`Editor::show`] reports.
pub struct EditorOutput {
    pub events: Vec<EditorEvent>,
    /// The text area's response.
    pub response: Response,
    /// The character under the pointer, while something is being dragged or
    /// on the frame of a click or release (it isn't computed on other frames:
    /// it costs a walk over the rows). Where to [`Command::InsertAt`] something
    /// dropped from elsewhere in the app (`interceptInternalDrops`).
    pub char_at_pointer: Option<usize>,
}

/// The editor widget, drawn into the space it's given. Build one per frame.
pub struct Editor<'a> {
    state: &'a mut EditorState,
    accessible_name: &'a str,
    context_menu: bool,
}

impl<'a> Editor<'a> {
    pub fn new(state: &'a mut EditorState) -> Self {
        Self {
            state,
            accessible_name: "Note text",
            context_menu: true,
        }
    }

    /// The text area's name for screen readers (default "Note text").
    pub fn accessible_name(mut self, name: &'a str) -> Self {
        self.accessible_name = name;
        self
    }

    /// Whether right-click opens the editor's context menu (default true).
    pub fn context_menu(mut self, on: bool) -> Self {
        self.context_menu = on;
        self
    }

    pub fn show(self, ui: &mut Ui) -> EditorOutput {
        let Self {
            state,
            accessible_name,
            context_menu,
        } = self;
        let ctx = ui.ctx().clone();
        let colors = EditorColors::from_palette(&theme::palette(&ctx));
        let time = ui.input(|i| i.time);
        let mut events = Vec::new();
        if std::mem::take(&mut state.focus_request) {
            ctx.memory_mut(|m| m.request_focus(state.id));
        }

        for command in std::mem::take(&mut state.pending) {
            state.exec(&ctx, command, time, &mut events);
        }
        let focused = ctx.memory(|m| m.has_focus(state.id));
        let was_composing = state.composing;
        if focused {
            let ime = state.track_ime(ui);
            if !ime && !state.composing {
                state.handle_keys(ui, time, &mut events);
            }
            state.handle_paste(ui, &mut events);
        }

        if state.find.open {
            egui::Panel::bottom(state.id.with("find-panel"))
                .frame(
                    Frame::NONE
                        .fill(theme::palette(&ctx).bg_base)
                        .inner_margin(Margin::symmetric(10, 6))
                        .stroke(Stroke::new(1.0, theme::palette(&ctx).border)),
                )
                .show(ui, |ui| state.find_panel(ui, time, &mut events));
        }

        let before = state.cursor(&ctx);
        let mut out = None;
        egui::CentralPanel::default()
            .frame(Frame::NONE.fill(colors.background))
            .show(ui, |ui| {
                out = Some(state.body(
                    ui,
                    &colors,
                    accessible_name,
                    context_menu,
                    before,
                    time,
                    was_composing,
                    &mut events,
                ));
            });
        let (response, char_at_pointer) = out.expect("central panel ran");
        state.completion_popup(&ctx, &colors);
        EditorOutput {
            events,
            response,
            char_at_pointer,
        }
    }
}

impl EditorState {
    /// Runs a command against the text.
    fn exec(
        &mut self,
        ctx: &egui::Context,
        command: Command,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let (anchor, head) = self.cursor(ctx);
        let edit = match command {
            Command::OrderedList => {
                list_edits::span_in_text(&self.text, anchor, head, to_ordered_list)
            }
            Command::UnorderedList => {
                list_edits::span_in_text(&self.text, anchor, head, to_unordered_list)
            }
            Command::IndentSubList => list_edits::sub_list_in_text(&self.text, head),
            Command::Bold => Some(edits::toggle_marker(&self.text, anchor, head, "**")),
            Command::Italic => Some(edits::toggle_marker(&self.text, anchor, head, "*")),
            Command::Insert(s) => Some(edits::replace_selection(&self.text, anchor, head, &s)),
            Command::InsertAt(at, s) => {
                let at = at.min(chars_in(&self.text));
                Some(edits::replace_selection(&self.text, at, at, &s))
            }
            Command::Undo | Command::Redo => {
                let cursor = if command == Command::Undo {
                    self.history.undo(&mut self.text)
                } else {
                    self.history.redo(&mut self.text)
                };
                if let Some((a, h)) = cursor {
                    self.recorded.clone_from(&self.text);
                    self.version += 1;
                    self.set_cursor(ctx, a, h);
                    events.push(EditorEvent::Changed);
                }
                None
            }
            Command::SelectAll => {
                self.set_cursor(ctx, 0, chars_in(&self.text));
                self.focus_request = true;
                None
            }
            Command::Copy | Command::Cut => {
                let (from, to) = (anchor.min(head), anchor.max(head));
                if from == to {
                    return;
                }
                let (bf, bt) = (byte_at(&self.text, from), byte_at(&self.text, to));
                ctx.copy_text(self.text[bf..bt].to_owned());
                self.focus_request = true;
                (command == Command::Cut)
                    .then(|| edits::replace_selection(&self.text, anchor, head, ""))
            }
            Command::Paste => {
                // The platform reads the clipboard and sends a paste event next frame.
                self.focus_request = true;
                ctx.memory_mut(|m| m.request_focus(self.id));
                ctx.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
                None
            }
            Command::Find => {
                // Seed the query from a one-line selection, as CodeMirror does.
                let (from, to) = (anchor.min(head), anchor.max(head));
                let (bf, bt) = (byte_at(&self.text, from), byte_at(&self.text, to));
                let sel = &self.text[bf..bt];
                if !sel.is_empty() && !sel.contains('\n') {
                    self.find.query = sel.to_owned();
                }
                self.set_find_open(true);
                None
            }
            Command::FindNext | Command::FindPrevious => {
                self.find_step(ctx, command == Command::FindNext);
                None
            }
        };
        if let Some(edit) = edit {
            self.apply_edit(ctx, (anchor, head), edit, time);
            self.focus_request = true;
            events.push(EditorEvent::Changed);
        }
    }

    /// Selects the next or previous find match from the selection.
    fn find_step(&mut self, ctx: &egui::Context, forward: bool) {
        let (anchor, head) = self.cursor(ctx);
        let (from, to) = (anchor.min(head), anchor.max(head));
        let sel = byte_at(&self.text, from)..byte_at(&self.text, to);
        let matches = self.find.matches(&self.text, self.version);
        if let Some(i) = find::next_match(matches, sel, forward) {
            let m = self.find.char_matches()[i].clone();
            self.set_cursor(ctx, m.start, m.end);
            self.reveal = Some((m.start, false));
        }
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

    /// The keys the editor handles itself; each one it acts on is removed
    /// from the input so `TextEdit` doesn't also see it.
    fn handle_keys(&mut self, ui: &mut Ui, time: f64, events: &mut Vec<EditorEvent>) {
        let ctx = ui.ctx().clone();
        let presses: Vec<(Key, Option<Key>, Modifiers)> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Key {
                        key,
                        physical_key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *physical_key, *modifiers)),
                    _ => None,
                })
                .collect()
        });
        for (key, physical, mods) in presses {
            let Some(action) = self.key_action(key, physical, mods) else {
                continue;
            };
            // Enter and Backspace are only ours when a list or markup edit applies;
            // otherwise TextEdit types them, so they group with the typing around them.
            let edit = match action {
                KeyAction::Enter | KeyAction::Backspace => match self.markup_edit(&ctx, &action) {
                    Some(edit) => Some(edit),
                    None => continue,
                },
                _ => None,
            };
            ui.input_mut(|i| {
                i.events.retain(|e| {
                    !matches!(e, Event::Key { key: k, physical_key: p, pressed: true, modifiers: m, .. }
                        if *k == key && *p == physical && *m == mods)
                });
            });
            match edit {
                Some(edit) => {
                    let before = self.cursor(&ctx);
                    self.apply_edit(&ctx, before, edit, time);
                    events.push(EditorEvent::Changed);
                }
                None => self.run_key_action(&ctx, action, time, events),
            }
        }
        if self.prefs.close_brackets {
            self.handle_brackets(ui, time, events);
        }
    }

    /// What a key press does, if the editor handles it.
    fn key_action(&self, key: Key, physical: Option<Key>, mods: Modifiers) -> Option<KeyAction> {
        let phys = physical.unwrap_or(key);
        let cmd = mods.command && !mods.alt;
        // The completion list owns the arrows, Enter, Tab and Escape while it's open.
        if self
            .completion
            .as_ref()
            .is_some_and(|c| !c.items.is_empty())
            && mods.is_none()
        {
            match key {
                Key::ArrowDown => return Some(KeyAction::CompletionMove(1)),
                Key::ArrowUp => return Some(KeyAction::CompletionMove(-1)),
                Key::Enter | Key::Tab => return Some(KeyAction::CompletionAccept),
                Key::Escape => return Some(KeyAction::CompletionDismiss),
                _ => {}
            }
        }
        // Alt+. / Alt+, insert arrows (by physical key: macOS's Alt+. types another character).
        if mods.alt && !mods.ctrl && !mods.mac_cmd && !mods.command && !mods.shift {
            return match phys {
                Key::Period => Some(KeyAction::Insert("→")),
                Key::Comma => Some(KeyAction::Insert("←")),
                _ => None,
            };
        }
        if cmd && mods.shift {
            return match phys {
                // Ctrl/Cmd+Shift+7 / 8, by physical key: Shift+7 is "&" only on some layouts.
                Key::Num7 => Some(KeyAction::Command(Command::OrderedList)),
                Key::Num8 => Some(KeyAction::Command(Command::UnorderedList)),
                Key::F => Some(KeyAction::Command(Command::Find)),
                Key::Z => Some(KeyAction::Command(Command::Redo)),
                Key::G => Some(KeyAction::Command(Command::FindPrevious)),
                _ => None,
            };
        }
        if cmd {
            return match key {
                Key::B => Some(KeyAction::Command(Command::Bold)),
                Key::I => Some(KeyAction::Command(Command::Italic)),
                Key::Z => Some(KeyAction::Command(Command::Undo)),
                Key::Y => Some(KeyAction::Command(Command::Redo)),
                Key::F => Some(KeyAction::Command(Command::Find)),
                Key::G => Some(KeyAction::Command(Command::FindNext)),
                _ => None,
            };
        }
        match (key, mods) {
            (Key::F3, m) if m.is_none() => Some(KeyAction::Command(Command::FindNext)),
            (Key::F3, m) if m.shift_only() => Some(KeyAction::Command(Command::FindPrevious)),
            (Key::Escape, m) if m.is_none() && self.find.open => Some(KeyAction::CloseFind),
            (Key::Enter, m) if m.is_none() => Some(KeyAction::Enter),
            (Key::Tab, m) if m.is_none() => Some(KeyAction::Indent(false)),
            (Key::Tab, m) if m.shift_only() => Some(KeyAction::Indent(true)),
            (Key::Backspace, m) if m.is_none() => Some(KeyAction::Backspace),
            _ => None,
        }
    }

    /// The list continuation (Enter) or markup deletion (Backspace) at the caret, if any.
    fn markup_edit(&self, ctx: &egui::Context, action: &KeyAction) -> Option<DocEdit> {
        let (anchor, head) = self.cursor(ctx);
        match action {
            KeyAction::Enter if anchor == head => list_edits::enter_in_text(&self.text, head),
            KeyAction::Backspace => edits::backspace(&self.text, anchor, head),
            _ => None,
        }
    }

    fn run_key_action(
        &mut self,
        ctx: &egui::Context,
        action: KeyAction,
        time: f64,
        events: &mut Vec<EditorEvent>,
    ) {
        let (anchor, head) = self.cursor(ctx);
        let edit = match action {
            KeyAction::Command(c) => {
                self.exec(ctx, c, time, events);
                None
            }
            KeyAction::Insert(s) => Some(edits::replace_selection(&self.text, anchor, head, s)),
            KeyAction::CloseFind => {
                self.set_find_open(false);
                None
            }
            KeyAction::CompletionMove(d) => {
                if let Some(c) = &mut self.completion {
                    let n = c.items.len() as isize;
                    c.selected = (c.selected as isize + d).rem_euclid(n) as usize;
                }
                None
            }
            KeyAction::CompletionAccept => self.accept_completion(ctx),
            KeyAction::CompletionDismiss => {
                self.completion_dismissed = self.completion.take().map(|c| c.from);
                None
            }
            // Tab and Shift+Tab are always ours (TextEdit would insert a tab character).
            KeyAction::Indent(outdent) => {
                list_edits::indent_in_text(&self.text, anchor, head, outdent)
            }
            // Handled with their edit in `handle_keys`.
            KeyAction::Enter | KeyAction::Backspace => None,
        };
        if let Some(edit) = edit {
            self.apply_edit(ctx, (anchor, head), edit, time);
            events.push(EditorEvent::Changed);
        }
    }

    /// Bracket auto-closing on typed text. Only when the frame's typed text is
    /// that single character, so the order of keystrokes is kept.
    fn handle_brackets(&mut self, ui: &mut Ui, time: f64, events: &mut Vec<EditorEvent>) {
        let typed: Vec<String> = ui.input(|i| {
            i.events
                .iter()
                .filter_map(|e| match e {
                    Event::Text(t) => Some(t.clone()),
                    _ => None,
                })
                .collect()
        });
        let [one] = typed.as_slice() else { return };
        let mut chars = one.chars();
        let (Some(ch), None) = (chars.next(), chars.next()) else {
            return;
        };
        let ctx = ui.ctx().clone();
        let (anchor, head) = self.cursor(&ctx);
        let Some(edit) = edits::type_char(&self.text, anchor, head, ch) else {
            return;
        };
        ui.input_mut(|i| i.events.retain(|e| !matches!(e, Event::Text(_))));
        self.apply_edit(&ctx, (anchor, head), edit, time);
        events.push(EditorEvent::Changed);
    }

    /// Image pastes, and pastes of nothing but `file://` URIs, go to the host.
    fn handle_paste(&mut self, ui: &mut Ui, events: &mut Vec<EditorEvent>) {
        let mut files = None;
        ui.input_mut(|i| {
            i.events.retain(|e| match e {
                Event::PasteImage(image) => {
                    events.push(EditorEvent::PasteImage(image.clone()));
                    false
                }
                Event::Paste(text) => match file_uris(text) {
                    Some(paths) => {
                        files = Some(paths);
                        false
                    }
                    None => true,
                },
                _ => true,
            });
        });
        if let Some(paths) = files {
            events.push(EditorEvent::PasteFiles(paths));
        }
    }

    /// Accepts the selected completion; returns the edit.
    fn accept_completion(&mut self, ctx: &egui::Context) -> Option<DocEdit> {
        let c = self.completion.take()?;
        let name = self.link_targets.get(*c.items.get(c.selected)?)?.clone();
        let (_, head) = self.cursor(ctx);
        Some(edits::accept_wikilink(&self.text, c.from, head, &name))
    }

    /// The scrolling text area with its gutter; returns its response and the
    /// character under the pointer.
    #[allow(clippy::too_many_arguments)]
    fn body(
        &mut self,
        ui: &mut Ui,
        colors: &EditorColors,
        accessible_name: &str,
        context_menu: bool,
        before: (usize, usize),
        time: f64,
        was_composing: bool,
        events: &mut Vec<EditorEvent>,
    ) -> (Response, Option<usize>) {
        let ctx = ui.ctx().clone();
        let mut area = if self.prefs.wrap {
            ScrollArea::vertical()
        } else {
            ScrollArea::both()
        }
        .id_salt(self.id.with("scroll"))
        .auto_shrink([false, false]);
        if let Some(offset) = self.scroll_request.take() {
            area = area.scroll_offset(offset);
        }
        let scrolled = area.show(ui, |ui| {
            self.text_area(
                ui,
                colors,
                accessible_name,
                context_menu,
                before,
                time,
                was_composing,
                events,
            )
        });
        self.scroll_offset = scrolled.state.clamped_offset();
        let _ = ctx;
        scrolled.inner
    }

    #[allow(clippy::too_many_arguments)]
    fn text_area(
        &mut self,
        ui: &mut Ui,
        colors: &EditorColors,
        accessible_name: &str,
        context_menu: bool,
        before: (usize, usize),
        time: f64,
        was_composing: bool,
        events: &mut Vec<EditorEvent>,
    ) -> (Response, Option<usize>) {
        let ctx = ui.ctx().clone();
        let font_size = self.prefs.font_size;
        let gutter = if self.prefs.line_numbers {
            let digits = self.highlighter_line_count().max(1).to_string().len() as f32;
            GUTTER_MIN.max(digits * font_size * 0.62 + 16.0)
        } else {
            0.0
        };
        // Painted behind the text once the galley is known: active line, matches.
        let backdrop = ui.painter().add(Shape::Noop);

        let spell_on = self.prefs.spell_check && self.spell.is_some();
        let spell_version = self.spell_version;
        let checker = self.spell.clone();
        let EditorState {
            text,
            highlighter,
            id,
            prefs,
            ..
        } = self;
        let opts = LayoutOptions {
            colors,
            font_size,
            wrap: prefs.wrap,
            spell: checker
                .as_deref()
                .filter(|_| spell_on)
                .map(|c| (c as &dyn SpellChecker, spell_version)),
        };
        let mut layouter = |ui: &Ui, buf: &dyn egui::TextBuffer, wrap_width: f32| {
            highlighter.layout(ui, buf.as_str(), wrap_width, &opts)
        };
        let output = TextEdit::multiline(text)
            .id(*id)
            .desired_width(f32::INFINITY)
            // Tab indents, and Escape (which closes the find panel or the
            // completion list) keeps the focus here, as in CodeMirror.
            .event_filter(egui::EventFilter {
                tab: true,
                horizontal_arrows: true,
                vertical_arrows: true,
                escape: true,
            })
            .frame(Frame::NONE.inner_margin(Margin {
                left: (gutter + PAD) as i8,
                right: PAD as i8,
                top: PAD as i8,
                bottom: PAD as i8,
            }))
            .layouter(&mut layouter)
            .show(ui);
        // Nothing visible names the field, so name it for screen readers.
        let response = output
            .response
            .response
            .clone()
            .accessible_name(accessible_name);
        // Our history replaces TextEdit's undoer, which holds whole-text snapshots.
        let mut te_state = output.state.clone();
        te_state.clear_undoer();
        te_state.store(&ctx, self.id);

        // Typing (and IME commits) become history steps once no composition is open.
        if !self.composing && (output.response.response.changed() || was_composing) {
            if self.recorded != self.text {
                let after = output
                    .cursor_range
                    .map_or(before, |r| (r.secondary.index.0, r.primary.index.0));
                self.history
                    .record(&self.recorded, &self.text, before, after, time, true);
                self.recorded.clone_from(&self.text);
                events.push(EditorEvent::Changed);
            }
            self.version += 1;
        } else if output.response.response.changed() {
            self.version += 1;
        }
        self.update_completion(&ctx, &output);

        let galley = output.galley.clone();
        let origin = output.galley_pos;
        let clip = ui.clip_rect();
        let head = output.cursor_range.map_or(0, |r| r.primary.index.0);
        let active_row = galley.layout_from_cursor(CCursor::new(head)).row;
        self.paint_backdrop(
            ui, backdrop, &galley, origin, clip, colors, gutter, active_row,
        );
        if gutter > 0.0 {
            self.paint_gutter(ui, &galley, origin, clip, colors, gutter, active_row);
        }
        if let Some((c, top)) = self.reveal.take() {
            let r = galley
                .pos_from_cursor(CCursor::new(c))
                .translate(origin.to_vec2());
            ui.scroll_to_rect(
                r.expand2(vec2(0.0, 4.0)),
                Some(if top { Align::TOP } else { Align::Center }),
            );
        }

        let dropped: Vec<PathBuf> = ui.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        // `cursor_from_pos` walks the galley's rows (about 12 ms at 50,000
        // lines), so only find the character under the pointer when
        // something needs it: a click, a drag in progress, or a drop.
        let needs_pointer = response.clicked()
            || response.secondary_clicked()
            || !dropped.is_empty()
            || egui::DragAndDrop::has_any_payload(&ctx)
            || ctx.input(|i| i.pointer.any_released());
        let char_at_pointer = ctx
            .pointer_latest_pos()
            .filter(|p| needs_pointer && response.rect.contains(*p))
            .map(|p| galley.cursor_from_pos(p - origin).index.0);

        // Ctrl/Cmd+click opens links and wikilinks.
        if response.clicked() && ui.input(|i| i.modifiers.command) {
            if let Some(c) = char_at_pointer {
                if let Some(link) = self.link_at_char(c) {
                    events.push(match link {
                        LinkTarget::Wiki(name) => EditorEvent::OpenWikilink(name),
                        LinkTarget::Url(url) => EditorEvent::OpenLink(url),
                    });
                }
            }
        }

        // Files dropped over the text.
        if !dropped.is_empty()
            && ctx
                .pointer_latest_pos()
                .is_none_or(|p| response.rect.contains(p))
        {
            events.push(EditorEvent::DropFiles(dropped));
        }

        if context_menu {
            if response.secondary_clicked() {
                self.menu_word = char_at_pointer.and_then(|c| self.misspelled_at(c));
            }
            response.context_menu(|ui| self.context_menu_ui(ui, time, events));
        }
        (response, char_at_pointer)
    }

    fn highlighter_line_count(&self) -> usize {
        match self.highlighter.line_count() {
            0 => self.text.matches('\n').count() + 1,
            n => n,
        }
    }

    /// The active line and the find matches, behind the text.
    #[allow(clippy::too_many_arguments)]
    fn paint_backdrop(
        &mut self,
        ui: &Ui,
        slot: egui::layers::ShapeIdx,
        galley: &egui::Galley,
        origin: Pos2,
        clip: Rect,
        colors: &EditorColors,
        gutter: f32,
        active_row: usize,
    ) {
        let ctx = ui.ctx();
        let mut shapes = Vec::new();
        let left = origin.x - gutter - PAD;
        let right = clip.right().max(origin.x + galley.size().x + PAD);
        // Every row of the caret's line (it may wrap).
        let rows = &galley.rows;
        if active_row < rows.len() {
            let mut first = active_row;
            while first > 0 && !rows[first - 1].ends_with_newline {
                first -= 1;
            }
            let mut last = active_row;
            while last + 1 < rows.len() && !rows[last].ends_with_newline {
                last += 1;
            }
            let top = origin.y + rows[first].pos.y;
            let bottom = origin.y + rows[last].rect().bottom();
            shapes.push(Shape::rect_filled(
                Rect::from_x_y_ranges(left..=right, top..=bottom),
                0.0,
                colors.active_line,
            ));
        }
        if self.find.open {
            self.find.matches(&self.text, self.version);
            let (anchor, head) = self.cursor(ctx);
            let sel = (anchor.min(head), anchor.max(head));
            let visible = clip.y_range();
            for m in self.find.char_matches() {
                let a = galley.pos_from_cursor(CCursor::new(m.start));
                if a.top() + origin.y > visible.max {
                    break;
                }
                let b = galley.pos_from_cursor(CCursor::new(m.end));
                if b.bottom() + origin.y < visible.min {
                    continue;
                }
                let fill = if (m.start, m.end) == sel {
                    colors.find_current
                } else {
                    colors.find_match
                };
                if (a.top() - b.top()).abs() < 1.0 {
                    let r = Rect::from_min_max(pos2(a.left(), a.top()), pos2(b.left(), a.bottom()));
                    shapes.push(Shape::rect_filled(r.translate(origin.to_vec2()), 2.0, fill));
                } else {
                    // A match across rows: mark its start.
                    let r = Rect::from_min_size(a.min, vec2(6.0, a.height()));
                    shapes.push(Shape::rect_filled(r.translate(origin.to_vec2()), 2.0, fill));
                }
            }
        }
        ui.painter().set(slot, Shape::Vec(shapes));
    }

    /// Line numbers for the visible rows.
    #[allow(clippy::too_many_arguments)]
    fn paint_gutter(
        &mut self,
        ui: &Ui,
        galley: &egui::Galley,
        origin: Pos2,
        clip: Rect,
        colors: &EditorColors,
        gutter: f32,
        active_row: usize,
    ) {
        let painter = ui.painter();
        let left = origin.x - gutter - PAD;
        let full = Rect::from_x_y_ranges(left..=left + gutter, clip.y_range());
        painter.rect_filled(full, 0.0, colors.gutter);
        painter.vline(
            left + gutter,
            clip.y_range(),
            Stroke::new(1.0, theme::palette(ui.ctx()).border),
        );
        let line_rows = self.highlighter.line_rows();
        // The mapping belongs to this galley (it always does unless the text
        // changed after layout, in which case the next frame draws them).
        if line_rows.last() != Some(&galley.rows.len()) {
            return;
        }
        let rows = &galley.rows;
        let first_visible = rows.partition_point(|r| origin.y + r.rect().bottom() < clip.top());
        let active_line = self.highlighter.line_of_row(active_row);
        let font = FontId::new(self.prefs.font_size * 0.86, FontFamily::Monospace);
        let mut line = self.highlighter.line_of_row(first_visible);
        while line + 1 < line_rows.len() {
            let row = &rows[line_rows[line]];
            let top = origin.y + row.pos.y;
            if top > clip.bottom() {
                break;
            }
            painter.text(
                pos2(left + gutter - 8.0, top + row.rect().height() / 2.0),
                Align2::RIGHT_CENTER,
                (line + 1).to_string(),
                font.clone(),
                if line == active_line {
                    colors.gutter_active
                } else {
                    colors.gutter_text
                },
            );
            line += 1;
        }
    }

    /// The link around character `c`.
    fn link_at_char(&self, c: usize) -> Option<LinkTarget> {
        let b = byte_at(&self.text, c);
        let start = self.text[..b].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[b..].find('\n').map_or(self.text.len(), |i| b + i);
        link_at(&self.text[start..end], b - start)
    }

    /// The misspelled word around character `c`, with suggestions.
    fn misspelled_at(&self, c: usize) -> Option<(std::ops::Range<usize>, String, Vec<String>)> {
        let checker = self.spell.as_ref().filter(|_| self.prefs.spell_check)?;
        let b = byte_at(&self.text, c);
        let start = self.text[..b].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[b..].find('\n').map_or(self.text.len(), |i| b + i);
        let line = &self.text[start..end];
        let word = spell::word_at(line, b - start)?;
        let w = &line[word.clone()];
        if checker.check(w) {
            return None;
        }
        let from = chars_in(&self.text[..start + word.start]);
        Some((from..from + chars_in(w), w.to_owned(), checker.suggest(w)))
    }

    /// The context menu (`handleContextMenu`), plus spelling suggestions.
    fn context_menu_ui(&mut self, ui: &mut Ui, time: f64, events: &mut Vec<EditorEvent>) {
        let ctx = ui.ctx().clone();
        let item = |ui: &mut Ui, icon: &str, label: &str, shortcut: &str| {
            let text = RichText::new(format!("{icon}  {label}"));
            ui.add(egui::Button::new(text).shortcut_text(shortcut))
                .clicked()
        };
        if let Some((range, word, suggestions)) = self.menu_word.clone() {
            for s in suggestions {
                if ui.button(RichText::new(&s).strong()).clicked() {
                    let edit = edits::replace_selection(&self.text, range.start, range.end, &s);
                    self.apply_edit(&ctx, (range.start, range.end), edit, time);
                    events.push(EditorEvent::Changed);
                    ui.close();
                }
            }
            if ui.button(format!("Add “{word}” to dictionary")).clicked() {
                events.push(EditorEvent::AddToDictionary(word));
                ui.close();
            }
            ui.separator();
        }
        let mut run = None;
        if item(ui, icons::OBJECT_GROUP, "Select All", "Ctrl+A") {
            run = Some(Command::SelectAll);
        }
        ui.separator();
        if item(ui, icons::SCISSORS, "Cut", "Ctrl+X") {
            run = Some(Command::Cut);
        }
        if item(ui, icons::COPY, "Copy", "Ctrl+C") {
            run = Some(Command::Copy);
        }
        if item(ui, icons::PASTE, "Paste", "Ctrl+V") {
            run = Some(Command::Paste);
        }
        ui.separator();
        if item(ui, icons::LIST_OL, "Ordered List", "Ctrl+Shift+7") {
            run = Some(Command::OrderedList);
        }
        if item(ui, icons::LIST_UL, "Unordered List", "Ctrl+Shift+8") {
            run = Some(Command::UnorderedList);
        }
        ui.separator();
        for (icon, label, format) in [
            (icons::FILE_CODE, "Save to HTML", SaveFormat::Html),
            (icons::FILE_PDF, "Save to PDF", SaveFormat::Pdf),
            (icons::FILE_LINES, "Save to Markdown", SaveFormat::Markdown),
        ] {
            if item(ui, icon, label, "") {
                events.push(EditorEvent::Save(format));
                ui.close();
            }
        }
        if let Some(command) = run {
            self.exec(&ctx, command, time, events);
            ui.close();
        }
    }

    /// Opens, filters or closes the `[[` completion after an edit or a caret move.
    fn update_completion(&mut self, ctx: &egui::Context, output: &egui::text_edit::TextEditOutput) {
        let focused = ctx.memory(|m| m.has_focus(self.id));
        let Some(range) = output.cursor_range.filter(|r| r.is_empty() && focused) else {
            self.completion = None;
            return;
        };
        let head = range.primary.index.0;
        let Some((from, query)) = edits::wikilink_query(&self.text, head) else {
            self.completion = None;
            self.completion_dismissed = None;
            return;
        };
        if self.completion_dismissed == Some(from) {
            return;
        }
        let q = query.to_lowercase();
        let items: Vec<usize> = self
            .link_targets
            .iter()
            .enumerate()
            .filter(|(_, n)| n.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .take(MAX_COMPLETIONS)
            .collect();
        if items.is_empty() {
            self.completion = None;
            return;
        }
        let selected = self
            .completion
            .as_ref()
            .filter(|c| c.from == from)
            .map_or(0, |c| c.selected.min(items.len() - 1));
        self.completion = Some(Completion {
            from,
            items,
            selected,
        });
        // Where to show it: under the caret.
        let pos = output
            .galley
            .pos_from_cursor(range.primary)
            .translate(output.galley_pos.to_vec2())
            .left_bottom();
        ctx.data_mut(|d| d.insert_temp(self.id.with("complete-pos"), pos));
    }

    /// The `[[` completion list, under the caret.
    fn completion_popup(&mut self, ctx: &egui::Context, colors: &EditorColors) {
        let Some(c) = self.completion.clone() else {
            return;
        };
        let Some(pos) = ctx.data(|d| d.get_temp::<Pos2>(self.id.with("complete-pos"))) else {
            return;
        };
        let p = theme::palette(ctx);
        let mut picked = None;
        egui::Area::new(self.id.with("complete"))
            .order(Order::Foreground)
            .fixed_pos(pos + vec2(0.0, 2.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .fill(p.bg_surface)
                    .stroke(Stroke::new(1.0, p.border))
                    .show(ui, |ui| {
                        ui.set_min_width(220.0);
                        ScrollArea::vertical().max_height(240.0).show(ui, |ui| {
                            for (k, &i) in c.items.iter().enumerate() {
                                let name = &self.link_targets[i];
                                let selected = k == c.selected;
                                let r = ui.selectable_label(
                                    selected,
                                    RichText::new(name).color(if selected {
                                        colors.link
                                    } else {
                                        colors.text
                                    }),
                                );
                                if selected {
                                    r.scroll_to_me(None);
                                }
                                if r.clicked() {
                                    picked = Some(k);
                                }
                            }
                        });
                    });
            });
        if let Some(k) = picked {
            if let Some(comp) = &mut self.completion {
                comp.selected = k;
            }
            let before = self.cursor(ctx);
            if let Some(edit) = self.accept_completion(ctx) {
                self.apply_edit(ctx, before, edit, ctx.input(|i| i.time));
                self.focus_request = true;
            }
        }
    }

    /// The find panel (CodeMirror's search panel).
    fn find_panel(&mut self, ui: &mut Ui, time: f64, events: &mut Vec<EditorEvent>) {
        let ctx = ui.ctx().clone();
        let query_id = self.id.with("find-query");
        let count = self.find.matches(&self.text, self.version).len();
        let invalid = self.find.invalid();
        let mut step = None;
        let mut replace_one = false;
        let mut replace_every = false;
        ui.horizontal(|ui| {
            let field = ui
                .add(
                    TextEdit::singleline(&mut self.find.query)
                        .id(query_id)
                        .hint_text("Find")
                        .desired_width(220.0)
                        .text_color_opt(invalid.then_some(Color32::from_rgb(0xe0, 0x4b, 0x4b))),
                )
                .accessible_name("Find");
            if std::mem::take(&mut self.find.focus_query) {
                field.request_focus();
            }
            if field.has_focus() || field.lost_focus() {
                let (enter, shift) = ui.input(|i| (i.key_pressed(Key::Enter), i.modifiers.shift));
                if enter {
                    step = Some(!shift);
                    field.request_focus();
                }
                if ui.input(|i| i.key_pressed(Key::Escape)) {
                    self.set_find_open(false);
                }
            }
            if ui.button("next").clicked() {
                step = Some(true);
            }
            if ui.button("previous").clicked() {
                step = Some(false);
            }
            if ui.button("all").clicked() && count > 0 {
                // One selection only in egui: select the first and report the count.
                step = Some(true);
            }
            ui.checkbox(&mut self.find.case_sensitive, "match case");
            ui.checkbox(&mut self.find.regex, "regexp");
            ui.checkbox(&mut self.find.whole_word, "by word");
            let label = if invalid {
                "invalid pattern".to_owned()
            } else if self.find.query.is_empty() {
                String::new()
            } else {
                format!("{count} found")
            };
            ui.label(RichText::new(label).small().weak());
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(egui::Button::new(icons::icon(icons::XMARK)).frame(false))
                    .on_hover_text("Close (Escape)")
                    .clicked()
                {
                    self.set_find_open(false);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.find.replacement)
                    .id(self.id.with("find-replace"))
                    .hint_text("Replace")
                    .desired_width(220.0),
            )
            .accessible_name("Replace");
            replace_one = ui.button("replace").clicked();
            replace_every = ui.button("replace all").clicked();
        });

        if let Some(forward) = step {
            self.find_step(&ctx, forward);
        }
        if replace_one {
            self.replace_current(&ctx, time, events);
        }
        if replace_every {
            let matches = self.find.matches(&self.text, self.version).to_vec();
            if !matches.is_empty() {
                let (text, _) = find::replace_all(&self.find, &self.text, &matches);
                let before = self.cursor(&ctx);
                let n = chars_in(&text);
                self.apply_edit(
                    &ctx,
                    before,
                    DocEdit {
                        text,
                        anchor: before.0.min(n),
                        head: before.1.min(n),
                    },
                    time,
                );
                events.push(EditorEvent::Changed);
            }
        }
    }

    /// Replaces the selected match and selects the next one (CodeMirror's `replaceNext`).
    fn replace_current(&mut self, ctx: &egui::Context, time: f64, events: &mut Vec<EditorEvent>) {
        let (anchor, head) = self.cursor(ctx);
        let sel = (anchor.min(head), anchor.max(head));
        let version = self.version;
        self.find.matches(&self.text, version);
        let found = self
            .find
            .char_matches()
            .iter()
            .position(|m| (m.start, m.end) == sel);
        let Some(i) = found else {
            self.find_step(ctx, true);
            return;
        };
        let byte_range = self.find.matches(&self.text, version)[i].clone();
        let replacement = self.find.replacement_for(&self.text, &byte_range);
        let edit = edits::replace_selection(&self.text, sel.0, sel.1, &replacement);
        self.apply_edit(ctx, (anchor, head), edit, time);
        events.push(EditorEvent::Changed);
        self.find_step(ctx, true);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum KeyAction {
    Command(Command),
    Insert(&'static str),
    Enter,
    /// Tab (false) or Shift+Tab (true).
    Indent(bool),
    Backspace,
    CloseFind,
    CompletionMove(isize),
    CompletionAccept,
    CompletionDismiss,
}

/// The paths of a paste made only of `file://` URIs (one per line), as a
/// file manager's "Copy" puts on the clipboard.
fn file_uris(text: &str) -> Option<Vec<PathBuf>> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() || !lines.iter().all(|l| l.starts_with("file://")) {
        return None;
    }
    lines
        .iter()
        .map(|l| {
            let path = percent_decode(&l["file://".len()..])?;
            // `file:///C:/x` on Windows.
            let path = if cfg!(windows) {
                path.trim_start_matches('/').to_owned()
            } else {
                path
            };
            Some(PathBuf::from(path))
        })
        .collect()
}

fn percent_decode(s: &str) -> Option<String> {
    let mut out = Vec::with_capacity(s.len());
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = s.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_pastes() {
        assert_eq!(
            file_uris("file:///home/a/My%20File.png\nfile:///tmp/b.txt\n"),
            Some(vec![
                PathBuf::from(if cfg!(windows) {
                    "home/a/My File.png"
                } else {
                    "/home/a/My File.png"
                }),
                PathBuf::from(if cfg!(windows) {
                    "tmp/b.txt"
                } else {
                    "/tmp/b.txt"
                }),
            ])
        );
        assert_eq!(file_uris("just text"), None);
        assert_eq!(file_uris("file:///a\nplain"), None);
    }
}
