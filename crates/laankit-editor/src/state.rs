//! [`EditorState`]: everything the editor keeps between frames. The host owns
//! one per open note (or one it reloads per note), reads the text from it
//! and drives it with commands.

use std::ops::Range;
use std::sync::Arc;

use egui::text::{CCursor, CCursorRange};
use egui::{Id, TextEdit};

use crate::editor_core::selection::{Sel, Selections};
use crate::editor_core::CoreState;
use crate::engine::Engine;
use crate::find::FindState;
use crate::highlight::Highlighter;
use crate::history::{Cursor, History};
use crate::list_edits::{byte_at, chars_in, classify_span, DocEdit, ListKind};
use crate::spell::SpellChecker;

/// Body text size (`fontSize: '14px'` in `Editor.svelte`).
pub const DEFAULT_FONT_SIZE: f32 = 14.0;

/// Display preferences.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EditorPrefs {
    pub font_size: f32,
    /// The line-number gutter (CodeMirror `basicSetup`).
    pub line_numbers: bool,
    /// Soft wrap (`EditorView.lineWrapping`).
    pub wrap: bool,
    /// Underline misspellings (`spellCheckEnabled`); needs a checker from
    /// [`EditorState::set_spell_checker`].
    pub spell_check: bool,
    /// Auto-close brackets and quotes (`closeBrackets` in `basicSetup`).
    pub close_brackets: bool,
    /// The text engine (the core engine unless `LAANKIT_EDITOR_ENGINE=textedit`).
    pub engine: Engine,
}

impl Default for EditorPrefs {
    fn default() -> Self {
        Self {
            font_size: DEFAULT_FONT_SIZE,
            line_numbers: true,
            wrap: true,
            spell_check: true,
            close_brackets: true,
            engine: Engine::default(),
        }
    }
}

/// Something the editor can be asked to do, from the host's toolbar, menus
/// or command palette. Keyboard shortcuts run the same commands.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// Ctrl+Shift+7.
    OrderedList,
    /// Ctrl+Shift+8.
    UnorderedList,
    /// The toolbar's "Indent as Sub-list".
    IndentSubList,
    /// Ctrl+B.
    Bold,
    /// Ctrl+I.
    Italic,
    Undo,
    Redo,
    SelectAll,
    /// The context menu's Cut/Copy/Paste go through the platform clipboard.
    Cut,
    Copy,
    Paste,
    /// Ctrl+F / Ctrl+Shift+F: the find panel, with its query focused.
    Find,
    FindNext,
    FindPrevious,
    /// Ctrl/Cmd+Alt+G: the Go to line bar (CodeMirror's `gotoLine`).
    GoToLine,
    /// Replaces the selection (or inserts at the caret).
    Insert(String),
    /// Inserts at a character position (e.g. where a file from the tree was dropped).
    InsertAt(usize, String),
}

/// The caret and selection, for the status bar (`projectState.cursorLine`,
/// `cursorCol`, `selectionLength`, `selectionRange`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorStatus {
    /// 1-based line of the caret.
    pub line: usize,
    /// 1-based column of the caret, in characters.
    pub col: usize,
    /// Selected characters.
    pub selection_len: usize,
    /// 1-based first and last line of a non-empty selection.
    pub selection_lines: Option<(usize, usize)>,
}

/// An open `[[` completion.
#[derive(Clone, Debug, Default)]
pub(crate) struct Completion {
    /// Character index just after `[[`.
    pub from: usize,
    /// Indices into [`EditorState::link_targets`].
    pub items: Vec<usize>,
    pub selected: usize,
}

/// The Go to line bar.
#[derive(Clone, Debug, Default)]
pub(crate) struct GotoBar {
    pub input: String,
    /// Focus the input next frame.
    pub focus: bool,
    /// The last Enter didn't parse.
    pub invalid: bool,
}

/// A position as (0-based line, column in characters).
pub(crate) type LineCol = (usize, usize);

/// Bracket and selection-match decorations, cached until the text, the
/// caret or the visible lines change.
#[derive(Clone, Debug, Default)]
pub(crate) struct DecorCache {
    /// (text version, anchor, head, first visible line, last visible line).
    pub key: Option<(u64, usize, usize, usize, usize)>,
    /// The bracket at the caret and its partner, as (line, column) in characters.
    pub brackets: Option<(LineCol, Option<LineCol>)>,
    /// Other occurrences of the selection: (line, columns in characters).
    pub matches: Vec<(usize, Range<usize>)>,
}

