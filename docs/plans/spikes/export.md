# Spike 5 — Export (Markdown → PDF without a webview)

> **Archived Phase 0 record.** The `apps/spikes/*` crates this describes were removed on
> 2026-10-18 after being promoted into `crates/` and the native apps; their code is in git
> history (commit `9922753`). Commands below refer to those removed crates.

**Result: PASS, with one cost to accept.** In-process Typst produces better
PDFs than the current webview print, on every platform, with no system
fonts or external tools. The cost is binary size: Typst adds about 43 MB to
the stripped release binary (about 14 MB xz-compressed).

## Method

- **Pipeline.** `frontmatter::strip_body` → comrak AST (the same GFM
  options as `export::markdown_to_html`, plus footnotes and
  `[[Real Name|Label]]` wikilinks) → Typst markup (`src/markup.rs`) →
  `typst::compile` against an in-memory `World` (`src/world.rs`) →
  `typst_pdf::pdf`.
- **Fonts.** Only fonts embedded through `typst-assets` (Libertinus Serif,
  New Computer Modern, DejaVu Sans Mono), plus any font bytes the caller
  passes in.
- **Files.** The `World` reads only image files under the project folder.
  There are no packages and no network access.
- **Escaping.**
  - Text: every ASCII character with markup meaning gets a backslash.
    Straight quotes are left alone so Typst turns them into smart quotes.
  - Code, URLs and paths: passed as Typst string literals.
  - Structure: emitted as function calls (`#list(..)`, `#enum(..)`,
    `#table(..)`, `#heading(..)`, `#quote(..)`, `#footnote[..]<fn-N>`), so
    indentation can't be misread.
- **Page setup.** Matches `export::pdf_document`: US Letter, 0.5 in margins
  plus the 24 px body padding, and the same colours for code, quote
  borders and table borders. The note name is the level-1 title, and
  Markdown headings nest under it.
- **Tests:** `cargo test -p spike-export`, 9 tests.
  - Markup-escape fuzz: all printable ASCII characters and pairs, plus
    20,000 random sequences of Typst/Markdown-special tokens. Each is
    re-parsed with `typst_syntax`, must contain only plain text, and must
    round-trip to the input.
  - String-literal round trip.
  - Random-Markdown compile fuzz: 150 documents per run. A one-off run of
    5,000 also compiled with no errors.
  - Construct and placeholder tests.
- **Fixtures.**
  - `fixtures/Rich Note.md`: every construct, plus `_attachments/` with a
    PNG, a 2400 px-wide PNG and an SVG whose name contains a space.
  - Pathological note: `spike-export --gen-pathological OUT.md`. It has
    1,826 lines (128 KB), 120 sections, a list nested 40 levels deep, a
    300 × 8 table and a 600-character URL.
- **Comparison.**
  - Old path: the same notes rendered by `export::pdf_document` (the HTML
    the Tauri build prints), printed by Electron 41 `printToPDF` with
    exactly the options in `apps/noteliner/src/main/main.js` (data: URL,
    Letter, 0.5 in margins).
  - Machine: 16 cores, Linux. All timings use `--profile fast-release`.

## Numbers

| Measure | Rich note (112 lines) | Pathological (1,826 lines, 43 pages) |
|---|---|---|
| Markdown → Typst | 0.6 ms | 15 ms |
| First export in a process (cold: library, fonts, syntax sets) | 205 ms | 660 ms |
| Re-export, same process | 3.5 ms (memoized) | 85 ms |
| PDF size | 137 KB (includes 54 KB of images) | 1.06 MB |
| Electron `printToPDF` (window load + print) | 590 ms, 77 KB, images missing | 877 ms, 240 KB |

| Build cost | Without typst | With typst | Delta |
|---|---|---|---|
| Release binary (LTO, stripped) | 1.2 MB | 44.6 MB | **+43.4 MB** |
| …xz-compressed | 0.4 MB | 14.5 MB | +14.1 MB |
| Clean dev build of the crate's dependencies | 12 s | 78 s | +66 s |
| Release (LTO) build of the crate | — | 7 min 43 s (other spikes were building at the same time) | — |
| Incremental rebuild after editing `markup.rs` | — | 3.9 s dev / 3.4 s spike | — |
| Crates in the dependency graph | — | 403 | — |

