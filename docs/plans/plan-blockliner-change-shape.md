# BlockLiner: Change Shape and Arrowheads — Plan

Status: **Implemented (2026-11-01), BlockLiner 2.4.0.** Lets the user turn a selected shape into another kind of shape (a rectangle into a diamond, say) and a line into an arrow or back, from the Properties pane, the right-click menu and the command palette.

## Decisions and changes during implementation

- **Open questions:** (1) Change Shape is a submenu (▸), not a dialog; (2) the pane says "Change Line:" with Line / Arrow; (3) Text stays in the list; (4) only BlockLiner was bumped, to 2.4.0, by hand (the other apps stay at 2.3.1).
- **Text and classes:** a Text shape is saved as `:::bl_text`, and a Mermaid node takes one class, so a shape with a class of its own that becomes Text loses that class when saved. This was already true of Text; the round-trip test records it.
- **Label edits:** an edit in progress is committed before the change, but in the same frame, so the two share one undo step (not separate steps as planned).
- **Pane height:** `properties::DEFAULT_HEIGHT` went from 380 to 500 px so Reset Style stays in view when both new rows show. A saved, smaller height still scrolls.
- **Menu placement:** Change Shape ▸ sits with Fill ▸ and Stroke ▸ (after Paste), not straight after Edit Label.

## Today

- **A shape's kind is fixed once drawn.** `Shape.kind` (`blockliner-core/src/model.rs`) is set by the tool that drew it or by the Mermaid brackets it was read with. To change it, the user has to delete the shape, draw a new one, then re-type the label, re-attach the lines and set the colours again.
- **The same goes for a line's arrowhead.** `Connector.kind` is `Line` (`a --- b`) or `Arrow` (`a --> b`), set by the Line or Arrow tool.
- **Everything else a change would need already exists:**
  - **Geometry:** outlines, anchors, label areas and click areas all work from `kind` and `rect`, so a shape that changes kind but keeps its rectangle stays put, and its glued lines stay glued.
  - **Saving:** `write` takes the brackets from `ShapeKind::delimiters()` and the line text from `ConnectorKind`. A changed kind saves correctly with no format change.
  - **Undo:** `History::observe` (`app.rs`) records one undo step per finished change to the diagram, so a kind change is undoable with no extra work.
  - **A pattern to copy:** Right-Angle / Straight (`Canvas::set_route`, `Act::RouteElbow` / `RouteStraight`, the right-click items and palette commands) already changes a property of every selected line. It is the model for both new features.
- **The Properties pane** (`properties.rs`) shows a summary line ("Rectangle “API Gateway”"), then the Fill and Stroke rows and Reset Style. It shows **Mixed** when the selected items differ.
- **The right-click menu** (`Canvas::context_menu` in `canvas/mod.rs`) has Edit Label, Cut / Copy / Paste, Fill ▸, Stroke ▸, Reset Style, Right-Angle / Straight, Delete and Select All.
- **Shape names are written in two places:** `Tool::name()` (`canvas/tools.rs`) and a `match` in `properties::summary`.

## Proposal

### What can change

- **Shapes:** any shape BlockLiner declares can become any of the 15 kinds it draws: Rectangle, Rounded Rectangle, Text and the twelve More Shapes. The list is in toolbar order: Rectangle, Rounded Rectangle, then `ShapeKind::MORE`, then Text.
  - **Kept-statement shapes are left alone.** A shape declared inside a subgraph or an unsupported edge (`external`) has its brackets in a statement BlockLiner keeps verbatim. It is skipped, just as for colours, with the same "N kept-statement shapes not affected" note.
  - **`ShapeKind::Other`** (unknown brackets, unreachable from the parser today) is shown as "Shape" in the list's current value. It can be changed *to* a real kind but is never offered as a choice.
- **Lines:** any selected connector (Mermaid edge or free line) can be switched between **Line** and **Arrow**. Edges BlockLiner keeps verbatim (`-.->`, `==>`, …) are not connectors, so they can't be selected and aren't affected.

### What a change keeps

- **Kept as they were:** the rectangle (position and size), id, label, colours (`style`), any `:::class`, and every line glued to the shape. The anchor points move with the new outline, for example onto a parallelogram's slanted sides or into an asymmetric shape's notch, because anchors are computed from the kind.
- **The size is not changed.** A rectangle turned into a circle becomes an ellipse in the same box, as a circle drawn wide does today. This keeps lines and the surrounding layout stable. Snapping to the new kind's default proportions is left for later (see *Out of scope*).
- **Text** is a borderless rectangle tagged `:::bl_text` when saved. Changing to or from Text only changes `kind`. `write` already adds or drops the tag, and a user class (`class`) is unaffected.
- **Lines:** only `kind` changes. The label, route (including a pinned middle), colour and `linkStyle` number all stay, because the edge count and order don't change. An arrow points at the line's `to` end, the end it was drawn towards.

