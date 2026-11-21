# noteliner-pptx

NoteLiner's PowerPoint export: a presentation as an editable `.pptx` file, written directly as Office Open XML with the `zip` crate. No other program is needed.

Titles, text, lists, and tables are PowerPoint text and tables, and pictures are pictures, so the slides can be edited. PowerPoint wraps the text itself and uses the fonts on the computer that opens the file, so a slide is close to NoteLiner's and not identical.

## API

```rust
use noteliner_pptx::{export, needs, Assets, Options};

let deck = noteliner_slides::parse_deck(&raw_note, Some(&presentation));

// The crate reads no files and draws nothing. `needs` lists the pictures and
// the display formulas the slides show, and the caller supplies them.
let wanted = needs(&deck);
let mut assets = Assets::default();
// assets.pictures.insert(src, Picture { bytes, format, natural });
// assets.formulas.insert(formula, Picture { .. });

let out = export(&deck, &assets, &Options { title: name, ..Default::default() })?;
std::fs::write("talk.pptx", out.bytes)?;
for line in out.warnings { /* "Slide 4: A picture could not be read: …" */ }
```

| Item | What it does |
|---|---|
| `needs(&deck)` | The picture addresses and the display formulas on the deck's slides, each once. |
| `Assets` | `pictures` by the address on the slide, and `formulas` by `Formula` (source, text size, and colour). |
| `Picture` | The bytes of a PNG, JPEG, or GIF file, and its size on the stage in stage pixels. |
| `Format::sniff(bytes)` | The type of a picture file, from its first bytes. |
| `Options` | The title, the header and footer placeholders, the two font names (Inter and JetBrains Mono), and the time the file says it was made. |
| `export(&deck, &assets, &options)` | The file's bytes, and a line for each part that could not be exported in full. |

`cargo run -p noteliner-pptx --example export -- deck.md out.pptx` exports a presentation without the app. It reads pictures from the paths on the slides and does not render formulas.

## What a slide becomes

A stage pixel is 6,350 EMU, the unit PowerPoint measures in. The three stage sizes are then PowerPoint's own 16:9, 16:10, and 4:3 slide sizes, and a font size in stage pixels is half that many points.

| On the slide | In the file |
|---|---|
| The title headings | The title placeholder, so the title shows in PowerPoint's outline and NoteLiner's importer reads it back as the title. The accent band is the placeholder's fill. |
| Paragraphs and lists | One text box for text that follows text. Runs carry bold, italic, underline, strikethrough, superscript, subscript, the monospace font, a highlight, and a link. |
| A list item | A paragraph with PowerPoint's bullet or number. Each numbered item says its own number, so a paragraph between two items does not restart the count. |
| A quote | A bar and a text box, or italic text on a Quote slide. |
| A code block | A rounded box in the code background, one paragraph for each line. |
| A table | A PowerPoint table, with a rule under each row. |
| A picture | A picture, contained in its place, or cropped to cover a Full Bleed slide. A picture used on several slides is in the file once. |
| A display formula | The picture in `Assets::formulas`, centred, with the source as its description. Without one, the source as text. |
| An inline formula | Its source, in the monospace font. |
| A video | A box with a link. |
| A missing picture | A box with the alt text and the address, and a warning. |
| Notes | The notes page, one paragraph for each line. |
| Header, footer, slide number | Text boxes. The number is PowerPoint's slide number field. |
| The slide theme | Explicit colours on every shape, and the file's theme colours. A dark theme maps PowerPoint's background to the theme's dark colour. |

## Layout

`slide.rs` follows `noteliner_slides::render`: the same padding, gaps, bands, columns, and font sizes, from the constants and the size functions that file exports. It does not copy the position of each line, since PowerPoint wraps text with its own measurements.

- The height of text is estimated from the number of characters (`text::lines`), which is enough to stack boxes.
- Parts that would run past the bottom of the slide are all made shorter by the same share, so every shape stays on the slide.
- The last text box of a stack takes the room that is left.
- On a Hero or a Quote slide the title box ends where the text is centred and grows upwards, and the body box starts there and grows downwards. A wrong estimate moves the pair and never overlaps them.
- Text boxes are set to shrink text that overflows (`normAutofit`).

## The package

| Part | Contents |
|---|---|
| `[Content_Types].xml`, `_rels/.rels`, `docProps/` | The package's index and properties. |
| `ppt/presentation.xml` | The slide size and the slide order. |
| `ppt/theme/theme1.xml`, `theme2.xml` | The deck's colours and fonts, and a plain theme for the notes. |
| `ppt/slideMasters/slideMaster1.xml`, `ppt/slideLayouts/slideLayout1.xml` | One master and one Title Only layout. |
| `ppt/notesMasters/notesMaster1.xml` | The notes page. |
| `ppt/slides/slideN.xml`, `ppt/notesSlides/notesSlideN.xml` | A slide and its notes. |
| `ppt/media/imageN.png` | The pictures, stored without compressing them again. |

The same deck always gives the same bytes when `Options::created` is not set.

## Tests

`cargo test -p noteliner-pptx`:

- **Well-formed:** every part of a written file parses as XML, has a content type, and has no shape id twice. Every relationship points at a part in the file, and every `r:id` is a relationship of its part.
- **Contents:** titles, lists, tables, code, links, notes, slide sizes, a slide's own background and theme, pictures (contained, cropped, used twice, and missing), formulas, and videos.
- **Text XML cannot hold:** control characters are dropped, and `<`, `&`, quotes, emoji, and right-to-left marks are kept.
- **On the slide:** long text, long code, and a long table in each of the eight layouts leave no shape outside the slide.
- **Generated decks:** 400 decks made from lines that are hard to parse, in six configurations, each exported and checked.

The checks do not open the file in PowerPoint. LibreOffice opens the sample deck (`soffice --headless --convert-to pdf out.pptx`). PowerPoint is stricter than LibreOffice, and only opening a file in PowerPoint shows whether it asks to repair it.