/// One editor's state. See the crate docs for how a host drives it.
pub struct EditorState {
    pub(crate) id: Id,
    pub(crate) text: String,
    /// The text as of the last recorded history step (for typing diffs).
    pub(crate) recorded: String,
    /// Bumped on every change of the text.
    pub(crate) version: u64,
    pub(crate) prefs: EditorPrefs,
    pub(crate) highlighter: Highlighter,
    pub(crate) history: History,
    pub(crate) find: FindState,
    pub(crate) pending: Vec<Command>,
    /// Page names offered after `[[` (the host keeps it current).
    pub(crate) link_targets: Arc<Vec<String>>,
    pub(crate) completion: Option<Completion>,
    /// Escape closed the completion for the wikilink starting here.
    pub(crate) completion_dismissed: Option<usize>,
    pub(crate) spell: Option<Arc<dyn SpellChecker>>,
    pub(crate) spell_version: u64,
    /// The scroll offset last frame, and one to apply next frame.
    pub(crate) scroll_offset: egui::Vec2,
    pub(crate) scroll_request: Option<egui::Vec2>,
    /// A character to scroll into view next frame, and whether to put it at the top.
    pub(crate) reveal: Option<(usize, bool)>,
    pub(crate) focus_request: bool,
    /// An IME composition is open (preedit text showing).
    pub(crate) composing: bool,
    /// Cached (version, cursor) → status and list kind.
    status_cache: Option<((u64, Cursor), CursorStatus, ListKind)>,
    /// The word under the pointer when the context menu opened, with its
    /// character range and suggestions.
    pub(crate) menu_word: Option<(Range<usize>, String, Vec<String>)>,
    pub(crate) goto: Option<GotoBar>,
    pub(crate) decor: DecorCache,
    /// This frame's input has a paste, cut or line-joining key that
    /// `TextEdit` handles (ordered lists are renumbered after it).
    pub(crate) structural_input: bool,
    /// The core engine's index of `text`, and the text version it matches.
    pub(crate) rope: ropey::Rope,
    pub(crate) rope_version: u64,
    pub(crate) core: CoreState,
}

impl EditorState {
    /// An empty editor. `id_salt` must be unique among the editors on screen.
    pub fn new(id_salt: impl std::hash::Hash + std::fmt::Debug) -> Self {
        Self {
            id: Id::unique(("laankit-editor", id_salt)),
            text: String::new(),
            recorded: String::new(),
            version: 0,
            prefs: EditorPrefs::default(),
            highlighter: Highlighter::default(),
            history: History::default(),
            find: FindState::default(),
            pending: Vec::new(),
            link_targets: Arc::new(Vec::new()),
            completion: None,
            completion_dismissed: None,
            spell: None,
            spell_version: 0,
            scroll_offset: egui::Vec2::ZERO,
            scroll_request: None,
            reveal: None,
            focus_request: false,
            composing: false,
            status_cache: None,
            menu_word: None,
            goto: None,
            decor: DecorCache::default(),
            structural_input: false,
            rope: ropey::Rope::new(),
            rope_version: 0,
            core: CoreState::default(),
        }
    }

    /// The layouter's counters (for the benchmark).
    pub fn layout_stats(&self) -> crate::highlight::HighlightStats {
        self.highlighter.stats
    }

    /// Line galleys the layouter holds (the core engine keeps only the ones
    /// near the viewport).
    pub fn held_galleys(&self) -> usize {
        self.highlighter.galley_count()
    }

    /// Bytes held by the undo history.
    pub fn history_bytes(&self) -> usize {
        self.history.bytes()
    }

    /// The engine in use.
    pub fn engine(&self) -> Engine {
        self.prefs.engine
    }

    pub(crate) fn is_core(&self) -> bool {
        self.prefs.engine == Engine::Core
    }

