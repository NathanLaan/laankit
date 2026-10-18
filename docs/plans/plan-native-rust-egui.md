# Option C — Pure Rust (egui) Apps

Status: **Accepted as Option C (2026-10-17). Phase 0 (spikes) passed on
the `laan/egui` branch, all five go. Phase 1 (`marina-egui`,
`marina-richtext`) done 2026-10-17. Phase 2 (ThreadLiner) built
2026-10-17 and in its week of daily use before cutover. Phase 3
(PageLiner) built 2026-10-17; Phase 4 (NoteLiner) next.** See
[Phase 0 results](#phase-0-results-2026-10-17),
[Phase 1 record](#phase-1-record-2026-10-17),
[Phase 2 record](#phase-2-record-2026-10-17) and
[Phase 3 record](#phase-3-record-2026-10-17). Follows ArtCraft's `artcraft-native` approach
for NoteLiner, ThreadLiner and PageLiner. Option C in
[plan-native-linux.md](plan-native-linux.md) points here; it replaces the
Python/GTK option that used to carry that letter.

## Decisions (2026-10-17)

1. This plan is **Option C**. Prototyping starts on the `laan/egui`
   branch.
2. **Targets: Linux, Windows and macOS.** Every dependency must build and
   ship on all three, which favours pure-Rust crates over system libraries
   (for example `hayro` over pdfium or Poppler). CI builds the spikes on
   all three.
3. **EPUB CFI compatibility is not required.** PageLiner can use a native
   EPUB reader with its own position format, and no webview is needed.
   Existing EPUB positions and highlights are reset (decided 2026-10-17).
4. **The Electron and Svelte builds stay as they are for now.**
5. **All 6 themes are kept.**

## Summary

ArtCraft (`storytold/artcraft`) shipped `artcraft-native`, a second desktop
app written entirely in Rust with egui. It has no Tauri and no JavaScript.
It sits next to the existing Tauri app, uses the same Tauri-free backend
crates, and reads the same data and credential files, so either app can be
used at any time. Features it hasn't ported yet show a "coming soon" page
instead of blocking the release.

Marina is already set up for this. Option A left the backends in
UI-independent crates (`marina-core`, `noteliner-core`, `threadliner-core`,
`pageliner-core`). Only `marina-tauri` and `apps/*/src-tauri` depend on
Tauri. This plan adds an egui front end on top of those crates for each app.

One earlier decision has to be reversed. `plan-native-linux.md` lists egui
under "Not recommended" because its text editing, rich text and HTML
support were too weak. ArtCraft shows that a polished, themed egui app is
practical, but it never had to render HTML or a PDF, or edit long
documents. Those are Marina's hard parts, so **Phase 0 is a set of
go/no-go spikes**, and nothing else starts until they pass.

## What ArtCraft did

ArtCraft repo: `artcraft-native/`, about 14k lines of Rust across 4 PRs
(#2067, #2068, #2079, #2115).

| Practice | How ArtCraft does it | Marina equivalent |
|---|---|---|
| Separate native crate, side by side | `artcraft-native/` workspace member; the Tauri app keeps shipping | `apps/<app>/native/` crates; the Tauri and Electron builds keep shipping until parity |
| Reuse Tauri-free backend crates | `artcraft_client`, `artcraft_router`, `tokens`, `enums` | `marina-core`, `<app>-core` (already done in Option A) |
| Shared on-disk state | Reads the Tauri app's cookie jar, `~/Artcraft/credentials`, `env_configs.json` | Same `~/.config/<App>` folders and byte-identical JSON, already guaranteed by the core crates |
| Backend thread + event queue | `backend::Backend` owns a tokio runtime; calls return `Event`s through an `mpsc` channel that the UI drains once per frame, then `ctx.request_repaint()` | Same pattern; git sync, feed polling, indexing and file I/O never block a frame |
| One app struct routes everything | `app.rs`: `handle_event` for backend results, `AppRequest`s from pages, overlays as `Option<…>` fields | Same, per app |
| Design tokens in one file | `theme.rs`: colour, radius, font and size constants copied from the web app's Tailwind tokens, with comments naming the source token | `marina-egui::theme`: the 6 desktop-ui themes as token sets read from `packages/desktop-ui/src/theme`, plus UI scale through `ctx.set_zoom_factor` |
| Own window chrome | Undecorated window; `ui/window.rs` handles drag, double-click maximize, edge resize and caption buttons; macOS keeps its traffic lights | Port of desktop-ui's custom titlebar into `marina-egui::window` |
| Bundled fonts and icons | Inter/Archivo/Geist Mono under the OFL, icons drawn as Lucide-style paths, `NOTICE` lists them | The same fonts desktop-ui uses, plus an icon module |
| Webview only where unavoidable | `wry` child webview only for the Midjourney sign-in, because its cookies can only be captured in a browser | Possibly article HTML and EPUB (see Phase 0); everything else native |
| Optional external tools, with a fallback | In-app video needs `ffmpeg` on the PATH, otherwise the video opens in the system player | `git` is required (already true); `pandoc`/`typst` could be optional for export |
| Lenient decoding | `backend::wire` structs tolerate unknown enum values | Already covered by the core crates' serde models |
| Unported features stay visible | `pages/other.rs` "coming soon" page with a link to the web tool | A "Not in the native build yet" panel that names the Tauri build |
| Headless UI tests | `egui_kittest` drives the prompt box by AccessKit role and label | Same for the file tree, editor helpers, feed list, palette and settings |
| Screenshot diagnostics | `ARTCRAFT_SCREENSHOT`, `ARTCRAFT_PAGE`, `ARTCRAFT_OPEN`, `ARTCRAFT_SCREENSHOT_EXIT` | `MARINA_SCREENSHOT`, `MARINA_VIEW`, `MARINA_OPEN`, `MARINA_SCREENSHOT_EXIT`; replaces the `MARINA_SMOKE_SCRIPT` hook for native builds |
| Parity tables in the README | Each feature is listed as Done / Done with caveat | One parity table per app, covering the IPC channel list from Phase 0 of the Linux plan |
| Renderer choice | eframe `glow`, because `wgpu-hal` clashed with the `windows` crate versions pinned for Tauri | Start with `glow` for the same reason, since Tauri crates share the workspace; revisit `wgpu` once Tauri is retired |

ArtCraft's dependency set is a reasonable starting point: `eframe`/`egui`
0.36 (`glow`, `accesskit`, `wayland`, `x11`, `persistence`), `egui_extras`
(images, SVG), `image`, `resvg`, `rfd` (file dialogs), `arboard` (clipboard
images), `open`, `directories`, `rodio`, and `egui_kittest` for tests.

## Why egui instead of GTK (Option B)

- **Themes.** The 6 custom themes map directly onto egui `Visuals` and a
  token module. Under libadwaita they'd fight the platform style.
- **Cross-platform comes back.** eframe runs on Windows and macOS, which
  answers the Linux plan's open question about dropping those targets.
- **One toolkit, no GObject.** No Relm4/gtk-rs learning curve, and no GTK
  runtime dependency outside Linux.
- **A proven local template.** ArtCraft's structure, chrome, tests and
  diagnostics can be copied almost unchanged.
- **Costs.** The UI won't look like GNOME. Accessibility through AccessKit
  is younger than GTK's. Text editing, HTML and PDF have to be built or
  sourced (Phase 0). There's no built-in spell checker or printing.

## Component mapping

| Today (Tauri/Svelte) | egui replacement | Confidence |
|---|---|---|
| desktop-ui titlebar, themes, scale | `marina-egui::{window, theme}` (port of ArtCraft `ui/window.rs`, `theme.rs`) | High |
| Command palette, `lib/fuzzy.js` | `marina-egui::palette` with a Rust port of `fuzzy.js` (or `nucleo-matcher`) | High |
| Settings shell, PaneHost, about | `marina-egui::{settings, panes}` with `egui::SidePanel`/`CentralPanel`, or `egui_tiles` | High |
| Toasts | Port of ArtCraft `ui/toast.rs` | High |
| Window state, secondary windows | eframe `persist_window` plus `ctx.show_viewport_deferred` | Medium |
| File tree with drag and drop | Custom tree widget using egui's dnd (`dnd_drag_source` / `dnd_drop_zone`) | Medium |
| CodeMirror 6 Markdown editor | `TextEdit::multiline` with a `layouter` that highlights Markdown (the approach of ArtCraft's `prompt_box/editor.rs`), plus `listEdits` ported as key handlers | **Spike**: performance on large files, IME, undo |
| Markdown preview, slides | `egui_commonmark` (pulldown-cmark), or a custom renderer over `comrak` to match today's output | Medium |
| Wikilinks, backlinks, search | Already in `noteliner-core` | High |
| PDF/HTML export (WebKit print) | Generate HTML with `comrak` and render the PDF with `typst` (as a crate or an optional binary), or a hidden `wry` webview's print | **Spike** |
| Spell check (WebKit) | `spellbook` (pure Rust Hunspell) with underline painting in the layouter | Medium, can come later |
| ThreadLiner article HTML (sanitized) | `ammonia`, then convert the HTML into native rich-text blocks (paragraphs, headings, lists, links, images, code), with "Open in browser" as a fallback | **Spike** |
| PageLiner PDF (pdf.js + text layer) | `hayro` (pure Rust, so it builds on all three platforms) for page bitmaps and text positions; `pdfium-render` with a bundled libpdfium per platform as the fallback | **Spike** |
| PageLiner EPUB (epub.js) | Native reader: `rbook`/`epub` for the container, XHTML converted into the same rich-text blocks as articles, positions stored as spine index + block + character offset | **Spike** |
| NoteLiner MCP server, `--mcp-bridge` | `noteliner-core::mcp`, unchanged; the native binary hosts the socket and the bridge flag | High |
| Update check | Move the logic from `marina-tauri::updates` into `marina-core` | High |
| Images in notes, covers, favicons | `egui_extras` image loaders plus a media cache (ArtCraft `backend/media_cache.rs` pattern) | High |

## Workspace layout

```
crates/
  marina-core/          # unchanged: git, JSON, paths, debounce (+ update check moved here)
  marina-tauri/         # unchanged until the Tauri builds retire
  marina-egui/          # NEW: theme, window chrome, palette, settings shell, panes, toasts, icons, fonts
  noteliner-core/ threadliner-core/ pageliner-core/   # unchanged, shared by both shells
apps/
  <app>/src-tauri/      # unchanged
  <app>/native/         # NEW crate `<app>-native`, binary `<app>-native`
    src/main.rs         # eframe::run_native, viewport, icon
    src/app.rs          # the app struct, event routing, saved prefs
    src/backend.rs      # tokio runtime wrapping <app>-core; Event enum
    src/views/…         # one module per screen or panel
    src/ui_tests.rs     # egui_kittest
```

- The binary is named `<app>-native` until cutover, so it can't collide with
  the `/usr/bin/<app>` installed by the Electron and Tauri packages.
- No crate in `crates/*-core` may depend on egui. This keeps the rule
  Option A set.
- `cargo run -p threadliner-native --release` is the developer entry point.
  Add `npm run native:<app>` shortcuts for symmetry.

## Phases

### Phase 0 — Spikes and decision (~2–3 weeks)

Each spike is a throwaway crate in `apps/spikes/<name>` with a
`RESULTS.md`. The phases below depend on them. Shared helpers (the eframe
launcher, `MARINA_SCREENSHOT`, timing stats) are in
`apps/spikes/common`. Measure with `--profile spike`: release
optimisation without the slow LTO build.

1. **Editor.** A 5,000-line Markdown file in `TextEdit` with a highlighting
   layouter. Pass if typing latency stays under 16 ms, IME works on Wayland
   and X11, and undo/redo and list continuation behave. Fallbacks:
   highlight only the visible range, or write a custom rope-backed editor
   widget, which adds about 3–4 weeks to Phase 4.
2. **Article HTML.** Render 50 real articles from the ThreadLiner test
   feeds through `ammonia` → blocks. Pass if 90% or more are readable
   without the "Open in browser" fallback.
3. **PDF.** `hayro` on real PDFs: render speed, text positions for
   selection and highlights, and mapping highlights to the current
   zoom-independent page units. Fall back to `pdfium-render` (with a
   bundled libpdfium per platform) only if `hayro` fails.
4. **EPUB.** A native reader: unzip and parse the book, convert each
   spine document into rich-text blocks (shared with spike 2), paginate or
   scroll, and save and restore a position (spine index + block +
   character offset).
5. **Export.** NoteLiner Markdown → PDF via `typst` against the current
   WebKit print output.

**Gate.** Record the go/no-go result in this file. If the editor or PDF
spike fails, keep that app on Tauri and port only the others. ArtCraft's
"coming soon" approach also allows shipping a native app with one
feature handed off to another build.

#### Phase 0 results (2026-10-17)

All five spikes passed. Details, numbers and how to reproduce them are in
each `apps/spikes/<name>/RESULTS.md`. Measured on Linux only; the CI
`spikes` job builds and tests them on Windows and macOS.

| Spike | Result | Key numbers | Carries into |
|---|---|---|---|
| 1. Editor (`TextEdit` + highlighting layouter) | **Go** | Typing p95 1.3–1.9 ms at 5k lines, ≤11.4 ms at 20k; fails at 50k (26–35 ms, inside `TextEdit` itself) | Phase 4. Needs a per-line galley cache: egui drops its paragraph cache after any idle frame, so the first keystroke after a pause would otherwise relayout the whole note (127 ms at 5k lines). Custom widget (4–6 weeks) only if >50k-line notes or screen-reader use on >10k-line notes matter. |
| 2. Article HTML (`ammonia` → blocks) | **Go** | 64/64 entries from 16 feeds readable; convert median 0.24 ms; first layout median 1.8 ms, 27 ms worst | Phase 2. Inline SVG, MathML and embeds become link-out cards. |
| 3. PDF (`hayro`) | **Go**, no pdfium | Open ≤7 ms; page at 1x/2x 6–8/10–23 ms; text 0.2–1.2 ms/page; output matches Poppler | Phase 3. Highlights stay in PageLiner's existing PDF-point units, no migration. Cap the page cache by bytes (~128 MB), not page count; write a ~100-line outline reader (hayro has no TOC API). |
| 4. EPUB (native, no webview) | **Go** | Open 2–18 ms; section layout median 0.6–7 ms, 41 ms worst | Phase 3. Scroll mode with chapter-crossing paging; positions are spine + block + char offset. |
| 5. Export (Typst in-process) | **Go** | 205 ms first export, 3.5 ms warm; better output than WebKit print (wikilinks, footnotes, code highlighting, repeated table headers) | Phase 4 step 9, behind a cargo feature. +43 MB binary (+14 MB compressed); split into a `noteliner-export` helper if that's too much. |

Changes to the later phases:

- **Fonts (Phase 1).** egui's default fonts have no bold, italic, CJK or
  emoji, and most non-Latin scripts are missing. `marina-egui` bundles a
  font family with real bold and italic faces plus a system-font
  fallback for missing glyphs. Typst export uses the same fonts.
- **Shared rich text (Phase 1).** `spike-richtext` becomes a
  `marina-richtext` crate for ThreadLiner articles, PageLiner EPUB and the
  NoteLiner preview. Open items: lay out long documents over several
  frames (15–40 ms today), selection across paragraphs (needed for EPUB
  highlights), row spans, and a wider EPUB CSS subset.
- **Accessibility.** With AccessKit on, each frame costs about 1.8 ms per
  1,000 editor lines even when idle. Phase 4 should give AccessKit only
  the visible text.
- **Shortcuts.** Match physical keys (as `Editor.svelte` does with
  `event.code`) so shortcuts work on non-US keyboard layouts.
- **Rust 1.92.** `hayro` and `typst` need it; the workspace minimum is now
  1.92.
- **Screenshots.** On Wayland, GNOME sends no frame callbacks while the
  session is locked or the window is hidden, so `MARINA_SCREENSHOT` never
  fires. Use `WAYLAND_DISPLAY=` (XWayland) or an unlocked session locally,
  and `egui_kittest` snapshots (wgpu, headless) in CI instead of a
  windowed screenshot job.
- **Manual checks still needed:**
  - Real IMEs: IBus/Fcitx on Wayland and X11, Windows TSF, macOS.
  - Screen readers: Orca, NVDA, VoiceOver.
  - Trackpad scrolling.
  - EPUB archive paths on Windows.

Unrelated bugs found along the way:

- **Electron PDF export drops images.** Chromium blocks `file://` images
  from a `data:` page. The Tauri build isn't affected.
- **YouTube feeds have no content field**, so their entries show no body
  in any build.

### Phase 1 — `marina-egui` foundation (~2–3 weeks)

- Port ArtCraft's `ui/window.rs` (drag, resize, caption buttons, macOS
  inset), `ui/toast.rs`, the icon approach and the font loading.
- `theme`: the 6 desktop-ui themes as token sets, light/dark, and UI scale.
- Fonts: a bundled family with bold and italic faces, plus a system-font
  fallback for CJK, emoji and other scripts.
- `marina-richtext`: promoted from `spike-richtext`, with progressive
  layout for long documents.
- Command palette with fuzzy search; settings window shell; pane host;
  about dialog; "not in the native build yet" panel.
- Diagnostics: `MARINA_SCREENSHOT` and related variables.
- An example binary, `marina-egui/examples/gallery.rs`, that shows every
  widget in every theme. This replaces the desktop-ui Playwright visual
  tests.
- Move the update check out of `marina-tauri` into `marina-core`.

#### Phase 1 record (2026-10-17)

Everything above is built. Run `cargo run -p marina-egui --example gallery`
to see it: the full app shell (title bar, toolbar, pane sidebar, command
palette, settings, About) and every widget, in any of the six themes.

- **`marina-egui`** (`crates/marina-egui`, see its README): `theme`
  (palettes generated from `theme/index.svelte.js`, same theme and scale
  ids), `fonts`, `icons` (Font Awesome codepoints generated from the npm
  package), `window` and `toast` (ported from ArtCraft, MIT OR Apache-2.0,
  credited in `NOTICE`), `titlebar`, `toolbar`, `commands` (registry and a
  `fuzzy_score` checked against `fuzzy.js` on about 1,000 pairs), `palette`,
  `settings`, `panes`, `about`, `not_ported` and `diagnostics`
  (`MARINA_SCREENSHOT`, `MARINA_THEME`, `MARINA_SCALE`). 41 tests, most of
  them kittest.
- **Commands return action values** of the app's own type instead of
  running closures, so the app applies them in its update loop with
  `&mut self`.
- **Fonts:** static Inter (Regular, SemiBold, Bold, Italic, Bold Italic)
  and JetBrains Mono (Regular, Bold, Italic), 3.3 MB with Font Awesome.
  Variable Inter was tried first, but egui shapes text about twice as
  slowly with it, which pushed opening a long article past 16 ms. Missing
  scripts and emoji fall back to system fonts through eframe's
  `system_fonts` feature.
- **`marina-richtext`** (`crates/marina-richtext`): the article and EPUB
  renderer, with progressive layout. The first frame is 11–16 ms whatever
  the document length (2,000 blocks: 12 ms, against 295 ms laying out
  everything). Pluggable `Resolver`s handle web and EPUB images and links.
  Its README lists what's still to do (cross-paragraph selection, row
  spans, more EPUB CSS).
- **`marina-core::updates`**: the GitHub release check, now shared; the
  Tauri updater uses it.
- **CI:** the `native` job lints and tests `marina-core`, `marina-egui`,
  `marina-richtext` and the spikes on Linux, Windows and macOS.
- **egui 0.37 deprecations** to follow in app code: `Id::new` → `Id::unique`
  or `IdSalt`, `ui.id()` → `ui.scope_id()`, `WidgetType` → `Role`. kittest
  harnesses need `.with_context_setup(marina_egui::fonts::install)`.

### Phase 2 — ThreadLiner pilot (~3–4 weeks)

The smallest UI and the most egui-friendly one: lists, sidebar, settings
and the article view.

- `backend.rs` wraps the `threadliner-core` poller, store and sync engine,
  and sends `Event`s.
- Feed and tag sidebar, entry list (virtualized with `ScrollArea::show_rows`),
  article view from spike 2, read/unread, and sync status.
- Parity table in `apps/threadliner/native/README.md`.
- **Exit criteria:** the parity checkpoint (below) passes for a week of
  daily use, and the kittest suite covers subscribe, read, tag and sync.

#### Phase 2 record (2026-10-17)

`apps/threadliner/native` (`threadliner-native`, about 4,600 lines):
`cargo run -p threadliner-native --release`, or `npm run
native:threadliner`. Its README has the architecture table, a parity table
against every IPC channel and Svelte feature, and a `demo_data` example
that fills a temporary config and data folder from real feeds.

- **Parity:** feeds, entries, tags, settings (theme and scale mirrored to
  the synced settings), the poller with notifications (`notify-rust`),
  setup, the remote-sync dialog and every `git:*` operation, Help, About
  with the update check, all shortcuts and a command palette.
- **Partial:**
  - Help is a dialog rather than a second window.
  - The article header doesn't scroll with the body.
  - Text selection works only within one paragraph.
  - Window geometry lives in eframe's storage, not the Tauri
    window-state file.
- **Not yet:** single-instance.
- **Shared data:** the same `ThreadLiner` config folder and data folder,
  byte-compatible JSON (tested). `marina_core::paths` now matches
  Electron's `userData` on Windows (`%APPDATA%`) and macOS
  (`~/Library/Application Support`), not only Linux.
- **Shared crates gained:**
  - `marina_egui::dialog`: modal shells, inputs, buttons and `confirm`.
  - `settings::select` and `theme_list_among`.
  - `ToolbarButton::attention`.
- **`rfd` pinned to 0.15:** `tauri-plugin-dialog` turns on rfd 0.16's
  `gtk3` feature, which conflicts with `xdg-portal`, so `threadliner-native`
  has no GTK dependency.
- **Tests:** 9, including a kittest flow against a local HTTP feed and a
  git temp folder (setup → add feed → read → mark unread → tag → assign),
  checked against the files on disk.
- **Still to check by hand before cutover:**
  - Notifications and the folder dialog on Windows, macOS and KDE.
  - The undecorated window on Windows and macOS.
  - Sync against a real remote.
  - Non-US keyboard layouts.
  - A week of daily use alongside the Tauri build on the same data (the
    parity checkpoint).

### Phase 3 — PageLiner (~3–5 weeks)

- Library grid with covers (media cache pattern), metadata and reading
  state from `pageliner-core`.
- PDF reader from spike 3: page cache, zoom, highlights and annotations in
  the existing units.
- EPUB reader using the spike 4 result.

#### Phase 3 record (2026-10-17)

`apps/pageliner/native` (`pageliner-native`, about 4,800 lines) plus the
`crates/pageliner-epub` reader crate. Run it with
`cargo run -p pageliner-native --release` or `npm run native:pageliner`.
Its README has the architecture and parity tables and a `demo_data`
example.

- **Library:**
  - All/EPUB/PDF filter, sorting, and a cover grid (covers decoded off-thread).
  - Import by dialog or by dropping files on the window; remove with confirm.
  - Settings, Git Sync (8 s auto-push, as in the Tauri host), About with the
    update check, every shortcut, and a Ctrl+K palette that also finds books.
- **PDF reader** (`hayro`):
  - Two render workers per book and a 128 MB texture cache.
  - Continuous scroll, zoom, fit width and a page field.
  - A Contents pane from the PDF outline, read by our own parser, plus
    Annotations and Search panes.
  - Text selection and copy; highlights and bookmarks stored exactly in the
    Svelte format and PDF units. Existing highlights carry over.
- **EPUB reader** (`pageliner-epub`, no webview):
  - Continuous scroll with paging, TOC, text size, reading themes, and a
    serif/sans toggle (Source Serif 4 is now bundled in `marina-egui`; fonts
    total about 4.2 MB).
  - Bookmarks; highlights with selection across paragraphs.
  - Positions reset as decided. Everything is stored under a `native` key,
    so the Tauri build's `cfi`/`bookmarks`/`highlights` are never touched
    (tested against `pageliner-core`'s merge).
- **marina-richtext gained:**
  - Selection across blocks with copy; highlight marks with click detection.
  - Table row spans.
  - More EPUB CSS: indent, margins, justify (drawn ragged-right, because
    egui spaces glyphs rather than words), small caps, sup/sub, image widths.
  - The first-frame performance is unchanged (11–16 ms).
- **Partial / not yet:**
  - Single-instance.
  - Window geometry is kept in eframe's storage.
  - A failed Sync Now shows only the error.
  - EPUB paging keys may still act while a dialog without a text field is
    open.
- **Tests:** 18 in the app (including a kittest flow: import a generated PDF
  → highlight → reopen with it restored → remove) and 8 in `pageliner-epub`
  (select → highlight → reopen, bookmarks, TOC, state merge).
- **Still to check by hand:**
  - The file dialog and drag and drop on Windows, macOS and KDE.
  - Git Sync against a real remote.
  - PDFs with unusual outlines or encryption.
  - Trackpad zoom at HiDPI.
  - Drag-select auto-scroll and Ctrl+C in EPUBs.
  - Heavily styled or fixed-layout EPUBs.

### Phase 4 — NoteLiner (~8–12 weeks, plus 3–4 if a custom editor is needed)

Same order as the Linux plan:

1. Project and index.
2. File tree with drag and drop.
3. Editor: auto-save, list edits, paste/drop attachments with `arboard`
   for clipboard images.
4. Preview.
5. Search, links and backlinks.
6. Tags, frontmatter and templates.
7. History.
8. Import.
9. Export.
10. Slides.
11. Hosting the MCP server and `--mcp-bridge`.

Spell check can follow as 4b.

### Phase 5 — Packaging and cutover (~2 weeks)

- `cargo-deb`, `cargo-generate-rpm` and AppImage. A Flatpak is now simple
  because there's no npm vendoring, only `flatpak-cargo-generator`.
  `.desktop` files and AppStream metainfo.
- CI: add `cargo test -p <app>-native` (kittest runs headless) and
  kittest snapshot images uploaded as artifacts. A windowed
  `MARINA_SCREENSHOT` job won't work on headless runners.
- Cutover per app: rename the binary to `<app>`, mark the Tauri package as
  replaced, and later remove `apps/<app>/src-tauri`, then `marina-tauri`
  and finally the Svelte renderer once all three apps have moved.
- Windows (MSI or NSIS via `cargo-packager`) and macOS (`.app`/`.dmg`,
  signed and notarised) builds from the same crates, in a CI matrix.

### Parity checkpoint

This is unchanged from the Linux plan: before each cutover, run the old
and new builds against the same git-synced data and confirm round trips
with no diff. The core crates' differential tests already cover the
formats, so this mainly checks UI flows.

## Testing

- **Core crates.** Existing unit and `#[ignore]` differential tests,
  unchanged.
- **UI.** `egui_kittest` harnesses per view, following
  `artcraft-native/src/prompt_box/ui_tests.rs`. Query by AccessKit role and
  label, which also catches missing accessible labels. One quirk from that
  file: fonts installed during the harness's first frame only take effect
  on the next frame.
- **Visual.** `MARINA_SCREENSHOT` for each view and theme in CI, compared
  by eye in PR review (as ArtCraft does), and later with a pixel-diff
  threshold.

## Conventions to borrow

- ArtCraft's `AGENTS.md` file-layout rules: constants first, outer types
  before inner types, constructors and then public methods and then
  private helpers, helpers below their callers.
- Doc comments on every token and constant that name where the value
  comes from (for example, "`base-fg/60`: secondary text").
- Use `log` macros only, never `println!`, outside tests.
- A README per native app with an architecture table (module → what it
  does) and a parity table.

## Risks

| Risk | Mitigation |
|---|---|
| egui `TextEdit` doesn't scale to long notes | Spike 1; visible-range highlighting; custom editor widget as a costed fallback |
| HTML content (articles, EPUB) looks worse than in a browser | Block renderer for most cases, "Open in browser" fallback, `wry` webview for EPUB if needed |
| `hayro` is young and may mis-render some PDFs | Spike 3 against real PDFs; pdfium (BSD-licensed, bundled per platform) as the fallback |
| Accessibility is weaker than GTK/WebKit | AccessKit is enabled; kittest queries enforce labels |
| Two UI stacks during the migration | Strict "no egui in `*-core`" rule; the Tauri builds receive no new features once a native app reaches parity |
| egui updates break things (0.x releases) | Pin minor versions in `[workspace.dependencies]`; upgrade all apps together |

## Later decisions (2026-10-17)

- **Option B (GTK4/libadwaita) is dropped.** Option C replaces it.
- **EPUB reading positions and highlights are reset** when PageLiner moves
  to the native reader. No CFI conversion is attempted.

## Rough total

About 20–30 weeks for one developer, including the spikes. ThreadLiner
could ship natively about 7–10 weeks in, and NoteLiner is the critical
path.