### Properties pane

New rows go between the summary and Fill:

```
PROPERTIES
Rectangle “API Gateway”

Change Shape:
[ ▭  Rectangle              ▾ ]

Change Line:
[ →  Arrow                  ▾ ]

Fill
…
```

- **Change Shape:** shown when at least one changeable shape is selected.
  - **The closed list** shows the current kind's icon and name, or "Mixed" (no icon) when the selected shapes differ.
  - **The open list** has one row per kind, with the toolbar's icon (`paint_tool_icon`) and the current kind ticked. These are the More Shapes popup's rows (`more_shapes_row`), without the key letters.
  - **Picking a kind** changes every changeable selected shape. Picking the current kind when they already agree does nothing.
- **Change Line:** shown when at least one line is selected. It offers **Line** and **Arrow**, with the toolbar's Line and Arrow icons, and shows "Mixed" when the selection has both.
- **Off states** follow the Fill row:
  - With only kept-statement shapes selected, Change Shape is shown disabled, with the existing reason text.
  - With nothing selected, neither row is shown.
- **The widget:** a small BlockLiner dropdown in `properties.rs`. It uses the same `egui::Popup` pattern as `laankit_egui::settings::dropdown` so it looks like the app's other selects, but adds icons and a Mixed value. `settings::select` can't show either. The accessible name is the row label, so tests can find it by "Change Shape" / "Change Line".

### Right-click menu

- **Change Shape ▸**, a submenu like Fill ▸ and Stroke ▸.
  - **When it shows:** whenever the right-click selection includes a changeable shape. It goes after Edit Label, in the group with Fill and Stroke.
  - **Its rows:** the same kinds, with the same icons and the current kind ticked. Picking one changes every changeable selected shape.
- **Line / Arrow** for lines, two items in the existing Right-Angle / Straight group:
  - **Add Arrowhead:** on when any selected line is a plain line.
  - **Remove Arrowhead:** on when any selected line is an arrow.

  This matches how Right-Angle / Straight work (both shown, each on only when it would change something), rather than a submenu with two entries.

### Command palette

- **Shape commands:** "Change Shape: Rectangle", "Change Shape: Diamond", … one per kind, in group "Edit". They are available when a changeable shape is selected. No key bindings: the single letters already pick drawing tools.
- **Line commands:** "Add Arrowheads" / "Remove Arrowheads", available under the same rules as the menu items.

### One shape-name list

- **Add `ShapeKind::name()`** in `blockliner-core` ("Rectangle", "Double Circle", …, "Shape" for `Other`).
- **Use it everywhere:**
  - `Tool::name()` uses it for the shape tools.
  - `properties::summary` replaces its `match` with it.
  - The new dropdown, menu and palette all use it.
- **Add `ShapeKind::CHANGEABLE: [ShapeKind; 15]`**, the list in toolbar order, so the pane, menu and palette can't get out of step.

### Edge cases

- **A label edit in progress** is committed first, as other menu actions do (`commit_label`), so the change and the new text become separate undo steps.
- **Mixed selections** of shapes and lines show both rows. Each row changes only its own kind of item.
- **The More Shapes button** is unaffected. Changing a shape doesn't change the current tool or the button's remembered shape.
- **Diagram reload** is unaffected. A kind change is an ordinary edit: it autosaves and is reverted by an outside edit like any other change.

## Phases

### Phase 1: Core (~½ day)

- `ShapeKind::name()` and `ShapeKind::CHANGEABLE` in `model.rs`. `Tool::name()` and `properties::summary` switched over.
- `Diagram::set_shape_kind(&mut self, ids: &[String], kind: &ShapeKind) -> usize`: changes each listed non-external shape that isn't already `kind`, and returns how many changed.
- `Diagram::set_connector_kind(&mut self, indexes: &[usize], kind: ConnectorKind) -> usize`, likewise.
- **Tests (core):**
  - The kind changes while rect, id, label, style, class and glued connectors are untouched.
  - External shapes are skipped.
  - `write` → `read` round-trips each of the 15 kinds after a change, including Rect ↔ Text adding and dropping `:::bl_text`.
  - Line ↔ Arrow keeps the label, route and `linkStyle` index.
  - A free line's `@line line|arrow` field flips.

### Phase 2: Commands and canvas (~½ day)

- `Act::ChangeShape(ShapeKind)`, `Act::AddArrowheads`, `Act::RemoveArrowheads` in `commands.rs`, applied in `App::apply` through `Canvas` helpers next to `set_route`:
  - `Canvas::change_shape(d, kind)`.
  - `Canvas::set_arrowheads(d, on)`.
  - `Canvas::selected_kinds(d) -> (Common<ShapeKind>, Common<ConnectorKind>)` for the Mixed / current display. This is generic over `Common`, or a small local enum if `style::Common` stays colour-only.
