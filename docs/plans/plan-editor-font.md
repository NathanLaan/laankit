# Editor Font Choice — Plan

Status: implemented (2026-11-22) except the System monospace font of Phase 5 and the screenshots of the editor in each font. See "Changes during implementation". Adds a setting to NoteLiner for the font of the Markdown editor, offering a few bundled fonts, both proportional and monospace, with a text size and a line height. A second setting, in NoteLiner and in BlockLiner, sets the font of diagram labels.

## Today

- **NoteLiner has no font setting.** The editor (`crates/laankit-editor`) hard-codes its fonts, at a fixed 14 px:
  - The highlighter (`highlight.rs`, `EditorColors::format`) uses `FontFamily::Proportional` (Inter) for text, the `laankit_egui::fonts` bold/italic families for emphasis and headings, and `FontFamily::Monospace` (JetBrains Mono NL) for code spans and fences.
  - The editor core (`editor_core/view.rs`) measures the row height with `FontId::proportional`. It estimates line widths and the gutter with fixed factors (`0.52 × size` per character, `0.62 × size` per digit).
  - The line-cache key (`Look`) holds the font size but no font.
- **Bundled fonts** (`crates/laankit-egui/assets/fonts`):
  - Inter (Regular, SemiBold, Bold, Italic, Bold Italic), with `calt` retagged so `->` isn't drawn as an arrow.
  - JetBrains Mono NL (Regular, Bold, Italic; no ligatures).
  - Source Serif 4 (Regular, Italic, Bold, Bold Italic).
  - Font Awesome.
  - New Computer Modern Math (in `noteliner-export`).
- **Preferences:** NoteLiner keeps them in `ui-preferences.json`, and writes merge a patch into the existing file, so a new key is safe.

## Proposal

### The fonts offered

