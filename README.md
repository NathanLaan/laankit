# LaanKit

LaanKit is a set of desktop apps that share one UI and backend foundation. Every app is built in pure `Rust`, using [egui](https://github.com/emilk/egui)/[eframe](https://github.com/emilk/egui/tree/master/crates/eframe), and stores its data as plain files you control: a git repository for the notes, feed, and library apps, amd any folder for BlockLiner.

## Apps

| App | Binary | What it is |
|-----|--------|------------|
| **NoteLiner**   | `noteliner`   | Outliner-style Markdown notes: a tree of notes with editor, preview, tags, wikilinks, backlinks, slides, PDF export and an optional MCP server for AI clients. |
| **ThreadLiner** | `threadliner` | RSS reader (RSS 2.0, Atom, JSON Feed) with git-synced JSON. |
| **PageLiner**   | `pageliner`   | EPUB/PDF e-reader and document library with bookmarks, highlights and reading-position persistence. |
| **BlockLiner**  | `blockliner`  | Visio-style diagram editor for software components and architecture, saved as Mermaid flowcharts, with PNG/SVG export. |

Each lives in `apps/<app>/native/` and is a Cargo package named `<app>-native`. See the per-app README (`apps/<app>/README.md`) and its detailed `apps/<app>/native/README.md`.

## Layout

| Path | What it is |
|------|------------|
| `apps/<app>/native/`         | The four apps (egui UIs over the crates below). |
| `crates/laankit-core/`       | Shared backend: git sync, JSON stores, XDG paths, debouncing. |
| `crates/laankit-egui/`       | Shared egui shell: themes, fonts, window chrome, command palette, settings, panes. |
| `crates/laankit-editor/`     | The Markdown editor widget (own text engine, highlighting, spell check). |
| `crates/laankit-richtext/`   | HTML/XHTML rendering (ThreadLiner articles, PageLiner EPUB). |
| `crates/<app>-core/`         | Each app's UI-independent backend. |
| `crates/noteliner-export/`   | In-process PDF export (Typst). |
| `crates/noteliner-slides/`   | Presentations. |
| `crates/pageliner-epub/`     | EPUB parsing/state. |
| `crates/blockliner-core/`    | The diagram model, Mermaid read/write, layout. |
| `packaging/`                 | Linux desktop/metainfo files and Flatpak manifests. |
| `scripts/`, `xtask/`         | Packaging helpers and the icon pipeline. |
| `docs/`                      | Packaging guide and historical plans. |

See [`ARCHITECTURE.md`](ARCHITECTURE.md) for how these fit together, and [`SECURITY.md`](SECURITY.md) for the security model and how to report a vulnerability.

## Prerequisites

- **Rust 1.92+** (install with [`rustup`](https://rustup.rs)).
- **`git`** on the `PATH` at runtime — NoteLiner, ThreadLiner and PageLiner shell out to it for sync and history. BlockLiner doesn't use git.
- **Linux build libraries:**

  ```bash
  sudo apt install build-essential pkg-config \
    libxkbcommon-dev libfontconfig1-dev libssl-dev \
    libxcb-render0-dev libxcb-shape0-dev libxcb-xfixes0-dev
  ```

  macOS and Windows need only the Rust toolchain. The runtime libraries the Linux packages depend on are listed in [`docs/packaging.md`](docs/packaging.md).

## Development

```bash
cargo run -p noteliner-native       # or pageliner-native / threadliner-native / blockliner-native
```

For an optimized run without the slow full-LTO release build (good for benchmarking or daily use from source):

```bash
cargo run -p noteliner-native --profile fast-release
```

Diagnostics shared by every app (open a project, take a screenshot, force a theme or scale) are documented in each app's `native/README.md`.

## Build Release Package

```bash
# Build an optimized NoteLiner binary at target/release/noteliner.
# For another app, swap in its package: threadliner-native, pageliner-native
# or blockliner-native (each binary is named after its app, e.g. target/release/pageliner).
cargo build --release -p noteliner-native
```

The `release` profile uses full LTO (smaller, slower to build). To try it without installing, run the binary directly from `target/release/`.

## Build Installation Packages

Packages are produced by `scripts/package-native.sh`, which builds the release binaries and wraps them per format. Install the packaging tools once:

```bash
cargo install --locked cargo-deb cargo-generate-rpm cargo-packager
```

Then:

```bash
# package every app:
scripts/package-native.sh
# package a single app:
scripts/package-native.sh noteliner
# package a single app in the specified format:
FORMATS="deb appimage" scripts/package-native.sh
```

Output goes to `target/packages/<app>/`. Host defaults are `deb rpm appimage` on Linux, `nsis` on Windows (run from Git Bash), `app dmg` on macOS. The full format table, CI release workflow, versioning (`scripts/set-version.sh`), runtime dependencies, Flatpak and code-signing status are in [`docs/packaging.md`](docs/packaging.md).

## Release Installation

Pre-built packages for Linux, Windows and macOS are attached to each [GitHub release](https://github.com/NathanLaan/laankit/releases).

- **Linux**
  - `.deb`: `sudo apt install ./noteliner_<ver>-1_amd64.deb`
  - `.rpm`: `sudo dnf install ./noteliner-<ver>-1.x86_64.rpm`
  - AppImage: `chmod +x noteliner_<ver>_x86_64.AppImage && ./noteliner_<ver>_x86_64.AppImage`
- **Windows** — run the `_x64-setup.exe` installer (per-user, no admin). Current builds are unsigned, so SmartScreen warns on first run: **More info → Run anyway**.
- **macOS** — open the `.dmg` and drag the app to `/Applications`. Current builds are unsigned, so the first launch is blocked: right-click → **Open → Open**, or `xattr -d com.apple.quarantine /Applications/<App>.app`.

`git` must be installed on the target machine for sync and history to work (not needed for BlockLiner).

## Tests and Linting

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

UI tests run headless via [`egui_kittest`](https://github.com/emilk/egui): they drive widgets through AccessKit and never open a real window. See each app's `ui_tests*` modules.

## Icons

Each app's packaged icons are generated from `apps/<app>/assets/icon.png` by `cargo run -p xtask -- icons` (into the git-ignored `packaging/generated/`). The master source is `apps/<app>/assets/icon.svg`; regenerate `icon.png` from it with any SVG rasterizer, e.g. `rsvg-convert -w 512 -h 512 assets/icon.svg -o assets/icon.png`.

## License Info

[`MIT LICENSE`](LICENSE). Bundled third-party fonts and assets are listed in [`NOTICE`](NOTICE).
