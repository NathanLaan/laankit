# marina-richtext

Sanitized HTML and XHTML reduced to native rich-text blocks and laid out with egui, with no
webview. Shared by ThreadLiner (articles), PageLiner (EPUB chapters) and, later, the NoteLiner
Markdown preview. Promoted from the Phase 0 spikes 2 and 4 (`apps/spikes/richtext`); see
`docs/plans/plan-native-rust-egui.md`.

## API

```rust
use marina_richtext::{from_article_html, Action, RichStyle, RichView, WebResolver};

// Once, at startup (fonts from marina-egui; images decode on egui_extras' background thread).
marina_egui::fonts::install(ctx);
egui_extras::install_image_loaders(ctx);

// When an entry is opened:
let doc = from_article_html(&entry.content, entry.link.as_deref());
view.reset();
view.restore(saved_position); // optional

// Every frame:
let style = RichStyle::from_ctx(ui.ctx()); // colours from the active Marina theme
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

### Selection and highlights

Dragging selects text across blocks (paragraphs, list items, quotes, code, table cells); the
view scrolls while the pointer is held past its top or bottom edge. Ctrl+C copies the selection
(each text part on its own line) when no text field has focus, and Escape clears it. A
`Selection` is `start..end` in document positions plus the text.

`Mark`s (id, start, end, colour) are painted behind the text, like the selection, by
`egui::text_selection::visuals::paint_text_selection`; a click (not a drag) on one is reported
as `RichOutput::clicked_mark`. PageLiner uses them for highlights.

### EPUB CSS

Honoured, because books use them to carry meaning or structure: `font-style`, `font-weight`,
`text-decoration`, monospace `font-family`; `text-align` including `justify`; `text-indent`,
`margin-left`, `padding-left` and the `margin`/`padding` shorthands (left indents accumulate
through nested blocks, capped at 6 em); `font-variant: small-caps` and
`text-transform: uppercase` (applied letter for letter, so positions don't move);
`vertical-align: super | sub` (as `<sup>`/`<sub>`: smaller and raised or lowered); image
`width` in `%` or lengths; `display: none`.

Ignored, because a reflowing reader shouldn't take them from the book: colours and backgrounds
(the reading theme owns them), font sizes and line heights (the reader's setting), vertical
margins, floats, positioning and `page-break-*`. Negative (hanging) `text-indent`s are dropped.
`justify` is laid out ragged-right for now: egui's justification spreads space between every
glyph, not between words, which reads worse. Row and column spans are padded with empty cells so
columns line up.

Images are handed to egui as URIs; fetching (`ehttp`) and decoding (`egui_extras`' image loader)
happen off the UI thread, and the image block takes its real height when the texture arrives.

### Fonts

The view uses what `marina_egui::fonts::install` registers and never reads system fonts:

| Text    | Font                                                                         |
|---------|------------------------------------------------------------------------------|
| regular | `FontFamily::Proportional` (Inter)                                           |
| italic  | the `sans-italic` family (`marina_egui::fonts::ITALIC`)                      |
| bold    | Inter Bold / Bold Italic (static faces from `marina_egui::fonts`)            |
| code    | `FontFamily::Monospace` (JetBrains Mono)                                     |
| serif   | Source Serif 4 Regular, Italic, Bold, Bold Italic for `Face::Serif` (`marina_egui::fonts::SERIF*`) |

Anything missing falls back to the regular face: italics slanted by egui, bold in the strong
colour. The view never asks for a family that isn't registered.

## Progressive layout

Phase 0 laid out every block in a document's first frame to learn its height: 27 ms for a
208-block article, 41 ms for a 570-block EPUB section. `RichView` spreads that work out:

1. **Estimates.** When a document opens (or the width changes) every block gets an estimated
   height from its text length at an average glyph width, so the scroll area has a size at once.
2. **The viewport first.** Each frame lays out and paints the blocks on screen, plus blocks
   within `OVERSCAN` (600 px) that are already measured. An unmeasured overscan block waits for
   step 3, so the first frame costs one screenful.
3. **A budget for the rest.** Unmeasured off-screen blocks are laid out invisibly, top to bottom,
   until `DEFAULT_BUDGET` (4 ms) of that work is spent (always at least one block), and their
   real heights replace the estimates. The view repaints until none are left (`settled()`).
4. **Anchoring.** The view records the block at the top of the viewport and its offset. If blocks
   above it change height, it computes the corrected scroll offset, discards the frame
   (`request_discard`), and starts the next pass at that offset, so the jump is never painted.
   That redraw pass measures nothing new above the viewport, because there would be no pass left
   to correct another shift. Restoring a position and following `#anchor` links use the same
   path. The view owns its `ScrollArea` with `animated(false)`: an animated programmatic scroll
   wouldn't land within the frame.

`RichView::with_budget(Duration::MAX)` gives the old behaviour: everything is measured in the
first frame.

## Numbers

`cargo run --profile spike -p marina-richtext --example bench [articles.json]`, headless at
1000×800 with tessellation. Each run uses a fresh context warmed up by a different document, so
glyphs are rasterized but this document's text has never been laid out (opening another article
in the app). The five longest of the Phase 0 cached articles were used (not committed). Load
average ≈ 7.5 (a busy desktop), so treat these as upper bounds.

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

Re-measured after adding cross-block selection, marks and the wider CSS (Phase 3): the same
within half a millisecond (2,000 blocks: 12.0 ms first frame; longest article 16.1 ms).

The first frame no longer grows with the document (12 ms at 2,000 blocks, against 295 ms when
measuring everything), and it's the slowest frame of each run.

What's left of the first frame is shaping the visible screenful's text for the first time. The
fonts matter: with the variable Inter that `marina-egui` first bundled, the same first frames
took 15–31 ms, so it now bundles static instances (Regular, SemiBold, Bold, Italic, Bold Italic),
which shape about twice as fast. egui's own small default fonts (`BENCH_EGUI_FONTS=1`) are
faster still (6–8 ms), but have no bold or italic faces.

## Tests

`cargo test -p marina-richtext`:

- HTML→blocks conversion, sanitizing, CSS (from the spike).
- Resolvers: `data:` and bundle images, default link handling.
- Fonts: fallbacks without the Marina fonts, and real italic/bold with them.
- Progressive layout: a 600-block document settles, with the same block positions as measuring
  everything at once.
- Anchoring: after jumping to block 300 the block stays exactly at the top of the viewport on
  every frame while the blocks above it are measured.
- `#anchor` links scroll to their block.
- `tests/render.rs`: an egui_kittest render in Light and Midnight, checked through the
  accessibility tree.
- `tests/select.rs`: dragging a selection across three blocks, Ctrl+C, clearing it with a click,
  and clicking a mark.
- Conversion of row/column spans, book CSS (indents, justify, small caps, `sup`/`sub`, image
  width) and text-part order.

## Not yet

- **Real justification.** `text-align: justify` is set ragged-right (see above).
- **Hanging indents.** Negative `text-indent`s are dropped.
- **Inline SVG, MathML and media embeds.** They're dropped; the plan is a card that links out.
- **Selection by keyboard and double-click.** Selection is by dragging only; there's no
  Shift+arrow extension, word or paragraph selection yet.
- **Floats and vertical margins.** Books that space paragraphs with margins instead of indents
  get the reader's own paragraph gap.