Six choices, three of each kind. Each needs Regular, Bold, Italic and Bold Italic faces and an OFL-style licence, and must have no programming ligatures (or have them removed as for Inter, because egui can't turn OpenType features off).

| Choice | Kind | Status | Why |
|---|---|---|---|
| **Inter** (default) | Proportional | Bundled | Today's look, the UI font |
| **Source Serif 4** | Proportional (serif) | Bundled (for books) | Long-form writing |
| **iA Writer Quattro** | Proportional | New (OFL) | Designed for writing Markdown |
| **JetBrains Mono** | Monospace | Bundled (NL build) | Code-friendly, already used for code spans |
| **IBM Plex Mono** | Monospace | New (OFL) | Neutral, very readable, no ligatures |
| **iA Writer Duo** | Duospace | New (OFL) | Monospace feel with wider M/W; popular for Markdown |

- **Size:** the three new families add an estimated 1.5–2 MB, to be measured. Fonts are compiled into the binary, but only the selected one is registered with egui, so unused ones cost no memory or glyph-atlas space.
- **Optional later (Phase 5):** a **"System monospace"** entry, resolved through the system font lookup eframe already uses (fontique: fontconfig, DirectWrite, CoreText). It's left out of the first version because system fonts may have ligatures and missing styles we can't control.

### How it behaves

- **The setting applies only to the editor** pane, not the preview, slides, PDF export, diagrams or the rest of the UI. Diagrams get their own setting in Phase 4. It's global (all projects), like the other editor preferences.
- **Code:**
  - With a monospace choice, code spans and fences use that same font, so everything lines up.
  - With a proportional choice, code stays in JetBrains Mono, as today.
- **Headings and emphasis** use the chosen family's bold and italic faces. Missing glyphs fall back to Inter, then the system fonts.
- **Metrics:** with a monospace font, Markdown tables and indented lists line up visually. The editor measures the font's real character width and line height instead of using the fixed 0.52/0.62 factors.
- **Switching** takes effect immediately, with no restart.
  - Every line is re-laid out once.
  - Caret, selection, scroll position (anchored to the top visible line), folds and undo are kept.

### Settings UI

- In NoteLiner's Settings, a new **Editor** group on the UI tab, with an **Editor font** list. Each row is drawn in its own font with a sample line (e.g. ``# Heading — `code` -> **bold**``), grouped as "Proportional" and "Monospace", like the theme list that draws each row in its own colours.
- Palette commands: "Editor Font: Inter", "Editor Font: IBM Plex Mono", …
- Stored as `editorFont: "<id>"` in `ui-preferences.json` (ids: `inter`, `source-serif`, `ia-quattro`, `jetbrains-mono`, `plex-mono`, `ia-duo`). An unknown or missing id means Inter.

## Phases

### Phase 1: Font registry in `laankit-egui` (~2 days)

- **`fonts::EditorFont`:** a list of the choices, each with an id, display name, kind (proportional/monospace) and four faces (bytes). `EditorFont::from_id`, `EditorFont::ALL`.
- **`fonts::install_editor_font(ctx, font)`:** registers the families `editor`, `editor-bold`, `editor-italic` and `editor-bold-italic` (falling back to Inter → icons → system), and `editor-mono[-bold|-italic]` (the font itself when monospace, otherwise JetBrains Mono).
  - It rebuilds the font definitions only when the choice changes.
  - Only the selected font's bytes are registered.
- **Bundling:** add IBM Plex Mono, iA Writer Quattro and iA Writer Duo (static TTFs from their official releases) with their licence files, `NOTICE` entries and README rows.
  - Inspect each face's `GSUB` for `calt`/`liga` arrows and ligatures. Retag them as done for Inter if present, and say so in `NOTICE`.
- **Tests:**
  - Every choice has all four faces and covers Latin plus common punctuation.
  - No `calt`/`liga` remains where it was removed (like the Inter test).
  - Installing a choice registers the `editor*` families.
  - Switching back and forth leaves no stale families.

### Phase 2: Editor support in `laankit-editor` (~3–4 days)

- **Preferences:** `EditorPrefs` gains `font: EditorFont`.
- **Highlighter:** `EditorColors::format` and `highlight.rs` use the `editor*` families for text and emphasis and `editor-mono*` for code. `Look` (the line-cache key) includes the font id, so a change re-lays out every line exactly once.
- **Editor core:**
  - Row height comes from the `editor` family.
  - Width estimates and the gutter's digit width come from the font's measured advance of `0` and an average glyph width, cached per font and size, instead of the fixed factors.
  - The find bar and fold placeholder are unaffected; the gutter stays in the monospace family.
  - Keep scroll anchoring stable across a font change (re-anchor to the first visible line).
- **Both engines:** the TextEdit fallback (`LAANKIT_EDITOR_ENGINE=textedit`) also uses the chosen families.
- **Tests:**
  - The highlighter uses the chosen families.
  - Changing the font invalidates the cache and keeps the caret and the top visible line.
  - Gutter width follows the font.
  - A kittest switching fonts while typing.
- **Benchmark:** typing p95 at 5k and 50k lines for Inter, IBM Plex Mono and Source Serif 4. Gate: within 10% of today.

### Phase 3: NoteLiner setting (~2 days)

- **Saving:** an `editorFont` pref in `config.rs` (read, default Inter, `set_prefs` patch), applied at startup and on change.
- **Settings:** the Editor group and font list (`laankit_egui::settings` style, each row in its own font), plus the palette commands.
- **Tests:**
  - The pref round-trips, the other keys in the file are kept, and unknown ids fall back to Inter.
  - A Settings kittest: choosing a font updates the pref file and the editor's families.
  - Offscreen screenshots (egui_kittest + wgpu, no real windows) of the editor in each font in a dark and a light theme.
- **Docs:** the NoteLiner and `laankit-editor` READMEs.

### Phase 4: Diagram font (~3 days)

A second setting, Diagram font, for the labels in BlockLiner diagrams, in NoteLiner and in the BlockLiner app. It is global: it applies to every diagram you view, in every project, and it is not saved in the `.mmd` file. A diagram opened on another computer shows in that computer's font.

- Today: the canvas (`crates/blockliner-egui/src/canvas/paint.rs`, `face`, `label_font`, and `edge_label_font`) draws shape and line labels in Inter, with Inter's bold and italic faces for styled labels. The SVG export names one family (`FONT` in `export.rs`), and the PNG export loads the bundled faces and sets Inter as the sans-serif family.
- Registry: `fonts::install_diagram_font(ctx, font)` registers the families `diagram`, `diagram-bold`, `diagram-italic`, and `diagram-bold-italic` from the same `EditorFont` list as Phase 1. The diagram font and the editor font are chosen separately, so both can be registered at once.
- Canvas: `face`, `label_font`, and `edge_label_font` use the `diagram*` families. So does the text box you type a label into, so the text does not change when you finish editing. The dimension labels, the limitation badge, the Objects pane, and the Properties panel stay in the UI font.
- Fonts offered: only the fonts that are about as wide as Inter, so a label that fits its shape in Inter still fits. A shape keeps the size saved in the file, and the plan does not resize shapes, because that would change files you only looked at. A test measures each offered font against Inter. The monospace fonts are much wider than Inter, so they are not offered.
- Exports: PNG and SVG use the diagram font, so an export matches the canvas. The PNG export sets the chosen family as the default in its font database. The SVG export names the chosen family, then a generic family (`sans-serif`, `serif`, or `monospace`) for computers that do not have the font. If the diagrams in presentations plan is built, a diagram on a slide is drawn through the same export, so it follows the setting too.
- Setting: a Diagram font list on the Diagrams tab of NoteLiner's Settings, drawn like the Editor font list, and stored as `diagramFont: "<id>"` in `ui-preferences.json`. BlockLiner gets the same list in its Settings, saved with its other preferences. Each app keeps its own choice. An unknown or missing id, or the id of a font that is not offered for diagrams, means Inter. Palette commands: "Diagram Font: Inter", and so on.
- Switching: takes effect immediately in every open diagram tab, with no restart. Selection, zoom, scroll, and undo are kept.
- Tests:
  - The canvas label fonts use the `diagram*` families, and the UI text on the canvas does not.
  - Changing the setting changes the font of two open diagram tabs.
  - The pref round-trips, and an unknown id falls back to Inter.
  - Opening and closing a diagram with a non-default font leaves the `.mmd` file unchanged.
  - The SVG export names the chosen family, and the PNG export differs between two fonts.
- Docs: the NoteLiner, BlockLiner, and `blockliner-egui` READMEs.

### Phase 5: Optional extras (~2–3 days, if wanted)

- **"System monospace"**, resolved through fontique. If the system font lacks a style, fall back to the bundled ones, and warn that ligatures may show.
- **Editor font size** (12–20 px) and **line height** (1.2–1.8) settings, since the size is fixed at 14 px today. Both reuse the same re-layout path.

## Rough total

About 1.5 weeks for Phases 1–3, about 3 days for Phase 4, plus 2–3 days if Phase 5 is wanted.

## Open questions

1. **The font list:** are these six right, or should any be swapped (e.g. Source Code Pro, Fira Mono or Cascadia Mono for a monospace; Literata for a serif)? Three new families add about 1.5–2 MB.
2. **Code in proportional mode:** should it stay in JetBrains Mono, or follow a separate "code font" choice?
3. **Phase 5:** do you want the system-monospace option, and font size and line-height settings, in this round?
4. **Scope:** should the preview pane also follow the editor font (optional checkbox), or stay as it is?

## Decisions

| Question | Answer |
|---|---|
| Should PNG and SVG exports follow the diagram font? | Yes. An export matches the canvas |
| Should the BlockLiner app get the Diagram font setting? | Yes, in the same round |
| May a wider font make a label overflow its shape? | No. The diagram font list is limited to fonts about as wide as Inter |

## Changes during implementation

- The font list is the six in the plan, code in a proportional font stays in JetBrains Mono, and the preview is unchanged. These were open questions 1, 2, and 4, and the plan's own proposals were used.
- The three added families are 1.6 MB, and JetBrains Mono's bold italic face was added for 0.2 MB, so every choice has four faces. None of the added faces has a ligature feature, so none was modified.
- `EditorPrefs` did not gain a `font`. egui's font families belong to the context, so the choice is the context's: an app calls `fonts::install_editor_font`, and the editor reads the fonts in use. The line cache's key holds the font's name and its measured sizes. A font that was just chosen is not in use until the next frame, and the measured sizes are what show which font a line was laid out in.
- The gutter's line numbers are in the editor's code font: the chosen font when it is monospace, otherwise JetBrains Mono.
- Settings shows the fonts in a select, like the other lists in Settings, with each name drawn in its own font and a heading for each kind. The rows have no sample line. The fonts that are not chosen are registered only while the list is open.
- A font change exposed a slow scroll drift in the editor: when lines above the view were measured a few at a time and each frame's shift was under half a pixel, the shifts were dropped. They now add up until they are corrected.
- Diagram fonts: the test measures each font against Inter in regular, bold, and italic. Source Serif 4 is 7% narrower than Inter and is offered. iA Writer Quattro is 11% wider and the monospace fonts are over 20% wider, so they are not. Source Serif 4's rows are taller than Inter's (19.2 px against 16.9 px at 14 px), so a label of several lines is a little taller. Shapes are not resized.
- BlockLiner's Diagram font is in its Settings, UI tab, and its commands are in the View section of the palette.
- Phase 5: the editor text size (12 to 20 px) and line height (the font's own, 1.2, 1.4, 1.6, or 1.8) are done, as `editorFontSize` and `editorLineHeight`. System monospace is not done.
- Benchmark (`editor_bench --font`, typing in the middle of a note, p95 in ms, one run each): before the change 2.96 at 5,000 lines and 6.08 at 50,000. After it, Inter 2.33 and 5.02, IBM Plex Mono 1.16 and 4.64, and Source Serif 4 2.01 and 4.70. All are inside the 10% gate. The runs vary by more than the fonts differ.
- Not done: the offscreen screenshots of the editor in each font. The editor was checked by eye in IBM Plex Mono in the running app.
- Not checked in the running app: the Settings lists and BlockLiner. They are covered by UI tests.
