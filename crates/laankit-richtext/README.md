# laankit-richtext

Sanitized HTML and XHTML reduced to native rich-text blocks and laid out with egui, with no webview. Shared by ThreadLiner (articles), PageLiner (EPUB chapters) and, later, the NoteLiner Markdown preview. Promoted from the Phase 0 spikes 2 and 4 (results in `docs/plans/spikes/article.md` and `epub.md`); see `docs/plans/plan-native-rust-egui.md`.

## API

```rust
use laankit_richtext::{from_article_html, Action, RichStyle, RichView, WebResolver};

// Once, at startup (fonts from laankit-egui; images decode on egui_extras' background thread).
laankit_egui::fonts::install(ctx);
egui_extras::install_image_loaders(ctx);

// When an entry is opened:
let doc = from_article_html(&entry.content, entry.link.as_deref());
view.reset();
view.restore(saved_position); // optional

// Every frame:
let style = RichStyle::from_ctx(ui.ctx()); // colours from the active LaanKit theme
let out = view.show(ui, &doc, &style, &mut WebResolver);
for Action::Open(url) in out.actions { /* open in the browser */ }
save(view.position());
```

| Item                                    | What it is                                                                               |
|-----------------------------------------|------------------------------------------------------------------------------------------|
| `from_article_html(html, link)`         | ThreadLiner's DOMPurify allow-list (`sanitize`), then conversion; links resolve against `link` |
| `from_xhtml(xhtml, &Options)`           | An EPUB chapter: `Options { base: Base::Path(..), stylesheet }`, internal links become archive paths |
| `from_html(html, &Options)`             | Already-safe HTML                                                                        |
| `Document`, `Block`, `Span`, `Style`    | The model: text, lists, quotes, code, images, rules, simple tables; `anchors` maps ids to blocks |
| `css::StyleSheet`                       | The EPUB CSS subset (see below)                                                          |
| `RichView`                              | Progressive layout in its own scroll area; `show`, `position`/`restore`, `restoring`, `scroll_to_anchor`, `scroll_to_offset`, `settled`, `last_frame`, `id_salt` |
| `RichView` selection and marks          | `selection(&doc) -> Option<Selection>`, `select`, `clear_selection`; `set_marks(Vec<Mark>)`, `marks` |
| `RichOutput`                            | Clicked links (`Action::Open`), scroll offset, viewport and content height (for paging), `clicked_mark`, `selection_changed` |
| `Position`                              | Block + character offset, counted through the block's text parts (`Block::text_parts`, `Block::part_text`); ordered |
| `RichStyle`                             | Size, face, column width, line height and colours; `from_ctx`/`from_palette` follow the six themes |
| `Resolver`                              | `image(ctx, src) -> Option<uri>` and `link(target) -> LinkAction` (`Open`, `Anchor`, `Ignore`) |
| `WebResolver`                           | `http(s)` images through egui_extras' HTTP loader, `data:` images decoded once           |
| `BundleResolver::new(scope, read)`      | Images read from a bundle (the EPUB zip) on first use and registered as `bytes://scope/path` |
| `fonts`                                 | The family names the view asks for, and its fallbacks                                    |

### Markdown preview content

What comrak (NoteLiner's preview, through `noteliner_core::export::markdown_to_html`) and `marked` emit, kept and drawn:

- **Task lists.** An `<input type="checkbox">` at the very start of a list item becomes a span with `Style::checkbox = Some(checked)` and the text `[ ]` or `[x]`, so copying gives task syntax and renderers that don't draw boxes (slides) still show the state. The view lays both out as the same three characters (checked and unchecked boxes line up) and draws a theme-coloured box over them: an outline (`check_border`), or a filled box (`check_fill`) with a tick (`check_mark`). Read-only, as in the Svelte preview. Any other `<input>` is dropped.
- **Table alignment.** `align` or CSS `text-align` on a `th`/`td` (CSS wins), else the column header's, as `Style::cell_align` on the cell's spans (kept off `Block::Table` so code matching on its fields still compiles). Cells are aligned within their column.
- **`<mark>`.** `Style::mark`: a yellow highlight with dark text (`mark_bg`, `mark_text`), as a browser draws it, readable in all six themes.
- **`<kbd>`.** `Style::kbd` (and `code`): monospace in a key cap, a filled, outlined frame (`code_bg`, `kbd_border`) drawn under the glyphs.

Articles (`from_article_html`) stay as strict as before: the sanitizer's allow-list (ThreadLiner's DOMPurify one) has no `input`, `mark` or `kbd`, so their text is kept unstyled.

