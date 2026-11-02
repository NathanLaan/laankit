# BlockLiner: Stroke Weight — Plan

Status: **Implemented (2026-11-02), BlockLiner 2.6.1.** See "Changes during implementation" at the end.

## Today

- **Every stroke has the same weight.**
  - **Canvas:** `canvas/paint.rs` draws every outline and line at `stroke_width(zoom) = (1.5 × zoom).clamp(1, 4)` screen pixels.
  - **Export:** `export.rs` writes a fixed `STROKE = 1.5`.
- **`stroke-width` is kept but ignored.**
  - A hand-written `style a stroke-width:4px` is read into `Style.extra`, written back unchanged, and drawn at 1.5.
  - Reset Style keeps it on purpose: the stroke-styles test `a_custom_dash_stays_until_replaced_and_reset_style_clears_it` asserts `stroke-width:2px` survives a reset.
- **Things that already allow for a thicker stroke:**
  - **Inset outlines:** outlines are inset by half the stroke width on the canvas (`r.shrink(stroke.width / 2.0)`) and in the export (`inset = STROKE / 2`), so a thick outline stays inside the shape.
  - **Dot size:** a dot's radius is `max(width / 2, 0.75)`.
  - **Halo:** the selection halo is `width + 4`.
  - **Not yet allowed for:** the dot spacing (`DOT_PITCH = 5`), the SVG dot pattern (`1.5 3.5`) and the line hit distance (`LINE_HIT = 5`) are fixed.
- **What Mermaid draws:**
  - Mermaid honours `stroke-width` in both `style` and `linkStyle`.
  - Its arrowheads stay the same size whatever the line weight.

## Proposal

### The weights

| Row in the list | Saved as | Drawn as (at 100 %) |
|---|---|---|
| **1px** | `stroke-width:1px` | 1 px |
| **1.5px (Default)** | nothing: the property is removed | 1.5 px, as today |
| **2px** | `stroke-width:2px` | 2 px |
| **2.5px** | `stroke-width:2.5px` | 2.5 px |
| **3px** | `stroke-width:3px` | 3 px |
| **4px** | `stroke-width:4px` | 4 px |

- **There is no separate "Default" option.**
  - The default is the 1.5px row, labelled **"1.5px (Default)"**.
  - Picking it removes `stroke-width`, so the default is never written. A diagram that never touches weight saves exactly as today.
  - An unset stroke shows that row ticked, and the closed drop-down reads "1.5px (Default)".
- **Reading back:**
  - These values read as the matching rows, with or without `px`, and ignoring a trailing `.0`:
    - `1px`, `2px`, `2.5px`, `3px` and `4px`;
    - a hand-written `1.5px`, which ticks the Default row.
  - **A hand-written value stays as written** until the user picks another weight. That includes `1.5px`, and picking any row replaces it.
  - **Custom values:** any other value (`5px`, `0.75px`, `0.2em`) is Custom.
    - It is kept and written back exactly as written.
    - It is drawn at its number when that is a plain or `px` number from 0.5 to 8, and at 1.5 otherwise.
    - It shows as "Custom".
    - This is the same rule as for Custom dashes.
- **Behaviour change for existing files:** a hand-written `stroke-width` is now **drawn** where it used to be ignored. The README and release notes say so.
- **Where it is saved:** the same places as a dash.
  - **A shape:** its `style` line.
  - **A Mermaid edge:** its `linkStyle` line, merged with its colour and dash. A dotted edge written as `-.->` still carries its weight in `linkStyle`.
  - **A free line:** a ` weight=<n>` field on its `%% @line` comment, next to ` stroke=` and ` dash=`, for example `weight=2.5`.
    - A Custom value is written only when it has no spaces.
    - An older build ignores the field, so it would drop the weight if it re-saved the file. This is the same caveat as `dash=`.

### Model

