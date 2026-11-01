# Presentations in NoteLiner

Status: **Phases 1–2 complete, Phase 3 partly done (2026-10-31).** Presenting and the speaker view are built natively (`crates/noteliner-slides`). Still to do: the rest of Phase 3 (slide PDF, handout PDF and notes-stripped Markdown export), and Phases 4 and 5.

## Decision

Recorded 2026-08-10, after comparing this plan against the standalone SlideLiner app. Presentations are built **inside NoteLiner**, because:

1. **The corpus already exists.** iA Presenter's thesis is that the talk *is* the document, so the right home is the app where the documents already live. A separate app inverts the premise. Adding `presentation:` to a note you already wrote is the whole value proposition, and only this plan has it.
2. **Deferring iA file compatibility removed the standalone plan's main advantage** — that requirement was the one thing genuinely easier in a greenfield codebase. Without it, a separate app mostly re-implements what NoteLiner ships.
3. **This direction is reversible.** `lib/slides.js`, `lib/layout.js`, `Slide.svelte`, and `deck-export-service.js` are exactly the modules a standalone app would need, proven against real decks. If presentations outgrow NoteLiner, they lift into a `@laankit/slides` package. Building the app first would mean maintaining two Markdown editors and a split vault.

Accepted cost: the sidebar pane stack goes from five to six, and a presentation tool really wants a thumbnail rail plus a large stage rather than a pane. The mitigation is structural — the **detached presenter window is the large stage** (§5), so the main window never becomes a deck editor. That is why the window lands in Phase 3 rather than late.

Sequencing note: Phase 1 is deliberately standalone and cheap. Build the parser and preview branch, point it at a real vault note, and confirm the premise before committing to Phases 2–5.

**Unrelated bug found and fixed while verifying Phase 1** (`package.json`): `prebuild` bundled the preload to `dist/preload.cjs`, then `vite build` — whose `outDir` is that same `dist/` with `emptyOutDir: true` — deleted it. Any `npm run build` therefore produced a `dist/` with no preload, so `window.api` was undefined in the built app. That broke the Playwright suite (9 of 12 failing) and would ship a non-functional packaged app, since `electron-builder.yml` packages `dist/**/*`. CI hits it too: noteliner's `test.yml` runs `npm run build` then `npm test`. Fix: `prebuild` → `postbuild`, so the preload is bundled after Vite empties the directory. PageLiner is unaffected — its renderer builds to `dist/renderer/`, so the two outputs never collide.

Fold iA-Presenter-style presenting into NoteLiner instead of building a fourth app: any note can be presented, via **export**, a **detached preview window** that goes fullscreen, **templates**, and a **slide-management UI**.

Out of scope for now (explicit): reading or writing iA Presenter's own document format, and its tab-indent visibility syntax.

---

## 1. Why fold in

The argument for the separate app was focus. The argument for folding in is stronger than it first looks:

- **The corpus already exists.** A NoteLiner vault is full of meeting notes, design docs, and outlines — exactly the material people turn into decks. Presenting a note you already wrote beats starting a new document in a different app.
- **80% of the machinery is already built and shipped.** Markdown editor, `marked` preview, attachments with a `attachment://` protocol (`src/main/main.js:308`), templates under `_templates/`, HTML/PDF/Markdown export (`main.js:926`–`1038`), a secondary-window helper already in use for the help window (`main.js:564`), git versioning, outline pane, command palette, `PaneHost` sidebar. A separate app re-implements all of it.
- **No format lock-in.** The deck is a normal `.md` note. Turn presentation off and it is still just a note — searchable, linkable, backlinked.
- **One app to maintain.** LaanKit already has three.

The cost: NoteLiner's surface area grows, and the toolbar/command list gets longer. Mitigation is that everything here is **inert until a note opts in** via frontmatter — no slides pane, no present button, no parsing cost for a note that isn't a deck.

---

## 2. The deck model

### 2.1 Opt-in via frontmatter

NoteLiner already maintains YAML frontmatter and — importantly — `frontmatter-service.js` **preserves user-added fields on every rewrite** (`MIRROR_FIELDS` is a closed set). So a `presentation:` block is a first-class citizen with no service changes:

