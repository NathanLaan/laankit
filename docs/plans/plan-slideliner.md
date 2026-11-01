# SlideLiner — Markdown Presentation App

Status: **not selected (2026-08-10).** Shelved in favour of folding presentations into NoteLiner — see `apps/noteliner/docs/plans/plan-presentations.md`. Nothing implemented.

## Decision

Recorded 2026-08-10. Presentations are being built **inside NoteLiner** instead. Three reasons:

1. **The corpus already exists.** iA Presenter's own thesis is that the talk *is* the document. A separate app inverts that — you start a new document in a different app, which is the friction the idea exists to remove. In NoteLiner, an existing note becomes a deck by adding one frontmatter block.
2. **Dropping iA file compatibility removed this plan's main advantage.** That requirement was the thing genuinely easier in a greenfield codebase with its own document model and tab-indent parser. Without it, this plan mostly re-implements what NoteLiner ships: CodeMirror editor, `marked` renderer, attachment protocol, templates, PDF/HTML export, secondary-window helper, git sync.
3. **Integration is reversible; this isn't.** The modules the integrated plan builds (`lib/slides.js`, `lib/layout.js`, `Slide.svelte`, `deck-export-service.js`) are exactly what a standalone SlideLiner needs, and would be proven against real decks by then. If presentations outgrow NoteLiner, lift them into a `@laankit/slides` package and build this app around them. Building the app first means owning two Markdown editors and a vault split across two tools.

The honest cost of the other direction, for the record: NoteLiner's sidebar already stacks five panes, and a presentation tool really wants a thumbnail rail plus a large stage rather than a sixth pane. What makes it survivable is that the **detached presenter window is the large stage**, so the main window never has to become a deck editor.

This document stays on the shelf rather than being deleted — if presentations outgrow NoteLiner, §4 (architecture) and §5 (phases) remain the plan, with the then-existing NoteLiner modules as the starting point.

