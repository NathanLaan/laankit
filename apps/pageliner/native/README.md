# PageLiner Native

PageLiner in pure Rust: an [egui](https://github.com/emilk/egui) UI over `pageliner-core`, with a [hayro](https://github.com/LaurenzV/hayro) PDF reader and no webview or JavaScript. It runs on Linux, Windows and macOS.

It reads and writes exactly what the Electron and Tauri builds do: the same `~/.config/PageLiner` (`ui-preferences.json`) and the same library (`library/pageliner.json`, `books/`, `covers/`, `state/<id>.json`), through the same `pageliner-core` library. PDF positions, bookmarks and highlights are stored in the Svelte reader's format, so you can switch builds mid-book.

## Running

```bash
cargo run -p pageliner-native --release     # or: npm run native:pageliner
cargo test -p pageliner-native
```

The package is `pageliner-native`; its binary is `pageliner`, the one the release packages install as `/usr/bin/pageliner` (the native build replaced the Tauri and Electron ones; see [`docs/packaging.md`](../../../docs/packaging.md)).

- **Import** with Ctrl+O or the Import button (EPUB and PDF), or **drop files** on the window.
- **Window chrome.** Settings → UI → Custom Window Titlebar switches between the system frame and LaanKit's title bar (shared with the Tauri build's `ui-preferences.json`; takes a restart).
- **Command palette.** Ctrl+K: every command, plus the books by title.

### Trying it without touching your library

```bash
export XDG_CONFIG_HOME=$(mktemp -d)
cargo run -p pageliner-native --example pageliner_demo_data -- ~/Books/*.pdf ~/Books/*.epub
cargo run -p pageliner-native --release
```

`pageliner_demo_data` imports the files into `$XDG_CONFIG_HOME/PageLiner/library` (Linux). On Windows the config folder is `%APPDATA%\PageLiner` and on macOS `~/Library/Application Support/PageLiner`, as `laankit_core::paths` mirrors Electron's `userData`.

## Architecture

| Module           | What it does                                                                                     |
|------------------|--------------------------------------------------------------------------------------------------|
| `main`           | The eframe window (1200 × 800, at least 800 × 500), icon, file drops, undecorated with the custom titlebar |
| `app`            | Owns everything; routes backend events, commands, view and reader actions (`App.svelte` + `stores/library.svelte.js`) |
| `backend`        | A tokio runtime around `pageliner-core` and git: the Tauri host's channels; results come back as `Event`s |
| `config`         | `ui-preferences.json`, with the Tauri host's defaults and write shape                            |
| `model`          | Sort modes, the format filter and counts                                                         |
| `commands`       | `registerCommands` and the zoom keys, as laankit-egui commands (shortcuts, palette, Settings list)  |
| `views`          | Library sidebar, grid of book cards (covers decoded off-thread, virtualized rows), status bar, drop overlay |
| `overlays`       | Settings, Git Sync, About, remove-book confirmation                                              |
| `reader`         | Routing by format, the reader bar, `tool_button`, the Annotations pane                            |
| `reader::pdf`    | The PDF reader: `doc` (hayro, outline), `text` (text layer and selection), `render` (worker threads, byte-capped texture cache), `annotations` (the state format) |
| `reader::epub`   | Adapter over `pageliner_epub::EpubView` (see below)                                                |
| `ui_tests`       | egui_kittest: import → open → highlight → page jump → reopen with the state restored → remove     |

### The PDF reader

- **Rendering.** Two worker threads per open book, each with its own parsed copy of the PDF, render pages with hayro. The view asks for the pages on screen plus half a screen ahead, nearest first, and replaces the requests every frame, so pages scrolled past are never rendered. The closest rendered scale is shown while the exact one renders.
- **Memory.** Page textures are capped at 128 MB of RGBA, least recently drawn first (Phase 0: 20 pages at HiDPI fit-width reach 335 MB). The pages on screen are never evicted.
- **Layout.** Continuous scroll rather than pdf.js's one page at a time; zoom −/+ (20–500%), fit width (the percentage button), previous/next, and a page field (click "n / N" and type a page).
- **Text.** A text layer from hayro's glyph positions, extracted on demand: drag across text to select it (Ctrl+C copies), then Highlight. Rects are stored in PDF points from the page's top left, exactly as `PdfReader.svelte` stores them.
- **Outline.** hayro has no outline API, so `doc::outline` walks `/Outlines` itself, resolving explicit destinations, `/GoTo` actions and named destinations (`/Names /Dests` and `/Dests`).
- **Search.** A worker extracts each page's text and reports matching pages as it goes.

### EPUB reader

`src/reader/epub.rs` adapts `pageliner_epub::EpubView` (see `crates/pageliner-epub/README.md`), which draws its own toolbar, Contents/Annotations sidebar and text.

