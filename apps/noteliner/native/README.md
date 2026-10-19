# NoteLiner Native

NoteLiner in pure Rust: an [egui](https://github.com/emilk/egui) UI over `noteliner-core`, with no
webview or JavaScript. It's Phase 4 of plan Option C
([`docs/plans/plan-native-rust-egui.md`](../../../docs/plans/plan-native-rust-egui.md)): the app
shell, the backend, the file tree, the dialogs, the Markdown editor (`crates/marina-editor`), the
preview, the SEARCH / TAGS / BACKLINKS panes, the History and Attachments panels, presentations
(`crates/noteliner-slides`), PDF export (`crates/noteliner-export`) and the MCP dialogs.

It reads and writes exactly what the Electron and Tauri builds do: the same projects
(`noteliner.json`, the notes and their frontmatter, `_attachments/`, `_templates/`, git commits),
the same `~/.config/NoteLiner` (`ui-preferences.json`, `recent-projects.json`,
`window-state.json`) and the same MCP runtime file and socket, through the same `noteliner-core`
workspace. You can switch builds on the same project at any time.

## Running

```bash
cargo run -p noteliner-native --release     # or: npm run native:noteliner
cargo test -p noteliner-native
```

The package is `noteliner-native`; its binary is `noteliner`, the one the release packages install
as `/usr/bin/noteliner` (the native build replaced the Tauri and Electron ones; see
[`docs/packaging.md`](../../../docs/packaging.md)). It needs `git` on the PATH, like the other
builds.

- **MCP.** Settings → MCP turns the server on. Point MCP clients at `noteliner
  --mcp-bridge` (the settings show the exact snippet); the bridge finds the running app through
  `mcp-runtime.json`, as the Tauri binary's `--mcp-bridge` does. The server needs a Unix socket, so
  on Windows it reports that it isn't available yet.
- **Window chrome.** Settings → UI → Custom Window Titlebar switches between the system frame and
  Marina's title bar (takes a restart).
- **Command palette.** Ctrl+K or Ctrl+Shift+P: every command, plus notes, tags and recent
  projects.
- **Markdown files** dropped on the window become notes; files dropped on the editor (or
  pasted, including images) become attachments of the open note.
- **Presenting.** F5, the toolbar's play button or "Present" in the palette shows the deck full
  screen from the caret's slide (arrows, Space, Page Up/Down, S for speaker notes, Escape).
- **Spell check** (F7) needs a Hunspell dictionary for your language; see "Spell checking" below.

### Trying it without touching your projects

```bash
export XDG_CONFIG_HOME=$(mktemp -d)
cargo run -p noteliner-native --example demo_data -- /tmp/nl-demo
cargo run -p noteliner-native --release          # the demo is in Recent Projects
```

`demo_data` creates a git project with nested notes, tags, wikilinks, a template and an image
attachment, and lists it in `$XDG_CONFIG_HOME/NoteLiner/recent-projects.json` (Linux). On Windows
the config folder is `%APPDATA%\NoteLiner` and on macOS `~/Library/Application Support/NoteLiner`,
as `marina_core::paths` mirrors Electron's `userData`.

### Diagnostics

`MARINA_SCREENSHOT=<file.png>` (with `MARINA_SCREENSHOT_EXIT=1`), `MARINA_THEME` and `MARINA_SCALE`
work as in every Marina app (see `marina_egui::diagnostics`). Also:

| Variable          | Effect                                                                                    |
|-------------------|-------------------------------------------------------------------------------------------|
| `MARINA_PROJECT`  | Open this project folder at launch                                                        |
| `MARINA_FILE`     | Then select the nth note in display order, or the note with this name                     |
| `MARINA_OPEN`     | Then open `settings[:ui\|mcp\|shortcuts]`, `sync`, `about`, `help`, `palette`, `new-file`, `new-project`, `project-settings`, `move`, `preview`, `slides`, `presentation-settings`, `present`, `log`, `search:<query>`, `tags`, `backlinks`, `history`, `attachments`, `tag-filter`, `tag-editor`, `mcp-walkthrough` or `mcp-confirm` (a sample request) |

The Help pages are `assets/help-pages.json`, generated from the Svelte build's
`lib/helpContent.js`: run `node apps/noteliner/native/scripts/gen-help.mjs` after editing it (a
test fails when the file is stale).

## Architecture

| Module           | What it does                                                                                       |
|------------------|----------------------------------------------------------------------------------------------------|
| `main`           | The eframe window (1200 × 800, at least 800 × 600), icon, undecorated with the custom titlebar; `--mcp-bridge` |
| `app`            | Owns everything; routes backend events, commands and view actions (`App.svelte`, `stores/project.svelte.js`, `stores/log.svelte.js`) |
| `backend`        | A tokio runtime around the shared `noteliner-core` workspace and MCP server: every channel of the Tauri host; results come back as `Event`s |
| `config`         | `ui-preferences.json`, `recent-projects.json` and the per-project `Layout` in `window-state.json`   |
| `model`          | Typed index entries, the tree order and sorts, drag-and-drop / move / clear-tags edits on the index JSON |
| `commands`       | `registerCommands`: every command and shortcut, for the keyboard, the palette and Settings          |
| `editor`         | `NoteEditor`: `marina-editor`'s widget with the autosave, status and event contract (see below)     |
| `spell`          | Hunspell dictionary lookup, background loading and the personal word list                           |
| `deck`           | The open deck, parsed per edit, and the `noteliner-slides` widgets (pane, preview, presenter)       |
| `views`          | `OpenScreen`, `StatusBar`, `LogPanel`; `views::tree` (`FileTree`), `views::sidebar` (`Sidebar`), `views::panes` (`SearchPane`, `BacklinksPane`, `TagsPane`, `TagFilterPopover`), `views::history` (`HistoryPanel`), `views::attachments` (`AttachmentPanel`), `views::preview` (`Preview`) |
| `overlays`       | The dialogs: New Project, Set Up, New File / Duplicate, Move, Delete, Clear Tags, Remove Recent, Project Settings, Settings, Remote Sync, Help, About, the busy dialogs, the Tag Editor (`overlays::tags`), Remove Attachment, and the MCP confirmation and walkthrough (`overlays::mcp`) |
| `svg_colors`     | The SVG named colours for tag chips (`lib/svgColors.js`)                                            |
| `ui_tests`       | egui_kittest: new project → new file → type → autosave → rename → drag-and-drop move → delete       |
| `ui_tests_panes` | egui_kittest: search → jump, backlinks → jump, history, tags pane / filter / editor, attachments insert and remove, MCP allow / session / deny; and a PDF export read back with hayro |
| `ui_tests_editor`| egui_kittest: typing autosaves, list Enter, `[[` completion, a pasted image becomes an attachment and its Markdown, Ctrl+click on a wikilink; SLIDES pane insert → disk, presentation settings save, presenter open/close; reopening a project restores the last note's caret and scroll (and only that note's) |

