# Spike 1 — Markdown editor: results

**Verdict: GO for Phase 4.** NoteLiner's editor can be egui's
`TextEdit::multiline` with a highlighting layouter, as long as the layouter
manages its own per-line galley cache (below). Typing at 5,000 lines costs
p95 ≈ 1.9 ms of CPU per frame, against a 16 ms budget, and 20,000 lines
also passes (p95 ≤ 11.4 ms). Very large notes (50,000+ lines) and screen-reader use on notes over
about 10,000 lines are the limits. A custom editor widget is only needed if
those cases matter.

## What was built

`apps/spikes/editor` (`spike-editor`):

- `gen.rs`: a deterministic Markdown generator covering frontmatter,
  headings, emphasis, links, `[[wikilinks]]`, inline code, nested bullet,
  task and numbered lists, code fences, tables and quotes.
- `highlight.rs`: a line-based scanner. Each line is highlighted from its
  text plus the block state it starts in (normal, frontmatter or fence).
  The layouter has three strategies, compared below.
- `list_edits.rs`: a one-to-one port of `listEdits.js`
  (`classifySpan`, `toUnorderedList`, `toOrderedList`, `indentAsSubList`).
  It also covers Enter (continues `-`/`*`/`+`, `N.`/`N)`, task items and
  `>` quotes; an empty item ends the list or outdents it one level) and
  Tab/Shift+Tab (indent or outdent the selected lines by 2 spaces).
- `editor.rs`: the widget. List keys are handled before `TextEdit` sees
  them, and each one adds an undo point, so it's a single Ctrl+Z step.
  List keys are skipped while an IME composition is open. The field has an
  accessible name; the kittest accessibility check failed until it did.
- `spike-editor [--lines N] [FILE]`: the GUI, with a toolbar for the list
  commands. `editor-bench`: the headless timings.
- Tests: 11 unit tests and 8 kittest UI tests. All pass, as does
  `clippy -D warnings`.

## Method

`editor-bench` drives `Context::run_ui` and then `Context::tessellate` in a
1200×800 window, with the same `ScrollArea` + editor as the GUI. That's all
the CPU work in a frame except the GPU upload. Each cell is p50 / p95 / max
in ms:

- typing columns: 60 keystrokes
- "after idle frame": 30 keystrokes, each one after a frame with no input
- wheel scroll: 60 frames

The machine is a 16-core Intel Arrow Lake laptop running Debian 13, built
with `--profile spike`. Other spikes were compiling at the same time, so
loaded runs were discarded. The line-galleys, incremental and AccessKit
rows come from runs at load ≈ 2.

### The three layout strategies

1. **Whole job.** The naive layouter: one `LayoutJob` for the whole
   document, relying on epaint's own per-paragraph galley cache. That cache
   evicts every entry not used in the previous frame. `TextEdit` reuses the
   layouter's galley on idle frames, so epaint's paragraphs get evicted and
   **the first keystroke after any pause lays out the whole document
   again**: 127 ms at 5k lines and 1.4 s at 50k. Continuous typing hides
   this, but normal typing doesn't.
2. **Line galleys.** One galley per line, cached by line hash and joined with
   `Galley::concat`. This removes the pause spike, but every line is still
   hashed and looked up on each edit.
3. **Incremental (default).** Diffs the new text against the previous text
   (common prefix and suffix), rescans and lays out only the changed lines
   plus any following lines whose block state changed (for example, after
   opening a fence), and reuses every other line's galley. A test checks it
   against a full rebuild after edits that change state.

### Results

Highlighting on, AccessKit off.

