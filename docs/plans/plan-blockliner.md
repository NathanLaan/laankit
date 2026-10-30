# BlockLiner — Architecture Diagramming App

Status: **MVP implemented (2026-10-30).** Design decisions settled (§9).
Phases 0–8 done: app scaffold, `crates/blockliner-core`, shapes, connectors,
undo and keyboard, folder and autosave, import layout, export, and packaging.
Hand checks on Windows and macOS remain
([`apps/blockliner/native/README.md`](../../apps/blockliner/native/README.md),
"Still to verify by hand"). **Phase 9 (right-angle connectors) implemented
(2026-10-30); phases 10–12 proposed** (§10): shape styles, Mermaid
database/subroutine/hexagon/diamond shapes, copy and paste.

A fourth marina app: a Visio-style canvas for software component and
architecture diagrams, where every diagram is saved as a **Mermaid** file.
You draw boxes and connectors by hand on a snapping grid. The file stays plain
Mermaid, so it renders on GitHub/GitLab, diffs in git, and can be pasted into
a NoteLiner note.

---

## 1. Name

**BlockLiner** (decided 2026-10-29). It has the `-Liner` suffix (NoteLiner,
ThreadLiner, PageLiner). Architecture diagrams are mostly component blocks
joined by lines, and Mermaid has a `block` diagram type the app could grow
into.

The app was first scaffolded as *ChartLiner* (a nautical chart as the map of
a system). It was renamed before any release, because "chart" suggests bar
and line charts. Nothing outside the repo used the old name.

