# BlockLiner Native

BlockLiner's app: an [egui](https://github.com/emilk/egui) UI over `blockliner-core`, with no webview or JavaScript. It runs on Linux, Windows, and macOS. The file format is described in the [BlockLiner README](../README.md#file-format).

## Running

```bash
cargo run -p blockliner-native --release
cargo run -p blockliner-native --release -- ~/diagrams/Checkout.mmd   # or a folder
cargo test -p blockliner-core -p blockliner-egui -p blockliner-native
```

The package is `blockliner-native`; its binary is `blockliner`, the one the release packages install as `/usr/bin/blockliner`.

- Opening. Ctrl+O (or the toolbar's folder button) opens a folder as the project; its `.mmd` files are the Project pane's diagrams. Opening another folder closes the last one's files first. Open File (Ctrl+Shift+O, or the Files pane's header button) opens `.mmd` files from anywhere. Each opened file joins the Files pane and gets a tab. The next start reopens the folder, the Files pane, the tabs, and the active tab. A path on the command line opens a folder as the project, or a `.mmd` file in a tab; the Linux desktop entry passes the file a file manager opens with BlockLiner.
- Tabs. Clicking a diagram in the Project pane or a file in the Files pane opens its tab, or switches to it. Switching writes the active tab's edits and keeps its undo history, pan, and zoom while the tab stays open. Closing a tab writes its edits (if that fails, the tab stays open), keeps the file in the Files pane, and switches to the tab to its right, else its left. A file deleted outside BlockLiner closes its tab and shows struck through in the Files pane until you close it.
- Closing. Close Folder (the toolbar button after Open Folder, or the palette) saves the open diagram, closes the folder's own files (files from elsewhere stay open), and goes back to "No folder open"; the next start then opens no folder. If the save fails, the folder stays open and the error is shown.
- Window chrome. Settings → UI → Custom Window Titlebar switches between the system frame and LaanKit's title bar (takes a restart).
- Command palette. Ctrl+K: every command.

## Keys