- **State.** EPUB positions and highlights were reset for the native reader (decision of 2026-10-17). Everything it saves lives under the book state's `native` key (position, bookmarks, highlights); the epub.js `cfi`, `bookmarks` and `highlights` keys are never written, so the Electron and Tauri builds keep theirs. `set_state` merges top-level keys, so neither build clobbers the other.
- **Prefs.** Text size, reading theme and face go to `ui-preferences.json` as `epubFontSize`, `epubTheme` (the Svelte keys) and `epubFace` (new).
- A file that isn't a readable EPUB opens an error view with the reason.

## Parity with the Svelte build

| Channel or feature                                     | Status          | Notes                                                                  |
|--------------------------------------------------------|-----------------|------------------------------------------------------------------------|
| `library:list`                                         | Done            | Sort (recently added, title, author) and the All/EPUB/PDF filter with counts |
| `library:import`                                       | Done            | `rfd` multi-file dialog with the same filters; plus drag and drop (native addition); the new book opens |
| `library:delete`                                       | Done            | Hover a card for the remove button; confirms with the Svelte wording   |
| `library:coverDataUrl`                                 | Done            | Decoded and downscaled off-thread; `object-fit: cover`; PDFs show the format icon, as in Svelte |
| `library:getBookData`, `library:getState`, `library:setState` | Done     | Same state keys; the page is saved 400 ms after it changes and on close |
| PDF reader: render, zoom, fit width, prev/next         | Done            | Continuous scroll instead of one page at a time                        |
| PDF reader: page jump                                  | Native addition | Click the page indicator and type a number                             |
| PDF reader: text selection and highlights              | Done            | Same rect units and fields; the highlight text keeps 120 characters     |
| PDF reader: bookmarks                                  | Done            | One per page, toggled from the bar                                     |
| PDF reader: Contents pane (outline)                    | Done            | Explicit, action and named destinations                                |
| PDF reader: Annotations pane                           | Done            | Jump and remove                                                        |
| PDF reader: Search pane                                | Done            | Pages containing the query, case-insensitive; jumps to the first match |
| PDF reader: arrow/Page/Home/End keys                   | Done            | Ignored while typing                                                    |
| EPUB reader                                            | Done            | Native (`pageliner-epub`); positions reset, own `native` state key     |
| Unsupported formats                                    | Done            | "Unsupported format" view                                              |
| `git:getInfo`, `git:enable`, `git:setRemote`, `git:syncNow`, `git:getStatus` | Done | Git Sync dialog; enabling writes `gitSyncEnabled` like `setUIPrefs` |
| Auto-sync (commit and push 8 s after changes)          | Done            | Debounced like the Tauri host; never pulls on its own                   |
| After Sync Now                                         | Native addition | The library reloads, so books and progress pulled from another device appear |
| `ui:getPrefs` / `ui:setPrefs`                          | Done            | Custom titlebar (restart relaunches), sidebar and status bar toggles    |
| Themes and UI scale                                    | Done            | All six themes; Ctrl+= / - / 0 and Ctrl+wheel                          |
| Keyboard shortcuts                                     | Done            | Ctrl+O, Ctrl+E, Ctrl+J, Ctrl+Shift+S, Ctrl+,, Ctrl+Shift+I, Ctrl+K     |
| Command palette                                        | Done            | Commands plus books by title                                           |
| Status bar                                             | Done            | Book or count, "Page n / N", format, scale                             |
| About and update check                                 | Done            | `laankit_core::updates`; PageLiner publishes no release repo, so it reports "latest", as the Tauri build does |
| Window size and position                               | Partial         | Remembered by eframe in its own storage, not the Tauri window-state file |
| Single instance                                        | Not yet         | A second launch opens a second window                                  |
| Sync Now error with status                             | Partial         | The error is shown; the Svelte dialog also showed the status alongside it |

## Diagnostics

| Variable                                   | Effect                                                            |
|--------------------------------------------|-------------------------------------------------------------------|
| `LAANKIT_SCREENSHOT=<png>`                  | Saves a screenshot once the window has settled (`LAANKIT_SCREENSHOT_EXIT=1` closes afterwards) |
| `LAANKIT_THEME`, `LAANKIT_SCALE`             | Override the theme and scale                                      |
| `LAANKIT_BOOK=<n or id>`                    | Opens the nth book in the grid's order, or the book with that id  |
| `LAANKIT_OPEN=<dialog>`                     | `settings[:ui\|shortcuts]`, `sync`, `about`, `palette` or `remove` |
| `LAANKIT_PDF_PAGE=<n>`                      | Opens the PDF at page n                                           |
| `LAANKIT_PDF_SELECT=<page>:<x0>,<y0>,<x1>,<y1>` | Selects and highlights that range (page units) once its text is in |

On GNOME Wayland the screenshot never fires while the session is locked; run with `WAYLAND_DISPLAY=` (XWayland) instead.

## Still to verify by hand

- The file dialog and drag and drop on Windows, macOS and KDE.
- The undecorated window on Windows and macOS.
- Git Sync against a real remote (enable, set remote, Sync Now).
- PDFs with unusual outlines (remote destinations, broken trees) and encrypted PDFs (hayro's support is partial).
- Trackpad scrolling and zoom feel on large PDFs at HiDPI.
