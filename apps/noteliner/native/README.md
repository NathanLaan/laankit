# NoteLiner Native

NoteLiner in pure Rust: an [egui](https://github.com/emilk/egui) UI over `noteliner-core`, with no webview or JavaScript. It covers the app shell, the backend, the tree of folders, the dialogs, the Markdown editor (`crates/laankit-editor`), the preview, the SEARCH / TAGS / BACKLINKS panes, the Attachments panel, presentations (`crates/noteliner-slides`), PDF export (`crates/noteliner-export`), and the MCP dialogs.

A project is a folder of notes and sub-folders, read and written through `noteliner-core` (see [`../README.md`](../README.md) for what a project is). Settings are in `~/.config/NoteLiner` (`ui-preferences.json`, `recent-projects.json`, `window-state.json`), with the MCP runtime file.

## Running

```bash
cargo run -p noteliner-native --release     # or: npm run native:noteliner
cargo test -p noteliner-native
```

The package is `noteliner-native`; its binary is `noteliner`, the one the release packages install as `/usr/bin/noteliner` (see [`docs/packaging.md`](../../../docs/packaging.md)). It runs no `git` command and doesn't need Git installed.

- MCP. Settings → MCP turns the server on. Point MCP clients at `noteliner --mcp-bridge` (the settings show the exact snippet); the bridge finds the running app through `mcp-runtime.json`, as the Tauri binary's `--mcp-bridge` does. The server needs a Unix socket, so on Windows it reports that it isn't available yet.
- Window chrome. Settings → UI → Custom Window Titlebar switches between the system frame and LaanKit's title bar (takes a restart).
- Command palette. Ctrl+K or Ctrl+Shift+P: every command, plus notes, tags, and recent projects.
- Markdown files dropped on the window become notes in the selected folder; files dropped on the editor (or pasted, including images) become attachments of the open note.
- Presenting. F5, the toolbar's play button, "Present" in the palette, a deck's right-click menu in the Files pane, or the Slides pane's header button shows the deck full screen from the caret's slide (arrows, Space, Page Up/Down, S for speaker notes, F11 to leave and return to full screen, Escape). Shift+F5 starts at the first slide, and "Present from This Slide" in a slide's menu starts there. Moving the mouse shows a bar with Previous, Next, the slide number, and Exit for three seconds.
- Choosing the display. The app reads the displays from the window about once a second (`read_displays`, through `eframe::Frame::winit_window`) and hands them to the presenter. With two or more, a presentation's menu in the Files pane and the palette offer "Present on Display N", D and the controls' display button move to the next display while presenting, and the choice is saved by name and size in `ui-preferences.json` (`presentDisplay`), so F5 uses it whenever that display is connected. "Present on This Window's Display" forgets it. This is not yet checked on two real displays; `cargo run -p noteliner-slides --example displays` is the trial program for that.
- The left toolbar keeps its standard buttons (Files, Outline, Tags, Attachments, Search, Backlinks) in the same place for every file type. Buttons for one file type are context buttons and always come after them, below a divider: today those are Slides, Present, and Present with Speaker View, for a presentation. The toolbar doesn't scroll, so a context button that doesn't fit above the bottom group is left out (`toolbar::fits`). Put a new button for one file type in the context group.
- Spell check (F7) needs a Hunspell dictionary for your language; see "Spell checking" below.

### Trying it without touching your projects

```bash
export XDG_CONFIG_HOME=$(mktemp -d)
cargo run -p noteliner-native --example noteliner_demo_data -- /tmp/nl-demo
cargo run -p noteliner-native --release          # the demo is in Recent Projects
```

`noteliner_demo_data` creates a project with notes in folders, tags, wikilinks, a template, and an image attachment, and lists it in `$XDG_CONFIG_HOME/NoteLiner/recent-projects.json` (Linux). On Windows the config folder is `%APPDATA%\NoteLiner` and on macOS `~/Library/Application Support/NoteLiner`, as `laankit_core::paths` mirrors Electron's `userData`.

### Diagnostics

`LAANKIT_SCREENSHOT=<file.png>` (with `LAANKIT_SCREENSHOT_EXIT=1`), `LAANKIT_THEME`, and `LAANKIT_SCALE` work as in every LaanKit app (see `laankit_egui::diagnostics`). Also:

| Variable          | Effect                                                                                    |
|-------------------|-------------------------------------------------------------------------------------------|
| `LAANKIT_PROJECT`  | Open this project folder at launch                                                        |
| `LAANKIT_FILE`     | Then select the nth note in display order, or the note with this name                     |
| `LAANKIT_OPEN`     | Then open `settings[:ui\|mcp\|shortcuts]`, `about`, `help`, `palette`, `new-file`, `new-folder`, `move`, `preview`, `slides`, `presentation-settings`, `present`, `log`, `search:<query>`, `tags`, `backlinks`, `attachments`, `tag-filter`, `tag-editor`, `mcp-walkthrough`, or `mcp-confirm` (a sample request) |

The Help pages are `assets/help-pages.json` (loaded at build time with `include_str!`); edit that file to change the in-app Help.

## Architecture

| Module           | What it does                                                                                       |
|------------------|----------------------------------------------------------------------------------------------------|
| `main`           | The eframe window (1200 × 800, at least 800 × 600), icon, undecorated with the custom titlebar; `--mcp-bridge` |
| `app`            | Owns everything; routes backend events, commands, and view actions |
| `backend`        | A tokio runtime around the shared `noteliner-core` workspace and MCP server: file and folder operations, the check for changes made by other programs, exports, and prefs; results come back as `Event`s |
| `config`         | `ui-preferences.json`, `recent-projects.json` and the per-project `Layout` in `window-state.json`   |
| `model`          | Typed index entries, the tree the folders make (folders first, then the sorted notes), and the tag edits on the index JSON |
| `commands`       | `registerCommands`: every command and shortcut, for the keyboard, the palette, and Settings          |
| `editor`         | `NoteEditor`: `laankit-editor`'s widget with the autosave, status, and event contract (see below)     |
| `tabs`           | The open tabs: their order, and the state of the tabs in the background (see Tabs)                   |
| `diagram`        | `DiagramDoc`: `blockliner-egui`'s editor with the same autosave and status contract (see Diagrams)   |
| `spell`          | Hunspell dictionary lookup, background loading, and the personal word list                           |
| `deck`           | The open deck, parsed per edit, and the `noteliner-slides` widgets (pane, preview, presenter)       |
| `views`          | `OpenScreen`, `StatusBar`, `LogPanel`; `views::tree` (`FileTree`), `views::sidebar` (`Sidebar`), `views::panes` (`SearchPane`, `BacklinksPane`, `TagsPane`, `TagFilterPopover`), `views::attachments` (`AttachmentPanel`), `views::preview` (`Preview`), `views::diagram` (the diagram toolbar and the Properties panel) |
| `overlays`       | The dialogs: New Item (a note, presentation, diagram, or folder) / Duplicate, Move, the delete questions (a note, a folder, and deleting for good when the trash isn't available), the question for a note changed on disk under unsaved edits, Clear Tags, Remove Recent, Settings, Help, About, the Importing dialog, the Tag Editor (`overlays::tags`), Remove Attachment, and the MCP confirmation and walkthrough (`overlays::mcp`) |
| `svg_colors`     | The SVG named colours for tag chips (`lib/svgColors.js`)                                            |
| `ui_tests`       | egui_kittest: open a plain folder → new file → type → autosave → rename → new folder → drag-and-drop move → delete to the trash |
| `ui_tests_folders` | egui_kittest: a plain folder opens with its sub-folders; New Folder from the header, a folder's menu, and the palette; New Item making a folder in the parent you pick; a new note lands in the selected folder; renaming, moving, and deleting a folder with its notes; a note opens with its frontmatter, and tags move both ways between the block and the Tags pane; a search result opens on the right line; and changes made by other programs (added, changed, moved, removed, and the Reload / Keep Mine question) |
| `ui_tests_panes` | egui_kittest: search → jump, backlinks → jump, tags pane / filter / editor, attachments insert and remove, MCP allow / session / deny; and a PDF export read back with hayro |
| `ui_tests_tabs`  | egui_kittest: each tab keeps its text and undo history, closing tabs shows the neighbour, Next File reuses the active tab, and the tabs come back when the project reopens |
| `ui_tests_diagram` | egui_kittest: New Diagram creates a `.mmd` and shows the canvas and the tools, drawing with a button and with a tool key autosaves, a tool key typed into Search doesn't draw, a note tab and a diagram tab keep their state, Export writes an SVG, and the toolbar's stacking-order buttons move a shape and the new order autosaves |
| `ui_tests_editor`| egui_kittest: typing autosaves, list Enter, `[[` completion, a pasted image becomes an attachment and its Markdown, and Ctrl+click on a wikilink; SLIDES pane insert → disk, presentation settings save, presenter open/close; reopening a project restores the last note's caret and scroll (and only that note's) |

### Editor

`src/editor.rs` holds `NoteEditor`, a wrapper around `laankit-editor` (see `crates/laankit-editor/README.md`). The editor runs on its own core engine; `LAANKIT_EDITOR_ENGINE=textedit noteliner` falls back to the previous engine (egui's `TextEdit`) for one release, should the core engine misbehave with an input method or screen reader. The app only uses this API:

- `load(id, name, filename, file)` when the file has been read, with its frontmatter; `clear()`; `renamed(id, name, filename)`.
- `text()` is the note's text without its frontmatter, which is what the preview, slides, outline, and counts read. `file_text()` is the whole file. Lines given to `scroll_to_line` and taken from `body_line()` are lines of the text.
- `frontmatter()` is the block as last read, and `frontmatter_unreadable()` says a half-typed line stops it from being read.
- `set_frontmatter(file)`, `retag(entry)`, and `set_presentation(value)` swap in a block NoteLiner changed itself (a tag from the Tags pane, Presentation Settings, a field a save put back). It is not an undo step, and the note isn't marked as edited.
- `replace(id, file)` takes a file changed on disk when no edit is pending; `hold(true)` stops the autosave while the app asks, and `reload(id, file)` takes the file over a pending edit.
- `show(ui, preview_on) -> Vec<EditorAction>`: the toolbar (list buttons and Preview) and the text. It returns `Save(SaveRequest)` when the 500 ms autosave timer fires (`scheduleSave`), the toolbar toggles, the context menu's exports, `PasteImage` / `AttachFiles` (saved through `file:addAttachment`, then the Markdown reference is inserted, as `processFiles` does), `OpenWikilink` / `OpenLink` (Ctrl/Cmd+click) and `AddToDictionary`.
- `saved(id, content, ok)` when the write finishes; `flush()` before switching notes, closing, or quitting, so a pending edit lands in the note it was made in.
- `apply_slide_edit(markdown, caret_line)`: `applySlideEdit` (one undo step, written at once).
- `insert(snippet, now)` / `insert_at(char, snippet, now)`: the Attachments panel's "Insert into Note", pasted attachments, and a note dragged from FILES onto the text (a link at the drop point, as a path from the open note's folder, such as `[name](../archive/file.md)`).
- `scroll_to_line(line)`; `status()`, `cursor()` (the line and column in the file), `selection_len()`, `selected_lines()`.
- `go_to_line()`: opens the editor's Go to line bar (Ctrl+Alt+G, or "Go to Line" in the palette).
- `view()` / `restore_view(anchor, head, scroll)`: the open note's caret and scroll, and putting them back (see below).

`[[` completes the names of the other notes in the index (`wikilinkCompletions`). The editor's display matches the CodeMirror setup (14 px, line numbers, wrapping); only `spellCheckEnabled` is a preference.

Reopening a project opens the tabs that were open and shows the active one, with its caret and scroll where they were (the editor plan's decision 3). Only the active note's place is remembered: the app keeps `lastFilePosition: { fileId, anchor, cursor, scrollX, scroll }` in the project's layout in `window-state.json` (the Tauri shape, plus this key, which the Svelte build ignores), updates it as the caret and scroll move, and saves it with the layout (debounced) and on close and quit. It's applied once the note has loaded, and ignored if the note is gone. Other notes open at the top, as in the Svelte build.

### Tabs

`src/tabs.rs` holds the open tabs. The active tab is the project's selected file, and its state lives in the app's `NoteEditor` or `DiagramDoc`. Leaving a tab writes its pending edit and stores its state in `Tabs`; entering one takes the state back, or reads the file if the tab has none. The tab strip is `laankit_egui::tabs`, which BlockLiner uses too.

| Action | Does |
|---|---|
| Choosing a file (the tree, a search result, a link, the palette) | Opens its tab at the end of the strip, or shows the tab it has |
| Previous File and Next File (Ctrl+PgUp, Ctrl+PgDn) | Show the file in the active tab's place, or show the tab it has |
| Close Tab (Ctrl+W, the tab's close button, a middle-click) | Writes the pending edit and shows the tab to the right, else the one to the left |
| Next Tab and Previous Tab (Ctrl+Tab, Ctrl+Shift+Tab) | Step through the strip, wrapping around |

A note in a background tab keeps its text, undo history, caret, and scroll. A diagram keeps its undo history and its pan and zoom. Closing a tab drops them. The layout in `window-state.json` keeps `openTabs` (the file ids in tab order) and `activeTab`; tabs whose file is gone are left out when the project reopens.

### Diagrams

A file whose name ends in `.mmd` is a diagram: a Mermaid flowchart in BlockLiner's format (see `apps/blockliner/README.md`). `noteliner-core` reads the kind from the extension (`FileKind`), writes no frontmatter into a diagram, doesn't scan it for wikilinks (`id[[label]]` is Mermaid's subroutine shape), and skips its `%%` layout lines in search.

`src/diagram.rs` holds `DiagramDoc`, a wrapper around `blockliner_egui::DiagramEditor` with the note editor's contract: `load` when `file:read` arrives, a write 500 ms after the diagram last changed, `flush` before switching tabs, and `take` and `restore` for a background tab. A diagram that BlockLiner can't read isn't opened, and its error is shown.

While a diagram is the active tab:

- The toolbar above the document shows BlockLiner's tools, Undo and Redo, the four stacking-order buttons (Move to Front, Move Up, Move Down, Move to Back), the grid, the dimension lines, the zoom, and the Properties and Export toggles.
- The Properties panel is to the right of the canvas (Ctrl+I), where the preview is for a note.
- BlockLiner's edit commands apply, with the same keys: the tool keys (V, R, O, and the rest), the digits, Delete, the arrow keys, Enter, Esc, Ctrl+Z, Ctrl+Shift+Z, Ctrl+A, Ctrl+B, Ctrl+] and Ctrl+[ (with Shift for the front and the back), and copy, cut, and paste. They are registered from `blockliner_egui::commands` and wait while a text field has the keyboard.
- Ctrl+=, Ctrl+-, and Ctrl+0 zoom NoteLiner's interface, as always. Ctrl+Shift+=, Ctrl+Shift+-, and Ctrl+Shift+0 zoom the canvas.
- Export Diagram writes a PNG or an SVG to the Downloads folder, and Save Diagram to Mermaid copies the `.mmd` there.

Settings has a Diagrams tab for the grid, the grid size of a new diagram, the unit of a grid mark, and the dimension lines. They are NoteLiner's own settings (`diagramShowGrid`, `diagramGridSize`, `diagramGridUnit`, `diagramShowDimensions`, and the `diagramExport*` keys in `ui-preferences.json`); BlockLiner's are separate. The layout keeps `showProperties` and `propertiesWidth`.

### Spell checking

The webview builds used the system spell checker. The native build uses Hunspell dictionaries (`laankit-editor`'s `spellcheck` feature, pure Rust): the system language (`LANG`, then `en_US`) is looked up in `<userData>/dictionaries/`, then `/usr/share/hunspell` and `/usr/share/myspell` (Linux) or `~/Library/Spelling` and `/Library/Spelling` (macOS). Windows has no shared Hunspell folder: put `en_US.aff` and `en_US.dic` (for example LibreOffice's) in `%APPDATA%\NoteLiner\dictionaries`. Without a dictionary nothing is underlined. "Add to dictionary" words go to `<userData>/dictionary.txt`. The dictionary loads on a background thread at startup (or when F7 turns checking on).

### Presentations

`src/deck.rs` follows `crates/noteliner-slides/README.md`'s contract: the deck is re-parsed when the note's text or `presentation:` block changes; the SLIDES pane (jump, rename, drag, context menu) and the deck commands (New Slide, Split, Move Up/Down, Speaker Notes) apply their edits through `apply_slide_edit`; the preview column renders a deck as slides; Presentation Settings writes the block through `file:setPresentation`; "Convert to Presentation" lands in the settings with the SLIDES pane open, as in Svelte. The status bar shows the caret's slide and the speaking time at 130 words a minute. Slide images come from `_attachments/` in the note's folder, like the preview's. Present (F5, toolbar, the Files and Slides panes) is full screen; Present from the First Slide (Shift+F5, palette) starts at slide 1; Present with Speaker View (Alt+F5, PowerPoint's Presenter View key; toolbar, palette, and the Files pane's menu) adds the speaker window (see `crates/noteliner-slides/README.md`).

### Math

`$…$`, `$$…$$`, and ```` ```math ```` blocks are typeset in the preview, slides, and speaker notes (`noteliner_core::export::preview_html` turns on comrak's math; `laankit_richtext::from_note_html` keeps it) and in PDF export. `src/math.rs` installs Typst (through the warm PDF exporter) as `laankit-richtext`'s math renderer when the backend attaches. LaTeX it can't convert shows as its source. The webview builds showed math as text; the HTML export still does.

### PDF export

`Backend::export(ExportKind::Pdf, …)` renders the note in-process with `noteliner-export` (Typst): a `LazyExporter` is built and warmed up on a background thread when the app starts, and each export runs on a blocking thread. The options match today's WebKit print (`export::pdf_document`): Letter, 0.5 in margins, the note's name as the title heading, images from `_attachments/` in the note's folder, wikilinks as styled text. The PDF goes to the Downloads folder, which then opens, like the HTML and Markdown exports.

## Parity with the Svelte build

The tables list what the Svelte builds had and where it is here. NoteLiner 3 leaves them in three ways: a project is a plain folder with no Git (no sync, history, or setup dialogs), the tree is the folders on disk in place of notes under notes, and the editor shows a note's frontmatter.

### IPC channels

| Channel                                                   | Status  | Notes                                                                   |
|-----------------------------------------------------------|---------|-------------------------------------------------------------------------|
| `dialog:openFolder`                                       | Done    | `rfd` folder picker                                                     |
| `project:open`, `project:close`                           | Done    | Any folder opens; close writes the pending edit                         |
| `project:init`                                            | Removed | A new project is an empty folder you open                               |
| `project:getIndex`, `project:saveIndex`                   | Changed | The backend's index is the one that counts; the app sends its tag changes |
| `file:read`, `file:getFrontmatter`, `file:write`          | Changed | The whole file is read and written, frontmatter included; saves refresh the entry's `modifiedAt`, `deck`, and `tags` |
| `file:create`, `file:duplicate`                           | Done    | Templates, folder, tags; Presentation seeds the deck body and block      |
| `file:delete`, `file:rename`                              | Done    | Delete sends the file to the trash                                      |
| Folders: create, rename, move, delete; move a note        | Native addition | See Folders in [`../README.md`](../README.md)                   |
| The check for changes made by other programs              | Native addition | Every few seconds while the window has the focus                |
| `file:setPresentation`                                    | Done    | Convert to / from presentation, and the Presentation Settings dialog    |
| `templates:list`, `templates:save`                        | Done    | Save as Template logs to the Log panel                                  |
| `dialog:openImportFile`, `file:import`                    | Done    | DOCX / PPTX, with the "Importing" dialog and the Log summary            |
| `file:convertToHtml`, `file:convertToMarkdown`            | Done    | To Downloads, then the folder opens                                     |
| `file:convertToPdf`                                       | Done    | `noteliner-export` (Typst) instead of WebKit print; see PDF export      |
| `links:getBacklinks`, `links:getAllNames`, `links:rebuild` | Done    | BACKLINKS pane; `[[` completion uses the index names, as the Svelte editor does; links rebuild on every change |
| `search:query`                                            | Done    | SEARCH pane, 300 ms debounce, case-insensitive                          |
| `file:getHistory`, `file:getHistoryContent`               | Removed | NoteLiner keeps no history of a note's earlier versions                 |
| `file:addAttachment`, `file:removeAttachment`, `dialog:openFiles` | Done | Attachments panel (add, open, insert, remove); images and files pasted or dropped on the editor |
| `file:getAttachmentPath`, `attachment://`                 | Done    | The preview reads `_attachments/` in the note's folder directly         |
| `git:*`                                                   | Removed | NoteLiner runs no `git` command                                         |
| `projects:getRecent`, `addRecent`, `removeRecent`         | Done    | Same `recent-projects.json` shape                                       |
| `window-state:getLayout`, `saveLayout`, `restoreBounds`   | Done    | Same `window-state.json`; panel layout and window bounds per project, plus the last-opened note's caret and scroll (`lastFilePosition`, native only) |
| `mcp:getStatus`, `mcp:confirm-response`, `mcp:confirm-request` | Done | Settings → MCP; the full confirmation dialog with raw arguments         |
| `ui:getPrefs`, `ui:setPrefs` and their side effects       | Done    | `writeFrontmatter` and `mcpEnabled` apply live                          |
| `ui:spellcheck-changed`                                   | Done    | F7 and Settings; Hunspell dictionaries instead of the webview's checker (see Spell checking) |
| `update:*`                                                | Done    | About → Check for Updates (`laankit_core::updates`)                      |
| `help:open`                                               | Done    | In-app Help dialog instead of a second window                           |
| `app:relaunch`                                            | Done    | After the titlebar setting                                              |
| `--mcp-bridge`                                            | Done    | Unix only for now (see above)                                           |
| Single instance                                           | Not yet |                                                                         |

### Components

| Svelte                                         | Status  | Notes                                                                 |
|------------------------------------------------|---------|-----------------------------------------------------------------------|
| `App.svelte` layout, commands, shortcuts, Ctrl+wheel zoom | Done | Every command, including the Presentation section; plus Present (F5) |
| `OpenScreen`, `RemoveRecentModal`              | Done    | One Open Folder action                                                |
| `NewProjectModal`, `SetupModal`                | Removed | Opening a folder always works                                         |
| `Toolbar`, `TitleBar` (custom titlebar)        | Done    |                                                                       |
| `Sidebar` (PaneHost order, heights, close)     | Done    |                                                                       |
| `FileTree`                                     | Changed | The folders on disk, with the notes in them: select, arrows, F2 / double-click rename, drag into a folder, and menus for a note, a folder, and the empty part of the tree. No notes under notes and no drag to reorder |
| `NewFileModal` (New Item, Duplicate), `MoveFileModal` | Changed | They take a folder in place of a parent note; the Folder type in New Item is a native addition |
| `DeleteFileModal`, `ClearTagsModal`            | Done    | Plus the questions for deleting a folder, and for deleting for good when the trash isn't available |
| `OutlinePane`                                  | Done    |                                                                       |
| `Editor`                                       | Done    | `laankit-editor` on its core engine: highlighting (code fences by language), list editing with ordered-list renumbering, shortcuts, find/replace, Go to line (Ctrl+Alt+G), bracket and selection matches, `[[` completion, paste/drop attachments, spell check, several cursors (Ctrl+click, Ctrl+D, find "all", Alt+drag), folding (gutter, Ctrl+Shift+[ / ]), and notes of 50,000+ lines. Its own parity table lists what's left |
| `Preview`                                      | Done    | comrak + laankit-richtext; wikilinks open or create notes; attachments |
| `StatusBar`, `LogPanel`                        | Done    | Including a deck's slide and speaking time. No branch chip; the log is the Activity Log, for MCP activity and import notes |
| `SettingsModal` (UI, MCP, Keyboard Shortcuts)  | Done    | Turning MCP on opens the walkthrough                                  |
| `ProjectSettingsModal`, `SyncModal`, `SyncingModal` | Removed | The project's folder is shown in Settings                          |
| `ImportingModal`                               | Done    |                                                                       |
| `AboutModal`, Help (`HelpApp`)                 | Done    |                                                                       |
| `McpConfirmModal`                              | Done    | Tool, action, raw arguments (payloads cut), Allow once / for session / Deny |
| `McpWalkthroughModal`                          | Done    | The client snippet uses `noteliner --mcp-bridge`                |
| `TagsPane`, `TagFilterPopover`, `TagEditorModal` | Done  | Chips toggle, Ctrl+T adds, notes dropped from FILES get the tag; filter with No Tags / Show all / Hide all; colours from the SVG palette |
| `SearchPane`, `BacklinksPane`                  | Done    | Results jump to the line                                              |
| `HistoryPanel`                                 | Removed |                                                                       |
| `AttachmentPanel`                              | Done    | Adds "Insert into Note" / "Copy Markdown" (native additions)          |
| `SlidesPane`, `DeckPreview`, `Slide`, `PresentationSettingsModal` | Done | `crates/noteliner-slides`; its parity table lists the gaps (`class=`, table alignment, local video, `<mark>`) |
| Presenter                                      | Native addition | Full screen, keys, speaker notes; the Svelte build has none   |
| `ContextMenu`                                  | Done    | egui context menus                                                    |