For comparison, today's Tauri NoteLiner `.deb` is 7.3 MB. The 43 MB is
about 9.6 MB of embedded fonts (including Foxit fonts for PDF images); the
rest is Typst itself: syntect/two-face syntaxes, ICU data, hyphenation,
hayagriva/CSL, the wasmi plugin runtime, krilla and hayro.

## Compared with the current output

The Typst output is better in every content respect:

- `[[wikilinks]]` render as link-styled labels. The old path prints the
  raw `[[Real Name|label]]`.
- Footnotes are real footnotes. The old path prints `[^note]` literally.
- Code blocks are syntax-highlighted.
- Table headers repeat on every page.
- Wide images scale to the page width.
- Missing or remote images get a grey `[image: alt]` placeholder instead of
  failing the export.
- Long unbreakable words wrap.

**Electron bug found.** In the Electron build, *no* images appear in PDFs.
`main.js` loads the page from a `data:` URL, and Chromium blocks the
`file://` attachment URLs from that origin. The Tauri build loads with base
`file:///`, so it isn't affected.

## Gaps and fixes

| Gap | Fix for Phase 4 |
|---|---|
| The body font is serif (Libertinus); the old export uses the system sans | Pass the sans that `marina-egui` bundles to `Fonts::new` and use `Exporter::with_family`. Checked with DejaVu Sans: works, 205 ms cold |
| No CJK glyphs, and no emoji unless an emoji font is added (the old export gets both from system fonts) | Add system fonts as a *fallback* (typst-kit `scan-fonts` / fontdb, cross-platform); bundling Noto CJK costs about 16 MB or more |
| Raw inline HTML: tags are dropped and their text kept (`<b>` loses bold); HTML blocks keep only their text | Acceptable; map a few tags (`b`, `i`, `u`, `sub`, `sup`, `kbd`) if notes use them |
| `<label>`-looking text is parsed as an HTML tag and dropped | Same as the browser path |
| Lists or quotes nested more than about 31 levels hit Typst's limit of 64 nested show rules | Fixed: levels past 24 are flattened into marked paragraphs |
| The 600-character URL ran off the page | Fixed: zero-width break opportunities in words over 60 characters |
| `pdfinfo` warns "Suspects object is wrong type" | Cosmetic; Poppler, mutool and Chromium open the files fine |
| typst 0.15 needs Rust 1.92; the workspace declares 1.85 | Raise the workspace `rust-version` when this lands |

**HTML export.** It stays on `noteliner_core::export::html_document`
(comrak), unchanged; `spike-export note.md out.html` calls it directly. Its
missing wikilink and footnote rendering is a separate, existing gap.

## Alternatives considered

- **pandoc (optional external tool).** Not viable as the default. It needs
  a PDF engine too (LaTeX, wkhtmltopdf, or Typst again), it's a separate
  install on Windows and macOS, and its output is hard to keep matching
  ours. It's still worth offering as an optional "Export via pandoc…"
  later for DOCX/ODT/EPUB if users ask.
- **printpdf / genpdf.** Small binaries, but we'd write the layout
  ourselves: line breaking, tables across pages, lists, code highlighting,
  footnotes, images and fonts. That's weeks of work for a worse result, and
  genpdf is barely maintained.
- **A hidden `wry` webview print.** It needs silent print-to-PDF code three
  times (WebKitGTK `PrintOperation`, WebView2 `PrintToPdf`, WKWebView
  `createPDF`) and brings back the webview Option C removes.

## Recommendation for Phase 4, step 9 (Export)

1. **Use Typst in-process.** Move `markup.rs` and `world.rs` into
   `noteliner-core::export` (or a `noteliner-export` crate), behind a
   `pdf-export` cargo feature.
2. **Keep one `Exporter` per app**, created lazily on a background thread
   on first use, so the 200 ms cold start never blocks a frame. Re-exports
   then take milliseconds.
3. **Fonts.** Bundle the UI sans and pass it as the body family, and add a
   system-font fallback for CJK and emoji.
4. **Decide on the size cost.** Accept about 43 MB (about 14 MB
   compressed), which is still far below Electron's 150 MB. If that's too
   much, ship export as a separate `noteliner-export` helper binary that
   the app runs as a subprocess, so the main binary stays small and the
   helper can be left out of minimal packages.
5. **Synergy with spike 3.** `typst` 0.15 already depends on `hayro` 0.7
   (PDF images). If PageLiner adopts hayro, align the versions so the code
   is shared.
6. **Fix the Electron image bug separately** (load from a temp file, or set
   a `baseURLForDataURL`), since the Electron build stays.
