# marina-editor

NoteLiner's Markdown editor for the native (egui) builds, the counterpart of
`apps/noteliner/src/renderer/components/Editor.svelte` (CodeMirror 6). Plan
Option C, Phase 4 (`docs/plans/plan-native-rust-egui.md`), and the editor
phases of `docs/plans/plan-editor-slides-richtext.md`.

It runs on **its own editor core** ([`editor_core`](src/editor_core/mod.rs),
the default since Phase 3) or, as a fallback for one release, on egui's
`TextEdit::multiline` (see [Engines](#engines-and-the-fallback)). Either way:

- **A cached, incremental highlighting layouter.** One galley per line; an
  edit rescans only the lines it touched. Promoted from Phase 0 spike 1
  (`docs/plans/spikes/editor.md`). The core engine lays out only the lines
  on screen.
- **Everything `Editor.svelte` and CodeMirror's `basicSetup` add around the
  text:** list editing, emphasis toggles, bracket auto-closing, the
  line-number gutter and active line, find and replace, `[[` completion,
  the context menu, and the paste, drop and link hooks the host handles.
- **Code fences highlighted by language** (`syntect`, its ~75 bundled
  syntaxes, pure Rust; [`code`](src/code.rs)), plus bracket matching,
  other occurrences of the selection, Go to line, and ordered lists that
  renumber after Enter, Backspace/Delete, Tab and paste. The logic is in
  engine-independent modules ([`code`](src/code.rs),
  [`matching`](src/matching.rs), [`list_edits`](src/list_edits.rs)) so it
  carries over to the planned custom editor core.
- **Undo as diffs, with a memory bound** (8 MiB of changed text, 1,000
  steps). egui's undoer keeps whole-text snapshots, about 250 MB for a
  2.5 MB note.

```bash
cargo run -p marina-editor --example editor -- note.md        # or --lines 20000
cargo run -p marina-editor --example editor --features spellcheck -- note.md
cargo run --profile fast-release -p marina-editor --example bench -- [--sizes 1000,5000] [--accesskit] [--fences 4] [--engine core|textedit]
cargo test -p marina-editor --features spellcheck
MARINA_EDITOR_ENGINE=textedit cargo run -p noteliner-native   # the fallback engine
```

## Engines and the fallback

| Engine            | What it is | Choose it |
|-------------------|------------|-----------|
| `Engine::Core` (default) | The editor's own widget: a rope index, several selections, only the visible lines laid out, folding | Default |
| `Engine::TextEdit` | egui's `TextEdit` with the same highlighter and commands: one selection, the whole note laid out, no folding | `MARINA_EDITOR_ENGINE=textedit` in the environment, or `EditorPrefs { engine: Engine::TextEdit, .. }` |

The fallback stays for one release while the core engine's IME and screen
reader support are checked by hand (see the end of this file), then goes.
Every shared test runs on both engines (`tests/editor.rs`); the core
engine's own features are tested in `tests/core.rs`.

### The core engine

| Module | What it does |
|--------|--------------|
| [`editor_core`](src/editor_core/mod.rs) | State between frames; the rope (`ropey`, LF-only lines) that indexes the note for char/byte/line lookups in O(log n), kept in step by splicing in what changed |
| [`editor_core::selection`](src/editor_core/selection.rs) | Several ranges, one primary, sorted and merged |
| [`editor_core::ops`](src/editor_core/ops.rs) | Word groups (CodeMirror's), smart Home, replacing many ranges in one pass |
| [`editor_core::fold`](src/editor_core/fold.rs) | What folds (headings to the next heading of the same or a higher level, fences, list items with nested lines) and the closed folds, moved along with edits |
| `editor_core::view` | Layout, input and painting (below) |

- **Model.** The note stays one `String`: `text()` hands out `&str`, and the
  list, find and markup helpers work on strings. The rope beside it answers
  every position question without scanning the note. An edit is a splice;
  the rope sync tells the highlighter which lines changed, so neither diffs
  the text again.
- **Layout.** Every line has a top: its measured height once laid out, an
  estimate before (one row, or as many as its length wraps to), none when
  folded. Only the visible lines get galleys. On frames that happen anyway
  (no repaint is requested for it), a 2 ms budget lays out off-screen lines
  to replace estimates; galleys past 3,000 are dropped again (their heights
  stay), so memory is bounded. When heights above the first visible line
  change, the scroll offset moves with them, so the text on screen stays put.
- **Input.** Everything the editor relied on `TextEdit` for: caret moves
  by character, word, row (keeping the column), page, line and document,
  with Shift; smart Home; click, double-click (word) and triple-click
  (line); drag selection with autoscroll; cut, copy and paste; IME (the
  preedit sits in the note, out of the history, until the commit, which is
  one step), with the candidate window placed at the caret.
- **Several cursors.** Ctrl/Cmd+click adds one (on a link it opens the link
  instead), Ctrl/Cmd+D selects the word, then the next occurrence, the
  find panel's "all" selects every match, Alt+drag makes a rectangle (the
  same character columns on each line, as CodeMirror). Typing, Backspace,
  Delete, Enter and paste act at every cursor in one undo step; paste puts
  one line at each cursor when the counts match; copy joins the selections
  with newlines. Escape goes back to one cursor. Undo restores the primary
  cursor only. List continuation, Tab and the toolbar's list commands act on
  the primary cursor.
- **Folding.** A chevron in the gutter (shown while the pointer is over
  it, and always for a closed fold), Ctrl/Cmd+Shift+[ / ] at the caret.
  A closed fold shows `…` after its first line. Folds live in memory, per
  open note; they move with edits around them and open when an edit or a
  caret reaches inside.
- **Screen readers.** AccessKit gets the visible lines as text runs (the
  field's value is those lines), the caret and selection when they're on
  screen, and the field's name. Nothing it gets grows with the note.

## API

```rust
use marina_editor::{Command, Editor, EditorEvent, EditorState, attachment_markdown};
use marina_editor::toolbar::{self, ToolbarAction, ToolbarProps};

// Once per editor (e.g. per open note, or one reloaded per note):
let mut state = EditorState::new("note-editor");      // unique among editors on screen
state.set_text(ctx, markdown);                         // loads a note: clears undo, caret at 0
state.set_link_targets(names_of_other_notes);          // what `[[` completes
state.set_prefs(EditorPrefs { font_size: 14.0, line_numbers: true, wrap: true,
                              spell_check: true, close_brackets: true });

// Every frame:
let actions = toolbar::show(ui, &mut state, &ToolbarProps {
    file_name: Some(&note.name), show_history, show_preview,
});
let out = Editor::new(&mut state).show(ui);           // fills the space it's given
for event in out.events { /* see the contract below */ }
```

| Item                                          | What it is                                                            |
|-----------------------------------------------|-----------------------------------------------------------------------|
| `EditorState`                                 | The text, caret, undo history, find state, prefs and caches           |
| `state.text()` / `state.version()`            | The note, and a counter bumped on every change                        |
| `state.set_text(ctx, s)`                      | Load a note (not an edit: no `Changed`, undo cleared)                 |
| `state.replace_text(ctx, s)`                  | Replace it as one undoable step (changed on disk while open)          |
| `state.run(Command)`                          | Queue a command for the next frame (toolbar, menu, palette)           |
| `state.cursor(ctx)` / `set_cursor(ctx, a, h)` | Caret/selection in characters (`set_cursor` scrolls it into view)     |
| `state.scroll_offset()` / `set_scroll_offset` | Scroll position                                                       |
| `state.restore_view(ctx, a, h, scroll)`       | Caret and scroll together, without scrolling to the caret (reopening a note where it was left) |
| `state.open_goto()` / `goto_open()`           | The Go to line bar                                                    |
| `state.scroll_to_line(ctx, n)`                | The outline's `scrollToLine`: caret to line n, line at the top, focus |
| `state.status(ctx)`                           | `CursorStatus { line, col, selection_len, selection_lines }`          |
| `state.list_kind(ctx)`                        | `ListKind` of the caret's lines (the list buttons' active state)      |
| `state.focus()`                               | Keyboard focus next frame                                             |
| `state.set_find_open(bool)`, `set_find_query` | The find panel                                                        |
| `state.set_spell_checker(Some(Arc<dyn SpellChecker>))` | Spell checking (see below)                                   |
| `Editor::new(&mut state).show(ui)`            | Draws it; returns `EditorOutput { events, response, char_at_pointer }` |
| `toolbar::show(ui, &mut state, &props)`       | The bar above the editor; returns `ToolbarAction`s for the host       |
| `attachment_markdown(name, file)`             | The Markdown `insertMarkdownReference` inserts for an attachment      |
| `EditorColors::from_palette(&palette)`        | The editor's colours (derived from the theme; see `highlight.rs`)     |
| `code::syntaxes(Some(ctx))` / `load_blocking()` | The code-fence syntaxes: loaded on a background thread on first use (`None` until then), or now |

`Command`: `OrderedList`, `UnorderedList`, `IndentSubList`, `Bold`, `Italic`,
`Undo`, `Redo`, `SelectAll`, `Cut`, `Copy`, `Paste`, `Find`, `FindNext`,
`FindPrevious`, `GoToLine`, `Insert(String)` (replaces the selection) and
`InsertAt(char, String)`.

## Integration contract (for `apps/noteliner/native`)

| `EditorEvent`             | When                                               | The host…                                                                                               |
|---------------------------|----------------------------------------------------|---------------------------------------------------------------------------------------------------------|
| `Changed`                 | The user changed the text (any edit, not `set_text`) | Debounces 500 ms and writes `state.text()` (`scheduleSave`); handles `git_config_required`              |
| `PasteImage(ColorImage)`  | An image was pasted (no text on the clipboard)     | Encodes it (PNG), saves it as an attachment, then `state.run(Command::Insert(attachment_markdown(..)))` |
| `PasteFiles(Vec<PathBuf>)`| The paste was only `file://` URIs (a file manager's Copy) | Same, per file (the 30 MB limit is the host's, as in `processFiles`)                              |
| `DropFiles(Vec<PathBuf>)` | Files were dropped on the editor                   | Same as `PasteFiles`. Ignore these drops elsewhere in the window                                        |
| `OpenWikilink(name)`      | Ctrl/Cmd+click on `[[name]]` or `[[name\|alias]]`   | Opens that note                                                                                         |
| `OpenLink(url)`           | Ctrl/Cmd+click on `[text](url)`                    | Opens the URL, or the note/attachment it points to                                                      |
| `Save(SaveFormat)`        | Context menu "Save to HTML/PDF/Markdown"           | Runs the export (`onSaveToHtml` …)                                                                      |
| `AddToDictionary(word)`   | Context menu on a misspelled word                  | Adds it to the checker and the user's word list, then `state.refresh_spelling()`                        |

- **Dragging a note from the file tree** (`interceptInternalDrops`): the tree
  keeps its own `egui::DragAndDrop` payload. On release over the editor,
  the host reads `out.char_at_pointer` and runs
  `Command::InsertAt(c, format!("[{name}](./{filename})"))`.
  `char_at_pointer` is only computed while something is dragged, or on a
  click or release, because it walks the galley's rows (about 12 ms at
  50,000 lines).
- **Toolbar:** `ToolbarAction::ToggleHistory` and `TogglePreview` are the
  host's. The list buttons run their commands themselves.
- **Shortcuts the host owns** stay with the host: Ctrl+H, Ctrl+P, Ctrl+S
  and the rest of `App.svelte`'s keymap. The editor only takes the keys
  listed below, and only while it has focus.
- **Theme:** the palette is read every frame, so theme changes apply
  without rebuilding (the Svelte editor recreates CodeMirror).
- **Spell checking:** build with `--features spellcheck` and call
  `state.set_spell_checker(Some(Arc::new(Hunspell::new(&aff, &dic)?)))`.
  The dictionary can be bundled, or taken from `/usr/share/hunspell`, or
  from a Windows or macOS dictionary folder. The Svelte build used the
  webview's checker for the system locale. Any type implementing
  `SpellChecker` works. Misspelled prose is underlined; code, URLs,
  wikilinks and frontmatter are skipped, as are words with digits or
  inner capitals (`HTTP`, `v2`). The context menu offers suggestions and
  "Add to dictionary".

### Keys

| Keys                                     | Action                                                                     |
|------------------------------------------|----------------------------------------------------------------------------|
| Enter                                    | Continue a list item, task or quote; end the list on an empty item (`insertNewlineContinueMarkup`); renumber the ordered items after it |
| Backspace after `- `, `1. `, `- [ ] `, `> ` | Remove that markup (`deleteMarkupBackward`); between `()` deletes the pair |
| Tab / Shift+Tab                          | Indent / outdent the selected lines by 2 spaces (`indentWithTab`)          |
| Ctrl/Cmd+Shift+7 / 8                     | Ordered / unordered list (physical keys, as `event.code`)                  |
| Ctrl/Cmd+B / I                           | Toggle `**` / `*`                                                          |
| Alt+. / Alt+,                            | Insert `→` / `←` (physical keys)                                           |
| Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Ctrl/Cmd+Y | Undo, redo                                                                 |
| Ctrl/Cmd+F, Ctrl/Cmd+Shift+F             | Find (seeded from a one-line selection)                                    |
| F3 / Shift+F3, Ctrl/Cmd+G / Shift+G      | Next / previous match                                                      |
| Ctrl/Cmd+Alt+G                           | Go to line: `42`, `42:7` (line:column), `+5` / `-5` (relative), `50%` (physical key) |
| Escape                                   | Close the completion list, then the find panel, then go back to one cursor; focus stays in the editor |
| Ctrl/Cmd+click, Ctrl/Cmd+D, Alt+drag      | Core engine: add a cursor, select the next occurrence, rectangular selection |
| Ctrl/Cmd+Shift+[ / ]                     | Core engine: fold / unfold at the caret (physical bracket keys)            |
| Ctrl/Cmd+A, Home / End, Ctrl+Home / End, Ctrl+←/→ (Alt on macOS), Page Up / Down | Select all; smart Home; document start/end; by word; by page (core engine; `TextEdit` has its own) |
| `(`, `[`, `{`, `"`, `'`                  | Auto-close, wrap a selection, step over a closer (`closeBrackets`)         |
| ↑ ↓ Enter Tab Escape (completion open)   | Choose, accept, dismiss                                                    |
| Triple-click                             | Select the line                                                            |

## Parity with `Editor.svelte` (CodeMirror `basicSetup`)

| Feature                                                    | Status   | Notes |
|------------------------------------------------------------|----------|-------|
| Markdown highlighting                                      | Done     | Line scanner: headings, emphasis, code, links, wikilinks, lists, tasks, quotes, tables, frontmatter |
| Code-language highlighting inside fences (`codeLanguages`) | Done     | `syntect`'s ~75 bundled syntaxes (75 in all; no TypeScript, TOML, Svelte or PowerShell); aliases such as `shell`, `py3`, `yml`; One Dark / CodeMirror default colours (`SyntaxColors` in `highlight.rs`). Loads in the background, fences are one colour until then |
| Theme colours                                              | Done     | Derived from the six themes' palettes (CodeMirror used One Dark / a light theme) |
| Line numbers, active line                                  | Done     | |
| Soft wrap (`lineWrapping`)                                 | Done     | `EditorPrefs::wrap` |
| Fold gutter                                                | Done (core) | Headings, fences, list items with nested lines; Ctrl/Cmd+Shift+[ / ] |
| Bracket matching highlight                                 | Done     | `()[]{}` at the caret (after it, then before), 10,000 characters at most; brackets in inline code skipped; an unmatched one in red |
| Bracket auto-closing                                       | Done     | Steps over any matching closer (CodeMirror tracks only the ones it inserted) |
| Highlight other occurrences of the selection               | Done     | A selected whole word (whole-word matches) or short run without whitespace (substring matches), visible lines only, 100 at most |
| Multiple cursors, rectangular selection                    | Done (core) | Ctrl/Cmd+click, Ctrl/Cmd+D, Alt+drag; undo restores the primary cursor; list keys act on the primary |
| List continuation, markup Backspace, Tab indent            | Done     | The following ordered items are renumbered (Enter, Backspace/Delete, Tab, paste) in the same undo step, if they were in sequence (CodeMirror's `renumberList`; `1. 1. 1.` lists are left alone) |
| List toolbar and Ctrl+Shift+7/8, indent as sub-list        | Done     | One undo step each |
| Ctrl+B / Ctrl+I, Alt+. / Alt+,                             | Done     | |
| Find and replace (case, regexp, by word)                   | Done     | "all" selects every match (core); the first one on `TextEdit` |
| Go to line (search keymap)                                 | Done     | Ctrl/Cmd+Alt+G bar: line, line:column, ±n, n% |
| `[[` completion                                            | Done     | Contains-match on names, 50 at most, `]]` added unless present |
| Undo/redo                                                  | Done     | Diff-based, 500 ms typing groups, 8 MiB / 1,000 steps |
| IME                                                        | Done     | Preedit isn't saved or recorded; the commit is one step; Enter while composing is the IME's |
| Paste/drop attachments                                     | Done     | Via host events (images, file URIs, dropped files) |
| Internal drag of a note into the editor                    | Done     | Via `char_at_pointer` and `InsertAt` |
| Context menu                                               | Done     | Plus spelling suggestions |
| Spell check                                                | Done     | `spellcheck` feature, host-supplied Hunspell dictionary |
| Ctrl/Cmd+click to open links                               | Native addition | `OpenLink` / `OpenWikilink` |
| `scrollToLine`, cursor/selection status                    | Done     | |
| Restore the caret and scroll of a reopened note            | Native addition | `restore_view`; NoteLiner keeps it for the last-opened note |
| Screen readers                                             | Done (core) | Visible lines as text runs, caret and selection; to be checked with real screen readers. `TextEdit` gives AccessKit the whole note every frame |
| Notes of 50,000 lines and more                             | Done (core) | Typing p95 under 6 ms at 50,000 lines (below) |

## Numbers

### Core engine against `TextEdit` (Phase 3, 2026-10-18)

`cargo run --profile fast-release -p marina-editor --example bench -- --sizes 5000,20000,50000 [--engine textedit] [--accesskit]`,
CPU per frame (`run_ui` and `tessellate`, 1200×800), p50 / p95 / max in ms.
Load average **≈4** (≈7.5 for the AccessKit rows).

| Engine   | Lines  | Open  | Idle               | Type at start      | Type in middle     | Type at end        | Enter in list      | Wheel scroll       |
|----------|--------|-------|--------------------|--------------------|--------------------|--------------------|--------------------|--------------------|
| Core     | 5,000  | 185   | 2.33 / 2.54 / 2.64 | 0.21 / 0.41 / 0.64 | 0.29 / 2.30 / 2.37 | 0.22 / 2.54 / 2.73 | 0.59 / 2.85 / 2.90 | 0.32 / 0.34 / 1.91 |
| Core     | 20,000 | 495   | 2.31 / 2.63 / 2.69 | 0.59 / 0.76 / 2.96 | 1.34 / 3.66 / 4.09 | 0.61 / 3.69 / 7.19 | 2.21 / 4.85 / 7.07 | 2.73 / 3.21 / 8.26 |
| Core     | 50,000 | 1,208 | 2.59 / 2.75 / 2.86 | 1.58 / 1.78 / 7.05 | 2.35 / 4.92 / 6.39 | 1.75 / 5.17 / 7.75 | 6.04 / 8.79 / 11.0 | 3.27 / 3.74 / 6.71 |
| TextEdit | 5,000  | 448   | 0.18 / 0.31 / 0.33 | 1.22 / 1.27 / 3.09 | 1.64 / 1.82 / 2.49 | 1.99 / 2.17 / 2.18 | 1.95 / 1.99 / 2.01 | 0.22 / 0.23 / 0.36 |
| TextEdit | 20,000 | 1,192 | 0.98 / 1.30 / 1.52 | 7.23 / 7.96 / 10.1 | 11.0 / 12.3 / 13.8 | 11.0 / 12.8 / 13.5 | 11.5 / 12.0 / 12.4 | 1.03 / 1.07 / 1.25 |
| TextEdit | 50,000 | 2,884 | 3.68 / 4.99 / 5.34 | 28.7 / 30.8 / 35.5 | 33.4 / 35.4 / 38.3 | 38.0 / 39.1 / 40.0 | 37.2 / 39.9 / 43.4 | 3.92 / 4.37 / 4.49 |

With AccessKit on (a screen reader running):

| Engine   | Lines  | Idle               | Type at start      | Type in middle     | Type at end        |
|----------|--------|--------------------|--------------------|--------------------|--------------------|
| Core     | 5,000  | 2.40 / 2.60 / 2.72 | 0.28 / 0.29 / 0.76 | 0.37 / 2.40 / 2.52 | 0.30 / 2.65 / 2.86 |
| Core     | 50,000 | 2.78 / 2.99 / 3.03 | 1.73 / 2.34 / 6.52 | 2.66 / 5.28 / 6.84 | 1.92 / 5.05 / 9.69 |
| TextEdit | 5,000  | 7.36 / 8.42 / 8.70 | 8.44 / 8.80 / 9.44 | 8.96 / 9.88 / 10.1 | 9.34 / 9.96 / 10.3 |
| TextEdit | 50,000 | 86.2 / 89.2 / 94.1 | 110 / 112 / 115    | 114 / 116 / 117    | 121 / 123 / 124    |

- **The plan's exit criteria hold:** typing at 50,000 lines is 1.8–5.2 ms
  at p95 (under 8 ms), 3.3 ms inside a code fence (`--fences 4`); with
  AccessKit on, an idle frame costs about the same at 5,000 and 50,000
  lines (2.6 and 3.0 ms) where `TextEdit` goes from 8.4 to 89 ms.
- **The core's idle column is the off-screen measuring budget** (≤2 ms
  per frame, only on frames that happen anyway) until every line has a
  measured height; then idle frames cost what `TextEdit`'s do.
- **Enter in a list** at 50,000 lines (8.8 ms p95) is list continuation:
  it still works on the whole note (to outdent a nested item it reads the
  lines above, and it renumbers what follows). Plain Enter is a typing frame.
- **Opening** scans every line (for fences and frontmatter) but lays out
  only what's on screen: 1.2 s at 50,000 lines against 2.9 s.
- What made typing fast besides the virtualized layout: the rope sync
  passes the edited lines to the highlighter (no second diff), the
  highlighter splices its line list instead of cloning it, prefix/suffix
  comparisons run a block at a time ([`fastcmp`](src/fastcmp.rs)), and the
  bracket and `[[` helpers run on the caret's line only.

### With code-fence highlighting (Phase 2, 2026-10-18, `TextEdit` engine)

`cargo run --profile fast-release -p marina-editor --example bench -- --sizes 1000,5000,20000 --fences 4`:
the generated notes plus four 200-line fences (Rust, Python, JSON, shell),
syntaxes loaded. Load average **≈20** (another agent compiling), so these are
upper bounds. p50 / p95 / max in ms, AccessKit off.

| Lines  | Open   | Idle               | Type at start      | Type in middle     | Type at end        | Type in a fence    | Wheel scroll       |
|--------|--------|--------------------|--------------------|--------------------|--------------------|--------------------|--------------------|
| 1,000  | 349    | 0.08 / 0.12 / 0.21 | 0.36 / 0.45 / 0.75 | 0.85 / 0.97 / 1.32 | 0.86 / 0.98 / 1.03 | 0.85 / 0.93 / 0.97 | 0.12 / 0.15 / 0.19 |
| 5,000  | 554    | 0.18 / 0.31 / 0.34 | 1.54 / 1.73 / 3.22 | 2.18 / 2.49 / 3.10 | 2.47 / 2.70 / 2.88 | 2.01 / 2.23 / 2.45 | 0.23 / 0.26 / 0.43 |
| 20,000 | 1,402  | 1.02 / 1.59 / 1.88 | 10.7 / 11.9 / 15.0 | 13.1 / 14.1 / 14.8 | 14.8 / 15.7 / 16.3 | 12.7 / 13.6 / 14.1 | 1.06 / 1.15 / 1.23 |

- **The gate holds:** typing at 5,000 lines stays under 4 ms at p95 (1.7–2.7
  ms), inside a fence too (2.2 ms). The same note without the big fences, run
  back to back: 1.7–2.5 ms, so the highlighting adds nothing measurable per
  keystroke (an edit re-highlights one line).
- **Opening** a note parses every fence line once: 554 ms against 440 ms for
  the same 5,000 lines without the big fences. Until the syntaxes have
  loaded (in the background, the first time a note is shown) fences are one
  colour, then only the fence lines are laid out again.
- **Binary size** (release, LTO, stripped): the `editor` example grows from 17.78 MB to
  18.60 MB (**+0.8 MB**: syntect's parser and its bundled syntaxes). The
  release `noteliner` binary didn't grow measurably (60.44 → 60.43 MB),
  because Typst (PDF export) already links syntect; that build also has
  other changes, so it's not a clean comparison.

### Before code highlighting

`cargo run --profile fast-release -p marina-editor --example bench`: CPU per frame
(`run_ui` and `tessellate`, 1200×800), p50 / p95 / max in ms. These were
taken on a 16-core laptop at load average **≈10**: other agents were
compiling. Run back to back at the same load, the Phase 0 spike measured
10–20% less while typing, because this widget also paints the gutter and
active line, records undo diffs and tracks completion. On a quiet machine,
scale both down (the spike's quiet runs were about 30% lower).

AccessKit off:

| Lines  | Open (ms) | Idle            | Type at start     | Type in middle    | Type at end       | Type after idle frame | Enter in list     | Wheel scroll      |
|--------|-----------|-----------------|-------------------|-------------------|-------------------|-----------------------|-------------------|-------------------|
| 1,000  | 119       | 0.11 / 0.15 / 0.27 | 0.47 / 0.52 / 0.94 | 0.56 / 0.98 / 1.49 | 0.69 / 1.15 / 1.27 | 0.58 / 1.03 / 1.07 | 0.53 / 0.58 / 0.60 | 0.18 / 0.19 / 0.36 |
| 5,000  | 321       | 0.25 / 0.41 / 0.47 | 1.90 / 2.07 / 4.36 | 2.65 / 3.18 / 4.20 | 3.02 / 3.50 / 3.73 | 2.45 / 3.12 / 3.22 | 2.74 / 2.98 / 2.99 | 0.31 / 0.34 / 0.66 |
| 20,000 | 1,089     | 1.29 / 2.19 / 2.57 | 14.1 / 15.5 / 17.0 | 15.8 / 16.8 / 18.3 | 15.6 / 17.7 / 17.9 | 13.2 / 14.5 / 14.9 | 12.4 / 13.4 / 13.5 | 1.05 / 1.09 / 1.35 |
| 50,000 | 2,030     | 4.42 / 5.63 / 5.84 | 30.2 / 32.3 / 39.5 | 34.8 / 37.0 / 37.6 | 41.0 / 42.6 / 43.8 | 35.3 / 37.5 / 37.9 | 35.6 / 37.4 / 37.9 | 4.42 / 5.06 / 5.69 |

AccessKit on (a screen reader running):

| Lines  | Idle               | Type in middle     | Type at end        | Wheel scroll       |
|--------|--------------------|--------------------|--------------------|--------------------|
| 1,000  | 1.60 / 2.64 / 2.94 | 1.48 / 1.85 / 2.18 | 1.58 / 1.90 / 1.92 | 1.20 / 1.31 / 1.40 |
| 5,000  | 7.93 / 8.53 / 9.36 | 11.1 / 12.0 / 12.4 | 11.1 / 12.2 / 12.7 | 7.84 / 9.12 / 9.60 |
| 20,000 | 34.4 / 37.6 / 38.7 | 63.3 / 68.5 / 70.4 | 48.5 / 49.6 / 51.2 | 36.4 / 37.1 / 38.1 |

- **Normal notes are fast:** 5,000 lines types at p95 2–3.5 ms (the plan's
  bar is 16 ms), and 20,000 lines is at the bar under this load. 50,000
  lines doesn't pass. That cost is inside `TextEdit` (it copies the whole
  text several times a frame) and in `Galley::concat`, as the spike found.
- **Screen readers:** egui's `TextEdit` gives AccessKit every row of the
  galley each frame, about 1.7 ms per 1,000 lines even when idle. Exposing
  only the visible rows would need a fork of `TextEdit`, so it isn't done.
  The cost is fine up to about 8,000 lines.
- **Undo memory:** typing 210 characters plus 20 list Enters keeps 234
  bytes of history. A whole-text snapshot undoer would keep 100 copies of
  the note.
- **Opening a note** lays out every line once: 0.3 s at 5,000 lines, 1 s at
  20,000, 2 s at 50,000 (one frame).

## Tests

`cargo test -p marina-editor --features spellcheck`: 60 unit tests and 49 kittest tests. The 19 tests in `tests/editor.rs` run once per engine (38); `tests/core.rs` has 11 for the core engine's own features.

- **Unit tests:**
  - highlighting (spans tile every line, incremental equals a full rebuild, the row-to-line map, misspellings split only prose)
  - list edits (the `listEdits.js` cases)
  - emphasis toggles, brackets, markup Backspace, wikilink completion edits and attachment Markdown
  - undo grouping and the memory bound
  - find options and replace-all with groups
  - spelling word splitting and Hunspell
  - file-URI pastes
  - code fences: syntaxes by name, extension and alias; Rust, Python, JSON
    and shell tokens; state across lines; edits re-highlight only what
    changed; the syntaxes arriving re-highlight only fence lines; theme
    colours
  - bracket matching (including inline code), selection queries and
    whole-word matching, Go to line parsing
  - ordered-list renumbering (inserts, deletes, pastes, nesting, `)` lists,
    quotes, continuation lines, lazy `1. 1. 1.` lists left alone)
- **Core engine units:** the rope follows edits and moves folds; the
  highlighter told the edited lines matches a full scan over 150 random
  edits (fences, headings, frontmatter, multi-line deletions); selections
  merge and keep their primary; word groups and smart Home; replacing many
  ranges; fold ranges and moving folds with edits; block prefix/suffix.
- **kittest, core engine (`tests/core.rs`):** Ctrl/Cmd+D and typing at
  every cursor (one undo step), Escape; Ctrl/Cmd+click adds a caret but
  opens a wikilink; find "all"; Alt+drag rectangles; multi-cursor paste and
  copy; word moves, smart Home and word Backspace; vertical moves keep the
  column and skip a fold, and a caret moved into a fold opens it; the fold
  gutter, a fold moved by an edit above it; double, triple click and drag
  selection; screen readers get the visible lines only (and follow the
  scroll); a 20,000-line note holds a bounded number of galleys.
- **kittest, both engines (`tests/editor.rs`):**
  - the text paints, and `set_text` isn't an edit
  - Enter continues lists as one undo step; plain Enter/Backspace type normally; Tab/Shift+Tab
  - typing groups into one undo step
  - Ctrl+B/I; Ctrl+Shift+7/8 and Alt+./, on physical keys with other logical keys
  - toolbar commands; bracket auto-closing
  - IME preedit/commit, including Enter while composing
  - `[[` completion (filter, arrows, Enter, Escape)
  - image and file pastes; find, replace all, Escape
  - status and list kind; scroll to line
  - Go to line (absolute, relative, invalid input, Escape)
  - renumbering after Enter, a deleted item, a paste and Tab, each undone
    in one step; plain typing never renumbers
  - bracket-match, unmatched-bracket and selection-match backgrounds paint

## Not yet / for the host to check

- **The `TextEdit` fallback** goes after one release, once the manual
  checks below pass on the core engine.
- **Languages outside syntect's bundled set** (TypeScript, TOML, Svelte, …)
  are one colour (plan decision 2: the bundled set only for now).
- **Core engine, not done:** dragging selected text to move it (neither
  engine does it); Alt+↑/↓ to move lines; list continuation and Tab at
  every cursor (they act on the primary); undo restoring every cursor.
- **To check by hand on the core engine** (headless tests can only inject
  events):
  - IME: IBus and Fcitx on Wayland and X11, Windows TSF, macOS input
    methods. Composition inline, the candidate window at the caret, commit
    and cancel, Enter and Backspace while composing.
  - Screen readers: Orca (Linux), NVDA (Windows), VoiceOver (macOS).
    Reading the visible lines, the caret and selection announced as it
    moves, reading after scrolling.
  - Alt+drag on Linux desktops that move windows with Alt+drag (use
    Alt+Shift+drag there, or change the window manager's modifier).
  - Trackpad scrolling and the scroll position staying put while lines
    off screen are measured.
  - The context menu's Paste on each platform (it asks the platform with
    `ViewportCommand::RequestPaste`); dropping files on Windows and macOS.