A fourth laankit app: write a talk as one Markdown document, get slides, speaker notes, a presenter view, and PDF / HTML / PPTX export. Modelled on [iA Presenter](https://ia.net/presenter).

---

## 1. Name

**SlideLiner** — recommended. Fits the `-Liner` family (NoteLiner, ThreadLiner, PageLiner), says exactly what it makes, and reads fine as a WM_CLASS / product name (`--class=SlideLiner`, `com.slideliner.app`).

Alternates, if a different emphasis is wanted:

| Name | Emphasis | Note |
|------|----------|------|
| **SlideLiner** | The output (slides) | Recommended — clearest, most searchable |
| **DeckLiner** | The artifact (a deck) | Good; slightly more jargon-y |
| **TalkLiner** | The narrative (iA's own framing) | Truest to the philosophy, least obvious in a launcher |
| **StageLiner** | The delivery | Ambiguous (stage = deploy?) |

The rest of this document assumes `slideliner`.

---

## 2. What iA Presenter actually is

Worth being precise, because the interesting part is a *constraint*, not a feature list. iA Presenter is not a slide editor with a Markdown import. It is a **text editor that projects a subset of the text**.

Four ideas carry the whole product:

1. **One document is both the talk and the slides.** You write prose. What the audience sees is derived from it. There is no separate notes field.
2. **Visibility is a syntax, not a mode.** In iA Presenter, tab-indented blocks are shown on the slide; unindented prose is speaker-only (teleprompter). Headings and tables are always visible.
3. **No manual layout.** The number and kind of visible blocks on a slide picks the layout (single / split / grid) automatically. The user cannot drag a text box, on purpose.
4. **The deck is a byproduct.** The same document exports as a slide PDF, a readable handout, a web page, or PPTX.

Everything else — themes, media manager, timers — is ordinary desktop-app surface area we already know how to build.

### Syntax summary (as documented by iA)

| Construct | Syntax | Visible to audience |
|---|---|---|
| Slide break | `---` on its own line (or 3× Return) | — |
| Headings | `#` … `######` | Always |
| Visible paragraph / list / quote | Tab-indented block | Yes |
| Speaker notes | Unindented prose | No (teleprompter only) |
| Comment | `// text` | No, and excluded from handouts |
| Tables | Pipe tables | Always |
| Images | `![alt](file.png)` | Yes |
| Video | Local file or YouTube link | Yes |
| Inline styling | `**b**`, `*i*`, `~~s~~`, `==highlight==`, `^sup^`, `~sub~` | Inherits block |
| Math | `$…$`, `$$…$$` (KaTeX) | Inherits block |
| Footnotes | `[^id]` / `[^id]: …` | Handout only |

Sources: [iA Presenter](https://ia.net/presenter), [Markdown guide](https://ia.net/presenter/support/basics/markdown), [support index](https://ia.net/presenter/support).

### Judgement call on the tab rule

The tab-indent rule is elegant and gives us **file compatibility with iA Presenter documents**, which is worth having. It is also the single most surprising thing about the app, and hostile to editors that convert tabs to spaces.

Recommendation: implement the tab rule as the default, and mitigate rather than replace it —

- CodeMirror gutter marker showing, per line, `visible` vs `notes`;
- `Tab` / `Shift-Tab` bound to "show/hide this block on the slide" with a toolbar button and `Ctrl+Shift+V`;
- an app-level setting `visibilityMode: 'tab' | 'explicit'`, where `explicit` treats *all* body text as visible and requires `// ` or a `> notes:` fence for speaker notes. Ship `tab` first; `explicit` is a Phase-6 nicety only if the tab rule proves annoying in real use.

---

## 3. Feature list

Tiered. **M** = MVP (must ship for the app to be worth opening), **C** = core (what makes it a credible iA Presenter alternative), **L** = later.

### 3.1 Editor

| # | Feature | Tier |
|---|---|---|
| E1 | CodeMirror 6 Markdown editor, same setup as NoteLiner (`@codemirror/lang-markdown`, one-dark/light themes, spellcheck toggle) | M |
| E2 | Slide-break awareness: `---` inserted by `Ctrl+Enter`; 3× Return auto-inserts a break | M |
| E3 | Visibility gutter (visible-on-slide vs notes) + `Tab` toggle + `Ctrl+Shift+V` | M |
| E4 | Slide thumbnail rail (left), click-to-scroll, drag-to-reorder slides (rewrites the Markdown) | C |
| E5 | Inline formatting shortcuts (bold/italic/highlight) — lift `toggleMarker()` from NoteLiner's `Editor.svelte` | M |
| E6 | Drag-and-drop / paste image → copies into deck media folder, inserts `![](media/x.png)` | M |
| E7 | Table insert/edit helper; code fence with syntax highlighting | C |
| E8 | Markdown / `.txt` import; PPTX import (reuse NoteLiner's `import-service.js` almost verbatim) | C |
| E9 | Outline pane (headings) + search pane, via shared `PaneHost` | C |
| E10 | Chart blocks (```chart fenced spec → rendered chart) | L |

### 3.2 Slide rendering / auto-layout

| # | Feature | Tier |
|---|---|---|
| R1 | Parse document → slide model (breaks, visible blocks, notes, headings) | M |
| R2 | Live slide preview at a chosen aspect ratio (16:9, 4:3, 16:10) | M |
| R3 | Auto-layout engine: 1 block → hero; 2 blocks → split; 3–4 → grid; heading+body → title/body; image-only → full-bleed | M |
| R4 | Fit-to-container by `transform: scale()` on a fixed-size stage, so one CSS layout serves preview, presenter, projector, and export | M |
| R5 | Image/video blocks (local video + YouTube embed) | C |
| R6 | KaTeX math, footnotes, definition lists | L |
| R7 | Per-slide overrides via a small directive comment (`<!-- layout: grid -->`, `<!-- bg: #111 -->`) — escape hatch, not a layout editor | C |

### 3.3 Themes

| # | Feature | Tier |
|---|---|---|
| T1 | 3–4 built-in themes as CSS-variable sheets (Dark, Light, Serif, Mono) | M |
| T2 | Theme controls: font family/size scale, accent colour, header/footer text, slide numbers | C |
| T3 | Progressive accent shift across the deck (iA's signature default) | C |
| T4 | Custom theme = a `theme.css` in the deck folder, loaded into the sandboxed stage | L |

### 3.4 Presentation

| # | Feature | Tier |
|---|---|---|
| P1 | Presentation mode: fullscreen slide window on a chosen display (`screen.getAllDisplays()`) | M |
| P2 | Presenter (speaker) view in a second window: current slide + next slide + teleprompter notes + elapsed timer | M |
| P3 | Keyboard/remote navigation (arrows, PageUp/Dn, space, `Esc`, `b` for black, `g` goto) | M |
| P4 | Teleprompter: auto-scrolling notes, adjustable size, bold cue emphasis | C |
| P5 | Timer/stopwatch, per-slide pacing target, over-time warning | C |
| P6 | Presenter tools: laser pointer / draw overlay, slide grid jump (`Ctrl+G`) | L |
| P7 | Windowed "share this window" mode sized for Zoom/Meet screen-share | C |

### 3.5 Export & sharing

| # | Feature | Tier |
|---|---|---|
| X1 | PDF — slides, one per page (hidden `BrowserWindow` + `printToPDF`, same as NoteLiner `main.js:1022`) | M |
| X2 | PDF — handout (slide + its notes, readable document flow) | C |
| X3 | HTML — self-contained single file with inlined CSS + base64 media | C |
| X4 | PPTX — via `pptxgenjs` (v4.0.1), native text boxes + images so slides stay editable | C |
| X5 | PPTX — image-per-slide fallback for pixel fidelity ("Export PPTX (images)") | C |
| X6 | PNG/JPG image sequence | L |
| X7 | Markdown export (strip notes / keep notes variants) | M |
| X8 | Static-site publish (folder of HTML) | L |

### 3.6 App shell (all from `@laankit/desktop-ui`, mostly free)

| # | Feature | Tier |
|---|---|---|
| S1 | Custom titlebar, theme system, UI scale, tabbed settings | M |
| S2 | Command palette with app commands | M |
| S3 | Status bar (slide count, current slide, word count, est. duration) | M |
| S4 | Recent decks / open screen (NoteLiner's `OpenScreen.svelte` pattern) | M |
| S5 | Optional Git sync of the deck folder (PageLiner/ThreadLiner model) | C |
| S6 | Autosave + local file history | C |
| S7 | Help window (secondary window + `helpContent.js` pattern) | C |

**Explicitly out of scope:** drag-to-position layout editing, animations and slide transitions beyond a cross-fade, real-time collaboration, cloud accounts. These are the things iA Presenter deliberately refuses, and the constraint is the product.

---

## 4. Architecture

### 4.1 Where it sits in the monorepo

```
apps/slideliner/
├── assets/icon.svg                # → icon.png via scripts/rasterize-icon.js
├── package.json                   # main: src/main/main.js
├── vite.config.mjs                # port 5254; inputs: index.html, presenter.html, stage.html
├── scripts/{dev.js,build-icons.sh} # copied from apps/pageliner
├── docs/plans/…
└── src/
    ├── main/
    │   ├── main.js                # window + IPC wiring
    │   ├── deck-service.js        # open/create/save deck, media folder, recents
    │   ├── media-service.js       # media:// protocol, import + dedupe by hash
    │   ├── export-service.js      # pdf / html / pptx / png
    │   ├── present-service.js     # display enumeration, stage + presenter windows, state bus
    │   ├── import-service.js      # md / pptx  (port of noteliner's)
    │   ├── git-sync.js            # copy of pageliner's
    │   └── preload.js             # bundled by esbuild → dist/preload.cjs
    └── renderer/
        ├── index.html / main.js / App.svelte        # editor window
        ├── presenter.html / presenter.js           # speaker view
        ├── stage.html / stage.js                   # audience/projector window
        ├── lib/
        │   ├── slide-parser.js     # ← the core. pure, no DOM, unit-testable
        │   ├── layout.js           # block set → layout class
        │   ├── render.js           # slide model → HTML (marked + extensions)
        │   └── themes/*.css
        ├── components/
        │   ├── Editor.svelte  ThumbnailRail.svelte  SlidePreview.svelte
        │   ├── Slide.svelte   Toolbar.svelte        StatusBar.svelte
        │   ├── ThemePane.svelte  ExportModal.svelte  DisplayPicker.svelte
        │   └── SettingsModal.svelte  OpenScreen.svelte
        └── stores/deck.svelte.js   # $state deck + derived slides
```

Ports so far: 5250 NoteLiner, 5251 ThreadLiner, 5252 playground, 5253 PageLiner → **5254 SlideLiner**.

### 4.2 The slide model

`slide-parser.js` is the heart of the app and the thing to get right first. Pure function, no Electron, no DOM — so it is trivially testable with plain Node assertions like `tests/integration/pptx-import.test.js`.

```js
// parse(markdown, { visibilityMode }) → Deck
Deck  = { meta, slides: Slide[] }
Slide = {
  index, sourceRange: { fromLine, toLine },
  blocks: Block[],          // audience-visible, in order
  notes:  Block[],          // teleprompter
  layout: 'hero'|'title-body'|'split'|'grid'|'full-bleed'|'quote'|'table',
  directives: { layout?, bg?, class? },
  title: string|null,       // first heading, for thumbnails/outline/PPTX
}
Block = { kind: 'heading'|'para'|'list'|'quote'|'code'|'table'|'image'|'video'|'math',
          level?, html, text, src?, alt? }
```

Design notes:

- `sourceRange` gives two-way binding for free: caret line → current slide (thumbnail highlight, preview follow), thumbnail click → scroll editor.
- Parse on a debounce (~120 ms) from `editorContent`; `$derived` in `deck.svelte.js` recomputes slides, thumbnails, outline, and status-bar counts from one source.
- Rendering uses `marked` (already a dependency in NoteLiner) with custom extensions for `==highlight==`, `^sup^`, `~sub~`, `// comment`, and `media/` URL rewriting to `media:///…`.

### 4.3 One layout, four surfaces

The stage is a **fixed pixel box** (e.g. 1920×1080 for 16:9) rendered by one `Slide.svelte`, then scaled by its container:

```
scale = min(containerW / stageW, containerH / stageH)
```

That single component then serves the editor preview, the thumbnail rail (small scale), the presenter view's current/next panes, the fullscreen stage window, and the export renderer. Any layout bug is one bug in one place, and the PDF matches the screen because it *is* the screen.

### 4.4 Presentation windows and state

- `present-service.js` opens two windows through the existing `@laankit/desktop-ui/secondary-window` helper (`id: 'stage'`, `id: 'presenter'`) — it already does dev-URL/prod-file switching, sandbox webPreferences, and focus-if-open.
- Stage window: `fullscreen: true` on the display chosen in `DisplayPicker`, `frame: false`, `setMenu(null)`, cursor auto-hide.
- State bus: main process owns `{ slideIndex, blank, elapsed, startedAt }` and broadcasts `present:state` to all three windows on change. Navigation keys from any window go `present:goto` → main → broadcast. Single source of truth; no window-to-window coupling.
- Presenter view is a Vite entry (`presenter.html`), exactly the pattern NoteLiner uses for `help.html`.

### 4.5 Deck storage

A deck is a **folder**, which makes media and Git sync straightforward:

```
<deck>/
├── deck.md            # the document
├── deck.json          # theme id, aspect ratio, theme overrides, pacing targets
├── media/…            # images/video, imported and content-hashed
└── theme.css          # optional custom theme (Phase 6)
```

Media is served through a `media://` protocol handler registered with `protocol.handle()` — the same approach as NoteLiner's `attachment://` (`main.js:308`) — so the renderer never needs filesystem access and the CSP stays tight.

Git sync (opt-in) is a direct lift of `apps/pageliner/src/main/git-sync.js`: version `deck.md`, `deck.json`, and `media/` (media matters here, unlike book blobs), debounced commit+push, explicit "Sync Now" for pull.

### 4.6 PPTX export

Two modes, because they trade off differently and both are wanted:

1. **Editable** (`pptxgenjs` 4.0.1): map each slide's blocks to text boxes and picture placeholders using the same geometry the CSS layout produced, and put `notes` in the slide's notes field. Fidelity is approximate (fonts, `==highlight==`, code blocks degrade), but the recipient can edit it — which is the whole reason people ask for PPTX.
2. **Fidelity** (image-per-slide): render each slide to PNG via `webContents.capturePage()` on the offscreen stage, then one full-bleed picture per slide with real notes text. Pixel-perfect, not editable.

Ship mode 1 as "Export → PowerPoint" and mode 2 as "PowerPoint (images)". Alternative considered: hand-rolling OOXML with JSZip + `@xmldom/xmldom`, which the repo already does for PPTX *import*. Rejected — writing valid `p:sld` XML, theme parts, and content-type overrides by hand is a lot of work for no gain over `pptxgenjs`.

---

## 5. Implementation phases

Each phase is independently runnable and reviewable, mirroring how PageLiner was built.

### Phase 0 — Scaffold (0.5 day)

Copy PageLiner's shell: `package.json`, `vite.config.mjs` (port 5254), `scripts/dev.js`, `scripts/build-icons.sh`, `electron-builder` block, icon SVG. Wire `@laankit/desktop-ui` chrome — titlebar, themes, UI scale, command palette, settings modal, status bar. Add root `package.json` scripts (`dev:slideliner`, `electron:slideliner`, `build:slideliner`, `icons:rasterize:slideliner`) and the README app entry.

**Done when:** `npm run electron:slideliner` opens a themed laankit window with working settings and palette.

### Phase 1 — Parser + preview (2–3 days) ← the risky one, do it early

`slide-parser.js`, `layout.js`, `render.js`, `Slide.svelte`, `SlidePreview.svelte`, CodeMirror editor, `deck.svelte.js`. Aspect-ratio switch. Node test suite over a fixture corpus (`tests/fixtures/*.md`), including a document exported from real iA Presenter to prove compatibility.

**Done when:** typing Markdown in the left pane updates a correct, scaled slide on the right; notes never leak onto slides.

### Phase 2 — Deck files + media (2 days)

`deck-service.js` (new/open/save/recents, autosave debounce), `OpenScreen.svelte`, `media-service.js` + `media://` protocol, drag-drop and paste image import, `deck.json` persistence, thumbnail rail with click-to-navigate.

**Done when:** a deck round-trips through disk with images intact, and the rail tracks the caret.

### Phase 3 — Presentation (2–3 days)

`present-service.js`, stage window, presenter window, display picker, keyboard navigation, blank/black, elapsed timer, teleprompter pane. Windowed share mode.

**Done when:** `F5` puts slides fullscreen on the external display while the presenter view shows current + next + notes + timer, and arrows drive both.

### Phase 4 — Export (2–3 days)

`export-service.js`: slide PDF, handout PDF, self-contained HTML, Markdown. Then `pptxgenjs` editable PPTX and the image-per-slide variant. `ExportModal.svelte` with per-format options.

**Done when:** all five formats open correctly in their native viewers and PowerPoint reports no repair prompt.

### Phase 5 — Themes + polish (2 days)

Built-in themes, `ThemePane.svelte` (font scale, accent, header/footer, slide numbers), progressive accent shift, per-slide directives, outline and search panes, duration estimate in the status bar, help window.

### Phase 6 — Import, sync, extras (2 days)

Markdown/PPTX import (port from NoteLiner), opt-in Git sync (port from PageLiner), file history, `explicit` visibility mode if the tab rule has proven irritating, video blocks, KaTeX.

**Rough total: 12–16 working days** to a complete app; Phases 0–3 (~7 days) already give something genuinely usable for giving a talk.

---

## 6. Risks

| Risk | Mitigation |
|---|---|
| Auto-layout looks bad on real content — the whole value proposition | Build a fixture deck of 30 awkward slides in Phase 1 and review visually before building anything on top |
| Tab-indent visibility confuses users | Gutter marker + `Tab` toggle + toolbar button from day one; `explicit` mode as a documented fallback |
| PPTX fidelity disappoints | Dual-mode export, and say plainly in the UI which mode is editable vs faithful |
| Scope creep toward a slide editor | The out-of-scope list in §3.6 is part of the design, not an omission |
| Presenter/stage window state drift | Single owner in main, broadcast-only to renderers |

## 7. Decisions needed before Phase 1

1. **Name** — SlideLiner, or one of the alternates?
2. **Deck = folder** (as proposed) or single `.md` with a sidecar media folder? Folder is cleaner for Git sync; single-file is friendlier to NoteLiner vaults.
3. **iA compatibility as a hard requirement?** If yes, the tab rule and `---` breaks are fixed, and the fixture corpus should include real iA documents. If no, we get freedom to pick a less surprising syntax.
4. **Phase 4 export priority order** — is PPTX the reason for the app, or is PDF-plus-presenter-mode enough for v1?