### Editor

`src/editor.rs` holds `NoteEditor`, a wrapper around `marina-editor` (see
`crates/marina-editor/README.md`). The editor runs on its own core engine;
`MARINA_EDITOR_ENGINE=textedit noteliner` falls back to the previous engine
(egui's `TextEdit`) for one release, should the core engine misbehave with an
input method or screen reader. The app only uses this API:

- `load(id, name, filename, body)` when `file:read` arrives; `clear()`; `renamed(id, name, filename)`.
- `show(ui, preview_on, history_on) -> Vec<EditorAction>`: the toolbar (list buttons, History,
  Preview) and the text. It returns `Save(SaveRequest)` when the 500 ms autosave timer fires
  (`scheduleSave`), the toolbar toggles, the context menu's exports, `PasteImage` / `AttachFiles`
  (saved through `file:addAttachment`, then the Markdown reference is inserted, as `processFiles`
  does), `OpenWikilink` / `OpenLink` (Ctrl/Cmd+click) and `AddToDictionary`.
- `saved(id, content, ok)` when the write finishes; `flush()` before switching notes, closing or
  quitting, so a pending edit lands in the note it was made in.
- `apply_slide_edit(markdown, caret_line)`: `applySlideEdit` (one undo step, written at once).
- `insert(snippet, now)` / `insert_at(char, snippet, now)`: the Attachments panel's "Insert into
  Note", pasted attachments, and a note dragged from FILES onto the text (a `[name](./file.md)`
  link at the drop point, `interceptInternalDrops`).
