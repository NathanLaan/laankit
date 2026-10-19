# Editor, Slides and Rich Text — Gap Analysis and Plan

Status: **All five phases done (2026-10-18).** Left: the hand checks
listed in the [Remaining](#remaining-after-the-plan) section, and removing
the TextEdit fallback engine after one release. See
[Decisions](#decisions-2026-10-18). Covers the user-facing gaps
left after the Option C port ([plan-native-rust-egui.md](plan-native-rust-egui.md))
in three shared crates:

- `crates/marina-editor`: NoteLiner's Markdown editor.
- `crates/noteliner-slides`: slides, deck preview and presenter.
- `crates/marina-richtext`: NoteLiner's preview, ThreadLiner's articles,
  PageLiner's EPUB reader, and the slide bodies.

The baseline is what users had in the Svelte build: CodeMirror 6
(`basicSetup` plus `markdown({ codeLanguages: languages })`) for the editor,
and `marked` HTML in a browser for the preview and slides.

## Summary

- **Most editor gaps share one cause.** `marina-editor` wraps egui's
  `TextEdit`. That gives the native editor one selection, whole-text
  layout and whole-text accessibility updates, so it can't do multiple
  cursors, rectangular selection or folding, and it slows past about 20,000
  lines. The other editor gaps (code-fence highlighting, bracket matching,
  highlighting other occurrences of the selection, Go to line) can be added
  on top of `TextEdit`.
- **Three rich-text gaps lose information** in documents users already
  have. Confirmed by feeding comrak's HTML through `from_html`:
  - **Task lists:** the checkboxes are dropped, so `- [ ]` and `- [x]`
    items look identical in the preview.
  - **Table alignment:** `align="right"` and `align="center"` from `:-:` are
    ignored; every cell is left-aligned.
  - **`<mark>`:** rendered as plain text with no highlight.

  Slides inherit all three because they render through `marina-richtext`.
- **Selection in rich text is drag-only.** Users can't double-click a word,
  triple-click a paragraph, or extend with Shift+arrows. That matters for
  copying from the preview, articles and EPUBs, which a browser handled.
- **Recommendation:** fix the information-losing gaps first (small,
  shared, and they also fix slides). Then add the editor features `TextEdit`
  allows. Then build a custom editor core, which is the one large item.

## Decisions (2026-10-18)

1. **The custom editor core (Phase 3) goes ahead.**
2. **Code fences: syntect's bundled set (about 75 languages) only for
   now.** A larger set such as `two-face` can come later.
3. **Cursor and scroll position are saved for the last-opened file only,
   per project**, and restored when the project reopens. Other notes open
   at the top, as in Svelte. This is in Phase 2.
4. **Phase 5 is in scope: the presenter speaker view and Typst math
   rendering. No video work**: local video stays a link card, with no
   poster frames and no FFmpeg playback.

## Gap inventory

Severity:
- **P1:** content renders wrongly, or a daily editing feature is missing.
- **P2:** a CodeMirror or browser behaviour that frequent users notice.
- **P3:** polish, rare content, or something the Svelte build didn't do
  either.

### Editor (`marina-editor`)

| Gap                                                       | In Svelte? | Severity | Constraint                                                                 | Effort |
|-----------------------------------------------------------|------------|----------|----------------------------------------------------------------------------|--------|
| Syntax highlighting inside code fences                    | Yes        | P1       | Fits the current layouter (per-line sections)                               | M      |
| Bracket-match highlight                                   | Yes        | P2       | Fits the layouter (background on two ranges)                                | S      |
| Highlight other occurrences of the selection              | Yes        | P2       | Fits the layouter                                                           | S      |
| Go to line                                                | Yes        | P2       | Dialog + existing `scroll_to_line`                                          | S      |
| Renumber following ordered-list items after insert/delete | No         | P2       | Pure text edit in `list_edits`                                              | S      |
| Find "select all matches"                                 | Yes        | P2       | Needs multiple selections → core                                            | (core) |
| Multiple cursors (Ctrl+click, Ctrl+D next occurrence)     | Yes        | P2       | `TextEdit` has one selection → core                                         | (core) |
| Rectangular selection (Alt+drag)                          | Yes        | P3       | → core                                                                      | (core) |
| Folding (headings, fences, lists) with a fold gutter      | Yes        | P2       | `TextEdit` can't hide text → core                                           | (core) |
| Notes over ~20k lines stay responsive (50k fails today)   | Yes        | P2       | Whole-text galley inside `TextEdit` → core                                  | (core) |
| Screen readers without the per-frame whole-text cost      | Yes        | P2       | egui sends the whole text to AccessKit every frame → core (visible lines only) | (core) |

S ≈ 1–3 days, M ≈ 1 week, L ≈ several weeks. "(core)" items come with
the custom editor core in Phase 3.

### Rich text (`marina-richtext`), used by the preview, articles, EPUB and slides

| Gap                                                    | In Svelte/browser? | Severity | Notes                                                              | Effort |
|--------------------------------------------------------|--------------------|----------|--------------------------------------------------------------------|--------|
| Task-list checkboxes (`- [ ]` / `- [x]`) dropped       | Yes (disabled boxes) | P1     | `<input>` is in the skipped list; draw a box glyph by checked state | S      |
| Table column alignment ignored                         | Yes                | P1       | Read `align` / `style="text-align"` on `th`/`td`                     | S      |
| `<mark>` not highlighted                               | Yes                | P1       | Add a `mark` style with a palette background                         | S      |
| `<kbd>` drawn as inline code                           | Key cap            | P3       | Optional key-cap frame                                               | S      |
| Double-click word, triple-click paragraph              | Yes                | P1       | Selection model exists; add click counts                             | S      |
| Shift+arrow / Shift+click selection extension          | Yes                | P2       | Keyboard caret over the block model                                  | M      |
| Select all (Ctrl+A) within the view                    | Yes                | P2       | Whole-document selection                                             | S      |
| Inline SVG dropped                                     | Yes                | P2       | Done (Phase 4): rasterized with `resvg` after sanitizing | M      |
| True justification (EPUB `justify`)                   | Yes                | P3       | Done (Phase 4): words spread, last line ragged | M      |
| Hanging indents (negative `text-indent`)               | Yes                | P3       | Done (Phase 4) | S      |
| Vertical margins / floats in EPUBs                     | Yes                | P3       | Margins done (Phase 4); floats not planned | M      |
| Media embeds (iframe/video/audio)                      | Played inline      | P3       | Link-out card                                                        | S      |
| Math (`$$`, MathML)                                    | No (text)          | P3       | Done (Phase 5): Typst renders `$…$`, `$$…$$` and math fences; MathML stays text | M  |

### Slides (`noteliner-slides`)

| Gap                                  | In Svelte? | Severity | Notes                                                                                   | Effort |
|--------------------------------------|------------|----------|-----------------------------------------------------------------------------------------|--------|
| `<mark>`, table alignment, task boxes | Yes       | P1       | **Done (2026-10-18):** the slide renderer reads `marina-richtext`'s `mark`, `cell_align`, `checkbox` and `kbd` styles and draws them in the slide theme's colours | — |
| Rounded image corners                | Yes        | P3       | Done (Phase 4): 8 px, not on full-bleed | S      |
| `class=` directive                   | Effectively no | P3   | Done (Phase 4): `theme-*` and `layout-*` honoured | S |
| Local video plays inline             | Yes        | —        | Out of scope (decision 4): stays a link card                                             | —      |
| Speaker view / presenter window      | No         | P3       | Done (Phase 5): a second window with notes, the next slide, timer and clock               | M      |

## Plan

Five phases, each shippable on its own. The order is by user impact per
unit of effort; the large editor core comes after the cheap fixes so users
get those first.

### Phase 1: Rich-text correctness (~1 week)

Fix what renders wrongly. All in `marina-richtext`, so the preview,
articles, EPUBs and slides improve together.

1. Task-list items: keep the checkbox state in the model and draw a
   checkbox glyph (unchecked/checked, matching the theme). Read-only, as
   in Svelte.
2. Table cell alignment from `align` and `style="text-align"`.
3. A `mark` style (palette-derived highlight background) and a key-cap
   look for `<kbd>`.
4. Selection: double-click word, triple-click paragraph, Ctrl+A,
   Shift+click to extend. Copy keeps working through the existing
   `Selection`.

Tests: conversion tests for each element; kittest for double- and
triple-click and Ctrl+A copy; a NoteLiner preview test for a note with
tasks, an aligned table and `==mark==`/`<mark>`. Check each against the
Svelte preview.

**Done (2026-10-18).** See `crates/marina-richtext/README.md` ("Markdown
preview content", "Selection and highlights").

- **Compatibility:** the model only gained `Style` fields (`checkbox`,
  `cell_align`, `mark`, `kbd`) and `RichStyle` colours, so slides, EPUB and
  the apps compile unchanged.
  - Slides already show tasks as `[ ]`/`[x]` text.
  - To get real boxes and cell alignment, slides must read
    `Style::checkbox` and `Style::cell_align`.
- **Articles stay strict:** no checkboxes, `<mark>` or `<kbd>`, as before.
- **Not done:** Shift+arrow keyboard selection (in the README's Not-yet
  list), and `==mark==`. comrak isn't run with a highlight extension, and
  `marked` had none either.
- **Tests:**
  - 7 new conversion and render tests, against the exact comrak HTML.
  - 4 new selection kittests.
  - The progressive-layout bench is unchanged apart from load noise.

### Phase 2: Editor features on the current engine (~2 weeks)

Everything the `TextEdit`-based editor can do without a rewrite. Each item
should survive the Phase 3 swap unchanged (same highlighter and commands).

1. **Code-fence highlighting:** `syntect` (pure Rust with the
   `default-fancy` feature, so it builds on all three platforms) with its
   bundled syntaxes, loaded lazily on a background thread.
   - Map syntect scopes to palette-derived colours for all six themes.
   - Cache highlighting per fence and invalidate on edit, to stay within the
     per-line cache budget.
   - Measure the binary-size cost (expected a few MB) and the typing p95
     at 5k lines with large fences.
2. **Bracket matching:** background on the pair around the cursor.
3. **Selection matches:** background on other occurrences of the selected
   word (whole-word, as CodeMirror does), capped per frame.
4. **Go to line:** a small dialog (CodeMirror's `Mod-Alt-g`) using
   `scroll_to_line`.
5. **Ordered-list renumbering** after Enter, delete or indent, in one undo
   step.
6. **Last-opened file position (decision 3):** NoteLiner saves the cursor and
   scroll offset of the note that's open, and restores them when the project
   reopens on that note. It's stored with the project's other layout state
   (`window-state.json`, keyed by project, as the Tauri host does), and only
   for that one note.

Tests: highlighter unit tests per language family; kittest for bracket
match, matches, Go to line and renumbering; a benchmark gate that typing
p95 at 5k lines stays under 4 ms.

**Done (2026-10-18).** Details, numbers and the parity table are in
`crates/marina-editor/README.md`.

- **Engine-independent modules**, so the Phase 3 swap keeps them:
  `marina_editor::code` (syntect, the bundled syntaxes, token classes),
  `matching` (bracket pairs, selection matches, Go to line parsing) and
  `list_edits::renumber_after_edit`. Only the painting is `TextEdit` glue.
- **Code fences:**
  - syntect with `default-syntaxes` + `regex-fancy` (no onig), loaded on a
    background thread on first use. Fences are one colour until then, and
    only fence lines are laid out again when the syntaxes arrive.
  - Each line keeps the fence's parse state, so an edit re-highlights until
    the state settles (one line for most edits); edits outside fences don't
    touch them.
  - Colours: One Dark on dark themes, CodeMirror's default style on light
    ones, with plain code and punctuation in the theme's text colours.
- **Bracket matching and selection matches** are painted behind the text
  from cached results; neither re-lays out a line.
- **Go to line:** a bar like the find panel, accepting `42`, `42:7`, `±n`
  and `n%`. In NoteLiner it's also a "Go to Line" command (Editor section,
  Ctrl+Alt+G).
- **Renumbering** follows CodeMirror's `renumberList` rule (only a run
  that was sequential before the edit), generalised to deletions, pastes
  and Tab. It's in the same undo step for both the editor's own list keys
  and `TextEdit`'s native Enter/Backspace/Delete/paste/cut.
- **Last-opened file position:** `lastFilePosition` in the project's layout
  (`window-state.json`). Reopening the project opens that note at its caret
  and scroll.
- **Numbers** at load average ≈20:
  - typing p95 at 5,000 lines with four 200-line fences is 1.7–2.7 ms (gate:
    under 4 ms), and 2.2 ms inside a fence
  - binary size: +0.8 MB for the editor alone; nothing measurable for
    NoteLiner, which already links syntect through Typst

### Phase 3: Custom editor core (~5–7 weeks)

**Done (2026-10-18), except the manual IME and screen-reader checks.**

- **Where:** `crates/marina-editor/src/editor_core/` (`Engine::Core`, the
  default). `TextEdit` stays as `Engine::TextEdit` for one release:
  `MARINA_EDITOR_ENGINE=textedit` or `EditorPrefs::engine`.
- **Public API unchanged.** NoteLiner needed no code changes; its 53 tests
  pass on the core engine.
- **Model:**
  - The note stays one `String` (the API hands out `&str`).
  - `ropey` indexes it for char, byte and line lookups, and is kept in step
    by splicing.
  - A selection set with a primary range.
  - The diff-based history: one step per command across all cursors; undo
    restores the primary cursor.
- **Layout:** only the visible lines are laid out. Heights are measured
  once laid out and estimated before, refined off screen on frames that
  happen anyway, and galleys past 3,000 are dropped. The scroll position is
  kept when heights above the viewport change.
- **Input:**
  - Caret moves by character, word, row, page, line and document.
  - Click, double- and triple-click, drag with autoscroll.
  - Clipboard, with one line per cursor on paste.
  - IME, with the preedit inline and `IMEOutput` at the caret.
- **Accessibility:** AccessKit gets the visible lines as text runs, plus
  the caret and selection.
- **Features:**
  - Ctrl/Cmd+click and Ctrl/Cmd+D cursors, find "all", Alt+drag
    rectangles (character columns, as CodeMirror).
  - Folding of headings, fences and list items: gutter chevrons,
    Ctrl/Cmd+Shift+[ / ], folds moved with edits.
- **Exit criteria:**

  | Criterion | Result |
  |---|---|
  | Typing p95 under 8 ms at 50k lines | Met: 1.8–5.2 ms (`TextEdit`: 31–39 ms), 3.3 ms in a code fence; load average ≈4 |
  | Idle frame cost independent of length, screen reader on | Met: 2.6 ms at 5k lines, 3.0 ms at 50k (`TextEdit`: 8.4 and 89 ms) |
  | All existing editor tests pass | Met: all 19 run on both engines, plus 11 core tests and 12 core unit tests |
  | IME checked by hand | Not yet; see the `marina-editor` README for the list |

- **Not done:**
  - List continuation, Tab and the list toolbar act on the primary cursor
    only.
  - Enter in a list at 50k lines is 8.8 ms p95 (it still reads the whole
    note).
  - Moving selected text by drag (neither engine has it).

Replace `TextEdit` inside `marina-editor` with our own widget. This unlocks
every "(core)" item in the editor table.

- **Model:**
  - A rope (`ropey`) and a selection set (several cursors or ranges, plus
    rectangular selection).
  - An undo history keeping the current diff-based design, extended to
    multiple selections.
- **Layout:**
  - Per-line galleys from the existing highlighter cache.
  - Only visible lines are laid out and painted.
  - Folded ranges are skipped; wrapped lines are measured lazily, as in
    `marina-richtext`'s progressive layout.
- **Input:**
  - Keyboard, mouse and touch.
  - IME through egui's `Ime` events and `IMEOutput` (preedit drawn inline).
  - Clipboard; drag-and-drop; the existing `EditorEvent`s unchanged.
- **Accessibility:** expose only the visible lines to AccessKit, plus
  caret and selection, removing the ~1.7 ms per 1,000 lines per frame.
- **Features on top:**
  - Multiple cursors: Ctrl/Cmd+click and Ctrl+D.
  - Find "select all".
  - Alt+drag rectangular selection.
  - Folding of headings, fences and list items, with a fold gutter.
- **Migration:**
  - Keep the public API (`EditorState`, `Editor::show`, `EditorEvent`,
    `Command`), so NoteLiner changes very little.
  - Run every existing `marina-editor` test against both engines during
    the switch.
  - Ship behind a setting with `TextEdit` as the fallback for one release,
    then remove the fallback.
- **Exit criteria:**
  - Typing p95 under 8 ms at 50k lines.
  - Idle frame cost independent of note length with a screen reader on.
  - All existing editor tests pass.
  - IME checked by hand on IBus/Fcitx, Windows TSF and macOS.

Risks: IME and accessibility are the hard parts (the spike estimated 4–6
weeks for a custom widget mostly because of them). Keep the fallback until
both are checked by hand.

### Phase 4: Rich-text and slides fidelity (~2 weeks)

**Done (2026-10-18).**
- **Inline SVG:** cleaned at conversion, rasterized off the UI thread with caps and a timeout.
  Articles take `<svg>` out before ammonia, clean it, and put it back as an `<img>`.
- **Justification:** spreads word gaps only, with a two-pass layout at about twice the
  left-aligned cost.
- **Hanging indents and EPUB vertical margins:** a paragraph's own margins, not inherited and
  not collapsing.
- **Slides:** rounded image corners, and `class=theme-*/layout-*`.
- **Not done:** Shift+arrow keyboard selection, left in marina-richtext's "Not yet" list.
- Details are in the marina-richtext and noteliner-slides READMEs.

1. Inline SVG rendered with `resvg` (articles, EPUBs, slides).
2. True justification: our own row layout that distributes extra space
   between words. Hanging indents.
3. EPUB vertical margins.
4. Slides:
   - rounded image corners
   - `class=` supporting the built-in `theme-*` and `layout-*` names

### Phase 5: Native additions (~2 weeks)

Beyond Svelte parity, both confirmed (decision 4).

1. **Presenter speaker view:** a second window (egui deferred viewport) on
   the other monitor if there is one. It shows:
   - the current slide's notes
   - the next slide
   - an elapsed timer, the clock, and the slide counter

   Both windows follow the same navigation keys. Starting the presenter
   offers "Present" or "Present with speaker view". Exiting restores both
   windows.
2. **Math rendered by Typst:** `$$…$$` blocks and `$…$` inline math in the
   preview, slides and PDF export, using the Typst already linked for
   export.
   - The LaTeX-style source is converted to Typst math syntax (a
     documented subset), and anything unsupported falls back to the source
     text.
   - Results are rendered to SVG then rasterised and cached by source and
     size, off the UI thread.
   - It sits behind the same `pdf` feature as export, so a build without
     Typst shows the source text.

No video work (decision 4).

**Done (2026-10-18).**

- **Speaker view** (`noteliner-slides`): `Presenter::with_speaker_view`.
  - It's a second window drawn as an egui *immediate* viewport, not a
    deferred one. One `Presenter` draws both windows in the same frame, so
    they can't drift and nothing is shared behind locks.
  - It shows the current slide, the next slide, the notes as rendered
    Markdown (math included), an elapsed timer (T pauses, R resets), the
    clock and "n / N".
  - Keys work in either window.
  - Placement: egui can't list monitors, so the audience window goes full
    screen where it is and the speaker window opens beside it. "Swap
    displays" (X) moves each to the other's place; it needs window
    positions, so it does nothing on Wayland.
  - Ending restores the main window's position and full-screen state, and
    closing the speaker window keeps presenting.
  - NoteLiner: "Present with Speaker View" on Alt+F5 (PowerPoint's
    Presenter View key), in the toolbar and the palette.
  - Tests: headless kittest (`tests/speaker.rs`: contents, sync, timer, end,
    Escape) and unit tests of the timer, steps and the viewport commands
    sent (full screen on and off, swap and restore).
- **Math:**
  - `noteliner_export::math::latex_to_typst` converts the documented
    subset, and `Exporter::math_svg` renders one formula as white-on-clear
    SVG.
  - New Computer Modern Math is bundled (GUST Font License, in `NOTICE`).
  - `marina_richtext::math`: a pluggable `MathRenderer`, a cached loader
    with one worker thread, inline formulas laid out over placeholder
    spaces, display formulas as blocks, and the source shown as code as a
    fallback.
  - `from_note_html` turns comrak's math into formulas, for notes only.
  - In use in the preview, slides, speaker notes and PDF (native Typst
    math). NoteLiner installs Typst as the renderer.
  - Behind the `pdf` feature: without Typst, the source shows.
  - Tests:
    - every symbol and construct compiles in Typst
    - a grammar fuzz and a no-panic fuzz
    - PDF math
    - rich-text placeholder width and fallback
    - slides conversion
    - NoteLiner rendering a formula through the real exporter
- **Fixed on the way:**
  - Radical bars went missing: `resvg` skips zero-height stroked paths.
    They're now rects, and the page fits the glyph bounds.
  - The CJK export test compared font counts, which the bundled math font
    now ties. It compares font names instead.

## Rough total

About 10–14 weeks for one developer:

| Phase | Work | Estimate |
|---|---|---|
| 1 | Rich-text correctness | 1 week |
| 2 | Editor features on the current engine | 2 weeks |
| 3 | Custom editor core | 5–7 weeks |
| 4 | Rich-text and slides fidelity | 2 weeks |
| 5 | Speaker view and Typst math | 2 weeks |

Phases 1 and 2 can run in parallel; they touch different crates. Phase 4
can overlap with Phase 3 for the same reason.

## Remaining after the plan

Also tracked, with the project-wide items, in
[plan-remaining-work.md](plan-remaining-work.md).

- **Hand checks:**
  - **IME:** IBus/Fcitx on Wayland and X11, Windows TSF and macOS, in the
    new editor core: inline composition, candidate window placement,
    commit and cancel.
  - **Screen readers:** Orca, NVDA and VoiceOver on the editor's
    visible-lines accessibility.
  - **Presenting:** real two-monitor presenting with the speaker view on
    X11, Windows and macOS; Swap displays is a no-op on Wayland.
  - **Math:** how sharp formulas look on HiDPI, and how long the first
    formula takes after launch.
- **Fallback engine:** remove the TextEdit engine
  (`MARINA_EDITOR_ENGINE=textedit`) one release after the IME and screen
  reader checks pass.
- **Known limits:**
  - List continuation, Tab and the list toolbar act on the primary cursor
    only.
  - Enter inside a list at 50k lines is 8.8 ms p95, because list
    continuation still scans the note.
  - Shift+arrow selection in rich text needs a keyboard caret in `RichView`.
  - Code fences: no TypeScript, TOML, Svelte or PowerShell grammar in
    syntect's bundled set (decision 2).

## Open questions

None at the moment; see [Decisions](#decisions-2026-10-18).
