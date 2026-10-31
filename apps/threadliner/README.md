# ThreadLiner

ThreadLiner is a desktop **RSS reader** that stores everything as git-synced
JSON. It supports RSS 2.0, Atom and JSON Feed, with read-state tracking and
optional cross-device sync through a git remote.

It is a pure-Rust desktop app built on [egui](https://github.com/emilk/egui) over
the shared `laankit-*` crates — no webview, no JavaScript. The shipped binary is
`threadliner`.

- **Run, build, install:** see the [root README](../../README.md).
- **The native app in depth:** see [`native/README.md`](native/README.md).
- **How it fits together:** [`ARCHITECTURE.md`](../../ARCHITECTURE.md).

## Features

- Subscribe to RSS 2.0, Atom and JSON Feed sources
- Article list with read/unread tracking and HTML article rendering
  (`laankit-richtext`)
- Tags and filtering
- Desktop notifications for new items
- Optional git sync of feeds and read state

## Data model

ThreadLiner stores all data as JSON in a local git repository:

- `configuration/settings.json` — app settings (theme, sync wait time)
- `data/feeds.json` — the feed index
- `data/feeds/<id>/entries.json` — per-feed entries with read status

Changes are committed locally on every action and pushed to a remote on a
debounce timer. Sync uses the system `git` CLI and your existing SSH
credentials; `git` must be installed at runtime.

## Historical plans

`docs/` holds the original planning and design notes, kept as a record of how
the app was built rather than as a guide to the current native build.
