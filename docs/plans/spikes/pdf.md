# Spike 3: PDF on hayro: results

> **Archived Phase 0 record.** The `apps/spikes/*` crates this describes were removed on 2026-10-18 after being promoted into `crates/` and the native apps; their code is in git history (commit `9922753`). Commands below refer to those removed crates.

**Verdict: pass. Go for Phase 3 with `hayro` 0.8.** pdfium isn't needed.

Date: 2026-10-17, Linux (Intel Arrow Lake iGPU, Mesa GL 4.6). Built with `--profile fast-release`. Other spikes were compiling during the runs, so treat the timings as upper bounds.

## What was built

- **`spike-pdf <file.pdf> [--state <state.json>]`.** A continuous-scroll reader:
  - two render threads (hayro, CPU) feed textures into a 20-page LRU cache
  - the closest rendered scale is shown while the exact one renders
  - zoom − / + / fit width, a page indicator and jump-to-page
  - drag-to-select text, then **Highlight**; highlights are saved to and loaded from PageLiner's `state/<id>.json` format
- **`spike-pdf --bench [--pages N] [--png-dir DIR] [--words PAGE] <pdf>…`.** The headless measurements below.
- **Tests** (`cargo test -p spike-pdf`, 8 tests):
  - glyph boxes on a hand-written PDF land in page units, also on `/Rotate 90` pages
  - drag selection gives the right text and line rects
  - highlights round-trip through `pageliner-core`'s `Library::set_state`/`get_state`
  - highlights written by the Svelte reader parse
- **Diagnostics.** `LAANKIT_SCREENSHOT` (from `spike-common`) plus `LAANKIT_PDF_SELECT=<page>:<x0>,<y0>,<x1>,<y1>`, which selects and highlights a range as if dragged, and `LAANKIT_PDF_PAGE`.

## PageLiner's format (unchanged)

- `state/<id>.json` holds `{ page, format: "pdf", bookmarks: […], highlights: [{ id, kind: "highlight", page, rects: [{x, y, w, h}], text, createdAt }] }`.
- `page` is 1-based.
- `rects` are in pdf.js's **scale-1 viewport units**: PDF points, origin at the top left of the rotated and cropped page. hayro's `page.initial_transform(true)` and `render_dimensions()` produce the same space, so stored highlights carry over with no conversion. The rotated-page test and the screenshot check confirm this.

## Numbers

| PDF | Pages | Open | First page (cold, 1x) | Render 1x p50 / p95 | 2x p50 / p95 | 3x p50 / p95 | Text p50 / p95 |
|---|---|---|---|---|---|---|---|
| Red Team Blues (calibre, CID TrueType, Letter) | 196 | 5 ms | 34–62 ms | 7.0 / 8.7 ms | 9.8 / 11 ms | 25 / 27 ms | 1.0 / 1.2 ms |
| Red Team Blues commentary (jsPDF, non-embedded base-14 fonts, images, A4) | 234 | 4–7 ms | 8–23 ms | 6–8 / 11–15 ms | 11–23 / 21–34 ms | 32 / 43 ms | 0.23 / 0.30 ms |
| Synthetic pdfLaTeX (Type 1, math, shading, transparency, rotated and cropped pages) | 3 | <1 ms | 5 ms | 1 / 4 ms | 3 / 4 ms | n/a | <2.4 ms |
| Synthetic XeLaTeX (OpenType: Greek, Cyrillic, Polish, quotes) | 1 | <1 ms | 2 ms | 1.6 ms | 3 ms | n/a | 0.6 ms |

- **Worst single page:** 51 ms at 3x. A page of body text at HiDPI fit-width (about 3x) takes about 25–45 ms on one core. With two workers, pre-rendering half a screen ahead covers normal scrolling. pdf.js in the Svelte app renders one page at a time and isn't faster.
- **Memory for 20 cached pages:**
  - Fit width at 1.5x: about 85 MB of RGBA (RSS +70 MB).
  - Fit width at 3x (HiDPI): about 335 MB (RSS +360 MB).
  - Phase 3 should budget the cache by bytes (for example 128 MB) rather than by page count. It should also free the CPU-side `ColorImage` after upload, which egui already does, so the cost is mostly GPU memory.

