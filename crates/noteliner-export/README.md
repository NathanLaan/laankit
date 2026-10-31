# noteliner-export

NoteLiner's PDF export without a webview, for the native (egui) build (plan
Option C, Phase 4, step 9). Promoted from the Phase 0 export spike;
`docs/plans/spikes/export.md` has the original comparison with Electron's `printToPDF`.

Frontmatter is stripped, comrak parses the Markdown (the same GFM options as
`noteliner_core::export`, plus footnotes and `[[Real Name|Label]]`
wikilinks), `markup` turns the AST into Typst markup, and Typst compiles it
to PDF in-process. No system binaries, packages or network are needed.

## API

```rust
use noteliner_export::{ExportOptions, LazyExporter, Title};

// At startup: built and warmed up on a background thread (~200 ms of
// one-time Typst setup, off the UI thread).
let faces = laankit_egui::fonts::bundled_faces().iter().map(|(_, bytes)| *bytes);
let exporter = LazyExporter::spawn(faces);

// Per export (blocks only if the exporter isn't ready yet; run it off the
// UI thread anyway, since a long note takes ~100 ms):
let pdf: Vec<u8> = exporter.get().export(&raw_note, &ExportOptions {
    title: Title::Text(note_name),   // or Title::Frontmatter { fallback }, or Title::None
    base_dir: Some(project_dir),     // where `_attachments/` lives
    ..Default::default()             // Letter, 0.5 in margins, Inter + JetBrains Mono
})?;
```

| Item                         | What it does                                                                     |
|------------------------------|----------------------------------------------------------------------------------|
| `Exporter::new(faces)`       | Fonts and Typst's library, kept warm across exports. `Send + Sync`               |
| `Exporter::export(raw, &opts)` | The raw note text (frontmatter and all) → PDF bytes                            |
| `Exporter::with_system_fallback(false)` | Never index system fonts (missing glyphs become tofu)                 |
| `Exporter::warm_up()`        | Compiles a tiny note so the first real export is fast                            |
| `LazyExporter::spawn(faces)` | `Exporter::new` + `warm_up` on a background thread; `get()` / `try_get()`       |
| `export_pdf(raw, &opts)`     | One-off export with a fresh exporter (fonts from `opts.faces`)                   |
| `export_html(name, raw)`     | The HTML export, unchanged: `noteliner_core::export::html_document`              |
| `typst_source(raw, &opts)`   | The generated Typst document, for debugging or saving a `.typ`                   |
| `ExportOptions`              | `title`, `base_dir`, `page` (`Paper::{Letter, A4, Legal}`, `margin_in`), `fonts` (family names), `wikilinks`, `faces` |
| `WikiLinks::{Text, Url(template)}` | Link-styled text (default), or real links: `{name}` in the template becomes the percent-encoded note name |
| `Error`                      | `PdfUnavailable` (built without `pdf`), `Compile`, `Pdf`                         |

`cargo run --profile fast-release -p noteliner-export --example export -- [--repeat N] [--typ OUT.typ] note.md out.pdf`
exports a note the way the app does and prints timings.

## Fonts

- **Bundled.** The caller passes the font files the app bundles,
  `laankit_egui::fonts::bundled_faces()` (added for this crate). Body text is
  Inter and code is JetBrains Mono, matching the app. The crate doesn't
  depend on `laankit-egui` itself, so a helper binary could export without
  linking egui. Typst's own embedded fonts (`typst-assets`) aren't used,
  which saves several MB.
- **System fallback.** Typst falls back glyph by glyph to any font it has.
  When a note contains characters the bundled faces lack (CJK, emoji,
  Arabic, Hebrew, …), the exporter indexes the system's fonts once through
  `typst-kit` (fontdb: fontconfig on Linux, the standard font folders on
  Windows and macOS) and reuses that set. Font files are read only when a
  glyph is needed. Notes in Latin, Greek or Cyrillic never touch the system
  fonts. Colour emoji (Noto Color Emoji) render in colour.

## Feature flag

`pdf` (default) pulls in Typst. Without it (`default-features = false`) the
crate still builds, `markup` and `typst_source` still work, and exports
return `Error::PdfUnavailable` ("PDF export isn't included in this build").

| Build (release: LTO, stripped)          | Size     | xz       |
|-----------------------------------------|----------|----------|
| `export` example, `--no-default-features` | 1.3 MB | 0.4 MB   |
| `export` example, with `pdf`            | 39.9 MB  | 11.8 MB  |
| **Delta**                               | **+38.6 MB** | **+11.4 MB** |

About 4.2 MB of the delta is `laankit-egui`'s bundled fonts, which the app
links anyway, so Typst adds roughly 34 MB (about 10 MB compressed) to the
app. The spike measured +43.4 MB because it also embedded Typst's own
fonts. A clean release build of the crate takes about 12 minutes on a busy
16-core machine; incremental dev rebuilds take seconds.

## Numbers

`--profile fast-release`, Linux, 16 cores, load average 16–19 (other builds
running), so treat these as upper bounds:

| Note                                         | First export (cold) | Re-export (median of 5) | PDF     |
|----------------------------------------------|---------------------|-------------------------|---------|
| `tests/fixtures/Rich Note.md` (112 lines, 3 images) | 262–276 ms   | 5–10 ms                 | 152 KB  |
| Pathological (1,826 lines, 43 pages)         | 800 ms              | 97 ms                   | 1.06 MB |
| CJK, emoji, Arabic, Hebrew (system fallback) | 93–102 ms           | 4 ms                    | 45 KB   |

Building an `Exporter` (parsing the bundled fonts) takes about 5 ms. The
cold numbers are what `LazyExporter`'s warm-up pays in the background.

## Tests

`cargo test -p noteliner-export` (and `--no-default-features`):

- **Escaping** (from the spike): every tricky token alone and in pairs,
  20,000 random sequences, and all printable ASCII pairs. Each is re-parsed
  with `typst-syntax`, must contain only plain text, and must round-trip.
  String literals round-trip too.
- **Compile fuzz:** 150 random Markdown documents per run, built from block
  markers, inline syntax, inline HTML tags, CJK and characters special to
  Typst.
- **End to end:** the rich fixture and an every-feature note are exported
  with the bundled fonts, then opened and rendered by `hayro` (PageLiner's
  PDF renderer): page count, no blank pages, Inter and JetBrains Mono
  embedded.
- **Other cases:**
  - Titles from options and frontmatter, and frontmatter never leaking.
  - Page setup and font families.
  - Wikilinks as text and as URLs.
  - Missing images becoming placeholders.
  - 40-level nesting and a 600-character word.
  - CJK and emoji pulling in a system font (when the machine has one).
  - `LazyExporter`.
  - The error without `pdf`.

## Markdown coverage

Headings (nested under the title when there is one), emphasis, strong,
strikethrough, inline code, links, autolinks, hard breaks, bullet, ordered
and task lists, block quotes, fenced and indented code (syntax-highlighted),
tables (alignment, header repeated on every page), footnotes, thematic
breaks, description lists, wikilinks, and images from the project folder
(wide ones scaled to the page; missing or remote ones become a grey
`[image: alt]` placeholder).

- **Inline HTML pairs map to styles:**

  | Tag                                  | Style                    |
  |--------------------------------------|--------------------------|
  | `<sup>`, `<sub>`                     | superscript, subscript   |
  | `<u>`, `<ins>`                       | underline                |
  | `<mark>`                             | highlight                |
  | `<kbd>`                              | a key cap                |
  | `<b>`, `<strong>`                    | strong                   |
  | `<i>`, `<em>`, `<cite>`, `<var>`     | emphasis                 |
  | `<s>`, `<del>`, `<strike>`           | strikethrough            |
  | `<small>`                            | smaller text             |

  `<br>` becomes a line break. Other tags, and tags without a partner, are
  dropped and their text kept.
- **HTML blocks** keep their text.
- **Frontmatter** is always stripped. `Title::Frontmatter` uses its `title`
  (or NoteLiner's `name` mirror field) as the heading.

## Math

Notes write math in LaTeX syntax (`$…$`, `$$…$$`, ```` ```math ````).
`math::latex_to_typst` converts a documented subset to Typst math (the
table is in `src/math.rs`: scripts, fractions, roots, Greek, relations
and operators, big operators with limits, `\left…\right`, matrices,
`cases`/`aligned`, `\text`/`\mathbb`/`\mathcal`…, accents, spacing) and
refuses the rest (`\color`, `\boxed`, `array`, macros, comments), so
callers show the source instead of something wrong.

- **PDF:** formulas are emitted as native Typst math, set in New Computer
  Modern Math (bundled: `assets/fonts/NewCMMath-Regular.otf`, GUST Font
  License, ~1.3 MB). Unsupported formulas print as their source.
- **`Exporter::math_svg(latex, display, size)`:** one formula as SVG,
  white on transparent, on a page that fits it (`top-edge`/`bottom-edge:
  "bounds"`, 3 pt above and below so radical bars aren't clipped). The
  NoteLiner preview and slides draw it tinted (via
  `laankit_richtext::math`). Typst's straight strokes (radical bars) are
  rewritten as rectangles (`math::straight_strokes_as_rects`), because
  rasterizers skip zero-height paths.
- **Tests** (`tests/math.rs`): every symbol in the table and ~30
  constructs compile in Typst; 200 generated formulas always compile;
  unsupported input is refused; a PDF embeds the math font and contains
  the native equations. Unit tests cover the conversion and a 20,000-case
  no-panic fuzz.

## Not yet

- **Remote images:** `https://` URLs become placeholders; nothing is
  downloaded.
- **Inline HTML styling:** attributes (`style=`, `class=`) are ignored, and
  `<a href>` keeps only its text.
- **Mermaid and other diagram fences** are set as code.
- **No custom stylesheet or header/footer options** (page numbers, date).
- **HTML export** still prints wikilinks and footnotes literally (an
  existing gap in `noteliner_core::export`).