| Keys | Does |
|---|---|
| V, R, O, L, A, J, E, T, N, or 1–9 and 0 | Select, Rectangle, Rounded Rectangle, Line, Arrow, Right-Angle Line, Right-Angle Arrow, Text, Note tool (Shift: keep the tool after each shape). The numbers follow the toolbar and show in each tool button's corner; they're physical keys, so the number row works on any layout |
| D, H, C, S, or 4 | Diamond, Hexagon, Database (cylinder), Subroutine. They share the More Shapes button with Stadium, Circle, Double Circle, Parallelogram, Reverse Parallelogram, Trapezoid, Inverted Trapezoid, and Asymmetric (no letters; the list or the palette): 4 (or a click) picks the one it shows, the last used; its corner triangle or a right-click lists all twelve |
| Drag a right-angle line's middle handle | Moves its middle segment across (double-click the handle: automatic again) |
| Right-click | Selects what's under it, then a menu: Edit Label, Cut, Copy, Paste, Change Shape ▸ (shapes), Fill ▸ (shapes) and Stroke ▸ (shapes and lines; eight presets, Default, Custom…), Stroke Style ▸ (Solid, Dashed, Dotted), Stroke Weight ▸ (1px … 4px), Text Weight ▸ (Normal, Bold), Text Style ▸ (Normal, Italic), Reset Style, Right-Angle / Straight and Add / Remove Arrowhead for lines, Delete, Select All, Select All Shapes, Select All Lines, and Select All Text. On a diagram in the Project pane: Rename…, Delete…, and Open Containing Folder… (the system file manager). On a file in the Files pane: Close (its row and its tab) and Open Containing Folder… |
| Drag / click (shape tool) | Draws a shape (click: the default size, 8 × 3 cells; 8 × 4 for a diamond, 6 × 4 for a database, 4 × 4 for a circle, 5 × 5 for a double circle) |
| Double-click a shape | Edits its label (Enter commits, Shift+Enter adds a line, Esc cancels) |
| Double-click a connector or its label, or Enter | Edits the edge label (`a -->\|"HTTPS"\| b`); empty removes it. Enter edits the single selected shape's or connector's label. A free line has no Mermaid edge to keep a label on |
| Arrow keys | Nudge one grid step (Shift: 5 steps, Alt: 1 px) |
| Alt while dragging | No snapping |
| Delete / Backspace, Esc | Delete the selection; cancel the gesture or clear the selection |
| Ctrl+A, Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y | Select all, undo, redo |
| Ctrl+Shift+A, Ctrl+Alt+A, Ctrl+Shift+T | Select all shapes (not text boxes), all lines, all text boxes |
| Ctrl+C, Ctrl+X, Ctrl+V | Copy, cut, paste. The clipboard holds the selection as Mermaid; a paste lands two cells down and right (further each time), or in the middle of the view if that's off screen. Plain text pastes as a text box |
| Ctrl+N, F2, Ctrl+O | New diagram, rename it, open a folder |
| Ctrl+Shift+O, Ctrl+W | Open files, close the active tab |
| Ctrl+Tab, Ctrl+Shift+Tab (or Ctrl+PageDown, Ctrl+PageUp) | Next tab, previous tab |
| Ctrl+Shift+E | Export (PNG or SVG) |
| Wheel, middle drag, Space + drag | Pan |
| Ctrl + wheel, pinch, Ctrl+= / Ctrl+- / Ctrl+0 | Zoom (Ctrl+0, or a click on the status bar's "Zoom" readout: back to 100 %) |
| Ctrl+', Ctrl+E, Ctrl+I, Ctrl+J | Toggle the grid, the sidebar (the Project, Files, and Properties panes, resized with the dividers between them and reordered by dragging their headers; Toggle Files Pane is in the palette), the Properties pane (Change Shape and Change Line, then fill, stroke, stroke style, stroke weight, and text weight and style of the selection; a line's stroke is its colour), the status bar |
| Ctrl+Shift+D | Toggle dimension lines (each shape's width and height, in the unit Settings picks for one grid mark) |
| Ctrl+B | Toggle Bold for the selection's labels (all Bold, or all Normal when they already are) |
| Ctrl+K, Ctrl+,, Ctrl+Shift+I | Command palette, Settings, About |

## Architecture

`blockliner-core` (`crates/blockliner-core`) knows nothing about egui:

| Module     | What it does                                                                                |
|------------|---------------------------------------------------------------------------------------------|
| `model`    | `Diagram`, `Shape`, `Connector`, `End` (glued to a shape, or free), `Rect`, `Point`          |
| `mermaid`  | `read` and `write`: the flowchart, the `%%` layout block, statements kept verbatim, label escapes |
| `layout`   | `layered`: positions for nodes a file gives no layout for (longest-path ranks, barycenter order) |
| `geometry`, `snap`, `connect` | Handles and resizing, the outlines, label areas, and anchors of the shapes that aren't boxes, grid snapping, where connector ends meet shapes, connector paths, hit-testing |
| `route`    | Right-angle routes: candidate paths scored by shapes crossed, bends, and length (cached per geometry in `connect`) |
| `ids`      | Mermaid ids from labels (`"Orders DB"` → `orders_db`)                                       |
| `history`  | Undo and redo as whole-diagram snapshots                                                    |
| `export`   | Which objects an export contains, and the page around them                                  |
| `style`    | Colours and dashes: `Colour`, `Style` (a shape's `style` line or an edge's `linkStyle`), `Dash` (solid, dashed, dotted, custom), the presets, label contrast |
| `clipboard`| Copy and paste: the selection as a diagram (ends on shapes left behind freed), new ids and placement on paste, and plain text as a text box |

`blockliner-egui` (`crates/blockliner-egui`) is the editor as egui widgets, shared with NoteLiner:

| Module     | What it does                                                                                |
|------------|---------------------------------------------------------------------------------------------|
| `editor`   | `DiagramEditor`: one diagram with its undo history, canvas, and active tool; runs an edit command, copy and paste, and the keys that aren't shortcuts |
| `commands` | `EditAct` (the edit commands) and their registrations, which each app adds to its own registry |
| `canvas`   | The grid, painting, dimension lines, pan and zoom, the tools and their gestures, and the label editor |
| `tools`    | The tool buttons, the More Shapes list, and the tools' keys                                  |
| `properties` | The Properties pane: Change Shape and Change Line (Line, Arrow, Right-Angle Line, Right-Angle Arrow) lists, the selection's Fill and Stroke (Mixed when they differ), hex fields, the Stroke Style, Stroke Weight, Text Weight, and Text Style lists, Reset Style |
| `glyphs`   | The icons in the lists: tools, stroke styles, weights, and text styles                       |
| `export`   | The SVG writer (labels wrapped by egui, as on the canvas) and PNG through `resvg`            |
| `dialogs`  | The Export and Custom colour dialogs                                                         |
| `units`    | What one grid mark measures (mm, cm, m, in, ft) and how a length reads in it                 |

The app (`apps/blockliner/native`):

| Module     | What it does                                                                                |
|------------|---------------------------------------------------------------------------------------------|
| `main`     | The eframe window (1280 × 800, at least 800 × 640), icon, app id, the command-line path        |
| `app`      | Owns everything but the diagram being edited, which is a `DiagramEditor`; routes commands, tab and Files actions, overlay actions, and autosave events; puts background tabs' state aside |
| `backend`  | The folder on disk: list, read, create, rename, delete; the debounced atomic autosave; outside changes to the open tabs' files |
| `config`   | `ui-preferences.json` in `~/.config/BlockLiner` (the platform equivalent elsewhere)           |
| `commands` | The laankit-egui command registry: the file, tab, and window commands, plus the edit commands from `blockliner-egui` |
| `views`    | The toolbar, the Project and Files lists, empty states, and the status bar. The tab strip is `laankit_egui::tabs` |
| `files`    | The open files: the Files pane's list, the tabs, and the active tab, as paths                 |
| `overlays` | Settings, About, New/Rename Diagram, and the delete confirmation                              |
| `ui_tests*`| egui_kittest flows, headless (below)                                                        |

### Files

- One diagram, one file: `<folder>/<name>.mmd`. The flowchart says what exists; the `%%` block at the end says where it is. Opening writes nothing; the first edit normalises a hand-written file, and from then on saves are byte-stable.
- Autosave writes 0.8 s after edits pause (temp file + rename), and at once before switching tabs, closing one, or quitting, so only the active tab ever has edits waiting.
- Outside changes to every open tab's file are noticed about once a second and reload as one undoable step in that tab's history, unless BlockLiner's own edits are waiting to be written. Files in the Files pane without a tab are only checked for being there; their tab reads them again when it opens.

### Export

The diagram is drawn once, as SVG: fixed light colours, the canvas's font, corner radius, arrowheads, and label wrapping. A PNG is that SVG rasterised by `resvg` with the bundled fonts, so it looks the same on every system. The page is the exported objects plus two grid cells, aligned to the grid.

## Tests

```bash
cargo test -p blockliner-core -p blockliner-egui -p blockliner-native
BLOCKLINER_BLESS=1 cargo test -p blockliner-native the_svg_matches   # rewrite the golden SVG
```

- Core: model, geometry, snapping, ids, history, layout (including 300 random graphs with no overlaps), export scope, and the Mermaid golden files in `crates/blockliner-core/tests/fixtures` with a round trip over 2,000 generated diagrams.
- App: egui_kittest, headless: drawing and editing shapes and lines, undo, the folder and autosave, tabs and the Files pane (each tab's own undo and view, closing tabs and files, files from outside the folder, and a restart), outside changes, importing plain Mermaid, the command line, and export (a golden SVG in `tests/fixtures/export.svg`, PNG pixels, and the dialog from the keyboard to the written file). They check widgets through AccessKit and files on disk, never pixels on screen.

## Diagnostics

| Variable                       | Effect                                                         |
|--------------------------------|----------------------------------------------------------------|
| `LAANKIT_SCREENSHOT=<png>`      | Saves a screenshot once the window has settled (`LAANKIT_SCREENSHOT_EXIT=1` closes afterwards) |
| `LAANKIT_THEME`, `LAANKIT_SCALE` | Override the theme and scale                                   |
| `LAANKIT_OPEN=<dialog>`         | `settings`, `about`, or `palette`                               |

## Still to verify by hand

- The folder picker, the Open File dialog, and the export save dialog on Windows, macOS, and KDE.
- The tab strip and the Files pane by eye: many tabs scrolling sideways, long names, and the light and dark themes at 100% and 200%.
- Ctrl+W and Ctrl+Tab on desktops that use them for their own shortcuts.
- Opening a `.mmd` from the file manager (Linux packages register `text/vnd.mermaid` for `*.mmd` and `*.mermaid`). Windows and macOS have no file association yet.
- The undecorated window on Windows and macOS.
- Trackpad pan and pinch zoom on macOS.
- Copy and paste through the system clipboard (Ctrl+C/X/V and the menu's Paste) on each platform, between two BlockLiner windows, and into and out of a text editor.
- Exported SVGs in browsers and in Inkscape (text uses the Inter font when installed, else the system sans-serif).