- `scroll_to_line(line)`; `status()`, `cursor()`, `selection_len()`, `selected_lines()`, `text()`.
- `go_to_line()`: opens the editor's Go to line bar (Ctrl+Alt+G, or "Go to Line" in the palette).
- `view()` / `restore_view(anchor, head, scroll)`: the open note's caret and scroll, and putting
  them back (see below).

`[[` completes the names of the other notes in the index (`wikilinkCompletions`). The editor's
display matches the CodeMirror setup (14 px, line numbers, wrapping); only `spellCheckEnabled` is
a preference.

**Reopening a project** opens the note that was open, with its caret and scroll where they were
(the editor plan's decision 3). Only that one note is remembered: the app keeps
`lastFilePosition: { fileId, anchor, cursor, scrollX, scroll }` in the project's layout in
`window-state.json` (the Tauri shape, plus this key, which the Svelte build ignores), updates it
as the caret and scroll move, and saves it with the layout (debounced) and on close and quit. It's
applied once the note has loaded, and ignored if the note is gone. Other notes open at the top, as
in the Svelte build.

### Spell checking

The webview builds used the system spell checker. The native build uses Hunspell dictionaries
(`marina-editor`'s `spellcheck` feature, pure Rust): the system language (`LANG`, then `en_US`)
is looked up in `<userData>/dictionaries/`, then `/usr/share/hunspell` and `/usr/share/myspell`
(Linux) or `~/Library/Spelling` and `/Library/Spelling` (macOS). Windows has no shared Hunspell
folder: put `en_US.aff` and `en_US.dic` (for example LibreOffice's) in
`%APPDATA%\NoteLiner\dictionaries`. Without a dictionary nothing is underlined. "Add to
dictionary" words go to `<userData>/dictionary.txt`. The dictionary loads on a background thread
at startup (or when F7 turns checking on).

### Presentations

`src/deck.rs` follows `crates/noteliner-slides/README.md`'s contract: the deck is re-parsed when
the note's text or `presentation:` block changes; the SLIDES pane (jump, rename, drag, context
menu) and the deck commands (New Slide, Split, Move Up/Down, Speaker Notes) apply their edits
through `apply_slide_edit`; the preview column renders a deck as slides; Presentation Settings
writes the block through `file:setPresentation`; "Convert to Presentation" lands in the settings
with the SLIDES pane open, as in Svelte. The status bar shows the caret's slide and the speaking
time at 130 words a minute. Slide images come from `_attachments/`, like the preview's.
**Present** (F5, toolbar) is full screen; **Present with Speaker View** (Alt+F5, PowerPoint's
Presenter View key; toolbar and palette) adds the speaker window (see
`crates/noteliner-slides/README.md`).

### Math

`$…$`, `$$…$$` and ```` ```math ```` blocks are typeset in the preview, slides and speaker notes
(`noteliner_core::export::preview_html` turns on comrak's math; `marina_richtext::from_note_html`
keeps it) and in PDF export. `src/math.rs` installs Typst (through the warm PDF exporter) as
`marina-richtext`'s math renderer when the backend attaches. LaTeX it can't convert shows as its
source. The webview builds showed math as text; the HTML export still does.

### PDF export

`Backend::export(ExportKind::Pdf, …)` renders the note in-process with `noteliner-export`
(Typst): a `LazyExporter` is built and warmed up on a background thread when the app starts,
and each export runs on a blocking thread. The options match today's WebKit print
(`export::pdf_document`): Letter, 0.5 in margins, the note's name as the title heading, images
from `_attachments/`, wikilinks as styled text. The PDF goes to the Downloads folder, which then
opens, like the HTML and Markdown exports.

## Parity with the Svelte build

### IPC channels

| Channel                                                   | Status  | Notes                                                                   |
|-----------------------------------------------------------|---------|-------------------------------------------------------------------------|
| `dialog:openFolder`                                       | Done    | `rfd` folder picker                                                     |
| `project:open`, `project:init`, `project:close`           | Done    | Close flushes the pending edit and push behind a "Syncing" dialog       |
| `project:getIndex`, `project:saveIndex`                   | Done    | The index JSON round-trips untouched apart from the edited fields       |
| `file:read`, `file:getFrontmatter`, `file:write`          | Done    | Saves also refresh the entry's `modifiedAt` / `deck` in the UI's index  |
| `file:create`, `file:duplicate`                           | Done    | Templates, parent, tags; Presentation seeds the deck body and block      |
| `file:delete`, `file:rename`                              | Done    |                                                                         |
| `file:setPresentation`                                    | Done    | Convert to / from presentation, and the Presentation Settings dialog    |
| `templates:list`, `templates:save`                        | Done    | Save as Template logs to the Log panel                                  |
| `dialog:openImportFile`, `file:import`                    | Done    | DOCX / PPTX, with the "Importing" dialog and the Log summary            |
| `file:convertToHtml`, `file:convertToMarkdown`            | Done    | To Downloads, then the folder opens                                     |
| `file:convertToPdf`                                       | Done    | `noteliner-export` (Typst) instead of WebKit print; see PDF export      |
| `links:getBacklinks`, `links:getAllNames`, `links:rebuild` | Done    | BACKLINKS pane; `[[` completion uses the index names, as the Svelte editor does; links rebuild on every change |
| `search:query`                                            | Done    | SEARCH pane, 300 ms debounce, case-insensitive                          |
| `file:getHistory`, `file:getHistoryContent`               | Done    | History panel (read-only, as in Svelte)                                 |
| `file:addAttachment`, `file:removeAttachment`, `dialog:openFiles` | Done | Attachments panel (add, open, insert, remove); images and files pasted or dropped on the editor |
| `file:getAttachmentPath`, `attachment://`                 | Done    | The preview reads `_attachments/` directly                              |
| `git:push`, `git:pull`, `git:pullRebase`                  | Done    | Remote Sync; failures are logged, as in the Tauri build                 |
| `git:getRemoteUrl`, `git:setRemoteUrl`, `git:removeRemote`, `git:pushUpstream` | Done | Connect / Disconnect                          |
| `git:getSyncStatus`, `git:getBranch`, `git:resetToRemote` | Done    | Status bar chip and Remote Sync                                         |
| `git:getConfig`, `git:setConfig`                          | Done    | Project Settings; opened as required when a commit needs an identity     |
| `projects:getRecent`, `addRecent`, `removeRecent`         | Done    | Same `recent-projects.json` shape                                       |
| `system:getInfo`, `fs:ensureDir`                          | Done    | New Project defaults                                                    |
| `window-state:getLayout`, `saveLayout`, `restoreBounds`   | Done    | Same `window-state.json`; panel layout and window bounds per project, plus the last-opened note's caret and scroll (`lastFilePosition`, native only) |
| `mcp:getStatus`, `mcp:confirm-response`, `mcp:confirm-request` | Done | Settings → MCP; the full confirmation dialog with raw arguments         |
| `ui:getPrefs`, `ui:setPrefs` and their side effects       | Done    | `writeFrontmatter` and `mcpEnabled` apply live                          |
| `ui:spellcheck-changed`                                   | Done    | F7 and Settings; Hunspell dictionaries instead of the webview's checker (see Spell checking) |
| `update:*`                                                | Done    | About → Check for Updates (`marina_core::updates`)                      |
| `help:open`                                               | Done    | In-app Help dialog instead of a second window                           |
| `app:relaunch`                                            | Done    | After the titlebar setting                                              |
| `--mcp-bridge`                                            | Done    | Unix only for now (see above)                                           |
| Single instance                                           | Not yet |                                                                         |

### Components

| Svelte                                         | Status  | Notes                                                                 |
|------------------------------------------------|---------|-----------------------------------------------------------------------|
| `App.svelte` layout, commands, shortcuts, Ctrl+wheel zoom | Done | Every command, including the Presentation section; plus Present (F5) |
| `OpenScreen`, `RemoveRecentModal`              | Done    |                                                                       |
| `NewProjectModal`, `SetupModal`                | Done    |                                                                       |
| `Toolbar`, `TitleBar` (custom titlebar)        | Done    |                                                                       |
| `Sidebar` (PaneHost order, heights, close)     | Done    |                                                                       |
| `FileTree`                                     | Done    | Select, arrows, F2 / double-click rename, drag-and-drop, context menu; collapsible branches (native addition); no drops into a note's own subtree |
| `NewFileModal` (New File, New Presentation, Duplicate), `MoveFileModal` | Done |                                                    |
| `DeleteFileModal`, `ClearTagsModal`            | Done    |                                                                       |
| `OutlinePane`                                  | Done    |                                                                       |
| `Editor`                                       | Done    | `marina-editor` on its core engine: highlighting (code fences by language), list editing with ordered-list renumbering, shortcuts, find/replace, Go to line (Ctrl+Alt+G), bracket and selection matches, `[[` completion, paste/drop attachments, spell check, several cursors (Ctrl+click, Ctrl+D, find "all", Alt+drag), folding (gutter, Ctrl+Shift+[ / ]), notes of 50,000+ lines. Its own parity table lists what's left |
| `Preview`                                      | Done    | comrak + marina-richtext; wikilinks open or create notes; attachments |
| `StatusBar`, `LogPanel`                        | Done    | Including a deck's slide and speaking time                            |
| `SettingsModal` (UI, MCP, Keyboard Shortcuts)  | Done    | Turning MCP on opens the walkthrough                                  |
| `ProjectSettingsModal`, `SyncModal`, `SyncingModal`, `ImportingModal` | Done |                                                              |
| `AboutModal`, Help (`HelpApp`)                 | Done    |                                                                       |
| `McpConfirmModal`                              | Done    | Tool, action, raw arguments (payloads cut), Allow once / for session / Deny |
| `McpWalkthroughModal`                          | Done    | The client snippet uses `noteliner --mcp-bridge`                |
| `TagsPane`, `TagFilterPopover`, `TagEditorModal` | Done  | Chips toggle, Ctrl+T adds, notes dropped from FILES get the tag; filter with No Tags / Show all / Hide all; colours from the SVG palette |
| `SearchPane`, `BacklinksPane`                  | Done    | Results jump to the line                                              |
| `HistoryPanel`, `AttachmentPanel`              | Done    | Attachments add "Insert into Note" / "Copy Markdown" (native additions) |
| `SlidesPane`, `DeckPreview`, `Slide`, `PresentationSettingsModal` | Done | `crates/noteliner-slides`; its parity table lists the gaps (`class=`, table alignment, local video, `<mark>`) |
| Presenter                                      | Native addition | Full screen, keys, speaker notes; the Svelte build has none   |
| `ContextMenu`                                  | Done    | egui context menus                                                    |
