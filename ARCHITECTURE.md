# Architecture

Marina is a Cargo workspace of four local-first desktop apps — **NoteLiner**,
**ThreadLiner**, **PageLiner** and **BlockLiner** — built on a shared Rust
foundation. Every app is a pure-Rust [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/master/crates/eframe)
program: there is no webview and no JavaScript, and each app's data is plain
files the user controls (a git repository for the first three, any folder of
Mermaid files for BlockLiner).

This document explains how the pieces fit together. For packaging and release
mechanics see [`docs/packaging.md`](docs/packaging.md); for the security model
see [`SECURITY.md`](SECURITY.md).

## Layers

```
┌──────────────────────────────────────────────────────────────┐
│  apps/<app>/native   the app: eframe window, views, overlays │
│                      + a tokio "backend" bridging to the core │
├───────────────┬──────────────────────────────┬───────────────┤
│  marina-egui  │  marina-editor  marina-richtext│  <app>-core   │  UI + app
│  (egui shell) │  (editor)       (HTML render)  │  (backend)    │  building blocks
├───────────────┴──────────────────────────────┴───────────────┤
│  marina-core   git sync · JSON stores · XDG paths · debounce  │  shared
│                · update check                                 │  foundation
└──────────────────────────────────────────────────────────────┘
```

Dependencies point downward only. The UI crates never reach into an app's
backend, and the core crates never depend on egui, so the backends stay testable
in isolation and the egui shell is reusable across apps.

## The shared foundation

### `marina-core`

Framework-independent backend shared by the apps (BlockLiner uses `paths`,
`json` and `debounce`, not `git`):

- **`paths`** — the per-platform config and data directories (mirroring where
  the apps have always stored data: `~/.config/<App>` and
  `~/.local/share/<app>` on Linux, the platform equivalents elsewhere).
- **`git`** — a thin wrapper over the system `git` CLI for init, commit, pull,
  push, remotes, branch and history. Marina shells out to `git` rather than
  linking a git library, so it uses the user's existing credentials and SSH
  config.
- **`json`** — atomic, order-preserving JSON stores for each app's index file.
- **`debounce`** — the timers behind "commit on change, push after a quiet
  period".
- **`updates`** — the launch-time update check.

### App cores (`noteliner-core`, `threadliner-core`, `pageliner-core`, `blockliner-core`)

Each app's own backend, independent of any UI. A core owns the on-disk model
(the index, notes/feeds/books, attachments) and the operations on it, and knows
nothing about egui. Supporting crates sit alongside: `noteliner-export`
(in-process PDF/Typst), `noteliner-slides` (presentations) and `pageliner-epub`
(EPUB parsing and reading state).

`blockliner-core` is the diagram model and its file format: a Mermaid
flowchart that says what exists, with the layout in `%%` comments Mermaid
ignores, so a diagram renders anywhere Mermaid does. It also lays out plain
Mermaid that has no positions, and keeps the statements it doesn't model
verbatim. BlockLiner has no index file and no sync: the open folder's `.mmd`
files are the diagrams, saved through `marina_core`'s atomic writes and
debouncer.

## The UI layer

### `marina-egui`

The shared egui shell — the native counterpart of the old web UI framework:
themes and fonts, the window chrome and optional custom title bar, the command
palette, the settings dialog, panes and toolbars. `cargo run -p marina-egui
--example gallery` shows the widgets in isolation.

**Why egui/eframe, with glow (not wgpu).** eframe is configured with the `glow`
(OpenGL) renderer rather than `wgpu`: glow is more broadly available across the
Linux desktops and drivers the apps target, and avoids a heavier GPU stack. The
feature set is pinned deliberately in the workspace `Cargo.toml` (`clipboard`
for the system clipboard, `links` for opening URLs, `accesskit` for
accessibility, `persistence` for eframe's own state).

### `marina-editor` and `marina-richtext`

`marina-editor` is NoteLiner's Markdown editor: its own text engine (not egui's
`TextEdit`), with syntax highlighting, list editing, multiple cursors, folding,
find/replace, `[[wikilink]]` completion and optional Hunspell spell check.
`marina-richtext` renders HTML/XHTML for ThreadLiner's articles and PageLiner's
EPUB content.

## An app

Each `apps/<app>/native` crate is an eframe program:

- **`main`** sets up the window (size, icon, app id, optional undecorated custom
  title bar) and the `persistence_path` for eframe's own state. NoteLiner's
  `main` also handles `--mcp-bridge`.
- **`app`** owns all UI state and routes three kinds of input: backend events,
  commands (keyboard / palette / menus) and view actions.
- **`backend`** runs a tokio runtime around the app's core. The UI sends
  requests over channels and the backend replies with `Event`s — the same
  request/response shape the apps used when the UI was a separate process, now
  in-process. This keeps git and filesystem work off the UI thread.
- **`views`** / **`overlays`** are the panels (tree, panes, preview, status bar)
  and the dialogs.

### NoteLiner's MCP server

NoteLiner can expose the open project to AI assistants over the
[Model Context Protocol](https://modelcontextprotocol.io). It is off by default
and local-only: it binds **no TCP port**, listening on a user-scoped Unix domain
socket, and external clients connect through the in-process bridge
(`noteliner --mcp-bridge`), discovered via a runtime file. Writes go through the
same save path as the UI (and so are committed to git), gated by optional
per-call confirmation and per-tool allow/deny. See
[`apps/noteliner/README.md`](apps/noteliner/README.md) and
[`apps/noteliner/native/README.md`](apps/noteliner/native/README.md).

## Cross-platform state

All apps store data in the shared `marina-core` paths, so settings and projects
are byte-compatible across platforms and across builds. eframe's own UI state
(theme, window bounds, pane layout) is kept separately in `app.ron` under the
app's data folder.

## Packaging pipeline

`xtask` turns each app's `assets/icon.png` into the icon sizes the packages need
(`cargo run -p xtask -- icons`), and `scripts/package-native.sh` builds the
release binaries and wraps them as `.deb`/`.rpm`/AppImage/NSIS/`.app`+`.dmg`.
Full details, including the Linux runtime dependencies and CI, are in
[`docs/packaging.md`](docs/packaging.md).

## Testing

Unit and integration tests live with each crate. UI tests use
[`egui_kittest`](https://github.com/emilk/egui) and run **headless**: they drive
the real app through AccessKit and assert on widgets and on-disk state, never
opening a window or comparing pixels. Each app's `ui_tests*` modules cover the
core flows (create → edit → autosave → rename → move → delete, plus the panes,
editor and, for NoteLiner, slides and PDF export; for BlockLiner, drawing,
undo, import and export, with a golden SVG). Run the whole suite with
`cargo test --workspace`.