- **Mirror `Dash`.** Add the following to `style.rs`:
  - `WEIGHT_KEY = "stroke-width"`.
  - `enum Weight { Preset(u8), Custom(String) }`.
    - `Preset` holds tenths of a pixel (10, 15, 20, 25, 30, 40), so the type stays `Eq` and each preset compares exactly.
    - 15 is the default. It appears only when read from a hand-written `1.5px`, because `set_weight` never writes it.
  - `pub const WEIGHTS: [u8; 6]`, the six presets in list order.
  - `Weight::parse`, `Weight::value`, `Weight::label`, and `Weight::px(&self) -> Option<f32>` for drawing.
    - `label` gives "1px", "1.5px (Default)", "2.5px", "Custom" and so on.
  - `Style::weight`, and `Style::set_weight(Option<Weight>)`, where `None` means the default.
    - The setter keeps the property's position and appends it when new, as `set_dash` does.
    - Setting the default removes the property.
- **Stored in `extra`.** As with the dash, there is no typed field, so property order and unknown values round-trip unchanged.
- **Selection summary:** `WeightCommon` / `common_weight` for the selection, plus `SelectionStyle.weight`.
  - Unset and a hand-written `1.5px` count as the same weight.
  - **Optional refactor:** `DashCommon` and the new type are the same shape, so they can become one generic `Agreed<T>`.
- **`Style::reset` clears the weight**, along with the colours and the dash.

### Drawing

- **Width on the canvas:**
  - **Shapes and lines with a weight:** drawn at `(weight × zoom).max(1.0)` screen pixels.
    - There is no upper clamp, so 4px stays visibly thicker than 3px when zoomed in.
  - **Default strokes:** keep today's `(1.5 × zoom).clamp(1, 4)` exactly, so existing diagrams look the same.
  - Either way, one helper, `paint::stroke_width(style, zoom)`, replaces the zoom-only function.
- **Shapes:** the existing inset handles thick outlines. The outline paths (`geometry::outline_path`) are already computed on the inset rectangle. Nothing else changes, including the Subroutine bars, the Database lid and the DoubleCircle ring.
- **Lines:**
  - **Arrowheads stay the size they are today** (`ARROW = 12` at 100 %), whatever the weight, as in Mermaid.
    - The line still stops at the head's base, so a thick line doesn't poke past the tip.
  - **Hit distance:** the line's reach becomes `LINE_HIT + width / 2`, so a thick line can be clicked anywhere on its ink.
- **Dashes and dots with a weight:**
  - **Dashed:** keep the saved `6 4` lengths. That matches what Mermaid draws for the same file.
  - **Dotted on the canvas:** the dot pitch becomes `max(DOT_PITCH, 2.5 × width)`, so 4px dots don't merge into a solid line.
  - **Dotted in SVG:** the pattern is scaled the same way. It is `w (pitch − w)` with square ends, instead of the fixed `1.5 3.5`. At 1.5px it equals today's pattern.
- **SVG and PNG export:**
  - `stroke-width` is written per element from the style, and `STROKE = 1.5` remains the default.
  - The inset uses the shape's own weight.
  - Arrowheads keep their size.
  - resvg renders it, so PNG export needs no extra work.
  - The golden `tests/fixtures/export.svg` should not change, because it has no weights.

### Properties pane

- **A "Stroke Weight:" drop-down** goes under Stroke Style and before Reset Style. It matches Change Shape, Change Line and Stroke Style.
  - **The list:** six rows, "1px", "1.5px (Default)", "2px", "2.5px", "3px", "4px".
    - Each row has a 20 px sample line at that thickness, drawn by `views::paint_glyph` through a new `Glyph::Weight(u8)` variant.
  - **The closed box shows:**
    - the current row's label;
    - "Mixed" when the selection disagrees;
    - "Custom" for any other value.
- **Enabled:** under the same rule as Stroke Style, which is any own shape or line selected. Kept-statement shapes are skipped and counted in the note.
- **`DEFAULT_HEIGHT`:** 560 now. It is re-measured with every row showing, so Reset Style still fits. That is about 600, checked by a test like the one in `a_mixed_selection_takes_one_style`.

