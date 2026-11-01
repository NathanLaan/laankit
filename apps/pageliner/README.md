# PageLiner

PageLiner is a desktop **e-reader and document library** for EPUB and PDF, with a cover-grid library, table-of-contents navigation, in-document search, bookmarks, highlights, reading-position persistence and optional git sync of the library index and reading state.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over the shared `laankit-*` crates — no webview, no JavaScript. The shipped binary is `pageliner`.

- **Run, build, install:** see the [root README](../../README.md).
- **The native app in depth:** see [`native/README.md`](native/README.md).
- **How it fits together:** [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Features

- **Library** — import EPUB/PDF, a cover grid with EPUB cover/metadata extraction, sort, delete.
- **PDF reader** — page navigation (buttons + arrow/Page keys), zoom and fit-width, a **Contents** (outline) pane and in-document **Search**, with persisted page position and a selectable text layer.
- **EPUB reader** — paginated rendering, arrow-key page turns, reading settings (font size, Light/Sepia/Dark), a **Contents** (TOC) pane and position persistence.
- **Annotations** — bookmarks and highlights for both formats, surfaced in a shared **Annotations** pane (jump + delete) and reapplied on reopen.
- **Git sync (opt-in)** — versions the library index and reading state through a git remote; book blobs and covers stay local.

## Data model

```
<library folder>/
├── pageliner.json     # index: [{ id, title, author, format, file, cover, tags, addedAt }]
├── books/<id>.<epub|pdf>
├── covers/<id>.<png|jpg>
└── state/<id>.json    # { position, bookmarks[], highlights[], lastOpenedAt }
```

Git sync is optional: `pageliner.json` and `state/` version cleanly, while the large `books/` and `covers/` blobs are kept local to avoid repo bloat. Sync uses the system `git` (no extra dependency).