    /// The selections, as (anchor, head) in characters, primary first (the
    /// core engine can have several; `TextEdit` has one).
    pub fn selections(&self, ctx: &egui::Context) -> Vec<Cursor> {
        if !self.is_core() {
            return vec![self.cursor(ctx)];
        }
        let sel = &self.core.sel;
        let p = sel.primary_index();
        std::iter::once(sel.primary())
            .chain(
                sel.ranges()
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != p)
                    .map(|(_, s)| *s),
            )
            .map(|s| (s.anchor, s.head))
            .collect()
    }

    /// Lines folded away by the core engine: (first line, last hidden line), 0-based.
    pub fn folds(&self) -> &[(usize, usize)] {
        self.core.folds.ranges()
    }

    /// The editor's id (focus, accessibility; also `TextEdit`'s id).
    pub fn id(&self) -> Id {
        self.id
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Bumped on every change, by the user or the host.
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Loads a note: replaces the text, clears undo, and puts the caret at
    /// the start. Doesn't count as a user edit (no [`crate::EditorEvent::Changed`]).
    pub fn set_text(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        self.text = text.into();
        self.recorded = self.text.clone();
        self.version += 1;
        self.history.clear();
        self.completion = None;
        self.scroll_request = Some(egui::Vec2::ZERO);
        self.core.folds.clear();
        self.core.ime = None;
        self.core.anchor = None;
        self.composing = false;
        self.set_cursor(ctx, 0, 0);
    }

    /// Replaces the text as one undoable step, keeping the caret where it
    /// can (for a note changed on disk while open, as `updateEditorContent`
    /// does). Doesn't emit [`crate::EditorEvent::Changed`].
    pub fn replace_text(&mut self, ctx: &egui::Context, text: impl Into<String>) {
        let text = text.into();
        if text == self.text {
            return;
        }
        let (anchor, head) = self.cursor(ctx);
        let n = chars_in(&text);
        let edit = DocEdit {
            text,
            anchor: anchor.min(n),
            head: head.min(n),
        };
        self.apply_edit(ctx, (anchor, head), edit, 0.0);
    }

    pub fn prefs(&self) -> EditorPrefs {
        self.prefs
    }

    pub fn set_prefs(&mut self, prefs: EditorPrefs) {
        if prefs.spell_check != self.prefs.spell_check {
            self.spell_version += 1;
        }
        self.prefs = prefs;
    }

    /// The page names `[[` completes (`projectState.index.files` names, minus the open note).
    pub fn set_link_targets(&mut self, names: Vec<String>) {
        self.link_targets = Arc::new(names);
    }

    /// The spell checker to use while [`EditorPrefs::spell_check`] is on.
    pub fn set_spell_checker(&mut self, checker: Option<Arc<dyn SpellChecker>>) {
        self.spell = checker;
        self.spell_version += 1;
    }

    /// Re-checks every line (after the host added a word to its dictionary).
    pub fn refresh_spelling(&mut self) {
        self.spell_version += 1;
    }

    /// The caret and selection as (anchor, head), in characters.
    pub fn cursor(&self, ctx: &egui::Context) -> Cursor {
        if self.is_core() {
            let p = self.core.sel.primary();
            return (p.anchor, p.head);
        }
        TextEdit::load_state(ctx, self.id)
            .and_then(|s| s.cursor.char_range())
            .map_or((0, 0), |r| (r.secondary.index.0, r.primary.index.0))
    }

    /// Moves the caret (or selects anchor..head), in characters, and scrolls it into view.
    pub fn set_cursor(&mut self, ctx: &egui::Context, anchor: usize, head: usize) {
        if self.is_core() {
            self.sync_rope();
            let n = self.chars_len();
            self.core.sel = Selections::single(Sel::new(anchor.min(n), head.min(n)));
            self.core.goal_x = None;
            self.reveal = Some((head.min(n), false));
            self.core_unfold_carets();
            return;
        }
        let n = chars_in(&self.text);
        let mut state = TextEdit::load_state(ctx, self.id).unwrap_or_default();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(anchor.min(n)),
            CCursor::new(head.min(n)),
        )));
        state.store(ctx, self.id);
        self.reveal = Some((head.min(n), false));
    }

    /// The scroll offset last frame (to restore when the note is reopened).
    pub fn scroll_offset(&self) -> egui::Vec2 {
        self.scroll_offset
    }

    pub fn set_scroll_offset(&mut self, offset: egui::Vec2) {
        self.scroll_request = Some(offset);
    }

    /// Restores a saved view: the caret (anchor and head, in characters) and
    /// the scroll offset, without scrolling the caret into view (the saved
    /// offset already shows it). For reopening a note where it was left.
    pub fn restore_view(
        &mut self,
        ctx: &egui::Context,
        anchor: usize,
        head: usize,
        scroll: egui::Vec2,
    ) {
        self.set_cursor(ctx, anchor, head);
        self.reveal = None;
        self.scroll_request = Some(scroll);
    }

    /// Opens the Go to line bar, focused (Ctrl/Cmd+Alt+G).
    pub fn open_goto(&mut self) {
        let bar = self.goto.get_or_insert_with(GotoBar::default);
        bar.focus = true;
        bar.invalid = false;
    }

    pub fn goto_open(&self) -> bool {
        self.goto.is_some()
    }

    /// Puts the caret at the start of 1-based `line`, scrolls that line to
    /// the top and focuses the editor (the outline's `scrollToLine`).
    pub fn scroll_to_line(&mut self, ctx: &egui::Context, line: usize) {
        if self.is_core() {
            self.sync_rope();
            let line = line.max(1).min(self.lines()) - 1;
            let c = self.rope.line_to_char(line);
            self.set_cursor(ctx, c, c);
            self.reveal = Some((c, true));
            self.focus_request = true;
            return;
        }
        let mut b = 0;
        for _ in 1..line.max(1) {
            match self.text[b..].find('\n') {
                Some(i) => b += i + 1,
                None => break,
            }
        }
        let c = chars_in(&self.text[..b]);
        self.set_cursor(ctx, c, c);
        self.reveal = Some((c, true));
        self.focus_request = true;
    }

    /// Gives the editor keyboard focus next frame.
    pub fn focus(&mut self) {
        self.focus_request = true;
    }

    /// Queues a command for the next frame.
    pub fn run(&mut self, command: Command) {
        self.pending.push(command);
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    /// Opens or closes the find panel.
    pub fn set_find_open(&mut self, open: bool) {
        self.find.open = open;
        self.find.focus_query = open;
        if !open {
            self.focus_request = true;
        }
    }

    pub fn find_open(&self) -> bool {
        self.find.open
    }

    /// Sets the find query (e.g. from the host's own search).
    pub fn set_find_query(&mut self, query: impl Into<String>) {
        self.find.query = query.into();
    }

    /// Caret line/column and selection (for the status bar).
    pub fn status(&mut self, ctx: &egui::Context) -> CursorStatus {
        self.status_and_kind(ctx).0
    }

    /// What the list buttons would act on (their active state, `listKind`).
    pub fn list_kind(&mut self, ctx: &egui::Context) -> ListKind {
        self.status_and_kind(ctx).1
    }

    fn status_and_kind(&mut self, ctx: &egui::Context) -> (CursorStatus, ListKind) {
        let cursor = self.cursor(ctx);
        let key = (self.version, cursor);
        if let Some((k, status, kind)) = &self.status_cache {
            if *k == key {
                return (*status, *kind);
            }
        }
        let (anchor, head) = cursor;
        let (from, to) = (anchor.min(head), anchor.max(head));
        if self.is_core() {
            let (status, kind) = self.core_status(from, to, head);
            self.status_cache = Some((key, status, kind));
            return (status, kind);
        }
        let (bf, bt, bh) = (
            byte_at(&self.text, from),
            byte_at(&self.text, to),
            byte_at(&self.text, head),
        );
        let line_of = |b: usize| self.text[..b].matches('\n').count() + 1;
        let line = line_of(bh);
        let line_start = self.text[..bh].rfind('\n').map_or(0, |i| i + 1);
        let col = chars_in(&self.text[line_start..bh]) + 1;
        let (from_line, to_line) = (line_of(bf), line_of(bt));
        let status = CursorStatus {
            line,
            col,
            selection_len: to - from,
            selection_lines: (from != to).then_some((from_line, to_line)),
        };
        // classify only the touched lines.
        let start = self.text[..bf].rfind('\n').map_or(0, |i| i + 1);
        let end = self.text[bt..]
            .find('\n')
            .map_or(self.text.len(), |i| bt + i);
        let span: Vec<&str> = self.text[start..end].split('\n').collect();
        let kind = classify_span(&span, 1, span.len());
        self.status_cache = Some((key, status, kind));
        (status, kind)
    }

    /// Status and list kind from the rope (O(log n) lookups, not a scan).
    fn core_status(&mut self, from: usize, to: usize, head: usize) -> (CursorStatus, ListKind) {
        self.sync_rope();
        let (line, col) = self.line_col(head);
        let (from_line, to_line) = (self.line_col(from).0, self.line_col(to).0);
        let status = CursorStatus {
            line: line + 1,
            col: col + 1,
            selection_len: to - from,
            selection_lines: (from != to).then_some((from_line + 1, to_line + 1)),
        };
        let span: Vec<&str> = (from_line..=to_line).map(|l| self.line_text(l)).collect();
        let kind = classify_span(&span, 1, span.len());
        (status, kind)
    }

    /// Opens any fold that hides a caret.
    pub(crate) fn core_unfold_carets(&mut self) {
        if self.core.folds.is_empty() {
            return;
        }
        let heads: Vec<usize> = self.core.sel.ranges().iter().map(|s| s.head).collect();
        for h in heads {
            let (line, _) = self.line_col(h);
            self.core.folds.unfold_hiding(line);
        }
    }

    /// Applies an edit as one undoable step and puts the caret where it says.
    pub(crate) fn apply_edit(
        &mut self,
        ctx: &egui::Context,
        before: Cursor,
        edit: DocEdit,
        time: f64,
    ) {
        self.history.record(
            &self.recorded,
            &edit.text,
            before,
            (edit.anchor, edit.head),
            time,
            false,
        );
        self.text = edit.text;
        self.recorded.clone_from(&self.text);
        self.version += 1;
        self.set_cursor(ctx, edit.anchor, edit.head);
    }
}