### Right-click menu

- **A "Stroke Weight ▸" submenu** goes after Stroke Style ▸, with the same six rows and samples. The current weight is ticked.

### Command palette

- **Six entries:** "Stroke Weight: 1px" … "Stroke Weight: 4px", including "Stroke Weight: 1.5px (Default)".
  - Their IDs are `style.strokeWeight.1`, `.1_5`, `.2`, `.2_5`, `.3` and `.4`.
  - They are in group Style, enabled when the selection can be styled.

### Reset Style

- **Clears the weight, along with the colours and the dash**, so the stroke returns to 1.5px, solid, in the theme's colours.
- **Tooltip and docs updated:** "Removes colours, stroke style and weight."
- **Existing test changed:** `a_custom_dash_stays_until_replaced_and_reset_style_clears_it` now asserts that `stroke-width` is removed.

### Text weight and style (Bold, Italic, Ctrl+B) — released as 2.6.1

**Today:**
- Every label is drawn in Inter Regular: on the canvas, in the label editor and in the SVG/PNG export.
- A hand-written `font-weight` is kept in `Style.extra` but ignored.
- Inter Bold is already bundled. `laankit_egui::fonts::BOLD_FAMILY` gives it to egui, and `bundled_faces()` gives it to resvg, so no new font files are needed.

**The options:**

| Row in the list | Saved as | Drawn as |
|---|---|---|
| **Normal** | nothing: the property is removed | Inter Regular, as today |
| **Bold** | `font-weight:bold` | Inter Bold |

- **Normal is the default.** It is never written, so a diagram that doesn't use Bold saves exactly as today.
- **Reading back:**
  - `bold`, `bolder` and the numbers 600–900 read as Bold.
  - Everything else (`normal`, `400`, `lighter`) reads as Normal.
  - A hand-written value is kept as written until the user picks Normal or Bold.
  - There is no "Custom" here, because every value falls on one side or the other.
- **What it applies to:**
  - **Shape labels**, every kind including Text boxes. Saved on the shape's `style` line.
  - **Edge labels:**
    - **A Mermaid edge:** saved on its `linkStyle` line (`linkStyle 0 font-weight:bold`), merged with its colour, dash and stroke width.
    - **A free line:** a ` text=bold` field on its `%% @line` comment, next to ` weight=`. An older build ignores the field.
  - It is set even when a shape or line has no label yet, so the label is bold once typed.
- **Mermaid check during implementation:**
  - Confirm that Mermaid renders `font-weight` from `style` on a node's label and from `linkStyle` on an edge's label.
  - If a version ignores it for edge labels, BlockLiner still saves and draws it. The README notes the difference.

**Italic:**
- A second drop-down, **"Text Style:"**, with rows **Normal** and **Italic**. It goes under Text Weight.
  - **Saved as:** `font-style:italic` for shapes and Mermaid edges. Normal is never written.
  - **Reading back:** `italic` and `oblique` read as Italic, and everything else as Normal. A hand-written value is kept until the user picks a row.
- **Faces:** Bold and Italic combine, using the bundled Inter Italic and Inter Bold Italic faces. `laankit_egui::fonts::Style::font` picks the face.
- **Free lines:** the `text=` field takes `bold`, `italic` or `bold-italic`.
- **Command palette:** "Text Style: Normal" and "Text Style: Italic".
- **Right-click menu:** a "Text Style ▸" submenu after Text Weight ▸.
- **Reset Style:** returns the label to Normal.
- **No shortcut:** Ctrl+I already toggles the Properties pane, so Italic has no key.

**Ctrl+B (toggle Bold):**
- **Behaviour:** if every selected shape and line is Bold, Ctrl+B makes them Normal; otherwise it makes them all Bold. It is one undo step.
- **Registration:**
  - It is registered as "Toggle Bold" in the Style section, so it appears in the palette and in Settings → Keyboard Shortcuts.
  - It is enabled when the selection can be styled.