### Selection and highlights

Dragging selects text across blocks (paragraphs, list items, quotes, code, table cells); the view scrolls while the pointer is held past its top or bottom edge. Double-click selects a word (letters, digits, `_` and inner apostrophes), triple-click a whole text part (a paragraph, list item paragraph, table cell or code block), and Shift+click extends from the selection's start, or from the last click when nothing is selected (Shift+drag keeps extending). Ctrl/Cmd+A selects the whole document when no text field has keyboard focus and the pointer is over the view (or it already has a selection), so it never takes Select All from a field or another view. Ctrl+C copies the selection (each text part on its own line) when no text field has focus, and Escape clears it. A `Selection` is `start..end` in document positions plus the text.

`Mark`s (id, start, end, colour) are painted behind the text, like the selection, by `egui::text_selection::visuals::paint_text_selection`; a click (not a drag) on one is reported as `RichOutput::clicked_mark`. PageLiner uses them for highlights.

### EPUB CSS

Honoured, because books use them to carry meaning or structure: `font-style`, `font-weight`, `text-decoration`, monospace `font-family`; `text-align` including `justify`; `text-indent` (negative is a hanging indent), `margin-left`, `padding-left` and the `margin`/`padding` shorthands (left indents accumulate through nested blocks, capped at 6 em); `margin-top` and `margin-bottom` on the paragraph's own element (not inherited; capped at 3 em), which replace the reader's paragraph gap; `font-variant: small-caps` and `text-transform: uppercase` (applied letter for letter, so positions don't move); `vertical-align: super | sub` (as `<sup>`/`<sub>`: smaller and raised or lowered); image `width` in `%` or lengths; `display: none`.

Ignored, because a reflowing reader shouldn't take them from the book: colours and backgrounds (the reading theme owns them), font sizes and line heights (the reader's setting), vertical padding, floats, positioning and `page-break-*`. Vertical margins don't collapse as in CSS: a paragraph's own top and bottom margins are used as they are. Row and column spans are padded with empty cells so columns line up.

**Justification** (`view::layout_text`): every row but a paragraph's last (and not one ending in a line break) is filled to the column by widening the gaps between words, never between letters. The row breaks come from a normal layout; a second layout puts the extra space in the `leading_space` of sections starting at those words, so the text and every position in it stay the same, and it's used only if its line breaks match (otherwise the paragraph stays ragged-right). egui's own `LayoutJob::justify` adds whole pixels between every pair of letters on sparse lines. Laying out justified text costs about twice as much as left-aligned (300 paragraphs: 148 ms against 74 ms, release build), still within the progressive layout's budget.

**Hanging indents**: a negative `text-indent` starts the first line left of the others, as far as the paragraph's left margin, and makes it as much wider, as in CSS. The galley stays column-wide; the first line draws into the margin.

### Math

