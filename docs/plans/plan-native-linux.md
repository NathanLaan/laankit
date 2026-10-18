# Native Linux Apps — Migration Plan

Status: **Option A implemented (2026-10-03). Option C (pure Rust, egui)
accepted 2026-10-17 and being prototyped; see
[plan-native-rust-egui.md](plan-native-rust-egui.md).** Earlier decision
(2026-10-02): do A + B, completing all of A first. See
[Option A — implementation record](#option-a--implementation-record).

Converts NoteLiner, ThreadLiner and PageLiner from Electron + Svelte into
native Linux applications. Lays out the stack options, a recommendation, and
a phased migration that works for whichever option is picked.

## Current state

npm-workspaces monorepo, Electron 41 + Svelte 5 + Vite, all three apps on
top of `@marina/desktop-ui`.

| Unit | Size (JS/Svelte/CSS) | Scope | Hard parts to port |
|---|---|---|---|
| `apps/noteliner` | ~17.7k lines, ~60 IPC channels | Markdown outliner, git auto-sync | CodeMirror 6 editor, live preview, drag-and-drop file tree, attachments, templates, frontmatter, wikilinks/backlinks, search, slides, DOCX/PPTX import (mammoth/jszip/xmldom), PDF/HTML export (`printToPDF`), MCP server over a Unix socket plus a stdio bridge (`bin/noteliner-mcp-bridge.js`) |
| `apps/threadliner` | ~6.1k lines, ~45 IPC channels | RSS/Atom/JSON Feed reader, git-synced JSON | Feed poller and parser (`rss-parser`), sanitized HTML article view (DOMPurify), tags, sync manager |
| `apps/pageliner` | ~3.3k lines, ~15 IPC channels | EPUB/PDF library and reader | pdf.js with a text layer for highlights, epub.js CFI positions, annotations, git sync |
| `packages/desktop-ui` | ~2.9k lines | Shared chrome | Custom titlebar, 6 themes plus UI scale, command palette (fuzzy search), settings shell, PaneHost, window-state and secondary-window helpers |

### What makes the port easier

- **The IPC surface is a clean contract.** Each app's `preload.js` plus its
  `ipcMain.handle` channels already separate the backend from the UI, so the
  backend can be ported service by service.
- **Git is already native.** All three apps shell out to the system `git`
  through `execFile`. There's no JS git library to replace.
- **Data is plain files.** `noteliner.json` plus Markdown, ThreadLiner's JSON
  store, and `pageliner.json` plus `state/*.json` can stay byte-compatible.
  This matters because the repos sync across machines during a migration.
- The backend is small (~7.6k lines across `src/main`). Most of the work is
  the ~25k lines of Svelte UI.

### What needs a webview or a replacement in any truly native stack

- RSS article HTML and EPUB content *are* HTML, so they need an embedded web
  engine (WebKitGTK or QtWebEngine).
- The Markdown preview and slides render HTML today.
- `printToPDF` export needs a substitute.
- The Playwright e2e suite (12 NoteLiner specs, plus the desktop-ui visual
  tests) doesn't carry over to native UI.

## Stack options

### Option A — Tauri 2 (Rust backend, keep the Svelte UI)

The Electron main process becomes Rust `#[tauri::command]`s, and `preload.js`
becomes `@tauri-apps/api` `invoke` calls. Renderer code stays largely as it
is.

- **Pros**
  - Lowest risk and fastest: roughly 4–8 weeks for all three apps.
  - Bundles of ~10 MB instead of ~150 MB, and much lower memory use.
  - Keeps CodeMirror, pdf.js, epub.js and the themes.
  - Produces `.deb`, AppImage and Flatpak, and stays cross-platform.
- **Cons**
  - Not native UI: a native binary running a WebKitGTK webview.
  - WebKitGTK quirks compared with Chromium, mainly rendering and pdf.js
    performance.
  - No `printToPDF`, so export needs a Rust crate or a call to pandoc/typst.
  - The MCP server has to be rewritten (`rmcp`).
- **Best if:** "native" means a lean, installable Linux binary without
  Electron.

### Option B — Rust + GTK4/libadwaita (gtk-rs + Relm4)

A truly native GNOME app.

| Today | Replacement |
|---|---|
| CodeMirror 6 | GtkSourceView 5 (Markdown highlighting built in) |
| pdf.js | `poppler-rs` |
| epub.js, article HTML | WebKitGTK 6 view, sandboxed |
| `rss-parser` | `feed-rs` |
| DOMPurify | `ammonia` |
| `marked` | `comrak` or `pulldown-cmark` |
| `gray-matter` | `serde_yaml` |
| mammoth / jszip / xmldom | `docx-rs` / `zip` + `quick-xml` |
| system `git` via `execFile` | keep shelling out, or `git2` |
| MCP service | `rmcp` |
| `electron-updater` | Flatpak or distro updates |

- **Pros**
  - Real native look and feel: HeaderBar, adaptive layouts, system dark
    mode, accessibility, portals.
  - Fast enough for the large-vault performance work.
  - Shared code moves into a `marina-core` crate plus a `marina-ui` widget
    crate.
- **Cons**
  - A full UI rewrite: about 4–7 months for one developer.
  - The 6 custom themes have to fit libadwaita's styling model. CSS
    overrides are possible but discouraged.
  - The custom titlebar becomes a HeaderBar.
  - Linux-first: the Windows and macOS targets in `electron-builder.yml`
    effectively go away.
- **Best if:** first-class GNOME apps and long-term performance are the goal.

### Option C — Pure Rust (egui)

Added 2026-10-17, following ArtCraft's `artcraft-native`. Cross-platform
(Linux, Windows, macOS), reusing the Option A core crates. See
[plan-native-rust-egui.md](plan-native-rust-egui.md). It replaces the
Python option below, which previously carried the letter C.

### Withdrawn — Python + GTK4/libadwaita (PyGObject)

The same native result and widgets as B (GtkSourceView, Poppler via GI,
WebKitGTK), with `feedparser`, `markdown-it-py`, `python-docx`, `nh3`, and
the official MCP Python SDK.

- **Pros**
  - Roughly 40% faster to write than Rust.
  - Good Flatpak/Flathub support (builder templates, Blueprint UI files).
  - Very readable code.
- **Cons**
  - Slower on large vaults and search indexing (can be fixed later with a
    Rust extension via PyO3).
  - Packaging Python dependencies in Flatpak is tedious.
  - Weaker type safety.
- **Best if:** you want native GNOME apps soon and favour iteration speed.

### Option D — Qt 6 (C++ with QML, or Python with PySide6)

Native, KDE-flavoured, cross-platform.

- **Component mapping**
  - Editor: `QPlainTextEdit` with KSyntaxHighlighting, or KTextEditor for a
    full editor component.
  - PDF: `QtPdf`.
  - EPUB and articles: `QtWebEngine` (Chromium again, which adds back
    ~100 MB) or `QTextBrowser` for simple HTML.
- **Pros**
  - Mature, and stays cross-platform, so Windows and macOS can be kept.
  - QML handles custom themes well, so the 6 themes port cleanly.
- **Cons**
  - C++ is costly to write.
  - Licensing is LGPL/GPL with dynamic linking.
  - Looks out of place on GNOME desktops.
- **Best if:** you want native but still need Windows/macOS, or you're on
  KDE.

### Not recommended

- **Iced, Slint, egui:** (egui reconsidered as Option C; see above) text
  editing, rich text and HTML rendering are too
  weak for an outliner editor or an e-reader.
- **Avalonia (C#):** native-ish, but has no good EPUB or HTML story.
- **Wails (Go):** basically Tauri, with a weaker ecosystem.

## Recommendation

- **If the goal is "drop Electron, keep shipping":** Option A (Tauri 2).
  It's quick, low-risk and keeps the cross-platform builds.
- **If the goal is "genuinely native Linux apps":** Option B (Rust +
  GTK4/libadwaita + Relm4). It's the best long-term fit, and the backend logic
  maps one-to-one onto mature crates. The Python/GTK option was the
  alternative to B when speed of delivery beat runtime performance.

A and B can be combined: do A first, then reuse its Rust backend crates as
the core of B. No backend work is thrown away.

## Migration plan

Written for A or B. D follows the same shape; Option C has its own phases
in [plan-native-rust-egui.md](plan-native-rust-egui.md).

### Phase 0 — Lock the contract (~1 week)

- Write down each app's IPC channels and on-disk formats (`noteliner.json`,
  ThreadLiner's data store, `pageliner.json`, `state/<id>.json`) as
  versioned schemas.
- Add golden-file tests against real sample vaults, so the new apps can be
  proven to read and write identical data.
- Decide whether the Windows and macOS builds are dropped.

### Phase 1 — Shared foundation (~2–3 weeks)

- **`marina-core` crate:**
  - Git sync: commit, then rebase-pull, then push, with debounce. The logic
    is currently duplicated across `noteliner/src/main/git-service.js`,
    `threadliner/src/main/git-sync.js` and `pageliner/src/main/git-sync.js`.
  - Settings, window state, recent-projects list.
  - XDG paths. Keep `~/.config/NoteLiner` etc., so existing settings and the
    MCP bridge keep working.
- **`marina-ui`:**
  - Command palette, with fuzzy search ported from
    `packages/desktop-ui/src/lib/fuzzy.js`.
  - Settings window, theme and scale handling, sidebar/PaneHost, about
    dialog.

### Phase 2 — ThreadLiner as the pilot (~3–5 weeks)

- Mid-sized, and exercises the full shared stack: lists, sidebar, sync,
  settings, sanitized HTML view.
- Shakes out `marina-core` and `marina-ui` before the hardest app.

### Phase 3 — PageLiner (~3–4 weeks)

- PDF via Poppler, with highlight rectangles stored in the same
  zoom-independent page units.
- EPUB in WebKitGTK, keeping epub.js inside the webview so CFI positions and
  highlights stay compatible. A native EPUB renderer would break CFI
  compatibility.

### Phase 4 — NoteLiner (~8–12 weeks)

Port in this order:

1. Project and index service.
2. File tree with drag and drop.
3. GtkSourceView editor: auto-save, list-edit helpers (`lib/listEdits.js`),
   paste/drop attachments.
4. Preview.
5. Search, links and backlinks.
6. Tags, frontmatter and templates.
7. History panel.
8. Import (DOCX/PPTX).
9. Export (WebKitGTK print-to-PDF).
10. Slides/presentations.
11. MCP server. Keep the same socket and `mcp-runtime.json` protocol so
    existing client configs keep working.

### Phase 5 — Packaging and release (~2 weeks)

- Flatpak manifests (Flathub-ready), plus `.deb` and AppImage.
- `.desktop` files and AppStream metainfo, replacing
  `scripts/install-desktop.sh`.
- Replace `electron-updater` with distro or Flatpak updates.
- CI: build Flatpaks in GitHub Actions.
- Tests: unit and integration tests in Rust for core logic, plus a light
  AT-SPI (dogtail) smoke suite in place of Playwright.

### Parity checkpoint

Before retiring each Electron build, run the old and new versions of the app
against the same synced git repo and confirm the data round-trips unchanged.

## Open questions

- Are Windows and macOS builds still required? This decides between B/C
  (Linux-first) and A/D (cross-platform).
- Which of the 6 themes survive under libadwaita (Options B/C)?
- Is MCP client-config compatibility (socket path, bridge script) a hard
  requirement?

## Option A — implementation record

Completed 2026-10-03. All three apps run on Tauri 2 with Rust backends; the
Electron builds are untouched and still pass their suites (NoteLiner: 26
Playwright e2e specs, plus the import/slides/lists/MCP integration tests).

### Architecture

- **The renderer and preload don't fork.** Each app's `preload.js` is bundled
  against `@marina/desktop-ui/tauri-shim` (an `electron` stand-in) and
  injected as a webview init script. `ipcRenderer.invoke(channel, …)` lands
  in one Rust `ipc` command that routes by the same channel name, so the
  Electron IPC surface *is* the contract and nothing in the Svelte code
  knows which shell it runs in.
- **Backends are UI-independent crates**, ready for Option B:
  `marina-core` (git, JSON, paths, debounce), `noteliner-core`,
  `threadliner-core`, `pageliner-core`. Only `marina-tauri` and the
  `apps/*/src-tauri` crates depend on Tauri.
- **Data compatibility** with the Electron builds is exact: same
  `~/.config/<App>` folders, `JSON.stringify(…, null, 2)` output, and key
  order. Checked with differential tests against the JS code:
  - js-yaml 3 dump/load (gray-matter frontmatter): 3,000 fuzzed cases,
    0 mismatches.
  - rss-parser entry fields (guid, title, link, content, author, date): 286
    entries from 15 real feeds, 0 mismatches. GUIDs drive de-duplication,
    so a mismatch would resurface old entries as unread.
  - PPTX import: byte-identical Markdown.
  - DOCX import: matches mammoth except where noted below.

  The differential harnesses are kept as `#[ignore]` tests
  (`yaml_diff`, `dump_for_diff`, `import_diff`).

### Electron-only features and their replacements

| Electron | Tauri build |
|---|---|
| `printToPDF` | WebKitGTK `PrintOperation` to the GTK file printer (Letter, 0.5 in margins) |
| Chromium session spell checker | WebKitGTK `WebContext` spell checking, system locale |
| `protocol.handle('attachment')` | Tauri async URI scheme; URLs are now `attachment://localhost/<file>` (Tauri rejects an empty host; Electron accepts both) |
| `bin/noteliner-mcp-bridge.js` (needs Node) | `noteliner --mcp-bridge` (same runtime file and socket protocol) |
| `electron-updater` | Checks GitHub Releases and opens the release page; system packages update through the package manager |
| `render-process-gone` → reload | WebKit `web-process-terminated` → reload |
| `-webkit-app-region: drag` | `TitleBar` calls `window.api.windowStartDragging()` under Tauri |
| Wayland/Vulkan and suspend/resume workarounds | Not needed (Chromium-specific) |

### Findings

- **Bug in the Electron DOCX import.** `import-service.js` passes
  `transformDocument` and `convertImage` inside mammoth's *input* object
  instead of its options, so mammoth ignores both. Tables are not stripped
  (cells come out as loose paragraphs) and images are inlined as base64
  `data:` URIs rather than saved as attachments. The Rust importer
  implements the intended behaviour. The JS fix is a one-line move of those
  two keys into the second argument of `convertToMarkdown`.
- Quote-styled DOCX paragraphs render as `> ` blockquotes (mammoth glued
  their text onto the next block).
- Debian package names come from `productName` (`note-liner`, etc.), and
  the Electron `.deb` installs the same `/usr/bin` launcher, so remove one
  before installing the other.

### Not done in A (moved to B / Phase 5)

- **Flatpak.** Tauri needs offline cargo and npm vendoring inside
  flatpak-builder. Deferred to the GTK build, where Flathub is the main
  channel. `.deb`, `.rpm` and AppImage are built by `tauri build` and the
  CI job.
- **A native test runner for the UI.** The Rust crates are unit-tested
  (39 tests). UI flows were checked with the debug-only
  `MARINA_SMOKE_SCRIPT` hook, which runs a script in the main window and
  reports through the `debug:log` channel.

