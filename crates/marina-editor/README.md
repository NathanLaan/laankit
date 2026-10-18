# marina-editor

NoteLiner's Markdown editor for the native (egui) builds, the counterpart of
`apps/noteliner/src/renderer/components/Editor.svelte` (CodeMirror 6). Plan
Option C, Phase 4; see `docs/plans/plan-native-rust-egui.md`.

It's egui's `TextEdit::multiline` with:

- **A cached, incremental highlighting layouter.** One galley per line, and
  an edit re-lays out only the lines it touched. Promoted from Phase 0
  spike 1 (`apps/spikes/editor/RESULTS.md`). egui's own whole-text layout
  stalls 127 ms on the first keystroke after any pause at 5,000 lines.
- **Everything `Editor.svelte` and CodeMirror's `basicSetup` add around the
  text:** list editing, emphasis toggles, bracket auto-closing, the
  line-number gutter and active line, find and replace, `[[` completion,
  the context menu, and the paste, drop and link hooks the host handles.
- **Undo as diffs, with a memory bound** (8 MiB of changed text, 1,000
  steps). egui's undoer keeps whole-text snapshots, about 250 MB for a
  2.5 MB note.

```bash
cargo run -p marina-editor --example editor -- note.md        # or --lines 20000
cargo run -p marina-editor --example editor --features spellcheck -- note.md
cargo run --profile spike -p marina-editor --example bench -- [--sizes 1000,5000] [--accesskit]
cargo test -p marina-editor --features spellcheck
```

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
| `state.cursor(ctx)` / `set_cursor(ctx, a, h)` | Caret/selection in characters, for per-file restore                   |
| `state.scroll_offset()` / `set_scroll_offset` | Scroll position, for per-file restore                                 |
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

`Command`: `OrderedList`, `UnorderedList`, `IndentSubList`, `Bold`, `Italic`,
`Undo`, `Redo`, `SelectAll`, `Cut`, `Copy`, `Paste`, `Find`, `FindNext`,
`FindPrevious`, `Insert(String)` (replaces the selection) and
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
| Enter                                    | Continue a list item, task or quote; end the list on an empty item (`insertNewlineContinueMarkup`) |
| Backspace after `- `, `1. `, `- [ ] `, `> ` | Remove that markup (`deleteMarkupBackward`); between `()` deletes the pair |
| Tab / Shift+Tab                          | Indent / outdent the selected lines by 2 spaces (`indentWithTab`)          |
| Ctrl/Cmd+Shift+7 / 8                     | Ordered / unordered list (physical keys, as `event.code`)                  |
| Ctrl/Cmd+B / I                           | Toggle `**` / `*`                                                          |
| Alt+. / Alt+,                            | Insert `→` / `←` (physical keys)                                           |
| Ctrl/Cmd+Z, Ctrl/Cmd+Shift+Z, Ctrl/Cmd+Y | Undo, redo                                                                 |
| Ctrl/Cmd+F, Ctrl/Cmd+Shift+F             | Find (seeded from a one-line selection)                                    |
| F3 / Shift+F3, Ctrl/Cmd+G / Shift+G      | Next / previous match                                                      |
| Escape                                   | Close the completion list, then the find panel; focus stays in the editor  |
| `(`, `[`, `{`, `"`, `'`                  | Auto-close, wrap a selection, step over a closer (`closeBrackets`)         |
| ↑ ↓ Enter Tab Escape (completion open)   | Choose, accept, dismiss                                                    |
| Triple-click                             | Select the line                                                            |

## Parity with `Editor.svelte` (CodeMirror `basicSetup`)