- **Palette entries** registered with `when` conditions: a changeable shape is selected / a plain line is selected / an arrow is selected.
- `MenuAction::ChangeShape(ShapeKind)`, `AddArrowheads`, `RemoveArrowheads` mapped to the acts in `app.rs`.

### Phase 3: Right-click menu (~½ day)

- The **Change Shape ▸** submenu with icon rows. `paint_tool_icon` moves from `views.rs` to a shared spot, or becomes `pub(crate)`.
- **Add Arrowhead / Remove Arrowhead** items in the route group.

### Phase 4: Properties pane (~1 day)

- `PropertiesAction::ChangeShape(ShapeKind)`, `SetArrowheads(bool)`.
- `PropertiesProps` gains the selection's shape kind and line kind (current / Mixed / none) and whether any shape is changeable.
- The icon dropdown widget, plus the **Change Shape:** and **Change Line:** rows with their disabled state and reason.
- **Minimum height:** check that the rows fit in the pane's `DEFAULT_HEIGHT` (380 px); `MIN_HEIGHT` scrolls as today.

### Phase 5: UI tests and docs (~½ day)

- **`ui_tests_change_shape.rs`** (headless, like `ui_tests_properties.rs`):
  - Select a rectangle, pick Diamond from "Change Shape" in the pane: the kind changes, the rect, label and colour are kept, the saved line reads `id{"…"}`, and one Ctrl+Z restores the rectangle.
  - Right-click a shape → Change Shape ▸ → Database. An arrow glued to its east side stays glued.
  - Two different shapes selected: the dropdown reads "Mixed"; picking Hexagon changes both.
  - A kept-statement shape selected with an own shape: only the own shape changes; the note says so.
  - Palette "Change Shape: Circle".
  - Lines: "Change Line" → Arrow and back; right-click Add / Remove Arrowhead on a mixed selection; a free line and a Mermaid edge both flip; one undo step each.
- **Docs:**
  - `apps/blockliner/README.md`: a *Change shape* bullet under Features.
  - `apps/blockliner/native/README.md`: the menu items in the right-click row of the keys table.
  - `ARCHITECTURE.md`, if it lists the Properties pane's contents.

### Phase 6: Version bump (~¼ hour)

The last step, once everything above is merged-ready and tests pass.

- **Increment BlockLiner's version** from **2.3.1** to **2.4.0**: a minor bump, because this adds features and doesn't change the file format.
- **Where the version lives:**
  - `apps/blockliner/native/Cargo.toml`, `[package] version`.
  - `packaging/linux/com.blockliner.app.metainfo.xml`, a new dated `<release version="2.4.0" date="…"/>` entry above 2.3.1.
  - `Cargo.lock`, the `blockliner-native` entry (refreshed by `cargo check`).
  - `crates/blockliner-core` keeps its own version (0.1.0), as all shared crates do.
- **How:** `scripts/set-version.sh 2.4.0` makes all three edits. Note that the script sets **every** app to that version, because `docs/packaging.md` says all apps share one version. If only BlockLiner should change, make the same three edits by hand instead (see open question 4).
- **Check:** `cargo test -p blockliner-native -p blockliner-core` still passes, and the About box shows 2.4.0.

**Total: about 3 days.**

## Out of scope (possible follow-ups)

- **Fit to the new shape:** resizing to the new kind's default proportions, e.g. making a circle square, as a separate "Fit Shape" command.
- **Reverse direction:** swapping a line's `from` / `to` so the arrow points the other way.
- **Arrowheads on both ends** (`<-->`) or other Mermaid link styles (dotted `-.->`, thick `==>`). These would need new `ConnectorKind`s and are a larger format change.
- **A shape-change key** (e.g. Ctrl+Shift+K to open the list at the pointer).

## Open questions

1. **"Change Shape…" with an ellipsis.** In menus, "…" usually means "opens a dialog". The plan uses a **Change Shape ▸ submenu**, which is one click shorter and matches Fill ▸ / Stroke ▸. Is that acceptable, or is a dialog (e.g. a grid of shape icons) wanted?
2. **Label wording for lines.** Is "Change Line:" with Line / Arrow the right wording for the pane, or would **"Arrowhead:"** with None / End read better? The menu items are "Add Arrowhead" / "Remove Arrowhead" either way.
3. **Text in the list.** Turning a shape into Text drops its outline. That's useful (box → caption), but it's the one choice that makes a shape look like it has disappeared when it has no label. Keep it in the list?
4. **BlockLiner only, or all apps?** All four apps are at 2.3.1, and `docs/packaging.md` says they share one version. Bumping only BlockLiner to 2.4.0 breaks that rule; running `scripts/set-version.sh 2.4.0` keeps it but also bumps NoteLiner, PageLiner and ThreadLiner with no changes of their own. Which is wanted? And is 2.4.0 the right number, or should it be the patch bump 2.3.2?
