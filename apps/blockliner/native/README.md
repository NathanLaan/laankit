# BlockLiner Native

BlockLiner's app: an [egui](https://github.com/emilk/egui) UI over `blockliner-core`, with no
webview or JavaScript. It runs on Linux, Windows and macOS. The design, file format and the
decisions behind them are in [`docs/plans/plan-blockliner.md`](../../../docs/plans/plan-blockliner.md).

## Running

```bash
cargo run -p blockliner-native --release
cargo run -p blockliner-native --release -- ~/diagrams/Checkout.mmd   # or a folder
cargo test -p blockliner-core -p blockliner-native
```

The package is `blockliner-native`; its binary is `blockliner`, the one the release packages
install as `/usr/bin/blockliner`.

- **Opening.** Ctrl+O (or the toolbar's folder button) opens a folder; its `.mmd` files are the
  sidebar's diagrams. The next start reopens the folder and diagram. A path on the command line
  (a `.mmd` file or a folder) opens that instead; the Linux desktop entry passes the file a file
  manager opens with BlockLiner.
- **Closing.** Close Folder (the toolbar button after Open Folder, or the palette) saves the open
  diagram and goes back to "No folder open"; the next start then opens no folder. If the save
  fails, the folder stays open and the error is shown.
- **Window chrome.** Settings → UI → Custom Window Titlebar switches between the system frame and
  Marina's title bar (takes a restart).
- **Command palette.** Ctrl+K: every command.

## Keys

| Keys | Does |
|---|---|
| V, R, O, L, A, J, E, T, or 1–8 | Select, Rectangle, Rounded Rectangle, Line, Arrow, Right-Angle Line, Right-Angle Arrow, Text tool (Shift: keep the tool after each shape). The numbers follow the toolbar and show in each tool button's corner; they're physical keys, so the number row works on any layout |
| Drag a right-angle line's middle handle | Moves its middle segment across (double-click the handle: automatic again) |
| Right-click | Selects what's under it, then a menu: Right-Angle / Straight (for lines), Delete, Select All |
| Drag / click (shape tool) | Draws a shape (click: the default 8 × 3 cells) |
| Double-click a shape | Edits its label (Enter commits, Shift+Enter adds a line, Esc cancels) |
| Arrow keys | Nudge one grid step (Shift: 5 steps, Alt: 1 px) |
| Alt while dragging | No snapping |
| Delete / Backspace, Esc | Delete the selection; cancel the gesture or clear the selection |
| Ctrl+A, Ctrl+Z, Ctrl+Shift+Z / Ctrl+Y | Select all, undo, redo |
| Ctrl+N, F2, Ctrl+O | New diagram, rename it, open a folder |
| Ctrl+Shift+E | Export (PNG or SVG) |
| Wheel, middle drag, Space + drag | Pan |
| Ctrl + wheel, pinch, Ctrl+= / Ctrl+- / Ctrl+0 | Zoom |
| Ctrl+', Ctrl+E, Ctrl+J | Toggle the grid, the sidebar, the status bar |
| Ctrl+K, Ctrl+,, Ctrl+Shift+I | Command palette, Settings, About |

## Architecture

`blockliner-core` (`crates/blockliner-core`) knows nothing about egui:

| Module     | What it does                                                                                |
|------------|---------------------------------------------------------------------------------------------|
| `model`    | `Diagram`, `Shape`, `Connector`, `End` (glued to a shape, or free), `Rect`, `Point`          |
| `mermaid`  | `read` and `write`: the flowchart, the `%%` layout block, statements kept verbatim, label escapes |
| `layout`   | `layered`: positions for nodes a file gives no layout for (longest-path ranks, barycenter order) |
| `geometry`, `snap`, `connect` | Handles and resizing, grid snapping, where connector ends meet shapes, connector paths, hit-testing |
| `route`    | Right-angle routes: candidate paths scored by shapes crossed, bends and length (cached per geometry in `connect`) |
| `ids`      | Mermaid ids from labels (`"Orders DB"` → `orders_db`)                                       |
| `history`  | Undo and redo as whole-diagram snapshots                                                    |
| `export`   | Which objects an export contains, and the page around them                                  |

The app (`apps/blockliner/native`):

| Module     | What it does                                                                                |
|------------|---------------------------------------------------------------------------------------------|
| `main`     | The eframe window (1280 × 800, at least 800 × 640), icon, app id, the command-line path        |
| `app`      | Owns everything; routes commands, canvas output, overlay actions and autosave events         |
| `backend`  | The folder on disk: list, read, create, rename, delete; the debounced atomic autosave; outside changes |
| `config`   | `ui-preferences.json` in `~/.config/BlockLiner` (the platform equivalent elsewhere)           |
| `commands` | The marina-egui command registry: shortcuts, palette and the Settings shortcut list         |
| `canvas`   | The grid, painting, pan and zoom, the tools and their gestures, the label editor            |
| `views`    | The toolbar, the diagram sidebar, empty states and the status bar                           |
| `overlays` | Settings, About, Export, New/Rename Diagram, delete confirmation                            |
| `export`   | The SVG writer (labels wrapped by egui, as on the canvas) and PNG through `resvg`            |
| `ui_tests*`| egui_kittest flows, headless (below)                                                        |

### Files

- **One diagram, one file:** `<folder>/<name>.mmd`. The flowchart says what exists; the `%%`
  block at the end says where it is. Opening writes nothing; the first edit normalises a
  hand-written file, and from then on saves are byte-stable.
- **Autosave** writes 0.8 s after edits pause (temp file + rename), and at once before switching
  diagrams or quitting.
- **Outside changes** are noticed about once a second and reload as one undoable step, unless
  BlockLiner's own edits are waiting to be written.

### Export

The diagram is drawn once, as SVG: fixed light colours, the canvas's font, corner radius,
arrowheads and label wrapping. A PNG is that SVG rasterised by `resvg` with the bundled fonts, so
it looks the same on every system. The page is the exported objects plus two grid cells, aligned
to the grid.

## Tests

```bash
cargo test -p blockliner-core -p blockliner-native
BLOCKLINER_BLESS=1 cargo test -p blockliner-native the_svg_matches   # rewrite the golden SVG
```

- **Core:** model, geometry, snapping, ids, history, layout (including 300 random graphs with no
  overlaps), export scope, and the Mermaid golden files in `crates/blockliner-core/tests/fixtures`
  with a round trip over 2,000 generated diagrams.
- **App:** egui_kittest, headless: drawing and editing shapes and lines, undo, the folder and
  autosave, outside changes, importing plain Mermaid, the command line, and export (a golden SVG
  in `tests/fixtures/export.svg`, PNG pixels, and the dialog from the keyboard to the written file).
  They check widgets through AccessKit and files on disk, never pixels on screen.

## Diagnostics

| Variable                       | Effect                                                         |
|--------------------------------|----------------------------------------------------------------|
| `MARINA_SCREENSHOT=<png>`      | Saves a screenshot once the window has settled (`MARINA_SCREENSHOT_EXIT=1` closes afterwards) |
| `MARINA_THEME`, `MARINA_SCALE` | Override the theme and scale                                   |
| `MARINA_OPEN=<dialog>`         | `settings`, `about` or `palette`                               |

## Still to verify by hand

- The folder picker and the export save dialog on Windows, macOS and KDE.
- Opening a `.mmd` from the file manager (Linux packages register `text/vnd.mermaid` for `*.mmd`
  and `*.mermaid`). Windows and macOS have no file association yet.
- The undecorated window on Windows and macOS.
- Trackpad pan and pinch zoom on macOS.
- Exported SVGs in browsers and in Inkscape (text uses the Inter font when installed, else the
  system sans-serif).