| Feature                                                    | Status   | Notes |
|------------------------------------------------------------|----------|-------|
| Markdown highlighting                                      | Done     | Line scanner: headings, emphasis, code, links, wikilinks, lists, tasks, quotes, tables, frontmatter |
| Code-language highlighting inside fences (`codeLanguages`) | Not yet  | Fence contents are one colour; `syntect` or tree-sitter later |
| Theme colours                                              | Done     | Derived from the six themes' palettes (CodeMirror used One Dark / a light theme) |
| Line numbers, active line                                  | Done     | |
| Soft wrap (`lineWrapping`)                                 | Done     | `EditorPrefs::wrap` |
| Fold gutter                                                | Not yet  | |
| Bracket matching highlight                                 | Not yet  | |
| Bracket auto-closing                                       | Done     | Steps over any matching closer (CodeMirror tracks only the ones it inserted) |
| Highlight other occurrences of the selection               | Not yet  | Matches are highlighted while the find panel is open |
| Multiple cursors, rectangular selection                    | Not yet  | `TextEdit` has one selection |
| List continuation, markup Backspace, Tab indent            | Done     | Renumbering the following ordered items after an insert isn't done |
| List toolbar and Ctrl+Shift+7/8, indent as sub-list        | Done     | One undo step each |
| Ctrl+B / Ctrl+I, Alt+. / Alt+,                             | Done     | |
| Find and replace (case, regexp, by word)                   | Done     | "all" selects the first match and shows the count (one selection only) |
| Go to line (search keymap)                                 | Not yet  | Hosts call `scroll_to_line` |
| `[[` completion                                            | Done     | Contains-match on names, 50 at most, `]]` added unless present |
| Undo/redo                                                  | Done     | Diff-based, 500 ms typing groups, 8 MiB / 1,000 steps |
| IME                                                        | Done     | Preedit isn't saved or recorded; the commit is one step; Enter while composing is the IME's |
| Paste/drop attachments                                     | Done     | Via host events (images, file URIs, dropped files) |
| Internal drag of a note into the editor                    | Done     | Via `char_at_pointer` and `InsertAt` |
| Context menu                                               | Done     | Plus spelling suggestions |
| Spell check                                                | Done     | `spellcheck` feature, host-supplied Hunspell dictionary |
| Ctrl/Cmd+click to open links                               | Native addition | `OpenLink` / `OpenWikilink` |
| `scrollToLine`, cursor/selection status                    | Done     | |
| Screen readers                                             | Partial  | The field is named; egui rebuilds its whole text for AccessKit each frame (costs below) |

## Numbers

`cargo run --profile spike -p marina-editor --example bench`: CPU per frame
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

`cargo test -p marina-editor --features spellcheck`: 30 unit tests and 16 kittest tests.

- **Unit tests:**
  - highlighting (spans tile every line, incremental equals a full rebuild, the row-to-line map, misspellings split only prose)
  - list edits (the `listEdits.js` cases)
  - emphasis toggles, brackets, markup Backspace, wikilink completion edits and attachment Markdown
  - undo grouping and the memory bound
  - find options and replace-all with groups
  - spelling word splitting and Hunspell
  - file-URI pastes
- **kittest:**
  - the text paints, and `set_text` isn't an edit
  - Enter continues lists as one undo step; plain Enter/Backspace type normally; Tab/Shift+Tab
  - typing groups into one undo step
  - Ctrl+B/I; Ctrl+Shift+7/8 and Alt+./, on physical keys with other logical keys
  - toolbar commands; bracket auto-closing
  - IME preedit/commit, including Enter while composing
  - `[[` completion (filter, arrows, Enter, Escape)
  - image and file pastes; find, replace all, Escape
  - status and list kind; scroll to line

## Not yet / for the host to check

- **Ligatures:** the bundled JetBrains Mono (in `marina-egui`) draws
  ligatures, so `->` shows as an arrow in code. epaint has no way to turn
  font features off. **Suggestion for `marina-egui`:** bundle
  *JetBrains Mono NL* (same metrics, no ligatures) instead.
- Fenced-code language highlighting, folding, bracket matching, matches
  of the selection, multiple cursors, and renumbering ordered lists.
- **To check by hand:**
  - real IMEs (IBus/Fcitx on Wayland and X11, Windows TSF, macOS)
  - screen readers
  - trackpad scrolling
  - the context menu's Paste on each platform (it asks the platform with `ViewportCommand::RequestPaste`)
  - dropping files on Windows and macOS