`from_note_html` (NoteLiner's preview and slides; never articles or books) turns comrak's math markup into formulas: inline `$…$` becomes a span with `Style::math` whose text is the LaTeX, and a paragraph that is only `$$…$$` (or a ```` ```math ```` block) becomes an image block whose `src` is `math::image_src(latex)` (alt text: the LaTeX, so copying and search keep it).

The crate doesn't typeset: the app installs a `math::MathRenderer` (NoteLiner's uses Typst) with `math::install(ctx, renderer)`. Its loader renders on one worker thread, caches by formula, mode and text size, and returns white glyphs that the view tints with the text colour, so one texture serves every theme. An inline formula is laid out as one invisible no-break space per source character (character positions match the model, and it never wraps inside), spaced to the formula's width and tall enough for it, and the texture is painted over them. Display formulas are centred and scaled down to fit the column. Without a renderer, while one renders, or when it fails (unsupported LaTeX), the source shows as code with the same characters. Tests: `tests/math.rs` (conversion, placeholder width, fallback, display blocks).

### Inline SVG

`<svg>` in an article, an EPUB chapter or the NoteLiner preview becomes an image (module `svg`):

- **Cleaning, at conversion.** The element is re-serialized as standalone XML keeping only drawing. Dropped:
  - `<script>`, `<foreignObject>` and embedded media, with their content
  - `on*` attributes, and anything with `javascript:`
  - every `href` except same-document references (`#id`) and embedded raster images (`data:image/png|jpeg|gif|webp`)

  Over 20,000 elements or 512 KB, the picture is dropped. The image block's `src` is the result as a `data:image/svg+xml;base64,…` URI, so the model and the apps' resolvers are unchanged.
- **Articles.** ThreadLiner's sanitizer takes each `<svg>` out before ammonia's HTML allow-list (which can't express a safe SVG subset), cleans it the same way, and puts it back as an `<img>` with that `data:` source.
- **EPUB covers keep working.** An `<svg>` that only wraps an external `<image>` is a cover and still shows that image.
- **Icons.** Pictures sized 40 px or less keep only their `<title>` inline, like small `<img>`s.
- **Rendering, in the view.** `svg::InlineSvgLoader` (an egui image loader, installed on first use) rasterizes with `resvg` on a worker thread at the size the view asks for. Results are capped at 2048 px a side and 4 megapixels, cached, and reported at the SVG's own size for layout. `usvg` fetches nothing: external image references resolve to nothing. A render still running after 3 s is reported as failed. `noteliner-slides` uses the same loader.

Images are handed to egui as URIs; fetching (`ehttp`) and decoding (`egui_extras`' image loader) happen off the UI thread, and the image block takes its real height when the texture arrives.

### Fonts

The view uses what `laankit_egui::fonts::install` registers and never reads system fonts:

| Text    | Font                                                                         |
|---------|------------------------------------------------------------------------------|
| regular | `FontFamily::Proportional` (Inter)                                           |
| italic  | the `sans-italic` family (`laankit_egui::fonts::ITALIC`)                      |
| bold    | Inter Bold / Bold Italic (static faces from `laankit_egui::fonts`)            |
| code    | `FontFamily::Monospace` (JetBrains Mono)                                     |
| serif   | Source Serif 4 Regular, Italic, Bold, Bold Italic for `Face::Serif` (`laankit_egui::fonts::SERIF*`) |

Anything missing falls back to the regular face: italics slanted by egui, bold in the strong colour. The view never asks for a family that isn't registered.

## Progressive layout

Phase 0 laid out every block in a document's first frame to learn its height: 27 ms for a 208-block article, 41 ms for a 570-block EPUB section. `RichView` spreads that work out:

1. **Estimates.** When a document opens (or the width changes) every block gets an estimated height from its text length at an average glyph width, so the scroll area has a size at once.
2. **The viewport first.** Each frame lays out and paints the blocks on screen, plus blocks within `OVERSCAN` (600 px) that are already measured. An unmeasured overscan block waits for step 3, so the first frame costs one screenful.
3. **A budget for the rest.** Unmeasured off-screen blocks are laid out invisibly, top to bottom, until `DEFAULT_BUDGET` (4 ms) of that work is spent (always at least one block), and their real heights replace the estimates. The view repaints until none are left (`settled()`).
4. **Anchoring.** The view records the block at the top of the viewport and its offset. If blocks above it change height, it computes the corrected scroll offset, discards the frame (`request_discard`), and starts the next pass at that offset, so the jump is never painted. That redraw pass measures nothing new above the viewport, because there would be no pass left to correct another shift. Restoring a position and following `#anchor` links use the same path. The view owns its `ScrollArea` with `animated(false)`: an animated programmatic scroll wouldn't land within the frame.

`RichView::with_budget(Duration::MAX)` gives the old behaviour: everything is measured in the first frame.

## Numbers

`cargo run --profile fast-release -p laankit-richtext --example bench [articles.json]`, headless at 1000×800 with tessellation. Each run uses a fresh context warmed up by a different document, so glyphs are rasterized but this document's text has never been laid out (opening another article in the app). The five longest of the Phase 0 cached articles were used (not committed). Load average ≈ 7.5 (a busy desktop), so treat these as upper bounds. Re-run on 2026-10-18 after the task-list, alignment, `<mark>`/`<kbd>` and selection work at load ≈ 10.7: eager and progressive first frames both rose about 15–25%, in line with the load (2,000 blocks: 14.9 ms progressive), so the new drawing adds no measurable per-block cost.

| Document              | Blocks | Eager first frame | Progressive first frame | Frames to settle | Jump to middle |
|-----------------------|--------|-------------------|-------------------------|------------------|----------------|
| synthetic             | 200    | 37.5 ms           | 10.9 ms                 | 8                | 12.9 ms        |
| synthetic             | 600    | 95.8 ms           | 11.1 ms                 | 21               | 12.4 ms        |
| synthetic             | 2000   | 295.3 ms          | 12.1 ms                 | 68               | 14.4 ms        |
| article (longest)     | 208    | 63.8 ms           | 16.4 ms                 | 12               | 19.0 ms        |
| article               | 126    | 28.3 ms           | 14.9 ms                 | 5                | 14.9 ms        |
| article               | 96     | 64.0 ms           | 16.2 ms                 | 10               | 17.8 ms        |
| article               | 88     | 34.8 ms           | 13.7 ms                 | 6                | 19.3 ms        |
| article               | 67     | 37.6 ms           | 11.4 ms                 | 6                | 14.5 ms        |

Re-measured after adding cross-block selection, marks and the wider CSS (Phase 3): the same within half a millisecond (2,000 blocks: 12.0 ms first frame; longest article 16.1 ms).

The first frame no longer grows with the document (12 ms at 2,000 blocks, against 295 ms when measuring everything), and it's the slowest frame of each run.

What's left of the first frame is shaping the visible screenful's text for the first time. The fonts matter: with the variable Inter that `laankit-egui` first bundled, the same first frames took 15–31 ms, so it now bundles static instances (Regular, SemiBold, Bold, Italic, Bold Italic), which shape about twice as fast. egui's own small default fonts (`BENCH_EGUI_FONTS=1`) are faster still (6–8 ms), but have no bold or italic faces.

## Examples

- `cargo run -p laankit-richtext --example preview [note.md]`: a note rendered the way NoteLiner's preview does it (tasks, an aligned table, `<mark>`, `<kbd>` by default). `LAANKIT_THEME` and `LAANKIT_SCREENSHOT` work as in the apps.
- `bench`: see [Numbers](#numbers).

## Tests

`cargo test -p laankit-richtext`:

- HTML→blocks conversion, sanitizing, CSS (from the spike).
- Resolvers: `data:` and bundle images, default link handling.
- Fonts: fallbacks without the LaanKit fonts, and real italic/bold with them.
- Progressive layout: a 600-block document settles, with the same block positions as measuring everything at once.
- Anchoring: after jumping to block 300 the block stays exactly at the top of the viewport on every frame while the blocks above it are measured.
- `#anchor` links scroll to their block.
- `tests/render.rs`: an egui_kittest render in Light and Midnight, checked through the accessibility tree.
- `tests/select.rs`: dragging a selection across three blocks, Ctrl+C, clearing it with a click, clicking a mark, double-click (a word), triple-click (a paragraph), Shift+click (extend and move the end) and Ctrl+A (only when hovered) with copy.
- `tests/preview.rs`: NoteLiner's comrak HTML for task lists (nested too), an aligned table, `<mark>` and `<kbd>`; checkboxes only at the start of an item; articles staying strict; header alignment carried to body cells and CSS beating `align`; checked and unchecked boxes painting differently in Light and Midnight.
- Conversion of row/column spans, book CSS (indents, justify, small caps, `sup`/`sub`, image width), hanging indents and vertical margins (own element only, capped), and text-part order.
- Inline SVG: scripts, handlers, `javascript:` and external references stripped (also through the article sanitizer); the element cap; icon-sized pictures as text; EPUB covers still shown; rendering within the pixel caps at the SVG's own reported size.
- Justification: every row but the last ends at the column's edge, the last stays ragged, and letters inside words keep their spacing. Hanging indents: the first line starts further left, ends at the column's edge when justified, and positions still map through it. `justify_cost` (ignored) times it.

## Not yet

- **MathML and media embeds.** They're dropped; the plan is a card that links out. (`$…$` math is typeset; see Math above.)
- **SVG text with web fonts.** Inline SVG `<text>` uses system fonts; `@font-face` isn't loaded.
- **`==mark==` syntax.** comrak isn't run with its highlight extension (the Svelte build's `marked` had none either), so only `<mark>` written as HTML is highlighted.
- **Selection by keyboard.** Shift+arrow (and Shift+Home/End) extension; selection is by pointer (drag, double/triple-click, Shift+click) and Ctrl+A.
- **Key caps over two rows.** A `<kbd>` that wraps keeps its monospace text but loses its frame.
- **Floats and collapsing margins.** Floats aren't laid out, and vertical margins don't collapse between neighbours.
