# Architecture

LaanKit is a set of four desktop apps, NoteLiner, ThreadLiner, PageLiner, and BlockLiner, that share one UI and backend foundation. Every app is built in pure `Rust`, using [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/master/crates/eframe), with no webview and no JavaScript. Each app stores its data as plain files you control: a git repository for NoteLiner, ThreadLiner, and PageLiner, and any folder of Mermaid files for BlockLiner.

This document explains how the pieces fit together. For packaging and releases, see [`docs/packaging.md`](docs/packaging.md). For the security model, see [`SECURITY.md`](SECURITY.md).

## Layers

```
┌────────────────────────────────────────────────────────────────────┐
│  apps/<app>/native   the app: eframe window, views, overlays       │
│                      + a tokio "backend" bridging to the core      │
├────────────────┬───────────────────────────────────┬───────────────┤
│  laankit-egui  │  laankit-editor  laankit-richtext │  <app>-core   │  UI + app
│  (egui shell)  │  (editor)        (HTML render)    │  (backend)    │  building blocks
├────────────────┴───────────────────────────────────┴───────────────┤
│  laankit-core   git sync · JSON stores · XDG paths · debounce      │  shared
│                 · update check                                     │  foundation
└────────────────────────────────────────────────────────────────────┘
```

Dependencies only point down. The UI crates never reach into an app's backend, and the core crates never depend on egui. This keeps the backends testable on their own, and lets every app reuse the egui shell.

## The shared foundation

### `laankit-core`

The backend shared by the apps, with no UI framework. BlockLiner uses `paths`, `json`, and `debounce`, but not `git`.

| Module | What it does |
|--------|--------------|
| `paths`    | The config and data folders for each platform: `~/.config/<App>` and `~/.local/share/<app>` on Linux, and the platform equivalents elsewhere. |
| `git`      | A thin wrapper over the system `git` CLI for init, commit, pull, push, remotes, branch, and history. LaanKit runs `git` instead of linking a git library, so it uses your existing credentials and SSH config. |
| `json`     | Atomic, order-preserving JSON stores for each app's index file. |
| `debounce` | The timers behind "commit on change, push after a quiet period". |
| `updates`  | The update check when an app starts. |

### App cores

Each app has its own backend crate: `noteliner-core`, `threadliner-core`, `pageliner-core`, and `blockliner-core`. A core owns the files on disk (the index, the notes, feeds, or books, and the attachments) and the operations on them, and knows nothing about egui. A few supporting crates sit alongside:

| Crate | What it does |
|-------|--------------|
| `noteliner-export` | In-process PDF export (Typst). |
| `noteliner-slides` | Presentations. |
| `pageliner-epub`   | EPUB parsing and reading state. |

`blockliner-core` is the diagram model and its file format. A diagram is a Mermaid flowchart that says what exists, with the layout in `%%` comments that Mermaid ignores, so a diagram renders anywhere Mermaid does. It can also lay out plain Mermaid that has no positions, and it keeps any statements it doesn't model exactly as written. BlockLiner has no index file and no sync. The `.mmd` files in the open folder are the diagrams, saved through the atomic writes and debouncer in `laankit-core`.

## The UI layer

### `laankit-egui`

The shared egui shell: themes and fonts, the window chrome and optional custom title bar, the command palette, the settings dialog, panes, and toolbars. Run `cargo run -p laankit-egui --example gallery` to see the widgets on their own.

#### Why glow and not wgpu

eframe uses the `glow` (OpenGL) renderer instead of `wgpu`. glow works on more of the Linux desktops and drivers the apps target, and avoids a heavier GPU stack. The eframe features are set in the workspace `Cargo.toml`: `glow`, `default_fonts`, `wayland` and `x11` for the window, `clipboard` for the system clipboard, `links` for opening URLs, `accesskit` for accessibility, and `persistence` for eframe's own state.

### `laankit-editor` and `laankit-richtext`

`laankit-editor` is NoteLiner's Markdown editor. It has its own text engine instead of egui's `TextEdit`, with syntax highlighting, list editing, multiple cursors, folding, find and replace, `[[wikilink]]` completion, and optional Hunspell spell check.

`laankit-richtext` renders HTML and XHTML for ThreadLiner's articles and PageLiner's EPUB content.

## An app

Each `apps/<app>/native` crate is an eframe program:

| Module | What it does |
|--------|--------------|
| `main`     | Sets up the window (size, icon, app id, and the optional custom title bar) and the `persistence_path` for eframe's own state. NoteLiner's `main` also handles `--mcp-bridge`. |
| `app`      | Owns all of the UI state, and handles three kinds of input: backend events, commands (keyboard, palette, and menus), and view actions. |
| `backend`  | Runs a tokio runtime around the app's core. The UI sends requests over channels, and the backend replies with `Event`s. This keeps git and file work off the UI thread. |
| `views`, `overlays` | The panels (tree, panes, preview, and status bar) and the dialogs. |

### NoteLiner's MCP server

NoteLiner can share the open project with AI assistants over the [Model Context Protocol](https://modelcontextprotocol.io). It is off by default and only works locally. It doesn't open a TCP port. Instead it listens on a Unix domain socket that only your user can reach, and AI clients connect through the bridge built into the app (`noteliner --mcp-bridge`), which finds the socket through a runtime file. Changes from an AI client are saved the same way as changes in the UI, so they are committed to git too. You can ask to confirm each change, and allow or deny each tool. See [`apps/noteliner/README.md`](apps/noteliner/README.md) and [`apps/noteliner/native/README.md`](apps/noteliner/native/README.md).

## Settings and state across platforms

Every app stores its data in the `laankit-core` paths, so settings and projects are the same files on every platform and in every build. eframe's own UI state (theme, window size and position, and pane layout) is kept separately, in `app.ron` in the app's data folder.

## Packaging

`xtask` turns each app's `assets/icon.png` into the icon sizes the packages need (`cargo run -p xtask -- icons`). `scripts/package-native.sh` builds the release binaries and packages them as `.deb`, `.rpm`, AppImage, an NSIS installer, and a macOS `.app` and `.dmg`. See [`docs/packaging.md`](docs/packaging.md) for the details, including the Linux runtime libraries and CI.

## Testing

Unit and integration tests live with each crate. The UI tests use [`egui_kittest`](https://github.com/emilk/egui/tree/master/crates/egui_kittest) and run headless. They drive the real app through AccessKit and check the widgets and the files on disk, without opening a window or comparing pixels.

Each app's `ui_tests*` modules cover the main flows: create, edit, autosave, rename, move, and delete, plus the panes and the editor. NoteLiner's also cover slides and PDF export. BlockLiner's cover drawing, styles, undo, import, and export, including a golden SVG file.

Run every test with `cargo test --workspace`.