```yaml
---
id: 4f2c…
name: Q3 Review
tags: [meetings]
presentation:
  theme: dark          # slide theme id
  aspect: "16:9"       # 16:9 | 16:10 | 4:3
  slideLevel: 2        # headings at this level start a new slide (0 = only ---)
  header: "{{name}}"   # running header; supports the existing {{…}} placeholders
  footer: "Confidential"
  slideNumbers: true
  firstSlideTitle: true   # H1 → title slide
---
```

A note is a deck **iff** `presentation` is present. Everything in this plan keys off that one predicate — `projectState.isDeck`.

### 2.2 Slides from structure, not new syntax

Because we are not chasing iA compatibility, we can pick the rule that makes existing notes presentable with **zero edits**:

| Rule | Effect |
|---|---|
| `slideLevel: N` | A heading of level ≤ N starts a new slide. `## Revenue` → new slide titled "Revenue". |
| `---` on its own line | Always starts a new slide (manual break / mid-section split). Safe: frontmatter `---` is only ever at the top of the file. |
| Content before the first break | Title slide if `firstSlideTitle`, otherwise slide 1. |

So a normal meeting note with `##` sections is already a deck once `presentation:` is added. That is the single biggest advantage of this approach over a new app, and it should be visible in the first demo.

### 2.3 Speaker notes stay invisible to everything else

iA's tab rule is out. Notes are marked with an **HTML comment block**:

```markdown
## Revenue

- Up 12% YoY
- Churn flat

<!-- notes
Open on the revenue beat. Don't read the numbers — they're on the slide.
-->
```