| Name | Emphasis | Note |
|------|----------|------|
| **BlockLiner** | Component blocks | Chosen. |
| **BoxLiner** | "Boxes and lines" sketches | Friendly, but informal and less clearly about software. |
| **DraftLiner** | Drafting (and a ship's *draft*) | "Draft" also reads as "unfinished document". |
| **ChartLiner** | The map of a system | The first name; "chart" suggests data charts. |

The app is `blockliner` (binary and crates `blockliner-core` /
`blockliner-native`), app id `com.blockliner.app` (the same pattern as
`com.pageliner.app`), and product folder `BlockLiner`. The Mermaid class for
text boxes is `bl_text`.

---

## 2. Scope

### MVP (this plan)

- **Shapes:** rectangle and round rectangle, each with a centred label.
- **Lines:** plain line and line with an arrowhead. An endpoint can **glue**
  to a shape's anchor (side midpoints) and follows the shape when it moves.
  An endpoint can also be left free on the grid.
- **Text boxes:** free-standing labels with no border.
- **Grid snapping:** positions, sizes and free line endpoints snap to the grid
  (default 20 px; 10/20/40 in Settings). Hold `Alt` to place freely. The grid
  is drawn and can be hidden.
- **Editing:** select (click, `Shift`+click, marquee), move, resize handles,
  delete, double-click to edit a label inline, undo/redo, pan (`Space`+drag
  or middle mouse) and zoom (`Ctrl`+wheel).
- **Files:** a project is a plain folder of `.mmd` files. Edits autosave
  (debounced). BlockLiner does **no git syncing**. If the folder happens to be
  a git repo, the user commits it with their own tools.
- **Export:** an Export dialog writes the diagram as a **PNG** or **SVG**
  image, exactly as laid out on the canvas, with or without the grid, for
  all objects or only the selected ones (§5.1).
- **Mermaid round-trip:** BlockLiner reads back what it writes, exactly
  (§4). It also imports a hand-written flowchart that has no layout data and
  lays it out automatically (§4.4).

### Not in the MVP (later phases, §7)

Orthogonal/elbow routing, edge labels, colours/styles, `subgraph` containers
(boundaries), copy/paste and duplicate, alignment/distribute, a stencil library (database, queue, actor, cloud), multiple pages, and
Mermaid diagram types other than `flowchart`.

Phases 9–12 (§10) take on right-angle routing, shape colours, the database,
subroutine, hexagon and diamond shapes, and copy/paste.

---

## 3. Architecture

BlockLiner follows the same layering as the other apps (`ARCHITECTURE.md`):

```
apps/blockliner/native      eframe app: canvas widget, tools, sidebar, overlays,
                            tokio backend (file IO, autosave)
crates/blockliner-core      document model, Mermaid read/write, geometry,
                            snapping, hit-testing, undo history: no egui
crates/marina-core          paths, json, debounce (unchanged; git unused)
crates/marina-egui          theme, toolbar, palette, settings, panes (unchanged)
```

### 3.1 `crates/blockliner-core` (new, no egui dependency)

| Module | Responsibility |
|---|---|
| `model` ✅ | `Diagram { direction, grid, shapes: Vec<Shape>, connectors: Vec<Connector>, preamble, passthrough }`. `Shape { id, kind: Rect \| RoundRect \| Text \| Other{open, close}, label, rect, class, external }`. `Connector { kind: Line \| Arrow, from: End, to: End, label }`. `End` is `Glued { shape, anchor: Option<N\|E\|S\|W> }` (`None` = dynamic glue, the nearest side) or `Free(Point)`. Its own `Point`/`Rect` types (f32), so it needs no egui. |
| `mermaid::write` ✅ | `Diagram` → `.mmd` text. Deterministic order (stable diffs). |
| `mermaid::read` ✅ | `.mmd` text → `Diagram`. A small hand-written lexer and reader for the `flowchart` subset in §4. Unknown statements are kept verbatim and written back. |
| `layout` ✅ | `layered`: positions for nodes without `@shape`, a basic layered layout (§4.4), below anything already placed. |
| `geometry` ✅ | Rectangle helpers, resize handles and maths, shape hit-testing, marquee containment. |
| `connect` ✅ | Anchor positions, connector end resolution (fixed and dynamic glue), distance-to-segment, connector and anchor hit-testing, `glue_at`, removing connectors. |
| `snap` ✅ | `snap(v, grid)`, `snap_point`. |
| `history` ✅ | Undo/redo as whole `Diagram` snapshots (200 kept). The app calls `observe` whenever the canvas is idle, so each finished change is one step without per-edit bookkeeping. Diagrams are small, so snapshots are simpler than an operation log and cannot get out of sync. |
| `export` ✅ | What an export contains: `scope(d, selection, lines) -> Scope { shapes, connectors, bounds }` for all objects or only the selected ones, and the page bounds (the objects' extent plus a margin, on the grid). No drawing: that needs text layout, which lives in the app (§5.1). |
| `ids` ✅ | Readable Mermaid node ids from labels (`"Orders DB"` → `orders_db`, de-duplicated). |

Everything in the core is unit-tested without a window.

### 3.2 `apps/blockliner/native` (new)

Same file layout as `apps/pageliner/native`:

| File | Contents |
|---|---|
| `main.rs` ✅ | Viewport (title, `APP_ID`, icon, custom title bar), `persistence_path`. Copy of PageLiner's. A command-line path (a `.mmd` file or a folder) opens instead of the last diagram (phase 8, for the desktop entry's `%f`). |
| `config.rs` ✅ | `Paths` / `Prefs` in `~/.config/BlockLiner/ui-preferences.json`: `customTitlebar`, `sidebarVisible`, `statusBarVisible`, `gridSize`, `showGrid`, `lastProject`, `lastDiagram`. |
| `commands.rs` ✅ | `Act` enum + `Registry`: tool selection (`V` select, `R` rect, `O` round rect, `L` line, `A` arrow, `T` text), undo/redo, delete, nudge, zoom, toggle grid/sidebar/status bar, new/rename/delete diagram, open folder, export (phase 7, `Ctrl+Shift+E`), settings, palette. |
| `backend.rs` ✅ | The project folder. List, read, create, rename and delete are small local files, so they run synchronously and return their result. The autosave runs on a small tokio runtime: debounced (0.8 s) through `marina_core::debounce`, atomic (temp file + rename), with `flush` before switching diagrams, renaming and quitting. `check` reports outside changes to the open file (not our own writes). |
| `app.rs` ✅ | `BlockLinerApp`: owns the backend, the open folder and diagram, the canvas, overlays, palette, toasts. Routes commands, sidebar and dialog actions; autosaves when the canvas is idle; looks at the folder once a second. |
| `canvas/view.rs`, `canvas/grid.rs` ✅ | Pan/zoom transform (zoom steps 10–600 %, zoom about a point) and the grid (minor lines, a major line every 5 cells, only majors when cells get small). |
| `canvas/mod.rs` ✅ | The canvas widget: `ui.allocate_painter`, view transform (pan + zoom), grid drawing, item painting, input dispatch to the active tool. Returns `CanvasAction`s (`Changed`, `SelectionChanged`, `EditLabel(id)`). |
| `canvas/tools.rs` ✅ | A small state machine per tool: `Idle → Dragging{start} → commit`. Rect tools snap the start and end corners. Line tools show anchor hints on hover and glue when released over one. |
| `canvas/paint.rs` ✅ | Draws shapes, connectors, arrowheads, selection handles and anchor hints with theme colours from `marina_egui::theme`. |
| `canvas/label.rs` ✅ | Inline label editing: an `egui::TextEdit` placed over the shape (multi-line, `Enter` commits, `Shift+Enter` adds a line, `Esc` cancels). |
| `views.rs` ✅ | Sidebar (the folder's diagrams, New button, right-click Rename/Delete), toolbar (`marina_egui::toolbar` with tool buttons; Open Folder, Settings, About), the empty states (no folder, no diagram), status bar (tool, save state, selection, cursor position in grid units, grid size, zoom, UI scale). |
| `overlays/` ✅ | Settings (grid size, show grid, title bar), the New/Rename name dialog, the delete confirmation, About, and `overlays/export.rs`, the Export dialog (§5.1). |
| `export.rs` ✅ | Draws a `Scope` as SVG text (shapes, connectors, arrowheads, labels, optional grid) in the export colours; labels wrap exactly as on the canvas, using egui's text layout for the line breaks. PNG output rasterises that SVG with `resvg`, loading the bundled fonts (`marina_egui::fonts::bundled_faces`) so text matches. Writes are atomic (`marina_core::json::write_string` / temp file + rename). |
| `ui_tests*.rs` ✅ | `egui_kittest` headless flows (§6), against a temp config folder and project folder. |

**Zoom keys.** Unlike the other apps, Ctrl+= / Ctrl+- / Ctrl+0 and
Ctrl+wheel zoom the *canvas*, as in other drawing tools. The UI scale stays in
Settings and the custom title bar. Plain wheel/trackpad scrolling, a middle
drag or Space+drag pan. The grid toggles with Ctrl+'.

**Rendering coordinates.** The model is stored in *diagram pixels at 100 %
zoom*. The canvas maps model → screen with `screen = origin + (p - pan) *
zoom`. Snapping always happens in model space, so zoom never changes where
an item lands.

---

## 4. File format: Mermaid with layout comments

Mermaid flowcharts have no coordinates, and Mermaid lays them out itself. To
keep the file valid Mermaid while recording exact positions, BlockLiner writes
the layout in `%%` comment lines. Mermaid ignores these lines.

### 4.1 Example

```mermaid
flowchart LR
  api["API Gateway"]
  orders("Orders Service")
  orders_db[("Orders DB")]
  note_1["Payments are async"]:::bl_text
  api --> orders
  orders --- orders_db
  classDef bl_text fill:none,stroke:none

%% blockliner 1 grid=20
%% @shape api x=40 y=80 w=160 h=60
%% @shape orders x=280 y=80 w=160 h=60
%% @shape orders_db x=520 y=80 w=160 h=60
%% @shape note_1 x=280 y=180 w=200 h=40
%% @edge api->orders from=E to=W
%% @line arrow from=600,240 to=orders:S
```

The BlockLiner block goes **at the end** because Mermaid front matter (`---`)
must be the first line of the file. The Mermaid part says what each shape is,
so `@shape` lines carry only geometry. `orders_db` uses a cylinder, which the
MVP can't draw. It keeps its delimiters (`ShapeKind::Other`), is drawn as a
rectangle, and is written back unchanged.

| Comment | Meaning |
|---|---|
| `%% blockliner <version> grid=<px>` | Format version (1) and grid size. |
| `%% @shape <id> x= y= w= h=` | Every shape's rectangle, in model order. |
| `%% @edge <from>-><to>[#n] [from=<anchor>] [to=<anchor>] [route=elbow [mid=x:<n>\|y:<n>]]` | Fixed anchors and the route of the nth edge between two nodes. Written only when an anchor is fixed or the route is right-angle; otherwise the glue is dynamic and the edge straight. Keyed by endpoints, not position, so a hand-inserted edge doesn't shift the others. |
| `%% @line <line\|arrow> from=<end> to=<end> [route=elbow [mid=…]]` | A connector with at least one free end. An end is `x,y` or `id[:anchor]`. |

Any other `%% @…` line is reserved and dropped. Other `%%` comments are kept.

### 4.2 Mapping

| BlockLiner | Mermaid | Notes |
|---|---|---|
| Rectangle | `id["label"]` | |
| Round rectangle | `id("label")` | |
| Text box | `id["label"]:::bl_text` + `classDef bl_text fill:none,stroke:none` | Renders as bare text in Mermaid. |
| Glued line | `a --- b` | |
| Glued arrow | `a --> b` | |
| Free line/arrow (either end not glued) | `%% @line …` only | Mermaid cannot draw a line that is not attached to nodes. The line appears in BlockLiner but not in other Mermaid renderers. The status bar shows a hint when the diagram has such lines. |
| Multi-line label | `<br>` inside the quoted label | `"` is escaped as `#quot;`. `#` and `<br` are escaped (`#35;`, `#lt;`) only where they would otherwise be decoded, so `C#` stays `C#`. |
| Edge label (read and kept; editing comes after the MVP) | `a -->\|"label"\| b` | Also reads `a -- label --> b`. |

New diagrams default to `LR`. A file's own direction is kept. `graph` is
written back as `flowchart`.

### 4.3 Round-trip rules

- **The Mermaid part is the source of truth for what exists.** The comments
  only add geometry. If a comment names a node that no longer exists (for
  example after a hand edit), BlockLiner drops it. A node with no comment
  gets an automatic position.
- **Statements BlockLiner doesn't model are kept** verbatim and in order:
  `classDef`, `class`, `style`, `click`, `linkStyle`, other `%%` comments,
  whole `subgraph … end` blocks, and edges with other link styles (`-.->`,
  `==>`, `a & b --> c`, …). They are written after the nodes and edges. Nodes
  that are declared only inside them are marked `external`. Those nodes are
  positioned and drawn, but not editable, because their declaration stays in
  the kept statement.
- **Writing is deterministic:** nodes in model order, then edges, then kept
  statements, then the layout block. Shapes are read back in `@shape` order,
  so a hand-written file is normalised by its first save and is
  byte-identical from then on (`tests/fixtures/handwritten*.mmd`).
- **Known limitation:** the first save moves modelled edges ahead of kept
  ones. A hand-written `linkStyle <index>` that mixes the two may then point
  at a different edge.
- Tested by golden files for both directions and a round trip over 2,000
  generated diagrams (`read(write(d)) == d`, and `write` is idempotent).

### 4.4 Importing plain Mermaid

When a file has nodes without `@shape` lines (pasted from a README, or hand
edited), `layout::layered` places them:

1. Find the longest-path rank of each node from the edges, following the
   flowchart direction.
2. Within each rank, order the nodes by the average position of their
   parents. One pass is enough for MVP-sized diagrams.
3. Space ranks and slots by cell size, snapped to the grid.

Ranks come from the connectors and also from the edges in statements kept
verbatim (a subgraph's `pay --> psp`, `a1 & a2 --> api`), so their nodes are
ranked too. Cycles are broken at the edge that closes them (depth-first, in
file order). Each rank is centred across the widest one.

This is a basic Sugiyama-style layout without crossing minimisation or dummy
nodes, so a long edge (spanning ranks, or closing a cycle) is a straight
line that can cross a box in between, until elbow routing (§7, after the
MVP). That is good enough for a starting point the user then rearranges.
Opening writes nothing; the layout is saved with the first edit.

**Shapes BlockLiner can't fully edit** carry a badge on their top-right
corner and explain themselves in the status bar when hovered or selected:
a lock on a shape declared only in a kept statement (move and resize only),
and the Mermaid brackets, such as `[( )]`, on a shape drawn as a box.

---

## 5. Interaction details

- **Creating:** with a shape tool, drag from corner to corner. A plain click
  creates a default-size shape (8×3 grid cells), which is faster for
  architecture diagrams. The new shape starts in label-edit mode. After one
  shape the tool returns to Select (Visio's behaviour). Holding `Shift`
  while choosing a tool keeps it active.
- **Connecting:** with a line/arrow tool, hovering a shape shows its 4
  anchors. Pressing on an anchor or a shape body glues the start. Releasing
  over a shape glues to the nearest anchor, otherwise to the snapped grid
  point. A selected connector shows endpoint handles, which can be dragged
  to re-glue or un-glue.
- **Glue:** dropping a line end within 8 screen points of a side midpoint
  glues it there (a fixed anchor, written as `@edge … from=E`). Dropping it
  elsewhere on a shape glues it *dynamically*: it uses whichever side
  midpoint is nearest the other end, so it turns as shapes move, and needs
  no `@edge` line. A line can't join a shape to itself. The shape a line end
  would glue to shows its four anchors while drawing or dragging, with the
  chosen one filled, or the shape outlined for dynamic glue.
- **Lines** are selected by clicking within 5 screen points, box-selected
  when both ends are inside, and deleted with Delete. Dragging a line moves
  its free ends only; glued ends stay with their shapes. A single selected
  line shows its two ends as handles (filled when glued), and dragging one
  re-glues or frees it. Edge labels read from a file are drawn at the
  midpoint, read-only.
- **Moving:** a drag moves the selection by whole grid steps. The delta is
  snapped, not each item's position, so items that are not on the grid
  (after `Alt` placement) keep their offset. Glued connectors follow.
- **Resizing:** 8 handles. Edges snap to the grid. The minimum size is one
  grid cell.
- **Ids:** a new shape takes its Mermaid id from its first label
  (`"Orders DB"` → `orders_db`); after that the id stays put when the label
  changes. An empty first label leaves `node`, `node_2`, ….
- **Deleting** a shape also deletes the connectors glued to it, since a
  Mermaid edge can't have a missing end. Shapes declared only in kept
  statements (`external`) can be moved and resized but not relabelled or
  deleted.
- **Esc** cancels a label edit, else a drag (moved or resized shapes go
  back), else clears the selection.
- **Undo:** one history entry per finished change. The history is taken
  whenever the canvas is idle (no drag, no label being edited), so a whole
  drag is one entry, and so is creating a shape together with its first
  label. `Ctrl+Z` undoes, and `Ctrl+Shift+Z` / `Ctrl+Y` redo. A new change
  after an undo drops the redo trail. Undo during a drag cancels the drag
  first. While a label is being edited, `Ctrl+Z` belongs to the text field.
- **Files:** `Ctrl+O` (or the toolbar's folder button) opens a folder; its
  `.mmd` files are the sidebar's diagrams (not subfolders), and the next
  start reopens the folder and diagram (`lastProject`, `lastDiagram`).
  `Ctrl+N` makes a new diagram (the name dialog suggests "Untitled",
  selected), `F2` renames the open one, and a row's right-click menu renames
  or deletes any of them (Delete Diagram is also in the palette). Names
  clash case-insensitively, since the folder may be synced to a system
  where file names ignore case. Deleting asks first and opens the neighbour.
  `Ctrl+E` (or the title bar's panel button) hides the sidebar. Close
  Folder (toolbar, palette) saves, closes and clears `lastProject`
  ([`plan-blockliner-close-folder-tool-keys.md`](plan-blockliner-close-folder-tool-keys.md)).
- **Autosave:** once the canvas is idle (the same moment the undo history is
  taken), a changed diagram is handed to the autosave, which writes it 0.8 s
  after edits pause. Opening a file writes nothing, so a hand-written file is
  only rewritten when it's edited. The status bar says "Edited" or "Saved",
  and the window title names the diagram.
- **Outside changes:** about once a second the app relists the folder and
  checks the open file. A change made elsewhere (an editor, `git pull`) is
  reloaded as one undoable step, unless BlockLiner's own edits are waiting
  to be written, in which case they win. A deleted file closes the diagram.
- **Keyboard:** arrow keys nudge the selection by one grid step (`Shift`: 5
  steps, `Alt`: 1 px), moving the free ends of selected lines too.
  `Delete`/`Backspace` deletes. `Ctrl+A` selects all shapes and lines. `Esc`
  cancels a label edit or gesture, else clears the selection.


### 5.1 Export

**Opening it.** An **Export** button in the toolbar's bottom group (above
Settings; Font Awesome `file-export`, added to `marina_egui::icons`),
`Ctrl+Shift+E`, and "Export…" in the command palette. It's available while a
diagram is open and has something in it.

**The dialog** (a compact `marina_egui::dialog::Dialog`, titled "Export
Diagram"):

| Option | Choices | Default |
|---|---|---|
| Format | **PNG** · **SVG** (a two-way segmented control) | the last one used |
| Objects | **All Objects** · **Selected Objects Only** (radio) | All Objects; Selected Objects Only is disabled when nothing is selected |
| Grid | **Show grid** (toggle) | off |
| Scale | **1×** · **2×** · **3×** (PNG only; hidden for SVG) | 2×, so text stays sharp on high-DPI screens and in documents |

Below the options, a muted line gives the output size ("1240 × 680 px" for
PNG, "620 × 340" for SVG), updated as options change. **Export…** opens the
system save dialog (`rfd::FileDialog::save_file`) in the project folder,
with the name `<diagram>.png` / `.svg` (`<diagram> (selection)` for a
selection export) and the format's filter. Saving closes the dialog with a
success toast naming the file; a failed write shows an error toast and
leaves the dialog open. **Cancel**, `Esc` and the X close it. The format,
grid and scale choices are remembered (`exportFormat`, `exportGrid`,
`exportScale` in `ui-preferences.json`); the Objects choice is not, since it
depends on the selection at the time.

**What's included.**

- *All Objects:* every shape and connector.
- *Selected Objects Only:* the selected shapes and lines, plus connectors
  glued at both ends to selected shapes, so a selected group keeps its
  arrows. A connector glued to a shape that isn't exported is left out
  (it would point at nothing).
- Never: the selection outline, handles, anchor hints, hover highlight, the
  limitation badges, or the label being edited (an edit in progress is
  committed first).

**Page.** The bounds of the included objects plus a margin of two grid
cells, rounded outward to the grid, so with the grid shown the lines match
the canvas cells. Coordinates are model pixels (100 % zoom); a PNG multiplies
them by the scale. The page starts at the bounds' top-left corner, so an
export never has a large empty area just because the shapes are far from the
origin.

**Look.** Exports go into documents and wikis, so they always use the light
palette (white background, dark ink) whatever the app's theme. Shapes,
round-rect corners, stroke widths and arrowheads match the canvas at 100 %.
`Other` Mermaid shapes are drawn with a solid outline (the canvas's dashes
are an editing cue). Edge labels from the file are drawn as on the canvas.
The grid, when shown, uses the canvas's minor and major line pattern in
light grey.

**SVG details.** Plain SVG 1.1: `<rect>`, `<line>`, `<polygon>` (arrowheads),
`<text>` with one `<tspan>` per wrapped line, `font-family` set to the UI
font with `sans-serif` as a fallback. One `<g id="…">` per shape, named by its
Mermaid id, so the file is easy to restyle or script. No embedded fonts (the
file stays small; the text stays editable).

---

## 6. Testing

- **Core (`cargo test -p blockliner-core`):** parser and writer golden files
  under `crates/blockliner-core/tests/fixtures/` (written by BlockLiner,
  hand-written Mermaid, and files with passthrough lines); round-trip
  property tests; snap, anchor and hit-test unit tests; layout tests
  (no overlap, ranks increase in the flow direction); history coalescing.
- **UI (`egui_kittest`, headless, as in the other apps):**
  - pick Rect tool → drag → a shape exists at the snapped rect → the label
    typed in is in the saved `.mmd`;
  - draw an arrow between two shapes → `a --> b` written → moving a shape
    keeps the connector glued;
  - undo/redo across create, move and delete;
  - `Alt` placement off the grid; grid size change in Settings;
  - opening a hand-written fixture shows the auto-laid-out nodes, and saving
    keeps the passthrough lines.
- **Export (phase 7):** core tests for `scope` (all vs selected; connectors
  kept only with both ends exported; free lines; bounds on the grid with
  the margin). App tests: the SVG for a small diagram is a golden file; a
  PNG's pixel size is the page size × scale; the grid toggle adds or drops
  the grid group; a kittest flow opens the dialog, picks SVG and Selected
  Objects Only, and checks the written file (the save dialog is bypassed in
  tests with a test-only path). The golden SVG is
  `apps/blockliner/native/tests/fixtures/export.svg`
  (`BLOCKLINER_BLESS=1` rewrites it).
- **Screenshots** for visual checks are rendered offscreen with
  `egui_kittest` + wgpu (`MARINA_SCREENSHOT`-style), never in a real window.

---

## 7. Phases

| Phase | Deliverable | Effort |
|---|---|---|
| **0. Scaffold** ✅ | `blockliner-core` + `apps/blockliner/native` crates in the workspace; window opens with toolbar, empty canvas, grid, pan/zoom; icon (`assets/icon.svg/png`); `config.rs` paths. | 1 day |
| **1. Core model + Mermaid** ✅ | `model`, `mermaid::read/write`, `ids`, layout comments, passthrough, golden + round-trip tests. Can be built in parallel with phase 0. | 2–3 days |
| **2. Shapes** ✅ | Select, Rect, RoundRect, Text tools; snap; move; resize handles; marquee; delete; inline label editing; painting with theme colours. | 3–4 days |
| **3. Connectors** ✅ | Line/Arrow tools, anchors, glue/unglue, endpoint handles, arrowheads, hit-testing on segments, free lines. | 2–3 days |
| **4. History + keyboard** ✅ | Snapshot undo/redo with gesture coalescing; nudge; select all; command registry and palette entries. | 1 day |
| **5. Project + persistence** ✅ | Backend: open folder, diagram list sidebar, new/rename/delete, debounced atomic autosave, external-change reload; `lastProject`. | 2 days |
| **6. Import layout** ✅ | `layout::layered` for nodes without positions; "not editable" badge for unsupported node shapes. | 1–2 days |
| **7. Export** ✅ | Export dialog (§5.1): PNG and SVG, show/hide grid, All Objects or Selected Objects Only, PNG scale; `blockliner_core::export::scope`; app `export.rs` (SVG writer, `resvg` PNG); toolbar button, `Ctrl+Shift+E`, palette entry; remembered options. | 2–3 days |
| **8. Ship** ✅ | Packaging (§8): deb/rpm/AppImage/NSIS/dmg metadata, desktop entry, AppStream, a `text/vnd.mermaid` MIME type for `*.mmd`, Flatpak manifest; opening a path from the command line; READMEs, `ARCHITECTURE.md`, `docs/packaging.md`. CI already covered the crates from phase 0. Linux packages built and inspected; Windows/macOS hand checks listed in the native README. | 1–2 days |

**MVP total: about 3–3.5 weeks.**

### After the MVP: planned phases (detail in §10)

| Phase | Deliverable | Effort |
|---|---|---|
| **9. Right-angle connectors** ✅ | Right-Angle Line and Right-Angle Arrow tools; routes made of horizontal and vertical segments that avoid shapes where they can; switching selected connectors between straight and right-angle; a draggable middle segment. Saved in the layout block (`route=elbow`); the Mermaid edge is unchanged. | 3–4 days |
| **10. Shape styles** | Fill and stroke colours for shapes: preset swatches and a custom hex value, from the canvas's right-click menu and the palette. Saved as Mermaid `style` lines, which BlockLiner now owns for its shapes. | 2 days |
| **11. More Mermaid shapes** | Database `[( )]`, subroutine `[[ ]]`, hexagon `{{ }}` and diamond `{ }` as real, editable shapes with their own outlines, label areas and hit-testing, behind a "More Shapes" toolbar button. | 2 days |
| **12. Copy and paste** | `Ctrl+C`, `Ctrl+X`, `Ctrl+V` for shapes and connectors, within a diagram, between diagrams and between BlockLiner windows. The clipboard holds Mermaid, so a copy also pastes into a README, and a flowchart copied from elsewhere pastes in laid out. | 2 days |

**Phases 9–12: about 2 weeks.** They are independent; this order puts the
most-wanted first. Copy and paste goes last so it carries routes, colours
and the new shapes from the start.

### After phase 12 (ideas, in rough value order)

1. **Editable edge labels** (`a -- "HTTPS" --> b`; read-only today).
2. **Subgraphs as boundary containers** (system/trust boundaries), which
   architecture diagrams use a lot. Maps to Mermaid `subgraph`.
3. Duplicate (`Ctrl+D`), align/distribute, z-order.
4. Connector colours (`linkStyle`, which addresses edges by index) and the
   remaining Mermaid shapes (stadium, circle, parallelogram, trapezoid).
5. Opening `.mmd` files from NoteLiner.
6. More export options: a dark or transparent background, PDF, and
   "copy image to clipboard".

---

## 8. Repo touchpoints

Adding a fourth app touches these places outside the new crates:

- `Cargo.toml`: add `crates/blockliner-core` (covered by `crates/*`) and
  `apps/blockliner/native` to `members`; add `blockliner-core` to
  `[workspace.dependencies]`.
- `apps/blockliner/native/Cargo.toml`: copy PageLiner's packaging metadata
  (`deb`, `generate-rpm`, `packager`) with the new names, binary
  `blockliner`, `rfd = "0.15"` for the folder picker and the export save
  dialog, and `resvg` (the workspace's 0.45, as `marina-richtext` uses, with
  only the `text` feature) for PNG export.
- `apps/blockliner/native/build.rs`: Windows icon, copied.
- Package `depends`/`requires`: as PageLiner's **minus `git`**, which
  BlockLiner never runs.
- `packaging/linux/com.blockliner.app.{desktop,metainfo.xml,appimage.desktop.hbs}`,
  `packaging/flatpak/com.blockliner.app.yml`. The desktop file registers
  `text/vnd.mermaid` / `*.mmd` as a MIME type it can open. shared-mime-info
  doesn't define that type, so the packages also install
  `packaging/linux/com.blockliner.app.mime.xml` (`*.mmd`, `*.mermaid`) into
  `/usr/share/mime/packages/`, and the app opens a path given on the command
  line (`Exec=blockliner %f`). The Flatpak has no network, SSH or git module.
- `xtask/src/main.rs`: add `("blockliner", "com.blockliner.app")` to the
  icon list.
- `scripts/package-native.sh` (`ALL_APPS`) and `scripts/set-version.sh`
  (`APPS`).
- `.github/workflows/build.yml`: add `-p blockliner-core -p blockliner-native`
  to the clippy and test lines. (`release-native.yml` does not list apps by
  name. It goes through `package-native.sh`.)
- `README.md` (apps table, layout) and `ARCHITECTURE.md` ("three apps" →
  four, app cores list).
- `apps/blockliner/README.md` and `apps/blockliner/native/README.md`.

---

## 9. Decisions and open questions

| # | Question | Recommendation |
|---|---|---|
| 1 | File is Mermaid + layout comments, or a JSON model with Mermaid export? | **Decided: Mermaid + comments.** It matches the request ("using Mermaid"), there is one file per diagram, it renders on GitHub, and it diffs well. The cost is the free-line limitation in §4.2. |
| 2 | Free lines not attached to shapes: keep them (BlockLiner-only) or disallow them? | **Decided: allowed**, with the status-bar hint. |
| 3 | Project = folder of `.mmd` files (NoteLiner-like), or open/save single files? | **Decided: a plain folder, no git syncing.** Single-file "Open…" can be added cheaply later. |
| 4 | Render Mermaid's own layout in-app as a "preview" tab? | No. There is no pure-Rust Mermaid renderer, and this app has no webview. The canvas is the view. |
| 5 | Reuse `noteliner-slides`' colour/theme helpers? | Only `marina_egui::theme` for MVP. Shape styles come later. |
| 6 | Should canvas primitives move to a shared crate (`marina-canvas`)? | Not yet. Extract them only if NoteLiner slides or a second app needs them, the same approach as the SlideLiner plan. |
| 7 | Export colours: follow the app theme, or always light? | **Light, always** (white background, dark ink): exports end up in documents and wikis, and an export shouldn't change with the theme. A dark or transparent background can be an option later (§7). |
| 8 | Draw exports separately for SVG and PNG, or once? | **Once, as SVG**; PNG rasterises that SVG with `resvg` and the bundled fonts. One drawing path means the two formats can't drift apart. |
| 9 | Where does the export drawing live? | **In the app** (`export.rs`), not in `blockliner-core`: label wrapping must match the canvas, which uses egui's text layout. The core holds only what is exported and the page bounds (`export::scope`), which is easy to unit-test. |
| 10 | Right-angle routes in the Mermaid part too? Mermaid 11 can curve a single edge (`e1@{ curve: step }`), and older renderers can set it for the whole diagram (`%%{init: {"flowchart": {"curve": "step"}}}%%`). | **Not at first.** Edge ids and per-edge curves are recent Mermaid syntax; a renderer that doesn't know it rejects the whole diagram, which would break the "renders on GitHub" promise. Routes live in the layout block only, and Mermaid lays out and curves the edge itself, as it does for every edge today. Revisit once GitHub's Mermaid supports per-edge curves. |
| 11 | Elbow tools: separate toolbar buttons, or a straight/right-angle switch on the Line and Arrow buttons? | **Separate buttons and keys** (as asked: new line and arrow types). A selected connector can still be switched either way. |
| 12 | Where do colours get picked? | **The canvas's right-click menu** (new in phase 9/10, also holding cut/copy/paste and straight/right-angle) and **palette commands**. A floating style bar or an inspector pane is a bigger UI change, left for when there are more properties than two colours. |
| 13 | Keys for the new tools (§10.0)? | `D` diamond, `H` hexagon, `C` database (cylinder), `S` subroutine, `J` right-angle line, `E` right-angle arrow. None clashes; the `Ctrl+` versions of C, E and J stay copy, sidebar and status bar. |
| 14 | Pasting text that isn't a flowchart? | **A new text box** with that text, at the centre of the view, as Excalidraw does. Pasting a flowchart imports its nodes and edges. |

---

## 10. Phases 9–12 in detail

### 10.0 Toolbar and keys after phase 11

Nine tool buttons instead of six. The numbers still follow the toolbar, so
Line, Arrow and Text move to new numbers. This is fine: the numbers were
added on 2026-10-30 and haven't shipped yet.

| # | Button | Letter | Notes |
|---|---|---|---|
| 1 | Select | `V` | |
| 2 | Rectangle | `R` | |
| 3 | Rounded Rectangle | `O` | |
| 4 | More Shapes | `D` `H` `C` `S` | A flyout (phase 11). The button shows and picks the last one used; a small corner triangle, or a right-click, opens the four. Each letter picks its shape directly. |
| 5 | Line | `L` | was 4 |
| 6 | Arrow | `A` | was 5 |
| 7 | Right-Angle Line | `J` | new (phase 9) |
| 8 | Right-Angle Arrow | `E` | new (phase 9) |
| 9 | Text | `T` | was 6 |

- **Shift + key** keeps the tool, as now.
- **Window size:** nine tools plus the five bottom buttons need about 600 px
  of height. Raise the window's minimum height from 500 to 640 and check an
  offscreen render at that size.
- **Right-click menu:** one canvas context menu serves phases 9, 10 and 12.
  - **On a selection:** Cut, Copy, Paste; Straight / Right-Angle (for
    connectors); Fill ▸, Stroke ▸, Reset Style (for shapes); Delete.
  - **On empty canvas:** Paste and Select All.
  - Every item is also a palette command.

### 10.1 Phase 9: right-angle connectors

**What the user sees.**
- **Tools:** two new tools, Right-Angle Line and Right-Angle Arrow, draw
  connectors made only of horizontal and vertical segments.
  - **Glue:** they glue exactly like the straight tools: a fixed side, the
    shape body (dynamic), or a free end.
  - **Following shapes:** the route is recomputed whenever an end or a shape
    moves, so it always fits.
- **Switching:** Straight / Right-Angle in the right-click menu (and the
  palette) changes the selected connectors either way, as one undo step.
- **Middle segment:** a selected right-angle connector shows a handle on its
  middle segment. Dragging it moves that segment sideways, snapped to the
  grid, and the offset is kept. Double-clicking the handle drops the offset
  (back to automatic).

**Model.**
- **The route:** `Connector.route: Route { Straight, Elbow { mid: Option<f32> } }`.
  `kind` stays `Line`/`Arrow`, because that's what Mermaid's `---`/`-->`
  says. `mid` is the dragged middle segment's x (or y), in model units.
- **One path for everything:** `Diagram::connector_path(&Connector) -> Vec<Point>`
  replaces `connector_points`. A straight connector is a 2-point path.
  Drawing, hit-testing (each segment, within 5 points), arrowheads (along the
  last segment), edge labels (at the path's midpoint by length), export and
  the export scope all read the path, so they can't disagree.

**The router** (`blockliner-core/src/route.rs`, pure and unit-tested):
1. **Stubs.** Each glued end leaves its side straight out for one grid cell
   (a fixed anchor's side, or for dynamic glue each of the four sides in
   turn). A free end has no side: it leaves along the axis that points most
   towards the other end.
2. **Candidates** between the two stub tips:
   - two one-bend routes (horizontal then vertical, and the reverse);
   - two two-bend routes through the midpoint (vertical middle segment at
     the mid x, horizontal at the mid y, snapped to the grid);
   - when a stub points away from the other end, routes that go around the
     outside of both boxes (one cell past the outermost edge).
   With `mid` set, only the two-bend routes are tried, through `mid`.
3. **Score** each candidate:
   - a large cost per shape the route crosses (the two end shapes included,
     so a route never cuts back through its own box);
   - a small cost per bend;
   - plus its length.
   Ties go to the first candidate, so routing is deterministic. Dynamic glue
   tries each side pair (16) and keeps the best. That's at most about 100
   tiny candidates per connector, cheap enough to run every frame.
4. **Tidy up:** drop collinear and duplicate points.

5. **Second pass** (added while building it): if the best route still
   crosses a shape, the routes around that shape's sides (one cell out) are
   tried too, so a shape between two others is gone round.

This avoids other shapes when one of the candidate routes does, which covers
typical architecture diagrams. A route that has to cross a box, because all
candidates do, still crosses it, but never its own end boxes (those cost
far more). Full maze routing (A* on a sparse grid) is a later refinement if
real diagrams need it.

**Speed.** Every frame asks for every path several times (drawing, hover
hit-testing, the marquee), so routes are cached by everything they depend
on: the ends, the pinned middle, every shape's rectangle and the grid. Only
frames where something moved route anything. Measured on 100 shapes with 98
right-angle connectors between random ones: about 9 ms (release) to route
them all, then 0.6 ms per frame (release) or 5 ms (debug) when nothing
moved.

**As built** (2026-10-30), beyond the above:
- **Toolbar now:** Select 1, Rectangle 2, Rounded Rectangle 3, Line 4,
  Arrow 5, Right-Angle Line 6 (`J`), Right-Angle Arrow 7 (`E`), Text 8. The
  numbers follow `Tool::ALL`, so phase 11's More Shapes button renumbers
  them to §10.0's table without further code.
- **Minimum window height** is already 640: eight tools and the bottom
  group overlapped at 500 (checked in an offscreen render).
- **Right-click menu:** Right-Angle and Straight (each greyed out when no
  selected connector would change), Delete, Select All. A right-click
  selects what's under it first, or clears the selection on empty canvas.
  Phases 10 and 12 add their items.
- **Palette:** "Make Connectors Right-Angle" and "Make Connectors
  Straight".

**File.**
- **Glued edges:** `%% @edge a->b route=elbow` (with `mid=240` once the
  middle segment is dragged), alongside the existing `from=`/`to=` anchors.
  An `@edge` line is now written when an anchor is fixed **or** the route
  isn't straight.
- **Free lines:** `%% @line arrow from=… to=… route=elbow`.
- **The Mermaid part doesn't change** (§9, question 10): `a --> b` stays
  `a --> b`.
- **No version bump:** the reader already ignores unknown fields. An older
  BlockLiner opens these files with straight connectors and, if the user
  edits them, writes them back straight. Acceptable for an app with no older
  installs. The format stays `blockliner 1`.

**Imported diagrams** keep straight connectors. Making long imported edges
right-angle could become a layout option later.

**Tests.**
- **Router unit tests:** each side combination; free ends; stubs pointing
  away; `mid`; collinear clean-up.
- **Random-diagram property test,** like the layout's 300 graphs:
  - every route is axis-aligned;
  - every route starts and ends on its anchors;
  - no route enters its own end shapes;
  - whenever any candidate avoided every other shape, the chosen route does
    too.
- **Mermaid:** golden files and the round trip with `route=` and `mid=`.
- **Headless UI:**
  - drawing with both tools;
  - switching a connector between straight and right-angle (and undoing it);
  - dragging the middle segment;
  - a route following a moved shape.
- **Export:** the golden SVG gains a `<polyline>` connector.

### 10.2 Phase 10: shape styles

**What the user sees.**
- **Fill ▸ and Stroke ▸:** two submenus in the right-click menu on selected
  shapes, each showing:
  - **Eight presets:** Gray, Blue, Teal, Green, Yellow, Orange, Red, Purple.
    Each fill is a light tint, paired with a darker stroke of the same hue,
    so picking a Fill also sets its matching Stroke unless the stroke was
    set on its own.
  - **Default:** back to the theme's colours.
  - **Custom…:** a hex field (`#rgb` or `#rrggbb`) with a preview.
- **Reset Style** clears both.
- **Several shapes:** a style applies to every selected shape, as one undo
  step.
- **Text boxes** take a fill and stroke too, so a text box can become a
  coloured note.
- **Labels on a custom fill** are drawn in near-black or white, whichever
  contrasts more with the fill, on the canvas and in exports. Without a
  custom fill, labels keep the theme's text colour.

**Model.**
- **The style:** `Shape.style: Style { fill, stroke, color: Option<Colour>, extra: Vec<(String, String)> }`.
- **`Colour`:** keeps the text as written (`#f9f`, `#dbeafe`, `red`) and a
  parsed RGBA when it can parse it (hex, and the CSS names Mermaid examples
  use). An unparseable value is kept and written back unchanged, but drawn
  in the default colour.
- **`extra`:** the other properties (`stroke-width`, `stroke-dasharray`, …),
  kept in order and written back unchanged. Not edited in this phase.

**File.**
- **Taking over `style`:** BlockLiner now owns `style <id> …` lines for its
  own shapes. They're read into `Shape.style` instead of being kept
  verbatim. A `style` line for an external shape, or one naming several
  ids, stays verbatim.
- **Several lines for one shape** merge; the later one wins.
- **Written** as one `style <id> fill:#dbeafe,stroke:#3b82f6,color:#111827`
  per styled shape, after the edges and before the kept statements, in
  shape order. Nothing is written for an unstyled shape.
- **`color` is written** with a custom fill, chosen for contrast, because
  Mermaid's own text colour follows the viewer's theme: a pale fill with
  white text (GitHub's dark mode) would be unreadable.
- **`classDef`/`class`** stay verbatim and aren't drawn: BlockLiner doesn't
  apply class styles to the canvas. `bl_text` keeps its meaning, and a
  shape's own `style` beats its class in Mermaid, as it does here.
- **First save:** a hand-written file's `style` lines move to the normalised
  position, as edges already do (the §4.3 rule: normalised once, then
  byte-stable).

**Tests.**
- **Mermaid:** reading and writing `style` (merge, `extra` kept, external
  and multi-id lines kept verbatim, unknown colours kept); golden files and
  the round trip with random styles.
- **Contrast:** the label colour against each preset and against extremes.
- **Headless UI:**
  - a preset from the right-click menu on two shapes (one undo step);
  - a custom hex value, including a rejected bad one;
  - Reset Style.
- **Export:** the golden SVG with a styled shape.

### 10.3 Phase 11: more Mermaid shapes

**What the user sees.**
- **The More Shapes button** (§10.0) offers four shapes:

  | Shape | Mermaid | Default size (cells) | Key |
  |---|---|---|---|
  | Diamond (decision) | `id{"label"}` | 8 × 4 | `D` |
  | Hexagon | `id{{"label"}}` | 8 × 3 | `H` |
  | Database (cylinder) | `id[("label")]` | 6 × 4 | `C` |
  | Subroutine | `id[["label"]]` | 8 × 3 | `S` |

- **Full shapes:** they draw, select, resize, glue, style (phase 10) and
  export like rectangles.
- **Existing files:** shapes of these kinds that files already contain lose
  their "not editable" badge and dashed outline. They were kept as
  `ShapeKind::Other` until now.
- **Still kept as before:** the remaining Mermaid shapes (stadium, circle,
  parallelogram, trapezoid, asymmetric).

**Model.**
- **New kinds:** `ShapeKind::{Diamond, Hexagon, Database, Subroutine}`. The
  reader maps the four delimiter pairs to them instead of `Other`.
- **No file change:** the writer emits the same delimiters, so files that
  already contain these shapes are byte-identical. There's nothing to
  migrate.

**Geometry** (`blockliner-core/src/geometry.rs`):
- **Outlines** within the shape's rectangle:
  - **Diamond:** the four side midpoints joined.
  - **Hexagon:** flat top and bottom; the left and right points inset by
    `min(h / 2, w / 4)`.
  - **Database:** sides plus an elliptical cap, top and bottom, of height
    `min(h / 5, 12)`.
  - **Subroutine:** a rectangle with inner vertical lines 8 px in from each
    side.
- **Hit-testing:** exact polygons for the diamond and hexagon, so a click in
  a diamond's empty corner selects what's behind it. The database and
  subroutine use their rectangle.
- **Anchors** stay the four side midpoints of the rectangle. Those are the
  diamond's vertices and the hexagon's points, which is where connectors
  should meet them anyway.
- **Label areas,** so text wraps inside the outline:
  - **Diamond:** the inner rectangle (half the width and height).
  - **Hexagon:** inside the points.
  - **Database:** below the top cap.
  - **Subroutine:** between the inner lines.

**Drawing.**
- **Canvas** (`canvas/paint.rs`): paths for the outlines.
- **Toolbar icons:** small drawings like the current tool icons.
- **Export:** the SVG writer gets matching `<polygon>`/`<path>` elements,
  and the PNG follows as before.

**Tests.**
- **Geometry:** outline points, polygon hit-tests (a diamond's corners
  miss), label areas.
- **Mermaid:** reading each delimiter into its kind; files with these
  shapes are byte-identical through read → write; the round-trip generator
  includes the new kinds.
- **Headless UI:** each key and the flyout; a diamond's default size;
  gluing an arrow to a diamond's vertex.
- **Export:** the golden SVG gains all four shapes.

### 10.4 Phase 12: copy and paste

**What the user sees.**
- **Copy, cut and paste:** `Ctrl+C`, `Ctrl+X` and `Ctrl+V`, plus the
  right-click menu and the palette.
  - **Copy** takes the selected shapes and lines.
  - **Cut** also deletes them (one undo step).
  - **Paste** adds them and selects them (one undo step).
- **What's copied:**
  - **Shapes** with labels, kinds, styles and sizes.
  - **Connectors** glued at both ends to copied shapes, plus any selected
    connector.
  - **Ends glued to a shape that isn't copied** become free ends at their
    current point, so a selected line is never lost.
  - **Routes and edge labels** come along.
- **Where it lands:**
  - **The first paste** lands two grid cells down and right of the
    originals; each further paste of the same copy steps another two cells.
  - **Off-screen:** if that would put it outside the view (another diagram,
    or the originals scrolled away), it's centred in the view instead.
  - Positions stay snapped.
- **Ids:** pasted shapes get new ids where the copied ones are taken
  (`orders` → `orders_2`, through `ids::unique`), and their connectors
  follow.
- **External shapes** (declared in a kept statement) are copied as ordinary
  shapes of their kind. The copy has its own declaration, so it's fully
  editable.

**The clipboard holds Mermaid text:** the selection written as a small
BlockLiner diagram (`flowchart`, nodes, edges, `style` lines and the layout
block, through `mermaid::write`). So:
- **Into a README or a NoteLiner note:** it pastes as valid Mermaid that
  renders as is.
- **Between BlockLiner windows and diagrams:** it pastes back exactly,
  layout included.
- **A flowchart copied from elsewhere** (a README, Mermaid Live) pastes in,
  laid out by `layout::layered` as on import, and placed by the same rules.
- **Text that isn't a flowchart** pastes as a new text box (§9, question 14).

**Mechanics.**
- **Clipboard events:** egui turns the platform's copy, cut and paste keys
  into `Event::Copy`, `Event::Cut` and `Event::Paste(text)`. The app handles
  those (not key bindings) when no text field has focus. While a label is
  being edited they belong to the text field, as now.
- **The commands:** the palette and menu commands for copy and cut use
  `ctx.copy_text`. Paste from the menu sends `ViewportCommand::RequestPaste`
  (egui 0.37), which delivers the clipboard as the same `Event::Paste`, so
  every paste goes through one path.
- **Core:** a new `blockliner-core/src/clipboard.rs`:
  - `fn copy(d, shapes, lines) -> Diagram` reuses the export scope's
    selection rule, with the free-end conversion above;
  - `fn paste(d, clip: Diagram, at: Offset) -> Vec<String>` renames ids,
    remaps glue and offsets positions, and returns the new ids to select.
- **Not in this phase:** images on the clipboard, and pasting into a
  selected shape's label.

**Tests.**
- **Core:**
  - copy then paste: the counts, new ids, remapped glue;
  - free-end conversion;
  - styles, routes and edge labels survive;
  - pasting the same copy repeatedly steps;
  - pasting into a diagram whose ids clash.
- **Clipboard text:** the copied text parses as Mermaid (a `mermaid::read`
  round trip), and a plain flowchart pastes laid out.
- **Headless UI:**
  - `Ctrl+C`/`Ctrl+V` and `Ctrl+X` through egui's clipboard events (one
    undo step each);
  - paste into a second diagram;
  - plain text becomes a text box;
  - nothing happens while a label is being edited.

### 10.5 Docs per phase

- **README tables:** the native README's keys and features tables
  (`apps/blockliner/native/README.md`, `apps/blockliner/README.md`).
- **This plan:** §4.1/§4.2 (the `route=`/`mid=` fields, `style` ownership,
  the four shapes in the mapping table) and the status line.
- **The help text** behind Settings → Keyboard Shortcuts comes from the
  command registry, so it updates itself.
