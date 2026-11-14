# NoteLiner: Diagrams and Tabs Plan

Status: implemented (2026-11-13), NoteLiner 2.4.0. Adds BlockLiner diagrams to NoteLiner as a second document type, tabs so you can keep several notes and diagrams open, and a toolbar above the document that carries BlockLiner's drawing tools while a diagram is open.

## Summary

- A NoteLiner project can hold diagrams beside notes. A diagram is a `.mmd` file in the project folder, in the same Mermaid format BlockLiner saves, so the same file opens in either app.
- Opening a diagram shows BlockLiner's canvas where the note editor would be. The toolbar above the document shows BlockLiner's tools in place of the note's formatting buttons.
- Tabs sit along the top of the document. Each tab is a note or a diagram and keeps its own undo history, cursor or view, and scroll position while it is open.
- BlockLiner's canvas, tools, Properties pane, and export move out of the BlockLiner app into a shared crate that both apps use. BlockLiner's tab strip moves into `laankit-egui`.
- The work is in six phases. Tabs for notes (P1 and P2) are useful on their own and can ship before diagrams.

## Decisions

The open questions were answered on 2026-11-13:

| # | Answer |
|---|---|
| Q1 | Choosing a file opens a new tab. Ctrl+W closes the current tab. Previous File and Next File reuse the active tab. |
| Q2 | A diagram's metadata lives in the index only. |
| Q3 | Ctrl+=, Ctrl+-, and Ctrl+0 zoom the interface. Ctrl+Shift+=, Ctrl+Shift+-, and Ctrl+Shift+0 zoom the canvas. |
| Q4 | The Properties pane is a panel to the right of the canvas. |
| Q5 | Undo, Redo, Grid, Dimensions, and zoom are on the diagram toolbar. |
| Q6 | Embedding a diagram in a note is not part of this work. |
| Q7 | Each app keeps its own diagram settings. |
| Q8 | NoteLiner 2.4.0. BlockLiner's version is unchanged. |

## Changes during implementation

