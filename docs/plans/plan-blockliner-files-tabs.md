# BlockLiner: Files Pane and Tabs Plan

Status: implemented (2026-11-09), BlockLiner 2.8.0, not yet checked by eye. Adds a resizable Files pane to BlockLiner's left sidebar and tabs above the canvas, so you can open a project folder and individual diagram files, keep several open at once, and switch between them.

## Summary

- A new Files pane in the left sidebar lists the files you have open: diagrams from the project folder and individual `.mmd` files from anywhere.
- Each open file can also be open in a tab, along the top of the canvas. Closing a tab keeps the file in the Files list; choosing it there opens the tab again.
- Right-clicking a file in the Files list offers Close, which removes it from the list and closes its tab if it has one.
- The sidebar becomes a stack of resizable panes: Project, Files, and Properties, using LaanKit's shared pane stack.

## Decisions and changes during implementation

- Open questions: answered with the plan's assumptions. Files and Project are separate panes, Close Folder closes the project's own files, a `.mmd` on the command line opens as a file, closing a tab drops its undo history, and Close Tab is Ctrl+W.
- Opening a different folder closes the last project's files first, as Close Folder does. Opening the same folder again changes nothing. Without a diagram named, the folder's first diagram opens only when no tab is open.
- Deleting the active diagram when no other tab is open opens its neighbour in the Project pane, as before tabs.
- A tab restored at start that was never shown has no stored state. It is read from its file when shown, and closes (its row showing as missing) if the file is gone.
- The Files and Properties panes have close buttons, the same as Toggle Files Pane and Ctrl+I. The Project pane has none. The first layout gives Project and Files 120 points each and Properties the rest.
- A file with a tab that isn't active shows a dot at the end of its Files row.
- `lastDiagram` is read once, when `openFiles` has never been written, and cleared at the next save.
- The UI test harness turns the Files pane off, so the Properties tests have the room they had. `ui_tests_tabs` turns it on.
- Not checked by eye: screenshots need a display, which the sandbox used for this work doesn't allow. It's listed under "Still to verify by hand" in the BlockLiner app README.

## Before this change

