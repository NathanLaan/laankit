# Spike 4 — Native EPUB reader

**Result: pass.** Three real books open in 2–18 ms and read cleanly, with
their CSS italics, centring and bold, images, internal links and the
table of contents working. Positions save and restore. **Go for the EPUB
half of Phase 3 (PageLiner).**

## Method

- **Opening a book.** The `epub` crate (2.1, pure Rust) reads the
  container, OPF, spine and NCX.
- **Table of contents.** EPUB 3 books without an NCX fall back to the
  `<nav epub:type="toc">` document, and then to the spine.
- **Chapters** are converted by `spike-richtext` (the same blocks as
  spike 2), as XHTML:
  - Self-closing non-void tags (`<a id="p12"/>`) are reopened before
    parsing.
  - Every stylesheet in the book is parsed with a small CSS subset:
    `font-style`, `font-weight`, `text-decoration`, `text-align`,
    `display: none` and monospace, matched on the last compound selector,
    plus inline `style` attributes.
  - Element ids become anchors.
  - Links and images resolve to `/`-separated, percent-decoded archive
    paths.
  - SVG cover wrappers (`<svg><image href>`) become images.
- **Images** are read from the archive into egui's byte loader as they're
  first drawn. They're capped at 85% of the window height, so covers fit.
- **Mode: scroll, with paging keys.**
  - Page Down, Space and → move by a screen minus 48 px; Page Up and ←
    move back.
  - Paging past either end of a section crosses into the next or previous
    one.
  - Real pagination was left out because egui has no column or page
    layout. Scroll mode keeps positions simple (block + offset) and costs
    only what's on screen, thanks to culling.
- **Positions** are saved as `{chapter, block, offset}` per book id in
  `$SPIKE_EPUB_DIR/positions.json`. They're written at most every 2 s
  while reading, on navigation, and on exit. The offset is a character
  offset into the top text block.
- **Testing.**
  - Automated: a synthetic EPUB 2 is built in memory in the tests. It has
    a percent-encoded path, a nested NCX, CSS, a cross-chapter link with a
    fragment, and a PNG. A headless round-trip test checks that a restored
    position reads back as the same block and line.
  - Manual: three real books from `~/Documents/books` (not committed):
    *Chokepoint Capitalism*, *The Canadian Miracle* and *Lawful
    Interception*.

```bash
cargo run --profile spike -p spike-epub -- report ~/Documents/books/*.epub
SPIKE_EPUB_DIR=<scratch> MARINA_CHAPTER=27 MARINA_SCREENSHOT=shot.png MARINA_SCREENSHOT_EXIT=1 \
  cargo run --profile spike -p spike-epub -- book.epub
```

## Numbers

Measured with `--profile spike` in a 1100×900 headless context, with an
18 px serif face, tessellation included.

| Book | Open | Sections | Blocks | Convert p50 / max | First frame p50 / max | Steady max | Jump max |
|---|---|---|---|---|---|---|---|
| Chokepoint Capitalism (715k chars) | 17.7 ms | 30 | 2175 | 0.90 / 4.63 ms | 7.1 / 41.0 ms | 0.27 ms | 2.3 ms |
| Lawful Interception | 2.1 ms | 3 | 291 | 0.37 / 1.81 ms | 0.6 / 24.2 ms | 0.14 ms | 1.6 ms |
| The Canadian Miracle | 2.7 ms | 10 | 136 | 0.16 / 0.54 ms | 0.6 / 7.0 ms | 0.12 ms | 1.4 ms |

The 41 ms worst case is the first frame of *Chokepoint*'s 570-block Notes
section, which is measured once when the section opens. Steady scrolling
stays under 0.3 ms per frame.

Screenshots were checked by eye:
- the cover
- the contents page (links)
- body text with footnote links
- the Notes section (italic titles, long URLs)
- centred italic epigraphs
- a restored mid-chapter position

## Gaps (to handle in Phase 3)

- **Slow first frame.** Large sections take 25–40 ms to open. Measure
  incrementally, or measure the next section in the background.
- **CSS.** Only the subset above is applied. Descendant and child
  combinators, pseudo-classes, `font-size`, margins and indents,
  `small-caps`, drop caps, floats and `@font-face` are ignored. Rules from
  every stylesheet apply to every chapter, so a rule can over-match.
  Real-book output still looked right, but books with heavy styling will
  look plainer than in epub.js.
- **Not rendered:** inline SVG (other than cover images), MathML,
  audio/video, fixed-layout EPUBs and right-to-left/vertical writing.
- **Selection** stays within one block (as in spike 2); highlights need a
  custom selection layer. Highlight anchors can use the same
  `{chapter, block, offset}` pairs as positions.
- **Converting existing CFI positions and highlights** wasn't attempted
  (decision 3). A best-effort mapping from the CFI's spine step plus text
  offset is possible.
- **Windows paths.** The `epub` crate joins archive paths with `PathBuf`,
  so on Windows it may produce `\` separators. The spike normalises them
  to `/` for its own lookups, but `get_resource_by_path` must be checked
  on Windows CI. The fallback is reading the zip directly.
- **Position display.** The status bar's position can show the previous
  frame's value. The saved file and the round-trip test are correct.

## Go / no-go

**Go.** A native EPUB reader over the shared rich-text blocks is fast and
faithful enough, and dropping CFI compatibility removes the only reason
for a webview. Phase 3 should share `spike-richtext`'s converter and
renderer with ThreadLiner, as a `marina-richtext` crate.