- The tab strip is `laankit_egui::tabs::strip`, with `TabItem` and `TabAction`. Each tab takes an optional icon, which NoteLiner uses for the file's type.
- Closing a tab sends its pending edit to be written and closes the tab at once, as choosing another note always has. A failed write shows its error; the tab doesn't stay open for it.
- P4's stage of opening a diagram as read-only text was skipped: P5 followed directly.
- The Properties panel has a header with a close button, as the Attachments panel does.
- The diagram settings are a Diagrams tab in Settings, not a section of the UI tab.
- Export Diagram writes the PNG or SVG to the Downloads folder and opens it, as the note exports do, with no save dialog. Save Diagram to Mermaid (the palette and the tree's right-click menu) copies the `.mmd` there.
- The diagram's commands are listed in the palette under Diagram, with ids that start `diagram.`.
- Ctrl+Shift+E, Ctrl+Shift+O, Ctrl+Shift+D, and Ctrl+' are not bound for diagrams in NoteLiner. Export Diagram, Toggle Grid, and Toggle Dimension Lines are on the diagram toolbar and in the palette.
- The diagram toolbar hides the file name when the window is too narrow for every button; the tab above shows it. The buttons still scroll sideways if they don't fit.
- A diagram that BlockLiner can't read closes its tab and shows the reader's error.
- A label still being typed is put into the diagram before a write, as BlockLiner does before it switches tabs.
- The History pane shows a diagram's version as a `mermaid` code block.
- The status bar shows the diagram's zoom, and the pointer's place while it is over the canvas.
- In BlockLiner, `Act::Edit(EditAct)` wraps the shared edit commands. BlockLiner's tests pass unchanged.
- `noteliner-core` gains `Project::create(kind, ...)`; `create_file` still creates a note.
- The CI workflow lints and tests `blockliner-egui`.

Checked by eye: a diagram tab in the dark and light themes, and a note tab, at one window size and scale (`LAANKIT_SCREENSHOT`). Not checked by eye: 200% scale, the Diagrams settings tab, and BlockLiner's own window after the move to the shared crate (its tests cover it). Opening one file in both apps wasn't tried by hand; both read and write it through the same `blockliner-core` functions.

## Before this change

### NoteLiner

- Every file in a project is a Markdown note. `Project::create_file` and `rename_file` hardcode `.md` (`crates/noteliner-core/src/project.rs`). There is no file type: a presentation is a note with a `presentation:` frontmatter block, cached in the index as `deck: true`.
- One document is open at a time: `Project.selected` and a single `NoteEditor` (`apps/noteliner/native/src/app.rs`, `editor.rs`). `NoteLinerApp::select` writes any waiting edit, clears the editor, and reads the next file. Clearing the editor drops its undo history, so undo is lost on every switch.
- The document area is a central panel holding `NoteEditor::show`. That draws a 44 point toolbar (`laankit_editor::toolbar`: the file name, the list buttons, and the History and Preview toggles) above the text. The preview, history, and attachments are separate panels to the right.
- The sidebar is a `laankit_egui::panes` stack: Files, Outline, Slides (decks only), Tags, Search, and Backlinks.
- `window-state.json` keeps one `lastFilePosition` per project: the file, cursor, and scroll of the one open note.
- Ctrl+PageUp and Ctrl+PageDown are Previous File and Next File. Ctrl+W, Ctrl+Tab, and Ctrl+Shift+Tab are unused.
- Several features read every file as Markdown: frontmatter mirroring (`reconcile_frontmatter` rewrites each file on open), the wikilink scan (`links.rs`), search, export, the History pane, and the MCP tools.
- NoteLiner doesn't draw Mermaid. A `mermaid` code fence shows as code in the preview and in exports.

### BlockLiner

- `blockliner-core` has no dependencies and no UI. It holds the diagram model, the Mermaid reader and writer (`read`, `write`), the undo history, the clipboard logic, and the export scope.
- The canvas, tools, Properties pane, and SVG and PNG export are modules of the BlockLiner app (`apps/blockliner/native/src/canvas`, `properties.rs`, `export.rs`, `units.rs`). `Canvas::show` and `properties::show` take a `Ui` and don't refer to the app, so they can be drawn inside another app's panel. They do use a few drawing helpers from the app's `views.rs`.
- The glue between them lives in `BlockLinerApp` (`app.rs`): the active tool, applying an edit command, copy and paste, recording undo steps, and the autosave check.
- The tool buttons are drawn by `views::toolbar`, which is built on `laankit_egui::toolbar::show`, a panel down the left edge of the window. It can't be placed above a document as it is.
- The commands are one `Act` enum that mixes diagram edits (tools, undo, select, style, nudge) with file, tab, and window commands.
- The tab strip (`views::tab_strip`, `TabItem`, `TabAction`) depends only on egui and `laankit-egui`. Its plan noted it was written so it could move later.
- The single-key tool shortcuts (V, R, T, the digits, and the rest) are skipped while a text field has the keyboard (`ctx.egui_wants_keyboard_input()`).

## Proposal

### Layout

```
+----+------------+---------------------------------------------------+
|    | FILES      | [ Roadmap x ] [ Architecture x ] [ Meeting ]      |
| t  |  Roadmap   |---------------------------------------------------|
| o  |  Architec. | Architecture | [V][R][O][More][L][A][J][E][T][N]  |
| o  |  Meeting   |--------------------------------------+------------|
| l  |------------|                                      | PROPERTIES |
| b  | TAGS       |                                      |  Fill      |
| a  |            |               canvas                 |  Stroke    |
| r  |            |                                      |            |
+----+------------+--------------------------------------+------------+
```

- The tab strip is a new row above the document, 32 points high, across the editor and the preview.
- Below it is the document toolbar, in the place and at the height of the note toolbar today. Its contents depend on the active tab's type.
- Below that is the document: the note editor, or the diagram canvas.
- The Properties pane is a panel to the right of the canvas, where the preview is for a note. It can be resized and hidden.

### Diagrams as a document type

- A file's type comes from its extension: `.md` is a note and `.mmd` is a diagram. Nothing new is stored in `noteliner.json`; the entry's `filename` already carries it. This follows the way a presentation is recognized, by what the file holds and not by a type field.
- A diagram's index entry is the same as a note's: id, name, filename, parent, order, tags, attachments, and the two dates. It sits in the Files tree like any note, can be a parent or a child, and can be tagged, moved, renamed, duplicated, and deleted.
- The file is BlockLiner's format, unchanged: a Mermaid flowchart followed by the `%% blockliner` layout lines. NoteLiner doesn't write frontmatter into a `.mmd` file, with or without the Write Frontmatter setting; a diagram's name, tags, and dates live only in the index.
- In the Files tree a diagram shows the `OBJECT_GROUP` icon, as files do in BlockLiner.

How each feature treats a diagram:

| Feature | With a diagram |
|---|---|
| New File dialog | The Type row gains Diagram, beside Note and Presentation. Templates are hidden for a diagram. |
| New Diagram | A palette command that opens the New File dialog with Diagram chosen. |
| Rename, Duplicate | Keep the `.mmd` extension. |
| Import, dropped files | A dropped or imported `.mmd` or `.mermaid` file becomes a diagram. A file BlockLiner can't read isn't imported, and the error is shown. |
| Preview | Hidden. The Preview toggle isn't shown on a diagram's toolbar. |
| Outline, Slides | Empty and hidden, as Slides is for a note. |
| Properties | A new panel to the right of the canvas, shown only while a diagram is the active tab. |
| Tags, Attachments | Work as for a note. Insert into Note is off for a diagram. |
| Search | Searches a diagram's Mermaid lines but skips the `%%` layout lines, so a match is a label and not a coordinate. Choosing a result opens the diagram. |
| Wikilinks | `[[Architecture]]` in a note opens the diagram, and diagrams are offered when you type `[[`. A diagram's own text is not scanned for links: `id[[label]]` is Mermaid's subroutine shape, not a link. |
| Backlinks | Lists the notes that link to the diagram. |
| History | Lists the file's versions and shows each as Mermaid source in a code block. Restore works as for a note. |
| Export | Offers PNG and SVG through BlockLiner's export dialog, and Mermaid (a copy of the `.mmd`). The note formats (Markdown, HTML, PDF) aren't offered. |
| Git sync | Unchanged: every file in the folder is committed. |
| Spell check, word count | Don't apply. The status bar shows the zoom and the pointer position in place of the word count. |
| Presentations | Convert to Presentation is off for a diagram. |
| MCP | See below. |

### MCP

| Tool or resource | With a diagram |
|---|---|
| `list_notes`, `noteliner://index` | Each entry gains `kind`: `note`, `deck`, or `diagram`. |
| `read_note`, `noteliner://note/{id}` | Returns the Mermaid text. The resource's type is `text/vnd.mermaid` for a diagram. |
| `create_note` | Gains an optional `kind` of `diagram`, which creates a `.mmd` file. |
| `update_note` | Writes the text with no frontmatter. Text BlockLiner can't read is refused with the reader's error, so a client can't leave a diagram that won't open. |
| `rename_note` | Keeps the extension. |
| `search`, `get_backlinks`, `set_tags`, attachments | As in the table above. |

`noteliner-core` gains a dependency on `blockliner-core` for this check and for a blank diagram's text. `blockliner-core` has no dependencies of its own.

### Tabs

- Each tab shows the file's icon (note, presentation, or diagram), its name, and a close button. The close button shows on the active tab and on the tab under the pointer. Middle-clicking a tab closes it.
- Choosing a file in the Files tree, a search result, a wikilink, a backlink, or the palette opens it in a new tab at the end of the strip, or activates its tab if it has one.
- Previous File and Next File move through the Files tree, as today, and show the file in the active tab's place, so stepping through a project doesn't fill the strip. If the file already has a tab, that tab is activated.
- Each open tab keeps its own state. A note keeps its text, undo history, cursor, and scroll. A diagram keeps its undo history and its pan and zoom; the selection is cleared when you leave it.
- Switching tabs writes the active tab's waiting edit first, as choosing another note does today. Only the active tab can have unsaved edits, so the autosave stays one file at a time.
- Closing a tab writes its waiting edit. If the write fails, the tab stays open and the error is shown. Closing the active tab activates the tab to its right, else the one to its left. With no tabs open, the document area shows "Open a file from the Files pane".
- Closing a tab drops its undo history. Opening the file again reads it from disk.
- Renaming a file renames its tab. Deleting a file closes its tab. Sync, Reset closes every tab, as it clears the editor today.
- When the tabs don't fit, the strip scrolls sideways and the active tab is scrolled into view.
- The Files tree highlights the active tab's file.

### The document toolbar

The toolbar above the document keeps its place and height. Its left end shows the file name for both types.

For a note it is unchanged: the list buttons, Indent as Sub-list, and the History and Preview toggles.

For a diagram it shows:

| Group | Buttons |
|---|---|
| Tools | Select, Rectangle, Rounded Rectangle, More Shapes (with its popup), Line, Arrow, Right-Angle Line, Right-Angle Arrow, Text, Note |
| Edit | Undo, Redo |
| View | Grid, Dimensions, the zoom readout (click to reset) |
| Right end | Properties, Export, History |

- The tool buttons are BlockLiner's, with the same icons, tooltips, and single-key shortcuts, laid out in a row. The active tool is highlighted.
- If the window is too narrow for every button, the toolbar scrolls sideways, as the tab strip does.
- BlockLiner's own toolbar keeps its place down the left edge of its window and draws the same buttons from the shared crate.

### Keyboard while a diagram is open

- BlockLiner's diagram shortcuts apply while a diagram is the active tab and no text field has the keyboard: the tool keys (V, R, O, and the rest), the digits, Delete, the arrow keys to nudge, Enter to edit a label, Esc, Ctrl+Z and Ctrl+Shift+Z, Ctrl+A, Ctrl+B, and copy, cut, and paste.
- They do nothing while a note is the active tab, and nothing while you type in Search, a dialog, or a label.
- NoteLiner's own shortcuts win where the two apps differ:

| Keys | BlockLiner | In NoteLiner with a diagram open |
|---|---|---|
| Ctrl+=, Ctrl+-, Ctrl+0 | Zoom the canvas | Zoom NoteLiner's interface, as today. |
| Ctrl+Shift+=, Ctrl+Shift+-, Ctrl+Shift+0 | (none) | Zoom the canvas in, out, and back to 100%. The canvas also zooms with a pinch and the toolbar's zoom readout. With a note open, Ctrl+Shift+= keeps what it does today. |
| Ctrl+I | Toggle Properties | Toggle Properties. Ctrl+I is italic only inside the note editor. |
| Ctrl+E, Ctrl+J, Ctrl+K, Ctrl+, | Sidebar, status bar, palette, Settings | The same commands in NoteLiner. |
| Ctrl+N, Ctrl+O, Ctrl+W, Ctrl+Tab | New, Open Folder, Close Tab, Next Tab | NoteLiner's New File, Open Folder, Close Tab, Next Tab. |
| Ctrl+Shift+E, Ctrl+Shift+O, Ctrl+Shift+D, Ctrl+' | BlockLiner commands | To be checked against NoteLiner's bindings in P5. Where NoteLiner uses the keys, its command wins and the BlockLiner command stays in the palette. |

### Commands and shortcuts

| Command | Shortcut | When |
|---|---|---|
| New Diagram | (palette only) | A project is open |
| Close Tab | Ctrl+W | A tab is open |
| Next Tab | Ctrl+Tab | Two or more tabs |
| Previous Tab | Ctrl+Shift+Tab | Two or more tabs |
| Toggle Properties | Ctrl+I | A diagram is the active tab |
| Zoom Canvas In, Zoom Canvas Out, Reset Canvas Zoom | Ctrl+Shift+=, Ctrl+Shift+-, Ctrl+Shift+0 | A diagram is the active tab |
| Export Diagram | (as Export today) | A diagram is the active tab |
| The diagram edit commands | As in BlockLiner | A diagram is the active tab |

- Ctrl+PageUp and Ctrl+PageDown stay Previous File and Next File. BlockLiner also uses them for tabs; NoteLiner doesn't.
- The diagram edit commands appear in the palette, in a Diagram section, only while a diagram is the active tab.

### Settings

NoteLiner's Settings gains a Diagrams section with BlockLiner's canvas settings, saved in NoteLiner's own `ui-preferences.json`:

| Key | Holds | Default |
|---|---|---|
| `diagramShowGrid` | Whether the grid shows | true |
| `diagramGridSize` | The grid size of a new diagram (10, 20, or 40) | BlockLiner's default |
| `diagramGridUnit` | The unit for dimension lines | cm |
| `diagramShowDimensions` | Whether dimension lines show | false |
| `diagramExportFormat`, `diagramExportGrid`, `diagramExportScale` | The last export choices | BlockLiner's defaults |

The two apps keep separate settings.

### Saved state

The per-project layout in `window-state.json` gains:

| Key | Holds |
|---|---|
| `openTabs` | The open tabs' file ids, in tab order |
| `activeTab` | The active tab's file id, or empty |
| `showProperties`, `propertiesWidth` | The Properties panel's toggle and width |

- `lastFilePosition` stays, and holds the active tab's position. Other tabs come back at the top of their file.
- On opening a project, tabs whose file is gone are left out. A project saved before tabs opens with one tab, from `lastFilePosition`.

### Shared code

A new crate, `blockliner-egui`, holds the diagram editor's UI. Both apps depend on it.

| Moves from the BlockLiner app | To |
|---|---|
| `canvas/` (the canvas, tools, painting, labels, grid, view) | `blockliner-egui::canvas` |
| `properties.rs` | `blockliner-egui::properties` |
| `export.rs`, `overlays/export.rs`, `overlays/colour.rs` | `blockliner-egui::export`, `blockliner-egui::dialogs` |
| `units.rs` | `blockliner-egui::units` |
| The drawing helpers in `views.rs` (`icon_row`, `Glyph`, `paint_glyph`, `paint_tool_icon`, `arrowhead`) and the tool buttons and More Shapes popup | `blockliner-egui::tools` |
| The diagram edit commands of `Act`, with `tool_key` and `tool_digit` | `blockliner-egui::commands::EditAct` |

It adds one new type, `DiagramEditor`, which takes over the glue that is in `BlockLinerApp` today:

| `DiagramEditor` | Does |
|---|---|
| Holds | The `Diagram`, its `History`, the `Canvas`, the active tool, the More Shapes choice, and the saved snapshot used to tell whether there are unsaved edits |
| `show(ui)` | Draws the canvas and records undo steps |
| `tool_buttons(ui, direction)` | Draws the tool buttons in a row or a column |
| `properties(ui)` | Draws the Properties pane and applies its changes |
| `apply(EditAct)` | Runs one edit command |
| `keys(ctx)` | Handles the diagram shortcuts, the nudge keys, and copy, cut, and paste, when no text field has the keyboard |
| `text()`, `is_dirty()`, `mark_saved()` | The Mermaid text to write, and the autosave check |
| `take()`, `restore()` | Hands a background tab's diagram, history, and view to the app and back |

`BlockLinerApp` keeps the project, the files, the tabs, the window, the autosave, the file watch, and the settings, and holds one `DiagramEditor`. BlockLiner looks and behaves the same after the move.

`laankit-egui` gains `tabs`: BlockLiner's `tab_strip`, `TabItem`, and `TabAction`, with an optional icon per tab and the id passed in by the app.

## Changes

### laankit-egui

| File | Change |
|---|---|
| `tabs.rs` (new) | The tab strip, moved from BlockLiner, with an icon per tab. |
| `lib.rs` | The new module. |

### blockliner-egui (new crate)

| File | Change |
|---|---|
| `canvas/`, `properties.rs`, `export.rs`, `units.rs`, `tools.rs`, `dialogs.rs`, `commands.rs` | Moved from the BlockLiner app, as in the table above. |
| `editor.rs` (new) | `DiagramEditor`. |
| `Cargo.toml` | Depends on `blockliner-core`, `laankit-egui`, `egui`, `resvg`, and `image`. |

### blockliner-native

| File | Change |
|---|---|
| `app.rs` | Holds a `DiagramEditor` in place of `diagram`, `history`, `canvas`, `tool`, `more_shape`, and the clipboard fields. `apply` passes edit commands to it. Tab switching uses `take` and `restore`. |
| `commands.rs` | `Act` keeps the file, tab, and window commands and wraps `EditAct`. |
| `views.rs` | The toolbar draws the shared tool buttons. The tab strip comes from `laankit-egui`. |
| `canvas/`, `properties.rs`, `export.rs`, `units.rs`, `overlays/export.rs`, `overlays/colour.rs` | Removed (moved). |
| `Cargo.toml` | Adds `blockliner-egui`; `resvg` moves with the export. |

### noteliner-core

| File | Change |
|---|---|
| `project.rs` | A `FileKind` (Note, Diagram) read from the extension. `create_file` takes a kind and writes a blank diagram for a diagram. `rename_file` keeps the extension. `read_file`, `write_file`, and `reconcile_frontmatter` leave a diagram's text alone. `search` skips a diagram's `%%` lines. |
| `links.rs` | Diagrams are link targets but are not scanned for links. |
| `mcp.rs` | `kind` in listings, `kind` on `create_note`, the check in `update_note`, and the resource type. |
| `Cargo.toml` | Adds `blockliner-core`. |

### noteliner-native

| File | Change |
|---|---|
| `tabs.rs` (new) | The open-tab model: the tab order, the active tab, and each background tab's stored state (a note's editor state, or a diagram's `take`). |
| `app.rs` | `select` becomes open-or-activate a tab. Leaving a tab stores its state; entering one restores it or reads the file. The document area draws the tab strip, then the note editor or the diagram editor. Autosave, export, rename, delete, and Sync Reset follow the tabs. The diagram's keys are handled when a diagram is active. |
| `editor.rs` | `NoteEditor` can hand over and take back its editor state, so a note's undo history survives a tab switch. |
| `views/diagram.rs` (new) | The diagram's document toolbar, the Properties panel, and its status bar text. |
| `views/tree.rs` | The diagram icon; the menu items that don't apply are off. |
| `views/history.rs` | A diagram's version shows as source. |
| `overlays/mod.rs` | The Type row's Diagram choice. |
| `overlays/settings.rs` | The Diagrams section. |
| `commands.rs` | New Diagram, Close Tab, Next Tab, Previous Tab, Toggle Properties, and the wrapped `EditAct` commands; `View` gains `is_diagram` and the tab count. |
| `config.rs` | The diagram settings, and `openTabs`, `activeTab`, `showProperties`, and `propertiesWidth` in the layout. |
| `backend.rs` | `create_file` takes a kind. Import accepts `.mmd` and `.mermaid`. Export writes PNG, SVG, or Mermaid for a diagram. |
| `Cargo.toml` | Adds `blockliner-core` and `blockliner-egui`. |

### Docs

- `apps/noteliner/README.md` and `apps/noteliner/native/README.md`: diagrams, tabs, the new shortcuts, the settings, and the module table.
- `apps/blockliner/README.md` and `apps/blockliner/native/README.md`: the module table, and that a diagram opens in NoteLiner too.
- `ARCHITECTURE.md`: the new crate and the tab strip's new home.

### Version

NoteLiner 2.4.0: `Cargo.toml`, `Cargo.lock`, and the metainfo release. BlockLiner has no change you can see, so its version stays.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1: Shared tab strip | Move `tab_strip` into `laankit-egui::tabs` with the icon option; BlockLiner uses it. | BlockLiner's tests pass, and its tabs look the same. |
| P2: Tabs for notes | The open-tab model in NoteLiner; the editor state handed over on a switch; the tab strip; Close Tab, Next Tab, and Previous Tab; `openTabs` and `activeTab`. Notes only. | UI tests open two notes, switch with undo and cursor kept, close tabs, and restart with the tabs restored. Every existing test passes. |
| P3: The shared diagram editor | The `blockliner-egui` crate; `DiagramEditor`; the `Act` split; BlockLiner rebuilt on it. | Every BlockLiner test passes unchanged, and BlockLiner looks the same by eye in both themes. |
| P4: Diagram files in a project | `FileKind` in `noteliner-core`; create, rename, duplicate, and import; no frontmatter; the link and search rules; MCP. No canvas yet: a diagram opens as its Mermaid text, read-only. | Unit tests cover each row of the feature table that `noteliner-core` owns. An existing project opens unchanged. |
| P5: The diagram document | `DiagramEditor` in the document area; the diagram toolbar; the Properties pane; the diagram shortcuts and the clash check; autosave; export; the Diagrams settings; the History source view. | UI tests draw, edit, undo, autosave, and export a diagram in NoteLiner, and switch between a note tab and a diagram tab. A diagram made in NoteLiner opens in BlockLiner, and one made in BlockLiner opens in NoteLiner. Checked by eye in both themes at 100% and 200%. |
| P6: Docs and version | The READMEs, `ARCHITECTURE.md`, and NoteLiner 2.4.0. | The docs match the apps; clippy and every test pass. |

P1 and P2 don't depend on P3 to P5, and P3 doesn't depend on P1 and P2.

## Tests

- `laankit-egui`: the tab strip reports Activate and Close, and closes on a middle-click.
- `noteliner-core`:
  - A diagram is created as `.mmd` with no frontmatter, and stays `.mmd` when renamed and duplicated.
  - Opening a project with Write Frontmatter on leaves a `.mmd` file byte for byte.
  - A subroutine shape (`a[[Label]]`) in a diagram adds no link, and `[[Diagram]]` in a note resolves to the diagram.
  - Search finds a label and doesn't match a `%%` line.
  - MCP: `kind` in listings, `create_note` with `kind: diagram`, `update_note` refusing text that doesn't parse, and `rename_note` keeping the extension.
- `blockliner-egui`: `DiagramEditor` applies an edit, undoes it, reports dirty, and round-trips through `take` and `restore` with its history and view.
- NoteLiner UI tests, in a new `ui_tests_tabs.rs` and `ui_tests_diagram.rs`:
  - Two notes in two tabs: type in one, switch, switch back, and undo still works.
  - Close Tab with the button, a middle-click, and Ctrl+W; closing the active tab activates its neighbour; closing the last shows the empty state.
  - Next File reuses the active tab; choosing a file in the tree opens a new one.
  - Rename and delete follow through to the tab.
  - Restart: the tabs and the active tab come back.
  - New Diagram creates a `.mmd`, opens the canvas, and shows the tool buttons in the document toolbar.
  - Drawing a rectangle with the toolbar button and with the R key writes it to disk; R typed into Search doesn't draw.
  - A note tab and a diagram tab: the toolbar and the right-hand panel change with the active tab.
  - Export a diagram to SVG (a test hook in place of the dialog, as in BlockLiner).
- BlockLiner's existing UI tests are the check on P1 and P3.

## Out of scope

- Showing a diagram inside a note: an embed such as `![[Architecture]]` in the preview and in the HTML and PDF exports. This is the natural next step and is left for its own plan.
- Drawing `mermaid` code fences in the preview.
- Mermaid diagram types other than the flowcharts BlockLiner edits.
- A diagram in a presentation's slides.
- Diagram templates.
- Reloading an open tab when its file changes outside NoteLiner, including through MCP. NoteLiner has no file watch today, and this plan doesn't add one.
- Dragging tabs to reorder them, split views, and Close Others and Close All.
- A rendered diagram in the History pane; it shows the source.
- BlockLiner's project and Files panes in NoteLiner. NoteLiner's Files tree does that job.

## Open questions

All answered: see Decisions above.