| Lines | Strategy | Open | Idle | Type at start | Type in middle | Type at end | Type after idle frame | Enter in list | Wheel scroll |
|---|---|---|---|---|---|---|---|---|---|
| 1,000 | whole job | 82 | 0.12 / 0.35 / 0.99 | 1.53 / 2.06 / 41.7 | 1.54 / 1.61 / 24.9 | 1.63 / 1.74 / 24.3 | **24.7 / 25.3 / 25.5** | 1.63 / 1.75 / 24.9 | 0.07 / 0.07 / 0.08 |
| 5,000 | whole job | 156 | 0.34 / 0.51 / 2.87 | 8.01 / 8.87 / 138 | 9.26 / 9.73 / 122 | 9.67 / 10.5 / 122 | **127 / 128 / 128** | 9.39 / 10.5 / 124 | 0.16 / 0.17 / 0.21 |
| 20,000 | whole job | 529 | 2.04 / 2.31 / 18.1 | 41.2 / 42.8 / 567 | 43.4 / 45.8 / 489 | 45.7 / 48.8 / 494 | **523 / 528 / 528** | 44.3 / 46.5 / 506 | 1.05 / 1.11 / 1.24 |
| 5,000 | line galleys | 153 | 0.34 / 0.47 / 0.51 | 2.30 / 2.36 / 3.90 | 2.68 / 2.75 / 2.79 | 3.05 / 3.15 / 3.22 | 2.68 / 2.75 / 2.77 | 2.79 / 2.92 / 3.28 | 0.15 / 0.16 / 0.18 |
| 20,000 | line galleys | 531 | 2.18 / 2.52 / 2.79 | 14.3 / 17.5 / 18.0 | 15.7 / 17.4 / 19.1 | 18.3 / 20.7 / 21.4 | 16.0 / 17.2 / 18.1 | 16.3 / 19.2 / 19.8 | 1.21 / 1.30 / 1.42 |
| 1,000 | **incremental** | 82 | 0.12 / 0.24 / 0.73 | 0.43 / 0.47 / 0.73 | 0.41 / 0.51 / 0.69 | 0.48 / 0.50 / 0.53 | 0.44 / 0.47 / 0.48 | 0.33 / 0.37 / 0.39 | 0.07 / 0.07 / 0.08 |
| 5,000 | **incremental** | 154 | 0.35 / 0.48 / 0.52 | 1.24 / **1.33** / 2.72 | 1.55 / **1.61** / 1.63 | 1.83 / **1.90** / 1.91 | 1.53 / **1.59** / 1.62 | 1.66 / 1.70 / 1.73 | 0.15 / 0.16 / 0.19 |
| 20,000 | **incremental** | 536 | 2.04 / 2.24 / 2.59 | 6.99 / 7.38 / 11.0 | 8.50 / 9.14 / 9.98 | 10.2 / 11.4 / 12.0 | 8.68 / 9.65 / 10.2 | 9.08 / 9.73 / 9.85 | 1.00 / 1.05 / 1.17 |
| 50,000 | **incremental** | 1,262 | 9.03 / 9.56 / 10.5 | 24.6 / 26.1 / 30.2 | 28.4 / 29.7 / 31.6 | 33.9 / 35.2 / 36.0 | 28.9 / 30.4 / 31.3 | 30.0 / 31.2 / 31.9 | 3.66 / 4.14 / 4.41 |

Incremental with **AccessKit on**, which is what happens when a screen
reader is running:

| Lines | Idle | Type in middle | Type at end | Wheel scroll |
|---|---|---|---|---|
| 1,000 | 1.45 / 2.31 / 3.02 | 1.31 / 1.43 / 1.45 | 1.37 / 1.43 / 1.46 | 1.04 / 1.06 / 1.13 |
| 5,000 | 8.27 / 10.0 / 10.5 | 9.81 / 11.0 / 13.6 | 9.64 / 10.6 / 10.9 | 8.32 / 8.77 / 8.93 |
| 20,000 | 35.4 / 37.3 / 37.7 | 42.6 / 45.1 / 46.2 | 43.5 / 45.0 / 45.8 | 34.8 / 37.4 / 37.9 |

At 50k lines the incremental strategy spends per keystroke:

- 4.3 ms on the diff and line vector
- 10.6 ms on `Galley::concat` and the whole-text copy
- about 15 ms inside `TextEdit` and painting: it copies the whole text several
  times per frame for change detection and undo snapshots, and walks every
  row for the cursor and for painting

All of these are O(document size), and they're the floor for any
`TextEdit`-based editor.

## Against the plan's criteria

| Criterion | Result |
|---|---|
| p95 < 16 ms per keystroke at 5k lines | **Pass**: 1.3–1.9 ms (incremental). It also passes at 20k (≤ 11.4 ms) and fails at 50k (26–35 ms). |
| IME | **Pass in kittest**: preedit and commit land at the cursor, Enter during or with a composition never triggers list continuation, and Enter works again after commit. Real input methods are still to check (below). |
| Undo/redo | **Pass**: Ctrl+Z / Ctrl+Shift+Z for typing; each list key (Enter, Tab, toolbar) is one undo step. |
| List continuation | **Pass**: bullets, numbers (`9.` → `10.`, `1)` → `2)`), tasks (a new item is unchecked), quotes, splitting mid-line, an empty item ends or outdents the list, Tab/Shift+Tab, and the toolbar commands with the same caret rules as `Editor.svelte`. |
| Cursor stays put across relayout | **Pass**: rewrapping (window resize), switching strategy or highlighting, and edits above the cursor. |

## Findings for the plan

