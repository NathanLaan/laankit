# BlockLiner: Stroke Styles (Solid, Dashed, Dotted) — Plan

Status: **Implemented (2026-11-02), BlockLiner 2.5.0.** See "Decisions and changes during implementation" at the end. Lets the user draw a shape's outline or a line as solid, dashed or dotted, set from the Properties pane, the right-click menu or the command palette, and saved as plain Mermaid.

## Today

- **Every stroke is solid.** The canvas (`canvas/paint.rs`) draws outlines with egui's solid primitives (`painter.rect`, `ellipse_stroke`, `convex_polygon`, `closed_line`) and connectors as solid polylines (`paint::connector`). The SVG export (`export.rs`) writes no `stroke-dasharray`.
- **Styles already round-trip unknown properties.** `Style` (`blockliner-core/src/style.rs`) holds `fill`, `stroke`, `color` and an `extra: Vec<(String, String)>` of everything else. A hand-written `style api stroke-dasharray:5 5` is read, kept and written back, but not drawn.
- **Where a style is saved depends on what it belongs to:**
  - **A shape:** a `style <id> …` line.
  - **A Mermaid edge:** a `linkStyle <n> …` line, numbered in file order.
  - **A free line:** a ` stroke=<colour>` field on its `%% @line` comment. The reader looks fields up by name, so an unknown field is ignored rather than refused.
- **Mermaid's own dotted links aren't editable.** `a -.-> b` and `a -.- b` lex as `LinkKind::Other` (`mermaid/lex.rs`, `classify`). The whole statement is kept verbatim: it is drawn as a plain edge, but can't be selected, styled or deleted.
- **A dashed outline already means something.** `ShapeKind::Other` (unknown brackets) is drawn as a dashed rectangle to read as "not quite this", and an empty Text box has a faint dashed outline so it can be found. Since the classic-shapes work, every bracket pair the parser recognises maps to a real kind, so `Other` is effectively unreachable.
- **egui can draw both patterns:** `egui::Shape::dashed_line(points, stroke, dash, gap)` and `egui::Shape::dotted_line(points, colour, spacing, radius)` take any polyline.

## Proposal

### The styles

Three choices, the same for shapes and lines:

| Style | Saved as | Drawn as (at 100 %) |
|---|---|---|
| **Solid** (default) | nothing: the property is removed | a solid stroke |
| **Dashed** | `stroke-dasharray:6 4` | 6 px dashes, 4 px gaps |
| **Dotted** | `stroke-dasharray:2 3`; a Mermaid edge uses `-.->` / `-.-` instead (see below) | round dots, 3 px apart |