- **While editing a label:** Ctrl+B doesn't reach the canvas, because the label's text field has focus.

**Model:**
- `FONT_WEIGHT_KEY = "font-weight"` and `FONT_STYLE_KEY = "font-style"`.
- `Style::italic` / `set_italic`, alongside `bold`.
- `Style::bold() -> bool` and `Style::set_bold(bool)`, which keep the property's position as `set_dash` does.
- `SelectionStyle.bold: Agreed<bool>`, either via the generic type from the stroke-weight refactor or a small `BoldCommon`.
- `Style::reset` clears it, so Reset Style returns the label to Normal along with the colours, dash and stroke weight.

**Drawing:**
- **Canvas:** `paint::label_font(zoom)` and `edge_label_font(zoom)` take `bold: bool` and pick `FontFamily::Name(BOLD_FAMILY)` for Bold. This covers:
  - `paint::label` (the wrapped shape label);
  - `paint::edge_label`;
  - the label editor in `canvas/label.rs`, so editing a bold label shows it bold;
  - the edge-label hit boxes (`Canvas::edge_labels`), which measure with the same font so clicking a bold label still selects it.
- **Export:**
  - `<text>` gets `font-weight="bold"`.
  - The galleys used to size edge-label backgrounds and wrap shape labels are laid out in the bold face.
  - resvg picks Inter Bold from the bundled faces, so PNGs match.
- **Unchanged:** shapes don't resize. Bold text wraps within the same label area, and may wrap one word earlier.

**Properties pane:**
- **A "Text Weight:" drop-down** goes under Stroke Weight and before Reset Style.
  - Its rows are **Normal** and **Bold**, each with a sample "Aa" in that weight.
  - The closed box shows the current row, or "Mixed".
- **Enabled:** under the same rule as the other style rows.
- **`DEFAULT_HEIGHT`:** re-measured again, to about 640, with every row showing.

**Right-click menu:** a **"Text Weight ▸"** submenu after Stroke Weight ▸, with the current weight ticked.

**Command palette:** "Text Weight: Normal" and "Text Weight: Bold" (`style.textWeight.normal` / `.bold`), in group Style.

**Actions:** `Act::TextWeight(bool)`, plus `MenuAction` and `PropertiesAction` variants. All are applied through `Canvas::style_selection(d, true, |s| s.set_bold(…))`.

## Phases

Stroke weight ships first as **2.6.0** (Phases 1–5). Text weight follows as **2.6.1** (Phases 6–7). Each release is tested and bumped on its own.

### Phase 1: Stroke weight — model and file format (~½ day)

- **Model:** `Weight`, `WEIGHT_KEY`, `WEIGHTS`, `Style::weight` / `set_weight`, `common_weight`, and `reset` clearing it.
  - Unit tests are modelled on `dashes`.
  - They include `1.5px` reading as the default and `2.50px` reading as 2.5px.
- **Free lines:** the `@line … weight=` field, read and written.
- **Writer:** `linkStyle` includes `stroke-width` for every edge, dotted ones too. Dotted edges already write `props_without_dash()`, which keeps the width.
- **Tests:**
  - Round-trips for each weight on a shape, an edge, a dotted edge and a free line.
  - A Custom value that comes back byte-for-byte.
  - Random weights added to the generated round-trip cases in `tests/mermaid.rs`.

### Phase 2: Stroke weight — drawing (~1 day)

- **Canvas:** `stroke_width(style, zoom)` for shapes and connectors, the dot pitch, and the line hit reach. Arrowheads are unchanged.
- **Export:** `stroke-width` and the inset per shape and line, and the dotted dasharray scaled with the weight.
- **Tests:** an export test for a 1px and a 4px shape and a 2.5px dotted line. Rebless `export.svg` only if default output changes, which it shouldn't.

