# Marina

Monorepo for desktop apps that share UI and desktop-shell infrastructure.
Each app ships two ways from the same Svelte renderer: the original
Electron build, and a native Linux build on Tauri with a Rust backend
(see [Native Linux builds](#native-linux-builds-tauri)).

## Apps

- `apps/noteliner/`   — outliner-style note-taking
- `apps/threadliner/` — RSS reader with git sync
- `apps/pageliner/`   — EPUB/PDF e-reader and document library

### NoteLiner

NoteLiner is a single-user outliner-style note-taking application, built with Electron and Svelte 5. It allows users to create and organize files in a hierarchical structure. Each file is written in Markdown with syntax highlighting, and all changes are automatically synced via Git.

NoteLiner should be considered BETA software. I wrote it for my own personal use, and it meets my specific needs.

### ThreadLiner

A desktop RSS reader built with Electron, Svelte, and Git-synced JSON. Supports RSS 2.0, Atom, and JSON Feed formats.

### PageLiner

An e-reader and document library for EPUB and PDF, with bookmarks, highlights, reading-position persistence, and optional Git sync of the library index and reading state.

## Packages

- `packages/desktop-ui/` — shared UI components, theme system, Electron host helpers, and the Tauri IPC shim.

## Rust crates

- `crates/marina-core/` — framework-independent backend shared by all apps: git sync, JSON stores, XDG paths, debouncing.
- `crates/marina-tauri/` — the Tauri host shared by all apps: the Electron-compatible IPC bridge, window chrome, UI prefs, dialogs, update check.
- `crates/marina-egui/` — the native (egui) counterpart of `desktop-ui` for the pure-Rust builds (plan Option C): themes, fonts, window chrome, command palette, settings, panes. Try it with `cargo run -p marina-egui --example gallery`.
- `crates/marina-richtext/` — HTML/XHTML rendering for the native builds (ThreadLiner articles, PageLiner EPUB).
- `crates/noteliner-core/`, `crates/threadliner-core/`, `crates/pageliner-core/` — each app's backend (ported from its `src/main/*.js`), independent of the UI shell so the native egui builds can reuse it.

## Prerequisites

- Node.js 20+ (Vite 6 prefers 22.12+; 20.x works with a warning)
- npm 9+

## Install

```bash
npm install
```

This installs all workspaces and links them via the root `package-lock.json`.

## Build

```bash
npm run build              # builds both apps (apps/<app>/dist/)
npm run build:noteliner
npm run build:threadliner
```

## Run in development

Both apps share the same dev orchestration (`scripts/dev.js` per app):
start Vite, wait for `Local:` in its output, then spawn Electron with
`NODE_ENV=development` so the main process loads the dev server URL
instead of the built file. Save in your editor → HMR.

```bash
npm run electron:dev -w noteliner       # Vite on 5250 + Electron
npm run electron:dev -w threadliner     # Vite on 5251 + Electron

# Root shortcuts:
npm run electron:noteliner
npm run electron:threadliner
```

The two apps use different Vite ports (5250 / 5251), so you can run them
side-by-side without a collision.

Renderer-only (no Electron window — useful when you just want to iterate
on Svelte components):

```bash
npm run dev -w noteliner       # or: npm run dev:noteliner
npm run dev -w threadliner     # or: npm run dev:threadliner
```

## Run a built app

After `npm run build`:

```bash
npm run start -w noteliner
npm run start -w threadliner
```

## Native build (egui)

The pure-Rust builds of plan Option C
([`docs/plans/plan-native-rust-egui.md`](docs/plans/plan-native-rust-egui.md)): egui UIs over the
same Rust backends, with no webview, for Linux, Windows and macOS. They share the Electron and Tauri
builds' settings and data folders. ThreadLiner came first, then PageLiner, then NoteLiner (its
first wave: the shell, file tree, dialogs and preview; the native editor, PDF export and slides
follow):

```bash
npm run native:threadliner     # cargo run -p threadliner-native --release
npm run native:pageliner       # cargo run -p pageliner-native --release
npm run native:noteliner       # cargo run -p noteliner-native --release
```

See [`apps/threadliner/native/README.md`](apps/threadliner/native/README.md),
[`apps/pageliner/native/README.md`](apps/pageliner/native/README.md) and
[`apps/noteliner/native/README.md`](apps/noteliner/native/README.md) for what's ported. These
are now *the* builds: their binaries are `threadliner`, `pageliner` and `noteliner` (the Cargo
packages keep the `-native` suffix), and their .deb/.rpm packages replace the Tauri and Electron
ones. `noteliner --mcp-bridge` is NoteLiner's MCP bridge. Packaging (deb, rpm, AppImage, Flatpak,
Windows installer, macOS .app/.dmg), the release workflow and how to roll back are in
[`docs/packaging.md`](docs/packaging.md).

