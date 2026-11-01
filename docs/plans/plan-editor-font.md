# Editor Font Choice — Plan

Status: **Draft for review (2026-10-19).** Adds a setting to NoteLiner for the font of the Markdown editor, offering a few bundled fonts, both proportional and monospace.

## Today

- **The Svelte build had no font setting.** CodeMirror used the system UI font at a fixed 14 px.
- **The native editor (`crates/laankit-editor`) hard-codes its fonts:**
  - The highlighter (`highlight.rs`, `EditorColors::format`) uses `FontFamily::Proportional` (Inter) for text, the `laankit_egui::fonts` bold/italic families for emphasis and headings, and `FontFamily::Monospace` (JetBrains Mono NL) for code spans and fences.
  - The editor core (`editor_core/view.rs`) measures the row height with `FontId::proportional`. It estimates line widths and the gutter with fixed factors (`0.52 × size` per character, `0.62 × size` per digit).
  - The line-cache key (`Look`) holds the font size but no font.
- **Bundled fonts** (`crates/laankit-egui/assets/fonts`):
  - Inter (Regular, SemiBold, Bold, Italic, Bold Italic), with `calt` retagged so `->` isn't drawn as an arrow.
  - JetBrains Mono NL (Regular, Bold, Italic; no ligatures).
  - Source Serif 4 (Regular, Italic, Bold, Bold Italic).
  - Font Awesome.
  - New Computer Modern Math (in `noteliner-export`).
- **Preferences:** NoteLiner keeps them in `ui-preferences.json`, and writes merge a patch into the existing file, so a new key is safe. The Electron and Tauri builds ignore keys they don't know.

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
- **Optional later (Phase 4):** a **"System monospace"** entry, resolved through the system font lookup eframe already uses (fontique: fontconfig, DirectWrite, CoreText). It's left out of the first version because system fonts may have ligatures and missing styles we can't control.

### How it behaves

- **The setting applies only to the editor** pane, not the preview, slides, PDF export or the rest of the UI. It's global (all projects), like the other editor preferences.
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
  - The pref round-trips with the Tauri file shape kept, and unknown ids fall back to Inter.
  - A Settings kittest: choosing a font updates the pref file and the editor's families.
  - Offscreen screenshots (egui_kittest + wgpu, no real windows) of the editor in each font in a dark and a light theme.
- **Docs:** the NoteLiner and `laankit-editor` READMEs and the parity table.

### Phase 4: Optional extras (~2–3 days, if wanted)

- **"System monospace"**, resolved through fontique. If the system font lacks a style, fall back to the bundled ones, and warn that ligatures may show.
- **Editor font size** (12–20 px) and **line height** (1.2–1.8) settings, since the size is fixed at 14 px today. Both reuse the same re-layout path.

## Rough total

About 1.5 weeks for Phases 1–3, plus 2–3 days if Phase 4 is wanted.

## Open questions

1. **The font list:** are these six right, or should any be swapped (e.g. Source Code Pro, Fira Mono or Cascadia Mono for a monospace; Literata for a serif)? Three new families add about 1.5–2 MB.
2. **Code in proportional mode:** should it stay in JetBrains Mono, or follow a separate "code font" choice?
3. **Phase 4:** do you want the system-monospace option, and font size and line-height settings, in this round?
4. **Scope:** should the preview pane also follow the editor font (optional checkbox), or stay as it is?