- **Style values use spaces, not commas.** Mermaid separates `style` properties with commas, so the values must be space-separated (`5 5` is also the form Mermaid's docs use).
- **Reading back:**
  - `stroke-dasharray` values matching the presets (`6 4`, `2 3`, ignoring extra spaces) read as Dashed and Dotted.
  - `none` or `0` reads as Solid.
  - Any other value is **Custom**: kept and written back exactly as written, drawn with its numbers when they parse (else solid), and shown in the UI as none of the three. Picking a style replaces it.
- **Mermaid's dotted links become editable:**
  - The lexer reads `-.->` as an arrow and `-.-` as a line, both Dotted, so hand-written dotted edges become ordinary connectors (selectable, colourable, deletable).
  - The writer writes a Dotted edge back as `-.->` / `-.-`, Mermaid's own syntax, so it looks dotted in any Mermaid renderer even without the `linkStyle` line.
  - **What Mermaid draws:** a `-.->` edge looks like short dashes there (Mermaid's dotted link is `stroke-dasharray:3`), which is close enough to read as dotted.
  - **Still kept verbatim:** thick links (`==>`) and links with arrowheads at both ends (`<-->`), as today.
- **Dashed edges** have no Mermaid syntax of their own, so they are written as `linkStyle <n> stroke-dasharray:6 4`, merged into the edge's existing `linkStyle` line with its colour.
- **Free lines** get a ` dash=dashed|dotted|<raw>` field on their `%% @line` comment, next to ` stroke=`. A raw Custom value is written only when it has no spaces, the same rule `stroke=` follows for colours. The format version stays 1: older BlockLiner builds ignore the field, so a free line's dash would be dropped if an old build re-saved the file.

### Model

- **A typed field.** Add `Style.dash: Option<Dash>` with `enum Dash { Dashed, Dotted, Custom(String) }`; `None` is Solid. It is parsed out of `extra` on read (as `fill` / `stroke` / `color` are) and written back in its old position among the properties, so a hand-written line's property order is unchanged.
- **Rules for an edge:** an edge's `Dotted` lives in its link syntax, not its `linkStyle`. Reading `-.->` together with a `linkStyle` dasharray uses the `linkStyle` value, the more specific of the two. Writing a Dotted edge emits `-.->` and no dasharray.
- **Helpers:**
  - `Dash::pattern(&self) -> Option<(f32, f32)>`: dash and gap for drawing (`None` for Solid or an unparsable Custom).
  - `Style::set_dash(Option<Dash>)`.
  - `style::common_dash(…)`, the counterpart of `style::common` for colours: `None` / `Same` / `Mixed` across a selection.

### Drawing

- **Shapes, canvas:** for a non-solid shape, fill the body as today with the stroke turned off, then draw the outline as a dashed or dotted path.
  - **New helper:** `geometry::outline_path(kind, rect, steps) -> Vec<Vec<Point>>`, the outline as one or more closed polylines.
  - **Where the paths come from:** polygons come from `polygon`; rectangles and rounded rectangles (corner arcs sampled), stadiums and circles / ellipses are sampled.
  - **Extra outline parts:** the database's lid, the subroutine's inner lines and the double circle's inner ring are extra paths in the same dash.
  - **Scaling:** dash, gap and dot size scale with the zoom, as the stroke width does.
  - **Solid stays as it is:** solid shapes keep today's primitives, so their look and speed don't change.
- **Lines, canvas:** dash or dot the polyline (straight or right-angle). The arrowhead stays solid. The dash restarts at each end so both ends start with a mark.
- **SVG and PNG export:** add `stroke-dasharray="6 4"` / `"2 3"` (Custom values as written) to the outline element. Dotted also gets `stroke-linecap="round"` so the dots look round. Arrowheads stay solid. resvg renders these, so PNG export needs no extra work.
- **Dashed already means something** (two existing uses):
  - **`ShapeKind::Other`:** drop its dashed rectangle and draw it solid. Its `[ ]` badge and status-bar note already say "drawn here as a box", and two different meanings for dashes would confuse.
  - **An empty Text box:** keep its faint finding aid, but make it a light dotted outline in `text_faint`. It is too faint to be mistaken for a style; a Text box given a stroke draws in its own style.

### Properties pane

A **Stroke Style:** row directly under the Stroke row, because it belongs to the stroke:

```
STROKE
[■ #3b82f6] Default ■ ■ ■ ■ ■ ■ ■ ■

STROKE STYLE:
[ ──── Solid ][ ‒ ‒ ‒ Dashed ][ ····· Dotted ]

[ Reset Style ]
```

- **A three-way segmented control**, not a dropdown: each button draws a short sample of its pattern plus its name. One click, and the choice is visible without opening anything.
- **State:**
  - The current style's button is pressed.
  - With **Mixed** or **Custom** selections, no button is pressed and a muted note reads "Mixed" or "Custom".
- **Picking a button** sets every selected shape BlockLiner declares and every selected line, exactly like the Stroke swatches. It is one undo step.
- **When it's on or off:** it follows the Stroke row, on when anything stylable is selected. Kept-statement shapes are skipped, with the existing note.
- **Accessibility:** each button is `Role::RadioButton` named "Stroke Style: Dashed" etc., so tests and screen readers can find them.
- **Pane height:** the row adds about 60 px. `properties::DEFAULT_HEIGHT` goes from 500 to about 560 so Reset Style stays in view with Change Shape and Change Line showing (measured as last time). A smaller saved height scrolls.

### Right-click menu

- A **Stroke Style ▸** submenu right after Stroke ▸, shown whenever Stroke ▸ is.
- It has three radio-style rows, Solid / Dashed / Dotted, each with its sample drawn and the current one ticked. Mixed or Custom ticks none.

### Command palette

"Stroke Style: Solid", "Stroke Style: Dashed", "Stroke Style: Dotted" in group "Style", available when the selection is stylable. No key bindings.

### Reset Style

**Reset Style** sets the dash back to Solid too: it means "back to the theme's look", and a dash is now a style BlockLiner manages. Other unknown properties (`stroke-width`, …) still stay. See open question 2.

## Phases

### Phase 1: Model and file format (~1 day)

- `Dash`, `Style.dash`, parsing out of `extra` with order kept, `Style::set_dash`, `Dash::pattern`, `common_dash`. `Style::reset` clears the dash.
- **Free lines:** the `@line … dash=` field, read and written.
- **Lexer and reader:** `-.->` / `-.-` become `Arrow` / `Line` connectors with `Dotted`. Labels in both forms: `a -.->|text| b` and `a -. text .-> b`.
- **Writer:** Dotted edges write `-.->` / `-.-`; Dashed and Custom go into the edge's `linkStyle`.
- **Tests:**
  - Golden-file updates (`tests/fixtures/handwritten*.mmd` gain a dotted edge, which becomes a connector).
  - Round-trips for each style on a shape, an edge and a free line.
  - Custom values preserved byte-for-byte.
  - `linkStyle` numbering unchanged when dotted edges stop being kept statements.
  - A `-.->` edge with its own `linkStyle` dasharray.
  - Generated round-trip cases with random dashes.

### Phase 2: Drawing (~1½ days)

- `geometry::outline_path` for all 15 kinds, with unit tests (closed, on the rectangle's edge, extra paths for the database, subroutine and double circle).
- Canvas: dashed/dotted outlines and connectors, scaled with zoom; `Other` drawn solid; the empty Text box's dotted aid.
- SVG export with `stroke-dasharray` / `stroke-linecap`, and a test like `the_mirrored_shapes_export_as_themselves`. Rebless `tests/fixtures/export.svg` only if solid output changes (it shouldn't).
- The Change Shape / Change Line icons are unaffected.

### Phase 3: Commands, menu and pane (~1 day)

- `Act::StrokeStyle(Option<Dash>)` (Custom is never picked), `MenuAction::StrokeStyle(…)`, and `PropertiesAction::StrokeStyle(…)`, all applied through `Canvas::style_selection(d, true, |s| s.set_dash(…))`.
- `SelectionStyle` gains `dash: DashCommon`.
- The palette entries, the **Stroke Style ▸** submenu, and the segmented row plus its sample painter (shared by the menu and pane).
- `DEFAULT_HEIGHT` adjusted after measuring.

### Phase 4: UI tests and docs (~½ day)

- **`ui_tests_stroke_styles.rs`:**
  - A shape made dashed from the pane: `style a stroke-dasharray:6 4` is saved, and one undo step restores it.
  - A Mermaid edge made dotted from the menu: written as `-.->`.
  - A free line made dashed: `dash=dashed`.
  - A mixed selection shows no button pressed; one pick sets all.
  - A hand-written Custom value shows "Custom" and survives until replaced.
  - Reset Style returns to Solid.
  - Palette commands.
  - A kept-statement shape is skipped.
- **Docs:**
  - `apps/blockliner/README.md`: a Stroke styles bullet and the File format notes (`stroke-dasharray`, `-.->`, `dash=`).
  - `apps/blockliner/native/README.md`: the menu row and the `properties` module line.
  - The format comment in `blockliner-core/src/mermaid/mod.rs`.

### Phase 5: Version bump (~¼ hour)

- Bump BlockLiner only, **2.4.0 → 2.5.0** (new feature), by hand as last time:
  - `apps/blockliner/native/Cargo.toml`
  - a dated `<release>` in `packaging/linux/com.blockliner.app.metainfo.xml`
  - `Cargo.lock`
- Re-run the BlockLiner tests.

**Total: about 4 days.**

## Out of scope (possible follow-ups)

- **Stroke width** (thin / normal / thick, Mermaid's `==>` thick links). It would be a natural next row in the pane and uses the same machinery.
- **Custom dash patterns** typed in by the user.
- **Arrowheads on both ends** (`<-.->`) and other link heads (`--o`, `--x`).
- **Dash styles for subgraph borders**, which BlockLiner doesn't draw yet.

## Open questions

1. **Dotted edges as `-.->`.** Writing dotted edges with Mermaid's own syntax looks right in every renderer and makes hand-written dotted edges editable, but it means edges store "dotted" differently from shapes and free lines. The alternative is always using `stroke-dasharray`: one mechanism everywhere, but hand-written `-.->` edges stay uneditable, and a renderer that ignores `linkStyle` would show a solid edge. The plan recommends `-.->`; agree?
2. **Should Reset Style reset the dash?** The plan says yes. The alternative keeps Reset Style colour-only, as its name and today's behaviour suggest.
3. **The dash lengths.** Is `6 4` / `2 3` the look you want? Mermaid's own dotted link is `3`, and Visio-style tools often use a longer dash, around `8 4`.
4. **The control.** Is a segmented control right for the pane, or would a dropdown matching Change Shape / Change Line be preferred for consistency?
5. **The version bump.** Should this plan end with the BlockLiner-only bump to 2.5.0, as the last one did?

## Decisions and changes during implementation

- **Answers to the open questions:**
  1. Dotted edges are written as Mermaid's `-.->` / `-.-`.
  2. Reset Style sets the stroke back to solid.
  3. The dash lengths are `6 4` and `2 3`.
  4. The pane uses a **drop-down** (like Change Shape / Change Line), not a segmented control. It is labelled "Stroke Style".
  5. BlockLiner alone is bumped to 2.5.0.
- **No separate `dash` field.** `Style::dash()` and `Style::set_dash()` read and write the `stroke-dasharray` entry in `Style.extra`. This keeps a hand-written line's property order with no extra bookkeeping.
- **A dotted edge with its own `linkStyle`.** A `-.->` edge whose `linkStyle` has a dasharray takes the `linkStyle` value. A dotted edge is marked Dotted only after its `linkStyle` is merged, so the dash ends up last among its properties, where the writer leaves it.
- **Dots in the SVG export.** Dots are drawn as on the canvas: `stroke-dasharray="1.5 3.5"`, a stroke-wide dot every 5 px, not `2 3` with round caps (which reads as short dashes). The file still saves `2 3`.
- **Custom dashes that can't be drawn.** A Custom value whose numbers don't parse is drawn solid and left out of the SVG. It is still kept in the `.mmd`.
- **Dashes at the ends of a line.** Dashes start at the line's start. The plan's "restart at each end" was not needed.
- **Dashed outlines that already existed.** `ShapeKind::Other` is now drawn solid, and an empty Text box's finding aid is dotted. `paint::dashed_rect` remains only for the box being dragged out.
- **Pane height.** `properties::DEFAULT_HEIGHT` is 560. A UI test clicks Reset Style with every row showing.