## Visual quality

Hayro's 2x renders were compared by eye with `pdftoppm -r 144` (Poppler):

- **Synthetic pdfLaTeX page 1** (math, shading, transparency, dashed strokes, table rules): indistinguishable.
- **Rotated page:** correct.
- **The commentary book's cover-style page** (images, coloured links, a QR code, rounded buttons): indistinguishable.
- **calibre book pages:** clean, with correct kerning and justification.

The only difference found: hayro truncates the pixmap size (`as u16`), so an A4 page at 2x is 1190×1683 rather than 1191×1684. That's harmless. Poppler renders the media box by default, while hayro, like pdf.js, uses the crop box, which is what the reader wants.

## Text and highlights

- **How it works.** A ~60-line `Device` (`src/text.rs`) receives every glyph run, including invisible OCR text, which hayro passes through for exactly this purpose. For each glyph it records the Unicode text (`as_unicode`: ToUnicode → AGL → `uniXXXX`) and a box built from the advance width and a 0.8/0.2 em ascent/descent.
- **Accuracy against `pdftotext -bbox`.** Word x-ranges match to 0.01 pt on both books. Line tops are 1.6–2.3 pt higher than Poppler's, because of the fixed ascent rather than the font's real metrics. A browser selection has similar slack, and real font metrics can tighten it if wanted.
- **Extraction speed:** 0.2–1.2 ms per page, so search across the whole book is cheap.
- **Missing text.** Pages with no text are image-only or blank, and `pdftotext` agrees (1 byte each).
- **Imperfections:**
  - LaTeX accents made from separate glyphs come out as `caf´e`. pdf.js also needs normalisation for this.
  - Type 3 glyphs get a guessed advance width.
  - Fill-then-stroke text arrives twice and is de-duplicated.
- **Selection.** Selection follows content-stream order (as pdf.js's text layer does) and merges glyphs into one rect per line. The screenshot check showed the highlight sitting exactly on the rendered text.
- **Storage round trip.** The highlight was written to `state.json` and reloaded on the next launch.

## Issues and risks for Phase 3

- **MSRV.** hayro 0.8 needs Rust **1.92**, but the workspace declares `rust-version = "1.85"`. This crate overrides it. Phase 3 needs a workspace bump (the local toolchain is 1.98).
- **Gaps hayro lists itself:** some blend, isolation and knockout group cases and colour-key masking, plus partial encrypted-PDF support. None of them showed up in these files, but expect a few odd PDFs.
- **Not `Send`.** `RenderCache` and `InterpreterCache` use `Rc`, so each render thread parses its own `Pdf` over shared bytes (parsing takes 5 ms, so this is fine).
- **No outline (table of contents) API.** The Contents pane needs about 100 lines walking `/Outlines` through hayro-syntax's dictionaries. Links and annotations would need the same.
- **Cross-platform.** hayro is pure Rust, `#![forbid(unsafe_code)]` and MIT/Apache-2.0, with no system libraries. It was only run on Linux here; the Windows and macOS CI matrix should build this crate.
- **Unrelated to PDF: screenshots on Wayland.** `LAANKIT_SCREENSHOT` runs hung with no frames on this Wayland session, but worked under X11 (`env -u WAYLAND_DISPLAY`). Worth checking in `spike-common` or with eframe.

## Recommendation

Use **hayro + hayro-interpret** for PageLiner's PDF reader in Phase 3:

- Keep the page-unit highlight format as it is.
- Budget the texture cache by bytes.
- Use 2 render workers.
- Add outline parsing, and optionally real font ascent/descent for tighter highlight boxes.

The pdfium fallback isn't needed. Revisit it only if users report PDFs that render wrong.