Why this and not a custom fence: it is invisible in `marked` (NoteLiner's existing Preview), invisible in Obsidian and every static-site generator, survives grep, and needs no parser exceptions. The note remains a *note*.

Inverse case — content in the note that should **not** reach the slide:

```markdown
<!-- skip
Long background paragraph that stays in the document but off the deck.
-->
```

Editor affordances make these ergonomic: `Ctrl+Shift+V` wraps the selection in a `notes` block, a gutter marker shows note ranges, and the `toggleMarker()` helper already in `Editor.svelte` is the model for the implementation.

### 2.4 Per-slide directives

Escape hatch, deliberately small, same comment mechanism:

```markdown
<!-- slide: layout=split bg=#101418 class=quote -->
```

Recognised keys: `layout` (`auto|hero|title-body|split|grid|full-bleed|quote`), `bg`, `class`, `header`, `footer`, `notes-only`. Unknown keys are ignored, so old notes never break.

### 2.5 Slide model (the one new pure module)

`src/renderer/lib/slides.js` — no DOM, no Electron, unit-testable with plain Node asserts like `tests/integration/pptx-import.test.js`:

```js
// parseDeck(markdown, presentationConfig) → { config, slides: Slide[] }
Slide = {
  index,
  sourceRange: { fromLine, toLine },   // ← two-way binding for free
  title: string|null,                  // heading text, for the slides pane
  headingLevel: number|null,
  blocks: Block[],                     // audience-visible
  notes: string,                        // teleprompter markdown
  directives: { layout?, bg?, class?, header?, footer? },
  layout: 'hero'|'title-body'|'split'|'grid'|'full-bleed'|'quote'|'table',
}
```

`sourceRange` is what makes the whole UI cheap: caret line → active slide in the pane (the exact trick `OutlinePane.svelte` already uses with `projectState.cursorLine`), and pane click → `projectState.scrollToLine`.

---

## 3. Creating and managing presentations

### 3.1 Creating one: a Type selector on New File

Yes — a **Type** selector on the New File modal, defaulting to Note, is the right primary entry point. It is the one place a user already goes to start something, and putting Presentation there is how the feature gets discovered at all. Three refinements to the idea:

```
┌─ New Presentation ─────────────────────────────────┐
│                                                    │
│  Type:                                             │
│  ┌───────────────────┬───────────────────────────┐ │
│  │  Note             │  Presentation           ✓ │ │  ← segmented, 2 buttons
│  └───────────────────┴───────────────────────────┘ │
│                                                    │
│  File Name:                                        │
│  [ Q3 Review                                    ]  │  ← keeps autofocus
│                                                    │
│  Parent:                                           │
│  [ (None)                                      ▾]  │
│                                                    │
│  Template:                                         │
│  [ Talk                                        ▾]  │  ← deck templates only
│                                                    │
│  Tags:                                             │
│  [ ✓ meetings ]  [   quarterly ]  …                │
│                                                    │
│                          [ Cancel ]  [    OK    ]  │
└────────────────────────────────────────────────────┘
```

**Refinement 1 — Type is a creation-time seed, not a new field on the note.** Choosing Presentation seeds the `presentation:` frontmatter block; it does not stamp a `type` property that then has to be kept in sync. `presentation:` being present remains the *only* definition of "is a deck" (§2.1), so a note converted by hand-editing frontmatter behaves identically to one created from the picker. Avoiding a second source of truth here is worth more than the convenience of an index field.

Consequence for the file tree: to show a distinct icon without reading every note's body, `noteliner.json` carries a **derived** `deck: true` cache, refreshed whenever a file is written (the frontmatter is already parsed on that path). It is a cache, not authority — frontmatter wins on any disagreement, and a full rebuild recomputes it. `FileTree.svelte:153` then picks `fa-person-chalkboard` instead of `fa-file-lines` for decks.

**Refinement 2 — Type filters the Template list, which is why it goes first.** The two controls are coupled: Type=Note lists `kind: note` templates (the current behaviour, since absent `kind` defaults to `note`); Type=Presentation lists `kind: deck` templates only, and defaults to the first one rather than Blank — plus a **Blank Presentation** option that seeds the minimum useful deck:

```markdown
---
presentation:
  theme: dark
  aspect: "16:9"
  slideLevel: 2
---

# {{title}}

<!-- notes
Opening line.
-->

## First point

-
```

Type sits above File Name because it changes the form below it. The name input keeps `use:focusInput`, so `Ctrl+N` → type a name → `Enter` still works without touching the new control.

**This needs no main-process changes.** Verified against gray-matter 4.0.3: `matter.stringify()` re-parses a raw string before merging data, so a template body that carries its own `presentation:` block flows through `TemplateService.bodyFor()` → `file:create` → `ProjectService.createFile()`'s `serialize(initialBody, data)` and lands in the new note's frontmatter alongside the mirrored `id`/`name`/`tags`, correctly merged. Deck templates are therefore *just templates*. The only tidy-up: strip the template's own `kind` key on create so it doesn't ride along into the note.

**Refinement 3 — the picker must not be the only entry point.** The premise of this plan is that the notes already exist, so converting matters more than creating:

| Entry point | Where |
|---|---|
| Type selector | New File modal (`Ctrl+N`) — primary, and how the feature is discovered |
| `file.newPresentation` — "New Presentation…" | Command palette; opens the same modal with Type preset |
| **"Convert to Presentation…"** | `FileTree` right-click menu (`FileTree.svelte:102`–`113`) and the deck-settings modal. Adds the `presentation:` block to an existing note and opens Presentation Settings. **The most important path.** |
| "Convert to Note" | Same menu, for a deck. Removes the block; content untouched |
| Toolbar | The existing New File button (`App.svelte:857`) is unchanged — no split button |

Modal mechanics: the header follows the choice ("New File" ⇄ "New Presentation"), and Duplicate mode gets `showType={false}` beside the `showTemplate={false}` it already passes (`App.svelte:907`–`909`) — a copy inherits its source's type.

Deliberately *not* in this dialog: theme, aspect ratio, slide level. Those live in Presentation Settings (§3.4) after the note exists. A create dialog that grows into a wizard is the wrong trade for a note-taking app.

### 3.2 SLIDES pane in the existing sidebar

Add a sixth pane to the `PaneHost` stack. This is a mechanical change against known anchors:

| File | Change |
|---|---|
| `App.svelte:34` | `VALID_PANE_KEYS` += `'slides'` |
| `App.svelte:37`–`60` | `DEFAULT_LAYOUT` += `showSlides: false`, `slidesHeight: 220`; `paneOrder` += `'slides'` |
| `Sidebar.svelte:156` | `panes` += `slidesVisible && { id: 'slides', title: 'SLIDES', … }` |
| new `components/SlidesPane.svelte` | The pane body |

The pane lists slides as `n · Title` rows with a thumbnail on the row (a scaled `Slide.svelte`, cheap because the stage is a `transform: scale()` render — see §5.2). Interactions:

- click → scroll editor to that slide (`projectState.scrollToLine`)
- active row tracks the caret, like the outline pane
- drag to reorder → **rewrites the markdown** by moving the slide's `sourceRange` block
- right-click → the `ContextMenu.svelte` component already used by the file tree: New Slide Below, Duplicate, Merge Into Previous, Split at Caret, Move Up/Down, Delete Slide, Set Layout ▸, Toggle Notes-Only
- the pane header gets a `+` button (`headerExtra` snippet, as `filesPane` and `tagsPane` already do)

All mutations go through one place — `src/renderer/lib/slideEdits.js`, functions of `(markdown, slides, index) → markdown` — so they are testable without a browser and the editor just receives a new document.

### 3.3 Slide headers

Two distinct things the term can mean; both are wanted:

**Per-slide header (the slide's heading).** Editing the title in the SLIDES pane row (double-click → inline edit) rewrites the heading line in the markdown. If a slide has no heading, the edit inserts one at `slideLevel`. A "promote/demote heading" action changes the level, which can merge or split slides — the pane shows that live, which is the point.

**Running header / footer (deck chrome).** From `presentation.header` / `presentation.footer`, rendered in the stage's top and bottom bands, with `{{name}}`, `{{date}}`, `{{page}}`, `{{total}}` placeholders — reusing `TemplateService.substitute()`'s vocabulary so there is one placeholder language in the app. Per-slide override via the `slide:` directive. Slide numbers are a separate toggle so a title slide can suppress them.

### 3.4 Presentation settings

`components/DeckSettingsModal.svelte` — theme, aspect ratio, slide level, header/footer, slide numbers, transition. Writes the `presentation:` block back into the note's frontmatter through the existing `file:write` + frontmatter path. Opened from the toolbar, the palette, and the SLIDES pane header. A "Turn this note into a presentation" action in the same modal is what creates the block in the first place.

### 3.5 Toolbar, commands, status bar

New commands (all `when: () => projectState.isDeck`, so they stay out of the palette for ordinary notes). Shortcuts chosen against the existing set in `App.svelte:137`–`251`, which is fully allocated for `Ctrl`-letter combos:

| Command | Shortcut |
|---|---|
| `present.start` — Present (detached window) | `F5` |
| `present.startHere` — Present from current slide | `Shift+F5` |
| `view.toggleSlides` — Toggle SLIDES pane | `Ctrl+Shift+G` |
| `slide.insertBreak` — Insert slide break | `Ctrl+Enter` |
| `slide.moveUp` / `slide.moveDown` | `Alt+Up` / `Alt+Down` |
| `slide.toggleNotes` — Wrap selection as speaker notes | `Ctrl+Shift+V` |
| `deck.export` — Export presentation… | `Ctrl+Alt+E` (pairs with `Ctrl+Alt+I` import) |
| `deck.settings` — Presentation settings | palette only |
| `deck.newFromTemplate` — New presentation… | palette only |

Status bar gains, for decks only: slide count, current slide, and an estimated speaking duration from the note word count (~130 wpm, configurable).

---

## 4. Templates

Templates already exist, are stored as plain markdown in `_templates/`, are git-versioned, and are excluded from the index and search (`template-service.js`). Three uses, one mechanism:

1. **Deck templates** — a starter note with a `presentation:` block and a few example slides. Needs no code: it is a normal template. Ship 3–4 (`Talk`, `Status Update`, `Design Review`, `Lightning Talk`) as first-run seeds written into `_templates/` when a project is created, and surface them in `NewFileModal` under a "Presentation" group. The grouping is the only real change: `TemplateService.list()` grows a `kind` field, read from each template's own frontmatter (`kind: deck`), defaulting to `note`.

2. **Slide snippets** — insertable single-slide blocks (`Title`, `Two Column`, `Quote`, `Image Full Bleed`, `Table`, `Section Divider`). Stored the same way with `kind: slide`, inserted at the caret from the SLIDES pane `+` menu. `substitute()` already handles the placeholders.

3. **Themes** — `_themes/<id>.css` in the project, listed alongside the built-ins in deck settings. Same "it's just a file in the project, so it versions and syncs" bargain. Phase 5.

`Save as Template` (`file.saveAsTemplate`, already implemented) gains a sibling `Save Slide as Template` that writes only the current slide's `sourceRange` with `kind: slide`.

---

## 5. The detached preview window

### 5.1 Window structure

Exactly the help-window pattern, which is already proven in this app: a new Vite entry plus `createSecondaryWindow`.

| File | Change |
|---|---|
| `vite.config.mjs` | `rollupOptions.input` += `present: src/renderer/present.html` (sits beside the existing `main` and `help` entries) |
| new `src/renderer/present.html`, `present.js`, `PresentApp.svelte` | The window. `present.js` mirrors `help.js`: `themeState.init({ appId: 'noteliner' })` then `mount()` |
| new `src/main/present-service.js` | Window lifecycle, display enumeration, fullscreen, state bus |
| `src/main/main.js` | `present:*` IPC handlers next to `help:open` (`main.js:582`) |
| `src/main/preload.js` | `present*` methods + an `onPresentState` subscriber (the `onMcpConfirmRequest` listener pattern) |

`createSecondaryWindow({ id: 'present', … })` gives singleton focus-if-open, dev/prod URL switching, and cleanup for free. Two additions it does not currently cover, both set after creation, no library change needed:

- `win.setFullScreen(true)` on a chosen display: `screen.getAllDisplays()` → user picks in a small `DisplayPicker.svelte` (remembered per project via the existing `window-state-service.js`) → position the window inside that display's `bounds` before going fullscreen.
- `frame: false` and `win.setMenu(null)` for a clean stage. `F11` toggles fullscreen, `Esc` exits and returns focus to the editor.

### 5.2 One stage renders everywhere

`components/Slide.svelte` renders a **fixed pixel box** (1920×1080 for 16:9) and its container scales it:

```
scale = min(containerW / stageW, containerH / stageH)
```

That one component then serves: the SLIDES pane thumbnails, the inline preview pane, the detached window, and the export renderer. Any layout bug is one bug in one place, and the exported PDF matches the screen because it *is* the screen. Rendering is `marked` (already a dependency) plus small extensions for the `notes`/`skip`/`slide:` comments and `attachment://` URL rewriting — `Preview.svelte` already does the wikilink-extension and attachment-rewriting work that this borrows from.

### 5.3 Live follow, and the state bus

The detached window has two modes:

- **Follow** (default while editing): the editor pushes the parsed deck and the caret's slide index; the window re-renders live. This is the "second-monitor live preview" that makes writing a deck pleasant.
- **Present**: navigation takes over. Arrows / PageUp/Dn / Space / click advance; `B` blanks; `G` opens a slide grid; `Home`/`End` jump.

State (`{ slideIndex, blank, mode, startedAt, elapsed }`) is owned by **main**, in `present-service.js`, and broadcast to both windows on change. Keys pressed in either window go `present:goto` → main → broadcast. Single source of truth, no window-to-window coupling, and the editor's SLIDES pane highlight follows the presenter automatically.

### 5.4 Speaker view

Same window, split layout, toggled with `S`: current slide (scaled down), next slide, the current slide's speaker notes at adjustable size, elapsed timer, and slide `n / total`. When only one display is available, this is what the presenter keeps on screen while sharing the *other* window in Zoom/Meet — so both windows are useful, not just fullscreen.

Deferred to a later phase (not needed for a usable v1): auto-scrolling teleprompter, pacing targets per slide, laser-pointer overlay.

---

## 6. Export

Extends the existing convert family rather than inventing a parallel one. Current handlers live at `main.js:926` (HTML), `:966` (Markdown), `:978` (PDF) and share a shape: read file → strip frontmatter → `marked` → write to `~/Downloads` → return `{ outputPath, downloadsDir }`. New work goes in `src/main/deck-export-service.js` and keeps that contract, plus a real save dialog (a small, welcome improvement over the hardcoded Downloads path).

| Format | How | Phase |
|---|---|---|
| **Slide PDF** — one slide per page | Hidden `BrowserWindow` + `printToPDF` with `pageSize` set to the stage size in inches and zero margins. `main.js:1015`–`1035` is the working template; the difference is that the HTML is the deck stage CSS, and page breaks come from `break-after: page` per slide. | 3 |
| **Handout PDF** — slide + its notes, in document flow | Same pipeline, different stylesheet. This is the artifact iA sells hardest and it is nearly free once the renderer exists. | 3 |
| **Self-contained HTML** | Deck HTML + inlined CSS + base64 attachments + a ~2 KB inline navigation script. Extends the existing `convertToHtml` handler with a deck branch. | 4 |
| **PPTX (editable)** | `pptxgenjs` (v4.0.1, new dependency). Map each slide's blocks to text boxes and pictures using the geometry the layout produced; speaker notes → `slide.addNotes()`. Fidelity is approximate (fonts, code blocks, highlights degrade) but the file is editable, which is why PPTX is requested at all. | 4 |
| **PPTX (images)** | `webContents.capturePage()` per slide off the offscreen stage → one full-bleed picture per slide + real notes text. Pixel-perfect, not editable. | 4 |
| **PNG sequence** | Same capture loop, written as files. | 5 |
| **Markdown (notes stripped / notes only)** | Trivial given the parser; useful for handing out the script. | 3 |

`components/ExportDeckModal.svelte` presents format, aspect ratio, theme, "include speaker notes", and destination in one dialog, replacing the current three separate context-menu items for decks (they stay for ordinary notes).

Alternative considered for PPTX: hand-rolled OOXML with `jszip` + `@xmldom/xmldom`, both already dependencies and already used to *read* PPTX in `import-service.js`. Rejected — writing valid `p:sld` parts, theme parts, and content-type overrides by hand is a lot of work for no gain over `pptxgenjs`.

PPTX **import** already exists (`import-service.js`, `Ctrl+Alt+I`) and produces `## Slide N` sections. One small change makes it a deck: emit a `presentation:` block with `slideLevel: 2` in the imported note's frontmatter. Round-trip PowerPoint → NoteLiner → PowerPoint then works, which is a genuinely useful story and costs about ten lines.

---

## 7. Phases

Each phase is independently reviewable, and the app stays shippable throughout. Every change is additive and gated on `presentation:` being present, so no existing behaviour moves.

**Phase 1 — Parser + inline preview. ✅ Done 2026-08-10.** Shipped: `lib/slides.js` (parser, config normalization, layout inference, `slideAtLine`, `estimateDuration`), `lib/attachments.js` (extracted from `Preview.svelte` so slides and the note preview resolve `_attachments/` identically), `Slide.svelte` (fixed 1920×1080 stage, `transform: scale()` fit, dark/light themes, seven layouts), `DeckPreview.svelte` (slide column, caret-following highlight, click-to-jump, notes shown *below* each slide), deck detection in `project.svelte.js` (`isDeck` / `presentation` / `deck`), and the deck branch in `Preview.svelte`.

Plumbing needed along the way: `readFile` strips frontmatter, so the renderer could not see `presentation:` at all. Added `ProjectService.readFrontmatter()`
+ `file:getFrontmatter` IPC + `getFrontmatter` in the preload.

Tests: `tests/integration/slides-parser.test.mjs` — 111 assertions, plain Node, no Electron (`npm run test:slides`); plus `tests/e2e/10-presentation.spec.js` driving real Electron for the done-when criterion. Full suite: 14 e2e passing.

*Done when:* adding `presentation:` to an existing meeting note shows correct slides in the preview pane, and notes never leak onto a slide. **Verified** — including a visual review of an 11-slide deliberately awkward deck (long headings, nested lists, code fence containing `---`, unequal split columns, 4-block grid, aligned table, headless slide).

Deviations from this plan, and what the visual review found:

- `lib/layout.js` was folded into `lib/slides.js` as `pickLayoutFor()`. It is ~20 lines operating on the same model; a separate module bought nothing.
- Fixed during review: long code lines were clipped (a projected slide cannot scroll, so `pre` now wraps), and a blanket `text-align: left` was overriding the `align` attributes marked emits for `--:` table columns.
- **Open UX finding for Phase 2:** the common `# Title` / `## Subtitle` idiom splits into two slides at `slideLevel: 2`, since the H2 is a break. Correct per the rule, surprising in practice. Options: let a title slide absorb an immediately-following heading when `firstSlideTitle` is on, or leave it and let `Ctrl+Enter`/the SLIDES pane make the split obvious. Needs a decision before the pane ships, because the pane is where users will notice it.
- Polish for Phase 5: `split` and `grid` bodies are top-aligned, which leaves a lot of dead space at the bottom of a sparse slide.

**Phase 2 — Creation UI + slide management. ✅ Done 2026-08-10.** Shipped: `lib/slideEdits.js` (insert, duplicate, delete, reorder/move, merge, split, retitle, set-layout, toggle-notes — all pure, 86 assertions), `SlidesPane.svelte` (thumbnails via the same `Slide.svelte`, caret tracking, click-to-jump, drag-to-reorder, context menu, inline retitle), `PresentationSettingsModal.svelte`, the Type selector in `NewFileModal.svelte` with `kind`-filtered templates, Convert to Presentation/Note in the `FileTree` menu, the deck icon, deck-gated toolbar button and commands, and slide/duration segments in the status bar.

Write plumbing added: `ProjectService.setPresentation()` + `file:setPresentation` IPC + preload method; the derived `deck` flag on index entries (`applyDeckFlag`, refreshed by `reconcileFrontmatter` — which already parsed every note on open, so it costs no extra I/O); `TemplateService.list()` now reports `kind`, and `createFile` merges a template's own frontmatter while stripping `kind`; `projectState.applySlideEdit()` and `selectionRange`.

*Done when:* a presentation can be created from the New File dialog or converted from any existing note, and its slides can be created, reordered, split, merged, retitled, and deleted from the pane with the markdown staying clean and the caret tracking correctly. **Verified** — 86 new unit assertions, 4 new e2e specs driving the real modal/context menu/keyboard, 18 e2e passing, plus a visual review of the pane, menu, dialog, and settings.

What the work turned up:

- **Separator decks are the fragile case, as predicted.** A `---` boundary is its own line and belongs to the *end* of the preceding slide's region, so moving the last slide to the front orphaned that separator at EOF and fused two slides. Reordering now strips the boundary off the block it moves and re-establishes boundaries at both seams (`insertSlideBlock`, `ensureBoundary`, `trimOrphanTail`). Heading-break decks never hit any of this; the machinery exists purely for separator decks, and the tests cover both.
- Blank-line hygiene is handled at splice seams only (`tidyRange`, `tidyJunction`), never by reformatting the whole note — a slide edit must not produce diff churn in parts of the note the user didn't touch.
- Fixed during visual review: the layout submenu checked both "Automatic" and the inferred layout; the check now marks what the slide *declares*, and Automatic names the inference (`Layout: Automatic (title-body)`).
- **The §3.1 title/subtitle finding is resolved without a parser change.** `# Title` / `## Subtitle` still splits — the rule stays predictable — but the SLIDES pane makes the split visible immediately, and "Merge Into Previous" is a one-click fix (covered by a unit test). Deck templates should model `# Title` + a plain subtitle paragraph so the idiom is taught by example rather than special-cased in the parser.

**Phase 3 — Detached preview window + PDF export (2–3 days).** `present.html` entry, `present-service.js`, display picker, fullscreen, follow mode, navigation, speaker view, slide PDF + handout PDF + markdown export. *Done when:* `F5` puts the deck fullscreen on the second display while the editor keeps working, and the exported PDF is indistinguishable from the screen.

**Phase 4 — HTML + PPTX export (2 days).** Self-contained HTML, `pptxgenjs` editable export, image-per-slide export, `ExportDeckModal.svelte`, deck-flavoured PPTX import. *Done when:* PowerPoint opens both variants with no repair prompt.

**Phase 5 — Templates, themes, polish (1–2 days).** Seeded starter deck templates + slide snippets (`kind: deck` / `kind: slide` bodies written into `_templates/` on project create), `_themes/*.css`, progressive accent, transitions, PNG export, help pages in `helpContent.js`, Playwright smoke test for present-window open/close.

**Total: 10–12 working days**, against 12–16 for the separate app — and Phases 1–3 (~7 days) already deliver "write a note, present it on the projector, hand out the PDF."

---

## 8. Compared to the separate app

| | Fold into NoteLiner | Separate SlideLiner app |
|---|---|---|
| Effort to usable | ~6 days | ~7 days |
| Effort to complete | 9–11 days | 12–16 days |
| Existing notes presentable | Yes, immediately | Only by copying them over |
| Reuses editor/preview/templates/git/attachments | All of it | Re-implements all of it |
| iA Presenter file compatibility | Not now (explicitly deferred) | Was a design goal |
| Focus / conceptual cleanliness | NoteLiner grows a mode | Clean single purpose |
| Long-term maintenance | One app | Four apps |
| Risk | Feature creep inside a mature app | Duplicated infrastructure drift |

The deciding question is whether a deck is *a document you already have* or *a thing you sit down to make*. iA Presenter's own answer — that the talk is the document — actually argues for folding it into the note-taking app.

## 9. Risks

| Risk | Mitigation |
|---|---|
| NoteLiner's UI gets crowded | Everything gates on `projectState.isDeck`; a non-deck note sees no new buttons, panes, or palette entries |
| Slide-reordering rewrites mangle the markdown | All mutations in `lib/slideEdits.js`, pure functions, unit-tested against fixtures before the UI is wired |
| `---` as a slide break collides with a note's existing horizontal rules | Only applies to notes that opted in; deck settings can set `slideLevel` only and ignore `---` if a vault uses rules heavily |
| HTML-comment notes syntax feels clunky to type | `Ctrl+Shift+V` wrap command + gutter marker + right-click "Mark as speaker notes" |
| Auto-layout looks bad on real content | Build a 30-slide awkward-content fixture in Phase 1 and review it visually before Phase 2 starts |
| PPTX fidelity disappoints | Dual-mode export, labelled in the UI as editable vs faithful |

## 10. Decisions needed before Phase 1

1. **Slide-break rule** — heading-driven with `slideLevel` (proposed), or require explicit `---` breaks always?
2. **Speaker-notes syntax** — HTML comment blocks (proposed), or a fenced `::: notes` block, which is prettier to type but visible as junk in other markdown tools?
3. **Detached window default mode** — live-follow while editing (proposed), or present-only, opened on demand?
4. **Export destination** — keep the existing `~/Downloads` convention, or move decks to a save dialog (proposed) and leave note exports as they are?
5. ~~**Type control style**~~ — settled in Phase 2: segmented buttons. Both options are visible, which is what makes the Presentation type discoverable.
6. ~~**Derived `deck` flag in `noteliner.json`**~~ — settled in Phase 2: yes. `reconcileFrontmatter` already parsed every note on open, so the flag is free, and the file-tree icon is the main signal that a note is a deck.
7. ~~**Title/subtitle splitting**~~ — settled in Phase 2: no parser change; the pane makes it visible and "Merge Into Previous" fixes it.
8. ~~**Does this replace a separate SlideLiner app?**~~ — settled: yes, presentations live in NoteLiner.