- One folder (the project) is open at a time, and one diagram in it. `App` holds an `Option<Project>` (the folder and its diagram names) and an `Option<Doc>` (the open diagram's name and its last saved state). `Diagram`, `History`, and `Canvas` hold the one diagram being edited (`apps/blockliner/native/src/app.rs`).
- A diagram is named by its file stem within the project folder. `App::path(name)` joins it to the folder, so every file operation needs a project.
- The sidebar shows only while a folder is open. It is the folder's diagram list (`views::sidebar`), with the Properties pane below it and a hand-made divider between them. Right-clicking a diagram offers Rename, Delete, and Open Containing Folder.
- Choosing a diagram flushes the autosave, closes the open one, and loads the new one with a fresh undo history and view.
- `Backend` (`backend.rs`) autosaves one pending file at a time. Saving a different path flushes the first one. `Backend::watch` and `Backend::check` watch one file for outside changes.
- The command line takes a folder, or one `.mmd` file, which opens its folder with that diagram selected.
- `ui-preferences.json` keeps `lastProject` and `lastDiagram`.
- LaanKit has a pane stack (`laankit_egui::panes`): headers, resizable dividers, reordering, and a serde `PaneLayout`, used by PageLiner. There is no shared tab bar.

## Proposal

### Layout

```
+-------------------+------------------------------------------------+
| PROJECT        +  | [ Overview x ] [ Payments ] [ notes/Auth x ]  |
|  Overview         |------------------------------------------------|
|  Payments         |                                                |
|  Storage          |                                                |
|-------------------|                   canvas                       |
| FILES      [^] +  |                                                |
|  Overview         |                                                |
|  Payments         |                                                |
|  Auth   ~/notes   |                                                |
|-------------------|                                                |
| PROPERTIES        |                                                |
|  ...              |                                                |
+-------------------+------------------------------------------------+
```

- The left sidebar is a `laankit_egui::panes` stack of three panes: Project, Files, and Properties. Each has a header and a draggable divider below it. The heights and order are saved in eframe's storage, as PageLiner does.
- Project is the folder's diagram list, as today, with its New button and right-click menu. It shows only while a folder is open.
- Files is new: see below. It shows while any file is open.
- Properties is the existing pane, toggled with Ctrl+I as today. Its hand-made divider is replaced by the pane stack's.
- The sidebar shows while a folder or any file is open, and Ctrl+E still hides and shows it.

### The Files pane

- It lists every open file, in the order you opened them.
- A row shows the diagram's name. A file outside the project folder also shows its folder, muted and shortened (`~/notes`), and so does any file whose name matches another row's.
- The row of the file in the active tab is highlighted. A file with a tab has a small marker; a file without one is shown in the normal text colour, without the marker.
- Clicking a row opens its tab (at the end of the tab strip) if it has none, and makes it the active tab.
- The header has an Open File button (Ctrl+Shift+O).
- Right-clicking a row offers:

| Item | Does |
|---|---|
| Close | Removes the file from the Files list and closes its tab, if it has one. |
| Open Containing Folder | Shows the file's folder in the system file manager, as the Project pane does. |

- A file that was deleted or moved outside BlockLiner stays in the list, struck through and muted, with a tooltip saying it is missing, until you close it. Clicking it says it can't be found.

### Tabs

- A strip of tabs sits along the top of the canvas, 32 points high, below the toolbar row and above the grid.
- Each tab shows the diagram's name and a close button (x). The close button shows on the active tab and on the tab under the pointer. Middle-clicking a tab closes it too.
- The active tab is drawn in the canvas's background colour, joined to the canvas; the others are in the surface colour.
- A file outside the project shows its folder in the tab's tooltip.
- When the tabs don't fit, the strip scrolls sideways with the mouse wheel, and the active tab is scrolled into view.
- Closing a tab writes any waiting edits first. If that write fails, the tab stays open and the error is shown, as Close Folder does today.
- Closing the active tab activates the tab to its right, else the one to its left. With no tabs left, the canvas shows an empty state: "Open a file from the Files or Project pane".
- Each tab keeps its own undo history and its own pan and zoom while it is open. The selection is cleared when you switch tabs. Closing a tab drops its undo history; opening it again starts fresh from the file.

### Opening files

| From | Does |
|---|---|
| Project pane, click a diagram | Adds it to Files if it isn't there, and opens or activates its tab. |
| Open File (Ctrl+Shift+O, the Files header, the palette) | A file dialog for one or more `.mmd` files; each is added to Files, and the last becomes the active tab. |
| Open Folder (Ctrl+O) | Opens the folder as the project. Open files and tabs stay open. |
| New Diagram (Ctrl+N) | Creates the diagram in the project folder, as today, and opens it in a new tab. Needs a project, as today. |
| Command line, a folder | Opens it as the project. |
| Command line, a `.mmd` file | Adds it to Files and opens its tab. The project is left as it was. |

- A file already in the Files list is never added twice; the paths are compared after `std::path::absolute`.
- A file that can't be read or parsed isn't added, and the error is shown, as today.

### Closing the project

- Close Folder closes the project's own files: their tabs and their Files rows. Files from outside the folder stay open.
- Renaming a diagram in the Project pane renames its Files row and tab. Deleting one closes its row and tab.

### Commands and shortcuts

| Command | Shortcut | When |
|---|---|---|
| Open File | Ctrl+Shift+O | Always |
| Close Tab | Ctrl+W | A tab is open |
| Close File | (palette only) | A tab is open: closes the active file's tab and Files row |
| Next Tab | Ctrl+Tab, Ctrl+PageDown | Two or more tabs |
| Previous Tab | Ctrl+Shift+Tab, Ctrl+PageUp | Two or more tabs |
| Toggle Files Pane | (palette only) | A file is open |

- Ctrl+W, Ctrl+Tab, Ctrl+PageDown, Ctrl+PageUp, and Ctrl+Shift+O are not used by BlockLiner today. Ctrl+Tab may need to be taken from egui's focus handling while the canvas has focus; P2 checks this.
- Rename Diagram and Delete Diagram act on the active tab, and stay off for a file outside the project, which the Project pane doesn't list.

### Saved state

`ui-preferences.json` gains:

| Key | Holds |
|---|---|
| `openFiles` | The Files list: absolute paths, in order |
| `openTabs` | The files with tabs, in tab order |
| `activeFile` | The active tab's path, or empty |

- `lastDiagram` is replaced by `activeFile`. On the first start after the change, a `lastDiagram` in the last project becomes the one open file and tab.
- On start, files that no longer exist are left out, and `lastProject` opens as today.
- The pane heights and order are kept with eframe's other layout state, as the Properties height is now (the `blockliner-properties-height` key is replaced by the `PaneLayout`).

### Saving and outside changes

- The autosave stays one pending file at a time. Switching tabs, closing a tab, and quitting flush the active file first, so only the active file can have waiting edits.
- `Backend` watches every open tab's file, not one. `Backend::watch(path, text)` and `Backend::unwatch(path)` keep a map of watched files, and `Backend::check` returns each file that changed, with its change.
- An outside edit to the active file is reloaded as an undoable step, as today. An outside edit to a background tab's file replaces that tab's stored diagram, as an undoable step in its own history, with the same "Reloaded" toast.
- A deleted or moved file closes its tab with the existing toast, and its Files row shows as missing.
- Files without a tab aren't watched or kept in memory. Opening the tab reads the file again.

## Changes

### blockliner-native

| File | Change |
|---|---|
| `app.rs` | Replace `Doc` with an open-file model: `files: Vec<OpenFile>` (path, display name, missing flag), `tabs: Vec<Tab>` (path, plus the stored `Diagram`, saved state, `History`, and `View` of a background tab), and `active: Option<usize>`. The active tab's diagram, history, and view stay in `self.diagram`, `self.history`, and `self.canvas`; switching tabs stores them in the tab and loads the next one's, so the canvas, commands, and properties code is unchanged. File operations take paths instead of names. `open_file`, `close_tab`, `close_file`, `activate`, and `open_files_dialog` are new; `open_path`, `close_folder`, `rename`, `delete`, `autosave`, `watch_folder`, the window title, and export follow the active tab's path. |
| `views.rs` | `files_pane` (the rows, the header button, the right-click menu) and `FilesAction`; `tab_strip` and `TabAction`; the empty state's new wording. The Project list keeps `sidebar`, renamed `project_pane`. |
| `app.rs` (`main_view`) | The sidebar becomes a `panes::show` stack; the tab strip is a top panel inside the central area, above the canvas. |
| `backend.rs` | Watching a map of files (`watch`, `unwatch`, `check` returning a list). The autosave is unchanged. |
| `commands.rs` | `Act::OpenFile`, `CloseTab`, `CloseFile`, `NextTab`, `PreviousTab`, `ToggleFiles`; `View` gains `tabs` and `files` counts for the `when` predicates. |
| `config.rs` | `openFiles`, `openTabs`, and `activeFile`, with the `lastDiagram` carry-over; the module table. |
| `main.rs` | The command-line file opens as a file rather than as its folder. |

No change to `blockliner-core`: the diagram format is per file and unchanged.

### Docs

- `apps/blockliner/README.md`: the Files pane, tabs, Open File, and the new shortcuts in the feature list; the new prefs in the settings line.
- `apps/blockliner/native/README.md`: the shortcuts table, the module table, and the files section (several files open, the watch).
- `ARCHITECTURE.md`, if it describes BlockLiner's single open diagram.

### Version

BlockLiner 2.8.0: `Cargo.toml`, `Cargo.lock`, and the metainfo release, as for 2.7.0.

## Phases

| Phase | Work | Done when |
|---|---|---|
| P1: Open-file model | `OpenFile` and `Tab` in `app.rs`; path-based open, close, rename, delete, autosave, and title; tab switching that stores and restores the diagram, history, and view; the multi-file watch in `backend.rs`; the prefs and the `lastDiagram` carry-over. No new UI yet: the Project pane drives it. | Every existing test passes; unit and UI tests open two diagrams, switch between them with undo kept, and close one. |
| P2: Tabs | `tab_strip`, close buttons, middle-click, sideways scrolling, the empty state, Close Tab, Next Tab, and Previous Tab, and the Ctrl+Tab check. | UI tests cover opening, switching, and closing tabs, and closing the active one. |
| P3: Files pane and pane stack | The `panes` stack for Project, Files, and Properties; `files_pane` with its header, missing rows, and right-click menu; Open File and its dialog; Close File; Toggle Files Pane; the command-line change. | UI tests cover Close from the right-click menu (row and tab gone), a closed tab staying in Files, reopening from Files, a file from outside the project, and Close Folder keeping outside files. Checked by eye in both themes at 100% and 200%. |
| P4: Docs and version | READMEs, `ARCHITECTURE.md`, version 2.8.0. | Docs match the app; clippy and every test pass. |

## Tests

- `config.rs`: the new prefs round-trip, unknown keys are kept, and `lastDiagram` carries over.
- `backend.rs`: watching two files reports each one's edit and removal; `unwatch` stops reports.
- A new `ui_tests_tabs.rs`:
  - Open two diagrams from the Project pane: both in Files, two tabs, the second active.
  - Undo in one tab, switch, switch back: the undo history and the view are kept.
  - Close a tab with its close button and with Ctrl+W: the file stays in Files; clicking it reopens the tab.
  - Right-click a Files row and choose Close: the row and its tab are gone, and the neighbour tab is active.
  - Open a file from outside the project (a test hook in place of the dialog, as `export_to` is for Export): its row shows its folder.
  - An outside edit to a background tab reloads it; deleting a tab's file closes the tab and marks the row missing.
  - Close Folder closes the project's files and keeps the outside one.
  - Restart (a new app on the same prefs): the Files list, tabs, and active tab come back.
- The existing UI tests keep passing: a single diagram opened from the Project pane behaves as today.

## Out of scope

- Dragging tabs to reorder them, and dragging a tab out to a new window.
- Split views (two diagrams side by side).
- Close Others and Close All on the tab and Files menus.
- Unsaved, untitled diagrams: every tab is a file on disk, as every diagram is today.
- A folder tree in the Project pane; it stays the folder's flat list of diagrams.
- Moving the tab strip into `laankit-egui` for the other apps. It is written so it can move later.

## Open questions

All answered with the plan's assumptions (see Decisions above).

| # | Question | Plan assumes |
|---|---|---|
| Q1 | Should the Files pane sit beside the Project list, or replace it, with the project's diagrams shown as a group inside Files? | Separate Project and Files panes. |
| Q2 | Should Close Folder also close the project's files, or leave everything open? | It closes the project's files and keeps outside ones. |
| Q3 | Should a `.mmd` file on the command line still open its folder as the project? | No: it opens as a file and the project is left as it was. |
| Q4 | Should closing a tab keep its undo history, so reopening it from Files can still undo? | No: closing a tab drops its history; only open tabs keep theirs. |
| Q5 | Is Ctrl+W right for Close Tab, given it closes the window in some desktop environments? | Ctrl+W, as in browsers and editors. |