### Phase 3: Stroke weight — commands, menu and pane (~½ day)

- **Actions:** `Act::StrokeWeight(Option<Weight>)`, plus `MenuAction` and `PropertiesAction` variants.
  - All are applied through `Canvas::style_selection(d, true, |s| s.set_weight(…))`.
  - `None` is the 1.5px (Default) row.
- **UI:**
  - `SelectionStyle.weight`.
  - `Glyph::Weight`.
  - The palette entries, the submenu and the pane drop-down (`weight_row`, next to `dash_row`).
  - `DEFAULT_HEIGHT` re-measured (about 600).

### Phase 4: Stroke weight — UI tests and docs (~½ day)

- **`ui_tests_stroke_weight.rs`:**
  - **Shape from the pane:** set to 3px, which saves `style a stroke-width:3px`, and undo restores it in one step.
  - **Back to the default:** picking 1.5px (Default) removes `stroke-width`.
  - **Edges from the menu:**
    - Set to 2.5px, which saves `linkStyle 0 stroke-width:2.5px`.
    - Then dotted, which saves `-.->` and keeps the `linkStyle` width.
  - **Free line from the palette:** set to 4px, which saves `weight=4`.
  - **Mixed selection:** shows "Mixed", and one pick sets every item.
  - **Hand-written values:**
    - `stroke-width:1.5px` shows "1.5px (Default)" and is kept as written.
    - `stroke-width:5px` shows "Custom" and survives other edits.
  - **Reset Style:** clears the weight, and the assertion in `ui_tests_stroke_styles.rs` is updated.
  - **Kept-statement shapes:** skipped.
  - **Reset Style fits:** it is still clickable with every row showing at the default pane height.
- **Docs:**
  - `apps/blockliner/README.md`: a Stroke weight bullet, the `@line weight=` row, a `stroke-width` file-format row, and the Reset Style wording.
  - `apps/blockliner/native/README.md`: the menu row.
  - The format comment in `blockliner-core/src/mermaid/mod.rs`.

### Phase 5: Version bump to 2.6.0 (~¼ hour)

- Bump BlockLiner only, **2.5.0 → 2.6.0**, in:
  - `apps/blockliner/native/Cargo.toml`
  - a dated `<release>` in the metainfo
  - `Cargo.lock`
- Re-run fmt, clippy (`-D warnings`) and the BlockLiner tests.

### Phase 6: Text weight and style — Bold, Italic, Ctrl+B (~1½ days)

- **Model:**
  - `FONT_WEIGHT_KEY`, `FONT_STYLE_KEY`, `Style::bold` / `set_bold`, `Style::italic` / `set_italic`, `SelectionStyle.bold` / `.italic`, and `reset` clearing them.
  - Unit tests for each `font-weight` value that reads as Bold or Normal.
- **File format:**
  - The `@line … text=bold` field.
  - `font-weight` merged into `linkStyle`.
  - Random bold added to the generated round-trip cases.
- **Drawing:**
  - The bold face in `label_font` / `edge_label_font`, the label editor and the edge-label hit boxes.
  - `font-weight="bold"` in the SVG export.
  - Check the Mermaid rendering of node and edge labels.
- **UI:**
  - `Act::TextWeight(bool)`, `Act::TextStyle(bool)` and `Act::ToggleBold` (Ctrl+B).
  - The palette entries, the Text Weight ▸ and Text Style ▸ submenus, and the two pane rows.
  - `DEFAULT_HEIGHT` re-measured (about 680).