## Native Linux builds (Tauri)

Each app has a `src-tauri/` crate that replaces the Electron main process
with Rust. The renderer and even `src/main/preload.js` are shared: at build
time the preload is bundled against `@marina/desktop-ui/tauri-shim`, which
stands in for the `electron` module, so `window.api` is identical in both
builds and every `ipcRenderer.invoke(channel, …)` reaches a Rust handler
with the same channel name.

Settings and data live in the same places as the Electron build
(`~/.config/<App>/…`, the same project/library/data folders, byte-identical
JSON and frontmatter), so you can switch between the two builds freely.

### Additional prerequisites

- Rust 1.92+ (`rustup`)
- Tauri's system libraries:

  ```bash
  sudo apt install build-essential pkg-config libwebkit2gtk-4.1-dev libssl-dev \
    libayatana-appindicator3-dev librsvg2-dev libxdo-dev patchelf
  ```

- `git` at runtime, as with the Electron build.

### Develop

```bash
npm run tauri:noteliner      # Vite on 5250 + the Tauri window, with HMR
npm run tauri:threadliner    # 5251
npm run tauri:pageliner      # 5253
```

### Test

```bash
cargo test --workspace
cargo clippy --workspace --all-targets
```

### Package

```bash
npm run tauri:build -w noteliner    # → target/release/bundle/{deb,rpm,appimage}/
npm run tauri:build -w threadliner
npm run tauri:build -w pageliner
```

Packages are 5–7 MB (vs. 100 MB+ for Electron) and depend on the system WebKitGTK. Since the
native cutover the Tauri binaries are `threadliner-tauri`, `pageliner-tauri` and
`noteliner-tauri` (installed as `/usr/bin/<app>-tauri`, packages `thread-liner` etc.); plain
`/usr/bin/<app>` belongs to the native packages, which conflict with and replace these. See
[`docs/packaging.md`](docs/packaging.md) for rolling back to them.

### NoteLiner's MCP server

In the Tauri build the `noteliner-tauri` binary is its own MCP bridge: point MCP
clients at `noteliner-tauri --mcp-bridge` (Settings → MCP shows the exact
snippet). No Node.js runtime is required.

## App icons

Each app's icon originates from a master SVG inside the app:

- `apps/noteliner/assets/icon.svg`
- `apps/threadliner/assets/icon.svg`

The pipeline from SVG to packaged app is:

1. `scripts/rasterize-icon.js` (headless Electron) renders each app's
   `assets/icon.svg` to a 512×512 `assets/icon.png` alongside it.
2. Each app's `build:icons` step (ImageMagick, in `apps/<app>/scripts/build-icons.sh`)
   pads that PNG into a square `apps/<app>/build/icon.png`.
3. `electron-builder` reads `build/icon.png` and derives the platform
   icons (`.icns`, `.ico`, multi-size PNG sets) at package time.

The Tauri builds use pre-generated sizes in `apps/<app>/src-tauri/icons/`;
regenerate them after changing `assets/icon.png` with
`npx tauri icon assets/icon.png -o src-tauri/icons` from the app folder
(then delete the `android/`, `ios/`, `Square*`, `StoreLogo` and `@2x`
outputs, which the Linux bundles don't use).

After editing either source SVG, regenerate the per-app PNGs:

```bash
npm run icons:rasterize                 # both apps
npm run icons:rasterize:noteliner       # one app
npm run icons:rasterize:threadliner
```

Commit the updated `apps/<app>/assets/icon.png` alongside the SVG change.
Step 2 runs automatically as part of `build:linux` / `build:win` / `build:mac` / `build:all`.
