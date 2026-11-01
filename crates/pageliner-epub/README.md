# pageliner-epub

PageLiner's native EPUB reader. No webview and no epub.js: the book is converted to `laankit-richtext` blocks and laid out by egui. It replaces `EpubReader.svelte` and `AnnotationsPane.svelte`.

```bash
cargo run -p pageliner-epub --example reader -- book.epub   # a standalone window
cargo test -p pageliner-epub
```

## What it does

- **The whole book in one scroll.** Every spine document is converted when the book opens and joined into one document, so scrolling and paging cross chapter boundaries. Opening measured 58 ms for a 30-chapter, 2,175-block book and 10–17 ms for shorter ones (`--profile fast-release`), so hosts may open books on a background thread but don't have to.
- **Toolbar** (as in the Svelte reader): back to the library, sidebar toggle, the chapter title, Highlight selection, Bookmark toggle, text size (60–220%), reading theme (Light, Sepia, Dark), serif/sans, previous/next page.
- **Sidebar**: the Contents pane (the current entry marked) and the Annotations pane (bookmarks and highlights, jump and delete), in a `laankit_egui::panes` stack.
- **Paging**: Page Down / Space / → and Page Up / ← (no modifiers, no text field focused), and the arrows at the sides of the page. A page keeps 48 px of the previous one.
- **Selection and highlights**: drag to select across paragraphs, Ctrl+C to copy, then Highlight. Highlights are painted under the text in `#f0c800` at 30%.
- **Links**: in-book links and contents entries scroll to their anchor (or their chapter's start when the anchor is unknown); external links go to the host.

## Integration contract

The host (PageLiner's native app) owns all I/O. The reader only returns events.

```rust
use pageliner_epub::{Book, EpubEvent, EpubView, NativeState, ReaderPrefs};

// Opening a book (`library` is pageliner_core::library::Library):
let book = Book::open(&library.book_file(&id).unwrap())?;           // or Book::from_bytes
let state = NativeState::from_book_state(&library.get_state(&id));   // the "native" key only
let prefs = ReaderPrefs::from_ui_prefs(&ui_prefs);                   // epubFontSize, epubTheme, epubFace
let mut reader = EpubView::new(book, state, prefs);

// Every frame, in the space the reader should fill:
for event in reader.show(ui) {
    match event {
        EpubEvent::SaveState(patch) => { library.set_state(&id, &patch)?; }
        EpubEvent::SavePrefs(patch) => { /* merge into the UI preferences file */ }
        EpubEvent::OpenUrl(url) => ctx.open_url(egui::OpenUrl::new_tab(url)),
        EpubEvent::Status(text) => status_bar = text,   // libraryState.readingStatus
        EpubEvent::Close => { /* back to the library; the last position was just saved */ }
    }
}

// Closing it any other way (switching books, quitting):
if let Some(patch) = reader.flush() {
    library.set_state(&id, &patch)?;
}
```

Call `egui_extras::install_image_loaders` and `laankit_egui::fonts::install` once at startup (images decode on egui_extras' background thread; the serif face comes from `laankit-egui`).

| Event          | When                                                                                         |
|----------------|----------------------------------------------------------------------------------------------|
| `SaveState`    | The position settled for 0.5 s (`saveProgress`'s debounce), a bookmark or highlight was added or removed, or Library was pressed |
| `SavePrefs`    | Text size, reading theme or face changed                                                     |
| `OpenUrl`      | An external link (`https:`, `mailto:`) was clicked                                           |
| `Status`       | The chapter at the top of the screen changed                                                 |
| `Close`        | Library was pressed (after a final `SaveState` if the position moved)                        |

The view also offers `go_to(Location)`, `page(forward)`, `toggle_bookmark()`, `highlight_selection()`, `delete_annotation(id)`, `jump_to_annotation(id)`, `location()`, `state()`, `prefs()` and `panes()`/`set_panes()` (the sidebar layout, if the host wants to keep it), for commands, shortcuts and tests. Each mutating call returns the patch to persist.

## Saved state

Per book, in `state/<id>.json`, under one key that only the native reader uses:

```json
{
  "cfi": "epubcfi(/6/4!/4/2/1:0)",
  "bookmarks": [ "…the Tauri/Electron build's, untouched…" ],
  "highlights": [ "…" ],
  "native": {
    "position": { "spine": 4, "block": 12, "offset": 37 },
    "bookmarks": [
      { "id": "…", "location": { "spine": 4, "block": 0, "offset": 0 }, "label": "Chapter 2", "createdAt": "2026-10-17T21:03:11.402Z" }
    ],
    "highlights": [
      { "id": "…", "start": { "spine": 4, "block": 3, "offset": 0 }, "end": { "spine": 4, "block": 5, "offset": 18 },
        "text": "First 120 characters, whitespace collapsed", "createdAt": "…" }
    ]
  },
  "lastOpenedAt": "…"
}
```

- `spine` is the spine document, `block` the top-level block within it, and `offset` a character offset through that block's text parts (`laankit_richtext::Block::part_text`).
- `Library::set_state` merges top-level keys, so the native reader's patches (`{ "native": … }`) and the other build's (`cfi`, `bookmarks`, `highlights`) never overwrite each other. A test checks both directions.
- CFI positions and highlights are not converted (decided 2026-10-17): a book opens at its start the first time the native reader opens it.
- Locations survive app updates as long as the converter splits the book into the same blocks. A changed converter can move a saved position by a few blocks; it's clamped to the book.

Reading preferences go in the UI preferences under `epubFontSize` and `epubTheme` (shared with the Svelte reader) and `epubFace` (`serif` or `sans`, native only).

## Not yet

- Real justification and hanging indents (see `laankit-richtext`'s README).
- Pagination (two-page spreads). The reader scrolls; paging moves a screen at a time.
- Search inside the book.
- Keyboard selection (Shift+arrows) and double-click word selection.
- Fixed-layout EPUBs (comics, picture books) are reflowed like any other book.