- **Tests:**
  - An export test with a bold shape label and edge label.
  - **`ui_tests_text_weight.rs`:**
    - **A shape label from the pane:** made Bold, which saves `style a font-weight:bold`, and one undo returns it to Normal.
    - **A Mermaid edge label from the menu:** made Bold, which saves `linkStyle 0 font-weight:bold`.
    - **A free line from the palette:** made Bold, which saves `text=bold`.
    - **Hand-written values:** `font-weight:600` shows Bold and is kept as written.
    - **Mixed selection:** shows "Mixed".
    - **Reset Style:** returns the label to Normal.
    - **Edge labels stay clickable:** clicking a bold edge label still selects it.
    - **Italic:** a label made Italic saves `font-style:italic`, and Bold together with Italic saves both. A free line saves `text=bold-italic`.
    - **Ctrl+B:** it toggles a selection to Bold and back, and a mixed selection becomes all Bold.
    - **Reset Style fits:** it is still clickable with every row showing.
- **Docs:**
  - `apps/blockliner/README.md`: a Text weight bullet, the `@line text=bold` row, a `font-weight` file-format row, and the Reset Style wording.
  - The native README's menu row.
  - The format comment in `mermaid/mod.rs`.

### Phase 7: Version bump to 2.6.1 (~¼ hour)

- Bump BlockLiner only, **2.6.0 → 2.6.1**, in:
  - `Cargo.toml`
  - a dated `<release>` in the metainfo
  - `Cargo.lock`
- Re-run fmt, clippy (`-D warnings`) and the BlockLiner tests.

**Total: about 2½ days for 2.6.0, then about 1½ days for 2.6.1.**

## Out of scope (possible follow-ups)

- **Thick links (`==>`).** These stay kept statements, drawn as a plain edge. Mermaid draws them at 3.5px, which isn't one of the six weights.
- **Free-form weights** typed in by the user.
- **Label size following the weight.**
- **Other text styling:** semibold, font size, and inline `<b>` markup inside a label. Today `<b>` is kept as literal text.
- **A shortcut for Italic.** Ctrl+I is taken by Toggle Properties.

## Decisions

1. **The weights and the default.**
   - There are six weights: 1, 1.5, 2, 2.5, 3 and 4px.
   - 1.5px is the default, labelled "1.5px (Default)".
   - There is no separate "Default" option.
   - Choosing 1.5px writes nothing.
2. **Reset Style clears the weight.**
3. **Arrowheads follow Mermaid:** they stay a fixed size at every weight.
4. **The free-line field is `weight=`.**
5. **Version:** BlockLiner-only bumps.
   - Stroke weight is 2.6.0.
   - Text weight follows as 2.6.1.
6. **Text weight:**
   - There are two options, Normal (the default, never written) and Bold (`font-weight:bold`).
   - It applies to shape and edge labels.
   - Reset Style clears it too.
7. **Italic and Ctrl+B:**
   - Italic is a "Text Style:" drop-down (Normal / Italic), saved as `font-style:italic`.
   - Ctrl+B toggles Bold.
   - Italic has no shortcut, because Ctrl+I is taken.

## Changes during implementation

- **Phases 1–6 shipped together.** Text weight and style (Phase 6) went in alongside stroke weight, so the 2.6.0 build already has Bold, Italic and Ctrl+B. Phase 7 then bumped BlockLiner to 2.6.1.
- **Shared selection type:** `style::Agreement<T>` / `agreement` is the generic selection summary. `DashCommon` is now an alias of it.
- **Pane layout:**
  - Text Weight and Text Style sit side by side in one row, to save height.
  - `DEFAULT_HEIGHT` is 680, measured with every row showing.
  - In windows shorter than about 860 px the pane is capped below that and scrolls to Reset Style. The UI tests scroll it into view with the mouse wheel (`ui_tests::pane_reset_style`).
- **Hit reach:** only a line with its own weight reaches further, by half that weight. Default-weight lines keep today's reach exactly, so nothing else moves.
- **Reset Style:** removes `stroke-width`, `font-weight` and `font-style` outright, including a hand-written `stroke-width:1.5px` or `font-weight:normal`. Picking a row that matches a hand-written value leaves the value as written.
- **Not verified:** whether Mermaid itself renders `font-weight` / `font-style` from `linkStyle` on edge labels. BlockLiner saves and draws them either way.