1. **Use the incremental per-line layouter.** egui's default whole-document
   layout path has a 0.1–1.4 s spike on the first keystroke after every
   pause. This is the most important result of the spike.
2. **`Galley::concat` gotcha.** `Galley::is_empty()` checks the job's
   `sections`, and the painter skips "empty" galleys. A concatenated galley
   built from a job with no sections lays out and edits correctly but paints
   nothing. `merged_job` adds one covering section, and
   `every_strategy_paints_text` guards against a regression.
3. **Fonts.** egui's bundled fonts have no CJK glyphs (the IME tests had to
   allow missing glyphs), and none for emoji or most non-Latin scripts.
   Phase 1 (`marina-egui`) needs a fallback chain: load system fonts (for
   example with `fontdb`) or bundle Noto subsets. This affects every app,
   not just the editor.
4. **Accessibility cost.** With AccessKit on, egui rebuilds the text field's
   accessibility tree every frame, about 1.8 ms per 1,000 lines even when
   idle. That's fine up to roughly 8k lines and too slow at 20k. Report it
   upstream, or expose only the visible rows to AccessKit if we go custom.
5. **Opening a note** is a one-off layout of every line: 156 ms at 5k
   lines, 0.54 s at 20k, 1.3 s at 50k. That's acceptable for normal notes.
   For big ones, lay out lines in parallel or show the visible part first.
6. **The undo history holds whole-text snapshots** (up to 100). That's
   fine for notes, but about 250 MB for a 2.5 MB document.
7. **Shortcuts.** Ctrl/Cmd+Shift+7/8 match egui's *logical* keys. On
   layouts where Shift+7 produces another key, this may not fire, the same
   problem `Editor.svelte` solves with `event.code`. egui exposes
   `physical_key`, so match on that in Phase 4.
8. **Not covered (Phase 4 work, not blockers):**
   - code-language highlighting inside fences (`syntect` or tree-sitter)
   - wikilink autocomplete (use ArtCraft's `@`-mention dropdown pattern)
   - the find panel
   - Alt+. / Alt+, arrows
   - Ctrl+B / Ctrl+I
   - renumbering the following items after inserting into an ordered list
   - spell checking

## Cost of the fallback: a custom editor widget

This is only needed if 50k-line notes, or screen readers on notes over
about 10k lines, must be fast. The widget would need:

- a rope buffer (`ropey`)
- a cursor and selection model
- virtualised layout and painting (only the visible lines, with a Fenwick
  tree of line heights)
- hit testing, and keyboard navigation over wrapped rows
- mouse selection
- clipboard support
- operation-based undo
- IME (preedit drawing, candidate-window position)
- AccessKit for the visible region

The highlighter and list edits from this spike carry over unchanged.
**Estimate: 4–6 weeks** to reach `TextEdit` parity on all three platforms.
That's more than the plan's 3–4, mostly for IME and accessibility testing.
Recommendation: don't build it now. Ship `TextEdit` with the incremental
layouter and revisit only if large notes become a real use case.

## Still to check by hand

- **Real IMEs:**
  - IBus and Fcitx5 on GNOME/KDE, both Wayland and X11
  - Windows TSF (Microsoft Japanese/Pinyin)
  - macOS Japanese/Pinyin, plus dead keys
  - candidate-window placement

  kittest only injects `ImeEvent`s.
- **macOS:** Cmd+Z / Cmd+Shift+Z, Option+arrow word movement, and
  Cmd+Shift+7/8 with non-US layouts.
- **Windows:** high-DPI rewrap, and AltGr layouts, where Ctrl+Alt arrives
  as AltGr.
- **Screen readers** (Orca, NVDA, VoiceOver) on a 5k-line note: frame
  times and whether the field is announced.
- **Wayland window updates.** In this session the eframe window rendered no
  frames under Wayland (screenshots only worked with `WAYLAND_DISPLAY=`,
  i.e. through XWayland), probably because the test window was never
  presented on screen. Confirm on a normal Wayland desktop.
- **Trackpad and momentum scrolling.** The wheel benchmark only uses
  discrete wheel events.

## Reproduce

```bash
cargo test -p spike-editor
cargo run --profile spike -p spike-editor --bin editor-bench                  # incremental
cargo run --profile spike -p spike-editor --bin editor-bench -- --whole-job   # or --line-galleys, --accesskit, --plain, --sizes 5000
cargo run --profile spike -p spike-editor -- --lines 20000                    # GUI (also --whole-job, --line-galleys, or a .md path)
MARINA_SCREENSHOT=/tmp/editor.png MARINA_SCREENSHOT_EXIT=1 cargo run --profile spike -p spike-editor
```
